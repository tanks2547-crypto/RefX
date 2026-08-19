//! Decode worker pool + priority queue + cancellation + timeout
//!
//! หลักการจาก ARCHITECTURE §3 และ docs/05 §3:
//!   * worker N = `clamp(cores - 2, 2, 6)` — เผื่อ core ให้ UI thread กับ OS
//!   * คิวเรียงตาม **ระยะจากกึ่งกลาง viewport** ภาพที่ผู้ใช้กำลังมองมาก่อนเสมอ
//!   * ทุก job ถือ `Arc<AtomicBool>` ยกเลิกได้ — pan ผ่าน 500 ภาพต้องไม่เผา CPU
//!   * ทุก job ขอโควตาจาก [`RamBudget`] ถังกลาง — เพดานคุมรวมทุก worker (I-6)
//!   * job ที่ใช้เวลาเกิน 20 วินาที ถือว่า timeout (docs/06 §3)
//!
//! ผลลัพธ์ส่งกลับ main thread ทาง `crossbeam-channel` เท่านั้น
//! **ไม่มี `Mutex<Document>` ข้ามเธรด** ตามที่ ARCHITECTURE §3 กำหนด

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

use image::RgbaImage;
use parking_lot::{Condvar, Mutex};
use refx_core::clipboard::{ClipboardContent, ClipboardError, ClipboardReader};

use crate::budget::{RamBudget, estimate_decode_bytes};
use crate::cache::{CacheKey, IoRequest, PathFingerprint, ThumbEntry, ThumbFormat};
use crate::decode::{
    Limits, LoadError, accept_rgba_guarded, decode_guarded_labelled, file_label, load_guarded,
    probe_dimensions, read_file_guarded,
};
use crate::hash::{ContentHash, hash_file};
use crate::thumb::{Thumbnail, make_thumbnail, read_orientation};
use crate::working::{self, WorkingImage};

/// เวลาสูงสุดต่อ job ก่อนถือว่า timeout (docs/05 §3)
pub const DECODE_TIMEOUT: Duration = Duration::from_secs(20);

/// เพดานเวลารอคำตอบจาก IO thread
///
/// ถ้า IO thread ค้าง worker ต้องไม่ค้างตาม — decode เองยังเร็วกว่ารอไม่จบ
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// ขนาด thumbnail ที่ถูกต้อง (128×128 RGBA)
const EXPECTED_THUMB_BYTES: usize =
    (crate::thumb::THUMB_SIZE * crate::thumb::THUMB_SIZE * 4) as usize;

/// จำนวน worker ตามสูตรใน docs/05 §3
///
/// ไม่ใช้ทุก core: เผื่อไว้ให้ UI thread และ OS — โปรแกรมที่ยึด CPU 100%
/// ทำให้ทั้งเครื่องหนืด แล้วผู้ใช้จะรู้สึกว่า "กิน CPU"
/// เพดาน 6 เพราะเกินกว่านั้นคอขวดอยู่ที่ดิสก์ ไม่ใช่ CPU
#[must_use]
pub fn default_worker_count() -> usize {
    let cores = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get);
    cores.saturating_sub(2).clamp(2, 6)
}

/// closure ที่ปลุก UI — แยกเป็น alias เพราะชนิดเต็มยาวจนอ่านไม่ออก
type WakeFn = Arc<dyn Fn() + Send + Sync>;

/// ตัวอ่าน clipboard ที่ชั้นบนเสียบเข้ามาตอนสร้าง pool
///
/// ★ หลักการเดียวกับ [`WakeHandle`]: `refx-asset` ไม่รู้จัก `winit` ฉันใด
/// ก็ไม่รู้จัก `arboard` ฉันนั้น (ARCHITECTURE §2) — มันรู้แค่ว่า "มีใครสักคน
/// หยิบของจาก clipboard มาให้ได้" ตัวจริงคือ `refx_platform::clipboard::SystemClipboard`
/// ซึ่ง `refx-ui` เป็นคนเสียบให้
///
/// ต่างจาก `WakeHandle` ตรงที่ **เสียบตอนสร้างเท่านั้น ต่อทีหลังไม่ได้** —
/// งาน clipboard ที่วิ่งมาก่อนใครจะต่อสายให้ ต้องไม่กลายเป็น "เงียบหาย"
type ClipboardHandle = Arc<dyn ClipboardReader>;

/// ★★ ที่พักของภาพที่วาง ที่ชั้นบนเสียบเข้ามาตอนสร้าง pool
///
/// หลักการเดียวกับ [`ClipboardHandle`] เป๊ะ: คนที่ *มีไบต์* คือ worker ตัวนี้
/// ส่วนคนที่ *รู้ว่าไฟล์ต้องไปไหนและเขียนยังไงให้ atomic* คือ `refx-io::spool`
/// ซึ่ง `refx-asset` พึ่งไม่ได้ (ARCHITECTURE §2) → กลับทิศด้วย trait ใน `refx-core`
///
/// `None` = pool นี้ไม่มีที่พัก (เทสต์/fuzz) ภาพที่วางจะขึ้นจอได้ตามปกติ
/// แต่คมได้แค่ระดับ thumbnail และหายไปตอนปิดโปรแกรม
type SpoolHandle = Arc<dyn refx_core::spool::PastedImageStore>;

/// ของที่ worker ทุกตัวใช้ร่วมกันตลอดอายุ pool
///
/// รวมเป็น struct เดียวเพราะส่งทีละตัวทำให้ลายเซ็นของ [`worker_loop`] ยาวจนอ่านไม่ออก
/// และทุกครั้งที่เพิ่มของใหม่ต้องไปแก้ทุกชั้นที่ส่งต่อกันลงไป
struct WorkerContext {
    stats: Arc<PoolStats>,
    budget: Arc<RamBudget>,
    limits: Limits,
    wake: WakeHandle,
    io: Option<crossbeam_channel::Sender<IoRequest>>,
    /// `None` = pool นี้อ่าน clipboard ไม่ได้ (เทสต์/fuzz ที่ไม่ต้องการ)
    /// งาน `JobSource::Clipboard` จะได้ `Failed` พร้อมเหตุผลที่ชัด **ไม่ใช่เงียบหาย**
    clipboard: Option<ClipboardHandle>,
    /// ที่พักของภาพที่วาง — ดู [`SpoolHandle`]
    spool: Option<SpoolHandle>,
}

/// ตัวปลุก UI ที่ worker เรียกเมื่อมีผลใหม่
///
/// ★ ถ้าไม่มีตัวนี้ ภาพที่ decode เสร็จจะไม่ขึ้นจนกว่าผู้ใช้จะขยับเมาส์
/// = "ลากภาพเข้ามาแล้วไม่มีอะไรเกิดขึ้น" ซึ่งเป็นข้อแรกสุดที่ CLAUDE.md ห้าม
/// (เงื่อนไขข้อ 2 ของ docs/04 §1)
///
/// เป็นช่องว่างที่เติมทีหลังได้ เพราะ pool ถูกสร้าง **ก่อน** event loop มีตัวตน
/// `refx-asset` จึงไม่ต้องรู้จัก winit เลย — รับแค่ closure
#[derive(Clone, Default)]
pub struct WakeHandle(Arc<parking_lot::Mutex<Option<WakeFn>>>);

impl std::fmt::Debug for WakeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WakeHandle")
            .field("connected", &self.0.lock().is_some())
            .finish()
    }
}

impl WakeHandle {
    /// ผูกตัวปลุกจริงเข้ามา (เรียกตอน event loop พร้อมแล้ว)
    pub fn connect(&self, waker: impl Fn() + Send + Sync + 'static) {
        *self.0.lock() = Some(Arc::new(waker));
    }

    /// ปลุก UI — ไม่ทำอะไรถ้ายังไม่ได้ผูก
    pub fn wake(&self) {
        // clone ออกมาก่อนแล้วปล่อย lock — ห้ามถือ lock ตอนเรียก closure
        // ไม่งั้นถ้า closure ไปเรียก wake() ซ้ำจะ deadlock
        let waker = self.0.lock().clone();
        if let Some(waker) = waker {
            waker();
        }
    }
}

/// ข้อมูลของ **ไฟล์** ที่ item ต้องเก็บไว้ — ไม่ใช่ข้อมูลของ *ภาพ* (P3-4)
///
/// ★ แยกจาก [`Thumbnail`] โดยตั้งใจ: นั่นคือผลของการ *ถอดรหัสภาพ* ส่วนนี่คือ
/// สิ่งที่ระบบไฟล์บอก · ภาพจาก clipboard มี `Thumbnail` แต่ไม่มี `SourceMeta`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceMeta {
    /// เวลาที่ไฟล์ถูกแก้ครั้งล่าสุด (unix **millis**) — `0` = ไม่รู้
    ///
    /// millis เพราะ `docs/02 §2.3` กำหนดหน่วยนี้ให้ `AssetRef::mtime`
    /// (cache key ใช้ **วินาที** ซึ่งเป็นคนละค่ากันโดยตั้งใจ — ดู `PathFingerprint`)
    pub mtime_ms: i64,
    /// ขนาดไฟล์เป็นไบต์ — `0` = ไม่รู้
    pub bytes: u64,
}

impl SourceMeta {
    /// อ่านจากระบบไฟล์ — ★ **อยู่บน worker เท่านั้น** (I-2)
    ///
    /// อ่านไม่ได้ = ค่าศูนย์ ไม่ใช่ error: การเรียงตามวันที่เป็นของแถม
    /// ส่วนภาพต้องขึ้นจอให้ได้เสมอ (I-3)
    #[must_use]
    pub fn read(path: &std::path::Path) -> Self {
        let Ok(meta) = std::fs::metadata(path) else {
            return Self::default();
        };
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0);
        Self {
            mtime_ms,
            bytes: meta.len(),
        }
    }
}

/// งานนี้ต้องการผลลัพธ์แบบไหน
///
/// ทั้งสองแบบใช้เกราะ decode ชุดเดียวกันหมด ต่างกันแค่ขั้นย่อขนาดตอนท้าย
// ★ ไม่ derive `Eq` เพราะ `Sample` ถือ f32 — และไม่มีใครต้องการมัน
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JobTarget {
    /// ภาพย่อ 128 px สำหรับ atlas — ผ่าน cache.sqlite
    Thumbnail,
    /// working texture ชั้น B พร้อม mip chain (docs/04 §4)
    ///
    /// **ไม่ผ่าน cache.sqlite** — schema ของ cache เก็บ thumbnail 128 px เท่านั้น
    /// และภาพขนาดนี้ decode ใหม่เร็วกว่าการขยาย schema ให้รองรับหลายขนาด
    Working {
        /// ความกว้าง/สูงเป้าหมาย (power of two)
        size: u32,
    },
    /// ★★ อ่านสีของ **pixel เดียว** จากภาพต้นฉบับ — color picker (P2-10)
    ///
    /// **ไม่ผ่าน cache.sqlite และห้ามผ่าน** — cache เก็บ thumbnail 128 px
    /// ที่ถูกบีบเป็นจัตุรัสแล้ว การอ่านสีจากมันคือการอ่าน**ค่าเฉลี่ยของ pixel
    /// ต้นฉบับหลายสิบตัว** ซึ่งไม่ใช่สีที่ผู้ใช้จิ้ม (ภาพ 4000² → 1 px ของ thumb
    /// คือ 31×23 px ของจริง) · ROADMAP P2-10 บังคับว่า **ต้องเป็นสีของต้นฉบับ**
    ///
    /// ยอมจ่ายค่า decode หนึ่งครั้งต่อการจิ้มหนึ่งครั้ง: มันเป็นการกระทำที่ผู้ใช้
    /// ตั้งใจทำทีละครั้ง ไม่ใช่สิ่งที่เกิดทุกเฟรม และเกราะ/เพดาน RAM/timeout
    /// ทั้งชุดใช้ร่วมกับเส้นทางปกติหมด (I-4)
    Sample {
        /// ตำแหน่งแนวนอนในภาพต้นฉบับ สัดส่วน `0..1` (จาก `refx_core::pick::source_uv`)
        u: f32,
        /// ตำแหน่งแนวตั้งในภาพต้นฉบับ สัดส่วน `0..1`
        v: f32,
    },
}

/// ภาพของงานนี้มาจากไหน
///
/// ★ ทั้งสองทางใช้เกราะ เพดาน RAM คิว และ cancellation ชุดเดียวกันหมด (I-4)
/// ต่างกันแค่ "หยิบ pixel มาจากไหน" ซึ่งเป็นสองบรรทัดแรกของงานเท่านั้น
#[derive(Debug, Clone)]
pub enum JobSource {
    /// ไฟล์บนดิสก์ — เส้นทางปกติของ drag & drop และ `--open-dir`
    File(PathBuf),

    /// ภาพจาก clipboard (`Ctrl+V`)
    ///
    /// ★ **ไม่ผ่าน cache.sqlite โดยตั้งใจ** (ตัดสิน P1-8): cache key ที่ผูกมัดไว้คือ
    /// `(hash, mtime, size)` ซึ่ง clipboard ไม่มี `mtime` ให้ ถ้าใส่ค่าปลอมแทน
    /// จุดบอดของ fast hash จะกลับมาทันที — ซึ่ง `mtime` มีไว้ปิดพอดี
    /// บวกกับภาพจาก clipboard ตามธรรมชาติใช้ครั้งเดียว การเก็บมีแต่จะไล่
    /// thumbnail ของไฟล์จริงออกจาก LRU
    ///
    /// **อ่าน clipboard เกิดบน worker นี้** เพราะการเปิด clipboard บล็อกได้ (I-2)
    Clipboard,
}

