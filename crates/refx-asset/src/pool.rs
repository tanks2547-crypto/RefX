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

/// งานนี้ต้องการผลลัพธ์แบบไหน
///
/// ทั้งสองแบบใช้เกราะ decode ชุดเดียวกันหมด ต่างกันแค่ขั้นย่อขนาดตอนท้าย
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Clipboard(#[from] refx_platform::clipboard::ClipboardError),
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
        /// เวลาที่ใช้ตั้งแต่หยิบงานจนเสร็จ
        elapsed: Duration,
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
    /// ถูกยกเลิกก่อนหรือระหว่างทำ (ผู้ใช้ pan ผ่านไปแล้ว)
    Cancelled {
        /// คีย์ของภาพ
        hash: ContentHash,
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
    /// ล้มเหลว — item จะขึ้นสถานะ "โหลดไม่ได้" ไม่ใช่ crash (I-7)
    Failed {
        /// คีย์ของภาพ
        hash: ContentHash,
        /// สาเหตุ
        reason: JobFailure,
    },
}

impl JobResult {
    /// คีย์ของงานนี้ ไม่ว่าผลจะเป็นอะไร
    #[must_use]
    pub fn hash(&self) -> ContentHash {
        match self {
            Self::Done { hash, .. }
            | Self::Working { hash, .. }
            | Self::Cancelled { hash }
            | Self::ClipboardFiles { hash, .. }
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
    ) -> Self {
        let workers = workers.max(1);
        let queue = Arc::new(Queue::new());
        let stats = Arc::new(PoolStats::default());
        let (tx, results) = crossbeam_channel::unbounded();

        let wake = WakeHandle::default();
        let mut handles = Vec::with_capacity(workers);
        for index in 0..workers {
            let queue = Arc::clone(&queue);
            let stats = Arc::clone(&stats);
            let budget = Arc::clone(&budget);
            let limits = limits.clone();
            let tx = tx.clone();
            let wake = wake.clone();
            let io = io.clone();

            match std::thread::Builder::new()
                .name(format!("refx-decode-{index}"))
                .spawn(move || {
                    worker_loop(&queue, &stats, &budget, &limits, &tx, &wake, io.as_ref())
                }) {
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
    #[must_use]
    pub fn with_defaults(io: Option<crossbeam_channel::Sender<IoRequest>>) -> Self {
        Self::new(
            default_worker_count(),
            Arc::new(RamBudget::new(crate::budget::DEFAULT_RAM_LIMIT)),
            Limits::for_system(refx_platform::memory::total_ram()),
            io,
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
fn worker_loop(
    queue: &Queue,
    stats: &PoolStats,
    budget: &Arc<RamBudget>,
    limits: &Limits,
    tx: &crossbeam_channel::Sender<JobResult>,
    wake: &WakeHandle,
    io: Option<&crossbeam_channel::Sender<IoRequest>>,
) {
    while let Some(job) = queue.pop() {
        let result = run_job(&job, budget, limits, stats, io);

        match &result {
            JobResult::Done { .. }
            | JobResult::Working { .. }
            | JobResult::ClipboardFiles { .. } => {
                stats.completed.fetch_add(1, AtomicOrdering::Relaxed);
            }
            JobResult::Cancelled { .. } => {
                stats.cancelled.fetch_add(1, AtomicOrdering::Relaxed);
            }
            JobResult::Failed { .. } => {
                stats.failed.fetch_add(1, AtomicOrdering::Relaxed);
            }
        }

        // main thread อาจปิดไปแล้ว — ไม่ใช่ error
        if tx.send(result).is_err() {
            break;
        }

        // ★ ปลุก UI ให้มาเก็บผล — ถ้าไม่ปลุก event loop จะหลับต่อ
        //   แล้วภาพจะไม่ขึ้นจนกว่าผู้ใช้จะขยับเมาส์
        //   winit รวบ request_redraw หลายครั้งเป็นเฟรมเดียวอยู่แล้ว จึงไม่เปลือง
        wake.wake();
    }
}

/// ทำงานหนึ่งชิ้นจนจบ
fn run_job(
    job: &Job,
    budget: &Arc<RamBudget>,
    limits: &Limits,
    stats: &PoolStats,
    io: Option<&crossbeam_channel::Sender<IoRequest>>,
) -> JobResult {
    let started = Instant::now();

    // ★ เช็คธงยกเลิก **ก่อนเริ่ม** — ผู้ใช้ pan ผ่านไปแล้วก็ไม่ต้องเสียแรงเลย
    if job.cancel.load(AtomicOrdering::Relaxed) {
        return JobResult::Cancelled { hash: job.hash };
    }

    let file = job.source.label();

    // ★ ถาม cache ก่อน — เจอแล้วไม่ต้องอ่านไฟล์ ไม่ต้อง decode เลย
    //   นี่คือเส้นทางที่ผู้ใช้เจอทุกวัน (เปิดไฟล์เดิมซ้ำ ๆ)
    //   working texture ข้ามขั้นนี้ — cache เก็บแต่ thumbnail 128 px (docs/05 §5)
    //   clipboard ข้ามเช่นกัน — ไม่มี mtime ให้ประกอบคีย์ (ดู `JobSource::Clipboard`)
    let lookup = match (job.target, job.source.file()) {
        (JobTarget::Thumbnail, Some(path)) => cache_lookup(path, io),
        _ => CacheLookup::Unavailable,
    };
    if let CacheLookup::Hit(thumb) = lookup {
        return JobResult::Done {
            hash: job.hash,
            thumb,
            elapsed: started.elapsed(),
        };
    }

    // ★ หยิบ pixel เข้ามา — จุดเดียวที่สองแหล่งต่างกัน หลังจากนี้เหมือนกันหมด
    let (image, _reservation) = match acquire_pixels(job, budget, limits) {
        Acquired::Ready { image, reservation } => (image, reservation),
        Acquired::Files(paths) => {
            return JobResult::ClipboardFiles {
                hash: job.hash,
                paths,
            };
        }
        Acquired::Cancelled => return JobResult::Cancelled { hash: job.hash },
        Acquired::Failed(reason) => {
            return JobResult::Failed {
                hash: job.hash,
                reason,
            };
        }
    };

    // ★ working texture แยกทางตรงนี้ — ใช้เกราะทุกชั้นร่วมกันมาจนถึงจุดนี้
    if let JobTarget::Working { size } = job.target {
        let built = working::build(&image, size);
        drop(image);
        let elapsed = started.elapsed();

        // ยกเลิกกลางทางได้ — ผู้ใช้ซูมออกไปแล้วก็ไม่ต้องส่งของหนักกลับไป
        if job.cancel.load(AtomicOrdering::Relaxed) {
            return JobResult::Cancelled { hash: job.hash };
        }
        return match built {
            Some(image) => JobResult::Working {
                hash: job.hash,
                image: Box::new(image),
                elapsed,
            },
            None => {
                tracing::warn!(file, size, "could not build the working texture");
                JobResult::Cancelled { hash: job.hash }
            }
        };
    }

    // ขั้น 6: ย่อเป็น thumbnail (Lanczos3) — ทำบน worker ไม่ใช่ UI thread
    // ขั้น 7 (BC7) ถูกตัดออกจาก P1 แล้ว — docs/04 §4
    let thumb = make_thumbnail(&image);
    drop(image); // คืน RAM ของภาพเต็มทันที ไม่ต้องรอจบฟังก์ชัน

    let elapsed = started.elapsed();

    // ★ Timeout: ยกเลิก decode กลางคันไม่ได้ในทางปฏิบัติ แต่ต้องไม่ให้ทั้งคิวค้าง
    //   และต้องบอกผู้ใช้ได้ว่าไฟล์ไหนมีปัญหา (docs/06 §3)
    if elapsed > DECODE_TIMEOUT {
        stats.timed_out.fetch_add(1, AtomicOrdering::Relaxed);
        tracing::warn!(file, ?elapsed, "decode took longer than the timeout");
        return JobResult::Failed {
            hash: job.hash,
            reason: JobFailure::Timeout {
                file,
                seconds: DECODE_TIMEOUT.as_secs(),
            },
        };
    }

    // เช็คธงครั้งสุดท้าย — ถ้าผู้ใช้ pan ผ่านไปแล้วก็ไม่ต้องส่งภาพกลับให้เปลือง
    if job.cancel.load(AtomicOrdering::Relaxed) {
        return JobResult::Cancelled { hash: job.hash };
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

    JobResult::Done {
        hash: job.hash,
        thumb: Box::new(thumb),
        elapsed,
    }
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
fn acquire_pixels(job: &Job, budget: &Arc<RamBudget>, limits: &Limits) -> Acquired {
    match &job.source {
        JobSource::File(path) => acquire_from_file(path, job, budget, limits),
        JobSource::Clipboard => acquire_from_clipboard(job, budget, limits),
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
fn acquire_from_clipboard(job: &Job, budget: &Arc<RamBudget>, limits: &Limits) -> Acquired {
    let content = match refx_platform::clipboard::read() {
        Ok(content) => content,
        Err(err) => return Acquired::Failed(err.into()),
    };

    let raw = match content {
        refx_platform::clipboard::ClipboardContent::Files(paths) => {
            return Acquired::Files(paths);
        }
        refx_platform::clipboard::ClipboardContent::Image(image) => image,
    };

    // อ่าน clipboard อาจนาน — ผู้ใช้อาจกดอย่างอื่นไปแล้ว
    if job.cancel.load(AtomicOrdering::Relaxed) {
        return Acquired::Cancelled;
    }

    // ★ ผ่านเกราะ **ก่อน** ขอโควตา — ถ้าขอก่อน ภาพที่ประกาศขนาดโกงจะไปนอนรอ
    //   ถังว่างเปล่า ๆ ทั้งที่สุดท้ายก็โดนปฏิเสธอยู่ดี
    let image = match accept_rgba_guarded(raw.width, raw.height, raw.rgba, limits) {
        Ok(image) => image,
        Err(err) => return Acquired::Failed(err.into()),
    };

    // ถือโควตาไว้เท่ากับที่ภาพกินจริง ตลอดช่วงที่ยังถือภาพอยู่ — เกณฑ์เดียวกับไฟล์
    let reservation = budget.reserve(estimate_decode_bytes(image.width(), image.height()));
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
                | JobResult::Cancelled { .. }
                | JobResult::ClipboardFiles { .. } => {}
            }
        }
        assert_eq!((failed, done), (1, 1), "ไฟล์เสียต้องไม่ลากไฟล์ดีลงไปด้วย");
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
        let pool = DecodePool::new(6, Arc::clone(&budget), Limits::default(), None);

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

    /// ★ วางจาก clipboard ต้องได้คำตอบกลับมาเสมอ และห้ามลากงานอื่นลงไปด้วย
    ///
    /// เครื่องที่รันเทสต์มีอะไรใน clipboard ก็ได้ — ภาพ, ไฟล์, ข้อความล้วน,
    /// หรือไม่มี clipboard เลย (CI ที่ไม่มี display) **ทุกกรณีต้องได้ผลหนึ่งชิ้น**
    /// ไม่ใช่ panic ไม่ใช่ค้าง และไฟล์ปกติที่ตามมาต้องยังเปิดได้ตามเดิม (I-7)
    #[test]
    fn clipboard_job_always_answers_and_never_kills_the_pool() {
        let good = write_png("clip", "good.png", 24, 16);
        let pool = test_pool(2);
        pool.submit(clipboard_job(b"clip"));
        pool.submit(job(good, 1.0, b"good"));

        let mut file_done = false;
        let mut clipboard_answered = false;
        for _ in 0..2 {
            let result = pool
                .results()
                .recv_timeout(Duration::from_secs(30))
                .expect("ทุกงานต้องได้คำตอบกลับมา");
            if result.hash() == crate::hash::hash_bytes(b"clip") {
                clipboard_answered = true;
                // ผลเป็นอะไรก็รับได้ ยกเว้น "ไม่มีอะไรกลับมา"
                // ★ พิมพ์ว่าเดินสาขาไหน เพราะมันขึ้นกับว่าเครื่องที่รันมีอะไรใน
                //   clipboard — คนที่ตรวจงานด้วยมือต้องรู้ว่าเทสต์นี้ครอบคลุมอะไรจริง
                match result {
                    JobResult::Done { thumb, .. } => {
                        println!(
                            "clipboard มีภาพ {}×{} → ได้ thumbnail แล้ว",
                            thumb.source_width, thumb.source_height
                        );
                        assert!(thumb.source_width > 0 && thumb.source_height > 0);
                        assert_eq!(thumb.pixels.len(), EXPECTED_THUMB_BYTES);
                    }
                    JobResult::ClipboardFiles { paths, .. } => {
                        println!("clipboard มีไฟล์ {} รายการ", paths.len());
                    }
                    JobResult::Cancelled { .. } => println!("งาน clipboard ถูกยกเลิก"),
                    JobResult::Failed { reason, .. } => {
                        println!("clipboard เปิดไม่ได้: {reason}");
                        assert!(!reason.to_string().trim().is_empty(), "error ไม่มีข้อความ");
                    }
                    JobResult::Working { .. } => panic!("ขอ thumbnail แต่ได้ working"),
                }
            } else {
                file_done = matches!(result, JobResult::Done { .. });
            }
        }
        assert!(clipboard_answered, "งาน clipboard เงียบหาย");
        assert!(file_done, "ไฟล์ปกติต้องยังเปิดได้แม้ clipboard จะเป็นอะไรก็ตาม");
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
