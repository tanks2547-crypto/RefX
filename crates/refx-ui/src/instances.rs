//! `Board` → `QuadInstance` — **จุดเดียวที่เรขาคณิตของเอกสารกลายเป็นของที่ GPU วาดได้**
//!
//! ★ ย้ายออกมาจาก `impl RefxApp` ตอน P5-1 ด้วยเหตุผลสองข้อ:
//!
//! 1. มันเป็น **ฟังก์ชันบริสุทธิ์** ที่ไม่แตะสถานะของแอปเลย — การเป็น associated
//!    fn ของ `RefxApp` ทำให้มันดูเหมือนต้องมีแอปอยู่ถึงจะเรียกได้ ทั้งที่ไม่ใช่
//! 2. `docs/08 §2` แถว `build_instances_1000` ต้องวัดมันได้จาก `benches/`
//!    ซึ่งเห็นเฉพาะ API สาธารณะ · เกณฑ์ที่วัดไม่ได้คือเกณฑ์ที่ไม่มีอยู่จริง
//!
//! ★★ **รับ `slot`/`tint` เข้ามาแทนที่จะรับสถานะการวาดทั้งก้อน** — สองฟิลด์นี้
//! คือทั้งหมดที่การแปลงต้องใช้ · ผลพลอยได้คือชั้นนี้ไม่ต้องรู้จัก `Thumbnail`,
//! `JobSource` หรือ cache ใด ๆ ซึ่งเป็นเหตุผลที่มันวัดได้โดยไม่ต้องมี GPU

use glam::Vec2;
use refx_core::board::{Board, ItemCanvas, ItemKind};
use refx_render::atlas::AtlasSlot;
use refx_render::instance::QuadInstance;

/// หด uv ของช่องใน atlas ลงตามกรอบ crop (P2-7)
///
/// ★ `crop` เป็นสัดส่วน **ของภาพต้นฉบับ** (docs/02 §2.1) ส่วน `slot` คือช่องที่ภาพนั้น
/// อยู่ใน atlas — จึงต้อง lerp กรอบ crop ลงในช่วงของช่อง ไม่ใช่เอาไปใช้ตรง ๆ
/// ถ้าใช้ตรง ๆ ภาพทุกใบจะไปสุ่มหยิบ pixel ของภาพอื่นในชั้นเดียวกันมาแสดง
///
/// ★★ **ช่วงของภาพต้นฉบับมาจาก `refx_core::pick::source_span` ที่เดียว** (P2-10)
/// ที่นี่ทำหน้าที่เดียวคือ lerp ช่วงนั้นลงในช่องของ atlas
///
/// เดิมสูตร crop+flip ถูกเขียนไว้ตรงนี้ และ picker ต้องเดินย้อนทางเดียวกัน
/// ถ้าปล่อยให้เขียนคนละที่ วันที่มีคนแก้ข้างเดียว **ผู้ใช้จะจิ้มตรงที่เห็นสีหนึ่ง
/// แล้วได้อีกสีหนึ่ง** โดยไม่มี error ที่ไหนเลย — เป็นรูปแบบเดียวกับบั๊ก `flip`
/// ที่ไม่ถึงทาง working texture ตอน P2-8 เป๊ะ ๆ
#[must_use]
pub fn crop_uv(slot: [f32; 4], canvas: &ItemCanvas) -> [f32; 4] {
    let [u0, v0, u1, v1] = slot;
    let [left, top, right, bottom] = refx_core::pick::source_span(canvas);
    [
        u0 + (u1 - u0) * left,
        v0 + (v1 - v0) * top,
        u0 + (u1 - u0) * right,
        v0 + (v1 - v0) * bottom,
    ]
}

