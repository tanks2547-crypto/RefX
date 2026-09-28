//! ★★★ ภาพที่เพิ่งลากเข้ามา **ไปลงที่ไหน และใหญ่เท่าไหร่** (ROADMAP P5-9b)
//!
//! # ทำไมต้องมีโมดูลนี้
//!
//! ตั้งแต่ P1 ภาพทุกใบถูกย่อให้ด้านยาว **128 หน่วย** แล้ววางเป็นตาราง 16 ช่อง
//! ช่องละ 160 ที่พิกัดตายตัว (2000, 2000) — คอมเมนต์เขียนว่า *"ไปก่อน — layout
//! จริงมาใน P2/P3"* แล้วไม่มีใครกลับมา · เจ้าของโปรเจกต์ลอง rc.1 (27 ก.ย. 2026)
//! แล้วเห็นภาพ ~30 px ที่ซูม 25% · snapshot ของการลากจริงของเขายืนยัน:
//! 2560×3712 → **88 × 128** · 3034×4156 → **93 × 128** · 4000×6641 → **77 × 128**
//!
//! # กติกา
//!
//! | | |
//! |---|---|
//! | ขนาด | **1 หน่วย world = 1 พิกเซลของภาพ ตอนวาง** — 2400 px ที่ซูม 25% = 600 px บนจอ |
//! | ที่วาง | **ในบริเวณที่มองเห็นอยู่** — ไหลซ้าย → ขวาจากมุมบนซ้าย ขึ้นแถวใหม่ที่ขอบขวา |
//! | กล้อง | **ไม่แตะ** — "การขยับมุมมองที่ผู้ใช้ไม่ได้ขอ คือการแย่งงานเขา" (`docs/02 §2.9`) |
//!
//! ★ `docs/02 §2.1` บอกว่า `ItemCanvas.size` **ไม่ใช่** ขนาดพิกเซลต้นฉบับ — ถูก:
//!   มันเป็นอิสระจากกันหลังวาง (ผู้ใช้ย่อ/ขยายได้) · สเปกไม่ได้บอกว่า **เริ่มที่เท่าไหร่**
//!   → ตัดสินที่นี่ว่าเริ่มที่ 1:1 ตามที่เจ้าของโปรเจกต์คาด และตามที่โปรแกรมกลุ่มเดียวกันทำ
//!
//! # ★★ การไหลต่อเนื่องได้ตราบที่กล้องยังไม่ขยับ
//!
//! ลากชุดที่สองเข้ามาขณะชุดแรกยัง decode อยู่ ต้องไม่ทับชุดแรก → [`Flow`] ไม่ได้
//! ถูกรีเซ็ตต่อการลากหนึ่งครั้ง แต่ **ผูกกับกล้อง ณ ตอนที่มันเริ่ม** · ผู้ใช้ pan/zoom
//! เมื่อไหร่ ภาพใบถัดไปเริ่มไหลใหม่ในบริเวณที่เห็นตอนนั้น — ซึ่งคือกติกาเดียวกัน
//! ("ลงในบริเวณที่เห็น") ใช้กับมุมมองใหม่
//!
//! ★ โมดูลนี้ไม่แตะ GPU ไม่แตะ `Board` — คืนแค่ตำแหน่ง/ขนาด ให้ผู้เรียกห่อเป็น
//!   `AddItems` เอง (I-3 · ทุกการเพิ่มผ่าน `Command`)

use refx_core::geom::Rect;
use refx_core::glam::Vec2;
use refx_core::layout::MAX_SIDE;

/// สัดส่วนของขอบรอบบริเวณที่เห็น — ภาพไม่ชิดขอบจอ
const MARGIN: f32 = 0.05;
/// ช่องว่างระหว่างภาพ เทียบกับความกว้างของบริเวณที่เห็น
const GAP: f32 = 0.02;
/// กรอบของภาพที่เปิดไม่ได้ (`Missing`) กว้างเท่านี้ของบริเวณที่เห็น
///
/// ★ ไม่รู้ขนาดจริงเพราะอ่านหัวไฟล์ไม่ผ่าน · ขนาดตายตัวในหน่วย world จะเล็กจนอ่านป้าย
///   ไม่ออกตอนซูมออก และใหญ่จนบังทั้งจอตอนซูมเข้า → ผูกกับสิ่งที่ผู้ใช้เห็นอยู่
const MISSING_WIDTH: f32 = 0.2;

/// กล้อง ณ ตอนที่การไหลเริ่ม — เปลี่ยนเมื่อไหร่ = เริ่มไหลใหม่
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewKey {
    center: Vec2,
    zoom: f32,
}

