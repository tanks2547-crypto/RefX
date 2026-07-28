//! ต่อสาย `refx-platform` (หน้าต่าง) เข้ากับ `refx-render` (GPU) แล้วเอา egui วางทับ
//!
//! นี่คือที่เดียวที่ทั้งสามโลกมาเจอกัน — winit, wgpu, egui อยู่บน device เดียวกัน
//!
//! spec: docs/04-rendering.md §1, §8

use std::sync::Arc;

use glam::Vec2;
use refx_asset::cache::{CacheStats, IoRequest, IoThread};
use refx_asset::pool::DecodePool;
use refx_core::view::Camera;

use crate::shell::LoadProgress;
use refx_platform::redraw::RedrawReason;
use refx_platform::window::{AppDelegate, WindowConfig};
use refx_render::atlas::{ThumbnailAtlas, layers_for_budget};
use refx_render::device::{DeviceError, FrameStatus, RenderContext, RenderOptions};
use refx_render::instance::QuadInstance;
use refx_render::pipeline::{CameraUniform, QuadPipeline};
use refx_render::texture::TextureAllocator;
use winit::event::WindowEvent;
use winit::window::Window;

/// อาร์กิวเมนต์จากบรรทัดคำสั่งที่ส่งต่อลงมาถึงชั้น render
#[derive(Debug, Clone, Default)]
pub struct AppArgs {
    /// จำลอง device lost หลังวาดครบ N เฟรม (ต้องเปิด feature `force-device-lost`)
    pub force_device_lost_after: Option<u64>,
    /// จำลอง device lost หลังผ่านไป N มิลลิวินาที — ทดสอบเส้นทาง "หลับแล้วตาย"
    pub force_device_lost_after_ms: Option<u64>,
    /// วาดสี่เหลี่ยมสีสุ่ม N อัน เพื่อทดสอบ pipeline (P0-6) และ pan/zoom (P0-7)
    pub demo_quads: Option<u32>,
    /// วัด frame time ต่อเนื่อง N วินาทีแล้วรายงานผล — โหมด benchmark
    pub bench_seconds: Option<u64>,
    /// ไฟล์ที่จะเปิดตั้งแต่เริ่มโปรแกรม (เหมือนลากเข้ามา)
    ///
    /// ใช้ทั้งกับการเปิดจากบรรทัดคำสั่งและวัดเวลา "เปิดไฟล์ → ภาพขึ้นจอ"
    pub open_files: Vec<std::path::PathBuf>,
}

/// PRNG แบบ xorshift64* — **deterministic เสมอ**
///
/// ใช้ seed คงที่เพื่อให้ benchmark เทียบกันได้ระหว่างการรัน
/// (CLAUDE.md: ผลลัพธ์ที่ไม่คงที่ทำให้วัดอะไรไม่ได้)
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// ค่าใน [0, 1)
    fn next_f32(&mut self) -> f32 {
        // ใช้ 24 บิตบน = ความละเอียดของ mantissa f32 พอดี
        ((self.next_u64() >> 40) as f32) / ((1u32 << 24) as f32)
    }
}

/// สร้างสี่เหลี่ยมสีสุ่มแบบกระจายทั่ว world สำหรับทดสอบ pipeline
fn demo_quads(count: u32, world: f32) -> Vec<QuadInstance> {
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    (0..count)
        .map(|_| {
            let size = 20.0 + rng.next_f32() * 60.0;
            QuadInstance::solid(
                rng.next_f32() * world,
                rng.next_f32() * world,
                size,
                size,
                [
                    0.25 + rng.next_f32() * 0.75,
                    0.25 + rng.next_f32() * 0.75,
                    0.25 + rng.next_f32() * 0.75,
                    1.0,
                ],
            )
        })
        .collect()
}

/// สถิติ frame time สำหรับโหมด benchmark
#[derive(Debug, Default)]
struct FrameStats {
    times_us: Vec<u64>,
}

impl FrameStats {
    fn record(&mut self, micros: u64) {
        self.times_us.push(micros);
    }

    /// รายงาน p50 / p99 / fps เฉลี่ย เทียบกับเพดานใน ARCHITECTURE §6
    ///
    /// **ต้องรายงาน present mode ด้วยเสมอ** — ถ้าเป็น `AutoVsync` ตัวเลขคือคาบสัญญาณจอ
    /// ไม่ใช่ต้นทุนการวาด เอาไปสรุปว่า "ผ่านเพดาน" ไม่ได้
    fn report(&mut self, quads: usize, present: wgpu::PresentMode) {
        if self.times_us.is_empty() {
            tracing::warn!("ไม่มีเฟรมให้วัด");
            return;
        }
        self.times_us.sort_unstable();
        let n = self.times_us.len();
        let p50 = self.times_us[n / 2] as f64 / 1000.0;
        let p99 = self.times_us[(n * 99 / 100).min(n - 1)] as f64 / 1000.0;
        let total: u64 = self.times_us.iter().sum();
        let mean_ms = total as f64 / n as f64 / 1000.0;
        let fps = if mean_ms > 0.0 { 1000.0 / mean_ms } else { 0.0 };

        let vsync_capped = matches!(
            present,
            wgpu::PresentMode::AutoVsync | wgpu::PresentMode::Fifo | wgpu::PresentMode::FifoRelaxed
        );

        tracing::info!(
            quads,
            frames = n,
            ?present,
            p50_ms = format!("{p50:.3}"),
            p99_ms = format!("{p99:.3}"),
            fps = format!("{fps:.1}"),
            "ผลวัด frame time"
        );

        println!(
            "quad {quads} | เฟรม {n} | present {present:?}\n\
             p50 {p50:.3} ms | p99 {p99:.3} ms | เฉลี่ย {mean_ms:.3} ms ({fps:.1} fps)"
        );
        if vsync_capped {
            println!("⚠ ตัวเลขนี้ชนเพดาน vsync ของจอ — อ่านเป็นต้นทุนการวาดไม่ได้ และสรุปเรื่อง headroom ไม่ได้");
        } else {
            println!(
                "เพดาน ARCHITECTURE §6: frame time ≤ 8 ms, p99 ≤ 16 ms  →  {}",
                if p50 <= 8.0 && p99 <= 16.0 {
                    "ผ่าน"
                } else {
                    "ไม่ผ่าน"
                }
            );
        }
    }
}

/// สีพื้นหลังของ canvas — เทาเข้มแบบเดียวกับโปรแกรมวาด
/// (ค่า linear เพราะ surface เป็น sRGB, GPU แปลง gamma ให้เอง)
const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.05,
    g: 0.06,
    b: 0.08,
    a: 1.0,
};

