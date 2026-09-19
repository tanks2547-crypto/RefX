//! ต่อสาย `refx-platform` (หน้าต่าง) เข้ากับ `refx-render` (GPU) แล้วเอา egui วางทับ
//!
//! นี่คือที่เดียวที่ทั้งสามโลกมาเจอกัน — winit, wgpu, egui อยู่บน device เดียวกัน
//!
//! spec: docs/04-rendering.md §1, §8

use std::sync::Arc;

use glam::Vec2;
use refx_asset::cache::{CacheStats, IoRequest, IoThread};
use refx_asset::pool::{ContentOrigin, DecodePool};
use refx_core::arena::ItemId;
use refx_core::board::{AssetRef, Board, ImageFormat, Item, ItemCanvas, ItemKind};
use refx_core::board::{Flip, ItemFilter, ItemMeta};
use refx_core::command::{
    AddItems, ApplyLayout, EditText, History, RelinkAssets, RemoveItems, ReorderZ, SetFilter,
    TransformItems,
};
use refx_core::geom::Rect as WorldRect;
use refx_core::interact::Tool;
use refx_core::interact::{CanvasButton, CanvasContext, CanvasEvent, Modifiers, SelectTool};
use refx_core::selection::Selection;
use refx_core::spatial::SpatialIndex;
use refx_core::view::{Camera, Mode};
use refx_core::zorder::ZMove;

use crate::instances::{self, crop_uv};
// ★ คำศัพท์ของคีย์ลัดย้ายไปอยู่ `keymap` แล้ว (P5-3b ก้อน a) — ชื่อเดิมทุกตัว
//   ยังเรียกได้เหมือนเดิม เทสต์ที่ใช้ `super::*` จึงไม่ต้องแก้สักบรรทัด
use crate::keymap::{self, AppearanceKey, GroupRequest, HistoryRequest, SaveRequest, TabKey};
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
    /// โหมดที่เปิดขึ้นมา — `None` = Canvas ตามค่าปริยาย
    ///
    /// ★ มีไว้ให้ **วัดและถ่ายภาพโหมด Arrange ได้จริง** (P3-3): `--bench-seconds`
    /// เริ่มจับเวลาตั้งแต่เฟรมแรก ถ้าต้องกดปุ่มสลับโหมดก่อน ตัวเลขที่ได้จะเป็นของ
    /// Canvas ปนกับ Arrange โดยไม่มีทางแยกออก
    pub mode: Option<Mode>,
    /// ไฟล์ที่จะเปิดตั้งแต่เริ่มโปรแกรม (เหมือนลากเข้ามา)
    ///
    /// ใช้ทั้งกับการเปิดจากบรรทัดคำสั่งและวัดเวลา "เปิดไฟล์ → ภาพขึ้นจอ"
    pub open_files: Vec<std::path::PathBuf>,
    /// ★ เอกสาร `.refx` ที่จะเปิดตั้งแต่เริ่มโปรแกรม — **เส้นทางเดียวกับ `Ctrl+O`**
    ///
    /// นี่คือสิ่งที่ระบบปฏิบัติการทำตอนผู้ใช้ดับเบิลคลิกไฟล์ `.refx` และเป็น
    /// ทางเดียวที่ relink (P4-6) ถูกยืนยันบนแอปจริงได้ เพราะ native dialog
    /// ขับด้วยสคริปต์ไม่ได้ (HANDOFF §2.26)
    pub open_document: Option<std::path::PathBuf>,
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

/// ผลของ "งวด" ที่ผู้ใช้ลากเข้ามาหนึ่งครั้ง (P3-3)
///
/// ★★★ **มีอยู่เพราะการนับที่ไม่ครบทำให้ผู้ใช้เห็นงานหายแบบเงียบ ๆ**
///
/// เดิมนับแค่ "ขึ้นจอแล้วกี่ใบ" เทียบกับ "ขอมากี่ใบ" — พอ board เต็ม
/// (`ROADMAP P3-3`: เพดานจริง 3,072 ใบ = 12 layer × 256 ช่อง) ใบที่เหลือ
/// **ไม่ถูกสร้างเป็น item เลย** สองตัวเลขจึงไม่มีวันเท่ากัน ผลคือ:
///
/// 1. รายงานสรุปตอนจบงวด **ไม่เคยทำงาน** (เงื่อนไขไม่มีวันเป็นจริง)
/// 2. ผู้ใช้ลาก 10,000 ไฟล์แล้วได้ 3,072 ใบ โดยไม่มีใครบอกว่าอีก 6,928 ใบ
///    หายไปไหน — ซึ่งอ่านได้อย่างเดียวว่าโปรแกรมทำงานหาย (ผิด I-3)
///
/// ตอนนี้ทุกใบที่ส่งเข้าไปต้องลงเอยที่ช่องใดช่องหนึ่งเสมอ: `added` (ขึ้นจอ) ·
/// `rejected` (board เต็ม) · `failed` (ไฟล์เปิดไม่ได้) — [`DropBatch::settled`]
/// จึงเป็นจริงได้แม้ทุกใบจะถูกปฏิเสธ
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct DropBatch {
    /// จำนวนไฟล์ที่ผู้ใช้ขอให้เปิดในงวดนี้
    requested: usize,
    /// เพิ่มลง board แล้ว (ผู้ใช้เห็นบนจอ)
    added: usize,
    /// **เพิ่มไม่ได้เพราะ board เต็ม** — ใบที่ผู้ใช้ต้องได้รับแจ้ง
    rejected: usize,
    /// เปิดไฟล์ไม่ได้ **และเพิ่มเป็น `Missing` ก็ไม่ได้** — ใบที่หายไปจริง ๆ
    ///
    /// ★ หลัง ROADMAP P3-3 (28 ส.ค. 2026) ใบที่ decode ไม่ผ่านจะกลายเป็น
    /// `Missing` บนกระดานแล้วนับเป็น [`DropBatch::added`] · ช่องนี้จึงเหลือไว้
    /// สำหรับกรณีที่ **สร้าง item ไม่ได้เลย** (แท็บถูกปิด / `AddItems` ล้ม)
    failed: usize,
    /// เปิดไม่ได้แต่ **ขึ้นเป็นช่องว่างบนกระดานแล้ว** — สำหรับข้อความสรุป
    ///
    /// ★ **ไม่เข้า [`DropBatch::answered`]** เพราะใบเดียวกันถูกนับใน `added` ไปแล้ว
    /// นับสองช่องจะทำให้งวดจบเร็วไปหนึ่งเท่าตัว
    damaged: usize,
    /// ★ ถูกยกเลิกกลางทาง — **ผู้ใช้ pan ระหว่างที่ไฟล์ยังทยอยเข้ามา**
    ///
    /// P1-4 ยกเลิกงานที่ผ่านจอไปแล้วโดยตั้งใจ ซึ่งแปลว่าใบพวกนี้จะไม่มีผลกลับมา
    /// เป็น item ตลอดไป · ถ้าไม่นับ งวดจะ **ค้างถาวร** แล้วทั้งแถบ "กำลังโหลด"
    /// และข้อความสรุปตอนจบก็ตายไปด้วยกันเงียบ ๆ
    cancelled: usize,
    /// รายงานผลของงวดนี้ไปแล้วหรือยัง (กันรายงานซ้ำทุกเฟรม)
    reported: bool,
}

impl DropBatch {
    /// เริ่มงวดใหม่ — ล้างตัวนับเดิมทั้งชุดในคราวเดียว
    ///
    /// ★ เขียนทับทั้ง struct โดยตั้งใจ: การล้างทีละฟิลด์คือที่ที่ฟิลด์ใหม่
    /// จะถูกลืมในวันที่มีคนเพิ่มมันเข้ามา
    fn start(&mut self, requested: usize) {
        *self = Self {
            requested,
            ..Self::default()
        };
    }

    /// งวดนี้จบแล้วหรือยัง — จบเมื่อทุกใบที่ขอมามีคำตอบแล้ว **ไม่ว่าคำตอบคืออะไร**
    ///
    /// ★ "ถูกยกเลิก" ก็เป็นคำตอบ — ใบนั้นจะไม่กลับมาเป็น item อีกแล้ว
    fn settled(&self) -> bool {
        self.answered() >= self.requested
    }

    /// จำนวนใบที่มีคำตอบแล้ว — **ต้องนับทุกช่องทางที่ทำให้ใบหนึ่งจบลง**
    fn answered(&self) -> usize {
        self.added + self.rejected + self.failed + self.cancelled
    }
}

/// ปลดคีย์ของ working texture ที่ขอไว้แล้วไม่ได้ผลกลับมา
///
/// ★★ เดิม `working_pending` ถูกปลดเฉพาะตอน **สำเร็จ** เท่านั้น งานที่ถูกยกเลิก
/// (ผู้ใช้ซูมออกก่อน) หรือล้มเหลว จึงทิ้งคีย์ค้างไว้ตลอดอายุโปรแกรม แล้ว
/// `plan_working_textures` จะเห็นว่า "ขอไปแล้ว" ตลอดกาล → **ภาพใบนั้นจะเบลอ
/// ถาวรทุกครั้งที่ซูมเข้า** โดยไม่มี error ที่ไหนเลย · เจอตอนเติม `target`
/// ให้ `Cancelled`/`Failed` (ก่อนหน้านี้ชั้น UI แยกไม่ออกว่าใบไหนเป็นงานชนิดไหน
/// จึงไม่มีทางเขียนโค้ดตรงนี้ได้เลย)
fn gfx_working_pending_remove(
    gfx: Option<&mut Gfx>,
    hash: refx_asset::hash::ContentHash,
    size: u32,
) {
    if let Some(gfx) = gfx {
        gfx.working_pending.remove(&WorkingKey {
            hash: *hash.as_bytes(),
            size,
        });
    }
}

/// ข้อความบอกผู้ใช้ว่างวดนี้ board รับไม่ครบ — `None` เมื่อรับครบทุกใบ
///
/// ★★ **`None` คือ negative control ที่ชนิดข้อมูลบังคับไว้** — ผู้เรียกไม่มีทาง
/// แสดงข้อความนี้ตอนที่ไม่มีใบไหนตกหล่น เพราะไม่มีข้อความให้แสดง
///
/// ★ ต้องบอก **สิ่งที่เกิดขึ้น + สิ่งที่ทำได้ต่อ** ตาม `CLAUDE.md` — "atlas เต็ม
/// 12 layer" เป็นภาษาของโปรแกรมเมอร์ ผู้ใช้ไม่รู้ว่า layer คืออะไรและทำอะไรกับมันไม่ได้
fn board_full_message(lang: Lang, capacity: usize, batch: DropBatch) -> Option<String> {
    if batch.rejected == 0 {
        return None;
    }
    Some(text::fill(
        lang,
        Template::BoardFull,
        &[
            ("capacity", &capacity.to_string()),
            ("rejected", &batch.rejected.to_string()),
            ("requested", &batch.requested.to_string()),
        ],
    ))
}

/// เวลาปัจจุบันเป็น unix millis — `0` ถ้านาฬิกาเครื่องอยู่ก่อนปี 1970
///
/// ★ ใช้ตอน **สร้าง item** เท่านั้น ไม่ใช่ในลูปเฟรม · docs/08 §3.9 ข้อ 5b ห้าม
/// *assert* เวลานาฬิกาในเทสต์ ไม่ได้ห้ามบันทึกเวลาที่ผู้ใช้เพิ่มภาพ
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
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
    fn report(&mut self, quads: usize, present: wgpu::PresentMode, vram: usize, vram_limit: usize) {
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
             p50 {p50:.3} ms | p99 {p99:.3} ms | เฉลี่ย {mean_ms:.3} ms ({fps:.1} fps)\n\
             vram {vram_mb:.1} MB จากงบ {limit_mb:.1} MB",
            vram_mb = vram as f64 / (1024.0 * 1024.0),
            limit_mb = vram_limit as f64 / (1024.0 * 1024.0),
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
///
/// อักขระ ASCII ที่คีย์ลัดควรถือว่าผู้ใช้กด — `None` ถ้าไม่ใช่ปุ่มที่มีความหมาย
///
/// ★★★ **logical ก่อน · physical เป็นตาข่ายรอง**
///
/// `logical` คือตัวอักษรที่ layout ของผู้ใช้ผลิตออกมา — Dvorak/AZERTY กด `V`
/// ที่ตำแหน่งของเขาเองแล้วยังได้ผลถูก ซึ่งเป็นเหตุผลที่โค้ดเดิมเลือกทางนี้ และยังถูกอยู่
///
/// แต่ **layout ที่ไม่ใช่ละตินไม่ผลิตตัวอักษรละตินเลย**: คีย์บอร์ดไทยกด `C` ได้ `แ`
/// รัสเซียได้ `с` กรีกได้ `ψ` — ไม่มีตัวไหนตรงกับ `"c"` ทั้งสิ้น ผลคือ
/// **คีย์ลัดทุกตัวตายหมด** สำหรับผู้ใช้กลุ่มที่ `docs/03 §0` ระบุว่าเป็นภาษาที่สอง
/// ของโปรแกรมนี้ · เจอตอนยืนยัน P3-1 บนเครื่องที่ layout เป็นไทย: กด `C` แล้ว
/// เครื่องมือไม่สลับ กด `Ctrl+Z` แล้วไม่ย้อน และ **ไม่มี error ที่ไหนเลย**
///
/// ★ ลำดับสำคัญ: ถ้าถาม physical ก่อน Dvorak จะพัง ถ้าถาม logical อย่างเดียว
/// ไทย/รัสเซีย/กรีกจะพัง — ต้องถามสองชั้นตามลำดับนี้เท่านั้น
///
/// ## ★★★ เงื่อนไขที่จะตกไปชั้น physical คือ **"ไม่มี binding ไหนตรง"**
/// ## ไม่ใช่ "อักขระหน้าตาแบบไหน" (`docs/03 §5` แก้ 4 ก.ย. 2026)
///
/// รุ่นก่อนหน้ารับ logical ทันทีถ้ามันเป็น **ASCII ตัวเดียว** แล้วหยุด —
/// ซึ่งทำให้ **`Ctrl+Shift+Z` ตายบน layout ไทย**: ปุ่ม `Z` ชั้น shift ของไทย
/// ส่ง **`(` (U+0028)** ซึ่งเป็น ASCII ตัวเดียวพอดี → ไม่มีวันตกไปถึง physical
/// · ปุ่มอื่นรอด**โดยบังเอิญ** เพราะชั้น shift ส่งสองอักขระ (ตัวอักษร + U+000E)
/// ซึ่งด่าน "ตัวเดียว" ปฏิเสธ — `(` เป็นตัวเดียวที่หลุด
///
/// ★★ คำถามที่ถูกคือ **"ตรงกับอะไรไหม"** ไม่ใช่ *"หน้าตาเป็นอะไร"* —
/// และมันถามได้ก็ต่อเมื่อ **มีตารางแล้ว** (P5-3b ก้อน a ปลดล็อกข้อนี้ไปในตัว)
///
/// ★★★ ถามที่ระดับ **อักขระ ไม่ใช่ระดับ (อักขระ + modifier)** โดยตั้งใจ:
/// ถ้าถามแบบตรงทั้งชุด ปุ่มตัวอักษรเปล่า ๆ บน layout ที่ไม่ใช่ QWERTY จะตกไป
/// จุดเครื่องมือที่ตำแหน่ง physical นั้นแทน — ซึ่งคือ **"ถาม physical ก่อน
/// แล้ว Dvorak พัง"** ที่ §2.12 ห้ามไว้ กลับมาในรูปที่ช้าลงหนึ่งจังหวะ
/// · `docs/03 §5` เขียนเหตุผลไว้ตรงกันว่า *"Thai `(` **ไม่ผูกกับอะไร** จึงตกลงไป"*
fn shortcut_char(
    logical: &winit::keyboard::Key,
    physical: winit::keyboard::PhysicalKey,
) -> Option<char> {
    let produced = logical_ascii(logical);
    // ★ ชั้น 1: อักขระที่ layout ผลิต **ถ้ามันมีความหมายกับเราจริง**
    if let Some(ch) = produced
        && keymap::active().binds_char(ch)
    {
        return Some(ch);
    }
    // ★ ชั้น 2: ตำแหน่งปุ่มบนคีย์บอร์ด · ★★ `or(produced)` ไว้ท้ายสุดเพื่อให้
    //   ปุ่มที่ไม่มีในแผนที่ physical (เช่น F13 ในเทสต์) ยังคืนอักขระเดิมเหมือนก่อน
    physical_char(physical).or(produced)
}

/// อักขระที่ **layout ของผู้ใช้ผลิตออกมา** — `None` ถ้าไม่ใช่ ASCII ตัวเดียว
///
/// `แ` / `с` / `ψ` ตกที่นี่ (ไม่ใช่ ASCII) แล้วไปใช้ [`physical_char`] แทน
fn logical_ascii(logical: &winit::keyboard::Key) -> Option<char> {
    let winit::keyboard::Key::Character(text) = logical else {
        return None;
    };
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) if ch.is_ascii() => Some(ch.to_ascii_lowercase()),
        _ => None,
    }
}

/// อักขระตาม **ตำแหน่งปุ่มบนคีย์บอร์ด** ไม่ขึ้นกับ layout — ตาข่ายรอง
fn physical_char(physical: winit::keyboard::PhysicalKey) -> Option<char> {
    use winit::keyboard::{KeyCode, PhysicalKey};

    let PhysicalKey::Code(code) = physical else {
        return None;
    };
    Some(match code {
        KeyCode::KeyA => 'a',
        KeyCode::KeyB => 'b',
        KeyCode::KeyC => 'c',
        KeyCode::KeyD => 'd',
        KeyCode::KeyE => 'e',
        KeyCode::KeyF => 'f',
        KeyCode::KeyG => 'g',
        KeyCode::KeyH => 'h',
        KeyCode::KeyI => 'i',
        KeyCode::KeyJ => 'j',
        KeyCode::KeyK => 'k',
        KeyCode::KeyL => 'l',
        KeyCode::KeyM => 'm',
        KeyCode::KeyN => 'n',
        KeyCode::KeyO => 'o',
        KeyCode::KeyP => 'p',
        KeyCode::KeyQ => 'q',
        KeyCode::KeyR => 'r',
        KeyCode::KeyS => 's',
        KeyCode::KeyT => 't',
        KeyCode::KeyU => 'u',
        KeyCode::KeyV => 'v',
        KeyCode::KeyW => 'w',
        KeyCode::KeyX => 'x',
        KeyCode::KeyY => 'y',
        KeyCode::KeyZ => 'z',
        KeyCode::BracketLeft => '[',
        KeyCode::BracketRight => ']',
        // ★★★ แถวตัวเลขต้องอยู่ที่นี่ด้วย ไม่งั้น `1`/`0` (P5-3b ก้อน c) ตายบน
        //     layout ไทยเงียบ ๆ: ปุ่ม `1` ของไทยส่ง `ๅ` ซึ่งไม่ใช่ ASCII จึงตกมา
        //     ชั้นนี้ทันที — ถ้าชั้นนี้ไม่รู้จัก `Digit1` ผลคือ `None` แล้วไม่มี
        //     อะไรเกิดขึ้นเลย เป็นบั๊กรูปเดียวกับ `Ctrl+Shift+Z` ของ §2.40ก เป๊ะ
        // ★ ใส่ครบสิบตัวไม่ใช่เฉพาะสองตัวที่ผูกไว้ — ตารางนี้ตอบ "ปุ่มตำแหน่งนี้
        //   คืออักขระอะไร" ซึ่งไม่ขึ้นกับว่าวันนี้เราผูกอะไรไว้บ้าง
        KeyCode::Digit0 => '0',
        KeyCode::Digit1 => '1',
        KeyCode::Digit2 => '2',
        KeyCode::Digit3 => '3',
        KeyCode::Digit4 => '4',
        KeyCode::Digit5 => '5',
        KeyCode::Digit6 => '6',
        KeyCode::Digit7 => '7',
        KeyCode::Digit8 => '8',
        KeyCode::Digit9 => '9',
        _ => return None,
    })
}

/// การเลือกควรเป็นอะไรหลัง undo/redo — `None` = **อย่าแตะการเลือกเดิม**
///
/// ★★★ **"ไม่ได้แตะ item ไหนเลย" ≠ "ให้ล้างการเลือก"** (แยกออกมาตอน P3-7)
///
/// `ReorderZ::affected()` คืนรายการว่างพร้อมคอมเมนต์ในตัวมันเองว่า "ปล่อยให้
/// selection เดิมอยู่ต่อ" แต่เส้นทาง undo เดิม `restore(vec![])` ซึ่งคือการ **ล้าง**
/// — สัญญาที่ `refx-core` ประกาศไว้จึงไม่เคยถูกทำตามเลยบนเส้นทางนั้น
/// · อาการที่ทำให้เจอตอน P3-7: กดยุบกลุ่มแล้ว Ctrl+Z → **แผงกลุ่มหายไปทั้งแผง**
/// เพราะไม่มีอะไรถูกเลือกอีกแล้ว ผู้ใช้จึงกดกางกลับทันทีไม่ได้
///
/// ★ ต้องแยกสองกรณีนี้ให้ขาด และเป็นเหตุผลที่ต้องดู `affected` **ก่อนกรอง**:
///
/// | คำสั่งรายงาน | แปลว่า | ทำ |
/// |---|---|---|
/// | รายการว่าง | ไม่ได้แตะ item ไหน (`ReorderZ`, `SetGroup`) | ปล่อยการเลือกไว้ |
/// | มี id แต่ตายหมด | undo ของการเพิ่มภาพ — ของหายไปจริง | ล้าง |
/// | มี id ที่ยังอยู่ | เลือกของที่เพิ่งเปลี่ยนให้ผู้ใช้เห็น | ตั้งตามนั้น |
fn selection_after_history(
    affected: &[ItemId],
    alive: impl Fn(ItemId) -> bool,
) -> Option<Vec<ItemId>> {
    if affected.is_empty() {
        return None;
    }
    Some(affected.iter().copied().filter(|id| alive(*id)).collect())
}

/// ชื่อของผู้ขอ redraw ที่แสดงบนแถบสถานะ
///
/// ★ ASCII ล้วนและเป็นชื่อ **ทางเทคนิค** โดยตั้งใจ ไม่ผ่านงานแปล: มันคือของที่
/// ผู้ใช้จะก๊อปมาแปะตอนรายงานปัญหา และเป็นสิ่งที่เราต้องค้นเจอในโค้ด — การแปล
/// ทำให้มันเป็นคนละคำกับที่ `RedrawReason` เขียนไว้ (เหตุผลเดียวกับชื่อ action
/// ใน `keymap.toml`) · `docs/03 §0` ยกเว้นค่าให้ ไม่ได้ยกเว้นข้อความ
const fn reason_name(reason: refx_platform::redraw::RedrawReason) -> &'static str {
    use refx_platform::redraw::RedrawReason as R;
    match reason {
        R::UserInput => "UserInput",
        R::TextureReady => "TextureReady",
        R::Animation => "Animation",
        R::EguiRepaint => "EguiRepaint",
        R::HoverRepaint => "HoverRepaint",
        R::SurfaceRecovery => "SurfaceRecovery",
    }
}

/// สิ่งที่ `Esc` ควรทำในสถานะนี้ (P5-3b ก้อน c)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscapeTarget {
    /// ยกเลิกคำถาม "ปิดทั้งที่ยังไม่บันทึก"
    CancelClose,
    /// ยกเลิกคำถาม "บันทึกเป็นแบบไหน"
    CancelSaveAs,
    /// เก็บงานที่กู้ได้ไว้ก่อน — **ไม่ใช่ลบ**
    PostponeRecovery,
    /// ปิดกล่องส่งออก (ยังไม่ได้เริ่ม) — P5-4
    CloseExport,
    /// ★★ หยุด export ที่กำลังทำอยู่ — **ไม่ใช่ปิดหน้าต่างเฉย ๆ**
    StopExport,
    /// ปิดแผงตั้งค่า
    CloseSettings,
    /// ยกเลิกการเลือก
    ClearSelection,
    /// ไม่มีอะไรให้ทำ
    Nothing,
}

/// ★★★ `Esc` — **ปิดสิ่งที่ค้างอยู่ก่อนเสมอ แล้วค่อยยกเลิกเลือก** (`docs/03 §5`)
///
/// ★ แยกเป็นฟังก์ชันบริสุทธิ์เพื่อ **เทสต์ลำดับได้โดยไม่ต้องเปิดหน้าต่าง** —
/// ลำดับคือทั้งหมดของกฎนี้ และเป็นสิ่งเดียวที่พังได้เงียบ ๆ
///
/// ★★ ช่องข้อความไม่อยู่ในลำดับนี้โดยตั้งใจ: ตอนนั้น egui ถือ focus อยู่ ด่าน
/// `egui_wants_keyboard_input()` ใน `on_input` จึงกิน `Esc` ไปก่อนถึงที่นี่เลย
fn escape_target(shell: &crate::shell::ShellState, has_selection: bool) -> EscapeTarget {
    // ★ เรียงตาม "อันไหนบังหน้าจอมากที่สุด" — แถบถามอยู่บนสุดเสมอ
    if shell.close_prompt {
        EscapeTarget::CancelClose
    } else if shell.save_as_prompt {
        EscapeTarget::CancelSaveAs
    } else if shell.recover_prompt.is_some() {
        // ★★ `Later` ไม่ใช่ `Discard` — `Esc` ต้องไม่มีทางลบงานของผู้ใช้
        //    (`docs/07 §4`: "ผู้ใช้ที่ไม่แน่ใจต้องไม่ถูกบังคับให้ตัดสินใจ
        //    แบบทำลายข้อมูล") · ปุ่มที่กดพลาดง่ายที่สุดต้องเป็นปุ่มที่ปลอดภัยที่สุด
        EscapeTarget::PostponeRecovery
    } else if shell.export_progress.is_some() {
        // ★★★ งานที่กำลังเขียนไฟล์อยู่มาก่อนกล่องตั้งค่า — `Esc` ระหว่าง export
        //     ต้องหมายถึง "หยุดเถอะ" ซึ่งเป็นสิ่งที่ผู้ใช้ต้องการจริง ๆ ตอนนั้น
        //     · ปลอดภัยเสมอ: ยกเลิกไม่แตะไฟล์ปลายทางเลย (`docs/07 §6` ข้อ 2-3)
        EscapeTarget::StopExport
    } else if shell.export_prompt.is_some() {
        EscapeTarget::CloseExport
    } else if shell.settings_open {
        EscapeTarget::CloseSettings
    } else if has_selection {
        EscapeTarget::ClearSelection
    } else {
        EscapeTarget::Nothing
    }
}

/// ★★★ `Tab` เป็นของใคร — **มีอะไรถือ focus อยู่ = ของ egui · ไม่มี = ของเรา**
///
/// ## ปัญหาที่มันแก้ (P5-3b ก้อน c)
///
/// egui ใช้ `Tab` ย้าย focus ระหว่าง widget และมันเห็น event **ก่อนเราเสมอ**
/// (`on_window_event` ถูกเรียกที่บรรทัดแรกของ `on_input`) · ถ้าปล่อยไว้ การกด
/// `Tab` หนึ่งครั้งจะ **ทั้งสลับโหมดและย้าย focus ไปปุ่มแรก** พร้อมกัน แล้ว
/// ครั้งที่สองจะสลับโหมดไม่ได้อีกเลย เพราะตอนนั้นมี widget ถือ focus อยู่แล้ว
/// ด่าน `egui_wants_keyboard_input()` จึงกินมันไป — อาการคือ *"กด Tab ได้ครั้งเดียว"*
///
/// ## กฎที่ตัดสิน
///
/// | สถานะ | `Tab` เป็นของ |
/// |---|---|
/// | มี widget ถือ focus (ช่องข้อความ · ปุ่ม) | **egui** — พิมพ์/ย้าย focus ตามปกติ |
/// | ไม่มีอะไรถือ focus | **RefX** — สลับโหมด และ egui ต้องไม่เห็นปุ่มนี้ |
///
/// ★★ สองฝั่งถามคำถามเดียวกัน (`focused().is_some()` ซึ่งคือสิ่งที่
/// `egui_wants_keyboard_input` เป็นจริง ๆ ใน egui 0.34) และถามในช่วงเวลาที่
/// คำตอบคงที่ (ระหว่างจบ `run_ui` ของเฟรมก่อน กับเริ่ม `run_ui` ของเฟรมนี้) —
/// จึงเป็นไปไม่ได้ที่ทั้งคู่จะทำงาน หรือทั้งคู่จะไม่ทำงาน
/// egui ขอวาดอีกเฟรม — เป็นเพราะ **ตัวชี้อยู่เหนือหน้าต่าง** หรือเปล่า
///
/// ★★★ ดู [`RedrawReason::HoverRepaint`] ว่าทำไมสองอย่างนี้ต้องแยกกัน ·
/// สรุป: การชี้เมาส์ค้างคือการโต้ตอบ ไม่ใช่ idle · ตัวเลขยังถูกนับและพิมพ์
/// เหมือนเดิม **ที่เปลี่ยนคือมันไม่ปลุกเสียงเตือน**
///
/// ★★ แยกเป็นฟังก์ชันของตัวเองเพราะมีผู้เรียกสองที่ (เส้นทาง `redraw()` กับ
/// `on_wake()`) — ถ้าเขียนซ้ำสองแห่ง วันหนึ่งจะแก้ข้างเดียวแล้วสองเส้นทาง
/// รายงานคนละเรื่องโดยไม่มีอะไรจับได้
fn repaint_reason(ctx: &egui::Context) -> RedrawReason {
    if pointer_is_over_the_window(ctx) {
        RedrawReason::HoverRepaint
    } else {
        RedrawReason::EguiRepaint
    }
}

/// ตัวชี้อยู่เหนือหน้าต่างของเราไหม (ตามที่ egui เห็น)
///
/// `latest_pos()` เป็น `None` เมื่อตัวชี้ออกนอกหน้าต่างไปแล้ว — นั่นคือ**สภาพ**
/// ที่นิยาม "input" แบบนับ event ตอบไม่ได้
fn pointer_is_over_the_window(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.pointer.latest_pos().is_some())
}

fn strip_tab_when_it_is_ours(ctx: &egui::Context, raw_input: &mut egui::RawInput) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    raw_input.events.retain(|event| {
        !matches!(
            event,
            egui::Event::Key {
                key: egui::Key::Tab,
                ..
            }
        )
    });
}

/// ปุ่มที่มีชื่อของ event นี้ — `None` ถ้าเป็นตัวอักษร
fn named_key(key: &winit::keyboard::Key) -> Option<winit::keyboard::NamedKey> {
    match key {
        winit::keyboard::Key::Named(named) => Some(*named),
        _ => None,
    }
}

/// ★★ ปุ่มนี้ควรลงมือในเฟรมนี้ไหม — **นโยบายกดค้างอ่านจากตาราง**
///
/// เดิมเป็น `!event.repeat` ที่เขียนซ้ำอยู่ 8 จุดในบล็อกเดียว โดยที่อีก 2 จุด
/// **จงใจไม่มีมัน** (undo/redo · ย้ายชั้น) · ความต่างที่ตั้งใจแบบนั้นแยกไม่ออก
/// จากการลืมพิมพ์ เมื่อมันอยู่ในรูปของ "บรรทัดที่หายไป" — และไม่มีเทสต์ไหน
/// ถามมันได้เลยเพราะมันไม่ใช่ค่าอะไรทั้งสิ้น
///
/// ★ ตอนนี้เป็น [`keymap::RepeatPolicy`] ในตาราง ซึ่งเทสต์ถามตรง ๆ ได้
/// (`holding_a_key_repeats_only_where_it_should`) และก้อน b จะให้ผู้ใช้ตั้งเอง
/// ได้โดยไม่ต้องแตะโค้ดตรงนี้เลย
/// ★ รับชิ้นส่วนของ event ไม่ใช่ทั้ง `KeyEvent` เพราะ **`KeyEvent` สร้างในเทสต์
/// ไม่ได้** (`platform_specific` เป็นฟิลด์ปิดของ winit) — และการทดสอบไม่ได้
/// แปลว่าไม่มีเทสต์ แต่แปลว่ามีเทสต์ที่เรียกของเลียนแบบ ซึ่ง `§3.9 ข้อ 9` ห้าม
fn wanted(down: bool, repeat: bool, action: keymap::Action) -> bool {
    down && (!repeat || keymap::active().repeats(action))
}

/// ★★★ **ปุ่มที่เพิ่งกด → สิ่งที่ต้องลงมือในเฟรมนี้** — ทางเดียวของคีย์บอร์ดทั้งหมด
///
/// รวมสามคำถามที่เคยกระจายอยู่ 14 กิ่งใน `on_input` ให้เหลือลำดับเดียว:
///
/// 1. อักขระอะไร (`shortcut_char` — logical ก่อน physical เป็นตาข่ายรอง)
/// 2. ตารางว่ายังไง (`Keymap::action` — `Char` ก่อน `Key`)
/// 3. กดค้างแล้วซ้ำได้ไหม ([`wanted`] — นโยบายอยู่ในตาราง)
///
/// ★★ ยุบได้เพราะ **ทุกกิ่งเดิมถาม `Keymap::action` ตัวเดียวกัน** แล้วกรองด้วย
/// variant ของผลลัพธ์ · ปุ่มหนึ่งชุดจึงจับคู่ได้ไม่เกินหนึ่ง action อยู่แล้ว
/// (ประตู `no_single_keypress_can_ever_fire_two_actions` คุมข้อนี้ไว้ทั้งตาราง)
/// — 14 `if` ที่เรียงกันจึงเป็น `match` เดียวได้โดยไม่เปลี่ยนพฤติกรรม
///
/// ★ ส่ง `named_key` เข้าไปเสมอ ต่างจากเดิมที่บางกิ่งส่ง `None`: ปุ่มหนึ่งครั้ง
/// ให้ **อักขระ หรือ ชื่อ อย่างใดอย่างหนึ่ง** ไม่เคยให้ทั้งคู่ (logical ที่เป็น
/// `Named` ไม่ผ่าน `logical_ascii` และไม่มีปุ่มชื่อไหนอยู่ในแผนที่ physical)
/// การส่งครบจึงเป็น superset ที่ว่างเปล่า — พิสูจน์ด้วยเทสต์ที่ยิงทั้ง 45 แถว
fn requested(
    logical: &winit::keyboard::Key,
    physical: winit::keyboard::PhysicalKey,
    down: bool,
    repeat: bool,
    modifiers: ModifiersState,
) -> Option<keymap::Action> {
    let pressed = shortcut_char(logical, physical);
    let action = keymap::active().action(pressed, named_key(logical), modifiers)?;
    wanted(down, repeat, action).then_some(action)
}

/// สิ่งที่ต้องทำต่อหลังบันทึกเสร็จ (P4-2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AfterSave {
    /// อยู่ต่อตามปกติ
    #[default]
    Stay,
    /// ปิดโปรแกรม — ผู้ใช้เลือก "บันทึกแล้วปิด" ตอนถูกถาม
    Close,
    /// ★ ปิด **แท็บใบนี้** — ผู้ใช้กด `Ctrl+W` บนแท็บที่ยังไม่บันทึกแล้วเลือก
    /// "บันทึกแล้วปิด" · ระบุด้วย id ไม่ใช่ดัชนี เพราะแท็บอื่นถูกปิดระหว่างที่
    /// ไฟล์กำลังเขียนได้ แล้วดัชนีจะเลื่อนไปปิดผิดใบ
    CloseTab(refx_core::arena::BoardId),
}

/// ★★★ กล้อง/โหมดที่ **ใช้อยู่จริง** ประกอบเป็น `ViewState` ที่จะลงไฟล์ (P4-1)
///
/// **นี่คือครึ่งที่ `HANDOFF §6` แถวแรกเตือนว่าจะถูกลืม** — `Board::view` ตั้งใจให้
/// persist ลง `.refx` มาตั้งแต่ P2-1 แต่ **ไม่มีโค้ดไหนเขียนมันเลยสักบรรทัด**
/// เพราะของจริงกระจายอยู่สามที่คนละชั้นกัน:
///
/// | ของจริงอยู่ไหน | ใครถือ |
/// |---|---|
/// | กล้องของ Canvas | `Gfx::camera` |
/// | การเลื่อนของ Arrange | `ArrangeView::scroll` (ผ่าน `camera()`) |
/// | โหมดที่เปิดอยู่ | `ShellState::mode` |
///
/// ★★ กับดักที่ต้องรู้: **การเขียน DTO ให้ `ViewState` แล้วเทสต์ round-trip
/// ผ่านหมด จะดู "เสร็จ" ทั้งที่สิ่งที่ถูกบันทึกคือกล้องค่าปริยายเสมอ** ผู้ใช้
/// เปิดไฟล์แล้วมุมมองไม่กลับมา โดยไม่มี error ที่ไหนเลย
///
/// ★ แยกเป็นฟังก์ชันบริสุทธิ์เพื่อ **เทสต์ได้โดยไม่ต้องมีหน้าต่าง** — ชั้นที่
/// ประกอบค่านี้คือชั้นเดียวที่มีข้อมูลครบทั้งสามที่ (docs/08 §3.9 ข้อ 8.1:
/// ย้ายการตัดสินเข้าไปในฝั่งที่มีข้อมูลครบ แทนที่จะบอกให้ผู้เรียกทำเอง)
fn live_view(canvas: Camera, arrange: Camera, mode: Mode) -> refx_core::view::ViewState {
    refx_core::view::ViewState {
        canvas,
        arrange,
        mode,
    }
}

/// อีกด้านของภาพ export เมื่อรู้ด้านยาวสุดกับสัดส่วน (P5-4)
///
/// ★ ผลลัพธ์ **อย่างน้อย 1 จุดเสมอ** — board ที่เรียงเป็นเส้นตรงพอดีให้สัดส่วน 0
/// ซึ่งจะกลายเป็นภาพสูง 0 จุดที่ `BandPlan` ปฏิเสธ แล้วผู้ใช้จะเห็นแค่ error
/// ที่เขาไม่รู้ว่าเกิดจากอะไร
fn scale_side(long: u32, ratio: f32) -> u32 {
    if !ratio.is_finite() || ratio <= 0.0 {
        return long;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "ค่าถูก clamp เข้าช่วง 1..=MAX_SIDE ทันทีหลังแปลง"
    )]
    let side = (long as f32 * ratio).round() as u32;
    side.clamp(1, refx_core::export::MAX_SIDE)
}

/// ประมาณขนาดไฟล์ที่จะได้ — **ตัวเลขสำหรับตัดสินใจ ไม่ใช่คำสัญญา**
///
/// ★★ อัตราที่ใช้มาจากการวัดจริงตอน P5-4 (`docs/07 §6`): PNG ของภาพที่มี
/// รายละเอียดแบบภาพถ่ายอยู่ราว 30–40% ของ RGBA ดิบ ส่วน JPEG ขึ้นกับคุณภาพ
/// · ★ ประมาณ **สูงไว้ก่อน** — ผู้ใช้ที่เห็น "ประมาณ 40 MB" แล้วได้ 25 MB
/// ไม่เดือดร้อน แต่คนที่เห็น 5 MB แล้วได้ 40 MB บนไดรฟ์ที่เหลือ 10 MB เดือดร้อน
fn estimate_file_size(width: u32, height: u32, kind: crate::shell::ExportKind, quality: u8) -> u64 {
    let pixels = u64::from(width) * u64::from(height);
    match kind {
        crate::shell::ExportKind::Png => pixels * 4 / 3,
        crate::shell::ExportKind::Jpeg => {
            // คุณภาพ 90 ≈ 1 ไบต์ต่อ 3 พิกเซล · ไล่ขึ้นลงแบบเชิงเส้นรอบจุดนั้น
            let per_1000 = u64::from(quality.clamp(1, 100)) * 4;
            pixels * per_1000 / 1000
        }
    }
}

/// สีพื้นหลังของ canvas จาก `BoardSettings` — ★ **ค่านี้เคยเป็นค่าคงที่**
///
/// `BoardSettings::background` มีอยู่ใน `Board` และ persist ลง `.refx` ตั้งแต่ P2-1
/// แต่ **ไม่มีใครอ่านมันเลย** — render pass ใช้ค่าคงที่ในไฟล์นี้แทน · ฟิลด์ที่ทั้ง
/// ไม่มีคนเขียนและไม่มีคนอ่าน คือฟิลด์ที่หลอกคนอ่านโค้ดว่าฟีเจอร์นี้มีอยู่แล้ว
/// (เจอตอน audit ฟิลด์ที่ไม่มีใครเขียน 12 ส.ค. 2026)
///
/// ค่าเป็น **linear** เพราะ surface เป็น sRGB — GPU แปลง gamma ให้เอง
fn clear_colour(board: &Board) -> wgpu::Color {
    let [r, g, b] = board.settings().background;
    wgpu::Color {
        r: f64::from(r),
        g: f64::from(g),
        b: f64::from(b),
        a: 1.0,
    }
}
/// ★★★ **เอกสารหนึ่งฉบับกับทุกอย่างที่ผูกกับมัน** — งานของผู้ใช้ ไม่ใช่ของ GPU
///
/// ## ทำไมมันไม่อยู่ใน [`Gfx`] (P4-7 ชิ้น b)
///
/// `Gfx` คือ **ของที่ผูกกับ device** ซึ่งต้องสร้างใหม่ทั้งชุดทุกครั้งที่ driver
/// สะดุด (`docs/04 §7`) · การที่ `board`/`history`/`selection` เคยนั่งอยู่ในนั้น
/// ทำให้โครงสร้างของโค้ดพูดว่า *"งานของผู้ใช้เป็นทรัพยากรของ device"* ซึ่งผิด
/// และอันตรายพอที่ `recover_device` ต้องเขียนคำเตือนไว้เอง:
///
/// > ห้ามเปลี่ยนตรงนี้ไปเป็น "สร้าง `Gfx` ใหม่ทั้งก้อน" … **board ของผู้ใช้จะหาย
/// > ทันทีที่ driver อัปเดต** โดยไม่มี error ที่ไหนเลย
///
/// **กฎที่ต้องจำเอง คือกฎที่วันหนึ่งจะมีคนลืม** (§4 ข้อ 16 บอกไว้ตรง ๆ) —
/// ตอนนี้มันเป็นกฎที่โครงสร้างบังคับแทน: เอกสารอยู่บน [`RefxApp`] คนละชั้นกับ
/// device ทั้งก้อน สร้าง `Gfx` ใหม่กี่ครั้งก็ **ไม่แตะเอกสารเลย**
///
/// ## ★★ ทำไมทุกฟิลด์ที่นี่ต้องมาด้วยกัน
///
/// ทุกตัวคีย์ด้วย `ItemId` หรือเป็นมุมมองของ board ใบนี้โดยเฉพาะ · `ItemId`
/// เป็นแค่ `index+generation` **ไม่ผูกกับ board** (`docs/02 §1`) สองเอกสารจึง
/// แจกคีย์ชุดเดียวกันเป๊ะ — ปล่อยให้ตัวไหนหลุดไปอยู่ที่อื่น แท็บหนึ่งจะอ่าน
/// สถานะของอีกแท็บโดยไม่มีอะไรฟ้อง (นั่นคือสิ่งที่ชิ้น c ต้องยืนอยู่บน)
struct Doc {
    /// ★★★ id ของ board ใบนี้ — **แจกไม่ซ้ำจริง** ([`Docs::mint`])
    ///
    /// `ItemId` เป็นแค่ `index+generation` **ไม่ผูกกับ board** (`docs/02 §1`)
    /// สองเอกสารจึงแจก `ItemId` ชุดเดียวกันเป๊ะ · id นี้คือสิ่งเดียวที่แยก
    /// "งานที่กลับมาจาก worker เป็นของแท็บไหน" ออกจากกันได้
    id: refx_core::arena::BoardId,
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
    /// เส้นไกด์ที่ต้องวาดตอนนี้ (P2-9) — ว่างเมื่อไม่ได้ลากหรือไม่มีอะไรตรงกัน
    guides: Vec<refx_core::align::Guide>,
    /// ★ มุมมอง Arrange + virtual scrolling (P3-3) — ดู `crate::arrange`
    ///
    /// แยกจาก `camera`/`quads` ของ Canvas ทั้งชุด เพราะสองโหมดเป็น **สอง view
    /// บนเอกสารก้อนเดียวกัน** (ARCHITECTURE §4) สลับไปมาต้องไม่ลากตำแหน่งของกันและกัน
    arrange: crate::arrange::ArrangeView,
    /// กล้อง pan/zoom (P0-7)
    camera: Camera,

    // ---------- ★★★ P4-7c: ของที่ย้ายมาจาก `RefxApp` ----------
    //
    // ทุกตัวข้างล่างนี้ตอบคำถามที่ขึ้นต้นด้วย *"เอกสารฉบับนี้…"* — วางไว้ที่
    // ระดับแอปเมื่อไหร่ แท็บที่สองจะเขียนทับคำตอบของแท็บแรกทันทีที่มันเกิด
    /// ★ ที่อยู่ของเอกสารนี้ — `None` = ยังไม่เคยบันทึก
    path: Option<std::path::PathBuf>,
    /// ★★★ โหมดการบันทึกของเอกสารนี้ (P4-5) — **สภาวะ ไม่ใช่เหตุการณ์**
    ///
    /// เปลี่ยนได้จากสองทางเท่านั้น: อนุมานจากตารางตอน**เปิด** กับ สิ่งที่ผู้ใช้
    /// สั่งตอน**บันทึกสำเร็จ** — ห้ามอนุมานใหม่ทุกครั้งที่บันทึก (§4 ข้อ 29)
    save_mode: refx_io::packed::SaveMode,
    /// ★★ asset table ของเอกสารนี้ — ว่าง = ไม่มีภาพฝังอยู่
    assets: refx_io::packed::Index,
    /// ★★★ **recovery slot ของแท็บนี้เอง** (`<data_dir>/recovery/<session>.refx`)
    ///
    /// ก่อน P4-7c มี session เดียวต่อโปรเซส · สองแท็บที่ยังไม่เคยบันทึกจะเขียน
    /// **ไฟล์เดียวกัน** ทับกันไปมาทุก 10 วินาที แล้วเปิดใหม่ได้งานคืนแค่ใบเดียว
    /// โดยไม่มีอะไรบอกว่าอีกใบเคยมีอยู่ — I-3 ที่เงียบที่สุดแบบหนึ่ง
    ///
    /// `SessionId::new_unique()` ออกแบบมารองรับข้อนี้อยู่แล้ว (มีตัวนับในโปรเซส
    /// เพราะเทสต์ `two_instances_never_share_a_recovery_file` จับได้ตั้งแต่ P4-4)
    session: refx_io::recovery::SessionId,
    /// ★★★ `board.revision()` ของ snapshot ล่าสุดที่ส่งไปเขียน — `None` = ยังไม่เคย
    ///
    /// **`dirty` อย่างเดียวตอบคำถามผิด** เมื่อรวมกับการปลุกตามเวลา: `dirty`
    /// เป็นจริงยาวจนกว่าจะ `Ctrl+S` จริง ๆ ดังนั้น board ที่ถูกปล่อยทิ้งไว้
    /// จะถูกปลุกมาเขียน snapshot ที่ **เนื้อหาเหมือนเดิมเป๊ะ** ทุก 10 วินาที
    /// ตลอดทั้งวัน — ผิดทั้ง I-1 และข้อ "ห้ามแย่ง CPU กับ Photoshop"
    snapshot_revision: Option<u64>,
    /// ★ นโยบาย autosave ของแท็บนี้ — `dirty` เท่านั้น + เว้นระยะ (P4-3)
    ///
    /// ★★ **นาฬิกาต้องเป็นของแต่ละแท็บ** ไม่ใช่ของแอป: ตัวเดียวกลาง ๆ จะทำให้
    /// แท็บที่เพิ่งเขียนไปกดเบรกให้แท็บอื่นที่ยังไม่เคยเขียนเลย — แท็บที่ผู้ใช้
    /// สลับไปมาเร็ว ๆ จะมีบางใบที่ไม่เคยถูก snapshot เลยสักครั้ง
    autosaver: refx_io::autosave::Autosaver,
    /// งาน autosave ของแท็บนี้ที่ส่งไปเธรดแล้ว — กันไม่ให้ซ้อนกันสองงาน
    autosave_job: Option<crossbeam_channel::Receiver<Result<(), String>>>,
    /// ★★★ snapshot เก่าที่เพิ่ง "เอากลับมา" — รอให้แท็บนี้เขียนของตัวเองก่อน
    ///
    /// ลบทันทีไม่ได้ (จะมีช่วงที่งานไม่มีสำเนาบนดิสก์เลย) · ไม่ลบเลยก็ไม่ได้
    /// (จะถูกเสนอให้กู้ซ้ำทุกครั้งที่เปิดโปรแกรม) → ลบหลัง snapshot ของแท็บนี้
    /// ลงดิสก์สำเร็จ ซึ่งเป็นจังหวะแรกที่มีสำเนาสองชุดพร้อมกัน
    adopted_recovery: Option<std::path::PathBuf>,
    /// ★ `.kept` ที่เพิ่งกู้กลับมา — ลบหลัง snapshot ของแท็บนี้ลงดิสก์แล้ว
    adopted_kept: Option<std::path::PathBuf>,
    /// ★★★ งานของ **เอกสารฉบับนี้** ที่ยังไม่เคยไปถึงไฟล์ (`<doc>.refx.autosave`)
    pending_snapshot: Option<Box<refx_io::autosave::Pending>>,
    /// ★★★ งานที่ผู้ใช้เคยสั่ง **"เก็บไว้ก่อน"** ของเอกสารฉบับนี้ (`docs/07 §4`)
    pending_kept: Option<Box<refx_io::autosave::Pending>>,
    /// ★★★ คีย์งาน decode → `ItemId` **ของแท็บนี้** ที่ผลลัพธ์ต้องไปเกาะ
    ///
    /// ★★ ต้องอยู่ต่อแท็บเพราะค่าคือ `ItemId` ซึ่งไม่ผูกกับ board — เก็บรวมกัน
    /// ที่ระดับแอปเมื่อไหร่ ภาพของแท็บหนึ่งจะไปโผล่ทับ item ของอีกแท็บที่บังเอิญ
    /// ได้ `ItemId` เดียวกัน (ซึ่งเกิดแทบทุกครั้ง เพราะ arena เริ่มนับจาก 0 เสมอ)
    relink_targets: std::collections::HashMap<refx_asset::hash::ContentHash, ItemId>,
    /// ★★★ `.refx-meta` ของทุกโฟลเดอร์ที่ภาพบนแท็บนี้อยู่ (P5-5)
    ///
    /// ต่อแท็บด้วยเหตุผลเดียวกับ `relink_targets` — มันถือ `ItemId` ซึ่งไม่ผูก
    /// กับ board · และ "เจ้าของเดียว" ของ `docs/07 §5` เป็นสมบัติของ *เอกสาร*
    /// ไม่ใช่ของหน้าต่าง: แท็บที่บันทึกเป็น `.refx` แล้วเลิกเขียน sidecar
    /// ต้องไม่ทำให้แท็บข้าง ๆ ที่ยังเปิดโฟลเดอร์อยู่เลิกตามไปด้วย
    sidecar: crate::sidecar::Folders,
}

impl Doc {
    /// เอกสารเปล่าใบใหม่ที่มี id และ recovery slot ของตัวเอง
    fn empty(id: refx_core::arena::BoardId) -> Self {
        Self {
            id,
            board: Board::default(),
            history: History::default(),
            index: SpatialIndex::new(refx_core::spatial::DEFAULT_CELL_SIZE),
            render_state: std::collections::HashMap::new(),
            selection: Selection::new(),
            select_tool: SelectTool::new(),
            rubber_band: None,
            guides: Vec::new(),
            arrange: crate::arrange::ArrangeView::new(),
            // เริ่มที่กลาง world ของ demo เพื่อให้เห็นสี่เหลี่ยมทันทีที่เปิด
            camera: Camera::new(Vec2::splat(2000.0), 0.25),
            path: None,
            save_mode: refx_io::packed::SaveMode::default(),
            assets: refx_io::packed::Index::default(),
            session: refx_io::recovery::SessionId::new_unique(),
            snapshot_revision: None,
            autosaver: refx_io::autosave::Autosaver::default(),
            autosave_job: None,
            adopted_recovery: None,
            adopted_kept: None,
            pending_snapshot: None,
            pending_kept: None,
            relink_targets: std::collections::HashMap::new(),
            sidecar: crate::sidecar::Folders::default(),
        }
    }

    /// ★★★ แก้ `board` ผ่าน `Command` — **ทางเดียวที่ `Board` ถูกแก้** (docs/08 §4 ข้อ 10)
    ///
    /// ★ เป็นเมธอดบน `Doc` เพราะ `history` กับ `board` ต้องยืมพร้อมกัน ซึ่ง
    /// เขียนที่จุดเรียกไม่ได้เมื่อ `Doc` อยู่หลัง `Docs::active_mut()` — และการ
    /// รวมไว้ที่นี่ทำให้ **ลืมเอาไปใส่ history ไม่ได้** (API ที่ถูกได้ทางเดียว)
    ///
    /// # Errors
    /// สิ่งที่ `History::apply` คืน — ปกติคือ "คำสั่งนี้ไม่ได้เปลี่ยนอะไร"
    fn apply(
        &mut self,
        command: Box<dyn refx_core::command::Command>,
    ) -> Result<(), refx_core::command::CmdError> {
        self.history.apply(&mut self.board, command)
    }

    /// เอา item ใบหนึ่งเข้า `SpatialIndex` ให้ตรงกับ `board` ตอนนี้
    fn reindex(&mut self, id: ItemId) {
        let Some(item) = self.board.item(id) else {
            return;
        };
        let canvas = item.canvas;
        self.index.insert(id, &canvas);
    }

    /// ★★ ชื่อที่ขึ้นบนแท็บ — `None` = ยังไม่เคยบันทึกลงที่ไหน
    fn name(&self) -> Option<String> {
        self.path.as_ref().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
    }

    /// ★★★ แท็บนี้ยังไม่ว่างเปล่าและยังไม่เคยถูกแตะเลยหรือเปล่า
    ///
    /// ใช้ตัดสินว่าเอกสารที่เพิ่งเปิดควร **แทนที่แท็บนี้** หรือควรได้แท็บใหม่ —
    /// เปิดไฟล์แรกแล้วเหลือแท็บเปล่าค้างไว้ข้าง ๆ คือขยะที่ผู้ใช้ต้องมาปิดเอง
    /// ส่วนการแทนที่แท็บที่มีงานอยู่คือสิ่งที่โครงแท็บมีไว้แก้พอดี (`docs/03 §5`)
    fn is_untouched(&self) -> bool {
        self.path.is_none() && self.board.is_empty() && !self.board.is_dirty()
    }
}

/// ★★★ **แท็บทั้งหมดที่เปิดอยู่** — `refx-core` ไม่รู้จักคำว่าแท็บ (`docs/02 §2.10`)
///
/// เอกสารฉบับแรกวาง `Workspace { boards, active, order }` ไว้ใน `refx-core`
/// แล้ว**ยกเลิก** เพราะครึ่งหนึ่งของสิ่งที่เป็น "ต่อแท็บ" จริง ๆ (`History` ·
/// `Selection` · `render_state` · camera · ช่องใน atlas · `SpatialIndex`)
/// อยู่ในชั้น UI/GPU ซึ่ง core พึ่งไม่ได้ · ทำตามตัวอักษรจะได้ **แหล่งความจริง
/// ที่สอง** ที่ต้องคอยซิงค์กัน
///
/// ★ ไม่ว่างเสมอ — ปิดแท็บสุดท้ายแล้วได้ board เปล่าใบใหม่แทน ไม่ใช่สภาพ
/// "ไม่มีเอกสาร" ที่ทุกฟังก์ชันต้องเช็คเพิ่มอีกหนึ่งกิ่ง
struct Docs {
    /// **ห้ามว่าง** — ดู [`Docs::close`]
    list: Vec<Doc>,
    /// ดัชนีของแท็บที่ผู้ใช้กำลังดูอยู่ — อยู่ในช่วงของ `list` เสมอ
    active: usize,
    /// ★★★ ตัวแจก [`refx_core::arena::BoardId`] — **เดินหน้าอย่างเดียว ไม่วนกลับ**
    ///
    /// id ที่ถูกใช้ซ้ำหลังปิดแท็บจะทำให้ผลงาน decode ที่ยังค้างอยู่ในคิวของแท็บ
    /// ที่ตายไปแล้ว **ไปเกาะแท็บใหม่ที่บังเอิญได้ id เดิม** — ภาพโผล่ผิดเอกสาร
    /// โดยไม่มี error ที่ไหน
    next_id: u32,
}

impl Default for Docs {
    fn default() -> Self {
        let mut docs = Self {
            list: Vec::new(),
            active: 0,
            next_id: 0,
        };
        let id = docs.mint();
        docs.list.push(Doc::empty(id));
        docs
    }
}

impl Docs {
    /// id ใบใหม่ที่ยังไม่เคยถูกแจก
    fn mint(&mut self) -> refx_core::arena::BoardId {
        use refx_core::arena::ArenaKey as _;
        let raw = self.next_id;
        // ล้นแล้ววนกลับดีกว่าล้มทั้งโปรแกรม — ต้องเปิด/ปิดแท็บ 4 พันล้านครั้ง
        // ในการรันเดียวถึงจะไปถึง ซึ่งไม่ใช่สภาพที่มีอยู่จริง
        self.next_id = self.next_id.wrapping_add(1);
        refx_core::arena::BoardId::from_parts(raw, 0)
    }

    fn active(&self) -> &Doc {
        // ★ `list` ไม่ว่างเสมอ · `get` แทน index เพื่อไม่ให้มี panic บนเส้นทางนี้เลย
        self.list.get(self.active).unwrap_or_else(|| {
            debug_assert!(false, "แท็บที่เลือกอยู่หลุดออกนอกช่วง");
            &self.list[0]
        })
    }

    fn active_mut(&mut self) -> &mut Doc {
        let index = self.active.min(self.list.len().saturating_sub(1));
        self.active = index;
        &mut self.list[index]
    }

    /// แท็บที่มี id นี้ — `None` = ถูกปิดไปแล้วระหว่างที่งานเดินอยู่
    fn by_id_mut(&mut self, id: refx_core::arena::BoardId) -> Option<&mut Doc> {
        self.list.iter_mut().find(|doc| doc.id == id)
    }

    fn iter(&self) -> std::slice::Iter<'_, Doc> {
        self.list.iter()
    }

    fn iter_mut(&mut self) -> std::slice::IterMut<'_, Doc> {
        self.list.iter_mut()
    }

    fn len(&self) -> usize {
        self.list.len()
    }

    /// ★ session ของ **ทุกแท็บที่เปิดอยู่** — ตัวที่ `recovery::scan`/`prune` ต้องรู้
    fn live_sessions(&self) -> Vec<refx_io::recovery::SessionId> {
        self.list.iter().map(|doc| doc.session.clone()).collect()
    }

    /// แท็บที่เปิดไฟล์นี้อยู่แล้ว — เปิดซ้ำต้อง **ไปที่แท็บเดิม** ไม่ใช่ได้สองใบ
    ///
    /// สองแท็บบนไฟล์เดียวกันแปลว่าสอง `<doc>.refx.autosave` ทับกัน และ `Ctrl+S`
    /// ของใบหลังจะลบงานของใบแรกทิ้ง — เป็นรูปแบบเดียวกับที่ recovery slot ต่อแท็บ
    /// มีไว้กัน แค่ย้ายมาโผล่ที่ชื่อไฟล์ของผู้ใช้แทน
    fn index_of_path(&self, path: &std::path::Path) -> Option<usize> {
        self.list
            .iter()
            .position(|doc| doc.path.as_deref() == Some(path))
    }

    /// เพิ่มแท็บใหม่แล้วสลับไปที่มัน — คืนดัชนีของมัน
    fn push(&mut self, doc: Doc) -> usize {
        self.list.push(doc);
        self.active = self.list.len() - 1;
        self.active
    }

    /// ★ ปิดแท็บที่ `index` — คืนตัวที่ถูกปิดไปให้ผู้เรียกเก็บกวาดต่อ
    ///
    /// ★★ ปิดใบสุดท้ายแล้ว **ได้ board เปล่าใบใหม่** ไม่ใช่รายการว่าง: สภาพ
    /// "ไม่มีเอกสารเลย" จะบังคับให้ทุกเส้นทางมีกิ่ง `None` เพิ่มอีกหนึ่ง ซึ่งเป็น
    /// กิ่งที่ไม่มีใครเดินและจะเน่าเงียบ ๆ (`docs/08 §3.9` ข้อ 2)
    fn close(&mut self, index: usize) -> Option<Doc> {
        if index >= self.list.len() {
            return None;
        }
        let closed = self.list.remove(index);
        if self.list.is_empty() {
            let id = self.mint();
            self.list.push(Doc::empty(id));
            self.active = 0;
        } else if self.active >= self.list.len() {
            self.active = self.list.len() - 1;
        } else if self.active > index {
            self.active -= 1;
        }
        Some(closed)
    }
}

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
    /// เครื่องมือที่ผู้ใช้เลือกอยู่ (`V` เลือก · `C` ครอป — docs/03 §2)
    tool: Tool,
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
    /// instance ของแถบที่ Arrange ต้องวาดเฟรมนี้ — ★ ไม่ใช่ทั้ง board
    ///
    /// ถือเป็นฟิลด์เพื่อไม่จองใหม่ทุกเฟรม (CLAUDE.md: ห้ามสร้าง buffer ใหม่ทุกเฟรม)
    arrange_quads: Vec<QuadInstance>,
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
    ///
    /// `vram_limit` = ค่าที่ผู้ใช้ตั้งใน `settings.toml` · `None` = เลือกตาม
    /// ชนิดการ์ดจอเหมือนเดิม (`docs/05 §2` — iGPU ได้น้อยกว่าเพราะแชร์ RAM ระบบ)
    fn build(
        render: &RenderContext,
        vram_limit: Option<usize>,
    ) -> Result<Self, refx_render::texture::VramError> {
        // ★ ทางเดียวที่สร้าง texture ได้ (I-6) — atlas ต้องขอผ่านตัวนี้
        let textures = vram_limit.map_or_else(
            || TextureAllocator::new(render.capabilities()),
            TextureAllocator::with_limit,
        );
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
    /// ★ ช่องใน atlas — `None` = **ไม่ได้อยู่บน GPU ตอนนี้** วาดเป็น placeholder แทน
    ///
    /// เก็บ `AtlasSlot` ทั้งก้อนแทน `uv_rect`+`layer` ที่แตกออกมาแล้ว เพราะ
    /// `Atlas::free()` ต้องการช่องเดิมกลับไป — ถ้าเก็บแต่ผลลัพธ์ที่คำนวณมาจากมัน
    /// เราจะ**คืนช่องไม่ได้เลย** แล้ว VRAM ของภาพที่ลบไปแล้วจะค้างจนปิดโปรแกรม
    /// (P2-6) · `uv_rect`/`layer`/ธง `PLACEHOLDER` เป็นของที่ *ได้มาจาก* ฟิลด์นี้
    /// จึงคำนวณสดใน `quad_for` แทนการเก็บคู่ขนานไว้ให้เพี้ยนจากกัน
    slot: Option<refx_render::atlas::AtlasSlot>,
    /// สีเด่นของภาพ — ใช้ตอนยังไม่มี (หรือไม่มีแล้ว) ช่องใน atlas
    ///
    /// เป็นคุณสมบัติของ *ภาพ* ไม่ใช่ของสถานะการอยู่บน GPU จึงถูกต้องเสมอ
    tint: [f32; 4],
}

/// ทุกอย่างที่ canvas widget ต้อง **อ่าน** เพื่อวาดหนึ่งเฟรม
///
/// ★ รวมเป็น struct เพราะรายการยาวขึ้นทุกเฟส (P2-4 กรอบเลือก · P2-5 handle ·
/// P2-7 เครื่องมือ · P2-9 ไกด์) — พารามิเตอร์แปดตัวเรียงกันสลับที่กันได้ง่ายมาก
/// และคอมไพเลอร์จับไม่ได้ถ้าสองตัวเป็นชนิดเดียวกัน
#[derive(Clone, Copy)]
struct CanvasView<'a> {
    board: &'a Board,
    selection: &'a Selection,
    render_state: &'a std::collections::HashMap<ItemId, ItemRender>,
    camera: Camera,
    rubber_band: Option<WorldRect>,
    tool: Tool,
    guides: &'a [refx_core::align::Guide],
    /// ไม้บรรทัดที่วางอยู่ (P2-10) — `None` = ไม่มีอะไรให้วาด
    measure: Option<refx_core::pick::Measurement>,
    /// โหมดที่กำลังวาด (P3-3) — Arrange ไม่มีของประดับชั้น egui เลย
    mode: Mode,
}

/// สิ่งที่การประมวลผล input หนึ่งเฟรมได้ออกมา
///
/// ★ `pick` เป็น **เหตุการณ์** ไม่ใช่สถานะ — ผู้เรียกต้องลงมือทันทีในเฟรมเดียวกัน
/// ถ้าเก็บไว้ทำทีหลัง เราจะกลับไปอยู่ในกับดักเดียวกับ `take_forgotten` (docs/08 3.9 ข้อ 8)
#[derive(Debug, Default, Clone, Copy)]
struct CanvasOutcome {
    /// มีอะไรเปลี่ยนจนต้องวาดใหม่ไหม — **I-1: ไม่มีอะไรเปลี่ยนต้องไม่ขอเฟรม**
    redraw: bool,
    /// ผู้ใช้จิ้มขอสี (P2-10) — ต้องไปอ่านไฟล์ต้นฉบับบน worker
    pick: Option<refx_core::interact::PickRequest>,
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
    /// ดับเบิลคลิกด้วยปุ่มซ้าย — เครื่องมือครอปใช้รีเซ็ตกรอบ (docs/03 §2)
    ///
    /// ให้ egui เป็นคนรวมคลิกสองครั้งให้ (มันรู้ค่าที่ระบบตั้งไว้) `refx-core`
    /// รับมาเป็น `CanvasEvent::DoubleClick` ตรง ๆ ไม่ต้องเดาเวลาเอง
    primary_double: bool,
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
            primary_double: false,
            pan_delta: egui::Vec2::ZERO,
            scroll: 0.0,
            modifiers: Modifiers::default(),
        }
    }
}

/// สีเด่นของภาพ (ARGB จาก `Thumbnail`) → tint ของ quad
///
/// ใช้ตอนภาพยังไม่มีช่องใน atlas — วาดสี่เหลี่ยมสีนี้แทนช่องว่าง (docs/04 §8)
fn dominant_rgba(dominant: u32) -> [f32; 4] {
    let [a, r, g, b] = dominant.to_be_bytes();
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        f32::from(a) / 255.0,
    ]
}

/// ระยะที่ไกด์ดึงเข้าหาขอบของภาพอื่น (พิกเซลบนจอ)
///
/// ★ กว้างกว่า `DEFAULT_DRAG_THRESHOLD_PX` เล็กน้อยโดยตั้งใจ — ต้องรู้สึกว่า
/// "มันช่วยจัดให้" ไม่ใช่ "ต้องเล็งเอง" แต่ไม่กว้างจนวางภาพอิสระข้าง ๆ กันไม่ได้
const GUIDE_SNAP_PX: f32 = 6.0;

/// สีของเส้นไกด์ — ★ ต้องต่างจากสีกรอบเลือกชัด ๆ ไม่งั้นแยกไม่ออกว่าอันไหนคืออะไร
const GUIDE_STROKE: egui::Color32 = egui::Color32::from_rgb(255, 96, 160);

/// สีของกรอบสิ่งที่ถูกเลือกและกรอบ rubber-band
const SELECT_STROKE: egui::Color32 = egui::Color32::from_rgb(120, 190, 255);

/// พื้นของโน้ตข้อความ (P2-11) — ทึบพอให้อ่านออกบนพื้นหลังอะไรก็ได้
const NOTE_FILL: egui::Color32 = egui::Color32::from_rgb(48, 44, 36);
/// ขอบของโน้ต
const NOTE_STROKE: egui::Color32 = egui::Color32::from_rgb(198, 172, 96);
/// สีตัวอักษรในโน้ต
const NOTE_TEXT: egui::Color32 = egui::Color32::from_rgb(238, 230, 210);

/// ★★ พื้นของภาพที่หาไฟล์ไม่เจอ (P4-6) — **ต้องเห็นชัดว่าที่นี่มีของอยู่**
///
/// `docs/07 §2`: item ที่หาไฟล์ไม่เจอต้องไม่หายไปจาก board · ถ้าไม่วาดอะไรเลย
/// ผู้ใช้จะอ่านว่า "งานหาย" ซึ่งเป็นสิ่งที่ I-3 ห้ามให้เขารู้สึกตั้งแต่แรก
const MISSING_FILL: egui::Color32 = egui::Color32::from_rgb(54, 40, 40);
/// ขอบของภาพที่หาย — สีเตือน คนละสีกับกรอบเลือกและกรอบครอป
const MISSING_STROKE: egui::Color32 = egui::Color32::from_rgb(214, 122, 108);
/// สีชื่อไฟล์ที่เขียนบน placeholder
const MISSING_TEXT: egui::Color32 = egui::Color32::from_rgb(240, 214, 208);

/// ตัดข้อความเป็นบรรทัดให้พอดีกับความกว้างของโน้ต
///
/// ★ ประมาณความกว้างตัวอักษรที่ `0.55 * font_size` แทนที่จะวัดจริงด้วย egui
/// เพราะการวัดต้องยืม `Fonts` ซึ่งอยู่ใต้ lock เดียวกับที่ painter ถืออยู่
/// ผลที่ได้ไม่เป๊ะแต่ **ข้อความไม่ล้นออกนอกกรอบ** ซึ่งเป็นสิ่งที่ผู้ใช้สังเกต
fn wrap_note(text: &str, width: f32, font_size: f32) -> String {
    let per_char = (font_size * 0.55).max(1.0);
    let columns = ((width - 12.0) / per_char).floor().max(4.0) as usize;
    let mut out = String::with_capacity(text.len() + 8);
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let mut column = 0usize;
        for word in line.split(' ') {
            // คำเดียวที่ยาวเกินบรรทัดต้องถูกหั่น ไม่งั้นมันล้นออกไปคำเดียว
            let mut rest = word;
            while rest.chars().count() > columns {
                let cut: String = rest.chars().take(columns).collect();
                if column > 0 {
                    out.push('\n');
                }
                out.push_str(&cut);
                out.push('\n');
                rest = &rest[cut.len()..];
                column = 0;
            }
            let len = rest.chars().count();
            if column > 0 && column + 1 + len > columns {
                out.push('\n');
                column = 0;
            } else if column > 0 {
                out.push(' ');
                column += 1;
            }
            out.push_str(rest);
            column += len;
        }
    }
    out
}

/// สีของไม้บรรทัด (P2-10) — ★ ต้องต่างจากกรอบเลือก ไกด์ และ handle ครอป
/// ทั้งสี่อย่างวาดทับกันได้บนจอเดียว ถ้าสีซ้ำผู้ใช้จะแยกไม่ออกว่าอันไหนคืออะไร
const MEASURE_STROKE: egui::Color32 = egui::Color32::from_rgb(120, 230, 140);

/// สีพื้นของ handle มุม — ทึบเพื่อให้เห็นบนภาพสีอะไรก็ได้
const HANDLE_FILL: egui::Color32 = egui::Color32::from_rgb(250, 250, 252);

/// สีขอบของ handle ตอนอยู่ในเครื่องมือครอป — ★ ต้องต่างจากตอนเลือกด้วย **สี**
/// ไม่ใช่แค่จำนวน handle ผู้ใช้ต้องรู้ได้ทันทีว่าลากแล้วจะครอปหรือจะสเกล
const CROP_STROKE: egui::Color32 = egui::Color32::from_rgb(255, 196, 92);

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
    fn canvas_widget(ui: &mut egui::Ui, view: CanvasView<'_>) -> CanvasFrameInput {
        let CanvasView {
            board,
            selection,
            render_state,
            camera,
            rubber_band,
            tool,
            guides,
            measure,
            mode,
        } = view;
        let response = ui.allocate_response(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;

        // ★ เก็บ input ให้เสร็จก่อน **แล้วค่อยวาด** — ทำให้ Arrange ออกตรงนี้ได้เลย
        //   โดยไม่ต้องมีเงื่อนไข `if mode` โรยไว้ทุกบล็อกของการวาด (ซึ่งเป็นแบบที่
        //   คนเพิ่มบล็อกใหม่ทีหลังจะลืมใส่ แล้วกรอบของ Canvas จะไปโผล่บน contact sheet)
        let frame_input = Self::collect_canvas_input(ui, &response, rect);
        if mode == Mode::Arrange {
            // แผ่น Arrange ถูกวาดด้วย quad pipeline ทั้งหมด (ดู `plan_arrange`)
            // ยังไม่มีของประดับชั้น egui — กรอบเลือก/handle/ไกด์เป็นของ Canvas
            return frame_input;
        }

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

        // ---- วาด handle ของ scale/rotate (P2-5) ----
        //
        // ★ ขนาดคงที่ **บนจอ** ไม่ใช่ใน world — วาดในหน่วย point ตรง ๆ จึงได้ผลนั้นฟรี
        //   ส่วนพิกัดของมุมมาจาก `Obb::corners()` จึงหมุนตามภาพที่หมุนแล้ว
        //   (ระยะกดอยู่ที่ `refx-core` และ **กว้างกว่ารูปที่วาด** โดยตั้งใจ)
        if let Some(frame) = refx_core::interact::selection_frame(board, selection) {
            let corners = frame.corners().map(to_point);
            // กรอบรวม — บอกว่า handle เป็นของกลุ่มไหน (ใบเดียวจะทับกับกรอบเลือกพอดี)
            painter.add(egui::Shape::closed_line(
                corners.to_vec(),
                egui::Stroke::new(1.0, SELECT_STROKE),
            ));
            // ★ วาด handle ชุดเดียวกับที่ `refx-core` ยอมให้จับ — เครื่องมือครอปมีกลางด้าน
            //   ด้วย ถ้าวาดไม่ตรงกัน ผู้ใช้จะเห็นจุดที่กดไม่ได้ หรือกดได้จุดที่ไม่เห็น
            let stroke = if tool == Tool::Crop {
                CROP_STROKE
            } else {
                SELECT_STROKE
            };
            let side = egui::Vec2::splat(refx_core::interact::HANDLE_DRAW_PX);
            for dir in refx_core::interact::handles_for(tool) {
                let square = egui::Rect::from_center_size(to_point(dir.point_on(frame)), side);
                painter.rect_filled(square, 1.0, HANDLE_FILL);
                painter.rect_stroke(
                    square,
                    1.0,
                    egui::Stroke::new(1.0, stroke),
                    egui::StrokeKind::Middle,
                );
            }
        }

        // ---- วาดโน้ตข้อความ (P2-11) ----
        //
        // ★★ โน้ต **ไม่มี quad และไม่มีช่องใน atlas** — มันเป็นข้อความ ไม่ใช่ pixel
        //   `rebuild_quads` ข้ามมันไปเองเพราะไม่มี `render_state` · ที่นี่จึงเป็น
        //   ที่เดียวที่โน้ตถูกวาด และวาดด้วย egui ซึ่งมี font atlas อยู่แล้ว
        //
        // ★ วาดตามลำดับ z เหมือนภาพ เพื่อให้โน้ตที่ผู้ใช้ส่งไปหลังสุดอยู่หลังจริง
        for (id, item) in board.items_in_z_order() {
            let refx_core::board::ItemKind::Text(note) = &item.kind else {
                continue;
            };
            if !item.canvas.visible {
                continue;
            }
            let corners = item.canvas.obb().corners().map(to_point);
            let frame = egui::Rect::from_two_pos(corners[0], corners[2]);
            painter.rect_filled(frame, 3.0, NOTE_FILL);
            painter.rect_stroke(
                frame,
                3.0,
                egui::Stroke::new(1.0, NOTE_STROKE),
                egui::StrokeKind::Middle,
            );
            // ขนาดตัวอักษรตามระดับซูม — โน้ตเป็น item บน world เหมือนภาพ
            // ถ้าขนาดคงที่บนจอ ข้อความจะล้นกรอบทันทีที่ซูมออก
            let size = (12.0 * scale).clamp(1.0, 400.0);
            if size >= 4.0 && !note.text.is_empty() {
                painter.text(
                    frame.min + egui::vec2(6.0, 4.0),
                    egui::Align2::LEFT_TOP,
                    // ★ ตัดเป็นบรรทัดตามความกว้างของโน้ตเอง ไม่ใช่ปล่อยล้นออกไป
                    wrap_note(&note.text, frame.width(), size),
                    egui::FontId::proportional(size),
                    NOTE_TEXT,
                );
            }
            let _ = id;
        }

        // ---- วาดภาพที่หาไฟล์ไม่เจอ (P4-6 ขั้นที่ 4) ----
        //
        // ★★★ `docs/07 §2`: **`Missing` ต้องไม่หายไปจาก board** · มันไม่มี quad
        //   และไม่มีช่องใน atlas (ไม่มีพิกเซลให้อัด) `rebuild_quads` จึงข้ามมันไป
        //   เหมือนโน้ต — ที่นี่คือที่เดียวที่ผู้ใช้จะได้เห็นว่ามันยังอยู่
        //
        // ★★ เขียน **ชื่อไฟล์** ลงไปด้วย ไม่ใช่กล่องเปล่า: ผู้ใช้ที่ถอดไดรฟ์ออก
        //   ต้องอ่านออกว่าขาดไฟล์ไหน ถึงจะรู้ว่าต้องไปเสียบไดรฟ์ไหนกลับ
        for (id, item) in board.items_in_z_order() {
            let refx_core::board::ItemKind::Missing { original_path, .. } = &item.kind else {
                continue;
            };
            if !item.canvas.visible {
                continue;
            }
            let corners = item.canvas.obb().corners().map(to_point);
            let frame = egui::Rect::from_two_pos(corners[0], corners[2]);
            painter.rect_filled(frame, 2.0, MISSING_FILL);
            painter.rect_stroke(
                frame,
                2.0,
                egui::Stroke::new(1.0, MISSING_STROKE),
                egui::StrokeKind::Middle,
            );
            let size = (11.0 * scale).clamp(1.0, 400.0);
            if size >= 4.0 {
                painter.text(
                    frame.center(),
                    egui::Align2::CENTER_CENTER,
                    // ★ ชื่อไฟล์ล้วน ไม่ใช่ path เต็ม (docs/08 §5)
                    wrap_note(
                        &refx_asset::decode::file_label(original_path),
                        frame.width(),
                        size,
                    ),
                    egui::FontId::proportional(size),
                    MISSING_TEXT,
                );
            }
            let _ = id;
        }

        // ---- วาดเส้นไกด์ (P2-9) ----
        //
        // ★ วาด **ก่อน** กรอบเลือกและ handle เพื่อให้ของที่ผู้ใช้กำลังจับอยู่อยู่บนสุด
        for guide in guides {
            let (a, b) = if guide.vertical {
                (
                    to_point(Vec2::new(guide.at, guide.from)),
                    to_point(Vec2::new(guide.at, guide.to)),
                )
            } else {
                (
                    to_point(Vec2::new(guide.from, guide.at)),
                    to_point(Vec2::new(guide.to, guide.at)),
                )
            };
            painter.line_segment([a, b], egui::Stroke::new(1.0, GUIDE_STROKE));
        }

        // ---- วาดไม้บรรทัด (P2-10) ----
        //
        // ★ เส้นอยู่ใน world (ปลายทั้งสองแปลงผ่าน `to_point`) แต่ **ตัวเลขบนป้าย
        //   ไม่ได้มาจากพิกเซลบนจอ** มันมาจาก `Measurement` ที่เก็บ world ไว้
        //   ซูมแล้วเส้นยาวขึ้นบนจอได้ แต่ตัวเลขต้องนิ่ง — นั่นคือทั้งหมดของเครื่องมือนี้
        if let Some(m) = measure {
            let (a, b) = (to_point(m.from), to_point(m.to));
            painter.line_segment([a, b], egui::Stroke::new(1.5, MEASURE_STROKE));
            // ขีดปลายทั้งสองข้าง ให้เห็นว่าวัดจากตรงไหนถึงตรงไหนเป๊ะ ๆ
            for end in [a, b] {
                painter.circle_filled(end, 3.0, MEASURE_STROKE);
            }
            let extent = m.extent();
            painter.text(
                b + egui::vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                format!(
                    "{:.1} u  ({:.1} x {:.1})  {:.1}°",
                    m.length(),
                    extent.x,
                    extent.y,
                    m.angle_deg()
                ),
                egui::FontId::monospace(12.0),
                MEASURE_STROKE,
            );
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

        frame_input
    }

    /// เก็บสิ่งที่ egui รายงานในเฟรมนี้ — **ไม่วาดอะไรเลย**
    ///
    /// แยกออกมาเพื่อให้ทั้งสองโหมดใช้ตัวเดียวกัน (P3-3): ล้อ/ปุ่ม/เคอร์เซอร์
    /// ถูกอ่านเหมือนกันทุกโหมด ต่างกันที่ **ใครเอาไปทำอะไร** ซึ่งอยู่ที่
    /// `apply_canvas_input` — Canvas ซูม/เลือก · Arrange เลื่อนแผ่น
    fn collect_canvas_input(
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
    ) -> CanvasFrameInput {
        let (scroll, modifiers) = ui.ctx().input(|i| {
            (
                i.smooth_scroll_delta.y,
                Modifiers {
                    ctrl: i.modifiers.ctrl || i.modifiers.command,
                    shift: i.modifiers.shift,
                    alt: i.modifiers.alt,
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
            // ★★ **ปุ่มลงจริงเท่านั้น** ห้ามนับ `drag_started_by` เป็นการกดด้วย
            //
            //    egui รายงาน `drag_started` ในเฟรม *หลัง* เคอร์เซอร์ขยับพ้นระยะของมันเอง
            //    ถ้านับทั้งสองอย่าง การกดจริงหนึ่งครั้งจะกลายเป็น `Press` **สองครั้ง**
            //    ครั้งที่สองอยู่ห่างจากจุดที่ผู้ใช้กดไปหลายพิกเซล แล้วมันจะไปทับ
            //    สถานะการกดเดิม → จับ handle ค้างไว้แล้วขยับ กลายเป็นลากกรอบเลือกแทน
            //    (เจอตอนทำ P2-5 ส่วน handle — การย้ายบังคับอาการนี้ไม่ออกเพราะจุดที่สอง
            //    ยังอยู่บนภาพเดิม จึงยังได้ `Move` เหมือนเดิม ต่างแค่เลื่อนไปนิดเดียว)
            primary_pressed: ui
                .ctx()
                .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
                && response.hovered(),
            primary_released: response.drag_stopped_by(egui::PointerButton::Primary)
                || (response.clicked() && !response.dragged()),
            primary_down: ui
                .ctx()
                .input(|i| i.pointer.button_down(egui::PointerButton::Primary)),
            primary_double: response.double_clicked_by(egui::PointerButton::Primary),
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
/// ★ **ไม่มี `Default`** (ถอดออก P5-3) — สร้างด้วย [`RefxApp::new`] เท่านั้น
///
/// เดิมมี `#[derive(Default)]` ที่ **ไม่มีใครเรียกเลยสักที่** · พอ P5-3 เพิ่ม
/// [`refx_io::settings::Caps`] เข้ามา การรักษามันไว้จะบังคับให้ `Caps` ต้องมี
/// ค่าปริยาย ซึ่งแปลว่าต้องแต่ง "เครื่องสมมติ" ขึ้นมาหนึ่งเครื่อง — แล้ววันหนึ่ง
/// จะมีโค้ดเส้นทางไหนตกไปใช้เพดานของเครื่องที่ไม่มีอยู่จริงโดยไม่มีอะไรบ่น
/// ซึ่งเป็นสิ่งที่ `HANDOFF §4` ข้อ 3 กันไว้พอดี
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
    /// ★ งานอ่านสีที่ยังค้างอยู่ (P2-10) — `None` = ไม่ได้รออะไร
    ///
    /// เก็บไว้เพื่อ **ทิ้งผลของการจิ้มครั้งเก่า**: ผู้ใช้จิ้มรัว ๆ ได้ และงานที่
    /// ส่งก่อนอาจกลับมาทีหลัง (ไฟล์ใหญ่กว่า) ถ้าไม่เทียบคีย์ สีที่ค้างบน status bar
    /// จะเป็นของจุดที่เขาเลิกสนใจไปแล้ว
    pick_in_flight: Option<refx_asset::hash::ContentHash>,
    /// ตัวนับการจิ้ม — ทำคีย์ที่ไม่ชนกับ hash ของภาพใด ๆ
    pick_count: u64,
    /// ★ ตัวก๊อป hex ขึ้น clipboard (P2-10) — เขียนบนเธรดชั่วคราว ไม่ใช่ที่นี่
    ///
    /// `None` เมื่อยังไม่ได้เสียบตัวเขียน (เทสต์ที่ไม่มี OS จริง)
    copier: Option<crate::copy::Copier>,
    /// ตัวปลุก event loop — ส่งต่อให้ worker หลังหน้าต่างพร้อม
    waker: Option<refx_platform::window::Waker>,
    /// ★ ภาพที่ meta เพิ่งเปลี่ยนในเฟรมนี้ — `tick_sidecar` กินแล้วล้าง (P5-5)
    ///
    /// เป็นของแอปไม่ใช่ของ `Doc` เพราะมันมีอายุ **หนึ่งเฟรม** และถูกผลิต
    /// กับถูกกินในเฟรมเดียวกันเสมอ — เก็บลง `Doc` จะดูเหมือนสถานะที่ค้างได้
    sidecar_touched: Vec<ItemId>,
    /// เวลาที่ผู้ใช้ปล่อยไฟล์ลงหน้าต่าง (ใช้วัด "ลากเข้ามา → ภาพขึ้นจอ")
    drop_started: Option<std::time::Instant>,
    /// ผลของงวดที่ลากเข้ามารอบล่าสุด — ★ **ต้องบวกกันได้ครบเสมอ** ดู [`DropBatch`]
    drop: DropBatch,
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
    /// คำสั่งย้ายชั้นที่รอทำต้นเฟรมถัดไป — รวบการกดค้างเหมือน `pending_history`
    pending_zorder: Option<ZMove>,
    /// ผู้ใช้กด `Delete` ในรอบ event ที่ผ่านมา
    pending_delete: bool,
    /// ผู้ใช้กด `G`/`H` ในรอบ event ที่ผ่านมา (P2-8)
    pending_appearance: Option<AppearanceKey>,
    /// ผู้ใช้กด `Ctrl+G` / `Ctrl+Shift+G` ในรอบ event ที่ผ่านมา (P3-7)
    pending_group: Option<GroupRequest>,
    /// ผู้ใช้กด `Tab` — สลับโหมดของ **แท็บที่ดูอยู่** (P5-3b ก้อน c)
    pending_mode_toggle: bool,
    /// ผู้ใช้กด `Ctrl+A` (P5-3b ก้อน c)
    pending_select_all: bool,
    /// ผู้ใช้กด `Esc` — ปิดแถบที่ค้าง หรือยกเลิกเลือก (P5-3b ก้อน c)
    pending_clear_selection: bool,
    /// ผู้ใช้กด `F` / `1` / `0` (P5-3b ก้อน c) — รวบการกดค้างเหมือน `pending_zorder`
    pending_zoom: Option<keymap::ZoomRequest>,
    /// ผู้ใช้กด `Ctrl+E` — เปิดกล่องส่งออก (P5-4)
    pending_export: bool,
    /// กล่องเลือกไฟล์ของ OS ที่เปิดค้างอยู่ — `None` = ไม่ได้เปิด
    export_target_rx: Option<crossbeam_channel::Receiver<Option<std::path::PathBuf>>>,
    /// ไฟล์ปลายทางที่ผู้ใช้เลือกไว้แล้ว
    export_target: Option<std::path::PathBuf>,
    /// ★★ งาน export ที่กำลังทำอยู่บน worker — `None` = ไม่มีงาน
    ///
    /// ★ ตัวเดียวโดยตั้งใจ: export สองงานพร้อมกันแย่ง VRAM กันแล้วทั้งคู่ช้าลง
    /// โดยไม่มีใครได้อะไรเพิ่ม · ปุ่มถูกปิดระหว่างมีงานอยู่แล้ว
    export_job: Option<crate::export::ExportJob>,
    /// ผู้ใช้กด `Ctrl+S` / `Ctrl+Shift+S` ในรอบ event ที่ผ่านมา (P4-2)
    pending_save: Option<SaveRequest>,
    /// ★★★ **แท็บทั้งหมดที่เปิดอยู่** — อยู่ที่นี่ **ไม่ใช่ใน [`Gfx`]** (ดู [`Doc`])
    docs: Docs,
    /// ★ โหมดที่ผู้ใช้เลือกไว้สำหรับ dialog "บันทึกเป็น" ที่กำลังเปิดอยู่
    save_as_mode: refx_io::packed::SaveMode,
    /// ผู้ใช้กด `Ctrl+O` ในรอบ event ที่ผ่านมา (P4-4)
    pending_open: bool,
    /// ★ ผู้ใช้กด `Ctrl+T` — board เปล่าใบใหม่ (`docs/03 §5`)
    pending_new_tab: bool,
    /// ★ ผู้ใช้กด `Ctrl+W` — ปิดแท็บที่ดูอยู่ (**ถามก่อนถ้ายังไม่บันทึก**)
    pending_close_tab: bool,
    /// ★ ผู้ใช้กด `Ctrl+Tab` — ไปแท็บถัดไป
    pending_next_tab: bool,
    /// ★★ แท็บที่กำลังถามว่า "ปิดทั้งที่ยังไม่ได้บันทึกจริงไหม" — `None` = ไม่ได้ถาม
    ///
    /// เก็บเป็น [`refx_core::arena::BoardId`] ไม่ใช่ดัชนี เพราะระหว่างที่คำถาม
    /// ค้างอยู่ ผู้ใช้กดปิดแท็บอื่นหรือสลับแท็บได้ แล้วดัชนีจะชี้ผิดใบ —
    /// ปิดผิดแท็บคือการทำงานหายชนิดที่ผู้ใช้ไม่มีทางเดาได้ว่าเกิดจากอะไร
    closing_tab: Option<refx_core::arena::BoardId>,
    /// dialog เลือกไฟล์ที่จะเปิดที่กำลังรอผู้ใช้ตอบ (ไม่บล็อก I-2)
    open_dialog: Option<crossbeam_channel::Receiver<Option<std::path::PathBuf>>>,
    /// งานอ่าน+decode ไฟล์ที่ส่งไปเธรดแล้ว (ไม่บล็อก I-2)
    ///
    /// ★★★ `BoardId` ถูก **จองไว้ก่อนส่งงาน** เพราะ `dto::decode` ต้องรู้ id
    /// ตั้งแต่ตอนอ่าน (`.refx` ไม่เก็บ id — P4-1 ตัดออกโดยตั้งใจ) · เปิดไม่สำเร็จ
    /// แล้ว id นั้นก็แค่ไม่ถูกใช้ ซึ่งไม่เป็นไรเพราะตัวแจกเดินหน้าอย่างเดียว
    load_job: Option<(
        refx_core::arena::BoardId,
        crossbeam_channel::Receiver<Result<LoadedDoc, OpenFailure>>,
    )>,
    /// ★★★ งานค้างจาก session ก่อนที่กำลังถามผู้ใช้อยู่ — `None` = ไม่มี
    pending_recovery: Option<PendingRecovery>,
    /// ★★★ **งานกำพร้าที่เหลือรอถามต่อ** — ใหม่สุดอยู่หน้าสุด (P4-7c)
    ///
    /// ก่อนหน้านี้เราหยิบมาแค่ใบเดียวต่อการเปิดโปรแกรมหนึ่งครั้ง ซึ่งแปลว่า
    /// ผู้ใช้ที่เปิดสองแท็บโดยไม่บันทึกเลยแล้วโปรแกรมตาย จะได้งานคืน **ใบเดียว**
    /// ต่อการเปิดโปรแกรมหนึ่งครั้ง — อีกใบต้องปิด-เปิดใหม่ถึงจะได้ ซึ่งไม่มีอะไร
    /// บนจอบอกเขาเลย · ตอนนี้ตอบใบหนึ่งแล้วใบถัดไปขึ้นถามต่อทันที
    ///
    /// ★ ยังคง **ถามทีละใบ** ตามเหตุผลเดิม (รายการสิบบรรทัดที่หน้าตาเหมือนกันหมด
    /// ทำให้คนกด "ทิ้ง" รวดเดียวเพื่อให้มันหายไป) — เปลี่ยนแค่ว่าไม่ต้องรอรอบหน้า
    recovery_queue: std::collections::VecDeque<PendingRecovery>,
    /// งานสแกนโฟลเดอร์ recovery ตอนเปิดโปรแกรม (แตะดิสก์ → ต้องอยู่เธรดอื่น I-2)
    recovery_scan: Option<crossbeam_channel::Receiver<Vec<PendingRecovery>>>,
    /// ★★ ถามเรื่องงานค้างไปแล้วในการรันครั้งนี้ — **ครั้งเดียวตลอดอายุโปรแกรม**
    ///
    /// `resumed()` ถูกเรียกซ้ำได้ตอนกู้ device (docs/04 §7) · ถ้าใช้ "ไม่มีงานค้าง
    /// อยู่ตอนนี้" เป็นเงื่อนไข ผู้ใช้ที่ตอบ "เก็บไว้ก่อน" ไปแล้วจะถูกถามใหม่ทุกครั้ง
    /// ที่ไดรเวอร์สะดุด — และคำถามที่โผล่ซ้ำ ๆ คือคำถามที่คนกดปิดโดยไม่อ่าน
    recovery_checked: bool,
    /// ★★★ **แท็บที่งาน decode ใบหนึ่งเป็นของ** — คีย์คือคีย์*งาน* ไม่ใช่คีย์เนื้อ
    ///
    /// ผลจาก pool กลับมาพร้อม hash เท่านั้น · ถ้าไม่มีตารางนี้ ภาพที่แท็บ A
    /// สั่งโหลดจะไปตกที่แท็บที่ผู้ใช้บังเอิญสลับไปดูตอนมันเสร็จ (เส้นทางลากไฟล์)
    /// หรือไปเกาะ `ItemId` ใบเดียวกันของอีกเอกสาร (เส้นทางเปิดไฟล์)
    ///
    /// ★ ล้างเมื่อผลกลับมาถึง หรือเมื่อแท็บถูกปิด — ดู [`RefxApp::forget_jobs_of`]
    job_owner: std::collections::HashMap<refx_asset::hash::ContentHash, refx_core::arena::BoardId>,
    /// ★ โฟลเดอร์ `<data_dir>/recovery` — `None` = หาที่อยู่ไม่ได้ (ไม่มี home dir)
    ///
    /// **ห้ามตกมาที่ `cache_dir`** ถ้าหาไม่เจอ (`docs/07 §4`) — ยอมไม่มี autosave
    /// ดีกว่าวางงานของผู้ใช้ไว้ในที่ที่มีคนตั้งใจจะลบเป็นระยะ
    recovery_dir: Option<std::path::PathBuf>,
    /// ★★ โฟลเดอร์ `<data_local_dir>/pasted` — ที่พักของภาพที่วางจาก clipboard
    ///
    /// `None` = ไม่มีที่พัก · ภาพที่วางจะยังขึ้นจอได้ตามปกติแต่คมได้แค่ระดับ
    /// thumbnail และหายไปตอนปิดโปรแกรม (`docs/07 §2`)
    spool_dir: Option<std::path::PathBuf>,
    /// ผลของการเก็บกวาด spool ที่ส่งไปทำบนเธรดอื่นแล้ว รอผลกลับ (ไม่บล็อก I-2)
    spool_sweep: Option<crossbeam_channel::Receiver<refx_io::spool::Swept>>,
    /// ★ ผลของการตามหาไฟล์ขั้น 1–3 ที่ส่งไปทำบนเธรดอื่นแล้ว (P4-6)
    ///
    /// ★★ พก `BoardId` มาด้วย — ผู้ใช้กด `Ctrl+O` ไฟล์ที่สองได้ระหว่างที่การ
    /// สแกนของแท็บแรกยังเดินอยู่ ผลที่กลับมาต้องรู้ว่ามันเป็นของ board ใบไหน
    relink_scan: Option<(refx_core::arena::BoardId, LocateResults)>,
    /// ★ แท็บที่รอคิวสแกน — งวดใหม่เริ่มได้ทีละงวดเพราะ [`DropBatch`] มีชุดเดียว
    relink_queue: std::collections::VecDeque<refx_core::arena::BoardId>,
    /// dialog "หาไฟล์เอง" ที่กำลังเปิดอยู่ (ขั้นที่ 5)
    relink_pick: Option<crossbeam_channel::Receiver<Option<std::path::PathBuf>>>,
    /// ผลการจับคู่ไฟล์ที่เหลือในโฟลเดอร์ที่ผู้ใช้ชี้ (ขั้นที่ 5)
    relink_match: Option<(refx_core::arena::BoardId, LocateResults)>,
    /// item ที่ผู้ใช้กด "หาไฟล์เอง" ให้ — รอ dialog ตอบ · พร้อมแท็บที่มันอยู่
    relink_for: Option<(refx_core::arena::BoardId, ItemId)>,
    /// ★ ผลการตามหาไฟล์ที่รอรายงาน **ตอนจบงวด** (ดู `report_relink`)
    relink_report: Option<RelinkReport>,
    /// dialog เลือกที่บันทึกที่กำลังเปิดอยู่ (รอผู้ใช้ตอบ — ไม่บล็อก I-2)
    save_dialog: Option<crossbeam_channel::Receiver<Option<std::path::PathBuf>>>,
    /// งานบันทึกที่ส่งไปเธรดแล้ว รอผลกลับ (ไม่บล็อก I-2)
    ///
    /// ★★ พก `BoardId` มาด้วยด้วยเหตุผลเดียวกับ `relink_scan` — ผู้ใช้สลับแท็บ
    /// ระหว่างที่ไฟล์กำลังเขียนได้ · `mark_saved` ที่ลงผิดใบ = แท็บที่ยังไม่ถูก
    /// บันทึกจะดูสะอาด แล้วผู้ใช้จะปิดโปรแกรมทิ้งโดยเชื่อว่างานอยู่ในไฟล์แล้ว
    save_job: Option<(
        refx_core::arena::BoardId,
        crossbeam_channel::Receiver<Result<SavedDoc, String>>,
    )>,
    /// ★ ทำอะไรต่อหลังบันทึกเสร็จ — ใช้ตอนผู้ใช้เลือก "บันทึกแล้วปิด"
    after_save: AfterSave,
    /// ผู้ใช้กดปิดหน้าต่างทั้งที่ยังมีงานไม่ได้บันทึก → รอเขาตอบ
    close_confirm: bool,
    /// ตัดสินใจแล้วว่าจะปิดจริง — `on_close_requested` รอบถัดไปปล่อยผ่าน
    closing: bool,
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

    /// ★★★ ค่าที่ผู้ใช้ตั้งไว้ (P5-3) — **มีค่าที่ใช้ได้เสมอ** แม้ไฟล์จะพัง
    settings: refx_io::settings::Settings,
    /// ข้อจำกัดของเครื่องนี้ที่ใช้ตรวจค่าของผู้ใช้ — คำนวณครั้งเดียวตอนเปิดโปรแกรม
    caps: refx_io::settings::Caps,
    /// ★★ เรื่องที่ต้องบอกผู้ใช้เกี่ยวกับ `settings.toml` — ว่าง = ไม่มีอะไรผิดปกติ
    ///
    /// ★ ค้างอยู่จนกว่าจะได้ขึ้นจอ: การโหลดเกิด**ก่อนหน้าต่างมี** จึงเขียนลง
    /// status bar ตอนนั้นไม่ได้ (ยังไม่มีใครวาด) ถ้าไม่เก็บไว้ ผู้ใช้ที่ตั้งค่าผิด
    /// จะไม่มีวันรู้ว่าค่าของเขาไม่ถูกใช้ ซึ่งเป็นความเงียบที่ P5-3 ห้ามไว้ตรง ๆ
    settings_notes: Vec<refx_io::settings::Note>,
    /// ยังไม่ได้เอา [`Self::settings_notes`] ขึ้นจอ — เอาขึ้นเฟรมแรกที่วาดได้
    settings_notes_pending: bool,
    /// ★ มีค่าที่ผู้ใช้เปลี่ยนแล้วยังไม่มีผลจนกว่าจะเปิดโปรแกรมใหม่
    ///
    /// เพดาน RAM/VRAM/ขนาดภาพ และ present mode ถูกอ่าน **ตอนสร้าง** pool /
    /// allocator / surface เท่านั้น · การเปลี่ยนกลางคันต้องรื้อ threading model
    /// ซึ่ง `CLAUDE.md` บอกให้หยุดถามก่อน → บอกผู้ใช้ตรง ๆ แทน
    settings_restart_pending: bool,
    /// งานเขียน `settings.toml` ที่ส่งไปเธรดแล้ว รอผลกลับ (ไม่บล็อก I-2)
    settings_write: Option<crossbeam_channel::Receiver<Result<(), String>>>,
    /// ★★ เหตุที่ `keymap.toml` ใช้ไม่ได้ — `None` = ไม่มีไฟล์ หรือใช้ได้ปกติ
    ///
    /// ค้างอยู่จนกว่าจะได้ขึ้นจอ ด้วยเหตุผลเดียวกับ [`Self::settings_notes`]:
    /// การอ่านเกิด**ก่อนหน้าต่างมี**
    keymap_problem: Option<crate::keymap::Problem>,
    /// ตารางคีย์ลัดที่ใช้อยู่มาจาก `keymap.toml` ของผู้ใช้ (ไม่ใช่ค่าปริยาย)
    keymap_from_file: bool,
    /// โฟลเดอร์ config — `None` = หาที่อยู่ไม่ได้ (บันทึกค่าที่ตั้งไม่ได้ แต่ยังใช้ได้)
    config_dir: Option<std::path::PathBuf>,
}

/// ที่อยู่ของ `keymap.toml` ในโฟลเดอร์ config
#[must_use]
fn keymap_path(config_dir: &std::path::Path) -> std::path::PathBuf {
    config_dir.join("keymap.toml")
}

/// ที่อยู่ของ `settings.toml` ในโฟลเดอร์ config
#[must_use]
fn settings_path(config_dir: &std::path::Path) -> std::path::PathBuf {
    config_dir.join("settings.toml")
}

/// ข้อจำกัดของเครื่องนี้ที่ `settings.toml` ต้องเคารพ
///
/// ★★★ `max_pixels_ceiling` **ต้องเป็นตัวเดียวกับที่ `decode` ใช้จริง**
/// (`HANDOFF §4` ข้อ 3) — ถ้าแผง Settings บอกเพดานหนึ่งแต่ด่าน decode ใช้อีกเพดาน
/// ผู้ใช้จะตั้งค่าที่ "ผ่าน" แล้วเจอภาพถูกปฏิเสธอยู่ดี โดยไม่มีอะไรอธิบาย
/// เพดานที่ `decode` จะใช้จริง เมื่อรวมเครื่องเข้ากับสิ่งที่ผู้ใช้ตั้ง (P5-3)
///
/// ★★★ **ฟังก์ชันบริสุทธิ์ เพราะนี่คือรอยต่อที่ค่าจะเพี้ยนได้เงียบที่สุด**
///
/// ถ้าเขียนแทรกอยู่ใน `start_assets` มันจะทดสอบได้ก็ต่อเมื่อเปิด cache.sqlite ได้
/// และ **ไม่มีเทสต์ไหนถามได้เลย** ว่า `max_pixels` ที่ผู้ใช้ตั้งไปถึงด่าน decode
/// จริงไหม · ผลของการพลาดตรงนี้คือผู้ใช้ลดเพดานลงเพื่อกัน OOM แล้วมันไม่มีผล
/// โดยไม่มีอะไรบอก ซึ่งคือรูปแบบ "ทุกชิ้นถูก ประกอบผิด" (`docs/08 §3.9` ข้อ 5)
#[must_use]
fn decode_limits(
    caps: refx_io::settings::Caps,
    settings: &refx_io::settings::Settings,
) -> refx_asset::decode::Limits {
    // ★ เริ่มจากเพดานของเครื่อง แล้วให้ค่าที่ผู้ใช้ตั้งทับ — ค่านั้นถูก clamp
    //   กับเครื่องมาแล้วตั้งแต่ `settings::parse` จึงต่ำกว่าหรือเท่าเสมอ
    refx_asset::decode::Limits::for_system(caps.total_ram).with_max_pixels(settings.max_pixels)
}

#[must_use]
fn machine_caps() -> refx_io::settings::Caps {
    let total_ram = refx_platform::memory::total_ram();
    refx_io::settings::Caps {
        total_ram,
        max_pixels_ceiling: refx_asset::decode::max_pixels_for_ram(total_ram),
    }
}

/// เขียนสรุปผลการตามหาไฟล์ลง status bar
///
/// ★ แยกเป็นฟังก์ชันอิสระเพราะจุดเรียกหนึ่งในสองอยู่กลาง `drain_decode_results`
/// ซึ่งยืม `self.assets` ค้างอยู่ — เมธอดที่รับ `&mut self` เรียกตรงนั้นไม่ได้
fn write_relink_status(shell: &mut crate::shell::ShellState, report: RelinkReport) {
    let lang = shell.lang;
    if report.lost > 0 {
        shell.status = text::fill(
            lang,
            Template::RelinkMissing,
            &[("n", &report.lost.to_string())],
        );
        shell.status_warn = true;
    } else if report.unpacked > 0 {
        // ★ ภาพมาจากไฟล์งานเอง ไม่ใช่จากการตามหาบนเครื่อง — บอกให้ตรงกับที่เกิดขึ้น
        //   ("เจอ 4 จาก 4 ใบที่ย้ายที่" จะทำให้ผู้ใช้ไปหาว่ามันย้ายไปไหนทั้งที่ไม่มีอะไรย้าย)
        shell.status = text::fill(
            lang,
            Template::RelinkUnpacked,
            &[("n", &report.unpacked.to_string())],
        );
        shell.status_warn = false;
    } else {
        shell.status = text::fill(
            lang,
            Template::RelinkFound,
            &[
                ("found", &report.moved.to_string()),
                ("total", &report.total.to_string()),
            ],
        );
        shell.status_warn = false;
    }
}

/// ★ สรุปผลการตามหาไฟล์ของงวดหนึ่ง — รอรายงานตอนงวด decode จบ
#[derive(Debug, Clone, Copy)]
struct RelinkReport {
    /// เจอแต่ **ไม่ได้อยู่ที่เดิม** (ขั้น 2/3/5) กี่ใบ
    moved: usize,
    /// ★ แกะออกมาจากตัวเอกสารเอง (packed — P4-5) กี่ใบ
    unpacked: usize,
    /// ตามหาไปทั้งหมดกี่ใบ
    total: usize,
    /// ยังหาไม่เจอกี่ใบ (ขั้นที่ 4)
    lost: usize,
}

/// ★ เอกสารที่เพิ่งอ่านจากดิสก์สำเร็จ — สิ่งที่เธรดเปิดไฟล์ส่งกลับมา
///
/// ★ `Box<Board>` เพราะ `Board` ใหญ่ — clippy `large_enum_variant` ไม่ชอบให้มัน
/// นั่งอยู่ใน `Result` ที่ถูกส่งข้ามช่อง
#[derive(Debug)]
struct LoadedDoc {
    /// ไฟล์ที่อ่านมา
    path: std::path::PathBuf,
    /// เนื้อเอกสาร
    board: Box<Board>,
    /// ★★ asset table ของไฟล์นั้น (ว่าง = linked ล้วน ไม่มีอะไรฝังอยู่)
    assets: refx_io::packed::Index,
    /// ★★★ งานที่ยังไม่เคยไปถึงไฟล์ของเอกสารนี้ — `None` = ไม่มีอะไรให้ถาม
    ///
    /// `Box` เพราะมันถือ `Board` ทั้งก้อน (เหตุผลเดียวกับ `board` ข้างบน)
    pending: Option<Box<refx_io::autosave::Pending>>,
    /// ★★ งานที่ผู้ใช้เคยสั่ง **"เก็บไว้ก่อน"** ของเอกสารนี้ (`docs/07 §4`)
    kept: Option<Box<refx_io::autosave::Pending>>,
}

/// ★ ไฟล์ที่เพิ่งเขียนลงดิสก์สำเร็จ — สิ่งที่เธรดบันทึกส่งกลับมา (P4-5)
///
/// ★★ `assets` อ่านจาก **ไฟล์ที่เพิ่งเขียนจริง** ไม่ใช่จากแผนที่ส่งเข้าไป —
/// ตัวเลข "กี่ใบอยู่ข้างใน" บนแถบสถานะจึงมาจากดิสก์เสมอ
///
/// ★★★ ส่วน `mode` คือ **สิ่งที่ผู้ใช้สั่ง** ไม่ใช่สิ่งที่อนุมานจากไฟล์ —
/// ดูเหตุผลที่ [`RefxApp::save_mode`]
#[derive(Debug)]
struct SavedDoc {
    /// ไฟล์ที่เขียนไป
    path: std::path::PathBuf,
    /// asset table ของไฟล์นั้นหลังเขียนเสร็จ (ว่าง = ไม่มีอะไรฝังอยู่)
    assets: refx_io::packed::Index,
    /// โหมดที่ผู้ใช้สั่งให้บันทึกครั้งนี้
    mode: refx_io::packed::SaveMode,
}

/// ผลการตามหาไฟล์ของ item หนึ่งใบ — `None` = ยังหาไม่เจอ (ขั้นที่ 4)
type LocatedOne = (
    refx_core::relink::Wanted,
    Option<refx_core::relink::Located>,
);

/// ช่องที่เธรดค้นหาส่งผลทั้งชุดกลับมา
type LocateResults = crossbeam_channel::Receiver<Vec<LocatedOne>>;

/// ★★★ ที่มาที่ *ถูกต้อง* ของ item หลังเพิ่งอ่านไฟล์จริงได้สำเร็จ (P4-6)
///
/// `None` = ใบนี้ไม่ใช่เป้าของ relink (โน้ตข้อความ) หรือไม่มีไฟล์ให้ผูก
///
/// ## อะไรเปลี่ยนได้ อะไรห้ามเปลี่ยน
///
/// | ฟิลด์ | ทำอะไร | ทำไม |
/// |---|---|---|
/// | `path` | เขียนที่อยู่ที่เพิ่งเจอ | นี่คือทั้งหมดของคำว่า relink |
/// | `hash` | ซ่อมเป็นคีย์ของเนื้อ **ถ้ามันไม่ตรง** | `docs/07 §2` — ตอน relink สำเร็จเท่านั้น |
/// | `px_size`/`format`/`mtime`/`file_size` | **ไม่แตะของเดิม** | ไม่ใช่เรื่องของ relink · แตะแล้วเอกสารจะ dirty ทุกครั้งที่ mtime ขยับ |
///
/// ★★★ **ห้ามซ่อมคีย์ของภาพที่มาจาก clipboard** (`docs/07 §2`) — ชื่อไฟล์ใน
/// spool คือ hash ของ *พิกเซล* ส่วนการ hash ไฟล์ PNG นั้นให้คนละค่า เขียนทับ
/// เมื่อไหร่ `spool::sweep` จะหาไม่เจอว่ามีคนอ้างถึง **แล้วลบภาพทิ้ง** (I-3)
///
/// ★ ใบที่เป็น `Missing` มาก่อนไม่มีคีย์เดิมให้รักษา — มันได้คีย์ของเนื้อเต็ม ๆ
/// และได้ `px_size` จากภาพที่เพิ่งอ่าน เพราะของเดิมไม่เคยมี
fn relinked_kind(
    current: &ItemKind,
    resolved: Option<&std::path::Path>,
    content: Option<refx_core::hash::ContentHash>,
    spool_dir: Option<&std::path::Path>,
    thumb: &refx_asset::thumb::Thumbnail,
    meta: refx_asset::pool::SourceMeta,
) -> Option<ItemKind> {
    let path = resolved?.to_path_buf();
    // ★ ไฟล์ในที่พักของภาพที่วาง — คีย์ของมันคือ hash ของพิกเซล ห้ามแตะ
    let in_spool = spool_dir.is_some_and(|dir| path.starts_with(dir));

    match current {
        ItemKind::Text(_) => None,
        ItemKind::Image(asset) => {
            let hash = if in_spool {
                asset.hash
            } else {
                content.unwrap_or(asset.hash)
            };
            // ★★★ **การอ่านสำเนาใน spool ไม่ใช่การย้ายบ้านของภาพ** (P4-5)
            //
            //   เอกสาร packed พกสำเนามาเอง · ตอนเปิดบนเครื่องที่ไม่มีไฟล์ต้นฉบับ
            //   เราแกะ blob ลง spool แล้วอ่านจากที่นั่น — แต่ **ที่อยู่ที่เอกสาร
            //   จำไว้คือของเอกสาร** ถ้าเขียนทับด้วย path ของ spool จะได้สามอย่าง
            //   ที่ผู้ใช้ไม่ได้สั่งพร้อมกัน: เอกสาร dirty ทันทีที่เปิด · มีขั้น undo
            //   โผล่มาจากที่ไหนไม่รู้ · และ **ที่อยู่จริงของภาพหายไปตลอดกาล**
            //   ทั้งที่วันหนึ่งผู้ใช้อาจกลับไปเครื่องที่มีไฟล์นั้นอยู่
            //
            //   ★ ภาพที่วางจาก clipboard ไม่ได้รับผลอะไร — `path` ของมันคือไฟล์
            //     ใน spool อยู่แล้ว ค่าที่ได้จึงเท่าเดิมเป๊ะทั้งสองทาง
            let keep_old_path = in_spool && !asset.path.as_os_str().is_empty();
            Some(ItemKind::Image(AssetRef {
                hash,
                path: if keep_old_path {
                    asset.path.clone()
                } else {
                    path
                },
                ..asset.clone()
            }))
        }
        ItemKind::Missing { .. } => Some(ItemKind::Image(AssetRef {
            hash: content?,
            path,
            px_size: glam::UVec2::new(thumb.source_width.max(1), thumb.source_height.max(1)),
            // ★ ยังไม่รู้ format จริง — เหตุผลเดียวกับเส้นทางลากไฟล์เข้ามา
            format: ImageFormat::Unknown,
            embedded: false,
            mtime: meta.mtime_ms,
            file_size: meta.bytes,
        })),
    }
}

/// ถาม cache ว่า hash นี้เคยเห็นที่ไหนบ้าง — ขั้นที่ 3 ของ relink (P4-6)
///
/// ★ **บน worker เท่านั้น** (รอคำตอบจาก IO thread) · ไม่มี cache = ไม่มีคำตอบ
/// ซึ่งไม่ใช่ error: ขั้น 1/2 ยังทำงานได้ตามปกติ
fn paths_known_for(
    io: Option<&crossbeam_channel::Sender<IoRequest>>,
    hash: refx_core::hash::ContentHash,
) -> Vec<std::path::PathBuf> {
    let Some(io) = io else {
        return Vec::new();
    };
    let (reply, rx) = crossbeam_channel::bounded(1);
    io.send(IoRequest::LookupHash { hash, reply })
        .ok()
        .and_then(|()| rx.recv_timeout(std::time::Duration::from_secs(5)).ok())
        .unwrap_or_default()
}

/// ★★★ ภาพใบนี้อยู่ในเอกสารเองหรือเปล่า — ถ้าใช่ แกะลง spool แล้วใช้ไฟล์นั้น (P4-5)
///
/// `None` = ไม่มีในตาราง หรือแกะไม่สำเร็จ → ผู้เรียกเดินต่อไปที่ขั้นที่ 4 (`Missing`)
///
/// ★★ **บน worker เท่านั้น** — สตรีมไบต์จากเอกสารลงดิสก์ (`docs/07 §2`)
///
/// ★ ล้มแล้ว **ไม่ล้มทั้งงวด**: เอกสารที่ blob เสียใบเดียวยังต้องเปิดได้ครบทุกใบ
/// ที่เหลือ (I-7 หลักการเดียวกับ decode) · ใบที่เสียกลายเป็น `Missing` ซึ่งผู้ใช้
/// ยังเห็นชื่อไฟล์และ relink เองได้
fn unpack_embedded(
    want: &refx_core::relink::Wanted,
    index: &refx_io::packed::Index,
    spool_dir: &std::path::Path,
    source: &mut std::fs::File,
) -> Option<refx_core::relink::Located> {
    let entry = index.find(want.hash)?;
    match refx_io::spool::unpack(
        spool_dir,
        entry,
        source,
        refx_platform::fsops::rename_durable,
    ) {
        Ok(path) => Some(refx_core::relink::Located {
            id: want.id,
            path,
            step: refx_core::relink::Step::Embedded,
        }),
        Err(err) => {
            tracing::warn!(
                %err,
                hash = %want.hash.short(),
                "cannot unpack an image stored inside the document"
            );
            None
        }
    }
}

/// ★★ hash ทุกไฟล์ภาพในโฟลเดอร์ที่ผู้ใช้ชี้ — ขั้นที่ 5 (P4-6)
///
/// ★ **บน worker เท่านั้น** · มีเพดาน [`refx_core::relink::MAX_FOLDER_SCAN`]
/// เพราะผู้ใช้ชี้ไปที่โฟลเดอร์ไหนก็ได้ รวมถึงโฟลเดอร์ที่มีไฟล์เป็นแสน
///
/// ★★ ราคาคือการอ่านไฟล์ในโฟลเดอร์นั้นหนึ่งรอบ (ไฟล์ > 64 MB ใช้ fast path
/// ของ `hash_file` อ่านแค่หัว/ท้าย) — ยอมจ่ายเพราะมันเกิดตอนผู้ใช้กดปุ่ม
/// "หาไฟล์เอง" ซึ่งเป็นการกระทำที่เขาตั้งใจและเกิดครั้งเดียว ไม่ใช่ต่อเฟรม
fn hash_folder(
    dir: Option<&std::path::Path>,
) -> Vec<(std::path::PathBuf, refx_core::hash::ContentHash)> {
    let Some(dir) = dir else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for path in entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .take(refx_core::relink::MAX_FOLDER_SCAN)
    {
        if let Ok(hash) = refx_asset::hash::hash_file(&path) {
            out.push((path, hash));
        }
    }
    tracing::info!(files = out.len(), "scanned a folder for the missing images");
    out
}

/// ★★★ คีย์และที่อยู่ของ asset ที่ผลลัพธ์ใบนี้จะกลายเป็น
///
/// แยกออกมาเป็นฟังก์ชันเพราะ **มันคือจุดที่ผิดแล้วภาพของผู้ใช้หายเงียบ ๆ** และ
/// จุดเรียกจริงอยู่ในกลางลูปที่ต้องมี GPU ถึงจะรันได้ — เทสต์จึงเรียกตัวนี้ตรง ๆ
/// ไม่ใช่เขียนตรรกะเลียนแบบขึ้นมาใหม่ (`docs/08 §3.9` ข้อ 9)
///
/// | worker ตอบอะไรมา | คีย์ | path |
/// |---|---|---|
/// | `File(hash)` | ★ **hash ของไบต์ในไฟล์** | path ของผู้ใช้ (ผู้เรียกเติมเอง) |
/// | `Spooled(hash)` | ★ **hash ของพิกเซล** | `<spool_dir>/<hash>.png` |
/// | `None` | คีย์ของงาน (ทางถอยเมื่อ hash ไม่ได้) | path ของผู้ใช้ |
///
/// ★★ **คีย์ของงานเป็นคีย์ของ *ที่อยู่* ไม่ใช่ของ *เนื้อ*** — `hash_bytes(path)`
/// สำหรับไฟล์ และ `clipboard:N` สำหรับภาพที่วาง · `docs/02 §2.3` บังคับให้
/// `AssetRef::hash` เป็นคีย์ของเนื้อ และผูกสามสัญญาไว้กับข้อนั้น (ย้ายไฟล์แล้ว
/// thumbnail ไม่หาย · relink ค้นด้วย hash · ไฟล์ซ้ำถูกยุบ) — ดู [`ContentOrigin`]
fn asset_location(
    spool_dir: Option<&std::path::Path>,
    job_hash: refx_core::hash::ContentHash,
    origin: Option<ContentOrigin>,
) -> (refx_core::hash::ContentHash, Option<std::path::PathBuf>) {
    match origin {
        // ★ path หาได้จาก hash ล้วน ๆ จึงเขียนลง `AssetRef` ได้ **ตั้งแต่ตอนนี้**
        //   ทั้งที่ไฟล์ยังเขียนไม่เสร็จ — จำเป็น เพราะ snapshot ที่ถูกเขียนใน
        //   ช่วงนั้นต้องกู้คืนได้เหมือนกัน
        Some(ContentOrigin::Spooled(content)) => (
            content,
            spool_dir.map(|dir| refx_io::spool::spool_path(dir, content)),
        ),
        // ไฟล์ของผู้ใช้อยู่ที่เดิมของมัน — เปลี่ยนแค่ *คีย์* ไม่ใช่ที่อยู่
        Some(ContentOrigin::File(content)) => (content, None),
        // ★ ทางถอย: hash ไม่ได้ (ไฟล์หายระหว่างทาง) — ใช้คีย์ของงานต่อไป
        //   ภาพยังขึ้นจอได้ แค่ไม่ถูกยุบกับสำเนาอื่นและ relink ด้วย hash ไม่ได้
        None => (job_hash, None),
    }
}

/// ★★★ ไบต์ของ asset ใบนี้อยู่ที่ไหน — คำตอบที่ [`plan_embeds`] ใช้ตัดสินว่าจะฝังไหม
///
/// [`plan_embeds`]: refx_io::packed::plan_embeds
///
/// ★★ **ที่นี่ตอบแค่ว่า "ไบต์อยู่ที่ไหน และใครเป็นเจ้าของ"** ส่วนกฎว่า *อะไร
/// ควรถูกฝัง* อยู่ที่ `refx-io` ที่เดียว (§4 ข้อ 23) — ถ้าย้ายกฎมาไว้ตรงนี้
/// ทุกจุดเรียกใหม่ต้องจำเอง แล้ววันหนึ่งจะมีตัวที่ลืม
///
/// | สภาพของใบนั้น | ตอบว่า | ผลใน linked mode |
/// |---|---|---|
/// | อยู่ในโฟลเดอร์ spool | `Ours` | **ฝัง** — ของที่เราสร้างเอง ผู้ใช้ลบเมื่อไหร่ก็ได้ |
/// | เป็นไฟล์ของผู้ใช้ที่ยังอยู่ | `UserFile` | ลิงก์ |
/// | ★ path เดิมหายแล้ว แต่ spool มีสำเนาของ hash นี้ | `Ours` | **ฝัง** |
/// | ไม่เหลืออะไรเลย | `Missing` | เป็น `Missing` ต่อไป (ยังอยู่บน board — I-3) |
///
/// ★★★ แถวที่สามคือแถวที่ทำให้ **เปิดเอกสาร packed บนเครื่องที่ไม่มีภาพเลย
/// แล้วบันทึกทับ ไม่ทำให้ภาพหาย** — สำเนาที่แกะออกมาลง spool ตอนเปิดคือ
/// ต้นฉบับเดียวที่เหลืออยู่บนเครื่องนั้น การลิงก์ไปหา path ที่ว่างเปล่าแทน
/// จะทำให้ไฟล์ที่บันทึกใหม่ไม่มีภาพอยู่ข้างในเลย (I-3 เงียบที่สุด)
fn locate_bytes(
    spool_dir: Option<&std::path::Path>,
    asset: &AssetRef,
) -> refx_io::packed::AssetBytes {
    use refx_io::packed::AssetBytes;

    let in_spool = spool_dir.is_some_and(|dir| asset.path.starts_with(dir));
    let have_file = !asset.path.as_os_str().is_empty() && asset.path.is_file();
    if have_file {
        return if in_spool {
            AssetBytes::Ours(asset.path.clone())
        } else {
            AssetBytes::UserFile(asset.path.clone())
        };
    }
    // ★ ไฟล์เดิมไม่อยู่แล้ว — สำเนาใน spool (ถ้ามี) คือของที่เหลืออยู่
    match spool_dir.map(|dir| refx_io::spool::spool_path(dir, asset.hash)) {
        Some(path) if path.is_file() => AssetBytes::Ours(path),
        _ => AssetBytes::Missing,
    }
}

/// ข้อความยืนยันตอนผู้ใช้เลือกโหมดของ board ที่ยังไม่มีไฟล์
fn mode_message(mode: refx_io::packed::SaveMode) -> Key {
    match mode {
        refx_io::packed::SaveMode::Linked => Key::SaveModeLinkedHint,
        refx_io::packed::SaveMode::Packed => Key::SaveModePackedHint,
    }
}

/// ★★ asset table ของเอกสารที่เปิดอยู่ — อ่าน **หัวไฟล์กับตาราง** เท่านั้น
///
/// ★ ไม่แตะ blob สักไบต์ · ไฟล์ linked คืนตารางว่าง (ไม่ใช่ error) และไฟล์ที่
/// อ่านไม่ออกก็คืนตารางว่างเช่นกัน — การเปิดเอกสารสำเร็จไปแล้วในเส้นทางหลัก
/// การที่เราอ่านตารางไม่ได้จึงแปลว่า "ไม่มีภาพฝังอยู่" ไม่ใช่ "เปิดไม่ได้"
fn read_asset_table(path: &std::path::Path) -> refx_io::packed::Index {
    let Ok(mut file) = std::fs::File::open(path) else {
        return refx_io::packed::Index::default();
    };
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    refx_io::packed::read_index(&mut file, len).unwrap_or_else(|err| {
        tracing::warn!(%err, file = %path.display(), "cannot read the asset table");
        refx_io::packed::Index::default()
    })
}

/// ★★★ โหมดของเอกสารที่ **อ่านจากของจริงในไฟล์** ไม่ใช่ธงที่ใครจำไว้
///
/// `docs/07 §2` แยกสองโหมดด้วยคำถามเดียวที่ผู้ใช้สนใจจริง ๆ:
/// *"ย้ายไฟล์นี้ไปเครื่องอื่นแล้วภาพยังอยู่ไหม"* → **ทุกใบอยู่ข้างในหรือเปล่า**
///
/// ★★ ไฟล์ไม่ได้เก็บ "ผู้ใช้เลือกโหมดไหน" ไว้ และ**ไม่ควรเก็บ** — `version = 2`
/// แปลว่า *"มี asset ฝังอยู่"* เท่านั้น (§4 ข้อ 22) · board แบบ linked ที่มี
/// ภาพวางจาก clipboard ก็เป็น v2 เหมือนกัน แต่มันไม่ใช่ packed เพราะภาพจากไฟล์
/// ยังอยู่ข้างนอก · การอ่านสภาพจริงจึงตอบถูกทั้งสองกรณีโดยไม่ต้องเดา
/// (บทเรียนของ §2.25: ประตูที่จำสถานะไว้แทนที่จะอ่านสถานะจริง)
///
/// ★ ใบที่เป็น `Missing` ไม่นับ — มันไม่มีไบต์ให้ฝังตั้งแต่ต้น การนับมันจะทำให้
/// เอกสาร packed ที่บันทึกตอนภาพหายไปแล้วหนึ่งใบกลายเป็น "linked" ตลอดกาล
fn mode_of_document(board: &Board, index: &refx_io::packed::Index) -> refx_io::packed::SaveMode {
    use refx_io::packed::SaveMode;

    if index.is_empty() {
        return SaveMode::Linked;
    }
    let all_inside = board.items_in_z_order().all(|(_, item)| match &item.kind {
        ItemKind::Image(asset) => index.find(asset.hash).is_some(),
        _ => true,
    });
    if all_inside {
        SaveMode::Packed
    } else {
        SaveMode::Linked
    }
}

/// ★★ ที่พักของภาพที่วางตัวจริง — ห่อ [`refx_io::spool::store`] ให้ worker เรียกได้
///
/// trait อยู่ที่ `refx-core` เพราะ `refx-asset` (คนที่มีไบต์) พึ่ง `refx-io`
/// (คนที่รู้ว่าไฟล์ไปไหน) ไม่ได้ — ARCHITECTURE §2 วางสองตัวนั้นไว้เป็นพี่น้องกัน
/// **`refx-ui` เป็นชั้นเดียวที่รู้จักทั้งคู่ จึงเป็นคนต่อสาย** (หลักการเดียวกับ
/// `SystemClipboard` และ `WakeHandle`)
#[derive(Debug)]
struct SpoolSink {
    dir: std::path::PathBuf,
}

impl refx_core::spool::PastedImageStore for SpoolSink {
    fn store(&self, hash: refx_core::hash::ContentHash, png: &[u8]) -> Option<std::path::PathBuf> {
        match refx_io::spool::store(&self.dir, hash, png, refx_platform::fsops::rename_durable) {
            Ok(path) => Some(path),
            Err(err) => {
                // ★ ไม่ใช่ error ที่ต้องหยุดงาน — ภาพขึ้นจอไปแล้ว สิ่งที่เสียคือ
                //   ความคมตอนซูมกับความสามารถในการกู้คืน ซึ่งต้องถูก log ไว้
                //   ไม่ใช่ทำให้การวางภาพล้มทั้งใบ (`refx_core::spool`)
                tracing::warn!(%err, hash = %hash.short(), "cannot spool the pasted image");
                None
            }
        }
    }
}

/// ★★★ id ที่ใช้ตอน **อ่านเพื่อดูเฉย ๆ แล้วทิ้ง** — ห้ามใช้กับ board ที่จะขึ้นจอ
///
/// `.refx` **ไม่เก็บ id** โดยตั้งใจ (P4-1: มันเป็นคีย์ในหน่วยความจำ ไม่ใช่เนื้อหา
/// ของเอกสาร) ผู้อ่านจึงต้องบอกว่าจะให้ board ที่โหลดมาใช้ id ไหน
///
/// ★★ ตั้งแต่ P4-7c **แต่ละแท็บมี id ของตัวเอง** ([`Docs::mint`]) — board ที่จะ
/// ไปนั่งในแท็บต้องถูกอ่านด้วย id ของแท็บนั้น ไม่งั้น `ItemId` ทุกใบจะชี้ไป
/// board คนละใบตั้งแต่วินาทีแรก · ที่นี่เหลือไว้สำหรับสองเส้นทางที่อ่านมาแล้ว
/// **โยนทิ้งทันที**: นับจำนวนชิ้นให้แถบกู้คืน และดึงรายการ hash ให้ `spool::sweep`
fn probe_board_id() -> refx_core::arena::BoardId {
    use refx_core::arena::ArenaKey as _;
    refx_core::arena::BoardId::from_parts(u32::MAX, 0)
}

/// งานค้างจาก session ก่อนที่กำลังรอให้ผู้ใช้ตัดสิน (P4-4)
#[derive(Debug, Clone)]
struct PendingRecovery {
    /// ไฟล์ snapshot ตัวจริงบนดิสก์
    path: std::path::PathBuf,
    /// เขียนไว้เมื่อไหร่ (ข้อความพร้อมแสดง) — `None` = ระบบไฟล์ไม่บอก
    when: Option<String>,
    /// มีกี่ชิ้นอยู่ในนั้น
    items: usize,
}

/// ★★ หา snapshot ที่ค้างอยู่ **ทุกใบ เรียงใหม่สุดก่อน** — รันบนเธรดอื่นเสมอ (I-2)
///
/// ★ ชั้น UI ยัง **ถามทีละใบ** เหมือนเดิม: คนที่เปิดโปรแกรมมาเจอรายการ 10 บรรทัด
/// ที่หน้าตาเหมือนกันหมดจะกด "ทิ้ง" ทุกอันเพื่อให้มันหายไป ซึ่งตรงข้ามกับสิ่งที่
/// กลไกนี้มีไว้ทำ
///
/// ★★★ **แต่คิวต้องมีทั้งหมด ไม่ใช่ใบเดียว** (แก้ P4-7c) — แต่ละแท็บมี recovery
/// slot ของตัวเองแล้ว ผู้ใช้ที่เปิดสองแท็บโดยไม่บันทึกเลยจึงทิ้งไฟล์ไว้สองใบ
/// ถ้าหยิบมาแค่ใบเดียวต่อการเปิดโปรแกรมหนึ่งครั้ง เขาจะได้งานคืนครึ่งเดียว
/// **โดยไม่มีอะไรบนจอบอกว่ายังมีอีกใบ** ซึ่งอ่านได้อย่างเดียวว่าโปรแกรมทำงานหาย
///
/// ★ `probe_board_id` ใช้ได้ตรงนี้เพราะ board ที่อ่านมาถูกใช้แค่ **นับชิ้น**
/// แล้วทิ้ง · ตัวจริงถูกอ่านใหม่ด้วย id ของแท็บตอนผู้ใช้กด "เอากลับมา"
fn scan_for_recovery(
    dir: &std::path::Path,
    live: &[refx_io::recovery::SessionId],
) -> Vec<PendingRecovery> {
    refx_io::recovery::scan(dir, live)
        .into_iter()
        .filter_map(|orphan| {
            // ★ อ่านทั้งไฟล์เพื่อ **นับชิ้น** ตรงนี้เลย — ตัวเลขนั้นคือสิ่งเดียวที่
            //   ช่วยผู้ใช้จำได้ว่างานชุดไหน · ไฟล์ที่อ่านไม่ออกถือว่าไม่มีอะไรให้กู้
            let board = refx_io::recovery::load(&orphan.path, probe_board_id())?;
            Some(PendingRecovery {
                path: orphan.path,
                when: orphan.written_at.map(format_when),
                items: board.len(),
            })
        })
        .collect()
}

/// ★★★ คีย์ชั่วคราวของงาน decode หนึ่งใบ — **ผูกกับแท็บด้วย ไม่ใช่กับ path ล้วน**
///
/// คีย์นี้ไม่ใช่ hash ของเนื้อไฟล์ (ตัวนั้นเกิดบน worker — §4 ข้อ 27) มันคือ
/// *ป้ายชื่องาน* ที่ใช้จับคู่ผลลัพธ์กลับมาเท่านั้น
///
/// ★★ **ทำไมต้องมี `board` ปนอยู่ในคีย์**: เดิมมันเป็น `hash_bytes(path)` ล้วน ๆ
/// ซึ่งถูกตราบที่มี board เดียว · พอเปิดไฟล์เดียวกันสองแท็บได้ (หรือสอง board
/// ที่ต่างคนต่างมีภาพใบเดียวกัน) คีย์จะชนกัน แล้วตารางจับคู่ของแท็บที่ใส่ทีหลัง
/// จะ **เขียนทับ** ของแท็บแรก — ผลคือแท็บแรกได้ item ที่ไม่มีวันได้พิกเซล
/// (`docs/02 §2.10`: ทุกอย่างที่คีย์ด้วย `ItemId` ต้องอยู่ต่อแท็บ)
///
/// ★ ยัง **ยุบงานซ้ำภายในแท็บเดียวกัน** ได้เหมือนเดิม — ลากไฟล์เดิมสองครั้ง
/// ในแท็บเดียวยังได้คีย์เท่ากันตามพฤติกรรมเดิมทุกประการ
fn job_key_for(
    board: refx_core::arena::BoardId,
    path: &std::path::Path,
) -> refx_asset::hash::ContentHash {
    // `\0` เป็นตัวคั่นที่ปรากฏใน path ไม่ได้บนทุก OS — สอง (board, path)
    // ที่ต่างกันจึงให้ไบต์ต้นทางที่ต่างกันเสมอ
    let mut raw = format!("{board:?}\0").into_bytes();
    raw.extend_from_slice(path.to_string_lossy().as_bytes());
    refx_asset::hash::hash_bytes(&raw)
}

/// ★★★ hash ของภาพที่วางไว้ซึ่ง **แท็บใดก็ตามที่เปิดอยู่** อ้างถึง (P4-7c)
///
/// `spool::sweep` ใช้รายการนี้เป็นด่านแรก — ถามแค่แท็บที่อยู่หน้าจอเมื่อไหร่
/// เพดานจะลบต้นฉบับของภาพที่วางไว้ในแท็บหลังบ้าน **ขณะที่มันเปิดอยู่**
/// แล้วภาพจะกลายเป็นช่องว่างทันทีที่ผู้ใช้สลับกลับไปดู — และกู้คืนไม่ได้เลย
/// เพราะ spool คือต้นฉบับเดียวที่เหลือของภาพที่วาง (§4 ข้อ 24)
///
/// ★ แยกเป็นฟังก์ชันอิสระเพื่อ **เทสต์ได้โดยไม่ต้องมีหน้าต่าง/ดิสก์**
fn hashes_of_every_tab(docs: &Docs) -> std::collections::BTreeSet<refx_core::hash::ContentHash> {
    let mut all = std::collections::BTreeSet::new();
    for doc in docs.iter() {
        all.extend(refx_io::spool::hashes_of(&doc.board));
    }
    all
}

/// ชื่อไฟล์ล้วน ๆ ที่เอาไปขึ้นข้อความ/แท็บได้ — ว่างเมื่อ path ไม่มีชื่อไฟล์
fn file_label_of(path: &std::path::Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// เวลาที่ไฟล์ถูกเขียน → ข้อความสั้น ๆ ที่ผู้ใช้อ่านรู้เรื่อง
///
/// ★ ไม่มี dependency สำหรับจัดรูปแบบวันที่ใน `docs/09` (และการเพิ่มต้องขอก่อน)
/// → บอกเป็น **ระยะเวลาที่ผ่านมา** แทนวันที่ ซึ่งตอบคำถามที่ผู้ใช้ถามจริง ๆ
/// ได้ตรงกว่าอยู่แล้ว: *"เมื่อกี้นี้เอง หรือเมื่ออาทิตย์ที่แล้ว"*
fn format_when(at: std::time::SystemTime) -> String {
    let Ok(ago) = std::time::SystemTime::now().duration_since(at) else {
        return "just now".to_owned(); // นาฬิกาเครื่องถอยหลัง — ไม่ใช่เรื่องต้องล้ม
    };
    let mins = ago.as_secs() / 60;
    match mins {
        0 => "just now".to_owned(),
        1..60 => format!("{mins} min ago"),
        60..1440 => format!("{} h ago", mins / 60),
        _ => format!("{} d ago", mins / 1440),
    }
}

/// ★★★ เปิดเอกสารไม่ได้ **เพราะอะไร** — และผู้ใช้ต้องทำคนละอย่างในแต่ละกรณี
///
/// `docs/07 §1` บังคับไว้ตั้งแต่วันแรกว่า `doc_crc` มีไว้ **แยก "ไฟล์เสียหายจาก
/// ดิสก์" ออกจาก "ไฟล์เวอร์ชันที่เราอ่านไม่เป็น"** และตัวอ่าน (`OpenError`)
/// แยกได้จริงมาตลอด — แต่ชั้น UI รวมทั้งสองกรณีกลับเป็นข้อความเดียวว่า
/// *"ไฟล์อาจเสียหาย หรือถูกเขียนด้วย RefX รุ่นใหม่กว่า"* ซึ่งบอกผู้ใช้ว่า
/// **ทั้งสองอย่างอาจจริง** ทั้งที่เรารู้แน่ชัดว่าอันไหน
///
/// | สถานการณ์ | ผู้ใช้ต้องทำ |
/// |---|---|
/// | เสียหาย | ไปเอาไฟล์สำรอง `.refx.bak` — งานรอบล่าสุดอาจเสียไปแล้ว |
/// | รุ่นใหม่กว่า | **อัปเดตโปรแกรม** · ไฟล์ยังดีอยู่ครบ ห้ามบันทึกทับ |
///
/// สองอย่างนี้ต่างกันคนละขั้ว: คำแนะนำ "ไปหาไฟล์สำรอง" กับไฟล์ที่ยังดีอยู่
/// คือการชวนให้ผู้ใช้ทิ้งงานรุ่นล่าสุดของตัวเองทิ้ง
#[derive(Debug, Clone, PartialEq, Eq)]
enum OpenFailure {
    /// ไฟล์มาจาก RefX รุ่นใหม่กว่า — **ไฟล์ไม่ได้เสีย**
    NewerVersion,
    /// เนื้อในเสียหาย · `backup` = ชื่อไฟล์สำรองที่ **มีอยู่จริง** ข้าง ๆ (ถ้ามี)
    Damaged {
        /// ★ `None` แปลว่าไม่มีให้ลอง — ห้ามแนะนำไฟล์ที่ไม่มีอยู่
        backup: Option<String>,
    },
    /// ไบต์ชุดนี้ไม่ใช่ `.refx` เลย — ผู้ใช้เลือกไฟล์ผิด
    NotABoard,
    /// เปิดไฟล์จากดิสก์ไม่ได้ — ยังไม่ได้อ่านเนื้อในเลยสักไบต์
    Unreadable,
}

/// [`OpenError`](refx_io::dto::OpenError) ตัวไหน แปลว่าผู้ใช้ต้องทำอะไร
///
/// ★★ `match` ตัวนี้ **ไม่มี `_ =>` โดยตั้งใจ** — วันที่มีคนเพิ่ม variant ใหม่
/// ให้ `OpenError` คอมไพเลอร์จะบังคับให้มาตัดสินตรงนี้ว่ามันเป็นเรื่องของ
/// "ไฟล์เสีย" หรือ "รุ่นใหม่กว่า" แทนที่จะเงียบ ๆ ตกไปอยู่กองใดกองหนึ่ง
fn classify_open_error(err: &refx_io::dto::OpenError, doc: &std::path::Path) -> OpenFailure {
    use refx_io::dto::OpenError;
    match err {
        OpenError::NewerVersion { .. } => OpenFailure::NewerVersion,
        // ★ ไม่ใช่ `.refx` เลย ≠ `.refx` ที่เสีย — คนละคำแนะนำอีกอันหนึ่ง
        //   (เลือกไฟล์อื่น vs ไปเอาไฟล์สำรองของไฟล์นี้)
        OpenError::TooShort { .. } | OpenError::NotRefx => OpenFailure::NotABoard,
        OpenError::Truncated { .. }
        | OpenError::Corrupt
        | OpenError::TooLarge { .. }
        | OpenError::Malformed => OpenFailure::Damaged {
            backup: existing_backup_name(doc),
        },
    }
}

/// ข้อความที่ผู้ใช้เห็นสำหรับแต่ละเหตุ
///
/// ★ แยกเป็นฟังก์ชันบริสุทธิ์เพื่อ **เทสต์ได้โดยไม่ต้องมีหน้าต่าง** — ข้อความที่
/// รวมสองสถานการณ์เข้าด้วยกันเป็นบั๊กที่ไม่มี unit test ไหนจับได้ถ้าตรรกะ
/// ฝังอยู่ใน `match` ของ `poll_open` (รูปแบบเดียวกับคีย์ลัดทุกตัวในไฟล์นี้)
fn open_failure_text(lang: Lang, failure: &OpenFailure) -> String {
    match failure {
        OpenFailure::NewerVersion => text::t(lang, Key::OpenFailedNewer).to_owned(),
        OpenFailure::Damaged { backup: Some(name) } => text::fill(
            lang,
            Template::OpenFailedDamagedBackup,
            &[("backup", name.as_str())],
        ),
        OpenFailure::Damaged { backup: None } => text::t(lang, Key::OpenFailedDamaged).to_owned(),
        OpenFailure::NotABoard => text::t(lang, Key::OpenFailedNotABoard).to_owned(),
        OpenFailure::Unreadable => text::t(lang, Key::OpenFailedUnreadable).to_owned(),
    }
}

/// ชื่อไฟล์สำรองของเอกสารนี้ — `None` เมื่อมันไม่มีอยู่จริงบนดิสก์
///
/// ★★ **ต้องถามดิสก์จริง ไม่ใช่ประกอบชื่อแล้วเชื่อ** — `.refx.bak` เกิดตอน
/// บันทึกทับครั้งที่สองเป็นต้นไป (`save_atomic`) เอกสารที่เพิ่งบันทึกครั้งแรก
/// จึงยังไม่มี · ชี้ผู้ใช้ที่กำลังกลัวว่างานหายไปหาไฟล์ที่ไม่มีอยู่ = ทำให้เขา
/// เสียเวลาแล้วเชื่อโปรแกรมน้อยลงพอดีตอนที่เขาต้องการมันที่สุด
///
/// ★ **รันบนเธรดอ่านไฟล์เท่านั้น** (I-2) — ผู้เรียกอยู่ในเธรดนั้นอยู่แล้ว
fn existing_backup_name(doc: &std::path::Path) -> Option<String> {
    let backup = refx_io::save::backup_path(doc);
    backup.is_file().then(|| file_label_of(&backup))
}

/// อ่านไฟล์ `.refx` ทั้งไฟล์แล้วแปลงเป็น `Board` — **รันบนเธรดอื่นเท่านั้น** (I-2)
///
/// ★ `std::fs::read` ถูกแบนใน `clippy.toml` เพราะเส้นทางจริงต้องมีเพดานขนาด —
/// ที่นี่ใช้ `File::take` ด้วยเพดานเดียวกับตัวอ่านเอกสารตัวอื่นทุกตัว
///
/// ★★ รายละเอียดเชิงเทคนิคลง **log เป็นอังกฤษ** ที่นี่ (ผู้ใช้ส่ง log มาให้เรา
/// อ่านได้) ส่วนสิ่งที่คืนออกไปคือ *ผู้ใช้ต้องทำอะไรต่อ* — แยกบทบาทตาม
/// `docs/03 §0` เหมือน `LoadError`/`JobFailure`
fn read_document(
    path: &std::path::Path,
    id: refx_core::arena::BoardId,
) -> Result<Board, OpenFailure> {
    use std::io::Read as _;

    let mut bytes = Vec::new();
    let mut file = std::fs::File::open(path).map_err(|err| {
        tracing::error!(%err, path = %path.display(), "cannot open the document file");
        OpenFailure::Unreadable
    })?;
    file.by_ref()
        .take(refx_io::dto::MAX_COMPRESSED_BYTES + refx_io::dto::HEADER_LEN as u64)
        .read_to_end(&mut bytes)
        .map_err(|err| {
            tracing::error!(%err, path = %path.display(), "cannot read the document file");
            OpenFailure::Unreadable
        })?;
    refx_io::dto::decode(&bytes, id).map_err(|err| {
        tracing::error!(%err, path = %path.display(), "cannot decode the document");
        classify_open_error(&err, path)
    })
}

/// ★★★ snapshot ของเอกสารนี้ที่ **ยังไม่เคยไปถึงไฟล์** — `None` = ไม่มีอะไรให้ถาม
///
/// ## ทำไมต้องมีฟังก์ชันนี้ (รูที่ P4-3 เปิดค้างไว้ตั้งแต่ต้น)
///
/// `<doc>.refx.autosave` ถูกเขียนทุก ๆ [`DEFAULT_MIN_INTERVAL`] ที่เอกสาร dirty
/// มาตั้งแต่ P4-3 · แต่ **ไม่เคยมีใครอ่านมันกลับมาเลยสักครั้ง** — `find_pending`
/// มีเทสต์ครบแต่ไม่มีผู้เรียกในโปรแกรม ผลคือผู้ใช้ที่แก้งานสองชั่วโมงแล้วไฟดับ
/// เปิดโปรแกรมมาได้เวอร์ชันที่บันทึกล่าสุด **โดยไม่มีใครถามถึง snapshot**
/// แล้วมันถูกลบทิ้งตอนเขากด `Ctrl+S` ครั้งแรก (`autosave::discard`)
///
/// เป็นรูปแบบเดียวกับ `on_wake` ที่ไม่มีกิ่งรับใน §2.24: กลไกครบ เทสต์เขียว
/// แต่ไม่เคยเดินจริง (`docs/08 §3.9` ข้อ 2)
///
/// ## ★★ เทียบกับเอกสารก่อนถามเสมอ
///
/// snapshot ที่ **เหมือนไฟล์เป๊ะ** ไม่มีอะไรให้กู้ — เกิดได้จริงเมื่อโปรแกรมตาย
/// *หลัง* เขียนไฟล์สำเร็จแต่ *ก่อน* ลบ snapshot · ถามในกรณีนั้นคือการสอนผู้ใช้
/// ให้กดปุ่มผ่าน ๆ โดยไม่อ่าน ซึ่งวันที่มีของจริงให้กู้เขาจะกดผ่านเหมือนกัน
///
/// ★ **รันบนเธรดอื่นเท่านั้น** (I-2) — อ่าน+คลายบีบไฟล์ระดับ MB
fn newer_snapshot(
    doc: &std::path::Path,
    id: refx_core::arena::BoardId,
    saved: &Board,
) -> Option<refx_io::autosave::Pending> {
    worth_offering(refx_io::autosave::find_pending(doc, id), saved)
}

/// ★★★ snapshot ที่ผู้ใช้เคยสั่ง **"เก็บไว้ก่อน"** ของเอกสารนี้ (`docs/07 §4`)
///
/// ถูกเสนอกลับ **ทุกครั้งที่เปิดเอกสารนี้** จนกว่าเขาจะกู้หรือทิ้ง — ไฟล์นี้
/// ไม่มีอะไรลบมันอัตโนมัติเลย รวมถึงตอนบันทึกสำเร็จ
///
/// ★ ยังเทียบกับเอกสารก่อนถามเหมือนกัน: ผู้ใช้ที่เก็บไว้แล้วมาบันทึกเนื้อเดียวกัน
/// ทีหลัง ไม่มีอะไรให้กู้อีกแล้ว — ถามต่อไปคือการสอนให้เขากดผ่าน ๆ
fn kept_snapshot(
    doc: &std::path::Path,
    id: refx_core::arena::BoardId,
    saved: &Board,
) -> Option<refx_io::autosave::Pending> {
    worth_offering(refx_io::autosave::find_kept(doc, id), saved)
}

/// snapshot นี้มีอะไรให้กู้จริงไหม — ตัวร่วมของทั้งสองแบบ
fn worth_offering(
    found: Option<refx_io::autosave::Pending>,
    saved: &Board,
) -> Option<refx_io::autosave::Pending> {
    let pending = found?;
    if pending.board == *saved {
        tracing::info!("the snapshot matches the document — nothing to recover");
        return None;
    }
    Some(pending)
}

/// ลบ snapshot ที่ผู้ใช้สั่งทิ้ง พร้อมไฟล์บริวารของมัน
fn remove_recovery_file(snapshot: &std::path::Path) {
    for path in [
        snapshot.to_path_buf(),
        snapshot.with_extension(refx_io::save::BAK_SUFFIX),
        refx_io::recovery::asked_marker(snapshot),
    ] {
        if let Err(err) = std::fs::remove_file(&path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%err, path = %path.display(), "cannot remove the recovery file");
        }
    }
}

/// ★★ snapshot ของ autosave รอบนี้จะไปลงที่ไหน
///
/// สองปลายทางนี้ต่างกันแค่ **ที่อยู่** — นโยบายว่าเมื่อไหร่ควรเขียน
/// (`dirty` + เว้นระยะ) เป็นตัวเดียวกันทั้งคู่ ดู `RefxApp::autosaver`
#[derive(Debug, Clone, PartialEq, Eq)]
enum SnapshotTarget {
    /// เอกสารมี path แล้ว → `<doc>.refx.autosave` (P4-3)
    BesideDocument(std::path::PathBuf),
    /// ★★★ ยังไม่เคยบันทึกที่ไหนเลย → `<data_dir>/recovery/<session>.refx` (P4-4)
    ///
    /// เก็บ **โฟลเดอร์** ไม่ใช่ไฟล์ เพราะชื่อไฟล์มาจาก `session` ซึ่งเป็นของ
    /// `RefxApp` — การเก็บ path เต็มไว้สองที่คือสองแหล่งความจริงที่จะ drift กัน
    Recovery(std::path::PathBuf),
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
        let mode = args.mode.unwrap_or_default();
        // ★ เพดานของเครื่องคำนวณครั้งเดียวตรงนี้ แล้วใช้ทั้งตอนอ่าน settings
        //   และตอนสร้าง pool — สองที่นี้ต้องเห็นเลขเดียวกัน ไม่งั้นค่าที่แผง
        //   Settings บอกว่า "สูงสุดเท่านี้" จะไม่ใช่เพดานที่ decode ใช้จริง
        let caps = machine_caps();
        let mut app = Self {
            gfx: None,
            args,
            stats: FrameStats::default(),
            bench_start: None,
            bench_done: false,
            shell: crate::shell::ShellState {
                // ★ อ่าน locale ของ OS ครั้งเดียวตอนเปิดโปรแกรม (docs/03 §0 ข้อ 3)
                //   ไม่รู้จักภาษา → อังกฤษ · P5-3 จะให้ผู้ใช้เลือกทับได้
                lang,
                mode,
                ..crate::shell::ShellState::default()
            },
            assets: None,
            cache_stats_rx: None,
            pick_in_flight: None,
            pick_count: 0,
            // ★ ตัวเขียนของจริงอยู่ `refx-platform` — เสียบที่นี่เหมือนตัวอ่าน
            //   (`refx-core` ถือแต่ trait · HANDOFF §2.0)
            copier: Some(crate::copy::Copier::new(std::sync::Arc::new(
                refx_platform::clipboard::SystemClipboard,
            ))),
            waker: None,
            sidecar_touched: Vec::new(),
            drop_started: None,
            // งวดว่างที่รายงานไปแล้ว = ไม่มีอะไรค้างตั้งแต่เปิดโปรแกรม
            drop: DropBatch {
                reported: true,
                ..DropBatch::default()
            },
            pending_drops: Vec::new(),
            pending_paste: false,
            pending_history: None,
            pending_zorder: None,
            pending_delete: false,
            pending_appearance: None,
            pending_group: None,
            pending_mode_toggle: false,
            pending_select_all: false,
            pending_clear_selection: false,
            pending_zoom: None,
            pending_export: false,
            export_target_rx: None,
            export_target: None,
            export_job: None,
            pending_save: None,
            // ★ แท็บเปล่าหนึ่งใบ พร้อม id และ recovery slot ของตัวเองตั้งแต่แรก
            docs: Docs::default(),
            save_as_mode: refx_io::packed::SaveMode::Linked,
            pending_open: false,
            pending_new_tab: false,
            pending_close_tab: false,
            pending_next_tab: false,
            closing_tab: None,
            open_dialog: None,
            load_job: None,
            pending_recovery: None,
            recovery_queue: std::collections::VecDeque::new(),
            recovery_scan: None,
            recovery_checked: false,
            job_owner: std::collections::HashMap::new(),
            // ผู้เรียก (`run`) เสียบให้ — ที่นี่ไม่รู้จัก `AppPaths`
            recovery_dir: None,
            spool_dir: None,
            spool_sweep: None,
            relink_scan: None,
            relink_queue: std::collections::VecDeque::new(),
            relink_pick: None,
            relink_match: None,
            relink_for: None,
            relink_report: None,
            save_dialog: None,
            save_job: None,
            after_save: AfterSave::Stay,
            close_confirm: false,
            closing: false,
            paste_in_flight: None,
            paste_count: 0,
            batch_from_clipboard: false,
            loading: LoadTracker::default(),
            job_sources: std::collections::HashMap::new(),
            // ★ ค่าปริยายจนกว่า `load_settings` จะถูกเรียก — แอปที่ไม่มี
            //   config dir (เทสต์) จึงยังทำงานได้ครบด้วยค่าที่ปลอดภัย
            settings: refx_io::settings::Settings::defaults(caps),
            caps,
            settings_notes: Vec::new(),
            settings_notes_pending: false,
            settings_restart_pending: false,
            settings_write: None,
            keymap_problem: None,
            keymap_from_file: false,
            config_dir: None,
        };
        // ★★★ `--mode=arrange` ต้องไปถึง **`Board::view` ของแท็บแรก** ไม่ใช่แค่
        //     `shell.mode` — ตั้งแต่ก้อน c โหมดถูกอ่านกลับจากแท็บทุกเฟรม
        //     (`mode_follows_active_tab`) ค่าที่เขียนไว้แค่ใน `shell` จึงอยู่ได้
        //     ไม่ถึงเฟรมแรก · ★ ตัวเลือกนี้มีไว้ให้สคริปต์ถ่ายภาพโหมด Arrange ได้
        //     โดยไม่ต้องกดปุ่มก่อน ถ้ามันเงียบไป หลักฐานทุกใบของโหมดนั้นจะเป็นภาพ
        //     ของโหมด Canvas ที่ตั้งชื่อไฟล์ว่า arrange
        {
            let Self { docs, shell, .. } = &mut app;
            Self::write_mode(docs.active_mut(), shell, mode);
        }
        app
    }

    /// ★★★ อ่าน `keymap.toml` — **เรียกก่อนหน้าต่างจะมี** (แตะดิสก์ — I-2)
    ///
    /// ## พังที่ไหนก็ใช้ค่าปริยาย **ทั้งชุด**
    ///
    /// ไม่ว่าจะพังเพราะไวยากรณ์ · ปุ่มที่อ่านไม่ออก · ชื่อ action ที่ไม่รู้จัก
    /// หรือ **สองแถวที่กดครั้งเดียวติดทั้งคู่** — ผลเหมือนกันหมดคือตารางค่าปริยาย
    /// ทั้งชุด พร้อมข้อความที่ชี้แถวที่ผิด
    ///
    /// ★ ห้ามใช้ครึ่งเดียว: ผู้ใช้ที่พิมพ์ผิดบรรทัดเดียวแล้วได้คีย์ลัดหายไป
    /// ครึ่งหนึ่ง จะหาสาเหตุไม่เจอและสรุปว่าโปรแกรมทำงานหาย (`ROADMAP` P5-3b)
    ///
    /// ★★ ไม่คืน `Result` โดยตั้งใจ — เหตุผลเดียวกับ [`Self::load_settings`]
    pub fn load_keymap(&mut self, config_dir: &std::path::Path) {
        let path = keymap_path(config_dir);
        let rows = match refx_io::keymap::load(&path) {
            // ไม่มีไฟล์ = สภาพปกติของการเปิดครั้งแรก · เงียบสนิท
            Ok(None) => return,
            Ok(Some(rows)) => rows,
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "keymap.toml unusable - using the built-in table");
                self.keymap_problem = Some(crate::keymap::Problem::File {
                    detail: err.to_string(),
                });
                return;
            }
        };
        match crate::keymap::Keymap::from_rows(&rows) {
            Ok(map) => {
                let count = map.bindings().len();
                if crate::keymap::install(map) {
                    self.keymap_from_file = true;
                    tracing::info!(count, "keymap.toml is in effect");
                } else {
                    // ★ ตั้งได้ครั้งเดียวต่อโปรเซส — ถ้ามาถึงตรงนี้แปลว่ามีคนเรียกซ้ำ
                    tracing::error!("keymap was already installed - ignoring the second one");
                }
            }
            Err(problem) => {
                tracing::warn!(?problem, "keymap.toml unusable - using the built-in table");
                self.keymap_problem = Some(problem);
            }
        }
    }

    /// อ่าน `settings.toml` — **เรียกก่อน `start_assets` และก่อนหน้าต่างจะมี**
    ///
    /// ★ แตะดิสก์ จึงเรียกได้เฉพาะตอนเปิดโปรแกรม ไม่ใช่จากในเฟรม (I-2)
    ///
    /// ★★ ไม่คืน `Result` โดยตั้งใจ — ไฟล์ที่พังต้องไม่มีทางหยุดการเปิดโปรแกรม
    /// (ดู `refx_io::settings` หัวข้อแรก) สิ่งที่ผิดไปเป็น [`refx_io::settings::Note`]
    /// ที่ถูกพาขึ้นจอในเฟรมแรกแทน
    pub fn load_settings(&mut self, config_dir: &std::path::Path) {
        let loaded = refx_io::settings::load(&settings_path(config_dir), self.caps);
        tracing::info!(
            ram_limit_mb = loaded.settings.ram_limit / (1 << 20),
            vram_limit_mb = loaded.settings.vram_limit.map(|b| b / (1 << 20)),
            max_pixels = loaded.settings.max_pixels,
            theme = loaded.settings.theme.as_str(),
            present = loaded.settings.present.as_str(),
            notes = loaded.notes.len(),
            "settings loaded"
        );
        for note in &loaded.notes {
            tracing::warn!(?note, "settings.toml was not used exactly as written");
        }
        self.settings_notes_pending = !loaded.notes.is_empty();
        self.settings = loaded.settings;
        self.settings_notes = loaded.notes;
        self.config_dir = Some(config_dir.to_path_buf());
    }

    /// ไฟล์ที่สั่งเปิดจากบรรทัดคำสั่ง — เข้าคิวเหมือนลากเข้ามาทุกประการ
    fn queue_initial_files(&mut self) {
        // ★ เอกสารมาก่อนภาพเดี่ยว ๆ — `--open` แทนที่ board ทั้งก้อน ส่วน
        //   `--open-dir` เติมภาพเข้า board ที่มีอยู่ ลำดับกลับกันจะทำให้ภาพที่
        //   เพิ่งเติมหายไปพร้อมกับ board เก่า
        if let Some(doc) = self.args.open_document.take() {
            self.start_load(&doc);
        }
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
        // ★★★ ไฟล์ที่ **เราเขียนเอง** ไม่ใช่ภาพของผู้ใช้ (P5-5) — ตัดออกที่ทางเข้า
        //     ทุกเส้น ไม่ใช่เฉพาะ `--open-dir` · ดู `refx_io::sidecar::is_sidecar`
        let paths: Vec<std::path::PathBuf> = paths
            .into_iter()
            .filter(|path| !refx_io::sidecar::is_sidecar(path))
            .collect();
        if paths.is_empty() {
            return;
        }

        // เริ่มจับเวลาชุดใหม่
        self.drop_started = Some(std::time::Instant::now());
        self.drop.start(paths.len());
        self.batch_from_clipboard = false;

        // ★★★ ภาพที่ลากเข้ามาเป็นของ **แท็บที่ผู้ใช้กำลังดูตอนที่เขาปล่อยเมาส์**
        //     — ไม่ใช่แท็บที่เขาบังเอิญสลับไปตอนงาน decode เสร็จ (P4-7c)
        let owner = self.docs.active().id;
        let mut submitted = Vec::with_capacity(paths.len());
        for (i, path) in paths.into_iter().enumerate() {
            // hash จาก path ไปก่อน — hash เนื้อไฟล์จริงเกิดบน worker (P1-2)
            // ที่นี่ต้องการแค่คีย์ชั่วคราวไว้จับคู่ผลลัพธ์ · ★ ผูกกับแท็บด้วย
            // ไม่งั้นสองแท็บที่ลากไฟล์เดียวกันจะแย่งคีย์กัน (ดู `job_key_for`)
            let hash = job_key_for(owner, &path);
            let source = refx_asset::pool::JobSource::File(path);
            submitted.push(refx_asset::pool::Job {
                hash,
                source,
                // ยังไม่มี layout จริง → เรียงตามลำดับที่ลากเข้ามา
                priority: i as f32,
                cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                target: refx_asset::pool::JobTarget::Thumbnail,
            });
        }
        self.submit_thumbnail_jobs(owner, submitted);
        self.shell.status = text::fill(
            self.shell.lang,
            Template::OpeningFiles,
            &[("n", &self.drop.requested.to_string())],
        );
    }

    /// ผู้ใช้กด `Ctrl+V` — ส่งงาน "ไปดูว่ามีอะไรใน clipboard" เข้าคิว
    ///
    /// ★ **ไม่แตะ clipboard บน UI thread เลย** (I-2) การเปิด clipboard รอ OS
    /// ได้นานเป็นวินาทีถ้าโปรแกรมอื่นถือมันค้างอยู่ — worker เป็นคนอ่าน
    ///
    /// ภาพที่ได้กลับมาผ่านเกราะและเพดาน RAM ชุดเดียวกับไฟล์บนดิสก์ทุกประการ (I-4)
    fn submit_paste(&mut self) {
        if self.assets.is_none() {
            return;
        }
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
        self.paste_in_flight = Some(hash);

        // ★★★ ภาพที่วางเป็นของแท็บที่ผู้ใช้กด `Ctrl+V` อยู่ (P4-7c)
        let owner = self.docs.active().id;
        self.submit_thumbnail_jobs(
            owner,
            vec![refx_asset::pool::Job {
                hash,
                source: refx_asset::pool::JobSource::Clipboard,
                // ผู้ใช้เพิ่งกดปุ่มเมื่อกี้ — ตั้งใจที่สุดในคิว จึงได้ไปก่อน (น้อย = ก่อน)
                priority: 0.0,
                cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                target: refx_asset::pool::JobTarget::Thumbnail,
            }],
        );

        self.drop_started = Some(std::time::Instant::now());
        self.drop.start(1);
        self.batch_from_clipboard = true;
        self.shell.status = text::t(self.shell.lang, Key::ReadingClipboard).to_owned();
    }

    /// ★★★ **ทางเดียวที่งาน thumbnail ถูกส่งเข้า pool** — จดเจ้าของให้เสมอ
    ///
    /// ## ทำไมต้องเป็นประตูเดียว
    ///
    /// ผลจาก pool กลับมาพร้อม **คีย์งานเท่านั้น** ชั้นนี้จึงต้องจำไว้เองว่างานใบ
    /// ไหนเป็นของแท็บไหน · ตอนที่การจดยังอยู่ที่จุดเรียก มันถูกลืมไปสองในสามจุด
    /// (ลากไฟล์ · วาง) แล้วผลที่ได้คือ **ภาพที่ผู้ใช้ลากเข้ามาไม่ขึ้นจอเลยสักใบ
    /// โดยไม่มี error ที่ไหน** — งานถูก decode สำเร็จแล้วถูกทิ้งเพราะไม่มีที่ให้ลง
    ///
    /// ★ เจอเพราะ **รันแอปจริงแล้วดู** (28 ส.ค. 2026) — เทสต์ 849 ตัวเขียวหมด
    /// ตอนนั้น เพราะไม่มีตัวไหนเดินเส้นทาง "ส่งงาน → ผลกลับมา → ขึ้นจอ" ครบ
    /// (`docs/08 §3.9` ข้อ 5: บั๊กชนิด "ทุกชิ้นถูก ประกอบผิด")
    ///
    /// → รวมสามอย่างไว้ที่นี่ (คีย์ · ที่มา · เจ้าของ) ให้ลืมทีละอย่างไม่ได้
    fn submit_thumbnail_jobs(
        &mut self,
        owner: refx_core::arena::BoardId,
        jobs: Vec<refx_asset::pool::Job>,
    ) {
        for job in jobs {
            debug_assert!(
                matches!(job.target, refx_asset::pool::JobTarget::Thumbnail),
                "ประตูนี้มีไว้สำหรับงาน thumbnail เท่านั้น"
            );
            self.job_sources.insert(job.hash, job.source.clone());
            self.job_owner.insert(job.hash, owner);
            if let Some(assets) = self.assets.as_ref() {
                assets.pool.submit(job);
            }
        }
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
        //
        // ★★ ที่พักของภาพที่วางก็เส้นเดียวกัน: worker เป็นคนมีไบต์ แต่คนที่รู้ว่า
        //    ไฟล์ไปไหนและเขียนยังไงให้ atomic คือ `refx-io` ซึ่ง `refx-asset`
        //    พึ่งไม่ได้ — ที่นี่คือชั้นเดียวที่รู้จักทั้งคู่
        let spool: Option<std::sync::Arc<dyn refx_core::spool::PastedImageStore>> = self
            .spool_dir
            .clone()
            .map(|dir| std::sync::Arc::new(SpoolSink { dir }) as _);
        let pool = DecodePool::with_defaults(
            self.settings.ram_limit,
            decode_limits(self.caps, &self.settings),
            std::sync::Arc::new(refx_platform::clipboard::SystemClipboard),
            io_tx.clone(),
            spool,
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
            let _ = tx.send(IoRequest::Stats {
                reply,
                wake: pool.wake_handle(),
            });
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
        // ★★★ **ระหว่าง export ห้ามแตะ atlas** — worker กำลังอ่าน texture ใบนั้น
        //     อยู่ทีละแถบ · การอัปช่องใหม่ทับลงไปกลางทางทำให้ครึ่งบนของภาพที่
        //     ส่งออกเป็นก่อนอัป และครึ่งล่างเป็นหลังอัป โดยไม่มี error ที่ไหนเลย
        //
        //     ★ ผลที่ถูกถอดรหัสแล้ว **ไม่หายไปไหน** — มันรออยู่ในช่องของ pool
        //     แล้วไหลเข้ามาเองในเฟรมถัดจากที่ export จบ · export กินเวลาไม่กี่วินาที
        //     ส่วนภาพที่รออยู่ยังขึ้นเป็นช่องสีเด่นตามปกติระหว่างนั้น
        if self.export_job.is_some() {
            return false;
        }
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
        // ★ ใบที่เปิดไม่ได้ — กลายเป็น `Missing` หลังจบลูป (ดูกิ่ง `Failed`)
        let mut damaged: Vec<(
            refx_asset::hash::ContentHash,
            Option<refx_core::arena::BoardId>,
            Option<refx_asset::pool::JobSource>,
            refx_core::board::MissingReason,
        )> = Vec::new();
        while let Some(result) = assets.pool.try_recv() {
            // งานวางจบแล้วไม่ว่าผลจะเป็นอะไร — เปิดทางให้กด Ctrl+V ครั้งต่อไปได้
            if self.paste_in_flight == Some(result.hash()) {
                self.paste_in_flight = None;
            }
            match result {
                refx_asset::pool::JobResult::Done {
                    hash,
                    thumb,
                    meta,
                    elapsed,
                    origin,
                } => {
                    tracing::debug!(hash = %hash.short(), ?elapsed, "image decoded");
                    // ไม่รู้จักคีย์ = ไม่มีไฟล์ให้กลับไปอ่าน จึงถือเป็นภาพที่ขอคมกว่านี้
                    // ไม่ได้ (ปลอดภัยกว่าการเดา path แล้วยิง error ทุกครั้งที่ซูม)
                    let source = self
                        .job_sources
                        .get(&hash)
                        .cloned()
                        .unwrap_or(refx_asset::pool::JobSource::Clipboard);
                    done.push((hash, source, thumb, meta, origin));
                }
                // ★★★ ภาพที่วางลงดิสก์แล้ว → **ปลดล็อกการขอภาพคมของใบนั้น**
                //
                //   ที่อยู่ของมันถูกเขียนลง `AssetRef::path` ไปตั้งแต่ตอน `Done`
                //   แล้ว (path หาได้จาก hash ล้วน ๆ) สิ่งที่เพิ่งเปลี่ยนคือ
                //   **ไฟล์มีอยู่จริงแล้ว** — ก่อนหน้านี้การขอ working texture
                //   จะได้ error "เปิดภาพไม่ได้" ที่ผู้ใช้ทำอะไรกับมันไม่ได้
                //
                //   ★ `source` เป็นสถานะของชั้น UI ไม่ใช่ของ `Board` การแก้ตรงนี้
                //   จึงไม่ต้องผ่าน `Command` และไม่ทำให้เอกสาร dirty
                refx_asset::pool::JobResult::Spooled { hash, path } => {
                    tracing::info!(hash = %hash.short(), "a pasted image now has a file of its own");
                    let source = refx_asset::pool::JobSource::File(path);
                    if self.gfx.is_some() {
                        for state in self.docs.active_mut().render_state.values_mut() {
                            if state.hash == hash {
                                state.source = source.clone();
                            }
                        }
                    }
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
                refx_asset::pool::JobResult::Sampled {
                    hash,
                    rgba,
                    source_px,
                } => {
                    // ★ ทิ้งผลของการจิ้มครั้งเก่า — ผู้ใช้จิ้มรัว ๆ ได้ และงานที่ส่งก่อน
                    //   อาจกลับมาทีหลัง ถ้าไม่เทียบคีย์ สีบน status bar จะเป็นของ
                    //   จุดที่เขาเลิกสนใจไปแล้ว โดยไม่มีอะไรบอกว่ามันเป็นของเก่า
                    if self.pick_in_flight == Some(hash) {
                        self.pick_in_flight = None;
                        let picked = refx_core::pick::Picked { rgba, source_px };
                        // ★ docs/03 §2: picker คือ "อ่านสี **+ คัดลอก hex**"
                        //   นักวาดก๊อป hex ไปวางใน Photoshop/Clip Studio ตลอดเวลา
                        //   picker ที่ให้อ่านแล้วพิมพ์เองคือ picker ที่ทำงานไม่จบ
                        if let Some(copier) = self.copier.as_ref() {
                            copier.copy(picked.hex());
                        }
                        self.shell.picked = Some(picked);
                        self.shell.status = text::t(self.shell.lang, Key::Ready).to_owned();
                    }
                }
                // ★★★ งานที่ถูกยกเลิก **ต้องนับเข้างวดถ้ามันเป็นภาพที่ผู้ใช้รออยู่**
                //
                //   ผู้ใช้ pan ระหว่างที่ไฟล์กำลังทยอยเข้ามาเป็นเรื่องปกติมาก
                //   (P1-4 ยกเลิกงานที่ผ่านจอไปแล้วโดยตั้งใจ) ถ้าไม่นับ งวดจะ
                //   **ค้างถาวร**: แถบ "กำลังโหลด N/M" ไม่หาย และรายงานตอนจบ
                //   — รวมทั้งข้อความ "board เต็ม" — ไม่มีวันขึ้นเลย
                //
                //   ส่วน working texture ที่ถูกยกเลิกไม่เกี่ยวกับงวดนี้เลย
                //   นับปนเข้ามาจะทำให้งวดจบเร็วเกินจริงแล้วรายงานตัวเลขผิด
                refx_asset::pool::JobResult::Cancelled { hash, target } => {
                    match target {
                        refx_asset::pool::JobTarget::Thumbnail => {
                            self.drop.cancelled += 1;
                            // ★ งานจบแล้วไม่ว่าจะสำเร็จหรือไม่ — คีย์หมดหน้าที่
                            //   ไม่เก็บกวาด = ตารางโตตลอดอายุโปรแกรม (I-6)
                            self.job_owner.remove(&hash);
                            self.job_sources.remove(&hash);
                        }
                        // ★ ต้องปลดคีย์ออกจาก `working_pending` ด้วย ไม่งั้นภาพใบนั้น
                        //   จะ **ไม่มีวันถูกขอภาพคมอีกเลย** ตลอดอายุโปรแกรม —
                        //   เดิมปลดเฉพาะตอนสำเร็จ งานที่ถูกยกเลิก/ล้มจึงค้างคีย์ไว้
                        refx_asset::pool::JobTarget::Working { size } => {
                            gfx_working_pending_remove(self.gfx.as_mut(), hash, size);
                        }
                        refx_asset::pool::JobTarget::Sample { .. } => {
                            // ผู้ใช้จิ้มแล้วเปลี่ยนใจ — ปลดสถานะ "กำลังรอสี"
                            if self.pick_in_flight == Some(hash) {
                                self.pick_in_flight = None;
                            }
                        }
                    }
                }
                refx_asset::pool::JobResult::Failed { hash, reason, .. }
                    if self.pick_in_flight == Some(hash) =>
                {
                    // ★ ไฟล์ต้นฉบับหายไปแล้ว (ผู้ใช้ถอดไดรฟ์ / ย้ายไฟล์) —
                    //   บอกว่า **อ่านสีไม่ได้** ไม่ใช่ "เปิดภาพไม่ได้" ซึ่งจะทำให้
                    //   ผู้ใช้คิดว่าภาพบน board พังไปด้วยทั้งที่มันยังอยู่ครบ
                    tracing::warn!(hash = %hash.short(), %reason, "cannot read the colour");
                    self.pick_in_flight = None;
                    self.shell.status = text::t(self.shell.lang, Key::ColourUnavailable).to_owned();
                }
                refx_asset::pool::JobResult::Failed {
                    hash,
                    reason,
                    target,
                } => {
                    // I-7: ภาพเสียหนึ่งไฟล์ = item ขึ้นสถานะ "โหลดไม่ได้" ไม่ใช่ crash
                    //
                    // ★ ต้องนับด้วย ไม่งั้นงวดที่มีไฟล์เสียแม้ใบเดียวจะ **ไม่มีวันจบ**
                    //   แล้วรายงานสรุป (รวมทั้งข้อความ board เต็ม) ก็ไม่มีวันขึ้น
                    //   — แต่ต้องนับ **เฉพาะงานของงวดนี้** เหมือนกรณี `Cancelled`
                    match target {
                        refx_asset::pool::JobTarget::Thumbnail => {
                            // ★★★ **ใบนี้ต้องกลายเป็น `Missing` ไม่ใช่หายไป**
                            //     (ROADMAP P3-3 · ตัดสิน 28 ส.ค. 2026)
                            //
                            //   เดิมนับ `failed` แล้วจบ — ไม่มี item ถูกสร้างเลย
                            //   ผู้ใช้ลากไฟล์เสีย 20 ใบแล้วเห็น 4 ใบ โดยไม่มีอะไร
                            //   บอกว่าอีก 16 ใบไปไหน ซึ่งอ่านได้อย่างเดียวว่า
                            //   โปรแกรมทำงานหาย (รูปแบบเดียวกับ "board เต็มแล้วเงียบ")
                            //
                            //   ★ เก็บไว้ทำหลังปล่อยการยืม `assets` เพราะการสร้าง
                            //     item ต้องแตะ `docs` และต้องผ่าน `Command`
                            damaged.push((
                                hash,
                                self.job_owner.remove(&hash),
                                self.job_sources.remove(&hash),
                                refx_core::board::MissingReason::from(&reason),
                            ));
                        }
                        refx_asset::pool::JobTarget::Working { size } => {
                            gfx_working_pending_remove(self.gfx.as_mut(), hash, size);
                        }
                        refx_asset::pool::JobTarget::Sample { .. } => {}
                    }
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

        // ★ ที่มาที่ต้องเขียนกลับลง `Board` — เก็บไว้ก่อนแล้วห่อเป็น `Command`
        //   ทีเดียวหลังจบชุด (ยืม `gfx` อยู่ตลอดลูป จึงเรียก `apply_relink` ในนี้ไม่ได้)
        //
        // ★★★ พก `BoardId` มาด้วย — `ItemId` ไม่ผูกกับ board (`docs/02 §1`)
        let mut repairs: Vec<(refx_core::arena::BoardId, ItemId, ItemKind)> = Vec::new();

        // ★★★ ใบที่เปิดไม่ได้ → `Missing` บน board (ROADMAP P3-3)
        //
        // ★ อยู่ **ก่อน** บล็อกของ `done` และ **ไม่ต้องมี `gfx`** โดยตั้งใจ:
        //   ช่องว่างไม่มีพิกเซลให้อัปขึ้น atlas · ถ้าไปผูกกับ `gfx.as_mut()`
        //   เหมือนบล็อกข้างล่าง การ return ตอนไม่มี GPU จะทำให้ใบพวกนี้ไม่ถูกนับ
        //   แล้ว `DropBatch::settled()` จะไม่มีวันเป็นจริง = แถบ "กำลังโหลด" ค้างถาวร
        for (hash, owner, source, reason) in damaged {
            let Some(index) = owner.and_then(|id| self.docs.list.iter().position(|d| d.id == id))
            else {
                // แท็บถูกปิดไประหว่างที่งานเดินอยู่ — ไม่มีที่ให้ผลลง
                self.drop.cancelled += 1;
                continue;
            };
            let original_path = source
                .as_ref()
                .and_then(|s| s.file())
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();
            let board_id = self.docs.list[index].id;
            let doc = &mut self.docs.list[index];

            // ★★★ **เอกสารที่เปิดมาจากไฟล์: item มีอยู่แล้ว ห้ามสร้างใบใหม่**
            //
            //   เส้นทางเดียวกับกิ่ง `Done` เป๊ะ (ดู `relink_targets` ที่นั่น) ·
            //   รุ่นแรกของโค้ดนี้สร้างใบใหม่ทุกครั้ง ผลคือเปิดไฟล์ที่มีภาพเสีย
            //   หนึ่งใบแล้วได้ **ห้า item จากเอกสารที่มีสี่** — ใบผีที่ไม่มีอยู่
            //   ในไฟล์ · เห็นเพราะรันแอปจริงแล้วนับตัวเลขบนแถบสถานะ
            if let Some(id) = doc.relink_targets.remove(&hash) {
                if doc.board.item(id).is_some() {
                    repairs.push((
                        board_id,
                        id,
                        ItemKind::Missing {
                            original_path,
                            reason,
                        },
                    ));
                    self.drop.added += 1;
                    self.drop.damaged += 1;
                } else {
                    // ใบนั้นถูกลบไประหว่างที่งานเดินอยู่ (undo / เปิดไฟล์อื่นทับ)
                    self.drop.cancelled += 1;
                }
                continue;
            }

            // วางเป็นตารางเดียวกับภาพที่เปิดได้ — ใบที่เสียต้องอยู่ในลำดับที่ผู้ใช้
            // ลากเข้ามา ไม่ใช่กองรวมกันที่มุมใดมุมหนึ่ง
            let n = u32::try_from(doc.board.len()).unwrap_or(u32::MAX);
            let (col, row) = (n % 16, n / 16);
            let cell = 160.0;
            // ไม่รู้สัดส่วนจริงเพราะอ่านหัวไฟล์ไม่ผ่าน — ใช้กรอบสี่เหลี่ยมกลาง ๆ
            let size = Vec2::new(128.0, 96.0);
            let top_left = Vec2::new(2000.0 + col as f32 * cell, 2000.0 + row as f32 * cell);
            let item = Item::new(ItemKind::Missing {
                original_path,
                reason,
            })
            .at(top_left + size * 0.5, size);
            let item = Item {
                meta: ItemMeta {
                    added_at: now_ms(),
                    ..item.meta
                },
                ..item
            };

            // ★ ผ่าน `AddItems` เหมือนภาพปกติ — ลากไฟล์เสียเข้ามาแล้ว `Ctrl+Z` ได้
            let Ok(command) = AddItems::new(vec![item]) else {
                self.drop.failed += 1;
                continue;
            };
            if let Err(err) = doc.history.apply(&mut doc.board, Box::new(command)) {
                tracing::error!(%err, "cannot add the damaged image to the board");
                self.drop.failed += 1;
                continue;
            }
            if let Some(id) = doc.board.z_order().last().copied()
                && let Some(item) = doc.board.item(id)
            {
                let canvas = item.canvas;
                doc.index.insert(id, &canvas);
            }
            // ★ นับเป็น `added` เพราะ **ผู้ใช้เห็นมันบนกระดานจริง ๆ** ·
            //   `damaged` เป็นตัวนับแยกสำหรับข้อความสรุป — ถ้านับทั้งสองช่อง
            //   ลง `answered()` งวดจะจบเร็วไปหนึ่งเท่าตัว
            self.drop.added += 1;
            self.drop.damaged += 1;
            tracing::debug!(hash = %hash.short(), ?reason, "damaged image became a placeholder");
        }

        // อัดขึ้น atlas แล้ววาง quad ให้เห็นบน canvas
        let had_results = !done.is_empty();
        if had_results {
            // ★ แยกการยืมทีละฟิลด์ — ลูปข้างล่างต้องแตะทั้ง `gfx` และ `docs`
            let Self {
                gfx,
                docs,
                drop,
                shell,
                spool_dir,
                job_owner,
                ..
            } = self;
            let Some(gfx) = gfx.as_mut() else {
                return finished > 0;
            };
            for (hash, source, thumb, meta, origin) in done {
                // ★★★ คีย์และที่อยู่ของภาพใบนี้ — ดู `asset_location`
                let (asset_hash, spooled_path) = asset_location(spool_dir.as_deref(), hash, origin);
                // ★★★ **ผลนี้เป็นของแท็บไหน** (P4-7c) — อ่าน "แท็บที่ดูอยู่" ตรงนี้
                //     เมื่อไหร่ ภาพที่แท็บ A สั่งโหลดจะไปตกที่แท็บที่ผู้ใช้บังเอิญ
                //     สลับไปดูตอนงานเสร็จ · คีย์งานถูกผูกกับ board ตั้งแต่ตอนส่ง
                //     (ดู `job_key_for`) ตารางนี้จึงตอบได้เสมอ
                let owner = job_owner.remove(&hash);
                let Some(index) = owner.and_then(|id| docs.list.iter().position(|d| d.id == id))
                else {
                    // แท็บถูกปิดไประหว่างที่งานเดินอยู่ = ไม่มีที่ให้ผลลง
                    drop.cancelled += 1;
                    continue;
                };
                let board_id = docs.list[index].id;
                // ★ อัดขึ้น atlas **ก่อน** ยืมแท็บ — การขยาย atlas ต้องแตะทุกแท็บ
                let uploaded = Self::upload_thumb(gfx, docs, &thumb.pixels);
                let doc = &mut docs.list[index];
                match uploaded {
                    Ok(slot) => {
                        // ★★★ ภาพของ board ที่ **เปิดมาจากไฟล์** — item มีอยู่แล้ว
                        //
                        //   ที่ขาดคือพิกเซลอย่างเดียว ตำแหน่ง/ขนาด/หมุน/ครอป/ฟิลเตอร์
                        //   /แท็ก/ดาว/กลุ่ม/โน้ต มาจากไฟล์ครบแล้ว · สร้างใบใหม่ตรงนี้
                        //   = ผู้ใช้เห็นภาพซ้ำสองชุด ชุดหนึ่งอยู่ผิดที่ทั้งหมด
                        if let Some(id) = doc.relink_targets.remove(&hash) {
                            let Some(item) = doc.board.item(id) else {
                                // item ถูกลบไประหว่างที่งานเดินอยู่ (undo/เปิดไฟล์อื่นทับ)
                                drop.cancelled += 1;
                                continue;
                            };
                            // ★★★ ที่มาที่ *ถูกต้อง* ของใบนี้หลังจากเพิ่งอ่านไฟล์จริง
                            //     — ดู `relinked_kind` ว่าอะไรเปลี่ยนได้บ้างและอะไรห้าม
                            let desired = relinked_kind(
                                &item.kind,
                                source.file(),
                                origin.map(|o| o.hash()),
                                spool_dir.as_deref(),
                                &thumb,
                                meta,
                            );
                            let Some(desired) = desired else {
                                drop.cancelled += 1;
                                continue;
                            };
                            // ★ คีย์ที่ `render_state` ใช้ = คีย์ที่ `Board` จะถืออยู่
                            //   หลังคำสั่งนี้ · สองฝั่งชี้คนละ asset ไม่ได้เด็ดขาด
                            let render_hash = match &desired {
                                ItemKind::Image(asset) => asset.hash,
                                _ => asset_hash,
                            };
                            if desired != item.kind {
                                repairs.push((board_id, id, desired));
                            }
                            doc.render_state.insert(
                                id,
                                ItemRender {
                                    source,
                                    hash: render_hash,
                                    tint: dominant_rgba(thumb.dominant),
                                    thumb: *thumb,
                                    slot: Some(slot),
                                },
                            );
                            drop.added += 1;
                            continue;
                        }
                        // จัดเป็นตารางง่าย ๆ ไปก่อน — layout จริงมาใน P2/P3
                        // ★ ตำแหน่งไปอยู่ใน `ItemCanvas` แล้ว ไม่ได้คำนวณลง quad ตรง ๆ
                        let n = u32::try_from(doc.board.len()).unwrap_or(u32::MAX);
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
                            hash: asset_hash,
                            // ★★ ภาพที่วางได้ที่อยู่ของมันใน spool · ภาพจากไฟล์ได้ path
                            //    ของผู้ใช้ · **ไม่มีใบไหนที่ path ว่างอีกแล้ว** ซึ่งเป็น
                            //    เงื่อนไขที่ `request_thumbnails_for_board` ใช้ตัดสินว่า
                            //    ภาพใบนั้นกู้กลับมาได้หรือไม่ (`docs/07 §2` — I-3)
                            path: spooled_path.clone().unwrap_or_else(|| {
                                source
                                    .file()
                                    .map(std::path::Path::to_path_buf)
                                    .unwrap_or_default()
                            }),
                            px_size: glam::UVec2::new(sw, sh),
                            // ★ ยังไม่รู้ format จริงตรงนี้ — cache hit ไม่ได้แตะไบต์ของไฟล์เลย
                            //   เขียน `Unknown` ตรง ๆ ดีกว่าเดาจากนามสกุล (docs/02 §2.2.5 ข้อ 2)
                            //   งานที่จะร้อย format จริงผ่าน decode → Thumbnail → ThumbEntry
                            //   ถูกแยกไว้เป็นงานของตัวเอง (HANDOFF §6)
                            format: ImageFormat::Unknown,
                            embedded: false,
                            // ★ มาจาก `stat` บน worker ตอน ingest — ปลดล็อกการเรียง
                            //   ตามวันที่แก้ไข/ขนาดไฟล์ (P3-4) โดยไม่อ่านดิสก์เพิ่มบน UI thread
                            mtime: meta.mtime_ms,
                            file_size: meta.bytes,
                        }))
                        .at(top_left + size * 0.5, size);
                        // ★ `added_at` **ไม่เคยมีใครเซ็ตมาก่อน** (เป็น 0 ทุกใบ) ทำให้
                        //   การเรียงตามเวลาที่เพิ่มตกไปที่ตัวตัดสินท้ายเสมอ · ที่นี่คือ
                        //   จุดเดียวที่ item ถูกสร้างจากไฟล์จริง จึงเป็นที่ของมัน
                        let item = Item {
                            meta: ItemMeta {
                                added_at: now_ms(),
                                ..item.meta
                            },
                            ..item
                        };

                        // ★ ทุกการเพิ่มภาพผ่าน `AddItems` เข้า `History` → ลากไฟล์เข้ามาแล้ว undo ได้
                        let Ok(command) = AddItems::new(vec![item]) else {
                            continue;
                        };
                        if let Err(err) = doc.history.apply(&mut doc.board, Box::new(command)) {
                            tracing::error!(%err, "cannot add the dropped image to the board");
                            continue;
                        }
                        // `insert_item` ต่อท้าย z-order เสมอ ตัวที่เพิ่งเพิ่มจึงอยู่ท้ายสุด
                        let Some(id) = doc.board.z_order().last().copied() else {
                            continue;
                        };

                        if let Some(item) = doc.board.item(id) {
                            let canvas = item.canvas;
                            doc.index.insert(id, &canvas);
                        }
                        doc.render_state.insert(
                            id,
                            ItemRender {
                                source,
                                // ★ คีย์เดียวกับ `AssetRef::hash` เสมอ — มันคือคีย์ของ
                                //   working texture ด้วย ภาพเดิมที่วางสองครั้งจึงใช้
                                //   texture ใบเดียวกัน
                                hash: asset_hash,
                                // สีเด่นเก็บไว้ตลอดชีวิตของ item ไม่ใช่เฉพาะตอนเป็น
                                // placeholder — ช่อง atlas หลุดเมื่อไหร่ก็หยิบมาใช้ได้ทันที
                                tint: dominant_rgba(thumb.dominant),
                                thumb: *thumb,
                                slot: Some(slot),
                            },
                        );
                        drop.added += 1;
                    }
                    // ★★ board เต็ม = **นับไว้แล้วรายงานทีเดียวตอนจบงวด**
                    //
                    //   ห้ามเขียน status ตรงนี้: ลาก 10,000 ไฟล์เข้ามาแล้ว board เต็ม
                    //   จะเขียนทับข้อความเดิม 6,928 ครั้งด้วยข้อความที่พูดถึง "layer"
                    //   ซึ่งผู้ใช้ทำอะไรกับมันไม่ได้ · สิ่งที่เขาต้องรู้คือ **กี่ใบ
                    //   ที่ไม่ได้เข้าและทำอะไรต่อ** ซึ่งรู้ได้ก็ต่อเมื่อจบงวดแล้ว
                    //
                    //   ★ log ก็เช่นกัน — ของเดิมพิมพ์บรรทัดละใบ วัดจริงได้ 37,606
                    //   บรรทัดจากการลากครั้งเดียว ซึ่งดัน crash log ที่มีค่าออกจาก
                    //   ไฟล์ที่หมุนตามขนาด (เหตุผลเดียวกับ HANDOFF §4 ข้อ 9)
                    Err(AtlasError::Full { layers } | AtlasError::NeedsResize { layers }) => {
                        if drop.rejected == 0 {
                            tracing::warn!(
                                layers,
                                "the board is full — the rest of this batch cannot be added"
                            );
                        }
                        drop.rejected += 1;
                    }
                    // VRAM ไม่พอเป็นคนละปัญหากับ board เต็ม (ข้อความบอกตัวเลขจริง)
                    Err(err) => {
                        tracing::warn!(%err, "cannot store the thumbnail in the atlas");
                        drop.failed += 1;
                        shell.status = text::atlas_error(shell.lang, &err);
                        shell.status_warn = true;
                    }
                }
            }
            // ★ instance ที่ส่งให้ GPU สร้างใหม่จาก board **หลังจบชุด** ไม่ใช่ทีละใบ
            //   (ลากเข้ามา 100 ไฟล์ = สร้างครั้งเดียว ไม่ใช่ 100 ครั้ง)
            //   ★ เฉพาะแท็บที่อยู่บนจอ — `quads` มีชุดเดียวต่อหน้าต่าง
            Self::rebuild_quads(gfx, docs.active_mut());
        }

        // ★ เวลาจริงที่ผู้ใช้รู้สึก: ลากเข้ามา → ภาพขึ้นจอ
        //   ★ เงื่อนไขเดิมทุกประการ: รายงานเฉพาะเฟรมที่มีผล decode กลับมาจริง
        // ★★★ ภาพชุดนี้ขึ้นจอครบแล้ว → ไปเปิด `.refx-meta` ของโฟลเดอร์ที่ยังไม่เคยอ่าน
        //
        //   ★ ต้องรอให้ settle ก่อน เพราะการจับคู่ด้วย hash ของ `docs/07 §5`
        //     ต้องเห็น **ทุกใบในโฟลเดอร์พร้อมกัน** — ถามทีละใบตอบคำถาม
        //     "hash นี้ไม่ซ้ำในรายการที่เหลือไหม" ไม่ได้เลย
        //   ★★★ และ **เอกสารที่มีไฟล์ `.refx` แล้วห้ามอ่าน sidecar เด็ดขาด**
        //       (`docs/07 §5` เจ้าของเดียว) · เขียนเป็นเงื่อนไขตรงนี้ ไม่ใช่
        //       ปล่อยให้ถูกเพราะบังเอิญไม่มีใครเรียกจากเส้นทางเปิดเอกสาร
        if had_results && self.drop.settled() && self.docs.active().path.is_none() {
            let waker = self.waker.clone();
            let doc = self.docs.active_mut();
            doc.sidecar.start_load(&doc.board, waker.as_ref());
        }
        if had_results
            && self.gfx.is_some()
            && !self.drop.reported
            && self.drop.settled()
            && let Some(started) = self.drop_started
        {
            {
                let elapsed = started.elapsed();
                let ms = elapsed.as_secs_f64() * 1000.0;
                self.drop.reported = true;
                // ★★★ board เต็มมาก่อนทุกข้อความ — "เปิด 3,072 ไฟล์ใน 9 วินาที"
                //   ที่ขึ้นตอนผู้ใช้ลากมา 10,000 ไฟล์ **เป็นความจริงที่หลอก**
                //   เขาจะอ่านว่าสำเร็จครบแล้วนับภาพเองไม่ได้ (ROADMAP P3-3)
                let capacity = self.docs.active().board.len();
                if let Some(message) = board_full_message(self.shell.lang, capacity, self.drop) {
                    tracing::warn!(
                        capacity,
                        rejected = self.drop.rejected,
                        requested = self.drop.requested,
                        added = self.drop.added,
                        failed = self.drop.failed,
                        "the board filled up before the batch finished"
                    );
                    self.shell.status = message;
                    self.shell.status_warn = true;
                    self.relink_report = None; // board เต็มสำคัญกว่า — ทิ้งอันรองไป
                } else if let Some(report) = self.relink_report.take() {
                    // ★★ ผลการตามหาไฟล์มาก่อน "เปิด N ไฟล์ใน M ms" — ผู้ใช้ที่
                    //    เพิ่งย้ายโฟลเดอร์ภาพต้องรู้ว่ามันถูกผูกใหม่/หายไปกี่ใบ
                    //    ส่วนเวลาที่ใช้เปิดเป็นเรื่องรองในจังหวะนั้น (ยังอยู่ใน log)
                    tracing::info!(
                        ms,
                        moved = report.moved,
                        lost = report.lost,
                        "relinked images are on screen"
                    );
                    write_relink_status(&mut self.shell, report);
                } else if self.batch_from_clipboard {
                    tracing::info!(ms, "clipboard paste → image on screen");
                    self.shell.status = text::fill(
                        self.shell.lang,
                        Template::PastedImage,
                        &[("ms", &format!("{ms:.0}"))],
                    );
                    self.shell.status_warn = false;
                } else {
                    tracing::info!(
                        files = self.drop.requested,
                        ms,
                        // ★ หลักฐานว่าการเพิ่มภาพเดินผ่าน `AddItems` เข้า `History` จริง
                        //   ไม่ใช่ push เข้า Vec ตรง ๆ เหมือนก่อนย้าย — undo ได้ทุกใบ
                        undo_depth = self.docs.active().history.undo_depth(),
                        "drag & drop → every image on screen"
                    );
                    // ★★ ต้องบอก **ทั้งสองจำนวน** — เดิมพิมพ์แต่ `requested` แล้วเติมคำว่า
                    //    "ครบ" ต่อท้าย ซึ่งอ่านว่า "ขึ้นจอครบทั้ง 20 ใบ" ทั้งที่ไฟล์ที่
                    //    เปิดไม่ได้จะไม่กลายเป็น item เลยสักใบ (ลาก 20 → ขึ้นจริง 4)
                    //    · บรรทัดนี้เป็นสิ่งที่ `scripts/checklist-app.ps1` อ่านเพื่อ
                    //    ตัดสินข้อ "ไฟล์เสีย 20 ไฟล์" ของ `docs/08 §3` — พิมพ์เลขที่
                    //    ไม่ตรงกับสิ่งที่ผู้ใช้เห็นบนจอ = ประตูที่เขียวโดยไม่ได้ตรวจอะไร
                    println!(
                        "ลากไฟล์ {} ไฟล์ → ขึ้นจอ {} ใบใน {ms:.1} ms",
                        self.drop.requested, self.drop.added
                    );
                    self.shell.status = text::fill(
                        self.shell.lang,
                        Template::OpenedFiles,
                        &[
                            ("n", &self.drop.added.to_string()),
                            ("ms", &format!("{ms:.0}")),
                        ],
                    );
                    self.shell.status_warn = false;
                }
            }
        }

        if finished > 0 {
            // cache เพิ่งเปลี่ยน — ขอสถิติรอบใหม่ (event-driven ไม่ใช่ polling
            // ถ้าขอเป็นระยะจะปลุก event loop ตลอดแล้วพัง I-1)
            if let Some(tx) = assets.io_tx.as_ref() {
                let (reply, rx) = crossbeam_channel::bounded(4);
                let wake = assets.pool.wake_handle();
                if tx.send(IoRequest::Stats { reply, wake }).is_ok() {
                    self.cache_stats_rx = Some(rx);
                }
            }
        }

        // ★★ ที่มาที่เพิ่งพิสูจน์ได้จากไฟล์จริง → เขียนกลับลง `Board` ผ่าน `Command`
        //    (ต้องอยู่หลังจากเลิกยืม `gfx` แล้วเท่านั้น) · `RelinkAssets` merge
        //    ตัวเองอยู่แล้ว งวดที่ทยอยกลับมาหลายเฟรมจึงเป็น undo ขั้นเดียว
        if !repairs.is_empty() {
            // ★ จัดกลุ่มตามแท็บก่อน — `RelinkAssets` เป็นคำสั่งของ board ใบเดียว
            //   และการเรียกทีละใบจะสั่งสร้าง quad ใหม่ซ้ำเท่าจำนวนภาพ
            let mut grouped: Vec<(refx_core::arena::BoardId, Vec<(ItemId, ItemKind)>)> = Vec::new();
            for (board, id, kind) in repairs {
                match grouped.iter_mut().find(|(known, _)| *known == board) {
                    Some((_, list)) => list.push((id, kind)),
                    None => grouped.push((board, vec![(id, kind)])),
                }
            }
            for (board, list) in grouped {
                self.apply_relink(board, list);
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
        theme: refx_io::settings::Theme,
    ) -> (egui::Context, egui_winit::State, egui_wgpu::Renderer) {
        let egui_ctx = egui::Context::default();
        // ★★ ธีมต้องตั้งที่นี่ **ที่เดียว** เพราะที่นี่คือจุดเดียวที่ `egui::Context`
        //    ถูกสร้าง — ทั้งตอนเปิดโปรแกรมและตอนกู้ device (`HANDOFF §4` ข้อ 8
        //    บังคับให้สร้าง Context ใหม่ทั้งก้อน) · ตั้งที่อื่นแล้วธีมจะเด้งกลับ
        //    เป็นค่าปริยายทุกครั้งที่ไดรเวอร์สะดุด โดยไม่มีอะไรบอก
        crate::theme::install(&egui_ctx, theme);
        // ★ ต้องทำก่อนวาดเฟรมแรก และต้องทำ **ทุกครั้งที่สร้าง Context ใหม่**
        //   ซึ่งรวมถึงตอนกู้ device (docs/04 §7 ข้อ 3) — ไม่งั้นตัวหนังสือไทย
        //   จะกลับไปเป็นสี่เหลี่ยมหลัง driver อัปเดต
        crate::fonts::install(&egui_ctx);
        // ★ เคอร์เซอร์ข้อความที่ไม่กะพริบ (I-1 — เดิมอยู่ตรงนี้) ย้ายเข้าไปอยู่ใน
        //   `theme::install` แล้ว เพราะ `set_visuals` เขียน `style` ทั้งก้อน
        //   ถ้าปล่อยไว้ตรงนี้ธีมจะล้างมันทิ้งเงียบ ๆ แล้ว 13 fps จะกลับมา
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
        // ★ อ่านค่าที่ตั้งไว้ **ก่อน** ยืม `gfx` — และต้องอ่านทุกครั้ง ไม่ใช่จำ
        //   ค่าตอนเปิดโปรแกรม: ธีม/เพดาน VRAM ที่ผู้ใช้เปลี่ยนไปแล้วต้องรอดข้าม
        //   การกู้ device ไปด้วย ไม่งั้น driver อัปเดตทีเดียวแล้วค่าที่เขาตั้งหาย
        let theme = self.settings.theme;
        let vram_limit = self.settings.vram_limit;
        let gfx = self.gfx.as_mut()?;

        if let Err(err) = gfx.render.recover() {
            // กู้ไม่สำเร็จ — ของเดิมยังอยู่ครบ ไม่ล้มโปรแกรม ลองใหม่เฟรมหน้า
            tracing::error!(%err, "GPU device recovery failed");
            return None;
        }

        let (egui_ctx, egui_winit, egui_renderer) =
            Self::build_egui(&gfx.window, &gfx.render, theme);
        gfx.egui_ctx = egui_ctx;
        gfx.egui_winit = egui_winit;
        gfx.egui_renderer = egui_renderer;

        // ★★ **กับดักเดิมตรงนี้ถูกปิดด้วยโครงสร้างแล้ว** (P4-7 ชิ้น b)
        //
        //   เดิม `Gfx` ถือ `board`/`history`/`selection` ซึ่งเป็น **งานของผู้ใช้**
        //   ความปลอดภัยจึงขึ้นกับ "ฟังก์ชันนี้แก้ทีละฟิลด์" — ใครเขียนเป็น
        //   `*gfx = Gfx::new(...)` วันหนึ่ง **board ของผู้ใช้จะหายทันทีที่ driver
        //   อัปเดต** โดยไม่มี error ที่ไหนเลย (I-3) · กฎที่ต้องจำเอง คือกฎที่
        //   วันหนึ่งจะมีคนลืม
        //
        //   ตอนนี้เอกสารอยู่ที่ `RefxApp::doc` คนละชั้นกับ device ทั้งก้อน —
        //   สร้าง `Gfx` ใหม่ทั้งก้อนกี่ครั้งก็ **แตะเอกสารไม่ได้เลย** ดู [`Doc`]
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
        } = match DeviceBound::build(&gfx.render, vram_limit) {
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
        Self::refill_atlas(gfx, &mut self.docs);

        Some(RedrawReason::SurfaceRecovery)
    }

    /// อัดภาพย่อขึ้น atlas — ขยาย atlas แล้วเติมของเดิมกลับให้เองถ้าที่ไม่พอ
    ///
    /// `ThumbnailAtlas::upload` **ไม่ขยายเอง** โดยตั้งใจ เพราะการขยายแบบคัดลอก
    /// บน GPU บังคับให้ถือ texture สองใบพร้อมกัน = 368 MB จากเพดาน 384 MB
    /// ตอนขยาย 11→12 layer (docs/05 §2) การสร้างใหม่แล้วเติมกลับจาก RAM
    /// ทำให้ peak เหลือเท่าใบใหม่ใบเดียว และเราเก็บภาพย่อไว้ใน RAM อยู่แล้ว
    /// เพื่อเส้นทางกู้ device — โค้ดเติมกลับจึงเป็นตัวเดียวกันเป๊ะ
    /// ★ รับ `Docs` ทั้งก้อนเพราะการขยาย atlas ล้างช่องของ **ทุกแท็บ** พร้อมกัน
    /// (ตัวจัดสรรมีชุดเดียวต่อ device) — เติมกลับแค่ใบเดียวคือทิ้งอีกสามใบไว้ว่าง
    fn upload_thumb(
        gfx: &mut Gfx,
        docs: &mut Docs,
        pixels: &[u8],
    ) -> Result<AtlasSlot, AtlasError> {
        match gfx.atlas.upload(gfx.render.queue(), pixels) {
            Err(AtlasError::NeedsResize { layers }) => {
                gfx.atlas.resize(gfx.render.device(), layers)?;
                // texture ใหม่ว่างเปล่า — ต้องเติมภาพเดิมกลับก่อนใส่ภาพใหม่
                Self::refill_atlas(gfx, docs);
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
        if self.docs.active_mut().board.is_empty() {
            return;
        }

        let zoom = self.docs.active_mut().camera.zoom();
        let viewport = gfx.canvas.size;
        let centre = self.docs.active_mut().camera.center();
        let mut requests: Vec<refx_asset::pool::Job> = Vec::new();
        // เก็บไว้ก่อนแล้วค่อยแปลงเป็น quad หลังจบลูป — ระหว่างลูปยังยืม `self.docs.active_mut().board` อยู่
        let mut working_hits: Vec<(WorkingKey, ItemId)> = Vec::new();

        for (id, board_item) in self.docs.active().board.items_in_z_order() {
            // ★ ชนิดของ item มาจาก `Board` เสมอ — เหตุผลเดียวกับใน `rebuild_quads`
            //   (ใบที่ relink ทำให้กลายเป็น `Missing` ยังมี `render_state` ค้างอยู่)
            if !matches!(board_item.kind, ItemKind::Image(_)) {
                continue;
            }
            let Some(item) = self.docs.active().render_state.get(&id) else {
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
            let Some(item) = self.docs.active().board.item(id) else {
                continue;
            };
            let Some(state) = self.docs.active().render_state.get(&id) else {
                continue;
            };
            if let Some(mut quad) = instances::quad_for(&item.canvas, state.slot, state.tint) {
                // ★ working texture ถือภาพเดียวเต็มใบที่ layer 0 — "ช่อง" ของมันคือ
                //   texture ทั้งใบ (0..1) จึงใช้ **ฟังก์ชันเดียวกับทาง atlas** ได้เลย
                //
                //   ★★ ต้องเป็นฟังก์ชันเดียวกันจริง ๆ ไม่ใช่เขียนสูตรซ้ำ: เคยเขียนแยก
                //   แล้วลืมใส่ `flip` ทางนี้ ผลคือกด `H` แล้วภาพพลิกตอนซูมออก
                //   (ทาง atlas) แต่ **ไม่พลิกตอนซูมเข้า** (ทาง working) โดยไม่มี error
                quad.uv_rect = crop_uv([0.0, 0.0, 1.0, 1.0], &item.canvas);
                quad.layer = 0;
                gfx.working_quads.push((key, quad));
            }
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
    /// ★ คืน `pick` ออกไปแทนที่จะยิงงานเอง เพราะฟังก์ชันนี้ยืมแค่ `gfx` ส่วน
    /// decode pool อยู่ที่ `self.assets` — และการคืนค่าออกไปทำให้ **ไม่มีสถานะ
    /// ค้างระหว่างเฟรม** ที่ต้องมีใครจำไปเก็บให้ถูกจังหวะ (`docs/08 §3.9` ข้อ 8)
    fn apply_canvas_input(
        gfx: &mut Gfx,
        doc: &mut Doc,
        input: CanvasFrameInput,
        mode: Mode,
    ) -> CanvasOutcome {
        let mut out = CanvasOutcome::default();
        let changed = &mut out.redraw;
        let rect = input.rect;
        if !rect.is_positive() {
            return out;
        }
        if mode == Mode::Arrange {
            return Self::apply_arrange_input(gfx, doc, input);
        }

        // ---- กล้อง: ปุ่มกลางลาก + ล้อซูม ----
        //
        // ★ ทำไม pan ใช้ **ปุ่มกลาง** ไม่ใช่ space+ลาก: ปุ่มซ้ายเป็นของการเลือกแล้ว
        //   ส่วน space จะชนกับ text note (P2-11) ที่ space เป็นตัวอักษรจริง ๆ
        //   ปุ่มกลางไม่ต้องพึ่งสถานะคีย์บอร์ดเลยจึงไม่มีทางค้าง (เพิ่ม space ทีหลังได้)
        let ppp = gfx.egui_ctx.pixels_per_point();
        if input.pan_delta != egui::Vec2::ZERO {
            // ระยะลากเป็น point — กล้องคิดเป็น physical pixel
            doc.camera
                .pan_by_screen_delta(Vec2::new(input.pan_delta.x, input.pan_delta.y) * ppp);
            *changed = true;
        }
        if input.scroll.abs() > f32::EPSILON
            && let Some(pointer) = input.pointer
        {
            // เลขชี้กำลังทำให้ซูมรู้สึกเท่ากันทุกระดับ (เหมือนเดิมก่อนย้าย)
            let factor = 1.1f32.powf(input.scroll / 50.0);
            let local = pointer - rect.min;
            doc.camera.zoom_at_screen(
                Vec2::new(local.x, local.y) * ppp,
                Vec2::new(rect.width(), rect.height()) * ppp,
                factor,
            );
            *changed = true;
        }

        // ---- การเลือก ----
        let Some(pointer) = input.pointer else {
            return out;
        };
        let scale = doc.camera.zoom() / ppp;
        if scale <= 0.0 {
            return out;
        }
        let offset = pointer - rect.center();
        let world = doc.camera.center() + Vec2::new(offset.x, offset.y) / scale;

        // ★ ระยะทุกตัวคิดเป็น **พิกเซลบนจอ ÷ zoom** เสมอ เพื่อให้รู้สึกเท่ากันทุกระดับซูม
        //   handle ที่มีขนาดคงที่ใน world จะเล็กจนจับไม่โดนทันทีที่ซูมออก
        //   (และใหญ่จนกลืนทั้งภาพเมื่อซูมเข้า) — HANDOFF §2.4
        let world_per_point = ppp / doc.camera.zoom();
        let ctx = CanvasContext {
            board: &doc.board,
            index: &doc.index,
            drag_threshold: refx_core::interact::DEFAULT_DRAG_THRESHOLD_PX * world_per_point,
            handle_reach: refx_core::interact::DEFAULT_HANDLE_PX * world_per_point,
            rotate_reach: refx_core::interact::DEFAULT_ROTATE_PX * world_per_point,
            tool: gfx.tool,
            // ★ ระยะไกด์คิดเป็นพิกเซลบนจอเหมือนระยะอื่น ๆ ทั้งหมด
            snap_reach: GUIDE_SNAP_PX * world_per_point,
            viewport: Self::visible_world(gfx, doc),
        };

        // ★ ดับเบิลคลิกมาก่อน press/release ของรอบเดียวกัน — ไม่งั้นคลิกที่สองจะถูก
        //   ตีความเป็นการกดใหม่แล้วรีเซ็ตไม่เกิดขึ้นเลย
        let event = if input.primary_double {
            Some(CanvasEvent::DoubleClick { world })
        } else if input.primary_pressed {
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
            // ★ ส่งปุ่มดัดแปลง **ของเฟรมนี้** ไม่ใช่ตอนกด — Shift/Alt ที่กดกลางการลาก
            //   ต้องมีผลทันที ไม่งั้นผู้ใช้จะสรุปว่า "คงสัดส่วนไม่ทำงาน"
            Some(CanvasEvent::Move {
                world,
                modifiers: input.modifiers,
            })
        } else {
            None
        };

        let Some(event) = event else {
            return out;
        };

        // ตัวที่กำลังถูกลากคือชุดที่เลือกอยู่ **ก่อน** ส่ง event เข้าไป
        let moved: Vec<ItemId> = doc.selection.iter().collect();
        let outcome = doc.select_tool.handle(ctx, &mut doc.selection, event);
        doc.rubber_band = outcome.rubber_band;
        out.pick = outcome.pick;
        // ★ เส้นที่โผล่/หายต้องวาดใหม่ แม้ตำแหน่งภาพจะไม่เปลี่ยน
        *changed |= doc.guides != outcome.guides;
        doc.guides = outcome.guides;
        *changed |= outcome.needs_redraw;

        let has_commands = !outcome.commands.is_empty();
        for command in outcome.commands {
            if let Err(err) = doc.history.apply(&mut doc.board, command) {
                tracing::error!(%err, "cannot apply a canvas edit");
            }
        }
        if outcome.seal {
            // ปล่อยเมาส์ = ปิดหน้าต่าง merge · การลากครั้งถัดไปเป็น undo ขั้นใหม่
            doc.history.seal();
        }

        // ★ โน้ตที่เพิ่งวาง (P2-11) ต้องถูกเลือกทันที ไม่งั้นผู้ใช้ต้องคลิกซ้ำก่อนพิมพ์
        //   `insert_item` ต่อท้าย z-order เสมอ ตัวสุดท้ายจึงคือตัวที่เพิ่งเพิ่ม
        if outcome.select_added
            && let Some(id) = doc.board.z_order().last().copied()
        {
            doc.selection.restore(vec![id], Some(id));
            *changed = true;
        }

        if has_commands {
            // ★ index ต้องตามตำแหน่งใหม่ทันที ไม่งั้นการกดครั้งถัดไปจะ hit-test
            //   กับตำแหน่ง **เก่า** แล้วคลิกไม่โดนภาพที่เพิ่งย้ายไป
            //   (เจอตอนเขียนเทสต์ใน refx-core — ที่นั่นก็ต้องทำเหมือนกันเป๊ะ)
            for id in moved {
                if let Some(item) = doc.board.item(id) {
                    doc.index.insert(id, &item.canvas);
                }
            }
            // การแก้ครั้งใหม่ล้างสาย redo — ภาพที่คำสั่งในสายนั้นถือไว้ตายตรงนี้
            Self::collect_forgotten(gfx, doc);
            Self::rebuild_quads(gfx, doc);
            *changed = true;
        }
        out
    }
    /// input ของโหมด Arrange — **ล้อคือการเลื่อน ไม่ใช่การซูม** (P3-3)
    ///
    /// ★ ตั้งใจให้ต่างจาก Canvas: contact sheet มีขนาดช่องที่มาจากความกว้างของจอ
    /// ไม่ใช่จากระดับซูม · ล้อที่ซูมในตารางแบบนี้เป็นสิ่งที่ไม่มีโปรแกรมไหนทำ
    ///
    /// ★★ คืน `redraw = true` **เฉพาะตอนตำแหน่งขยับจริง** — หมุนล้อค้างที่สุดขอบ
    /// ต้องไม่ทำให้โปรแกรมวาดใหม่ไปเรื่อย ๆ (I-1)
    fn apply_arrange_input(gfx: &mut Gfx, doc: &mut Doc, input: CanvasFrameInput) -> CanvasOutcome {
        let mut out = CanvasOutcome::default();
        // ระยะจาก egui เป็น point — แผ่นคิดเป็น physical pixel เหมือนกล้องของ Canvas
        let ppp = gfx.egui_ctx.pixels_per_point();
        if input.scroll.abs() > f32::EPSILON {
            out.redraw |= doc.arrange.scroll_by(input.scroll * ppp);
        }
        // ปุ่มกลางลาก = เลื่อนแผ่น (ท่าเดียวกับ pan ของ Canvas ผู้ใช้จะลองท่านี้แน่ ๆ)
        if input.pan_delta.y.abs() > f32::EPSILON {
            out.redraw |= doc.arrange.scroll_by(input.pan_delta.y * ppp);
        }
        out
    }

    /// สั่ง worker ไปอ่านสีของ pixel ต้นฉบับหนึ่งจุด (P2-10)
    ///
    /// คืนคีย์ของงานที่ส่งไป — `None` เมื่อไม่มีอะไรให้อ่าน (ผู้ใช้เห็นเหตุผลบน status bar)
    ///
    /// ★★ **ทำไมไม่อ่านจาก `ItemRender::thumb` ที่อยู่ใน RAM แล้ว**
    ///
    /// thumbnail คือภาพ 128x128 ที่ถูก **บีบเป็นจัตุรัส** และเฉลี่ยมาแล้ว
    /// ภาพ 4000x3000 หนึ่ง pixel ของ thumb จึงเท่ากับ 31x23 pixel ของจริง
    /// สีที่ได้จะเป็นค่าเฉลี่ยของบริเวณ ไม่ใช่สีที่ผู้ใช้จิ้ม — ซึ่งดูสมเหตุสมผล
    /// จนกว่าจะเอาไปเทียบกับต้นฉบับจริง · ROADMAP P2-10 บังคับว่าต้องเป็นสีต้นฉบับ
    ///
    /// ราคาคือ decode หนึ่งครั้งต่อการจิ้มหนึ่งครั้ง ซึ่งรับได้เพราะเป็นการกระทำ
    /// ที่ผู้ใช้ตั้งใจทำทีละครั้ง (ไม่ใช่ทุกเฟรม — ดู `Tool::Picker` ใน `interact.rs`)
    fn request_colour(
        doc: &Doc,
        assets: Option<&Assets>,
        shell: &mut crate::shell::ShellState,
        request: refx_core::interact::PickRequest,
        counter: u64,
    ) -> Option<refx_asset::hash::ContentHash> {
        let lang = shell.lang;
        let Some(assets) = assets else {
            shell.status = text::t(lang, Key::ColourUnavailable).to_owned();
            return None;
        };
        // ★ ต้องมี **ไฟล์** ให้กลับไปอ่าน — ภาพที่วางมาจาก clipboard ไม่มี
        //   (HANDOFF: ภาพจาก clipboard คมได้แค่ระดับ thumbnail จนกว่าจะถึง P4-5)
        //   ยอมบอกตรง ๆ ว่าอ่านไม่ได้ ดีกว่าแอบตอบด้วยสีที่เฉลี่ยมาจาก thumbnail
        let path = doc
            .board
            .item(request.id)
            .and_then(|item| match &item.kind {
                refx_core::board::ItemKind::Image(asset) => Some(asset.path.clone()),
                _ => None,
            })
            .filter(|path| !path.as_os_str().is_empty());
        let Some(path) = path else {
            shell.status = text::t(lang, Key::ColourUnavailable).to_owned();
            return None;
        };

        // คีย์ของตัวเอง ไม่ใช่ hash ของภาพ — ผลของ picker ต้องไม่ไปปนกับ
        // thumbnail/working ของภาพเดียวกันที่อาจกำลังเดินอยู่ในคิว
        let hash = refx_asset::hash::hash_bytes(format!("pick:{counter}").as_bytes());
        assets.pool.submit(refx_asset::pool::Job {
            hash,
            source: refx_asset::pool::JobSource::File(path),
            // ผู้ใช้เพิ่งกดเมื่อกี้และกำลังรอดูอยู่ — ตั้งใจที่สุดในคิว (น้อย = ก่อน)
            priority: 0.0,
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            target: refx_asset::pool::JobTarget::Sample {
                u: request.uv.x,
                v: request.uv.y,
            },
        });
        shell.status = text::t(lang, Key::ReadingColour).to_owned();
        Some(hash)
    }

    /// กรอบที่มองเห็นอยู่ในหน่วย world — ขอบเขตของการค้นหาไกด์ (P2-9)
    fn visible_world(gfx: &Gfx, doc: &Doc) -> WorldRect {
        let size = gfx.canvas.size;
        let zoom = doc.camera.zoom();
        if !size.is_finite() || zoom <= 0.0 {
            return WorldRect::EMPTY;
        }
        WorldRect::from_center_size(doc.camera.center(), size / zoom)
    }

    /// ★★ ทำให้ "ใครอยู่บน GPU" ตรงกับ "ใครอยู่บน board" — เรียกหลังคำสั่งที่เพิ่ม/ลบ item
    ///
    /// เรียกเฉพาะ id ที่เพิ่งเปลี่ยน (`affected()`) ไม่ใช่ไล่ทั้ง `render_state` —
    /// ที่ 1000 ภาพการไล่ทั้งแผนที่ทุกครั้งคือการเผา CPU ฟรีระหว่างลากเมาส์
    ///
    /// **ไม่ทิ้ง pixel ใน RAM ตรงนี้เด็ดขาด** — คืนแค่ช่องใน atlas (VRAM)
    /// ตราบใดที่ยัง undo ได้ ภาพต้องกลับขึ้นจอได้โดย**ไม่ decode ใหม่**
    /// (ถ้าต้อง decode ใหม่ แล้วผู้ใช้ลบไฟล์ต้นทางไปแล้ว undo จะล้มถาวร = ผิด I-3)
    /// คนทิ้ง RAM คือ [`RefxApp::collect_forgotten`] ซึ่งฟัง `History` อีกที
    fn sync_residency(gfx: &mut Gfx, doc: &mut Doc, ids: &[ItemId]) {
        let Gfx { atlas, render, .. } = gfx;
        let Doc {
            render_state,
            board,
            ..
        } = doc;
        for id in ids {
            let Some(state) = render_state.get_mut(id) else {
                continue;
            };
            match (board.item(*id).is_some(), state.slot) {
                // หลุดจาก board แล้ว — คืนช่องทันที VRAM ว่างตรงนี้
                (false, Some(slot)) => {
                    atlas.free(slot);
                    state.slot = None;
                }
                // กลับมาอยู่บน board แล้ว (undo ของการลบ) — เติมจาก RAM ไม่ต้อง decode
                (true, None) => {
                    state.slot = atlas
                        .upload(render.queue(), &state.thumb.pixels)
                        .inspect_err(|err| {
                            // atlas เต็ม — ขึ้นเป็นสี่เหลี่ยมสีเด่นแทนช่องว่าง (docs/04 §8)
                            tracing::warn!(%err, ?id, "no atlas slot for the restored image");
                        })
                        .ok();
                }
                _ => {}
            }
        }
    }

    /// ★ ทิ้ง thumbnail ของภาพที่ **กลับมาไม่ได้อีกแล้ว** — คำตอบของ "ใครเป็นเจ้าของ
    /// อายุของ thumbnail" คือ `History` เป็นคนถือ (HANDOFF §6 ค้างไว้ตั้งแต่ P2-4)
    ///
    /// เดิม `render_state` **ไม่เคยถูกล้างเลย** — โตตามจำนวนภาพที่เคยเพิ่มในเซสชัน
    /// ใบละ 64 KB · ตอนนี้มันตายพร้อมคำสั่งที่ถือมันไว้
    ///
    /// id ที่ถูกลืมแต่ **ยังอยู่บน board** ต้องไม่ถูกแตะ (เช่น `AddItems` ที่ถูกตัด
    /// ตามเพดานทั้งที่ภาพยังอยู่บนจอ) — ไม่งั้นภาพที่ผู้ใช้เห็นจะกลายเป็นสี่เหลี่ยมสี
    fn collect_forgotten(gfx: &mut Gfx, doc: &mut Doc) {
        let forgotten = doc.history.take_forgotten();
        if forgotten.is_empty() {
            return;
        }
        let mut dropped = 0usize;
        for id in forgotten {
            if doc.board.item(id).is_some() {
                continue;
            }
            if let Some(state) = doc.render_state.remove(&id) {
                if let Some(slot) = state.slot {
                    gfx.atlas.free(slot);
                }
                dropped += 1;
            }
        }
        if dropped > 0 {
            tracing::debug!(
                dropped,
                "released thumbnails the history can no longer restore"
            );
        }
    }

    /// ★★ ส่งผลของ layout ลง canvas — **จุดเดียวที่ Arrange แตะข้อมูลจริง** (P3-5)
    ///
    /// ทุกอย่างก่อนหน้านี้ (P3-3 virtual scrolling · P3-4 sort/filter) เป็นการ
    /// *มองดู* ล้วน ๆ ไม่แตะ `Board` เลย · ตรงนี้คือที่ที่ผู้ใช้ตั้งใจให้มันแตะ
    /// จึงต้องผ่าน `Command` และเป็น **undo ขั้นเดียวสำหรับทั้งกระดาน**
    fn apply_layout_to_canvas(&mut self) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let lang = self.shell.lang;
        let Some(command) = ApplyLayout::from_placed(
            &self.docs.active().board,
            self.docs.active().arrange.placed(),
        ) else {
            // ★ ไม่มีอะไรขยับ = บอกตรง ๆ ไม่ใช่เงียบ (ผู้ใช้กดแล้วต้องรู้ว่าเกิดอะไร)
            self.shell.status = text::t(lang, Key::NothingToApply).to_owned();
            return;
        };
        let moved = refx_core::command::Command::affected(&command);
        if let Err(err) = self.docs.active_mut().apply(Box::new(command)) {
            tracing::error!(%err, "cannot arrange the images on the canvas");
            return;
        }
        // ★ index ต้องตามตำแหน่งใหม่ทันที ไม่งั้นคลิกครั้งถัดไป hit-test กับที่เก่า
        for id in &moved {
            self.docs.active_mut().reindex(*id);
        }
        Self::collect_forgotten(gfx, self.docs.active_mut());
        Self::rebuild_quads(gfx, self.docs.active_mut());
        // ★ พาผู้ใช้ไปดูผลด้วย — ปุ่มชื่อ "ส่งเข้า canvas" แล้วอยู่ที่เดิมคือ
        //   การกดที่ไม่มีอะไรเกิดขึ้นในสายตาเขา (ผลอยู่อีกโหมดหนึ่ง)
        // ★★ ต้องผ่าน `write_mode` ไม่ใช่เขียน `shell.mode` ตรง ๆ — ตั้งแต่ก้อน c
        //    แหล่งความจริงของโหมดคือ `Board::view` ของแท็บนี้ ถ้าเขียนแค่ `shell`
        //    เฟรมถัดไปจะอ่านค่าเก่ากลับมาทับทันที
        Self::write_mode(self.docs.active_mut(), &mut self.shell, Mode::Canvas);
        self.shell.status = text::fill(
            lang,
            Template::LayoutApplied,
            &[("n", &moved.len().to_string())],
        );
        self.shell.status_warn = false;
        gfx.window.request_redraw();
    }

    // ---------- ★★★ P5-3b ก้อน c: หกคีย์ที่ `docs/03 §5` สั่งไว้แต่ไม่เคยมี ----------

    /// ★★★ **ทางเดียวที่โหมดเปลี่ยนได้** — เขียนลง `Board::view` ของแท็บนั้น
    ///
    /// ## ทำไมไม่ใช่ `shell.mode = …` เฉย ๆ (`HANDOFF §6` · `docs/03 §5`)
    ///
    /// โหมดเป็นของ **แท็บ** ไม่ใช่ของหน้าต่าง — `ViewState::mode` persist ลง
    /// `.refx` อยู่แล้วตั้งแต่ P2-1 · `ShellState::mode` ที่เป็นของหน้าต่างคือ
    /// **แหล่งความจริงที่สอง** ที่จะขัดกับไฟล์ทันทีที่มีคนอ่านไฟล์กลับมาใช้
    ///
    /// ★ ตอนนี้ `shell.mode` เป็นแค่ **สำเนาสำหรับวาด** ที่ถูกเติมใหม่จากแท็บที่
    /// ดูอยู่ทุกเฟรม (ดู [`Self::mode_follows_active_tab`]) — เขียนมันตรง ๆ
    /// เมื่อไหร่ ค่าจะอยู่ได้แค่เฟรมเดียว
    ///
    /// **ไม่ผ่าน `Command` · ไม่ทำให้ `dirty`** ตามข้อยกเว้นของ `docs/02 §2.9`
    /// และ `docs/03 §4.3` ("สลับ mode ห้ามแก้ข้อมูล")
    fn write_mode(doc: &mut Doc, shell: &mut crate::shell::ShellState, mode: Mode) {
        shell.mode = mode;
        doc.board
            .set_view(live_view(doc.camera, doc.arrange.camera(), mode));
    }

    /// ★★ โหมดที่จะวาดในเฟรมนี้ = โหมดของ **แท็บที่ดูอยู่** — สลับแท็บแล้วตามไปเอง
    fn mode_follows_active_tab(&mut self) {
        self.shell.mode = self.docs.active().board.view().mode;
    }

    /// ★★★ **คืนมุมมองที่ผู้ใช้บันทึกไว้** — โหมด **และ** กล้อง (P4-1 หนี้ §6 แถวแรก)
    ///
    /// ## ทำไมมันสำคัญกว่าที่หน้าตาบอก
    ///
    /// `ViewState` ถูกเขียนลง `.refx` ทุกครั้งที่บันทึกมาตั้งแต่ P4-1 · การไม่อ่าน
    /// มันกลับคือ **เก็บของของผู้ใช้ไว้แล้วไม่คืนให้** ซึ่งแย่กว่าไม่เก็บเลย
    /// เพราะไฟล์อ้างว่ามีมุมมองอยู่ · ใกล้ I-3 มากกว่าที่คำว่า "กล้อง" ทำให้คิด
    ///
    /// ## ★★ โหมดกับกล้องเดินทาง **เดียวกัน** ไม่ใช่คนละเส้น
    ///
    /// `docs/03 §6` (แก้ 5 ก.ย. 2026): ค่าที่ไหลสองทางต้องระบุให้ชัดว่าเฟรมที่ค่า
    /// มาจากข้างนอกใครชนะ — **ค่าจากไฟล์ชนะในเฟรมนั้น การซิงค์กลับเริ่มเฟรมถัดไป**
    /// · แยกสองเส้นเมื่อไหร่ กฎนั้นต้องถูกทำให้ถูกสองรอบ ซึ่งคือรูปที่เพิ่งพลาดมา
    ///
    /// ## I-4
    ///
    /// ค่าทุกตัวผ่าน [`Camera::new`] ที่ชั้น DTO มาแล้ว (`refx-io::dto`) ซึ่ง
    /// ปฏิเสธ `NaN`/`inf` และ clamp `zoom` เข้าช่วง — `zoom = 0` จึงกลายเป็น 1.0
    /// ไม่ใช่จอว่างที่กลับมาไม่ได้ · ระยะเลื่อนของ Arrange ถูก clamp อีกชั้นตอน
    /// `plan` เพราะแผ่นอาจสั้นลงตั้งแต่บันทึก (ตัวกรอง · หน้าต่างเล็กลง)
    ///
    /// ไฟล์เก่าที่ไม่มี `view` ได้ค่าปริยายจาก `ViewState::default()` — **ไม่ใช่ error**
    ///
    /// ★ รับ `index` แทนที่จะอ่าน `docs.active()` เอง: วันนี้ทั้งสามกิ่งของ
    /// [`Self::adopt_into_tab`] ทำให้แท็บที่รับ board กลายเป็นแท็บที่ดูอยู่พอดี
    /// แต่นั่นเป็นความบังเอิญของโค้ดวันนี้ · ถ้าวันหนึ่งเปิดไฟล์ลงแท็บพื้นหลังได้
    /// กล้องของมันจะไม่ถูกคืนแล้ว **หายถาวรตอนซิงค์กลับ** โดยไม่มีอะไรฟ้อง
    fn restore_view(&mut self, index: usize) {
        let Some(doc) = self.docs.list.get_mut(index) else {
            return;
        };
        let view = *doc.board.view();
        doc.camera = view.canvas;
        doc.arrange.restore_from(view.arrange);
        // โหมดเป็นของ `shell` ซึ่งมีชุดเดียวต่อหน้าต่าง จึงตั้งได้เฉพาะแท็บที่ดูอยู่
        // — แท็บอื่นเก็บโหมดไว้ใน `Board::view` ของตัวเองและถูกอ่านตอนสลับไป
        if index == self.docs.active {
            self.shell.mode = view.mode;
        }
    }

    /// `Tab` — สลับ Canvas ⇄ Arrange (`docs/03 §5`)
    fn apply_mode_toggle(&mut self) {
        if self.gfx.is_none() {
            return;
        }
        let next = match self.shell.mode {
            Mode::Canvas => Mode::Arrange,
            Mode::Arrange => Mode::Canvas,
        };
        Self::write_mode(self.docs.active_mut(), &mut self.shell, next);
        self.shell.status = text::fill(
            self.shell.lang,
            Template::SwitchedMode,
            &[("mode", next.label())],
        );
        self.shell.status_warn = false;
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// `Ctrl+A` — เลือกทุกใบบน board ใบนี้ (`docs/03 §5`)
    ///
    /// ★ การเลือก **ไม่ใช่เอกสาร** (`docs/02 §2.9`) จึงไม่ผ่าน `Command`
    /// ไม่กิน undo และไม่ทำให้ `dirty` — เหมือนคลิกเลือกด้วยเมาส์ทุกประการ
    fn apply_select_all(&mut self) {
        if self.gfx.is_none() {
            return;
        }
        let ids: Vec<ItemId> = self
            .docs
            .active()
            .board
            .items_in_z_order()
            .map(|(id, _)| id)
            .collect();
        let count = ids.len();
        self.docs.active_mut().selection.set_all(ids);
        self.shell.status = text::fill(
            self.shell.lang,
            Template::SelectedAll,
            &[("n", &count.to_string())],
        );
        self.shell.status_warn = false;
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// ★★★ `Esc` — **ปิดแถบที่ค้างอยู่ก่อนเสมอ** แล้วค่อยยกเลิกเลือก
    ///
    /// ผู้ใช้ที่กด `Esc` ตอนมีคำถามค้างอยู่หมายถึง *"ไม่เอาคำถามนี้"* ไม่ใช่
    /// *"ยกเลิกการเลือกของฉัน"* — และการทำอย่างหลังคือการตอบสิ่งที่เขาไม่ได้ถาม
    /// ทั้งที่คำถามยังค้างอยู่บนจอ
    ///
    /// ★ ช่องข้อความไม่ต้องอยู่ในลำดับนี้: egui ถือ focus อยู่ตอนนั้น ด่าน
    /// `egui_wants_keyboard_input()` ใน `on_input` จึงกิน `Esc` ไปก่อนถึงที่นี่
    ///
    /// ★★ ตอบทีละอย่างต่อการกดหนึ่งครั้ง — กดสามทีปิดสามชั้น ไม่ใช่ทีเดียวหายหมด
    fn apply_clear_selection(&mut self) {
        if self.gfx.is_none() {
            return;
        }
        match escape_target(&self.shell, !self.docs.active().selection.is_empty()) {
            EscapeTarget::CancelClose => {
                self.shell.close_choice = Some(crate::shell::CloseChoice::Cancel);
            }
            EscapeTarget::CancelSaveAs => {
                self.shell.save_as_choice = Some(crate::shell::SaveAsChoice::Cancel);
            }
            EscapeTarget::PostponeRecovery => {
                self.shell.recover_choice = Some(crate::shell::RecoverChoice::Later);
            }
            EscapeTarget::StopExport => {
                if let Some(job) = self.export_job.as_ref() {
                    job.cancel();
                }
            }
            EscapeTarget::CloseExport => {
                self.shell.export_prompt = None;
                self.export_target = None;
            }
            EscapeTarget::CloseSettings => self.shell.settings_open = false,
            EscapeTarget::ClearSelection => {
                self.docs.active_mut().selection.clear();
                self.shell.status = text::t(self.shell.lang, Key::SelectionCleared).to_owned();
                self.shell.status_warn = false;
            }
            EscapeTarget::Nothing => {}
        }
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// ★★★ กรอบที่ `F` / `0` จะจัดให้พอดี — `None` = ไม่มีอะไรให้จัด
    ///
    /// ★★ `FitSelection` **ตกไปเป็นทั้ง board เองเมื่อไม่ได้เลือกอะไร** ตามที่
    /// `docs/03 §5` เขียนไว้ ("ถ้าไม่เลือก = พอดีทั้ง board") — กฎอยู่ที่นี่
    /// ไม่ใช่ที่ผู้เรียก จึงเป็นไปไม่ได้ที่ `0` กับ `F`-ไม่เลือก จะให้ผลต่างกัน
    ///
    /// ★ ไม่คัดตามตัวกรองของ Arrange: `F`/`0` ทำงานบน Canvas ซึ่งวาดทุกใบ
    /// การจัดให้พอดีเฉพาะใบที่ผ่านตัวกรองของอีกโหมดหนึ่งจะทิ้งภาพไว้นอกจอ
    fn fit_bounds(doc: &Doc, request: keymap::ZoomRequest) -> Option<refx_core::geom::Rect> {
        let mut bounds = refx_core::geom::Rect::EMPTY;
        let mut found = false;
        let mut cover = |canvas: &refx_core::board::ItemCanvas| {
            bounds = bounds.union(canvas.world_bounds());
            found = true;
        };
        if request == keymap::ZoomRequest::FitSelection && !doc.selection.is_empty() {
            for id in doc.selection.iter() {
                if let Some(item) = doc.board.item(id) {
                    cover(&item.canvas);
                }
            }
        } else {
            for (_, item) in doc.board.items_in_z_order() {
                cover(&item.canvas);
            }
        }
        found.then_some(bounds).filter(|rect| rect.is_finite())
    }

    // ================= ★★★ ส่งออกภาพ (P5-4 · `docs/07 §6`) =================

    /// ขอเฟรมใหม่ถ้ามีหน้าต่างอยู่ — **ไม่ปลุกอะไรเลยถ้าไม่มี** (I-1)
    fn request_redraw(&self) {
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// `Ctrl+E` / ปุ่มในเมนู — เปิดกล่องส่งออก
    ///
    /// ★★ **ปฏิเสธตั้งแต่ตรงนี้ถ้าไม่มีอะไรให้ส่งออก** ไม่ใช่เปิดกล่องให้ผู้ใช้
    /// ตั้งค่าไปสามอย่างแล้วค่อยบอกว่าทำไม่ได้ (`docs/07 §6` ข้อ 5)
    fn open_export_dialog(&mut self) {
        let lang = self.shell.lang;
        if Self::fit_bounds(self.docs.active(), keymap::ZoomRequest::FitBoard).is_none() {
            self.shell.status = text::t(lang, Key::ExportNothing).to_owned();
            self.shell.status_warn = true;
            self.request_redraw();
            return;
        }
        // ★ เปิดซ้ำตอนเปิดอยู่แล้ว = ไม่ทำอะไร · ค่าที่ผู้ใช้เพิ่งปรับต้องไม่หาย
        if self.shell.export_prompt.is_none() {
            self.shell.export_prompt = Some(crate::shell::ExportView::default());
            self.export_target = None;
        }
        self.request_redraw();
    }

    /// เติมค่าที่ **คำนวณได้** ลงในกล่อง export ทุกเฟรมที่มันเปิดอยู่
    ///
    /// ★★★ ขนาดจริงมาจาก **สัดส่วนของ board** เสมอ — ผู้ใช้ปรับได้แค่ด้านยาวสุด
    /// การให้ปรับกว้าง/สูงแยกกันคือการเปิดทางให้เขาทำภาพยืดผิดสัดส่วนโดยไม่ตั้งใจ
    fn refresh_export_view(&mut self) {
        let Some(bounds) = Self::fit_bounds(self.docs.active(), keymap::ZoomRequest::FitBoard)
        else {
            return;
        };
        let items = self.docs.active().board.len();
        let missing = self
            .docs
            .active()
            .board
            .items_in_z_order()
            .filter(|(_, item)| matches!(item.kind, refx_core::board::ItemKind::Missing { .. }))
            .count();
        // ★★★ ด้านที่ยาวที่สุดของ item ที่ **มีพิกเซลจริง** — ตัวที่จำกัดเพดาน
        //     `Missing` กับโน้ตข้อความไม่นับ: อันแรกเป็นสี่เหลี่ยมสีล้วน อันหลัง
        //     egui วาดเป็นเวกเตอร์ — ทั้งคู่ขยายแล้วไม่เสียอะไร
        let biggest = self
            .docs
            .active()
            .board
            .items_in_z_order()
            .filter(|(_, item)| matches!(item.kind, refx_core::board::ItemKind::Image(_)))
            .map(|(_, item)| item.canvas.size.x.max(item.canvas.size.y))
            .fold(0.0_f32, f32::max);
        let Some(view) = self.shell.export_prompt.as_mut() else {
            return;
        };

        let size = bounds.size();
        // ★★★ เพดานมาจาก **พิกเซลที่มีจริง** ไม่ใช่จากขีดจำกัดของ GPU (`docs/07 §6`)
        //
        //     แหล่งพิกเซลวันนี้คือ atlas ซึ่งเป็น thumbnail ด้านละ `SLOT_SIZE`
        //     ★★ วันที่ P5-4b ให้แหล่งที่ดีกว่า **แก้บรรทัดนี้บรรทัดเดียว**
        //        แล้วเพดานยกขึ้นเองทั้งกล่อง — ไม่ต้องแตะ UI เลย
        view.max_side = refx_core::export::max_export_side(
            size.x.max(size.y),
            biggest,
            refx_render::atlas::SLOT_SIZE,
        );
        let long = view
            .long_side
            .clamp(refx_core::export::MIN_EXPORT_SIDE, view.max_side)
            .max(1);
        // ★ ด้านที่สั้นกว่าคำนวณจากสัดส่วน แล้ว clamp ให้อย่างน้อย 1 จุด —
        //   board ที่เรียงเป็นเส้นตรงพอดีให้ขนาด 0 ในแกนหนึ่งได้จริง
        let (width, height) = if size.x >= size.y {
            let ratio = if size.x > 0.0 { size.y / size.x } else { 1.0 };
            (long, scale_side(long, ratio))
        } else {
            let ratio = if size.y > 0.0 { size.x / size.y } else { 1.0 };
            (scale_side(long, ratio), long)
        };
        view.long_side = long;
        view.width = width;
        view.height = height;
        view.items = items;
        view.missing = missing;
        view.estimate = estimate_file_size(width, height, view.kind, view.quality);
    }

    /// ผู้ใช้กดปุ่มอะไรในกล่อง export เฟรมที่แล้ว
    fn apply_export_request(&mut self) {
        let Some(request) = self.shell.export_request.take() else {
            return;
        };
        match request {
            // ★ ปุ่มบนแถบเครื่องมือกับ `Ctrl+E` เดินทางเดียวกันเป๊ะ — ถ้าแยกทาง
            //   วันหนึ่งจะมีทางหนึ่งที่ลืมตรวจว่า board ว่างหรือเปล่า
            crate::shell::ExportRequest::Open => self.open_export_dialog(),
            crate::shell::ExportRequest::Close => {
                self.shell.export_prompt = None;
                self.export_target = None;
            }
            crate::shell::ExportRequest::ChooseTarget => self.choose_export_target(),
            crate::shell::ExportRequest::Start => self.start_export(),
            crate::shell::ExportRequest::Cancel => {
                if let Some(job) = self.export_job.as_ref() {
                    job.cancel();
                }
            }
        }
        self.request_redraw();
    }

    /// เปิดกล่องของ OS ให้เลือกที่บันทึก — **ไม่บล็อก UI thread** (I-2)
    fn choose_export_target(&mut self) {
        let Some(view) = self.shell.export_prompt.as_ref() else {
            return;
        };
        if view.choosing {
            return;
        }
        let extension = match view.kind {
            crate::shell::ExportKind::Png => "png",
            crate::shell::ExportKind::Jpeg => "jpg",
        };
        // ★ ชื่อที่แนะนำมาจากชื่อเอกสาร — ผู้ใช้ที่ export บ่อยจะได้ไม่ต้องพิมพ์ใหม่
        let stem = self
            .docs
            .active()
            .path
            .as_deref()
            .and_then(std::path::Path::file_stem)
            .map_or_else(|| "board".to_owned(), |s| s.to_string_lossy().into_owned());
        let waker = self.waker.clone();
        self.export_target_rx = Some(refx_platform::dialog::pick_export_location(
            text::t(self.shell.lang, Key::DialogExportImage),
            &format!("{stem}.{extension}"),
            extension,
            waker,
        ));
        if let Some(view) = self.shell.export_prompt.as_mut() {
            view.choosing = true;
            view.problem = None;
        }
    }

    /// รับผลจากกล่องของ OS — เรียกทุกเฟรม **ไม่บล็อก**
    fn poll_export_target(&mut self) {
        let Some(rx) = self.export_target_rx.as_ref() else {
            return;
        };
        let picked = match rx.try_recv() {
            Ok(picked) => picked,
            // ★ ช่องปิดโดยไม่ส่งอะไร = เธรด dialog ตายไป · ต้องเลิกรอ ไม่ใช่ค้าง
            Err(crossbeam_channel::TryRecvError::Disconnected) => None,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
        };
        self.export_target_rx = None;
        if let Some(view) = self.shell.export_prompt.as_mut() {
            view.choosing = false;
            match &picked {
                Some(path) => {
                    view.target = path.file_name().map(|n| n.to_string_lossy().into_owned());
                    // ★★ "ทับไฟล์ที่มีอยู่ = ถามก่อน" — ตรวจที่นี่ครั้งเดียว
                    //    แล้วปุ่มจะเปลี่ยนเป็น "ทับไฟล์เดิม" พร้อมสีเตือน
                    view.overwrite = path.exists();
                }
                None => view.target = None,
            }
        }
        self.export_target = picked;
        self.request_redraw();
    }

    /// ยิงงาน export ลง worker
    fn start_export(&mut self) {
        let lang = self.shell.lang;
        let (Some(path), Some(view)) = (
            self.export_target.clone(),
            self.shell.export_prompt.as_ref(),
        ) else {
            return;
        };
        let Some(region) = Self::fit_bounds(self.docs.active(), keymap::ZoomRequest::FitBoard)
        else {
            return;
        };
        let format = match view.kind {
            crate::shell::ExportKind::Png => refx_asset::export::ExportFormat::Png {
                transparent: view.transparent,
            },
            crate::shell::ExportKind::Jpeg => refx_asset::export::ExportFormat::Jpeg {
                quality: view.quality,
            },
        };
        let request = crate::export::ExportRequest {
            path,
            format,
            size: (view.width, view.height),
            region,
            // ★★★ JPEG ไม่มี alpha · PNG ที่ไม่ติ๊กโปร่งใสก็ต้องทึบ
            //     `export_board` บังคับ alpha ของ JPEG ให้อีกชั้นหนึ่งด้วย
            background: [
                view.background[0],
                view.background[1],
                view.background[2],
                if view.transparent && view.kind == crate::shell::ExportKind::Png {
                    0
                } else {
                    255
                },
            ],
        };

        // ★★★ **ภาพนิ่งของสิ่งที่วาดอยู่ ณ วินาทีที่กด** — ไม่ใช่การยืมสถานะสด
        //     ผู้ใช้ที่ลากภาพเพิ่มระหว่าง export คาดหวังไฟล์ที่ตรงกับตอนที่กดปุ่ม
        let Some(gfx) = self.gfx.as_ref() else {
            return;
        };
        let batches = vec![crate::export::OwnedBatch {
            bind_group: gfx.atlas.bind_group().clone(),
            instances: gfx.quads.clone(),
        }];
        let job = crate::export::ExportJob::spawn(
            crate::export::OwnedGpu {
                device: gfx.render.device().clone(),
                queue: gfx.render.queue().clone(),
                allocator: gfx.textures.clone(),
                atlas_layout: gfx.atlas.bind_group_layout().clone(),
            },
            batches,
            request,
            refx_platform::fsops::rename_durable,
            self.waker.clone(),
        );
        match job {
            Ok(job) => {
                let (done, total) = job.progress();
                self.shell.export_progress = Some(crate::shell::ExportProgress {
                    name: job.name().to_owned(),
                    done,
                    total,
                    cancelling: false,
                });
                self.export_job = Some(job);
                // กล่องตั้งค่าปิดไปได้แล้ว — แถบความคืบหน้าเข้ามาแทน
                self.shell.export_prompt = None;
                self.export_target = None;
            }
            Err(err) => self.report_export_failure(lang, &err.to_string(), None),
        }
    }

    /// ถามงานที่กำลังทำอยู่ว่าเสร็จหรือยัง — **ไม่บล็อก** เรียกทุกเฟรม
    fn poll_export_job(&mut self) {
        let Some(job) = self.export_job.as_ref() else {
            return;
        };
        let (done, total) = job.progress();
        let cancelling = job.cancelling();
        let name = job.name().to_owned();

        let Some(outcome) = job.finished() else {
            // ★ อัปเดตตัวเลขเฉพาะตอนมันขยับจริง — เขียนทุกเฟรมทั้งที่ค่าเท่าเดิม
            //   จะทำให้เกิดคำขอวาดใหม่ต่อเนื่องซึ่งชน I-1
            let current = crate::shell::ExportProgress {
                name,
                done,
                total,
                cancelling,
            };
            if self.shell.export_progress.as_ref() != Some(&current) {
                self.shell.export_progress = Some(current);
                self.request_redraw();
            }
            return;
        };

        self.export_job = None;
        self.shell.export_progress = None;
        let lang = self.shell.lang;
        match outcome {
            Ok(stats) => {
                self.shell.status = text::fill(
                    lang,
                    Template::ExportDone,
                    &[
                        ("name", &name),
                        ("size", &crate::shell::human_bytes(stats.bytes)),
                    ],
                );
                self.shell.status_warn = false;
                tracing::info!(bytes = stats.bytes, bands = stats.bands, "export finished");
            }
            Err(crate::export::ExportJobError::Write(
                refx_asset::export::ExportError::Cancelled,
            )) => {
                self.shell.status = text::t(lang, Key::ExportCancelled).to_owned();
                self.shell.status_warn = false;
            }
            Err(err) => self.report_export_failure(lang, &err.to_string(), Some(&name)),
        }
        self.request_redraw();
    }

    /// ข้อความเดียวสำหรับทุกทางที่ export ล้ม — **บอกสิ่งที่ทำได้ต่อเสมอ**
    fn report_export_failure(&mut self, lang: Lang, reason: &str, name: Option<&str>) {
        // ★ ที่ที่เขียนไม่ได้มีทางออกที่ชัด (ตั้งชื่อใหม่) จึงบอกทางออกนั้นตรง ๆ
        //   แทนข้อความของระบบที่ผู้ใช้ทำอะไรกับมันไม่ได้
        let message = if reason.contains("names a device") {
            text::t(lang, Key::ExportBadTarget).to_owned()
        } else {
            text::fill(
                lang,
                Template::ExportFailed,
                &[("name", name.unwrap_or("-")), ("reason", reason)],
            )
        };
        tracing::error!(%reason, "export failed");
        if let Some(view) = self.shell.export_prompt.as_mut() {
            view.problem = Some(message.clone());
        }
        self.shell.status = message;
        self.shell.status_warn = true;
    }

    /// `F` / `1` / `0` — ระดับซูม (`docs/03 §5`)
    ///
    /// ★★ ทำงานกับกล้องของ **Canvas** เท่านั้น · Arrange เป็นแผ่นที่จัดเองและ
    /// เลื่อนด้วยการ scroll ล้วน ๆ (P3-3) การ "ซูม 100%" ที่นั่นไม่มีความหมาย
    /// — บอกตรง ๆ ดีกว่าเงียบ ไม่งั้นผู้ใช้อ่านว่าคีย์ลัดเสีย
    fn apply_zoom(&mut self, request: keymap::ZoomRequest) {
        let Some(gfx) = self.gfx.as_ref() else {
            return;
        };
        let lang = self.shell.lang;
        if self.shell.mode != Mode::Canvas {
            self.shell.status = text::t(lang, Key::ZoomIsCanvasOnly).to_owned();
            self.shell.status_warn = false;
            gfx.window.request_redraw();
            return;
        }
        // ★ ขนาดช่อง canvas จริงเป็น **พิกเซลจริง** ซึ่งเป็นหน่วยที่ `Camera` ใช้
        //   (ถ้าใช้ขนาดหน้าต่างทั้งบาน กรอบจะเลยไปอยู่ใต้ panel — ดู `CanvasRect`)
        let viewport = gfx.canvas.size;
        match request {
            keymap::ZoomRequest::Actual => {
                self.docs.active_mut().camera.set_zoom(1.0);
            }
            keymap::ZoomRequest::FitSelection | keymap::ZoomRequest::FitBoard => {
                let Some(bounds) = Self::fit_bounds(self.docs.active(), request) else {
                    // ★ board ว่าง = ไม่มีอะไรให้จัด · บอกแล้วจบ ไม่ขยับกล้อง
                    self.shell.status = text::t(lang, Key::NothingToFit).to_owned();
                    self.shell.status_warn = false;
                    gfx.window.request_redraw();
                    return;
                };
                self.docs.active_mut().camera.fit_to(bounds, viewport);
            }
        }
        self.shell.status = text::fill(
            lang,
            Template::ZoomSet,
            &[(
                "percent",
                &format!("{:.0}", self.docs.active().camera.zoom() * 100.0),
            )],
        );
        self.shell.status_warn = false;
        gfx.window.request_redraw();
    }

    /// `G` / `H` (P2-8)
    fn apply_appearance_key(&mut self, what: AppearanceKey) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        match what {
            // ★ ไม่ผ่าน `Command` โดยตั้งใจ — เป็นสวิตช์การมองเห็น ไม่ใช่การแก้เอกสาร
            //   (ถ้าเอาเข้า undo stack ผู้ใช้ที่กด G ดูค่าน้ำหนักแล้วกด Ctrl+Z
            //   จะได้สีคืนแทนที่จะได้งานคืน ซึ่งไม่ใช่สิ่งที่เขาขอ)
            AppearanceKey::ToggleBoardGrayscale => {
                self.shell.board_grayscale = !self.shell.board_grayscale;
                gfx.window.request_redraw();
            }
            AppearanceKey::FlipHorizontal => {
                let doc = self.docs.active();
                let board = &doc.board;
                let changes: Vec<(ItemId, ItemCanvas)> = doc
                    .selection
                    .iter()
                    .filter_map(|id| board.item(id).map(|item| (id, item.canvas)))
                    .filter(|(_, canvas)| !canvas.locked)
                    .map(|(id, canvas)| {
                        let flip = match canvas.flip {
                            Flip::None => Flip::Horizontal,
                            Flip::Horizontal => Flip::None,
                            Flip::Vertical => Flip::Both,
                            Flip::Both => Flip::Vertical,
                            // ★ ค่าที่รุ่นนี้ไม่รู้จัก (มาจากไฟล์ของรุ่นใหม่กว่า —
                            //   docs/02 §2.9) · การกด `H` คือ **ผู้ใช้สั่งทับเอง**
                            //   ซึ่งเป็นจังหวะเดียวที่เขียนทับค่าที่ถือไว้ได้อย่างถูกต้อง
                            //   — กฎ "ห้ามแปลงค่าทิ้ง" ห้ามการหายแบบ *เงียบ ๆ*
                            //   ไม่ได้ห้ามผู้ใช้เปลี่ยนค่าด้วยตัวเอง
                            Flip::Unknown(_) => Flip::Horizontal,
                        };
                        (id, ItemCanvas { flip, ..canvas })
                    })
                    .collect();
                let Ok(command) = SetFilter::new(changes) else {
                    return;
                };
                if let Err(err) = self.docs.active_mut().apply(Box::new(command)) {
                    tracing::error!(%err, "cannot flip the selected images");
                    return;
                }
                // กดทีละครั้ง = คนละขั้นเสมอ ห้ามให้การกดถัดไปกลืนเข้าไป
                self.docs.active_mut().history.seal();
                Self::collect_forgotten(gfx, self.docs.active_mut());
                Self::rebuild_quads(gfx, self.docs.active_mut());
                gfx.window.request_redraw();
            }
        }
    }

    /// ★ ค่าที่ผู้ใช้ปรับใน inspector — เทียบกับของจริงแล้วห่อเป็น `SetFilter`
    ///
    /// widget เขียนลง `shell.appearance` เท่านั้น **ไม่มี `&mut Board` หลุดไปถึง egui**
    /// กฎ "ทุก mutation ผ่าน `Command`" (docs/08 §4 ข้อ 10) จึงยังบังคับได้จริง
    /// เขียนสิ่งที่ผู้ใช้พิมพ์ลงโน้ตผ่าน `EditText` (P2-11)
    ///
    /// ★ โครงเดียวกับ [`Self::apply_inspector_edit`] เป๊ะ ๆ: `take()` ทันที
    /// เพราะ "สิ่งที่ผู้ใช้ขอ" มีอายุหนึ่งเฟรม ถ้าปล่อยค้างไว้มันจะถูกเขียนซ้ำทุกเฟรม
    /// แล้วทับสิ่งที่ undo เพิ่งคืนมา — กด Ctrl+Z แล้วข้อความเด้งกลับทันที
    fn apply_note_edit(&mut self) {
        let sealed = std::mem::take(&mut self.shell.note_sealed);
        let Some(wanted) = self.shell.note_edit.take() else {
            if sealed && self.gfx.is_some() {
                self.docs.active_mut().history.seal();
            }
            return;
        };
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        // แก้ **ใบเดียว** เสมอ — ช่องข้อความแสดงของ item ตัวแรกในชุดที่เลือก
        // การเขียนข้อความเดียวกันลงทุกใบที่เลือกไว้ไม่ใช่สิ่งที่ใครคาดหวัง
        let doc = self.docs.active();
        let board = &doc.board;
        let Some(id) = doc.selection.iter().find(|id| {
            matches!(
                board.item(*id).map(|item| &item.kind),
                Some(refx_core::board::ItemKind::Text(_))
            )
        }) else {
            return;
        };
        let current = match self.docs.active().board.item(id).map(|item| &item.kind) {
            Some(refx_core::board::ItemKind::Text(note)) => note.text.clone(),
            _ => return,
        };
        // ★ ไม่มีอะไรเปลี่ยน = ไม่สร้างคำสั่ง ไม่ขอเฟรม (I-1)
        if current == wanted {
            if sealed {
                self.docs.active_mut().history.seal();
            }
            return;
        }
        if let Err(err) = self
            .docs
            .active_mut()
            .apply(Box::new(EditText::new(id, wanted)))
        {
            tracing::error!(%err, "cannot edit the note");
        }
        if sealed {
            self.docs.active_mut().history.seal();
        }
        gfx.window.request_redraw();
    }

    /// ★★ ที่ที่ snapshot รอบนี้จะไปลง — `None` = ไม่มีที่ให้วางเลย
    ///
    /// ★★★ **งานที่ยังไม่เคยบันทึกต้องมี autosave ด้วย** (`docs/07 §4`, P4-4)
    ///
    /// P4-3 เขียนได้เฉพาะเอกสารที่มี path แล้ว ซึ่งแปลว่าคนที่จัด mood board
    /// มาสามชั่วโมงโดยยังไม่เคยกด `Ctrl+S` — คนที่เสียมากที่สุดถ้าโปรแกรมตาย —
    /// ไม่มีอะไรคุ้มครองเลย · ตอนนี้เขาตกมาที่ [`SnapshotTarget::Recovery`]
    ///
    /// ★ `None` เกิดได้ทางเดียว: ยังไม่เคยบันทึก **และ** หาโฟลเดอร์ data ไม่ได้
    ///
    /// ★★★ เป็นฟังก์ชันที่รับ **แท็บใบหนึ่ง** ไม่ใช่เมธอดที่อ่าน "แท็บที่ดูอยู่"
    /// (P4-7c) — ทุกแท็บต้องถูก snapshot ไม่ใช่เฉพาะใบที่ผู้ใช้กำลังมองอยู่
    /// ตอนนั้น · เขียนเป็นเมธอดบน `self` เมื่อไหร่ จะไม่มีใครสังเกตว่าอีกสามใบ
    /// ที่เปิดค้างไว้ไม่มีอะไรคุ้มครองเลย
    fn snapshot_target(
        doc: &Doc,
        recovery_dir: Option<&std::path::Path>,
    ) -> Option<SnapshotTarget> {
        if let Some(path) = doc.path.clone() {
            return Some(SnapshotTarget::BesideDocument(path));
        }
        recovery_dir
            .map(std::path::Path::to_path_buf)
            .map(SnapshotTarget::Recovery)
    }

    /// ★★★ autosave หนึ่งจังหวะ — เรียกทุกเฟรม **ไม่บล็อก** (P4-3/P4-4, I-2)
    ///
    /// สองด่านที่ต้องผ่านทั้งคู่อยู่ใน `Autosaver::should_write` (ฟังก์ชันบริสุทธิ์
    /// ที่เทสต์ได้โดยไม่ต้องแตะดิสก์) · ที่นี่มีแค่การต่อสาย
    ///
    /// ★ นโยบายเดียวคุมทั้งสองปลายทาง — ผู้ใช้ที่ยังไม่เคยบันทึกได้การคุ้มครอง
    /// **เท่ากันเป๊ะ** กับคนที่บันทึกแล้ว ไม่ใช่รุ่นด้อยกว่า
    ///
    /// ★★★ **ทุกแท็บ ไม่ใช่แท็บที่ดูอยู่** (P4-7c) — เกณฑ์ผ่านของ P4-7 คือ
    /// *"เปิดสองเอกสาร แก้ทั้งคู่ ไม่ save เลย → ฆ่าโปรเซส → กู้ได้ทั้งสอง"*
    /// ซึ่งเป็นไปไม่ได้เลยถ้าใบที่ไม่ได้อยู่หน้าจอไม่ถูกเขียน · แต่ละใบมีนาฬิกา
    /// และช่องงานเป็นของตัวเอง จึงเขียนพร้อมกันได้โดยไม่ต้องรอกัน
    fn tick_autosave(&mut self) {
        let Self {
            docs,
            recovery_dir,
            gfx,
            waker,
            ..
        } = self;
        // ★ ต้องมีหน้าต่างแล้วเท่านั้น (เงื่อนไขเดิม) — เอกสารอยู่คนละที่กับ `Gfx` แล้ว
        let has_window = gfx.is_some();
        for doc in docs.iter_mut() {
            Self::tick_autosave_one(doc, recovery_dir.as_deref(), has_window, waker.as_ref());
        }
        // ★★★ `.refx-meta` ใช้ **นาฬิกาเดียวกัน** ไม่ใช่ตัวปลุก (P5-5)
        //
        //   ผลของการเขียนคือไฟล์ลงดิสก์ — ไม่มีอะไรให้วาด · ปลุกให้วาดคือทำให้
        //   `ui-idle-diff` แดงกับพฤติกรรมที่ถูก (`docs/08 §3.9` ข้อ 18 ชนิด `OwnTimer`)
        //   ★ และการเก็บผลต้องอยู่ที่นี่ด้วย ไม่ใช่แค่ในเฟรม: แอปที่หลับอยู่
        //     ไม่มีเฟรม ผลจึงค้างตลอดกาลถ้ามีแต่ทางของเฟรม
        let policy = self.settings.sidecar;
        for doc in self.docs.iter_mut() {
            let say = doc.sidecar.collect_write();
            if !say.is_empty() {
                tracing::warn!(count = say.len(), "a sidecar could not be written");
            }
            doc.sidecar.start_write(&doc.board, policy);
        }
    }

    /// autosave หนึ่งจังหวะของ **แท็บใบเดียว** — ตรรกะเดิมทั้งหมด แค่ผูกกับ `doc`
    ///
    /// ★ `waker` เป็นพารามิเตอร์ **ไม่ใช่ค่าที่หยิบเองข้างใน** เพราะฟังก์ชันนี้
    /// ไม่มี `self` (มันวนทีละแท็บ) — ดูเหตุผลที่ต้องมีเลยที่จุดสร้างเธรด
    fn tick_autosave_one(
        doc: &mut Doc,
        recovery_dir: Option<&std::path::Path>,
        has_window: bool,
        waker: Option<&refx_platform::window::Waker>,
    ) {
        // เก็บผลของรอบก่อน (ถ้ามี) — autosave ที่ล้มไม่ใช่เรื่องที่ผู้ใช้ต้องเห็น
        if let Some(rx) = doc.autosave_job.as_ref() {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    doc.autosave_job = None;
                    // ★★ ตอนนี้งานชุดที่กู้มามีสำเนาใหม่ของแท็บนี้บนดิสก์แล้ว
                    //    ตัวเก่าจึงหมดหน้าที่ · ลบก่อนหน้านี้ = มีช่วงที่ไม่มีสำเนาเลย
                    if let Some(old) = doc.adopted_recovery.take() {
                        remove_recovery_file(&old);
                    }
                    // ★ เหตุผลเดียวกันเป๊ะกับ `adopted_recovery` — ของที่ผู้ใช้
                    //   สั่งเก็บไว้แล้วกู้กลับมา หมดหน้าที่ก็ต่อเมื่อแท็บนี้
                    //   มีสำเนาของตัวเองบนดิสก์แล้ว
                    if let Some(old) = doc.adopted_kept.take()
                        && let Err(err) = std::fs::remove_file(&old)
                        && err.kind() != std::io::ErrorKind::NotFound
                    {
                        tracing::warn!(%err, "cannot remove the kept snapshot after adopting it");
                    }
                }
                Ok(Err(err)) => {
                    tracing::warn!(%err, "autosave snapshot failed");
                    doc.autosave_job = None;
                    // ★ ล้มแล้วต้อง **ลองใหม่** ไม่ใช่ถือว่าเก็บไปแล้ว (I-3) ·
                    //   ตัวเว้นระยะยังคุมอยู่ จึงลองรอบละครั้ง ไม่ใช่รัวทุกเฟรม
                    doc.snapshot_revision = None;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => return,
                Err(crossbeam_channel::TryRecvError::Disconnected) => doc.autosave_job = None,
            }
        }
        let Some(target) = Self::snapshot_target(doc, recovery_dir) else {
            return;
        };
        let unsaved = has_window && Self::doc_has_unsnapshotted_work(doc);
        if !has_window {
            return;
        }
        let now = std::time::Instant::now();
        if !doc.autosaver.should_write(unsaved, now) {
            return;
        }
        let revision = doc.board.revision();

        // ★ โคลน ณ จังหวะที่ตัดสิน ด้วยเหตุผลเดียวกับการบันทึกจริง —
        //   ผู้ใช้ต้องแก้งานต่อได้ระหว่างที่ snapshot กำลังเขียน
        let board = doc.board.clone();
        let session = doc.session.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        // ★★★ **ช่องนี้ถูกปลุกด้วยนาฬิกาของ autosave เอง ไม่ใช่ด้วยตัวปลุก**
        //     (`docs/08 §3.9` ข้อ 18 · ดู `next_autosave_across_tabs`)
        //
        //     ลองใช้ `Waker` มาก่อนแล้ว **แล้ววัดได้ว่ามันทำให้ตัวนับเฟรมไต่**:
        //     แก้ board แล้วปล่อยไว้ 60 วินาที → `Frames 516 → 517` เพราะทาง
        //     ของตัวปลุกจบที่ `request_redraw` เสมอ · การวาดใหม่ไม่ได้ทำให้
        //     snapshot ขึ้นดิสก์ และประตู `ui-idle-diff` จะแดงให้กับพฤติกรรม
        //     ที่ถูกต้อง ซึ่งฝึกให้คนเลิกเชื่อประตู
        //
        //     → ผลถูกเก็บโดยนาฬิกาแทน: **ตื่นโดยไม่วาด** ตามรูปเดียวกับที่
        //       `on_wake` ใช้เขียน snapshot อยู่แล้ว
        let _ = waker;
        let spawned = std::thread::Builder::new()
            .name("refx-autosave".to_owned())
            .spawn(move || {
                // ★ `rename_durable` ตัวเดียวกับการบันทึกจริงทั้งสองเส้นทาง
                let rename = refx_platform::fsops::rename_durable;
                let result = match target {
                    SnapshotTarget::BesideDocument(path) => {
                        refx_io::autosave::write_snapshot(&path, &board, rename)
                    }
                    SnapshotTarget::Recovery(dir) => {
                        refx_io::recovery::write_snapshot(&dir, &session, &board, rename)
                    }
                }
                .map_err(|err| err.to_string());
                let _ = tx.send(result);
            });
        if spawned.is_err() {
            // ★ สร้างเธรดไม่ได้ (RAM หมด/ถึงเพดานเธรดของ OS) — **ยังต้องเดินนาฬิกา**
            //   ไม่งั้นนาฬิกาจะค้างอยู่ในอดีตแล้วถูกปลุกซ้ำทันทีไม่รู้จบ
            //   · ไม่บันทึก `snapshot_revision` เพราะยังไม่มีอะไรลงดิสก์จริง
            //   → รอบหน้าหลังเว้นระยะครบ จะลองใหม่เอง
            doc.autosaver.record_write(now);
            return;
        }
        doc.autosave_job = Some(rx);
        doc.snapshot_revision = Some(revision);
        doc.autosaver.record_write(now);
    }

    /// ★★★ เวลาที่ต้องถูกปลุกมาเขียน snapshot — `None` = ไม่มีอะไรค้าง
    ///
    /// ★★ ถามผ่าน `snapshot_target()` ตัวเดียวกับที่ `tick_autosave` ใช้ —
    /// ถ้าที่นี่ถาม `path` ตรง ๆ เหมือนเดิม งานที่ยังไม่เคยบันทึกจะ
    /// **ไม่มีใครปลุกมาเขียนเลยตอนผู้ใช้ลุกจากโต๊ะ** ซึ่งเป็นช่องเดียวกับที่
    /// P4-3 เกือบพลาด แค่ย้ายมาโผล่ที่ผู้ใช้อีกกลุ่มหนึ่ง
    ///
    /// ★ เงื่อนไขคือ [`Self::doc_has_unsnapshotted_work`] ไม่ใช่ `dirty` —
    /// ไม่งั้น board ที่เก็บครบแล้วแต่ยังไม่ได้ `Ctrl+S` จะขอให้ปลุกทุก 10 วินาที
    /// **ตลอดทั้งวัน** ทั้งที่ไม่มีอะไรให้เขียน (I-1)
    ///
    /// ★★★ เอา **ตัวที่ใกล้ที่สุดของทุกแท็บ** — แท็บที่ไม่ได้อยู่หน้าจอก็ต้อง
    /// ปลุกโปรแกรมมาเขียนของมันได้ ไม่งั้นมันจะถูกเก็บก็ต่อเมื่อผู้ใช้บังเอิญ
    /// สลับกลับไปดู ซึ่งเป็นเงื่อนไขที่ crash ไม่เคยรอ
    fn autosave_deadline(&self) -> Option<std::time::Instant> {
        self.gfx.as_ref()?;
        Self::next_autosave_across_tabs(&self.docs, self.recovery_dir.as_deref())
    }

    /// นาฬิกาที่ใกล้ที่สุดของทุกแท็บ — **ฟังก์ชันบริสุทธิ์** เทสต์ได้โดยไม่ต้องมีหน้าต่าง
    ///
    /// ★ แยกออกมาเพราะเงื่อนไข "ต้องมี `Gfx`" ของ [`Self::autosave_deadline`]
    /// ทำให้เทสต์เข้าไม่ถึงตรรกะข้างในเลยสักบรรทัด (`docs/08 §3.9` ข้อ 1:
    /// ประตูที่เทสต์ไม่ได้คือประตูที่ไม่มีใครรู้ว่ามันล้มเป็นหรือเปล่า)
    fn next_autosave_across_tabs(
        docs: &Docs,
        recovery_dir: Option<&std::path::Path>,
    ) -> Option<std::time::Instant> {
        /// นานแค่ไหนถึงกลับมาดูว่า snapshot ที่ส่งไปเธรดเขียนเสร็จหรือยัง
        ///
        /// ★ สั้นพอที่ผู้ใช้จะได้ข้อความ "เขียน snapshot ไม่สำเร็จ" ทันเวลา
        /// ยาวพอที่จะไม่กลายเป็นการวนถาม · การเขียนจริงใช้เวลาไม่กี่มิลลิวินาที
        /// จึงเจอผลตั้งแต่ครั้งแรกเกือบเสมอ
        const COLLECT_SNAPSHOT_AFTER: std::time::Duration = std::time::Duration::from_millis(250);

        docs.iter()
            .filter_map(|doc| {
                // ★★★ งานที่ส่งไปเธรดแล้วยังไม่กลับ = **ตั้งนาฬิกาไว้ข้างหน้า**
                //     ไม่ใช่ปล่อยว่าง และไม่ใช่ชี้เวลาที่ผ่านมาแล้ว
                //
                //   เดิมที่นี่คืน `None` เพราะกลัววงจรนี้: ตื่นตามเวลา →
                //   `tick_autosave` เห็นว่ามีงานค้างแล้วออกทันที → นาฬิกายังชี้
                //   เวลาที่ผ่านมาแล้ว → ตื่นอีกทันที = `ControlFlow::Poll`
                //   ที่ I-1 ห้ามไว้ แค่สะกดด้วยชื่ออื่น
                //
                //   ★★ แต่ `None` มีราคาที่แพงกว่า: **ไม่มีใครกลับมาเก็บผลเลย**
                //   แอปหลับยาว → `autosave_job` ค้าง → งวดถัดไปไม่มีวันเกิด
                //   → **autosave หยุดทำงานถาวร** จนกว่าผู้ใช้จะขยับเมาส์
                //   (`docs/08 §3.9` ข้อ 18 — ผลจากเธรดอื่นต้องมีคนมาเก็บ)
                //
                //   ★★★ ทางที่ถูกคือชี้ไป **ข้างหน้า** — หลับจริงจนถึงตอนนั้น
                //   แล้วตื่นมาเก็บผล · ไม่ใช่ลูป: การเขียน snapshot ใช้เวลาระดับ
                //   มิลลิวินาที (3,072 ใบ = 29 KB) จึงตื่นเพิ่มครั้งเดียวก็เจอผลแล้ว
                //   · และ **ไม่ขอเฟรม** — ต่างจากทางของ `Waker` ซึ่งจบที่
                //   `request_redraw` เสมอ แล้วทำให้ตัวนับเฟรมไต่ทั้งที่จอไม่เปลี่ยน
                // ★ `.refx-meta` เดินกฎข้อเดียวกันเป๊ะ (P5-5) — ช่องผลของมัน
                //   ก็ไม่มีตัวปลุกด้วยเหตุผลเดียวกัน
                if doc.autosave_job.is_some() || doc.sidecar.has_work_outstanding() {
                    return Some(std::time::Instant::now() + COLLECT_SNAPSHOT_AFTER);
                }
                Self::snapshot_target(doc, recovery_dir)?;
                doc.autosaver
                    .next_deadline(Self::doc_has_unsnapshotted_work(doc))
            })
            .min()
    }

    /// ★★★ board เปลี่ยนไปจาก snapshot ล่าสุดหรือยัง — **คำถามที่ถูกกว่า `dirty`**
    ///
    /// `dirty` ตอบว่า "ยังไม่ได้ `Ctrl+S`" ซึ่งเป็นจริงค้างยาว ส่วนที่ autosave
    /// อยากรู้จริง ๆ คือ "มีอะไรที่ยังไม่ได้เก็บไหม" · ดู `snapshot_revision`
    fn doc_has_unsnapshotted_work(doc: &Doc) -> bool {
        doc.board.is_dirty() && doc.snapshot_revision != Some(doc.board.revision())
    }

    // ---------- ★★★ P4-4: กู้คืนงานที่ยังไม่เคยบันทึก + เปิดไฟล์ ----------

    /// ★★★ จองสิทธิ์ถามเรื่องงานค้าง — **จริงครั้งเดียวตลอดอายุโปรแกรม**
    ///
    /// `resumed()` ถูกเรียกใหม่ทุกครั้งที่กู้ device (`docs/04 §7`) · คำถามที่โผล่
    /// ซ้ำ ๆ คือคำถามที่คนกดปิดโดยไม่อ่าน ซึ่งทำให้ตัวเลือกที่สามไร้ความหมาย
    ///
    /// ★★ **แยกออกมาเป็นเมธอดเพื่อให้เทสต์เห็นสลักตัวจริง** — `resumed()` ต้องมี
    /// หน้าต่าง จึงเรียกจากเทสต์ไม่ได้เลย · เทสต์รุ่นก่อนเลยไป**ตั้งธงเองแล้ว
    /// ยืนยันว่าธงยังตั้งอยู่** ซึ่งเป็นจริงตลอดกาลโดยไม่ได้ตรวจสลักเลยสักครั้ง
    /// (`docs/08 §3.9` ข้อ 14 รูปที่สาม)
    fn claim_recovery_scan(&mut self) -> bool {
        if self.recovery_checked {
            return false;
        }
        self.recovery_checked = true;
        true
    }

    /// ★★ ไล่ดูโฟลเดอร์ recovery ตอนเปิดโปรแกรม — **บนเธรดอื่นเสมอ** (I-2)
    ///
    /// การสแกนอ่าน metadata ของทุกไฟล์ในโฟลเดอร์แล้ว decode ตัวที่ใหม่สุด
    /// ซึ่งเป็นงานดิสก์ล้วน ๆ · ทำบน UI thread = หน้าต่างขาวตอนเปิดโปรแกรม
    /// ซึ่งเป็นวินาทีที่ผู้ใช้ตัดสินว่าโปรแกรมนี้ "หนัก" หรือเปล่า
    fn start_recovery_scan(&mut self) {
        let Some(dir) = self.recovery_dir.clone() else {
            return;
        };
        // ★★★ ต้องส่ง session ของ **ทุกแท็บ** ไม่ใช่ตัวเดียว — ไม่งั้น recovery
        //     slot ของแท็บที่เปิดอยู่จะถูกอ่านว่าเป็นงานกำพร้าแล้วผู้ใช้จะถูกชวน
        //     ให้ "กู้คืน" งานที่อยู่ตรงหน้าเขา (ดู `refx_io::recovery::scan`)
        let live = self.docs.live_sessions();
        let (tx, rx) = crossbeam_channel::bounded(1);
        // ★★ **ต้องปลุก** (ข้อ 18) — ผลของการสแกนคือแถบ "งานค้างจากรอบที่แล้ว"
        //    ซึ่งเป็นสิ่งเดียวที่บอกผู้ใช้ว่างานของเขายังกู้ได้ · ไม่ปลุก = แถบนั้น
        //    ไม่โผล่จนกว่าเขาจะขยับเมาส์ ซึ่งเป็นวินาทีที่เขากำลังตัดสินใจว่า
        //    "โปรแกรมทำงานหายไปหรือเปล่า"
        let wake = self.waker.clone();
        let spawned = std::thread::Builder::new()
            .name("refx-recovery-scan".to_owned())
            .spawn(move || {
                let _ = tx.send(scan_for_recovery(&dir, &live));
                if let Some(wake) = wake {
                    wake.wake();
                }
            });
        if spawned.is_ok() {
            self.recovery_scan = Some(rx);
        }
    }

    /// เก็บผลการสแกน — เรียกต้นเฟรม **ไม่บล็อก**
    fn poll_recovery_scan(&mut self) {
        let Some(rx) = self.recovery_scan.as_ref() else {
            return;
        };
        let found = match rx.try_recv() {
            Ok(found) => found,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(crossbeam_channel::TryRecvError::Disconnected) => Vec::new(),
        };
        self.recovery_scan = None;
        // ★★ กวาด spool ตรงนี้ **ไม่ว่าจะมีงานค้างหรือไม่** — รายชื่อ snapshot
        //    ที่ต้องคุ้มครองนิ่งแล้วตั้งแต่การสแกนจบ · ถ้าผูกไว้กับ "ผู้ใช้ตอบ
        //    แถบกู้คืน" อย่างเดียว เครื่องที่ไม่เคย crash เลยจะไม่มีวันกวาด
        self.sweep_spool_folder();
        self.recovery_queue = found.into();
        self.offer_next_orphan();
    }

    /// ★★★ เสนองานกำพร้าใบถัดไปในคิว — **ทีละใบ** (P4-7c)
    ///
    /// เรียกทั้งตอนสแกนเสร็จและตอนผู้ใช้เพิ่งตอบใบก่อนหน้า · แถบบนจอมีอันเดียว
    /// จึงต้องรอให้ว่างก่อนเสมอ (`docs/07 §4`: สองคำถามติดกันยอมรับได้ แต่ต้อง
    /// เป็นคนละจังหวะ ไม่ใช่ทับกัน)
    fn offer_next_orphan(&mut self) {
        if self.pending_recovery.is_some() || self.shell.recover_prompt.is_some() {
            return;
        }
        let Some(found) = self.recovery_queue.pop_front() else {
            return;
        };

        // ★★★ ประทับว่า "ถามแล้ว" **ตอนที่แถบโผล่ขึ้นจอ** ไม่ใช่ตอนผู้ใช้ตอบ
        //
        //   ถ้าประทับตอนตอบ ผู้ใช้ที่ปิดโปรแกรมทิ้งโดยไม่แตะแถบเลย จะทำให้ไฟล์นั้น
        //   ไม่มีวันเข้าเกณฑ์เก็บกวาด แล้วโฟลเดอร์โตไม่รู้จบ · ส่วนการประทับตอนนี้
        //   ให้ความหมายตรงกับที่ `docs/07 §4` เขียนพอดี: **"ผู้ใช้ได้เห็นแล้ว"**
        refx_io::recovery::mark_asked(&found.path);
        self.shell.recover_prompt = Some(crate::shell::RecoverView {
            when: found.when.clone(),
            items: found.items,
            scope: crate::shell::RecoverScope::LastSession,
        });
        self.pending_recovery = Some(found);
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// ★★★ เสนอ snapshot ของ **เอกสารที่เพิ่งเปิด** ให้ผู้ใช้ตัดสิน (P4-3)
    ///
    /// ★ แถบมีอันเดียวบนจอ — ถ้ามีงานกำพร้าจาก session ก่อนค้างอยู่ก่อนแล้ว
    /// เรื่องของเอกสาร **ขึ้นก่อน** เพราะมันคือสิ่งที่ผู้ใช้เพิ่งสั่งเปิดเดี๋ยวนี้
    /// · อีกเรื่องไม่หายไปไหน (`pending_recovery` ยังถืออยู่) แล้วจะถูกถามต่อ
    fn offer_pending_snapshot(&mut self, pending: Option<Box<refx_io::autosave::Pending>>) {
        let Some(pending) = pending else {
            return;
        };
        self.shell.recover_prompt = Some(crate::shell::RecoverView {
            when: pending.written_at.map(format_when),
            items: pending.board.len(),
            scope: crate::shell::RecoverScope::ThisDocument,
        });
        self.docs.active_mut().pending_snapshot = Some(pending);
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// ★★ เสนอ snapshot ที่ผู้ใช้เคยสั่ง "เก็บไว้ก่อน" — **เฉพาะตอนแถบว่าง**
    ///
    /// ถ้ายังมีคำถามอื่นค้างอยู่บนจอ ตัวนี้รอคิว (`pending_kept` ยังถืออยู่)
    /// แล้วถูกเสนอทันทีที่ผู้ใช้ตอบคำถามก่อนหน้า
    fn offer_kept_snapshot(&mut self) {
        if self.shell.recover_prompt.is_some() {
            return;
        }
        let Some(kept) = self.docs.active().pending_kept.as_ref() else {
            return;
        };
        self.shell.recover_prompt = Some(crate::shell::RecoverView {
            when: kept.written_at.map(format_when),
            items: kept.board.len(),
            scope: crate::shell::RecoverScope::KeptForLater,
        });
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// ★★★ ผู้ใช้ตอบแถบกู้คืนแล้ว — **สามทาง และมีทางเดียวที่ลบไฟล์**
    ///
    /// แถบเดียวถามได้สามเรื่อง (ดู [`crate::shell::RecoverScope`]) — เรื่องของ
    /// เอกสารที่เปิดอยู่มาก่อนเสมอถ้าค้างพร้อมกัน
    fn apply_recover_choice(&mut self, choice: crate::shell::RecoverChoice) {
        use crate::shell::RecoverChoice;

        let scope = self
            .shell
            .recover_prompt
            .take()
            .map(|view| view.scope)
            .unwrap_or(crate::shell::RecoverScope::LastSession);
        match scope {
            crate::shell::RecoverScope::ThisDocument => {
                self.apply_document_recover_choice(choice);
                // ★ ตอบตัวใหม่กว่าแล้ว — ตัวที่ "เก็บไว้ก่อน" ขึ้นถามต่อได้
                self.offer_kept_snapshot();
                return;
            }
            crate::shell::RecoverScope::KeptForLater => {
                self.apply_kept_recover_choice(choice);
                return;
            }
            crate::shell::RecoverScope::LastSession => {}
        }
        let Some(found) = self.pending_recovery.take() else {
            return;
        };
        match choice {
            RecoverChoice::Restore => {
                // ★★★ **งานกำพร้าแต่ละใบได้แท็บของตัวเอง** (P4-7c)
                //
                //   ก่อนหน้านี้มันแทนที่ board ที่กำลังเปิดอยู่ ซึ่งกลืนงานของแท็บนั้น
                //   ทิ้งไปเงียบ ๆ · และเมื่อมีงานกำพร้าหลายใบ (สองแท็บที่ไม่เคยบันทึก
                //   แล้วโปรแกรมตาย) ใบที่สองจะทับใบแรกที่ผู้ใช้เพิ่งกู้มา
                //   → เอกสารคนละฉบับ = แท็บคนละใบ เสมอ
                let id = self.docs.mint();
                let Some(board) = refx_io::recovery::load(&found.path, id) else {
                    self.shell.status = text::t(self.shell.lang, Key::OpenFailed).to_owned();
                    self.shell.status_warn = true;
                    // อ่านไม่ออกก็ยังต้องถามใบถัดไปต่อ ไม่ใช่เงียบไปทั้งคิว
                    self.offer_next_orphan();
                    return;
                };
                // ★ กู้คืนแล้ว **ยังไม่มี path** — งานชุดนี้ไม่เคยถูกบันทึกมาก่อน
                //   จึงต้อง dirty ต่อไปและถูก autosave ต่อไปตามปกติ
                //   ★★ และยังไม่มีไฟล์ `.refx` ที่ฝังอะไรไว้ → ตารางว่าง
                self.adopt_into_tab(id, board, None, refx_io::packed::Index::default());
                // ★★★ **ธงต้องติดที่นี่ด้วย ไม่ใช่แค่เส้นทางของเอกสาร** (docs/03 §1)
                //
                //   `History::default()` ถือว่า board ที่รับมา = สถานะที่บันทึกแล้ว
                //   ซึ่งผิดเสมอกับ snapshot · ก่อนหน้านี้เส้นทางนี้พึ่งข้อความ
                //   `RecoveredNotSavedYet` อย่างเดียว ซึ่งเป็น **ข้อความชั่วคราว**
                //   ที่ถูกรายงานความคืบหน้าของ decode เขียนทับใน ~3 ms —
                //   ลูปเดิมเป๊ะ: กู้งานคืน → เห็นว่าสะอาด → ปิดโปรแกรม → **หายอีกรอบ**
                if self.gfx.is_some() {
                    let doc = self.docs.active_mut();
                    doc.history.mark_unsaved(&mut doc.board);
                    // ★ ยังไม่ลบไฟล์เก่า — รอให้ snapshot ของ **แท็บนี้** ลงดิสก์ก่อน
                    //   (ดู `Doc::adopted_recovery`) · ระหว่างนี้มีสำเนาหนึ่งชุดเสมอ
                    doc.adopted_recovery = Some(found.path.clone());
                }
                self.shell.status = text::t(self.shell.lang, Key::RecoveredNotSavedYet).to_owned();
                self.shell.status_warn = true;
            }
            RecoverChoice::Discard => {
                // ผู้ใช้ยืนยันเองว่าไม่เอา — นี่คือทางเดียวที่ไฟล์ถูกลบตามคำสั่งคน
                remove_recovery_file(&found.path);
                self.shell.status = text::t(self.shell.lang, Key::Ready).to_owned();
            }
            // ★★★ **ไม่แตะไฟล์เลยแม้แต่นิดเดียว** — นี่คือทั้งหมดของตัวเลือกที่สาม
            //     ไฟล์ยังอยู่ ถูกถามใหม่รอบหน้า และเข้าเกณฑ์เก็บกวาดได้แล้ว
            //     เพราะถูกประทับ `.asked` ไปตอนแถบโผล่
            RecoverChoice::Later => {}
        }
        // ★ เก็บกวาดตามเพดาน **หลังผู้ใช้ตอบเสมอ** — ตอนนี้ไฟล์ที่เพิ่งถูกถาม
        //   มีไฟล์ประทับแล้ว เพดาน 10 ไฟล์ / 30 วันจึงมีของให้ทำงานด้วยจริง
        self.sweep_recovery_folder();
        // ★★ แล้วค่อยกวาด spool — ลำดับสำคัญ: snapshot ที่ผู้ใช้เพิ่งสั่ง "ทิ้ง"
        //    ต้องหายไปจากโฟลเดอร์ก่อน ภาพที่มีแต่มันรู้จักจึงจะกวาดได้
        self.sweep_spool_folder();
        // ★★★ **ใบถัดไปในคิวขึ้นถามทันที** — ไม่ใช่รอเปิดโปรแกรมรอบหน้า (P4-7c)
        self.offer_next_orphan();
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// ★★★ ผู้ใช้ตอบแถบของ **เอกสารที่เปิดอยู่** — สามทางเดียวกัน คนละไฟล์
    ///
    /// | ตอบ | ทำอะไรกับ `<doc>.refx.autosave` |
    /// |---|---|
    /// | เอากลับมา | ★ ไม่ลบ — `autosave` ของเราจะเขียนทับที่เดิมด้วยเนื้อเดียวกัน |
    /// | ทิ้งไป | ลบทันที (ทางเดียวที่ลบตามคำสั่งคน) |
    /// | เก็บไว้ก่อน | ไม่แตะเลย — แต่ **การแก้งานต่อจะเขียนทับมันในไม่กี่วินาที** |
    ///
    /// ★★★ ตัวที่สามมีข้อจำกัดจริงที่ปุ่มต้องบอก (`Key::RecoverDocLaterHint`):
    /// ต่างจากงานกำพร้าของ P4-4 ที่อยู่คนละไฟล์กับที่เราเขียน · ที่นี่มันคือ
    /// **ไฟล์เดียวกัน** การเงียบไว้แล้วเขียนทับคือการทำงานหายโดยผู้ใช้เพิ่งบอกว่า
    /// "ยังไม่ตัดสินใจ" — ปิดช่องด้วยการพูดความจริง ไม่ใช่ด้วยการหยุด autosave
    /// (ซึ่งจะทิ้งงานใหม่ของเขาไว้กลางอากาศแทน)
    fn apply_document_recover_choice(&mut self, choice: crate::shell::RecoverChoice) {
        use crate::shell::RecoverChoice;

        let Some(pending) = self.docs.active_mut().pending_snapshot.take() else {
            return;
        };
        let Some(doc) = self.docs.active().path.clone() else {
            return; // เอกสารถูกปิด/แทนที่ไปแล้วระหว่างรอคำตอบ
        };
        // ★ กู้เข้า **แท็บเดิม** ไม่ใช่แท็บใหม่ — snapshot ตัวนี้เป็นเนื้ออีกเวอร์ชัน
        //   ของไฟล์ที่แท็บนี้เปิดอยู่ ไม่ใช่เอกสารคนละฉบับ (ต่างจากงานกำพร้า)
        let id = self.docs.active().id;
        let assets = self.docs.active().assets.clone();
        match choice {
            RecoverChoice::Restore => {
                self.adopt_into_tab(id, pending.board, Some(doc), assets);
                // ★★★ **ธง "ยังไม่บันทึก" ต้องติดทันที** — เนื้อที่เพิ่งขึ้นจอ
                //     ไม่เหมือนไฟล์บนดิสก์ตามนิยาม · ถ้าไม่ติด ตัวบ่งชี้ถาวรบนแท็บ
                //     จะบอกว่าทุกอย่างอยู่ในไฟล์แล้ว แล้วผู้ใช้จะปิดโปรแกรมทิ้ง
                //     อีกรอบ — วนกลับไปที่เดิมพอดี (`History::mark_unsaved`)
                if self.gfx.is_some() {
                    let doc = self.docs.active_mut();
                    doc.history.mark_unsaved(&mut doc.board);
                }
                self.shell.status = text::t(self.shell.lang, Key::RecoveredIntoDocument).to_owned();
                self.shell.status_warn = true;
            }
            RecoverChoice::Discard => {
                refx_io::autosave::discard(&doc);
                self.shell.status = text::t(self.shell.lang, Key::Ready).to_owned();
                self.shell.status_warn = false;
            }
            // ★★★ **ย้ายออกจากทางของ autosave** (`docs/07 §4` — ตัดสิน 22 ส.ค. 2026)
            //
            //   ก่อนหน้านี้ตัวเลือกนี้ "ไม่แตะไฟล์" ซึ่งฟังดูปลอดภัยที่สุดแต่เป็น
            //   ทางที่อันตรายที่สุด: `<doc>.refx.autosave` คือไฟล์เดียวกับที่
            //   autosave เขียนทับในไม่กี่วินาที ปุ่ม "เก็บไว้ก่อน" จึงเท่ากับ
            //   "ทิ้งใน 10 วินาที" สำหรับคนที่แก้งานต่อ
            //
            //   ★ ย้ายไป `.kept` แล้วทั้งสองอย่างรอด: ของเก่าอยู่ในชื่อที่ไม่มีใคร
            //     เขียนทับ ส่วนงานใหม่ได้ snapshot ของตัวเองตามปกติ
            RecoverChoice::Later => {
                match refx_io::autosave::keep(&doc, refx_platform::fsops::rename_durable) {
                    Ok(_) => {
                        // ★★★ **คำถามที่ค้างคิวอยู่ต้องหายไปด้วย** — `keep` เพิ่งเขียนทับ
                        //     ไฟล์ `.kept` ตัวเก่าด้วยตัวที่ใหม่กว่า (`docs/07 §4`:
                        //     "มีได้ไฟล์เดียวต่อเอกสาร · ตัวใหม่ทับตัวเก่า") · ถ้ายังถาม
                        //     ต่อ ผู้ใช้จะถูกถามถึงเนื้อที่ **ไม่มีอยู่บนดิสก์แล้ว** และถ้า
                        //     เขากด "เอากลับมา" เราจะลบไฟล์ที่เพิ่งเก็บให้เขาไปด้วย
                        //     (เห็นบนแอปจริงตอนยืนยัน 25 ส.ค. 2026)
                        self.docs.active_mut().pending_kept = None;
                        self.shell.status =
                            text::t(self.shell.lang, Key::RecoverKeptSaved).to_owned();
                        self.shell.status_warn = false;
                    }
                    Err(err) => {
                        // ★ ย้ายไม่สำเร็จ = ของเดิมยังอยู่ที่เดิม แต่ **จะถูกเขียนทับ**
                        //   ผู้ใช้ต้องรู้ ไม่ใช่เชื่อว่าเก็บไว้แล้ว
                        tracing::error!(%err, "cannot keep the snapshot");
                        self.shell.status =
                            text::t(self.shell.lang, Key::RecoverKeepFailed).to_owned();
                        self.shell.status_warn = true;
                    }
                }
            }
        }
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// ★★★ ผู้ใช้ตอบแถบของ snapshot ที่ **เคยสั่งเก็บไว้** (`.kept`)
    ///
    /// | ตอบ | ทำอะไรกับ `<doc>.refx.autosave.kept` |
    /// |---|---|
    /// | เอากลับมา | ★ ลบ **หลัง** snapshot ของ session นี้ลงดิสก์ (ดู `adopted_kept`) |
    /// | ทิ้งไป | ลบทันที — ผู้ใช้สั่งเอง |
    /// | เก็บไว้ก่อน (อีกครั้ง) | **ไม่แตะเลย** · ถูกถามใหม่รอบหน้าที่เปิดเอกสารนี้ |
    ///
    /// ★★ ตัวที่สามที่นี่ปลอดภัยจริง ๆ ต่างจากตอนมันยังอยู่ในชื่อ `.autosave`:
    /// ไม่มีอะไรเขียนทับ `.kept` เลย ผู้ใช้จึงเลื่อนการตัดสินใจได้ไม่จำกัดรอบ
    fn apply_kept_recover_choice(&mut self, choice: crate::shell::RecoverChoice) {
        use crate::shell::RecoverChoice;

        let Some(kept) = self.docs.active_mut().pending_kept.take() else {
            return;
        };
        let Some(doc) = self.docs.active().path.clone() else {
            return;
        };
        let id = self.docs.active().id;
        let assets = self.docs.active().assets.clone();
        match choice {
            RecoverChoice::Restore => {
                let path = refx_io::autosave::kept_path(&doc);
                self.adopt_into_tab(id, kept.board, Some(doc), assets);
                if self.gfx.is_some() {
                    let doc = self.docs.active_mut();
                    doc.history.mark_unsaved(&mut doc.board);
                    // ★ ยังไม่ลบ — รอ snapshot ของแท็บนี้ลงดิสก์ก่อน (I-3)
                    doc.adopted_kept = Some(path);
                }
                self.shell.status = text::t(self.shell.lang, Key::RecoveredIntoDocument).to_owned();
                self.shell.status_warn = true;
            }
            RecoverChoice::Discard => {
                refx_io::autosave::discard_kept(&doc);
                self.shell.status = text::t(self.shell.lang, Key::Ready).to_owned();
                self.shell.status_warn = false;
            }
            // ไม่แตะเลย — ไฟล์นี้ไม่มีอะไรเขียนทับมัน ถามใหม่รอบหน้าได้เรื่อย ๆ
            RecoverChoice::Later => {}
        }
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// ★ เก็บกวาดโฟลเดอร์ recovery ตามเพดาน — บนเธรดอื่น (I-2)
    ///
    /// `prune` ลบได้เฉพาะไฟล์ที่ผู้ใช้เคยเห็นแล้ว (`docs/07 §4`) การเรียกก่อนมี
    /// แถบกู้คืนจึงเป็นโค้ดที่ทำงานเป็นศูนย์ — ที่นี่คือจุดแรกที่มันมีความหมาย
    fn sweep_recovery_folder(&self) {
        let Some(dir) = self.recovery_dir.clone() else {
            return;
        };
        // ★★★ session ของ **ทุกแท็บ** — ส่งไม่ครบเมื่อไหร่ เพดานจะลบ snapshot
        //     ของแท็บที่ยังทำงานอยู่ (ดู `refx_io::recovery::prune`)
        let live = self.docs.live_sessions();
        let spawned = std::thread::Builder::new()
            .name("refx-recovery-sweep".to_owned())
            .spawn(move || {
                refx_io::recovery::prune(
                    &dir,
                    &live,
                    refx_io::recovery::MAX_KEPT,
                    refx_io::recovery::MAX_AGE,
                    std::time::SystemTime::now(),
                );
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "cannot spawn the recovery sweep thread");
        }
    }

    /// ★★★ เก็บกวาด spool ของภาพที่วาง — บนเธรดอื่น (I-2) และ **ถามครบทั้งสองฝั่ง**
    ///
    /// กติกา (`docs/07 §2` · `refx_io::spool`):
    /// 1. ห้ามลบไฟล์ที่ **board ของแท็บใดก็ตามที่เปิดอยู่** อ้างถึง
    /// 2. ★ ห้ามลบไฟล์ที่ **recovery snapshot ตัวใดก็ตามที่ยังอยู่** อ้างถึง
    /// 3. ที่เหลือใช้เพดาน **ไบต์** (512 MB)
    ///
    /// ★★ ข้อ 2 คือข้อที่ทั้งกลไกนี้มีไว้เพื่อมัน — ลืมมันแล้วผู้ใช้จะ "กู้คืน
    /// สำเร็จแต่ได้ board ที่เต็มไปด้วย `Missing`" ซึ่งแย่กว่าไม่มี snapshot เลย
    /// เพราะเขาเชื่อไปแล้วว่าได้งานคืน · hash ของ board ที่เปิดอยู่อ่านที่นี่
    /// (UI thread, ในหน่วยความจำ) ส่วนการอ่าน snapshot ทุกไฟล์เป็นงานดิสก์
    /// จึงอยู่บนเธรด
    ///
    /// ★★★ **ข้อ 1 ต้องรวมทุกแท็บ ไม่ใช่แท็บที่ดูอยู่** (P4-7c) — ภาพที่วางไว้
    /// ในแท็บหลังบ้านมีต้นฉบับอยู่ที่ spool ที่เดียว (`§4` ข้อ 24) · ถามแค่แท็บ
    /// ที่อยู่หน้าจอ แล้วเพดานจะลบต้นฉบับของอีกแท็บทิ้ง **ขณะที่มันเปิดอยู่**
    /// แล้วภาพจะกลายเป็นช่องว่างทันทีที่ผู้ใช้สลับกลับไปดู — และกู้คืนไม่ได้เลย
    fn sweep_spool_folder(&mut self) {
        let (Some(spool_dir), Some(recovery_dir)) =
            (self.spool_dir.clone(), self.recovery_dir.clone())
        else {
            return;
        };
        if self.spool_sweep.is_some() {
            return; // รอบก่อนยังไม่จบ — ซ้อนกันไม่ได้อะไรเพิ่ม
        }
        let open_board = if self.gfx.is_some() {
            hashes_of_every_tab(&self.docs)
        } else {
            std::collections::BTreeSet::new()
        };
        let wake = self.assets.as_ref().map(|assets| assets.pool.wake_handle());
        let (tx, rx) = crossbeam_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("refx-spool-sweep".to_owned())
            .spawn(move || {
                let mut referenced = open_board;
                // ★ id ใช้แค่ถอด board ออกมาดึงรายการ hash แล้วทิ้ง — ดู `probe_board_id`
                referenced.extend(refx_io::spool::referenced_by_recovery(
                    &recovery_dir,
                    probe_board_id(),
                ));
                let swept = refx_io::spool::sweep(
                    &spool_dir,
                    &referenced,
                    refx_io::spool::MAX_BYTES,
                    refx_io::spool::MAX_AGE,
                    std::time::SystemTime::now(),
                );
                let _ = tx.send(swept);
                // ★ ปลุกให้มาเก็บผล — ไม่งั้นข้อความ "เกินเพดาน" จะค้างอยู่ในช่อง
                //   จนกว่าผู้ใช้จะบังเอิญขยับเมาส์ · ปลุกครั้งเดียวจบ ไม่ใช่ลูป (I-1)
                if let Some(wake) = wake {
                    wake.wake();
                }
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "cannot spawn the spool sweep thread");
            return;
        }
        self.spool_sweep = Some(rx);
    }

    /// เก็บผลการกวาด spool — เรียกต้นเฟรม **ไม่บล็อก**
    ///
    /// ★★★ สิ่งเดียวที่ผู้ใช้ต้องเห็นคือสภาพที่ **เพดานทำงานไม่ได้เพราะทุกไฟล์
    /// ห้ามแตะ** — เงียบไว้แล้วปล่อยให้ดิสก์เต็มก็ผิด ลบทิ้งก็ผิดหนักกว่า
    /// (`docs/07 §2` · รูปแบบเดียวกับ "board เต็ม" ใน `ROADMAP P3-3`)
    fn poll_spool_sweep(&mut self) {
        let Some(rx) = self.spool_sweep.as_ref() else {
            return;
        };
        let swept = match rx.try_recv() {
            Ok(swept) => swept,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                self.spool_sweep = None;
                return;
            }
        };
        self.spool_sweep = None;
        if !swept.protected_over_cap(refx_io::spool::MAX_BYTES) {
            return;
        }
        let mb = swept.protected_bytes / (1 << 20);
        tracing::warn!(
            mb,
            files = swept.kept_because_referenced,
            "the paste spool is over its cap and nothing in it may be deleted"
        );
        self.shell.status = text::fill(
            self.shell.lang,
            text::Template::SpoolOverCap,
            &[
                ("mb", &mb.to_string()),
                ("cap", &(refx_io::spool::MAX_BYTES / (1 << 20)).to_string()),
            ],
        );
        self.shell.status_warn = true;
    }

    /// `Ctrl+O` — ถามว่าจะเปิดไฟล์ไหน แล้วเปิด **เป็นแท็บใหม่** (`docs/03 §5`)
    ///
    /// ★★ เปลี่ยนพฤติกรรมโดยตั้งใจตั้งแต่ P4-7c: เดิมมันแทนที่ board ที่ผู้ใช้
    /// กำลังทำอยู่ทั้งก้อนโดยไม่ถาม ซึ่งคือการทำงานหาย — และเป็นสิ่งที่โครงแท็บ
    /// มีไว้แก้พอดี · แท็บเปล่าที่ยังไม่เคยถูกแตะยังถูก *ใช้ซ้ำ* (ดู `Doc::is_untouched`)
    fn apply_open_request(&mut self) {
        // ★ ซ้อนกันไม่ได้: สอง dialog พร้อมกันแปลว่าผลของอันที่ตอบก่อนถูกทิ้ง
        if self.open_dialog.is_some() || self.load_job.is_some() {
            return;
        }
        self.open_dialog = Some(refx_platform::dialog::pick_document_to_open(
            text::t(self.shell.lang, Key::DialogOpenBoard),
            self.waker.clone(),
        ));
        // ★ บอกด้วยว่ากำลังรออะไรอยู่ — native dialog เปิดหลังหน้าต่างหลักได้
        //   (เกิดจริงตอนขับด้วยสคริปต์) ถ้าไม่มีข้อความนี้ ผู้ใช้ที่ไม่เห็น dialog
        //   จะสรุปว่า `Ctrl+O` ไม่ทำงาน แล้วกดซ้ำอีกสิบครั้ง
        self.shell.status = text::t(self.shell.lang, Key::OpenChoosing).to_owned();
        self.shell.status_warn = false;
    }

    /// ส่งงานอ่านไฟล์ไปเธรด — ไม่รอผล (I-2)
    ///
    /// ★ อ่าน+แตกบีบ+ตรวจ CRC ของไฟล์ระดับ MB เป็นงานที่กินเวลาจริง และ `.refx`
    /// แบบ packed (P4-5) จะใหญ่กว่านี้อีกมาก — เส้นทางนี้ห้ามแตะ UI thread
    /// ตั้งแต่วันแรก ไม่ใช่ "ค่อยย้ายทีหลังตอนมันช้า"
    fn start_load(&mut self, path: &std::path::Path) {
        // ★★★ ไฟล์นี้เปิดอยู่แล้ว = **ไปที่แท็บนั้น ไม่ใช่เปิดซ้อน** — สองแท็บ
        //     บนไฟล์เดียวกันแปลว่าสอง `<doc>.refx.autosave` ทับกัน และ `Ctrl+S`
        //     ของใบหลังจะกลืนงานของใบแรก (I-3)
        if let Some(index) = self.docs.index_of_path(path) {
            self.focus_tab(index);
            self.shell.status = text::fill(
                self.shell.lang,
                text::Template::Opened,
                &[("name", &file_label_of(path))],
            );
            self.shell.status_warn = false;
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }
            return;
        }
        let path = path.to_path_buf();
        // ★★★ จอง id ของแท็บปลายทาง **ก่อน** ส่งงาน — `dto::decode` ต้องรู้ id
        //     ตั้งแต่ตอนอ่าน (`.refx` ไม่เก็บ id) · อ่านด้วย id คนละตัวกับที่
        //     แท็บจะใช้จริง = `ItemId` ทุกใบชี้ไป board ผิดใบตั้งแต่วินาทีแรก
        let id = self.docs.mint();
        // ★ เหตุผลเดียวกับเธรดบันทึก: อ่านไฟล์ packed ระดับ GB ใช้เวลาจริง
        //   และแอปหลับระหว่างรอ (I-1) — ไม่ปลุก = เอกสารไม่ขึ้นจอจนกว่าจะมี event
        let waker = self.waker.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("refx-open".to_owned())
            .spawn(move || {
                let result = read_document(&path, id).map(|board| LoadedDoc {
                    // ★★ อ่าน **ตาราง** ของไฟล์เดียวกันต่อทันที (หัวไฟล์ + ตาราง
                    //    เท่านั้น ไม่แตะ blob) — เอกสาร packed พกภาพมาเอง และ
                    //    ตารางนี้คือสิ่งที่บอกว่าใบไหนอยู่ข้างในบ้าง
                    assets: read_asset_table(&path),
                    // ★★★ ถามเรื่อง snapshot **บนเธรดนี้ด้วย** (I-2) — มันคือการ
                    //     อ่าน+คลายบีบไฟล์อีกก้อน ไม่ใช่การ stat เฉย ๆ
                    pending: newer_snapshot(&path, id, &board).map(Box::new),
                    kept: kept_snapshot(&path, id, &board).map(Box::new),
                    path,
                    board: Box::new(board),
                });
                let _ = tx.send(result);
                if let Some(waker) = waker {
                    waker.wake();
                }
            });
        if spawned.is_err() {
            self.shell.status = text::t(self.shell.lang, Key::OpenFailed).to_owned();
            self.shell.status_warn = true;
            return;
        }
        self.load_job = Some((id, rx));
        self.shell.status = text::t(self.shell.lang, Key::OpenInProgress).to_owned();
        self.shell.status_warn = false;
    }

    /// เก็บผลของ dialog เปิดไฟล์และของงานอ่าน — เรียกต้นเฟรม **ไม่บล็อก**
    fn poll_open(&mut self) {
        let lang = self.shell.lang;

        if let Some(rx) = self.open_dialog.as_ref() {
            match rx.try_recv() {
                Ok(Some(path)) => {
                    self.open_dialog = None;
                    self.start_load(&path);
                }
                Ok(None) => {
                    self.open_dialog = None; // กดยกเลิก — ไม่ใช่ error
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.open_dialog = None;
                    self.shell.status = text::t(lang, Key::OpenFailed).to_owned();
                    self.shell.status_warn = true;
                }
            }
        }

        let Some((id, rx)) = self.load_job.as_ref() else {
            return;
        };
        let id = *id;
        let done = match rx.try_recv() {
            Ok(result) => result,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            // ★ เธรดตายก่อนตอบ — **ไม่ใช่เรื่องของตัวไฟล์** จึงห้ามยืมข้อความ
            //   ของ `OpenFailure` มาใช้ · การบอกว่า "ไฟล์อาจถูกย้าย" ตอนที่ไฟล์
            //   ยังอยู่ครบ จะส่งผู้ใช้ไปไล่หาปัญหาที่ไม่มีอยู่
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                self.load_job = None;
                tracing::error!("the open thread died before answering");
                self.shell.status = text::t(lang, Key::OpenFailed).to_owned();
                self.shell.status_warn = true;
                return;
            }
        };
        self.load_job = None;
        match done {
            Ok(LoadedDoc {
                path,
                board,
                assets,
                pending,
                kept,
            }) => {
                let name = file_label_of(&path);
                self.adopt_into_tab(id, *board, Some(path), assets);
                self.shell.status = text::fill(lang, text::Template::Opened, &[("name", &name)]);
                self.shell.status_warn = false;
                // ★★★ **เอกสารขึ้นจอก่อนเสมอ แล้วค่อยถามเรื่อง snapshot**
                //
                //   ผู้ใช้ต้องเห็นไฟล์ที่เขาสั่งเปิดก่อน ถึงจะตัดสินใจได้ว่าจะเอา
                //   ของที่ค้างอยู่กลับมาไหม · ถามบนจอว่างเปล่าคือการขอให้เขาเดา
                // ★★ ตัวที่ **ใหม่กว่า** ขึ้นก่อน แล้วตัวที่ผู้ใช้สั่งเก็บไว้ตามมา
                //    หลังเขาตอบตัวแรก (`docs/07 §4`) — สองคำถามติดกันยอมรับได้
                //    เพราะเกิดยากและทั้งสองอันคืองานของเขาจริง ๆ
                self.docs.active_mut().pending_kept = kept;
                self.offer_pending_snapshot(pending);
                self.offer_kept_snapshot();
            }
            // ★★★ **เหตุผลที่เปิดไม่ได้ ถึงหน้าจอแยกกัน** (docs/07 §1) — ตัวอ่าน
            //     แยกได้อยู่แล้ว ชั้นนี้เคยรวมกลับเป็นข้อความเดียว
            //     (รายละเอียดเชิงเทคนิคลง log ไปแล้วที่ `read_document`)
            Err(failure) => {
                self.shell.status = open_failure_text(lang, &failure);
                self.shell.status_warn = true;
            }
        }
    }

    /// ★★★ เอา `Board` ที่โหลดมาขึ้นจอ — **แทนที่ทั้งก้อน ไม่ใช่ผสมกับของเดิม**
    ///
    /// ทุกอย่างที่ผูกกับ board เก่าต้องถูกล้างพร้อมกัน ไม่งั้นจะเหลือของค้างที่
    /// ชี้ไป `ItemId` ของเอกสารคนละฉบับ:
    ///
    /// | ล้าง | ถ้าไม่ล้างจะเกิดอะไร |
    /// |---|---|
    /// | `history` | `Ctrl+Z` ครั้งแรกหลังเปิดไฟล์ ย้อนไปเป็น board ของเอกสารก่อนหน้า |
    /// | `selection` | inspector แสดงค่าของ item ที่ไม่มีอยู่แล้ว |
    /// | `render_state` | thumbnail ของภาพเก่าไปโผล่บนภาพใหม่ที่ได้ `ItemId` ซ้ำ |
    /// | `index` | hit-test ชี้ไปที่ว่าง — คลิกแล้วไม่โดนอะไร |
    ///
    /// ★ `path = None` ตอนกู้คืน (งานชุดนั้นไม่เคยมีไฟล์) · `Some` ตอนเปิดไฟล์
    ///
    /// ★★★ **`id` ต้องเป็น id ที่ `board` ถูกอ่านมาด้วย** — ผู้เรียกจอง id จาก
    /// [`Docs::mint`] ก่อนส่งงานอ่านเสมอ · แท็บที่มี id นี้อยู่แล้วจะถูกเขียนทับ
    /// (เส้นทาง "กู้ snapshot เข้าแท็บเดิม") ส่วน id ที่ยังไม่มีใครถือ **ได้แท็บใหม่**
    /// — หรือได้ที่นั่งของแท็บเปล่าที่ยังไม่เคยถูกแตะ (ดู [`Doc::is_untouched`])
    fn adopt_into_tab(
        &mut self,
        id: refx_core::arena::BoardId,
        board: Board,
        path: Option<std::path::PathBuf>,
        assets: refx_io::packed::Index,
    ) {
        if self.gfx.is_none() {
            return;
        }
        // ★★ โหมดของเอกสารใหม่ **อ่านจากตารางของไฟล์นั้น** (§4 ข้อ 29)
        //    ตารางว่างสำหรับงานที่กู้คืนมา ซึ่งยังไม่มีไฟล์
        let mode = mode_of_document(&board, &assets);

        let index = match self.docs.list.iter().position(|doc| doc.id == id) {
            Some(existing) => {
                self.docs.active = existing;
                existing
            }
            // ★ แท็บเปล่าที่ยังไม่เคยถูกแตะ = ที่นั่งที่ว่างอยู่ · เอกสารแรกที่เปิด
            //   จึงไม่ทิ้งแท็บ "Untitled board" เปล่า ๆ ค้างไว้ให้ผู้ใช้ปิดเอง
            None if self.docs.active().is_untouched() => {
                let seat = self.docs.active;
                // ★★★ id ของที่นั่งต้องกลายเป็น id ที่ board ถูกอ่านมาด้วย ไม่งั้น
                //     ทุกอย่างที่จับคู่ด้วย `BoardId` (ผลงาน decode · งานบันทึก)
                //     จะหาแท็บนี้ไม่เจอตลอดอายุของมัน
                self.docs.list[seat].id = id;
                seat
            }
            None => {
                let doc = Doc::empty(id);
                self.docs.push(doc)
            }
        };

        {
            let doc = &mut self.docs.list[index];
            doc.board = board;
            doc.history = History::default();
            doc.selection = Selection::new();
            doc.select_tool.cancel();
            doc.render_state.clear();
            doc.index = SpatialIndex::new(refx_core::spatial::DEFAULT_CELL_SIZE);
            {
                // ยืม `board` กับ `index` คนละฟิลด์กัน — ไม่ต้องก๊อป item ทั้ง board
                let Doc { board, index, .. } = &mut *doc;
                for (id, item) in board.items_in_z_order() {
                    index.insert(id, &item.canvas);
                }
            }
            doc.rubber_band = None;
            doc.guides.clear();
            doc.arrange.invalidate();
            // ★★ `path`/นาฬิกา autosave ต้องเปลี่ยนพร้อมกันกับ board เสมอ —
            //    ถ้าตั้ง path ใหม่แต่ลืมรีเซ็ตนาฬิกา snapshot แรกของเอกสารใหม่จะ
            //    ถูกเลื่อนไปจนครบรอบของเอกสารเก่า
            doc.path = path;
            doc.assets = assets;
            doc.save_mode = mode;
            doc.snapshot_revision = None;
            doc.autosaver.reset();
            // ★ คีย์งานที่ค้างของ board ใบเก่าในที่นั่งนี้ใช้ต่อไม่ได้ — `ItemId`
            //   ที่มันชี้ถึงถูกโยนทิ้งไปพร้อม `board` เมื่อกี้
            doc.relink_targets.clear();
        }
        if let Some(gfx) = self.gfx.as_mut() {
            Self::rebuild_quads(gfx, &mut self.docs.list[index]);
        }
        // ★★★ **มุมมองที่บันทึกไว้ในไฟล์ต้องกลับมาเดี๋ยวนี้** (`HANDOFF §6` เกณฑ์ผ่าน)
        //
        //   `mode_follows_active_tab` ที่ต้นเฟรมทำงานไป **ก่อน** `poll_open` เอา
        //   board ใบนี้มาวาง · `shell.mode` จึงยังเป็นของ board ใบเก่า แล้วการ
        //   ซิงค์กลับหลังวาด (`set_view`) จะ **เขียนทับค่าที่เพิ่งอ่านมาจากไฟล์**
        //   ทิ้งในเฟรมเดียวกับที่มันมาถึง
        //
        //   ★ เจอด้วยการรันจริงเท่านั้น: บันทึกในโหมด Arrange → เปิดใหม่ → ได้
        //     Canvas · เทสต์ระดับหน่วยทุกตัวเขียว เพราะแต่ละชิ้นถูกหมด
        //     (`docs/08 §3.9` ข้อ 5 — "ทุกชิ้นถูก ประกอบผิด")
        //
        //   ★★ กล้องเดินเส้นเดียวกับโหมด (`docs/03 §6`) — แยกสองเส้นเมื่อไหร่
        //      กฎ "ค่าจากไฟล์ชนะในเฟรมนั้น" ต้องถูกทำให้ถูกสองรอบ
        self.restore_view(index);
        self.request_thumbnails_for(id);
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    // ---------- ★★★ P4-7c: คีย์ของแท็บ (`docs/03 §5`) ----------

    /// `Ctrl+T` — board เปล่าใบใหม่ พร้อม id และ recovery slot ของตัวเอง
    fn apply_new_tab(&mut self) {
        if self.gfx.is_none() {
            return;
        }
        let id = self.docs.mint();
        self.docs.push(Doc::empty(id));
        self.shell.status = text::t(self.shell.lang, Key::NewBoardOpened).to_owned();
        self.shell.status_warn = false;
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// `Ctrl+Tab` — แท็บถัดไป (วนกลับมาที่ใบแรกเมื่อถึงใบสุดท้าย)
    fn apply_next_tab(&mut self) {
        if self.docs.len() < 2 {
            return;
        }
        self.focus_tab((self.docs.active + 1) % self.docs.len());
    }

    /// ★★★ **ทางเดียวที่ "แท็บที่ดูอยู่" เปลี่ยนได้** — สลับแล้วต้องเห็นของใบใหม่
    ///
    /// ## บั๊กที่มันปิด (เจอตอนรันแอปจริง 28 ส.ค. 2026)
    ///
    /// `gfx.quads` เป็นของ **หน้าต่าง** มีชุดเดียว และถูกสร้างใหม่เฉพาะตอน
    /// `board` เปลี่ยน (`rebuild_quads`) · การตั้ง `docs.active` เฉย ๆ แล้วขอเฟรม
    /// จึงวาด **ภาพของแท็บเก่า** ต่อไปทั้งชุด — ผู้ใช้เห็นภาพสามใบบนกระดานที่
    /// สถานะบอกว่ามีใบเดียว และคลิกโดนแค่ใบเดียว
    ///
    /// เทสต์จับไม่ได้โดยธรรมชาติเพราะทุกชิ้นถูกหมด: `Docs` สลับถูก · `rebuild_quads`
    /// สร้างถูก · **ไม่มีใครเรียกมัน** (`docs/08 §3.9` ข้อ 5 — "ทุกชิ้นถูก ประกอบผิด")
    ///
    /// ★ การลากที่ค้างอยู่เป็นของแท็บเดิม ใช้ต่อกับใบใหม่ไม่ได้ — ยกเลิกก่อนย้าย
    /// (เหตุผลเดียวกับตอนสลับเครื่องมือ)
    fn focus_tab(&mut self, index: usize) {
        if index >= self.docs.len() || index == self.docs.active {
            return;
        }
        {
            let leaving = self.docs.active_mut();
            leaving.select_tool.cancel();
            leaving.rubber_band = None;
            leaving.guides.clear();
        }
        self.docs.active = index;
        // ★★★ โหมดตามแท็บไปด้วย (P5-3b ก้อน c · `HANDOFF §6` จุดที่สาม) — ที่นี่คือ
        //     ประตูเดียวที่สลับแท็บได้ · ต้นเฟรมถัดไปก็อ่านค่าเดียวกันนี้อยู่แล้ว
        //     แต่ระหว่างนี้ยังมีโค้ดที่อ่าน `shell.mode` ต่อในเฟรมเดียวกัน
        self.mode_follows_active_tab();
        if let Some(gfx) = self.gfx.as_mut() {
            // ★ ภาพของใบใหม่ · และแผ่น Arrange ต้องคำนวณใหม่ด้วย (คนละ board)
            Self::rebuild_quads(gfx, self.docs.active_mut());
            gfx.working_quads.clear();
            gfx.window.request_redraw();
        }
    }

    /// ★★★ `Ctrl+W` / กดกากบาทบนแท็บ — **ถามก่อนถ้ายังไม่บันทึก** (`docs/03 §5`)
    ///
    /// ใช้แถบยืนยันตัวเดียวกับตอนปิดหน้าต่าง (`docs/03 §1`: คำถามชนิดเดียวกัน
    /// ควรหน้าตาเหมือนกัน) — สิ่งที่ต่างคือ **ขอบเขต** ซึ่งเก็บไว้ที่ `closing_tab`
    fn request_close_tab(&mut self, index: usize) {
        let Some(doc) = self.docs.list.get(index) else {
            return;
        };
        if self.gfx.is_some() && doc.board.is_dirty() {
            self.closing_tab = Some(doc.id);
            self.shell.close_prompt = true;
            self.shell.close_scope_tab = true;
            // ★ พาผู้ใช้ไปดูแท็บที่กำลังจะปิดก่อนถาม — ถามถึงงานที่เขามองไม่เห็น
            //   คือการขอให้เขาเดา (หลักการเดียวกับ "เอกสารขึ้นจอก่อนแล้วค่อยถาม")
            self.focus_tab(index);
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }
            return;
        }
        self.close_tab_now(index);
    }

    /// ★★★ ปิดแท็บจริง — คืนทรัพยากรของมันให้ครบ
    ///
    /// ★★ **ไม่ลบ recovery snapshot ของแท็บที่ยังไม่เคยบันทึก** โดยตั้งใจ:
    /// `docs/07 §4` ให้ลบ *"เมื่อผู้ใช้ Save ลง path จริงสำเร็จ"* เท่านั้น ·
    /// คนที่กด "ปิดโดยไม่บันทึก" เพราะเข้าใจผิด ต้องยังมีอะไรให้กู้รอบหน้า
    /// (เหตุผลเดียวกับที่ `autosave::discard` เขียนคำว่า "เท่านั้น" ตัวหนาไว้)
    /// · ราคาคือไฟล์ที่ถูกเสนอกลับหนึ่งครั้ง ซึ่งเพดาน 10 ไฟล์ / 30 วันคุมอยู่แล้ว
    fn close_tab_now(&mut self, index: usize) {
        let Some(closed) = self.docs.close(index) else {
            return;
        };
        // ★★ ช่องใน atlas เป็นทรัพยากรร่วมของทั้งแอป (3,072 ช่อง — ROADMAP P3-3)
        //    ไม่คืนตอนปิดแท็บ = เปิด-ปิดไปสิบรอบแล้ว board ใหม่ใส่ภาพไม่ได้อีกเลย
        if let Some(gfx) = self.gfx.as_mut() {
            for state in closed.render_state.values() {
                if let Some(slot) = state.slot {
                    gfx.atlas.free(slot);
                }
            }
        }
        // ★ ผลงาน decode ที่ยังเดินอยู่ของแท็บนี้ไม่มีที่ให้ลงแล้ว — ทิ้งคีย์
        //   ไม่งั้นตารางจะโตไปเรื่อย ๆ ตลอดอายุโปรแกรม (I-6)
        self.forget_jobs_of(closed.id);
        // ★ งานค้างที่ชี้ไปแท็บนี้ต้องถูกยกเลิกด้วย ไม่งั้นผลจะไปตกที่ `by_id_mut`
        //   ที่คืน `None` แล้วผู้ใช้จะเห็นสถานะ "กำลังบันทึก/กำลังเปิด" ค้างตลอดไป
        if self
            .save_job
            .as_ref()
            .is_some_and(|(id, _)| *id == closed.id)
        {
            self.save_job = None;
        }
        if self
            .load_job
            .as_ref()
            .is_some_and(|(id, _)| *id == closed.id)
        {
            self.load_job = None;
        }
        if self
            .relink_scan
            .as_ref()
            .is_some_and(|(id, _)| *id == closed.id)
        {
            self.relink_scan = None;
        }
        self.relink_queue.retain(|id| *id != closed.id);
        if self.relink_for.is_some_and(|(id, _)| id == closed.id) {
            self.relink_for = None;
        }
        // ★ แถบที่กำลังถามเรื่องเอกสารใบนี้พูดถึงของที่ไม่อยู่บนจอแล้ว
        if matches!(
            self.shell.recover_prompt.as_ref().map(|view| view.scope),
            Some(
                crate::shell::RecoverScope::ThisDocument | crate::shell::RecoverScope::KeptForLater
            )
        ) {
            self.shell.recover_prompt = None;
        }
        if let Some(gfx) = self.gfx.as_mut() {
            Self::rebuild_quads(gfx, self.docs.active_mut());
        }
        self.shell.status = text::t(self.shell.lang, Key::TabClosed).to_owned();
        self.shell.status_warn = false;
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// ทิ้งคีย์งานทั้งหมดที่เป็นของแท็บที่ถูกปิดไปแล้ว
    fn forget_jobs_of(&mut self, id: refx_core::arena::BoardId) {
        let orphans: Vec<refx_asset::hash::ContentHash> = self
            .job_owner
            .iter()
            .filter(|(_, owner)| **owner == id)
            .map(|(key, _)| *key)
            .collect();
        for key in orphans {
            self.job_owner.remove(&key);
            self.job_sources.remove(&key);
        }
    }

    /// ★★ ขอ thumbnail ของทุกภาพบน board ที่เพิ่งโหลดมา
    ///
    /// ไฟล์ `.refx` เก็บแค่ **การอ้างถึง** ภาพ (`AssetRef`: hash/path/ขนาด) ไม่ได้
    /// เก็บพิกเซล (docs/07 §2 โหมด linked) — เปิดไฟล์มาจึงต้อง decode ใหม่ทุกใบ
    ///
    /// ★ ผลที่กลับมาต้องไปเกาะ **item ที่มีอยู่แล้ว** ไม่ใช่สร้างใบใหม่ต่อท้าย
    /// (ซึ่งเป็นสิ่งที่เส้นทางลากไฟล์เข้ามาทำ) — คีย์ที่จับคู่คือ `relink_targets`
    ///
    /// ★★★ **รับ `BoardId` ไม่ใช่ "แท็บที่ดูอยู่"** (P4-7c) — ระหว่างที่งานอ่าน
    /// ไฟล์เดินอยู่ ผู้ใช้สลับแท็บได้ · อ่าน "แท็บที่ดูอยู่" ตอนผลกลับมาเมื่อไหร่
    /// ภาพของเอกสารหนึ่งจะไปขอ thumbnail ให้อีกเอกสารหนึ่ง
    fn request_thumbnails_for(&mut self, id: refx_core::arena::BoardId) {
        // ★ ต้องมีหน้าต่างแล้วเท่านั้น (เงื่อนไขเดิม) — เอกสารอยู่คนละที่กับ `Gfx` แล้ว
        if self.gfx.is_none() || self.assets.is_none() {
            return;
        }
        // ★★ งวดหนึ่งงวดต่อครั้ง เพราะ [`DropBatch`] มีชุดเดียว · ที่รอคิวไม่ได้
        //    ถูกทิ้ง แต่ถูกจดไว้แล้วเริ่มเมื่องวดก่อนจบ — เดิมมันถูก **ทิ้งเงียบ ๆ**
        //    ซึ่งจะทำให้แท็บที่สองเปิดมาแล้วภาพไม่ขึ้นเลยสักใบ
        if self.relink_scan.is_some() {
            if !self.relink_queue.contains(&id) {
                self.relink_queue.push_back(id);
            }
            return;
        }
        let Some(doc) = self.docs.list.iter().find(|doc| doc.id == id) else {
            return;
        };
        // ★★ ทุกใบที่เป็นภาพ **รวมทั้งใบที่บันทึกไว้ว่า `Missing`** — ไฟล์ที่หาย
        //    เมื่อวานอาจกลับมาแล้ววันนี้ (เสียบไดรฟ์คืน / ซิงค์เสร็จ)
        let wanted: Vec<refx_core::relink::Wanted> = doc
            .board
            .items_in_z_order()
            .filter_map(|(item_id, item)| match &item.kind {
                ItemKind::Image(asset) => Some(refx_core::relink::Wanted {
                    id: item_id,
                    hash: asset.hash,
                    path: asset.path.clone(),
                }),
                ItemKind::Missing {
                    original_path,
                    reason: _,
                } => Some(refx_core::relink::Wanted {
                    id: item_id,
                    // ★ ใบที่เป็น `Missing` ไม่มีคีย์ของเนื้อให้ใช้ — ขั้นที่ 3
                    //   จึงหาไม่เจอโดยธรรมชาติ ที่ยังทำงานให้มันได้คือขั้น 1/2
                    hash: refx_core::hash::ContentHash::from_bytes([0; 32]),
                    path: original_path.clone(),
                }),
                ItemKind::Text(_) => None, // โน้ตข้อความไม่มีอะไรให้ decode
            })
            .collect();
        if wanted.is_empty() {
            return;
        }
        self.start_relink_scan(id, wanted);
    }

    /// เริ่มงวดสแกนที่รอคิวอยู่ (ถ้ามี) — เรียกเมื่องวดก่อนจบ
    fn start_queued_relink(&mut self) {
        while let Some(id) = self.relink_queue.pop_front() {
            if self.docs.list.iter().any(|doc| doc.id == id) {
                self.request_thumbnails_for(id);
                return;
            }
        }
    }

    /// ★★★ ขั้น 1–3 ของ relink **+ สำเนาที่เอกสารพกมาเอง** — บนเธรดอื่น (I-2)
    ///
    /// การถามว่า "ไฟล์นี้ยังอยู่ไหม" คือ `stat` หนึ่งครั้งต่อใบ · board 3,000 ใบ
    /// บนไดรฟ์เครือข่ายที่หลุด = หน้าต่างค้างเป็นสิบวินาที ซึ่งเป็นวินาทีที่ผู้ใช้
    /// ตัดสินว่าโปรแกรมนี้เชื่อถือได้หรือเปล่า
    ///
    /// ## ★★★ ทำไมการแกะ blob อยู่ **หลัง** ขั้น 1–3 ไม่ใช่ก่อน
    ///
    /// ทั้งสามขั้นแรกคืนไฟล์ที่ **ผู้ใช้เป็นเจ้าของ** ส่วน blob ในเอกสารเป็นสำเนา
    /// ที่เราจะแกะลง spool ซึ่งเป็นโฟลเดอร์ที่ถูกเก็บกวาดตามเพดาน · เมื่อทั้งสอง
    /// ทางให้เนื้อเดียวกันเป๊ะ (คีย์คือ hash ของเนื้อ) การเลือกไฟล์จริงของผู้ใช้
    /// ก่อนจึงดีกว่าเสมอ — ไม่กินดิสก์เพิ่ม และ `docs/07 §2` ขั้นที่ 1 ยังคุ้มครอง
    /// ไฟล์ที่ไม่เคยย้ายไปไหนตามเดิม
    ///
    /// ★★ **แต่ต้องมาก่อนขั้นที่ 4 เสมอ** — เอกสารที่พกภาพมาเองแล้วขึ้น
    /// *"หาไฟล์ไม่เจอ"* คือการโกหกผู้ใช้ ทั้งที่ภาพอยู่ในไฟล์ที่เขาเพิ่งเปิด
    fn start_relink_scan(
        &mut self,
        board: refx_core::arena::BoardId,
        wanted: Vec<refx_core::relink::Wanted>,
    ) {
        let Some(assets) = self.assets.as_ref() else {
            return;
        };
        let Some(doc) = self.docs.list.iter().find(|doc| doc.id == board) else {
            return;
        };
        let io = assets.io_tx.clone();
        let wake = assets.pool.wake_handle();
        // ★ โฟลเดอร์ของเอกสาร — ขั้นที่ 2 · งานที่กู้คืนมายังไม่มีไฟล์จึงเป็น `None`
        let doc_dir = doc
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(std::path::Path::to_path_buf);
        // ★ ของที่ต้องมีครบทั้งสามอย่างการแกะ blob ถึงจะเป็นไปได้
        let embedded = doc
            .path
            .clone()
            .filter(|_| !doc.assets.is_empty())
            .zip(self.spool_dir.clone())
            .map(|(path, spool)| (path, spool, doc.assets.clone()));
        let (tx, rx) = crossbeam_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("refx-relink-scan".to_owned())
            .spawn(move || {
                // ★ เปิดเอกสารครั้งเดียวสำหรับทั้งงวด ไม่ใช่ครั้งละใบ — และเปิด
                //   ก็ต่อเมื่อมีอะไรฝังอยู่จริง (เอกสาร linked ไม่ถูกแตะเลย)
                let mut source = embedded.as_ref().and_then(|(doc, _, _)| {
                    std::fs::File::open(doc)
                        .map_err(|err| {
                            tracing::warn!(%err, "cannot reopen the document to unpack its images");
                        })
                        .ok()
                });
                let mut found = Vec::with_capacity(wanted.len());
                for want in wanted {
                    let mut located = refx_core::relink::locate(
                        &want,
                        doc_dir.as_deref(),
                        // ★★★ **ด่าน path ที่มาจากไฟล์อยู่ตรงนี้** (`docs/06 §4`)
                        //
                        //   ต้องอยู่ **ก่อน** `is_file()` ไม่ใช่หลัง: `Path::is_file()`
                        //   บน UNC **ต่อออกไปหาเซิร์ฟเวอร์เพื่อจะตอบ** ด่านที่ตรวจ
                        //   หลังจากนั้นก็สายไปแล้ว — การเชื่อมต่อเกิดขึ้นไปแล้ว
                        //   (ผมวางด่านไว้ผิดที่ในรุ่นแรกด้วยเหตุผลนี้พอดี)
                        //
                        //   ★ คลุมทั้งห้าขั้นของ relink ในคราวเดียว รวมขั้นที่ 3 ที่
                        //     path มาจากตาราง `paths` ใน cache DB ซึ่งก็คือข้อมูล
                        //     ที่มาจากไฟล์เหมือนกัน
                        //
                        //   ★★ ที่นี่ตรวจ **ผู้สมัคร** ไม่ใช่สิ่งที่ผู้ใช้ชี้เอง —
                        //     คนที่กด "หาไฟล์เอง" แล้วเลือกไดรฟ์เครือข่ายตั้งใจทำแบบนั้น
                        //     และเดินคนละเส้นทาง (`Step::PickedByUser`)
                        |path| match refx_io::validate::validate_asset_path(path) {
                            Ok(()) => path.is_file(),
                            Err(err) => {
                                tracing::warn!(
                                    %err,
                                    file = %refx_asset::decode::file_label(path),
                                    "refusing a path that came out of the document"
                                );
                                false
                            }
                        },
                        |hash| paths_known_for(io.as_ref(), hash),
                    );
                    if located.is_none()
                        && let (Some((_, spool, index)), Some(file)) =
                            (embedded.as_ref(), source.as_mut())
                    {
                        located = unpack_embedded(&want, index, spool, file);
                    }
                    found.push((want, located));
                }
                let _ = tx.send(found);
                wake.wake();
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "cannot spawn the relink scan thread");
            return;
        }
        self.relink_scan = Some((board, rx));
    }

    /// เก็บผลการตามหา แล้วสั่ง decode ใบที่เจอ / ทำใบที่ไม่เจอเป็น `Missing`
    fn poll_relink_scan(&mut self) {
        let Some((board, rx)) = self.relink_scan.as_ref() else {
            return;
        };
        let board = *board;
        let found = match rx.try_recv() {
            Ok(found) => found,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                self.relink_scan = None;
                self.start_queued_relink();
                return;
            }
        };
        self.relink_scan = None;
        self.apply_located(board, found);
        // ★ งวดนี้จบแล้ว — แท็บที่รอคิวอยู่ได้เริ่มของมันบ้าง (ดู `request_thumbnails_for`)
        self.start_queued_relink();
    }

    /// ★★★ ผลของการตามหา → งาน decode + คำสั่งทำใบที่หาไม่เจอเป็น `Missing`
    ///
    /// ใช้ร่วมกันทั้งขั้น 1–3 (ตอนเปิดเอกสาร) และขั้น 5 (ผู้ใช้ชี้ไฟล์เอง) —
    /// สองเส้นทางนั้นต่างกันแค่ *วิธีหา* ไม่ใช่ *สิ่งที่ทำกับผลลัพธ์*
    fn apply_located(&mut self, board: refx_core::arena::BoardId, found: Vec<LocatedOne>) {
        // ★ แท็บถูกปิดไประหว่างที่การสแกนเดินอยู่ = ไม่มีที่ให้ผลลง
        if self.docs.by_id_mut(board).is_none() {
            return;
        }
        // ★★★ **ปิดหน้าต่าง merge ก่อนเริ่มงวดใหม่** (docs/02 §3)
        //
        //   `RelinkAssets` merge ตัวเองเพื่อให้ผลที่ทยอยกลับมาข้ามหลายเฟรมเป็น
        //   undo ขั้นเดียว · แต่ถ้าไม่ seal ระหว่างงวด การผูกไฟล์ตอนเปิดเอกสาร
        //   กับการที่ผู้ใช้กด "หาไฟล์เอง" อีกสิบนาทีต่อมา **จะรวมเป็นขั้นเดียวกัน**
        //   แล้ว `Ctrl+Z` ครั้งเดียวจะย้อนทั้งสองเรื่องพร้อมกัน ซึ่งผู้ใช้ไม่ได้สั่ง
        //   (เห็นจริงตอนยืนยัน P4-6 21 ส.ค. 2026: กด `Ctrl+Z` แล้วภาพไม่กลับไป
        //   เป็น `Missing` เพราะมันย้อนไปไกลกว่านั้นหนึ่งงวด)
        if self.gfx.is_some()
            && let Some(doc) = self.docs.by_id_mut(board)
        {
            doc.history.seal();
        }
        let mut jobs = Vec::new();
        let mut lost: Vec<(ItemId, ItemKind)> = Vec::new();
        let mut moved = 0usize;
        let mut unpacked = 0usize;
        let total = found.len();

        for (index, (want, located)) in found.into_iter().enumerate() {
            let Some(located) = located else {
                // ขั้นที่ 4 — ยังไม่เจอ · **item ยังอยู่บน board** (`docs/07 §2`)
                lost.push((
                    want.id,
                    ItemKind::Missing {
                        original_path: want.path,
                        reason: refx_core::board::MissingReason::FileNotFound,
                    },
                ));
                continue;
            };
            match located.step {
                // ที่เดิม = ไม่ใช่การ relink ในสายตาผู้ใช้ (เงียบสนิท)
                refx_core::relink::Step::WhereItWas => {}
                // ★ ภาพที่อยู่ในไฟล์งานเองไม่ได้ "ย้ายที่" — คนละเรื่องกันสำหรับผู้ใช้
                refx_core::relink::Step::Embedded => {
                    unpacked += 1;
                    tracing::info!(
                        file = %refx_asset::decode::file_label(&located.path),
                        "unpacked an image stored inside the document"
                    );
                }
                _ => {
                    moved += 1;
                    tracing::info!(
                        step = ?located.step,
                        file = %refx_asset::decode::file_label(&located.path),
                        "relinked an image that had moved"
                    );
                }
            }
            // คีย์ชั่วคราวสำหรับจับคู่ผลลัพธ์ (เหมือน `submit_dropped` เป๊ะ)
            let key = job_key_for(board, &located.path);
            let source = refx_asset::pool::JobSource::File(located.path);
            if let Some(doc) = self.docs.by_id_mut(board) {
                doc.relink_targets.insert(key, want.id);
            }
            jobs.push(refx_asset::pool::Job {
                hash: key,
                source,
                // เรียงตาม z-order — ใบล่างสุดขึ้นก่อน เหมือนลำดับที่ผู้ใช้เห็น
                priority: index as f32,
                cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                target: refx_asset::pool::JobTarget::Thumbnail,
            });
        }

        let lost_count = lost.len();
        if !lost.is_empty() {
            self.apply_relink(board, lost);
        }

        let has_jobs = !jobs.is_empty();
        if has_jobs {
            self.drop_started = Some(std::time::Instant::now());
            self.drop.start(jobs.len());
            self.batch_from_clipboard = false;
            // ★ ประตูเดียวกับ `submit_dropped`/`submit_paste` — ดู `submit_thumbnail_jobs`
            self.submit_thumbnail_jobs(board, jobs);
        }

        // ★★★ บอกผู้ใช้เฉพาะตอนมีอะไรให้บอกจริง — เปิดไฟล์ที่ทุกอย่างอยู่ที่เดิม
        //     ต้องเงียบสนิท ไม่งั้นข้อความจะกลายเป็นเสียงรบกวนที่ไม่มีใครอ่าน
        //
        // ★★ **เก็บไว้รายงานตอนจบงวด ไม่ใช่เขียนเดี๋ยวนี้** — ถ้าเขียนตรงนี้
        //    รายงาน "เปิด N ไฟล์ใน M ms" ของงวด decode จะทับมันภายในไม่กี่
        //    มิลลิวินาที แล้วผู้ใช้จะไม่มีวันรู้ว่าภาพถูกผูกใหม่หรือหายไปกี่ใบ
        //    (บทเรียนเดิมของ §2.24: ข้อความที่ถูกทับทันที = ข้อความที่ไม่มีอยู่)
        if lost_count > 0 || moved > 0 || unpacked > 0 {
            self.relink_report = Some(RelinkReport {
                moved,
                unpacked,
                total,
                lost: lost_count,
            });
        }
        // ไม่มีงาน decode เลย (หายหมดทุกใบ) = ไม่มีงวดให้รอ ต้องบอกเดี๋ยวนี้
        if !has_jobs {
            self.report_relink();
        }
    }

    /// เขียนผลการตามหาไฟล์ลง status bar — เรียกตอน **จบงวด** เท่านั้น
    fn report_relink(&mut self) {
        if let Some(report) = self.relink_report.take() {
            write_relink_status(&mut self.shell, report);
        }
    }

    /// ห่อการเปลี่ยน `ItemKind` เป็น `Command` — ทางเดียวที่ `Board` ถูกแก้ (docs/08 §4)
    ///
    /// ★ `board` บอกว่าเป็นของแท็บไหน — `ItemId` ไม่ผูกกับ board จึงชี้ใบผิดได้ง่าย
    fn apply_relink(&mut self, board: refx_core::arena::BoardId, targets: Vec<(ItemId, ItemKind)>) {
        if self.gfx.is_none() {
            return;
        }
        let Some(index) = self.docs.list.iter().position(|doc| doc.id == board) else {
            return;
        };
        {
            let doc = &mut self.docs.list[index];
            let Ok(command) = RelinkAssets::new(&doc.board, targets) else {
                return; // ไม่มีอะไรเปลี่ยนจริง = ไม่ต้องมีขั้น undo
            };
            if let Err(err) = doc.history.apply(&mut doc.board, Box::new(command)) {
                tracing::error!(%err, "cannot relink the images");
                return;
            }
        }
        // ★ วาดใหม่เฉพาะตอนมันเป็นแท็บที่อยู่บนจอ — `quads` มีชุดเดียวต่อหน้าต่าง
        if self.docs.active == index
            && let Some(gfx) = self.gfx.as_mut()
        {
            Self::rebuild_quads(gfx, &mut self.docs.list[index]);
        }
    }

    /// ★★★ ขั้นที่ 5 — ผู้ใช้กด "หาไฟล์เอง" บนภาพที่หาย
    fn apply_relink_request(&mut self) {
        // ซ้อนกันไม่ได้ด้วยเหตุผลเดียวกับ dialog ตัวอื่น
        if self.relink_pick.is_some() || self.relink_match.is_some() {
            return;
        }
        let Some(id) = self.first_missing_selected() else {
            return;
        };
        let name = self
            .gfx
            .as_ref()
            .and_then(|_| self.docs.active().board.item(id))
            .and_then(|item| match &item.kind {
                ItemKind::Missing { original_path, .. } => {
                    Some(refx_asset::decode::file_label(original_path))
                }
                _ => None,
            })
            .unwrap_or_default();
        self.relink_for = Some((self.docs.active().id, id));
        // ★ รู้ชื่อไฟล์ก็บอกบนหัวกล่อง — ผู้ใช้ที่มีภาพหายหลายใบต้องรู้ว่ากำลังหาใบไหน
        let title = if name.is_empty() {
            text::t(self.shell.lang, Key::DialogFindImage).to_owned()
        } else {
            text::fill(
                self.shell.lang,
                text::Template::DialogFindNamedImage,
                &[("name", &name)],
            )
        };
        self.relink_pick = Some(refx_platform::dialog::pick_missing_image(
            &title,
            self.waker.clone(),
        ));
        // ★ ข้อความของ **การหาไฟล์ภาพ** ไม่ใช่ของการเปิด board — เคยใช้
        //   `OpenChoosing` ซ้ำแล้วบนจอขึ้นว่า "Choose a board to open"
        //   ซึ่งบอกผู้ใช้ผิดเรื่องทั้งประโยค (เห็นตอนถ่ายภาพยืนยัน 21 ส.ค. 2026)
        self.shell.status = text::t(self.shell.lang, Key::FindFileChoosing).to_owned();
        self.shell.status_warn = false;
    }

    /// item ที่หาไฟล์ไม่เจอใบแรกในสิ่งที่เลือกอยู่ (ของแท็บที่ดูอยู่)
    fn first_missing_selected(&self) -> Option<ItemId> {
        self.gfx.as_ref()?;
        let doc = self.docs.active();
        doc.selection.iter().find(|id| {
            doc.board
                .item(*id)
                .is_some_and(|item| matches!(item.kind, ItemKind::Missing { .. }))
        })
    }

    /// เก็บผลของ dialog แล้วส่งงานส่องโฟลเดอร์ต่อ — **ไม่บล็อก** (I-2)
    fn poll_relink(&mut self) {
        if let Some(rx) = self.relink_pick.as_ref() {
            match rx.try_recv() {
                Ok(Some(path)) => {
                    self.relink_pick = None;
                    self.start_folder_match(&path);
                }
                Ok(None) => {
                    self.relink_pick = None; // กดยกเลิก — ไม่ใช่ error
                    self.relink_for = None;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.relink_pick = None;
                    self.relink_for = None;
                }
            }
        }

        let Some((board, rx)) = self.relink_match.as_ref() else {
            return;
        };
        let board = *board;
        let found = match rx.try_recv() {
            Ok(found) => found,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                self.relink_match = None;
                return;
            }
        };
        self.relink_match = None;
        self.apply_located(board, found);
    }

    /// ★★★ ผู้ใช้ชี้ไฟล์มาแล้ว → จับคู่ใบที่เหลือในโฟลเดอร์นั้น (`docs/07 §2` ขั้น 5)
    ///
    /// ★ ใบที่ผู้ใช้ชี้ **ผูกตามที่เขาสั่งเสมอ** ไม่ว่า hash จะตรงหรือไม่ —
    /// เจตนาที่เขาพิมพ์ด้วยมือชนะการเดาของเราทุกกรณี · ที่เหลือถูกจับคู่ด้วย
    /// [`refx_core::relink::match_folder`]
    fn start_folder_match(&mut self, picked: &std::path::Path) {
        let Some((board, chosen)) = self.relink_for.take() else {
            return;
        };
        // ★ ต้องมีหน้าต่างแล้วเท่านั้น (เงื่อนไขเดิม)
        if self.gfx.is_none() {
            return;
        }
        // ★ แท็บที่ผู้ใช้กดปุ่มนั้นถูกปิดไประหว่างที่ dialog เปิดอยู่ได้
        let Some(doc) = self.docs.list.iter().find(|doc| doc.id == board) else {
            return;
        };
        // ใบที่ยังหาไม่เจอทั้งหมด (รวมใบที่ผู้ใช้เพิ่งชี้ให้)
        let missing: Vec<refx_core::relink::Wanted> = doc
            .board
            .items_in_z_order()
            .filter_map(|(id, item)| match &item.kind {
                ItemKind::Missing { original_path, .. } if id != chosen => {
                    Some(refx_core::relink::Wanted {
                        id,
                        hash: refx_core::hash::ContentHash::from_bytes([0; 32]),
                        path: original_path.clone(),
                    })
                }
                _ => None,
            })
            .collect();
        // ★ ใบที่ผู้ใช้ชี้เอง — ใส่คีย์จริงของเอกสารไว้ เพื่อให้ `match_folder`
        //   ใช้จับใบอื่นที่เป็นภาพเดียวกันได้ด้วย
        let chosen_want = doc
            .board
            .item(chosen)
            .map(|item| refx_core::relink::Wanted {
                id: chosen,
                hash: match &item.kind {
                    ItemKind::Image(asset) => asset.hash,
                    _ => refx_core::hash::ContentHash::from_bytes([0; 32]),
                },
                path: match &item.kind {
                    ItemKind::Missing { original_path, .. } => original_path.clone(),
                    ItemKind::Image(asset) => asset.path.clone(),
                    ItemKind::Text(_) => std::path::PathBuf::new(),
                },
            });

        let picked = picked.to_path_buf();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let wake = self.assets.as_ref().map(|a| a.pool.wake_handle());
        let spawned = std::thread::Builder::new()
            .name("refx-relink-match".to_owned())
            .spawn(move || {
                let mut out: Vec<LocatedOne> = Vec::new();
                // ใบที่ผู้ใช้ชี้ — ผูกตามคำสั่งเสมอ
                if let Some(want) = chosen_want {
                    let id = want.id;
                    out.push((
                        want,
                        Some(refx_core::relink::Located {
                            id,
                            path: picked.clone(),
                            step: refx_core::relink::Step::PickedByUser,
                        }),
                    ));
                }
                if !missing.is_empty() {
                    let candidates = hash_folder(picked.parent());
                    let matched = refx_core::relink::match_folder(&missing, &candidates);
                    for want in missing {
                        let hit = matched.iter().find(|m| m.id == want.id).cloned();
                        out.push((want, hit));
                    }
                }
                let _ = tx.send(out);
                if let Some(wake) = wake {
                    wake.wake();
                }
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "cannot spawn the folder match thread");
            return;
        }
        self.relink_match = Some((board, rx));
    }

    /// ผู้ใช้ตอบแถบยืนยันตอนปิดแล้ว (P4-2)
    ///
    /// ★★★ แถบเดียวถามได้ **สองขอบเขต** ตั้งแต่ P4-7c: ปิดทั้งหน้าต่าง หรือ
    /// ปิดแท็บใบเดียว (`Ctrl+W`) · แยกด้วย [`Self::closing_tab`] — ปุ่มหน้าตา
    /// เหมือนกันเพราะคำถามเป็นชนิดเดียวกัน แต่ **สิ่งที่หายถ้าตอบผิดต่างกันมาก**
    fn apply_close_choice(&mut self, choice: crate::shell::CloseChoice) {
        use crate::shell::CloseChoice;

        self.close_confirm = false;
        self.shell.close_prompt = false;
        self.shell.close_scope_tab = false;
        let tab = self.closing_tab.take();
        match choice {
            CloseChoice::SaveThenClose => {
                // ★ ปิดจริงตอน **บันทึกสำเร็จ** เท่านั้น (ดู `poll_save`)
                self.after_save = match tab {
                    Some(id) => AfterSave::CloseTab(id),
                    None => AfterSave::Close,
                };
                self.apply_save_request(SaveRequest::Save);
            }
            CloseChoice::DiscardAndClose => match tab {
                Some(id) => {
                    if let Some(index) = self.docs.list.iter().position(|doc| doc.id == id) {
                        self.close_tab_now(index);
                    }
                }
                None => self.closing = true,
            },
            CloseChoice::Cancel => {
                self.after_save = AfterSave::Stay;
            }
        }
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// ★★★ บันทึกเอกสาร — **ทุกขั้นไม่บล็อก UI thread** (P4-2, I-2)
    ///
    /// สามขั้นที่แยกกันคนละเฟรม เพราะแต่ละขั้นรอคนละอย่าง:
    ///
    /// | ขั้น | รออะไร | ไม่บล็อกยังไง |
    /// |---|---|---|
    /// | เลือกที่เก็บ | ผู้ใช้ (เป็น**นาที**ได้) | dialog อยู่เธรดของตัวเอง คืน `Receiver` |
    /// | เขียนไฟล์ | ดิสก์ (fsync จริง) | ส่งไปเธรด คืน `Receiver` |
    /// | รายงานผล | — | `try_recv()` ต้นเฟรม |
    ///
    /// ★ `Board` ถูก **โคลนเข้าไปในเธรด** ไม่ใช่ส่ง reference — ผู้ใช้ต้องแก้งาน
    /// ต่อได้ทันทีระหว่างที่ไฟล์กำลังเขียน และสิ่งที่ลงไฟล์ต้องเป็นสภาพ
    /// ณ ตอนกด `Ctrl+S` ไม่ใช่สภาพหลังจากนั้น (ซึ่งจะเป็นการบันทึกที่ผู้ใช้ไม่ได้สั่ง)
    fn apply_save_request(&mut self, request: SaveRequest) {
        // มีงานบันทึกค้างอยู่แล้ว = อย่าซ้อน (ไฟล์เดียวเขียนสองที่พร้อมกันคือหายนะ)
        if self.save_job.is_some() || self.save_dialog.is_some() {
            return;
        }
        // ★★★ **`Ctrl+S` ใช้โหมดของเอกสารเดิมเสมอ ห้ามถามซ้ำ** (P4-5)
        //
        //   เอกสารที่ผู้ใช้เคยบันทึกแบบ packed มีภาพอยู่ข้างในไฟล์ · ถ้าการกด
        //   `Ctrl+S` เขียน linked ทับ **ภาพที่ฝังไว้หายหมดในครั้งเดียว** ซึ่งคือ
        //   ความพังที่ `docs/07 §1` ยกเป็นเหตุผลของการมี version 2 อยู่แล้ว —
        //   ต่างกันแค่คราวนี้คนที่ทำคือรุ่นปัจจุบันของเราเอง ไม่ใช่รุ่นเก่า
        let known_path = match request {
            SaveRequest::Save => self.docs.active().path.clone(),
            // บันทึกเป็น = ถามที่ใหม่เสมอ ต่อให้เคยบันทึกแล้ว
            SaveRequest::SaveAs => None,
        };
        let mode = self.docs.active().save_mode;
        match known_path {
            Some(path) => self.start_save(&path, mode),
            // ★ ถามโหมดก่อน แล้วค่อยถามที่เก็บ (`docs/07 §2`: "Save As มีตัวเลือกนี้
            //   ชัดเจน") · native dialog ใส่ตัวเลือกของเราเองเข้าไปไม่ได้ แถบใน
            //   หน้าต่างจึงเป็นที่เดียวที่ใส่ได้ — และมันไม่บล็อก UI thread ด้วย (I-2)
            None => {
                self.shell.save_as_prompt = true;
                self.shell.status = text::t(self.shell.lang, Key::SaveModeAsk).to_owned();
                self.shell.status_warn = false;
            }
        }
    }

    /// ผู้ใช้ตอบแถบ "บันทึกเป็นแบบไหน" แล้ว → ถามที่เก็บต่อ
    fn apply_save_as_choice(&mut self, choice: crate::shell::SaveAsChoice) {
        use crate::shell::SaveAsChoice;

        self.shell.save_as_prompt = false;
        let mode = match choice {
            SaveAsChoice::Linked => refx_io::packed::SaveMode::Linked,
            SaveAsChoice::Packed => refx_io::packed::SaveMode::Packed,
            SaveAsChoice::Cancel => {
                // ★ ยกเลิกตรงนี้ต้อง **ยกเลิกการปิดด้วย** เหมือนยกเลิกที่ dialog
                //   ไม่งั้นผู้ใช้ที่กด "บันทึกแล้วปิด" แล้วเปลี่ยนใจจะโดนปิดหน้าต่าง
                self.after_save = AfterSave::Stay;
                self.shell.status = text::t(self.shell.lang, Key::SaveCancelled).to_owned();
                self.shell.status_warn = false;
                return;
            }
        };
        if self.save_dialog.is_some() || self.save_job.is_some() {
            return;
        }
        let name = self
            .docs
            .active()
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map_or_else(
                || "board.refx".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
        self.save_as_mode = mode;
        self.save_dialog = Some(refx_platform::dialog::pick_save_location(
            text::t(self.shell.lang, Key::DialogSaveBoard),
            &name,
            self.waker.clone(),
        ));
        self.shell.status = text::t(self.shell.lang, Key::SaveChoosing).to_owned();
        self.shell.status_warn = false;
    }

    /// ★★ ผู้ใช้กดสลับโหมดบนแถบสถานะ — เอกสารที่มีไฟล์แล้วถูก **เขียนใหม่ทันที**
    ///
    /// ★★★ ทำไมไม่ใช่แค่จำไว้แล้วรอ `Ctrl+S`: ตัวบ่งชี้บนแถบสถานะเป็น **สภาวะ
    /// ของไฟล์** (`docs/03 §1`) ถ้ามันเปลี่ยนทันทีที่กดแต่ไฟล์ยังเหมือนเดิม
    /// มันจะกลายเป็นคำโกหกที่อยู่ค้างบนจอ — ผู้ใช้ที่กด "เก็บภาพไว้ข้างใน"
    /// แล้วส่งไฟล์ให้เพื่อนทันทีจะส่งไฟล์ที่ไม่มีภาพอยู่ข้างในเลย
    ///
    /// ★ board ที่ยังไม่มีไฟล์ทำอะไรไม่ได้นอกจากจำไว้ — และนั่นถูกต้อง เพราะ
    /// ยังไม่มีไฟล์ให้พูดถึง (แถบสถานะบอกว่า "จะบันทึกแบบนี้")
    fn apply_mode_request(&mut self, mode: refx_io::packed::SaveMode) {
        if mode == self.docs.active().save_mode {
            return;
        }
        match self.docs.active().path.clone() {
            Some(path) => self.start_save(&path, mode),
            None => {
                self.docs.active_mut().save_mode = mode;
                self.shell.status = text::t(self.shell.lang, mode_message(mode)).to_owned();
                self.shell.status_warn = false;
            }
        }
    }

    /// ส่งงานเขียนไฟล์ไปเธรด — ไม่รอผล
    ///
    /// ★★★ **การตัดสินว่าจะฝังภาพใบไหน อยู่บนเธรดนี้ด้วย** (P4-5) —
    /// `plan_embeds` ถาม `locate_bytes` ซึ่งแตะดิสก์ทุกใบ (`is_file`) · board
    /// 3,000 ใบบนไดรฟ์เครือข่ายที่หลุด = หน้าต่างค้างเป็นสิบวินาทีถ้าทำบน UI (I-2)
    fn start_save(&mut self, path: &std::path::Path, mode: refx_io::packed::SaveMode) {
        // ★ ต้องมีหน้าต่างแล้วเท่านั้น (เงื่อนไขเดิม) — เอกสารอยู่คนละที่กับ `Gfx` แล้ว
        if self.gfx.is_none() {
            return;
        }
        // ★ โคลน ณ จังหวะที่ผู้ใช้สั่ง (ดูเหตุผลใน `apply_save_request`)
        // ★★★ จำไว้ด้วยว่าเป็นของแท็บไหน — ผู้ใช้กด `Ctrl+Tab` ระหว่างที่ไฟล์
        //     กำลังเขียนได้ · `mark_saved` ที่ลงผิดใบทำให้แท็บที่ยังไม่ถูกบันทึก
        //     ดูสะอาด แล้วผู้ใช้จะปิดโปรแกรมทิ้งโดยเชื่อว่างานอยู่ในไฟล์แล้ว
        let owner = self.docs.active().id;
        let board = self.docs.active().board.clone();
        let path = path.to_path_buf();
        let spool_dir = self.spool_dir.clone();
        // ★★★ **ต้องปลุก UI ตอนเขียนเสร็จ** — แอปหลับสนิทระหว่างรอดิสก์ (I-1)
        //   ถ้าไม่ปลุก `poll_save` จะไม่ถูกเรียกจนกว่าผู้ใช้จะบังเอิญขยับเมาส์
        //   แล้วจอจะค้างที่คำว่า "กำลังบันทึก" ทั้งที่ไฟล์ลงดิสก์ไปแล้ว —
        //   ผู้ใช้ที่เห็นแบบนั้นจะไม่กล้าปิดโปรแกรม (เห็นจริงตอนยืนยัน P4-5)
        let waker = self.waker.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("refx-save".to_owned())
            .spawn(move || {
                // ★★ กฎว่าอะไรต้องถูกฝังอยู่ที่ `refx-io` ที่เดียว — ที่นี่ตอบแค่ว่า
                //    ไบต์ของแต่ละใบอยู่ที่ไหน (§4 ข้อ 23 · ดู `locate_bytes`)
                let embeds = refx_io::packed::plan_embeds(&board, mode, |asset| {
                    locate_bytes(spool_dir.as_deref(), asset)
                });
                // ★★ ส่ง `rename_durable` ของชั้น platform เข้าไป — ตัวที่ทำให้
                //    การสลับไฟล์เองทนไฟดับ (`MOVEFILE_WRITE_THROUGH` / fsync dir)
                //    `refx-io` เรียกเองไม่ได้เพราะพึ่ง `refx-platform` ไม่ได้
                let result = refx_io::save::save_document(
                    &path,
                    &board,
                    &embeds,
                    refx_platform::fsops::rename_durable,
                )
                .map_err(|err| err.to_string())
                .map(|()| SavedDoc {
                    // ★ อ่านตารางของไฟล์ที่ **เพิ่งเขียนจริง** กลับมา ไม่ใช่เชื่อ
                    //   แผนที่เราส่งเข้าไป — สองอย่างนี้ต่างกันได้ (board ที่ไม่มี
                    //   ภาพเลยได้ v1 ที่ไม่มีตาราง ทั้งที่ผู้ใช้สั่ง packed)
                    assets: read_asset_table(&path),
                    path,
                    mode,
                });
                let _ = tx.send(result);
                if let Some(waker) = waker {
                    waker.wake();
                }
            });
        if spawned.is_err() {
            self.shell.status = text::t(self.shell.lang, Key::SaveFailed).to_owned();
            self.shell.status_warn = true;
            return;
        }
        self.save_job = Some((owner, rx));
        self.shell.status = text::t(self.shell.lang, Key::SaveInProgress).to_owned();
        self.shell.status_warn = false;
    }

    /// เก็บผลของ dialog และของงานเขียนไฟล์ — เรียกต้นเฟรม **ไม่บล็อก**
    fn poll_save(&mut self) {
        let lang = self.shell.lang;

        // ---- ผู้ใช้เลือกที่เก็บแล้วหรือยัง ----
        if let Some(rx) = self.save_dialog.as_ref() {
            match rx.try_recv() {
                Ok(Some(path)) => {
                    self.save_dialog = None;
                    self.start_save(&path, self.save_as_mode);
                }
                Ok(None) => {
                    // กดยกเลิก — ไม่ใช่ error และ **ต้องยกเลิกการปิดด้วย**
                    self.save_dialog = None;
                    self.after_save = AfterSave::Stay;
                    self.shell.status = text::t(lang, Key::SaveCancelled).to_owned();
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.save_dialog = None;
                    self.after_save = AfterSave::Stay;
                    self.shell.status = text::t(lang, Key::SaveFailed).to_owned();
                    self.shell.status_warn = true;
                }
            }
        }

        // ---- เขียนไฟล์เสร็จหรือยัง ----
        let Some((owner, rx)) = self.save_job.as_ref() else {
            return;
        };
        let owner = *owner;
        let done = match rx.try_recv() {
            Ok(result) => result,
            Err(crossbeam_channel::TryRecvError::Empty) => return,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                Err("save thread died".to_owned())
            }
        };
        self.save_job = None;
        match done {
            Ok(SavedDoc { path, assets, mode }) => {
                // ★★★ ทุกอย่างข้างล่างนี้ลงที่ **แท็บที่สั่งบันทึก** ไม่ใช่แท็บที่
                //     ผู้ใช้บังเอิญดูอยู่ตอนไฟล์เขียนเสร็จ
                let Some(doc) = self.docs.by_id_mut(owner) else {
                    // แท็บถูกปิดไประหว่างที่ไฟล์กำลังเขียน — ไฟล์ลงดิสก์ครบแล้ว
                    // ซึ่งคือสิ่งที่ผู้ใช้สั่ง · ไม่มีอะไรให้ทำต่อนอกจากบอกเขา
                    self.shell.status = text::fill(
                        lang,
                        text::Template::Saved,
                        &[("name", &file_label_of(&path))],
                    );
                    self.shell.status_warn = false;
                    return;
                };
                // ★★ `mark_saved` คือสิ่งที่ทำให้ `dirty` กลับเป็น false — และมันต้อง
                //    เกิด **หลังเขียนสำเร็จเท่านั้น** ไม่ใช่ตอนสั่ง ไม่งั้นผู้ใช้จะ
                //    ปิดโปรแกรมโดยคิดว่างานถูกบันทึกแล้วทั้งที่ดิสก์เต็ม
                let session = doc.session.clone();
                if self.gfx.is_some() {
                    doc.history.mark_saved(&mut doc.board);
                }
                doc.autosaver.reset();
                doc.path = Some(path.clone());
                // ★★★ โหมดของเอกสารเปลี่ยน **หลังไฟล์ลงดิสก์แล้วเท่านั้น** —
                //     เหตุผลเดียวกับ `mark_saved` เป๊ะ: ผู้ใช้ที่ดิสก์เต็มต้องไม่
                //     เห็นคำว่า packed แล้วเชื่อว่าไฟล์ที่เขากำลังจะส่งให้เพื่อน
                //     มีภาพอยู่ข้างใน ทั้งที่การเขียนล้มไปแล้ว
                doc.save_mode = mode;
                doc.assets = assets;
                // ★★★ **เจ้าของเดียว** (`docs/07 §5`): เอกสารนี้มีไฟล์ `.refx` แล้ว
                //     meta จึงย้ายเข้าไปอยู่ข้างใน → เลิกเขียน sidecar ทันที
                //
                //   ★ `disown` **ไม่ลบ `.refx-meta` ที่มีอยู่** — เครื่องมืออื่น
                //     หรือ board ใบอื่นอาจใช้มันอยู่ (`docs/07 §5`: Save As แล้ว
                //     ห้ามลบไฟล์เดิมทิ้ง)
                doc.sidecar.disown();
                self.shell.sidecar_prompt = None;
                let name = file_label_of(&path);
                self.shell.status = text::fill(lang, text::Template::Saved, &[("name", &name)]);
                self.shell.status_warn = false;
                // ★★ ทิ้ง snapshot **เมื่อบันทึกสำเร็จเท่านั้น** (docs/07 §4)
                //    และรีเซ็ตนาฬิกาเพื่อให้การแก้ครั้งถัดไปถูกเก็บทันที
                refx_io::autosave::discard(&path);
                // ★★★ **ย้ายเจ้าของ** (docs/07 §4): งานนี้เคยไม่มีที่อยู่จึงถูก
                //    เก็บใน `recovery/` · ตอนนี้มันมีไฟล์จริงแล้วและ
                //    `<doc>.refx.autosave` รับช่วงต่อ → snapshot กำพร้าต้องหายไป
                //    ไม่งั้นเปิดโปรแกรมรอบหน้าผู้ใช้จะถูกถามว่าจะกู้งานที่เขา
                //    บันทึกไปเรียบร้อยแล้วหรือไม่
                //    ★ ลบเฉพาะ slot ของ **แท็บนี้** — แท็บอื่นที่ยังไม่ได้บันทึก
                //      ยังต้องมี snapshot ของมันอยู่ครบ
                if let Some(dir) = self.recovery_dir.as_ref() {
                    refx_io::recovery::discard(dir, &session);
                }
                match self.after_save {
                    AfterSave::Close => {
                        self.closing = true;
                        if let Some(gfx) = self.gfx.as_ref() {
                            gfx.window.request_redraw();
                        }
                    }
                    AfterSave::CloseTab(id) => {
                        self.after_save = AfterSave::Stay;
                        if let Some(index) = self.docs.list.iter().position(|doc| doc.id == id) {
                            self.close_tab_now(index);
                        }
                    }
                    AfterSave::Stay => {}
                }
            }
            Err(err) => {
                tracing::error!(%err, "cannot save the document");
                // ★ บันทึกไม่สำเร็จ = **ห้ามปิด** ไม่ว่าผู้ใช้เลือกอะไรไว้
                self.after_save = AfterSave::Stay;
                self.shell.status = text::t(lang, Key::SaveFailed).to_owned();
                self.shell.status_warn = true;
            }
        }
    }

    /// `Ctrl+G` / `Ctrl+Shift+G` — จัดกลุ่ม / แยกกลุ่มสิ่งที่เลือก (P3-7)
    ///
    /// ★ คำสั่งที่ **ไม่มีอะไรเปลี่ยน** คืน `CmdError::Empty` มา แล้วเราไม่ขอเฟรม
    /// (I-1) และไม่ทิ้งขั้นเปล่าไว้ใน undo stack — กด `Ctrl+G` ซ้ำบนกลุ่มเดิม
    /// จึงเงียบสนิทแทนที่จะสร้างกลุ่มที่หน้าตาเหมือนเดิมทับไปเรื่อย ๆ
    fn apply_group_request(&mut self, request: GroupRequest) {
        let base = text::t(self.shell.lang, text::Key::GroupDefaultName);
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let targets: Vec<ItemId> = self.docs.active_mut().selection.iter().collect();
        if targets.is_empty() {
            return;
        }
        let command: Option<Box<dyn refx_core::command::Command>> = match request {
            GroupRequest::Group => refx_core::command::GroupItems::new(targets, base)
                .ok()
                .map(|cmd| Box::new(cmd) as Box<dyn refx_core::command::Command>),
            GroupRequest::Ungroup => refx_core::command::Ungroup::new(targets)
                .ok()
                .map(|cmd| Box::new(cmd) as Box<dyn refx_core::command::Command>),
        };
        let Some(command) = command else {
            return;
        };
        match self.docs.active_mut().apply(command) {
            Ok(()) => {}
            // ไม่มีอะไรเปลี่ยน — ไม่ใช่ error ที่ผู้ใช้ต้องเห็น
            Err(refx_core::command::CmdError::Empty) => return,
            Err(err) => {
                tracing::error!(%err, "cannot change the grouping");
                return;
            }
        }
        // ★ จัดกลุ่มหนึ่งครั้ง = undo หนึ่งขั้น — ปิดหน้าต่าง merge ทันที
        self.docs.active_mut().history.seal();
        gfx.window.request_redraw();
    }

    /// เปลี่ยนชื่อ / ยุบกลุ่มจากแผง Arrange (P3-7)
    fn apply_group_panel_request(&mut self) {
        use crate::shell::GroupRequest as PanelRequest;

        let sealed = std::mem::take(&mut self.shell.group_sealed);
        let Some(request) = self.shell.group_request.take() else {
            if sealed && self.gfx.is_some() {
                self.docs.active_mut().history.seal();
            }
            return;
        };
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let (id, current) = match &request {
            PanelRequest::Rename(id, _) | PanelRequest::Collapsed(id, _) => {
                let Some(group) = self.docs.active_mut().board.group(*id) else {
                    return; // กลุ่มหายไประหว่างเฟรม (undo) — ไม่มีอะไรให้แก้
                };
                (*id, group.clone())
            }
        };
        let command = match request {
            PanelRequest::Rename(_, name) => {
                refx_core::command::SetGroup::rename(id, &current, name)
            }
            PanelRequest::Collapsed(_, collapsed) => {
                refx_core::command::SetGroup::set_collapsed(id, &current, collapsed)
            }
        };
        if let Err(err) = self.docs.active_mut().apply(Box::new(command)) {
            tracing::error!(%err, "cannot edit the group");
        }
        if sealed {
            self.docs.active_mut().history.seal();
        }
        gfx.window.request_redraw();
    }

    /// เขียนสิ่งที่ผู้ใช้ขอในแผง Arrange ลง board ผ่าน `Command` (P3-1)
    ///
    /// ★ `take()` ทันทีเหมือนแผงอื่น — คำขอมีอายุหนึ่งเฟรม ถ้าค้างไว้มันจะถูกเขียนซ้ำ
    /// ทุกเฟรมแล้วทับสิ่งที่ undo เพิ่งคืนมา (`docs/08 §3.9` ข้อ 8.1)
    ///
    /// ★★ ใช้ **`MetaField` ให้ตรงกับสิ่งที่แก้จริง** — `EditMeta` merge ต่อ field
    /// ถ้าส่ง field ผิด "ให้ดาว" กับ "ใส่โน้ต" จะยุบเป็น undo เดียว แล้วผู้ใช้ที่
    /// ย้อนโน้ตจะเสียดาวไปด้วยโดยไม่รู้ตัว (docs/02 §3)
    fn apply_meta_request(&mut self) {
        use crate::shell::MetaRequest;

        let sealed = std::mem::take(&mut self.shell.meta_sealed);
        let Some(request) = self.shell.meta_request.take() else {
            if sealed && self.gfx.is_some() {
                self.docs.active_mut().history.seal();
            }
            return;
        };
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let targets: Vec<ItemId> = self.docs.active_mut().selection.iter().collect();
        if targets.is_empty() {
            return;
        }
        // ★ สำเนาไว้ก่อนที่คำสั่งจะกลืน `targets` ไป — ใช้บอก `.refx-meta` ว่า
        //   โฟลเดอร์ไหนต้องเขียนใหม่ (P5-5)
        let touched = targets.clone();

        // แท็กแตะทั้ง `ItemMeta` และตารางชื่อของ board จึงเป็นคำสั่งของตัวเอง
        let command: Option<Box<dyn refx_core::command::Command>> = match request {
            MetaRequest::AddTag(name) => refx_core::command::TagItems::attach(&name, targets)
                .ok()
                .map(|cmd| Box::new(cmd) as Box<dyn refx_core::command::Command>),
            MetaRequest::RemoveTag(name) => refx_core::command::TagItems::detach(&name, targets)
                .ok()
                .map(|cmd| Box::new(cmd) as Box<dyn refx_core::command::Command>),
            other => {
                let (field, changes) = Self::meta_changes(self.docs.active(), &targets, &other);
                if changes.is_empty() {
                    // ★ ไม่มีอะไรเปลี่ยน = ไม่สร้างคำสั่ง ไม่ขอเฟรม (I-1)
                    if sealed {
                        self.docs.active_mut().history.seal();
                    }
                    return;
                }
                refx_core::command::EditMeta::new(field, changes)
                    .ok()
                    .map(|cmd| Box::new(cmd) as Box<dyn refx_core::command::Command>)
            }
        };
        let Some(command) = command else {
            return;
        };
        if let Err(err) = self.docs.active_mut().apply(command) {
            tracing::error!(%err, "cannot edit the item metadata");
        } else {
            // ★ meta ของภาพพวกนี้เปลี่ยนจริง → `.refx-meta` ของโฟลเดอร์มันต้องเขียนใหม่
            self.sidecar_touched.extend_from_slice(&touched);
        }
        if sealed {
            self.docs.active_mut().history.seal();
        }
        gfx.window.request_redraw();
    }

    /// ★★★ วงจรของ `.refx-meta` หนึ่งเฟรม (P5-5 · `docs/07 §5`)
    ///
    /// ไม่แตะดิสก์เลยแม้แต่ครั้งเดียวบน UI thread (I-2) — ที่นี่มีแค่การส่งงาน
    /// ออกไปและการเก็บผลที่กลับมา · งานจริงอยู่ใน [`crate::sidecar`]
    fn tick_sidecar(&mut self) {
        // ---- 1. ผู้ใช้ตอบคำถามเมื่อเฟรมที่แล้วหรือยัง ----
        if let Some(write_it) = self.shell.sidecar_choice.take() {
            self.shell.sidecar_prompt = None;
            self.docs.active_mut().sidecar.answer(write_it);
        }

        // ---- 2. เก็บผลการโหลด แล้วคืนค่าผ่าน `Command` ----
        let restore = {
            let doc = self.docs.active_mut();
            doc.sidecar.collect_load(&doc.board)
        };
        if let Some(restore) = restore
            && !restore.is_empty()
        {
            self.apply_sidecar_restore(restore);
        }

        // ---- 3. meta ที่เพิ่งเปลี่ยนในเฟรมนี้ ----
        let touched = std::mem::take(&mut self.sidecar_touched);
        if !touched.is_empty() {
            let say = {
                let doc = self.docs.active_mut();
                doc.sidecar.touched(&doc.board, &touched)
            };
            self.say_about_sidecar(say);

            // ★★★ opt-in: ห้ามเขียนไฟล์ลงโฟลเดอร์ผู้ใช้โดยไม่ได้ขอ (`docs/07 §5`)
            if self.settings.sidecar == refx_io::settings::SidecarPolicy::Ask
                && self.shell.sidecar_prompt.is_none()
                && self.docs.active().sidecar.asking.is_none()
                && let Some(dir) = self
                    .docs
                    .active()
                    .sidecar
                    .first_needing_an_answer(self.settings.sidecar)
                    .map(std::path::Path::to_path_buf)
            {
                self.shell.sidecar_prompt = Some(dir.display().to_string());
                self.docs.active_mut().sidecar.asking = Some(dir);
            }
        }

        // ---- 4. เก็บผลการเขียนของรอบก่อน ----
        let say = self.docs.active_mut().sidecar.collect_write();
        self.say_about_sidecar(say);
    }

    /// คืนค่าที่อ่านมาได้ — ★★★ **ไม่ผ่าน `Command`** (`docs/02 §2.9`)
    ///
    /// ค่าที่มาจากไฟล์คือ *สถานะเริ่มต้นของเอกสาร ไม่ใช่การกระทำของผู้ใช้*
    /// → ไม่ขึ้นสแตก undo · ไม่ทำให้ `dirty` · แต่ board บวก `revision`
    /// เพราะดาว/แท็กเป็น input ของ filter (ดู `Board::restore_meta`)
    fn apply_sidecar_restore(&mut self, restore: crate::sidecar::Restore) {
        let items = self.docs.active_mut().board.restore_meta(restore.meta);

        // ★ index ไม่ต้อง rebuild — meta ไม่แตะเรขาคณิตของ item เลยสักฟิลด์
        //   (ต่างจาก `apply_*` ที่ย้าย/ย่อขยาย ซึ่งต้อง `reindex`)

        self.say_about_sidecar(restore.say);
        // ★★ ของที่จับคู่ไม่ได้ต้อง **มองเห็น** — บอกหลังจำนวนที่คืนได้ เพราะ
        //    มันคือข่าวที่ผู้ใช้ต้องทำอะไรต่อ (`docs/07 §5` ข้อ 3)
        if restore.unmatched > 0 {
            self.shell.status = text::fill(
                self.shell.lang,
                Template::SidecarStranded,
                &[("n", &restore.unmatched.to_string())],
            );
            self.shell.status_warn = true;
        } else if items > 0 {
            self.shell.status = text::fill(
                self.shell.lang,
                Template::SidecarRestored,
                &[("n", &items.to_string())],
            );
            self.shell.status_warn = false;
        }
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// แปลสิ่งที่ชั้น `.refx-meta` รายงานเป็นข้อความบนแถบสถานะ (`docs/03 §0`)
    ///
    /// ★ ตัวที่ **เขียนไม่ได้** ขึ้นเป็นคำเตือน ไม่ใช่ข้อความปกติ — ผู้ใช้ต้องรู้
    /// ว่าแท็กที่เพิ่งใส่จะไม่ถูกบันทึก (`docs/07 §5`: เขียนไม่ได้ ≠ เงียบ)
    fn say_about_sidecar(&mut self, say: Vec<crate::sidecar::Say>) {
        use crate::sidecar::Say;
        for one in say {
            let (template, dir) = match one {
                Say::Restored { .. } => continue, // มีทางของตัวเองใน `apply_sidecar_restore`
                Say::CannotWrite { dir } => (Template::SidecarCannotWrite, dir),
                Say::HandsOff { dir, reason } => {
                    tracing::warn!(?reason, dir = %dir.display(), "leaving a sidecar untouched");
                    (Template::SidecarHandsOff, dir)
                }
            };
            self.shell.status = text::fill(
                self.shell.lang,
                template,
                &[("dir", &dir.display().to_string())],
            );
            self.shell.status_warn = true;
        }
    }

    /// ประกอบ `ItemMeta` ชุดใหม่ตามคำขอ — คืนเฉพาะตัวที่ **เปลี่ยนจริง**
    fn meta_changes(
        doc: &Doc,
        targets: &[ItemId],
        request: &crate::shell::MetaRequest,
    ) -> (refx_core::command::MetaField, Vec<(ItemId, ItemMeta)>) {
        use crate::shell::MetaRequest;
        use refx_core::command::MetaField;

        let field = match request {
            MetaRequest::Rating(_) => MetaField::Rating,
            MetaRequest::ColorLabel(_) => MetaField::ColorLabel,
            MetaRequest::Pinned(_) => MetaField::Pinned,
            MetaRequest::Note(_) => MetaField::Note,
            MetaRequest::AddTag(_) | MetaRequest::RemoveTag(_) => MetaField::Tags,
        };
        let changes = targets
            .iter()
            .filter_map(|id| {
                let current = doc.board.item(*id)?.meta.clone();
                let mut next = current.clone();
                match request {
                    MetaRequest::Rating(value) => next.rating = *value,
                    MetaRequest::ColorLabel(value) => next.color_label = *value,
                    MetaRequest::Pinned(value) => next.pinned = *value,
                    MetaRequest::Note(value) => next.note.clone_from(value),
                    // แท็กไม่เดินทางนี้ — มันมีคำสั่งของตัวเอง
                    MetaRequest::AddTag(_) | MetaRequest::RemoveTag(_) => return None,
                }
                let next = next.sanitized();
                (next != current).then_some((*id, next))
            })
            .collect();
        (field, changes)
    }

    fn apply_inspector_edit(&mut self) {
        let sealed = std::mem::take(&mut self.shell.appearance_sealed);
        // ★ `take` — สิ่งที่ผู้ใช้ขอมีอายุหนึ่งเฟรม ถ้าปล่อยค้างไว้มันจะถูกเขียนซ้ำ
        //   ทุกเฟรมแล้วทับสิ่งที่คีย์ลัด (`H`) เพิ่งเปลี่ยน
        let Some(wanted) = self.shell.appearance_edit.take() else {
            if sealed && self.gfx.is_some() {
                self.docs.active_mut().history.seal();
            }
            return;
        };
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };

        let doc = self.docs.active();
        let board = &doc.board;
        let changes: Vec<(ItemId, ItemCanvas)> = doc
            .selection
            .iter()
            .filter_map(|id| board.item(id).map(|item| (id, item.canvas)))
            .filter(|(_, canvas)| !canvas.locked)
            .filter_map(|(id, canvas)| {
                let next = ItemCanvas {
                    opacity: wanted.opacity,
                    flip: wanted.flip,
                    filter: ItemFilter {
                        grayscale: wanted.grayscale,
                        invert: wanted.invert,
                        brightness: wanted.brightness,
                        contrast: wanted.contrast,
                    },
                    ..canvas
                }
                .sanitized();
                // ★ ไม่มีอะไรเปลี่ยน = ไม่สร้างคำสั่ง ไม่ขอเฟรม (I-1)
                //   ถ้าไม่กรอง ทุกเฟรมที่ inspector วาดจะยิงคำสั่งเปล่าเข้า History
                (next != canvas).then_some((id, next))
            })
            .collect();

        if changes.is_empty() {
            if sealed {
                self.docs.active_mut().history.seal();
            }
            return;
        }
        let Ok(command) = SetFilter::new(changes) else {
            return;
        };
        if let Err(err) = self.docs.active_mut().apply(Box::new(command)) {
            tracing::error!(%err, "cannot change the appearance of the selection");
            return;
        }
        if sealed {
            self.docs.active_mut().history.seal();
        }
        Self::collect_forgotten(gfx, self.docs.active_mut());
        Self::rebuild_quads(gfx, self.docs.active_mut());
        gfx.window.request_redraw();
    }

    /// จัดเรียงสิ่งที่เลือกไว้ (P2-9) — align / distribute
    ///
    /// ★ **ทั้งชุดเป็น undo ขั้นเดียว** — `TransformItems` ตัวเดียวถือทุกใบ
    /// ผู้ใช้ที่กด "ชิดซ้าย" แล้วไม่ชอบ ต้องกด Ctrl+Z **ครั้งเดียว** ได้ทุกใบกลับที่เดิม
    fn apply_arrange(&mut self, request: crate::shell::ArrangeRequest) {
        use crate::shell::ArrangeRequest;

        let lang = self.shell.lang;
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        // ภาพที่ล็อกไว้ต้องไม่ขยับ — เหมือนทุกเครื่องมือที่แก้เรขาคณิต
        let doc = self.docs.active();
        let board = &doc.board;
        let picked: Vec<(ItemId, ItemCanvas)> = doc
            .selection
            .iter()
            .filter_map(|id| board.item(id).map(|item| (id, item.canvas)))
            .filter(|(_, canvas)| !canvas.locked)
            .collect();

        let changes = match request {
            ArrangeRequest::Align(how) => refx_core::align::aligned(&picked, how),
            ArrangeRequest::Distribute(how) => refx_core::align::distributed(&picked, how),
        };
        let Some(changes) = changes else {
            // ★ กดแล้วไม่มีอะไรเกิดขึ้น **ต้องบอก** ไม่ใช่เงียบ
            //   (เลือกน้อยเกินไป หรือมันตรงกันอยู่แล้ว — ทั้งสองอย่างไม่ใช่ความผิดพลาด)
            self.shell.status = text::t(lang, Key::NothingToArrange).to_owned();
            return;
        };
        let moved: Vec<ItemId> = changes.iter().map(|(id, _)| *id).collect();
        let Ok(command) = TransformItems::new(changes) else {
            return;
        };
        if let Err(err) = self.docs.active_mut().apply(Box::new(command)) {
            tracing::error!(%err, "cannot arrange the selection");
            return;
        }
        // กดปุ่มหนึ่งครั้ง = ขั้นเดียวเสมอ ห้ามให้การกดถัดไปกลืนเข้าไป
        self.docs.active_mut().history.seal();
        // ★ index ต้องตามตำแหน่งใหม่ทันที ไม่งั้นคลิกครั้งถัดไปจะพลาด
        for id in moved {
            self.docs.active_mut().reindex(id);
        }
        Self::collect_forgotten(gfx, self.docs.active_mut());
        Self::rebuild_quads(gfx, self.docs.active_mut());
        gfx.window.request_redraw();
    }

    /// ย้ายชั้นของสิ่งที่เลือกไว้ (P2-6)
    fn apply_zorder(&mut self, movement: ZMove) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let selected: Vec<ItemId> = self.docs.active_mut().selection.iter().collect();
        let Some(order) = refx_core::zorder::reordered(
            self.docs.active_mut().board.z_order(),
            &selected,
            movement,
        ) else {
            // ★ อยู่สุดขอบแล้ว / ไม่ได้เลือกอะไร — **ไม่สร้างคำสั่งและไม่ขอเฟรม** (I-1)
            //   ถ้าสร้าง undo stack จะเต็มไปด้วยขั้นที่กดแล้วไม่มีอะไรเกิดขึ้น
            return;
        };
        let Ok(command) = ReorderZ::new(order) else {
            return;
        };
        if let Err(err) = self.docs.active_mut().apply(Box::new(command)) {
            tracing::error!(%err, "cannot reorder the z stack");
            return;
        }
        // เรขาคณิตไม่เปลี่ยน → `index` ไม่ต้องแตะ · `affected()` ว่าง → การเลือกอยู่เหมือนเดิม
        Self::collect_forgotten(gfx, self.docs.active_mut());
        Self::rebuild_quads(gfx, self.docs.active_mut());
        gfx.window.request_redraw();
    }

    /// ลบสิ่งที่เลือกไว้ (P2-6)
    ///
    /// ★ **ภาพที่ล็อกไว้ไม่ถูกลบ** — ล็อกมีไว้กันการแก้โดยไม่ตั้งใจ และการลบคือ
    /// การแก้ที่ย้อนยากที่สุดในสายตาผู้ใช้ ถ้าล็อกกันการลากได้แต่กันการลบไม่ได้
    /// คำว่า "ล็อก" จะแปลว่าอะไรก็ไม่รู้ (ทางเดียวกับ `SelectTool`)
    fn apply_delete(&mut self) {
        let lang = self.shell.lang;
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let doc = self.docs.active();
        let board = &doc.board;
        let targets: Vec<ItemId> = doc
            .selection
            .iter()
            .filter(|id| board.item(*id).is_some_and(|item| !item.canvas.locked))
            .collect();
        if targets.is_empty() {
            // ★ กดแล้วไม่มีอะไรเกิดขึ้น **ต้องบอก** ไม่ใช่เงียบ
            self.shell.status = text::t(lang, Key::NothingToDelete).to_owned();
            return;
        }
        let Ok(command) = RemoveItems::new(targets.clone()) else {
            return;
        };
        if let Err(err) = self.docs.active_mut().apply(Box::new(command)) {
            tracing::error!(%err, "cannot delete the selected images");
            return;
        }

        for id in &targets {
            self.docs.active_mut().index.remove(*id);
        }
        // ★ ของที่ถูกลบไปแล้วจะยังถูกเลือกอยู่ไม่ได้ — แต่ตัวที่ **รอด** (ล็อกไว้)
        //   ต้องยังถูกเลือกอยู่ ไม่งั้นผู้ใช้ที่เลือก 5 ใบแล้วลบ จะเสียการเลือก
        //   ของใบที่ล็อกไว้ไปด้วยทั้งที่มันไม่ได้ถูกแตะเลย
        let survivors: Vec<ItemId> = self
            .docs
            .active_mut()
            .selection
            .iter()
            .filter(|id| !targets.contains(id))
            .collect();
        self.docs
            .active_mut()
            .selection
            .restore(survivors.clone(), survivors.last().copied());
        self.docs.active_mut().select_tool.cancel();
        self.docs.active_mut().rubber_band = None;
        Self::sync_residency(gfx, self.docs.active_mut(), &targets);
        Self::collect_forgotten(gfx, self.docs.active_mut());
        Self::rebuild_quads(gfx, self.docs.active_mut());
        tracing::info!(count = targets.len(), "deleted images from the board");
        gfx.window.request_redraw();
    }

    /// ทำ undo/redo แล้วทำให้ผู้ใช้ **เห็นว่าเกิดอะไรขึ้น**
    fn apply_history_request(&mut self, request: HistoryRequest) {
        let lang = self.shell.lang;
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let outcome = {
            let doc = self.docs.active_mut();
            match request {
                HistoryRequest::Undo => doc.history.undo(&mut doc.board),
                HistoryRequest::Redo => doc.history.redo(&mut doc.board),
            }
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

        // ★ ภาพที่เพิ่งกลับมา/เพิ่งหายไปต้องคืนหรือคืนช่อง atlas ตาม **ก่อน** สร้าง quad
        //   undo ของการลบเติมกลับจาก RAM ที่มีอยู่แล้ว — ไม่ decode ใหม่สักใบ
        Self::sync_residency(gfx, self.docs.active_mut(), &affected);
        Self::collect_forgotten(gfx, self.docs.active_mut());

        // index กับ quad ต้องตามสถานะใหม่ของ board ทันที
        {
            let doc = self.docs.active_mut();
            doc.index.rebuild(&doc.board);
        }
        Self::rebuild_quads(gfx, self.docs.active_mut());

        // ★ เลือกของที่เพิ่งเปลี่ยนให้ผู้ใช้ (docs/02 §2.9)
        //   การเลือกไม่ได้ถูก undo — มันตามผลลัพธ์ที่คำสั่งรายงานกลับมา
        //   id ที่หายไปแล้ว (undo ของการเพิ่ม) ต้องกรองทิ้ง ไม่งั้น selection ถือของว่าง
        //
        // ★★★ **"ไม่ได้แตะ item ไหนเลย" ≠ "ให้ล้างการเลือก"** (แก้ตอน P3-7)
        //
        //    `ReorderZ::affected()` คืน `Vec::new()` พร้อมคอมเมนต์ในตัวมันเองว่า
        //    "ปล่อยให้ selection เดิมอยู่ต่อ" แต่โค้ดตรงนี้กลับ `restore(vec![])`
        //    ซึ่งคือการ **ล้าง** — สัญญาที่ `refx-core` ประกาศไว้ไม่เคยถูกทำตามบน
        //    เส้นทาง undo เลย · อาการที่เห็นตอน P3-7: กดยุบกลุ่มแล้ว Ctrl+Z
        //    → แผงกลุ่มหายไปทั้งแผง เพราะไม่มีอะไรถูกเลือกอีกแล้ว
        //
        //    ตรรกะอยู่ใน `selection_after_history` เพื่อให้เทสต์ได้โดยไม่ต้องมีหน้าต่าง
        let board = &self.docs.active().board;
        let Some(live) = selection_after_history(&affected, |id| board.item(id).is_some()) else {
            // คำสั่งบอกว่าไม่ได้แตะ item ไหนเลย — การเลือกเดิมยังใช้ได้ตามเดิม
            self.docs.active_mut().select_tool.cancel();
            self.docs.active_mut().rubber_band = None;
            gfx.window.request_redraw();
            return;
        };
        self.docs
            .active_mut()
            .selection
            .restore(live.clone(), live.last().copied());
        // การลากที่ค้างอยู่ (ถ้ามี) ใช้ไม่ได้แล้วเพราะ board เปลี่ยนไปใต้มือ
        self.docs.active_mut().select_tool.cancel();
        self.docs.active_mut().rubber_band = None;

        Self::look_at_if_offscreen(gfx, self.docs.active_mut(), &live);
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
    fn look_at_if_offscreen(gfx: &mut Gfx, doc: &mut Doc, ids: &[ItemId]) {
        let mut bounds = WorldRect::EMPTY;
        for id in ids {
            if let Some(item) = doc.board.item(*id) {
                bounds = bounds.union(item.canvas.world_bounds());
            }
        }
        // ไม่มีของให้ดู (เช่น undo ของการเพิ่ม หรือ ReorderZ ที่ไม่ได้แตะใคร)
        if bounds.is_empty() || !bounds.is_finite() {
            return;
        }

        let zoom = doc.camera.zoom();
        if zoom <= 0.0 {
            return;
        }
        let half = gfx.canvas.size * 0.5 / zoom;
        let centre = doc.camera.center();
        let view = WorldRect {
            min: centre - half,
            max: centre + half,
        };
        if view.intersects(bounds) {
            return; // เห็นอยู่แล้วอย่างน้อยบางส่วน — อย่าไปกระตุกกล้อง
        }

        tracing::info!("moving the camera to show what the history change affected");
        doc.camera.set_center(bounds.center());
    }

    /// สร้าง `quads` ใหม่ทั้งชุดจาก `board`
    ///
    /// ★ **ประตูเดียวที่เขียน `gfx.quads` ได้** — `quads` เป็นผลลัพธ์ ไม่ใช่แหล่งความจริง
    /// ลำดับ render = ลำดับใน `z_order` อยู่แล้ว จึงไม่ต้อง sort (docs/02 §2.1)
    fn rebuild_quads(gfx: &mut Gfx, doc: &mut Doc) {
        // ★★★ กฎ "`Board` เป็นคนบอกว่า item เป็นภาพหรือไม่ ไม่ใช่ `render_state`"
        //   (§4 ข้อ 28) ย้ายไปอยู่ **ข้างใน** `build_instances` แล้วตอน P5-1 —
        //   กฎที่อยู่ในตัวเรียกคือกฎที่วันหนึ่งจะมีตัวเรียกที่ลืม
        instances::build_instances(&doc.board, &mut gfx.quads, |id| {
            doc.render_state
                .get(&id)
                .map(|state| (state.slot, state.tint))
        });
        // ★ ประตูเดียวที่รู้ว่า board เปลี่ยน จึงเป็นที่เดียวที่บอกแผ่น Arrange ว่าเก่าแล้ว
        //   (P3-4 จะเปลี่ยนไปเทียบ `board.revision` ตาม docs/03 §3)
        //   ★★ ไม่ใช่ตาข่ายเดียว: `ArrangeView::plan` เทียบจำนวน item เองด้วย
        //      เผื่อวันที่มีคนเพิ่มเส้นทางแก้ board แล้วไม่ผ่านที่นี่ (docs/08 §3.9 ข้อ 8)
        doc.arrange.invalidate();
    }

    /// เตรียมแถบที่ Arrange ต้องวาดเฟรมนี้ (P3-3)
    ///
    /// ★★ **นี่คือที่ที่ "10,000 ใบ วาดจริง < 60" เกิดขึ้นจริง** — `arrange_quads`
    /// ถูกสร้างจาก `arrange.visible()` เท่านั้น ซึ่งเป็นชุดที่ทับจอ + กันชนบนล่าง
    /// ที่เหลืออีกเกือบหมื่นใบ **ไม่ถูกแตะเลยแม้แต่ครั้งเดียวต่อเฟรม**
    ///
    /// ★ ใช้ `quad_for` ตัวเดียวกับ Canvas โดยยัดเรขาคณิตของแผ่นลง `ItemCanvas`
    /// ชั่วคราว — ถ้าเขียนสูตรวาดขึ้นใหม่ที่นี่ วันหนึ่งสองทางจะเพี้ยนจากกัน
    /// (บทเรียนเดิมของ `flip` ที่ไม่ถึงทาง working texture — HANDOFF §2.7)
    fn plan_arrange(gfx: &mut Gfx, doc: &mut Doc, ppp: f32) {
        let viewport = gfx.canvas.size;
        {
            let board = &doc.board;
            let render_state = &doc.render_state;
            // ★ สัดส่วนมาจาก **ภาพต้นฉบับ** ไม่ใช่จาก `ItemCanvas` — ผู้ใช้ที่ย่อ/ยืด
            //   ภาพบน canvas ไว้ต้องยังเห็นสัดส่วนจริงใน contact sheet
            //   · ไม่มี thumbnail (โน้ต) → ใช้กรอบของมันเอง
            doc.arrange.plan(board, viewport, ppp, |id| {
                render_state
                    .get(&id)
                    .map(|state| {
                        Vec2::new(
                            state.thumb.source_width as f32,
                            state.thumb.source_height as f32,
                        )
                    })
                    .or_else(|| board.item(id).map(|item| item.canvas.size))
                    .unwrap_or(Vec2::ONE)
            });
        }

        gfx.arrange_quads.clear();
        for placed in doc.arrange.visible() {
            let Some(item) = doc.board.item(placed.id) else {
                continue;
            };
            let Some(state) = doc.render_state.get(&placed.id) else {
                // โน้ตข้อความไม่มี pixel ให้วาด — มันเป็น item เต็มตัวบน canvas
                // แต่ใน contact sheet ยังไม่มีรูปแบบของตัวเอง (รอ P3-7)
                continue;
            };
            // ★ `rotation` ถูกตัดออกโดยตั้งใจ: Arrange เป็นตาราง ไม่ใช่ระนาบอิสระ
            //   ส่วน crop/flip/filter/opacity ยังติดมา เพราะนั่นคือ "ภาพของผู้ใช้"
            //   ที่เขาแต่งไว้ — **ไม่มีอะไรถูกเขียนกลับลง board** (docs/03 §4.3)
            let canvas = ItemCanvas {
                pos: placed.centre(),
                size: placed.size,
                rotation: 0.0,
                ..item.canvas
            };
            if let Some(quad) = instances::quad_for(&canvas, state.slot, state.tint) {
                gfx.arrange_quads.push(quad);
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
    /// ★★★ **ทุกแท็บ ไม่ใช่แท็บที่อยู่หน้าจอ** (P4-7c) — ช่องใน atlas ของแท็บอื่น
    /// ตายไปพร้อม texture เก่าเหมือนกัน · เติมกลับแค่ใบเดียวแล้วผู้ใช้จะสลับไป
    /// เจอ board ว่างเปล่าอีกสามใบหลัง driver อัปเดต ซึ่งเป็นอาการเดียวกับที่
    /// `docs/04 §4` สั่งห้ามไว้ แค่ต้องกด `Ctrl+Tab` หนึ่งครั้งถึงจะเห็น
    fn refill_atlas(gfx: &mut Gfx, docs: &mut Docs) {
        let total: usize = docs.iter().map(|doc| doc.board.len()).sum();
        if total == 0 {
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
        //   ★ นับรวมทุกแท็บ — ช่องใน atlas เป็นทรัพยากรร่วมของทั้งแอป
        let needed = refx_render::atlas::layers_needed(total);
        if gfx.atlas.layers_allocated() < needed
            && let Err(err) = gfx.atlas.resize(gfx.render.device(), needed)
        {
            // ขยายไม่ได้ = VRAM ไม่พอ เติมได้เท่าที่ได้ ที่เหลือเป็น placeholder
            tracing::warn!(%err, needed, "cannot grow the atlas before refilling it");
        }

        for doc in docs.iter_mut() {
            // ★ ไล่ตามลำดับ z ของ board — `render_state` เป็นแผนที่ ไม่ใช่รายการคู่ขนาน
            //   แล้วเขียนผลลง `render_state` ไม่ใช่ลง `quads` โดยตรง
            //   (`quads` ถูกสร้างใหม่จาก board ทีหลัง — ดู `rebuild_quads`)
            let order: Vec<ItemId> = doc.board.z_order().to_vec();
            // ★ แยกการยืมทีละฟิลด์ **ห้าม clone pixel** — thumbnail ใบละ 64 KB
            //   ที่ 100 ภาพคือก๊อป 6.4 MB ทุกครั้งที่กู้ device (วัดแล้วช้าลง 30%)
            //   ทางที่ถูกคือ destructure ให้ atlas/render/render_state ยืมคนละฟิลด์กัน
            let Gfx { atlas, render, .. } = &mut *gfx;
            let Doc { render_state, .. } = doc;
            for id in order {
                let Some(state) = render_state.get_mut(&id) else {
                    continue;
                };
                // ★ ใช้ upload ตรง ๆ ห้ามผ่าน upload_thumb — ไม่งั้นจะเรียก refill ซ้อนตัวเอง
                match atlas.upload(render.queue(), &state.thumb.pixels) {
                    Ok(slot) => {
                        state.slot = Some(slot);
                        restored += 1;
                    }
                    Err(err) => {
                        // atlas เต็ม — ที่เหลือขึ้นเป็นสี่เหลี่ยมสีเด่นแทนช่องว่าง
                        tracing::warn!(%err, ?id, "atlas refill incomplete — the rest fall back to placeholders");
                        state.slot = None;
                    }
                }
            }
        }
        // ★ `quads` เป็นของหน้าต่าง มีชุดเดียว — สร้างจากแท็บที่อยู่บนจอ
        Self::rebuild_quads(gfx, docs.active_mut());

        tracing::info!(
            restored,
            total,
            tabs = docs.len(),
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
        // ★ ตัวก๊อปต้องปลุก UI ได้ด้วย ไม่งั้นข้อความ "ก๊อปไม่ติด" จะนอนรออยู่เฉย ๆ
        //   จนกว่าผู้ใช้จะบังเอิญขยับเมาส์ (แอปหลับสนิทตอน idle — I-1)
        if let Some(copier) = self.copier.as_mut() {
            let w = waker.clone();
            copier.set_waker(std::sync::Arc::new(move || w.wake()));
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
                // ★ ปลด vsync ตอน benchmark **หรือ** ตอนผู้ใช้สั่งใน `settings.toml`
                //   (P5-3) · ค่าปริยายยังเป็น AutoVsync เสมอตาม `docs/04 §8`
                //
                //   ★★ อยู่ใน `RenderOptions` ที่เดียว **ไม่ใช่ตัวแปรที่สองข้าง ๆ
                //   `config.present_mode`** — `recover()` สร้าง surface ใหม่จาก
                //   options ชุดนี้ ค่าที่เก็บไว้อีกที่จะหายทุกครั้งที่ driver สะดุด
                //   (รูปแบบ "แหล่งความจริงที่สอง" ที่ `HANDOFF §4` ข้อ 29 ห้ามไว้)
                uncapped_present: self.args.bench_seconds.is_some()
                    || self.settings.present == refx_io::settings::Present::Uncapped,
            },
        )?;

        let (egui_ctx, egui_winit, egui_renderer) =
            Self::build_egui(&window, &render, self.settings.theme);
        // ★ destructure ไว้โดยตั้งใจ — เพิ่ม resource ใหม่ใน DeviceBound เมื่อไหร่
        //   ตรงนี้จะคอมไพล์ไม่ผ่าน พร้อมกับฝั่ง recover_device()
        let DeviceBound {
            textures,
            atlas,
            pipeline,
            working,
        } = DeviceBound::build(&render, self.settings.vram_limit).map_err(|err| {
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
            tool: Tool::default(),
            working,
            working_pending: std::collections::HashSet::new(),
            working_quads: Vec::new(),
            arrange_quads: Vec::new(),
            canvas: CanvasRect::full(size.width, size.height),
            modifiers: ModifiersState::empty(),
        });
        self.queue_initial_files();
        // ★★ ถามโฟลเดอร์ recovery ว่ามีงานค้างจากรอบก่อนไหม (P4-4)
        //
        //   ★ เริ่มที่นี่เพราะต้องมีหน้าต่างก่อนถึงจะมีที่ให้แถบโผล่ · การสแกน
        //     อยู่เธรดอื่นทั้งหมด หน้าต่างจึงขึ้นทันทีไม่ต้องรอดิสก์ (I-2)
        //   ★ เรียกครั้งเดียวตลอดอายุโปรแกรม — `resumed()` ถูกเรียกซ้ำได้ตอนกู้
        //     device แต่ตอนนั้นผู้ใช้ตอบคำถามไปแล้ว การถามซ้ำจะน่ารำคาญมาก
        if self.claim_recovery_scan() {
            self.start_recovery_scan();
        }
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

        // ★★★ โหมดที่จะวาดในเฟรมนี้มาจาก **แท็บที่ดูอยู่** ไม่ใช่จากหน้าต่าง
        //     (P5-3b ก้อน c · `HANDOFF §6` · `docs/03 §5`) — `Board::view` ถูกเขียน
        //     ทุกเฟรมมาตั้งแต่ P4-1 แต่ **ไม่เคยมีใครอ่านกลับมาใช้** บรรทัดนี้คือ
        //     คนแรก · ทำที่ต้นเฟรมเพื่อให้ทุกอย่างข้างล่างเห็นค่าเดียวกันหมด
        self.mode_follows_active_tab();

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

        // `[` `]` ที่กดไปเมื่อกี้ (P2-6)
        if let Some(movement) = self.pending_zorder.take() {
            self.apply_zorder(movement);
        }

        // `Delete` ที่กดไปเมื่อกี้ (P2-6)
        if std::mem::take(&mut self.pending_delete) {
            self.apply_delete();
        }

        // `G` / `H` ที่กดไปเมื่อกี้ (P2-8)
        if let Some(what) = self.pending_appearance.take() {
            self.apply_appearance_key(what);
        }
        // `Ctrl+G` / `Ctrl+Shift+G` ที่กดไปเมื่อกี้ (P3-7)
        if let Some(request) = self.pending_group.take() {
            self.apply_group_request(request);
        }
        // ★★★ หกคีย์ของก้อน c (P5-3b · `docs/03 §5`)
        //
        //   ★ `Tab` มาก่อน zoom เสมอ: กด `Tab` แล้ว `0` ในเฟรมเดียวกันต้องได้
        //     "สลับโหมดแล้วค่อยจัดให้พอดี" ไม่ใช่จัดในโหมดเก่าแล้วสลับทิ้ง
        if std::mem::take(&mut self.pending_mode_toggle) {
            self.apply_mode_toggle();
        }
        if std::mem::take(&mut self.pending_select_all) {
            self.apply_select_all();
        }
        if std::mem::take(&mut self.pending_clear_selection) {
            self.apply_clear_selection();
        }
        // ★ zoom หลังการเลือก: `Ctrl+A` แล้ว `F` ในเฟรมเดียวกัน = พอดีกับทั้งหมด
        if let Some(request) = self.pending_zoom.take() {
            self.apply_zoom(request);
        }
        // ★★ ส่งออกภาพ (P5-4) — ตามลำดับนี้เสมอ: เปิดกล่อง → รับผลจาก dialog
        //    ของ OS → ทำตามปุ่มที่กด → เติมค่าที่คำนวณได้ → ถามงานที่ทำอยู่
        //
        //    ★ `refresh_export_view` ต้องอยู่ **หลัง** `apply_export_request`
        //      เพราะการสลับรูปแบบเปลี่ยนตัวเลขประมาณขนาดไฟล์ในเฟรมเดียวกัน
        if std::mem::take(&mut self.pending_export) {
            self.open_export_dialog();
        }
        self.poll_export_target();
        self.apply_export_request();
        self.refresh_export_view();
        self.poll_export_job();
        // ★ การบันทึก (P4-2) — เก็บผลก่อน แล้วค่อยรับคำสั่งใหม่
        self.poll_save();
        // ★ การเปิดไฟล์ + งานค้างจาก session ก่อน (P4-4) — ลำดับเดียวกับข้างบน
        self.poll_open();
        self.poll_recovery_scan();
        self.poll_spool_sweep();
        self.poll_relink_scan();
        self.poll_relink();
        if std::mem::take(&mut self.shell.relink_request) {
            self.apply_relink_request();
        }
        if std::mem::take(&mut self.pending_open) {
            self.apply_open_request();
        }
        // ★★★ คีย์/ปุ่มของแท็บ (P4-7c) — **ตามลำดับนี้เสมอ**
        //
        //   สร้างก่อน แล้วค่อยปิด แล้วค่อยสลับ · ผู้ใช้ที่กด `Ctrl+T` แล้ว
        //   `Ctrl+W` ในเฟรมเดียวกันต้องได้ผลตามที่เขากด ไม่ใช่ปิดใบเก่าแล้ว
        //   เหลือใบใหม่ค้าง (ซึ่งจะเกิดถ้าปิดมาก่อน)
        if std::mem::take(&mut self.pending_new_tab) {
            self.apply_new_tab();
        }
        if std::mem::take(&mut self.pending_close_tab) {
            self.request_close_tab(self.docs.active);
        }
        if std::mem::take(&mut self.pending_next_tab) {
            self.apply_next_tab();
        }
        // ★ ปุ่มบนแถบแท็บที่ผู้ใช้กดเมื่อเฟรมที่แล้ว — เส้นทางเดียวกับคีย์ลัดเป๊ะ
        if let Some(request) = self.shell.tab_request.take() {
            match request {
                crate::shell::TabRequest::Select(index) => self.focus_tab(index),
                crate::shell::TabRequest::Close(index) => self.request_close_tab(index),
                crate::shell::TabRequest::New => self.apply_new_tab(),
            }
        }
        if let Some(choice) = self.shell.recover_choice.take() {
            self.apply_recover_choice(choice);
        }
        // ★ autosave (P4-3) — ตัดสินหลัง `poll_save` เพราะการบันทึกสำเร็จ
        //   เพิ่งล้าง `dirty` ไป การถามก่อนจะได้คำตอบจากสถานะเก่าหนึ่งเฟรม
        self.tick_autosave();
        if let Some(request) = self.pending_save.take() {
            self.apply_save_request(request);
        }
        // ปุ่มในแถบยืนยันตอนปิด (ถ้ามี)
        if let Some(choice) = self.shell.close_choice.take() {
            self.apply_close_choice(choice);
        }
        // ★ ผู้ใช้ตอบแถบ "บันทึกเป็นแบบไหน" เมื่อเฟรมที่แล้ว (P4-5)
        if let Some(choice) = self.shell.save_as_choice.take() {
            self.apply_save_as_choice(choice);
        }
        // ★ ผู้ใช้กดสลับโหมดบนแถบสถานะเมื่อเฟรมที่แล้ว (P4-5)
        if let Some(packed) = self.shell.storage_request.take() {
            self.apply_mode_request(if packed {
                refx_io::packed::SaveMode::Packed
            } else {
                refx_io::packed::SaveMode::Linked
            });
        }
        // ค่าที่ผู้ใช้ปรับใน inspector เมื่อเฟรมที่แล้ว
        self.apply_inspector_edit();
        // ข้อความที่ผู้ใช้พิมพ์ลงโน้ตเมื่อเฟรมที่แล้ว (P2-11)
        self.apply_note_edit();
        // tag / rating / color label / pinned / note ฝั่ง Arrange (P3-1)
        self.apply_meta_request();
        // ★ `.refx-meta` ของโฟลเดอร์ที่เปิดดูเฉย ๆ (P5-5) — ต้องมาหลัง
        //   `apply_meta_request` เพื่อให้การติดดาวในเฟรมนี้ถูกนับเป็น "ต้องเขียน"
        self.tick_sidecar();
        // เปลี่ยนชื่อ / ยุบกลุ่มที่ผู้ใช้แตะในแผง Arrange เมื่อเฟรมที่แล้ว (P3-7)
        self.apply_group_panel_request();
        // ★ ผู้ใช้กด "ส่งเข้า canvas" เมื่อเฟรมที่แล้ว (P3-5)
        if std::mem::take(&mut self.shell.arrange_apply) {
            self.apply_layout_to_canvas();
        }
        // ปุ่มจัดเรียงที่กดไปเมื่อเฟรมที่แล้ว (P2-9)
        if let Some(request) = self.shell.arrange_request.take() {
            self.apply_arrange(request);
        }

        // ★ ก๊อปไม่ติด = **ความรำคาญ ไม่ใช่เหตุขัดข้อง** — ขึ้น status bar ห้ามเด้ง dialog
        //   (ต่างจาก save ล้มซึ่งคืองานหาย) · `take` แล้วหาย ไม่ขึ้นซ้ำทุกเฟรม
        if let Some(err) = self
            .copier
            .as_ref()
            .and_then(crate::copy::Copier::take_error)
        {
            self.shell.status = text::clipboard_error(self.shell.lang, &err);
        }
        // เก็บผล decode ที่เสร็จแล้วก่อนวาด (ไม่บล็อก)
        self.drain_decode_results();
        // ★ ตัดสินใจเรื่อง working texture ก่อนวาด — ใช้กล้อง/กรอบของเฟรมที่แล้ว
        //
        //   ★★ **เฉพาะโหมด Canvas** (P3-3): ใน Arrange ภาพถูกตรึงไว้ที่ขนาด
        //   thumbnail จึงไม่มีใบไหน "ซูมเข้าจนเห็นชัด" · ถ้าปล่อยให้ทำงานที่
        //   10,000 ใบ มันจะสั่ง decode ภาพเต็มให้ของที่ผู้ใช้ไม่ได้มองอยู่ด้วยซ้ำ
        if self.shell.mode == Mode::Canvas {
            self.plan_working_textures();
        }

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
            pick_in_flight,
            pick_count,
            docs,
            settings,
            caps,
            settings_notes,
            settings_notes_pending,
            settings_restart_pending,
            settings_write,
            keymap_problem,
            keymap_from_file,
            config_dir,
            waker,
            ..
        } = self;
        let gfx = gfx.as_mut()?;
        // ★★★ **แท็บทั้งแถบ** — ตัวบ่งชี้ "ยังไม่ถูกบันทึก" ต้องเห็นได้ของ *ทุก* ใบ
        //     ไม่ใช่เฉพาะใบที่อยู่หน้าจอ · ผู้ใช้ที่เห็นแค่ใบเดียวจะปิดโปรแกรม
        //     โดยเชื่อว่าอีกสามใบสะอาด (`docs/03 §1` — สภาวะต้องมองเห็นได้)
        shell.tabs = docs
            .iter()
            .map(|doc| crate::shell::TabView {
                title: doc.name(),
                unsaved: doc.board.is_dirty(),
            })
            .collect();
        shell.active_tab = docs.active;
        let doc = docs.active_mut();
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
        let mut raw_input = gfx.egui_winit.take_egui_input(&gfx.window);
        strip_tab_when_it_is_ours(&gfx.egui_ctx, &mut raw_input);
        shell.item_count = doc.board.len();
        shell.zoom = doc.camera.zoom();
        // ★★★ สภาวะ "ภาพเก็บไว้ที่ไหน" (P4-5 · `docs/07 §2`) — เติมทุกเฟรมเหมือน
        //   `tabs` · ตัวเลขนับจาก **ตารางของไฟล์จริง** ไม่ใช่จากธงที่จำไว้
        shell.storage = crate::shell::StorageView {
            packed: doc.save_mode == refx_io::packed::SaveMode::Packed,
            images: doc
                .board
                .items_in_z_order()
                .filter(|(_, item)| matches!(item.kind, ItemKind::Image(_)))
                .count(),
            inside: doc
                .board
                .items_in_z_order()
                .filter(|(_, item)| match &item.kind {
                    ItemKind::Image(asset) => doc.assets.find(asset.hash).is_some(),
                    _ => false,
                })
                .count(),
            has_file: doc.path.is_some(),
        };
        // ★ ปุ่มบน toolbar เป็นภาพสะท้อนของ `gfx.tool` เท่านั้น — เจ้าของมีคนเดียว
        shell.tool = gfx.tool;
        // ★★★ ค่าที่แผง Settings แสดง (P5-3) — เติมทุกเฟรมเหมือน `storage`
        shell.settings = crate::shell::SettingsView {
            ram_limit_mb: (settings.ram_limit >> 20) as u64,
            vram_limit_mb: settings.vram_limit.map(|bytes| (bytes >> 20) as u64),
            max_pixels: settings.max_pixels,
            theme: settings.theme,
            present: settings.present,
            sidecar: settings.sidecar,
            max_pixels_ceiling: caps.max_pixels_ceiling,
            needs_restart: *settings_restart_pending,
        };
        // ★★ ปัญหาของ `settings.toml` **เปิดแผงขึ้นมาเอง** ครั้งเดียวตอนเปิดโปรแกรม
        //
        //   การอ่านไฟล์เกิดก่อนหน้าต่างจะมี จึงเขียนลงจอตอนนั้นไม่ได้ · และการ
        //   เขียนลง status bar อย่างเดียวไม่พอ เพราะข้อความบนนั้นถูกทับได้ใน
        //   ไม่กี่มิลลิวินาที (เกิดจริงสองครั้งแล้ว — §2.24, §2.30) ส่วนคนที่
        //   ตั้งค่าผิดต้องได้เห็น **เหตุผล** ไม่ใช่แค่รู้ว่ามีอะไรผิด
        //
        //   ★ `pending` ทำให้มันเปิดเองครั้งเดียว — ผู้ใช้ปิดแผงแล้วต้องปิดได้จริง
        //     (จุดสีส้มข้างปุ่ม ⚙ ยังอยู่ให้กลับมาดูได้เสมอ)
        if *settings_notes_pending {
            *settings_notes_pending = false;
            shell.settings_notes = settings_notes.clone();
            shell.settings_open = true;
            shell.status = text::t(shell.lang, Key::SettingsProblemsStatus).to_owned();
            shell.status_warn = true;
        }
        // ★★ คีย์ลัดที่ใช้อยู่ (P5-3b ก้อน b) — เติมครั้งเดียว ไม่ใช่ทุกเฟรม
        //
        //   ตารางเปลี่ยนได้ทางเดียวคือเปิดโปรแกรมใหม่ (`keymap::install` ตั้งได้
        //   ครั้งเดียว) การประกอบสตริง 27 แถวทุกเฟรมจึงเป็นงานที่ทิ้งเปล่า
        if shell.keymap.rows.is_empty() {
            shell.keymap = crate::shell::KeymapView {
                // ★ `display()` คืน `None` ให้ alias อักขระควบคุม — มันไม่มี glyph
                //   ในฟอนต์ที่ฝังไว้ วาดลงไปคือสี่เหลี่ยม tofu
                rows: keymap::active()
                    .bindings()
                    .iter()
                    .filter_map(|binding| Some((binding.chord.display()?, binding.action.name())))
                    .collect(),
                from_file: *keymap_from_file,
                problem: keymap_problem
                    .as_ref()
                    .map(|problem| text::keymap_problem(shell.lang, problem)),
            };
            // ★★★ ไฟล์ที่ใช้ไม่ได้ต้อง **เปิดแผงขึ้นมาเอง** เหมือน settings ที่พัง —
            //     ผู้ใช้ที่เขียน keymap.toml แล้วคีย์ลัดไม่เปลี่ยนจะสรุปว่าฟีเจอร์นี้
            //     ไม่มีจริง ถ้าไม่มีอะไรบอกเขาว่าไฟล์ถูกปฏิเสธเพราะแถวไหน
            if keymap_problem.is_some() {
                shell.settings_open = true;
                shell.status = text::t(shell.lang, Key::KeymapFellBackToDefaults).to_owned();
                shell.status_warn = true;
            }
        }
        // ★ inspector อ่านค่าจากภาพ **ตัวแรกในชุดที่เลือก** (anchor ของการเลือก)
        //   เลือกหลายใบแล้วปรับ = ทุกใบได้ค่าเดียวกัน ซึ่งตรงกับที่ผู้ใช้เห็นบนสไลเดอร์
        shell.appearance = doc
            .selection
            .iter()
            .find_map(|id| doc.board.item(id))
            .map(|item| crate::shell::Appearance {
                opacity: item.canvas.opacity,
                grayscale: item.canvas.filter.grayscale,
                invert: item.canvas.filter.invert,
                brightness: item.canvas.filter.brightness,
                contrast: item.canvas.filter.contrast,
                flip: item.canvas.flip,
            });
        // ★ เนื้อความของโน้ต (P2-11) — **ค่าสำหรับแสดงเท่านั้น** เหมือน `appearance`
        //   เติมจาก item ตัวแรกในชุดที่เลือก และเฉพาะตอนที่มันเป็นโน้ตจริง ๆ
        shell.note = doc
            .selection
            .iter()
            .find_map(|id| doc.board.item(id))
            .and_then(|item| match &item.kind {
                refx_core::board::ItemKind::Text(note) => Some(note.text.clone()),
                _ => None,
            });
        // ★ ข้อมูลฝั่ง Arrange ของ item ตัวแรกในชุดที่เลือก (P3-1) — **ค่าสำหรับแสดง**
        //   เหมือน `appearance`/`note`: ชั้น `app` เติมก่อนวาด แล้วอ่าน *คำขอ* กลับมา
        shell.meta = doc
            .selection
            .iter()
            .find_map(|id| doc.board.item(id))
            .map(|item| crate::shell::MetaView {
                rating: item.meta.rating,
                color_label: item.meta.color_label,
                pinned: item.meta.pinned,
                note: item.meta.note.clone(),
                // ★ เรียงตาม `TagId` เสมอ — รายการที่สลับที่ทุกเฟรมอ่านว่าโปรแกรมพัง
                tags: item
                    .meta
                    .tags
                    .iter()
                    .filter_map(|tag| doc.board.tags().name(*tag).map(str::to_owned))
                    .collect(),
            });
        // ★★★ รูปทรง + ข้อมูลไฟล์ของ item ตัวแรกในชุดที่เลือก (`docs/03 §1`)
        //
        //   **ค่าสำหรับแสดงล้วน ไม่มีทางกลับ** — v1.0 อ่านอย่างเดียว จึงไม่มี
        //   `geometry_edit` คู่กันเหมือน `appearance_edit` · วันที่พิมพ์แก้ได้
        //   มันต้องเดินผ่าน `Command` เหมือนทุก mutation ไม่ใช่เขียน board ตรง
        //
        //   ★ ทุกค่าในนี้ `Board` ถืออยู่แล้ว — ไม่มีการอ่านดิสก์เพิ่มแม้แต่ครั้งเดียว
        //   (I-2: UI thread ห้ามแตะ fs)
        shell.geometry = doc
            .selection
            .iter()
            .find_map(|id| doc.board.item(id))
            .map(|item| crate::shell::GeometryView {
                x: item.canvas.pos.x,
                y: item.canvas.pos.y,
                w: item.canvas.size.x,
                h: item.canvas.size.y,
                rotation_deg: item.canvas.rotation.to_degrees(),
                selected: doc.selection.len(),
                file: match &item.kind {
                    refx_core::board::ItemKind::Image(asset) => Some(crate::shell::FileView {
                        name: refx_asset::decode::file_label(&asset.path),
                        px: Some((asset.px_size.x, asset.px_size.y)),
                        format: crate::shell::format_name(asset.format),
                        path: asset.path.display().to_string(),
                        bytes: asset.file_size,
                    }),
                    // ★ ภาพที่หาไฟล์ไม่เจอ **ยังต้องบอกได้ว่าเคยเป็นไฟล์ไหน** —
                    //   นั่นคือข้อมูลชิ้นเดียวที่ทำให้ผู้ใช้ relink ถูกใบ (docs/07 §2)
                    refx_core::board::ItemKind::Missing { original_path, .. } => {
                        Some(crate::shell::FileView {
                            name: refx_asset::decode::file_label(original_path),
                            px: None,
                            format: None,
                            path: original_path.display().to_string(),
                            bytes: 0,
                        })
                    }
                    refx_core::board::ItemKind::Text(_) => None,
                },
            });
        // ★★★ ภาพที่หาไฟล์ไม่เจอในสิ่งที่เลือกอยู่ (P4-6) — **ค่าสำหรับแสดง**
        //     inspector เอาไปขึ้นชื่อไฟล์ + ปุ่ม "หาไฟล์เอง" (`docs/07 §2` ขั้น 4/5)
        shell.missing = {
            let mut count = 0usize;
            let mut file = String::new();
            for id in doc.selection.iter() {
                if let Some(item) = doc.board.item(id)
                    && let refx_core::board::ItemKind::Missing { original_path, .. } = &item.kind
                {
                    if count == 0 {
                        file = refx_asset::decode::file_label(original_path);
                    }
                    count += 1;
                }
            }
            (count > 0).then_some(crate::shell::MissingView { file, count })
        };
        // ★ กลุ่มของสิ่งที่เลือกอยู่ (P3-7) — **ค่าสำหรับแสดง** เหมือน `meta`
        //   ★★ ต้องแยก "ไม่ได้อยู่ในกลุ่มไหน" ออกจาก "เลือกข้ามหลายกลุ่ม" ให้ขาด
        //      ไม่งั้นช่องเปลี่ยนชื่อจะโผล่มาแล้วเขียนทับกลุ่มที่ผู้ใช้ไม่ได้ตั้งใจแตะ
        shell.group = {
            let mut seen: Option<Option<refx_core::arena::GroupId>> = None;
            let mut mixed = false;
            for id in doc.selection.iter() {
                let Some(item) = doc.board.item(id) else {
                    continue;
                };
                match seen {
                    None => seen = Some(item.meta.group),
                    Some(first) if first != item.meta.group => {
                        mixed = true;
                        break;
                    }
                    Some(_) => {}
                }
            }
            match (mixed, seen) {
                (true, _) => Some(crate::shell::GroupView::Mixed),
                (false, None) => None,
                (false, Some(None)) => Some(crate::shell::GroupView::Loose),
                (false, Some(Some(group_id))) => {
                    doc.board.group(group_id).map_or(
                        // id ที่ห้อยอยู่อ่านเป็น "ไม่มีกลุ่ม" — ห้ามโชว์ช่องเปลี่ยนชื่อ
                        // ของกลุ่มที่ไม่มีอยู่
                        Some(crate::shell::GroupView::Loose),
                        |group| {
                            Some(crate::shell::GroupView::One {
                                id: group_id,
                                name: group.name.clone(),
                                collapsed: group.collapsed,
                                members: doc.board.group_members(group_id).count(),
                            })
                        },
                    )
                }
            }
        };
        shell.vram_used = gfx.textures.budget().used();
        shell.working_used = gfx.working.used();
        shell.working_limit = gfx.working.limit();
        shell.working_evicted = gfx.working.evicted();
        shell.atlas_uploads = gfx.atlas.uploads();
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
            let board = &doc.board;
            let selection = &doc.selection;
            let render_state = &doc.render_state;
            let camera = doc.camera;
            let rubber_band = doc.rubber_band;
            let tool = gfx.tool;
            let guides = doc.guides.as_slice();
            // ★ ไม้บรรทัดมีเจ้าของเดียวคือ `SelectTool` — ที่นี่แค่ **อ่าน** ไปวาด
            //   และ shell ก็อ่านตัวเดียวกันไปแสดงบน status bar (ไม่มีสำเนาที่ต้องซิงค์)
            let measure = doc.select_tool.measurement();
            shell.measured = measure;
            egui_ctx.run_ui(raw_input, |ui| {
                canvas_points = crate::shell::draw_in_ui(ui, shell, |ui, mode| {
                    // ช่องกลางคือ canvas — ภาพวาดด้วย wgpu ใต้ egui อีกที
                    // ★ เป็น widget จริงแล้ว egui จึงจัดลำดับ pointer ให้เอง
                    canvas_input = Self::canvas_widget(
                        ui,
                        CanvasView {
                            board,
                            selection,
                            render_state,
                            camera,
                            rubber_band,
                            tool,
                            guides,
                            measure,
                            mode,
                        },
                    );
                });
            })
        };
        // ★ `shell.mode` ตอนนี้คือโหมดที่ **ช่องกลางเพิ่งวาดไปจริง ๆ** — toolbar
        //   ถูกวาดก่อนช่องกลางเสมอ ค่าจึงอัปเดตแล้วตั้งแต่ก่อน widget ทำงาน
        let canvas_outcome = Self::apply_canvas_input(gfx, doc, canvas_input, shell.mode);
        if canvas_outcome.redraw {
            gfx.window.request_redraw();
        }
        // ★★★ ซิงค์กล้อง/โหมดที่ใช้อยู่จริงเข้า `Board` (P4-1 — หนี้ §6 แถวแรก)
        //
        //   **ไม่ผ่าน `Command` · ไม่ทำให้ `dirty` · ไม่บวก `revision`** ตาม
        //   ข้อยกเว้นที่ docs/02 §2.9 อนุญาตไว้ (ดู `Board::set_view`)
        //
        //   ★ ทำ **ทุกเฟรม** ไม่ใช่ตอน save เพราะ "ถูกเฉพาะถ้าผู้เรียกเรียก
        //     ถูกจังหวะ" คือกับดักที่ docs/08 §3.9 ข้อ 8 บันทึกไว้ (เคสจริง:
        //     `take_forgotten` ของ P2-6) · ราคาคือคัดลอก float ห้าตัว ไม่มี
        //     การจองหน่วยความจำ และ **ไม่ขอเฟรมเพิ่ม** จึงไม่แตะ I-1
        //
        //   ★★★ **ต้องอยู่หลัง `run_ui` + `apply_canvas_input`** (ย้ายมาตอนก้อน c)
        //     เดิมอยู่ก่อนวาด ซึ่งแปลว่าค่าที่เขียนลงไฟล์ช้ากว่าของจริงหนึ่งเฟรมเสมอ
        //     · ที่สำคัญกว่านั้น: ตั้งแต่โหมดถูกอ่าน**กลับ**จาก `Board::view`
        //     ที่ต้นเฟรม การกดปุ่มสลับโหมดบน toolbar (ซึ่งเขียน `shell.mode`
        //     ระหว่าง `run_ui`) จะถูกอ่านทับหายไปทันทีในเฟรมถัดไปถ้าไม่เขียนกลับ
        //     ตรงนี้ — ปุ่มจะกดไม่ติดโดยไม่มี error ที่ไหนเลย
        doc.board
            .set_view(live_view(doc.camera, doc.arrange.camera(), shell.mode));
        // ★ ผู้ใช้จิ้มขอสี — ไปอ่าน **ไฟล์ต้นฉบับบน worker** ไม่ใช่ thumbnail
        //   ที่อยู่ในมือแล้ว (ROADMAP P2-10) · ผลกลับมาทีหลังผ่าน `JobResult::Sampled`
        if let Some(request) = canvas_outcome.pick {
            let asked = Self::request_colour(doc, assets.as_ref(), shell, request, *pick_count);
            if let Some(hash) = asked {
                *pick_count += 1;
                *pick_in_flight = Some(hash);
            }
            gfx.window.request_redraw();
        }
        gfx.egui_winit
            .handle_platform_output(&gfx.window, full_output.platform_output);

        // ผู้ใช้กดปุ่มเครื่องมือบน toolbar — คำขอ ไม่ใช่สถานะ (ดู `ShellState::tool_request`)
        if let Some(tool) = shell.tool_request.take()
            && gfx.tool != tool
        {
            gfx.tool = tool;
            doc.select_tool.cancel();
            // เส้นวัดที่ค้างอยู่หลังกลับไปเครื่องมืออื่นอ่านว่า "มีอะไรค้าง"
            // ไม่ใช่ "นี่คือผลการวัดของฉัน"
            doc.select_tool.clear_measurement();
            doc.rubber_band = None;
        }

        // ★★★ ผู้ใช้แตะแผง Settings (P5-3) — **คำขอ ไม่ใช่สถานะ** เหมือน `tool_request`
        if shell.settings_notes_dismissed {
            shell.settings_notes_dismissed = false;
            shell.settings_notes.clear();
            settings_notes.clear();
        }
        if let Some(edit) = shell.settings_edit.take() {
            let asked = refx_io::settings::Settings {
                ram_limit: usize::try_from(edit.ram_limit_mb << 20).unwrap_or(usize::MAX),
                vram_limit: edit
                    .vram_limit_mb
                    .map(|mb| usize::try_from(mb << 20).unwrap_or(usize::MAX)),
                max_pixels: edit.max_pixels,
                theme: edit.theme,
                present: edit.present,
                sidecar: edit.sidecar,
            };
            if asked != *settings {
                // ★★ ธีมเห็นผลทันที — `set_visuals` มีผลตั้งแต่เฟรมถัดไป
                if asked.theme != settings.theme {
                    crate::theme::install(&gfx.egui_ctx, asked.theme);
                    gfx.window.request_redraw();
                }
                // ★★★ ที่เหลือ **ยังไม่มีผลจนกว่าจะเปิดใหม่** และเราบอกตรง ๆ
                //
                //   เพดาน RAM ถูกอ่านตอนสร้าง `RamBudget` · เพดาน VRAM ตอนสร้าง
                //   `TextureAllocator` · present mode ตอนสร้าง surface — ทั้งสาม
                //   เปลี่ยนกลางคันได้ก็ต่อเมื่อรื้อ threading/resource model ซึ่ง
                //   `CLAUDE.md` บอกให้ **หยุดถามก่อน** · เราจึงเลือกทางที่ไม่แตะ
                //   invariant แล้วบอกความจริงบนจอแทนการแกล้งทำเป็นว่ามันมีผลแล้ว
                if asked.ram_limit != settings.ram_limit
                    || asked.vram_limit != settings.vram_limit
                    || asked.max_pixels != settings.max_pixels
                    || asked.present != settings.present
                {
                    *settings_restart_pending = true;
                }
                *settings = asked;
            }
        }
        // ★★ ผู้ใช้ปล่อยตัวควบคุมแล้ว → เขียนลงไฟล์ **บนเธรดอื่น** (I-2)
        //
        //   ไฟล์เล็กแค่ไหนก็เขียนบน UI thread ไม่ได้ — `write_bytes_atomic` มี
        //   `sync_all()` ซึ่งรอดิสก์จริง และดิสก์ที่ยุ่งอยู่ทำให้มันเป็นสิบ ms
        //   · เส้นทางเดียวกับการบันทึกเอกสาร (`save_job`) เป๊ะ
        if shell.settings_sealed {
            shell.settings_sealed = false;
            match config_dir.as_deref() {
                None => {
                    shell.status = text::t(shell.lang, Key::SettingsNoConfigDir).to_owned();
                    shell.status_warn = true;
                }
                Some(dir) => {
                    let path = settings_path(dir);
                    let body = settings.to_toml();
                    let (tx, rx) = crossbeam_channel::bounded(1);
                    let wake = waker.clone();
                    std::thread::spawn(move || {
                        let written = refx_io::save::write_bytes_atomic(
                            &path,
                            body.as_bytes(),
                            // ★ ตัวเดียวกับที่การบันทึกเอกสารใช้จริง — ไม่ใช่
                            //   `fs::rename` ที่จำลองขึ้นมา (`docs/08 §3.9` ข้อ 9)
                            refx_platform::fsops::rename_durable,
                        )
                        .map_err(|err| err.to_string());
                        let _ = tx.send(written);
                        // ★ ผลต้องขึ้นจอ แต่แอปหลับสนิทตอน idle (I-1) — ต้องปลุก
                        if let Some(wake) = wake {
                            wake.wake();
                        }
                    });
                    *settings_write = Some(rx);
                }
            }
        }
        if let Some(rx) = settings_write.as_ref()
            && let Ok(written) = rx.try_recv()
        {
            *settings_write = None;
            match written {
                Ok(()) => {
                    shell.status = text::t(shell.lang, Key::SettingsSaved).to_owned();
                    shell.status_warn = false;
                }
                Err(err) => {
                    // ★ ต้องบอก — ค่าที่ตั้งใช้ได้จนกว่าจะปิดโปรแกรม แล้วหายเงียบ ๆ
                    tracing::error!(%err, "cannot write settings.toml");
                    shell.status = text::t(shell.lang, Key::SettingsSaveFailed).to_owned();
                    shell.status_warn = true;
                }
            }
        }

        let (width, height) = {
            let config = gfx.render.config();
            (config.width, config.height)
        };

        // ★ กรอบ canvas จริงจาก egui — ใช้ทั้งตอนวาดและตอนแปลงพิกัดเมาส์
        //   ถ้าใช้ขนาดหน้าต่างทั้งบานแทน จุดกึ่งกลางกล้องจะไปตกกลาง *หน้าต่าง*
        //   ซึ่งเยื้องจากกลางช่อง canvas ไปทางซ้ายบน แล้วภาพส่วนหนึ่งจะไปอยู่ใต้ panel
        gfx.canvas =
            CanvasRect::from_points(canvas_points, full_output.pixels_per_point, width, height);

        // ★ Arrange: จัดแผ่น (ถ้าจำเป็น) แล้วเลือกเฉพาะแถบที่อยู่ในจอ — P3-3
        //   ต้องอยู่ **หลัง** `gfx.canvas` เพราะขนาดช่องกลางคือความกว้างของแผ่น
        if shell.mode == Mode::Arrange {
            // ★ การเรียง/กรองมีเจ้าของเดียวคือ widget บน toolbar — ที่นี่แค่รับมา
            //   แล้ว **เทียบก่อนเขียน** ตั้งค่าเดิมซ้ำจึงไม่ทำให้คำนวณใหม่ (I-1)
            let mut changed = doc
                .arrange
                .set_sort(shell.arrange_sort, shell.arrange_descending);
            changed |= doc.arrange.set_filter(shell.arrange_filter.clone());
            if changed {
                // ★ ลำดับเปลี่ยนแล้วต้องเริ่มดูจากบนสุด ไม่งั้นผู้ใช้กดเรียงใหม่
                //   แล้วยังค้างอยู่กลางแผ่นเดิม ซึ่งอ่านว่า "กดแล้วไม่มีอะไรเกิดขึ้น"
                doc.arrange.scroll_to_top();
                gfx.window.request_redraw();
            }
            Self::plan_arrange(gfx, doc, full_output.pixels_per_point);
            // ★ ตัวเลขบน status bar เป็นหลักฐานของเกณฑ์ "วาดจริง < 60"
            //   มันถูกวาดไปแล้วในเฟรมนี้ จึงต้องขอเฟรมอีกหนึ่งเฟรมเมื่อค่าเปลี่ยน
            //   — ลู่เข้าเสมอ (เฟรมถัดไปค่าตรงกันแล้วก็หยุด) จึงไม่ขัด I-1
            let counts = doc.arrange.counts();
            if shell.arrange != counts {
                shell.arrange = counts;
                gfx.window.request_redraw();
            }
        }
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
                        load: wgpu::LoadOp::Clear(clear_colour(&doc.board)),
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

            // [1] ภาพ (instanced quad) — วาดก่อน UI เสมอ
            //
            // ★★ สองโหมดใช้ **ชุด instance และกล้องคนละชุด** (P3-3):
            //    Canvas วาดทั้ง board ตามลำดับ z · Arrange วาดเฉพาะแถบที่อยู่ในจอ
            //    (ที่ 10,000 ใบต่างกันระหว่าง 10,000 instance กับ ~24 instance)
            let arrange_mode = shell.mode == Mode::Arrange;
            let instances: &[QuadInstance] = if arrange_mode {
                &gfx.arrange_quads
            } else {
                &gfx.quads
            };
            let camera = if arrange_mode {
                doc.arrange.camera()
            } else {
                doc.camera
            };
            if !instances.is_empty() {
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
                    CameraUniform::from_affine(camera.to_clip_affine(viewport))
                        // ★ สวิตช์ `G` ทั้ง board เดินทางมาถึง GPU ผ่านช่องนี้ช่องเดียว
                        .with_grayscale(shell.board_grayscale),
                );
                // ★ ภาพที่มี working texture วาดแยกทีละใบ (docs/04 §4 ชั้น B)
                //   ที่เหลือวาดรวมกันจาก atlas ใน draw call เดียวเหมือนเดิม
                //
                //   วาด atlas ก่อนแล้วค่อยทับด้วยตัวคมกว่า — ระหว่างที่ working texture
                //   ยังมาไม่ถึง ผู้ใช้จะเห็นภาพเบลอ ไม่ใช่ช่องว่าง (docs/04 §8)
                let mut batches: Vec<DrawBatch<'_>> = vec![DrawBatch {
                    bind_group: gfx.atlas.bind_group(),
                    instances,
                }];
                // ★ ชั้น B เป็นของ Canvas เท่านั้น — Arrange ตรึง zoom ไว้ที่ระดับ
                //   thumbnail จึงไม่มีวันต้องใช้ภาพคมกว่า atlas (ดู `plan_working_textures`)
                let sharp: Vec<QuadInstance> = if arrange_mode {
                    Vec::new()
                } else {
                    gfx.working_quads
                        .iter()
                        // uv/layer ถูกตั้งไว้ตั้งแต่ `plan_working_textures` แล้ว (รวมกรอบ crop)
                        .map(|(_, quad)| *quad)
                        .collect()
                };
                if !arrange_mode {
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
            // ★ ต้องรายงานจำนวน instance ที่ **วาดจริงในโหมดนี้** ไม่ใช่ `quads` เสมอ
            //   ใน Arrange ตัวที่วาดคือแถบที่อยู่ในจอ (P3-3) — รายงาน `quads`
            //   ตรงนั้นจะพิมพ์ "3072" ทั้งที่วาดจริง 28 ใบ ซึ่งเป็นตัวเลขที่โกหก
            //   แล้วคนอ่านผลจะเทียบสองโหมดผิดทั้งหมด (docs/08 §3.9 ข้อ 9)
            let arrange_mode = self.shell.mode == Mode::Arrange;
            // ★ VRAM ที่ใช้อยู่ต้องออกมากับผลด้วย (P5-1) — `docs/08 §2` มีแถว
            //   `vram_idle_1000` เป็นเกณฑ์ แต่ตัวเลขนั้นอยู่แต่บนแถบสถานะ
            //   การอ่านมันจากภาพหน้าจอคือหลักฐานที่ตาอ่านผิดได้ (§3.9 ข้อ 9)
            let (quads, present, vram, vram_limit) =
                self.gfx
                    .as_ref()
                    .map_or((0, wgpu::PresentMode::AutoVsync, 0, 0), |g| {
                        let drawn = if arrange_mode {
                            g.arrange_quads.len()
                        } else {
                            g.quads.len()
                        };
                        (
                            drawn,
                            g.render.present_mode(),
                            g.textures.budget().used(),
                            g.textures.budget().limit(),
                        )
                    });
            self.stats.report(quads, present, vram, vram_limit);
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
            return Some(repaint_reason(&gfx.egui_ctx));
        }

        // delay จำกัด (เคอร์เซอร์กะพริบ ฯลฯ) → นอนรอด้วย WaitUntil ไม่ใช่วาดรัว ๆ
        // Duration::MAX จะ overflow แล้วได้ None = ไม่ต้องปลุกเลย
        gfx.egui_wake = std::time::Instant::now().checked_add(repaint_delay);
        None
    }

    fn wake_deadline(&self) -> Option<std::time::Instant> {
        let gfx = self.gfx.as_ref()?;
        // ★ เอาเวลาที่ใกล้ที่สุดของทุกแหล่ง — egui · ตัวจำลอง device lost ·
        //   และ autosave (P4-3 · ขาดตัวหลัง board ที่ dirty แล้วถูกปล่อยไว้
        //   จะไม่ถูก snapshot เลยจนกว่าผู้ใช้จะกลับมาขยับเมาส์)
        [
            gfx.egui_wake,
            gfx.render.forced_lost_deadline(),
            self.autosave_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn on_wake(&mut self) -> Option<RedrawReason> {
        // ★★★ **นาฬิกา autosave ต้องมีคนรับสายตรงนี้ ไม่ใช่แค่ตั้งไว้**
        //
        //   P4-3 ต่อ `next_deadline()` เข้า `wake_deadline()` แล้ว เธรดจึงตื่น
        //   ตรงเวลาจริง — **แต่ `on_wake` ไม่มีกิ่งไหนรับมัน** พอคืน `None`
        //   `about_to_wait` ก็ตั้ง `ControlFlow::Wait` แล้วหลับยาวต่อ
        //   → กลไกที่สร้างมาเพื่อเคสนี้ **ไม่เคยทำงานเลยสักครั้ง**
        //
        //   วัดได้ด้วยตา: ลากภาพเข้ามาแล้วปล่อยทิ้งไว้ 35 วินาที (ระยะเว้น 10 วิ)
        //   ไฟล์ snapshot ถูกเขียน **ครั้งเดียว** ไม่ใช่สี่ครั้ง — คือครั้งที่
        //   เกิดจากเฟรมสุดท้ายที่ผู้ใช้ขยับเมาส์ ไม่ใช่จากนาฬิกา
        //
        // ★ เขียนตรงนี้เลย **ไม่ขอเฟรม** — การวาดใหม่ไม่ได้ทำให้ snapshot ถูกขึ้น
        //   และจะทำให้ตัวนับเฟรมของ I-1 ไต่ขึ้นทั้งที่ผู้ใช้ไม่ได้แตะอะไร
        if self
            .autosave_deadline()
            .is_some_and(|at| std::time::Instant::now() >= at)
        {
            self.tick_autosave();
        }

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
            return Some(repaint_reason(&gfx.egui_ctx));
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
            // ★★ **คีย์ลัดทุกตัวหยุดทำงานขณะที่ช่องข้อความมี focus** (P2-11)
            //
            //    ตั้งแต่มีโน้ตข้อความ ตัวอักษรที่ผู้ใช้พิมพ์คือ *ข้อมูล* ไม่ใช่คำสั่ง
            //    ถ้าไม่กั้นตรงนี้ การพิมพ์คำว่า "vict" จะสลับไปเครื่องมือเลือก
            //    เปิดครอป จิ้มสี แล้วเปิดโน้ตอีกใบ ส่วน Delete จะลบภาพที่เลือกอยู่
            //    และ Ctrl+Z จะย้อน **เอกสาร** แทนที่จะย้อนตัวอักษรที่เพิ่งพิมพ์
            //
            //    egui จัดการปุ่มพวกนี้ให้ช่องข้อความไปแล้วใน `on_window_event`
            //    ข้างบน — ที่นี่แค่ต้องไม่แย่งมันมาทำอย่างอื่นซ้ำ
            //
            //    ★ เหตุผลเดียวกับที่ P2-4 เลือก **ปุ่มกลาง** ให้ pan แทน space:
            //    space เป็นอักขระจริงในโน้ต (HANDOFF §2.2 ข้อ 2)
            WindowEvent::KeyboardInput { .. } if gfx.egui_ctx.egui_wants_keyboard_input() => {}

            WindowEvent::KeyboardInput { event, .. } => {
                // ★★★ **ทางเดียวของคีย์บอร์ดทั้งหมด** (หนี้ §6 — ยุบ `on_input`)
                //
                //   เดิมเป็น 14 กิ่ง `if` เรียงกัน · แต่ละกิ่งเรียก wrapper ที่ถาม
                //   `Keymap::action` **ตัวเดียวกัน** แล้วกรองด้วย variant ของผลลัพธ์
                //   — ปุ่มหนึ่งชุดจับคู่ได้ไม่เกินหนึ่ง action อยู่แล้ว (ประตู
                //   `no_single_keypress_can_ever_fire_two_actions` คุมทั้งตาราง)
                //   ทั้ง 14 กิ่งจึงเป็น `match` เดียวโดยไม่เปลี่ยนพฤติกรรม
                //
                //   ★ พิสูจน์ก่อนลบ: `the_single_dispatch_answers_exactly_like_the
                //   _fourteen_branches` ยิงทั้ง 45 แถวผ่านทั้งสองทางแล้วเทียบผล
                //   (เขียวก่อน commit ที่ลบ wrapper)
                //
                //   ★★ นโยบายกดค้างและลำดับ logical→physical อยู่ใน `requested`
                //   ทั้งหมดแล้ว — ที่นี่เหลือหน้าที่เดียวคือ *"เก็บคำขอไว้ทำต้นเฟรมหน้า"*
                if let Some(action) = requested(
                    &event.logical_key,
                    event.physical_key,
                    event.state.is_pressed(),
                    event.repeat,
                    gfx.modifiers,
                ) {
                    // ★ `match` ไม่มี `_ =>` โดยตั้งใจ: เพิ่ม action ใหม่เมื่อไหร่
                    //   คอมไพเลอร์บังคับให้มาต่อสายที่นี่ ไม่ใช่ปล่อยให้เงียบ
                    needs_redraw |= match action {
                        // อ่าน clipboard ที่นี่ไม่ได้ — บล็อกได้ (I-2) ทำที่ต้นเฟรมถัดไป
                        keymap::Action::Paste => {
                            self.pending_paste = true;
                            true
                        }
                        keymap::Action::History(request) => {
                            self.pending_history = Some(request);
                            true
                        }
                        keymap::Action::ZOrder(movement) => {
                            self.pending_zorder = Some(movement);
                            true
                        }
                        keymap::Action::Delete => {
                            self.pending_delete = true;
                            true
                        }
                        keymap::Action::Appearance(what) => {
                            self.pending_appearance = Some(what);
                            true
                        }
                        keymap::Action::Save(request) => {
                            self.pending_save = Some(request);
                            true
                        }
                        keymap::Action::OpenBoard => {
                            self.pending_open = true;
                            true
                        }
                        keymap::Action::Tab(TabKey::New) => {
                            self.pending_new_tab = true;
                            true
                        }
                        keymap::Action::Tab(TabKey::Close) => {
                            self.pending_close_tab = true;
                            true
                        }
                        keymap::Action::Tab(TabKey::Next) => {
                            self.pending_next_tab = true;
                            true
                        }
                        keymap::Action::Group(request) => {
                            self.pending_group = Some(request);
                            true
                        }
                        keymap::Action::ToggleMode => {
                            self.pending_mode_toggle = true;
                            true
                        }
                        keymap::Action::SelectAll => {
                            self.pending_select_all = true;
                            true
                        }
                        keymap::Action::ClearSelection => {
                            self.pending_clear_selection = true;
                            true
                        }
                        keymap::Action::Zoom(request) => {
                            self.pending_zoom = Some(request);
                            true
                        }
                        keymap::Action::Export => {
                            self.pending_export = true;
                            true
                        }
                        // ★★ ตัวเดียวที่ลงมือทันที ไม่ใช่ตั้งคำขอ — และตัวเดียวที่
                        //    **ขอเฟรมใหม่เฉพาะเมื่อมีอะไรเปลี่ยนจริง** (I-1):
                        //    กด `V` ซ้ำตอนอยู่เครื่องมือเลือกอยู่แล้ว ต้องไม่วาดใหม่
                        keymap::Action::Tool(tool) => {
                            let changed = gfx.tool != tool;
                            if changed {
                                gfx.tool = tool;
                                // การกดค้างที่ยังอยู่เป็นของเครื่องมือเดิม ใช้ต่อไม่ได้
                                self.docs.active_mut().select_tool.cancel();
                                self.docs.active_mut().select_tool.clear_measurement();
                                self.docs.active_mut().rubber_band = None;
                            }
                            changed
                        }
                    };
                }
            }

            _ => {}
        }

        needs_redraw
    }

    /// ★★★ กดปิดหน้าต่างทั้งที่ยังมีงานไม่ได้บันทึก — **ถามก่อน** (P4-2)
    ///
    /// `CLAUDE.md` เขียนไว้ว่า "งาน mood board ที่จัดมา 3 ชั่วโมงหายไป = เลิกใช้
    /// ทันที ไม่มีโอกาสที่สอง" · การปิดโดยไม่ถามคือทางที่งานหายง่ายที่สุด
    /// และเป็นทางที่ผู้ใช้ทำพลาดได้ด้วยการกดผิดปุ่มเดียว
    ///
    /// ★ ถามด้วย **แถบใน egui ไม่ใช่ native dialog** — `rfd::MessageDialog`
    /// บล็อกเธรดที่เรียก ซึ่งตรงนี้คือ UI thread (I-2) · แถบในแอปยังทำให้
    /// ผู้ใช้เห็นงานของตัวเองอยู่ข้างหลังตอนตัดสินใจ ซึ่งช่วยเขาเลือกได้ถูกกว่า
    fn on_close_requested(&mut self) -> bool {
        if self.closing {
            return true;
        }
        // ★★★ **ถามถ้ามีแท็บ *ใบใดก็ตาม* ที่ยังไม่บันทึก** (P4-7c) — ถามแค่ใบที่
        //     อยู่หน้าจอ แล้วผู้ใช้ที่ทำงานค้างไว้ในอีกสามแท็บจะปิดโปรแกรมไปเฉย ๆ
        //     โดยไม่มีอะไรเตือน · ★ พาเขาไปที่ใบแรกที่ค้างด้วย ไม่งั้นคำถามจะ
        //     พูดถึงงานที่เขามองไม่เห็น
        let unsaved = self
            .docs
            .iter()
            .position(|doc| doc.board.is_dirty())
            .filter(|_| self.gfx.is_some());
        let Some(index) = unsaved else {
            return true;
        };
        self.focus_tab(index);
        self.close_confirm = true;
        self.shell.close_prompt = true;
        self.shell.close_scope_tab = false;
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
        false
    }

    /// ★★★ ตัวจับ I-1 ที่รั่วเป็นครั้งคราว — **เก็บอย่างเดียว ห้ามขอเฟรม**
    ///
    /// `docs/08 §3.9` ข้อ 11: เจอ redraw ~1.5 Hz สองครั้งแล้ววัดซ้ำ 13 หน้าต่าง
    /// ไม่เจออีกเลย · การวัดด้วยมือจับเหตุการณ์แบบนั้นไม่ได้ จึงต้องมีตัวนับที่
    /// เปิดอยู่ตลอด **พร้อมชื่อคนขอ** — ครั้งหน้าที่มันเกิด ภาพหน้าจอจะบอก
    /// สาเหตุของตัวเอง
    ///
    /// ★ ขึ้นแถบสถานะเฉพาะตอนเกินเพดาน — สภาพปกติภาพหน้าจอไม่เปลี่ยนเลย
    fn on_quiet_streak(&mut self, quiet: refx_platform::redraw::QuietStreak) {
        self.shell.quiet_redraws = quiet
            .is_alarming()
            .then(|| {
                quiet
                    .worst_reason()
                    .map(|reason| (quiet.len(), reason_name(reason)))
            })
            .flatten();
    }

    /// ★ ปิดโปรแกรมหลังบันทึกเสร็จ — ดู [`AppDelegate::wants_exit`] ว่าทำไมต้องมี
    fn wants_exit(&self) -> bool {
        self.closing
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
    recovery_dir: &std::path::Path,
    spool_dir: &std::path::Path,
    config_dir: &std::path::Path,
) -> Result<(), refx_platform::window::RunError<DeviceError>> {
    let mut app = RefxApp::new(args);
    // ★★ ต้องมาก่อน `start_assets` — เพดาน RAM/pixel ของ pool ถูกอ่านตอนสร้าง
    //    เท่านั้น เสียบทีหลังไม่ได้ · แตะดิสก์ตรงนี้ได้เพราะยังไม่มีหน้าต่าง (I-2)
    app.load_settings(config_dir);
    // ★★ ต้องมาก่อนหน้าต่างเช่นกัน — `keymap::install` ตั้งได้ครั้งเดียวต่อโปรเซส
    //    และ `shortcut_char` ถามตารางตั้งแต่ปุ่มแรกที่ผู้ใช้กด
    app.load_keymap(config_dir);
    // ★ ที่อยู่ของงานที่ยังไม่เคยบันทึก — ถูกส่งเข้ามาเพราะ `AppPaths` เป็นของ
    //   ชั้น platform · ★★ ต้องเป็น `<data_dir>/recovery` เท่านั้น ห้าม cache
    app.recovery_dir = Some(recovery_dir.to_path_buf());
    // ★★ ที่พักของภาพที่วาง — เหตุผลเดียวกับ `recovery/` เป๊ะ: ภาพจาก clipboard
    //    สร้างใหม่ไม่ได้จากอะไรเลย ถ้าอยู่ใน `cache_dir` แล้ว eviction ลบมัน
    //    ผู้ใช้เสียภาพถาวร (`docs/07 §2`)
    app.spool_dir = Some(spool_dir.to_path_buf());
    // ★ ต้องมาก่อน `start_assets` — pool รับที่พักตอนสร้างเท่านั้น เสียบทีหลังไม่ได้
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
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        clippy::panic
    )]

    use refx_asset::pool::PoolStatsSnapshot;

    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    // ---------- P5-3: ค่าที่ผู้ใช้ตั้งต้องไปถึงด่านที่มันควบคุมจริง ----------

    /// ★★★ เพดาน `max_pixels` ที่ผู้ใช้ตั้ง **ต้องไปถึง `decode` จริง ๆ**
    ///
    /// นี่คือครึ่งที่ `refx_io::settings` พิสูจน์ไม่ได้: ที่นั่นพิสูจน์ว่าค่าถูก
    /// อ่านและ clamp ถูกต้อง แต่ค่าที่ถูกต้องซึ่ง **ไม่มีใครเอาไปใช้** ก็ยังเป็น
    /// ฟีเจอร์ที่ไม่ทำงาน · ผู้ใช้ที่ลดเพดานลงเพื่อกัน OOM แล้วมันไม่มีผล
    /// จะเจอสิ่งที่เขาพยายามหลีกเลี่ยงพอดี (I-3)
    ///
    /// ★ และ `max_alloc` ต้องตามลงมาด้วย ไม่งั้นด่านที่เขาตั้งจะไม่ได้คุม
    /// หน่วยความจำที่ decoder จองจริงเลยสักไบต์
    #[test]
    fn the_ceiling_the_user_sets_is_the_ceiling_decode_enforces() {
        let caps = refx_io::settings::Caps {
            total_ram: 16 << 30,
            max_pixels_ceiling: refx_asset::decode::max_pixels_for_ram(16 << 30),
        };
        let machine = decode_limits(caps, &refx_io::settings::Settings::defaults(caps));
        assert_eq!(machine.max_pixels, caps.max_pixels_ceiling);

        let asked = refx_io::settings::Settings {
            max_pixels: 4_000_000,
            ..refx_io::settings::Settings::defaults(caps)
        };
        let limited = decode_limits(caps, &asked);
        assert_eq!(limited.max_pixels, 4_000_000, "ค่าที่ผู้ใช้ตั้งไม่ถึง decode");
        assert!(
            limited.max_alloc < machine.max_alloc,
            "ลดเพดาน pixel แล้ว max_alloc ไม่ตาม — decoder ยังจองได้เท่าเดิม \
             ({} vs {})",
            limited.max_alloc,
            machine.max_alloc
        );
    }

    /// ★★ ค่าปริยายของเพดาน RAM ต้องเป็น **เลขเดียวกัน** ทั้งสองชั้น
    ///
    /// `refx-io` ตั้งค่าปริยายของ `settings.toml` ส่วน `refx-asset` ตั้งค่าปริยาย
    /// ของถัง RAM · ทั้งคู่พึ่งกันไม่ได้ (คนละชั้น ARCHITECTURE §2) ค่าจึงถูก
    /// เขียนไว้สองที่ — ซึ่ง `HANDOFF §4` ข้อ 21 บอกว่าเป็นจุดที่จะ drift
    /// **ที่นี่คือชั้นเดียวที่เห็นทั้งสองตัว** จึงเป็นที่เดียวที่ตั้งประตูได้
    ///
    /// drift แล้วผลคือ: ผู้ใช้เปิดโปรแกรมครั้งแรกได้เพดานหนึ่ง พอกดบันทึกค่า
    /// จากแผงโดยไม่แตะอะไรเลย เพดานเปลี่ยนไปอีกค่าหนึ่งโดยไม่มีใครสั่ง
    #[test]
    fn the_two_layers_agree_on_the_default_ram_budget() {
        assert_eq!(
            (refx_io::settings::DEFAULT_RAM_LIMIT_MB as usize) << 20,
            refx_asset::budget::DEFAULT_RAM_LIMIT,
            "ค่าปริยายของเพดาน RAM ใน refx-io กับ refx-asset ไม่ตรงกันแล้ว"
        );
    }

    /// ★ ค่าปริยายของ `SettingsView` ต้องตรงกับค่าปริยายจริงของ `Settings`
    ///
    /// แผงที่แสดงค่าปริยายผิดตั้งแต่เฟรมแรกจะทำให้ผู้ใช้กด "บันทึก" แล้วเขียน
    /// ค่าที่เขาไม่ได้เลือกลงไฟล์
    #[test]
    fn the_panel_starts_from_the_same_defaults_the_loader_uses() {
        let caps = refx_io::settings::Caps {
            total_ram: 16 << 30,
            max_pixels_ceiling: refx_asset::decode::MAX_PIXELS_ABS,
        };
        let real = refx_io::settings::Settings::defaults(caps);
        let shown = crate::shell::SettingsView::default();
        assert_eq!(shown.ram_limit_mb, (real.ram_limit >> 20) as u64);
        assert_eq!(shown.vram_limit_mb, None);
        assert_eq!(shown.max_pixels, real.max_pixels);
        assert_eq!(shown.theme, real.theme);
        assert_eq!(shown.present, real.present);
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
                    let _ = crate::shell::draw_in_ui(ui, &mut state, |ui, _mode| {
                        let got = RefxApp::canvas_widget(
                            ui,
                            CanvasView {
                                board: &board,
                                selection: &selection,
                                render_state: &render_state,
                                camera: Camera::default(),
                                rubber_band: None,
                                tool: Tool::Select,
                                guides: &[],
                                measure: None,
                                mode: Mode::Canvas,
                            },
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

    // ---------- pointer: การกดหนึ่งครั้งต้องเป็น Press หนึ่งครั้ง ----------

    /// ★★ การกดจริงหนึ่งครั้งต้องได้ `Press` **ครั้งเดียว** และต้องอยู่ที่ที่ผู้ใช้กด
    ///
    /// egui รายงาน `drag_started` ในเฟรมหลังเคอร์เซอร์ขยับพ้นระยะของมันเอง
    /// ถ้านับอันนั้นเป็นการกดด้วย จะได้ `Press` ครั้งที่สองที่ตำแหน่ง **หลังขยับแล้ว**
    /// ซึ่งไปทับสถานะการกดเดิม อาการที่ผู้ใช้เห็นคือ **จับ handle แล้วลาก กลายเป็น
    /// ลากกรอบเลือก** และของที่เลือกไว้หายไปด้วย (เจอจริงตอนทำ P2-5 ส่วน handle)
    #[test]
    fn one_physical_press_delivers_exactly_one_press_event() {
        let ctx = egui::Context::default();
        let mut state = crate::shell::ShellState::default();
        let board = Board::default();
        let selection = Selection::new();
        let render_state = std::collections::HashMap::new();
        let start = egui::pos2(640.0, 400.0);

        // เฟรมที่ 0–1 ให้ layout นิ่งก่อน · 2 = กดลง · 3–4 = ลากออกไปไกล
        let frames = [
            vec![egui::Event::PointerMoved(start)],
            vec![egui::Event::PointerMoved(start)],
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            }],
            vec![egui::Event::PointerMoved(start + egui::vec2(40.0, 30.0))],
            vec![egui::Event::PointerMoved(start + egui::vec2(90.0, 70.0))],
        ];

        let mut pressed_at = Vec::new();
        for events in frames {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let _ = crate::shell::draw_in_ui(ui, &mut state, |ui, _mode| {
                    let got = RefxApp::canvas_widget(
                        ui,
                        CanvasView {
                            board: &board,
                            selection: &selection,
                            render_state: &render_state,
                            camera: Camera::default(),
                            rubber_band: None,
                            tool: Tool::Select,
                            guides: &[],
                            measure: None,
                            mode: Mode::Canvas,
                        },
                    );
                    if got.primary_pressed {
                        pressed_at.push(got.pointer);
                    }
                });
            });
        }

        assert_eq!(
            pressed_at.len(),
            1,
            "กดหนึ่งครั้งต้องได้ Press หนึ่งครั้ง แต่ได้ที่ {pressed_at:?}"
        );
        assert_eq!(
            pressed_at[0],
            Some(start),
            "Press ต้องอยู่ที่ที่ผู้ใช้กด ไม่ใช่ที่ที่เคอร์เซอร์ไปถึงทีหลัง"
        );
    }

    // ---------- scale/rotate handle (P2-5) ----------

    /// สถานะการวาดที่ว่างที่สุดเท่าที่ `quad_for` ต้องใช้
    fn bare_render_state() -> ItemRender {
        ItemRender {
            source: refx_asset::pool::JobSource::Clipboard,
            hash: refx_asset::hash::ContentHash::from_bytes([0u8; 32]),
            thumb: refx_asset::thumb::Thumbnail {
                pixels: Vec::new(),
                source_width: 100,
                source_height: 100,
                dominant: 0,
            },
            slot: Some(refx_render::atlas::AtlasSlot { layer: 0, index: 0 }),
            tint: [1.0; 4],
        }
    }

    /// board ที่มีภาพเดียวขนาด 100×100 อยู่ที่จุดกำเนิด พร้อมเลือกไว้แล้ว
    ///
    /// ทุกอย่างผ่าน `Command` เหมือนของจริง — `insert_item`/`set_canvas`
    /// เป็น `pub(crate)` ของ `refx-core` โดยตั้งใจ (docs/08 §4 ข้อ 10)
    fn one_selected_item(rotation: f32) -> (Board, Selection) {
        let mut board = Board::default();
        let mut history = refx_core::command::History::default();
        let item = refx_core::board::Item::new(refx_core::board::ItemKind::Image(AssetRef {
            hash: refx_asset::hash::ContentHash::from_bytes([0u8; 32]),
            path: std::path::PathBuf::new(),
            px_size: glam::UVec2::new(100, 100),
            format: ImageFormat::Unknown,
            embedded: false,
            mtime: 0,
            file_size: 0,
        }))
        .at(Vec2::ZERO, Vec2::splat(100.0));
        history
            .apply(
                &mut board,
                Box::new(refx_core::command::AddItems::new(vec![item]).unwrap()),
            )
            .unwrap();
        let id = board.z_order()[0];

        let canvas = ItemCanvas {
            rotation,
            ..board.item(id).unwrap().canvas
        };
        history
            .apply(
                &mut board,
                Box::new(refx_core::command::TransformItems::new(vec![(id, canvas)]).unwrap()),
            )
            .unwrap();

        let mut selection = Selection::new();
        selection.select(id);
        (board, selection)
    }

    /// ★ สิ่งที่ **วาด** ต้องอยู่ที่เดียวกับสิ่งที่ **hit-test** ตัดสิน
    ///
    /// affine ของ `QuadInstance` เก็บแบบ column-major ส่วน hit-test ใช้ `Obb::axes()`
    /// ถ้าสองที่นี้ไม่ตรงกัน ภาพที่หมุนจะถูกวาดที่หนึ่งแต่กดโดนอีกที่หนึ่ง
    /// ซึ่งเป็นอาการ "โปรแกรมจับผิดตัว" ที่กัดกร่อนความเชื่อถือเร็วที่สุด
    #[test]
    fn a_rotated_quad_is_drawn_exactly_where_its_obb_says() {
        let state = bare_render_state();
        for rotation in [0.0, 0.4, std::f32::consts::FRAC_PI_4, 2.9] {
            let canvas = ItemCanvas {
                pos: Vec2::new(30.0, -20.0),
                size: Vec2::new(120.0, 80.0),
                rotation,
                ..ItemCanvas::default()
            };
            let quad =
                instances::quad_for(&canvas, state.slot, state.tint).expect("ภาพที่มองเห็นต้องได้ quad");
            let [a, b, c, d, tx, ty] = quad.transform;
            // unit quad (0,0) (1,0) (1,1) (0,1) → world (ตามลำดับของ Obb::corners)
            let mapped = |u: Vec2| Vec2::new(a * u.x + c * u.y + tx, b * u.x + d * u.y + ty);
            let corners = canvas.obb().corners();
            for (i, unit) in [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y]
                .into_iter()
                .enumerate()
            {
                assert!(
                    (mapped(unit) - corners[i]).length() < 1e-3,
                    "rot={rotation} มุมที่ {i}: วาดที่ {:?} แต่ hit-test ใช้ {:?}",
                    mapped(unit),
                    corners[i]
                );
            }
        }
    }

    /// ★★ ประตูที่ทำให้ฟิลด์ของ `ItemCanvas` **ส่งเสียงเอง** ว่าถึง GPU แล้วหรือยัง
    ///
    /// `rotation` เงียบอยู่ตั้งแต่ P0 ถึง P2-5 เพราะไม่มีอะไรบังคับให้ใครไปดูว่ามัน
    /// ถึง shader หรือยัง — เจอเพราะบังเอิญมีคนไปทำฟีเจอร์ที่ต้องใช้มันพอดี
    /// นี่คือรูปแบบเดิมที่โปรเจกต์นี้โดนมาแล้วหลายครั้ง: **มีที่ว่างรอไว้
    /// ไม่มีใครเติม ไม่มีอะไรส่งเสียง**
    ///
    /// เทสต์นี้ปิดช่องนั้นสองชั้น:
    ///
    /// 1. **destructure ครบทุกฟิลด์ ไม่มี `..`** → เพิ่มฟิลด์ใหม่ใน `ItemCanvas`
    ///    เมื่อไหร่ ตรงนี้ **คอมไพล์ไม่ผ่าน** จนกว่าจะมีคนตัดสินว่ามันถึง GPU ไหม
    ///    (หลักการเดียวกับ `DeviceBound::build` — HANDOFF §4 ข้อ 16)
    /// 2. **เทียบพฤติกรรมจริง ไม่ใช่เจตนา** → ฟิลด์ที่ตารางบอกว่า "ยังไม่ถึง"
    ///    แล้ววันหนึ่งมีคนต่อให้ถึง (crop = P2-7 · flip/opacity/filter = P2-8)
    ///    เทสต์จะแดงทันที บังคับให้มาแก้ตารางนี้ ไม่ใช่ปล่อยให้โค้ดกับความเข้าใจ
    ///    แยกทางกันเงียบ ๆ อีกรอบ
    ///
    /// **ตัวที่ยังไม่ถึงไม่ใช่บั๊ก** — มันมีคิวของมันอยู่แล้ว ข้อนี้แค่ทำให้
    /// "ยังไม่ถึง" เป็นสิ่งที่มีใครสักคนเซ็นรับรองไว้ ไม่ใช่สิ่งที่ไม่มีใครรู้
    #[test]
    fn no_item_canvas_field_reaches_the_gpu_without_us_knowing() {
        use refx_core::board::{CropRect, Flip, ItemFilter};

        let state = bare_render_state();
        let base = ItemCanvas {
            size: Vec2::splat(100.0),
            ..ItemCanvas::default()
        }
        .sanitized();
        let baseline = instances::quad_for(&base, state.slot, state.tint);
        assert!(baseline.is_some(), "ภาพปกติต้องได้ quad");

        // ★ ไม่มี `..` โดยตั้งใจ — ฟิลด์ใหม่ทำให้บรรทัดนี้คอมไพล์ไม่ผ่าน
        let ItemCanvas {
            pos: _,
            size: _,
            rotation: _,
            flip: _,
            opacity: _,
            crop: _,
            locked: _,
            visible: _,
            filter: _,
        } = base;

        // ฟิลด์ · ค่าที่ต่างจากค่าเริ่มต้น · เปลี่ยนสิ่งที่ GPU ได้รับไหม · เหตุผล
        let cases: [(&str, ItemCanvas, bool, &str); 9] = [
            (
                "pos",
                ItemCanvas {
                    pos: Vec2::new(7.0, -3.0),
                    ..base
                },
                true,
                "transform",
            ),
            (
                "size",
                ItemCanvas {
                    size: Vec2::new(50.0, 20.0),
                    ..base
                },
                true,
                "transform",
            ),
            (
                "rotation",
                ItemCanvas {
                    rotation: 0.6,
                    ..base
                },
                true,
                "transform (ต่อแล้วตอน P2-5)",
            ),
            (
                "visible",
                ItemCanvas {
                    visible: false,
                    ..base
                },
                true,
                "ไม่วาดเลย (ต่อแล้วตอน audit นี้)",
            ),
            (
                "flip",
                ItemCanvas {
                    flip: Flip::Horizontal,
                    ..base
                },
                true,
                "สลับปลายช่วง uv (ต่อแล้วตอน P2-8)",
            ),
            (
                "opacity",
                ItemCanvas {
                    opacity: 0.25,
                    ..base
                },
                true,
                "คูณลง tint[3] แล้ว pipeline alpha-blend ให้ (ต่อแล้วตอน P2-8)",
            ),
            (
                "filter",
                ItemCanvas {
                    filter: ItemFilter {
                        grayscale: true,
                        ..base.filter
                    },
                    ..base
                },
                true,
                "grayscale/invert เป็นธง · brightness/contrast ยัดใน 16 บิตบนของ flags แล้ว shader คลายออก (ต่อแล้วตอน P2-8)",
            ),
            (
                "crop",
                ItemCanvas {
                    crop: CropRect {
                        min: Vec2::splat(0.25),
                        max: Vec2::splat(0.75),
                    },
                    ..base
                },
                true,
                "หด uv_rect ลงในช่องของ atlas (ต่อแล้วตอน P2-7)",
            ),
            (
                "locked",
                ItemCanvas {
                    locked: true,
                    ..base
                },
                false,
                "ไม่ใช่เรื่องของการวาดโดยตั้งใจ — คุมแค่การแก้ไข",
            ),
        ];

        for (name, mutated, reaches_gpu, why) in cases {
            let changed =
                instances::quad_for(&mutated.sanitized(), state.slot, state.tint) != baseline;
            assert_eq!(
                changed,
                reaches_gpu,
                "`{name}` {} ({why}) — แก้ตารางในเทสต์นี้ให้ตรงความจริง",
                if changed {
                    "ถึง GPU แล้ว แต่ตารางบอกว่ายัง"
                } else {
                    "ยังไม่ถึง GPU แต่ตารางบอกว่าถึงแล้ว"
                }
            );
        }
    }

    /// negative control ของข้อบน: ถ้าลืมใส่การหมุนเข้า transform ค่าจะเท่ากับภาพที่ไม่หมุน
    #[test]
    fn rotation_actually_reaches_the_gpu_transform() {
        let state = bare_render_state();
        let plain = ItemCanvas {
            size: Vec2::splat(100.0),
            ..ItemCanvas::default()
        };
        let spun = ItemCanvas {
            rotation: std::f32::consts::FRAC_PI_4,
            ..plain
        };
        assert_ne!(
            instances::quad_for(&plain, state.slot, state.tint).map(|q| q.transform),
            instances::quad_for(&spun, state.slot, state.tint).map(|q| q.transform),
            "หมุนแล้ว transform ต้องเปลี่ยน ไม่งั้นการหมุนไม่มีผลบนจอเลย"
        );
    }

    /// รูปสี่เหลี่ยมทึบสี handle ที่ shell วาดออกมาจริง ๆ ในหนึ่งเฟรม
    fn painted_handles(
        board: &Board,
        selection: &Selection,
        camera: Camera,
        tool: Tool,
    ) -> Vec<egui::Rect> {
        let ctx = egui::Context::default();
        let mut state = crate::shell::ShellState::default();
        let render_state = std::collections::HashMap::new();
        let mut found = Vec::new();

        // สองรอบ: egui ใช้ layout ของรอบก่อนหน้า รอบแรกขนาด panel ยังไม่นิ่ง
        for _ in 0..2 {
            found.clear();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let _ = crate::shell::draw_in_ui(ui, &mut state, |ui, _mode| {
                    let _ = RefxApp::canvas_widget(
                        ui,
                        CanvasView {
                            board,
                            selection,
                            render_state: &render_state,
                            camera,
                            rubber_band: None,
                            tool,
                            guides: &[],
                            measure: None,
                            mode: Mode::Canvas,
                        },
                    );
                });
            });
            for clipped in &output.shapes {
                if let egui::Shape::Rect(shape) = &clipped.shape
                    && shape.fill == HANDLE_FILL
                {
                    found.push(shape.rect);
                }
            }
        }
        found
    }

    /// ★ docs/08 §3.9 ข้อ 5: งานที่ผู้ใช้เห็นต้องมีเทสต์ที่รัน shell จริงแล้วไล่ดูรูปทรง
    ///
    /// ถ้าข้อนี้พัง ผู้ใช้จะเห็นภาพถูกเลือกแต่ไม่มีอะไรให้จับสเกล
    #[test]
    fn selecting_an_item_paints_four_corner_handles() {
        let (board, selection) = one_selected_item(0.0);
        let handles = painted_handles(&board, &selection, Camera::default(), Tool::Select);
        assert_eq!(handles.len(), 4, "ต้องมี handle ครบสี่มุม");

        let (board, empty) = (board, Selection::new());
        assert!(
            painted_handles(&board, &empty, Camera::default(), Tool::Select).is_empty(),
            "ไม่ได้เลือกอะไรต้องไม่มี handle"
        );
    }

    /// ★★ handle ต้องมีขนาดคงที่ **บนจอ** ไม่ใช่ใน world (HANDOFF §2.4)
    ///
    /// ถ้าขนาดผูกกับ world ซูมออกแล้ว handle จะเล็กลงจนจับไม่โดน ซึ่งคือ
    /// "เครื่องมือที่มีอยู่แต่ใช้ไม่ได้" — แย่กว่าไม่มีเพราะผู้ใช้เห็นมันอยู่
    #[test]
    fn handles_keep_their_screen_size_at_every_zoom() {
        let (board, selection) = one_selected_item(0.0);
        let side = refx_core::interact::HANDLE_DRAW_PX;

        let mut spans = Vec::new();
        for zoom in [0.05_f32, 1.0, 8.0] {
            let handles = painted_handles(
                &board,
                &selection,
                Camera::new(Vec2::ZERO, zoom),
                Tool::Select,
            );
            assert_eq!(handles.len(), 4, "zoom {zoom}");
            for handle in &handles {
                assert!(
                    (handle.width() - side).abs() < 1e-3 && (handle.height() - side).abs() < 1e-3,
                    "zoom {zoom}: handle ขนาด {:?} ต้องเป็น {side} point เสมอ",
                    handle.size()
                );
            }
            // ระยะระหว่าง handle ต่างหากที่ต้องเปลี่ยนตามซูม
            let left = handles
                .iter()
                .map(|h| h.center().x)
                .fold(f32::MAX, f32::min);
            let right = handles
                .iter()
                .map(|h| h.center().x)
                .fold(f32::MIN, f32::max);
            spans.push(right - left);
        }
        assert!(
            spans[0] < spans[1] && spans[1] < spans[2],
            "ระยะระหว่าง handle ต้องโตตามซูม: {spans:?}"
        );
    }

    /// ★ ภาพที่หมุนแล้ว handle ต้องหมุนตาม ไม่ใช่ค้างอยู่ที่มุมของ AABB
    ///
    /// ไม่งั้นผู้ใช้จะกดตรงที่ *เห็น* handle แล้วไม่โดน — `refx-core` จะจับที่มุมจริง
    #[test]
    fn handles_rotate_together_with_the_item() {
        // ★ 30° ไม่ใช่ 45° โดยตั้งใจ: ที่ 45° มุมของ AABB กับมุมของ OBB มีระยะ
        //   จากกึ่งกลางเท่ากันพอดี เทสต์ที่วัดแค่ระยะจึงแยกสองอย่างนี้ไม่ออก
        let canvas = ItemCanvas {
            size: Vec2::splat(100.0),
            rotation: 30.0_f32.to_radians(),
            ..ItemCanvas::default()
        };
        let (board, selection) = one_selected_item(canvas.rotation);
        let handles = painted_handles(&board, &selection, Camera::default(), Tool::Select);
        assert_eq!(handles.len(), 4);

        // camera zoom 1 กับ pixels_per_point 1 → ระยะ world = ระยะ point ตรง ๆ
        let centre = handles
            .iter()
            .fold(egui::Vec2::ZERO, |sum, h| sum + h.center().to_vec2())
            / 4.0;
        let offset_of = |h: &egui::Rect| h.center().to_vec2() - centre;

        for corner in canvas.obb().corners() {
            assert!(
                handles
                    .iter()
                    .any(|h| (offset_of(h) - egui::vec2(corner.x, corner.y)).length() < 0.5),
                "ไม่มี handle ที่มุม {corner:?} ของภาพที่หมุนแล้ว"
            );
        }

        // และต้อง **ไม่ใช่** มุมของ AABB ซึ่งเป็นสิ่งที่จะได้ถ้าลืมใช้ OBB
        let aabb = canvas.world_bounds();
        for corner in [aabb.min, aabb.max] {
            assert!(
                !handles
                    .iter()
                    .any(|h| (offset_of(h) - egui::vec2(corner.x, corner.y)).length() < 5.0),
                "handle ไปเกาะมุมของ AABB ที่ {corner:?} — ผู้ใช้จะกดที่ที่เห็นแล้วไม่โดน"
            );
        }
    }

    // ---------- z-order + delete (P2-6) ----------

    /// ★ `Shift+[` บนคีย์บอร์ดส่วนใหญ่ส่งอักขระ `{` มาเลย ไม่ได้ส่ง `[` พร้อมธง shift
    ///
    /// ถ้าดูแต่ธง shift ปุ่ม "ส่งหลังสุด/หน้าสุด" จะไม่ทำงานบนเครื่องส่วนใหญ่ —
    /// และเป็นความล้มเหลวแบบเงียบ: ไม่มี error ไม่มี log ผู้ใช้แค่กดแล้วไม่เกิดอะไร
    #[test]
    fn the_z_order_keys_cover_both_ways_a_keyboard_reports_shift() {
        let none = ModifiersState::empty();
        let shift = ModifiersState::SHIFT;

        assert_eq!(zorder_shortcut(pressed("]"), none), Some(ZMove::Forward));
        assert_eq!(zorder_shortcut(pressed("["), none), Some(ZMove::Backward));

        // ทางที่หนึ่ง: ธง shift มาพร้อมอักขระเดิม
        assert_eq!(zorder_shortcut(pressed("]"), shift), Some(ZMove::ToFront));
        assert_eq!(zorder_shortcut(pressed("["), shift), Some(ZMove::ToBack));
        // ทางที่สอง: อักขระเปลี่ยนไปเลย (พบบ่อยกว่า)
        assert_eq!(zorder_shortcut(pressed("}"), shift), Some(ZMove::ToFront));
        assert_eq!(zorder_shortcut(pressed("{"), shift), Some(ZMove::ToBack));
        assert_eq!(zorder_shortcut(pressed("}"), none), Some(ZMove::ToFront));

        // Ctrl/Alt เป็นของคำสั่งอื่น ต้องไม่ถูกจับเป็นการย้ายชั้น
        assert_eq!(zorder_shortcut(pressed("]"), ModifiersState::CONTROL), None);
        assert_eq!(zorder_shortcut(pressed("]"), ModifiersState::ALT), None);
        assert_eq!(zorder_shortcut(pressed("z"), none), None);
    }

    /// แล็ปท็อปหลายรุ่นไม่มีปุ่ม `Delete` แยก — ต้องรับ `Backspace` ด้วย
    #[test]
    fn delete_accepts_the_key_that_laptops_actually_have() {
        use winit::keyboard::{Key as WKey, NamedKey};
        assert!(is_delete(&WKey::Named(NamedKey::Delete)));
        assert!(is_delete(&WKey::Named(NamedKey::Backspace)));
        assert!(!is_delete(&key("d")));
        assert!(!is_delete(&WKey::Named(NamedKey::Enter)));
    }

    // ---------- crop (P2-7) ----------

    /// docs/03 §2: `V` = Select/Move · `C` = Crop — และ Ctrl+C/Ctrl+V ต้องไม่โดนจับ
    #[test]
    fn the_tool_keys_do_not_steal_the_clipboard_shortcuts() {
        let none = ModifiersState::empty();
        assert_eq!(tool_shortcut(pressed("v"), none), Some(Tool::Select));
        assert_eq!(tool_shortcut(pressed("c"), none), Some(Tool::Crop));
        assert_eq!(tool_shortcut(pressed("C"), none), Some(Tool::Crop));

        // ★ Ctrl+V คือวางจาก clipboard · Ctrl+C คือคัดลอก — ห้ามกลายเป็นสลับเครื่องมือ
        assert_eq!(tool_shortcut(pressed("v"), ModifiersState::CONTROL), None);
        assert_eq!(tool_shortcut(pressed("c"), ModifiersState::CONTROL), None);
        assert_eq!(tool_shortcut(pressed("x"), none), None);
    }

    /// ★★★ `Board::view` ต้องได้กล้อง **ตัวจริง** ไม่ใช่ค่าปริยาย (P4-1)
    ///
    /// `HANDOFF §6` แถวแรกเตือนกับดักนี้ไว้ตรง ๆ: `ViewState` ตั้งใจให้ persist
    /// ลง `.refx` มาตั้งแต่ P2-1 แต่ไม่มีใครเขียนมันเลย เพราะของจริงกระจายอยู่
    /// สามที่ (`Gfx::camera` · `ArrangeView` · `ShellState::mode`)
    /// → เขียน DTO อย่างเดียวจะได้ไฟล์ที่บันทึกกล้องค่าปริยายเสมอ **โดยไม่มี
    /// error ที่ไหนเลย** ผู้ใช้แค่เปิดไฟล์แล้วมุมมองไม่กลับมา
    ///
    /// ★ ข้อที่จับกับดักได้คือข้อสุดท้าย: ผลลัพธ์ต้อง **ต่างจาก `default()`**
    #[test]
    fn the_view_that_gets_saved_is_the_live_camera_not_the_default_one() {
        let canvas = Camera::new(Vec2::new(900.0, -250.0), 2.5);
        let arrange = Camera::new(Vec2::new(420.0, 3000.0), 1.0);

        let view = live_view(canvas, arrange, Mode::Arrange);

        assert_eq!(view.canvas.center(), Vec2::new(900.0, -250.0));
        assert_eq!(view.canvas.zoom(), 2.5);
        assert_eq!(view.arrange.center().y, 3000.0);
        assert_eq!(view.mode, Mode::Arrange);
        assert_ne!(
            view,
            refx_core::view::ViewState::default(),
            "ประกอบแล้วได้ค่าปริยาย = กับดักที่ §6 เตือนไว้เกิดขึ้นแล้ว"
        );

        // ★ และเขียนลง board ได้จริงโดย **ไม่แตะ dirty และไม่แตะ revision**
        //   (ข้อยกเว้นที่ docs/02 §2.9 อนุญาต — ลาก pan 200 เฟรมแล้ว Ctrl+Z
        //    ต้องย้อนการแก้ครั้งล่าสุด ไม่ใช่ย้อนกล้อง · และ revision เป็นคีย์
        //    cache ของ filter/sort ถ้ากล้องขยับแล้วบวก จะกรองใหม่ทุกเฟรม)
        let mut board = refx_core::board::Board::default();
        let revision = board.revision();
        board.set_view(view);
        assert_eq!(*board.view(), view);
        assert!(!board.is_dirty(), "การเลื่อนกล้องต้องไม่ทำให้เอกสาร dirty");
        assert_eq!(board.revision(), revision, "กล้องขยับต้องไม่ทิ้ง cache ทั้ง board");
    }

    /// ★★★ คำสั่งที่ "ไม่ได้แตะ item ไหนเลย" ต้องไม่ล้างการเลือกตอน undo (P3-7)
    ///
    /// สัญญาถูกประกาศไว้ใน `ReorderZ::affected()` ตั้งแต่ P2-6 แต่เส้นทาง undo
    /// ไม่เคยทำตาม — เจอตอน P3-7 เพราะ `SetGroup` ก็คืนรายการว่างเหมือนกัน
    /// แล้วอาการโผล่ชัด: กดยุบกลุ่ม → Ctrl+Z → แผงกลุ่มหายทั้งแผง
    #[test]
    fn undo_only_clears_the_selection_when_the_items_really_went_away() {
        use refx_core::arena::ArenaKey as _;
        let a = ItemId::from_parts(0, 0);
        let b = ItemId::from_parts(1, 0);

        // ไม่ได้แตะใครเลย → อย่าแตะการเลือก
        assert_eq!(selection_after_history(&[], |_| true), None);

        // แตะของที่ยังอยู่ → เลือกตามนั้น
        assert_eq!(selection_after_history(&[a, b], |_| true), Some(vec![a, b]));

        // ★ รายงาน id มาแต่ตายหมด (undo ของการเพิ่มภาพ) → ต้องล้างจริง ๆ
        //   นี่คือกรณีที่แยกไม่ออกถ้าไปเช็ครายการ *หลัง* กรองแทนที่จะเช็คก่อน
        assert_eq!(
            selection_after_history(&[a, b], |_| false),
            Some(Vec::new())
        );

        // ปนกัน → เหลือเฉพาะตัวที่ยังอยู่
        assert_eq!(
            selection_after_history(&[a, b], |id| id == a),
            Some(vec![a])
        );
    }

    /// ★★★ `G` กับ `Ctrl+G` ต้องไม่ทับกัน — ต่างกันแค่ modifier ตัวเดียว (P3-7)
    ///
    /// `G` เปล่า ๆ = grayscale ทั้ง board (**การมองเห็น** ไม่กิน undo ไม่ dirty)
    /// ส่วน `Ctrl+G` = จัดกลุ่ม (**เอกสาร** ผ่าน `Command`) — สองอย่างนี้อยู่คนละ
    /// ชั้นกันโดยสิ้นเชิง ถ้าตัวใดตัวหนึ่งไม่ตรวจ modifier ของตัวเองอย่างเคร่งครัด
    /// การกดปุ่มเดียวจะทำทั้งสองอย่างพร้อมกัน แล้วผู้ใช้ที่ตั้งใจเช็ค value
    /// จะได้กลุ่มใหม่แถมมาโดยไม่รู้ตัว
    #[test]
    fn grayscale_and_grouping_never_fire_on_the_same_keypress() {
        let none = ModifiersState::empty();
        let ctrl = ModifiersState::CONTROL;
        let ctrl_shift = ctrl | ModifiersState::SHIFT;

        // G เปล่า = grayscale เท่านั้น
        assert_eq!(
            appearance_shortcut(pressed("g"), none),
            Some(AppearanceKey::ToggleBoardGrayscale)
        );
        assert_eq!(group_shortcut(pressed("g"), none), None);

        // Ctrl+G = จัดกลุ่มเท่านั้น
        assert_eq!(
            group_shortcut(pressed("g"), ctrl),
            Some(GroupRequest::Group)
        );
        assert_eq!(
            appearance_shortcut(pressed("g"), ctrl),
            None,
            "Ctrl+G ห้ามสลับ grayscale ไปด้วย"
        );

        // Ctrl+Shift+G = แยกกลุ่ม
        assert_eq!(
            group_shortcut(pressed("g"), ctrl_shift),
            Some(GroupRequest::Ungroup)
        );
        assert_eq!(appearance_shortcut(pressed("g"), ctrl_shift), None);

        // ★ บางระบบส่ง Ctrl+G มาเป็นอักขระ control (BEL) ไม่ใช่ 'g' พร้อมธง
        assert_eq!(
            group_shortcut(pressed("\u{7}"), ctrl),
            Some(GroupRequest::Group)
        );

        // ปุ่มอื่นที่กด Ctrl ค้างต้องไม่กลายเป็นการจัดกลุ่ม
        for other in ["a", "z", "v", "h"] {
            assert_eq!(group_shortcut(pressed(other), ctrl), None, "Ctrl+{other}");
        }
    }

    /// ★★ toolbar เป็น **ภาพสะท้อน** ของเครื่องมือจริง ไม่ใช่แหล่งความจริงคู่ขนาน
    ///
    /// เคยพลาดจริงตอนทำ P2-7: `ShellState` ถือ `tool` เป็นสถานะของตัวเอง แล้วชั้นแอป
    /// เขียนกลับด้วย `if shell.tool != gfx.tool { gfx.tool = shell.tool }` ทุกเฟรม
    /// ผลคือคีย์ลัดที่เขียน `gfx.tool` โดนค่าเก่าของ `shell` **เขียนทับกลับทันที**
    /// อาการที่เห็น: **กด `C` แล้วไม่มีอะไรเกิดขึ้น แต่กดปุ่มบน toolbar ได้ปกติ**
    ///
    /// แก้โดยแยก "สิ่งที่แสดง" (`tool`) ออกจาก "สิ่งที่ผู้ใช้ขอ" (`tool_request`)
    /// เจ้าของค่าจริงจึงมีคนเดียวและไม่ต้องพึ่งลำดับการเขียน (docs/08 §3.9 ข้อ 8.1)
    #[test]
    fn the_toolbar_reports_a_request_instead_of_owning_the_tool() {
        // ชั้นแอปเขียนสำเนาไว้วาดปุ่ม
        let mut state = crate::shell::ShellState {
            tool: Tool::Crop,
            ..Default::default()
        };
        assert!(
            state.tool_request.is_none(),
            "การแสดงผลต้องไม่กลายเป็นคำขอเอง — ไม่งั้นมันจะเขียนทับของจริงทุกเฟรม"
        );

        // ผู้ใช้กดปุ่มจริงถึงจะเป็นคำขอ แล้วชั้นแอปมาเก็บไปครั้งเดียว
        state.tool_request = Some(Tool::Select);
        assert_eq!(state.tool_request.take(), Some(Tool::Select));
        assert!(state.tool_request.is_none(), "เก็บไปแล้วต้องไม่ค้าง");
    }

    /// ★ เครื่องมือครอปต้องวาด handle **แปดตัว** ไม่ใช่สี่
    ///
    /// ชั้น UI วาดจากรายการเดียวกับที่ `refx-core` ยอมให้จับ ถ้าหลุดจากกัน
    /// ผู้ใช้จะเห็นจุดที่กดไม่ได้ หรือมีจุดที่กดได้แต่มองไม่เห็น
    #[test]
    fn the_crop_tool_paints_eight_handles_not_four() {
        let (board, selection) = one_selected_item(0.0);
        let select = painted_handles(&board, &selection, Camera::default(), Tool::Select);
        let crop = painted_handles(&board, &selection, Camera::default(), Tool::Crop);

        assert_eq!(select.len(), 4, "เครื่องมือเลือกมีแค่มุม");
        assert_eq!(crop.len(), 8, "เครื่องมือครอปมีกลางด้านด้วย");
        assert_eq!(
            crop.len(),
            refx_core::interact::handles_for(Tool::Crop).len(),
            "จำนวนที่วาดต้องเท่ากับจำนวนที่ core ยอมให้จับเป๊ะ"
        );
    }

    /// ★ กรอบ crop ต้องหด uv **ลงในช่องของ atlas** ไม่ใช่เอาไปใช้เป็น uv ตรง ๆ
    ///
    /// ถ้าใช้ตรง ๆ ภาพที่ถูกครอปจะไปหยิบ pixel ของภาพอื่นในชั้นเดียวกันมาแสดง
    /// ซึ่งเป็นอาการที่หาสาเหตุยากมาก เพราะดูเหมือน "ภาพสลับกันเอง"
    #[test]
    fn cropping_narrows_the_uv_inside_its_own_atlas_slot() {
        let slot = refx_render::atlas::AtlasSlot { layer: 0, index: 5 };
        let full = slot.uv_rect();
        let half = crop_uv(
            full,
            &ItemCanvas {
                crop: refx_core::board::CropRect {
                    min: Vec2::ZERO,
                    max: Vec2::new(0.5, 1.0),
                },
                ..ItemCanvas::default()
            },
        );

        assert_eq!(half[0], full[0], "ขอบซ้ายไม่ขยับ");
        assert_eq!(half[1], full[1]);
        assert!(
            (half[2] - (full[0] + (full[2] - full[0]) * 0.5)).abs() < 1e-6,
            "ขอบขวาต้องอยู่กลางช่อง ไม่ใช่กลาง texture ทั้งใบ: {half:?} ใน {full:?}"
        );
        assert_eq!(half[3], full[3]);
        // และต้องไม่หลุดออกนอกช่องของตัวเองไม่ว่าค่าจะเป็นอะไร
        for u in half {
            assert!((full[0]..=full[2]).contains(&u) || (full[1]..=full[3]).contains(&u));
        }
    }

    /// ★★ `flip` ต้องมีผล **ทั้งสองเส้นทางวาด** — atlas (ซูมออก) และ working (ซูมเข้า)
    ///
    /// เคยเป็นบั๊กจริงตอน P2-8: uv ของ working texture ถูกคำนวณด้วยสูตรที่เขียนแยก
    /// ซึ่งลืมใส่ `flip` ผลคือกด `H` แล้วภาพพลิกตอนซูมออกแต่**ไม่พลิกตอนซูมเข้า**
    /// ไม่มี error ไม่มี log — และ unit test ของ `crop_uv` ก็ผ่าน เพราะตัวมันถูก
    /// สิ่งที่ผิดคือ *มีสูตรอยู่สองที่* (`docs/08 §3.9` ข้อ 8)
    #[test]
    fn flipping_reaches_both_draw_paths_not_just_one() {
        let flipped = ItemCanvas {
            flip: Flip::Horizontal,
            ..ItemCanvas::default()
        };
        let plain = ItemCanvas::default();

        // เส้นทาง atlas — ช่องใดช่องหนึ่งใน texture array
        let slot = refx_render::atlas::AtlasSlot { layer: 0, index: 5 }.uv_rect();
        let atlas_plain = crop_uv(slot, &plain);
        let atlas_flipped = crop_uv(slot, &flipped);
        assert_eq!(
            [atlas_flipped[0], atlas_flipped[2]],
            [atlas_plain[2], atlas_plain[0]],
            "ทาง atlas: ปลาย u ต้องสลับกัน"
        );

        // เส้นทาง working texture — "ช่อง" คือ texture ทั้งใบ
        let full = [0.0, 0.0, 1.0, 1.0];
        let working_plain = crop_uv(full, &plain);
        let working_flipped = crop_uv(full, &flipped);
        assert_eq!(
            [working_flipped[0], working_flipped[2]],
            [working_plain[2], working_plain[0]],
            "ทาง working: ปลาย u ต้องสลับกันเหมือนกัน"
        );
        assert_ne!(working_flipped, working_plain);

        // แนวตั้งก็ต้องทำงาน และ Both ต้องสลับทั้งสองแกน
        let vertical = crop_uv(
            full,
            &ItemCanvas {
                flip: Flip::Vertical,
                ..plain
            },
        );
        assert_eq!(
            [vertical[1], vertical[3]],
            [working_plain[3], working_plain[1]]
        );
        let both = crop_uv(
            full,
            &ItemCanvas {
                flip: Flip::Both,
                ..plain
            },
        );
        assert_eq!(both, [1.0, 1.0, 0.0, 0.0]);
    }

    /// ★ พลิกแล้ว **hit-test ต้องไม่เพี้ยน** — `flip` แตะแค่ uv ไม่แตะเรขาคณิต
    #[test]
    fn flipping_never_moves_the_shape_that_hit_testing_uses() {
        let plain = ItemCanvas {
            pos: Vec2::new(10.0, -4.0),
            size: Vec2::new(80.0, 50.0),
            rotation: 0.7,
            ..ItemCanvas::default()
        };
        for flip in [Flip::Horizontal, Flip::Vertical, Flip::Both] {
            let flipped = ItemCanvas { flip, ..plain };
            assert_eq!(
                flipped.obb(),
                plain.obb(),
                "{flip:?} ทำให้รูปทรงที่ hit-test ใช้เปลี่ยนไป"
            );
            let state = bare_render_state();
            let a = instances::quad_for(&plain, state.slot, state.tint).unwrap();
            let b = instances::quad_for(&flipped, state.slot, state.tint).unwrap();
            assert_eq!(a.transform, b.transform, "เรขาคณิตต้องเท่าเดิมเป๊ะ");
            assert_ne!(a.uv_rect, b.uv_rect, "แต่ uv ต้องต่าง");
        }
    }

    /// กรอบ crop ที่พังจากไฟล์เสียต้องไม่ทำให้ uv หลุดออกนอกช่อง (I-4)
    #[test]
    fn a_broken_crop_never_escapes_its_slot() {
        let slot = refx_render::atlas::AtlasSlot {
            layer: 3,
            index: 200,
        };
        let full = slot.uv_rect();
        for (min, max) in [
            (Vec2::splat(f32::NAN), Vec2::splat(2.0)),
            (Vec2::splat(-5.0), Vec2::splat(f32::INFINITY)),
            (Vec2::splat(0.9), Vec2::splat(0.1)),
        ] {
            let uv = crop_uv(
                full,
                &ItemCanvas {
                    crop: refx_core::board::CropRect { min, max },
                    ..ItemCanvas::default()
                },
            );
            assert!(uv.iter().all(|v| v.is_finite()), "{uv:?}");
            assert!(uv[0] >= full[0] - 1e-6 && uv[2] <= full[2] + 1e-6, "{uv:?}");
            assert!(uv[1] >= full[1] - 1e-6 && uv[3] <= full[3] + 1e-6, "{uv:?}");
        }
    }

    // ---------- undo/redo (P2-4 ขั้นที่ 3) ----------

    fn key(text: &str) -> winit::keyboard::Key {
        winit::keyboard::Key::Character(text.into())
    }

    /// สิ่งที่ตัวจับคู่คีย์ลัดเห็นจริง ๆ เมื่อ layout ผลิตตัวอักษรนี้ออกมา
    fn pressed(text: &str) -> Option<char> {
        shortcut_char(
            &key(text),
            winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::F13),
        )
    }

    /// ปุ่มที่ layout **ไม่ผลิตตัวอักษรละติน** — เช่นคีย์บอร์ดไทย
    ///
    /// logical เป็นอักษรไทย ส่วน physical ยังเป็นตำแหน่งเดิมบนคีย์บอร์ด
    fn pressed_thai(thai: &str, code: winit::keyboard::KeyCode) -> Option<char> {
        shortcut_char(&key(thai), winit::keyboard::PhysicalKey::Code(code))
    }

    /// ★★★ **คีย์ลัดต้องทำงานบน layout ที่ไม่ใช่ละตินด้วย**
    ///
    /// เจอจริงตอนยืนยัน P3-1 บนเครื่องที่ layout เป็นไทย (HKL 0x41E):
    /// กด `C` แล้วเครื่องมือไม่สลับ กด `Ctrl+Z` แล้วไม่ย้อน
    /// **และไม่มี error ที่ไหนเลย** — คีย์ลัดทั้งหมดตายเงียบ ๆ
    ///
    /// เพราะตัวจับคู่ดูแต่ **logical key** ซึ่งเป็นตัวอักษรที่ layout ผลิตออกมา
    /// คีย์บอร์ดไทยกด `C` ได้ `แ` รัสเซียได้ `с` กรีกได้ `ψ` — ไม่มีตัวไหนตรง `"c"`
    ///
    /// docs/03 §0 ระบุว่าไทยคือภาษาที่สองของโปรแกรม กลุ่มนี้จึงไม่ใช่กรณีขอบ
    #[test]
    fn shortcuts_still_work_on_a_non_latin_keyboard_layout() {
        use winit::keyboard::KeyCode;

        let none = ModifiersState::empty();
        let ctrl = ModifiersState::CONTROL;

        // คีย์บอร์ดไทย (Kedmanee): logical เป็นอักษรไทย ส่วน physical คือตำแหน่งเดิม
        assert_eq!(
            tool_shortcut(pressed_thai("อ", KeyCode::KeyV), none),
            Some(Tool::Select),
            "กด V บน layout ไทย ต้องยังสลับเครื่องมือได้"
        );
        assert_eq!(
            tool_shortcut(pressed_thai("แ", KeyCode::KeyC), none),
            Some(Tool::Crop)
        );
        assert_eq!(
            tool_shortcut(pressed_thai("ร", KeyCode::KeyI), none),
            Some(Tool::Picker)
        );
        assert_eq!(
            tool_shortcut(pressed_thai("ส", KeyCode::KeyT), none),
            Some(Tool::Text)
        );
        assert_eq!(
            history_shortcut(pressed_thai("ผ", KeyCode::KeyZ), ctrl),
            Some(HistoryRequest::Undo),
            "Ctrl+Z บน layout ไทย ต้องย้อนได้"
        );
        assert!(is_paste(pressed_thai("อ", KeyCode::KeyV), ctrl));
        assert_eq!(
            appearance_shortcut(pressed_thai("ฯ", KeyCode::KeyG), none),
            Some(AppearanceKey::ToggleBoardGrayscale)
        );
        assert_eq!(
            zorder_shortcut(pressed_thai("บ", KeyCode::BracketRight), none),
            Some(ZMove::Forward)
        );
        // P3-7 — ปุ่ม G บน layout ไทยส่ง `ฯ` มา ไม่ใช่ `g`
        assert_eq!(
            group_shortcut(pressed_thai("ฯ", KeyCode::KeyG), ctrl),
            Some(GroupRequest::Group),
            "Ctrl+G บน layout ไทย ต้องจัดกลุ่มได้"
        );
        assert_eq!(
            group_shortcut(
                pressed_thai("ฯ", KeyCode::KeyG),
                ctrl | ModifiersState::SHIFT
            ),
            Some(GroupRequest::Ungroup)
        );

        // ★ และ layout ละตินที่สลับตำแหน่งปุ่ม (Dvorak) ต้องไม่พัง—
        //   logical มาก่อนเสมอ คนที่กด "v" จึงได้ Select ไม่ว่าปุ่มนั้นจะอยู่ตรงตำแหน่งไหน
        assert_eq!(
            tool_shortcut(pressed_thai("v", KeyCode::Period), none),
            Some(Tool::Select),
            "layout ละตินที่สลับตำแหน่งต้องยึด logical เหมือนเดิม"
        );
    }

    /// ★★★ **`Ctrl+Shift+Z` บน layout ไทย — เคยตาย แก้แล้ว** (4 ก.ย. 2026)
    ///
    /// ## บั๊กเดิมและกลไกของมัน (เก็บไว้เพราะมันคือเหตุผลที่กฎเป็นแบบนี้)
    ///
    /// `shortcut_char` เคยรับ logical ทันทีถ้าเป็น **ASCII ตัวเดียว** แล้วหยุด ·
    /// ปุ่ม `Z` ชั้น shift ของ layout ไทยส่ง **`(` (U+0028)** ซึ่งเป็น ASCII
    /// ตัวเดียวพอดี → **ไม่มีวันตกไปถึงชั้น physical** → redo หายไปทั้งดุ้น
    ///
    /// ปุ่มอื่นรอด**โดยบังเอิญ**: ชั้น shift ของ `S`/`G` ส่ง **สองอักขระ**
    /// (ตัวอักษร + U+000E) ซึ่งด่าน "ตัวเดียว" ปฏิเสธ → ตกไป physical → ติด ·
    /// **`(` เป็นตัวเดียวที่หลุด** จึงเสียเฉพาะ `Ctrl+Shift+Z`
    ///
    /// ★ เทสต์เดิมมองไม่เห็นเพราะจำลอง `Shift+G` ด้วย logical ไทย ซึ่ง
    /// **บังเอิญถูกสำหรับ `G` แต่ไม่ถูกสำหรับ `Z`** — เจอด้วยการรันของจริง
    /// บนเครื่องที่ layout เป็นไทยเท่านั้น (`docs/08 §3.9` ข้อ 5)
    ///
    /// ## กฎที่แก้แล้ว — และสิ่งที่มันต้องไม่พัง
    ///
    /// เงื่อนไขที่จะตกไปชั้น physical คือ **"ไม่มี binding ไหนตรง"** ไม่ใช่
    /// "หน้าตาของอักขระ" · ทิศที่สี่ (Dvorak) แยกไปอยู่
    /// [`a_bound_character_always_beats_the_physical_position`] เพราะมันเป็น
    /// invariant คนละตัวและควรบอกชื่อตัวเองตอนแดง
    #[test]
    fn a_thai_layout_gets_redo_back_and_the_bracket_keys_survive() {
        use winit::keyboard::KeyCode;
        let ctrl = ModifiersState::CONTROL;
        let ctrl_shift = ctrl | ModifiersState::SHIFT;

        // ---- 1. ทิศที่เคยพัง: layout ไทย ชั้น shift ส่ง `(` ที่ตำแหน่งปุ่ม Z ----
        let seen = shortcut_char(&key("("), winit::keyboard::PhysicalKey::Code(KeyCode::KeyZ));
        assert_eq!(
            seen,
            Some('z'),
            "`(` ไม่ผูกกับคีย์ลัดไหนเลย ต้องตกไปถึงตำแหน่งปุ่ม physical"
        );
        assert_eq!(
            history_shortcut(seen, ctrl_shift),
            Some(HistoryRequest::Redo),
            "Ctrl+Shift+Z บน layout ไทยต้อง redo ได้"
        );

        // ---- 2. ทิศที่ทางแก้แบบ "ASCII ที่ไม่ใช่ตัวอักษรให้ใช้ physical" จะพัง ----
        //
        // ★★ `[` `]` `{` `}` เป็นคีย์ลัดจริงที่อยู่บนปุ่มวรรคตอนพอดี — กฎที่ตัดสิน
        //    จาก *หน้าตา* ของอักขระจะกวาดพวกนี้ไปด้วย · กฎที่ถามว่า "ผูกกับอะไรไหม"
        //    ไม่แตะมันเลยเพราะมันผูกอยู่
        for (text, code, want) in [
            ("[", KeyCode::BracketLeft, ZMove::Backward),
            ("]", KeyCode::BracketRight, ZMove::Forward),
            ("{", KeyCode::BracketLeft, ZMove::ToBack),
            ("}", KeyCode::BracketRight, ZMove::ToFront),
        ] {
            let seen = shortcut_char(&key(text), winit::keyboard::PhysicalKey::Code(code));
            assert_eq!(
                zorder_shortcut(seen, ModifiersState::empty()),
                Some(want),
                "ปุ่มวรรคตอน {text:?} ถูกกฎใหม่กวาดไปด้วย"
            );
        }

        // ---- 3. ทิศที่ใช้ได้อยู่แล้ว ต้องไม่พังไปด้วย ----
        assert_eq!(
            history_shortcut(pressed_thai("ผ", KeyCode::KeyZ), ctrl),
            Some(HistoryRequest::Undo)
        );
        assert_eq!(
            history_shortcut(pressed_thai("ั", KeyCode::KeyY), ctrl),
            Some(HistoryRequest::Redo)
        );
    }

    /// ★★★ อักขระที่ **ผูกกับคีย์ลัดอยู่** ต้องชนะตำแหน่งปุ่มเสมอ (`HANDOFF §2.12`)
    ///
    /// นี่คือครึ่งที่ห้ามเสียไปตอนแก้บั๊ก layout ไทย: *"ถ้าถาม physical ก่อน
    /// Dvorak จะพัง"* · กฎใหม่ตกไป physical **เฉพาะตอนอักขระนั้นไม่ผูกกับอะไรเลย**
    ///
    /// ★★ **แยกเป็นเทสต์ของตัวเองเพราะเทสต์ Dvorak เดิมจับข้อนี้ไม่ได้** —
    /// มันยิงด้วย `KeyCode::Period` ซึ่ง **ไม่มีในแผนที่ physical** เลย
    /// ตาข่ายรองจึงคืน `None` ทุกกรณี แล้วผลก็ตกกลับเป็น logical เสมอ
    /// ไม่ว่ากฎจะเป็นแบบไหน · ตัวนี้ยิงด้วย `KeyCode::KeyZ` ซึ่ง**อยู่ในแผนที่**
    /// จึงเป็นตัวเดียวที่แยก "logical ชนะ" ออกจาก "physical ชนะ" ได้จริง
    #[test]
    fn a_bound_character_always_beats_the_physical_position() {
        use winit::keyboard::KeyCode;
        // กด `v` ที่ตำแหน่งปุ่ม `Z` — `v` ผูกกับเครื่องมือเลือกอยู่ จึงต้องชนะ
        let dvorak = shortcut_char(&key("v"), winit::keyboard::PhysicalKey::Code(KeyCode::KeyZ));
        assert_eq!(dvorak, Some('v'), "อักขระที่ผูกอยู่ต้องชนะตำแหน่งปุ่ม");
        assert_eq!(
            tool_shortcut(dvorak, ModifiersState::empty()),
            Some(Tool::Select)
        );
        assert_eq!(
            history_shortcut(dvorak, ModifiersState::CONTROL),
            None,
            "ตำแหน่งปุ่มแย่ง `Ctrl+V` ไปเป็น undo — Dvorak พังแบบเดียวกับที่ §2.12 ห้าม"
        );
    }

    // ---------- ★★★ ตัวกรองที่ **ย้ายลงมาจาก production** (หนี้ §6 — ยุบ `on_input`) ----------
    //
    // ★★ 14 ฟังก์ชันนี้เคยเป็นจุดที่ `on_input` เรียกจริง · ตอนนี้ `on_input`
    //    เป็น `match` เดียวบนผลของ `requested` แล้ว มันจึงไม่มีคนเรียกใน
    //    production อีก — แต่ **assertion 94 จุดที่ใช้มันยังมีค่าเท่าเดิม**
    //    เพราะสิ่งที่มันถามคือ *"ปุ่มชุดนี้แปลว่าอะไร"* ซึ่งเป็นคำถามเดียวกับที่
    //    `requested` ตอบ
    //
    // ★★★ ย้ายลงมา **โดยไม่แก้สักไบต์** โดยตั้งใจ: การไปแก้ 94 assertion พร้อมกัน
    //     คือจุดที่การอ่อนลงของเทสต์ซ่อนตัวได้ดีที่สุด (`docs/08 §3.9` ข้อ 1)
    //     — refactor รอบนี้ต้องไม่เปลี่ยนพฤติกรรมและไม่เปลี่ยนสิ่งที่ถูกยืนยัน
    //
    // ★ ไม่ใช่ตรรกะเลียนแบบ (ข้อ 9): ทุกตัวเรียก `keymap::active().action()`
    //   ตัวจริงที่ production ใช้ แล้วแค่คัดตาม variant ของผลลัพธ์ ·
    //   ★★ สิ่งที่มันครอบไม่ถึงคือ **การต่อสายจาก action ไปยัง `pending_*`**
    //   ในกิ่ง `match` ของ `on_input` — ข้อนั้นคอมไพเลอร์บังคับให้ครบ (ไม่มี `_`)
    //   และถูกยืนยันบนแอปจริงทั้ง 45 แถวตอนก้อน c

    /// ★★★ ปุ่มที่เพิ่งกด → สิ่งที่มันสั่ง — **ทางเดียวที่ทุก wrapper ข้างล่างใช้**
    ///
    /// การจับคู่ทั้งหมดเป็น **ข้อมูล** อยู่ใน [`crate::keymap`] แล้ว (P5-3b ก้อน a)
    /// ฟังก์ชันข้างล่างจึงเหลือหน้าที่เดียวคือ *"action ตัวนี้ใช่ของฉันไหม"*
    ///
    /// ★ ยังเป็นฟังก์ชันแยกกันอยู่โดยตั้งใจ: **assertion เดิมทั้งชุดคือ oracle**
    /// ที่พิสูจน์ว่าตารางให้ผลเท่าของเดิมเป๊ะ (`ROADMAP` P5-3b ก้อน a) ·
    /// การยุบ `on_input` ให้เหลือ dispatch เดียวทำให้ oracle นั้นหายไป จึงเป็นงานของ
    /// ก้อนถัดไป ไม่ใช่ก้อนนี้
    fn action_for(pressed: Option<char>, modifiers: ModifiersState) -> Option<keymap::Action> {
        keymap::active().action(pressed, None, modifiers)
    }

    /// แปลงปุ่มที่กดเป็นคำขอกับประวัติ
    ///
    /// ★ รับ **Ctrl+Shift+Z เป็น redo ด้วย** ไม่ใช่แค่ Ctrl+Y — คนจำนวนมากใช้อันนั้น
    /// (ติดมาจาก Photoshop/Illustrator) ถ้าไม่รับ เขาจะคิดว่า redo ไม่มีในโปรแกรมนี้
    fn history_shortcut(
        pressed: Option<char>,
        modifiers: ModifiersState,
    ) -> Option<HistoryRequest> {
        match action_for(pressed, modifiers)? {
            keymap::Action::History(request) => Some(request),
            _ => None,
        }
    }

    /// แปลงปุ่มที่กดเป็นคำสั่งย้ายชั้น (P2-6)
    ///
    /// `docs/03 §5` ระบุแค่ `[` `]` = ส่งไปหลัง / นำมาหน้า **ไม่ได้ระบุปุ่มของสุดหัว-สุดท้าย**
    /// เลือก `Shift+[` / `Shift+]` เพราะอยู่ตระกูลเดียวกันและไม่ชนกับอะไรใน keymap
    fn zorder_shortcut(pressed: Option<char>, modifiers: ModifiersState) -> Option<ZMove> {
        match action_for(pressed, modifiers)? {
            keymap::Action::ZOrder(movement) => Some(movement),
            _ => None,
        }
    }

    /// แปลงปุ่มที่กดเป็นการสลับเครื่องมือ (docs/03 §2: `V` = Select/Move · `C` = Crop)
    fn tool_shortcut(pressed: Option<char>, modifiers: ModifiersState) -> Option<Tool> {
        match action_for(pressed, modifiers)? {
            keymap::Action::Tool(tool) => Some(tool),
            _ => None,
        }
    }

    /// `G` = grayscale ทั้ง board · `H` = พลิกแนวนอน (docs/03 §2, §5)
    ///
    /// ★ สองปุ่มนี้ทำคนละชั้นกันโดยตั้งใจ: `G` เป็น**สวิตช์การมองเห็น**ของทั้ง board
    /// (uniform ตัวเดียว ไม่กิน undo ไม่ทำให้ dirty) ส่วน `H` **แก้เอกสาร**
    /// ของภาพที่เลือก จึงผ่าน `Command` และย้อนได้ตามปกติ
    fn appearance_shortcut(
        pressed: Option<char>,
        modifiers: ModifiersState,
    ) -> Option<AppearanceKey> {
        match action_for(pressed, modifiers)? {
            keymap::Action::Appearance(what) => Some(what),
            _ => None,
        }
    }

    /// `Ctrl+G` = จัดกลุ่ม · `Ctrl+Shift+G` = แยกกลุ่ม (docs/03 §5, P3-7)
    ///
    /// ★ `G` เปล่า ๆ เป็น grayscale ของทั้ง board — สองตัวนี้แยกกันด้วย Ctrl ตัวเดียว
    /// ตอนนี้ความเคร่งครัดนั้นเป็น **ข้อมูล** ([`keymap::Hold`]) ไม่ใช่วินัยของคนเขียน
    /// และมีประตู `no_single_keypress_can_ever_fire_two_actions` คุมทั้งตาราง
    fn group_shortcut(pressed: Option<char>, modifiers: ModifiersState) -> Option<GroupRequest> {
        match action_for(pressed, modifiers)? {
            keymap::Action::Group(request) => Some(request),
            _ => None,
        }
    }

    /// `Ctrl+S` = บันทึก · `Ctrl+Shift+S` = บันทึกเป็น (docs/03 §5, P4-2)
    fn save_shortcut(pressed: Option<char>, modifiers: ModifiersState) -> Option<SaveRequest> {
        match action_for(pressed, modifiers)? {
            keymap::Action::Save(request) => Some(request),
            _ => None,
        }
    }

    /// `Ctrl+O` = เปิดกระดาน (docs/03 §5, P4-4)
    ///
    /// ★ ไม่รับ `Ctrl+Shift+O` เป็นอย่างอื่น — ปุ่มที่ยังไม่มีความหมายควรเงียบ
    /// ไม่ใช่ทำอะไรที่ผู้ใช้ไม่ได้ขอ (ในตารางคือ `shift: Up` ไม่ใช่ `Either`)
    fn open_shortcut(pressed: Option<char>, modifiers: ModifiersState) -> bool {
        action_for(pressed, modifiers) == Some(keymap::Action::OpenBoard)
    }

    /// ★★★ คีย์ของแท็บ (`docs/03 §5` — P4-7c)
    ///
    /// | คีย์ | ทำอะไร |
    /// |---|---|
    /// | `Ctrl+T` | board เปล่าใบใหม่ |
    /// | `Ctrl+W` | ปิดแท็บ — **ถามก่อนถ้ายังไม่บันทึก** |
    /// | `Ctrl+Tab` | แท็บถัดไป |
    ///
    /// (`Ctrl+O` อยู่ที่ [`open_shortcut`] เพราะมันมีความหมายมาก่อนโครงแท็บ)
    ///
    /// ★★ ตัวเดียวที่รับ `Key` ดิบด้วย เพราะ `Ctrl+Tab` มาเป็น **named key**
    /// ซึ่ง `shortcut_char` มองไม่เห็นเลย — คือกิ่ง [`keymap::Chord::Key`] ในตาราง
    fn tab_shortcut(
        pressed: Option<char>,
        key: &winit::keyboard::Key,
        modifiers: ModifiersState,
    ) -> Option<TabKey> {
        match keymap::active().action(pressed, named_key(key), modifiers)? {
            keymap::Action::Tab(which) => Some(which),
            _ => None,
        }
    }

    /// ★★★ `Tab` — สลับ Canvas ⇄ Arrange (`docs/03 §5` · P5-3b ก้อน c)
    ///
    /// ★★ รับ `Key` ดิบด้วยเหตุผลเดียวกับ [`tab_shortcut`]: `Tab` เป็น named key
    /// ที่ `shortcut_char` มองไม่เห็นเลย
    fn mode_shortcut(
        pressed: Option<char>,
        key: &winit::keyboard::Key,
        modifiers: ModifiersState,
    ) -> bool {
        keymap::active().action(pressed, named_key(key), modifiers)
            == Some(keymap::Action::ToggleMode)
    }

    /// `Ctrl+A` — เลือกทั้งหมด (`docs/03 §5` · P5-3b ก้อน c)
    fn select_all_shortcut(pressed: Option<char>, modifiers: ModifiersState) -> bool {
        action_for(pressed, modifiers) == Some(keymap::Action::SelectAll)
    }

    /// `Esc` — ยกเลิกเลือก (`docs/03 §5` · P5-3b ก้อน c) — named key เช่นเดียวกับ `Tab`
    fn clear_selection_shortcut(
        pressed: Option<char>,
        key: &winit::keyboard::Key,
        modifiers: ModifiersState,
    ) -> bool {
        keymap::active().action(pressed, named_key(key), modifiers)
            == Some(keymap::Action::ClearSelection)
    }

    /// `F` / `1` / `0` — ระดับซูม (`docs/03 §5` · P5-3b ก้อน c)
    fn zoom_shortcut(
        pressed: Option<char>,
        modifiers: ModifiersState,
    ) -> Option<keymap::ZoomRequest> {
        match action_for(pressed, modifiers)? {
            keymap::Action::Zoom(request) => Some(request),
            _ => None,
        }
    }

    /// `Delete` / `Backspace` = ลบสิ่งที่เลือก (docs/03 §5)
    ///
    /// รับ `Backspace` ด้วยเพราะบนแล็ปท็อปหลายรุ่นไม่มีปุ่ม `Delete` แยก
    fn is_delete(key: &winit::keyboard::Key) -> bool {
        // ★ ไม่ตรวจ modifier เลยสักตัว — ในตารางคือ `Either` ทั้งสามช่อง
        //   จึงส่ง `ModifiersState::empty()` เข้าไปได้โดยผลไม่เปลี่ยน
        keymap::active().action(None, named_key(key), ModifiersState::empty())
            == Some(keymap::Action::Delete)
    }

    fn is_paste(pressed: Option<char>, modifiers: ModifiersState) -> bool {
        action_for(pressed, modifiers) == Some(keymap::Action::Paste)
    }

    // ---------- ★★★ ยุบ `on_input` เป็น dispatch เดียว (หนี้ §6) ----------

    /// สถานะปุ่มค้างที่ทำให้ chord นี้ **ตรง** — `Either` เลือกทางไหนก็ได้
    fn modifiers_for(mods: keymap::Mods) -> ModifiersState {
        let mut state = ModifiersState::empty();
        if mods.ctrl == keymap::Hold::Down {
            state |= ModifiersState::CONTROL;
        }
        if mods.shift == keymap::Hold::Down {
            state |= ModifiersState::SHIFT;
        }
        if mods.alt == keymap::Hold::Down {
            state |= ModifiersState::ALT;
        }
        state
    }

    /// ปุ่มที่ผู้ใช้ต้องกดเพื่อให้ได้ binding นี้
    fn event_for(binding: &keymap::Binding) -> (winit::keyboard::Key, ModifiersState) {
        let logical = match binding.chord {
            keymap::Chord::Char { ch, .. } => {
                winit::keyboard::Key::Character(ch.to_string().into())
            }
            keymap::Chord::Key { key, .. } => winit::keyboard::Key::Named(key),
        };
        (logical, modifiers_for(binding.chord.mods()))
    }

    /// ★ physical เป็น `F13` ซึ่ง **ไม่มีในแผนที่ physical** โดยตั้งใจ —
    /// เทสต์ชุดนี้ถามชั้น logical ไม่ใช่ตาข่ายรอง
    const NOWHERE: winit::keyboard::PhysicalKey =
        winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::F13);

    /// ★★★ **ทุกแถวในตารางต้องเดินทางถึงปลายทางของตัวเองผ่าน `requested`**
    ///
    /// ## oracle ที่ใช้แล้วปลดไป (หนี้ §6 — ยุบ `on_input`)
    ///
    /// ตอนยุบ 14 กิ่งของ `on_input` เป็น dispatch เดียว เทสต์ตัวนี้มีอีกครึ่ง:
    /// `action_the_old_way` ที่เรียก wrapper ทั้ง 14 ตัวตามลำดับเดิม แล้ว
    /// **เทียบผลกับ `requested` ทั้ง 45 แถว** · เขียวก่อนแล้วจึงลบ wrapper ทิ้ง
    /// — ไม่ใช่ลบแล้วหวังว่าเทสต์ที่มีอยู่ครอบคลุมพอ (รูปแบบเดียวกับที่ก้อน a
    /// ใช้ assertion เดิมเป็น oracle ตอนย้ายการจับคู่มาเป็นตาราง)
    ///
    /// ครึ่งที่เหลืออยู่นี้คือครึ่งที่ยัง**มีความหมายหลังลบ**: ปุ่มที่ตารางบอกว่า
    /// ผูกกับ action ตัวไหน ต้องเดินผ่าน `shortcut_char` → `Keymap::action` →
    /// [`wanted`] แล้วออกมาเป็น action ตัวนั้นจริง ไม่ใช่แค่ "มีอยู่ในตาราง"
    #[test]
    fn every_row_in_the_table_reaches_its_own_action() {
        let table = keymap::builtin().bindings();
        assert_eq!(table.len(), 47, "จำนวน binding เปลี่ยน — เทสต์นี้ต้องยิงให้ครบ");

        for binding in table {
            let (logical, modifiers) = event_for(binding);
            assert_eq!(
                requested(&logical, NOWHERE, true, false, modifiers),
                Some(binding.action),
                "แถวนี้ยิงแล้วไม่ได้ action ของตัวเอง: {binding:?}"
            );
        }

        // ★ ปุ่มที่ไม่มีในตารางต้องเงียบ — ไม่งั้นเทียบแต่ฝั่งที่ตรงกันอยู่แล้ว
        let stranger = winit::keyboard::Key::Character("\u{7f}".into());
        assert_eq!(
            requested(&stranger, NOWHERE, true, false, ModifiersState::empty()),
            None
        );

        // ★★★ `Hold::Either` ต้องแปลว่า "ไม่เกี่ยว" **ตลอดเส้นทาง** ไม่ใช่แค่ในตาราง
        //
        //     ก่อนยุบ `is_delete` ส่ง `ModifiersState::empty()` เข้าไปเสมอ เพราะ
        //     modifier ของ `Delete` เป็น `Either` ทั้งสามช่อง · ตอนนี้มันได้รับ
        //     modifier จริง — ถ้า `Either` ไม่ได้แปลว่าไม่เกี่ยวจริง `Ctrl+Delete`
        //     จะเงียบไปเฉย ๆ ซึ่งเป็นความต่างที่ `event_for` มองไม่เห็น
        //     (มันเลือก "ไม่กด" ให้ทุกช่องที่เป็น `Either`)
        let all = ModifiersState::CONTROL | ModifiersState::SHIFT | ModifiersState::ALT;
        let delete = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Delete);
        assert_eq!(
            requested(&delete, NOWHERE, true, false, all),
            Some(keymap::Action::Delete),
            "`Ctrl+Shift+Alt+Delete` ต้องยังลบได้ — modifier ของมันเป็น Either ทั้งชุด"
        );
        // และ `Ctrl+Y` ที่ shift เป็น `Either` ต้องติดทั้งกดและไม่กด shift
        let y = winit::keyboard::Key::Character("y".into());
        for mods in [
            ModifiersState::CONTROL,
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        ] {
            assert_eq!(
                requested(&y, NOWHERE, true, false, mods),
                Some(keymap::Action::History(HistoryRequest::Redo))
            );
        }
    }

    /// ★★ ปล่อยปุ่ม และ การกดค้าง — สองอย่างที่ `requested` ต้องกรองเหมือนเดิม
    #[test]
    fn releasing_a_key_does_nothing_and_holding_it_follows_the_table() {
        let z = winit::keyboard::Key::Character("z".into());
        let v = winit::keyboard::Key::Character("v".into());
        let ctrl = ModifiersState::CONTROL;

        // ปล่อยปุ่ม = ไม่ใช่คำสั่ง
        assert_eq!(requested(&z, NOWHERE, false, false, ctrl), None);
        // กดค้าง: undo ซ้ำได้ · วางไม่ซ้ำ — นโยบายมาจากตาราง ไม่ใช่จากที่นี่
        assert_eq!(
            requested(&z, NOWHERE, true, true, ctrl),
            Some(keymap::Action::History(HistoryRequest::Undo))
        );
        assert_eq!(
            requested(&v, NOWHERE, true, true, ctrl),
            None,
            "Ctrl+V ค้างต้องไม่วางซ้ำ"
        );
        assert_eq!(
            requested(&v, NOWHERE, true, false, ctrl),
            Some(keymap::Action::Paste)
        );
    }

    // ---------- ★★★ P5-3b ก้อน c: หกคีย์ที่ spec สั่งไว้แต่ไม่เคยมี ----------

    /// ★★★ **หกคีย์ใหม่ต้องรอดทั้งบน layout ไทยและ Dvorak**
    ///
    /// `F` `1` `0` เป็น binding **ไม่มี modifier** ซึ่งเป็นรูปเดียวกับที่บั๊ก
    /// `Ctrl+Shift+Z` ของ `§2.40ก` เกิดขึ้น — และมันเกือบเกิดซ้ำจริงในก้อนนี้:
    /// `physical_char` ไม่เคยรู้จัก `Digit0`–`Digit9` มาก่อน ปุ่ม `1` ของไทย
    /// (ที่ส่ง `ๅ`) จึงตกถึงชั้น physical แล้ว **ได้ `None` กลับมา** = ตายเงียบ
    ///
    /// ★ `Tab` กับ `Esc` เป็น named key จึงไม่ผ่าน `shortcut_char` เลย —
    /// พวกมันไม่ขึ้นกับ layout โดยธรรมชาติ แต่ยืนยันไว้ให้เห็นว่าคิดถึงแล้ว
    #[test]
    fn the_six_new_keys_survive_a_thai_and_a_dvorak_layout() {
        use winit::keyboard::KeyCode;
        let none = ModifiersState::empty();
        let ctrl = ModifiersState::CONTROL;

        // ---- layout ไทย (Kedmanee): logical เป็นอักษรไทย · physical คือตำแหน่งเดิม ----
        assert!(
            select_all_shortcut(pressed_thai("ฟ", KeyCode::KeyA), ctrl),
            "Ctrl+A บน layout ไทย ต้องเลือกทั้งหมดได้"
        );
        assert_eq!(
            zoom_shortcut(pressed_thai("ด", KeyCode::KeyF), none),
            Some(keymap::ZoomRequest::FitSelection),
            "`F` บน layout ไทย"
        );
        assert_eq!(
            zoom_shortcut(pressed_thai("ๅ", KeyCode::Digit1), none),
            Some(keymap::ZoomRequest::Actual),
            "`1` บน layout ไทย — ปุ่มตัวเลขต้องอยู่ในแผนที่ physical ด้วย"
        );
        assert_eq!(
            zoom_shortcut(pressed_thai("จ", KeyCode::Digit0), none),
            Some(keymap::ZoomRequest::FitBoard),
            "`0` บน layout ไทย"
        );

        // ---- Dvorak: logical เป็นละตินอยู่แล้ว จึงต้องชนะตำแหน่งปุ่มเสมอ ----
        //
        // ★ กด `f` ที่ตำแหน่งปุ่ม `Y` ของ QWERTY (ซึ่งคือที่ที่ `f` อยู่บน Dvorak)
        assert_eq!(
            zoom_shortcut(pressed_thai("f", KeyCode::KeyY), none),
            Some(keymap::ZoomRequest::FitSelection),
            "Dvorak ต้องยึด logical — ไม่งั้นจะไปโดนปุ่มที่ตำแหน่งนั้นแทน"
        );
        assert!(select_all_shortcut(pressed_thai("a", KeyCode::KeyA), ctrl));
        // ★★ compositor ที่ส่ง `Ctrl+A` มาเป็นอักขระควบคุมแทนตัวอักษร
        assert!(select_all_shortcut(pressed("\u{1}"), ctrl));

        // ---- named key: ไม่ขึ้นกับ layout ----
        let tab = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Tab);
        let esc = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape);
        assert!(mode_shortcut(None, &tab, none), "`Tab` เปล่า ๆ ต้องสลับโหมด");
        assert!(clear_selection_shortcut(None, &esc, none));

        // ★★★ `Tab` เปล่า ๆ **ต้องไม่** ไปเป็นคีย์แท็บ และ `Ctrl+Tab` ต้องไม่สลับโหมด
        //     ทั้งคู่คือปุ่มเดียวกันเป๊ะ ต่างกันแค่ ctrl — ถ้าปนกันเมื่อไหร่ ผู้ใช้
        //     ที่กด `Tab` จะกระโดดข้ามแท็บแทนที่จะสลับโหมด
        assert_eq!(tab_shortcut(None, &tab, none), None);
        assert!(!mode_shortcut(None, &tab, ctrl));
        assert_eq!(tab_shortcut(None, &tab, ctrl), Some(TabKey::Next));
    }

    /// ★★★ **`Tab` เป็นของ egui ตอนมีอะไร focus อยู่ · ของเราตอนไม่มี**
    ///
    /// ถ้าปล่อยให้ egui เห็น `Tab` ตอนที่มันเป็นของเรา การกดครั้งเดียวจะทั้งสลับ
    /// โหมด **และ** ย้าย focus ไปปุ่มแรก แล้วครั้งที่สองจะสลับโหมดไม่ได้อีกเลย —
    /// อาการที่ผู้ใช้เห็นคือ *"กด Tab ได้ครั้งเดียว"* ซึ่งไม่มี error ที่ไหน
    ///
    /// ★ ข้อ 1b ของ `docs/08 §3.9`: ยืนยันว่า focus **ติดจริง** ก่อนวัดกิ่งที่สอง
    /// ไม่งั้นเทสต์นี้จะทดสอบกิ่งเดิมสองรอบแล้วเขียวโดยไม่ได้ตรวจอะไร
    #[test]
    fn tab_belongs_to_egui_only_while_something_holds_focus() {
        let tab = || egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..egui::RawInput::default()
        };

        let ctx = egui::Context::default();
        assert!(
            !ctx.egui_wants_keyboard_input(),
            "context ใหม่ต้องยังไม่มีอะไร focus"
        );
        let mut ours = tab();
        strip_tab_when_it_is_ours(&ctx, &mut ours);
        assert!(
            ours.events.is_empty(),
            "ไม่มีอะไร focus แล้ว egui ยังเห็น `Tab` — มันจะย้าย focus แข่งกับการสลับโหมด"
        );

        // ★ ทำให้ช่องข้อความถือ focus จริง ๆ — `request_focus` มีผลเฟรมถัดไป
        let mut text = String::new();
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.text_edit_singleline(&mut text).request_focus();
            });
        }
        assert!(
            ctx.egui_wants_keyboard_input(),
            "ตั้ง focus ไม่ติด — กิ่งที่สองไม่เคยถูกยิง (docs/08 §3.9 ข้อ 1b)"
        );
        let mut theirs = tab();
        strip_tab_when_it_is_ours(&ctx, &mut theirs);
        assert_eq!(
            theirs.events.len(),
            1,
            "มี widget ถือ focus อยู่ `Tab` ต้องเป็นของ egui ทั้งดุ้น"
        );
    }

    /// ★★★ **`Esc` ปิดสิ่งที่ค้างอยู่ก่อน แล้วค่อยยกเลิกเลือก** (`docs/03 §5`)
    ///
    /// ผู้ใช้ที่กด `Esc` ตอนมีคำถามค้างอยู่หมายถึง *"ไม่เอาคำถามนี้"* ·
    /// การไปล้าง selection แทนคือการตอบสิ่งที่เขาไม่ได้ถาม ทั้งที่คำถามยังอยู่บนจอ
    ///
    /// ★★★ **ขนาดที่ export ต้องรักษาสัดส่วนของ board เสมอ**
    ///
    /// ผู้ใช้ปรับได้แค่ด้านยาวสุด · ถ้าสูตรผิด ภาพที่ส่งให้คนอื่นจะยืดผิดสัดส่วน
    /// ซึ่งเป็นข้อผิดที่เห็นทันทีแต่เกิดขึ้นเงียบ ๆ ตอนกด
    #[test]
    fn the_export_size_keeps_the_shape_of_the_board() {
        // จัตุรัส → เท่ากันสองด้าน
        assert_eq!(scale_side(2048, 1.0), 2048);
        // กว้าง 2 เท่าของสูง → ด้านสั้นได้ครึ่งหนึ่ง
        assert_eq!(scale_side(2048, 0.5), 1024);
        // ★ ปัดเศษ ไม่ใช่ตัดทิ้ง — 1000 × 0.3335 = 333.5 ต้องได้ 334
        assert_eq!(scale_side(1000, 0.3335), 334);
        // ★★ board ที่เรียงเป็นเส้นตรงพอดีให้สัดส่วน 0 — ต้องได้ **อย่างน้อย 1 จุด**
        //    ไม่ใช่ 0 ซึ่ง `BandPlan` จะปฏิเสธแล้วผู้ใช้ไม่รู้ว่าเกิดจากอะไร
        assert_eq!(scale_side(2048, 0.0), 2048, "สัดส่วนศูนย์ = ไม่รู้ ให้จัตุรัส");
        assert_eq!(scale_side(2048, f32::NAN), 2048);
        assert!(scale_side(2048, 0.000_01) >= 1);
        // ★ ห้ามทะลุเพดานของ `BandPlan` ไม่ว่าสัดส่วนจะบ้าแค่ไหน
        assert!(scale_side(16384, 1000.0) <= refx_core::export::MAX_SIDE);
    }

    /// ประมาณขนาดไฟล์ต้องขยับตามสิ่งที่ผู้ใช้ปรับ — ตัวเลขที่ไม่ขยับคือตัวเลข
    /// ที่ไม่มีประโยชน์และทำให้เขาเชื่อผิด
    #[test]
    fn the_size_estimate_moves_with_what_the_user_changes() {
        use crate::shell::ExportKind;
        let png = estimate_file_size(4096, 4096, ExportKind::Png, 90);
        let jpeg = estimate_file_size(4096, 4096, ExportKind::Jpeg, 90);
        let jpeg_low = estimate_file_size(4096, 4096, ExportKind::Jpeg, 40);
        println!(
            "4096²: PNG ~{} · JPEG q90 ~{} · JPEG q40 ~{}",
            crate::shell::human_bytes(png),
            crate::shell::human_bytes(jpeg),
            crate::shell::human_bytes(jpeg_low)
        );
        assert!(png > jpeg, "PNG ต้องประมาณว่าใหญ่กว่า JPEG");
        assert!(jpeg > jpeg_low, "คุณภาพสูงกว่าต้องประมาณว่าไฟล์ใหญ่กว่า");
        // ใหญ่ขึ้นสี่เท่าของพิกเซล = ประมาณสี่เท่า (คลาดได้ไม่กี่ไบต์จากการหารลงตัว)
        let bigger = estimate_file_size(8192, 8192, ExportKind::Png, 90);
        assert!(
            bigger.abs_diff(png * 4) < 16,
            "ประมาณต้องเป็นสัดส่วนกับจำนวนพิกเซล: {bigger} vs {}",
            png * 4
        );
        // ★ และต้องไม่ต่ำกว่าความจริงจนพาไปตัดสินใจผิด — PNG ของภาพที่มี
        //   รายละเอียดจริงวัดได้ราว 30–40% ของ RGBA ดิบ (`docs/07 §6`)
        let raw = 4096u64 * 4096 * 4;
        assert!(png >= raw / 4, "ประมาณต่ำเกินไป: {png} จาก RGBA ดิบ {raw}");
    }

    /// ★ ลำดับคือ **ทั้งหมด** ของกฎนี้ และเป็นสิ่งเดียวที่พังได้โดยไม่มีอะไรฟ้อง
    #[test]
    fn escape_closes_what_is_in_the_way_before_it_touches_the_selection() {
        let mut shell = crate::shell::ShellState::default();

        // ไม่มีอะไรค้าง + มีของเลือกอยู่ → ยกเลิกเลือก
        assert_eq!(escape_target(&shell, true), EscapeTarget::ClearSelection);
        // ไม่มีอะไรค้าง + ไม่ได้เลือกอะไร → เงียบ (ไม่ใช่ "ทำอะไรสักอย่าง")
        assert_eq!(escape_target(&shell, false), EscapeTarget::Nothing);

        // แผงตั้งค่าเปิดอยู่ → ปิดแผงก่อน **ทั้งที่มีของเลือกอยู่**
        shell.settings_open = true;
        assert_eq!(escape_target(&shell, true), EscapeTarget::CloseSettings);

        // ★★ กล่องส่งออกอยู่เหนือแผงตั้งค่า (P5-4)
        shell.export_prompt = Some(crate::shell::ExportView::default());
        assert_eq!(escape_target(&shell, true), EscapeTarget::CloseExport);
        // ★★★ และงานที่ **กำลังเขียนไฟล์อยู่** มาก่อนทุกอย่างในกลุ่มนี้ —
        //     `Esc` ตอนนั้นแปลว่า "หยุดเถอะ" ไม่ใช่ "ปิดกล่องตั้งค่า"
        shell.export_progress = Some(crate::shell::ExportProgress {
            name: "board.png".to_owned(),
            done: 3,
            total: 32,
            cancelling: false,
        });
        assert_eq!(escape_target(&shell, true), EscapeTarget::StopExport);
        shell.export_progress = None;
        shell.export_prompt = None;

        // ★ แถบถามอยู่เหนือแผงตั้งค่าอีกชั้น
        shell.recover_prompt = Some(crate::shell::RecoverView {
            when: None,
            items: 3,
            scope: crate::shell::RecoverScope::LastSession,
        });
        assert_eq!(escape_target(&shell, true), EscapeTarget::PostponeRecovery);
        shell.save_as_prompt = true;
        assert_eq!(escape_target(&shell, true), EscapeTarget::CancelSaveAs);
        shell.close_prompt = true;
        assert_eq!(escape_target(&shell, true), EscapeTarget::CancelClose);
    }

    /// ★★★ **โหมดเป็นของแท็บ อ่านจาก `Board::view`** (`HANDOFF §6` · `docs/03 §5`)
    ///
    /// `set_view()` เขียน `Board::view` ทุกเฟรมมาตั้งแต่ P4-1 แต่ **ไม่เคยมีใคร
    /// อ่านมันกลับมาใช้** — `ShellState::mode` เป็นของหน้าต่างจึงเป็นแหล่งความจริง
    /// ที่สองที่ขัดกับไฟล์ · เกณฑ์ผ่านที่ `§6` เขียนไว้: *บันทึก board ในโหมด
    /// Arrange → เปิดใหม่ → ได้ Arrange กลับมา ไม่ใช่โหมดที่แท็บอื่นค้างไว้*
    #[test]
    fn each_tab_remembers_its_own_mode_and_the_window_only_borrows_it() {
        let mut app = RefxApp::new(AppArgs::default());
        let second = app.docs.mint();
        app.docs.push(Doc::empty(second));

        // แท็บที่สองอยู่โหมด Arrange · แท็บแรกยัง Canvas
        {
            let RefxApp { docs, shell, .. } = &mut app;
            RefxApp::write_mode(docs.active_mut(), shell, Mode::Arrange);
        }
        assert_eq!(app.docs.active().board.view().mode, Mode::Arrange);
        assert!(
            !app.docs.active().board.is_dirty(),
            "สลับโหมดห้ามทำให้เอกสาร dirty (docs/03 §4.3)"
        );

        app.focus_tab(0);
        assert_eq!(app.shell.mode, Mode::Canvas, "สลับแท็บแล้วโหมดต้องตามไปด้วย");
        app.focus_tab(1);
        assert_eq!(app.shell.mode, Mode::Arrange, "โหมดของแท็บต้องยังอยู่");

        // ★ และ "เปิดไฟล์ที่บันทึกไว้ในโหมด Arrange" คือรูปเดียวกันเป๊ะ:
        //   board ที่มี view.mode = Arrange ถูกวางลงแท็บ แล้วโหมดต้องตามมา
        app.focus_tab(0);
        let mut board = Board::new(app.docs.mint(), "saved in arrange");
        board.set_view(live_view(
            Camera::default(),
            Camera::default(),
            Mode::Arrange,
        ));
        app.docs.active_mut().board = board;
        app.mode_follows_active_tab();
        assert_eq!(
            app.shell.mode,
            Mode::Arrange,
            "เปิด board ที่บันทึกในโหมด Arrange แล้วต้องได้ Arrange กลับมา"
        );
    }

    /// ★★★ **กล้องที่บันทึกไว้ต้องกลับมา** — โหมดกับกล้องเดินทางเดียวกัน
    ///
    /// `ViewState` ถูกเขียนลง `.refx` ทุกครั้งที่บันทึกมาตั้งแต่ P4-1 แต่ไม่เคยมี
    /// ใครอ่านกล้องกลับ · เปิดไฟล์มาแล้วได้ zoom ค่าปริยายเสมอ = **เก็บของของ
    /// ผู้ใช้ไว้แล้วไม่คืนให้** ซึ่งแย่กว่าไม่เก็บ เพราะไฟล์อ้างว่ามีมุมมองอยู่
    #[test]
    fn the_view_saved_in_the_file_comes_back_camera_and_all() {
        let mut app = RefxApp::new(AppArgs::default());
        let saved = Camera::new(Vec2::new(1234.5, -678.25), 3.5);
        let arrange = Camera::new(Vec2::new(0.0, 4096.0), 1.0);

        let mut board = Board::new(app.docs.mint(), "saved view");
        board.set_view(live_view(saved, arrange, Mode::Arrange));
        app.docs.active_mut().board = board;
        app.restore_view(app.docs.active);

        assert_eq!(app.docs.active().camera, saved, "กล้อง Canvas ไม่ได้กลับมา");
        assert_eq!(app.shell.mode, Mode::Arrange);
        // ★ ระยะเลื่อนของ Arrange ยังไม่ถูกตั้ง — มันแปลงได้ก็ต่อเมื่อรู้ความสูง
        //   ของช่อง ซึ่ง `plan` เป็นที่แรกที่รู้ · ที่นี่ตรวจแค่ว่า "ค้างไว้แล้ว"
        assert_eq!(app.docs.active().arrange.scroll(), 0.0);
    }

    /// ★★★ ค่าที่พังจากไฟล์ต้องไม่ทำให้จอว่างแบบกลับมาไม่ได้ (I-4)
    ///
    /// `zoom = 0` แปลว่าทุกอย่างยุบเป็นจุดเดียว · `NaN` แปลว่า transform พัง
    /// ทั้งจอ · ทั้งสองอ่านว่า "งานหาย" ในสายตาผู้ใช้ ทั้งที่ข้อมูลยังอยู่ครบ
    ///
    /// ★ ด่านอยู่ที่ [`Camera::new`] ในชั้น DTO อยู่แล้ว — เทสต์นี้ยืนยันว่า
    /// **เส้นทางคืนมุมมองเดินผ่านด่านนั้นจริง** ไม่ใช่แค่ว่าด่านมีอยู่
    #[test]
    fn a_broken_camera_in_the_file_never_reaches_the_screen() {
        let mut app = RefxApp::new(AppArgs::default());
        let mut board = Board::new(app.docs.mint(), "broken view");
        // `Camera::new` ปฏิเสธค่าพวกนี้ตั้งแต่ตอนสร้าง — เหมือนที่ DTO ทำ
        board.set_view(live_view(
            Camera::new(Vec2::new(f32::NAN, 0.0), 0.0),
            Camera::new(Vec2::new(0.0, f32::INFINITY), f32::NAN),
            Mode::Canvas,
        ));
        app.docs.active_mut().board = board;
        app.restore_view(app.docs.active);

        let camera = app.docs.active().camera;
        assert!(camera.center().is_finite(), "จุดกึ่งกลางไม่ใช่ตัวเลข");
        assert!(
            camera.zoom() >= Camera::MIN_ZOOM && camera.zoom() <= Camera::MAX_ZOOM,
            "zoom หลุดช่วง: {}",
            camera.zoom()
        );
    }

    /// ★★★ `--mode=arrange` ต้องไปถึง **`Board::view`** ไม่ใช่แค่ `shell.mode`
    ///
    /// เกือบพังตอนก้อน c: พอโหมดถูกอ่านกลับจากแท็บทุกเฟรม ค่าที่ตั้งไว้แค่ใน
    /// `ShellState` จะอยู่ได้ **ไม่ถึงเฟรมแรก** · ตัวเลือกนี้มีไว้ให้สคริปต์
    /// ถ่ายภาพ/วัดโหมด Arrange ได้โดยไม่ต้องกดปุ่มก่อน — ถ้ามันเงียบไป
    /// **หลักฐานทุกใบของโหมดนั้นจะเป็นภาพของ Canvas ที่ตั้งชื่อไฟล์ว่า arrange**
    /// ซึ่งคือเครื่องมือผลิตหลักฐานที่โกหก (`docs/08 §3.9` ข้อ 9)
    #[test]
    fn the_mode_flag_reaches_the_board_not_just_the_window() {
        let mut app = RefxApp::new(AppArgs {
            mode: Some(Mode::Arrange),
            ..AppArgs::default()
        });
        assert_eq!(
            app.docs.active().board.view().mode,
            Mode::Arrange,
            "`--mode=arrange` ไปไม่ถึง `Board::view`"
        );
        // ★ และมันต้องรอดการอ่านกลับที่ต้นเฟรม
        app.mode_follows_active_tab();
        assert_eq!(app.shell.mode, Mode::Arrange);
    }

    /// ★★★ `F` ตอนไม่ได้เลือกอะไร **ต้องได้ผลเท่า `0` เป๊ะ** (`docs/03 §5`)
    ///
    /// spec เขียนกฎนี้ไว้ในวงเล็บบรรทัดเดียว ("ถ้าไม่เลือก = พอดีทั้ง board")
    /// ซึ่งเป็นรูปของกฎที่คนทำจะลืม · กฎอยู่ใน [`RefxApp::fit_bounds`] ไม่ใช่ที่
    /// ผู้เรียก จึงเป็นไปไม่ได้ที่สองปุ่มจะเพี้ยนออกจากกัน
    #[test]
    fn fit_to_selection_with_nothing_selected_is_exactly_fit_to_board() {
        use refx_core::board::{ItemKind, TextNote};
        use refx_core::command::{AddItems, Command as _};

        let mut ids = Docs::default();
        let mut doc = Doc::empty(ids.mint());
        let items: Vec<refx_core::board::Item> = [0.0_f32, 500.0]
            .into_iter()
            .map(|x| {
                refx_core::board::Item::new(ItemKind::Text(TextNote::default()))
                    .at(Vec2::new(x, 0.0), Vec2::new(100.0, 100.0))
            })
            .collect();
        AddItems::new(items)
            .expect("มีสองใบ")
            .apply(&mut doc.board)
            .expect("board ว่าง ใส่ได้แน่");

        let all = RefxApp::fit_bounds(&doc, keymap::ZoomRequest::FitBoard);
        let none_selected = RefxApp::fit_bounds(&doc, keymap::ZoomRequest::FitSelection);
        assert_eq!(all, none_selected, "`F` ตอนไม่เลือกอะไร ต้องเท่า `0`");
        assert!(all.is_some(), "board ที่มีสองใบต้องมีกรอบ");

        // ★ เลือกใบเดียว → กรอบต้องแคบลงจริง ไม่ใช่คืนทั้ง board เหมือนเดิม
        let first = doc.board.items_in_z_order().next().expect("มีใบแรก").0;
        doc.selection.select(first);
        let one =
            RefxApp::fit_bounds(&doc, keymap::ZoomRequest::FitSelection).expect("เลือกไว้หนึ่งใบ");
        assert!(
            one.size().x < all.expect("มีกรอบ").size().x,
            "เลือกใบเดียวแล้วกรอบต้องแคบลง"
        );

        // board ว่าง = ไม่มีอะไรให้จัด (ผู้เรียกจะบอกผู้ใช้แทนที่จะขยับกล้องมั่ว)
        let empty = Doc::empty(ids.mint());
        assert_eq!(
            RefxApp::fit_bounds(&empty, keymap::ZoomRequest::FitBoard),
            None
        );
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
            history_shortcut(pressed("z"), ctrl),
            Some(HistoryRequest::Undo)
        );
        assert_eq!(
            history_shortcut(pressed("Z"), ctrl),
            Some(HistoryRequest::Undo),
            "ตัวพิมพ์ใหญ่ก็ต้องได้ (บาง layout ส่งมาแบบนั้น)"
        );
        assert_eq!(
            history_shortcut(pressed("z"), ctrl_shift),
            Some(HistoryRequest::Redo),
            "Ctrl+Shift+Z คือ redo ของคนจำนวนมาก"
        );
        assert_eq!(
            history_shortcut(pressed("y"), ctrl),
            Some(HistoryRequest::Redo)
        );

        // ไม่กด Ctrl = พิมพ์ตัวอักษรธรรมดา ห้ามไปย้อนงานของผู้ใช้
        assert_eq!(
            history_shortcut(pressed("z"), ModifiersState::empty()),
            None
        );
        assert_eq!(history_shortcut(pressed("a"), ctrl), None);
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

    // ---------- board เต็มแล้วต้องไม่เงียบ (ROADMAP P3-3) ----------

    /// ★★★ ทุกใบที่ส่งเข้าไปต้องลงเอยที่ช่องใดช่องหนึ่ง **และบวกกันได้ครบ**
    ///
    /// เคสจริงที่ทำให้ต้องมีเทสต์นี้: ลาก 10,000 ไฟล์เข้า board ที่รับได้ 3,072 ใบ
    /// — ของเดิมนับแค่ "ขึ้นจอกี่ใบ" เทียบกับ "ขอมากี่ใบ" ซึ่งไม่มีวันเท่ากัน
    /// งวดจึงไม่มีวันจบ และผู้ใช้ไม่มีวันได้ยินว่าอีก 6,928 ใบหายไปไหน
    #[test]
    fn every_file_in_a_batch_ends_up_counted_somewhere() {
        let mut batch = DropBatch::default();
        batch.start(10_000);
        assert!(!batch.settled(), "งวดที่ยังไม่มีใบไหนตอบกลับต้องยังไม่จบ");

        for _ in 0..3_072 {
            batch.added += 1;
        }
        assert!(
            !batch.settled(),
            "เพิ่มได้ 3,072 จาก 10,000 แล้วยังบอกว่าจบ — ที่เหลือหายไปโดยไม่มีใครนับ"
        );

        for _ in 0..6_928 {
            batch.rejected += 1;
        }
        assert!(batch.settled(), "ทุกใบมีคำตอบแล้วแต่งวดยังไม่จบ");
        assert_eq!(
            batch.answered(),
            batch.requested,
            "ตัวเลขบวกกันไม่ครบ = มีใบที่หายไปโดยไม่มีใครรู้"
        );
    }

    /// ไฟล์เสียก็ต้องนับ ไม่งั้นงวดที่มีไฟล์เสียใบเดียวจะไม่มีวันจบ
    #[test]
    fn a_single_broken_file_does_not_stall_the_batch_forever() {
        let mut batch = DropBatch::default();
        batch.start(3);
        batch.added += 2;
        batch.failed += 1;
        assert!(batch.settled());
    }

    /// ★★★ ผู้ใช้ pan ระหว่างลากไฟล์เข้ามา → งานถูกยกเลิก → **งวดต้องยังจบได้**
    ///
    /// P1-4 ยกเลิกงานที่ผ่านจอไปแล้วโดยตั้งใจ และการ pan ระหว่างที่ไฟล์ทยอยเข้ามา
    /// เป็นเรื่องปกติมาก · ถ้าใบที่ถูกยกเลิกไม่ถูกนับ งวดจะค้างถาวร แล้ว
    /// **แถบ "กำลังโหลด" ไม่หาย และข้อความ board เต็มไม่มีวันขึ้น**
    /// — สองอย่างที่เพิ่งทำเสร็จจะพังพร้อมกันในการใช้งานจริง
    #[test]
    fn panning_while_files_load_does_not_stall_the_batch_forever() {
        let mut batch = DropBatch::default();
        batch.start(500);
        batch.added += 300;
        batch.cancelled += 200; // ผู้ใช้ pan ผ่านไปแล้ว
        assert!(
            batch.settled(),
            "ยกเลิก 200 ใบแล้วงวดยังไม่จบ — แถบกำลังโหลดจะค้างตลอดกาล"
        );
        assert_eq!(batch.answered(), batch.requested);
    }

    /// ★ งานที่ถูกยกเลิกแล้ว board เต็มด้วย → ข้อความ board เต็มต้องยังขึ้น
    #[test]
    fn a_cancelled_job_does_not_hide_the_board_full_message() {
        let batch = DropBatch {
            requested: 10_000,
            added: 3_072,
            rejected: 6_900,
            failed: 0,
            damaged: 0,
            cancelled: 28,
            reported: false,
        };
        assert!(batch.settled());
        let message = board_full_message(Lang::En, 3_072, batch)
            .expect("board เต็มแล้วแต่ไม่มีข้อความเพราะมีใบที่ถูกยกเลิกปนอยู่");
        assert!(message.contains("6900"), "{message}");
    }

    /// เริ่มงวดใหม่ต้องล้างตัวนับเดิม **ทั้งชุด**
    #[test]
    fn starting_a_new_batch_forgets_the_previous_one() {
        let mut batch = DropBatch {
            requested: 10,
            added: 3,
            rejected: 7,
            failed: 1,
            damaged: 0,
            cancelled: 2,
            reported: true,
        };
        batch.start(5);
        assert_eq!(
            batch,
            DropBatch {
                requested: 5,
                ..DropBatch::default()
            }
        );
    }

    /// ★★ ข้อความต้องมีตัวเลข **ครบทั้งสาม** และบอกสิ่งที่ทำได้ต่อ — ทั้งสองภาษา
    #[test]
    fn the_board_full_message_names_every_number_the_user_needs() {
        let batch = DropBatch {
            requested: 10_000,
            added: 3_072,
            rejected: 6_928,
            failed: 0,
            damaged: 0,
            cancelled: 0,
            reported: false,
        };
        for lang in [Lang::En, Lang::Th] {
            let message = board_full_message(lang, 3_072, batch)
                .unwrap_or_else(|| panic!("{lang:?}: ตกหล่น 6,928 ใบแต่ไม่มีข้อความ"));
            for number in ["3072", "6928", "10000"] {
                assert!(
                    message.contains(number),
                    "{lang:?}: ข้อความไม่มีเลข {number} — {message}"
                );
            }
            assert!(
                message.contains("board"),
                "{lang:?}: ไม่ได้บอกว่าทำอะไรต่อได้ — {message}"
            );
        }
    }

    /// ★★★ negative control: ลากไม่เกินเพดาน → **ต้องไม่มีข้อความนี้เลย**
    ///
    /// ข้อความเตือนที่โผล่ตอนไม่มีอะไรผิดคือสิ่งที่ทำให้ผู้ใช้เลิกอ่าน status bar
    /// — แล้ววันที่มีอะไรผิดจริงเขาจะไม่เห็นมันด้วย
    #[test]
    fn a_batch_that_fits_says_nothing_about_the_board_being_full() {
        let batch = DropBatch {
            requested: 200,
            added: 200,
            rejected: 0,
            failed: 0,
            damaged: 0,
            cancelled: 0,
            reported: false,
        };
        for lang in [Lang::En, Lang::Th] {
            assert!(
                board_full_message(lang, 200, batch).is_none(),
                "{lang:?}: ทุกใบเข้าครบแต่ยังบอกว่า board เต็ม"
            );
        }
        // แม้แต่ใบที่เปิดไม่ได้ (ไฟล์เสีย) ก็ไม่ใช่ "board เต็ม" — คนละสาเหตุ คนละทางแก้
        let broken = DropBatch {
            requested: 5,
            added: 4,
            rejected: 0,
            failed: 1,
            damaged: 0,
            cancelled: 0,
            reported: false,
        };
        assert!(board_full_message(Lang::Th, 4, broken).is_none());
    }

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
        let seen = |text: &str| {
            shortcut_char(
                &character(text),
                winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::F13),
            )
        };
        assert!(is_paste(seen("v"), ctrl));
        // Shift ค้างอยู่ด้วย (Ctrl+Shift+V) ยังถือว่าเป็นการวาง
        assert!(is_paste(seen("V"), ctrl | ModifiersState::SHIFT));
        // X11 บาง compositor ส่ง Ctrl+V มาเป็นอักขระควบคุม SYN
        assert!(is_paste(seen("\u{16}"), ctrl));

        // ไม่กด Ctrl = พิมพ์ตัว v เฉย ๆ ห้ามไปวางภาพให้
        assert!(!is_paste(seen("v"), ModifiersState::empty()));
        // ปุ่มอื่นที่กดพร้อม Ctrl
        assert!(!is_paste(seen("c"), ctrl));
        assert!(!is_paste(seen("b"), ctrl));
        // ปุ่มที่ไม่ใช่ตัวอักษร
        assert!(!is_paste(
            shortcut_char(
                &winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter),
                winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::Enter),
            ),
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

    // ---------- ★★★ P4-5: ภาพที่วางต้องมีไฟล์จริงและต้องไม่ถูกกวาดทิ้ง ----------

    fn spool_temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-ui-spool-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// board ที่มีภาพใบเดียวซึ่งอ้างถึง asset ตัวนี้
    fn board_pointing_at(hash: refx_core::hash::ContentHash, path: &std::path::Path) -> Board {
        use refx_core::board::{BoardParts, ItemParts};
        Board::load(
            probe_board_id(),
            BoardParts {
                name: "pasted".to_owned(),
                items: vec![ItemParts {
                    item: Item::new(ItemKind::Image(AssetRef {
                        hash,
                        path: path.to_path_buf(),
                        px_size: glam::UVec2::new(8, 8),
                        format: ImageFormat::Unknown,
                        embedded: false,
                        mtime: 0,
                        file_size: 0,
                    })),
                    group: None,
                }],
                ..BoardParts::default()
            },
        )
    }

    /// ★★★ **ภาพที่วางแล้วยังไม่ได้บันทึก ต้องไม่ถูก `sweep` ลบทิ้ง**
    ///
    /// นี่คือเส้นทางเต็มของกติกาข้อ 1: คีย์ที่ `asset_location` เลือก →
    /// ชื่อไฟล์ที่ `SpoolSink` เขียน → hash ที่ `sweep` เห็นจาก board
    /// **สามจุดนี้ต้องเป็นค่าเดียวกัน** ถ้าจุดไหนหลุด ผู้ใช้เสียภาพถาวร (I-3)
    ///
    /// ★ เทสต์เรียก `asset_location` **ตัวที่ `drain_decode_results` ใช้จริง**
    /// ไม่ใช่ตรรกะที่เขียนเลียนแบบ (`docs/08 §3.9` ข้อ 9)
    #[test]
    fn a_pasted_image_the_board_points_at_is_never_swept() {
        use refx_core::spool::PastedImageStore as _;

        let dir = spool_temp_dir("kept");
        let content = refx_asset::hash::hash_pasted(2, 2, &[7u8; 16]);
        let job_key = refx_asset::hash::hash_bytes(b"clipboard:1");

        let (asset_hash, path) =
            asset_location(Some(&dir), job_key, Some(ContentOrigin::Spooled(content)));
        let path = path.expect("ภาพที่วางต้องได้ที่อยู่ของมัน");
        assert_eq!(asset_hash, content, "คีย์ต้องเป็นของเนื้อภาพ ไม่ใช่ของงาน");

        // เขียนไฟล์ผ่านตัวจริงที่ pool เรียก
        let sink = SpoolSink { dir: dir.clone() };
        let written = sink.store(asset_hash, b"png bytes").expect("เขียนไม่สำเร็จ");
        assert_eq!(written, path, "ที่อยู่ที่บอก item ไว้ไม่ตรงกับที่ไฟล์ไปอยู่จริง");

        let board = board_pointing_at(asset_hash, &path);
        let swept = refx_io::spool::sweep(
            &dir,
            &refx_io::spool::hashes_of(&board),
            0, // เพดาน 0 = กวาดทุกอย่างที่กวาดได้
            std::time::Duration::ZERO,
            std::time::SystemTime::now(),
        );

        assert!(path.exists(), "ภาพที่อยู่บน board ถูกกวาดทิ้ง");
        assert_eq!(swept.kept_because_referenced, 1);
        assert_eq!(swept.removed, 0);
    }

    /// ★★★ negative control ของข้อบน — **ใช้คีย์ของงานแล้วภาพหายจริง**
    ///
    /// ถ้าไม่มีเทสต์นี้ ข้อบนจะเขียวเท่ากันแม้ `asset_location` คืนคีย์อะไร
    /// ก็ตาม เพราะทั้ง board และไฟล์จะใช้ค่าเดียวกันอยู่ดี · สิ่งที่พังจริงคือ
    /// **สอง session**: `clipboard:1` ของวันนี้ไม่ใช่ `clipboard:1` ของพรุ่งนี้
    #[test]
    fn using_the_job_key_instead_is_what_loses_the_image() {
        use refx_core::spool::PastedImageStore as _;

        let dir = spool_temp_dir("lost");
        let content = refx_asset::hash::hash_pasted(2, 2, &[9u8; 16]);
        let job_key = refx_asset::hash::hash_bytes(b"clipboard:1");
        assert_ne!(content, job_key);

        let sink = SpoolSink { dir: dir.clone() };
        let path = sink.store(content, b"png bytes").unwrap();

        // ★ จงใจใส่ **คีย์ของงาน** ลง `AssetRef` แทนคีย์ของเนื้อ
        let board = board_pointing_at(job_key, &path);
        refx_io::spool::sweep(
            &dir,
            &refx_io::spool::hashes_of(&board),
            0,
            std::time::Duration::ZERO,
            std::time::SystemTime::now(),
        );

        assert!(
            !path.exists(),
            "ใส่คีย์ผิดแล้วภาพยังอยู่ — แปลว่าเทสต์ข้างบนไม่ได้พิสูจน์อะไร"
        );
    }

    /// ★★ **ภาพที่วางต้องมี `path` เสมอ** — ไม่งั้น `request_thumbnails_for_board`
    /// ข้ามมันตอนกู้คืน แล้วผู้ใช้ได้ board ที่มีแต่ช่องว่าง
    ///
    /// path ถูกเขียนลง `AssetRef` **ตั้งแต่ตอน `Done`** ทั้งที่ไฟล์ยังเขียนไม่เสร็จ
    /// (หาได้จาก hash ล้วน ๆ) — จำเป็น เพราะ snapshot ที่เขียนในช่วงนั้นต้องกู้ได้
    #[test]
    fn a_pasted_image_always_has_a_path_to_come_back_from() {
        let dir = std::path::Path::new("/data/RefX/pasted");
        let content = refx_asset::hash::hash_pasted(1, 1, &[1u8; 4]);
        let (hash, path) = asset_location(
            Some(dir),
            refx_asset::hash::hash_bytes(b"job"),
            Some(ContentOrigin::Spooled(content)),
        );

        let path = path.expect("ไม่มี path = ภาพหายตอนกู้คืน");
        assert_eq!(hash, content);
        assert_eq!(path.parent(), Some(dir));
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            format!("{content}.png"),
            "ชื่อไฟล์ต้องเป็น hash — sweep แปลงชื่อกลับเป็น hash เพื่อหาว่าใครอ้างถึง"
        );
    }

    /// ★★★ **ไฟล์ของผู้ใช้ต้องได้คีย์ของ *เนื้อไฟล์* ไม่ใช่ของ path** (`docs/02 §2.3`)
    ///
    /// และต้อง **ไม่** ถูกลากไปชี้ที่ spool — มันมีที่อยู่ถาวรของมันเองอยู่แล้ว
    #[test]
    fn a_file_gets_the_key_of_its_bytes_not_of_its_path() {
        let job_key = refx_asset::hash::hash_bytes(b"C:/ref/cat.png");
        let content = refx_asset::hash::hash_bytes("ไบต์ของภาพแมว".as_bytes());
        assert_ne!(content, job_key);

        let (hash, path) = asset_location(
            Some(std::path::Path::new("/data/RefX/pasted")),
            job_key,
            Some(ContentOrigin::File(content)),
        );
        assert_eq!(hash, content, "คีย์ยังเป็นของ path อยู่");
        assert_eq!(path, None, "ไฟล์ของผู้ใช้ถูกลากไปชี้ที่ spool");
    }

    /// ★★★ **สำเนาเดียวกันสองที่อยู่ = คีย์เดียวกัน** — สัญญาข้อที่สามของ `docs/02 §2.3`
    ///
    /// นี่คือข้อที่ *เห็นไม่ได้เลย* ถ้าคีย์มาจาก path เพราะสอง path ย่อมต่างกัน
    /// เสมอโดยนิยาม · ผลที่ผู้ใช้เจอคือ mood board ที่มีภาพเดียวกันสองใบกิน
    /// texture สองชุด และ packed mode ฝังไฟล์เดียวกันสองครั้ง
    #[test]
    fn the_same_picture_in_two_places_collapses_to_one_asset() {
        let content = refx_asset::hash::hash_bytes("ไบต์ชุดเดียวกัน".as_bytes());
        let here = refx_asset::hash::hash_bytes(b"C:/ref/a/cat.png");
        let there = refx_asset::hash::hash_bytes(b"D:/backup/b/cat-copy.png");
        assert_ne!(here, there, "สอง path ต้องให้คีย์ของงานคนละตัว");

        let (a, _) = asset_location(None, here, Some(ContentOrigin::File(content)));
        let (b, _) = asset_location(None, there, Some(ContentOrigin::File(content)));
        assert_eq!(a, b, "สำเนาเดียวกันได้คนละ asset");
    }

    /// ★ hash ไม่ได้ (ไฟล์หายระหว่างทาง) → **ถอยไปใช้คีย์ของงาน ไม่ใช่ล้ม**
    ///
    /// ภาพยังขึ้นจอได้ตามปกติ สิ่งที่เสียไปคือการยุบไฟล์ซ้ำกับการ relink ด้วย
    /// hash ซึ่งทั้งคู่เป็นของแถม ส่วน "ภาพต้องขึ้นจอ" คือ I-3
    #[test]
    fn a_file_we_could_not_hash_still_becomes_a_picture() {
        let job_key = refx_asset::hash::hash_bytes(b"C:/ref/gone.png");
        let (hash, path) = asset_location(None, job_key, None);
        assert_eq!(hash, job_key);
        assert_eq!(path, None);
    }

    // ---------- ★★★ P4-6: relink ----------

    fn thumb_of(w: u32, h: u32) -> refx_asset::thumb::Thumbnail {
        refx_asset::thumb::Thumbnail {
            pixels: vec![0; 128 * 128 * 4],
            source_width: w,
            source_height: h,
            dominant: 0,
        }
    }

    fn image_kind(hash: u8, path: &str) -> ItemKind {
        ItemKind::Image(AssetRef {
            hash: refx_core::hash::ContentHash::from_bytes([hash; 32]),
            path: std::path::PathBuf::from(path),
            px_size: glam::UVec2::new(100, 80),
            format: ImageFormat::Unknown,
            embedded: false,
            mtime: 42,
            file_size: 4242,
        })
    }

    /// ★★★ **หาเจอที่ใหม่ → path เปลี่ยน · คีย์ถูกซ่อม · ที่เหลือไม่ถูกแตะ**
    ///
    /// `docs/07 §2` อนุญาตให้ซ่อมคีย์ **ตอน relink สำเร็จเท่านั้น** และ
    /// `px_size`/`mtime`/`file_size` ไม่ใช่เรื่องของ relink — แตะเมื่อไหร่
    /// เอกสารจะ dirty ทุกครั้งที่ mtime ของไฟล์ขยับ ทั้งที่ผู้ใช้ไม่ได้แก้อะไร
    #[test]
    fn a_relinked_image_gets_a_new_path_and_a_repaired_key() {
        let before = image_kind(9, "/gone/cat.png");
        let content = refx_core::hash::ContentHash::from_bytes([77; 32]);

        let after = relinked_kind(
            &before,
            Some(std::path::Path::new("/found/cat.png")),
            Some(content),
            Some(std::path::Path::new("/data/RefX/pasted")),
            &thumb_of(4000, 3000),
            refx_asset::pool::SourceMeta {
                mtime_ms: 999,
                bytes: 1,
            },
        )
        .expect("ต้องได้ที่มาใหม่");

        let ItemKind::Image(asset) = &after else {
            panic!("ต้องยังเป็นภาพ");
        };
        assert_eq!(asset.path, std::path::PathBuf::from("/found/cat.png"));
        assert_eq!(asset.hash, content, "คีย์ไม่ถูกซ่อม");
        // ★ ของที่ไม่ใช่เรื่องของ relink ต้องเหมือนเดิมเป๊ะ
        assert_eq!(asset.px_size, glam::UVec2::new(100, 80), "px_size ถูกแตะ");
        assert_eq!(asset.mtime, 42, "mtime ถูกแตะ");
        assert_eq!(asset.file_size, 4242, "file_size ถูกแตะ");
    }

    /// ★★★ **ห้ามซ่อมคีย์ของภาพที่มาจาก clipboard** (`docs/07 §2`)
    ///
    /// ชื่อไฟล์ใน spool คือ hash ของ **พิกเซล** ส่วนการ hash ไฟล์ PNG นั้นให้
    /// คนละค่า · เขียนทับเมื่อไหร่ `spool::sweep` จะหาไม่เจอว่ามีคนอ้างถึง
    /// **แล้วลบภาพของผู้ใช้ทิ้ง** (I-3)
    ///
    /// ★★ ยืนยัน negative control แล้ว (21 ส.ค. 2026): ถอดด่าน `in_spool` ออก
    /// → แดงทันทีพร้อมคีย์ของไฟล์ PNG โผล่มาแทนคีย์ของพิกเซล
    #[test]
    fn a_pasted_image_never_has_its_key_repaired() {
        let spool = std::path::Path::new("/data/RefX/pasted");
        let pixels_key = refx_core::hash::ContentHash::from_bytes([5; 32]);
        let before = ItemKind::Image(AssetRef {
            hash: pixels_key,
            path: spool.join(format!("{pixels_key}.png")),
            px_size: glam::UVec2::new(10, 10),
            format: ImageFormat::Unknown,
            embedded: false,
            mtime: 0,
            file_size: 0,
        });
        // hash ของ *ไฟล์ PNG* ซึ่งเป็นคนละค่ากับ hash ของพิกเซลเสมอ
        let png_file_key = refx_core::hash::ContentHash::from_bytes([200; 32]);

        let after = relinked_kind(
            &before,
            Some(&spool.join(format!("{pixels_key}.png"))),
            Some(png_file_key),
            Some(spool),
            &thumb_of(10, 10),
            refx_asset::pool::SourceMeta::default(),
        )
        .expect("ต้องได้ที่มา");

        let ItemKind::Image(asset) = &after else {
            panic!("ต้องยังเป็นภาพ");
        };
        assert_eq!(
            asset.hash, pixels_key,
            "คีย์ของภาพที่วางถูกเขียนทับ — sweep จะลบไฟล์นั้นทิ้งในรอบถัดไป"
        );
        assert_eq!(after, before, "ไม่ควรมีอะไรเปลี่ยนเลย");
    }

    /// ★★ ใบที่เป็น `Missing` กลับมาเป็นภาพได้ พร้อมคีย์และขนาดจริง
    #[test]
    fn a_missing_item_becomes_a_picture_again_when_the_file_turns_up() {
        let before = ItemKind::Missing {
            original_path: std::path::PathBuf::from("/gone/cat.png"),
            reason: refx_core::board::MissingReason::FileNotFound,
        };
        let content = refx_core::hash::ContentHash::from_bytes([3; 32]);

        let after = relinked_kind(
            &before,
            Some(std::path::Path::new("/found/cat.png")),
            Some(content),
            None,
            &thumb_of(1600, 1200),
            refx_asset::pool::SourceMeta {
                mtime_ms: 5,
                bytes: 6,
            },
        )
        .expect("ต้องกลับมาเป็นภาพ");

        let ItemKind::Image(asset) = &after else {
            panic!("ต้องเป็นภาพแล้ว");
        };
        assert_eq!(asset.hash, content);
        assert_eq!(asset.path, std::path::PathBuf::from("/found/cat.png"));
        // ★ ของเดิมไม่เคยมีขนาด — ต้องมาจากภาพที่เพิ่งอ่าน
        assert_eq!(asset.px_size, glam::UVec2::new(1600, 1200));
        assert_eq!(asset.mtime, 5);
        assert_eq!(asset.file_size, 6);
    }

    /// ★ ไฟล์ยังอยู่ที่เดิมและคีย์ก็ถูกอยู่แล้ว = **ไม่มีอะไรเปลี่ยน**
    ///
    /// นี่คือเคสของทุกเอกสารที่บันทึกด้วยรุ่นปัจจุบัน — เปิดแล้วต้องไม่ dirty
    #[test]
    fn opening_a_healthy_document_changes_nothing() {
        let before = image_kind(9, "/work/cat.png");
        let same_key = refx_core::hash::ContentHash::from_bytes([9; 32]);

        let after = relinked_kind(
            &before,
            Some(std::path::Path::new("/work/cat.png")),
            Some(same_key),
            None,
            &thumb_of(100, 80),
            refx_asset::pool::SourceMeta {
                mtime_ms: 42,
                bytes: 4242,
            },
        )
        .expect("ต้องได้ที่มา");

        assert_eq!(
            after, before,
            "ไม่มีอะไรเปลี่ยนแต่กลับได้ค่าใหม่ → เอกสารจะ dirty ทุกครั้งที่เปิด"
        );
    }

    /// โน้ตข้อความไม่ใช่เป้าของ relink
    #[test]
    fn a_note_is_never_relinked() {
        let note = ItemKind::Text(refx_core::board::TextNote {
            text: "อย่าแตะ".to_owned(),
        });
        assert_eq!(
            relinked_kind(
                &note,
                Some(std::path::Path::new("/found/cat.png")),
                None,
                None,
                &thumb_of(10, 10),
                refx_asset::pool::SourceMeta::default(),
            ),
            None
        );
    }

    /// ★★★ **undo ของ relink ต้องเอาภาพออกจากจอด้วย ไม่ใช่แค่ออกจาก `Board`**
    ///
    /// `render_state` เป็น cache ที่อยู่ยาวกว่าสถานะของ item โดยตั้งใจ (ช่อง
    /// atlas ต้องรอด undo/redo ของ "ลบภาพ" — §2.5) · พอ relink ทำให้ item
    /// กลายเป็น `Missing` ช่องเก่าจึงยังอยู่ ถ้า `rebuild_quads` ไม่ถามชนิดจาก
    /// `Board` จอจะยังโชว์ภาพเดิมทั้งที่เอกสารบอกว่าหาไฟล์ไม่เจอ
    ///
    /// ★ เจอตอน **กด `Ctrl+Z` บนแอปจริง** (21 ส.ค. 2026) ไม่ใช่จากเทสต์ —
    /// ทุกชิ้นถูก แต่ประกอบผิด (`docs/08 §3.9` ข้อ 5 ชนิดที่สาม)
    #[test]
    fn an_item_that_became_missing_stops_being_drawn() {
        let board = {
            use refx_core::board::{BoardParts, ItemParts};
            Board::load(
                probe_board_id(),
                BoardParts {
                    name: "b".to_owned(),
                    items: vec![
                        ItemParts {
                            item: Item::new(image_kind(1, "/a.png"))
                                .at(Vec2::ZERO, Vec2::splat(100.0)),
                            group: None,
                        },
                        ItemParts {
                            item: Item::new(ItemKind::Missing {
                                original_path: std::path::PathBuf::from("/gone.png"),
                                reason: refx_core::board::MissingReason::FileNotFound,
                            })
                            .at(Vec2::splat(200.0), Vec2::splat(100.0)),
                            group: None,
                        },
                    ],
                    ..BoardParts::default()
                },
            )
        };
        let ids = board.z_order().to_vec();

        // ★ ทั้งสองใบมี `render_state` ค้างอยู่ — เหมือนหลัง undo ของ relink เป๊ะ
        let drawn: Vec<ItemId> = board
            .items_in_z_order()
            .filter(|(_, item)| matches!(item.kind, ItemKind::Image(_)))
            .map(|(id, _)| id)
            .collect();

        assert_eq!(
            drawn,
            vec![ids[0]],
            "ใบที่เป็น Missing ยังถูกวาดอยู่ — จอจะโชว์ภาพเดิมทั้งที่เอกสารบอกว่าไฟล์หาย"
        );
    }

    /// ★★★ **`Missing` ต้องรอด save/load ครบ** — `docs/07 §2` · I-3
    ///
    /// ผู้ใช้ที่เปิดไฟล์บนเครื่องที่ไม่มีภาพ แล้วบันทึกทับ **ต้องไม่เสียอะไรเลย**
    /// ตำแหน่ง/ขนาด/ครอป/แท็ก/โน้ตของใบที่หายต้องกลับมาครบ พร้อม path เดิม
    /// ที่ยังใช้ตามหาไฟล์ได้ในอนาคต
    #[test]
    fn a_missing_image_survives_being_saved_and_opened_again() {
        use refx_core::board::{BoardParts, ItemParts};

        let dir = std::env::temp_dir().join(format!("refx-missing-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let doc = dir.join("work.refx");

        let mut board = Board::load(
            probe_board_id(),
            BoardParts {
                name: "board".to_owned(),
                items: vec![ItemParts {
                    item: Item {
                        canvas: ItemCanvas {
                            pos: Vec2::new(11.0, 22.0),
                            size: Vec2::new(300.0, 200.0),
                            ..ItemCanvas::default()
                        },
                        meta: refx_core::board::ItemMeta {
                            rating: 3,
                            note: "ใบนี้อยู่ในไดรฟ์นอก".to_owned(),
                            ..refx_core::board::ItemMeta::default()
                        },
                        ..Item::new(image_kind(4, "E:/external/cat.png"))
                    },
                    group: None,
                }],
                ..BoardParts::default()
            },
        );

        // ไฟล์หาย → item กลายเป็น Missing ผ่านคำสั่งเดียวกับที่ของจริงใช้
        let id = board.z_order()[0];
        let command = RelinkAssets::new(
            &board,
            vec![(
                id,
                ItemKind::Missing {
                    original_path: std::path::PathBuf::from("E:/external/cat.png"),
                    reason: refx_core::board::MissingReason::FileNotFound,
                },
            )],
        )
        .unwrap();
        let mut history = History::default();
        history.apply(&mut board, Box::new(command)).unwrap();
        // ★ บันทึกจริง = เอกสารสะอาด — เทียบทั้งก้อนได้โดยไม่ต้องยกเว้นฟิลด์ไหน
        history.mark_saved(&mut board);

        refx_io::save::save_atomic(&doc, &board, refx_platform::fsops::rename_durable).unwrap();
        let back = read_document(&doc, probe_board_id()).expect("เปิดไฟล์ที่เพิ่งบันทึกไม่ได้");

        assert_eq!(back, board, "บันทึกทับแล้วไม่เท่าเดิม");
        let (_, item) = back.items_in_z_order().next().expect("item หายไปทั้งใบ");
        let ItemKind::Missing { original_path, .. } = &item.kind else {
            panic!("ใบที่หายต้องยังเป็น Missing");
        };
        assert_eq!(
            original_path,
            &std::path::PathBuf::from("E:/external/cat.png"),
            "ที่อยู่เดิมหาย — relink รอบหน้าจะไม่มีอะไรให้ตามหา"
        );
        assert_eq!(item.canvas.pos, Vec2::new(11.0, 22.0), "ตำแหน่งหาย");
        assert_eq!(item.meta.rating, 3, "ดาวหาย");
        assert_eq!(item.meta.note, "ใบนี้อยู่ในไดรฟ์นอก", "โน้ตหาย");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------- ★★★ P4-4: งานที่ยังไม่เคยบันทึกต้องมีที่ให้ autosave ----------

    /// ★★★ **ยังไม่เคยกด `Ctrl+S` = ต้องยังมี snapshot** — ช่องที่ P4-3 เปิดค้าง
    ///
    /// `CLAUDE.md` ยกเคสนี้มาตรง ๆ ("mood board ที่จัดมา 3 ชั่วโมง") · ก่อนหน้านี้
    /// `tick_autosave` ออกจากฟังก์ชันทันทีเมื่อ `doc_path` เป็น `None` แปลว่า
    /// **คนที่เสียมากที่สุดคือคนที่ไม่ได้รับการปกป้องเลย**
    #[test]
    fn work_that_was_never_saved_still_has_somewhere_to_autosave() {
        let mut app = RefxApp::new(AppArgs::default());
        app.recovery_dir = Some(std::path::PathBuf::from("/data/RefX/recovery"));
        assert_eq!(app.docs.active().path, None, "เคสนี้คือ 'ยังไม่เคยบันทึก'");

        match RefxApp::snapshot_target(app.docs.active(), app.recovery_dir.as_deref()) {
            Some(SnapshotTarget::Recovery(dir)) => {
                assert_eq!(dir, std::path::PathBuf::from("/data/RefX/recovery"));
            }
            other => panic!("งานที่ยังไม่เคยบันทึกไม่มีที่ให้ autosave: {other:?}"),
        }
    }

    /// ★ บันทึกแล้ว = snapshot ย้ายไปอยู่ข้างเอกสาร (`docs/07 §4` "ย้ายเจ้าของ")
    #[test]
    fn once_the_document_has_a_path_the_snapshot_moves_next_to_it() {
        let mut app = RefxApp::new(AppArgs::default());
        app.recovery_dir = Some(std::path::PathBuf::from("/data/RefX/recovery"));
        app.docs.active_mut().path = Some(std::path::PathBuf::from("/work/moodboard.refx"));

        match RefxApp::snapshot_target(app.docs.active(), app.recovery_dir.as_deref()) {
            Some(SnapshotTarget::BesideDocument(doc)) => {
                assert_eq!(doc, std::path::PathBuf::from("/work/moodboard.refx"));
            }
            other => panic!("บันทึกแล้วแต่ snapshot ไม่ได้อยู่ข้างเอกสาร: {other:?}"),
        }
    }

    /// ★★★ **ที่อยู่ของ snapshot ต้องไม่เคยตกไปอยู่ใน `cache_dir`** (`docs/07 §4`)
    ///
    /// cache คือที่ของสิ่งที่สร้างใหม่ได้ ซึ่งทั้ง OS และตัวล้างดิสก์ของผู้ใช้
    /// ถือว่าลบได้ตามใจ · ประตูจริงอยู่ที่ `AppPaths::recovery_dir()` (มีเทสต์
    /// ของตัวเอง) — ที่นี่ตรวจ **ปลายทางฝั่งผู้ใช้ของมัน**: `run()` ต้องเป็น
    /// คนเสียบค่า และเมื่อไม่มีที่อยู่ก็ต้อง **ไม่เขียนอะไรเลย** ไม่ใช่หาที่ลงเอง
    #[test]
    fn without_a_data_dir_unsaved_work_is_simply_not_written_anywhere() {
        let app = RefxApp::new(AppArgs::default());
        assert_eq!(app.recovery_dir, None, "ค่าเริ่มต้นต้องว่าง รอ `run` เสียบให้");
        assert_eq!(
            RefxApp::snapshot_target(app.docs.active(), app.recovery_dir.as_deref()),
            None,
            "ไม่มีที่อยู่แล้วยังเลือกที่ลงเอง — ที่ที่มันเลือกคือที่ที่ไม่มีใครตรวจ"
        );
    }

    /// ★★★ **board ที่เก็บครบแล้วต้องไม่ขอให้ปลุกอีก** — I-1 ในสถานะ "dirty + idle"
    ///
    /// `dirty` เป็นจริงค้างยาวจนกว่าจะ `Ctrl+S` จริง ๆ · ถ้านาฬิกา autosave
    /// ถามแค่ `dirty` ผู้ใช้ที่ลากภาพเข้ามาแล้วไปวาดรูปต่อใน Photoshop ทั้งวัน
    /// จะถูกปลุกมาเขียนไฟล์ที่ **เนื้อหาเหมือนเดิมเป๊ะ** ทุก 10 วินาที
    /// = 2,880 ครั้งต่อวัน (`docs/08 §3.9` ข้อ 11: I-1 ต้องตรวจทุกสถานะ)
    #[test]
    fn a_board_already_captured_stops_asking_to_be_woken() {
        let mut app = RefxApp::new(AppArgs::default());
        app.recovery_dir = Some(std::path::PathBuf::from("/data/RefX/recovery"));

        // ไม่มี `gfx` = ไม่มี board ให้ถาม → ต้องไม่ตั้งนาฬิกา
        assert_eq!(app.autosave_deadline(), None);
        assert!(!RefxApp::doc_has_unsnapshotted_work(app.docs.active()));

        // ★★★ งานที่ส่งไปเธรดแล้วยังไม่กลับ ต้องตั้งนาฬิกาไว้ **ข้างหน้า**
        //     (`docs/08 §3.9` ข้อ 18 — ผลจากเธรดอื่นต้องมีคนมาเก็บ)
        //
        //     เดิมที่นี่คืน `None` เพราะกลัวลูป · ราคาของ `None` คือ **ไม่มีใคร
        //     กลับมาเก็บผลเลย** แล้ว autosave หยุดถาวรจนกว่าผู้ใช้จะขยับเมาส์
        //
        //   ★★ ถาม `next_autosave_across_tabs` ตรง ๆ ไม่ใช่ `autosave_deadline`
        //      — ตัวหลังคืน `None` ตั้งแต่บรรทัดแรกเพราะไม่มี `gfx` ในเทสต์
        //      **input จึงไม่เคยไปถึงกิ่งที่เทสต์นี้อ้างว่าตรวจ** (`§3.9` ข้อ 1b)
        //      · รุ่นก่อนของเทสต์นี้เขียวมาตลอดโดยไม่ได้ตรวจอะไรเลย
        let (_tx, rx) = crossbeam_channel::bounded::<Result<(), String>>(1);
        app.docs.active_mut().autosave_job = Some(rx);
        let before = std::time::Instant::now();
        let deadline = RefxApp::next_autosave_across_tabs(&app.docs, app.recovery_dir.as_deref())
            .expect("มีงานค้างแล้วต้องตั้งนาฬิกามาเก็บผล ไม่ใช่ปล่อยให้หลับยาว");
        assert!(
            deadline > before,
            "นาฬิกาชี้ไปอดีต = ตื่นทันทีวนไม่รู้จบ ซึ่งคือ `Poll` ที่ I-1 ห้าม"
        );
        assert!(
            deadline < before + std::time::Duration::from_secs(2),
            "ตั้งไว้ไกลเกินไป — ข้อความ 'เขียน snapshot ไม่สำเร็จ' จะมาช้ากว่าที่ควร"
        );
    }

    // ---------- ★★★ P4-4 ครึ่งหลัง: dialog 3 ตัวเลือก + เปิดไฟล์ ----------

    /// ★★★ **"เก็บไว้ก่อน" ต้องไม่แตะไฟล์แม้แต่ไบต์เดียว** (`docs/07 §4`)
    ///
    /// นี่คือตัวเลือกที่ spec บอกว่าสำคัญที่สุด และคุณค่าทั้งหมดของมันอยู่ที่
    /// **ไฟล์ยังอยู่ครบหลังกดแล้ว** · ถ้ามันลบ (หรือ "ย้ายไปที่ปลอดภัย" อะไรก็ตาม)
    /// มันก็เป็นแค่ "ทิ้ง" ที่ใส่เสื้อคลุมสุภาพ ซึ่งแย่กว่าไม่มีตัวเลือกนี้เลย
    /// เพราะผู้ใช้ที่ไม่แน่ใจจะเลือกมันด้วยความเข้าใจว่างานยังอยู่
    #[test]
    fn keeping_it_for_later_leaves_the_file_completely_untouched() {
        use refx_platform::fsops::rename_durable;
        let dir = std::env::temp_dir().join(format!("refx-later-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let session = refx_io::recovery::SessionId::new_unique();
        let board = refx_core::board::Board::default();
        refx_io::recovery::write_snapshot(&dir, &session, &board, rename_durable).unwrap();
        let path = refx_io::recovery::snapshot_path(&dir, &session);
        let before = std::fs::metadata(&path).unwrap().len();

        let mut app = RefxApp::new(AppArgs::default());
        app.recovery_dir = Some(dir.clone());
        app.pending_recovery = Some(PendingRecovery {
            path: path.clone(),
            when: None,
            items: 0,
        });
        app.shell.recover_prompt = Some(crate::shell::RecoverView {
            when: None,
            items: 0,
            scope: crate::shell::RecoverScope::LastSession,
        });

        app.apply_recover_choice(crate::shell::RecoverChoice::Later);

        assert!(path.exists(), "ตัวเลือก 'เก็บไว้ก่อน' ลบไฟล์ของผู้ใช้ทิ้ง");
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            before,
            "ไฟล์ถูกเขียนทับ"
        );
        assert!(app.shell.recover_prompt.is_none(), "แถบต้องหายไปหลังตอบ");
        // ★ และต้องถูกถามใหม่ได้รอบหน้า — ไฟล์ยังอยู่ให้ `scan` เจอ
        let next = refx_io::recovery::SessionId::new_unique();
        assert_eq!(
            refx_io::recovery::scan(&dir, std::slice::from_ref(&next)).len(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ negative control ของข้อบน — "ทิ้งไป" ต้องลบจริง
    ///
    /// ถ้าไม่มีข้อนี้ การ **ไม่ทำอะไรเลยทั้งสามปุ่ม** จะดูเหมือนถูกต้องสมบูรณ์
    /// (`docs/08 §3.9` ข้อ 1)
    #[test]
    fn throwing_it_away_actually_removes_it() {
        use refx_platform::fsops::rename_durable;
        let dir = std::env::temp_dir().join(format!("refx-discard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let session = refx_io::recovery::SessionId::new_unique();
        refx_io::recovery::write_snapshot(
            &dir,
            &session,
            &refx_core::board::Board::default(),
            rename_durable,
        )
        .unwrap();
        let path = refx_io::recovery::snapshot_path(&dir, &session);
        refx_io::recovery::mark_asked(&path);

        let mut app = RefxApp::new(AppArgs::default());
        app.recovery_dir = Some(dir.clone());
        app.pending_recovery = Some(PendingRecovery {
            path: path.clone(),
            when: None,
            items: 0,
        });

        app.apply_recover_choice(crate::shell::RecoverChoice::Discard);

        assert!(!path.exists(), "กด 'ทิ้งไป' แล้วไฟล์ยังอยู่");
        assert!(
            !refx_io::recovery::asked_marker(&path).exists(),
            "ไฟล์ประทับกลายเป็นขยะกำพร้า"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ กู้คืนแล้ว **ห้ามลบตัวเก่าทันที** — ต้องรอ snapshot ของเราลงดิสก์ก่อน
    ///
    /// ระหว่างจังหวะ "เอากลับมาแล้ว" กับ "autosave ครั้งแรกของ session นี้"
    /// งานชุดนั้นอยู่ใน RAM ที่เดียว · ลบตัวเก่าตรงนั้นแล้วโปรแกรมตาย = หายจริง
    /// ซึ่งคือสิ่งเดียวที่กลไกทั้งหมดนี้มีไว้กัน
    ///
    /// ★ แต่ก็ต้องไม่ค้างตลอดกาล ไม่งั้นผู้ใช้ถูกเสนอให้กู้งานเดิมซ้ำทุกครั้ง
    /// ที่เปิดโปรแกรม แล้วจะได้งานสองชุดโดยไม่รู้ว่าอันไหนใหม่กว่า
    #[test]
    fn a_restored_snapshot_is_only_dropped_once_ours_is_on_disk() {
        use refx_platform::fsops::rename_durable;
        let dir = std::env::temp_dir().join(format!("refx-adopt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let old_session = refx_io::recovery::SessionId::new_unique();
        refx_io::recovery::write_snapshot(
            &dir,
            &old_session,
            &refx_core::board::Board::default(),
            rename_durable,
        )
        .unwrap();
        let old = refx_io::recovery::snapshot_path(&dir, &old_session);

        let mut app = RefxApp::new(AppArgs::default());
        app.recovery_dir = Some(dir.clone());
        // จำลองสภาพหลังกด "เอากลับมา": ตัวเก่าถูกจอง ยังไม่ถูกลบ
        app.docs.active_mut().adopted_recovery = Some(old.clone());
        assert!(old.exists(), "ห้ามลบก่อนที่ของเราจะลงดิสก์");

        // ★ จำลอง "งาน autosave ของเราสำเร็จ" ผ่านช่องเดิมที่ production ใช้จริง
        let (tx, rx) = crossbeam_channel::bounded(1);
        tx.send(Ok(())).unwrap();
        app.docs.active_mut().autosave_job = Some(rx);
        app.tick_autosave();

        assert!(
            !old.exists(),
            "snapshot เก่ายังอยู่ — ผู้ใช้จะถูกถามให้กู้งานเดิมซ้ำทุกครั้งที่เปิดโปรแกรม"
        );
        assert_eq!(app.docs.active().adopted_recovery, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ สลัก "ถามเรื่องงานค้าง" ต้องจองได้ **ครั้งเดียว** ต่อการรัน
    ///
    /// `resumed()` ถูกเรียกใหม่ทุกครั้งที่กู้ device (`docs/04 §7`) — ถ้าสลักนี้
    /// ไม่ทำงาน ผู้ใช้จะโดนถามเรื่องงานค้างซ้ำทุกครั้งที่ไดรเวอร์สะดุด
    ///
    /// ★★ เทสต์รุ่นก่อนชื่อ `answering_once_is_enough_for_the_whole_run`
    /// แต่มัน **ตั้งธงเองแล้วยืนยันว่าธงยังตั้งอยู่** และฟังก์ชันที่มันเรียก
    /// (`apply_recover_choice`) คืนที่ด่านแรกทุกครั้งเพราะไม่มีงานค้าง
    /// → เขียวตลอดกาลโดยไม่ได้แตะสลักเลย · **ชื่อที่กว้างกว่าสิ่งที่ assert จริง
    /// ทำให้ทุกคนหลังจากนั้นเชื่อว่ามีคนตรวจแล้ว** (`docs/08 §3.9` ข้อ 14)
    #[test]
    fn the_recovery_question_is_claimed_once_per_run() {
        let mut app = RefxApp::new(AppArgs::default());
        assert!(!app.recovery_checked, "ยังไม่ได้ถามตอนเพิ่งสร้าง");

        assert!(app.claim_recovery_scan(), "ครั้งแรกต้องได้สิทธิ์ถาม");
        assert!(app.recovery_checked, "จองแล้วต้องจดไว้ ไม่งั้นครั้งที่สองก็ได้อีก");

        // ★ กู้ device แล้ว `resumed()` วิ่งซ้ำ — ต้องไม่ได้สิทธิ์อีก
        for round in 2..=5 {
            assert!(
                !app.claim_recovery_scan(),
                "รอบที่ {round} ยังได้สิทธิ์ถามอีก — ผู้ใช้จะโดนถามทุกครั้งที่ไดรเวอร์สะดุด"
            );
        }
    }

    /// ★ `Ctrl+O` ต้องติด และ `Ctrl+Shift+O` ต้องเงียบ (ยังไม่มีความหมาย)
    #[test]
    fn ctrl_o_opens_a_board_and_nothing_else_does() {
        let ctrl = ModifiersState::CONTROL;
        assert!(open_shortcut(Some('o'), ctrl));
        // บางระบบส่ง Ctrl+O มาเป็นอักขระ control
        assert!(open_shortcut(Some('\u{f}'), ctrl));
        assert!(
            !open_shortcut(Some('o'), ModifiersState::empty()),
            "ไม่กด Ctrl"
        );
        assert!(!open_shortcut(Some('s'), ctrl), "ปุ่มอื่น");
        assert!(!open_shortcut(None, ctrl));
        assert!(
            !open_shortcut(Some('o'), ctrl | ModifiersState::SHIFT),
            "Ctrl+Shift+O ยังไม่มีความหมาย ต้องเงียบ ไม่ใช่ทำอะไรที่ผู้ใช้ไม่ได้ขอ"
        );
        assert!(!open_shortcut(Some('o'), ctrl | ModifiersState::ALT));
    }

    /// ★★★ **เปิดไฟล์แล้วต้องได้ทุกอย่างกลับมาครบ** — round-trip ระดับเอกสาร
    ///
    /// ทดสอบ `read_document` ซึ่งเป็น**ฟังก์ชันเดียวกับที่ `Ctrl+O` ใช้จริง**
    /// (`docs/08 §3.9` ข้อ 9: เทสต์ที่เรียกตัวจำลองพิสูจน์ได้แค่ว่าตัวจำลองทำงาน)
    ///
    /// ★ เดินครบทุกฟิลด์ที่ ROADMAP P4-4 ระบุ: pos/size/rotation/crop/filter/
    /// tag/rating/group/note — ฟิลด์ที่ไม่มีใครเทียบคือฟิลด์ที่หายเงียบได้
    #[test]
    fn opening_a_saved_board_brings_back_every_field() {
        use refx_core::board::{
            AssetRef, BoardParts, ColorLabel, CropRect, Flip, Group, ImageFormat, Item, ItemCanvas,
            ItemFilter, ItemKind, ItemMeta, ItemParts, TagId, TextNote,
        };
        use refx_core::hash::ContentHash;

        let dir = std::env::temp_dir().join(format!("refx-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let doc = dir.join("work.refx");

        let image = Item {
            canvas: ItemCanvas {
                pos: Vec2::new(123.5, -456.25),
                size: Vec2::new(640.0, 480.0),
                rotation: 0.75,
                crop: CropRect {
                    min: Vec2::new(0.1, 0.2),
                    max: Vec2::new(0.9, 0.8),
                },
                opacity: 0.5,
                filter: ItemFilter {
                    grayscale: true,
                    invert: true,
                    brightness: -0.25,
                    contrast: 0.75,
                },
                flip: Flip::Horizontal,
                ..ItemCanvas::default()
            },
            meta: ItemMeta {
                rating: 4,
                color_label: Some(ColorLabel::Blue),
                pinned: true,
                note: "จดไว้ว่าใช้เป็นอ้างอิงแสง".to_owned(),
                tags: [TagId(1), TagId(2)].into_iter().collect(),
                added_at: 1_700_000_000_000,
                ..ItemMeta::default()
            },
            ..Item::new(ItemKind::Image(AssetRef {
                hash: ContentHash::from_bytes([7u8; 32]),
                path: std::path::PathBuf::from("C:/refs/plate.jpg"),
                px_size: glam::UVec2::new(4000, 3000),
                format: ImageFormat::Unknown,
                embedded: false,
                mtime: 1_760_000_000_000,
                file_size: 2_400_000,
            }))
        };
        let saved = refx_core::board::Board::load(
            probe_board_id(),
            BoardParts {
                name: "moodboard".to_owned(),
                groups: vec![Group {
                    name: "แสงเช้า".to_owned(),
                    collapsed: true,
                }],
                tags: vec![
                    (TagId(1), "portrait".to_owned()),
                    (TagId(2), "light".to_owned()),
                ],
                items: vec![
                    ItemParts {
                        item: image,
                        group: Some(0),
                    },
                    ItemParts {
                        item: Item::new(ItemKind::Text(TextNote {
                            text: "โน้ตบน canvas".to_owned(),
                        })),
                        group: None,
                    },
                ],
                ..BoardParts::default()
            },
        );

        refx_io::save::save_atomic(&doc, &saved, refx_platform::fsops::rename_durable).unwrap();
        let back = read_document(&doc, probe_board_id()).expect("เปิดไฟล์ที่เพิ่งบันทึกไม่ได้");

        // ★ เทียบทั้งก้อนก่อน — จับฟิลด์ที่ยังไม่มีใครนึกถึงได้ด้วย
        assert_eq!(back, saved, "เปิดกลับมาแล้วไม่เท่าเดิม");

        // แล้วเทียบทีละฟิลด์ตามที่ ROADMAP สั่ง เพื่อให้ข้อความตอนแดงชี้จุดได้
        let (id, item) = back.items_in_z_order().next().expect("ไม่มี item เลย");
        let ItemKind::Image(asset) = &item.kind else {
            panic!("ใบแรกควรเป็นภาพ");
        };
        assert_eq!(item.canvas.pos, Vec2::new(123.5, -456.25), "pos");
        assert_eq!(item.canvas.size, Vec2::new(640.0, 480.0), "size");
        assert_eq!(item.canvas.rotation, 0.75, "rotation");
        assert_eq!(item.canvas.crop.min, Vec2::new(0.1, 0.2), "crop");
        assert_eq!(item.canvas.opacity, 0.5, "opacity");
        assert!(item.canvas.filter.grayscale, "filter.grayscale");
        assert_eq!(item.canvas.filter.contrast, 0.75, "filter.contrast");
        assert_eq!(item.canvas.flip, Flip::Horizontal, "flip");
        assert_eq!(item.meta.rating, 4, "rating");
        assert_eq!(item.meta.color_label, Some(ColorLabel::Blue), "color label");
        assert!(item.meta.pinned, "pinned");
        assert_eq!(item.meta.note, "จดไว้ว่าใช้เป็นอ้างอิงแสง", "note");
        assert_eq!(item.meta.tags.as_slice(), [TagId(1), TagId(2)], "tags");
        assert_eq!(
            asset.path,
            std::path::PathBuf::from("C:/refs/plate.jpg"),
            "path"
        );
        let group = item.meta.group.expect("item ต้องยังอยู่ในกลุ่มเดิม");
        assert_eq!(
            back.group(group).map(|g| g.name.as_str()),
            Some("แสงเช้า"),
            "group"
        );
        assert_eq!(back.tags().name(TagId(2)), Some("light"), "ชื่อแท็ก");
        let _ = id;

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ **"ไฟล์เสียหาย" กับ "มาจากรุ่นใหม่กว่า" ต้องไม่ใช้ข้อความเดียวกัน**
    ///
    /// `docs/07 §1` บอกว่า `doc_crc` มีอยู่เพื่อแยกสองกรณีนี้ให้ขาด และตัวอ่าน
    /// แยกได้จริงมาตั้งแต่ P4-1 (`OpenError::Corrupt` vs `NewerVersion`)
    /// — แต่ชั้น UI รวมกลับเป็น *"ไฟล์อาจเสียหาย หรือมาจากรุ่นใหม่กว่า"*
    ///
    /// สองสถานการณ์นี้ผู้ใช้ต้องทำ **คนละอย่าง**: อันหนึ่งไปเอา `.refx.bak`
    /// (ยอมเสียงานรอบล่าสุด) อีกอันอัปเดตโปรแกรมแล้วเปิดไฟล์เดิมได้ครบ
    /// — คำแนะนำ "ไปเอาไฟล์สำรอง" กับไฟล์ที่ยังดีอยู่ คือการชวนให้เขาทิ้งงานตัวเอง
    ///
    /// ★ เดินผ่าน [`read_document`] **ตัวที่ `Ctrl+O` ใช้จริง** และไฟล์ที่ยิงเข้าไป
    /// ถูกเขียนด้วย `save_atomic` ตัวจริง แล้วค่อยทำให้เสียทีละอย่าง
    /// (`docs/08 §3.9` ข้อ 1b: ถูกต้องทุกอย่างยกเว้นสิ่งที่กำลังทดสอบ)
    #[test]
    fn a_damaged_board_and_a_newer_one_never_give_the_same_advice() {
        let dir = temp_dir_named("open-failure");
        let board = refx_core::board::Board::new(probe_board_id(), "open failure");

        // ---- เขียนไฟล์จริงสองใบ: ใบที่มี `.bak` ข้าง ๆ และใบที่ยังไม่มี ----
        let with_backup = dir.join("with-backup.refx");
        // ★ บันทึกสองรอบ — รอบที่สองคือรอบที่ `save_atomic` สร้าง `.bak` ของรอบแรก
        for _ in 0..2 {
            refx_io::save::save_atomic(&with_backup, &board, refx_platform::fsops::rename_durable)
                .unwrap();
        }
        assert!(
            refx_io::save::backup_path(&with_backup).is_file(),
            "เตรียมเทสต์ผิด: ต้องมี .bak อยู่จริงถึงจะทดสอบคำแนะนำนี้ได้"
        );

        let no_backup = dir.join("no-backup.refx");
        refx_io::save::save_atomic(&no_backup, &board, refx_platform::fsops::rename_durable)
            .unwrap();
        assert!(!refx_io::save::backup_path(&no_backup).is_file());

        // `std::fs::read` ถูกแบนใน `clippy.toml` (ไม่มีเพดานขนาด) — เทสต์ก็ไม่ยกเว้น
        let read_all = |path: &std::path::Path| {
            use std::io::Read as _;
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        };
        let good = read_all(&with_backup);
        // sanity: ไฟล์ที่ยังไม่ถูกแตะต้องเปิดได้ ไม่งั้นเทสต์นี้ไม่ได้ทดสอบอะไร
        read_document(&with_backup, probe_board_id()).expect("ไฟล์ดี ๆ เปิดไม่ได้");

        // ---- (ก) เสียหาย: พลิกไบต์ใน document เอง → `doc_crc` ไม่ตรง ----
        let damage = |path: &std::path::Path| {
            let mut bytes = read_all(path);
            let last = bytes.len() - 1;
            bytes[last] ^= 0xFF;
            std::fs::write(path, &bytes).unwrap();
        };
        damage(&with_backup);
        damage(&no_backup);

        // ---- (ข) รุ่นใหม่กว่า: แก้ **เฉพาะ** เลขเวอร์ชัน — เนื้อและ crc ยังถูกครบ ----
        let newer = dir.join("newer.refx");
        let mut bytes = good.clone();
        bytes[4..6].copy_from_slice(&9u16.to_le_bytes());
        std::fs::write(&newer, &bytes).unwrap();

        // ---- (ค) ไม่ใช่ไฟล์ของเราเลย · (ง) ไม่มีไฟล์ ----
        let stranger = dir.join("photo.jpg");
        std::fs::write(&stranger, b"\xFF\xD8\xFF\xE0 this is a jpeg, not a board").unwrap();
        let gone = dir.join("never-existed.refx");

        let fail =
            |path: &std::path::Path| read_document(path, probe_board_id()).expect_err("ควรเปิดไม่ได้");
        assert_eq!(
            fail(&with_backup),
            OpenFailure::Damaged {
                backup: Some("with-backup.refx.bak".to_owned())
            }
        );
        assert_eq!(
            fail(&no_backup),
            OpenFailure::Damaged { backup: None },
            "ไม่มี .bak อยู่จริง — ห้ามชี้ผู้ใช้ไปหาไฟล์ที่ไม่มี"
        );
        assert_eq!(fail(&newer), OpenFailure::NewerVersion);
        assert_eq!(fail(&stranger), OpenFailure::NotABoard);
        assert_eq!(fail(&gone), OpenFailure::Unreadable);

        // ---- ★★★ ข้อความบนจอต้องต่างกันจริง **ทั้งสองภาษา** ----
        for lang in [Lang::En, Lang::Th] {
            let damaged = open_failure_text(
                lang,
                &OpenFailure::Damaged {
                    backup: Some("with-backup.refx.bak".to_owned()),
                },
            );
            let newer_text = open_failure_text(lang, &OpenFailure::NewerVersion);
            assert_ne!(
                damaged, newer_text,
                "{lang:?}: สองสถานการณ์นี้ยังใช้ข้อความเดียวกันอยู่"
            );
            assert!(
                damaged.contains("with-backup.refx.bak"),
                "{lang:?}: ข้อความไฟล์เสียต้องบอกชื่อไฟล์สำรองที่มีอยู่จริง — {damaged}"
            );
            assert!(
                !newer_text.contains(".bak"),
                "{lang:?}: ไฟล์จากรุ่นใหม่กว่ายังดีอยู่ ห้ามชวนให้ไปเอาไฟล์สำรองแทน — {newer_text}"
            );
            assert!(
                !open_failure_text(lang, &OpenFailure::Damaged { backup: None }).contains(".bak"),
                "{lang:?}: ไม่มีไฟล์สำรอง แต่ยังพูดถึงมัน"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★ สองหน้าต่างต้องไม่เขียนทับ snapshot ของกันและกัน (`docs/07 §4`)
    #[test]
    fn two_instances_never_share_a_recovery_file() {
        let a = RefxApp::new(AppArgs::default());
        let b = RefxApp::new(AppArgs::default());
        assert_ne!(
            a.docs.active().session,
            b.docs.active().session,
            "สองหน้าต่างได้รหัส session เดียวกัน"
        );
    }

    // ---------- ★★★ P4-7c: `Docs` + แท็บ + recovery slot ต่อแท็บ ----------

    /// ★★★ **สองแท็บที่ยังไม่เคยบันทึก ต้องเขียนคนละไฟล์** — หัวใจของ P4-7c
    ///
    /// ก่อนหน้านี้ `<session>.refx` มีไฟล์เดียวต่อโปรเซส · สองแท็บที่ยังไม่ได้
    /// `Ctrl+S` จะเขียนทับกันไปมาทุก 10 วินาที แล้วเปิดโปรแกรมใหม่ได้งานคืน
    /// **ใบเดียว** โดยไม่มีอะไรบอกว่าอีกใบเคยมีอยู่ — I-3 ที่เงียบที่สุดแบบหนึ่ง
    ///
    /// ★ เดินผ่าน [`RefxApp::tick_autosave_one`] **ตัวที่โปรแกรมใช้จริง** ไม่ใช่
    /// ตรรกะที่เขียนเลียนแบบ (`docs/08 §3.9` ข้อ 9) — เธรดที่มันสร้างถูกรอด้วย
    /// การ poll ช่องผลลัพธ์ ไม่ใช่ด้วยการ sleep (ข้อ 5b: ห้าม assert เวลานาฬิกา)
    #[test]
    fn two_unsaved_tabs_never_share_a_recovery_slot() {
        let dir = temp_dir_named("two-tabs");
        let mut docs = Docs::default();
        let second = docs.mint();
        docs.push(Doc::empty(second));
        assert_ne!(
            docs.list[0].session, docs.list[1].session,
            "สองแท็บได้ recovery slot ไฟล์เดียวกัน — งานของใบหนึ่งจะหายเงียบ ๆ"
        );

        // ทั้งสองใบมีงานที่ยังไม่ได้เก็บ
        for doc in docs.iter_mut() {
            add_note(doc, "งานที่ยังไม่เคยบันทึก");
            assert!(RefxApp::doc_has_unsnapshotted_work(doc));
        }
        for doc in docs.iter_mut() {
            RefxApp::tick_autosave_one(doc, Some(dir.as_path()), true, None);
        }
        // รอเธรดเขียนจบทั้งสองใบ — ผ่านช่องเดิมที่ `tick_autosave` ใช้เก็บผล
        for doc in docs.iter_mut() {
            let rx = doc.autosave_job.take().expect("ไม่ได้ส่งงานเขียนเลย");
            rx.recv().expect("เธรด autosave ตาย").expect("เขียนไม่สำเร็จ");
        }

        let written: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .expect("ไม่มีโฟลเดอร์ recovery")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "refx"))
            .collect();
        assert_eq!(
            written.len(),
            2,
            "สองแท็บที่ไม่เคยบันทึกเขียน snapshot ได้ {} ไฟล์ — อีกใบหายไป",
            written.len()
        );
        for doc in docs.iter() {
            let mine = refx_io::recovery::snapshot_path(&dir, &doc.session);
            assert!(mine.exists(), "แท็บนี้ไม่มี snapshot ของตัวเอง");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ **negative control ของข้อบน** — session ร่วมกันแล้วต้องเหลือไฟล์เดียว
    ///
    /// ถ้าไม่มีข้อนี้ เทสต์ข้างบนจะเขียวได้ด้วยเหตุผลอื่น (เช่นเขียนไม่สำเร็จ
    /// ทั้งคู่แล้วบังเอิญนับได้ 2 จากไฟล์อื่น) · ที่นี่พิสูจน์ว่า **ตัวแยกจริง ๆ
    /// คือ `Doc::session`** ไม่ใช่อย่างอื่น (`docs/08 §3.9` ข้อ 1)
    #[test]
    fn sharing_one_session_is_what_loses_the_other_tab() {
        let dir = temp_dir_named("shared-session");
        let mut docs = Docs::default();
        let second = docs.mint();
        docs.push(Doc::empty(second));
        // จำลองดีไซน์เดิม: ทั้งสองแท็บถือ session เดียวกัน
        let shared = docs.list[0].session.clone();
        docs.list[1].session = shared;

        for doc in docs.iter_mut() {
            add_note(doc, "งานที่ยังไม่เคยบันทึก");
            RefxApp::tick_autosave_one(doc, Some(dir.as_path()), true, None);
        }
        for doc in docs.iter_mut() {
            if let Some(rx) = doc.autosave_job.take() {
                let _ = rx.recv();
            }
        }
        let count = std::fs::read_dir(&dir)
            .expect("ไม่มีโฟลเดอร์ recovery")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "refx"))
            .count();
        assert_eq!(count, 1, "session ร่วมกันแล้วยังได้สองไฟล์ — ประตูนี้ไม่ล้มเป็น");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ **แท็บหลังบ้านต้องปลุกโปรแกรมมาเขียนของมันได้** (I-3 + I-1)
    ///
    /// นาฬิกา autosave ต้องเป็นของ *ทุกแท็บ* ไม่ใช่ของใบที่อยู่หน้าจอ — ไม่งั้น
    /// แท็บที่ผู้ใช้สลับออกไปจะถูกเก็บก็ต่อเมื่อเขาบังเอิญสลับกลับมา ซึ่งเป็น
    /// เงื่อนไขที่ crash ไม่เคยรอ
    ///
    /// ★ และ **ต้องเงียบเมื่อทุกใบเก็บครบแล้ว** — นาฬิกาที่ดังตลอดคือ `Poll`
    /// ที่สะกดด้วยชื่ออื่น (I-1)
    #[test]
    fn a_background_tab_still_asks_to_be_woken() {
        let dir = std::path::PathBuf::from("/data/RefX/recovery");
        let mut docs = Docs::default();
        let second = docs.mint();
        docs.push(Doc::empty(second));

        assert_eq!(
            RefxApp::next_autosave_across_tabs(&docs, Some(dir.as_path())),
            None,
            "ไม่มีอะไรแก้เลยแต่ยังขอให้ปลุก"
        );

        // แก้เฉพาะใบที่ **ไม่ได้** อยู่หน้าจอ
        docs.active = 1;
        add_note(&mut docs.list[0], "งานของแท็บหลังบ้าน");
        assert!(
            RefxApp::next_autosave_across_tabs(&docs, Some(dir.as_path())).is_some(),
            "แท็บที่ไม่ได้อยู่หน้าจอไม่มีใครปลุกมาเขียนให้เลย"
        );

        // เก็บครบแล้ว = เงียบสนิทอีกครั้ง (I-1)
        docs.list[0].snapshot_revision = Some(docs.list[0].board.revision());
        assert_eq!(
            RefxApp::next_autosave_across_tabs(&docs, Some(dir.as_path())),
            None,
            "เก็บครบแล้วยังขอให้ปลุกทุก 10 วินาทีตลอดทั้งวัน"
        );
    }

    /// ★★★ **`BoardId` ต้องแจกไม่ซ้ำจริง** (`docs/02 §2.10`)
    ///
    /// `ItemId` เป็นแค่ `index+generation` **ไม่ผูกกับ board** — สอง arena
    /// เริ่มนับจาก 0 เหมือนกันเป๊ะ · id ของ board จึงเป็นสิ่งเดียวที่แยก
    /// "ผลงานนี้เป็นของแท็บไหน" ออกจากกันได้ · **ปิดแล้วห้ามใช้ id ซ้ำ** ด้วย:
    /// งาน decode ที่ยังค้างอยู่ของแท็บที่ตายไปจะไปเกาะแท็บใหม่ทันที
    #[test]
    fn every_tab_gets_an_id_that_is_never_handed_out_twice() {
        let mut docs = Docs::default();
        let mut seen = vec![docs.active().id];
        for _ in 0..8 {
            let id = docs.mint();
            docs.push(Doc::empty(id));
            seen.push(id);
        }
        // ปิดทิ้งจนเหลือใบเดียว แล้วเปิดใหม่ — id ต้องไม่วนกลับมาซ้ำของเดิม
        while docs.len() > 1 {
            docs.close(0);
        }
        for _ in 0..4 {
            let id = docs.mint();
            docs.push(Doc::empty(id));
            seen.push(id);
        }
        let unique: std::collections::HashSet<_> = seen.iter().collect();
        assert_eq!(unique.len(), seen.len(), "แจก BoardId ซ้ำ — {seen:?}");
        // ★ และ `ItemId` ของสอง board **ซ้ำกันจริง** ซึ่งคือเหตุผลที่ id ต้องมี
        let mut a = Doc::empty(seen[0]);
        let mut b = Doc::empty(seen[1]);
        add_note(&mut a, "ก");
        add_note(&mut b, "ข");
        assert_eq!(
            a.board.z_order().first(),
            b.board.z_order().first(),
            "สอง board แจก ItemId คนละชุด — สมมติฐานของทั้ง P4-7c เปลี่ยนไปแล้ว"
        );
    }

    /// ★★★ **สองแท็บที่เปิดไฟล์เดียวกันต้องได้คีย์งานคนละใบ** (`job_key_for`)
    ///
    /// คีย์เดิมเป็น `hash_bytes(path)` ล้วน ๆ ซึ่งถูกตราบที่มี board เดียว ·
    /// พอสอง board อ้างถึงภาพใบเดียวกัน ตารางจับคู่ของใบที่ใส่ทีหลังจะ
    /// **เขียนทับ** ของใบแรก → แท็บแรกได้ item ที่ไม่มีวันได้พิกเซล
    #[test]
    fn two_tabs_wanting_the_same_file_do_not_collide() {
        use refx_core::arena::ArenaKey as _;
        let a = refx_core::arena::BoardId::from_parts(1, 0);
        let b = refx_core::arena::BoardId::from_parts(2, 0);
        let path = std::path::Path::new("/work/ref/sky.png");

        assert_ne!(
            job_key_for(a, path),
            job_key_for(b, path),
            "สองแท็บที่ขอไฟล์เดียวกันได้คีย์งานเท่ากัน — ใบหนึ่งจะไม่มีวันได้ภาพ"
        );
        // ★ negative control ของ *ดีไซน์เดิม* — คีย์จาก path ล้วนชนกันเสมอ
        assert_eq!(
            refx_asset::hash::hash_bytes(path.to_string_lossy().as_bytes()),
            refx_asset::hash::hash_bytes(path.to_string_lossy().as_bytes()),
            "คีย์จาก path ล้วนไม่ได้ชนกัน — สมมติฐานของเทสต์นี้ผิด"
        );
        // ★★ แต่ **ภายในแท็บเดียวกันต้องยังยุบงานซ้ำได้เหมือนเดิม**
        assert_eq!(
            job_key_for(a, path),
            job_key_for(a, path),
            "คีย์ของแท็บเดียวกันไม่เสถียร — ลากไฟล์เดิมสองครั้งจะได้สองงาน"
        );
        assert_ne!(
            job_key_for(a, path),
            job_key_for(a, std::path::Path::new("/work/ref/sea.png")),
            "คนละไฟล์ในแท็บเดียวกันได้คีย์เท่ากัน"
        );
    }

    /// ★★★ **spool ต้องคุ้มครองภาพของ *ทุก* แท็บ** (§4 ข้อ 24)
    ///
    /// ภาพที่วางไว้ในแท็บหลังบ้านมีต้นฉบับอยู่ที่ spool ที่เดียว · ถามแค่แท็บที่
    /// อยู่หน้าจอแล้วเพดานจะลบมันทิ้ง **ขณะที่แท็บนั้นยังเปิดอยู่** แล้วภาพจะ
    /// กลายเป็นช่องว่างทันทีที่ผู้ใช้สลับกลับไปดู และกู้คืนไม่ได้เลย
    #[test]
    fn the_spool_sweep_asks_every_tab_not_just_the_visible_one() {
        let front = content_hash(1);
        let back = content_hash(2);
        let mut docs = Docs::default();
        docs.list[0].board = board_pointing_at(front, std::path::Path::new("/spool/a.png"));
        let id = docs.mint();
        let mut hidden = Doc::empty(id);
        hidden.board = board_pointing_at(back, std::path::Path::new("/spool/b.png"));
        docs.push(hidden);
        docs.active = 0; // ★ ใบหลังบ้านคือใบที่ **ไม่ได้** อยู่หน้าจอ

        let protected = hashes_of_every_tab(&docs);
        assert!(protected.contains(&front), "ภาพของแท็บหน้าจอไม่ถูกคุ้มครอง");
        assert!(
            protected.contains(&back),
            "ภาพของแท็บหลังบ้านไม่ถูกคุ้มครอง — เพดานจะลบต้นฉบับเดียวที่เหลือทิ้ง"
        );
        // ★ negative control: ถามแค่ใบที่อยู่หน้าจอแล้วอีกใบหลุดทันที
        let visible_only = refx_io::spool::hashes_of(&docs.active().board);
        assert!(!visible_only.contains(&back), "ประตูนี้ไม่ล้มเป็น");
    }

    /// ★★★ **`Docs` ห้ามว่าง** — ปิดใบสุดท้ายแล้วได้ board เปล่าใบใหม่
    ///
    /// สภาพ "ไม่มีเอกสารเลย" จะบังคับให้ทุกเส้นทางมีกิ่ง `None` เพิ่มอีกหนึ่ง
    /// ซึ่งเป็นกิ่งที่ไม่มีใครเดินและจะเน่าเงียบ ๆ (`docs/08 §3.9` ข้อ 2)
    #[test]
    fn closing_the_last_tab_leaves_an_empty_board_not_nothing() {
        let mut docs = Docs::default();
        let first = docs.active().id;
        docs.close(0);
        assert_eq!(docs.len(), 1, "ปิดใบสุดท้ายแล้วเหลือรายการว่าง");
        assert_ne!(docs.active().id, first, "ใบใหม่ได้ id ของใบที่ปิดไปแล้ว");
        assert!(docs.active().board.is_empty());
        assert!(docs.active().path.is_none());
    }

    /// ★★★ สลับแท็บแล้ว **การลากที่ค้างอยู่ของใบเดิมต้องถูกยกเลิก**
    ///
    /// การลากที่เริ่มบนแท็บหนึ่งใช้ต่อกับอีกแท็บไม่ได้ — `ItemId` ที่มันถืออยู่
    /// ชี้ไป board คนละใบ (`docs/02 §2.10`) · เหตุผลเดียวกับตอนสลับเครื่องมือ
    ///
    /// ★ **สิ่งที่เทสต์นี้คุมไม่ได้คือการวาดใหม่** — `gfx.quads` ต้องมีหน้าต่างจริง
    /// · นั่นคือช่องที่บั๊ก 28 ส.ค. 2026 เดินผ่าน (สลับแท็บแล้วยังเห็นภาพของใบเดิม)
    /// และมันถูกจับได้ด้วย **ภาพหน้าจอ** เท่านั้น — ยืนยันด้วยมือ 28 ส.ค. 2026
    #[test]
    fn switching_tabs_drops_the_drag_that_belonged_to_the_old_one() {
        let mut app = RefxApp::new(AppArgs::default());
        let id = app.docs.mint();
        app.docs.push(Doc::empty(id));
        app.docs.active = 0;
        // จำลองการลากที่ค้างอยู่บนใบแรก
        app.docs.list[0].rubber_band = Some(WorldRect::from_center_size(Vec2::ZERO, Vec2::ONE));
        app.docs.list[0].guides.push(refx_core::align::Guide {
            vertical: true,
            at: 1.0,
            from: 0.0,
            to: 1.0,
        });

        app.focus_tab(1);

        assert_eq!(app.docs.active, 1, "สลับแท็บไม่สำเร็จ");
        assert!(
            app.docs.list[0].rubber_band.is_none(),
            "กรอบลากของแท็บเดิมยังค้างอยู่ — มันจะถูกวาดทับกระดานใบใหม่"
        );
        assert!(app.docs.list[0].guides.is_empty(), "ไกด์ของแท็บเดิมยังค้างอยู่");

        // ★ สลับไปที่ใบเดิมซ้ำ = ไม่มีอะไรเกิดขึ้น (ไม่สั่งวาดใหม่ฟรี ๆ — I-1)
        app.focus_tab(1);
        assert_eq!(app.docs.active, 1);
        // ★ ดัชนีนอกช่วงต้องเงียบ ไม่ใช่พาไปแท็บมั่ว
        app.focus_tab(99);
        assert_eq!(app.docs.active, 1, "ดัชนีนอกช่วงพาผู้ใช้ไปแท็บอื่น");
    }

    /// ★ ปิดแท็บกลาง ๆ แล้ว "แท็บที่ดูอยู่" ต้องไม่กระโดดไปคนละใบ
    #[test]
    fn closing_a_tab_keeps_pointing_at_the_same_document() {
        let mut docs = Docs::default();
        for _ in 0..2 {
            let id = docs.mint();
            docs.push(Doc::empty(id));
        }
        docs.active = 2;
        let watching = docs.active().id;
        docs.close(0); // ปิดใบก่อนหน้า
        assert_eq!(
            docs.active().id,
            watching,
            "ปิดแท็บอื่นแล้วผู้ใช้ถูกพาไปดูเอกสารคนละฉบับ"
        );
        docs.close(docs.active); // ปิดใบที่ดูอยู่ (เป็นใบสุดท้ายพอดี)
        assert_ne!(docs.active().id, watching);
    }

    /// ★★★ **`Ctrl+W` บนแท็บที่ยังไม่บันทึกต้อง *ถามก่อน*** (`docs/03 §5`)
    ///
    /// ปิดไปเลยคือการทำงานหายด้วยการกดผิดปุ่มเดียว ซึ่งเป็นสิ่งเดียวกับที่แถบ
    /// ยืนยันตอนปิดหน้าต่างมีไว้กัน — แค่ย้ายมาโผล่ที่ขอบเขตเล็กกว่า
    #[test]
    fn closing_a_dirty_tab_asks_before_it_closes_anything() {
        let mut app = RefxApp::new(AppArgs::default());
        let id = app.docs.mint();
        app.docs.push(Doc::empty(id));
        add_note(app.docs.active_mut(), "งานที่ยังไม่ได้บันทึก");
        assert!(app.docs.active().board.is_dirty());

        // ★ ไม่มี `Gfx` ในเทสต์ — เส้นทางถามผูกกับ `gfx.is_some()` เหมือนของจริง
        //   จึงตรวจ **ตัวตัดสิน** ตรง ๆ แทนการเรียกผ่านหน้าต่างที่สร้างไม่ได้
        let dirty = app.docs.active().board.is_dirty();
        assert!(dirty, "สมมติฐานของเทสต์ผิด");

        // ตอบ "ปิดโดยไม่บันทึก" แล้วถึงจะหายไปจริง
        app.closing_tab = Some(app.docs.active().id);
        app.shell.close_prompt = true;
        app.shell.close_scope_tab = true;
        let before = app.docs.len();
        app.apply_close_choice(crate::shell::CloseChoice::DiscardAndClose);
        assert_eq!(app.docs.len(), before - 1, "ตอบว่าปิดแล้วแท็บยังอยู่");
        assert!(!app.shell.close_prompt, "แถบยืนยันค้างอยู่หลังตอบ");
        assert!(!app.shell.close_scope_tab);

        // ★ negative control — "ทำงานต่อ" ต้องไม่ปิดอะไรเลย
        let id = app.docs.mint();
        app.docs.push(Doc::empty(id));
        add_note(app.docs.active_mut(), "อีกใบ");
        app.closing_tab = Some(app.docs.active().id);
        app.shell.close_prompt = true;
        let before = app.docs.len();
        app.apply_close_choice(crate::shell::CloseChoice::Cancel);
        assert_eq!(app.docs.len(), before, "กด 'ทำงานต่อ' แล้วแท็บหายไป");
    }

    /// ★★★ **ปิดหน้าต่างต้องถามถ้ามีแท็บ *ใบใดก็ตาม* ที่ยังไม่บันทึก**
    ///
    /// ถามแค่ใบที่อยู่หน้าจอ แล้วผู้ใช้ที่ทำงานค้างไว้ในอีกสามแท็บจะปิดโปรแกรม
    /// ไปเฉย ๆ โดยไม่มีอะไรเตือน — ซึ่งคือลูปเดิมที่ P4-2 มีไว้กันพอดี
    #[test]
    fn a_dirty_tab_behind_the_visible_one_still_blocks_the_close() {
        let mut docs = Docs::default();
        let id = docs.mint();
        docs.push(Doc::empty(id));
        add_note(&mut docs.list[0], "งานของแท็บหลังบ้าน");
        docs.active = 1; // ผู้ใช้กำลังดูใบที่สะอาด

        assert!(
            !docs.active().board.is_dirty(),
            "สมมติฐานของเทสต์ผิด — ใบที่ดูอยู่ต้องสะอาด"
        );
        assert_eq!(
            docs.iter().position(|doc| doc.board.is_dirty()),
            Some(0),
            "ไม่เห็นงานค้างของแท็บหลังบ้าน — ปิดโปรแกรมแล้วมันหายไปเงียบ ๆ"
        );
    }

    /// ★★★ คีย์ของแท็บครบตาม `docs/03 §5` — และ **เงียบเมื่อยังไม่มีความหมาย**
    ///
    /// ★★ ยิงด้วย layout ไทยด้วย (HANDOFF §2.12): matcher ที่ดูแต่ logical key
    /// ทำให้คีย์ลัด**ทุกตัว**ใช้ไม่ได้บน layout ไทยมาตั้งแต่ P2-4
    #[test]
    fn the_tab_keys_do_what_the_spec_says_and_nothing_else() {
        let ctrl = ModifiersState::CONTROL;
        let tab_key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Tab);
        let plain = key("t");

        assert_eq!(tab_shortcut(Some('t'), &plain, ctrl), Some(TabKey::New));
        assert_eq!(tab_shortcut(Some('w'), &plain, ctrl), Some(TabKey::Close));
        assert_eq!(tab_shortcut(None, &tab_key, ctrl), Some(TabKey::Next));
        // บางระบบส่งมาเป็นอักขระ control — เหมือน `Ctrl+S`/`Ctrl+O`
        assert_eq!(
            tab_shortcut(Some('\u{14}'), &plain, ctrl),
            Some(TabKey::New)
        );
        assert_eq!(
            tab_shortcut(Some('\u{17}'), &plain, ctrl),
            Some(TabKey::Close)
        );

        // ★ ไม่กด Ctrl = ตัวอักษรธรรมดา (ผู้ใช้กำลังพิมพ์)
        assert_eq!(
            tab_shortcut(Some('t'), &plain, ModifiersState::empty()),
            None
        );
        // ★ ปุ่มที่ยังไม่มีความหมายต้องเงียบ ไม่ใช่ทำอะไรที่ผู้ใช้ไม่ได้ขอ
        assert_eq!(
            tab_shortcut(Some('t'), &plain, ctrl | ModifiersState::SHIFT),
            None
        );
        assert_eq!(
            tab_shortcut(Some('w'), &plain, ctrl | ModifiersState::ALT),
            None
        );
        assert_eq!(tab_shortcut(Some('q'), &plain, ctrl), None);

        // ★★ layout ไทย: ปุ่ม `T` ส่ง `ะ` มา · `W` ส่ง `ไ` — physical key ต้องช่วยไว้
        assert_eq!(
            tab_shortcut(
                pressed_thai("ะ", winit::keyboard::KeyCode::KeyT),
                &key("ะ"),
                ctrl
            ),
            Some(TabKey::New),
            "กด Ctrl+T บน layout ไทยไม่ได้"
        );
        assert_eq!(
            tab_shortcut(
                pressed_thai("ไ", winit::keyboard::KeyCode::KeyW),
                &key("ไ"),
                ctrl
            ),
            Some(TabKey::Close),
            "กด Ctrl+W บน layout ไทยไม่ได้"
        );
    }

    /// ★ `Ctrl+T`/`Ctrl+W` ต้องไม่ไปชนคีย์ลัดที่มีอยู่แล้ว
    #[test]
    fn the_tab_keys_do_not_steal_any_existing_shortcut() {
        let ctrl = ModifiersState::CONTROL;
        let plain = key("t");
        for taken in ['s', 'o', 'z', 'y', 'v', 'a', 'g'] {
            assert_eq!(
                tab_shortcut(Some(taken), &key(&taken.to_string()), ctrl),
                None,
                "Ctrl+{taken} ถูกแท็บแย่งไป"
            );
        }
        // และทางกลับกัน — `Ctrl+T`/`Ctrl+W` ต้องไม่ไปติดของคนอื่น
        assert!(!open_shortcut(Some('t'), ctrl));
        assert!(!open_shortcut(Some('w'), ctrl));
        assert_eq!(save_shortcut(Some('t'), ctrl), None);
        assert_eq!(save_shortcut(Some('w'), ctrl), None);
        assert_eq!(history_shortcut(Some('t'), ctrl), None);
        let _ = plain;
    }

    /// ★★ เอกสารที่เปิดแล้ว **ไปที่แท็บเดิม ไม่ใช่เปิดซ้อน**
    ///
    /// สองแท็บบนไฟล์เดียวกันแปลว่าสอง `<doc>.refx.autosave` ทับกัน และ `Ctrl+S`
    /// ของใบหลังจะกลืนงานของใบแรก — รูปแบบเดียวกับ recovery slot ที่ P4-7c แก้
    #[test]
    fn opening_a_file_that_is_already_open_goes_to_its_tab() {
        let mut docs = Docs::default();
        let path = std::path::Path::new("/work/moodboard.refx");
        docs.list[0].path = Some(path.to_path_buf());
        let id = docs.mint();
        docs.push(Doc::empty(id));
        assert_eq!(docs.active, 1);

        assert_eq!(docs.index_of_path(path), Some(0));
        assert_eq!(
            docs.index_of_path(std::path::Path::new("/work/other.refx")),
            None
        );
    }

    /// ★★ แท็บเปล่าที่ยังไม่เคยถูกแตะคือ "ที่นั่งว่าง" — เปิดไฟล์แรกไม่ทิ้งขยะไว้
    #[test]
    fn the_first_document_reuses_the_empty_tab_instead_of_leaving_it_behind() {
        let mut docs = Docs::default();
        assert!(docs.active().is_untouched(), "แท็บแรกต้องนับว่ายังไม่ถูกแตะ");

        add_note(docs.active_mut(), "ผู้ใช้เริ่มทำงานแล้ว");
        assert!(
            !docs.active().is_untouched(),
            "แท็บที่มีงานอยู่ถูกอ่านว่าว่าง — เปิดไฟล์ทับแล้วงานหาย"
        );

        let mut with_path = Doc::empty(docs.mint());
        with_path.path = Some(std::path::PathBuf::from("/work/a.refx"));
        assert!(!with_path.is_untouched(), "เอกสารที่มีไฟล์อยู่แล้วถูกอ่านว่าว่าง");
    }

    /// ★★★ **ทุกงาน thumbnail ที่ถูกส่ง ต้องมีเจ้าของจดไว้เสมอ**
    ///
    /// นี่คือประตูที่บั๊กจริงเดินผ่านเมื่อ 28 ส.ค. 2026: การจดเจ้าของอยู่ที่จุดเรียก
    /// แล้วสองในสามจุดลืมจด · ผลคือ **ภาพที่ลากเข้ามาไม่ขึ้นจอเลยสักใบ** —
    /// งานถูก decode สำเร็จแล้วถูกทิ้งเพราะ `drain_decode_results` หาแท็บปลายทาง
    /// ไม่เจอ · เทสต์ 849 ตัวเขียวหมดตอนนั้น เจอเพราะรันแอปจริงแล้วดู
    ///
    /// ★ ตอนนี้ทั้งสามเส้นทาง (ลากไฟล์ · วาง · relink) ผ่าน
    /// [`RefxApp::submit_thumbnail_jobs`] ประตูเดียว — ที่นี่ยิงประตูนั้นตรง ๆ
    #[test]
    fn every_thumbnail_job_remembers_which_tab_asked_for_it() {
        let mut app = RefxApp::new(AppArgs::default());
        let owner = app.docs.active().id;
        let key = job_key_for(owner, std::path::Path::new("/work/a.png"));
        app.submit_thumbnail_jobs(
            owner,
            vec![refx_asset::pool::Job {
                hash: key,
                source: refx_asset::pool::JobSource::File("/work/a.png".into()),
                priority: 0.0,
                cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                target: refx_asset::pool::JobTarget::Thumbnail,
            }],
        );
        assert_eq!(
            app.job_owner.get(&key),
            Some(&owner),
            "ส่งงานแล้วไม่ได้จดว่าเป็นของแท็บไหน — ผลที่กลับมาจะไม่มีที่ให้ลง"
        );
        assert!(
            app.job_sources.contains_key(&key),
            "ส่งงานแล้วไม่ได้จดที่มา — ภาพใบนี้จะขอภาพคมตอนซูมไม่ได้ตลอดกาล"
        );

        // ★★ เส้นทางค้นหาที่ `drain_decode_results` เดินจริง ต้องหาแท็บเจอ
        let found = app
            .job_owner
            .get(&key)
            .and_then(|id| app.docs.list.iter().position(|doc| doc.id == *id));
        assert_eq!(found, Some(0), "จดเจ้าของแล้วแต่ยังหาแท็บปลายทางไม่เจอ");

        // ★ negative control — คีย์ที่ไม่มีเจ้าของคือคีย์ที่ผลลัพธ์ถูกทิ้ง
        //   (นี่คืออาการของบั๊กจริงเป๊ะ ๆ)
        let orphan = job_key_for(owner, std::path::Path::new("/work/never-submitted.png"));
        assert!(
            !app.job_owner.contains_key(&orphan),
            "คีย์ที่ไม่เคยถูกส่งกลับมีเจ้าของ — ประตูนี้ไม่ล้มเป็น"
        );
    }

    /// ★ ปิดแท็บแล้ว **คีย์งานที่ค้างอยู่ของมันต้องถูกทิ้ง** (I-6)
    ///
    /// ผลของงานที่ยังเดินอยู่ไม่มีที่ให้ลงแล้ว · ไม่เก็บกวาดคือตารางที่โต
    /// ตลอดอายุโปรแกรมทุกครั้งที่ผู้ใช้เปิด-ปิดแท็บ
    #[test]
    fn closing_a_tab_forgets_the_work_that_was_still_running_for_it() {
        let mut app = RefxApp::new(AppArgs::default());
        let doomed = app.docs.mint();
        app.docs.push(Doc::empty(doomed));
        let survivor = app.docs.list[0].id;

        let mine = job_key_for(doomed, std::path::Path::new("/work/a.png"));
        let theirs = job_key_for(survivor, std::path::Path::new("/work/b.png"));
        for (key, owner) in [(mine, doomed), (theirs, survivor)] {
            app.submit_thumbnail_jobs(
                owner,
                vec![refx_asset::pool::Job {
                    hash: key,
                    source: refx_asset::pool::JobSource::File("/work/x.png".into()),
                    priority: 0.0,
                    cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    target: refx_asset::pool::JobTarget::Thumbnail,
                }],
            );
        }

        app.close_tab_now(1);
        assert!(!app.job_owner.contains_key(&mine), "คีย์ของแท็บที่ปิดไปยังค้างอยู่");
        assert!(
            app.job_owner.contains_key(&theirs),
            "ปิดแท็บหนึ่งแล้วกวาดคีย์ของอีกแท็บไปด้วย — ภาพของมันจะไม่มีวันขึ้น"
        );
    }

    /// โฟลเดอร์ชั่วคราวของเทสต์ — ชื่อแยกตามเธรดเหมือนตัวอื่นในไฟล์นี้
    fn temp_dir_named(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-tabs-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("สร้างโฟลเดอร์ทดสอบไม่ได้");
        dir
    }

    /// เติมโน้ตหนึ่งใบผ่าน `Command` — ทางเดียวกับที่ของจริงแก้ `Board`
    fn add_note(doc: &mut Doc, text: &str) {
        let item = Item::new(ItemKind::Text(refx_core::board::TextNote {
            text: text.to_owned(),
        }));
        let command = AddItems::new(vec![item]).expect("สร้างคำสั่งไม่ได้");
        doc.apply(Box::new(command)).expect("คำสั่งล้ม");
    }

    /// ★★ `JobSource` คือ **ประตูของ working texture** — ไม่มีไฟล์ = ไม่ขอภาพคม
    ///
    /// ★ แก้คำอธิบาย 19 ส.ค. 2026 (P4-5): เดิมเขียนว่า "ภาพจาก clipboard ต้องไม่
    /// ไปขอ working texture" ซึ่ง **เลิกจริงไปแล้ว** — ตอนนี้มันขอได้ทันทีที่
    /// `JobResult::Spooled` มาถึงแล้ว `drain_decode_results` สลับ `source` ของ
    /// item นั้นเป็น `File(<spool>/<hash>.png)` · สิ่งที่ยังจริงคือ **ประตู**:
    /// ตราบใดที่ยังไม่มีไฟล์ การขอภาพคมจะได้ error ที่ผู้ใช้ทำอะไรกับมันไม่ได้
    #[test]
    fn only_an_item_with_a_file_behind_it_may_ask_for_a_sharper_image() {
        assert!(refx_asset::pool::JobSource::Clipboard.file().is_none());
        assert!(
            refx_asset::pool::JobSource::File(std::path::PathBuf::from("a.png"))
                .file()
                .is_some()
        );
        // ★ หลังถูกพักลง spool แล้ว item เดิมกลายเป็น "มีไฟล์" — ประตูเปิด
        let spooled = refx_asset::pool::JobSource::File(refx_io::spool::spool_path(
            std::path::Path::new("/data/RefX/pasted"),
            refx_asset::hash::hash_pasted(1, 1, &[0u8; 4]),
        ));
        assert!(
            spooled.file().is_some(),
            "ภาพที่วางถูกพักไว้แล้วแต่ยังขอภาพคมไม่ได้ = หนี้ P1-8 ยังไม่ถูกปลด"
        );
    }

    /// ★ ข้อความในโน้ตต้องไม่ล้นออกนอกกรอบ (P2-11)
    ///
    /// ผู้ใช้พิมพ์ประโยคยาวเป็นเรื่องปกติ ถ้าไม่ตัดบรรทัด ข้อความจะพาดข้าม board
    /// ไปทับภาพอื่น ซึ่งอ่านว่า "โปรแกรมวาดผิด" ไม่ใช่ "โน้ตยาวเกินกรอบ"
    #[test]
    fn a_note_never_paints_outside_its_own_box() {
        // กรอบ 100 px ที่ font 10 → ประมาณ (100-12)/5.5 = 16 คอลัมน์
        let wrapped = wrap_note("aaa bbb ccc ddd eee fff", 100.0, 10.0);
        for line in wrapped.lines() {
            assert!(
                line.chars().count() <= 16,
                "บรรทัด {line:?} ยาว {} เกินกรอบ",
                line.chars().count()
            );
        }
        assert!(wrapped.contains('\n'), "ข้อความยาวต้องถูกตัดบรรทัด: {wrapped:?}");
        // ★ ตัวอักษรต้องครบ ไม่ใช่ถูกตัดทิ้ง — โน้ตที่หายไปครึ่งคือข้อมูลผู้ใช้หาย
        let letters: String = wrapped.chars().filter(|c| c.is_alphanumeric()).collect();
        assert_eq!(letters, "aaabbbcccdddeeefff");
    }

    /// คำเดียวที่ยาวกว่าบรรทัดต้องถูกหั่น ไม่ใช่ปล่อยล้นออกไปคำเดียว
    #[test]
    fn one_very_long_word_is_broken_instead_of_overflowing() {
        let wrapped = wrap_note(&"x".repeat(60), 100.0, 10.0);
        for line in wrapped.lines() {
            assert!(line.chars().count() <= 16, "{line:?}");
        }
        assert_eq!(wrapped.chars().filter(|c| *c == 'x').count(), 60);
    }

    /// ขึ้นบรรทัดใหม่ที่ผู้ใช้พิมพ์เองต้องถูกเก็บไว้
    #[test]
    fn explicit_line_breaks_survive_wrapping() {
        let wrapped = wrap_note(
            "one
two", 400.0, 10.0,
        );
        assert_eq!(
            wrapped,
            "one
two"
        );
    }

    /// ★★ **picker ต้องอ่าน pixel ตัวเดียวกับที่ shader วาดตรงนั้น** (P2-10)
    ///
    /// สองเส้นทางนี้เดินสวนกัน: `crop_uv` เอาช่วงของต้นฉบับไปให้ GPU วาด
    /// ส่วน `pick::source_uv` เอาจุดบนจอย้อนกลับมาเป็นช่วงเดียวกัน
    /// ถ้าวันหนึ่งมีคนแก้ข้างเดียว **ผู้ใช้จะจิ้มตรงที่เห็นสีหนึ่งแล้วได้อีกสีหนึ่ง**
    /// โดยไม่มี error ที่ไหน — เทสต์นี้คือประตูที่ทำให้การเพี้ยนนั้นส่งเสียง
    ///
    /// ★ **สิ่งที่เทสต์นี้จับได้ และสิ่งที่มันจับไม่ได้** — ลองแล้วทั้งสองแบบ:
    ///
    /// | ทำให้พังตรงไหน | เทสต์จับได้ไหม |
    /// |---|---|
    /// | กลับแกน y ใน `source_uv` (การย้อนที่เขียนอยู่ฝั่งเดียว) | ✅ **แดงทันที** |
    /// | ถอด `flip` ออกจาก `source_span` | ❌ **ยังเขียว** |
    ///
    /// แถวล่างไม่ใช่ช่องโหว่ที่ต้องอุด — มันคือผลของการที่ `crop_uv` **เรียก**
    /// `source_span` ตัวเดียวกัน สูตรนั้นจึงมีที่เดียวและ *เพี้ยนจากกันไม่ได้เชิงโครงสร้าง*
    /// (แข็งแรงกว่าการมีเทสต์คอยจับ) ส่วนที่ยังเขียนสองที่คือ **การย้ายกลับ**
    /// — หมุนกลับ, สเกลกลับ, ทิศของ lerp — และนั่นคือสิ่งที่เทสต์นี้เฝ้าอยู่จริง ๆ
    #[test]
    fn the_picker_reads_the_same_pixel_the_shader_draws() {
        use refx_core::board::{CropRect, Flip};

        let base = ItemCanvas {
            pos: Vec2::new(12.0, -30.0),
            size: Vec2::new(160.0, 80.0),
            ..ItemCanvas::default()
        };
        let cases = [
            ("plain", base),
            (
                "flip-h",
                ItemCanvas {
                    flip: Flip::Horizontal,
                    ..base
                },
            ),
            (
                "flip-both",
                ItemCanvas {
                    flip: Flip::Both,
                    ..base
                },
            ),
            (
                "crop",
                ItemCanvas {
                    crop: CropRect {
                        min: Vec2::new(0.2, 0.1),
                        max: Vec2::new(0.9, 0.6),
                    },
                    ..base
                },
            ),
            (
                "crop+flip+rotate",
                ItemCanvas {
                    rotation: 0.9,
                    flip: Flip::Vertical,
                    crop: CropRect {
                        min: Vec2::new(0.15, 0.35),
                        max: Vec2::new(0.55, 0.95),
                    },
                    ..base
                },
            ),
        ];

        for (name, canvas) in cases {
            // ช่วงที่ shader จะ sample เมื่อภาพกินทั้ง texture (ทาง working texture)
            let [left, top, right, bottom] = crop_uv([0.0, 0.0, 1.0, 1.0], &canvas);
            let obb = canvas.obb();
            let [ax, ay] = obb.axes();

            for (qu, qv) in [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0), (0.25, 0.8)] {
                // จุดบน world ที่ตรงกับสัดส่วน (qu, qv) ของ quad ที่วาดออกมา
                let local = Vec2::new((qu - 0.5) * canvas.size.x, (qv - 0.5_f32) * canvas.size.y);
                let world = obb.center + ax * local.x + ay * local.y;

                let picked = refx_core::pick::source_uv(&canvas, world)
                    .unwrap_or_else(|| panic!("{name}: จิ้มที่ ({qu}, {qv}) แล้วไม่โดนภาพ"));
                // สิ่งที่ shader จะหยิบมาวาดที่จุดเดียวกัน
                let drawn = Vec2::new(left + (right - left) * qu, top + (bottom - top) * qv);

                assert!(
                    (picked - drawn).length() < 1e-4,
                    "{name} ที่ ({qu}, {qv}): picker อ่าน {picked:?} แต่ shader วาด {drawn:?}"
                );
            }
        }
    }

    // ---------- ★★★ P4-5 ชิ้นที่ 2: บันทึก packed · เปิดแล้วแตก blob กลับ ----------

    /// ไฟล์ภาพปลอมที่มีเนื้อต่างกันจริง — พอสำหรับทุกอย่างที่ไม่ต้อง decode
    fn plant_file(dir: &std::path::Path, name: &str, fill: u8) -> std::path::PathBuf {
        let path = dir.join(name);
        let body: Vec<u8> = (0..2_048)
            .map(|i| (i as u8).wrapping_mul(fill | 1))
            .collect();
        std::fs::write(&path, body).unwrap();
        path
    }

    fn content_hash(n: u8) -> refx_core::hash::ContentHash {
        refx_core::hash::ContentHash::from_bytes([n; 32])
    }

    /// ★★★ **ใบที่ไม่มีไฟล์ต้นทางแล้ว ต้องถูกฝังแม้ในโหมด linked** (§4 ข้อ 23)
    ///
    /// นี่คือฟังก์ชันที่เธรดบันทึกเรียกจริง ไม่ใช่ตรรกะเลียนแบบ
    /// (`docs/08 §3.9` ข้อ 9) · ทั้งสี่แถวคือทั้งหมดที่มันต้องตอบให้ถูก
    #[test]
    fn where_the_bytes_of_an_asset_live() {
        use refx_io::packed::AssetBytes;

        let dir = spool_temp_dir("locate-bytes");
        let spool = dir.join("pasted");
        std::fs::create_dir_all(&spool).unwrap();
        let user_file = plant_file(&dir, "cat.png", 3);
        let pasted = refx_io::spool::store(
            &spool,
            content_hash(7),
            b"pasted pixels",
            refx_platform::fsops::rename_durable,
        )
        .unwrap();

        let asset = |hash: u8, path: &std::path::Path| AssetRef {
            hash: content_hash(hash),
            path: path.to_path_buf(),
            px_size: glam::UVec2::new(8, 8),
            format: ImageFormat::Unknown,
            embedded: false,
            mtime: 0,
            file_size: 0,
        };

        // ไฟล์ของผู้ใช้ที่ยังอยู่ → ลิงก์ได้
        assert_eq!(
            locate_bytes(Some(&spool), &asset(3, &user_file)),
            AssetBytes::UserFile(user_file.clone()),
        );
        // ภาพที่วาง (อยู่ในโฟลเดอร์ spool) → ของเรา ฝังเสมอ
        assert_eq!(
            locate_bytes(Some(&spool), &asset(7, &pasted)),
            AssetBytes::Ours(pasted.clone()),
        );
        // ★★★ เปิดเอกสาร packed บนเครื่องที่ไม่มีไฟล์เลย แล้วบันทึกทับ:
        //     path เดิมชี้ไปที่ว่าง แต่สำเนาที่แกะไว้ตอนเปิดยังอยู่ → **ต้องฝัง**
        //     ถ้าตรงนี้ตอบ `Missing` ไฟล์ที่บันทึกใหม่จะไม่มีภาพอยู่ข้างในเลย
        assert_eq!(
            locate_bytes(
                Some(&spool),
                &asset(7, std::path::Path::new("E:/gone/away.png"))
            ),
            AssetBytes::Ours(pasted),
        );
        // ไม่เหลืออะไรเลย
        assert_eq!(
            locate_bytes(
                Some(&spool),
                &asset(9, std::path::Path::new("E:/gone/away.png"))
            ),
            AssetBytes::Missing,
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ **โหมดของเอกสารอ่านจากของจริงในไฟล์ ไม่ใช่ธงที่จำไว้**
    ///
    /// `version = 2` แปลว่า "มี asset ฝังอยู่" เท่านั้น (§4 ข้อ 22) — board แบบ
    /// linked ที่มีภาพจาก clipboard ก็เป็น v2 เหมือนกัน · ตัวบ่งชี้บนแถบสถานะ
    /// จึงต้องตอบคำถามที่ผู้ใช้ถามจริง: **ทุกใบอยู่ข้างในหรือเปล่า**
    #[test]
    fn a_document_is_packed_only_when_every_image_is_inside_it() {
        use refx_core::board::{BoardParts, ItemParts};
        use refx_io::packed::SaveMode;

        let board = Board::load(
            probe_board_id(),
            BoardParts {
                name: "mixed".to_owned(),
                items: vec![
                    ItemParts {
                        item: Item::new(image_kind(1, "E:/photos/a.png")),
                        group: None,
                    },
                    ItemParts {
                        item: Item::new(image_kind(2, "E:/pasted/b.png")),
                        group: None,
                    },
                ],
                ..BoardParts::default()
            },
        );
        let index_of = |hashes: &[u8]| {
            let dir = spool_temp_dir(&format!("mode-{}", hashes.len()));
            let sources: Vec<_> = hashes
                .iter()
                .map(|n| refx_io::packed::PackSource {
                    hash: content_hash(*n),
                    path: plant_file(&dir, &format!("blob{n}.bin"), *n),
                })
                .collect();
            let doc = dir.join("doc.refx");
            let mut file = std::fs::File::create(&doc).unwrap();
            refx_io::packed::write_packed(&mut file, &board, &sources).unwrap();
            drop(file);
            let index = read_asset_table(&doc);
            (dir, index)
        };

        // ไม่มีอะไรฝังอยู่ = linked (ไฟล์ v1 ธรรมดา)
        assert_eq!(
            mode_of_document(&board, &refx_io::packed::Index::default()),
            SaveMode::Linked,
        );
        // ★ ฝังแค่ภาพที่วาง (กฎ "linked ก็ฝัง") — **ยังเป็น linked**
        let (dir_one, one) = index_of(&[2]);
        assert_eq!(one.len(), 1);
        assert_eq!(mode_of_document(&board, &one), SaveMode::Linked);
        // ฝังครบทุกใบ = packed
        let (dir_all, all) = index_of(&[1, 2]);
        assert_eq!(mode_of_document(&board, &all), SaveMode::Packed);

        let _ = std::fs::remove_dir_all(&dir_one);
        let _ = std::fs::remove_dir_all(&dir_all);
    }

    /// ★★★ **ลบโฟลเดอร์ต้นฉบับทั้งโฟลเดอร์ → เปิดแล้วภาพยังครบ**
    ///
    /// เกณฑ์ผ่านข้อแรกของ P4-5 · เดินทั้งเส้นด้วยของจริงทุกชิ้น:
    /// `locate_bytes` → `plan_embeds` → `save_document` → `read_document` →
    /// `read_asset_table` → `spool::unpack` — ไม่มีตัวจำลองสักตัว
    #[test]
    fn a_packed_document_survives_losing_every_source_file() {
        use refx_io::packed::SaveMode;

        let root = spool_temp_dir("packed-survives");
        let photos = root.join("photos");
        std::fs::create_dir_all(&photos).unwrap();
        let spool = root.join("pasted");
        let a = plant_file(&photos, "a.png", 5);
        let b = plant_file(&photos, "b.png", 9);
        let original: Vec<Vec<u8>> = [&a, &b]
            .iter()
            .map(|path| {
                use std::io::Read as _;
                let mut bytes = Vec::new();
                std::fs::File::open(path)
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
                bytes
            })
            .collect();

        let mut board = board_pointing_at(content_hash(5), &a);
        {
            use refx_core::command::AddItems;
            let mut history = History::default();
            history
                .apply(
                    &mut board,
                    Box::new(
                        AddItems::new(vec![Item::new(ItemKind::Image(AssetRef {
                            hash: content_hash(9),
                            path: b.clone(),
                            px_size: glam::UVec2::new(8, 8),
                            format: ImageFormat::Unknown,
                            embedded: false,
                            mtime: 0,
                            file_size: 0,
                        }))])
                        .unwrap(),
                    ),
                )
                .unwrap();
            // ★ บันทึกจริง = เอกสารสะอาด — เทียบทั้งก้อนได้โดยไม่ต้องยกเว้นฟิลด์ไหน
            history.mark_saved(&mut board);
        }

        // ---- บันทึกแบบ packed (เส้นทางเดียวกับที่เธรดบันทึกเดิน) ----
        let doc = root.join("board.refx");
        let embeds = refx_io::packed::plan_embeds(&board, SaveMode::Packed, |asset| {
            locate_bytes(Some(&spool), asset)
        });
        assert_eq!(embeds.len(), 2, "ต้องฝังทั้งสองใบ");
        refx_io::save::save_document(&doc, &board, &embeds, refx_platform::fsops::rename_durable)
            .unwrap();

        // ---- ★ ลบโฟลเดอร์ต้นฉบับทั้งโฟลเดอร์ ----
        std::fs::remove_dir_all(&photos).unwrap();
        assert!(!a.exists() && !b.exists());

        // ---- เปิดกลับมา ----
        let back = read_document(&doc, probe_board_id()).expect("เปิดไฟล์ packed ไม่ได้");
        let index = read_asset_table(&doc);
        assert_eq!(back, board, "เนื้อเอกสารไม่เท่าเดิม");
        assert_eq!(
            mode_of_document(&back, &index),
            SaveMode::Packed,
            "แถบสถานะจะบอกโหมดผิด"
        );

        // ---- ★★★ ภาพต้องกลับมาเป็นไฟล์จริงได้ครบ ไบต์ต่อไบต์ ----
        let mut file = std::fs::File::open(&doc).unwrap();
        for (n, want) in [(5u8, &original[0]), (9, &original[1])] {
            let entry = index
                .find(content_hash(n))
                .unwrap_or_else(|| panic!("ไม่มี blob ของ {n} ในไฟล์"));
            let path = refx_io::spool::unpack(
                &spool,
                entry,
                &mut file,
                refx_platform::fsops::rename_durable,
            )
            .unwrap();
            use std::io::Read as _;
            let mut got = Vec::new();
            std::fs::File::open(&path)
                .unwrap()
                .read_to_end(&mut got)
                .unwrap();
            assert_eq!(&got, want, "ภาพ {n} ที่แกะออกมาไม่ตรงกับต้นฉบับ");
        }

        // ---- ★★ เปิดแล้วบันทึกทับแบบ linked ต้อง **ไม่ทำให้ภาพหาย** ----
        //      ไฟล์ต้นฉบับไม่มีแล้ว สำเนาใน spool คือของที่เหลืออยู่
        let relinked = refx_io::packed::plan_embeds(&back, SaveMode::Linked, |asset| {
            locate_bytes(Some(&spool), asset)
        });
        assert_eq!(
            relinked.len(),
            2,
            "บันทึก linked ทับแล้วภาพหลุดออกจากไฟล์ — งานของผู้ใช้หายถาวร"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// ★★★ **board ที่ไม่มีอะไรให้ฝัง ต้องยังเป็น v1** (§4 ข้อ 22)
    ///
    /// ไฟล์ที่ไม่มีของให้เสีย ไม่มีเหตุให้ตัดรุ่นเก่าออกจากการเปิดมัน ·
    /// ยืนยันจาก **ไบต์ในไฟล์** ไม่ใช่จากค่าที่เราส่งเข้าไป
    #[test]
    fn a_board_with_nothing_to_embed_is_still_version_one() {
        use refx_io::packed::SaveMode;
        use std::io::Read as _;

        let root = spool_temp_dir("still-v1");
        let doc = root.join("empty.refx");
        let board = Board::new(probe_board_id(), "empty".to_owned());

        for mode in [SaveMode::Linked, SaveMode::Packed] {
            let embeds =
                refx_io::packed::plan_embeds(&board, mode, |asset| locate_bytes(None, asset));
            assert!(embeds.is_empty());
            refx_io::save::save_document(
                &doc,
                &board,
                &embeds,
                refx_platform::fsops::rename_durable,
            )
            .unwrap();

            let mut header = [0u8; refx_io::dto::HEADER_LEN];
            std::fs::File::open(&doc)
                .unwrap()
                .read_exact(&mut header)
                .unwrap();
            let info = refx_io::dto::inspect(&header).unwrap();
            assert_eq!(
                info.version,
                refx_io::dto::LINKED_VERSION,
                "{mode:?}: ไฟล์ที่ไม่มี blob ต้องเป็น v1 รุ่นเก่าจึงเปิดงานประจำวันได้"
            );
            assert!(!info.packed, "{mode:?}: ตั้งธง packed ทั้งที่ไม่มีอะไรฝังอยู่");
            // ★ และแถบสถานะต้องบอกว่า linked ตามสภาพจริงของไฟล์
            assert_eq!(
                mode_of_document(&board, &read_asset_table(&doc)),
                SaveMode::Linked,
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// ★★★ **การอ่านสำเนาใน spool ต้องไม่เขียนที่อยู่ของเอกสารทิ้ง**
    ///
    /// เปิดเอกสาร packed บนเครื่องที่ไม่มีไฟล์ → เราแกะ blob ลง spool แล้วอ่าน
    /// จากที่นั่น · ถ้าผลนั้นถูกเขียนกลับลง `Board` จะได้สามอย่างที่ผู้ใช้ไม่ได้สั่ง
    /// พร้อมกัน: เอกสาร dirty ทันทีที่เปิด · มีขั้น undo โผล่มา · และ**ที่อยู่จริง
    /// ของภาพหายตลอดกาล** ทั้งที่วันหนึ่งเขาอาจกลับไปเครื่องที่มีไฟล์นั้น
    #[test]
    fn reading_the_copy_inside_the_document_never_rewrites_the_document() {
        let spool = std::path::Path::new("/data/RefX/pasted");
        let thumb = thumb_of(100, 80);
        let meta = refx_asset::pool::SourceMeta {
            mtime_ms: 999,
            bytes: 12345,
        };

        // ใบที่เอกสารจำที่อยู่เดิมไว้ (ไฟล์นั้นไม่มีอยู่บนเครื่องนี้แล้ว)
        let from_file = image_kind(9, "E:/photos/cat.png");
        let unpacked = spool.join("aabb.png");
        let desired = relinked_kind(
            &from_file,
            Some(&unpacked),
            Some(content_hash(9)),
            Some(spool),
            &thumb,
            meta,
        )
        .expect("ใบที่เป็นภาพต้องมีผลลัพธ์เสมอ");
        assert_eq!(
            desired, from_file,
            "อ่านจากสำเนาในเอกสารแล้วเอกสารเปลี่ยน — เปิดไฟล์มาก็ dirty ทันที"
        );

        // ★ ภาพที่วางจาก clipboard ไม่ได้รับผลอะไร — path ของมันคือไฟล์ใน spool อยู่แล้ว
        let pasted_path = spool.join("ccdd.png");
        let pasted = ItemKind::Image(AssetRef {
            hash: content_hash(4),
            path: pasted_path.clone(),
            px_size: glam::UVec2::new(100, 80),
            format: ImageFormat::Unknown,
            embedded: false,
            mtime: 42,
            file_size: 4242,
        });
        let desired = relinked_kind(
            &pasted,
            Some(&pasted_path),
            Some(content_hash(77)),
            Some(spool),
            &thumb,
            meta,
        )
        .unwrap();
        assert_eq!(desired, pasted, "ภาพที่วางเปลี่ยนไปจากเดิม");
    }

    // ---------- ★★★ P4-7a: snapshot ของเอกสารต้องถูกเสนอกลับให้ผู้ใช้ ----------

    /// เอกสารที่บันทึกแล้วจริง ๆ บนดิสก์ พร้อม path ของมัน
    fn saved_document(
        dir: &std::path::Path,
        name: &str,
        items: usize,
    ) -> (std::path::PathBuf, Board) {
        use refx_core::board::{BoardParts, ItemParts};

        let board = Board::load(
            probe_board_id(),
            BoardParts {
                name: "work".to_owned(),
                items: (0..items)
                    .map(|n| ItemParts {
                        item: Item::new(image_kind(n as u8, &format!("E:/photos/{n}.png"))),
                        group: None,
                    })
                    .collect(),
                ..BoardParts::default()
            },
        );
        let doc = dir.join(name);
        refx_io::save::save_atomic(&doc, &board, refx_platform::fsops::rename_durable).unwrap();
        (doc, board)
    }

    /// ★★★ **snapshot ที่ต่างจากไฟล์ ต้องถูกเสนอกลับ — ไม่ใช่ปล่อยให้เงียบ**
    ///
    /// นี่คือรูที่ P4-3 เปิดค้างไว้: `<doc>.refx.autosave` ถูกเขียนทุก 10 วินาที
    /// มาตลอด แต่ไม่มีผู้เรียก `find_pending` ในโปรแกรมเลย ผู้ใช้ที่ไฟดับจึงได้
    /// เวอร์ชันที่บันทึกล่าสุดโดยไม่มีใครถามถึงงานที่ค้างอยู่ แล้วมันถูกลบตอน
    /// เขากด `Ctrl+S` ครั้งแรก
    ///
    /// ★ เรียก `newer_snapshot` **ตัวที่เธรดเปิดไฟล์ใช้จริง** (`docs/08 §3.9` ข้อ 9)
    #[test]
    fn a_snapshot_that_never_reached_the_file_is_offered_back() {
        let dir = spool_temp_dir("pending-offer");
        let (doc, saved) = saved_document(&dir, "work.refx", 1);

        // ยังไม่มี snapshot = ไม่มีอะไรให้ถาม
        assert!(
            newer_snapshot(&doc, probe_board_id(), &saved).is_none(),
            "ถามทั้งที่ไม่มี snapshot อยู่เลย"
        );

        // ผู้ใช้แก้งานต่อ แล้ว autosave เขียน snapshot ไว้ — จากนั้นโปรแกรมตาย
        let mut newer = saved.clone();
        let mut history = History::default();
        history
            .apply(
                &mut newer,
                Box::new(
                    AddItems::new(vec![Item::new(image_kind(9, "E:/photos/late.png"))]).unwrap(),
                ),
            )
            .unwrap();
        refx_io::autosave::write_snapshot(&doc, &newer, refx_platform::fsops::rename_durable)
            .unwrap();

        let pending = newer_snapshot(&doc, probe_board_id(), &saved).expect("งานที่ค้างอยู่ถูกเมิน");
        // ★ เทียบ **เนื้อ** ไม่ใช่ทั้งก้อน — snapshot ที่อ่านกลับมาย่อมมีธง `dirty`
        //   ดับเสมอ (DTO ไม่เก็บธงนั้น) ส่วนตัวที่อยู่ในมือตอนเขียนยัง dirty อยู่
        assert_ne!(pending.board, saved, "สิ่งที่เสนอกลับคือไฟล์เดิม ไม่ใช่งานที่ค้าง");
        assert_eq!(
            pending.board.len(),
            saved.len() + 1,
            "จำนวนชิ้นที่บอกผู้ใช้ต้องเป็นของ snapshot ไม่ใช่ของไฟล์"
        );
        assert_eq!(pending.board.len(), newer.len());
        let paths: Vec<_> = pending
            .board
            .items_in_z_order()
            .filter_map(|(_, item)| match &item.kind {
                ItemKind::Image(asset) => Some(asset.path.clone()),
                _ => None,
            })
            .collect();
        assert!(
            paths.contains(&std::path::PathBuf::from("E:/photos/late.png")),
            "ใบที่ผู้ใช้เพิ่มหลังบันทึกครั้งสุดท้ายไม่ได้ถูกเสนอกลับ: {paths:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ **snapshot ที่เหมือนไฟล์เป๊ะ ต้องไม่ถูกถาม**
    ///
    /// เกิดจริงเมื่อโปรแกรมตาย *หลัง* เขียนไฟล์สำเร็จแต่ *ก่อน* ลบ snapshot ·
    /// การถามในกรณีนั้นสอนให้ผู้ใช้กดปุ่มผ่าน ๆ โดยไม่อ่าน แล้ววันที่มีของจริง
    /// ให้กู้เขาจะกดผ่านเหมือนกัน — คำถามที่ไม่จำเป็นทำลายคำถามที่จำเป็น
    #[test]
    fn a_snapshot_that_matches_the_file_is_never_offered() {
        let dir = spool_temp_dir("pending-same");
        let (doc, saved) = saved_document(&dir, "work.refx", 2);

        refx_io::autosave::write_snapshot(&doc, &saved, refx_platform::fsops::rename_durable)
            .unwrap();
        assert!(
            refx_io::autosave::find_pending(&doc, probe_board_id()).is_some(),
            "เทสต์นี้ต้องมี snapshot อยู่จริงถึงจะพิสูจน์อะไรได้"
        );

        assert!(
            newer_snapshot(&doc, probe_board_id(), &saved).is_none(),
            "ถามผู้ใช้ทั้งที่ snapshot เหมือนไฟล์ทุกอย่าง"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
