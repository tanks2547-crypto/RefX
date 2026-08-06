//! Color picker + measure tool (P2-10) — ฟังก์ชันบริสุทธิ์ทั้งไฟล์
//!
//! ★★ **หัวใจของ picker คือคำว่า "ต้นฉบับ"** (ROADMAP P2-10)
//!
//! สีที่อยู่บนจอผ่าน grayscale · brightness · contrast · invert · opacity
//! และการผสมกับพื้นหลังมาแล้วทั้งหมด — มันคือสีของ *ภาพที่เรากำลังแสดง*
//! ไม่ใช่สีของ *ภาพ* · นักวาดเปิด grayscale ไว้เพื่อดู value แล้วจิ้มดูสีจริง
//! ของ reference เป็นเรื่องปกติ ถ้า picker อ่านจากจอเขาจะได้สีเทาทุกครั้ง
//! ซึ่งไร้ประโยชน์สิ้นเชิงและ **ดูไม่ออกว่าผิด** จนกว่าจะเอาไปใช้จริง
//!
//! ไฟล์นี้จึงมีหน้าที่เดียว: แปลงจุดบน world ให้เป็น **พิกัดในภาพต้นฉบับ**
//! ส่วนการไปอ่าน pixel จริงเป็นงานของ `refx-asset` บน worker (I-2)
//!
//! spec: docs/03-modes-and-ui.md §2, ROADMAP P2-10

use glam::Vec2;

use crate::board::{Flip, ItemCanvas};

/// ช่วงของภาพต้นฉบับที่ถูกวาดจริง เป็นสัดส่วน `0..1` — `[left, top, right, bottom]`
///
/// ★★ **นี่คือสูตรเดียวที่ทั้ง shader และ picker ต้องใช้ร่วมกัน**
///
/// `refx-ui::crop_uv` map ช่วงนี้ลงในช่องของ atlas ส่วน [`source_uv`] ใช้มัน
/// เดินย้อนทาง — ถ้าเขียนแยกกันสองที่ วันหนึ่งมันจะเพี้ยนจากกันแล้ว **ผู้ใช้จะจิ้ม
/// ตรงที่เห็นสีหนึ่งแล้วได้อีกสีหนึ่ง** โดยไม่มี error ที่ไหนเลย
/// (เกิดมาแล้วตอน P2-8: uv ของทาง working texture ลืมใส่ `flip` เพราะเขียนแยก)
///
/// ปลายช่วงถูก **สลับ** เมื่อ `flip` — นั่นคือทั้งหมดที่ `flip` ทำ เรขาคณิตไม่ขยับ
/// `left > right` จึงเป็นค่าที่ถูกต้อง ไม่ใช่ค่าที่พัง
#[must_use]
pub fn source_span(canvas: &ItemCanvas) -> [f32; 4] {
    let crop = canvas.crop.sanitized();
    let (mut left, mut right) = (crop.min.x, crop.max.x);
    let (mut top, mut bottom) = (crop.min.y, crop.max.y);
    if matches!(canvas.flip, Flip::Horizontal | Flip::Both) {
        std::mem::swap(&mut left, &mut right);
    }
    if matches!(canvas.flip, Flip::Vertical | Flip::Both) {
        std::mem::swap(&mut top, &mut bottom);
    }
    [left, top, right, bottom]
}

/// จุดใน world → พิกัดใน **ภาพต้นฉบับ** เป็นสัดส่วน `0..1`
///
/// คืน `None` เมื่อจุดอยู่นอกภาพ หรือค่าที่ได้ใช้ไม่ได้ (I-4)
///
/// ย้อนทั้งสามชั้นที่คั่นอยู่ระหว่าง world กับ pixel ต้นฉบับ:
/// **หมุน** (`Obb::axes`) → **สเกล** (ครึ่งขนาด) → **crop + flip** ([`source_span`])
#[must_use]
pub fn source_uv(canvas: &ItemCanvas, world: Vec2) -> Option<Vec2> {
    if !world.is_finite() {
        return None;
    }
    let obb = canvas.sanitized().obb();
    let [ax, ay] = obb.axes();
    let offset = world - obb.center;
    // พิกัดท้องถิ่นหลังหมุนกลับ — นิยามเดียวกับที่ `Obb::contains_point` ใช้
    // ถ้าสองที่นี้ไม่ตรงกัน ผู้ใช้จะจิ้มโดนภาพแต่ picker บอกว่าไม่โดน
    let local = Vec2::new(offset.dot(ax), offset.dot(ay));
    let (hx, hy) = (obb.half_size.x, obb.half_size.y);
    if hx <= 0.0 || hy <= 0.0 {
        return None;
    }
    let quad = Vec2::new(local.x / (hx * 2.0) + 0.5, local.y / (hy * 2.0) + 0.5);
    if !(0.0..=1.0).contains(&quad.x) || !(0.0..=1.0).contains(&quad.y) {
        return None;
    }

    let [left, top, right, bottom] = source_span(canvas);
    // ★ ช่วงที่สลับปลาย (flip) ทำงานได้เองด้วย lerp ตัวเดียวกัน ไม่ต้องแยกกรณี
    let uv = Vec2::new(
        left + (right - left) * quad.x,
        top + (bottom - top) * quad.y,
    );
    uv.is_finite().then(|| uv.clamp(Vec2::ZERO, Vec2::ONE))
}