impl JobSource {
    /// ไฟล์ต้นทาง ถ้ามี — clipboard ไม่มีไฟล์ให้กลับไปอ่านซ้ำ
    #[must_use]
    pub fn file(&self) -> Option<&std::path::Path> {
        match self {
            Self::File(path) => Some(path),
            Self::Clipboard => None,
        }
    }

    /// ป้ายสำหรับ log และข้อความ error — **ชื่อไฟล์อย่างเดียว ไม่ใช่ path เต็ม**
    /// (docs/08 §5: path เต็มมีชื่อผู้ใช้อยู่ในนั้น)
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::File(path) => file_label(path),
            Self::Clipboard => "(clipboard)".to_owned(),
        }
    }
}

/// งาน decode หนึ่งชิ้น
#[derive(Debug, Clone)]
pub struct Job {
    /// คีย์ที่ใช้จับคู่ผลลัพธ์กลับไปหา item บน board
    pub hash: ContentHash,
    /// ภาพมาจากไหน
    pub source: JobSource,
    /// ระยะจากกึ่งกลาง viewport — **น้อย = ทำก่อน**
    ///
    /// ภาพนอกจอ (prefetch) ให้บวก penalty คงที่ไปเลยเพื่อให้ไปอยู่ท้ายคิวเสมอ
    pub priority: f32,
    /// ธงยกเลิก — ตั้งเป็น `true` เมื่อภาพหลุดออกนอก viewport
    pub cancel: Arc<AtomicBool>,
    /// ต้องการผลลัพธ์แบบไหน
    pub target: JobTarget,
}

/// เหตุผลที่ decode ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum JobFailure {
    /// ไฟล์ไม่ผ่านเกราะ
    #[error(transparent)]
    Load(#[from] LoadError),

    /// ใช้เวลานานเกินเพดาน
    #[error("decoding {file} took longer than {seconds}s")]
    Timeout {
        /// ชื่อไฟล์
        file: String,
        /// เพดานเวลา (วินาที)
        seconds: u64,
    },

    /// อ่าน clipboard ไม่ได้ หรือใน clipboard ไม่มีอะไรที่เปิดเป็นภาพได้
    #[error(transparent)]
    Clipboard(#[from] ClipboardError),
}

/// ผลของงาน decode
#[derive(Debug)]
pub enum JobResult {
    /// สำเร็จ
    Done {
        /// คีย์ของภาพ
        hash: ContentHash,
        /// thumbnail ที่พร้อมอัดลง atlas
        ///
        /// ส่ง thumbnail ไม่ใช่ภาพเต็ม — ภาพ 4000² คือ 64 MB ส่วน thumbnail คือ 64 KB
        thumb: Box<Thumbnail>,
        /// ★ mtime + ขนาดของ **ไฟล์ต้นฉบับ** (P3-4) — ศูนย์เมื่อไม่มีไฟล์ (clipboard)
        meta: SourceMeta,
        /// เวลาที่ใช้ตั้งแต่หยิบงานจนเสร็จ
        elapsed: Duration,
        /// ★★★ hash ของ **เนื้อภาพ** สำหรับภาพที่ไม่มีไฟล์ต้นทาง (clipboard)
        ///
        /// `Some` = ภาพใบนี้กำลังจะถูกพักลง spool ที่ `<spool_dir>/<hash>.png`
        /// ผู้เรียกต้องใช้ค่านี้เป็น `AssetRef::hash` **ไม่ใช่คีย์ของงาน** เพราะ:
        ///
        /// | ใคร | ต้องการอะไร |
        /// |---|---|
        /// | `spool::sweep` | `AssetRef::hash` ต้องตรงกับ **ชื่อไฟล์ใน spool** ไม่งั้นมันจะถูกลบทิ้งทั้งที่ board อ้างถึงอยู่ |
        /// | วางภาพเดิมซ้ำ | คีย์เดียวกัน → ไฟล์เดียว (คีย์ของงานเป็น `clipboard:N` ซึ่งต่างกันทุกครั้ง) |
        ///
        /// ★★ เป็น **hash ไม่ใช่ path** โดยตั้งใจ: ตอนที่ข้อความนี้ถูกส่ง ไฟล์ยัง
        /// เขียนไม่เสร็จ · การ encode PNG ของภาพ 6000×4000 กินเวลา **1.07 วินาที**
        /// ซึ่ง `docs/07 §2` ห้ามไม่ให้มาขวางการที่ภาพขึ้นจอ → ส่ง `Done` ออกไปก่อน
        /// แล้วยืนยันด้วย [`JobResult::Spooled`] เมื่อไฟล์ลงดิสก์จริง
        spooled: Option<ContentHash>,
    },
    /// working texture พร้อมใช้ (docs/04 §4 ชั้น B)
    Working {
        /// คีย์ของภาพ
        hash: ContentHash,
        /// ภาพความละเอียดกลางพร้อม mip chain
        image: Box<WorkingImage>,
        /// เวลาที่ใช้ตั้งแต่หยิบงานจนเสร็จ
        elapsed: Duration,
    },
    /// ★ สีของ pixel เดียวจากภาพต้นฉบับ — color picker (P2-10)
    Sampled {
        /// คีย์ของภาพ
        hash: ContentHash,
        /// สี RGBA **ของต้นฉบับ** ยังไม่ผ่าน filter ใด ๆ
        rgba: [u8; 4],
        /// pixel ที่อ่านมาจริง (หลังแก้ EXIF orientation แล้ว)
        source_px: (u32, u32),
    },
    /// ถูกยกเลิกก่อนหรือระหว่างทำ (ผู้ใช้ pan ผ่านไปแล้ว)
    Cancelled {
        /// คีย์ของภาพ
        hash: ContentHash,
        /// ★★ **งานนี้เป็นงานชนิดไหน** — ผู้เรียกต้องแยกให้ออกว่าใบที่หายไป
        /// เป็นภาพที่ผู้ใช้กำลังรออยู่ (`Thumbnail`) หรือเป็นแค่ภาพคมกว่าเดิม
        /// (`Working`) ที่หายไปแล้วไม่มีใครเดือดร้อน
        ///
        /// ★ เดิมไม่มีฟิลด์นี้ ชั้น UI จึงนับงวดที่ลากเข้ามาให้จบไม่ได้เลย:
        /// นับทุกใบที่ถูกยกเลิก = นับงาน working texture ปนเข้ามาแล้วงวดจบเร็ว
        /// เกินจริง · ไม่นับเลย = **ผู้ใช้ pan ระหว่างลากไฟล์แล้วงวดค้างถาวร**
        /// (แถบ "กำลังโหลด" ไม่หาย และข้อความ board เต็มไม่มีวันขึ้น)
        target: JobTarget,
    },
    /// ★ ใน clipboard เป็น **รายชื่อไฟล์** (ก๊อปไฟล์จาก Explorer) ไม่ใช่ภาพดิบ
    ///
    /// worker เป็นคนเดียวที่รู้ได้ เพราะต้องเปิด clipboard ถึงจะเห็น และการเปิด
    /// clipboard บล็อกได้ (I-2) — ส่งกลับให้ UI ยัดเข้าเส้นทาง drag & drop
    /// เส้นเดิมทั้งเส้น (มี cache, มี EXIF, ขอภาพคมตอนซูมได้)
    ClipboardFiles {
        /// คีย์ของงานที่ขอมา — ใช้ปิดสถานะ "กำลังวาง" ของ UI
        hash: ContentHash,
        /// ไฟล์ที่อยู่ใน clipboard
        paths: Vec<PathBuf>,
    },
    /// ★★★ ภาพที่วาง **ลงดิสก์เรียบร้อยแล้ว** — ตามหลัง [`JobResult::Done`] ของงานเดียวกัน
    ///
    /// ★ นี่ไม่ใช่ "งาน" ใหม่ — เป็นผลตามหลังของงานที่ถูกนับไปแล้ว จึงไม่เข้า
    /// สถิติและไม่เข้างวดที่ผู้ใช้กำลังรออยู่ (ไม่งั้นงวดจะจบเร็วเกินจริง)
    ///
    /// ★★ **ส่งเมื่อไฟล์มีอยู่จริงเท่านั้น** เพราะนี่คือสัญญาณที่ปลดล็อกการขอ
    /// working texture ของภาพใบนั้น — ปลดก่อนไฟล์พร้อม = ผู้ใช้ได้ error
    /// "เปิดภาพไม่ได้" ที่เขาทำอะไรกับมันไม่ได้ ทุกครั้งที่ซูมเข้าในช่วงนั้น
    Spooled {
        /// hash ของเนื้อภาพ — ตรงกับ `spooled` ใน [`JobResult::Done`] และกับ `AssetRef::hash`
        hash: ContentHash,
        /// ไฟล์ที่พักไว้ (`<spool_dir>/<hash>.png`)
        path: PathBuf,
    },
    /// ล้มเหลว — item จะขึ้นสถานะ "โหลดไม่ได้" ไม่ใช่ crash (I-7)
    Failed {
        /// คีย์ของภาพ
        hash: ContentHash,
        /// สาเหตุ
        reason: JobFailure,
        /// งานนี้เป็นงานชนิดไหน — เหตุผลเดียวกับ [`JobResult::Cancelled`]
        target: JobTarget,
    },
}

impl JobResult {
    /// คีย์ของงานนี้ ไม่ว่าผลจะเป็นอะไร
    #[must_use]
    pub fn hash(&self) -> ContentHash {
        match self {
            Self::Done { hash, .. }
            | Self::Working { hash, .. }
            | Self::Sampled { hash, .. }
            | Self::Cancelled { hash, .. }
            | Self::ClipboardFiles { hash, .. }
            | Self::Spooled { hash, .. }
            | Self::Failed { hash, .. } => *hash,
        }
    }
}

/// สถิติของ pool สำหรับ status bar (I-6) และการตรวจว่า cancel ทำงานจริง
#[derive(Debug, Default)]
pub struct PoolStats {
    /// งานที่ส่งเข้ามาทั้งหมด
    pub submitted: AtomicU64,
    /// งานที่ทำเสร็จ
    pub completed: AtomicU64,
    /// งานที่ถูกยกเลิก — ★ ตัวเลขนี้คือหลักฐานว่า cancellation ทำงาน
    pub cancelled: AtomicU64,
    /// งานที่ล้มเหลว
    pub failed: AtomicU64,
    /// งานที่ timeout
    pub timed_out: AtomicU64,
}

impl PoolStats {
    /// อ่านค่าเป็นตัวเลขธรรมดา
    #[must_use]
    pub fn snapshot(&self) -> PoolStatsSnapshot {
        PoolStatsSnapshot {
            submitted: self.submitted.load(AtomicOrdering::Relaxed),
            completed: self.completed.load(AtomicOrdering::Relaxed),
            cancelled: self.cancelled.load(AtomicOrdering::Relaxed),
            failed: self.failed.load(AtomicOrdering::Relaxed),
            timed_out: self.timed_out.load(AtomicOrdering::Relaxed),
        }
    }
}

/// สำเนาสถิติ ณ ขณะหนึ่ง
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PoolStatsSnapshot {
    /// งานที่ส่งเข้ามาทั้งหมด
    pub submitted: u64,
    /// งานที่ทำเสร็จ
    pub completed: u64,
    /// งานที่ถูกยกเลิก
    pub cancelled: u64,
    /// งานที่ล้มเหลว
    pub failed: u64,
    /// งานที่ timeout
    pub timed_out: u64,
}

impl PoolStatsSnapshot {
    /// งานที่จบแล้วทุกแบบรวมกัน
    #[must_use]
    pub fn finished(&self) -> u64 {
        self.completed + self.cancelled + self.failed
    }
}

/// รายการในคิว — เรียงตาม priority น้อยไปมาก
struct Pending {
    job: Job,
    /// ลำดับที่ส่งเข้ามา ใช้ตัดสินเมื่อ priority เท่ากัน
    ///
    /// จำเป็นเพื่อให้ผลลัพธ์ **deterministic** — `BinaryHeap` ไม่รับประกันลำดับของค่าที่เท่ากัน
    seq: u64,
    /// priority แปลงเป็นจำนวนเต็มที่เรียงได้ (f32 ไม่ implement `Ord`)
    key: u32,
}

impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.seq == other.seq
    }
}
impl Eq for Pending {}

impl Ord for Pending {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap เป็น max-heap แต่เราอยาก "priority น้อยมาก่อน" จึงกลับด้าน
        other
            .key
            .cmp(&self.key)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}
impl PartialOrd for Pending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// แปลง priority (f32 ระยะทาง) เป็นคีย์จำนวนเต็มที่เรียงลำดับได้เหมือนกัน
///
/// สำหรับ f32 ที่ไม่ติดลบ รูปแบบบิต IEEE-754 เรียงลำดับตรงกับค่าจริงอยู่แล้ว
/// ค่าติดลบ/NaN ถือเป็น 0 (สำคัญที่สุด) เพราะเป็นค่าที่ไม่ควรเกิดอยู่แล้ว
fn priority_key(priority: f32) -> u32 {
    if priority.is_nan() || priority <= 0.0 {
        return 0;
    }
    priority.to_bits()
}

/// คิวงานที่ worker แย่งกันหยิบ
struct Queue {
    heap: Mutex<QueueState>,
    ready: Condvar,
}