/// แปลง item หนึ่งใบเป็น instance ที่ GPU วาดได้
///
/// ★ **จุดเดียวที่เรขาคณิตของ `Board` กลายเป็น `QuadInstance`**
/// `transform` ใช้มุมซ้ายบน (unit quad คือ 0..1) ส่วน `ItemCanvas::pos` คือจุดกึ่งกลาง
/// จึงต้องลบครึ่งขนาดออก — ถ้าทำผิดตรงนี้ภาพทุกใบจะเลื่อนไปครึ่งตัว
///
/// affine เก็บแบบ **column-major** ตาม `apply_affine` ใน `quad.wgsl`:
/// `(a, b)` คือภาพของแกน x ของ unit quad, `(c, d)` คือภาพของแกน y
/// ซึ่งตรงกับ `Obb::axes()` พอดี — hit-test กับสิ่งที่วาดจึงใช้นิยามเดียวกัน
/// (ถ้าสองที่นี้ไม่ตรงกัน ภาพที่หมุนจะกดไม่โดนที่ที่ตาเห็น)
/// ★ คืน `None` = **ไม่วาดใบนี้** — `visible` เป็นฟิลด์เดียวที่ตัดสินแบบนั้น
///
/// เดิม `visible` ถูกเคารพที่ hit-test (`spatial.rs`) และที่กรอบเลือก
/// แต่ **ไม่ถูกเคารพตอนวาด quad** — ภาพที่ซ่อนไว้จะยังขึ้นจอโดยกดไม่โดน
/// ยังไม่มีใครตั้ง `visible = false` ได้ในวันนี้ แต่ `.refx` จะพามันมาตอน P4-1
/// (พบตอนกวาด audit ฟิลด์ → shader 4 ส.ค. 2026)
///
/// `slot` = ช่องใน atlas (`None` = ยังไม่ได้อยู่บน GPU → วาด placeholder ด้วย `tint`)
#[must_use]
pub fn quad_for(
    canvas: &ItemCanvas,
    slot: Option<AtlasSlot>,
    tint: [f32; 4],
) -> Option<QuadInstance> {
    if !canvas.visible {
        return None;
    }
    let [x_axis, y_axis] = canvas.obb().axes();
    let (a, b) = (x_axis * canvas.size.x).into();
    let (c, d) = (y_axis * canvas.size.y).into();
    // จุดกึ่งกลาง → มุมซ้ายบนของ quad **หลังหมุนแล้ว**
    let origin = canvas.pos - (Vec2::new(a, b) + Vec2::new(c, d)) * 0.5;
    // ★ ไม่มีช่องใน atlas = วาดสี่เหลี่ยมสีเด่นแทน **ห้ามข้ามไม่วาด** (docs/04 §8)
    //   ผู้ใช้ต้องเห็นว่า layout ยังอยู่ครบ ไม่ใช่ช่องว่างที่อ่านได้ว่า "ภาพหาย"
    let (uv_rect, layer, mut tint, mut flags) = match slot {
        Some(slot) => (crop_uv(slot.uv_rect(), canvas), slot.layer, [1.0; 4], 0),
        None => (
            [0.0, 0.0, 1.0, 1.0],
            0,
            tint,
            refx_render::instance::flags::PLACEHOLDER,
        ),
    };

    // ★ opacity คูณลงช่อง alpha ของ tint — pipeline เปิด alpha blending ไว้แล้ว
    //   (`BlendState::ALPHA_BLENDING`) ภาพโปร่งซ้อนกันจึงผสมตามลำดับ z ที่วาด
    tint[3] *= canvas.opacity.clamp(0.0, 1.0);

    // filter ที่คำนวณใน shader — ไม่แตะ texture เลยแม้แต่ไบต์เดียว (docs/04 §3)
    let filter = canvas.filter.sanitized();
    if filter.grayscale {
        flags |= refx_render::instance::flags::GRAYSCALE;
    }
    if filter.invert {
        flags |= refx_render::instance::flags::INVERT;
    }

    Some(QuadInstance {
        transform: [a, b, c, d, origin.x, origin.y],
        uv_rect,
        // ★ tint เป็นไบต์แล้ว (docs/04 §3.5) — ปลายทาง framebuffer 8 บิตต่อช่อง
        //   ความละเอียดที่หายไปมองไม่เห็น แต่ที่ที่ได้คืนมาเลี้ยง brightness/contrast
        //   ให้เป็น f32 เต็มได้ ซึ่ง**เห็นความต่างจริง**บนสไลเดอร์
        tint: refx_render::instance::pack_tint(tint),
        layer,
        flags,
        adjust: [filter.brightness, filter.contrast],
        reserved: 0,
    })
}

/// สร้าง instance ทั้งชุดของ board — **ประตูเดียวที่ `gfx.quads` ถูกเขียน**
///
/// ★★★ **`Board` เป็นคนบอกว่า item นี้เป็นภาพหรือไม่ ไม่ใช่สถานะการวาด**
/// (`HANDOFF §4` ข้อ 28)
///
/// สถานะการวาดเป็น cache ที่อยู่ยาวกว่าสถานะของ item **โดยตั้งใจ** (ช่อง atlas
/// ต้องรอด undo/redo ของ "ลบภาพ") · พอ relink ทำให้ item กลายเป็น `Missing`
/// ช่องเก่าจึงยังอยู่ · ถ้าไม่ถามชนิดจาก board ตรงนี้ **undo ของ relink จะคืน
/// `Missing` ใน board แต่จอยังโชว์ภาพเดิม** (เห็นจริงตอนยืนยัน P4-6)
///
/// ★ กฎข้อนี้อยู่ **ในนี้** ไม่ใช่ในตัวเรียก — ถ้าอยู่ในตัวเรียก ทุกตัวเรียกใหม่
/// ต้องจำให้ได้เอง แล้ววันหนึ่งจะมีตัวที่ลืม (บทเรียนซ้ำของโปรเจกต์นี้)
///
/// `state_of` ตอบว่า item นี้มีสถานะการวาดไหม และถ้ามี ช่อง atlas กับสีเด่นคืออะไร
pub fn build_instances(
    board: &Board,
    out: &mut Vec<QuadInstance>,
    state_of: impl Fn(refx_core::arena::ItemId) -> Option<(Option<AtlasSlot>, [f32; 4])>,
) {
    out.clear();
    for (id, item) in board.items_in_z_order() {
        if !matches!(item.kind, ItemKind::Image(_)) {
            continue;
        }
        if let Some((slot, tint)) = state_of(id)
            && let Some(quad) = quad_for(&item.canvas, slot, tint)
        {
            out.push(quad);
        }
    }
}
