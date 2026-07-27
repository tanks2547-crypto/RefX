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
    Limits, LoadError, decode_guarded, load_guarded, probe_dimensions, read_file_guarded,
};
use crate::hash::{ContentHash, hash_file};
use crate::thumb::{Thumbnail, make_thumbnail, read_orientation};

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

/// งาน decode หนึ่งชิ้น
#[derive(Debug, Clone)]
pub struct Job {
    /// คีย์ของเนื้อไฟล์
    pub hash: ContentHash,
    /// ไฟล์ที่จะอ่าน
    pub path: PathBuf,
    /// ระยะจากกึ่งกลาง viewport — **น้อย = ทำก่อน**
    ///
    /// ภาพนอกจอ (prefetch) ให้บวก penalty คงที่ไปเลยเพื่อให้ไปอยู่ท้ายคิวเสมอ
    pub priority: f32,
    /// ธงยกเลิก — ตั้งเป็น `true` เมื่อภาพหลุดออกนอก viewport
    pub cancel: Arc<AtomicBool>,
}

/// เหตุผลที่ decode ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum JobFailure {
    /// ไฟล์ไม่ผ่านเกราะ
    #[error(transparent)]
    Load(#[from] LoadError),

    /// ใช้เวลานานเกินเพดาน
    #[error(
        "ใช้เวลาเปิดภาพ {file} นานเกิน {seconds} วินาที — ข้ามไฟล์นี้ไปก่อน\n\
         ถ้าไฟล์อยู่บนไดรฟ์เครือข่ายหรือ cloud ลองคัดลอกมาไว้ในเครื่องก่อน"
    )]
    Timeout {
        /// ชื่อไฟล์
        file: String,
        /// เพดานเวลา (วินาที)
        seconds: u64,
    },
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
    /// ถูกยกเลิกก่อนหรือระหว่างทำ (ผู้ใช้ pan ผ่านไปแล้ว)
    Cancelled {
        /// คีย์ของภาพ
        hash: ContentHash,
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
            Self::Done { hash, .. } | Self::Cancelled { hash } | Self::Failed { hash, .. } => *hash,
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
                Err(err) => tracing::error!(%err, index, "สร้าง decode worker ไม่ได้"),
            }
        }
        // ทิ้ง sender ต้นฉบับ ไม่งั้น receiver จะไม่มีวันเห็นว่า worker ปิดหมดแล้ว
        drop(tx);

        tracing::info!(
            workers = handles.len(),
            ram_limit_mb = budget.limit() / (1 << 20),
            "เปิด decode pool"
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
                tracing::error!("decode worker จบแบบผิดปกติ");
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
            JobResult::Done { .. } => {
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

    let file = job.path.file_name().map_or_else(
        || "(ไม่ทราบชื่อไฟล์)".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );

    // ★ ถาม cache ก่อน — เจอแล้วไม่ต้องอ่านไฟล์ ไม่ต้อง decode เลย
    //   นี่คือเส้นทางที่ผู้ใช้เจอทุกวัน (เปิดไฟล์เดิมซ้ำ ๆ)
    let lookup = cache_lookup(job, io);
    if let CacheLookup::Hit(thumb) = lookup {
        return JobResult::Done {
            hash: job.hash,
            thumb,
            elapsed: started.elapsed(),
        };
    }

    // อ่านไฟล์ (ไม่ mmap — docs/06 §3) แล้วดูขนาดจาก header ก่อนขอโควตา
    let bytes = match read_for_job(job, limits) {
        Ok(bytes) => bytes,
        Err(reason) => {
            return JobResult::Failed {
                hash: job.hash,
                reason,
            };
        }
    };

    // เช็คซ้ำหลังอ่านไฟล์เสร็จ — การอ่านอาจใช้เวลานานถ้าไฟล์อยู่บนไดรฟ์เครือข่าย
    if job.cancel.load(AtomicOrdering::Relaxed) {
        return JobResult::Cancelled { hash: job.hash };
    }

    let (width, height) = match probe_dimensions(&bytes, limits) {
        Ok(dims) => dims,
        Err(err) => {
            return JobResult::Failed {
                hash: job.hash,
                reason: err.into(),
            };
        }
    };

    // ★ ขอโควตาจากถังกลาง — นอนรอถ้ายังไม่ว่าง (I-6)
    let needed = estimate_decode_bytes(width, height);
    let _reservation = budget.reserve(needed);

    // รอโควตาอาจใช้เวลานาน — เช็คธงอีกรอบก่อนลงมือจริง
    if job.cancel.load(AtomicOrdering::Relaxed) {
        return JobResult::Cancelled { hash: job.hash };
    }

    let image = match decode_guarded(&bytes, limits) {
        Ok(image) => image,
        Err(err) => {
            return JobResult::Failed {
                hash: job.hash,
                reason: err.into(),
            };
        }
    };

    // ขั้น 5: แก้ EXIF orientation — ภาพจากมือถือจะตะแคงถ้าไม่ทำ
    let image = read_orientation(&bytes).apply(image);
    // ขั้น 6: ย่อเป็น thumbnail (Lanczos3) — ทำบน worker ไม่ใช่ UI thread
    // ขั้น 7 (BC7) ถูกตัดออกจาก P1 แล้ว — docs/04 §4
    let thumb = make_thumbnail(&image);
    drop(image); // คืน RAM ของภาพเต็มทันที ไม่ต้องรอจบฟังก์ชัน

    let elapsed = started.elapsed();

    // ★ Timeout: ยกเลิก decode กลางคันไม่ได้ในทางปฏิบัติ แต่ต้องไม่ให้ทั้งคิวค้าง
    //   และต้องบอกผู้ใช้ได้ว่าไฟล์ไหนมีปัญหา (docs/06 §3)
    if elapsed > DECODE_TIMEOUT {
        stats.timed_out.fetch_add(1, AtomicOrdering::Relaxed);
        tracing::warn!(file, ?elapsed, "decode ใช้เวลานานเกินเพดาน");
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
fn cache_lookup(job: &Job, io: Option<&crossbeam_channel::Sender<IoRequest>>) -> CacheLookup {
    let Some(io) = io else {
        return CacheLookup::Unavailable;
    };
    let Ok(meta) = std::fs::metadata(&job.path) else {
        return CacheLookup::Unavailable; // ปล่อยให้ read_for_job รายงาน error ที่ชัดกว่า
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
            path: job.path.clone(),
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
            let Ok(hash) = hash_file(&job.path) else {
                return CacheLookup::Unavailable;
            };
            let _ = io.send(IoRequest::RecordPath {
                path: job.path.clone(),
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

/// อ่านไฟล์ของงานนี้เข้าหน่วยความจำ
///
/// ใช้ [`read_file_guarded`] ตัวเดียวกับ `load_guarded` — เกราะชุดเดียว ไม่เขียนซ้ำ
fn read_for_job(job: &Job, limits: &Limits) -> Result<Vec<u8>, JobFailure> {
    read_file_guarded(&job.path, limits).map_err(JobFailure::Load)
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
            path,
            priority,
            cancel: Arc::new(AtomicBool::new(false)),
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
                JobResult::Cancelled { .. } => {}
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
                path: path.clone(),
                priority: i as f32,
                cancel: Arc::clone(flag),
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
                path: path.clone(),
                priority,
                cancel: Arc::new(AtomicBool::new(false)),
            });
        }

        let mut order = Vec::new();
        for _ in 0..3 {
            order.push(
                pool.results()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .hash(),
            );
        }

        let expected: Vec<_> = [10u32, 20, 30]
            .iter()
            .map(|v| crate::hash::hash_bytes(&v.to_le_bytes()))
            .collect();
        // งานแรกอาจถูกหยิบไปก่อนที่ตัวอื่นจะเข้าคิวทัน จึงเทียบเฉพาะสองตัวหลัง
        assert_eq!(
            &order[order.len() - 2..],
            &expected[expected.len() - 2..],
            "คิวต้องเรียงตาม priority: ได้ {order:?}"
        );
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
                path: path.clone(),
                priority: i as f32,
                cancel: Arc::new(AtomicBool::new(false)),
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
                path: path.clone(),
                priority: i as f32,
                cancel: Arc::new(AtomicBool::new(false)),
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
}