struct QueueState {
    pending: BinaryHeap<Pending>,
    next_seq: u64,
    shutdown: bool,
}

impl Queue {
    fn new() -> Self {
        Self {
            heap: Mutex::new(QueueState {
                pending: BinaryHeap::new(),
                next_seq: 0,
                shutdown: false,
            }),
            ready: Condvar::new(),
        }
    }

    fn push(&self, job: Job) {
        let mut state = self.heap.lock();
        let seq = state.next_seq;
        state.next_seq += 1;
        let key = priority_key(job.priority);
        state.pending.push(Pending { job, seq, key });
        drop(state);
        self.ready.notify_one();
    }

    /// หยิบงานถัดไป — `None` เมื่อ pool ปิด
    fn pop(&self) -> Option<Job> {
        let mut state = self.heap.lock();
        loop {
            if let Some(next) = state.pending.pop() {
                return Some(next.job);
            }
            if state.shutdown {
                return None;
            }
            self.ready.wait(&mut state);
        }
    }

    fn shutdown(&self) {
        let mut state = self.heap.lock();
        state.shutdown = true;
        state.pending.clear();
        drop(state);
        self.ready.notify_all();
    }

    fn len(&self) -> usize {
        self.heap.lock().pending.len()
    }
}

/// กลุ่ม worker ที่ทำงาน decode
pub struct DecodePool {
    queue: Arc<Queue>,
    workers: Vec<std::thread::JoinHandle<()>>,
    results: crossbeam_channel::Receiver<JobResult>,
    stats: Arc<PoolStats>,
    budget: Arc<RamBudget>,
    wake: WakeHandle,
}

/// ผลของการถาม cache ก่อน decode
enum CacheLookup {
    /// เจอใน cache — ไม่ต้อง decode เลย
    Hit(Box<Thumbnail>),
    /// ไม่เจอ — ต้อง decode พร้อมคีย์ที่จะใช้เก็บผล
    Miss(CacheKey),
    /// ไม่มี cache ให้ใช้ (เปิด DB ไม่ได้) — decode ตรง ๆ
    Unavailable,
}

impl DecodePool {
    /// เปิด pool ด้วยจำนวน worker และเพดาน RAM ที่กำหนด
    ///
    /// # Panics
    /// ไม่ panic — ถ้าสร้าง thread ไม่ได้จะข้ามตัวนั้นแล้ว log ไว้
    #[must_use]
    pub fn new(
        workers: usize,
        budget: Arc<RamBudget>,
        limits: Limits,
        io: Option<crossbeam_channel::Sender<IoRequest>>,
        clipboard: Option<ClipboardHandle>,
        spool: Option<SpoolHandle>,
    ) -> Self {
        let workers = workers.max(1);
        let queue = Arc::new(Queue::new());
        let stats = Arc::new(PoolStats::default());
        let (tx, results) = crossbeam_channel::unbounded();

        let wake = WakeHandle::default();
        let mut handles = Vec::with_capacity(workers);
        for index in 0..workers {
            let queue = Arc::clone(&queue);
            let ctx = WorkerContext {
                stats: Arc::clone(&stats),
                budget: Arc::clone(&budget),
                limits: limits.clone(),
                wake: wake.clone(),
                io: io.clone(),
                clipboard: clipboard.clone(),
                spool: spool.clone(),
            };
            let tx = tx.clone();

            match std::thread::Builder::new()
                .name(format!("refx-decode-{index}"))
                .spawn(move || worker_loop(&queue, &ctx, &tx))
            {
                Ok(handle) => handles.push(handle),
                Err(err) => tracing::error!(%err, index, "cannot spawn a decode worker"),
            }
        }
        // ทิ้ง sender ต้นฉบับ ไม่งั้น receiver จะไม่มีวันเห็นว่า worker ปิดหมดแล้ว
        drop(tx);

        tracing::info!(
            workers = handles.len(),
            ram_limit_mb = budget.limit() / (1 << 20),
            "decode pool started"
        );

        Self {
            queue,
            workers: handles,
            results,
            stats,
            budget,
            wake,
        }
    }

    /// ตัวปลุก UI — ผูกเข้ากับ event loop หลังหน้าต่างพร้อม
    #[must_use]
    pub fn wake_handle(&self) -> WakeHandle {
        self.wake.clone()
    }

    /// เปิด pool ด้วยค่าเริ่มต้นที่ **ผูกกับเครื่องจริง**
    ///
    /// เพดานขนาดภาพคำนวณจาก RAM ที่ติดตั้ง (docs/05 §3) ไม่ใช่ค่าคงที่
    /// — ภาพที่ผ่านเกราะมาได้จึงไม่มีทางเกิน 1/8 ของ RAM เครื่อง
    ///
    /// `total_ram` · `clipboard` · `spool` **รับเข้ามา ไม่ได้ไปถามเอง** เพราะทั้งสาม
    /// อย่างต้องถาม OS หรือแตะดิสก์ ซึ่งเป็นงานของ `refx-platform`/`refx-io` —
    /// ชั้น asset ไม่รู้จักมัน (ARCHITECTURE §2, HANDOFF §2.0) ผู้เรียกจริงคือ `refx-ui`
    #[must_use]
    pub fn with_defaults(
        total_ram: u64,
        clipboard: ClipboardHandle,
        io: Option<crossbeam_channel::Sender<IoRequest>>,
        spool: Option<SpoolHandle>,
    ) -> Self {
        Self::new(
            default_worker_count(),
            Arc::new(RamBudget::new(crate::budget::DEFAULT_RAM_LIMIT)),
            Limits::for_system(total_ram),
            io,
            Some(clipboard),
            spool,
        )
    }

    /// ส่งงานเข้าคิว — **ไม่บล็อก** เรียกจาก UI thread ได้ (I-2)
    pub fn submit(&self, job: Job) {
        self.stats.submitted.fetch_add(1, AtomicOrdering::Relaxed);
        self.queue.push(job);
    }

    /// รับผลที่เสร็จแล้วโดยไม่รอ — **UI thread ใช้ตัวนี้** (I-2)
    pub fn try_recv(&self) -> Option<JobResult> {
        self.results.try_recv().ok()
    }

    /// ช่องรับผล (สำหรับเทสต์หรือ worker ที่รอได้)
    #[must_use]
    pub fn results(&self) -> &crossbeam_channel::Receiver<JobResult> {
        &self.results
    }

    /// สถิติสำหรับ status bar
    #[must_use]
    pub fn stats(&self) -> PoolStatsSnapshot {
        self.stats.snapshot()
    }

    /// จำนวนงานที่ยังค้างคิว
    #[must_use]
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// เพดาน/การใช้ RAM ปัจจุบัน (ไบต์) — ใช้แสดงบน status bar
    #[must_use]
    pub fn ram_usage(&self) -> (usize, usize) {
        (self.budget.used(), self.budget.limit())
    }

    /// จำนวน worker ที่เปิดได้จริง
    #[must_use]
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }
}

impl Drop for DecodePool {
    fn drop(&mut self) {
        self.queue.shutdown();
        for handle in self.workers.drain(..) {
            if handle.join().is_err() {
                tracing::error!("decode worker ended abnormally");
            }
        }
    }
}

/// ลูปของ worker หนึ่งตัว
fn worker_loop(queue: &Queue, ctx: &WorkerContext, tx: &crossbeam_channel::Sender<JobResult>) {
    let stats = &ctx.stats;
    while let Some(job) = queue.pop() {
        let (result, deferred) = run_job(&job, ctx);

        match &result {
            JobResult::Done { .. }
            | JobResult::Working { .. }
            | JobResult::Sampled { .. }
            | JobResult::ClipboardFiles { .. } => {
                stats.completed.fetch_add(1, AtomicOrdering::Relaxed);
            }
            JobResult::Cancelled { .. } => {
                stats.cancelled.fetch_add(1, AtomicOrdering::Relaxed);
            }
            JobResult::Failed { .. } => {
                stats.failed.fetch_add(1, AtomicOrdering::Relaxed);
            }
            // ★ ไม่มีทางมาถึงตรงนี้: `Spooled` ถูกส่งจากกิ่งด้านล่างเท่านั้น
            //   ไม่ใช่ผลของ `run_job` — และมันไม่ใช่งานจึงไม่เข้าสถิติ
            JobResult::Spooled { .. } => {}
        }

        // main thread อาจปิดไปแล้ว — ไม่ใช่ error
        if tx.send(result).is_err() {
            break;
        }

        // ★ ปลุก UI ให้มาเก็บผล — ถ้าไม่ปลุก event loop จะหลับต่อ
        //   แล้วภาพจะไม่ขึ้นจนกว่าผู้ใช้จะขยับเมาส์
        //   winit รวบ request_redraw หลายครั้งเป็นเฟรมเดียวอยู่แล้ว จึงไม่เปลือง
        ctx.wake.wake();

        // ★★★ **หลังจากภาพขึ้นจอแล้วเท่านั้น** ค่อย encode PNG ลง spool
        //
        //   `docs/07 §2` บังคับข้อนี้ไว้ตรง ๆ และตัวเลขก็บอกเอง: 6000×4000
        //   ใช้เวลา encode **1.07 วินาที** ถ้าทำก่อนส่ง `Done` ผู้ใช้จะเห็น
        //   โปรแกรมค้างทุกครั้งที่กด Ctrl+V กับภาพใหญ่
        //
        //   ★ ลำดับนี้ถูกบังคับด้วย **โครงสร้าง** ไม่ใช่ด้วยความจำ: `run_job`
        //   ส่งงานที่เลื่อนออกไปกลับมาเป็นค่าคืน มันจึงเขียนให้ทำก่อนส่งผลไม่ได้
        if let Some(task) = deferred {
            let hash = task.hash;
            let stored = spool_pasted_image(&task, ctx);
            drop(task); // คืนโควตา RAM ของภาพเต็มทันที ไม่รอรอบถัดไปของลูป
            if let Some(path) = stored {
                if tx.send(JobResult::Spooled { hash, path }).is_err() {
                    break;
                }
                ctx.wake.wake();
            }
        }
    }
}

/// ★★ งานที่ต้องทำ **หลังส่งผลออกไปแล้ว** — พัก PNG ของภาพที่วางลงดิสก์
///
/// มีอยู่เพื่อบังคับลำดับด้วยชนิดข้อมูล: ตราบใดที่มันเป็น *ค่าคืน* ของ
/// [`run_job`] การ encode จะเกิดก่อน `Done` ถูกส่งไม่ได้เลย (`docs/07 §2`)
struct SpoolTask {
    /// hash ของเนื้อภาพ — ชื่อไฟล์ใน spool
    hash: ContentHash,
    /// พิกเซลที่จะถูก encode (ยังถืออยู่เพราะยังไม่ได้เขียน)
    image: RgbaImage,
    /// ★ ใบจองโควตา RAM ของภาพเต็ม — ต้องถือต่อจนกว่าจะ encode เสร็จ
    /// ไม่งั้นถังกลางจะคิดว่าว่างแล้วปล่อยงานอื่นเข้ามาซ้อน (I-6)
    _reservation: crate::budget::RamReservation,
}

/// เขียน PNG ของภาพที่วางลง spool — **เรียกหลังส่งผลออกไปแล้วเท่านั้น**
///
/// คืน `None` เมื่อทำไม่สำเร็จ ซึ่ง **ไม่ใช่ error ที่ต้องหยุดงาน**: ภาพขึ้นจอ
/// ไปแล้วเรียบร้อย สิ่งที่เสียไปคือความคมตอนซูมกับความสามารถในการกู้คืน
fn spool_pasted_image(task: &SpoolTask, ctx: &WorkerContext) -> Option<PathBuf> {
    let spool = ctx.spool.as_ref()?;
    let started = Instant::now();
    let png = match crate::encode::to_png(&task.image) {
        Ok(png) => png,
        Err(err) => {
            tracing::warn!(%err, hash = %task.hash.short(), "cannot encode the pasted image");
            return None;
        }
    };
    let bytes = png.len();
    let path = spool.store(task.hash, &png)?;
    tracing::info!(
        hash = %task.hash.short(),
        bytes,
        elapsed = ?started.elapsed(),
        "spooled a pasted image"
    );
    Some(path)
}

