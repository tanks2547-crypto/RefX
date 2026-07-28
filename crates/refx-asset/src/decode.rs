//! `decode_guarded` — เกราะทุกชั้นรอบการ decode ภาพ
//!
//! **ทุกไฟล์คือ input ที่ไม่น่าไว้ใจ (I-4)** ไม่ว่าจะมาจากดิสก์ clipboard หรือ drag & drop
//! ฟังก์ชันนี้คือด่านเดียวที่ภาพจากภายนอกผ่านเข้ามาได้ ห้ามมีทางลัดอื่น
//!
//! ลำดับเกราะ (ห้ามสลับ — แต่ละชั้นกันคนละอย่าง):
//!   1. ขนาดไฟล์         — กันไฟล์ยักษ์ตั้งแต่ยังไม่แตะเนื้อใน
//!   2. magic bytes      — รู้ format จริง **ห้ามเชื่อนามสกุลไฟล์**
//!   3. header → w × h   — ★ กัน decompression bomb **ก่อน** allocate
//!   4. `catch_unwind`   — บั๊กใน decoder ต้องไม่ล้มโปรแกรม (I-7)
//!
//! spec: docs/06-security.md §3, docs/05-memory-and-assets.md §2

use std::io::Cursor;
use std::panic::AssertUnwindSafe;

use image::{ImageFormat, RgbaImage};

/// format ที่อนุญาต — ต้องตรงกับ feature ของ `image` ใน Cargo.toml
///
/// PSD / AVIF ไม่อยู่ในรายการโดยตั้งใจ (ROADMAP: ความเสี่ยง decoder สูง ทำใน v2)
pub const ALLOWED_FORMATS: &[ImageFormat] = &[
    ImageFormat::Png,
    ImageFormat::Jpeg,
    ImageFormat::WebP,
    ImageFormat::Gif,
    ImageFormat::Bmp,
    ImageFormat::Tga,
    ImageFormat::Tiff,
];

/// เพดานที่ตรวจก่อน decode
///
/// ค่าเริ่มต้นมาจาก docs/05-memory-and-assets.md §2
#[derive(Debug, Clone)]
pub struct Limits {
    /// ขนาดไฟล์สูงสุด (ไบต์)
    pub max_file_bytes: u64,
    /// จำนวน pixel สูงสุด — เกราะหลักกัน decompression bomb
    pub max_pixels: u64,
    /// ความกว้าง/สูงสูงสุดต่อด้าน
    pub max_dimension: u32,
    /// เพดานการจองหน่วยความจำที่ยอมให้ `image` ใช้
    pub max_alloc: u64,
    /// format ที่ยอมรับ
    pub allowed_formats: &'static [ImageFormat],
}

/// เพดานสัมบูรณ์ที่ไม่ยอมให้เกินไม่ว่าเครื่องจะใหญ่แค่ไหน — 16384² (docs/05 §3)
pub const MAX_PIXELS_ABS: u64 = 268_435_456;

/// ไบต์ที่ใช้ต่อ pixel ตอน decode: RGBA 4 ไบต์ × เผื่อบัฟเฟอร์กลางของ decoder 2 เท่า
const BYTES_PER_PIXEL_PEAK: u64 = 4 * 2;

/// สัดส่วนของ RAM ทั้งเครื่องที่ยอมให้ **ภาพเดียว** ใช้ได้
const RAM_FRACTION_PER_IMAGE: u64 = 8;

/// เพดาน pixel ที่ผูกกับ RAM ของเครื่องจริง (docs/05 §3)
///
/// ★ ทำไมต้องผูกกับเครื่อง: ภาพ 16384² ขอ RAM ~2 GiB ตอน decode
/// บนเครื่อง 8 GB ที่เปิด Photoshop อยู่ = swap หนักหรือโดน OOM killer
/// = **งานผู้ใช้หาย** ซึ่งผิด I-3 การ "รอให้ถัง RAM ว่างก่อน" ไม่ได้กันเรื่องนี้เลย
/// มันแค่เลื่อนเวลาตาย — ต้องกันที่ต้นทางคือไม่รับภาพที่ใหญ่เกินเครื่องตั้งแต่แรก
///
///   * เครื่อง 8 GB  → ~11,500²  (ยังพอสแกน A3 300 dpi ได้)
///   * เครื่อง 16 GB → ชนเพดาน 16,384²
#[must_use]
pub fn max_pixels_for_ram(total_ram: u64) -> u64 {
    let per_image = total_ram / RAM_FRACTION_PER_IMAGE;
    (per_image / BYTES_PER_PIXEL_PEAK).min(MAX_PIXELS_ABS)
}

impl Default for Limits {
    /// เพดานที่ยังไม่รู้จักเครื่อง — ใช้ `MAX_PIXELS_ABS`
    ///
    /// ★ โค้ดจริงต้องใช้ [`Limits::for_system`] เสมอ ตัวนี้ไว้ใช้ในเทสต์เท่านั้น
    fn default() -> Self {
        Self {
            max_file_bytes: 512 << 20, // 512 MB
            max_pixels: MAX_PIXELS_ABS,
            max_dimension: 65_535, // เพดานของ PNG/JPEG เอง
            max_alloc: 1 << 30,    // 1 GB
            allowed_formats: ALLOWED_FORMATS,
        }
    }
}

