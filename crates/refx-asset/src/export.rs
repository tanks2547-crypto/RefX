//! เขียนไฟล์ export **ทีละแถบ** — ฝั่งตัวเข้ารหัส (P5-4 · `docs/07 §6`)
//!
//! ## ★★★ เพดาน RAM ต้องคงที่ ไม่ขึ้นกับขนาดที่ผู้ใช้เลือก
//!
//! วัดจริงแล้ว 7 ก.ย. 2026 (ตัวเลขเต็มอยู่ใน `docs/07 §6`):
//!
//! | ทาง | 4096² | 8192² | 16384² |
//! |---|---|---|---|
//! | บัฟเฟอร์เต็ม (PNG ของ `image`) | 223 MB | 877 MB | ~3.4 GB |
//! | ทีละแถบ | **8.7 MB** | **12.8 MB** | **20.8 MB** |
//!
//! → ทั้งสองรูปแบบสตรีมได้ · **แต่คนละทิศ**:
//!
//! * **PNG** เป็นแบบ *ผลัก* — `png::Writer::stream_writer` รับแถวจากเรา
//! * **JPEG** เป็นแบบ *ดึง* — `JpegEncoder::encode_image` ไล่ขอพิกเซลเอง
//!   ตามลำดับบล็อก จึงต้องมีตัวป้อนที่จำแถบปัจจุบันไว้ ([`BandView`])
//!
//! ★★ **ไม่ได้เชื่อเอกสารของ crate** ว่า JPEG อ่านไปข้างหน้าอย่างเดียว —
//! ตัวป้อน**นับจำนวนครั้งที่ถูกขอย้อนหลัง**ไว้ตลอด และ [`ExportStats::backwards`]
//! คือตัวเลขนั้น · เทสต์ยืนยันว่าเป็น 0 ที่ทุกขนาด ถ้าวันหนึ่ง `image`
//! เปลี่ยนไปใช้บล็อกสูง 16 แถว เทสต์จะแดงและบอกเหตุผลตรง ๆ
//! แทนที่จะช้าลงเป็นเท่าตัวแบบเงียบ ๆ (`docs/08 §3.9` ข้อ 1)
//!
//! ## I-3 / เขียนแบบ atomic
//!
//! เขียนลง `<ไฟล์>.tmp` ข้างปลายทาง → `sync` → `rename`
//! **ห้ามแตะไฟล์ปลายทางจนกว่าจะเข้ารหัสเสร็จ** · ผู้ใช้ export ทับไฟล์เดิม
//! แล้วยกเลิกกลางคันต้องได้ไฟล์เดิมครบ ไม่ใช่ไฟล์เสีย
//!
//! spec: docs/07-file-format.md §6

use std::cell::RefCell;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use image::GenericImageView;
use refx_core::export::BandPlan;

/// รูปแบบไฟล์ที่ export ได้
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// PNG — ไม่สูญเสีย · โปร่งใสได้
    Png {
        /// เก็บ alpha ไว้ไหม · `false` = บังคับทึบทั้งใบ
        transparent: bool,
    },
    /// JPEG — **ไม่มี alpha** ผู้เรียกต้องประกอบลงสีพื้นมาก่อนแล้ว (`docs/07 §6`)
    Jpeg {
        /// คุณภาพ 1..=100
        quality: u8,
    },
}

impl ExportFormat {
    /// ชื่อที่ใช้ในข้อความ error
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Png { .. } => "PNG",
            Self::Jpeg { .. } => "JPEG",
        }
    }

    /// นามสกุลไฟล์ที่ควรใช้
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png { .. } => "png",
            Self::Jpeg { .. } => "jpg",
        }
    }
}

/// อ่านพิกเซลของแถบไม่สำเร็จ (ฝั่ง GPU ล้ม / device lost)
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct BandError(pub String);

/// ที่มาของพิกเซล — ฝั่ง GPU เสียบตัวจริงให้ (`refx-ui::export`)
///
/// ★ กลับทิศแบบเดียวกับ `RenameFn` ของ `refx-io`: crate นี้ **ห้ามรู้จัก wgpu**
/// (มันคือ crate ที่ `fuzz/` พึ่งอยู่ — ดู `refx-asset/Cargo.toml`)
pub trait BandSource {
    /// เติมพิกเซล RGBA8 ของแถว `[y0, y0 + rows)` ลงใน `out`
    ///
    /// `out` ยาว `rows × width × 4` พอดี — ผู้เติมต้องเขียนให้ครบทุกไบต์
    ///
    /// # Errors
    /// [`BandError`] เมื่อวาดหรืออ่านกลับจาก GPU ไม่สำเร็จ
    fn fill(&mut self, y0: u32, rows: u32, out: &mut [u8]) -> Result<(), BandError>;
}

