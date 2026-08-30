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
        let (slot, tint) = match &item.kind {
            ItemKind::Image(_) => match state_of(id) {
                Some(state) => state,
                None => continue, // ยังไม่มีพิกเซล — ใบนี้ยังไม่มีอะไรให้วาด
            },
            // ★★★ **ใบที่เปิดไม่ได้ต้องเห็นบนจอ** (ROADMAP P3-3 · ตัดสิน 28 ส.ค. 2026)
            //
            // เดิมที่นี่ข้าม `Missing` ทิ้งไปเลย ผลคือภาพที่หาไฟล์ไม่เจอ
            // **มองไม่เห็นบนแคนวาสเลยสักใบ** — มีแต่ตัวเลขบนแถบสถานะกับช่อง
            // inspector ที่ต้องเลือกใบนั้นให้ได้ก่อนถึงจะเห็น (แล้วจะเลือกของที่
            // มองไม่เห็นได้ยังไง) · ผู้ใช้ลาก 20 ใบต้องเห็น 20 ใบ จะเป็นภาพหรือ
            // ช่องว่างก็ได้ แต่ห้ามหาย
            //
            // ★★ **บังคับ `slot = None` เสมอ ห้ามถาม `state_of`** — `render_state`
            // เป็น cache ที่อยู่ยาวกว่าสถานะของ item โดยตั้งใจ (§4 ข้อ 28) ใบที่
            // เพิ่งกลายเป็น `Missing` จาก undo ของ relink ยังถือช่อง atlas เดิมอยู่
            // ถ้าหยิบมาใช้ จอจะโชว์ภาพเก่าของใบที่เอกสารบอกว่าหาไม่เจอแล้ว
            ItemKind::Missing { .. } => (None, MISSING_TINT),
            // โน้ตข้อความวาดด้วย egui ไม่ใช่ quad
            ItemKind::Text(_) => continue,
        };
        if let Some(quad) = quad_for(&item.canvas, slot, tint) {
            out.push(quad);
        }
    }
}

/// สีของช่องว่างที่แทนภาพซึ่งเปิดไม่ได้/หาไม่เจอ
///
/// ★ ต้องต่างจาก placeholder ของภาพที่ **กำลังโหลด** ซึ่งใช้สีเด่นของภาพเอง —
/// ผู้ใช้ต้องแยก "เดี๋ยวก็มา" ออกจาก "ใบนี้มีปัญหา" ได้โดยไม่ต้องอ่านอะไร
/// · เลือกโทนแดงหม่นเพราะอ่านว่า "มีอะไรผิด" โดยไม่ตะโกนเท่าสีแดงสด
const MISSING_TINT: [f32; 4] = [0.45, 0.28, 0.28, 1.0];

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use refx_core::arena::{ArenaKey as _, BoardId, ItemId};
    use refx_core::board::{
        AssetRef, BoardParts, ImageFormat, Item, ItemParts, MissingReason, TextNote,
    };
    use refx_core::hash::ContentHash;
    use refx_render::instance::flags;

    fn slot() -> AtlasSlot {
        AtlasSlot { layer: 3, index: 7 }
    }

    fn image_item() -> Item {
        Item::new(ItemKind::Image(AssetRef {
            hash: ContentHash::from_bytes([1; 32]),
            path: std::path::PathBuf::from("a.png"),
            px_size: glam::UVec2::new(64, 64),
            format: ImageFormat::Unknown,
            embedded: false,
            mtime: 0,
            file_size: 0,
        }))
        .at(Vec2::new(100.0, 100.0), Vec2::new(80.0, 60.0))
    }

    fn missing_item() -> Item {
        Item::new(ItemKind::Missing {
            original_path: std::path::PathBuf::from("gone.png"),
            reason: MissingReason::Damaged,
        })
        .at(Vec2::new(300.0, 100.0), Vec2::new(80.0, 60.0))
    }

    fn board_of(items: Vec<Item>) -> Board {
        Board::load(
            BoardId::from_parts(0, 0),
            BoardParts {
                items: items
                    .into_iter()
                    .map(|item| ItemParts { item, group: None })
                    .collect(),
                ..BoardParts::default()
            },
        )
    }

    /// ★★★ **ใบที่เปิดไม่ได้ต้องมี quad ของตัวเอง** (ROADMAP P3-3)
    ///
    /// ก่อนหน้านี้ `Missing` ถูกข้ามทิ้ง ผู้ใช้จึงเห็นแค่ช่องว่างบนกระดาน
    /// แล้วอ่านได้อย่างเดียวว่าโปรแกรมทำภาพหาย
    #[test]
    fn an_image_that_cannot_be_opened_still_takes_up_space_on_the_board() {
        let board = board_of(vec![image_item(), missing_item()]);
        let mut out = Vec::new();
        build_instances(&board, &mut out, |_| Some((Some(slot()), [1.0; 4])));

        assert_eq!(out.len(), 2, "ใบที่เปิดไม่ได้หายไปจากจอ");
        let missing = out[1];
        assert!(
            missing.flags & flags::PLACEHOLDER != 0,
            "ใบที่เปิดไม่ได้ต้องวาดเป็นช่องว่าง ไม่ใช่ภาพ"
        );
    }

    /// ★★★ **negative control ของ `HANDOFF §4` ข้อ 28**
    ///
    /// `render_state` อยู่ยาวกว่าสถานะของ item โดยตั้งใจ — ใบที่เพิ่งกลายเป็น
    /// `Missing` จาก undo ของ relink **ยังถือช่อง atlas เดิมอยู่** ถ้า
    /// `build_instances` หยิบช่องนั้นมาใช้ จอจะโชว์ภาพเก่าของใบที่เอกสารบอกว่า
    /// หาไม่เจอแล้ว ซึ่งเป็นบั๊กที่เคยเกิดจริงตอน P4-6 และเทสต์ตอนนั้นจับไม่ได้
    #[test]
    fn a_missing_item_never_reuses_the_atlas_slot_it_used_to_have() {
        let board = board_of(vec![missing_item()]);
        let mut out = Vec::new();
        // `state_of` ตอบว่ายังมีช่องอยู่ — เหมือนสภาพจริงหลัง undo ของ relink
        build_instances(&board, &mut out, |_| Some((Some(slot()), [1.0; 4])));

        assert_eq!(out.len(), 1);
        assert!(
            out[0].flags & flags::PLACEHOLDER != 0,
            "หยิบช่อง atlas ของภาพเก่ามาวาดให้ใบที่หาไฟล์ไม่เจอ"
        );
        assert_eq!(out[0].layer, 0, "ยังชี้ layer ของช่องเดิมอยู่");
    }

    /// โน้ตข้อความไม่ใช่ quad — egui เป็นคนวาด
    #[test]
    fn a_text_note_is_not_drawn_as_a_quad() {
        let board = board_of(vec![Item::new(ItemKind::Text(TextNote {
            text: "x".to_owned(),
        }))]);
        let mut out = Vec::new();
        build_instances(&board, &mut out, |_| Some((Some(slot()), [1.0; 4])));
        assert!(out.is_empty());
    }

    /// ★ ใบที่ยังไม่มีพิกเซลเลย (ยังโหลดไม่เสร็จ) ต้องไม่ถูกวาด — ต่างจาก
    /// `Missing` ซึ่งจบแล้วและต้องเห็น
    #[test]
    fn an_image_still_loading_is_not_drawn_yet() {
        let board = board_of(vec![image_item()]);
        let mut out = Vec::new();
        build_instances(&board, &mut out, |_: ItemId| None);
        assert!(out.is_empty());
    }
}