/// สถานะกราฟิกทั้งหมด — เกิดหลังหน้าต่างพร้อมเท่านั้น
struct Gfx {
    window: Arc<Window>,
    render: RenderContext,
    /// pipeline ของ instanced quad — ผูกกับ device ต้องสร้างใหม่หลังกู้
    pipeline: QuadPipeline,
    /// atlas ของ thumbnail — ผูกกับ device เช่นกัน
    atlas: ThumbnailAtlas,
    /// ★ ทางเดียวที่สร้าง texture ได้ (I-6 / CLAUDE.md)
    textures: TextureAllocator,
    egui_ctx: egui::Context,
    egui_winit: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    /// รุ่นของ device ที่ resource ชุดนี้ผูกอยู่ — ใช้ตรวจว่าต้องสร้างใหม่ไหม
    device_generation: u64,
    /// เวลาที่ egui ขอให้ปลุกมาวาดอีกครั้ง (เคอร์เซอร์กะพริบ, animation)
    ///
    /// `None` = ไม่มีอะไรค้าง หลับยาวได้
    egui_wake: Option<std::time::Instant>,
    /// สี่เหลี่ยมทดสอบ (P0-6/P0-7) — ว่างเปล่าตอนใช้งานจริง
    quads: Vec<QuadInstance>,
    /// ★ thumbnail ของทุก item บน board เก็บไว้เติม atlas กลับหลังกู้ device
    ///
    /// docs/04 §4: ถ้าไม่เติมกลับ ผู้ใช้จะเห็น **board ว่างเปล่า** หลัง driver อัปเดต
    /// ซึ่งจากมุมเขาแยกไม่ออกจาก "งานหาย" แล้วงานที่ P0-5 ทำมาทั้งหมดเสียเปล่า
    ///
    /// เก็บ pixel ไว้ใน RAM เลยเพราะ 128×128×4 = 64 KB ต่อภาพ
    /// (1000 ภาพ = 64 MB ซึ่งยังอยู่ในงบ) และเร็วกว่าอ่านกลับจาก sqlite มาก
    board_thumbs: Vec<refx_asset::thumb::Thumbnail>,
    /// กล้อง pan/zoom (P0-7)
    camera: Camera,
    /// ★ กรอบของช่อง canvas จริง (physical pixel) — **ไม่ใช่ขนาดหน้าต่างทั้งบาน**
    ///
    /// egui เป็นคนบอกว่าช่องกลางอยู่ตรงไหนหลังหัก panel ซ้าย/ขวา/บน/ล่างออกแล้ว
    /// ค่านี้ถูกใช้สองที่และ **ต้องเป็นค่าเดียวกัน** ไม่งั้นภาพกับเมาส์จะไม่ตรงกัน:
    ///   1. `set_viewport` ของ render pass + กรอบอ้างอิงของกล้องตอนวาด
    ///   2. แปลงพิกัดเคอร์เซอร์ตอน zoom เข้าหาเมาส์ (P0-7)
    canvas: CanvasRect,
    /// ตำแหน่งเคอร์เซอร์ล่าสุดบนจอ (physical pixel, พิกัดหน้าต่าง)
    cursor: Vec2,
    /// กำลังลากเพื่อ pan อยู่หรือไม่
    panning: bool,
}

/// แปลงสถิติสะสมของ decode pool เป็นความคืบหน้าของ **งวดปัจจุบัน**
///
/// `PoolStats` นับสะสมตลอดอายุโปรแกรม ถ้าเอาไปแสดงตรง ๆ ผู้ใช้ที่ลากภาพชุดที่สอง
/// เข้ามาจะเห็น "กำลังโหลด 100 / 150" ทั้งที่ในใจเขาคือ "0 จาก 50"
/// จึงจำจุดที่คิวว่างครั้งล่าสุดไว้เป็นเส้นเริ่มของงวดถัดไป
#[derive(Debug, Default)]
struct LoadTracker {
    /// จำนวนงานสะสม ณ ตอนที่คิวว่างครั้งล่าสุด
    base: u64,
}

impl LoadTracker {
    /// อัปเดตจากสถิติล่าสุด — คืน `None` เมื่อไม่มีงานค้าง
    fn update(&mut self, stats: refx_asset::pool::PoolStatsSnapshot) -> Option<LoadProgress> {
        let (done, total) = (stats.finished(), stats.submitted);

        // คิวว่าง = จบงวดนี้แล้ว ตั้งเส้นเริ่มใหม่ไว้รองานชุดถัดไป
        // `>=` ไม่ใช่ `==` เพราะ snapshot อ่านตัวนับหลายตัวแบบไม่ atomic
        // ผลลัพธ์อาจ "จบเกินที่ส่ง" ชั่วขณะได้ ซึ่งไม่ใช่ความผิดปกติ
        if done >= total {
            self.base = total;
            return None;
        }

        Some(LoadProgress {
            done: done.saturating_sub(self.base),
            total: total.saturating_sub(self.base),
        })
    }
}

/// กรอบของช่อง canvas ในหน่วย physical pixel
#[derive(Debug, Clone, Copy)]
struct CanvasRect {
    /// มุมซ้ายบนเทียบกับมุมซ้ายบนของ surface
    min: Vec2,
    /// กว้าง × สูง — รับประกันว่า ≥ 1 เสมอ (wgpu ปฏิเสธ viewport ขนาด 0)
    size: Vec2,
}

impl CanvasRect {
    /// ค่าเริ่มต้นก่อนที่ egui จะบอกกรอบจริงในเฟรมแรก — ใช้ทั้งหน้าต่างไปก่อน
    fn full(width: u32, height: u32) -> Self {
        Self {
            min: Vec2::ZERO,
            size: Vec2::new(width.max(1) as f32, height.max(1) as f32),
        }
    }

    /// แปลง rect ของ egui (หน่วย point) เป็น physical pixel แล้วตัดให้อยู่ในผิววาด
    ///
    /// ต้องตัดกรอบเสมอ: `set_viewport` ที่ล้นขอบ attachment เป็น validation error
    /// ของ wgpu ซึ่งจะทำให้ทั้งเฟรมหายไป ไม่ใช่แค่ภาพเยื้อง
    fn from_points(rect: egui::Rect, pixels_per_point: f32, width: u32, height: u32) -> Self {
        let scale = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
            pixels_per_point
        } else {
            1.0
        };
        let (surface_w, surface_h) = (width.max(1) as f32, height.max(1) as f32);

        // ค่าที่ไม่ใช่ตัวเลขต้องถูกแทนที่ก่อนถึง clamp เสมอ (I-4)
        // `Rect::NOTHING` (เฟรมแรก ก่อน egui บอกกรอบจริง) ให้ ±inf ออกมาตรง ๆ
        // และ `f32::clamp` จะ **panic** ถ้าขอบเป็น NaN หรือ min > max
        let finite = |value: f32, fallback: f32| if value.is_finite() { value } else { fallback };

        // จำกัด x/y ไว้ที่ surface-1 เพื่อให้เหลือที่ให้ viewport อย่างน้อย 1 px เสมอ
        // ถ้าปล่อยให้ x เท่ากับ surface_w พอดี ขอบบนของ clamp จะกลายเป็น 0 < 1 แล้ว panic
        let x = finite(rect.min.x * scale, 0.0).clamp(0.0, (surface_w - 1.0).max(0.0));
        let y = finite(rect.min.y * scale, 0.0).clamp(0.0, (surface_h - 1.0).max(0.0));
        let w = finite(rect.width() * scale, surface_w).clamp(1.0, surface_w - x);
        let h = finite(rect.height() * scale, surface_h).clamp(1.0, surface_h - y);

        Self {
            min: Vec2::new(x, y),
            size: Vec2::new(w, h),
        }
    }

    /// พิกัดเคอร์เซอร์ของหน้าต่าง → พิกัดภายในช่อง canvas
    fn to_local(self, cursor: Vec2) -> Vec2 {
        cursor - self.min
    }

    /// จุดนี้อยู่ในช่อง canvas ไหม (พิกัดหน้าต่าง)
    fn contains(self, point: Vec2) -> bool {
        let max = self.min + self.size;
        point.x >= self.min.x && point.x < max.x && point.y >= self.min.y && point.y < max.y
    }
}

/// แอปหลักของ RefX
///
/// `Default` = ยังไม่มีหน้าต่าง (winit 0.30 บังคับให้สร้างหน้าต่างใน `resumed()`)
#[derive(Default)]
pub struct RefxApp {
    gfx: Option<Gfx>,
    args: AppArgs,
    /// สถิติ frame time — เก็บเฉพาะโหมด benchmark
    stats: FrameStats,
    /// เวลาที่เริ่มโหมด benchmark
    bench_start: Option<std::time::Instant>,
    /// รายงานผล benchmark ไปแล้วหรือยัง (กันรายงานซ้ำ)
    bench_done: bool,
    /// สถานะของ shell (mode, status bar) — อยู่นอก `Gfx` เพราะไม่ผูกกับ device
    /// จึงรอดจาก device lost ไปได้
    shell: crate::shell::ShellState,