impl Limits {
    /// เพดานที่คำนวณจาก RAM ของเครื่องนี้
    ///
    /// **เรียกครั้งเดียวตอนเปิดโปรแกรม** แล้วส่งต่อ — ไม่ใช่เรียกซ้ำทุก job
    #[must_use]
    pub fn for_system(total_ram: u64) -> Self {
        let max_pixels = max_pixels_for_ram(total_ram);
        let side = (max_pixels as f64).sqrt() as u64;
        tracing::info!(
            ram_gb = total_ram / (1 << 30),
            max_pixels,
            approx_side = side,
            capped = max_pixels >= MAX_PIXELS_ABS,
            "ตั้งเพดานขนาดภาพตาม RAM ของเครื่อง"
        );
        Self {
            max_pixels,
            // ให้ image จองได้ไม่เกินเพดานเดียวกัน ไม่ใช่ 1 GB ตายตัว
            max_alloc: max_pixels.saturating_mul(BYTES_PER_PIXEL_PEAK),
            ..Self::default()
        }
    }
}

/// โหลดภาพไม่สำเร็จ
///
/// ★ ข้อความใน `#[error(…)]` เป็น **อังกฤษสำหรับ log และนักพัฒนา** (docs/03 §0)
/// `thiserror` คอมไพล์มันเป็น format string ตั้งแต่ตอน build จึงแปลตอนรันไม่ได้
///
/// **ข้อความที่ผู้ใช้เห็นอยู่ที่ `refx-ui::text::load_error`** ซึ่งประกอบขึ้นจาก
/// **ฟิลด์** ของ error แต่ละตัว แล้วแปลตามภาษาที่เลือก — กฎเดิมใน CLAUDE.md
/// ที่ว่าข้อความต้องบอก "เกิดอะไร + ทำอะไรต่อได้" ยังอยู่ครบ แค่ย้ายที่อยู่
///
/// ทุก variant จึงต้อง**เก็บฟิลด์ให้ครบพอที่จะประกอบข้อความได้** ห้ามยัดข้อมูล
/// ลงไปในสตริงอย่างเดียว
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// ไฟล์ใหญ่เกินเพดาน
    #[error("file is too large: {actual_mb} MB, limit is {limit_mb} MB")]
    FileTooLarge {
        /// ขนาดจริง (MB)
        actual_mb: u64,
        /// เพดาน (MB)
        limit_mb: u64,
    },

    /// ภาพมี pixel มากเกินเพดาน — เคสนี้รวม decompression bomb
    #[error("image is too large: {width}x{height} = {pixels} pixels, limit is {limit}")]
    ImageTooLarge {
        /// ความกว้างที่ประกาศใน header
        width: u32,
        /// ความสูงที่ประกาศใน header
        height: u32,
        /// จำนวน pixel ที่คำนวณได้
        pixels: u64,
        /// เพดาน
        limit: u64,
    },

    /// ไม่รู้จักชนิดไฟล์
    #[error("unrecognised file type (magic bytes match no supported format)")]
    UnknownFormat,

    /// รู้จัก format แต่ไม่อนุญาต
    #[error("format {format:?} is not in the allow-list")]
    FormatNotAllowed {
        /// format ที่ตรวจเจอ
        format: ImageFormat,
    },

    /// header อ่านไม่ได้
    #[error("cannot read the image header (file damaged or truncated)")]
    BadHeader,

    /// decoder panic — จับได้แล้วแปลงเป็น error ธรรมดา
    #[error("image decoder panicked and was caught (I-7)")]
    DecoderPanic,

    /// decode ล้มด้วยสาเหตุปกติ
    #[error("decode failed: {0}")]
    Decode(#[from] image::ImageError),

    /// อ่านไฟล์จากดิสก์ไม่ได้
    ///
    /// เจอบ่อยกับไฟล์บน OneDrive/Dropbox ที่ยังไม่ได้ sync ลงเครื่องจริง
    #[error("cannot read {file}: {source}")]
    Io {
        /// ชื่อไฟล์ (ไม่ใช่ path เต็ม — docs/08 §5)
        file: String,
        /// สาเหตุจากระบบไฟล์
        source: std::io::Error,
    },

    /// ที่อยู่นี้ไม่ใช่ไฟล์ (เป็นโฟลเดอร์ หรือ device node)
    #[error("{file} is not a regular file")]
    NotAFile {
        /// ชื่อที่ระบุมา
        file: String,
    },
}