/// สีที่จิ้มได้ พร้อมที่มาของมัน
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picked {
    /// สี RGBA ของ pixel ต้นฉบับ — **ยังไม่ผ่าน filter ใด ๆ ทั้งสิ้น**
    pub rgba: [u8; 4],
    /// พิกัด pixel ในภาพต้นฉบับ (ไว้ให้ผู้ใช้ยืนยันว่าจิ้มโดนที่ที่คิด)
    pub source_px: (u32, u32),
}

impl Picked {
    /// `#RRGGBB` ตัวพิมพ์ใหญ่ — รูปแบบที่โปรแกรมวาดทุกตัวรับได้
    ///
    /// ★ **ไม่ใส่ alpha** โดยตั้งใจ: ช่อง hex ของ Photoshop / Clip Studio /
    /// Krita รับ 6 หลัก การใส่ 8 หลักทำให้วางแล้วขึ้นว่าค่าผิด
    #[must_use]
    pub fn hex(self) -> String {
        format!(
            "#{:02X}{:02X}{:02X}",
            self.rgba[0], self.rgba[1], self.rgba[2]
        )
    }
}

// ---------------------------------------------------------------------------
// measure
// ---------------------------------------------------------------------------

/// การวัดระยะหนึ่งครั้ง — **เก็บเป็น world ทั้งคู่**
///
/// ★★ เก็บ world ไม่ใช่พิกเซลบนจอ เพราะนักวาดใช้เครื่องมือนี้เทียบสัดส่วน
/// ระหว่างสองส่วนของ reference · ถ้าเก็บเป็นพิกเซลบนจอ **ตัวเลขจะเปลี่ยนทุกครั้ง
/// ที่ซูม** ซึ่งทำให้มันไม่มีความหมายเลย (วัดตอนซูม 50% แล้วซูมเข้าเทียบต่อไม่ได้)
///
/// ไม่อยู่ใน `Board` และไม่ผ่าน `Command` โดยตั้งใจ — เป็นสถานะของ *เครื่องมือ*
/// ไม่ใช่ของ *เอกสาร* เหมือนกล้องและการเลือก (docs/02 §2.9)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurement {
    /// จุดเริ่ม (world)
    pub from: Vec2,
    /// จุดจบ (world)
    pub to: Vec2,
}

impl Measurement {
    /// ระยะเป็น world unit — **ไม่ขึ้นกับระดับซูม**
    #[must_use]
    pub fn length(self) -> f32 {
        let d = self.to - self.from;
        if d.is_finite() { d.length() } else { 0.0 }
    }

    /// ระยะแยกตามแกน (world) — ตอบคำถาม "กว้างเท่าไหร่ สูงเท่าไหร่"
    #[must_use]
    pub fn extent(self) -> Vec2 {
        let d = self.to - self.from;
        if d.is_finite() {
            Vec2::new(d.x.abs(), d.y.abs())
        } else {
            Vec2::ZERO
        }
    }