    /// pool ถอดรหัสภาพ + IO thread — ไม่ผูกกับ GPU จึงรอด device lost เช่นกัน
    ///
    /// `None` เมื่อเปิด cache ไม่ได้จริง ๆ — โปรแกรมยังใช้งานได้ แค่ไม่มี cache
    assets: Option<Assets>,
    /// ช่องรับสถิติ cache จาก IO thread (ไม่บล็อก UI thread — I-2)
    cache_stats_rx: Option<crossbeam_channel::Receiver<CacheStats>>,
    /// ตัวปลุก event loop — ส่งต่อให้ worker หลังหน้าต่างพร้อม
    waker: Option<refx_platform::window::Waker>,
    /// เวลาที่ผู้ใช้ปล่อยไฟล์ลงหน้าต่าง (ใช้วัด "ลากเข้ามา → ภาพขึ้นจอ")
    drop_started: Option<std::time::Instant>,
    /// จำนวนไฟล์ในชุดที่ลากเข้ามารอบล่าสุด
    drop_expected: usize,
    /// จำนวนที่ขึ้นจอแล้วในรอบนี้
    drop_shown: usize,
    /// รายงานเวลาของรอบนี้ไปแล้วหรือยัง
    drop_reported: bool,
    /// ไฟล์ที่เพิ่งถูกลากเข้ามา — winit ส่งมาทีละไฟล์ จึงรวบไว้ก่อนแล้วส่งเป็นชุดเดียว
    pending_drops: Vec<std::path::PathBuf>,
    /// แปลงสถิติสะสมของ pool เป็นความคืบหน้าของงวดปัจจุบัน
    loading: LoadTracker,
}

/// ส่วนที่จัดการภาพ — อยู่คนละโลกกับ GPU
struct Assets {
    pool: DecodePool,
    /// ต้องถือไว้ให้ IO thread มีชีวิตอยู่ (drop = ปิด thread + ล้าง cache)
    ///
    /// `None` เมื่อเปิด cache ไม่ได้ — โปรแกรมยังใช้งานได้ แค่ decode ใหม่ทุกครั้ง
    _io: Option<IoThread>,
    io_tx: Option<crossbeam_channel::Sender<IoRequest>>,
}

impl RefxApp {
    /// สร้างแอปที่ยังไม่ผูกกับหน้าต่าง
    #[must_use]
    pub fn new(args: AppArgs) -> Self {
        Self {
            gfx: None,
            args,
            stats: FrameStats::default(),
            bench_start: None,
            bench_done: false,
            shell: crate::shell::ShellState::default(),
            assets: None,
            cache_stats_rx: None,
            waker: None,
            drop_started: None,
            drop_expected: 0,
            drop_shown: 0,
            drop_reported: true,
            pending_drops: Vec::new(),
            loading: LoadTracker::default(),
        }
    }

    /// ไฟล์ที่สั่งเปิดจากบรรทัดคำสั่ง — เข้าคิวเหมือนลากเข้ามาทุกประการ
    fn queue_initial_files(&mut self) {
        let files = std::mem::take(&mut self.args.open_files);
        if !files.is_empty() {
            self.pending_drops.extend(files);
        }
    }

    /// ผู้ใช้ลากไฟล์เข้ามา — ส่งเข้าคิว decode
    ///
    /// **ไม่แตะดิสก์บน UI thread เลย** (I-2) — แค่ส่ง path เข้าคิว
    /// การอ่านไฟล์/hash/decode เกิดบน worker ทั้งหมด
    fn submit_dropped(&mut self, paths: Vec<std::path::PathBuf>) {
        let Some(assets) = self.assets.as_ref() else {
            return;
        };
        if paths.is_empty() {
            return;
        }

        // เริ่มจับเวลาชุดใหม่
        self.drop_started = Some(std::time::Instant::now());
        self.drop_expected = paths.len();
        self.drop_shown = 0;
        self.drop_reported = false;

        for (i, path) in paths.into_iter().enumerate() {
            // hash จาก path ไปก่อน — hash เนื้อไฟล์จริงเกิดบน worker (P1-2)
            // ที่นี่ต้องการแค่คีย์ชั่วคราวไว้จับคู่ผลลัพธ์
            let hash = refx_asset::hash::hash_bytes(path.to_string_lossy().as_bytes());
            assets.pool.submit(refx_asset::pool::Job {
                hash,
                path,
                // ยังไม่มี layout จริง → เรียงตามลำดับที่ลากเข้ามา
                priority: i as f32,
                cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            });
        }
        self.shell.status = format!("กำลังเปิด {} ไฟล์…", self.drop_expected);
    }

    /// เปิด decode pool + IO thread
    ///
    /// เปิด cache ไม่ได้ **ไม่ใช่เหตุให้โปรแกรมล้ม** — thumbnail สร้างใหม่ได้เสมอ
    /// (docs/05 §5) จึงแค่ log แล้วทำงานต่อโดยไม่มี cache
    pub fn start_assets(&mut self, cache_db: &std::path::Path) {
        // ★ IO thread ต้องมาก่อน pool — worker ต้องถือช่องคุยกับ cache ตั้งแต่เกิด
        //   ไม่งั้น job แรก ๆ จะ decode ทั้งที่ cache มีของอยู่แล้ว
        let (io, io_tx) = match IoThread::spawn(cache_db) {
            Ok(io) => {
                let tx = io.sender();
                (Some(io), tx)
            }
            Err(err) => {
                // เปิด cache ไม่ได้ไม่ใช่เหตุให้ล้ม — thumbnail สร้างใหม่ได้เสมอ
                tracing::error!(%err, "เปิด cache ไม่ได้ — ทำงานต่อโดยไม่มี cache");
                self.shell.status = "ใช้งานได้ แต่ไม่มี cache ภาพย่อ".to_owned();
                (None, None)
            }
        };

        let pool = DecodePool::with_defaults(io_tx.clone());
        let (used, limit) = pool.ram_usage();
        self.shell.ram_used = used;
        self.shell.ram_limit = limit;

        tracing::info!(
            workers = pool.worker_count(),
            ram_limit_mb = limit / (1 << 20),
            cache = io.is_some(),
            "เปิด decode pool"
        );

        // ขอสถิติครั้งแรก — หลังจากนี้ขอใหม่เฉพาะตอนมีงาน decode เสร็จ
        // (ถ้าขอเป็นระยะจะปลุก event loop ตลอด แล้วพัง I-1)
        if let Some(tx) = io_tx.as_ref() {
            let (reply, rx) = crossbeam_channel::bounded(4);
            let _ = tx.send(IoRequest::Stats { reply });
            self.cache_stats_rx = Some(rx);
        }

        self.assets = Some(Assets {
            pool,
            _io: io,
            io_tx,
        });
    }

