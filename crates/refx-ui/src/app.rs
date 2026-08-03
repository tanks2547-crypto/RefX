//! ต่อสาย `refx-platform` (หน้าต่าง) เข้ากับ `refx-render` (GPU) แล้วเอา egui วางทับ
//!
//! นี่คือที่เดียวที่ทั้งสามโลกมาเจอกัน — winit, wgpu, egui อยู่บน device เดียวกัน
//!
//! spec: docs/04-rendering.md §1, §8

use std::sync::Arc;

use glam::Vec2;
use refx_asset::cache::{CacheStats, IoRequest, IoThread};
use refx_asset::pool::DecodePool;
use refx_core::arena::ItemId;
use refx_core::board::{AssetRef, Board, ImageFormat, Item, ItemCanvas, ItemKind};
use refx_core::command::{AddItems, History};
use refx_core::geom::Rect as WorldRect;
use refx_core::interact::{CanvasButton, CanvasContext, CanvasEvent, Modifiers, SelectTool};
use refx_core::selection::Selection;
use refx_core::spatial::SpatialIndex;
use refx_core::view::Camera;

use crate::shell::LoadProgress;
use crate::text::{self, Key, Lang, Template};
use refx_platform::redraw::RedrawReason;
use refx_platform::window::{AppDelegate, WindowConfig};
use refx_render::atlas::{AtlasError, AtlasSlot, ThumbnailAtlas, layers_for_budget};
use refx_render::device::{DeviceError, FrameStatus, RenderContext, RenderOptions};
use refx_render::instance::QuadInstance;
use refx_render::pipeline::DrawBatch;
use refx_render::pipeline::{CameraUniform, QuadPipeline};
use refx_render::texture::TextureAllocator;
use refx_render::working::{WorkingCache, WorkingKey};
use winit::event::WindowEvent;
use winit::keyboard::ModifiersState;
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
    /// บังคับภาษาของ UI แทนค่าที่อ่านได้จาก OS
    ///
    /// `None` = ใช้ locale ของเครื่อง (docs/03 §0 ข้อ 3)
    /// มีไว้ให้ตรวจงานแปลได้โดยไม่ต้องไปเปลี่ยนภาษาของทั้งเครื่อง
    /// และเป็นกลไกเดียวกับที่ Settings จะใช้ตอน P5-3
    pub lang: Option<Lang>,
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
            tracing::warn!("no frames were measured");
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
            "frame time results"
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

/// ปุ่มนี้คือ "วางจาก clipboard" (`Ctrl+V`) หรือไม่
///
/// แยกเป็นฟังก์ชันบริสุทธิ์เพื่อ **ทดสอบได้จริงโดยไม่ต้องเปิดหน้าต่าง** — คีย์ลัด
/// ที่พังเงียบ ๆ เป็นบั๊กที่ไม่มีเทสต์ไหนจับได้ถ้าตรรกะฝังอยู่ใน `match` ของ event
///
/// ดู **logical key** ไม่ใช่ตำแหน่งปุ่ม เพื่อให้ layout ที่ไม่ใช่ QWERTY ยังวางถูกปุ่ม
/// (Dvorak กด `V` ที่ตำแหน่งอื่น) — บาง compositor บน X11 ส่ง `Ctrl+V` มาเป็น
/// อักขระควบคุม `SYN` (U+0016) จึงรับตัวนั้นด้วย
///
/// TODO(P6): macOS ใช้ `Cmd+V` ต้องรับ `super_key()` เพิ่มตอนทำ P6
/// ผู้ใช้ขออะไรกับประวัติ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryRequest {
    /// Ctrl+Z
    Undo,
    /// Ctrl+Y หรือ Ctrl+Shift+Z
    Redo,
}

/// แปลงปุ่มที่กดเป็นคำขอกับประวัติ
///
/// ★ รับ **Ctrl+Shift+Z เป็น redo ด้วย** ไม่ใช่แค่ Ctrl+Y — คนจำนวนมากใช้อันนั้น
/// (ติดมาจาก Photoshop/Illustrator) ถ้าไม่รับ เขาจะคิดว่า redo ไม่มีในโปรแกรมนี้
fn history_shortcut(
    key: &winit::keyboard::Key,
    modifiers: ModifiersState,
) -> Option<HistoryRequest> {
    if !modifiers.control_key() {
        return None;
    }
    let winit::keyboard::Key::Character(text) = key else {
        return None;
    };
    // ปุ่มควบคุมบางระบบส่งมาเป็นอักขระ control (Ctrl+Z = 0x1A, Ctrl+Y = 0x19)
    if text.eq_ignore_ascii_case("z") || text.as_str() == "\u{1a}" {
        return Some(if modifiers.shift_key() {
            HistoryRequest::Redo
        } else {
            HistoryRequest::Undo
        });
    }
    if text.eq_ignore_ascii_case("y") || text.as_str() == "\u{19}" {
        return Some(HistoryRequest::Redo);
    }
    None
}

