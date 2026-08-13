//! `Camera` — pan / zoom ของ viewport
//!
//! อยู่ใน `refx-core` เพราะเป็นคณิตศาสตร์ล้วน ไม่มี GPU ไม่มี OS
//! ทดสอบได้ครบโดยไม่ต้องเปิดหน้าต่าง — นี่คือตัวชี้วัดว่าแยกชั้นถูก
//!
//! ระบบพิกัด:
//!   * **world** — หน่วยพิกเซลที่ zoom = 1, origin ซ้ายบน, y ชี้ลง (เหมือนโปรแกรมวาด)
//!   * **screen** — พิกเซลจริงบนหน้าต่าง, origin ซ้ายบน, y ชี้ลง
//!
//! spec: docs/02-data-model.md, ROADMAP P0-7

use glam::Vec2;

/// โหมดการทำงาน — **สอง view บน document ก้อนเดียวกัน ไม่ใช่สองโปรแกรม**
///
/// สลับไปมาไม่ทำลายข้อมูลอีกฝั่ง เพราะ `ItemCanvas` กับ `ItemMeta` อยู่คู่กันเสมอ
/// (ARCHITECTURE §4)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// ระนาบอิสระ วางทับกันได้ — จัด mood board, เทียบสัดส่วน
    #[default]
    Canvas,
    /// ตารางจัดอัตโนมัติ — คัดภาพ ติดแท็ก หาไฟล์
    Arrange,
}

impl Mode {
    /// ชื่อที่แสดงบนปุ่มสลับโหมด
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Canvas => "Canvas",
            Self::Arrange => "Arrange",
        }
    }

    /// ค่าที่ลงไฟล์ — **ตัวเลขพวกนี้เป็นสัญญาถาวร ห้ามสลับ**
    #[must_use]
    pub fn to_wire(self) -> u8 {
        match self {
            Self::Canvas => 0,
            Self::Arrange => 1,
        }
    }

    /// อ่านค่าจากไฟล์ — ★ **ตกกลับเป็น `Canvas` โดยตั้งใจ ไม่ถือค่าดิบไว้**
    ///
    /// ต่างจาก `ColorLabel`/`Flip`/`MissingReason` ที่ต้อง round-trip เพราะ
    /// **ไม่มีข้อมูลของผู้ใช้หาย**: โหมดที่เปิดค้างไว้เป็นมุมมอง ไม่ใช่เนื้อหา
    /// เปิดไฟล์มาแล้วอยู่โหมดที่คุ้นเคยแทนโหมดที่ไม่รู้จัก คือสิ่งที่ถูกต้องกว่า
    /// (`docs/02 §2.2b` เซ็นรับรองข้อนี้ไว้แล้ว)
    #[must_use]
    pub fn from_wire(value: u8) -> Self {
        match value {
            1 => Self::Arrange,
            _ => Self::Canvas,
        }
    }
}

/// กล้อง 2 มิติ
///
/// `center` คือจุดใน world ที่ถูกวางไว้ **กลางจอพอดี**
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    center: Vec2,
    zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

impl Camera {
    /// ซูมออกได้ไกลสุด (ภาพเล็กลง 100 เท่า)
    pub const MIN_ZOOM: f32 = 0.01;
    /// ซูมเข้าได้มากสุด (ภาพใหญ่ขึ้น 64 เท่า)
    pub const MAX_ZOOM: f32 = 64.0;

    /// สร้างกล้องที่จุดและระดับซูมที่กำหนด (ค่าถูก clamp ให้ปลอดภัยเสมอ)
    #[must_use]
    pub fn new(center: Vec2, zoom: f32) -> Self {
        let mut camera = Self::default();
        camera.set_center(center);
        camera.set_zoom(zoom);
        camera
    }

    /// จุดใน world ที่อยู่กลางจอ
    #[must_use]
    pub fn center(&self) -> Vec2 {
        self.center
    }

    /// ระดับซูมปัจจุบัน — อยู่ใน `[MIN_ZOOM, MAX_ZOOM]` เสมอ
    #[must_use]
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// ตั้งจุดกึ่งกลาง — ค่าที่ไม่ใช่ตัวเลขจะถูกปฏิเสธ (I-4)
    ///
    /// ค่า `NaN`/`inf` เข้ามาได้จริงจากไฟล์ `.refx` ที่เสียหาย
    /// ถ้าปล่อยผ่านจะกลายเป็น transform ที่ทำให้ภาพหายทั้งจอโดยหาสาเหตุไม่เจอ
    pub fn set_center(&mut self, center: Vec2) {
        if center.x.is_finite() && center.y.is_finite() {
            self.center = center;
        }
    }