    /// ดึงผล decode ที่เสร็จแล้วออกจากคิว — **ไม่บล็อก** (I-2)
    ///
    /// คืน `true` ถ้ามีอะไรเปลี่ยนจนต้องวาดใหม่
    ///
    /// TODO(P1-5): เอา `image` ไปอัดลง atlas แทนที่จะทิ้ง
    /// TODO(P1-4): worker ต้องปลุก event loop ด้วย `EventLoopProxy` เมื่อมีผลใหม่
    ///   (เงื่อนไขข้อ 2 ใน docs/04 §1) ตอนนี้ผลจะถูกเก็บตอนวาดเฟรมถัดไปเท่านั้น
    fn drain_decode_results(&mut self) -> bool {
        let Some(assets) = self.assets.as_ref() else {
            return false;
        };

        let mut finished = 0u32;
        let mut done = Vec::new();
        while let Some(result) = assets.pool.try_recv() {
            match result {
                refx_asset::pool::JobResult::Done {
                    hash,
                    thumb,
                    elapsed,
                } => {
                    tracing::debug!(hash = %hash.short(), ?elapsed, "ถอดรหัสภาพเสร็จ");
                    done.push(thumb);
                }
                refx_asset::pool::JobResult::Cancelled { .. } => {}
                refx_asset::pool::JobResult::Failed { hash, reason } => {
                    // I-7: ภาพเสียหนึ่งไฟล์ = item ขึ้นสถานะ "โหลดไม่ได้" ไม่ใช่ crash
                    tracing::warn!(hash = %hash.short(), %reason, "เปิดภาพไม่ได้");
                    self.shell.status = reason.to_string();
                }
            }
            finished += 1;
        }

        // อัดขึ้น atlas แล้ววาง quad ให้เห็นบน canvas
        if !done.is_empty()
            && let Some(gfx) = self.gfx.as_mut()
        {
            for thumb in done {
                match gfx
                    .atlas
                    .upload(gfx.render.device(), gfx.render.queue(), &thumb.pixels)
                {
                    Ok(slot) => {
                        // จัดเป็นตารางง่าย ๆ ไปก่อน — layout จริงมาใน P2/P3
                        let n = gfx.quads.len() as u32;
                        let (col, row) = (n % 16, n / 16);
                        let cell = 160.0;
                        // คงอัตราส่วนภาพเดิมไว้ ไม่บีบให้เป็นจัตุรัส
                        let (sw, sh) = (thumb.source_width.max(1), thumb.source_height.max(1));
                        let scale =
                            128.0 / f32::from(u16::try_from(sw.max(sh)).unwrap_or(u16::MAX));
                        gfx.quads.push(QuadInstance {
                            transform: [
                                sw as f32 * scale,
                                0.0,
                                0.0,
                                sh as f32 * scale,
                                2000.0 + col as f32 * cell,
                                2000.0 + row as f32 * cell,
                            ],
                            uv_rect: slot.uv_rect(),
                            tint: [1.0, 1.0, 1.0, 1.0],
                            layer: slot.layer,
                            flags: 0, // มี texture จริงแล้ว ไม่ใช่ placeholder
                        });
                        gfx.board_thumbs.push(*thumb);
                        self.drop_shown += 1;
                    }
                    Err(err) => {
                        tracing::warn!(%err, "เก็บภาพย่อลง atlas ไม่ได้");
                        self.shell.status = err.to_string();
                    }
                }
            }

            // ★ เวลาจริงที่ผู้ใช้รู้สึก: ลากเข้ามา → ภาพขึ้นจอ
            if !self.drop_reported
                && self.drop_shown >= self.drop_expected
                && let Some(started) = self.drop_started
            {
                let elapsed = started.elapsed();
                self.drop_reported = true;
                tracing::info!(
                    files = self.drop_expected,
                    ms = elapsed.as_secs_f64() * 1000.0,
                    "ลากไฟล์เข้ามา → ภาพขึ้นจอครบ"
                );
                println!(
                    "ลากไฟล์ {} ไฟล์ → ขึ้นจอครบใน {:.1} ms",
                    self.drop_expected,
                    elapsed.as_secs_f64() * 1000.0
                );
                self.shell.status = format!(
                    "เปิด {} ไฟล์ใน {:.0} ms",
                    self.drop_expected,
                    elapsed.as_secs_f64() * 1000.0
                );
            }
        }

        if finished > 0 {
            // cache เพิ่งเปลี่ยน — ขอสถิติรอบใหม่ (event-driven ไม่ใช่ polling
            // ถ้าขอเป็นระยะจะปลุก event loop ตลอดแล้วพัง I-1)
            if let Some(tx) = assets.io_tx.as_ref() {
                let (reply, rx) = crossbeam_channel::bounded(4);
                if tx.send(IoRequest::Stats { reply }).is_ok() {
                    self.cache_stats_rx = Some(rx);
                }
            }
        }
        finished > 0
    }

    /// อยู่ในโหมด benchmark และยังไม่ครบเวลาหรือไม่
    fn bench_running(&self) -> bool {
        if self.bench_done {
            return false;
        }
        let Some(seconds) = self.args.bench_seconds else {
            return false;
        };
        self.bench_start
            .is_none_or(|start| start.elapsed().as_secs() < seconds)
    }