fn is_paste(key: &winit::keyboard::Key, modifiers: ModifiersState) -> bool {
    if !modifiers.control_key() {
        return false;
    }
    match key {
        winit::keyboard::Key::Character(text) => text.eq_ignore_ascii_case("v") || text == "\u{16}",
        _ => false,
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
    /// ★ instance ที่ส่งให้ GPU — **ผลลัพธ์ที่คำนวณจาก `board` ล้วน ๆ**
    ///
    /// ไม่ใช่แหล่งความจริงอีกต่อไป: ทุกอย่างที่เขียนลงนี้ต้องมาจาก
    /// [`RefxApp::rebuild_quads`] เท่านั้น ห้ามมีใครแก้ตรง ๆ
    /// (เดิมตำแหน่งภาพถูกคำนวณสด ๆ ตอน ingest แล้วเก็บไว้ที่นี่ที่เดียว)
    quads: Vec<QuadInstance>,
    /// ★ เอกสารของผู้ใช้ — แหล่งความจริงเดียวของเรขาคณิตและลำดับชั้น
    board: Board,
    /// undo/redo — ทุกการแก้ `board` ผ่านที่นี่ (I-3)
    history: History,
    /// index สำหรับ hit-test/culling — ตามหลัง `board` เสมอ
    index: SpatialIndex,
    /// สถานะฝั่ง render ต่อ item (atlas slot, thumbnail, ต้นทาง)
    render_state: std::collections::HashMap<ItemId, ItemRender>,
    /// ★ สิ่งที่ผู้ใช้เลือกอยู่ — **อยู่นอก `Board` โดยตั้งใจ** (docs/02 §2.9)
    ///
    /// ไม่ persist ไม่ undo ไม่ทำให้เอกสาร dirty — คลิกดูภาพเฉย ๆ ต้องไม่ทำให้
    /// ผู้ใช้โดนถาม "บันทึกไหม" ตอนปิด
    selection: Selection,
    /// เครื่องสถานะของการเลือก (คลิก · Ctrl+คลิก · ลากกรอบ)
    select_tool: SelectTool,
    /// กรอบ rubber-band ที่กำลังลากอยู่ (world) — `None` = ไม่ต้องวาด
    rubber_band: Option<WorldRect>,
    /// ★ thumbnail ของทุก item บน board เก็บไว้เติม atlas กลับหลังกู้ device
    ///
    /// docs/04 §4: ถ้าไม่เติมกลับ ผู้ใช้จะเห็น **board ว่างเปล่า** หลัง driver อัปเดต
    /// ซึ่งจากมุมเขาแยกไม่ออกจาก "งานหาย" แล้วงานที่ P0-5 ทำมาทั้งหมดเสียเปล่า
    ///
    /// ★ working texture ชั้น B — texture แยกต่อภาพตอนซูมเข้า (docs/04 §4)
    ///
    /// ครึ่งบนของงบ VRAM · อีกครึ่งเป็นของ atlas
    working: WorkingCache,
    /// คีย์ที่สั่ง decode ไปแล้วแต่ยังไม่ได้ผลกลับ
    ///
    /// ★ ถ้าไม่มีตัวนี้ ทุกเฟรมระหว่างซูมจะสั่งงานเดิมซ้ำจนคิวท่วมและเผา CPU ทิ้ง
    working_pending: std::collections::HashSet<WorkingKey>,
    /// batch ที่จะวาดเฟรมนี้ — เก็บไว้เป็นฟิลด์เพื่อไม่ต้องจองใหม่ทุกเฟรม
    working_quads: Vec<(WorkingKey, QuadInstance)>,
    /// กล้อง pan/zoom (P0-7)
    camera: Camera,
    /// ปุ่มค้าง (Ctrl/Shift/Alt) ล่าสุด — winit ส่งมาแยก event ไม่ได้แนบมากับปุ่ม
    modifiers: ModifiersState,
    /// ★ กรอบของช่อง canvas จริง (physical pixel) — **ไม่ใช่ขนาดหน้าต่างทั้งบาน**
    ///
    /// egui เป็นคนบอกว่าช่องกลางอยู่ตรงไหนหลังหัก panel ซ้าย/ขวา/บน/ล่างออกแล้ว
    /// ค่านี้ถูกใช้สองที่และ **ต้องเป็นค่าเดียวกัน** ไม่งั้นภาพกับเมาส์จะไม่ตรงกัน:
    ///   1. `set_viewport` ของ render pass + กรอบอ้างอิงของกล้องตอนวาด
    ///   2. แปลงพิกัดเคอร์เซอร์ตอน zoom เข้าหาเมาส์ (P0-7)
    canvas: CanvasRect,
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

/// resource ทุกชิ้นที่ **ผูกกับ GPU device รุ่นหนึ่ง** — ตายพร้อม device เสมอ
///
/// ★ **ทำไมต้องรวมเป็นชุดเดียว:** P1-7 เพิ่ม working texture เข้ามาทีหลัง แล้ว
/// `recover_device()` ไม่ได้สร้างมันใหม่ — หลังกู้ device เสร็จ `WorkingCache`
/// ยังถือ texture/bind group ของ device ที่ **ตายไปแล้ว** ทั้งชุด แล้วเอาไปวาดต่อ
/// (validation error ทุกเฟรม) ส่วน `working_pending` ก็ยังจำคีย์เดิมไว้
/// ทำให้ภาพคมไม่มีวันถูกขอใหม่อีกเลย
///
/// ตอนนี้ทั้ง `window_ready` และ `recover_device` สร้างผ่าน [`DeviceBound::build`]
/// **จุดเดียวกัน** และรับด้วยการ destructure — ถ้ามีคนเพิ่ม resource ใหม่เข้ามา
/// ทั้งสองที่จะคอมไพล์ไม่ผ่านจนกว่าจะจัดการให้ครบทั้งคู่
/// (หลักการเดียวกับ `GpuStack` ใน `refx-render::device`)
struct DeviceBound {
    textures: TextureAllocator,
    atlas: ThumbnailAtlas,
    pipeline: QuadPipeline,
    working: WorkingCache,
}

impl DeviceBound {
    /// สร้าง resource ที่ผูกกับ device ปัจจุบันทั้งชุด
    fn build(render: &RenderContext) -> Result<Self, refx_render::texture::VramError> {
        // ★ ทางเดียวที่สร้าง texture ได้ (I-6) — atlas ต้องขอผ่านตัวนี้
        let textures = TextureAllocator::new(render.capabilities());
        // atlas ขอได้ไม่เกินครึ่งงบ VRAM — อีกครึ่งเผื่อ working texture (P1-7)
        // ★ ตัวเลขนี้เป็น **เพดาน** ไม่ใช่การจองจริง — atlas จองทีละ layer
        //   ตอนมีภาพเข้ามาจริง เปิดโปรแกรมเปล่าจึงกิน VRAM ≈ 0 (docs/05 §2)
        let atlas_budget = textures.budget().limit() as u64 / 2;
        // อีกครึ่งเป็นของ working texture ชั้น B (docs/05 §1)
        let working_budget = textures.budget().limit() / 2;
        let max_layers = layers_for_budget(render.capabilities(), atlas_budget);

        let atlas = ThumbnailAtlas::new(render.device(), &textures, max_layers)?;
        let pipeline =
            QuadPipeline::new(render.device(), render.format(), atlas.bind_group_layout());
        let working = WorkingCache::new(
            render.device(),
            textures.clone(),
            working_budget,
            render.generation(),
        );

        Ok(Self {
            textures,
            atlas,
            pipeline,
            working,
        })
    }
}

/// สถานะ **ฝั่ง render** ของ item หนึ่งใบ — คีย์ด้วย `ItemId` ของ `Board`
///
/// ★ **ห้ามเอาเรขาคณิตกลับมาไว้ที่นี่** (ย้ายแล้ว 3 ส.ค. 2026) ตำแหน่ง/ขนาด/หมุน
/// เป็นของ `ItemCanvas` ใน `Board` ที่เดียว ส่วนที่นี่เก็บเฉพาะของที่ผูกกับ GPU
/// หรือกับการ decode — ถ้าเก็บสองที่ วันหนึ่งจะเพี้ยนจากกันแล้วภาพจะไปอยู่คนละที่
/// กับที่ hit-test คิดว่ามันอยู่
struct ItemRender {
    /// ภาพนี้มาจากไหน
    ///
    /// ★ ต้องเก็บไว้เพราะ working texture ต้อง decode ใหม่จาก **ไฟล์จริง**
    /// ภาพที่วางมาจาก clipboard ไม่มีไฟล์ให้กลับไปอ่าน จึงขอภาพคมกว่าเดิมไม่ได้
    /// (ดู [`RefxApp::plan_working_textures`]) — ชนิดข้อมูลบังคับให้ต้องตัดสินใจ
    /// ตรงนั้น แทนที่จะเผลอส่ง path ว่างเข้าไปแล้วได้ error ทุกครั้งที่ซูม
    source: refx_asset::pool::JobSource,
    /// คีย์ของภาพ (ใช้เป็นคีย์ของ working cache ด้วย)
    hash: refx_asset::hash::ContentHash,
    /// ภาพย่อ 128 px — เก็บไว้เติม atlas กลับหลังกู้ device หรือหลังขยาย atlas
    ///
    /// เก็บ pixel ไว้ใน RAM เลยเพราะ 128×128×4 = 64 KB ต่อภาพ
    /// (1000 ภาพ = 64 MB ซึ่งยังอยู่ในงบ) และเร็วกว่าอ่านกลับจาก sqlite มาก
    thumb: refx_asset::thumb::Thumbnail,
    /// ช่องใน atlas ที่ thumbnail ตัวนี้อยู่
    uv_rect: [f32; 4],
    /// ชั้นใน texture array
    layer: u32,
    /// สีคูณ — เป็นสีเด่นของภาพตอนยังเป็น placeholder
    tint: [f32; 4],
    /// ธงของ quad (PLACEHOLDER ฯลฯ)
    flags: u32,
}

/// สิ่งที่ canvas widget เก็บได้จาก egui ในเฟรมหนึ่ง
///
/// ★ **เก็บไว้ก่อน แล้วค่อยเอาไปประมวลผลหลัง `run_ui` จบ** — ระหว่างอยู่ในคลอเชอร์
/// เรายืม `gfx` แบบอ่านอย่างเดียวเพื่อวาด จึงแก้อะไรไม่ได้ การแยกสองจังหวะแบบนี้
/// ยังทำให้ตรรกะการเลือกทดสอบแยกได้ด้วย (มันอยู่ใน `refx-core` ทั้งก้อน)
#[derive(Debug, Clone, Copy)]
struct CanvasFrameInput {
    /// กรอบของ widget (หน่วย point)
    rect: egui::Rect,
    /// ตำแหน่งเคอร์เซอร์ (point) ถ้าอยู่เหนือ canvas
    pointer: Option<egui::Pos2>,
    /// กดปุ่มซ้ายลงในเฟรมนี้
    primary_pressed: bool,
    /// ปล่อยปุ่มซ้ายในเฟรมนี้
    primary_released: bool,
    /// ปุ่มซ้ายกดค้างอยู่
    primary_down: bool,
    /// ระยะที่ลากด้วยปุ่มกลาง (point)
    pan_delta: egui::Vec2,
    /// จำนวนคลิกของล้อ
    scroll: f32,
    /// Ctrl/Shift ตอนนี้
    modifiers: Modifiers,
}

impl Default for CanvasFrameInput {
    fn default() -> Self {
        Self {
            rect: egui::Rect::NOTHING,
            pointer: None,
            primary_pressed: false,
            primary_released: false,
            primary_down: false,
            pan_delta: egui::Vec2::ZERO,
            scroll: 0.0,
            modifiers: Modifiers::default(),
        }
    }
}

/// สีของกรอบสิ่งที่ถูกเลือกและกรอบ rubber-band
const SELECT_STROKE: egui::Color32 = egui::Color32::from_rgb(120, 190, 255);

impl RefxApp {
    /// ★ canvas เป็น **widget จริงของ egui** — ไม่ใช่การเดาว่า pointer เป็นของใคร
    ///
    /// docs/03 §1 บันทึกทางแก้ชั่วคราวไว้ (`egui_is_using_pointer()` +
    /// `canvas.contains(cursor)`) พร้อมบอกว่าทางที่ถูกคือทำแบบนี้ เพราะ
    /// `CentralPanel` กิน root rect จนหมด `is_pointer_over_egui()` จึงจริงทุกจุด
    /// บน canvas ทำให้ `consumed` ใช้ตัดสินไม่ได้
    ///
    /// พอจองพื้นที่ด้วย `allocate_response` แล้ว **egui เป็นคนจัดลำดับให้เอง**:
    /// คลิกบน toolbar/inspector จะไม่ตกมาถึงเรา เพราะ widget พวกนั้นกิน response ไปก่อน
    fn canvas_widget(
        ui: &mut egui::Ui,
        board: &Board,
        selection: &Selection,
        render_state: &std::collections::HashMap<ItemId, ItemRender>,
        camera: Camera,
        rubber_band: Option<WorldRect>,
    ) -> CanvasFrameInput {
        let response = ui.allocate_response(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;

        // world → point: renderer แปลง world → **physical pixel** ด้วยตัวคูณ `zoom`
        // การวาดทับด้วย egui อยู่ในหน่วย point จึงต้องหารด้วย pixels_per_point
        // ไม่งั้นกรอบที่วาดจะเลื่อนจากภาพทันทีที่จอมี DPI ไม่ใช่ 100%
        let scale = camera.zoom() / ui.ctx().pixels_per_point();
        let centre = camera.center();
        let to_point = |world: Vec2| -> egui::Pos2 {
            let offset = (world - centre) * scale;
            rect.center() + egui::vec2(offset.x, offset.y)
        };

        // ---- วาดกรอบของสิ่งที่ถูกเลือก ----
        let painter = ui.painter_at(rect);
        for id in selection.iter() {
            let Some(item) = board.item(id) else {
                continue;
            };
            if !item.canvas.visible || !render_state.contains_key(&id) {
                continue;
            }
            // ใช้สี่มุมจริง (หมุนแล้ว) ไม่ใช่ AABB — ไม่งั้นภาพที่หมุนจะได้กรอบที่
            // ใหญ่กว่าตัวมันเองอย่างเห็นได้ชัด
            let corners = item.canvas.obb().corners().map(to_point);
            painter.add(egui::Shape::closed_line(
                corners.to_vec(),
                egui::Stroke::new(2.0, SELECT_STROKE),
            ));
        }

        // ---- วาดกรอบ rubber-band ----
        if let Some(band) = rubber_band {
            let band = egui::Rect::from_two_pos(to_point(band.min), to_point(band.max));
            painter.rect_filled(band, 0.0, SELECT_STROKE.gamma_multiply(0.15));
            painter.rect_stroke(
                band,
                0.0,
                egui::Stroke::new(1.0, SELECT_STROKE),
                egui::StrokeKind::Inside,
            );
        }

        let (scroll, modifiers) = ui.ctx().input(|i| {
            (
                i.smooth_scroll_delta.y,
                Modifiers {
                    ctrl: i.modifiers.ctrl || i.modifiers.command,
                    shift: i.modifiers.shift,
                },
            )
        });

        CanvasFrameInput {
            rect,
            // `hover_pos` คืน `None` เมื่อ pointer อยู่เหนือ widget อื่นที่ทับอยู่
            // — นี่คือสิ่งที่ทำให้คลิกบน toolbar ไม่ทะลุมาโดนภาพข้างหลัง
            pointer: response.hover_pos().or_else(|| {
                // ระหว่างลากค้าง เคอร์เซอร์ออกนอก widget ได้ ยังต้องตามให้ทัน
                response
                    .dragged()
                    .then(|| ui.ctx().input(|i| i.pointer.latest_pos()))
                    .flatten()
            }),
            primary_pressed: response.drag_started_by(egui::PointerButton::Primary)
                || ui
                    .ctx()
                    .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
                    && response.hovered(),
            primary_released: response.drag_stopped_by(egui::PointerButton::Primary)
                || (response.clicked() && !response.dragged()),
            primary_down: ui
                .ctx()
                .input(|i| i.pointer.button_down(egui::PointerButton::Primary)),
            pan_delta: if response.dragged_by(egui::PointerButton::Middle) {
                response.drag_delta()
            } else {
                egui::Vec2::ZERO
            },
            scroll: if response.hovered() { scroll } else { 0.0 },
            modifiers,
        }
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
    /// ผู้ใช้กด `Ctrl+V` ในรอบ event ที่ผ่านมา — ส่งงานตอนต้นเฟรมถัดไป
    ///
    /// ไม่อ่าน clipboard ตรงนี้เด็ดขาด: การเปิด clipboard บล็อกได้ (I-2)
    pending_paste: bool,
    /// งานวางที่ส่งไปแล้วแต่ยังไม่ได้ผลกลับ
    ///
    /// คำขอ undo/redo ที่รอทำต้นเฟรมถัดไป
    ///
    /// เป็น `Option` จึงรวบการกดค้างให้เหลือครั้งเดียวต่อเฟรมโดยอัตโนมัติ —
    /// กด Ctrl+Z ค้างแล้วย้อนเรื่อย ๆ ได้ตามที่คนคาดหวัง แต่ไม่ถล่มทั้งสแตกในเฟรมเดียว
    pending_history: Option<HistoryRequest>,
    /// ★ กันการกด `Ctrl+V` รัว ๆ ให้เหลือทีละครั้ง — ภาพจาก clipboard ใหญ่ได้
    /// ระดับ 6000×4000 (96 MB) และ `arboard` จอง RAM ก้อนนั้นก่อนที่เพดานของเรา
    /// จะได้ตรวจ ถ้าปล่อยให้ซ้อนกันสิบใบคือแย่ง RAM กับ Photoshop ตรง ๆ
    paste_in_flight: Option<refx_asset::hash::ContentHash>,
    /// นับครั้งที่วาง — ใช้ทำคีย์ที่ไม่ซ้ำให้แต่ละครั้ง (clipboard ไม่มี path ให้ใช้)
    paste_count: u64,
    /// ชุดล่าสุดที่กำลังรออยู่มาจาก clipboard หรือไม่ (ใช้เลือกข้อความสรุป)
    batch_from_clipboard: bool,
    /// แปลงสถิติสะสมของ pool เป็นความคืบหน้าของงวดปัจจุบัน
    loading: LoadTracker,
    /// ที่มาของงานที่ส่งเข้า pool — ผลลัพธ์กลับมาพร้อม hash เท่านั้น
    ///
    /// working texture ต้อง decode ใหม่จากไฟล์จริง จึงต้องจำที่มาไว้จับคู่
    job_sources:
        std::collections::HashMap<refx_asset::hash::ContentHash, refx_asset::pool::JobSource>,
}

/// ส่วนที่จัดการภาพ — อยู่คนละโลกกับ GPU
///
/// ★ **ลำดับฟิลด์ที่นี่คือลำดับ drop และมันสำคัญจริง ๆ**
/// `IoThread::drop` ปิด sender ของตัวเองแล้ว **join** เธรด IO ซึ่งจะจบก็ต่อเมื่อ
/// sender ทุกใบถูก drop หมด ถ้า `io_tx` (ใบที่ clone ไว้) ยังอยู่ตอนนั้น
/// การ join จะรอตลอดกาล = ปิดหน้าต่างแล้วโปรเซสไม่ตาย ล็อก single-instance
/// ค้าง แล้วเปิดโปรแกรมใหม่ไม่ได้อีกเลย
///
/// ลำดับที่ถูกคือ: worker (ถือ clone ของ `io_tx` คนละใบ) → `io_tx` → `IoThread`
struct Assets {
    pool: DecodePool,
    io_tx: Option<crossbeam_channel::Sender<IoRequest>>,
    /// ต้องถือไว้ให้ IO thread มีชีวิตอยู่ (drop = ปิด thread + ล้าง cache)
    ///
    /// `None` เมื่อเปิด cache ไม่ได้ — โปรแกรมยังใช้งานได้ แค่ decode ใหม่ทุกครั้ง
    _io: Option<IoThread>,
}

impl RefxApp {
    /// สร้างแอปที่ยังไม่ผูกกับหน้าต่าง
    #[must_use]
    pub fn new(args: AppArgs) -> Self {
        let lang = args.lang.unwrap_or_else(Lang::from_system);
        Self {
            gfx: None,
            args,
            stats: FrameStats::default(),
            bench_start: None,
            bench_done: false,
            shell: crate::shell::ShellState {
                // ★ อ่าน locale ของ OS ครั้งเดียวตอนเปิดโปรแกรม (docs/03 §0 ข้อ 3)
                //   ไม่รู้จักภาษา → อังกฤษ · P5-3 จะให้ผู้ใช้เลือกทับได้
                lang,
                ..crate::shell::ShellState::default()
            },
            assets: None,
            cache_stats_rx: None,
            waker: None,
            drop_started: None,
            drop_expected: 0,
            drop_shown: 0,
            drop_reported: true,
            pending_drops: Vec::new(),
            pending_paste: false,
            pending_history: None,
            paste_in_flight: None,
            paste_count: 0,
            batch_from_clipboard: false,
            loading: LoadTracker::default(),
            job_sources: std::collections::HashMap::new(),
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
        if self.assets.is_none() {
            return;
        }
        if paths.is_empty() {
            return;
        }

        // เริ่มจับเวลาชุดใหม่
        self.drop_started = Some(std::time::Instant::now());
        self.drop_expected = paths.len();
        self.drop_shown = 0;
        self.drop_reported = false;
        self.batch_from_clipboard = false;

        let mut submitted = Vec::with_capacity(paths.len());
        for (i, path) in paths.into_iter().enumerate() {
            // hash จาก path ไปก่อน — hash เนื้อไฟล์จริงเกิดบน worker (P1-2)
            // ที่นี่ต้องการแค่คีย์ชั่วคราวไว้จับคู่ผลลัพธ์
            let hash = refx_asset::hash::hash_bytes(path.to_string_lossy().as_bytes());
            let source = refx_asset::pool::JobSource::File(path);
            self.job_sources.insert(hash, source.clone());
            submitted.push(refx_asset::pool::Job {
                hash,
                source,
                // ยังไม่มี layout จริง → เรียงตามลำดับที่ลากเข้ามา
                priority: i as f32,
                cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                target: refx_asset::pool::JobTarget::Thumbnail,
            });
        }
        if let Some(assets) = self.assets.as_ref() {
            for job in submitted {
                assets.pool.submit(job);
            }
        }
        self.shell.status = text::fill(
            self.shell.lang,
            Template::OpeningFiles,
            &[("n", &self.drop_expected.to_string())],
        );
    }

    /// ผู้ใช้กด `Ctrl+V` — ส่งงาน "ไปดูว่ามีอะไรใน clipboard" เข้าคิว
    ///
    /// ★ **ไม่แตะ clipboard บน UI thread เลย** (I-2) การเปิด clipboard รอ OS
    /// ได้นานเป็นวินาทีถ้าโปรแกรมอื่นถือมันค้างอยู่ — worker เป็นคนอ่าน
    ///
    /// ภาพที่ได้กลับมาผ่านเกราะและเพดาน RAM ชุดเดียวกับไฟล์บนดิสก์ทุกประการ (I-4)
    fn submit_paste(&mut self) {
        let Some(assets) = self.assets.as_ref() else {
            return;
        };
        // วางทีละครั้ง — ดูเหตุผลที่ฟิลด์ `paste_in_flight`
        if self.paste_in_flight.is_some() {
            tracing::debug!("the previous paste has not finished — ignoring this one");
            return;
        }

        // clipboard ไม่มี path ให้ทำคีย์ จึงนับครั้งเอา — ต่างคีย์กันทุกครั้งที่วาง
        // (วางภาพเดิมซ้ำ = ผู้ใช้ตั้งใจให้ได้สองใบ ไม่ใช่ให้ทับกัน)
        self.paste_count += 1;
        let hash =
            refx_asset::hash::hash_bytes(format!("clipboard:{}", self.paste_count).as_bytes());
        self.job_sources
            .insert(hash, refx_asset::pool::JobSource::Clipboard);
        self.paste_in_flight = Some(hash);

        assets.pool.submit(refx_asset::pool::Job {
            hash,
            source: refx_asset::pool::JobSource::Clipboard,
            // ผู้ใช้เพิ่งกดปุ่มเมื่อกี้ — ตั้งใจที่สุดในคิว จึงได้ไปก่อน (น้อย = ก่อน)
            priority: 0.0,
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            target: refx_asset::pool::JobTarget::Thumbnail,
        });

        self.drop_started = Some(std::time::Instant::now());
        self.drop_expected = 1;
        self.drop_shown = 0;
        self.drop_reported = false;
        self.batch_from_clipboard = true;
        self.shell.status = text::t(self.shell.lang, Key::ReadingClipboard).to_owned();
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
                tracing::error!(%err, "cannot open the cache — continuing without one");
                self.shell.status = text::t(self.shell.lang, Key::RunningWithoutCache).to_owned();
                (None, None)
            }
        };

        // ★ `refx-ui` เป็นชั้นเดียวที่รู้จักทั้ง OS และ decode pool จึงเป็นคนเสียบ
        //   ของที่ต้องถาม OS ให้ (ARCHITECTURE §2, HANDOFF §2.0) — `refx-asset`
        //   ไม่ depend `refx-platform` แล้ว หลักการเดียวกับ `WakeHandle` กับ winit
        let pool = DecodePool::with_defaults(
            refx_platform::memory::total_ram(),
            std::sync::Arc::new(refx_platform::clipboard::SystemClipboard),
            io_tx.clone(),
        );
        let (used, limit) = pool.ram_usage();
        self.shell.ram_used = used;
        self.shell.ram_limit = limit;

        tracing::info!(
            workers = pool.worker_count(),
            ram_limit_mb = limit / (1 << 20),
            cache = io.is_some(),
            "decode pool started"
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
        let mut ready: Vec<(
            refx_asset::hash::ContentHash,
            Box<refx_asset::working::WorkingImage>,
        )> = Vec::new();
        // ไฟล์ที่พบใน clipboard — ส่งต่อเข้าเส้นทาง drag & drop หลังปล่อย borrow
        let mut pasted_files: Vec<std::path::PathBuf> = Vec::new();
        while let Some(result) = assets.pool.try_recv() {
            // งานวางจบแล้วไม่ว่าผลจะเป็นอะไร — เปิดทางให้กด Ctrl+V ครั้งต่อไปได้
            if self.paste_in_flight == Some(result.hash()) {
                self.paste_in_flight = None;
            }
            match result {
                refx_asset::pool::JobResult::Done {
                    hash,
                    thumb,
                    elapsed,
                } => {
                    tracing::debug!(hash = %hash.short(), ?elapsed, "image decoded");
                    // ไม่รู้จักคีย์ = ไม่มีไฟล์ให้กลับไปอ่าน จึงถือเป็นภาพที่ขอคมกว่านี้
                    // ไม่ได้ (ปลอดภัยกว่าการเดา path แล้วยิง error ทุกครั้งที่ซูม)
                    let source = self
                        .job_sources
                        .get(&hash)
                        .cloned()
                        .unwrap_or(refx_asset::pool::JobSource::Clipboard);
                    done.push((hash, source, thumb));
                }
                refx_asset::pool::JobResult::ClipboardFiles { hash, paths } => {
                    // ก๊อปไฟล์จาก Explorer มาวาง — เดินเส้นทางเดียวกับลากไฟล์เข้ามา
                    // ทั้งเส้น (มี cache, มี EXIF, ขอภาพคมตอนซูมได้)
                    tracing::info!(hash = %hash.short(), count = paths.len(), "pasted a file list from the clipboard");
                    self.job_sources.remove(&hash);
                    pasted_files.extend(paths);
                }
                refx_asset::pool::JobResult::Working {
                    hash,
                    image,
                    elapsed,
                } => {
                    tracing::debug!(
                        hash = %hash.short(), size = image.size, ?elapsed,
                        "working texture ready"
                    );
                    ready.push((hash, image));
                }
                refx_asset::pool::JobResult::Cancelled { .. } => {}
                refx_asset::pool::JobResult::Failed { hash, reason } => {
                    // I-7: ภาพเสียหนึ่งไฟล์ = item ขึ้นสถานะ "โหลดไม่ได้" ไม่ใช่ crash
                    tracing::warn!(hash = %hash.short(), %reason, "cannot open the image");
                    // ★ `reason.to_string()` เป็นอังกฤษสำหรับ log เท่านั้น (docs/03 §0)
                    //   ข้อความของผู้ใช้ประกอบจากฟิลด์ของ error แล้วแปลตามภาษา
                    self.shell.status = text::job_failure(self.shell.lang, &reason);
                }
            }
            finished += 1;
        }

        // ---- working texture ที่ decode เสร็จ → ขึ้น VRAM ----
        if !ready.is_empty()
            && let Some(gfx) = self.gfx.as_mut()
        {
            for (hash, image) in ready {
                let key = WorkingKey {
                    hash: *hash.as_bytes(),
                    size: image.size,
                };
                gfx.working_pending.remove(&key);
                let layout = gfx.atlas.bind_group_layout();
                if let Err(err) = gfx.working.insert(
                    gfx.render.device(),
                    gfx.render.queue(),
                    layout,
                    key,
                    &image.levels,
                ) {
                    // ไม่พอก็ใช้ thumbnail ต่อไป — ภาพยังขึ้น แค่เบลอกว่า
                    tracing::warn!(%err, size = key.size, "cannot upload the working texture");
                }
            }
        }

        // อัดขึ้น atlas แล้ววาง quad ให้เห็นบน canvas
        if !done.is_empty()
            && let Some(gfx) = self.gfx.as_mut()
        {
            for (hash, source, thumb) in done {
                match Self::upload_thumb(gfx, &thumb.pixels) {
                    Ok(slot) => {
                        // จัดเป็นตารางง่าย ๆ ไปก่อน — layout จริงมาใน P2/P3
                        // ★ ตำแหน่งไปอยู่ใน `ItemCanvas` แล้ว ไม่ได้คำนวณลง quad ตรง ๆ
                        let n = u32::try_from(gfx.board.len()).unwrap_or(u32::MAX);
                        let (col, row) = (n % 16, n / 16);
                        let cell = 160.0;
                        // คงอัตราส่วนภาพเดิมไว้ ไม่บีบให้เป็นจัตุรัส
                        let (sw, sh) = (thumb.source_width.max(1), thumb.source_height.max(1));
                        let scale =
                            128.0 / f32::from(u16::try_from(sw.max(sh)).unwrap_or(u16::MAX));
                        let size = Vec2::new(sw as f32 * scale, sh as f32 * scale);
                        // ★ `transform` ของ quad ใช้ **มุมซ้ายบน** ส่วน `ItemCanvas::pos`
                        //   คือ **จุดกึ่งกลาง** (docs/02 §2.1) — บวกครึ่งขนาดตอนแปลง
                        //   ถ้าลืมข้อนี้ ภาพทุกใบจะเลื่อนไปครึ่งตัวจากที่เคยเป็น
                        let top_left =
                            Vec2::new(2000.0 + col as f32 * cell, 2000.0 + row as f32 * cell);

                        let item = Item::new(ItemKind::Image(AssetRef {
                            hash,
                            path: source
                                .file()
                                .map(std::path::Path::to_path_buf)
                                .unwrap_or_default(),
                            px_size: glam::UVec2::new(sw, sh),
                            // ★ ยังไม่รู้ format จริงตรงนี้ — cache hit ไม่ได้แตะไบต์ของไฟล์เลย
                            //   เขียน `Unknown` ตรง ๆ ดีกว่าเดาจากนามสกุล (docs/02 §2.2.5 ข้อ 2)
                            //   งานที่จะร้อย format จริงผ่าน decode → Thumbnail → ThumbEntry
                            //   ถูกแยกไว้เป็นงานของตัวเอง (HANDOFF §6)
                            format: ImageFormat::Unknown,
                            embedded: false,
                        }))
                        .at(top_left + size * 0.5, size);

                        // ★ ทุกการเพิ่มภาพผ่าน `AddItems` เข้า `History` → ลากไฟล์เข้ามาแล้ว undo ได้
                        let Ok(command) = AddItems::new(vec![item]) else {
                            continue;
                        };
                        if let Err(err) = gfx.history.apply(&mut gfx.board, Box::new(command)) {
                            tracing::error!(%err, "cannot add the dropped image to the board");
                            continue;
                        }
                        // `insert_item` ต่อท้าย z-order เสมอ ตัวที่เพิ่งเพิ่มจึงอยู่ท้ายสุด
                        let Some(id) = gfx.board.z_order().last().copied() else {
                            continue;
                        };

                        if let Some(item) = gfx.board.item(id) {
                            gfx.index.insert(id, &item.canvas);
                        }
                        gfx.render_state.insert(
                            id,
                            ItemRender {
                                source,
                                hash,
                                thumb: *thumb,
                                uv_rect: slot.uv_rect(),
                                tint: [1.0, 1.0, 1.0, 1.0],
                                layer: slot.layer,
                                flags: 0, // มี texture จริงแล้ว ไม่ใช่ placeholder
                            },
                        );
                        self.drop_shown += 1;
                    }
                    Err(err) => {
                        tracing::warn!(%err, "cannot store the thumbnail in the atlas");
                        self.shell.status = text::atlas_error(self.shell.lang, &err);
                    }
                }
            }
            // ★ instance ที่ส่งให้ GPU สร้างใหม่จาก board **หลังจบชุด** ไม่ใช่ทีละใบ
            //   (ลากเข้ามา 100 ไฟล์ = สร้างครั้งเดียว ไม่ใช่ 100 ครั้ง)
            Self::rebuild_quads(gfx);

            // ★ เวลาจริงที่ผู้ใช้รู้สึก: ลากเข้ามา → ภาพขึ้นจอ
            if !self.drop_reported
                && self.drop_shown >= self.drop_expected
                && let Some(started) = self.drop_started
            {
                let elapsed = started.elapsed();
                let ms = elapsed.as_secs_f64() * 1000.0;
                self.drop_reported = true;
                if self.batch_from_clipboard {
                    tracing::info!(ms, "clipboard paste → image on screen");
                    self.shell.status = text::fill(
                        self.shell.lang,
                        Template::PastedImage,
                        &[("ms", &format!("{ms:.0}"))],
                    );
                } else {
                    tracing::info!(
                        files = self.drop_expected,
                        ms,
                        // ★ หลักฐานว่าการเพิ่มภาพเดินผ่าน `AddItems` เข้า `History` จริง
                        //   ไม่ใช่ push เข้า Vec ตรง ๆ เหมือนก่อนย้าย — undo ได้ทุกใบ
                        undo_depth = self.gfx.as_ref().map_or(0, |g| g.history.undo_depth()),
                        "drag & drop → every image on screen"
                    );
                    println!("ลากไฟล์ {} ไฟล์ → ขึ้นจอครบใน {ms:.1} ms", self.drop_expected);
                    self.shell.status = text::fill(
                        self.shell.lang,
                        Template::OpenedFiles,
                        &[
                            ("n", &self.drop_expected.to_string()),
                            ("ms", &format!("{ms:.0}")),
                        ],
                    );
                }
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

        // ★ ไฟล์จาก clipboard เข้าคิวเป็นชุดใหม่ — เส้นทางเดียวกับลากไฟล์เข้ามาเป๊ะ
        //   (ต้องอยู่หลังจากเลิกยืม `assets` แล้วเท่านั้น)
        if !pasted_files.is_empty() {
            self.submit_dropped(pasted_files);
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
        // ★ ต้องทำก่อนวาดเฟรมแรก และต้องทำ **ทุกครั้งที่สร้าง Context ใหม่**
        //   ซึ่งรวมถึงตอนกู้ device (docs/04 §7 ข้อ 3) — ไม่งั้นตัวหนังสือไทย
        //   จะกลับไปเป็นสี่เหลี่ยมหลัง driver อัปเดต
        crate::fonts::install(&egui_ctx);
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
            tracing::error!(%err, "GPU device recovery failed");
            return None;
        }

        let (egui_ctx, egui_winit, egui_renderer) = Self::build_egui(&gfx.window, &gfx.render);
        gfx.egui_ctx = egui_ctx;
        gfx.egui_winit = egui_winit;
        gfx.egui_renderer = egui_renderer;

        // ★★ ห้ามเปลี่ยนตรงนี้ไปเป็น "สร้าง `Gfx` ใหม่ทั้งก้อน"
        //
        //   `Gfx` ถือ `board` / `history` / `selection` ซึ่งเป็น **งานของผู้ใช้**
        //   ไม่ใช่ของที่ผูกกับ device (บ้านที่ผิด — ควรย้ายออกตอน P4-7 multi-board)
        //   ตอนนี้ปลอดภัยเพราะฟังก์ชันนี้แก้ทีละฟิลด์ ของที่ไม่ได้แตะจึงรอด
        //   แต่ถ้าวันหนึ่งมีคนเขียนเป็น `*gfx = Gfx::new(...)` **board ของผู้ใช้จะหาย
        //   ทันทีที่ driver อัปเดต** โดยไม่มี error ที่ไหนเลย = ผิด I-3 เต็ม ๆ
        //
        // ★ resource ที่ผูกกับ device เดิม **ต้องสร้างใหม่ทั้งชุด** (docs/04 §7 ข้อ 3)
        //   สร้างผ่านจุดเดียวกับตอนเปิดโปรแกรม แล้วรับด้วยการ destructure
        //   เพื่อให้คอมไพเลอร์บังคับว่าห้ามลืมชิ้นไหน (ดู `DeviceBound`)
        // TODO(P1-5): re-upload thumbnail จาก cache.sqlite แทนที่จะปล่อยว่าง
        let DeviceBound {
            textures,
            atlas,
            pipeline,
            working,
        } = match DeviceBound::build(&gfx.render) {
            Ok(bound) => bound,
            Err(err) => {
                tracing::error!(%err, "cannot build the resources for the new device");
                return None;
            }
        };
        gfx.textures = textures;
        gfx.atlas = atlas;
        gfx.pipeline = pipeline;
        // ★ WorkingCache เก่าถือ texture ของ device ที่ตายไปแล้ว — ทิ้งทั้งก้อน
        //   ถ้าเก็บไว้ `contains()` จะตอบ true แล้วเราจะเอา bind group ของ device
        //   ที่ตายแล้วไปวาด (validation error ทุกเฟรม) — ผู้ใช้เห็นภาพหาย
        gfx.working = working;
        // คีย์ที่ "สั่งไปแล้วรอผลอยู่" ก็ตายไปกับ device เดิม ถ้าไม่ล้าง ภาพคม
        // จะไม่มีวันถูกขอใหม่เลยเพราะ insert() คืน false ตลอด
        gfx.working_pending.clear();
        gfx.working_quads.clear();
        gfx.device_generation = gfx.render.generation();

        // ★ เติม atlas กลับ (docs/04 §4) — ถ้าไม่ทำ ผู้ใช้จะเห็น board ว่างเปล่า
        //   หลัง driver อัปเดต ซึ่งแยกไม่ออกจาก "งานหาย" แล้วเขาจะปิดโปรแกรมทิ้ง
        //   ทำให้งานกู้ device ทั้งหมดเสียเปล่า
        Self::refill_atlas(gfx);

        Some(RedrawReason::SurfaceRecovery)
    }

    /// อัดภาพย่อขึ้น atlas — ขยาย atlas แล้วเติมของเดิมกลับให้เองถ้าที่ไม่พอ
    ///
    /// `ThumbnailAtlas::upload` **ไม่ขยายเอง** โดยตั้งใจ เพราะการขยายแบบคัดลอก
    /// บน GPU บังคับให้ถือ texture สองใบพร้อมกัน = 368 MB จากเพดาน 384 MB
    /// ตอนขยาย 11→12 layer (docs/05 §2) การสร้างใหม่แล้วเติมกลับจาก RAM
    /// ทำให้ peak เหลือเท่าใบใหม่ใบเดียว และเราเก็บภาพย่อไว้ใน RAM อยู่แล้ว
    /// เพื่อเส้นทางกู้ device — โค้ดเติมกลับจึงเป็นตัวเดียวกันเป๊ะ
    fn upload_thumb(gfx: &mut Gfx, pixels: &[u8]) -> Result<AtlasSlot, AtlasError> {
        match gfx.atlas.upload(gfx.render.queue(), pixels) {
            Err(AtlasError::NeedsResize { layers }) => {
                gfx.atlas.resize(gfx.render.device(), layers)?;
                // texture ใหม่ว่างเปล่า — ต้องเติมภาพเดิมกลับก่อนใส่ภาพใหม่
                Self::refill_atlas(gfx);
                gfx.atlas.upload(gfx.render.queue(), pixels)
            }
            other => other,
        }
    }

    /// เลือกว่าเฟรมนี้ภาพไหนควรใช้ working texture แล้วสั่ง decode ตัวที่ยังไม่มี
    ///
    /// เรียกก่อนวาดทุกเฟรม — แต่ **ไม่ขอเฟรมใหม่เอง** (I-1) เฟรมเกิดเพราะผู้ใช้
    /// ขยับหรือเพราะ worker ปลุกเมื่อมีของใหม่เท่านั้น ซูมนิ่งแล้วจึงกลับไป idle
    ///
    /// docs/04 §4 ชั้น B: ภาพที่ขนาดบนจอ > 128 px ขอ texture แยกที่ pow2 พอดีขนาดบนจอ
    fn plan_working_textures(&mut self) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        gfx.working_quads.clear();
        // ★ ตัวดักบั๊กที่เคยเกิดจริง: หลังกู้ device ถ้าใครลืมสร้าง WorkingCache ใหม่
        //   เราจะเอา bind group ของ device ที่ตายไปแล้วไปวาด แล้วภาพหายทั้ง board
        //   ให้ล้มดัง ๆ ตั้งแต่ build debug แทนที่จะไปพังเงียบ ๆ ที่เครื่องผู้ใช้
        debug_assert_eq!(
            gfx.working.generation(),
            gfx.render.generation(),
            "working texture cache ยังเป็นของ device รุ่นเก่า — ลืมสร้างใหม่ตอนกู้ device"
        );
        if gfx.board.is_empty() {
            return;
        }

        let zoom = gfx.camera.zoom();
        let viewport = gfx.canvas.size;
        let centre = gfx.camera.center();
        let mut requests: Vec<refx_asset::pool::Job> = Vec::new();
        // เก็บไว้ก่อนแล้วค่อยแปลงเป็น quad หลังจบลูป — ระหว่างลูปยังยืม `gfx.board` อยู่
        let mut working_hits: Vec<(WorkingKey, ItemId)> = Vec::new();

        for (id, board_item) in gfx.board.items_in_z_order() {
            let Some(item) = gfx.render_state.get(&id) else {
                continue;
            };
            let canvas = &board_item.canvas;
            // ★ ภาพที่วางมาจาก clipboard ไม่มีไฟล์ให้กลับไป decode ใหม่ จึงคมได้แค่
            //   ระดับ thumbnail ถ้าไม่ข้ามตรงนี้ ทุกครั้งที่ซูมจะได้งานที่ล้มเหลว
            //   แน่นอนหนึ่งใบ พร้อมข้อความ error ที่ผู้ใช้ทำอะไรกับมันไม่ได้
            if item.source.file().is_none() {
                continue;
            }
            // ขนาดบนจอ = ขนาดใน world × ซูม — อ่านจาก `ItemCanvas` ไม่ใช่จาก quad แล้ว
            let world_side = canvas.size.x.abs().max(canvas.size.y.abs());
            let on_screen = world_side * zoom;

            let source_side = item.thumb.source_width.max(item.thumb.source_height);
            let Some(size) = refx_asset::working::working_size_for(on_screen, source_side) else {
                continue;
            };

            // ★ นอกจอไม่ต้องขอ — เกณฑ์เดียวกับ culling คือกึ่งกลาง item เทียบ viewport
            //   ถ้าไม่กรอง การซูมเข้าลึก ๆ จะสั่ง decode ทั้ง board ทั้งที่เห็นไม่กี่ใบ
            //   `ItemCanvas::pos` คือจุดกึ่งกลางอยู่แล้ว ไม่ต้องบวกครึ่งขนาดเหมือนเดิม
            let offset = (canvas.pos - centre) * zoom;
            let margin = viewport * 0.5 + glam::Vec2::splat(world_side * zoom);
            if offset.x.abs() > margin.x || offset.y.abs() > margin.y {
                continue;
            }

            let key = WorkingKey {
                hash: *item.hash.as_bytes(),
                size,
            };
            if gfx.working.contains(key) {
                working_hits.push((key, id));
                continue;
            }
            if gfx.working_pending.insert(key) {
                requests.push(refx_asset::pool::Job {
                    hash: item.hash,
                    source: item.source.clone(),
                    // ภาพที่อยู่ใกล้กึ่งกลางจอมาก่อน (docs/05 §3)
                    priority: offset.length(),
                    cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    target: refx_asset::pool::JobTarget::Working { size },
                });
            }
        }

        // ★ quad ของภาพคมสร้างจาก `Board` เหมือนกัน — ไม่ได้ก๊อปมาจาก `quads`
        //   ที่อาจเป็นของเฟรมก่อน (แหล่งความจริงเดียวคือ `board`)
        for (key, id) in working_hits {
            let Some(item) = gfx.board.item(id) else {
                continue;
            };
            let Some(state) = gfx.render_state.get(&id) else {
                continue;
            };
            gfx.working_quads
                .push((key, Self::quad_for(&item.canvas, state)));
        }

        if requests.is_empty() {
            return;
        }
        if let Some(assets) = self.assets.as_ref() {
            tracing::debug!(count = requests.len(), "queued working texture decodes");
            for job in requests {
                assets.pool.submit(job);
            }
        }
    }

    /// เอา input ที่ widget เก็บมาไปขยับกล้องและสั่งเครื่องมือเลือก
    ///
    /// คืน `true` เมื่อมีอะไรเปลี่ยนจนต้องวาดใหม่ — **I-1: ไม่มีอะไรเปลี่ยนต้องไม่ขอเฟรม**
    fn apply_canvas_input(gfx: &mut Gfx, input: CanvasFrameInput) -> bool {
        let mut changed = false;
        let rect = input.rect;
        if !rect.is_positive() {
            return false;
        }

        // ---- กล้อง: ปุ่มกลางลาก + ล้อซูม ----
        //
        // ★ ทำไม pan ใช้ **ปุ่มกลาง** ไม่ใช่ space+ลาก: ปุ่มซ้ายเป็นของการเลือกแล้ว
        //   ส่วน space จะชนกับ text note (P2-11) ที่ space เป็นตัวอักษรจริง ๆ
        //   ปุ่มกลางไม่ต้องพึ่งสถานะคีย์บอร์ดเลยจึงไม่มีทางค้าง (เพิ่ม space ทีหลังได้)
        let ppp = gfx.egui_ctx.pixels_per_point();
        if input.pan_delta != egui::Vec2::ZERO {
            // ระยะลากเป็น point — กล้องคิดเป็น physical pixel
            gfx.camera
                .pan_by_screen_delta(Vec2::new(input.pan_delta.x, input.pan_delta.y) * ppp);
            changed = true;
        }
        if input.scroll.abs() > f32::EPSILON
            && let Some(pointer) = input.pointer
        {
            // เลขชี้กำลังทำให้ซูมรู้สึกเท่ากันทุกระดับ (เหมือนเดิมก่อนย้าย)
            let factor = 1.1f32.powf(input.scroll / 50.0);
            let local = pointer - rect.min;
            gfx.camera.zoom_at_screen(
                Vec2::new(local.x, local.y) * ppp,
                Vec2::new(rect.width(), rect.height()) * ppp,
                factor,
            );
            changed = true;
        }

        // ---- การเลือก ----
        let Some(pointer) = input.pointer else {
            return changed;
        };
        let scale = gfx.camera.zoom() / ppp;
        if scale <= 0.0 {
            return changed;
        }
        let offset = pointer - rect.center();
        let world = gfx.camera.center() + Vec2::new(offset.x, offset.y) / scale;

        // ระยะเริ่มลากคิดเป็นพิกเซลบนจอเสมอ เพื่อให้รู้สึกเท่ากันทุกระดับซูม
        let ctx = CanvasContext {
            board: &gfx.board,
            index: &gfx.index,
            drag_threshold: refx_core::interact::DEFAULT_DRAG_THRESHOLD_PX * ppp
                / gfx.camera.zoom(),
        };

        let event = if input.primary_pressed {
            Some(CanvasEvent::Press {
                button: CanvasButton::Primary,
                world,
                modifiers: input.modifiers,
            })
        } else if input.primary_released {
            Some(CanvasEvent::Release {
                button: CanvasButton::Primary,
                world,
            })
        } else if input.primary_down {
            Some(CanvasEvent::Move { world })
        } else {
            None
        };

        let Some(event) = event else {
            return changed;
        };

        // ตัวที่กำลังถูกลากคือชุดที่เลือกอยู่ **ก่อน** ส่ง event เข้าไป
        let moved: Vec<ItemId> = gfx.selection.iter().collect();
        let outcome = gfx.select_tool.handle(ctx, &mut gfx.selection, event);
        gfx.rubber_band = outcome.rubber_band;
        changed |= outcome.needs_redraw;

        let has_commands = !outcome.commands.is_empty();
        for command in outcome.commands {
            if let Err(err) = gfx.history.apply(&mut gfx.board, command) {
                tracing::error!(%err, "cannot apply a canvas edit");
            }
        }
        if outcome.seal {
            // ปล่อยเมาส์ = ปิดหน้าต่าง merge · การลากครั้งถัดไปเป็น undo ขั้นใหม่
            gfx.history.seal();
        }

        if has_commands {
            // ★ index ต้องตามตำแหน่งใหม่ทันที ไม่งั้นการกดครั้งถัดไปจะ hit-test
            //   กับตำแหน่ง **เก่า** แล้วคลิกไม่โดนภาพที่เพิ่งย้ายไป
            //   (เจอตอนเขียนเทสต์ใน refx-core — ที่นั่นก็ต้องทำเหมือนกันเป๊ะ)
            for id in moved {
                if let Some(item) = gfx.board.item(id) {
                    gfx.index.insert(id, &item.canvas);
                }
            }
            Self::rebuild_quads(gfx);
            changed = true;
        }
        changed
    }

    /// ทำ undo/redo แล้วทำให้ผู้ใช้ **เห็นว่าเกิดอะไรขึ้น**
    fn apply_history_request(&mut self, request: HistoryRequest) {
        let lang = self.shell.lang;
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let outcome = match request {
            HistoryRequest::Undo => gfx.history.undo(&mut gfx.board),
            HistoryRequest::Redo => gfx.history.redo(&mut gfx.board),
        };

        let affected = match outcome {
            Ok(Some(affected)) => affected,
            Ok(None) => {
                // ★ ไม่มีอะไรให้ย้อนแล้ว **ต้องบอก** ไม่ใช่เงียบ
                //   ความเงียบอ่านได้ว่า "โปรแกรมไม่ตอบสนอง" แล้วผู้ใช้จะกดซ้ำ ๆ
                self.shell.status = text::t(
                    lang,
                    match request {
                        HistoryRequest::Undo => Key::NothingToUndo,
                        HistoryRequest::Redo => Key::NothingToRedo,
                    },
                )
                .to_owned();
                return;
            }
            Err(err) => {
                // ย้อนไม่ได้ = มีคนแก้ board นอกเส้นทาง Command (ผิด I-3)
                tracing::error!(%err, ?request, "history operation failed");
                return;
            }
        };

        // index กับ quad ต้องตามสถานะใหม่ของ board ทันที
        gfx.index.rebuild(&gfx.board);
        Self::rebuild_quads(gfx);

        // ★ เลือกของที่เพิ่งเปลี่ยนให้ผู้ใช้ (docs/02 §2.9)
        //   การเลือกไม่ได้ถูก undo — มันตามผลลัพธ์ที่คำสั่งรายงานกลับมา
        //   id ที่หายไปแล้ว (undo ของการเพิ่ม) ต้องกรองทิ้ง ไม่งั้น selection ถือของว่าง
        let live: Vec<ItemId> = affected
            .into_iter()
            .filter(|id| gfx.board.item(*id).is_some())
            .collect();
        gfx.selection.restore(live.clone(), live.last().copied());
        // การลากที่ค้างอยู่ (ถ้ามี) ใช้ไม่ได้แล้วเพราะ board เปลี่ยนไปใต้มือ
        gfx.select_tool.cancel();
        gfx.rubber_band = None;

        Self::look_at_if_offscreen(gfx, &live);
        gfx.window.request_redraw();
    }

    /// เลื่อนกล้องไปหาสิ่งที่เพิ่งเปลี่ยน **ถ้ามันมองไม่เห็นเลย**
    ///
    /// ★ รายละเอียดที่ทำให้ undo รู้สึกเชื่อถือได้: ถ้าของที่ถูกย้อนอยู่นอกจอ
    /// ผู้ใช้จะเห็นว่า "กด Ctrl+Z แล้วไม่มีอะไรเกิดขึ้น" ซึ่งอ่านได้ว่าโปรแกรมพัง
    /// แล้วเขาจะกดซ้ำ ๆ จน **ย้อนเลยจุดที่ตั้งใจ** — งานหายโดยที่กลไกกันงานหาย
    /// ทำงานถูกต้องทุกขั้นตอน
    ///
    /// เลื่อนเฉพาะตอน **มองไม่เห็นเลย** ไม่ใช่ตอนโผล่ไม่ครบ — การกระตุกกล้อง
    /// ทั้งที่ผู้ใช้เห็นของอยู่แล้วน่ารำคาญกว่าประโยชน์ที่ได้
    ///
    /// ไม่มี animation โดยตั้งใจ: I-1 บังคับว่า idle ต้อง 0% CPU การเลื่อนแบบ
    /// ค่อย ๆ ไถลต้องวาดต่อเนื่องหลายเฟรม ซึ่งแลกไม่คุ้มกับความสวยตรงนี้
    fn look_at_if_offscreen(gfx: &mut Gfx, ids: &[ItemId]) {
        let mut bounds = WorldRect::EMPTY;
        for id in ids {
            if let Some(item) = gfx.board.item(*id) {
                bounds = bounds.union(item.canvas.world_bounds());
            }
        }
        // ไม่มีของให้ดู (เช่น undo ของการเพิ่ม หรือ ReorderZ ที่ไม่ได้แตะใคร)
        if bounds.is_empty() || !bounds.is_finite() {
            return;
        }

        let zoom = gfx.camera.zoom();
        if zoom <= 0.0 {
            return;
        }
        let half = gfx.canvas.size * 0.5 / zoom;
        let centre = gfx.camera.center();
        let view = WorldRect {
            min: centre - half,
            max: centre + half,
        };
        if view.intersects(bounds) {
            return; // เห็นอยู่แล้วอย่างน้อยบางส่วน — อย่าไปกระตุกกล้อง
        }

        tracing::info!("moving the camera to show what the history change affected");
        gfx.camera.set_center(bounds.center());
    }

    /// แปลง item หนึ่งใบเป็น instance ที่ GPU วาดได้
    ///
    /// ★ **จุดเดียวที่เรขาคณิตของ `Board` กลายเป็น `QuadInstance`**
    /// `transform` ใช้มุมซ้ายบน (unit quad คือ 0..1) ส่วน `ItemCanvas::pos` คือจุดกึ่งกลาง
    /// จึงต้องลบครึ่งขนาดออก — ถ้าทำผิดตรงนี้ภาพทุกใบจะเลื่อนไปครึ่งตัว
    fn quad_for(canvas: &ItemCanvas, state: &ItemRender) -> QuadInstance {
        let half = canvas.size * 0.5;
        QuadInstance {
            transform: [
                canvas.size.x,
                0.0,
                0.0,
                canvas.size.y,
                canvas.pos.x - half.x,
                canvas.pos.y - half.y,
            ],
            uv_rect: state.uv_rect,
            tint: state.tint,
            layer: state.layer,
            flags: state.flags,
        }
    }

    /// สร้าง `quads` ใหม่ทั้งชุดจาก `board`
    ///
    /// ★ **ประตูเดียวที่เขียน `gfx.quads` ได้** — `quads` เป็นผลลัพธ์ ไม่ใช่แหล่งความจริง
    /// ลำดับ render = ลำดับใน `z_order` อยู่แล้ว จึงไม่ต้อง sort (docs/02 §2.1)
    fn rebuild_quads(gfx: &mut Gfx) {
        gfx.quads.clear();
        for (id, item) in gfx.board.items_in_z_order() {
            if let Some(state) = gfx.render_state.get(&id) {
                gfx.quads.push(Self::quad_for(&item.canvas, state));
            }
        }
    }

    /// อัด thumbnail ของทุก item กลับขึ้น atlas ที่เพิ่งสร้างใหม่
    ///
    /// เรียกจากสองที่ที่ทำให้ texture เดิมหายไป: กู้ device (P0-5) และขยาย atlas
    /// (`upload_thumb`) — ทั้งสองกรณีภาพเดิมหายพร้อม texture เก่า ต้องเติมกลับจาก RAM
    ///
    /// ระหว่างที่ยังเติมไม่ครบ item ที่เหลือถูกทำเป็น **placeholder สีเด่น**
    /// ไม่ใช่ช่องว่าง (docs/04 §4, §8) — ผู้ใช้ต้องเห็นว่า layout ยังอยู่ครบ
    fn refill_atlas(gfx: &mut Gfx) {
        if gfx.board.is_empty() {
            return;
        }
        let started = std::time::Instant::now();
        let mut restored = 0usize;

        // ★ atlas ที่เพิ่งสร้าง (ตอนกู้ device) มี **0 layer** เพราะจองแบบ lazy
        //   (docs/05 §2) ต้องขยายให้พอ **ก่อน** เริ่มเติม ไม่ใช่ตอนเจอ NeedsResize
        //   กลางทาง — `resize()` ล้างตัวจัดสรรทั้งชุด ช่องที่แจกไปแล้วในรอบนี้
        //   จะชี้ไปที่ texture ที่ถูกทิ้งไปแล้ว
        //
        //   ถ้าไม่ทำขั้นนี้ ภาพ **ทุกใบ** กลายเป็น placeholder หลังกู้ device
        //   (เจอจริง 29 ก.ค. 2026: restored=0 total=8) ซึ่งคือ "board ว่างเปล่า"
        //   ที่ docs/04 §4 สั่งห้ามไว้ตรง ๆ
        let needed = refx_render::atlas::layers_needed(gfx.board.len());
        if gfx.atlas.layers_allocated() < needed
            && let Err(err) = gfx.atlas.resize(gfx.render.device(), needed)
        {
            // ขยายไม่ได้ = VRAM ไม่พอ เติมได้เท่าที่ได้ ที่เหลือเป็น placeholder
            tracing::warn!(%err, needed, "cannot grow the atlas before refilling it");
        }

        // ★ ไล่ตามลำดับ z ของ board — `render_state` เป็นแผนที่ ไม่ใช่รายการคู่ขนาน
        //   แล้วเขียนผลลง `render_state` ไม่ใช่ลง `quads` โดยตรง
        //   (`quads` ถูกสร้างใหม่จาก board ทีหลัง — ดู `rebuild_quads`)
        let order: Vec<ItemId> = gfx.board.z_order().to_vec();
        // ★ แยกการยืมทีละฟิลด์ **ห้าม clone pixel** — thumbnail ใบละ 64 KB
        //   ที่ 100 ภาพคือก๊อป 6.4 MB ทุกครั้งที่กู้ device (วัดแล้วช้าลง 30%)
        //   ทางที่ถูกคือ destructure ให้ atlas/render/render_state ยืมคนละฟิลด์กัน
        let Gfx {
            atlas,
            render,
            render_state,
            ..
        } = gfx;
        for id in order {
            let Some(state) = render_state.get_mut(&id) else {
                continue;
            };
            let dominant = state.thumb.dominant;
            // ★ ใช้ upload ตรง ๆ ห้ามผ่าน upload_thumb — ไม่งั้นจะเรียก refill ซ้อนตัวเอง
            let uploaded = atlas.upload(render.queue(), &state.thumb.pixels);
            match uploaded {
                Ok(slot) => {
                    state.uv_rect = slot.uv_rect();
                    state.layer = slot.layer;
                    state.tint = [1.0, 1.0, 1.0, 1.0];
                    state.flags &= !refx_render::instance::flags::PLACEHOLDER;
                    restored += 1;
                }
                Err(err) => {
                    // atlas เต็ม — ที่เหลือขึ้นเป็นสี่เหลี่ยมสีเด่นแทนช่องว่าง
                    tracing::warn!(%err, ?id, "atlas refill incomplete — the rest fall back to placeholders");
                    let [a, r, g, b] = dominant.to_be_bytes();
                    state.tint = [
                        f32::from(r) / 255.0,
                        f32::from(g) / 255.0,
                        f32::from(b) / 255.0,
                        f32::from(a) / 255.0,
                    ];
                    state.flags |= refx_render::instance::flags::PLACEHOLDER;
                }
            }
        }
        Self::rebuild_quads(gfx);

        tracing::info!(
            restored,
            total = gfx.board.len(),
            ms = started.elapsed().as_secs_f64() * 1000.0,
            "refilled thumbnails into the new atlas"
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
        // ★ destructure ไว้โดยตั้งใจ — เพิ่ม resource ใหม่ใน DeviceBound เมื่อไหร่
        //   ตรงนี้จะคอมไพล์ไม่ผ่าน พร้อมกับฝั่ง recover_device()
        let DeviceBound {
            textures,
            atlas,
            pipeline,
            working,
        } = DeviceBound::build(&render).map_err(|err| {
            tracing::error!(%err, "cannot create the atlas");
            DeviceError::NoSupportedFormat
        })?;
        let device_generation = render.generation();

        let quads = self.args.demo_quads.map_or_else(Vec::new, |n| {
            let quads = demo_quads(n, 4000.0);
            tracing::info!(count = quads.len(), "generated demo quads");
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
            board: Board::default(),
            history: History::default(),
            index: SpatialIndex::new(refx_core::spatial::DEFAULT_CELL_SIZE),
            render_state: std::collections::HashMap::new(),
            selection: Selection::new(),
            select_tool: SelectTool::new(),
            rubber_band: None,
            working,
            working_pending: std::collections::HashSet::new(),
            working_quads: Vec::new(),
            // เริ่มที่กลาง world ของ demo เพื่อให้เห็นสี่เหลี่ยมทันทีที่เปิด
            camera: Camera::new(Vec2::splat(2000.0), 0.25),
            canvas: CanvasRect::full(size.width, size.height),
            modifiers: ModifiersState::empty(),
        });
        self.queue_initial_files();
        Ok(())
    }

    fn redraw(&mut self) -> Option<RedrawReason> {
        // GPU หน่วยความจำเต็ม — ทิ้ง cache ก่อนทำอย่างอื่น (docs/04 §7)
        if self.gfx.as_mut().is_some_and(|g| g.render.take_oom()) {
            // TODO(P1-6): cache.emergency_evict() ทิ้ง T2 แล้ว T1
            // TODO(P4-2): autosave ทันทีถ้ายังไม่พอ
            tracing::error!("GPU out of memory — there is no cache to drop yet (P0)");
        }

        let frame_start = std::time::Instant::now();

        // ไฟล์ที่ลากเข้ามาในรอบ event ที่ผ่านมา — ส่งเป็นชุดเดียวเพื่อจับเวลาได้ถูก
        if !self.pending_drops.is_empty() {
            let batch = std::mem::take(&mut self.pending_drops);
            self.submit_dropped(batch);
        }

        // ★ Ctrl+V ที่กดไปเมื่อกี้ — งานอ่าน clipboard เกิดบน worker ทั้งหมด (I-2)
        if std::mem::take(&mut self.pending_paste) {
            self.submit_paste();
        }

        // Ctrl+Z / Ctrl+Y ที่กดไปเมื่อกี้
        if let Some(request) = self.pending_history.take() {
            self.apply_history_request(request);
        }

        // เก็บผล decode ที่เสร็จแล้วก่อนวาด (ไม่บล็อก)
        self.drain_decode_results();
        // ★ ตัดสินใจเรื่อง working texture ก่อนวาด — ใช้กล้อง/กรอบของเฟรมที่แล้ว
        self.plan_working_textures();

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
        shell.item_count = gfx.board.len();
        shell.zoom = gfx.camera.zoom();
        shell.vram_used = gfx.textures.budget().used();
        shell.working_used = gfx.working.used();
        shell.working_limit = gfx.working.limit();
        shell.working_evicted = gfx.working.evicted();
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
        let mut canvas_input = CanvasFrameInput::default();
        // ★ clone `Context` ออกมาก่อน (มันเป็น `Arc` ข้างใน) เพื่อปลดการยืม `gfx`
        //   คลอเชอร์ข้างล่างจะได้ยืมฟิลด์อื่นของ `gfx` แบบอ่านอย่างเดียวไปวาดกรอบ
        //   สิ่งที่ถูกเลือกได้ แล้วค่อยแก้สถานะหลัง `run_ui` จบ
        let egui_ctx = gfx.egui_ctx.clone();
        let full_output = {
            let board = &gfx.board;
            let selection = &gfx.selection;
            let render_state = &gfx.render_state;
            let camera = gfx.camera;
            let rubber_band = gfx.rubber_band;
            egui_ctx.run_ui(raw_input, |ui| {
                canvas_points = crate::shell::draw_in_ui(ui, shell, |ui| {
                    // ช่องกลางคือ canvas — ภาพวาดด้วย wgpu ใต้ egui อีกที
                    // ★ เป็น widget จริงแล้ว egui จึงจัดลำดับ pointer ให้เอง
                    canvas_input = Self::canvas_widget(
                        ui,
                        board,
                        selection,
                        render_state,
                        camera,
                        rubber_band,
                    );
                });
            })
        };
        if Self::apply_canvas_input(gfx, canvas_input) {
            gfx.window.request_redraw();
        }
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
                // ★ ภาพที่มี working texture วาดแยกทีละใบ (docs/04 §4 ชั้น B)
                //   ที่เหลือวาดรวมกันจาก atlas ใน draw call เดียวเหมือนเดิม
                //
                //   วาด atlas ก่อนแล้วค่อยทับด้วยตัวคมกว่า — ระหว่างที่ working texture
                //   ยังมาไม่ถึง ผู้ใช้จะเห็นภาพเบลอ ไม่ใช่ช่องว่าง (docs/04 §8)
                let mut batches: Vec<DrawBatch<'_>> = vec![DrawBatch {
                    bind_group: gfx.atlas.bind_group(),
                    instances: &gfx.quads,
                }];
                let sharp: Vec<QuadInstance> = gfx
                    .working_quads
                    .iter()
                    .map(|(_, quad)| {
                        // working texture มีภาพเดียวเต็มใบ layer 0
                        QuadInstance {
                            uv_rect: [0.0, 0.0, 1.0, 1.0],
                            layer: 0,
                            ..*quad
                        }
                    })
                    .collect();
                // อัปเดต LRU ก่อน แล้วค่อยเก็บ reference ไปวาด — ยืมคนละแบบ
                for (key, _) in &gfx.working_quads {
                    gfx.working.touch(*key);
                }
                for (index, (key, _)) in gfx.working_quads.iter().enumerate() {
                    if let Some(bind_group) = gfx.working.bind_group(*key) {
                        batches.push(DrawBatch {
                            bind_group,
                            instances: &sharp[index..=index],
                        });
                    }
                }
                let calls = gfx
                    .pipeline
                    .draw_batches(gfx.render.queue(), &mut pass, &batches);
                shell.draw_calls = calls;
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

        // ★ pointer เป็นเรื่องของ egui ทั้งหมดแล้ว (แก้ 3 ส.ค. 2026)
        //
        //   เดิมต้องเดาเองว่า pointer เป็นของใครด้วย `egui_is_using_pointer()` +
        //   `canvas.contains(cursor)` เพราะ `CentralPanel` กิน root rect จนหมด
        //   ทำให้ `response.consumed` เป็น true ทุกจุดบน canvas (docs/03 §1)
        //
        //   ตอนนี้ canvas เป็น widget จริงด้วย `allocate_response` แล้ว egui จึงเป็น
        //   คนจัดลำดับให้เอง — คลิกบน toolbar/inspector ไม่ตกมาถึง canvas เพราะ
        //   widget พวกนั้นกิน response ไปก่อน ที่นี่จึงเหลือแค่ event ที่ egui ไม่สนใจ
        //   (ลากไฟล์เข้ามา, ปุ่มค้าง, Ctrl+V) ส่วนเมาส์ทั้งหมดไปอยู่ `canvas_widget`

        match event {
            // ★ ลากไฟล์เข้ามา — เส้นทางหลักที่ผู้ใช้เอาภาพเข้าโปรแกรม (P1-8)
            WindowEvent::DroppedFile(path) => {
                // winit ส่งมาทีละไฟล์ รวมเป็นชุดเดียวถ้ามาติด ๆ กัน
                self.pending_drops.push(path.clone());
                needs_redraw = true;
            }

            // ปุ่มกดค้างสถานะไว้เอง — ต้องจำไว้เพราะ KeyboardInput ไม่ได้แนบมาให้
            WindowEvent::ModifiersChanged(modifiers) => {
                gfx.modifiers = modifiers.state();
            }

            // ★ Ctrl+V — วางภาพจาก clipboard (docs/03 §5, P1-8)
            WindowEvent::KeyboardInput { event, .. } => {
                // `repeat` = ผู้ใช้กดค้างไว้ ไม่ใช่เจตนาจะวางหลายรอบ
                // ถ้าไม่กรอง การกดค้างหนึ่งวินาทีจะสั่งอ่าน clipboard หลายสิบครั้ง
                if event.state.is_pressed()
                    && !event.repeat
                    && is_paste(&event.logical_key, gfx.modifiers)
                {
                    // อ่าน clipboard ที่นี่ไม่ได้ — บล็อกได้ (I-2) ทำที่ต้นเฟรมถัดไป
                    self.pending_paste = true;
                    needs_redraw = true;
                }
                // ★ undo/redo **ยอมให้กดค้างซ้ำได้** ต่างจาก Ctrl+V โดยตั้งใจ
                //   กด Ctrl+Z ค้างแล้วย้อนเรื่อย ๆ เป็นสิ่งที่ทุกคนคาดหวัง
                //   ส่วนการวางซ้ำ ๆ ไม่ใช่ (แถมภาพจาก clipboard ใหญ่ได้เป็นร้อย MB)
                if event.state.is_pressed()
                    && let Some(request) = history_shortcut(&event.logical_key, gfx.modifiers)
                {
                    self.pending_history = Some(request);
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

    /// ★ egui เป็นคนตัดสินว่า pointer เป็นของ canvas หรือของ widget อื่น
    ///
    /// เดิมเราเดาเองด้วย `CanvasRect::contains()` เพราะ `CentralPanel` กิน root rect
    /// จนหมด ทำให้ `response.consumed` ใช้ไม่ได้ (docs/03 §1) — ตอนนี้ canvas เป็น
    /// widget จริงแล้ว เทสต์นี้จึงยิงของจริง: วาง pointer บน panel ซ้ายแล้วบน canvas
    /// แล้วดูว่า widget ตอบต่างกันจริงไหม
    ///
    /// ถ้าข้อนี้พัง คลิกปุ่มบน toolbar จะทะลุไปเลือกภาพข้างหลังด้วย
    #[test]
    fn the_canvas_widget_only_takes_the_pointer_inside_itself() {
        fn pointer_seen_at(pos: egui::Pos2) -> bool {
            let ctx = egui::Context::default();
            let mut state = crate::shell::ShellState::default();
            let board = Board::default();
            let selection = Selection::new();
            let render_state = std::collections::HashMap::new();
            let mut seen = false;

            // สองรอบ: egui ใช้ layout ของรอบก่อนหน้า รอบแรกขนาด panel ยังไม่นิ่ง
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 800.0),
                    )),
                    events: vec![egui::Event::PointerMoved(pos)],
                    ..Default::default()
                };
                let _ = ctx.run_ui(input, |ui| {
                    let _ = crate::shell::draw_in_ui(ui, &mut state, |ui| {
                        let got = RefxApp::canvas_widget(
                            ui,
                            &board,
                            &selection,
                            &render_state,
                            Camera::default(),
                            None,
                        );
                        seen = got.pointer.is_some();
                    });
                });
            }
            seen
        }

        assert!(
            pointer_seen_at(egui::pos2(640.0, 400.0)),
            "กลาง canvas ต้องเป็นของเรา"
        );
        assert!(
            !pointer_seen_at(egui::pos2(60.0, 400.0)),
            "บน Library ต้องไม่ใช่"
        );
        assert!(
            !pointer_seen_at(egui::pos2(1240.0, 400.0)),
            "บน Inspector ต้องไม่ใช่"
        );
        assert!(
            !pointer_seen_at(egui::pos2(640.0, 20.0)),
            "บน toolbar ต้องไม่ใช่"
        );
    }

    // ---------- undo/redo (P2-4 ขั้นที่ 3) ----------

    fn key(text: &str) -> winit::keyboard::Key {
        winit::keyboard::Key::Character(text.into())
    }

    /// ★ Ctrl+Shift+Z ต้องเป็น redo ไม่ใช่ undo
    ///
    /// คนจำนวนมากใช้อันนี้แทน Ctrl+Y (ติดมาจาก Photoshop/Illustrator)
    /// ถ้าไม่รับ เขาจะสรุปว่าโปรแกรมนี้ไม่มี redo
    #[test]
    fn history_shortcuts_cover_what_people_actually_press() {
        let ctrl = ModifiersState::CONTROL;
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;

        assert_eq!(
            history_shortcut(&key("z"), ctrl),
            Some(HistoryRequest::Undo)
        );
        assert_eq!(
            history_shortcut(&key("Z"), ctrl),
            Some(HistoryRequest::Undo),
            "ตัวพิมพ์ใหญ่ก็ต้องได้ (บาง layout ส่งมาแบบนั้น)"
        );
        assert_eq!(
            history_shortcut(&key("z"), ctrl_shift),
            Some(HistoryRequest::Redo),
            "Ctrl+Shift+Z คือ redo ของคนจำนวนมาก"
        );
        assert_eq!(
            history_shortcut(&key("y"), ctrl),
            Some(HistoryRequest::Redo)
        );

        // ไม่กด Ctrl = พิมพ์ตัวอักษรธรรมดา ห้ามไปย้อนงานของผู้ใช้
        assert_eq!(history_shortcut(&key("z"), ModifiersState::empty()), None);
        assert_eq!(history_shortcut(&key("a"), ctrl), None);
    }

    /// ★ กติกาการเลื่อนกล้องหลัง undo — ทดสอบเป็นคณิตศาสตร์ล้วน ไม่ต้องเปิดหน้าต่าง
    ///
    /// เลื่อนเฉพาะตอนของที่เปลี่ยน **มองไม่เห็นเลย** ถ้ายังโผล่อยู่บางส่วน
    /// การกระตุกกล้องน่ารำคาญกว่าประโยชน์
    #[test]
    fn the_camera_only_chases_things_that_are_completely_offscreen() {
        // กรอบที่กล้องเห็นอยู่ (world) — ตรงกับสูตรใน look_at_if_offscreen
        fn visible(camera: &Camera, canvas: Vec2) -> refx_core::geom::Rect {
            let half = canvas * 0.5 / camera.zoom();
            refx_core::geom::Rect {
                min: camera.center() - half,
                max: camera.center() + half,
            }
        }

        let camera = Camera::new(Vec2::ZERO, 1.0);
        let canvas = Vec2::new(800.0, 600.0);
        let view = visible(&camera, canvas);

        let on_screen = refx_core::geom::Rect::from_center_size(Vec2::ZERO, Vec2::splat(50.0));
        assert!(view.intersects(on_screen), "อยู่กลางจอ ไม่ต้องเลื่อน");

        let edge =
            refx_core::geom::Rect::from_center_size(Vec2::new(390.0, 0.0), Vec2::splat(50.0));
        assert!(view.intersects(edge), "โผล่แค่บางส่วนก็ยังไม่ต้องเลื่อน");

        let far =
            refx_core::geom::Rect::from_center_size(Vec2::new(5_000.0, 5_000.0), Vec2::splat(50.0));
        assert!(!view.intersects(far), "อยู่ไกลจนมองไม่เห็น ต้องเลื่อนไปหา");
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
        // รูปแบบต้องตรงกับ docs/05 §6 และต้องเปลี่ยนตามภาษาจริง
        assert_eq!(
            tracker
                .update(stats(1000, 312, 0, 0))
                .map(|progress| progress.label(Lang::Th)),
            Some("กำลังโหลด 312 / 1000".to_owned())
        );
        assert_eq!(
            tracker
                .update(stats(1000, 312, 0, 0))
                .map(|progress| progress.label(Lang::En)),
            Some("Loading 312 / 1000".to_owned())
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
    ///
    /// (พิกัดเคอร์เซอร์มาจาก `response.hover_pos()` ของ egui แล้ว จึงเทียบกับ
    /// กรอบของ widget ตรง ๆ ไม่ต้องผ่าน `CanvasRect` อีก)
    #[test]
    fn cursor_at_canvas_centre_maps_to_camera_centre() {
        let c = CanvasRect::from_points(rect(200.0, 60.0, 800.0, 700.0), 1.0, 1280, 800);
        let camera = Camera::new(Vec2::new(2000.0, 2000.0), 0.25);
        let on_screen = camera.world_to_screen(camera.center(), c.size);
        assert_eq!(on_screen, c.size * 0.5);
    }

    // ---------- Ctrl+V (P1-8) ----------

    fn character(text: &str) -> winit::keyboard::Key {
        winit::keyboard::Key::Character(text.into())
    }

    /// ★ คีย์ลัดที่พังเงียบ ๆ ไม่มีใครเห็นจนกว่าผู้ใช้จะบ่น — บังคับด้วยเทสต์
    #[test]
    fn only_ctrl_v_counts_as_paste() {
        let ctrl = ModifiersState::CONTROL;
        assert!(is_paste(&character("v"), ctrl));
        // Shift ค้างอยู่ด้วย (Ctrl+Shift+V) ยังถือว่าเป็นการวาง
        assert!(is_paste(&character("V"), ctrl | ModifiersState::SHIFT));
        // X11 บาง compositor ส่ง Ctrl+V มาเป็นอักขระควบคุม SYN
        assert!(is_paste(&character("\u{16}"), ctrl));

        // ไม่กด Ctrl = พิมพ์ตัว v เฉย ๆ ห้ามไปวางภาพให้
        assert!(!is_paste(&character("v"), ModifiersState::empty()));
        // ปุ่มอื่นที่กดพร้อม Ctrl
        assert!(!is_paste(&character("c"), ctrl));
        assert!(!is_paste(&character("b"), ctrl));
        // ปุ่มที่ไม่ใช่ตัวอักษร
        assert!(!is_paste(
            &winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter),
            ctrl
        ));
    }

    /// ★ กด `Ctrl+V` รัว ๆ ต้องส่งงานทีละใบ
    ///
    /// ภาพจาก clipboard ใหญ่ได้ระดับ 6000×4000 (96 MB) และ `arboard` จอง RAM
    /// ก้อนนั้นก่อนที่เพดานของเราจะได้ตรวจ ถ้าปล่อยให้ซ้อนกันคือแย่ง RAM
    /// กับ Photoshop ที่ผู้ใช้เปิดคู่กันอยู่ตรง ๆ (ลำดับความสำคัญข้อ 3)
    #[test]
    fn holding_paste_down_only_submits_one_job_at_a_time() {
        let dir = std::env::temp_dir().join(format!("refx-paste-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = RefxApp::new(AppArgs::default());
        app.start_assets(&dir.join("cache.sqlite"));

        let submitted = |app: &RefxApp| app.assets.as_ref().map_or(0, |a| a.pool.stats().submitted);

        app.submit_paste();
        assert_eq!(submitted(&app), 1, "กดครั้งแรกต้องส่งงานหนึ่งใบ");
        assert!(app.paste_in_flight.is_some());

        // กดซ้ำระหว่างที่ใบเดิมยังไม่กลับ — ต้องไม่ส่งเพิ่ม
        for _ in 0..20 {
            app.submit_paste();
        }
        assert_eq!(
            submitted(&app),
            1,
            "กดรัว ๆ แล้วงานซ้อนกัน {} ใบ",
            submitted(&app)
        );

        // ใบเดิมจบแล้วต้องวางใหม่ได้ (ไม่ใช่ล็อกตายไปตลอด)
        app.paste_in_flight = None;
        app.submit_paste();
        assert_eq!(submitted(&app), 2, "ใบเดิมจบแล้วต้องวางใหม่ได้");
    }

    /// ★ ปิดโปรแกรมแล้วต้อง **ตายจริง** ไม่ใช่ค้างเป็นผี
    ///
    /// `window::run` ถือ delegate ไว้แล้ว drop ตอน event loop จบ ถ้าลำดับ drop
    /// ของ [`Assets`] ผิด (`IoThread` ก่อน `io_tx`) การ join เธรด IO จะรอตลอดกาล
    /// ผลที่ผู้ใช้เจอคือ **หน้าต่างหายไปแต่ RefX.exe ยังอยู่** แล้วล็อก single-instance
    /// ค้าง จนเปิดโปรแกรมใหม่ไม่ได้อีกเลย (ยืนยันด้วยมือมาแล้วว่าเกิดจริง 29 ก.ค. 2026)
    #[test]
    fn dropping_assets_finishes_instead_of_hanging_forever() {
        let dir = std::env::temp_dir().join(format!("refx-shutdown-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("cache.sqlite");

        let (tx, rx) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let mut app = RefxApp::new(AppArgs::default());
            app.start_assets(&db);
            drop(app); // ← จุดที่เคยค้าง
            let _ = tx.send(());
        });

        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(30)).is_ok(),
            "drop แล้วไม่จบภายใน 30 วินาที = ปิดโปรแกรมแล้วโปรเซสไม่ตาย"
        );
    }

    /// ภาพที่วางมาจาก clipboard ไม่มีไฟล์ให้ decode ซ้ำ — ต้องไม่ไปขอ working texture
    #[test]
    fn clipboard_items_are_marked_as_having_no_source_file() {
        assert!(refx_asset::pool::JobSource::Clipboard.file().is_none());
        assert!(
            refx_asset::pool::JobSource::File(std::path::PathBuf::from("a.png"))
                .file()
                .is_some()
        );
    }
}