/// ทำงานหนึ่งชิ้นจนจบ
///
/// ★ ค่าคืนที่สองคืองานที่ต้องทำ **หลังส่งผลแล้ว** ([`SpoolTask`])
fn run_job(job: &Job, ctx: &WorkerContext) -> (JobResult, Option<SpoolTask>) {
    let started = Instant::now();
    let io = ctx.io.as_ref();

    // ★ เช็คธงยกเลิก **ก่อนเริ่ม** — ผู้ใช้ pan ผ่านไปแล้วก็ไม่ต้องเสียแรงเลย
    if job.cancel.load(AtomicOrdering::Relaxed) {
        return (
            JobResult::Cancelled {
                hash: job.hash,
                target: job.target,
            },
            None,
        );
    }

    let file = job.source.label();

    // ★ mtime + ขนาดของไฟล์ต้นฉบับ (P3-4) — **อ่านบน worker เท่านั้น** (I-2)
    //   ต้องอ่านก่อนแยกทาง cache เพราะเส้นทาง cache hit ก็ต้องได้ค่านี้เหมือนกัน
    //   · ราคาคือ `stat` หนึ่งครั้งต่อไฟล์ (ไมโครวินาที) ซึ่งเกิดตอน ingest
    //     ครั้งเดียวต่อภาพ ไม่ใช่ต่อเฟรม
    let meta = job.source.file().map(SourceMeta::read).unwrap_or_default();

    // ★ ถาม cache ก่อน — เจอแล้วไม่ต้องอ่านไฟล์ ไม่ต้อง decode เลย
    //   นี่คือเส้นทางที่ผู้ใช้เจอทุกวัน (เปิดไฟล์เดิมซ้ำ ๆ)
    //   working texture ข้ามขั้นนี้ — cache เก็บแต่ thumbnail 128 px (docs/05 §5)
    //   clipboard ข้ามเช่นกัน — ไม่มี mtime ให้ประกอบคีย์ (ดู `JobSource::Clipboard`)
    let lookup = match (job.target, job.source.file()) {
        (JobTarget::Thumbnail, Some(path)) => cache_lookup(path, io),
        _ => CacheLookup::Unavailable,
    };
    if let CacheLookup::Hit(thumb) = lookup {
        return (
            JobResult::Done {
                hash: job.hash,
                thumb,
                meta,
                elapsed: started.elapsed(),
                // cache hit เกิดกับไฟล์บนดิสก์เท่านั้น — clipboard ไม่เข้า cache
                spooled: None,
            },
            None,
        );
    }

    // ★ หยิบ pixel เข้ามา — จุดเดียวที่สองแหล่งต่างกัน หลังจากนี้เหมือนกันหมด
    let (image, reservation) = match acquire_pixels(job, ctx) {
        Acquired::Ready { image, reservation } => (image, reservation),
        Acquired::Files(paths) => {
            return (
                JobResult::ClipboardFiles {
                    hash: job.hash,
                    paths,
                },
                None,
            );
        }
        Acquired::Cancelled => {
            return (
                JobResult::Cancelled {
                    hash: job.hash,
                    target: job.target,
                },
                None,
            );
        }
        Acquired::Failed(reason) => {
            return (
                JobResult::Failed {
                    hash: job.hash,
                    reason,
                    target: job.target,
                },
                None,
            );
        }
    };

    // ★ working texture แยกทางตรงนี้ — ใช้เกราะทุกชั้นร่วมกันมาจนถึงจุดนี้
    if let JobTarget::Working { size } = job.target {
        let built = working::build(&image, size);
        drop(image);
        let elapsed = started.elapsed();

        // ยกเลิกกลางทางได้ — ผู้ใช้ซูมออกไปแล้วก็ไม่ต้องส่งของหนักกลับไป
        if job.cancel.load(AtomicOrdering::Relaxed) {
            return (
                JobResult::Cancelled {
                    hash: job.hash,
                    target: job.target,
                },
                None,
            );
        }
        return match built {
            Some(image) => (
                JobResult::Working {
                    hash: job.hash,
                    image: Box::new(image),
                    elapsed,
                },
                None,
            ),
            None => {
                tracing::warn!(file, size, "could not build the working texture");
                (
                    JobResult::Cancelled {
                        hash: job.hash,
                        target: job.target,
                    },
                    None,
                )
            }
        };
    }

    // ★ picker แยกทางตรงนี้ — ใช้เกราะทุกชั้นร่วมกันมาจนถึงจุดนี้เหมือน working
    //   อ่าน pixel เดียวแล้วทิ้งภาพทันที ไม่ย่อ ไม่เข้า cache ไม่ส่งของหนักกลับ
    if let JobTarget::Sample { u, v } = job.target {
        let (source_px, rgba) = sample_pixel(&image, u, v);
        drop(image);
        if job.cancel.load(AtomicOrdering::Relaxed) {
            return (
                JobResult::Cancelled {
                    hash: job.hash,
                    target: job.target,
                },
                None,
            );
        }
        return (
            JobResult::Sampled {
                hash: job.hash,
                rgba,
                source_px,
            },
            None,
        );
    }

    // ขั้น 6: ย่อเป็น thumbnail (Lanczos3) — ทำบน worker ไม่ใช่ UI thread
    // ขั้น 7 (BC7) ถูกตัดออกจาก P1 แล้ว — docs/04 §4
    let thumb = make_thumbnail(&image);

    // ★★★ ภาพที่ **ไม่มีไฟล์ต้นทาง** ต้องได้คีย์จากพิกเซลของมันเอง
    //
    //   คีย์ของงานเป็น `clipboard:N` ซึ่งต่างกันทุกครั้งที่วาง · ถ้าเอาไปใช้เป็น
    //   `AssetRef::hash` จะพังสองทางพร้อมกัน: วางภาพเดิมซ้ำได้ไฟล์ละใบใน spool
    //   และ `spool::sweep` จะหาชื่อไฟล์ที่ board อ้างถึงไม่เจอ **แล้วลบทิ้ง**
    //
    //   ★ ทำ **ก่อน** ส่ง `Done` เพราะ item ต้องมีคีย์ที่ถูกตั้งแต่ถูกสร้าง
    //   (ราคาคือการ hash ครั้งเดียว ไม่ใช่การ encode ที่กินเป็นวินาที)
    let spooled = (ctx.spool.is_some() && job.source.file().is_none())
        .then(|| crate::hash::hash_pasted(image.width(), image.height(), image.as_raw()));

    let deferred = spooled.map(|hash| SpoolTask {
        hash,
        image,
        _reservation: reservation,
    });
    // ★ ภาพเต็มถูกส่งต่อให้ `SpoolTask` แล้วในกรณี clipboard — กรณีอื่นคืน RAM ทันที
    //   (ไม่ต้องรอจบฟังก์ชัน) เหมือนเดิมทุกประการ

    let elapsed = started.elapsed();

    // ★ Timeout: ยกเลิก decode กลางคันไม่ได้ในทางปฏิบัติ แต่ต้องไม่ให้ทั้งคิวค้าง
    //   และต้องบอกผู้ใช้ได้ว่าไฟล์ไหนมีปัญหา (docs/06 §3)
    if elapsed > DECODE_TIMEOUT {
        ctx.stats.timed_out.fetch_add(1, AtomicOrdering::Relaxed);
        tracing::warn!(file, ?elapsed, "decode took longer than the timeout");
        return (
            JobResult::Failed {
                hash: job.hash,
                reason: JobFailure::Timeout {
                    file,
                    seconds: DECODE_TIMEOUT.as_secs(),
                },
                target: job.target,
            },
            None,
        );
    }

    // เช็คธงครั้งสุดท้าย — ถ้าผู้ใช้ pan ผ่านไปแล้วก็ไม่ต้องส่งภาพกลับให้เปลือง
    //
    // ★ ยกเลิกแล้ว = ไม่มี item บน board = ไม่มีใครอ้างถึงไฟล์ใน spool
    //   จึงทิ้ง `deferred` ไปด้วย ไม่งั้นเราจะเขียนไฟล์ 78 MB ที่รอบเก็บกวาด
    //   รอบถัดไปจะลบทิ้งอยู่ดี
    if job.cancel.load(AtomicOrdering::Relaxed) {
        return (
            JobResult::Cancelled {
                hash: job.hash,
                target: job.target,
            },
            None,
        );
    }

    // เก็บลง cache เพื่อให้ครั้งหน้าไม่ต้อง decode อีก
    if let (CacheLookup::Miss(key), Some(io)) = (&lookup, io) {
        let _ = io.send(IoRequest::PutThumb {
            key: *key,
            entry: Box::new(ThumbEntry {
                width: thumb.source_width,
                height: thumb.source_height,
                format: 0,
                thumb_fmt: ThumbFormat::Rgba8,
                thumb: thumb.pixels.clone(),
                dominant: thumb.dominant,
            }),
        });
    }

    (
        JobResult::Done {
            hash: job.hash,
            thumb: Box::new(thumb),
            meta,
            elapsed,
            spooled,
        },
        deferred,
    )
}

/// อ่านสีของ pixel เดียวจากภาพที่ decode มาแล้ว
///
/// `u`/`v` เป็นสัดส่วน `0..1` ของภาพต้นฉบับ (ผลจาก `refx_core::pick::source_uv`)
///
/// ★ **ปัดลงแล้ว clamp** ไม่ปัดใกล้สุด: `u = 1.0` พอดี (ผู้ใช้จิ้มขอบขวาสุด)
/// ต้องได้ pixel สุดท้าย ไม่ใช่ pixel ที่ `width` ซึ่งอยู่นอกภาพ
/// ค่าที่ไม่ใช่ตัวเลขตกเป็น 0 เสมอ — ห้ามให้ index หลุดขอบไม่ว่าอะไรจะเข้ามา (I-4)
fn sample_pixel(image: &RgbaImage, u: f32, v: f32) -> ((u32, u32), [u8; 4]) {
    let axis = |t: f32, extent: u32| -> u32 {
        let last = extent.saturating_sub(1);
        if !t.is_finite() {
            return 0;
        }
        // คูณด้วย extent (ไม่ใช่ last) แล้ว clamp — ทำให้แต่ละ pixel กินช่วงเท่ากัน
        let scaled = t.clamp(0.0, 1.0) * extent as f32;
        (scaled as u32).min(last)
    };
    let (w, h) = (image.width(), image.height());
    if w == 0 || h == 0 {
        return ((0, 0), [0; 4]);
    }
    let (x, y) = (axis(u, w), axis(v, h));
    ((x, y), image.get_pixel(x, y).0)
}

/// ผลของการหยิบ pixel เข้ามา
enum Acquired {
    /// ได้ภาพพร้อมใบจองโควตา RAM ที่ต้องถือไว้ตลอดอายุของภาพ
    Ready {
        image: RgbaImage,
        reservation: crate::budget::RamReservation,
    },
    /// clipboard มีรายชื่อไฟล์ ไม่ใช่ภาพดิบ
    Files(Vec<PathBuf>),
    /// ผู้ใช้ pan/ซูมผ่านไปแล้ว
    Cancelled,
    /// ไม่ผ่านเกราะ
    Failed(JobFailure),
}

/// หยิบ pixel ของงานนี้เข้ามาให้พร้อมใช้
///
/// ★ นี่คือจุดเดียวที่ไฟล์กับ clipboard เดินคนละทาง หลังจากฟังก์ชันนี้คืนค่า
/// ทุกอย่าง (orientation ที่ทำไปแล้ว, ย่อ, timeout, cache, การส่งกลับ) เหมือนกันหมด
fn acquire_pixels(job: &Job, ctx: &WorkerContext) -> Acquired {
    match &job.source {
        JobSource::File(path) => acquire_from_file(path, job, &ctx.budget, &ctx.limits),
        JobSource::Clipboard => acquire_from_clipboard(job, ctx),
    }
}

/// อ่านไฟล์จากดิสก์แล้ว decode ผ่านเกราะครบทุกชั้น
fn acquire_from_file(
    path: &std::path::Path,
    job: &Job,
    budget: &Arc<RamBudget>,
    limits: &Limits,
) -> Acquired {
    // อ่านไฟล์ (ไม่ mmap — docs/06 §3) แล้วดูขนาดจาก header ก่อนขอโควตา
    let bytes = match read_file_guarded(path, limits) {
        Ok(bytes) => bytes,
        Err(err) => return Acquired::Failed(err.into()),
    };

    // เช็คซ้ำหลังอ่านไฟล์เสร็จ — การอ่านอาจใช้เวลานานถ้าไฟล์อยู่บนไดรฟ์เครือข่าย
    if job.cancel.load(AtomicOrdering::Relaxed) {
        return Acquired::Cancelled;
    }

    let (width, height) = match probe_dimensions(&bytes, limits) {
        Ok(dims) => dims,
        Err(err) => return Acquired::Failed(err.into()),
    };

    // ★ ขอโควตาจากถังกลาง — นอนรอถ้ายังไม่ว่าง (I-6)
    let reservation = budget.reserve(estimate_decode_bytes(width, height));

    // รอโควตาอาจใช้เวลานาน — เช็คธงอีกรอบก่อนลงมือจริง
    if job.cancel.load(AtomicOrdering::Relaxed) {
        return Acquired::Cancelled;
    }

    let image = match decode_guarded_labelled(&bytes, limits, &file_label(path)) {
        Ok(image) => image,
        Err(err) => return Acquired::Failed(JobFailure::Load(err)),
    };

    // แก้ EXIF orientation — ภาพจากมือถือจะตะแคงถ้าไม่ทำ
    Acquired::Ready {
        image: read_orientation(&bytes).apply(image),
        reservation,
    }
}