    /// สร้าง egui ทั้งชุดใหม่ให้ผูกกับ device ปัจจุบัน
    ///
    /// แยกออกมาเพราะต้องเรียกทั้งตอนเปิดโปรแกรมและตอนกู้ device
    fn build_egui(
        window: &Arc<Window>,
        render: &RenderContext,
    ) -> (egui::Context, egui_winit::State, egui_wgpu::Renderer) {
        let egui_ctx = egui::Context::default();
        let egui_winit = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            Some(render.capabilities().max_texture_dimension_2d as usize),
        );
        // docs/04 §6: ส่ง format จริง (sRGB) เข้าไปให้ egui รู้ตัว
        // จะได้ไม่แก้ gamma ซ้ำซ้อน — ยืนยันว่าใช้ sRGB ต่อไป ไม่เปลี่ยนเป็น linear
        let egui_renderer = egui_wgpu::Renderer::new(
            render.device(),
            render.format(),
            egui_wgpu::RendererOptions::default(),
        );
        (egui_ctx, egui_winit, egui_renderer)
    }

    /// กู้ device ที่หายไป แล้วสร้าง resource ที่ผูกกับ device ใหม่ทั้งหมด
    ///
    /// docs/04 §7 ข้อ 3 — **document state ไม่ถูกแตะเลย** งานผู้ใช้ไม่หาย
    ///
    /// ตัดสินใจเอง: สร้าง `egui::Context` ใหม่ทั้งก้อนแทนที่จะเก็บของเดิมไว้
    /// เพราะ font atlas ของ egui ผูกกับ `Renderer` เดิม ถ้าเก็บ Context ไว้
    /// แต่สร้าง Renderer ใหม่ ตัว atlas จะไม่ถูกอัปโหลดซ้ำ → UI หาย
    /// ผลข้างเคียงคือสถานะชั่วคราวของ UI (ตำแหน่ง scroll ฯลฯ) รีเซ็ต
    /// ซึ่งยอมรับได้เพราะ device lost เป็นเหตุการณ์นาน ๆ ครั้ง และ "เสถียร" มาก่อน
    fn recover_device(&mut self) -> Option<RedrawReason> {
        let gfx = self.gfx.as_mut()?;

        if let Err(err) = gfx.render.recover() {
            // กู้ไม่สำเร็จ — ของเดิมยังอยู่ครบ ไม่ล้มโปรแกรม ลองใหม่เฟรมหน้า
            tracing::error!(%err, "กู้ GPU device ไม่สำเร็จ");
            return None;
        }

        let (egui_ctx, egui_winit, egui_renderer) = Self::build_egui(&gfx.window, &gfx.render);
        gfx.egui_ctx = egui_ctx;
        gfx.egui_winit = egui_winit;
        gfx.egui_renderer = egui_renderer;
        // atlas + pipeline ผูกกับ device เดิม ต้องสร้างใหม่ทั้งคู่ (docs/04 §7 ข้อ 3)
        // TODO(P1-5): re-upload thumbnail จาก cache.sqlite แทนที่จะปล่อยว่าง
        // ★ สร้าง allocator ใหม่ด้วย — ของเก่านับโควตาของ device ที่ตายไปแล้ว
        gfx.textures = TextureAllocator::new(gfx.render.capabilities());
        let atlas_budget = (gfx.textures.budget().limit() / 2) as u64;
        let max_layers = layers_for_budget(gfx.render.capabilities(), atlas_budget);
        match ThumbnailAtlas::new(gfx.render.device(), &gfx.textures, max_layers) {
            Ok(atlas) => gfx.atlas = atlas,
            Err(err) => {
                tracing::error!(%err, "สร้าง atlas ใหม่หลังกู้ device ไม่ได้");
                return None;
            }
        }
        gfx.pipeline = QuadPipeline::new(
            gfx.render.device(),
            gfx.render.format(),
            gfx.atlas.bind_group_layout(),
        );
        gfx.device_generation = gfx.render.generation();

        // ★ เติม atlas กลับ (docs/04 §4) — ถ้าไม่ทำ ผู้ใช้จะเห็น board ว่างเปล่า
        //   หลัง driver อัปเดต ซึ่งแยกไม่ออกจาก "งานหาย" แล้วเขาจะปิดโปรแกรมทิ้ง
        //   ทำให้งานกู้ device ทั้งหมดเสียเปล่า
        Self::refill_atlas(gfx);

        Some(RedrawReason::SurfaceRecovery)
    }

    /// อัด thumbnail ของทุก item กลับขึ้น atlas ใหม่หลังกู้ device
    ///
    /// ระหว่างที่ยังเติมไม่ครบ item ที่เหลือถูกทำเป็น **placeholder สีเด่น**
    /// ไม่ใช่ช่องว่าง (docs/04 §4, §8) — ผู้ใช้ต้องเห็นว่า layout ยังอยู่ครบ
    fn refill_atlas(gfx: &mut Gfx) {
        if gfx.board_thumbs.is_empty() {
            return;
        }
        let started = std::time::Instant::now();
        let mut restored = 0usize;

        for (index, thumb) in gfx.board_thumbs.iter().enumerate() {
            let Some(quad) = gfx.quads.get_mut(index) else {
                break;
            };
            match gfx
                .atlas
                .upload(gfx.render.device(), gfx.render.queue(), &thumb.pixels)
            {
                Ok(slot) => {
                    quad.uv_rect = slot.uv_rect();
                    quad.layer = slot.layer;
                    quad.tint = [1.0, 1.0, 1.0, 1.0];
                    quad.flags &= !refx_render::instance::flags::PLACEHOLDER;
                    restored += 1;
                }
                Err(err) => {
                    // atlas เต็ม — ที่เหลือขึ้นเป็นสี่เหลี่ยมสีเด่นแทนช่องว่าง
                    tracing::warn!(%err, index, "เติม atlas กลับไม่ครบ — ที่เหลือใช้ placeholder");
                    let [a, r, g, b] = thumb.dominant.to_be_bytes();
                    quad.tint = [
                        f32::from(r) / 255.0,
                        f32::from(g) / 255.0,
                        f32::from(b) / 255.0,
                        f32::from(a) / 255.0,
                    ];
                    quad.flags |= refx_render::instance::flags::PLACEHOLDER;
                }
            }
        }

        tracing::info!(
            restored,
            total = gfx.board_thumbs.len(),
            ms = started.elapsed().as_secs_f64() * 1000.0,
            "เติม atlas กลับหลังกู้ device"
        );
    }
}

impl AppDelegate for RefxApp {
    type Error = DeviceError;

    fn set_waker(&mut self, waker: refx_platform::window::Waker) {
        // ★ ผูกตัวปลุกเข้ากับ decode pool — worker ที่ decode เสร็จจะปลุก event loop
        //   ถ้าไม่มีขั้นนี้ ภาพจะไม่ขึ้นจนกว่าผู้ใช้จะขยับเมาส์ (docs/04 §1 ข้อ 2)
        if let Some(assets) = self.assets.as_ref() {
            let w = waker.clone();
            assets.pool.wake_handle().connect(move || w.wake());
        }
        self.waker = Some(waker);
    }

    fn window_ready(&mut self, window: Arc<Window>) -> Result<(), Self::Error> {
        let size = window.inner_size();
        let render = RenderContext::new(
            Arc::clone(&window),
            size.width,
            size.height,
            RenderOptions {
                force_device_lost_after: self.args.force_device_lost_after,
                force_device_lost_after_ms: self.args.force_device_lost_after_ms,
                // ปลด vsync เฉพาะตอน benchmark — ใช้งานปกติเป็น AutoVsync เสมอ
                uncapped_present: self.args.bench_seconds.is_some(),
            },
        )?;

        let (egui_ctx, egui_winit, egui_renderer) = Self::build_egui(&window, &render);
        // ★ ทางเดียวที่สร้าง texture ได้ (I-6) — atlas ต้องขอผ่านตัวนี้
        let textures = TextureAllocator::new(render.capabilities());
        // atlas ขอได้ไม่เกินครึ่งงบ VRAM — อีกครึ่งเผื่อ working texture (P1-7)
        // ★ ตัวเลขนี้เป็น **เพดาน** ไม่ใช่การจองจริง — atlas จองทีละ layer
        //   ตอนมีภาพเข้ามาจริง เปิดโปรแกรมเปล่าจึงกิน VRAM ≈ 0 (docs/05 §2)
        let atlas_budget = textures.budget().limit() as u64 / 2;
        let max_layers = layers_for_budget(render.capabilities(), atlas_budget);
        let atlas = ThumbnailAtlas::new(render.device(), &textures, max_layers).map_err(|err| {
            tracing::error!(%err, "สร้าง atlas ไม่ได้");
            DeviceError::NoSupportedFormat
        })?;
        let pipeline =
            QuadPipeline::new(render.device(), render.format(), atlas.bind_group_layout());
        let device_generation = render.generation();

        let quads = self.args.demo_quads.map_or_else(Vec::new, |n| {
            let quads = demo_quads(n, 4000.0);
            tracing::info!(count = quads.len(), "สร้างสี่เหลี่ยมทดสอบ");
            quads
        });

        self.gfx = Some(Gfx {
            window,
            render,
            pipeline,
            atlas,
            textures,
            egui_ctx,
            egui_winit,
            egui_renderer,
            device_generation,
            egui_wake: None,
            quads,
            board_thumbs: Vec::new(),
            // เริ่มที่กลาง world ของ demo เพื่อให้เห็นสี่เหลี่ยมทันทีที่เปิด
            camera: Camera::new(Vec2::splat(2000.0), 0.25),
            canvas: CanvasRect::full(size.width, size.height),
            cursor: Vec2::ZERO,
            panning: false,
        });
        self.queue_initial_files();
        Ok(())
    }

