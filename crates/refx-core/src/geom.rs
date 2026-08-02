//! `Rect` · `Obb` — รูปทรงสำหรับ culling, hit-test และ rubber-band
//!
//! อยู่ใน `refx-core` เพราะเป็นคณิตศาสตร์ล้วน — ทดสอบครบได้โดยไม่ต้องเปิดหน้าต่าง
//!
//! ★ **ทำไมต้องมี `Obb` ไม่ใช่แค่ `Rect`:** item หมุนได้ (`ItemCanvas::rotation`)
//! ถ้า hit-test ใช้ AABB อย่างเดียว ผู้ใช้จะคลิกโดนภาพที่หมุน 45° ทั้งที่เคอร์เซอร์
//! อยู่นอกภาพชัด ๆ — รู้สึกเหมือนโปรแกรม "จับผิดตัว" ซึ่งกัดกร่อนความเชื่อถือเร็วมาก
//! AABB ใช้เป็นด่านหยาบ (broad phase) แล้ว `Obb` เป็นด่านละเอียด
//!
//! ทุกฟังก์ชันในไฟล์นี้ต้องทนค่า `NaN`/`inf` ได้ (I-4) — ค่าพวกนี้มาจากไฟล์ผู้ใช้ได้จริง

use glam::Vec2;

/// สี่เหลี่ยมแนวแกน — `min` มุมซ้ายบน, `max` มุมขวาล่าง (y ชี้ลง)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// มุมซ้ายบน
    pub min: Vec2,
    /// มุมขวาล่าง
    pub max: Vec2,
}

impl Rect {
    /// สี่เหลี่ยมที่ไม่กินพื้นที่เลย — จุดเริ่มของการ union
    pub const EMPTY: Self = Self {
        min: Vec2::new(f32::INFINITY, f32::INFINITY),
        max: Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY),
    };

    /// สร้างจากสองมุม โดยจัดให้ `min` ≤ `max` เสมอ
    ///
    /// ผู้ใช้ลาก rubber-band ขึ้นซ้ายได้ตามปกติ ซึ่งให้ `min > max` มาตรง ๆ
    #[must_use]
    pub fn from_corners(a: Vec2, b: Vec2) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// สร้างจากจุดกึ่งกลางกับขนาดเต็ม
    #[must_use]
    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        let half = size.abs() * 0.5;
        Self {
            min: center - half,
            max: center + half,
        }
    }

    /// จุดกึ่งกลาง
    #[must_use]
    pub fn center(self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    /// ความกว้าง/สูง
    #[must_use]
    pub fn size(self) -> Vec2 {
        self.max - self.min
    }

    /// ไม่กินพื้นที่เลย (รวมถึงกรณี `EMPTY` และค่าที่ไม่ใช่ตัวเลข)
    #[must_use]
    pub fn is_empty(self) -> bool {
        !(self.max.x > self.min.x && self.max.y > self.min.y)
    }

    /// จุดนี้อยู่ในกรอบหรือไม่ (ขอบนับว่าอยู่ใน)
    #[must_use]
    pub fn contains_point(self, point: Vec2) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// ทับกันหรือไม่ — แตะขอบพอดีถือว่าทับ
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }

    /// กรอบนี้กินกรอบอีกอันทั้งหมดหรือไม่ (rubber-band แบบ "ต้องอยู่ในกรอบทั้งตัว")
    #[must_use]
    pub fn contains_rect(self, other: Self) -> bool {
        self.min.x <= other.min.x
            && self.min.y <= other.min.y
            && self.max.x >= other.max.x
            && self.max.y >= other.max.y
    }

    /// ขยายออกทุกด้านเท่ากัน (ใช้ทำ margin ของ prefetch — docs/04 §5)
    #[must_use]
    pub fn expand(self, amount: f32) -> Self {
        if !amount.is_finite() {
            return self;
        }
        Self {
            min: self.min - Vec2::splat(amount),
            max: self.max + Vec2::splat(amount),
        }
    }

    /// กรอบที่เล็กที่สุดที่คลุมทั้งสองอัน
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// ค่าทุกตัวเป็นตัวเลขจริงหรือไม่ (I-4)
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }
}