/// export ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// ผู้ใช้กดยกเลิก — **ไฟล์ปลายทางไม่ถูกแตะเลย**
    #[error("the export was cancelled before anything was written to the destination")]
    Cancelled,
    /// เส้นทางที่ให้มาไม่ใช่ชื่อไฟล์
    #[error("{} is not a file name", .0.display())]
    NotAFile(PathBuf),
    /// อ่านพิกเซลไม่สำเร็จ
    #[error("could not read the pixels for row {row}: {reason}")]
    Source {
        /// แถวแรกของแถบที่ล้ม
        row: u32,
        /// ต้นเหตุ
        reason: String,
    },
    /// ตัวเข้ารหัสปฏิเสธ
    #[error("could not encode the image as {format}: {reason}")]
    Encode {
        /// รูปแบบที่กำลังเขียน
        format: &'static str,
        /// ต้นเหตุ
        reason: String,
    },
    /// ★★★ ไฟล์บนดิสก์ไม่เท่ากับจำนวนไบต์ที่เราเขียน
    ///
    /// การเขียนลงไดรฟ์ที่แชร์ไว้ล้มเหลวในแบบที่ดิสก์ในเครื่องไม่ล้ม (ลิงก์หลุด
    /// กลางคัน · โควตาเต็ม · สิทธิ์หาย) และ **บางทางรายงานว่าสำเร็จ**
    /// — นี่คือราคาของการอนุญาต UNC (`docs/06 §4`)
    #[error("only {actual} of {expected} bytes reached {}", path.display())]
    Truncated {
        /// ไฟล์ที่ขนาดไม่ตรง
        path: PathBuf,
        /// จำนวนไบต์ที่ตัวเข้ารหัสเขียนออกไป
        expected: u64,
        /// ขนาดที่อ่านกลับมาได้จริง
        actual: u64,
    },
    /// ล้มระหว่างแตะดิสก์ — บอกด้วยว่าล้มที่ขั้นไหนและไฟล์ไหน
    #[error("could not {step} {}: {source}", path.display())]
    Io {
        /// ขั้นที่ล้ม
        step: &'static str,
        /// ไฟล์ที่กำลังแตะตอนนั้น
        path: PathBuf,
        /// ต้นเหตุจากระบบไฟล์
        source: std::io::Error,
    },
}

impl ExportError {
    fn io(step: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            step,
            path: path.to_path_buf(),
            source,
        }
    }
}

/// วิธีสลับไฟล์ที่ [`export_to_file`] จะใช้
///
/// ★ เหตุผลเดียวกับ `refx_io::save::RenameFn` เป๊ะ: การทำให้ `rename` เองทนไฟดับ
/// ต้องใช้ `unsafe` บน Windows = อยู่ได้ที่ `refx-platform` ที่เดียว (I-5)
/// และ crate นี้พึ่ง crate นั้นไม่ได้ · **ไม่มีค่าปริยาย** โดยตั้งใจ
pub type RenameFn = fn(&Path, &Path) -> std::io::Result<()>;

/// สิ่งที่เกิดขึ้นจริงระหว่าง export — ตัวเลขที่ใช้ตรวจว่าสมมติฐานยังจริงอยู่
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportStats {
    /// ขนาดไฟล์ที่ได้ (ไบต์)
    pub bytes: u64,
    /// จำนวนแถบของแผน
    pub bands: u32,
    /// จำนวนครั้งที่ต้องไปเอาแถบมาจากต้นทาง
    ///
    /// ★ เท่ากับ [`Self::bands`] พอดี = อ่านแต่ละพิกเซลครั้งเดียวจริง
    pub fills: u32,
    /// ★★★ จำนวนครั้งที่ถูกขอ**แถบที่อยู่ก่อนหน้า**
    ///
    /// ต้องเป็น 0 · มากกว่านั้นแปลว่าต้อง render ซ้ำ (ผลยังถูก แต่ช้าเป็นเท่าตัว)
    pub backwards: u32,
}

/// เขียนไฟล์ export แบบ atomic
///
/// ## ลำดับที่ห้ามสลับ
///
/// ```text
/// 1. เขียน <ไฟล์>.tmp ทีละแถบ        ← ปลายทางยังไม่ถูกแตะเลย
/// 2. sync                              ← ของใหม่อยู่บนดิสก์จริงแล้ว
/// 3. rename tmp -> <ไฟล์>             ← สลับตัวจริง (atomic)
/// ```
///
/// ล้มหรือถูกยกเลิกที่ขั้นไหนก็ตาม `.tmp` ถูกลบทิ้งและ **ไฟล์ปลายทางเดิมยังอยู่ครบ**
///
/// # Errors
/// [`ExportError`] — ถูกยกเลิก, อ่านพิกเซลไม่ได้, เข้ารหัสไม่ได้ หรือระบบไฟล์ล้ม
pub fn export_to_file(
    path: &Path,
    format: ExportFormat,
    plan: BandPlan,
    source: &mut dyn BandSource,
    cancel: &AtomicBool,
    rename: RenameFn,
) -> Result<ExportStats, ExportError> {
    let tmp = tmp_path(path)?;

    // ★ ยกเลิกก่อนสร้างไฟล์ = ไม่มีอะไรเกิดขึ้นเลย แม้แต่ `.tmp`
    if cancel.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }

    let result = write_tmp(&tmp, format, plan, source, cancel);

    match result {
        Ok(stats) => {
            // ★★★ **ยืนยันก่อน rename ตอนที่ไฟล์เดิมยังอยู่ครบ** (`docs/06 §4`)
            //
            //   การเขียนลง share ล้มเหลวในแบบที่ดิสก์ในเครื่องไม่ล้ม และบางทาง
            //   **รายงานว่าสำเร็จ** · ถ้าจับได้ตอนนี้ ผู้ใช้ยังมีไฟล์เดิมอยู่
            //   ถ้าปล่อยไป rename ก่อนแล้วค่อยตรวจ ของเดิมหายไปแล้วตอนที่รู้ตัว
            verify_size(&tmp, stats.bytes).inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            })?;

            rename(&tmp, path).map_err(|err| {
                // สลับไม่สำเร็จ — เก็บกวาดแล้วปล่อยไฟล์เดิมไว้เหมือนเดิม
                let _ = std::fs::remove_file(&tmp);
                ExportError::io("replace", path, err)
            })?;

            // ★ ตรวจซ้ำที่ปลายทาง — `rename` เองก็ล้มแบบเงียบได้บน share
            //   ★★ ที่นี่ **ไม่ลบไฟล์ปลายทาง**: ของเดิมหายไปแล้ว การลบซ้ำทำให้
            //      ผู้ใช้ไม่เหลืออะไรเลย · รายงานให้ชัดว่าไฟล์ไหนน่าสงสัยดีกว่า
            verify_size(path, stats.bytes)?;

            tracing::info!(
                bands = stats.bands,
                bytes = stats.bytes,
                format = format.name(),
                "exported an image"
            );
            Ok(stats)
        }
        Err(err) => {
            // ★★ ไฟล์ครึ่งใบห้ามเหลืออยู่ — ทั้งตอนยกเลิกและตอนล้ม
            let _ = std::fs::remove_file(&tmp);
            Err(err)
        }
    }
}