    fn redraw(&mut self) -> Option<RedrawReason> {
        // GPU หน่วยความจำเต็ม — ทิ้ง cache ก่อนทำอย่างอื่น (docs/04 §7)
        if self.gfx.as_mut().is_some_and(|g| g.render.take_oom()) {
            // TODO(P1-6): cache.emergency_evict() ทิ้ง T2 แล้ว T1
            // TODO(P4-2): autosave ทันทีถ้ายังไม่พอ
            tracing::error!("GPU หน่วยความจำเต็ม — ยังไม่มี cache ให้ทิ้งใน P0");
        }

        let frame_start = std::time::Instant::now();

        // ไฟล์ที่ลากเข้ามาในรอบ event ที่ผ่านมา — ส่งเป็นชุดเดียวเพื่อจับเวลาได้ถูก
        if !self.pending_drops.is_empty() {
            let batch = std::mem::take(&mut self.pending_drops);
            self.submit_dropped(batch);
        }

        // เก็บผล decode ที่เสร็จแล้วก่อนวาด (ไม่บล็อก)
        self.drain_decode_results();

        let status = self.gfx.as_mut()?.render.acquire_frame();

        let frame = match status {
            FrameStatus::Ready(frame) => frame,
            // หน้าต่างถูกบัง/driver ไม่ว่าง — หลับต่อ ห้ามขอวาดใหม่ (I-1)
            FrameStatus::Skip => return None,
            // เพิ่ง configure surface ใหม่ — ต้องวาดอีกรอบ
            FrameStatus::Recovered => return Some(RedrawReason::SurfaceRecovery),
            // device หายทั้งก้อน — สร้างใหม่ทั้งชุดแล้วค่อยวาดเฟรมหน้า
            FrameStatus::DeviceLost => return self.recover_device(),
        };

        // แยก borrow ของ gfx กับ shell ออกจากกัน (ทั้งคู่เป็นฟิลด์ของ self)
        let Self {
            gfx,
            shell,
            assets,
            cache_stats_rx,
            loading,
            ..
        } = self;
        let gfx = gfx.as_mut()?;
        debug_assert_eq!(
            gfx.device_generation,
            gfx.render.generation(),
            "resource ยังผูกกับ device รุ่นเก่าอยู่ — ต้องสร้างใหม่หลังกู้"
        );
        shell.frames_drawn += 1;

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // ---- UI pass ----
        let raw_input = gfx.egui_winit.take_egui_input(&gfx.window);
        shell.item_count = gfx.quads.len();
        shell.zoom = gfx.camera.zoom();
        shell.vram_used = gfx.textures.budget().used();
        shell.vram_limit = gfx.textures.budget().limit();
        if let Some(assets) = assets.as_ref() {
            let (used, limit) = assets.pool.ram_usage();
            shell.ram_used = used;
            shell.ram_limit = limit;
            let stats = assets.pool.stats();
            shell.decode_queued = assets.pool.queued();
            shell.decode_cancelled = stats.cancelled;
            // ★ อ่านจากสถิติของ pool ตอนวาดเท่านั้น — ไม่มี timer ไม่มี polling
            //   เฟรมเกิดขึ้นอยู่แล้วทุกครั้งที่ worker ทำงานเสร็จแล้วปลุก UI (I-1)
            shell.loading = loading.update(stats);
        }
        if let Some(rx) = cache_stats_rx.as_ref()
            && let Ok(stats) = rx.try_recv()
        {
            shell.cache_thumbs = stats.thumb_count;
            shell.cache_bytes = stats.size_bytes;
        }
        let mut canvas_points = egui::Rect::NOTHING;
        let full_output = gfx.egui_ctx.run_ui(raw_input, |ui| {
            canvas_points = crate::shell::draw_in_ui(ui, shell, |ui| {
                // ช่องกลางคือ canvas — ภาพวาดด้วย wgpu ใต้ egui อีกที
                // ตรงนี้แค่จองพื้นที่ไว้ P2 จะใส่ hit-test/tool overlay
                ui.allocate_space(ui.available_size());
            });
        });
        gfx.egui_winit
            .handle_platform_output(&gfx.window, full_output.platform_output);

        let (width, height) = {
            let config = gfx.render.config();
            (config.width, config.height)
        };

        // ★ กรอบ canvas จริงจาก egui — ใช้ทั้งตอนวาดและตอนแปลงพิกัดเมาส์
        //   ถ้าใช้ขนาดหน้าต่างทั้งบานแทน จุดกึ่งกลางกล้องจะไปตกกลาง *หน้าต่าง*
        //   ซึ่งเยื้องจากกลางช่อง canvas ไปทางซ้ายบน แล้วภาพส่วนหนึ่งจะไปอยู่ใต้ panel
        gfx.canvas =
            CanvasRect::from_points(canvas_points, full_output.pixels_per_point, width, height);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let jobs = gfx
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, delta) in &full_output.textures_delta.set {
            gfx.egui_renderer
                .update_texture(gfx.render.device(), gfx.render.queue(), *id, delta);
        }