impl ViewKey {
    /// กล้องหนึ่งจังหวะ
    #[must_use]
    pub fn new(center: Vec2, zoom: f32) -> Self {
        Self { center, zoom }
    }
}

/// การไหลของภาพใหม่ในบริเวณที่เห็นหนึ่งบริเวณ
#[derive(Debug, Clone, PartialEq)]
pub struct Flow {
    key: ViewKey,
    view: Rect,
    /// มุมบนซ้ายของช่องถัดไป
    next: Vec2,
    /// ขอบล่างของแถวปัจจุบัน — แถวถัดไปเริ่มใต้ตรงนี้
    row_bottom: f32,
    /// แถวปัจจุบันมีภาพแล้วหรือยัง — ภาพแรกของแถวไม่ถูกดันขึ้นแถวใหม่แม้จะกว้างกว่าจอ
    row_used: bool,
}

impl Flow {
    /// เริ่มไหลที่มุมบนซ้ายของบริเวณที่เห็น
    #[must_use]
    pub fn new(key: ViewKey, view: Rect) -> Self {
        let size = view.size();
        let margin = size.x.min(size.y) * MARGIN;
        let start = view.min + Vec2::splat(margin);
        Self {
            key,
            view,
            next: start,
            row_bottom: start.y,
            row_used: false,
        }
    }

    /// ยังเป็นการไหลของกล้องตัวนี้อยู่ไหม
    #[must_use]
    pub fn belongs_to(&self, key: ViewKey) -> bool {
        self.key == key
    }