/// ตรวจว่าขนาดภาพอยู่ในเพดานไหม
///
/// แยกเป็นฟังก์ชันบริสุทธิ์เพื่อทดสอบขอบเขตได้โดยไม่ต้องสร้างภาพ 268 ล้าน pixel จริง
/// (ภาพขนาดนั้นกิน RAM 1 GB — สร้างในเทสต์ไม่ไหว)
///
/// # Errors
/// คืน [`LoadError::ImageTooLarge`] เมื่อเกินเพดานด้านใดด้านหนึ่งหรือเกินจำนวน pixel รวม
pub fn check_dimensions(width: u32, height: u32, limits: &Limits) -> Result<(), LoadError> {
    // คูณใน u64 เสมอ — u32 × u32 ล้นได้ง่ายมากและจะกลายเป็นเลขเล็กที่ "ผ่าน" เกราะไปเฉย ๆ
    let pixels = u64::from(width) * u64::from(height);

    if width > limits.max_dimension || height > limits.max_dimension {
        return Err(LoadError::ImageTooLarge {
            width,
            height,
            pixels,
            limit: limits.max_pixels,
        });
    }
    if pixels > limits.max_pixels {
        return Err(LoadError::ImageTooLarge {
            width,
            height,
            pixels,
            limit: limits.max_pixels,
        });
    }
    Ok(())
}

/// อ่านไฟล์เข้าหน่วยความจำแล้ว decode พร้อมเกราะครบทุกชั้น
///
/// ★ **ห้าม memory-map ไฟล์ของผู้ใช้** (docs/06 §3, ตัดสิน 27 ก.ค. 2026)
/// ถ้าไฟล์ที่ map ไว้ถูกตัดสั้นลงระหว่างที่เรายังถืออยู่ การอ่าน page นั้นได้ **SIGBUS**
/// ซึ่งเป็น *signal* ไม่ใช่ panic → `catch_unwind` จับไม่ได้ → เกราะชั้น 4 ไร้ผลทั้งชั้น
/// นักวาดเก็บ reference ไว้บน Dropbox/OneDrive/NAS เป็นเรื่องปกติ ซึ่ง sync ตัดไฟล์ตลอดเวลา
///
/// ต้นทุนที่แลกมาคือ memcpy ครั้งเดียว (ปกติ 2–20 MB ≈ 1 ms) เทียบกับ decode
/// ที่จอง w×h×4 อยู่แล้ว แทบไม่ต่าง
///
/// **ต้องเรียกบน worker thread เท่านั้น** — แตะดิสก์ (I-2)
///
/// # Errors
/// คืน [`LoadError`] เสมอเมื่อทำไม่สำเร็จ — ไม่ panic ไม่ว่าไฟล์จะเป็นอะไร
pub fn load_guarded(path: &std::path::Path, limits: &Limits) -> Result<RgbaImage, LoadError> {
    let bytes = read_file_guarded(path, limits)?;
    decode_guarded_labelled(&bytes, limits, &file_label(path))
}

/// อ่านไฟล์เข้าหน่วยความจำพร้อมเกราะเรื่องขนาด (ชั้น 1)
///
/// แยกออกมาเพื่อให้ decode pool ใช้ **เส้นทางเดียวกัน** — ถ้าเขียนสองที่
/// เกราะสองชุดจะเพี้ยนจากกันเมื่อมีคนแก้ข้างเดียว
///
/// ไม่ใช้ `std::fs::read` เพราะมันอ่านทั้งไฟล์ตามที่ metadata บอก
/// ซึ่งเชื่อไม่ได้ถ้าไฟล์กำลังถูก sync เขียนทับอยู่
///
/// **ต้องเรียกบน worker thread เท่านั้น** — แตะดิสก์ (I-2)
///
/// # Errors
/// คืน [`LoadError`] เมื่ออ่านไม่ได้หรือไฟล์ใหญ่เกินเพดาน
pub fn read_file_guarded(path: &std::path::Path, limits: &Limits) -> Result<Vec<u8>, LoadError> {
    use std::io::Read as _;

    // ★ เช็คขนาดจาก metadata ก่อน — ปฏิเสธไฟล์ยักษ์ตั้งแต่ยังไม่เปิดอ่าน
    // ถ้าอ่านก่อนแล้วค่อยเช็ค เท่ากับยอมให้ไฟล์ 40 GB ดูด RAM ไปแล้ว
    let metadata = std::fs::metadata(path).map_err(|source| LoadError::Io {
        file: file_label(path),
        source,
    })?;

    if !metadata.is_file() {
        return Err(LoadError::NotAFile {
            file: file_label(path),
        });
    }

    let size = metadata.len();
    if size > limits.max_file_bytes {
        return Err(LoadError::FileTooLarge {
            actual_mb: size / (1 << 20),
            limit_mb: limits.max_file_bytes / (1 << 20),
        });
    }

    let io_err = |source: std::io::Error| LoadError::Io {
        file: file_label(path),
        source,
    };

    // ไฟล์อาจโตขึ้นระหว่าง metadata กับ read (sync เขียนทับพอดี)
    // `take` จึงจำกัดไว้ที่เพดานอีกชั้น ไม่เชื่อ metadata อย่างเดียว
    let file = std::fs::File::open(path).map_err(io_err)?;
    let mut reader = file.take(limits.max_file_bytes);
    let mut buffer = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    reader.read_to_end(&mut buffer).map_err(io_err)?;
    Ok(buffer)
}