        let mut encoder =
            gfx.render
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("refx-frame"),
                });
        let extra = gfx.egui_renderer.update_buffers(
            gfx.render.device(),
            gfx.render.queue(),
            &mut encoder,
            &jobs,
            &screen,
        );

        {
            // docs/04 §2: pass เดียว — clear แล้ววาด egui ทับ
            // (P0-6 จะแทรก instanced quad ระหว่างสองขั้นนี้)
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("refx-main-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // egui-wgpu 0.34 ต้องการ RenderPass<'static>
            let mut pass = pass.forget_lifetime();

            // [1] ภาพทั้งหมด (instanced quad) — วาดก่อน UI เสมอ
            if !gfx.quads.is_empty() {
                // ★ จำกัดการวาดไว้ในช่อง canvas เท่านั้น ไม่ให้ล้นไปใต้ panel
                //   egui ตั้ง viewport กลับเป็นเต็มจอเองตอนเริ่ม render() จึงไม่ต้องคืนค่า
                pass.set_viewport(
                    gfx.canvas.min.x,
                    gfx.canvas.min.y,
                    gfx.canvas.size.x,
                    gfx.canvas.size.y,
                    0.0,
                    1.0,
                );
                let viewport = gfx.canvas.size;
                gfx.pipeline.set_camera(
                    gfx.render.queue(),
                    CameraUniform::from_affine(gfx.camera.to_clip_affine(viewport)),
                );
                gfx.pipeline.draw(
                    gfx.render.queue(),
                    &mut pass,
                    gfx.atlas.bind_group(),
                    &gfx.quads,
                );
            }

            // [2] UI chrome ทับข้างบน
            gfx.egui_renderer.render(&mut pass, &jobs, &screen);
        }

        gfx.render
            .queue()
            .submit(extra.into_iter().chain(std::iter::once(encoder.finish())));
        frame.present();

        for id in &full_output.textures_delta.free {
            gfx.egui_renderer.free_texture(id);
        }

        // ---- โหมด benchmark: วัด frame time ต่อเนื่อง (P0-6) ----
        // จงใจฝืน I-1 เพราะต้องวาดต่อเนื่องถึงจะวัด fps ได้
        // ใช้เฉพาะตอนสั่ง --bench-seconds เท่านั้น การใช้งานปกติไม่แตะเส้นทางนี้
        if self.args.bench_seconds.is_some() && !self.bench_done {
            self.bench_start.get_or_insert_with(std::time::Instant::now);
            self.stats
                .record(frame_start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64);

            if self.bench_running() {
                return Some(RedrawReason::Animation);
            }
            self.bench_done = true;
            let (quads, present) = self
                .gfx
                .as_ref()
                .map_or((0, wgpu::PresentMode::AutoVsync), |g| {
                    (g.quads.len(), g.render.present_mode())
                });
            self.stats.report(quads, present);
            return None;
        }

        // โหมดทดสอบ device lost เท่านั้น — build ปกติคืน false เสมอ
        // (จงใจฝืน I-1 เพราะต้องนับเฟรมให้ถึง N โดยไม่ต้องขยับเมาส์)
        if gfx.render.wants_forced_frames() {
            return Some(RedrawReason::Animation);
        }

        // ★ I-1: ขอเฟรมถัดไปเฉพาะตอน egui บอกว่ามีอะไรค้างจริง
        //   ตอน idle egui คืน Duration::MAX → หลับสนิท
        let repaint_delay = full_output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(std::time::Duration::MAX, |v| v.repaint_delay);

        if repaint_delay.is_zero() {
            // ต้องวาดต่อทันที (มี animation กำลังเล่น)
            gfx.egui_wake = None;
            return Some(RedrawReason::EguiRepaint);
        }

        // delay จำกัด (เคอร์เซอร์กะพริบ ฯลฯ) → นอนรอด้วย WaitUntil ไม่ใช่วาดรัว ๆ
        // Duration::MAX จะ overflow แล้วได้ None = ไม่ต้องปลุกเลย
        gfx.egui_wake = std::time::Instant::now().checked_add(repaint_delay);
        None
    }

    fn wake_deadline(&self) -> Option<std::time::Instant> {
        let gfx = self.gfx.as_ref()?;
        // เอาเวลาที่ใกล้ที่สุดของทั้งสองแหล่ง (egui กับตัวจำลอง device lost)
        match (gfx.egui_wake, gfx.render.forced_lost_deadline()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, b) => b,
        }
    }

    fn on_wake(&mut self) -> Option<RedrawReason> {
        let gfx = self.gfx.as_mut()?;

        // ตัวจำลอง device lost ตามเวลา — เคลียร์นาฬิกาในตัวแล้ว
        if gfx.render.fire_forced_device_lost() {
            // ยังไม่กู้ตรงนี้ ปล่อยให้ acquire_frame เห็น DeviceLost แล้วไปทาง
            // recover_device() เส้นทางเดียวกับ device lost ของจริงทุกประการ
            return Some(RedrawReason::SurfaceRecovery);
        }

        // ถึงเวลาที่ egui ขอไว้
        if gfx
            .egui_wake
            .is_some_and(|at| std::time::Instant::now() >= at)
        {
            gfx.egui_wake = None;
            return Some(RedrawReason::EguiRepaint);
        }

        None
    }

    fn on_input(&mut self, event: &WindowEvent) -> bool {
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
        };

        // egui ได้เห็น event ก่อนเสมอ และมีสิทธิ์ "กิน" มัน
        // (RedrawRequested ถูก refx-platform คัดออกไปแล้ว จึงไม่มีลูปเลี้ยงตัวเอง)
        let response = gfx.egui_winit.on_window_event(&gfx.window, event);
        let mut needs_redraw = response.repaint;

        // ★ ห้ามใช้ `response.consumed` เดี่ยว ๆ เป็นตัวตัดสิน — วัดแล้วว่าไม่ได้
        //
        //   egui ถือว่า `CentralPanel` เป็นพื้นที่ของตัวเอง และเพราะ panel นั้นกิน
        //   root rect ที่เหลือจนหมด `is_pointer_over_egui()` จึงเป็น **true ทุกจุด
        //   บน canvas** → `consumed = true` เสมอ → เดิม pan/zoom ไม่เคยทำงานเลย
        //   (ยืนยันด้วย log จริง: consumed=true over_egui=true using=false)
        //
        //   แยกสองกรณีที่ต่างกันจริง ๆ แทน:
        //     * egui **กำลังใช้** pointer อยู่ (กดปุ่ม/ลาก slider) → เป็นของ egui
        //     * แค่ hover อยู่เหนือช่อง canvas → เป็นของเรา
        //
        //   TODO(P2): พอมี tool overlay เป็น widget จริงในช่อง canvas ให้เปลี่ยนไป
        //   ใช้ `ui.allocate_response(.., Sense::click_and_drag())` แล้วขับกล้อง
        //   จาก response นั้นแทน เพื่อให้ egui เป็นคนตัดสินให้ทั้งหมด
        let egui_owns_pointer = gfx.egui_ctx.egui_is_using_pointer()
            || (response.consumed && !gfx.canvas.contains(gfx.cursor));
        if egui_owns_pointer {
            return needs_redraw;
        }

        match event {
            // ★ ลากไฟล์เข้ามา — เส้นทางหลักที่ผู้ใช้เอาภาพเข้าโปรแกรม (P1-8)
            WindowEvent::DroppedFile(path) => {
                // winit ส่งมาทีละไฟล์ รวมเป็นชุดเดียวถ้ามาติด ๆ กัน
                self.pending_drops.push(path.clone());
                needs_redraw = true;
            }

            WindowEvent::CursorMoved { position, .. } => {
                let next = Vec2::new(position.x as f32, position.y as f32);
                if gfx.panning {
                    gfx.camera.pan_by_screen_delta(next - gfx.cursor);
                    needs_redraw = true;
                }
                gfx.cursor = next;
            }

            WindowEvent::MouseInput { state, button, .. } => {
                // ลากด้วยปุ่มกลาง หรือปุ่มซ้าย (P2 จะแยกปุ่มซ้ายไปทำ select)
                if matches!(
                    button,
                    winit::event::MouseButton::Middle | winit::event::MouseButton::Left
                ) {
                    gfx.panning = state.is_pressed();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                // แปลง delta สองแบบของ winit ให้เป็น "จำนวนคลิกของล้อ"
                let notches = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => *y,
                    // touchpad ส่งเป็นพิกเซล — หารให้ได้สเกลใกล้เคียงล้อเมาส์
                    winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 50.0,
                };
                if notches.is_finite() && notches != 0.0 {
                    // เลขชี้กำลังทำให้ซูมรู้สึกเท่ากันทุกระดับ
                    // (ถ้าบวก/ลบตรง ๆ ตอนซูมเข้ามาก ๆ จะกระโดดแรงจนเวียนหัว)
                    let factor = 1.1f32.powf(notches);
                    // ★ ต้องใช้กรอบเดียวกับตอนวาด (ช่อง canvas ไม่ใช่ทั้งหน้าต่าง)
                    //   ไม่งั้นจุดใต้เคอร์เซอร์จะเลื่อนตอนซูม ซึ่งเป็นข้อกำหนดหลักของ P0-7
                    gfx.camera.zoom_at_screen(
                        gfx.canvas.to_local(gfx.cursor),
                        gfx.canvas.size,
                        factor,
                    );
                    needs_redraw = true;
                }
            }

            _ => {}
        }

        needs_redraw
    }

    fn on_resize(&mut self, width: u32, height: u32) {
        if let Some(gfx) = self.gfx.as_mut() {
            gfx.render.resize(width, height);
        }
    }
}