    /// ★ ที่วางของภาพขนาด `w × h` พิกเซล — คืน **(จุดกึ่งกลาง, ขนาด)** ในหน่วย world
    ///
    /// จุดกึ่งกลางเพราะ `ItemCanvas::pos` คือกึ่งกลาง (`docs/02 §2.1`)
    pub fn place_image(&mut self, w: u32, h: u32) -> (Vec2, Vec2) {
        // 1:1 · กันศูนย์ (ไฟล์ที่ประกาศขนาด 0) และกันใหญ่เกินเพดานของ layout
        #[expect(
            clippy::cast_precision_loss,
            reason = "ขนาดภาพถูกจำกัดด้วยเพดาน decode ซึ่งต่ำกว่า 2^24 มาก — f32 แทนได้ตรง"
        )]
        let size = Vec2::new(w.max(1) as f32, h.max(1) as f32).min(Vec2::splat(MAX_SIDE));
        self.take(size)
    }

    /// ที่วางของภาพที่เปิดไม่ได้ — ไม่รู้ขนาดจริง ใช้กรอบ 4:3 ตามสัดส่วนของบริเวณที่เห็น
    pub fn place_missing(&mut self) -> (Vec2, Vec2) {
        let width = (self.view.size().x * MISSING_WIDTH).max(1.0);
        self.take(Vec2::new(width, width * 0.75))
    }

    fn take(&mut self, size: Vec2) -> (Vec2, Vec2) {
        let view = self.view.size();
        let margin = view.x.min(view.y) * MARGIN;
        let gap = view.x * GAP;
        let right = self.view.max.x - margin;

        // ขึ้นแถวใหม่ถ้าล้นขอบขวา — ยกเว้นภาพแรกของแถว (ภาพที่กว้างกว่าจอเอง
        // ขึ้นแถวใหม่ไปก็ยังล้นอยู่ดี และจะทิ้งแถวว่างไว้ข้างบน)
        if self.row_used && self.next.x + size.x > right {
            self.next = Vec2::new(self.view.min.x + margin, self.row_bottom + gap);
            self.row_used = false;
        }
        let top_left = self.next;
        self.next.x += size.x + gap;
        self.row_bottom = self.row_bottom.max(top_left.y + size.y);
        self.row_used = true;
        (top_left + size * 0.5, size)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::float_cmp)]

    use super::*;

    fn view(center: Vec2, zoom: f32) -> (ViewKey, Rect) {
        // หน้าต่าง 1280×800 เหมือนภาพหน้าจอที่ใช้วัด
        (
            ViewKey::new(center, zoom),
            Rect::from_center_size(center, Vec2::new(1280.0, 800.0) / zoom),
        )
    }

    /// ★★★ **ภาพของจริงของเจ้าของโปรเจกต์** — ต้องได้ขนาดพิกเซลของมัน ไม่ใช่ 128
    ///
    /// ตัวเลขจาก snapshot การลากจริง 27 ก.ย. 2026 (กล้องปริยาย ซูม 0.25)
    #[test]
    fn the_images_from_the_real_drag_land_at_their_own_pixel_size() {
        let (key, rect) = view(Vec2::splat(2000.0), 0.25);
        let mut flow = Flow::new(key, rect);
        for (w, h) in [(2560, 3712), (3034, 4156), (4000, 6641)] {
            let (_, size) = flow.place_image(w, h);
            #[expect(clippy::cast_precision_loss, reason = "ขนาดทดสอบเล็ก")]
            let want = Vec2::new(w as f32, h as f32);
            assert_eq!(size, want, "{w}×{h} ถูกย่อ/ขยาย — ควรเป็น 1:1");
            // ที่ซูม 25% ด้านยาวบนจอต้องเป็นหลักร้อยพิกเซล ไม่ใช่ ~30
            assert!(size.y * 0.25 > 500.0, "{w}×{h} บนจอยังเล็กเกินไป");
        }
    }

    /// ★★ ภาพแรกต้อง **อยู่ในบริเวณที่เห็น** — ที่ไหนก็ได้บนโลก ไม่ใช่ที่ 2000 เสมอ
    #[test]
    fn the_first_image_lands_inside_what_the_user_is_looking_at() {
        for (center, zoom) in [
            (Vec2::splat(2000.0), 0.25),
            (Vec2::new(-50_000.0, 7_000.0), 1.0),
            (Vec2::ZERO, 4.0),
        ] {
            let (key, rect) = view(center, zoom);
            let mut flow = Flow::new(key, rect);
            let (pos, size) = flow.place_image(100, 80);
            let item = Rect::from_center_size(pos, size);
            assert!(
                item.min.x >= rect.min.x
                    && item.min.y >= rect.min.y
                    && item.max.x <= rect.max.x
                    && item.max.y <= rect.max.y,
                "ภาพเล็กไปลงนอกบริเวณที่เห็น: {item:?} ไม่อยู่ใน {rect:?}"
            );
        }
    }

    /// ★★ ภาพในชุดเดียวกัน **ไม่ทับกัน** — ทั้งในแถวเดียวกันและตอนขึ้นแถวใหม่
    #[test]
    fn images_in_one_flow_never_overlap() {
        let (key, rect) = view(Vec2::ZERO, 0.25);
        let mut flow = Flow::new(key, rect);
        let placed: Vec<Rect> = [
            (2400, 1600),
            (1200, 3000),
            (800, 800),
            (5000, 400),
            (300, 300),
            (4000, 6641),
        ]
        .iter()
        .map(|&(w, h)| {
            let (pos, size) = flow.place_image(w, h);
            Rect::from_center_size(pos, size)
        })
        .collect();
        for (i, a) in placed.iter().enumerate() {
            for b in &placed[i + 1..] {
                let overlap = a.min.x < b.max.x
                    && b.min.x < a.max.x
                    && a.min.y < b.max.y
                    && b.min.y < a.max.y;
                assert!(!overlap, "ภาพทับกัน: {a:?} กับ {b:?}");
            }
        }
    }

    /// ★ ภาพที่กว้างกว่าจอเองไม่ทิ้งแถวว่างไว้ข้างบน
    #[test]
    fn an_image_wider_than_the_view_starts_its_own_row_without_an_empty_one_above() {
        let (key, rect) = view(Vec2::ZERO, 1.0);
        let mut flow = Flow::new(key, rect);
        let (first, _) = flow.place_image(5000, 100);
        let top = rect.min.y + 800.0f32.min(1280.0) * MARGIN;
        assert_eq!(first.y - 50.0, top, "ภาพแรกถูกดันลงแถวถัดไป");
    }

    /// ★ `Missing` ผูกกับบริเวณที่เห็น — อ่านป้ายออกได้ทุกระดับซูม
    #[test]
    fn a_missing_image_frame_scales_with_the_view() {
        for zoom in [0.05, 1.0, 16.0] {
            let (key, rect) = view(Vec2::ZERO, zoom);
            let mut flow = Flow::new(key, rect);
            let (_, size) = flow.place_missing();
            let on_screen = size.x * zoom;
            assert!(
                (on_screen - 1280.0 * MISSING_WIDTH).abs() < 0.5,
                "ซูม {zoom}: กรอบ Missing กว้าง {on_screen} px บนจอ"
            );
        }
    }

    /// ★ การไหลรู้ว่ากล้องขยับแล้ว — ผู้เรียกใช้ตัวนี้ตัดสินว่าจะเริ่มไหลใหม่
    #[test]
    fn a_flow_knows_the_camera_it_started_with() {
        let (key, rect) = view(Vec2::ZERO, 1.0);
        let flow = Flow::new(key, rect);
        assert!(flow.belongs_to(ViewKey::new(Vec2::ZERO, 1.0)));
        assert!(!flow.belongs_to(ViewKey::new(Vec2::new(1.0, 0.0), 1.0)));
        assert!(!flow.belongs_to(ViewKey::new(Vec2::ZERO, 0.5)));
    }
}