/// อ่านสิ่งที่อยู่ใน clipboard แล้วผ่านเกราะเท่าที่มีความหมายกับ pixel ดิบ
///
/// ★ **ทำไมอยู่บน worker:** การเปิด clipboard รอ OS ได้นานเป็นวินาทีถ้าโปรแกรมอื่น
/// ถือมันค้างอยู่ ทำบน UI thread = แอปค้างทั้งบาน (I-2)
///
/// ★ **ข้อจำกัดที่ยังปิดไม่ได้:** `arboard` จอง RAM ของ pixel ตั้งแต่ก่อนคืนค่า
/// เราจึงขอโควตาถังกลางได้หลังภาพอยู่ในมือแล้ว (ต่างจากไฟล์ที่ขอก่อน decode)
/// ผลคือ peak ชั่วคราวของการวางหนึ่งครั้งอยู่นอกถัง — ชั้น UI จึงจำกัดให้
/// **วางได้ทีละครั้ง** เพื่อไม่ให้ซ้อนกันหลายก้อน
fn acquire_from_clipboard(job: &Job, ctx: &WorkerContext) -> Acquired {
    // ★ ไม่มีตัวอ่านเสียบไว้ = ตอบว่า "ใช้ clipboard ไม่ได้" ให้ชัด **ห้ามเงียบ**
    //   (docs/08 §3.9 ข้อ 2) ถ้าปล่อยให้งานหายไปเฉย ๆ ผู้ใช้จะเห็นแค่ "กด Ctrl+V
    //   แล้วไม่มีอะไรเกิดขึ้น" ซึ่งแยกไม่ออกจากบั๊กและไม่มีร่องรอยให้ตามด้วย
    let Some(reader) = ctx.clipboard.as_ref() else {
        return Acquired::Failed(
            ClipboardError::Unavailable {
                reason: "no clipboard reader was wired into this decode pool".to_owned(),
            }
            .into(),
        );
    };

    let content = match reader.read() {
        Ok(content) => content,
        Err(err) => return Acquired::Failed(err.into()),
    };

    let raw = match content {
        ClipboardContent::Files(paths) => return Acquired::Files(paths),
        ClipboardContent::Image(image) => image,
    };

    // อ่าน clipboard อาจนาน — ผู้ใช้อาจกดอย่างอื่นไปแล้ว
    if job.cancel.load(AtomicOrdering::Relaxed) {
        return Acquired::Cancelled;
    }

    // ★ ผ่านเกราะ **ก่อน** ขอโควตา — ถ้าขอก่อน ภาพที่ประกาศขนาดโกงจะไปนอนรอ
    //   ถังว่างเปล่า ๆ ทั้งที่สุดท้ายก็โดนปฏิเสธอยู่ดี
    let image = match accept_rgba_guarded(raw.width, raw.height, raw.rgba, &ctx.limits) {
        Ok(image) => image,
        Err(err) => return Acquired::Failed(err.into()),
    };

    // ถือโควตาไว้เท่ากับที่ภาพกินจริง ตลอดช่วงที่ยังถือภาพอยู่ — เกณฑ์เดียวกับไฟล์
    let reservation = ctx
        .budget
        .reserve(estimate_decode_bytes(image.width(), image.height()));
    Acquired::Ready { image, reservation }
}

/// ถาม cache ว่ามี thumbnail ของไฟล์นี้อยู่แล้วไหม
///
/// ขั้นตอน (docs/05 §3–§5):
///   1. `metadata()` → mtime + size
///   2. ถาม `paths` — ถ้า mtime+size ตรง ข้าม hash ไปได้เลย
///      (hash ไฟล์ 4000px ใช้เวลาพอ ๆ กับอ่านมันทั้งไฟล์)
///   3. ยังไม่มี → `hash_file()` แล้วบันทึกลง `paths`
///   4. ถาม `thumbs` ด้วยคีย์ (hash, mtime, size)
///
/// **รันบน worker thread** จึงรอคำตอบจาก IO thread ได้ (I-2 คุมแค่ UI thread)
///
/// รับ `path` ไม่ใช่ทั้ง `Job` เพราะ **มีแต่งานที่มีไฟล์จริงเท่านั้นที่เข้า cache ได้** —
/// clipboard ไม่มี `mtime` ให้ประกอบคีย์ (ดู [`JobSource::Clipboard`]) การให้ชนิดข้อมูล
/// บังคับไว้ตรงนี้ทำให้ลืมไม่ได้
fn cache_lookup(
    path: &std::path::Path,
    io: Option<&crossbeam_channel::Sender<IoRequest>>,
) -> CacheLookup {
    let Some(io) = io else {
        return CacheLookup::Unavailable;
    };
    let Ok(meta) = std::fs::metadata(path) else {
        return CacheLookup::Unavailable; // ปล่อยให้ขั้นอ่านไฟล์รายงาน error ที่ชัดกว่า
    };

    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0);
    let fingerprint = PathFingerprint {
        mtime,
        size: meta.len(),
    };

    // 2. path → hash (ข้ามการ hash ถ้าไฟล์ไม่ถูกแก้)
    let (reply, rx) = crossbeam_channel::bounded(1);
    let known = io
        .send(IoRequest::LookupPath {
            path: path.to_path_buf(),
            fingerprint,
            reply,
        })
        .ok()
        .and_then(|()| rx.recv_timeout(IO_TIMEOUT).ok())
        .flatten();

    // 3. ยังไม่รู้จัก → hash จริงแล้วจำไว้
    let hash = match known {
        Some(hash) => hash,
        None => {
            let Ok(hash) = hash_file(path) else {
                return CacheLookup::Unavailable;
            };
            let _ = io.send(IoRequest::RecordPath {
                path: path.to_path_buf(),
                hash,
                fingerprint,
            });
            hash
        }
    };

    let key = CacheKey::new(hash, fingerprint);

    // 4. ถาม thumbnail
    let (reply, rx) = crossbeam_channel::bounded(1);
    let hit = io
        .send(IoRequest::GetThumb { key, reply })
        .ok()
        .and_then(|()| rx.recv_timeout(IO_TIMEOUT).ok())
        .flatten();

    match hit {
        Some(entry) if entry.thumb.len() == EXPECTED_THUMB_BYTES => {
            CacheLookup::Hit(Box::new(Thumbnail {
                pixels: entry.thumb,
                source_width: entry.width,
                source_height: entry.height,
                dominant: entry.dominant,
            }))
        }
        // แถวขนาดผิด = cache เพี้ยน ถือว่า miss แล้ว decode ใหม่ทับ
        _ => CacheLookup::Miss(key),
    }
}

/// สะดวกสำหรับผู้เรียก: โหลดไฟล์เดียวแบบ synchronous (ใช้ในเทสต์/เครื่องมือ)
///
/// **ห้ามเรียกจาก UI thread** (I-2)
///
/// # Errors
/// คืน [`LoadError`] เมื่อไฟล์ไม่ผ่านเกราะ
pub fn load_single(path: &std::path::Path, limits: &Limits) -> Result<RgbaImage, LoadError> {
    load_guarded(path, limits)
}

#[cfg(test)]
mod tests {
    // เทสต์ต้อง panic! ได้เมื่อผลไม่ตรงชนิดที่คาด — การยืนยัน "ล้มเหลวด้วยเหตุผลที่ถูก" สำคัญ
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::io::Cursor;

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("refx-pool-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_png(tag: &str, name: &str, w: u32, h: u32) -> PathBuf {
        let path = temp_dir(tag).join(name);
        let img = RgbaImage::from_pixel(w, h, image::Rgba([10, 200, 30, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&path, &out).unwrap();
        path
    }

    fn job(path: PathBuf, priority: f32, seed: &[u8]) -> Job {
        Job {
            hash: crate::hash::hash_bytes(seed),
            source: JobSource::File(path),
            priority,
            cancel: Arc::new(AtomicBool::new(false)),
            target: JobTarget::Thumbnail,
        }
    }

    fn test_pool(workers: usize) -> DecodePool {
        DecodePool::new(
            workers,
            Arc::new(RamBudget::new(64 << 20)),
            Limits::default(),
            None, // ไม่มี cache ในเทสต์ — วัดเส้นทาง decode ล้วน
            None, // ไม่มี clipboard — เทสต์ที่ต้องการจะเสียบ FakeClipboard เอง
            None, // ไม่มี spool — เทสต์ที่ต้องการจะเสียบ FakeSpool เอง
        )
    }

    /// clipboard ปลอมที่ **สั่งได้ว่าจะให้ตอบอะไร**
    ///
    /// ★ ทำไมต้องมี: ก่อนแยกชั้น เทสต์ clipboard อ่านของจริงบนเครื่องที่รัน
    /// ผลจึงขึ้นกับว่าใครก๊อปอะไรค้างไว้ — เทสต์ที่ผ่านเพราะ "clipboard ว่าง"
    /// ไม่ได้พิสูจน์ว่าเส้นทางภาพหรือเส้นทางไฟล์ทำงาน (docs/08 §3.9 ข้อ 1)
    /// ตอนนี้ทั้งสามสาขาถูกบังคับให้เดินจริงทุกครั้งที่รันเทสต์
    ///
    /// ของจริงยังถูกตรวจอยู่ที่ `refx-platform::clipboard` ซึ่งเป็นที่ที่มันอยู่
    #[derive(Debug)]
    struct FakeClipboard(std::sync::Mutex<Option<Result<ClipboardContent, ClipboardError>>>);

    impl FakeClipboard {
        fn answering(answer: Result<ClipboardContent, ClipboardError>) -> ClipboardHandle {
            Arc::new(Self(std::sync::Mutex::new(Some(answer))))
        }

        fn with_image(w: u32, h: u32) -> ClipboardHandle {
            Self::answering(Ok(ClipboardContent::Image(image_of(w, h))))
        }
    }

    /// ภาพดิบใน clipboard — ลายที่ต่างกันตามขนาด (คนละภาพต้องได้คนละคีย์)
    fn image_of(w: u32, h: u32) -> refx_core::clipboard::ClipboardImage {
        refx_core::clipboard::ClipboardImage {
            width: w,
            height: h,
            rgba: (0..(w as usize) * (h as usize) * 4)
                .map(|i| (i as u32).wrapping_mul(2_654_435_761).to_le_bytes()[0])
                .collect(),
        }
    }

    /// clipboard ที่ตอบ **ภาพเดิมได้ไม่จำกัดครั้ง** — สำหรับเทสต์ที่วางซ้ำ
    #[derive(Debug)]
    struct RepeatingClipboard(refx_core::clipboard::ClipboardImage);

    impl RepeatingClipboard {
        fn with_image(w: u32, h: u32) -> ClipboardHandle {
            Arc::new(Self(image_of(w, h)))
        }
    }

    impl ClipboardReader for RepeatingClipboard {
        fn read(&self) -> Result<ClipboardContent, ClipboardError> {
            Ok(ClipboardContent::Image(self.0.clone()))
        }
    }

    impl ClipboardReader for FakeClipboard {
        fn read(&self) -> Result<ClipboardContent, ClipboardError> {
            match self.0.lock() {
                // อ่านได้ครั้งเดียว — เทสต์ที่เผลอส่งงาน clipboard สองใบจะได้
                // คำตอบที่ต่างกัน แทนที่จะผ่านไปเงียบ ๆ
                Ok(mut slot) => slot.take().unwrap_or(Err(ClipboardError::NoImage)),
                Err(_) => Err(ClipboardError::Busy),
            }
        }
    }

    fn clipboard_pool(reader: ClipboardHandle) -> DecodePool {
        DecodePool::new(
            2,
            Arc::new(RamBudget::new(64 << 20)),
            Limits::default(),
            None,
            Some(reader),
            None,
        )
    }

    #[test]
    fn worker_count_is_in_range() {
        let n = default_worker_count();
        assert!((2..=6).contains(&n), "ได้ {n} worker");
    }

    #[test]
    fn decodes_a_real_file() {
        let path = write_png("ok", "a.png", 32, 24);
        let pool = test_pool(2);
        pool.submit(job(path, 0.0, b"a"));

        let result = pool
            .results()
            .recv_timeout(Duration::from_secs(10))
            .expect("ต้องได้ผลกลับมา");
        match result {
            JobResult::Done { thumb, .. } => {
                assert_eq!((thumb.source_width, thumb.source_height), (32, 24));
                assert_eq!(thumb.pixels.len(), 128 * 128 * 4);
            }
            other => panic!("ต้องสำเร็จ แต่ได้ {other:?}"),
        }
        assert_eq!(pool.stats().completed, 1);
    }

    /// ★ ไฟล์เสียต้องได้ `Failed` ไม่ใช่ล้ม pool (I-7)
    #[test]
    fn broken_file_fails_without_killing_pool() {
        let dir = temp_dir("broken");
        let bad = dir.join("bad.png");
        // ลายเซ็น PNG ถูกต้อง แต่เนื้อในเป็นขยะ — ผ่าน guess_format แล้วพังตอนอ่าน header
        let mut broken = b"\x89PNG\r\n\x1a\n".to_vec();
        broken.extend_from_slice("ขยะ".as_bytes());
        std::fs::write(&bad, &broken).unwrap();
        let good = write_png("broken", "good.png", 16, 16);

        let pool = test_pool(2);
        pool.submit(job(bad, 0.0, b"bad"));
        pool.submit(job(good, 1.0, b"good"));

        let mut failed = 0;
        let mut done = 0;
        for _ in 0..2 {
            match pool
                .results()
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
            {
                JobResult::Failed { .. } => failed += 1,
                JobResult::Done { .. } => done += 1,
                JobResult::Working { .. }
                | JobResult::Sampled { .. }
                | JobResult::Cancelled { .. }
                | JobResult::ClipboardFiles { .. }
                | JobResult::Spooled { .. } => {}
            }
        }
        assert_eq!((failed, done), (1, 1), "ไฟล์เสียต้องไม่ลากไฟล์ดีลงไปด้วย");
    }

    // ---------- color picker (P2-10) ----------

    /// ภาพที่แต่ละ pixel มีสีไม่ซ้ำใคร — สีบอกได้ทันทีว่าอ่านมาจากช่องไหน
    fn write_gradient_png(tag: &str, name: &str, w: u32, h: u32) -> PathBuf {
        let path = temp_dir(tag).join(name);
        let mut img = RgbaImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            #[expect(clippy::cast_possible_truncation, reason = "ภาพเทสต์เล็กกว่า 256")]
            let (x8, y8) = (x as u8, y as u8);
            *px = image::Rgba([x8, y8, 7, 255]);
        }
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&path, &out).unwrap();
        path
    }