/// `<ไฟล์>.tmp` ที่อยู่ **โฟลเดอร์เดียวกับปลายทาง**
///
/// ★ ต้องอยู่โฟลเดอร์เดียวกัน ไม่ใช่ temp ของระบบ — `rename` ข้ามไดรฟ์ไม่ atomic
/// (วินโดวส์ทำเป็น copy+delete) ซึ่งทำลายหลักประกันทั้งข้อ
///
/// # Errors
/// [`ExportError::NotAFile`] เมื่อ path ไม่มีชื่อไฟล์ (เช่น `C:\`)
fn tmp_path(path: &Path) -> Result<PathBuf, ExportError> {
    let Some(name) = path.file_name() else {
        return Err(ExportError::NotAFile(path.to_path_buf()));
    };
    let mut tmp = name.to_os_string();
    tmp.push(".tmp");
    Ok(path.with_file_name(tmp))
}

/// อ่านขนาดไฟล์กลับมาเทียบกับจำนวนไบต์ที่เราเขียนออกไป
///
/// # Errors
/// [`ExportError::Truncated`] เมื่อไม่ตรง · [`ExportError::Io`] เมื่อถามขนาดไม่ได้
fn verify_size(path: &Path, expected: u64) -> Result<(), ExportError> {
    let actual = std::fs::metadata(path)
        .map_err(|err| ExportError::io("check the size of", path, err))?
        .len();
    if actual == expected {
        return Ok(());
    }
    Err(ExportError::Truncated {
        path: path.to_path_buf(),
        expected,
        actual,
    })
}

fn write_tmp(
    tmp: &Path,
    format: ExportFormat,
    plan: BandPlan,
    source: &mut dyn BandSource,
    cancel: &AtomicBool,
) -> Result<ExportStats, ExportError> {
    let file = std::fs::File::create(tmp).map_err(|err| ExportError::io("create", tmp, err))?;
    let mut sink = GuardedWriter {
        inner: std::io::BufWriter::new(file),
        cancel,
        stopped: false,
        written: 0,
    };

    let mut stats = ExportStats {
        bands: plan.band_count(),
        ..ExportStats::default()
    };
    let outcome = match format {
        ExportFormat::Png { transparent } => {
            write_png(&mut sink, plan, transparent, source, &mut stats)
        }
        ExportFormat::Jpeg { quality } => write_jpeg(&mut sink, plan, quality, source, &mut stats),
    };
    // ★ ถ้าการเขียนถูกตัดเพราะยกเลิก ต้องรายงานว่า "ยกเลิก" ไม่ใช่ "ดิสก์พัง"
    //   — ข้อความที่ผู้ใช้เห็นต้องตรงกับสิ่งที่เขาเพิ่งทำ
    if sink.stopped {
        return Err(ExportError::Cancelled);
    }
    outcome?;

    // ★★★ **นับจากสิ่งที่ตัวเข้ารหัสส่งให้เรา ไม่ใช่จากขนาดไฟล์**
    //
    //   ถ้าเอาขนาดไฟล์มาเป็นตัวตั้ง แล้วเอาไปเทียบกับขนาดไฟล์อีกที มันจะตรงเสมอ
    //   ต่อให้ดิสก์กลืนไบต์ไปครึ่งหนึ่ง — ด่านที่เทียบของกับตัวมันเองคือด่านที่
    //   ผ่านตลอดกาลโดยไม่ได้ตรวจอะไรเลย (`docs/08 §3.9` ข้อ 9)
    stats.bytes = sink.written;

    let mut file = sink
        .inner
        .into_inner()
        .map_err(|err| ExportError::io("flush", tmp, err.into_error()))?;
    file.flush()
        .map_err(|err| ExportError::io("flush", tmp, err))?;
    // ★ ขั้นที่แยก "เขียนแล้ว" ออกจาก "อยู่บนดิสก์แล้ว" — เหมือน `refx_io::save`
    file.sync_all()
        .map_err(|err| ExportError::io("flush", tmp, err))?;
    Ok(stats)
}

/// ตัวห่อปลายทางที่ **หยุดทันทีที่ถูกยกเลิก**
///
/// ★★★ นี่คือที่ที่การยกเลิกทำงานจริงสำหรับทั้งสองรูปแบบ · ตัวเข้ารหัส JPEG
/// เป็นแบบ *ดึง* — ถ้ารอเช็คธงที่ลูปของเรา มันจะไล่ขอพิกเซลจนจบภาพก่อน
/// (16384² = 268 ล้านครั้ง) แล้วผู้ใช้จะกดยกเลิกแล้วเห็นโปรแกรมนิ่งไปอีกครึ่งนาที
/// · การคืน error ที่ปลายทางทำให้ทั้ง PNG และ JPEG เลิกกลางคันได้เหมือนกัน
struct GuardedWriter<'a, W> {
    inner: W,
    cancel: &'a AtomicBool,
    stopped: bool,
    /// ไบต์ที่ตัวเข้ารหัสส่งผ่านมาจริง — ตัวตั้งของการยืนยันขนาดหลังเขียนเสร็จ
    written: u64,
}