/// ชื่อไฟล์อย่างเดียว — **ห้ามใส่ path เต็มลง log** (มีชื่อผู้ใช้อยู่ในนั้น, docs/08 §5)
fn file_label(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || "(ไม่ทราบชื่อไฟล์)".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// อ่านแค่ขนาดภาพจาก header — **ไม่ decode** จึงเร็วและไม่จอง memory
///
/// decode pool ใช้ค่านี้ประเมินโควตา RAM ก่อนลงมือ decode จริง (I-6)
/// ผ่านเกราะชั้น 1–3 ครบเหมือน [`decode_guarded`]
///
/// # Errors
/// คืน [`LoadError`] เมื่อไฟล์ไม่ผ่านเกราะชั้นใดชั้นหนึ่ง
pub fn probe_dimensions(bytes: &[u8], limits: &Limits) -> Result<(u32, u32), LoadError> {
    let len = bytes.len() as u64;
    if len > limits.max_file_bytes {
        return Err(LoadError::FileTooLarge {
            actual_mb: len / (1 << 20),
            limit_mb: limits.max_file_bytes / (1 << 20),
        });
    }

    let format = image::guess_format(bytes).map_err(|_| LoadError::UnknownFormat)?;
    if !limits.allowed_formats.contains(&format) {
        return Err(LoadError::FormatNotAllowed { format });
    }

    let mut probe = image::ImageReader::with_format(Cursor::new(bytes), format);
    probe.limits(image_limits(limits));
    let (width, height) = probe.into_dimensions().map_err(|_| LoadError::BadHeader)?;
    check_dimensions(width, height, limits)?;
    Ok((width, height))
}

/// decode ภาพจาก byte ดิบพร้อมเกราะครบทุกชั้น
///
/// รับ `&[u8]` เพราะภาพมาได้จากทั้งไฟล์ (ผ่าน [`load_guarded`]) และ clipboard
///
/// # Errors
/// คืน [`LoadError`] เสมอเมื่อทำไม่สำเร็จ — **ไม่ panic ไม่ว่า input จะเป็นอะไร**
pub fn decode_guarded(bytes: &[u8], limits: &Limits) -> Result<RgbaImage, LoadError> {
    decode_guarded_labelled(bytes, limits, "(clipboard)")
}

/// เหมือน [`decode_guarded`] แต่ระบุป้ายไว้ใช้ตอน decoder panic
fn decode_guarded_labelled(
    bytes: &[u8],
    limits: &Limits,
    label: &str,
) -> Result<RgbaImage, LoadError> {
    // ---- ชั้น 1: ขนาดไฟล์ ----
    let len = bytes.len() as u64;
    if len > limits.max_file_bytes {
        return Err(LoadError::FileTooLarge {
            actual_mb: len / (1 << 20),
            limit_mb: limits.max_file_bytes / (1 << 20),
        });
    }

    // ---- ชั้น 2: format จาก magic bytes (ห้ามเชื่อนามสกุล) ----
    let format = image::guess_format(bytes).map_err(|_| LoadError::UnknownFormat)?;
    if !limits.allowed_formats.contains(&format) {
        return Err(LoadError::FormatNotAllowed { format });
    }

    // ---- ชั้น 3: อ่าน header ให้รู้ขนาดก่อน allocate ★ ----
    // ตั้ง limit ของ image ตั้งแต่ตอนอ่าน header ด้วย เพื่อให้แม้แต่การ parse header
    // ก็ถูกคุมเพดานการจองหน่วยความจำ
    let mut probe = image::ImageReader::with_format(Cursor::new(bytes), format);
    probe.limits(image_limits(limits));
    let (width, height) = probe.into_dimensions().map_err(|_| LoadError::BadHeader)?;
    check_dimensions(width, height, limits)?;

    // ---- ชั้น 4: เกราะ panic (I-7) ----
    // บั๊กใน decoder ของ crate ภายนอกต้องไม่ล้มโปรแกรมทั้งตัว
    // ผลลัพธ์คือ item ขึ้นสถานะ "โหลดไม่ได้" ไม่ใช่ crash
    //
    // ★ ตั้งธงให้ panic hook รู้ว่า panic ที่จะเกิดถูกดักไว้แล้ว
    //   ไม่งั้นโฟลเดอร์ที่มีไฟล์เสีย 500 ไฟล์ = backtrace 500 ชุดลง log
    //   จนหมุนทับ crash log จริงหายหมด (docs/06 §3)
    let decoded = {
        let _guard = refx_platform::panic_guard::enter(label);
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
            reader.limits(image_limits(limits));
            reader.decode()
        }))
        .map_err(|_| LoadError::DecoderPanic)??
    };

    // ★ ขนาดจริงหลัง decode ต้องตรวจซ้ำ — header โกหกได้
    // (บาง format มีขนาดในหลายที่ และ decoder อาจเชื่อค่าที่ต่างจากที่เราตรวจไป)
    check_dimensions(decoded.width(), decoded.height(), limits)?;

    Ok(decoded.into_rgba8())
}