    #[test]
    fn sampling_reads_the_pixel_the_fraction_points_at() {
        let mut img = RgbaImage::new(4, 2);
        for (x, y, px) in img.enumerate_pixels_mut() {
            #[expect(clippy::cast_possible_truncation, reason = "ภาพ 4x2")]
            let (x8, y8) = (x as u8, y as u8);
            *px = image::Rgba([x8, y8, 0, 255]);
        }
        assert_eq!(sample_pixel(&img, 0.0, 0.0).0, (0, 0));
        assert_eq!(sample_pixel(&img, 0.99, 0.99).0, (3, 1));
        // ★ ขอบขวาสุดพอดีต้องเป็น pixel สุดท้าย ไม่ใช่ตัวที่อยู่นอกภาพ
        assert_eq!(sample_pixel(&img, 1.0, 1.0).0, (3, 1));
        assert_eq!(sample_pixel(&img, 1.0, 1.0).1, [3, 1, 0, 255]);
        // แต่ละ pixel กินช่วงเท่ากัน: 1/4 ของความกว้างคือ pixel ที่ 1
        assert_eq!(sample_pixel(&img, 0.25, 0.0).0, (1, 0));
        assert_eq!(sample_pixel(&img, 0.5, 0.0).0, (2, 0));
    }

    /// I-4: ค่าที่พังต้องไม่ทำให้ index หลุดขอบ (จะ panic ใน `get_pixel`)
    #[test]
    fn sampling_survives_broken_fractions() {
        let img = RgbaImage::from_pixel(3, 3, image::Rgba([1, 2, 3, 4]));
        for (u, v) in [
            (f32::NAN, 0.5),
            (0.5, f32::INFINITY),
            (-10.0, 0.5),
            (99.0, 99.0),
        ] {
            let ((x, y), rgba) = sample_pixel(&img, u, v);
            assert!(x < 3 && y < 3, "หลุดขอบที่ ({u}, {v}) → ({x}, {y})");
            assert_eq!(rgba, [1, 2, 3, 4]);
        }
    }

    /// ★★ งาน `Sample` ต้องได้สีของ **ต้นฉบับ** ไม่ใช่ค่าเฉลี่ยจาก thumbnail
    ///
    /// เทสต์นี้บังคับความต่างให้เห็นเป็นตัวเลข: ภาพ 256×256 ที่ไล่สีทุก pixel
    /// ถ้าใครเผลอไปอ่านจาก thumbnail 128×128 (ซึ่งบีบเป็นจัตุรัสและเฉลี่ยมาแล้ว)
    /// ค่าที่ได้จะเพี้ยนจากค่าที่ถูกต้องทันที
    #[test]
    fn a_sample_job_returns_the_true_source_pixel() {
        let path = write_gradient_png("sample", "grad.png", 256, 256);
        let pool = test_pool(1);
        pool.submit(Job {
            hash: crate::hash::hash_bytes(b"sample"),
            source: JobSource::File(path),
            priority: 0.0,
            cancel: Arc::new(AtomicBool::new(false)),
            // กลางภาพพอดี → pixel (128, 64)
            target: JobTarget::Sample { u: 0.5, v: 0.25 },
        });

        let result = pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .unwrap();
        let JobResult::Sampled {
            rgba, source_px, ..
        } = result
        else {
            panic!("ได้ {result:?} ซึ่งไม่ใช่ Sampled");
        };
        assert_eq!(source_px, (128, 64));
        assert_eq!(rgba, [128, 64, 7, 255], "ต้องเป็นสีของ pixel ต้นฉบับเป๊ะ");
    }

    /// งาน picker ต้องไม่แตะ cache — cache เก็บแต่ thumbnail ที่ถูกบีบแล้ว
    #[test]
    fn a_sample_job_never_answers_from_the_thumbnail_cache() {
        let path = write_gradient_png("samplecache", "grad.png", 64, 64);
        let pool = test_pool(1);
        // ส่ง thumbnail ก่อนเพื่อให้ cache (ถ้ามี) อุ่น แล้วค่อยขอ sample ด้วย hash เดียวกัน
        pool.submit(job(path.clone(), 0.0, b"same"));
        let first = pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .unwrap();
        assert!(matches!(first, JobResult::Done { .. }), "ได้ {first:?}");

        pool.submit(Job {
            hash: crate::hash::hash_bytes(b"same"),
            source: JobSource::File(path),
            priority: 0.0,
            cancel: Arc::new(AtomicBool::new(false)),
            target: JobTarget::Sample { u: 0.0, v: 0.0 },
        });
        let second = pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .unwrap();
        let JobResult::Sampled { rgba, .. } = second else {
            panic!("cache ตอบแทน picker: ได้ {second:?}");
        };
        assert_eq!(rgba, [0, 0, 7, 255]);
    }

    #[test]
    fn missing_file_fails_gracefully() {
        let pool = test_pool(2);
        pool.submit(job(temp_dir("gone").join("ไม่มีจริง.png"), 0.0, b"gone"));
        let result = pool
            .results()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        assert!(matches!(result, JobResult::Failed { .. }));
    }

    /// ★ cancellation: ตั้งธงก่อนส่ง → ต้องไม่เสียแรง decode เลย
    #[test]
    fn cancelled_before_start_is_skipped() {
        let path = write_png("cancel", "c.png", 64, 64);
        let pool = test_pool(1);

        let j = job(path, 0.0, b"c");
        j.cancel.store(true, AtomicOrdering::Relaxed);
        pool.submit(j);

        let result = pool
            .results()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        assert!(
            matches!(result, JobResult::Cancelled { .. }),
            "ได้ {result:?}"
        );
        assert_eq!(pool.stats().cancelled, 1);
        assert_eq!(pool.stats().completed, 0, "ต้องไม่ decode เลย");
    }

    /// ★★★ ผลของงานที่ถูกยกเลิก/ล้มเหลว **ต้องบอกได้ว่ามันเป็นงานชนิดไหน**
    ///
    /// ชั้น UI นับ "งวดที่ผู้ใช้ลากเข้ามา" ให้จบไม่ได้เลยถ้าแยกไม่ออกว่าใบที่หายไป
    /// เป็นภาพที่เขากำลังรอ (`Thumbnail`) หรือเป็นภาพคมกว่าเดิม (`Working`)
    /// ที่หายแล้วไม่มีใครเดือดร้อน:
    ///
    /// * นับทุกใบ → งาน working texture ปนเข้ามา งวดจบเร็วเกินจริง ตัวเลขผิด
    /// * ไม่นับเลย → **ผู้ใช้ pan ระหว่างลากไฟล์แล้วงวดค้างถาวร** แถบ "กำลังโหลด"
    ///   ไม่หาย และข้อความสรุปตอนจบ (รวมถึง "board เต็ม") ไม่มีวันขึ้น
    ///
    /// เดิม `Cancelled`/`Failed` มีแต่ `hash` ซึ่งทำให้ **เขียนโค้ดที่ถูกไม่ได้เลย**
    #[test]
    fn a_cancelled_job_says_what_kind_of_job_it_was() {
        let path = write_png("kind", "k.png", 64, 64);
        let pool = test_pool(1);

        for target in [JobTarget::Thumbnail, JobTarget::Working { size: 512 }] {
            let mut j = job(path.clone(), 0.0, b"k");
            j.target = target;
            j.cancel.store(true, AtomicOrdering::Relaxed);
            pool.submit(j);

            let result = pool
                .results()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            match result {
                JobResult::Cancelled { target: got, .. } => {
                    assert_eq!(got, target, "ผลของงานที่ถูกยกเลิกบอกชนิดผิด — ชั้น UI จะนับงวดผิดตาม")
                }
                other => panic!("ต้องได้ Cancelled แต่ได้ {other:?}"),
            }
        }
    }

    /// ผลของงานที่ **ล้มเหลว** ก็ต้องบอกชนิดเหมือนกัน ด้วยเหตุผลเดียวกันเป๊ะ
    #[test]
    fn a_failed_job_says_what_kind_of_job_it_was() {
        let missing = temp_dir("failkind").join("does-not-exist.png");
        let pool = test_pool(1);

        let mut j = job(missing, 0.0, b"missing");
        j.target = JobTarget::Working { size: 256 };
        pool.submit(j);

        let result = pool
            .results()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        match result {
            JobResult::Failed { target, .. } => {
                assert_eq!(target, JobTarget::Working { size: 256 });
            }
            other => panic!("ต้องได้ Failed แต่ได้ {other:?}"),
        }
    }

    /// ★ จำลอง "pan เร็วผ่าน 500 ภาพ" — งานส่วนใหญ่ต้องถูกยกเลิกจริง
    ///
    /// ตัวเลข `cancelled` คือหลักฐานว่า cancellation ทำงาน ไม่ใช่แค่มีโค้ดอยู่
    #[test]
    fn bulk_cancellation_actually_skips_work() {
        let path = write_png("bulk", "b.png", 96, 96);
        let pool = test_pool(2);

        let flags: Vec<Arc<AtomicBool>> =
            (0..500).map(|_| Arc::new(AtomicBool::new(false))).collect();

        // ยกเลิก 490 ตัวแรกทันที (ผู้ใช้ pan ผ่านไปแล้ว) เหลือ 10 ตัวท้ายที่ยังมองอยู่
        for flag in flags.iter().take(490) {
            flag.store(true, AtomicOrdering::Relaxed);
        }

        for (i, flag) in flags.iter().enumerate() {
            pool.submit(Job {
                hash: crate::hash::hash_bytes(&(i as u32).to_le_bytes()),
                source: JobSource::File(path.clone()),
                priority: i as f32,
                cancel: Arc::clone(flag),
                target: JobTarget::Thumbnail,
            });
        }

        for _ in 0..500 {
            pool.results()
                .recv_timeout(Duration::from_secs(30))
                .unwrap();
        }

        let stats = pool.stats();
        assert_eq!(stats.finished(), 500);
        assert!(
            stats.cancelled >= 490,
            "ต้องยกเลิกอย่างน้อย 490 งาน แต่ยกเลิกแค่ {} (decode ไป {})",
            stats.cancelled,
            stats.completed
        );
    }

    /// ★ priority: งานที่ priority น้อยต้องออกก่อน
    ///
    /// ใช้ worker เดียวเพื่อให้ลำดับผลลัพธ์เป็นลำดับของคิวจริง ๆ
    #[test]
    fn lower_priority_value_runs_first() {
        let path = write_png("prio", "p.png", 16, 16);
        let pool = test_pool(1);

        // ส่งแบบสลับลำดับ: 30, 10, 20
        for (priority, seed) in [(30.0f32, 30u32), (10.0, 10), (20.0, 20)] {
            pool.submit(Job {
                hash: crate::hash::hash_bytes(&seed.to_le_bytes()),
                source: JobSource::File(path.clone()),
                priority,
                cancel: Arc::new(AtomicBool::new(false)),
                target: JobTarget::Thumbnail,
            });
        }

        // ★ ยืนยันแค่ว่า **ทุกใบได้ทำจนครบ** ไม่ยืนยันลำดับตรงนี้
        //
        //   ลำดับผลลัพธ์ที่ปลายทางขึ้นกับ *จังหวะ* ว่า worker หยิบงานไปกี่ใบก่อนที่
        //   ใบถัดไปจะเข้าคิวทัน — บนเครื่องพัฒนามันหยิบไม่ทันจึงดูเหมือนเรียงถูก
        //   แต่บน CI (2 core) worker ทำใบแรกจบก่อนใบที่สามเข้าคิวด้วยซ้ำ
        //   เทสต์เดิมจึงล้ม **ทั้งที่คิวทำงานถูกต้องเป๊ะ** (เจอจริง 1 ส.ค. 2026)
        //
        //   ตัวคิวเองถูกทดสอบแบบ deterministic ที่ `queue_pops_in_priority_order`
        let mut done = Vec::new();
        for _ in 0..3 {
            done.push(
                pool.results()
                    .recv_timeout(Duration::from_secs(30))
                    .unwrap()
                    .hash(),
            );
        }
        done.sort_unstable_by_key(|hash| *hash.as_bytes());

        let mut expected: Vec<_> = [10u32, 20, 30]
            .iter()
            .map(|v| crate::hash::hash_bytes(&v.to_le_bytes()))
            .collect();
        expected.sort_unstable_by_key(|hash| *hash.as_bytes());
        assert_eq!(done, expected, "ต้องได้ผลกลับมาครบทุกใบ");
    }

    /// ★ คิวเรียงตาม priority — ทดสอบ **ที่ตัวคิวโดยตรง ไม่ผ่าน worker**
    ///
    /// นี่คือกลไกจริงที่ docs/05 §3 สั่งไว้ว่า "ภาพที่ผู้ใช้กำลังมองมาก่อนเสมอ"
    /// ทดสอบตรงนี้ได้ผลคงที่ 100% เพราะไม่มีเธรดมาเกี่ยว — ต่างจากการดูลำดับ
    /// ผลลัพธ์ที่ปลายทางซึ่งขึ้นกับความเร็วเครื่อง
    #[test]
    fn queue_pops_in_priority_order() {
        let queue = Queue::new();
        let hash_of = |seed: u32| crate::hash::hash_bytes(&seed.to_le_bytes());

        // ใส่สลับลำดับ แล้วต้องออกมาเรียงจากน้อยไปมาก (น้อย = ใกล้กึ่งกลางจอ = ทำก่อน)
        for (priority, seed) in [(30.0f32, 30u32), (10.0, 10), (20.0, 20)] {
            queue.push(Job {
                hash: hash_of(seed),
                source: JobSource::File(PathBuf::from("x.png")),
                priority,
                cancel: Arc::new(AtomicBool::new(false)),
                target: JobTarget::Thumbnail,
            });
        }

        let order: Vec<ContentHash> = (0..3).map(|_| queue.pop().unwrap().hash).collect();
        assert_eq!(order, vec![hash_of(10), hash_of(20), hash_of(30)]);
    }