/// สี่เหลี่ยมที่หมุนได้ — รูปทรงจริงของ item บน canvas
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obb {
    /// จุดกึ่งกลาง
    pub center: Vec2,
    /// ครึ่งหนึ่งของขนาด
    pub half_size: Vec2,
    /// การหมุน (เรเดียน)
    pub rotation: f32,
}

impl Obb {
    /// แกนท้องถิ่นสองแกน (หน่วย) — `[แกน x, แกน y]`
    #[must_use]
    pub fn axes(self) -> [Vec2; 2] {
        let (sin, cos) = if self.rotation.is_finite() {
            self.rotation.sin_cos()
        } else {
            (0.0, 1.0)
        };
        [Vec2::new(cos, sin), Vec2::new(-sin, cos)]
    }

    /// สี่มุม เรียงตามเข็มนาฬิกาจากมุมซ้ายบนในพิกัดท้องถิ่น
    #[must_use]
    pub fn corners(self) -> [Vec2; 4] {
        let [x, y] = self.axes();
        let (hx, hy) = (x * self.half_size.x, y * self.half_size.y);
        [
            self.center - hx - hy,
            self.center + hx - hy,
            self.center + hx + hy,
            self.center - hx + hy,
        ]
    }

    /// AABB ที่คลุมรูปทรงนี้ — **ด่านหยาบของ hit-test และคีย์ของ spatial index**
    #[must_use]
    pub fn aabb(self) -> Rect {
        let corners = self.corners();
        let mut rect = Rect {
            min: corners[0],
            max: corners[0],
        };
        for corner in &corners[1..] {
            rect.min = rect.min.min(*corner);
            rect.max = rect.max.max(*corner);
        }
        rect
    }

    /// จุดนี้อยู่บนภาพจริงหรือไม่ — **ด่านละเอียดของ hit-test**
    ///
    /// หมุนจุดกลับเข้าพิกัดท้องถิ่นของ item แล้วเทียบกับครึ่งขนาด
    /// (ถูกกว่าและแม่นกว่าการทดสอบกับสี่มุมทีละด้าน)
    #[must_use]
    pub fn contains_point(self, point: Vec2) -> bool {
        if !point.is_finite() || !self.center.is_finite() {
            return false;
        }
        let [x, y] = self.axes();
        let offset = point - self.center;
        let local = Vec2::new(offset.dot(x), offset.dot(y));
        local.x.abs() <= self.half_size.x.abs() && local.y.abs() <= self.half_size.y.abs()
    }