    /// มุมเป็นองศา `-180..=180` · **0° = ไปทางขวา · บวก = ทวนเข็ม**
    ///
    /// ★ กลับเครื่องหมายแกน y เพราะ y ของ world ชี้ **ลง** (พิกัดจอ) แต่คนอ่านมุม
    /// แบบคณิตศาสตร์ที่ y ชี้ขึ้น · ถ้าไม่กลับ ลากขึ้นบนจะได้มุมติดลบ
    /// ซึ่งตรงข้ามกับที่ผู้ใช้เห็นทุกโปรแกรม
    #[must_use]
    pub fn angle_deg(self) -> f32 {
        let d = self.to - self.from;
        if !d.is_finite() || d == Vec2::ZERO {
            return 0.0;
        }
        (-d.y).atan2(d.x).to_degrees()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::float_cmp)]

    use super::*;
    use crate::board::CropRect;

    fn canvas_at(pos: Vec2, size: Vec2) -> ItemCanvas {
        ItemCanvas {
            pos,
            size,
            ..ItemCanvas::default()
        }
    }

    // ---------- source_uv ----------

    #[test]
    fn the_centre_of_an_untouched_image_is_the_centre_of_the_source() {
        let canvas = canvas_at(Vec2::new(10.0, 20.0), Vec2::new(100.0, 50.0));
        let uv = source_uv(&canvas, Vec2::new(10.0, 20.0)).unwrap();
        assert!(
            (uv.x - 0.5).abs() < 1e-6 && (uv.y - 0.5).abs() < 1e-6,
            "{uv:?}"
        );
    }

    #[test]
    fn corners_map_to_the_corners_of_the_source() {
        let canvas = canvas_at(Vec2::ZERO, Vec2::new(100.0, 100.0));
        assert_eq!(
            source_uv(&canvas, Vec2::new(-50.0, -50.0)),
            Some(Vec2::ZERO)
        );
        assert_eq!(source_uv(&canvas, Vec2::new(50.0, 50.0)), Some(Vec2::ONE));
    }

    #[test]
    fn a_point_outside_the_image_picks_nothing() {
        let canvas = canvas_at(Vec2::ZERO, Vec2::new(100.0, 100.0));
        assert_eq!(source_uv(&canvas, Vec2::new(60.0, 0.0)), None);
        assert_eq!(source_uv(&canvas, Vec2::new(0.0, -51.0)), None);
    }

    /// ★★ จิ้มด้านซ้ายของภาพที่ **พลิกแนวนอน** ต้องได้ pixel จาก**ขวา**ของต้นฉบับ
    ///
    /// นี่คือข้อที่พลาดแล้วไม่มีอะไรฟ้อง: ภาพยังอยู่ที่เดิม กดก็ยังโดน
    /// ได้สีมาก็ดูสมเหตุสมผล — ผิดแค่ว่ามันเป็นสีของอีกฝั่งหนึ่งของภาพ
    #[test]
    fn flipping_reads_the_other_side_of_the_source() {
        let mut canvas = canvas_at(Vec2::ZERO, Vec2::new(100.0, 100.0));
        canvas.flip = Flip::Horizontal;
        let uv = source_uv(&canvas, Vec2::new(-40.0, 0.0)).unwrap();
        // ซ้ายสุดของภาพบนจอ (quad u = 0.1) ต้องเป็น u = 0.9 ของต้นฉบับ
        assert!((uv.x - 0.9).abs() < 1e-5, "{uv:?}");
        assert!((uv.y - 0.5).abs() < 1e-5, "{uv:?}");

        canvas.flip = Flip::Vertical;
        let uv = source_uv(&canvas, Vec2::new(0.0, -40.0)).unwrap();
        assert!((uv.x - 0.5).abs() < 1e-5, "{uv:?}");
        assert!((uv.y - 0.9).abs() < 1e-5, "{uv:?}");
    }

    /// ★ ครอปแล้วต้องอ่านจาก **ช่วงที่เหลืออยู่** ไม่ใช่จากภาพเต็ม
    #[test]
    fn cropping_shifts_which_part_of_the_source_is_read() {
        let mut canvas = canvas_at(Vec2::ZERO, Vec2::new(100.0, 100.0));
        canvas.crop = CropRect {
            min: Vec2::new(0.25, 0.0),
            max: Vec2::new(0.75, 1.0),
        };
        // กึ่งกลางของสิ่งที่เห็น = กึ่งกลางของช่วงที่ครอปไว้ = 0.5 พอดี
        assert!((source_uv(&canvas, Vec2::ZERO).unwrap().x - 0.5).abs() < 1e-6);
        // ขอบซ้ายของสิ่งที่เห็น = 0.25 ของต้นฉบับ
        let uv = source_uv(&canvas, Vec2::new(-50.0, 0.0)).unwrap();
        assert!((uv.x - 0.25).abs() < 1e-6, "{uv:?}");
    }

    /// ★ ภาพที่หมุนแล้วต้องยังจิ้มถูกจุด — ย้อนการหมุนก่อนเสมอ
    #[test]
    fn rotation_is_undone_before_reading_the_source() {
        let mut canvas = canvas_at(Vec2::ZERO, Vec2::new(100.0, 100.0));
        canvas.rotation = std::f32::consts::FRAC_PI_2; // 90°
        // หมุน 90° แล้ว "ขวาของภาพ" ไปชี้ลงล่างบนจอ
        let uv = source_uv(&canvas, Vec2::new(0.0, 40.0)).unwrap();
        assert!((uv.x - 0.9).abs() < 1e-5, "{uv:?}");
        assert!((uv.y - 0.5).abs() < 1e-5, "{uv:?}");
    }

    /// ★★ picker ต้องเห็นจุดเดียวกับที่ hit-test เห็น
    ///
    /// ถ้าสองอย่างนี้ไม่ตรงกัน ผู้ใช้จะจิ้มโดนขอบภาพแล้ว "ไม่มีอะไรเกิดขึ้น"
    /// หรือแย่กว่านั้นคือจิ้มนอกภาพแล้วได้สีมา
    #[test]
    fn picking_agrees_with_hit_testing_on_every_edge() {
        let mut canvas = canvas_at(Vec2::new(7.0, -3.0), Vec2::new(80.0, 40.0));
        canvas.rotation = 0.7;
        let obb = canvas.obb();
        for step in 0..400 {
            #[expect(clippy::cast_precision_loss, reason = "ดัชนีเล็ก")]
            let angle = step as f32 * 0.0157;
            let probe = obb.center + Vec2::new(angle.cos(), angle.sin()) * 25.0;
            assert_eq!(
                obb.contains_point(probe),
                source_uv(&canvas, probe).is_some(),
                "ไม่ตรงกันที่ {probe:?}"
            );
        }
    }

    /// I-4: ค่าที่พังต้องไม่ทำให้ได้พิกัดขยะออกมา
    #[test]
    fn broken_values_pick_nothing() {
        let canvas = canvas_at(Vec2::ZERO, Vec2::new(100.0, 100.0));
        assert_eq!(source_uv(&canvas, Vec2::new(f32::NAN, 0.0)), None);
        assert_eq!(source_uv(&canvas, Vec2::new(0.0, f32::INFINITY)), None);
    }

    #[test]
    fn hex_has_six_digits_and_ignores_alpha() {
        let picked = Picked {
            rgba: [0x0A, 0xB2, 0xFF, 0x10],
            source_px: (0, 0),
        };
        assert_eq!(picked.hex(), "#0AB2FF");
    }

    // ---------- measure ----------

    /// ★★ ระยะเป็น world unit — ตัวเลขต้องไม่ขึ้นกับซูม
    ///
    /// เทสต์นี้ตรวจ**คุณสมบัติเชิงอัลกอริทึม** ไม่ใช่จับเวลา (docs/08 §3.9 ข้อ 5b):
    /// ป้อนจุด world เดิมเข้าไป ผลต้องเท่ากันเป๊ะเสมอ ไม่ว่ากล้องจะอยู่ที่ไหน
    #[test]
    fn distance_is_in_world_units_so_zoom_cannot_change_it() {
        let m = Measurement {
            from: Vec2::new(0.0, 0.0),
            to: Vec2::new(30.0, 40.0),
        };
        assert_eq!(m.length(), 50.0);
        assert_eq!(m.extent(), Vec2::new(30.0, 40.0));
    }

    #[test]
    fn angle_is_zero_to_the_right_and_positive_going_up() {
        let at = |x: f32, y: f32| {
            Measurement {
                from: Vec2::ZERO,
                to: Vec2::new(x, y),
            }
            .angle_deg()
        };
        assert!((at(10.0, 0.0) - 0.0).abs() < 1e-4);
        // y ของ world ชี้ลง — ลากขึ้นบนคือ y ติดลบ และต้องได้ +90°
        assert!((at(0.0, -10.0) - 90.0).abs() < 1e-4);
        assert!((at(0.0, 10.0) + 90.0).abs() < 1e-4);
        assert!((at(10.0, -10.0) - 45.0).abs() < 1e-4);
    }

    #[test]
    fn a_measurement_of_nothing_is_zero_not_nan() {
        let m = Measurement {
            from: Vec2::ZERO,
            to: Vec2::ZERO,
        };
        assert_eq!(m.length(), 0.0);
        assert_eq!(m.angle_deg(), 0.0);

        let broken = Measurement {
            from: Vec2::new(f32::NAN, 0.0),
            to: Vec2::ZERO,
        };
        assert_eq!(broken.length(), 0.0);
        assert_eq!(broken.angle_deg(), 0.0);
        assert_eq!(broken.extent(), Vec2::ZERO);
    }
}