/// แปลงเพดานของเราเป็นเพดานของ `image` (เกราะชั้นสองที่ crate นั้นบังคับเอง)
fn image_limits(limits: &Limits) -> image::Limits {
    let mut out = image::Limits::default();
    out.max_image_width = Some(limits.max_dimension);
    out.max_image_height = Some(limits.max_dimension);
    out.max_alloc = Some(limits.max_alloc);
    out
}

#[cfg(test)]
mod tests {
    // เทสต์เกราะต้อง panic! ได้เมื่อเจอ error variant ผิด — การยืนยันว่าโดนปฏิเสธ
    // "ด้วยเหตุผลที่ถูกต้อง" สำคัญพอ ๆ กับการโดนปฏิเสธ
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    // ---------- ตัวช่วยสร้าง input ----------

    /// สร้าง PNG จริงขนาด w×h
    fn real_png(width: u32, height: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(width, height, image::Rgba([200, 100, 50, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    /// CRC32 (พหุนามของ PNG) — เขียนเองเพราะ refx-asset ไม่มี crc32fast
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    /// ★ decompression bomb: PNG เล็ก ๆ ที่ **โกหกใน header** ว่าใหญ่มหาศาล
    ///
    /// สร้างจาก PNG จริงแล้วแก้ IHDR + คำนวณ CRC ใหม่ เพื่อให้ทุกอย่างถูกต้อง
    /// ยกเว้นขนาดที่ประกาศไว้ — ถ้า CRC ผิด decoder จะปฏิเสธด้วยเหตุผลอื่น
    /// แล้วเทสต์จะ "ผ่านเพราะเหตุผลผิด"
    fn png_lying_about_size(width: u32, height: u32) -> Vec<u8> {
        let mut png = real_png(1, 1);
        // layout: signature 0..8 | len 8..12 | "IHDR" 12..16 | data 16..29 | crc 29..33
        png[16..20].copy_from_slice(&width.to_be_bytes());
        png[20..24].copy_from_slice(&height.to_be_bytes());
        let crc = crc32(&png[12..29]); // CRC ครอบ type + data
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        png
    }

    fn truncated_jpeg() -> Vec<u8> {
        let img = RgbaImage::from_pixel(64, 64, image::Rgba([10, 20, 30, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
            .unwrap();
        out.truncate(out.len() / 2); // ตัดครึ่ง
        out
    }

    /// ไฟล์ .exe ที่ถูกตั้งชื่อเป็น .png — decode_guarded รับแต่ byte
    /// จึงพิสูจน์โดยตรงว่าเราไม่เคยเชื่อนามสกุลไฟล์เลย
    fn fake_png_actually_exe() -> Vec<u8> {
        let mut out = b"MZ\x90\x00\x03\x00\x00\x00\x04\x00\x00\x00\xff\xff\x00\x00".to_vec();
        out.extend_from_slice(&[0x00; 512]);
        out.extend_from_slice(b"This program cannot be run in DOS mode.");
        out
    }

    /// bytes สุ่มแบบ deterministic (seed คงที่ เทสต์ต้องได้ผลเดิมทุกครั้ง)
    fn random_bytes(n: usize) -> Vec<u8> {
        let mut state = 0x1234_5678_9ABC_DEF0u64;
        (0..n)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
            })
            .collect()
    }

    // ---------- ★ ชุดที่ต้องได้ Err ทุกอัน ไม่ panic ไม่ OOM ----------

    #[test]
    fn empty_file_is_rejected() {
        let err = decode_guarded(&[], &Limits::default()).unwrap_err();
        assert!(matches!(err, LoadError::UnknownFormat), "ได้ {err:?}");
    }

    #[test]
    fn one_byte_file_is_rejected() {
        let err = decode_guarded(&[0x89], &Limits::default()).unwrap_err();
        assert!(matches!(err, LoadError::UnknownFormat), "ได้ {err:?}");
    }

    #[test]
    fn random_bytes_are_rejected() {
        for size in [16, 1024, 64 * 1024] {
            let data = random_bytes(size);
            let result = decode_guarded(&data, &Limits::default());
            assert!(result.is_err(), "bytes สุ่ม {size} ไบต์ ไม่ควร decode ผ่าน");
        }
    }

    /// ★ เคสสำคัญที่สุด: PNG 4 KB ที่ประกาศ 65535×65535
    ///
    /// ถ้าเกราะชั้น 3 พัง decoder จะพยายามจอง ~17 GB แล้วเครื่องค้าง
    #[test]
    fn decompression_bomb_is_rejected_before_allocating() {
        let bomb = png_lying_about_size(65_535, 65_535);
        assert!(bomb.len() < 4096, "ไฟล์ทดสอบต้องเล็ก ({} ไบต์)", bomb.len());

        let err = decode_guarded(&bomb, &Limits::default()).unwrap_err();
        // ต้องโดนปฏิเสธเพราะ "ใหญ่เกินไป" ไม่ใช่เพราะ header เสีย
        // ถ้าได้ BadHeader แปลว่าเทสต์ผ่านด้วยเหตุผลผิด และเกราะจริงไม่เคยถูกเรียก
        match err {
            LoadError::ImageTooLarge { width, height, .. } => {
                assert_eq!((width, height), (65_535, 65_535));
            }
            other => panic!("ต้องเป็น ImageTooLarge แต่ได้ {other:?}"),
        }
    }

    #[test]
    fn truncated_jpeg_is_rejected() {
        let data = truncated_jpeg();
        let result = decode_guarded(&data, &Limits::default());
        assert!(result.is_err(), "JPEG ที่ตัดครึ่งไม่ควร decode ผ่าน");
    }

    #[test]
    fn exe_disguised_as_png_is_rejected() {
        let data = fake_png_actually_exe();
        let err = decode_guarded(&data, &Limits::default()).unwrap_err();
        // ต้องดูจาก magic bytes ไม่ใช่ชื่อไฟล์
        assert!(matches!(err, LoadError::UnknownFormat), "ได้ {err:?}");
    }

    /// ขอบเขตของเพดานขนาดไฟล์: ใหญ่เกินไป 1 ไบต์ ต้องโดนปฏิเสธ
    #[test]
    fn file_one_byte_over_limit_is_rejected() {
        let data = real_png(64, 64);
        let limits = Limits {
            max_file_bytes: data.len() as u64 - 1,
            ..Limits::default()
        };
        let err = decode_guarded(&data, &limits).unwrap_err();
        assert!(matches!(err, LoadError::FileTooLarge { .. }), "ได้ {err:?}");
    }

    /// ไฟล์ขนาดเท่าเพดานพอดีต้องผ่านด่านนี้ไปได้
    #[test]
    fn file_exactly_at_limit_is_accepted() {
        let data = real_png(64, 64);
        let limits = Limits {
            max_file_bytes: data.len() as u64,
            ..Limits::default()
        };
        decode_guarded(&data, &limits).expect("ขนาดเท่าเพดานพอดีต้องผ่าน");
    }

    #[test]
    fn disallowed_format_is_rejected() {
        let limits = Limits {
            allowed_formats: &[ImageFormat::Jpeg], // อนุญาตเฉพาะ JPEG
            ..Limits::default()
        };
        let png = real_png(8, 8);
        let err = decode_guarded(&png, &limits).unwrap_err();
        assert!(
            matches!(err, LoadError::FormatNotAllowed { format } if format == ImageFormat::Png),
            "ได้ {err:?}"
        );
    }

    // ---------- ★ ขอบเขต MAX_PIXELS พอดี / เกิน 1 pixel ----------

    /// ทดสอบด้วยภาพจริงที่ decode ได้ โดยลดเพดานลงให้ทดสอบไหว
    #[test]
    fn image_exactly_at_pixel_limit_is_accepted() {
        let limits = Limits {
            max_pixels: 64, // 8×8 พอดี
            ..Limits::default()
        };
        let png = real_png(8, 8);
        let img = decode_guarded(&png, &limits).expect("ขนาดเท่าเพดานพอดีต้องผ่าน");
        assert_eq!((img.width(), img.height()), (8, 8));
    }

    #[test]
    fn image_one_pixel_over_limit_is_rejected() {
        let limits = Limits {
            max_pixels: 63, // น้อยกว่า 8×8 อยู่ 1 จุด
            ..Limits::default()
        };
        let png = real_png(8, 8);
        let err = decode_guarded(&png, &limits).unwrap_err();
        assert!(
            matches!(
                err,
                LoadError::ImageTooLarge {
                    pixels: 64,
                    limit: 63,
                    ..
                }
            ),
            "ได้ {err:?}"
        );
    }

    /// ขอบเขตของค่าเริ่มต้นจริง (16384²) — ทดสอบผ่านฟังก์ชันบริสุทธิ์
    /// เพราะสร้างภาพ 268 ล้าน pixel จริงกิน RAM 1 GB
    #[test]
    fn default_pixel_limit_boundary_is_exact() {
        let limits = Limits::default();
        assert_eq!(limits.max_pixels, 268_435_456);

        // 16384 × 16384 = 268,435,456 พอดี → ต้องผ่าน
        check_dimensions(16_384, 16_384, &limits).expect("เท่าเพดานพอดีต้องผ่าน");

        // เกินไป 1 pixel → ต้องไม่ผ่าน
        let err = check_dimensions(16_385, 16_384, &limits).unwrap_err();
        assert!(matches!(err, LoadError::ImageTooLarge { .. }), "ได้ {err:?}");
    }

    // ---------- เพดานที่ผูกกับ RAM ของเครื่อง (docs/05 §3) ----------

    /// ★ ตัวเลขตัวอย่างใน spec ต้องออกมาตรง
    #[test]
    fn max_pixels_scales_with_machine_ram() {
        // เครื่อง 8 GB → ~11,500² ตามที่ spec เขียนไว้
        let eight_gb = max_pixels_for_ram(8 << 30);
        let side = (eight_gb as f64).sqrt() as u64;
        assert!(
            (11_000..=12_000).contains(&side),
            "เครื่อง 8 GB ควรได้ประมาณ 11,500² แต่ได้ {side}²"
        );

        // เครื่อง 16 GB → ชนเพดานสัมบูรณ์
        assert_eq!(max_pixels_for_ram(16 << 30), MAX_PIXELS_ABS);
        // เครื่องใหญ่มากก็ห้ามเกินเพดานสัมบูรณ์
        assert_eq!(max_pixels_for_ram(256 << 30), MAX_PIXELS_ABS);
    }

    /// ภาพเดียวต้องไม่มีทางกินเกิน 1/8 ของ RAM เครื่อง
    ///
    /// ข้อนี้คือสิ่งที่ทำให้กติกา "อนุมัติเมื่อถังว่างสนิท" ของ RamBudget ปลอดภัยจริง
    #[test]
    fn single_image_never_exceeds_one_eighth_of_ram() {
        for gb in [4u64, 8, 16, 32, 64] {
            let ram = gb << 30;
            let peak = max_pixels_for_ram(ram) * 8; // 4 ไบต์/pixel × เผื่อ 2 เท่า
            assert!(
                peak <= ram / 8,
                "เครื่อง {gb} GB: ภาพเดียวใช้ {peak} ไบต์ ซึ่งเกิน 1/8 ของ RAM"
            );
        }
    }

    #[test]
    fn for_system_sets_matching_alloc_cap() {
        let limits = Limits::for_system(8 << 30);
        assert_eq!(limits.max_pixels, max_pixels_for_ram(8 << 30));
        // max_alloc ต้องสอดคล้องกับ max_pixels ไม่ใช่ 1 GB ตายตัว
        assert_eq!(limits.max_alloc, limits.max_pixels * 8);
        assert!(
            limits.max_pixels < MAX_PIXELS_ABS,
            "เครื่อง 8 GB ต้องไม่ชนเพดาน"
        );
    }

    /// เครื่องเล็กมากต้องไม่ได้เพดาน 0 (จะเปิดภาพอะไรไม่ได้เลย)
    #[test]
    fn tiny_machine_still_gets_usable_limit() {
        let limits = Limits::for_system(1 << 30); // 1 GB
        assert!(limits.max_pixels > 0);
        let side = (limits.max_pixels as f64).sqrt() as u64;
        assert!(side >= 4000, "เครื่อง 1 GB ยังควรเปิดภาพ 4000² ได้ ({side}²)");
    }

    #[test]
    fn dimension_cap_is_enforced_separately() {
        let limits = Limits::default();
        // 70000 × 1 = 70,000 จุด (ไม่เกิน max_pixels) แต่ด้านเดียวเกิน max_dimension
        let err = check_dimensions(70_000, 1, &limits).unwrap_err();
        assert!(matches!(err, LoadError::ImageTooLarge { .. }), "ได้ {err:?}");
    }

    /// u32 × u32 ล้นได้ — ถ้าคูณใน u32 ผลจะกลายเป็นเลขเล็กแล้ว "ผ่าน" เกราะไปเฉย ๆ
    #[test]
    fn pixel_count_does_not_overflow() {
        let limits = Limits {
            max_dimension: u32::MAX,
            max_pixels: 268_435_456,
            ..Limits::default()
        };
        // 65536 × 65536 = 2^32 ซึ่งล้น u32 กลายเป็น 0
        let err = check_dimensions(65_536, 65_536, &limits).unwrap_err();
        match err {
            LoadError::ImageTooLarge { pixels, .. } => {
                assert_eq!(pixels, 4_294_967_296, "ต้องคำนวณใน u64 ไม่ใช่ u32");
            }
            other => panic!("ได้ {other:?}"),
        }
    }

    // ---------- ภาพปกติต้องยังใช้ได้ ----------

    #[test]
    fn valid_png_decodes() {
        let png = real_png(32, 16);
        let img = decode_guarded(&png, &Limits::default()).expect("PNG ปกติต้องเปิดได้");
        assert_eq!((img.width(), img.height()), (32, 16));
        assert_eq!(img.get_pixel(0, 0).0, [200, 100, 50, 255]);
    }

    #[test]
    fn valid_jpeg_decodes() {
        let img = RgbaImage::from_pixel(24, 24, image::Rgba([90, 90, 90, 255]));
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .unwrap();

        let out = decode_guarded(&jpeg, &Limits::default()).expect("JPEG ปกติต้องเปิดได้");
        assert_eq!((out.width(), out.height()), (24, 24));
    }

    // ---------- โหลดจากไฟล์จริง (แทนที่ mmap) ----------

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("refx-decode-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_temp(tag: &str, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = temp_dir(tag).join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn load_reads_real_file() {
        let path = write_temp("ok", "cat.png", &real_png(20, 10));
        let img = load_guarded(&path, &Limits::default()).expect("ไฟล์ปกติต้องเปิดได้");
        assert_eq!((img.width(), img.height()), (20, 10));
    }

    #[test]
    fn load_missing_file_is_error_not_panic() {
        let path = temp_dir("missing").join("ไม่มีอยู่จริง.png");
        let err = load_guarded(&path, &Limits::default()).unwrap_err();
        assert!(matches!(err, LoadError::Io { .. }), "ได้ {err:?}");
    }

    #[test]
    fn load_directory_is_rejected() {
        let dir = temp_dir("isdir");
        let err = load_guarded(&dir, &Limits::default()).unwrap_err();
        // โฟลเดอร์ต้องถูกปฏิเสธอย่างสุภาพ ไม่ใช่พยายามอ่านแล้วพัง
        assert!(
            matches!(err, LoadError::NotAFile { .. } | LoadError::Io { .. }),
            "ได้ {err:?}"
        );
    }

    /// ★ เช็คขนาดจาก metadata **ก่อน** อ่านเนื้อไฟล์
    #[test]
    fn load_rejects_oversized_file_without_reading_it() {
        let data = real_png(64, 64);
        let path = write_temp("big", "big.png", &data);
        let limits = Limits {
            max_file_bytes: data.len() as u64 - 1,
            ..Limits::default()
        };
        let err = load_guarded(&path, &limits).unwrap_err();
        assert!(matches!(err, LoadError::FileTooLarge { .. }), "ได้ {err:?}");
    }

    /// ไฟล์ว่างบนดิสก์ (เจอบ่อยตอน sync ยังไม่เสร็จ) ต้องได้ Err ไม่ panic
    #[test]
    fn load_empty_file_on_disk() {
        let path = write_temp("empty", "ยังไม่ sync.png", &[]);
        let err = load_guarded(&path, &Limits::default()).unwrap_err();
        assert!(matches!(err, LoadError::UnknownFormat), "ได้ {err:?}");
    }

    /// ★ จำลองสิ่งที่ Dropbox ทำ: ไฟล์ถูกตัดเหลือครึ่งเดียว
    ///
    /// นี่คือเคสที่ทำให้ต้องเลิกใช้ mmap — ถ้า map ไว้จะได้ SIGBUS แล้วโปรเซสตาย
    /// อ่านเข้า Vec แล้วจึงได้แค่ Err ธรรมดา
    #[test]
    fn load_truncated_file_is_error_not_crash() {
        let full = real_png(64, 64);
        let path = write_temp("trunc", "กำลัง sync.png", &full[..full.len() / 2]);
        let result = load_guarded(&path, &Limits::default());
        assert!(result.is_err(), "ไฟล์ที่ถูกตัดต้องได้ Err");
    }

    #[test]
    fn file_label_has_no_full_path() {
        // docs/08 §5: ห้ามมี path เต็มใน log (มีชื่อผู้ใช้อยู่ในนั้น)
        let label = file_label(std::path::Path::new("C:/Users/somchai/ภาพ/cat.png"));
        assert_eq!(label, "cat.png");
        assert!(!label.contains("somchai"));
    }

    /// ★ รวมทุก input ที่ควรพัง — ยิงรวดเดียวเพื่อยืนยันว่า "ไม่ panic" จริง
    ///
    /// ถ้ามีอันไหน panic เทสต์นี้จะล้มทันที (panic ไม่ถูก catch ที่ระดับเทสต์)
    #[test]
    fn no_input_ever_panics() {
        let limits = Limits::default();
        let mut cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            vec![0x89],
            vec![0xFF; 3],
            random_bytes(16),
            random_bytes(4096),
            fake_png_actually_exe(),
            truncated_jpeg(),
            png_lying_about_size(65_535, 65_535),
            png_lying_about_size(0, 0),
            png_lying_about_size(1, u32::MAX),
        ];
        // PNG จริงที่ถูกตัดทุกความยาว — เจอ header ครึ่ง ๆ กลาง ๆ ทุกแบบ
        let full = real_png(16, 16);
        for cut in [1, 8, 12, 20, 33, full.len() / 2, full.len() - 1] {
            cases.push(full[..cut.min(full.len())].to_vec());
        }

        for (i, case) in cases.iter().enumerate() {
            // ไม่สนว่า Ok หรือ Err — สนแค่ว่า "ต้องกลับมาได้" ไม่ panic ไม่ค้าง
            let result = decode_guarded(case, &limits);
            if let Err(err) = result {
                // ทุก error ต้องมีข้อความอ่านรู้เรื่อง ไม่ใช่ Debug เปล่า ๆ
                assert!(!err.to_string().is_empty(), "เคส {i} ไม่มีข้อความ error");
            }
        }
    }
}