    /// priority เท่ากันต้องออกตามลำดับที่ส่งเข้ามา (FIFO)
    ///
    /// `BinaryHeap` ไม่รับประกันลำดับของค่าที่เท่ากัน ถ้าไม่มีตัวตัดสิน `seq`
    /// ผลจะสลับไปมาระหว่างการรัน = ไม่ deterministic (CLAUDE.md ห้ามไว้)
    #[test]
    fn equal_priorities_keep_submission_order() {
        let queue = Queue::new();
        let hash_of = |seed: u32| crate::hash::hash_bytes(&seed.to_le_bytes());
        for seed in [1u32, 2, 3, 4, 5] {
            queue.push(Job {
                hash: hash_of(seed),
                source: JobSource::File(PathBuf::from("x.png")),
                priority: 7.0, // เท่ากันหมด
                cancel: Arc::new(AtomicBool::new(false)),
                target: JobTarget::Thumbnail,
            });
        }
        let order: Vec<ContentHash> = (0..5).map(|_| queue.pop().unwrap().hash).collect();
        let expected: Vec<ContentHash> = [1u32, 2, 3, 4, 5].iter().map(|s| hash_of(*s)).collect();
        assert_eq!(order, expected, "priority เท่ากันต้อง FIFO ไม่ใช่สุ่ม");
    }

    #[test]
    fn priority_key_orders_like_float() {
        assert!(priority_key(1.0) < priority_key(2.0));
        assert!(priority_key(0.5) < priority_key(1.0));
        assert_eq!(priority_key(0.0), 0);
        // ค่าที่ไม่ควรเกิด ต้องไม่ทำให้คิวพัง
        assert_eq!(priority_key(f32::NAN), 0);
        assert_eq!(priority_key(-5.0), 0);
    }

    /// ★ เพดาน RAM คุมรวมทุก worker — ไม่ใช่ต่อ job
    #[test]
    fn ram_stays_under_shared_limit() {
        let path = write_png("ram", "r.png", 512, 512); // ~2 MB หลัง decode (×2 = 4 MB)
        let budget = Arc::new(RamBudget::new(8 << 20)); // 8 MB — พอแค่ ~2 งานพร้อมกัน
        let pool = DecodePool::new(6, Arc::clone(&budget), Limits::default(), None, None, None);

        for i in 0..40u32 {
            pool.submit(Job {
                hash: crate::hash::hash_bytes(&i.to_le_bytes()),
                source: JobSource::File(path.clone()),
                priority: i as f32,
                cancel: Arc::new(AtomicBool::new(false)),
                target: JobTarget::Thumbnail,
            });
        }
        for _ in 0..40 {
            pool.results()
                .recv_timeout(Duration::from_secs(30))
                .unwrap();
        }

        assert_eq!(budget.used(), 0, "คืนโควตาครบทุกงาน");
        assert_eq!(pool.stats().completed, 40);
    }

    #[test]
    fn pool_shuts_down_cleanly_with_queued_work() {
        let path = write_png("shutdown", "s.png", 32, 32);
        let pool = test_pool(2);
        for i in 0..50u32 {
            pool.submit(Job {
                hash: crate::hash::hash_bytes(&i.to_le_bytes()),
                source: JobSource::File(path.clone()),
                priority: i as f32,
                cancel: Arc::new(AtomicBool::new(false)),
                target: JobTarget::Thumbnail,
            });
        }
        // drop ทั้งที่ยังมีงานค้าง — ต้องไม่ค้าง (คิวถูกล้างแล้ว join)
        drop(pool);
    }

    #[test]
    fn stats_start_at_zero() {
        let pool = test_pool(2);
        let stats = pool.stats();
        assert_eq!(stats, PoolStatsSnapshot::default());
        assert_eq!(pool.queued(), 0);
        let (used, limit) = pool.ram_usage();
        assert_eq!(used, 0);
        assert_eq!(limit, 64 << 20);
    }

    // ---------- clipboard (P1-8) ----------

    fn clipboard_job(seed: &[u8]) -> Job {
        Job {
            hash: crate::hash::hash_bytes(seed),
            source: JobSource::Clipboard,
            priority: 0.0,
            cancel: Arc::new(AtomicBool::new(false)),
            target: JobTarget::Thumbnail,
        }
    }

    /// ★ ภาพดิบใน clipboard ต้องออกมาเป็น thumbnail ที่ใช้ได้จริง
    ///
    /// เดิมเทสต์นี้อ่าน clipboard ของเครื่องที่รัน ผลจึงขึ้นกับว่าใครก๊อปอะไรค้างไว้
    /// — บน CI ที่ไม่มี display มันเดินสาขา `Failed` ทุกครั้ง แปลว่า **เส้นทางภาพ
    /// ไม่เคยถูกตรวจบน CI เลย** ตอนนี้บังคับให้เดินสาขาภาพเสมอ
    #[test]
    fn a_raw_image_from_the_clipboard_becomes_a_thumbnail() {
        let pool = clipboard_pool(FakeClipboard::with_image(24, 16));
        pool.submit(clipboard_job(b"clip-image"));

        match pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .expect("ต้องได้ผลกลับมา")
        {
            JobResult::Done { thumb, .. } => {
                assert_eq!((thumb.source_width, thumb.source_height), (24, 16));
                assert_eq!(thumb.pixels.len(), EXPECTED_THUMB_BYTES);
            }
            other => panic!("ต้องได้ thumbnail แต่ได้ {other:?}"),
        }
        assert_eq!(pool.ram_usage().0, 0, "คืนโควตา RAM ครบ");
    }

    /// ก๊อปไฟล์จาก Explorer แล้ววาง → ต้องได้ **รายชื่อไฟล์** กลับไปให้ UI
    /// ยัดเข้าเส้นทาง drag & drop ไม่ใช่พยายาม decode เอง
    #[test]
    fn a_file_list_from_the_clipboard_is_handed_back_to_the_ui() {
        let files = vec![PathBuf::from("a.png"), PathBuf::from("b.jpg")];
        let pool = clipboard_pool(FakeClipboard::answering(Ok(ClipboardContent::Files(
            files.clone(),
        ))));
        pool.submit(clipboard_job(b"clip-files"));

        match pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .expect("ต้องได้ผลกลับมา")
        {
            JobResult::ClipboardFiles { paths, .. } => assert_eq!(paths, files),
            other => panic!("ต้องได้รายชื่อไฟล์ แต่ได้ {other:?}"),
        }
    }

    /// ★ clipboard เปิดไม่ได้ต้องไม่ล้ม pool และงานอื่นต้องเดินต่อได้ (I-7)
    #[test]
    fn a_broken_clipboard_fails_that_job_only() {
        let good = write_png("clip", "good.png", 24, 16);
        let pool = clipboard_pool(FakeClipboard::answering(Err(ClipboardError::Busy)));
        pool.submit(clipboard_job(b"clip-busy"));
        pool.submit(job(good, 1.0, b"good"));

        let mut file_done = false;
        let mut clipboard_failed = false;
        for _ in 0..2 {
            let result = pool
                .results()
                .recv_timeout(Duration::from_secs(30))
                .expect("ทุกงานต้องได้คำตอบกลับมา");
            if result.hash() == crate::hash::hash_bytes(b"clip-busy") {
                match result {
                    JobResult::Failed { reason, .. } => {
                        assert!(matches!(
                            reason,
                            JobFailure::Clipboard(ClipboardError::Busy)
                        ));
                        assert!(!reason.to_string().trim().is_empty(), "error ไม่มีข้อความ");
                        clipboard_failed = true;
                    }
                    other => panic!("clipboard พังต้องได้ Failed แต่ได้ {other:?}"),
                }
            } else {
                file_done = matches!(result, JobResult::Done { .. });
            }
        }
        assert!(clipboard_failed, "งาน clipboard เงียบหาย");
        assert!(file_done, "ไฟล์ปกติต้องยังเปิดได้แม้ clipboard จะพัง");
        assert_eq!(pool.ram_usage().0, 0, "คืนโควตา RAM ครบ");
    }

    /// ★ pool ที่ไม่มีตัวอ่านเสียบไว้ (fuzz / เทสต์) ต้อง **ตอบว่าใช้ไม่ได้**
    /// ไม่ใช่เงียบหายหรือ panic — ถ้าเงียบ ผู้ใช้จะเห็นแค่ "กด Ctrl+V แล้วไม่มี
    /// อะไรเกิดขึ้น" ซึ่งแยกไม่ออกจากบั๊ก (docs/08 §3.9 ข้อ 2)
    #[test]
    fn a_pool_without_a_clipboard_reader_says_so_instead_of_going_quiet() {
        let pool = test_pool(1);
        pool.submit(clipboard_job(b"clip-unwired"));

        match pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .expect("ต้องได้ผลกลับมา ไม่ใช่เงียบหาย")
        {
            JobResult::Failed { reason, .. } => {
                assert!(matches!(
                    reason,
                    JobFailure::Clipboard(ClipboardError::Unavailable { .. })
                ));
            }
            other => panic!("ต้องได้ Failed แต่ได้ {other:?}"),
        }
    }

    /// ★ ภาพจาก clipboard ต้องผ่าน **เกราะชุดเดียวกับไฟล์บนดิสก์**
    ///
    /// ข้อนี้คือเหตุผลที่ `refx-platform::clipboard` ไม่ตรวจอะไรเลยโดยตั้งใจ —
    /// ถ้าเกราะมีสองชุด วันหนึ่งจะมีคนแก้ข้างเดียวแล้วเพี้ยนจากกัน
    #[test]
    fn a_lying_clipboard_image_is_rejected_by_the_same_guard_as_files() {
        // ประกาศ 1000×1000 แต่ส่ง byte มาแค่หยิบมือ
        let pool = clipboard_pool(FakeClipboard::answering(Ok(ClipboardContent::Image(
            refx_core::clipboard::ClipboardImage {
                width: 1000,
                height: 1000,
                rgba: vec![0; 16],
            },
        ))));
        pool.submit(clipboard_job(b"clip-lie"));

        match pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .expect("ต้องได้ผลกลับมา")
        {
            JobResult::Failed { reason, .. } => {
                assert!(
                    matches!(reason, JobFailure::Load(_)),
                    "ต้องโดนเกราะของ decode ปฏิเสธ ไม่ใช่ error คนละชุด: {reason}"
                );
            }
            other => panic!("ภาพที่โกงขนาดต้องถูกปฏิเสธ แต่ได้ {other:?}"),
        }
        assert_eq!(pool.ram_usage().0, 0, "คืนโควตา RAM ครบ");
    }

    /// ★ ข้อผูกมัด §4 ข้อ 4: cache key คือ `(hash, mtime, size)`
    ///
    /// clipboard ไม่มี mtime จึง **ห้ามแตะ cache เลย** — ไม่ใช่ "แตะแล้วพลาด"
    /// เทสต์นี้ดักที่ช่องคุยกับ IO thread โดยตรง: งาน clipboard ต้องไม่ส่งคำขอ
    /// สักใบ ส่วนงานของไฟล์ต้องส่ง เพื่อพิสูจน์ว่าช่องนี้ทำงานอยู่จริง
    #[test]
    fn clipboard_never_touches_the_cache() {
        let (io_tx, io_rx) = crossbeam_channel::unbounded::<IoRequest>();
        let pool = DecodePool::new(
            1,
            Arc::new(RamBudget::new(64 << 20)),
            Limits::default(),
            Some(io_tx),
            Some(FakeClipboard::with_image(8, 8)),
            None,
        );

        pool.submit(clipboard_job(b"no-cache"));
        pool.results()
            .recv_timeout(Duration::from_secs(30))
            .expect("ต้องได้ผลกลับมา");
        assert!(
            io_rx.try_recv().is_err(),
            "งาน clipboard ส่งคำขอไปหา cache ซึ่งไม่มี mtime ให้ประกอบคีย์"
        );

        // ช่องเดียวกันนี้ต้องมีของจริงไหลผ่านเมื่อเป็นไฟล์ — ไม่งั้นเทสต์ข้างบนว่างเปล่า
        //
        // ตอบกลับให้ด้วย (เป็น "ไม่มีใน cache" ทุกใบ) ไม่งั้น worker จะรอจนครบ
        // `IO_TIMEOUT` สองรอบ = เทสต์ช้าไป 10 วินาทีโดยไม่ได้ตรวจอะไรเพิ่ม
        let asked = Arc::new(AtomicU64::new(0));
        let responder = {
            let asked = Arc::clone(&asked);
            std::thread::spawn(move || {
                for request in io_rx {
                    asked.fetch_add(1, AtomicOrdering::Relaxed);
                    match request {
                        IoRequest::LookupPath { reply, .. } => {
                            let _ = reply.send(None);
                        }
                        IoRequest::GetThumb { reply, .. } => {
                            let _ = reply.send(None);
                        }
                        _ => {}
                    }
                }
            })
        };

        let path = write_png("cache-io", "c.png", 16, 16);
        pool.submit(job(path, 0.0, b"cache-io"));
        pool.results()
            .recv_timeout(Duration::from_secs(30))
            .expect("ต้องได้ผลกลับมา");
        drop(pool); // ปิด sender ฝั่ง worker ให้ responder จบลูปได้
        responder.join().expect("responder ต้องจบปกติ");
        assert!(
            asked.load(AtomicOrdering::Relaxed) > 0,
            "งานของไฟล์ต้องคุยกับ cache — ถ้าไม่คุยเลย เทสต์ข้างบนก็ไม่ได้พิสูจน์อะไร"
        );
    }