/// เปิดหน้าต่างแล้วรันจนผู้ใช้ปิดโปรแกรม
///
/// `cache_db` คือที่อยู่ของ `cache.sqlite`
pub fn run(
    args: AppArgs,
    cache_db: &std::path::Path,
) -> Result<(), refx_platform::window::RunError<DeviceError>> {
    let mut app = RefxApp::new(args);
    app.start_assets(cache_db);
    refx_platform::window::run(
        app,
        WindowConfig {
            title: "RefX".to_owned(),
            ..WindowConfig::default()
        },
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use refx_asset::pool::PoolStatsSnapshot;

    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    /// ★ `set_viewport` ที่ล้นขอบ attachment คือ validation error ของ wgpu
    /// ซึ่งทำให้ **ทั้งเฟรมหายไป** ไม่ใช่แค่ภาพเยื้อง จึงต้องตัดกรอบเสมอ
    #[test]
    fn canvas_rect_never_escapes_the_surface() {
        let (sw, sh) = (1280u32, 800u32);
        for r in [
            rect(200.0, 60.0, 900.0, 700.0),    // ปกติ
            rect(-50.0, -50.0, 2000.0, 2000.0), // ล้นทุกด้าน
            rect(1400.0, 900.0, 100.0, 100.0),  // อยู่นอกจอทั้งก้อน
            egui::Rect::NOTHING,                // ค่าที่ยังไม่ถูกเติมในเฟรมแรก
        ] {
            let c = CanvasRect::from_points(r, 1.0, sw, sh);
            assert!(c.min.x >= 0.0 && c.min.y >= 0.0, "{r:?} → {c:?}");
            assert!(c.size.x >= 1.0 && c.size.y >= 1.0, "viewport ขนาด 0: {c:?}");
            assert!(
                c.min.x + c.size.x <= sw as f32 && c.min.y + c.size.y <= sh as f32,
                "ล้นขอบ surface: {c:?}"
            );
        }
    }

    /// egui ให้ rect มาเป็น point — ต้องคูณ scale ของจอก่อนใช้เป็น physical pixel
    #[test]
    fn points_are_scaled_to_physical_pixels() {
        let c = CanvasRect::from_points(rect(100.0, 50.0, 400.0, 300.0), 2.0, 2560, 1600);
        assert_eq!(c.min, Vec2::new(200.0, 100.0));
        assert_eq!(c.size, Vec2::new(800.0, 600.0));
    }

    /// ค่า scale ที่พังต้องไม่ทำให้ viewport กลายเป็น NaN แล้วทั้งเฟรมหาย (I-4)
    #[test]
    fn broken_scale_falls_back_instead_of_producing_nan() {
        for scale in [f32::NAN, 0.0, -1.0, f32::INFINITY] {
            let c = CanvasRect::from_points(rect(10.0, 10.0, 100.0, 100.0), scale, 1280, 800);
            assert!(
                c.min.is_finite() && c.size.is_finite(),
                "scale {scale}: {c:?}"
            );
            assert!(c.size.x >= 1.0 && c.size.y >= 1.0);
        }
    }

    /// ★ ตัวตัดสินว่า event ของเมาส์เป็นของ canvas หรือของ egui
    ///
    /// ถ้าข้อนี้ผิด pan/zoom จะไม่ทำงาน (เคยเป็นมาแล้ว) หรือแย่งปุ่มบน UI ไป
    #[test]
    fn contains_marks_only_points_inside_the_canvas() {
        let c = CanvasRect::from_points(rect(200.0, 60.0, 800.0, 700.0), 1.0, 1280, 800);

        assert!(c.contains(Vec2::new(600.0, 400.0)), "กลาง canvas");
        assert!(c.contains(Vec2::new(200.0, 60.0)), "มุมซ้ายบนนับเป็นข้างใน");

        assert!(!c.contains(Vec2::new(100.0, 400.0)), "อยู่บน Library");
        assert!(!c.contains(Vec2::new(1100.0, 400.0)), "อยู่บน Inspector");
        assert!(!c.contains(Vec2::new(600.0, 30.0)), "อยู่บน toolbar");
        assert!(!c.contains(Vec2::new(600.0, 780.0)), "อยู่บน status bar");
        assert!(!c.contains(Vec2::new(1000.0, 760.0)), "มุมขวาล่างนับเป็นข้างนอก");
    }

    // ---------- ตัวนับความคืบหน้า (docs/05 §6 เงื่อนไขข้อ 3) ----------

    fn stats(submitted: u64, completed: u64, cancelled: u64, failed: u64) -> PoolStatsSnapshot {
        PoolStatsSnapshot {
            submitted,
            completed,
            cancelled,
            failed,
            timed_out: 0,
        }
    }

    #[test]
    fn no_work_means_no_progress_shown() {
        let mut tracker = LoadTracker::default();
        assert_eq!(tracker.update(stats(0, 0, 0, 0)), None);
        // โหลดจบพอดี — แถบต้องหายไป ไม่ค้างที่ 100%
        assert_eq!(tracker.update(stats(100, 100, 0, 0)), None);
    }

    #[test]
    fn progress_counts_up_during_a_batch() {
        let mut tracker = LoadTracker::default();
        assert_eq!(
            tracker.update(stats(1000, 312, 0, 0)),
            Some(LoadProgress {
                done: 312,
                total: 1000
            })
        );
        assert_eq!(
            tracker
                .update(stats(1000, 312, 0, 0))
                .map(LoadProgress::label),
            Some("กำลังโหลด 312 / 1000".to_owned()),
            "รูปแบบต้องตรงกับ docs/05 §6"
        );
    }

    /// งานที่ถูกยกเลิก/ล้มเหลวก็ไม่ค้างคิวแล้ว ต้องนับเป็น "จบ" ด้วย
    ///
    /// ไม่งั้นโฟลเดอร์ที่มีไฟล์เสียปนอยู่จะค้างที่ "997 / 1000" ตลอดไป
    /// ซึ่งอ่านได้ว่า "โปรแกรมแฮงก์"
    #[test]
    fn cancelled_and_failed_count_as_finished() {
        let mut tracker = LoadTracker::default();
        assert_eq!(
            tracker.update(stats(500, 400, 60, 40)),
            None,
            "400+60+40 = 500 = จบครบแล้ว"
        );
    }

    /// ★ ลากชุดที่สองเข้ามาต้องเริ่มนับใหม่จาก 0 ไม่ใช่ต่อยอดจากชุดแรก
    #[test]
    fn a_second_batch_starts_counting_from_zero() {
        let mut tracker = LoadTracker::default();
        tracker.update(stats(100, 40, 0, 0));
        assert_eq!(tracker.update(stats(100, 100, 0, 0)), None); // ชุดแรกจบ

        assert_eq!(
            tracker.update(stats(150, 100, 0, 0)),
            Some(LoadProgress { done: 0, total: 50 }),
            "ชุดที่สองต้องขึ้น 0 / 50 ไม่ใช่ 100 / 150"
        );
        assert_eq!(
            tracker.update(stats(150, 130, 0, 0)),
            Some(LoadProgress {
                done: 30,
                total: 50
            })
        );
    }

    /// ลากเพิ่มระหว่างที่ชุดเดิมยังโหลดไม่เสร็จ — ยอดรวมต้องโตตาม
    #[test]
    fn dropping_more_while_loading_grows_the_total() {
        let mut tracker = LoadTracker::default();
        tracker.update(stats(100, 30, 0, 0));
        assert_eq!(
            tracker.update(stats(180, 30, 0, 0)),
            Some(LoadProgress {
                done: 30,
                total: 180
            })
        );
    }

    /// snapshot อ่านตัวนับหลายตัวแบบไม่ atomic — "จบเกินที่ส่ง" ชั่วขณะเกิดได้จริง
    /// ต้องไม่ underflow และต้องไม่โชว์ค่าเพี้ยน
    #[test]
    fn inconsistent_snapshot_never_underflows() {
        let mut tracker = LoadTracker::default();
        assert_eq!(tracker.update(stats(10, 12, 0, 0)), None);

        // และสัดส่วนต้องไม่ทะลุ 100% ไม่ว่าตัวเลขจะเพี้ยนแค่ไหน
        let odd = LoadProgress {
            done: 99,
            total: 10,
        };
        assert!((odd.fraction() - 1.0).abs() < f32::EPSILON);
        assert!((LoadProgress { done: 0, total: 0 }).fraction() > 0.0);
    }

    /// เคอร์เซอร์ที่กลางช่อง canvas ต้องแปลงเป็นกลางกรอบของกล้องพอดี
    ///
    /// นี่คือสิ่งที่ทำให้ "ซูมแล้วจุดใต้เคอร์เซอร์ไม่ขยับ" ยังจริงอยู่
    /// แม้ canvas จะไม่ได้อยู่กลางหน้าต่าง
    #[test]
    fn cursor_at_canvas_centre_maps_to_camera_centre() {
        let c = CanvasRect::from_points(rect(200.0, 60.0, 800.0, 700.0), 1.0, 1280, 800);
        let cursor = c.min + c.size * 0.5;
        assert_eq!(c.to_local(cursor), c.size * 0.5);

        // จุดกึ่งกลางกล้องต้องตกลงตรงนั้นพอดี
        let camera = Camera::new(Vec2::new(2000.0, 2000.0), 0.25);
        let on_screen = camera.world_to_screen(camera.center(), c.size);
        assert_eq!(on_screen, c.to_local(cursor));
    }
}