    /// ตั้งระดับซูม — clamp เข้าช่วงที่ปลอดภัยเสมอ
    pub fn set_zoom(&mut self, zoom: f32) {
        if zoom.is_finite() && zoom > 0.0 {
            self.zoom = zoom.clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        }
    }

    /// เลื่อนกล้องตามระยะที่ **ลากบนจอ** (หน่วยพิกเซลหน้าจอ)
    ///
    /// หารด้วย zoom เพื่อให้ภาพเลื่อนตามเคอร์เซอร์พอดีทุกระดับซูม
    /// ถ้าไม่หาร ตอนซูมเข้าภาพจะวิ่งเร็วกว่าเมาส์จนรู้สึกลื่นไถล
    pub fn pan_by_screen_delta(&mut self, delta: Vec2) {
        if !delta.x.is_finite() || !delta.y.is_finite() {
            return;
        }
        self.set_center(self.center - delta / self.zoom);
    }

    /// แปลงพิกัดหน้าจอ → world
    #[must_use]
    pub fn screen_to_world(&self, screen: Vec2, viewport: Vec2) -> Vec2 {
        self.center + (screen - viewport * 0.5) / self.zoom
    }

    /// แปลงพิกัด world → หน้าจอ
    #[must_use]
    pub fn world_to_screen(&self, world: Vec2, viewport: Vec2) -> Vec2 {
        (world - self.center) * self.zoom + viewport * 0.5
    }

    /// ★ ซูมเข้าหาเคอร์เซอร์ — จุดใน world ที่อยู่ใต้เคอร์เซอร์ต้อง **ไม่ขยับ**
    ///
    /// นี่คือรายละเอียดที่ทำให้รู้สึกว่าโปรแกรม "เชื่อถือได้" ถ้าซูมเข้ากลางจอแทน
    /// ผู้ใช้จะต้องคอย pan ตามตลอดเวลา ซึ่งน่ารำคาญมากเวลาไล่ดูรายละเอียดภาพ
    pub fn zoom_at_screen(&mut self, cursor: Vec2, viewport: Vec2, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        if !cursor.x.is_finite() || !cursor.y.is_finite() {
            return;
        }

        let before = self.screen_to_world(cursor, viewport);
        self.set_zoom(self.zoom * factor);
        let after = self.screen_to_world(cursor, viewport);

        // ชดเชยให้จุดเดิมกลับมาอยู่ใต้เคอร์เซอร์เหมือนเดิม
        self.set_center(self.center + (before - after));
    }

    /// affine world→clip สำหรับส่งให้ shader: `[a, b, c, d, tx, ty]`
    ///
    /// clip space ของ wgpu มี y ชี้ **ขึ้น** จึงต้องกลับแกน y
    #[must_use]
    pub fn to_clip_affine(&self, viewport: Vec2) -> [f32; 6] {
        let w = if viewport.x.is_finite() && viewport.x >= 1.0 {
            viewport.x
        } else {
            1.0
        };
        let h = if viewport.y.is_finite() && viewport.y >= 1.0 {
            viewport.y
        } else {
            1.0
        };
        let sx = 2.0 * self.zoom / w;
        let sy = -2.0 * self.zoom / h;
        [sx, 0.0, 0.0, sy, -self.center.x * sx, -self.center.y * sy]
    }
}

/// กล้องของทั้งสองโหมด เก็บแยกกัน
///
/// ★ แยกกล้องต่อโหมดเพราะสลับโหมดแล้วกลับมา ผู้ใช้คาดหวังว่ายังอยู่ตำแหน่งเดิม
/// ถ้าใช้กล้องร่วมกัน canvas จะเด้งไปอยู่ที่ที่ arrange เพิ่ง scroll ไว้ ซึ่งน่ารำคาญมาก
/// เวลาสลับไปมาบ่อย ๆ (docs/02 §5)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ViewState {
    /// กล้องของโหมด Canvas (pan/zoom)
    pub canvas: Camera,
    /// กล้องของโหมด Arrange (scroll/zoom)
    pub arrange: Camera,
    /// โหมดที่กำลังใช้อยู่
    pub mode: Mode,
}

impl ViewState {
    /// กล้องของโหมดปัจจุบัน
    #[must_use]
    pub fn active(&self) -> &Camera {
        match self.mode {
            Mode::Canvas => &self.canvas,
            Mode::Arrange => &self.arrange,
        }
    }