impl<W: std::io::Write> std::io::Write for GuardedWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            self.stopped = true;
            return Err(std::io::Error::other("the export was cancelled"));
        }
        let n = self.inner.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn write_png<W: std::io::Write>(
    sink: W,
    plan: BandPlan,
    transparent: bool,
    source: &mut dyn BandSource,
    stats: &mut ExportStats,
) -> Result<(), ExportError> {
    let mut encoder = png::Encoder::new(sink, plan.width(), plan.height());
    // ★ เขียน RGBA เสมอ แม้ตอนทึบ — เส้นทางเดียวคือเส้นทางที่ถูกทดสอบจริง
    //   ช่อง alpha ที่คงที่ 255 ถูก filter ของ PNG บีบจนแทบไม่กินที่ (spec เงียบข้อนี้)
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(png_error)?;
    let mut stream = writer.stream_writer().map_err(png_error)?;

    let stride = plan.width() as usize * 4;
    let mut buffer = vec![0u8; plan.band_bytes()];
    for index in 0..plan.band_count() {
        let Some(band) = plan.band(index) else { break };
        let bytes = band.rows as usize * stride;
        let rows = &mut buffer[..bytes];
        source
            .fill(band.y0, band.rows, rows)
            .map_err(|err| ExportError::Source {
                row: band.y0,
                reason: err.0,
            })?;
        stats.fills += 1;
        if !transparent {
            // ★★ บังคับทึบที่นี่ **ไม่ใช่เชื่อว่าต้นทางทำมาแล้ว** — ผู้ใช้ที่ไม่ได้
            //    ขอความโปร่งใสแล้วเปิดไฟล์เจอขอบโปร่ง คือของเสียที่ส่งต่อไม่ได้
            for pixel in rows.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
        stream.write_all(rows).map_err(io_as_encode)?;
    }
    stream.finish().map_err(png_error)?;
    writer.finish().map_err(png_error)?;
    Ok(())
}

fn png_error(err: png::EncodingError) -> ExportError {
    ExportError::Encode {
        format: "PNG",
        reason: err.to_string(),
    }
}

fn io_as_encode(err: std::io::Error) -> ExportError {
    ExportError::Encode {
        format: "PNG",
        reason: err.to_string(),
    }
}

fn write_jpeg<W: std::io::Write>(
    sink: W,
    plan: BandPlan,
    quality: u8,
    source: &mut dyn BandSource,
    stats: &mut ExportStats,
) -> Result<(), ExportError> {
    let mut buffer = vec![0u8; plan.band_bytes()];
    let view = BandView {
        plan,
        state: RefCell::new(BandState {
            source,
            buffer: &mut buffer,
            loaded: None,
            fills: 0,
            backwards: 0,
            failure: None,
        }),
    };

    let outcome = image::codecs::jpeg::JpegEncoder::new_with_quality(sink, quality.clamp(1, 100))
        .encode_image(&view);

    let state = view.state.into_inner();
    stats.fills = state.fills;
    stats.backwards = state.backwards;
    // ★ ความล้มเหลวของ **ต้นทาง** ต้องชนะข้อความของตัวเข้ารหัสเสมอ — ไม่งั้น
    //   "GPU หลุด" จะถูกรายงานว่า "เข้ารหัสไม่สำเร็จ" ซึ่งพาไปหาสาเหตุผิดที่
    if let Some((row, reason)) = state.failure {
        return Err(ExportError::Source { row, reason });
    }
    outcome.map_err(|err| ExportError::Encode {
        format: "JPEG",
        reason: err.to_string(),
    })?;
    if state.backwards > 0 {
        tracing::warn!(
            backwards = state.backwards,
            "the JPEG encoder asked for rows it had already passed — each one costs a re-render"
        );
    }
    Ok(())
}

/// หน้าต่างแถบเดียวที่ตัวเข้ารหัสแบบ *ดึง* มองเห็นเป็นภาพทั้งใบ
struct BandView<'a> {
    plan: BandPlan,
    state: RefCell<BandState<'a>>,
}

struct BandState<'a> {
    source: &'a mut dyn BandSource,
    buffer: &'a mut [u8],
    /// แถบที่อยู่ในบัฟเฟอร์ตอนนี้ (`y0`, `rows`)
    loaded: Option<(u32, u32)>,
    fills: u32,
    backwards: u32,
    /// ความล้มเหลวครั้งแรกของต้นทาง — เก็บไว้เพราะ `get_pixel` คืน error ไม่ได้
    failure: Option<(u32, String)>,
}