    // ---------- ★★★ spool ของภาพที่วาง (P4-5) ----------

    /// ที่พักปลอม — จำทุกอย่างที่ถูกเก็บ และ **หน่วงได้ตามสั่ง**
    ///
    /// ★ การหน่วงคือหัวใจ: มันทำให้เทสต์ถามได้ว่า *"ภาพขึ้นจอก่อนไฟล์ลงดิสก์
    /// จริงไหม"* ซึ่งเป็นข้อบังคับของ `docs/07 §2` ที่ไม่มีเทสต์แบบอื่นถามได้
    #[derive(Debug)]
    struct FakeSpool {
        dir: PathBuf,
        stored: std::sync::Mutex<Vec<(ContentHash, usize)>>,
        /// เปิดประตูให้ `store` เดินต่อ — `None` = ไม่หน่วง
        gate: Option<crossbeam_channel::Receiver<()>>,
    }

    impl FakeSpool {
        fn new(tag: &str) -> Arc<Self> {
            Arc::new(Self {
                dir: temp_dir(tag),
                stored: std::sync::Mutex::new(Vec::new()),
                gate: None,
            })
        }

        fn gated(tag: &str) -> (Arc<Self>, crossbeam_channel::Sender<()>) {
            let (tx, rx) = crossbeam_channel::bounded(1);
            (
                Arc::new(Self {
                    dir: temp_dir(tag),
                    stored: std::sync::Mutex::new(Vec::new()),
                    gate: Some(rx),
                }),
                tx,
            )
        }

        fn stored(&self) -> Vec<(ContentHash, usize)> {
            self.stored.lock().map(|s| s.clone()).unwrap_or_default()
        }
    }

    impl refx_core::spool::PastedImageStore for FakeSpool {
        fn store(&self, hash: ContentHash, png: &[u8]) -> Option<PathBuf> {
            if let Some(gate) = self.gate.as_ref() {
                gate.recv_timeout(Duration::from_secs(30)).ok()?;
            }
            self.stored.lock().ok()?.push((hash, png.len()));
            let path = self.dir.join(format!("{hash}.png"));
            std::fs::write(&path, png).ok()?;
            Some(path)
        }
    }

    fn spool_pool(reader: ClipboardHandle, spool: Arc<FakeSpool>) -> DecodePool {
        DecodePool::new(
            1, // worker ตัวเดียว — ลำดับของข้อความจึงเป็นลำดับของงานจริง ๆ
            Arc::new(RamBudget::new(64 << 20)),
            Limits::default(),
            None,
            Some(reader),
            Some(spool),
        )
    }

    /// ★★★ **ภาพขึ้นจอก่อน · ไฟล์ลงดิสก์ทีหลัง** — `docs/07 §2` บังคับข้อนี้ไว้
    ///
    /// วัดแล้วว่า PNG ของภาพ 6000×4000 ใช้เวลา encode **1.07 วินาที** ถ้ามันเกิด
    /// ก่อน `Done` ถูกส่ง ผู้ใช้จะเห็นโปรแกรมค้างทุกครั้งที่กด `Ctrl+V` กับภาพใหญ่
    ///
    /// ★ เทสต์นี้ **ล้มเป็น** โดยนิยาม: ถ้าใครย้ายการ encode ไปไว้ก่อนส่ง `Done`
    /// worker จะไปนอนรอประตูที่ยังไม่เปิด แล้ว `recv_timeout` ตรงนี้จะหมดเวลา
    /// — ไม่ใช่เทสต์ที่ "ผ่านเพราะเร็วพอ" แต่เป็นการบังคับลำดับด้วยการบล็อกจริง
    ///
    /// ★★ ยืนยันแล้วด้วย negative control (19 ส.ค. 2026): ย้าย `spool_pasted_image`
    /// ไปไว้ก่อน `tx.send(result)` ใน `worker_loop` → เทสต์นี้ **แดงพร้อมข้อความ
    /// `Done ต้องมาก่อน โดยไม่ต้องรอ store: Timeout`** แล้วถอดออก
    #[test]
    fn the_image_reaches_the_screen_before_the_png_reaches_the_disk() {
        let (spool, open_gate) = FakeSpool::gated("spool-order");
        let pool = spool_pool(FakeClipboard::with_image(32, 24), Arc::clone(&spool));
        pool.submit(clipboard_job(b"order"));

        // ★ ประตูยัง **ไม่เปิด** — `store` ค้างอยู่ แต่ `Done` ต้องมาถึงแล้ว
        let first = pool
            .results()
            .recv_timeout(Duration::from_secs(10))
            .expect("Done ต้องมาก่อน โดยไม่ต้องรอ store");
        let spooled = match first {
            JobResult::Done { thumb, spooled, .. } => {
                assert_eq!((thumb.source_width, thumb.source_height), (32, 24));
                spooled.expect("ภาพที่วางต้องได้คีย์ของเนื้อภาพติดมาด้วย")
            }
            other => panic!("ข้อความแรกต้องเป็น Done แต่ได้ {other:?}"),
        };
        assert!(
            spool.stored().is_empty(),
            "ไฟล์ลงดิสก์ไปแล้วทั้งที่ประตูยังไม่เปิด — เทสต์นี้ไม่ได้วัดอะไร"
        );

        // เปิดประตู → การยืนยันต้องตามมา พร้อม path จริง
        open_gate.send(()).unwrap();
        match pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .expect("ต้องมีการยืนยันตามมาหลังไฟล์ลงดิสก์")
        {
            JobResult::Spooled { hash, path } => {
                assert_eq!(hash, spooled, "คีย์ในการยืนยันต้องตรงกับที่ Done บอกไว้");
                assert!(path.exists(), "ยืนยันว่าเก็บแล้วแต่ไฟล์ไม่มีอยู่จริง");
                assert_eq!(
                    path.file_name().unwrap().to_string_lossy(),
                    format!("{hash}.png"),
                    "ชื่อไฟล์ต้องเป็น hash — ไม่งั้น sweep หาไม่เจอแล้วลบทิ้ง"
                );
            }
            other => panic!("ต้องได้ Spooled แต่ได้ {other:?}"),
        }
        assert_eq!(spool.stored().len(), 1);
    }

    /// ★★★ วางภาพเดิมซ้ำสามครั้ง → **คีย์เดียว** (ที่ `spool::store` แปลงเป็นไฟล์เดียว)
    ///
    /// คีย์ของ *งาน* ต่างกันทุกครั้ง (`clipboard:N`) โดยตั้งใจ — ผู้ใช้ที่วางซ้ำ
    /// ต้องการภาพสามใบบน board · แต่คีย์ของ *เนื้อ* ต้องเหมือนกัน ไม่งั้น
    /// spool จะเก็บ PNG 78 MB ไว้สามชุด
    #[test]
    fn pasting_the_same_image_three_times_spools_one_file() {
        let spool = FakeSpool::new("spool-dedup");
        let pool = spool_pool(RepeatingClipboard::with_image(24, 16), Arc::clone(&spool));

        let mut job_keys = Vec::new();
        let mut content_keys = Vec::new();
        for n in 0..3u8 {
            let job = clipboard_job(&[b'p', n]);
            job_keys.push(job.hash);
            pool.submit(job);
            // Done แล้ว Spooled สลับกันไป — worker ตัวเดียวจึงเป็นคู่ ๆ เสมอ
            for _ in 0..2 {
                if let JobResult::Done { hash, spooled, .. } = pool
                    .results()
                    .recv_timeout(Duration::from_secs(30))
                    .expect("ต้องได้ผลกลับมา")
                {
                    assert_eq!(hash, job_keys[n as usize]);
                    content_keys.push(spooled.expect("ต้องมีคีย์ของเนื้อภาพ"));
                }
            }
        }

        assert_eq!(job_keys.len(), 3);
        assert!(
            job_keys[0] != job_keys[1] && job_keys[1] != job_keys[2],
            "คีย์ของงานต้องต่างกันทุกครั้ง ไม่งั้นวางซ้ำจะได้ภาพใบเดียว"
        );
        assert_eq!(content_keys.len(), 3);
        assert!(
            content_keys.windows(2).all(|w| w[0] == w[1]),
            "ภาพเดิมได้คนละคีย์ → spool จะเก็บซ้ำสามชุด"
        );
        let files = std::fs::read_dir(&spool.dir).unwrap().count();
        assert_eq!(files, 1, "ได้ {files} ไฟล์แทนที่จะเป็นไฟล์เดียว");
    }

    /// ไบต์ที่ส่งเข้า spool ต้องเป็น **PNG ของภาพนั้นจริง ๆ** ไม่ใช่ thumbnail
    ///
    /// ถ้าเผลอส่ง thumbnail ไป ภาพที่กู้กลับมาจะเป็นก้อน 128 px ตลอดไป —
    /// ซึ่งเป็นหนี้ที่ทั้ง P4-5 มีไว้เพื่อปลด
    #[test]
    fn what_lands_in_the_spool_is_the_full_size_png() {
        let spool = FakeSpool::new("spool-content");
        let pool = spool_pool(FakeClipboard::with_image(200, 120), Arc::clone(&spool));
        pool.submit(clipboard_job(b"content"));
        for _ in 0..2 {
            pool.results()
                .recv_timeout(Duration::from_secs(30))
                .expect("ต้องได้ผลกลับมา");
        }

        let stored = spool.stored();
        assert_eq!(stored.len(), 1);
        let path = spool.dir.join(format!("{}.png", stored[0].0));
        let back = image::open(&path).unwrap().to_rgba8();
        assert_eq!(
            back.dimensions(),
            (200, 120),
            "ขนาดไม่ตรง — เก็บ thumbnail แทนภาพเต็มอยู่"
        );
    }

    /// ★ ไม่มีที่พักเสียบไว้ = เหมือนเดิมทุกประการ (เทสต์/fuzz ต้องไม่เขียนดิสก์)
    #[test]
    fn without_a_spool_a_paste_behaves_exactly_as_before() {
        let pool = clipboard_pool(FakeClipboard::with_image(16, 16));
        pool.submit(clipboard_job(b"no-spool"));
        match pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .expect("ต้องได้ผลกลับมา")
        {
            JobResult::Done { spooled, .. } => assert!(
                spooled.is_none(),
                "ไม่มีที่พักแต่ยังบอกว่าภาพถูกพักไว้ — path ที่ชี้ไปที่ว่างคือ I-3"
            ),
            other => panic!("ต้องสำเร็จ แต่ได้ {other:?}"),
        }
        assert!(
            pool.results()
                .recv_timeout(Duration::from_millis(200))
                .is_err(),
            "ไม่ควรมีข้อความตามมาเมื่อไม่มีที่พัก"
        );
    }

    /// ★★ ไฟล์บนดิสก์ **ไม่ต้องพักซ้ำ** — มันมีที่อยู่ถาวรของมันเองอยู่แล้ว
    #[test]
    fn a_file_on_disk_is_never_copied_into_the_spool() {
        let spool = FakeSpool::new("spool-file");
        let pool = spool_pool(FakeClipboard::with_image(8, 8), Arc::clone(&spool));
        let path = write_png("spool-file-src", "s.png", 16, 16);
        pool.submit(job(path, 0.0, b"on-disk"));

        match pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .expect("ต้องได้ผลกลับมา")
        {
            JobResult::Done { spooled, .. } => assert!(spooled.is_none()),
            other => panic!("ต้องสำเร็จ แต่ได้ {other:?}"),
        }
        assert!(
            spool.stored().is_empty(),
            "ก๊อปไฟล์ของผู้ใช้ลง spool = กินดิสก์เป็นสองเท่าโดยไม่ได้อะไร"
        );
    }

    /// ★ งานที่ถูกยกเลิกต้องไม่ทิ้งไฟล์ 78 MB ไว้ให้รอบเก็บกวาดมาลบทีหลัง
    #[test]
    fn a_cancelled_paste_never_reaches_the_spool() {
        let spool = FakeSpool::new("spool-cancel");
        let pool = spool_pool(FakeClipboard::with_image(16, 16), Arc::clone(&spool));
        let job = clipboard_job(b"cancelled");
        job.cancel.store(true, AtomicOrdering::Relaxed);
        pool.submit(job);

        match pool
            .results()
            .recv_timeout(Duration::from_secs(30))
            .expect("ต้องได้ผลกลับมา")
        {
            JobResult::Cancelled { .. } => {}
            other => panic!("ต้องถูกยกเลิก แต่ได้ {other:?}"),
        }
        assert!(spool.stored().is_empty());
    }

    /// ป้ายที่ไปโผล่ใน log ห้ามมี path เต็ม (docs/08 §5 — path มีชื่อผู้ใช้อยู่)
    #[test]
    fn job_labels_never_leak_a_full_path() {
        let source = JobSource::File(PathBuf::from("C:/Users/somchai/ref/แมว.png"));
        assert_eq!(source.label(), "แมว.png");
        assert!(!source.label().contains("somchai"));
        assert_eq!(JobSource::Clipboard.label(), "(clipboard)");
        assert!(JobSource::Clipboard.file().is_none());
    }
}