    /// กล้องของโหมดปัจจุบัน (แก้ได้)
    pub fn active_mut(&mut self) -> &mut Camera {
        match self.mode {
            Mode::Canvas => &mut self.canvas,
            Mode::Arrange => &mut self.arrange,
        }
    }
}

#[cfg(test)]
mod tests {
    // เทียบ float ตรง ๆ ได้ในเทสต์: ค่าที่ assert คือค่าคงที่หลัง clamp/ประกอบ struct
    // ซึ่งต้องเท่ากันเป๊ะ ไม่ใช่ผลจากการคำนวณทศนิยมสะสม
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use super::*;

    const VIEWPORT: Vec2 = Vec2::new(800.0, 600.0);

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn center_maps_to_screen_center() {
        let camera = Camera::new(Vec2::new(123.0, 456.0), 2.5);
        let screen = camera.world_to_screen(camera.center(), VIEWPORT);
        assert!(close(screen, VIEWPORT * 0.5), "ได้ {screen:?}");
    }

    #[test]
    fn screen_world_roundtrip() {
        let camera = Camera::new(Vec2::new(-40.0, 90.0), 0.75);
        for point in [Vec2::ZERO, Vec2::new(800.0, 600.0), Vec2::new(313.0, 271.0)] {
            let world = camera.screen_to_world(point, VIEWPORT);
            let back = camera.world_to_screen(world, VIEWPORT);
            assert!(close(back, point), "{point:?} → {world:?} → {back:?}");
        }
    }

    /// ★ ข้อกำหนดหลักของ P0-7
    #[test]
    fn zoom_keeps_point_under_cursor_fixed() {
        let cursor = Vec2::new(610.0, 137.0);
        let mut camera = Camera::new(Vec2::new(200.0, 150.0), 1.0);

        let world_before = camera.screen_to_world(cursor, VIEWPORT);
        camera.zoom_at_screen(cursor, VIEWPORT, 1.25);
        let screen_after = camera.world_to_screen(world_before, VIEWPORT);

        assert!(
            close(screen_after, cursor),
            "จุดใต้เคอร์เซอร์ขยับ: {cursor:?} → {screen_after:?}"
        );
    }

    /// ซูมเข้า-ออกหลายรอบแล้วจุดใต้เคอร์เซอร์ต้องยังไม่ขยับ (กัน error สะสม)
    #[test]
    fn repeated_zoom_does_not_drift() {
        let cursor = Vec2::new(240.0, 480.0);
        let mut camera = Camera::new(Vec2::ZERO, 1.0);
        let world_before = camera.screen_to_world(cursor, VIEWPORT);

        for _ in 0..60 {
            camera.zoom_at_screen(cursor, VIEWPORT, 1.1);
        }
        for _ in 0..60 {
            camera.zoom_at_screen(cursor, VIEWPORT, 1.0 / 1.1);
        }

        let screen_after = camera.world_to_screen(world_before, VIEWPORT);
        assert!(
            (screen_after - cursor).length() < 0.5,
            "ซูมกลับไปกลับมาแล้วเลื่อน {:.3} px",
            (screen_after - cursor).length()
        );
    }

    /// ★ "ไม่มี jitter ที่ zoom สุดทั้งสองทาง" — ข้อกำหนด P0-7
    #[test]
    fn zoom_clamps_at_both_extremes() {
        let cursor = Vec2::new(400.0, 300.0);

        let mut camera = Camera::default();
        for _ in 0..500 {
            camera.zoom_at_screen(cursor, VIEWPORT, 2.0);
        }
        assert_eq!(camera.zoom(), Camera::MAX_ZOOM);
        assert!(camera.center().is_finite());

        let mut camera = Camera::default();
        for _ in 0..500 {
            camera.zoom_at_screen(cursor, VIEWPORT, 0.5);
        }
        assert_eq!(camera.zoom(), Camera::MIN_ZOOM);
        assert!(camera.center().is_finite());
    }

    /// ชนเพดานซูมแล้วกล้องต้อง **ไม่ขยับเลย** ไม่งั้นภาพจะไหลตอนผู้ใช้ scroll ค้าง
    #[test]
    fn zooming_past_limit_does_not_move_camera() {
        let cursor = Vec2::new(700.0, 100.0);
        let mut camera = Camera::new(Vec2::new(10.0, 20.0), Camera::MAX_ZOOM);
        let before = camera.center();

        for _ in 0..20 {
            camera.zoom_at_screen(cursor, VIEWPORT, 1.5);
        }
        assert_eq!(camera.zoom(), Camera::MAX_ZOOM);
        assert!(
            close(camera.center(), before),
            "กล้องไหลทั้งที่ซูมชนเพดาน: {before:?} → {:?}",
            camera.center()
        );
    }