    /// ทับกับกรอบแนวแกนหรือไม่ — ใช้กับ rubber-band และ culling
    ///
    /// separating axis theorem บนสี่แกน: แกนของ `Rect` สองแกน + แกนของ `Obb` สองแกน
    /// สำหรับรูปนูนสองรูป ถ้าไม่มีแกนไหนแยกกันได้ แปลว่าทับกัน
    #[must_use]
    pub fn intersects_rect(self, rect: Rect) -> bool {
        if rect.is_empty() || !rect.is_finite() || !self.center.is_finite() {
            return false;
        }
        let [ax, ay] = self.axes();
        let rect_center = rect.center();
        let rect_half = rect.size() * 0.5;
        let half = self.half_size.abs();

        for axis in [Vec2::X, Vec2::Y, ax, ay] {
            // ระยะยื่นครึ่งหนึ่งของแต่ละรูปบนแกนนี้
            let rect_reach = rect_half.x * axis.x.abs() + rect_half.y * axis.y.abs();
            let obb_reach = half.x * ax.dot(axis).abs() + half.y * ay.dot(axis).abs();
            let gap = (self.center - rect_center).dot(axis).abs();
            if gap > rect_reach + obb_reach {
                return false; // เจอแกนที่แยกกันได้ = ไม่ทับแน่นอน
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const TAU_8: f32 = std::f32::consts::TAU / 8.0; // 45°

    fn square(center: Vec2, side: f32, rotation: f32) -> Obb {
        Obb {
            center,
            half_size: Vec2::splat(side * 0.5),
            rotation,
        }
    }

    // ---------- Rect ----------

    #[test]
    fn from_corners_normalises_a_backwards_drag() {
        // ผู้ใช้ลาก rubber-band ขึ้นซ้ายเป็นเรื่องปกติ
        let rect = Rect::from_corners(Vec2::new(10.0, 10.0), Vec2::new(-5.0, -2.0));
        assert_eq!(rect.min, Vec2::new(-5.0, -2.0));
        assert_eq!(rect.max, Vec2::new(10.0, 10.0));
        assert!(!rect.is_empty());
    }

    #[test]
    fn touching_edges_count_as_intersecting() {
        let a = Rect::from_corners(Vec2::ZERO, Vec2::new(10.0, 10.0));
        let b = Rect::from_corners(Vec2::new(10.0, 0.0), Vec2::new(20.0, 10.0));
        assert!(a.intersects(b));
        assert!(!a.contains_rect(b));
        assert!(a.contains_rect(a));
    }

    #[test]
    fn union_starts_from_empty_without_swallowing_everything() {
        let rect = Rect::from_corners(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0));
        assert_eq!(Rect::EMPTY.union(rect), rect);
        assert!(Rect::EMPTY.is_empty());
    }

    /// I-4: ค่าจากไฟล์ที่เสียหายต้องไม่ทำให้ hit-test คืนผลมั่ว
    #[test]
    fn non_finite_input_never_reports_a_hit() {
        let rect = Rect::from_corners(Vec2::ZERO, Vec2::new(10.0, 10.0));
        assert!(!rect.contains_point(Vec2::new(f32::NAN, 5.0)));
        assert!(!square(Vec2::ZERO, 10.0, 0.0).contains_point(Vec2::new(f32::NAN, 0.0)));
        assert!(!square(Vec2::new(f32::NAN, 0.0), 10.0, 0.0).contains_point(Vec2::ZERO));

        let broken = Rect {
            min: Vec2::new(f32::NAN, 0.0),
            max: Vec2::new(1.0, 1.0),
        };
        assert!(!square(Vec2::ZERO, 10.0, 0.0).intersects_rect(broken));
    }

    // ---------- Obb ----------

    #[test]
    fn an_unrotated_obb_matches_its_aabb() {
        let obb = square(Vec2::new(5.0, 5.0), 10.0, 0.0);
        let aabb = obb.aabb();
        assert!((aabb.min - Vec2::ZERO).length() < 1e-4, "ได้ {:?}", aabb.min);
        assert!((aabb.max - Vec2::new(10.0, 10.0)).length() < 1e-4);
    }

    /// ★ เหตุผลที่ต้องมี `Obb`: AABB ของภาพที่หมุน 45° ใหญ่กว่าตัวภาพราว 41%
    /// ถ้าใช้ AABB ตัดสิน ผู้ใช้จะคลิกโดนภาพทั้งที่เคอร์เซอร์อยู่นอกภาพชัด ๆ
    #[test]
    fn a_rotated_square_rejects_the_corners_its_aabb_would_accept() {
        let obb = square(Vec2::ZERO, 10.0, TAU_8);
        let aabb = obb.aabb();

        // มุมของ AABB อยู่นอกตัวภาพจริง
        let corner = Vec2::new(aabb.max.x - 0.2, aabb.max.y - 0.2);
        assert!(aabb.contains_point(corner), "ต้องอยู่ใน AABB ก่อน");
        assert!(!obb.contains_point(corner), "แต่ต้องไม่นับว่าโดนภาพ");

        // กลางภาพและปลายแกนที่หมุนแล้ว ต้องยังโดน
        assert!(obb.contains_point(Vec2::ZERO));
        let along = obb.axes()[0] * 4.9;
        assert!(obb.contains_point(along), "จุดในแนวแกนของภาพต้องโดน");
    }

    #[test]
    fn aabb_of_a_rotated_square_is_bigger_by_root_two() {
        let obb = square(Vec2::ZERO, 10.0, TAU_8);
        let size = obb.aabb().size();
        let expected = 10.0 * std::f32::consts::SQRT_2;
        assert!((size.x - expected).abs() < 1e-3, "ได้ {size:?}");
        assert!((size.y - expected).abs() < 1e-3);
    }

    #[test]
    fn rotating_by_a_full_turn_changes_nothing() {
        let point = Vec2::new(3.0, 1.0);
        let plain = square(Vec2::ZERO, 8.0, 0.0);
        let spun = square(Vec2::ZERO, 8.0, std::f32::consts::TAU);
        assert_eq!(plain.contains_point(point), spun.contains_point(point));
    }

    // ---------- SAT ----------

    #[test]
    fn rect_and_obb_overlap_is_symmetric_with_containment() {
        let obb = square(Vec2::ZERO, 10.0, 0.0);
        let inside = Rect::from_corners(Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0));
        let overlapping = Rect::from_corners(Vec2::new(4.0, 4.0), Vec2::new(20.0, 20.0));
        let far = Rect::from_corners(Vec2::new(100.0, 100.0), Vec2::new(120.0, 120.0));

        assert!(obb.intersects_rect(inside));
        assert!(obb.intersects_rect(overlapping));
        assert!(!obb.intersects_rect(far));
    }

    /// ★ เคสที่ SAT มีไว้เพื่อ: กรอบที่ทับ **AABB** ของภาพที่หมุน แต่ไม่ทับตัวภาพ
    #[test]
    fn a_rect_that_only_touches_the_aabb_corner_is_not_a_hit() {
        let obb = square(Vec2::ZERO, 10.0, TAU_8);
        let aabb = obb.aabb();

        // กรอบเล็ก ๆ ที่มุมขวาล่างของ AABB — อยู่นอกตัวสี่เหลี่ยมที่หมุนแล้ว
        let corner = Rect::from_corners(aabb.max - Vec2::splat(0.5), aabb.max);
        assert!(aabb.intersects(corner), "ต้องทับ AABB ก่อน");
        assert!(!obb.intersects_rect(corner), "แต่ต้องไม่นับว่าทับตัวภาพ");
    }

    #[test]
    fn an_empty_rect_never_intersects() {
        let obb = square(Vec2::ZERO, 10.0, 0.0);
        assert!(!obb.intersects_rect(Rect::EMPTY));
        let degenerate = Rect::from_corners(Vec2::ZERO, Vec2::ZERO);
        assert!(!obb.intersects_rect(degenerate));
    }

    /// จุดที่อยู่ในภาพ ต้องทำให้กรอบเล็ก ๆ รอบจุดนั้นทับภาพด้วยเสมอ
    /// — สองฟังก์ชันนี้ต้องไม่ขัดกันเอง ไม่งั้นคลิกกับ rubber-band จะให้คำตอบคนละอย่าง
    #[test]
    fn point_test_and_rect_test_agree_with_each_other() {
        let obb = square(Vec2::new(3.0, -2.0), 12.0, 0.7);
        for i in -30..30 {
            for j in -30..30 {
                let point = Vec2::new(i as f32 * 0.5, j as f32 * 0.5);
                if obb.contains_point(point) {
                    let dot =
                        Rect::from_corners(point - Vec2::splat(0.01), point + Vec2::splat(0.01));
                    assert!(
                        obb.intersects_rect(dot),
                        "{point:?} อยู่ในภาพแต่กรอบรอบจุดกลับไม่ทับ"
                    );
                }
            }
        }
    }
}