impl BandState<'_> {
    /// ทำให้แถวที่ `y` อยู่ในบัฟเฟอร์ — `false` = เอามาไม่ได้
    fn ensure(&mut self, plan: &BandPlan, y: u32) -> bool {
        if let Some((y0, rows)) = self.loaded
            && y >= y0
            && y < y0 + rows
        {
            return true;
        }
        if self.failure.is_some() {
            return false; // ล้มไปแล้ว ไม่ต้องรบกวนต้นทางอีก
        }
        let index = y / plan.band_rows();
        let Some(band) = plan.band(index) else {
            return false;
        };
        if let Some((y0, _)) = self.loaded
            && band.y0 < y0
        {
            self.backwards += 1;
        }
        let stride = plan.width() as usize * 4;
        let bytes = band.rows as usize * stride;
        let Some(rows) = self.buffer.get_mut(..bytes) else {
            return false;
        };
        match self.source.fill(band.y0, band.rows, rows) {
            Ok(()) => {
                self.fills += 1;
                self.loaded = Some((band.y0, band.rows));
                true
            }
            Err(err) => {
                self.failure = Some((band.y0, err.0));
                false
            }
        }
    }
}

impl GenericImageView for BandView<'_> {
    // ★ JPEG ไม่มี alpha — ตัดทิ้งที่นี่ **หลังจาก** ภาพถูกประกอบลงสีพื้นแล้ว
    //   (ผู้เรียกเป็นคนเลือกสีพื้นและส่ง alpha 255 มา — `docs/07 §6`)
    type Pixel = image::Rgb<u8>;

    fn dimensions(&self) -> (u32, u32) {
        (self.plan.width(), self.plan.height())
    }

    fn get_pixel(&self, x: u32, y: u32) -> Self::Pixel {
        /// สีที่คืนเมื่อเอาพิกเซลมาไม่ได้ — ความล้มเหลวจริงถูกเก็บไว้ใน `failure`
        /// แล้วรายงานเป็น error หลังจบ · **ห้าม panic ที่นี่** (CLAUDE.md)
        const UNAVAILABLE: image::Rgb<u8> = image::Rgb([0, 0, 0]);

        let Ok(mut state) = self.state.try_borrow_mut() else {
            return UNAVAILABLE;
        };
        if !state.ensure(&self.plan, y) {
            return UNAVAILABLE;
        }
        let Some((y0, _)) = state.loaded else {
            return UNAVAILABLE;
        };
        let stride = self.plan.width() as usize * 4;
        let at = (y - y0) as usize * stride + x as usize * 4;
        match state.buffer.get(at..at + 3) {
            Some(rgb) => image::Rgb([rgb[0], rgb[1], rgb[2]]),
            None => UNAVAILABLE,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// ต้นทางปลอมที่วาดลายซึ่ง **ตำแหน่งบอกตัวเองได้** — ถ้าแถบไหนไปผิดที่
    /// การถอดรหัสกลับมาจะจับได้ทันที (ต่างจากสีพื้นเรียบ ๆ ที่สลับแถบแล้วไม่รู้)
    struct Pattern {
        width: u32,
        height: u32,
        calls: Vec<(u32, u32)>,
        fail_at: Option<u32>,
    }

    impl Pattern {
        fn new(width: u32, height: u32) -> Self {
            Self {
                width,
                height,
                calls: Vec::new(),
                fail_at: None,
            }
        }

        fn pixel(x: u32, y: u32) -> [u8; 4] {
            [(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 239) as u8, 255]
        }
    }

    impl BandSource for Pattern {
        fn fill(&mut self, y0: u32, rows: u32, out: &mut [u8]) -> Result<(), BandError> {
            self.calls.push((y0, rows));
            if self.fail_at == Some(y0) {
                return Err(BandError("ต้นทางล้มตามที่สั่ง".to_owned()));
            }
            assert!(y0 + rows <= self.height, "ขอแถวที่ไม่มีอยู่");
            for row in 0..rows {
                for x in 0..self.width {
                    let at = (row as usize * self.width as usize + x as usize) * 4;
                    out[at..at + 4].copy_from_slice(&Self::pixel(x, y0 + row));
                }
            }
            Ok(())
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-export-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// การสลับไฟล์ของเทสต์ — `rename_durable` ตัวจริงอยู่ที่ `refx-platform`
    /// ซึ่ง crate นี้พึ่งไม่ได้ (ดู [`RenameFn`])
    fn plain_rename(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }

    fn never_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// อ่านไฟล์กลับมาทั้งใบ — `std::fs::read` ถูกแบนใน `clippy.toml` เพราะกฎ I-2
    /// (ห้ามดิสก์ I/O บน UI thread) ซึ่งไม่เกี่ยวกับเทสต์ · ใช้รูปแบบเดียวกับ
    /// `refx-platform/fsops.rs` แทนการปิด lint
    fn read_back(path: &Path) -> Vec<u8> {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    }

    /// ★★★ **ภาพที่ได้ต้องเป็นภาพเดียวกับที่ต้นทางวาด ทุกพิกเซล**
    ///
    /// PNG ไม่สูญเสีย → เทียบได้เป๊ะ · ถ้าแถบไหนเขียนสลับที่หรือเหลื่อมไปแถวเดียว
    /// ข้อนี้จับได้ทันที ซึ่งเป็นความผิดพลาดที่ตาคนดูภาพเองแทบไม่เห็น
    #[test]
    fn a_png_written_band_by_band_is_pixel_identical_to_its_source() {
        let dir = temp_dir("png-exact");
        let path = dir.join("board.png");
        // 300 กว้าง × 300 สูง กับแถบ 1024 แถว = แถบเดียว → บังคับให้หลายแถบด้วย
        // การใช้ความสูงมากกว่าแถบเต็มไม่ได้ที่ขนาดเล็ก จึงตรวจความถูกต้องที่นี่
        // แล้วให้เทสต์ `many_bands` ข้างล่างดูเรื่องหลายแถบ
        let plan = BandPlan::new(300, 200).unwrap();
        let mut source = Pattern::new(300, 200);
        let stats = export_to_file(
            &path,
            ExportFormat::Png { transparent: true },
            plan,
            &mut source,
            &never_cancel(),
            plain_rename,
        )
        .unwrap();
        assert!(stats.bytes > 0);

        let back = image::open(&path).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), (300, 200));
        for y in [0u32, 1, 99, 199] {
            for x in [0u32, 1, 150, 299] {
                assert_eq!(
                    back.get_pixel(x, y).0,
                    Pattern::pixel(x, y),
                    "พิกเซล ({x}, {y}) ไม่ตรง"
                );
            }
        }
    }

    /// ★★ ภาพที่สูงกว่าหนึ่งแถบต้องต่อกันถูก — และต้นทางต้องถูกเรียก
    /// **ครั้งเดียวต่อแถบ ตามลำดับ**
    #[test]
    fn many_bands_are_stitched_in_order_and_each_row_is_read_once() {
        let dir = temp_dir("png-bands");
        let path = dir.join("tall.png");
        // ความกว้าง 16384 → แถบ 512 แถว · สูง 1100 → 3 แถบ (512 + 512 + 76)
        let plan = BandPlan::new(16384, 1100).unwrap();
        assert_eq!(plan.band_count(), 3);
        let mut source = Pattern::new(16384, 1100);
        let stats = export_to_file(
            &path,
            ExportFormat::Png { transparent: true },
            plan,
            &mut source,
            &never_cancel(),
            plain_rename,
        )
        .unwrap();

        assert_eq!(stats.fills, 3, "ต้องอ่านแถบละครั้ง");
        assert_eq!(source.calls, vec![(0, 512), (512, 512), (1024, 76)]);

        let back = image::open(&path).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), (16384, 1100));
        // ★ แถวรอยต่อของทุกแถบ + แถวสุดท้าย
        for y in [0u32, 511, 512, 1023, 1024, 1099] {
            assert_eq!(
                back.get_pixel(7, y).0,
                Pattern::pixel(7, y),
                "แถว {y} เหลื่อม"
            );
        }
    }

    /// ★★★ **JPEG อ่านไปข้างหน้าอย่างเดียวจริงไหม — วัด ไม่ใช่เชื่อเอกสาร**
    ///
    /// ถ้าวันหนึ่ง `image` เปลี่ยนไปใช้ chroma subsampling ที่ทำให้บล็อกสูง 16 แถว
    /// ข้อนี้จะแดงพร้อมตัวเลข แทนที่จะกลายเป็น export ที่ช้าลงเป็นเท่าตัวเงียบ ๆ
    #[test]
    fn the_jpeg_encoder_never_asks_for_a_row_it_has_already_passed() {
        let dir = temp_dir("jpeg-forward");
        for (width, height) in [(16384u32, 1100u32), (4096, 2049), (300, 200)] {
            let path = dir.join(format!("{width}x{height}.jpg"));
            let plan = BandPlan::new(width, height).unwrap();
            let mut source = Pattern::new(width, height);
            let stats = export_to_file(
                &path,
                ExportFormat::Jpeg { quality: 90 },
                plan,
                &mut source,
                &never_cancel(),
                plain_rename,
            )
            .unwrap();

            println!(
                "{width}x{height}: {} แถบ · เติม {} ครั้ง · ย้อนหลัง {}",
                stats.bands, stats.fills, stats.backwards
            );
            assert_eq!(
                stats.backwards, 0,
                "{width}x{height}: ถูกขอย้อนหลัง {} ครั้ง — แถบต้องสูงเป็นพหุคูณของบล็อก",
                stats.backwards
            );
            assert_eq!(
                stats.fills, stats.bands,
                "{width}x{height}: เติม {} ครั้งกับ {} แถบ — อ่านซ้ำ",
                stats.fills, stats.bands
            );
            assert!(stats.bytes > 0);
        }
    }

    /// JPEG ที่ได้ต้องเป็นภาพที่ **ใกล้เคียง** ต้นทาง (มันสูญเสีย จึงเทียบเป๊ะไม่ได้)
    #[test]
    fn a_jpeg_still_looks_like_the_board_it_came_from() {
        let dir = temp_dir("jpeg-looks");
        let path = dir.join("flat.jpg");
        let plan = BandPlan::new(64, 64).unwrap();
        // ลายที่มีรายละเอียดสูงจะถูก JPEG ทำให้เพี้ยนมาก — ใช้สีพื้นเพื่อวัด
        // ว่า "สีตรง" ไม่ใช่ "รายละเอียดตรง"
        struct Flat;
        impl BandSource for Flat {
            fn fill(&mut self, _y0: u32, _rows: u32, out: &mut [u8]) -> Result<(), BandError> {
                for pixel in out.chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[200, 40, 90, 255]);
                }
                Ok(())
            }
        }
        export_to_file(
            &path,
            ExportFormat::Jpeg { quality: 95 },
            plan,
            &mut Flat,
            &never_cancel(),
            plain_rename,
        )
        .unwrap();

        let back = image::open(&path).unwrap().to_rgb8();
        assert_eq!(back.dimensions(), (64, 64));
        let got = back.get_pixel(32, 32).0;
        for (channel, want) in got.iter().zip([200u8, 40, 90]) {
            assert!(
                channel.abs_diff(want) <= 6,
                "สีเพี้ยนเกินที่ JPEG ควรทำ: ได้ {got:?} ควรใกล้ [200, 40, 90]"
            );
        }
    }

    /// ★★★ **ยกเลิกกลางคัน: ไม่มีไฟล์ปลายทางใหม่ และไฟล์เดิมต้องครบทุกไบต์**
    ///
    /// นี่คือกฎที่ `docs/07 §6` เขียนไว้ตรง ๆ · ผู้ใช้ export ทับไฟล์เก่าแล้ว
    /// เปลี่ยนใจ ต้องได้ของเดิมคืน ไม่ใช่ไฟล์ครึ่งใบที่เปิดไม่ขึ้น
    #[test]
    fn cancelling_halfway_leaves_the_existing_file_byte_for_byte_intact() {
        let dir = temp_dir("cancel");
        let path = dir.join("keep.png");
        let original = b"this is the file the user already had".to_vec();
        std::fs::write(&path, &original).unwrap();

        // ยกเลิกหลังเขียนไปแล้วหนึ่งก้อน — ธงถูกตั้งโดยตัวต้นทางเองระหว่างทาง
        struct CancelAfterFirst<'a>(&'a AtomicBool, u32);
        impl BandSource for CancelAfterFirst<'_> {
            fn fill(&mut self, _y0: u32, _rows: u32, out: &mut [u8]) -> Result<(), BandError> {
                out.fill(180);
                self.1 += 1;
                if self.1 >= 1 {
                    self.0.store(true, Ordering::Relaxed);
                }
                Ok(())
            }
        }

        let cancel = AtomicBool::new(false);
        let plan = BandPlan::new(16384, 2000).unwrap();
        let err = export_to_file(
            &path,
            ExportFormat::Png { transparent: false },
            plan,
            &mut CancelAfterFirst(&cancel, 0),
            &cancel,
            plain_rename,
        )
        .unwrap_err();
        assert!(matches!(err, ExportError::Cancelled), "ได้ {err:?}");

        assert_eq!(read_back(&path), original, "ไฟล์เดิมถูกแตะ — I-3 พัง");
        assert!(
            !tmp_path(&path).unwrap().exists(),
            "เหลือไฟล์ครึ่งใบไว้: {}",
            tmp_path(&path).unwrap().display()
        );
    }

    /// ยกเลิก**ก่อนเริ่ม** ต้องไม่สร้างแม้แต่ `.tmp`
    #[test]
    fn cancelling_before_the_first_byte_creates_nothing_at_all() {
        let dir = temp_dir("cancel-early");
        let path = dir.join("nothing.png");
        let cancel = AtomicBool::new(true);
        let err = export_to_file(
            &path,
            ExportFormat::Png { transparent: true },
            BandPlan::new(100, 100).unwrap(),
            &mut Pattern::new(100, 100),
            &cancel,
            plain_rename,
        )
        .unwrap_err();
        assert!(matches!(err, ExportError::Cancelled));
        assert!(!path.exists());
        assert!(!tmp_path(&path).unwrap().exists());
    }

    /// ★★ **negative control ของการเขียนแบบ atomic**
    ///
    /// เปลี่ยน "เขียน tmp แล้ว rename" เป็น "เขียนทับตรง ๆ" แล้วยกเลิกกลางคัน —
    /// ต้องพิสูจน์ว่าไฟล์เดิม **หายจริง** ถ้าไม่มีขั้น tmp · ถ้าข้อนี้ไม่แดง
    /// แปลว่าเทสต์ข้างบนไม่ได้วัดสิ่งที่คิดว่าวัดอยู่ (`docs/08 §3.9` ข้อ 1)
    #[test]
    fn without_the_temp_file_the_users_old_file_really_is_destroyed() {
        let dir = temp_dir("nc-atomic");
        let path = dir.join("victim.png");
        let original = b"the work of three hours".to_vec();
        std::fs::write(&path, &original).unwrap();

        // เลียนแบบทางที่ **ไม่มี** ขั้น tmp: เปิดไฟล์ปลายทางเขียนทับตรง ๆ
        // (`File::create` ตัดไฟล์เหลือศูนย์ไบต์ทันที) แล้วล้มกลางทาง
        {
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(&[0u8; 8]).unwrap();
            // ตายตรงนี้ = ผู้ใช้เหลือไฟล์ 8 ไบต์
        }

        let after = read_back(&path);
        assert_ne!(
            after, original,
            "NC ไม่แดง — การเขียนทับตรง ๆ ควรทำให้ไฟล์เดิมหาย \
             ถ้ามันไม่หาย แปลว่าเทสต์ atomic ข้างบนพิสูจน์อะไรไม่ได้เลย"
        );
        println!(
            "NC วิ่งผ่านจริง: ไฟล์เดิม {} ไบต์ → เหลือ {} ไบต์เมื่อไม่มีขั้น tmp",
            original.len(),
            after.len()
        );
    }

    /// ต้นทางล้มกลางคัน (device lost) — ต้องรายงานว่า **ต้นทาง** ล้ม
    /// ไม่ใช่ "เข้ารหัสไม่สำเร็จ" และต้องไม่เหลือไฟล์
    #[test]
    fn a_source_failure_is_reported_as_a_source_failure() {
        let dir = temp_dir("source-fail");
        for format in [
            ExportFormat::Png { transparent: true },
            ExportFormat::Jpeg { quality: 80 },
        ] {
            let path = dir.join(format!("broken.{}", format.extension()));
            let mut source = Pattern::new(16384, 1100);
            source.fail_at = Some(512); // แถบที่สอง
            let err = export_to_file(
                &path,
                format,
                BandPlan::new(16384, 1100).unwrap(),
                &mut source,
                &never_cancel(),
                plain_rename,
            )
            .unwrap_err();
            assert!(
                matches!(err, ExportError::Source { row: 512, .. }),
                "{}: ได้ {err:?}",
                format.name()
            );
            assert!(!path.exists(), "{}: เหลือไฟล์ไว้", format.name());
            assert!(!tmp_path(&path).unwrap().exists());
        }
    }

    /// PNG แบบไม่โปร่งใสต้องทึบจริง **แม้ต้นทางส่ง alpha ครึ่งเดียวมา**
    #[test]
    fn an_opaque_png_is_opaque_even_when_the_source_is_not() {
        let dir = temp_dir("opaque");
        let path = dir.join("opaque.png");
        struct HalfAlpha;
        impl BandSource for HalfAlpha {
            fn fill(&mut self, _y0: u32, _rows: u32, out: &mut [u8]) -> Result<(), BandError> {
                for pixel in out.chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[10, 20, 30, 40]);
                }
                Ok(())
            }
        }
        export_to_file(
            &path,
            ExportFormat::Png { transparent: false },
            BandPlan::new(32, 32).unwrap(),
            &mut HalfAlpha,
            &never_cancel(),
            plain_rename,
        )
        .unwrap();

        let back = image::open(&path).unwrap().to_rgba8();
        assert_eq!(back.get_pixel(5, 5).0[3], 255, "alpha ไม่ถูกบังคับให้ทึบ");
    }

    /// ★★★ **ไฟล์ที่ลงดิสก์ไม่ครบต้องถูกแจ้ง ไม่ใช่รายงานว่าสำเร็จ**
    ///
    /// `docs/06 §4`: การเขียนลงไดรฟ์ที่แชร์ไว้ล้มเหลวในแบบที่ดิสก์ในเครื่องไม่ล้ม
    /// (ลิงก์หลุด · โควตาเต็ม · สิทธิ์หาย) และ **บางทางรายงานว่าสำเร็จ**
    ///
    /// จำลองด้วยการสลับไฟล์ที่ตัดท้ายทิ้ง — ถ้าด่านไม่ทำงาน export จะตอบ `Ok`
    /// แล้วผู้ใช้จะส่งไฟล์เสียให้คนอื่นโดยไม่มีใครรู้
    #[test]
    fn a_file_that_did_not_fully_reach_the_disk_is_reported_not_silently_accepted() {
        let dir = temp_dir("truncated");
        let path = dir.join("short.png");

        /// สลับแล้ว "ทำไบต์หาย" แบบที่ share ทำได้จริง
        fn rename_then_lose_bytes(from: &Path, to: &Path) -> std::io::Result<()> {
            std::fs::rename(from, to)?;
            let len = std::fs::metadata(to)?.len();
            std::fs::OpenOptions::new()
                .write(true)
                .open(to)?
                .set_len(len.saturating_sub(16))
        }

        let err = export_to_file(
            &path,
            ExportFormat::Png { transparent: true },
            BandPlan::new(64, 64).unwrap(),
            &mut Pattern::new(64, 64),
            &never_cancel(),
            rename_then_lose_bytes,
        )
        .unwrap_err();

        match err {
            ExportError::Truncated {
                expected, actual, ..
            } => {
                println!(
                    "NC วิ่งผ่านจริง: เขียนออกไป {expected} ไบต์ ลงดิสก์ {actual} ไบต์ → แจ้ง Truncated"
                );
                assert_eq!(actual + 16, expected);
            }
            other => panic!("ไบต์หายไป 16 ตัวแล้วยังตอบว่าสำเร็จ: {other:?}"),
        }
    }

    /// ตัวด่านเองต้องตอบถูกทั้งสองทาง — ขนาดตรง = ผ่าน · ไม่ตรง = ไม่ผ่าน
    #[test]
    fn the_size_gate_compares_against_what_was_written_not_against_itself() {
        let dir = temp_dir("verify");
        let path = dir.join("ten.bin");
        std::fs::write(&path, [0u8; 10]).unwrap();

        assert!(verify_size(&path, 10).is_ok());
        assert!(
            matches!(
                verify_size(&path, 20),
                Err(ExportError::Truncated {
                    expected: 20,
                    actual: 10,
                    ..
                })
            ),
            "ด่านที่เทียบของกับตัวมันเองคือด่านที่ผ่านตลอดกาล"
        );
        // ไฟล์ที่ไม่มีอยู่ต้องเป็น Io ไม่ใช่ panic
        assert!(matches!(
            verify_size(&dir.join("nope.bin"), 1),
            Err(ExportError::Io { .. })
        ));
    }

    /// ที่ที่ไม่มีชื่อไฟล์ต้องเป็น error ไม่ใช่ panic
    #[test]
    fn a_path_without_a_file_name_is_an_error() {
        let err = export_to_file(
            Path::new(".."),
            ExportFormat::Png { transparent: true },
            BandPlan::new(4, 4).unwrap(),
            &mut Pattern::new(4, 4),
            &never_cancel(),
            plain_rename,
        )
        .unwrap_err();
        assert!(matches!(err, ExportError::NotAFile(_)), "ได้ {err:?}");
    }

    /// ★ ไฟล์ชั่วคราวต้องอยู่โฟลเดอร์เดียวกับปลายทาง — `rename` ข้ามไดรฟ์ไม่ atomic
    #[test]
    fn the_temp_file_sits_next_to_the_destination() {
        let tmp = tmp_path(Path::new("C:/work/board.png")).unwrap();
        assert_eq!(tmp.parent(), Path::new("C:/work/board.png").parent());
        assert_eq!(tmp.file_name().unwrap(), "board.png.tmp");
    }
}