    #[test]
    fn pan_follows_cursor_at_any_zoom() {
        for zoom in [0.25f32, 1.0, 4.0] {
            let mut camera = Camera::new(Vec2::ZERO, zoom);
            let anchor = Vec2::new(400.0, 300.0);
            let world_before = camera.screen_to_world(anchor, VIEWPORT);

            let drag = Vec2::new(50.0, -30.0);
            camera.pan_by_screen_delta(drag);

            // จุดเดิมต้องเลื่อนไปตามระยะที่ลากพอดี ไม่ว่าซูมเท่าไหร่
            let screen_after = camera.world_to_screen(world_before, VIEWPORT);
            assert!(
                close(screen_after, anchor + drag),
                "zoom={zoom} คาด {:?} ได้ {screen_after:?}",
                anchor + drag
            );
        }
    }

    /// I-4: ค่าจากไฟล์ที่เสียหายต้องไม่ทำให้กล้องพัง
    #[test]
    fn rejects_non_finite_input() {
        let mut camera = Camera::new(Vec2::new(5.0, 5.0), 2.0);
        let before = camera;

        camera.set_center(Vec2::new(f32::NAN, 0.0));
        camera.set_center(Vec2::new(0.0, f32::INFINITY));
        camera.set_zoom(f32::NAN);
        camera.set_zoom(0.0);
        camera.set_zoom(-3.0);
        camera.pan_by_screen_delta(Vec2::new(f32::NAN, 1.0));
        camera.zoom_at_screen(Vec2::new(f32::NAN, 0.0), VIEWPORT, 1.5);
        camera.zoom_at_screen(Vec2::ZERO, VIEWPORT, f32::INFINITY);

        assert_eq!(camera, before, "ค่าเสียต้องถูกปฏิเสธ ไม่ใช่เปลี่ยนสถานะกล้อง");
    }

    #[test]
    fn new_clamps_out_of_range_zoom() {
        assert_eq!(Camera::new(Vec2::ZERO, 1e9).zoom(), Camera::MAX_ZOOM);
        assert_eq!(Camera::new(Vec2::ZERO, 1e-9).zoom(), Camera::MIN_ZOOM);
        // ค่าพังสนิท → คงค่าเริ่มต้นไว้ ไม่ panic
        assert_eq!(Camera::new(Vec2::ZERO, f32::NAN).zoom(), 1.0);
    }

    #[test]
    fn clip_affine_matches_world_to_screen() {
        let camera = Camera::new(Vec2::new(120.0, 80.0), 1.75);
        let affine = camera.to_clip_affine(VIEWPORT);
        let world = Vec2::new(300.0, 220.0);

        // ใช้ affine เดียวกับที่ shader ใช้
        let clip_x = affine[0] * world.x + affine[2] * world.y + affine[4];
        let clip_y = affine[1] * world.x + affine[3] * world.y + affine[5];
        // clip → screen (y กลับด้าน)
        let screen = Vec2::new(
            (clip_x + 1.0) * VIEWPORT.x * 0.5,
            (1.0 - clip_y) * VIEWPORT.y * 0.5,
        );

        assert!(
            close(screen, camera.world_to_screen(world, VIEWPORT)),
            "affine ของ shader ไม่ตรงกับ world_to_screen: {screen:?}"
        );
    }

    #[test]
    fn clip_affine_survives_zero_viewport() {
        let camera = Camera::default();
        assert!(
            camera
                .to_clip_affine(Vec2::ZERO)
                .iter()
                .all(|v| v.is_finite())
        );
    }

    /// ★ ข้อกำหนดของ docs/02 §5: สลับโหมดไปกลับแล้วต้องอยู่ที่เดิมทั้งสองฝั่ง
    #[test]
    fn each_mode_keeps_its_own_camera_across_switches() {
        let mut view = ViewState::default();

        view.active_mut().set_center(Vec2::new(100.0, 200.0));
        view.mode = Mode::Arrange;
        assert_eq!(
            view.active().center(),
            Vec2::ZERO,
            "arrange ต้องเป็นกล้องคนละตัว"
        );

        view.active_mut().set_center(Vec2::new(-30.0, 5.0));
        view.mode = Mode::Canvas;
        assert_eq!(
            view.active().center(),
            Vec2::new(100.0, 200.0),
            "กลับมาแล้วต้องอยู่ที่เดิม ไม่ใช่เด้งไปที่ที่ arrange scroll ไว้"
        );

        view.mode = Mode::Arrange;
        assert_eq!(view.active().center(), Vec2::new(-30.0, 5.0));
    }
}
