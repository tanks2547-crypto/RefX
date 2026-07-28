//! ★ ทางเดียวของโปรเจกต์ที่เรียก `fast_image_resize`
//!
//! ### ทำไมต้องมีไฟล์นี้ — วัดมาแล้ว ไม่ใช่ความสวยงาม
//!
//! ตอนทำ P1-7 มีการเรียก `Resizer::resize` เพิ่มเป็นจุดที่สอง (thumbnail กับ
//! working texture) ผลคือ binary release โตจาก **15.40 → 17.99 MB (+2.6 MB)**
//! ซึ่งเป็น 10% ของงบทั้งก้อน ทั้งที่ทั้งสองจุดใช้ชนิดข้อมูลเดียวกันเป๊ะ
//!
//! ทดสอบแยกทีละอย่างแล้ว:
//!   * เปลี่ยน filter (Bilinear ↔ Lanczos3) — ขนาดเท่าเดิม ไม่ใช่สาเหตุ
//!   * ตัด mip loop ออกไปใช้ box filter เขียนเอง — ขนาดเท่าเดิม ไม่ใช่สาเหตุ
//!   * ตัดการเรียก `resize` ของ working ออกทั้งหมด — **ลดลง 2.6 MB**
//!
//! → ต้นทุนอยู่ที่ **จำนวนจุดที่เรียก** ไม่ใช่ตัวเลือกที่ส่งเข้าไป
//!   รวมทุกจุดมาเรียกผ่านฟังก์ชันเดียวที่ `#[inline(never)]` จึงได้โค้ดชุดเดียว
//!
//! ผลพลอยได้ที่สำคัญกว่าขนาดไฟล์: การย่อภาพของทั้งโปรเจกต์เดินผ่านเส้นทางเดียว
//! เหมือนที่ `read_file_guarded` เป็นเส้นทางเดียวของการอ่านไฟล์ — สองที่ที่ทำ
//! เรื่องเดียวกันจะเพี้ยนจากกันเสมอเมื่อมีคนแก้ข้างเดียว

use fast_image_resize::images::{Image as FirImage, ImageRef as FirImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbaImage;

/// ตัวกรองที่โปรเจกต์นี้ใช้ (docs/05 §3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Lanczos3 — สำหรับ thumbnail ที่ย่อแรงมากและผู้ใช้เห็นตลอดเวลา
    Best,
    /// Triangle/Bilinear — สำหรับ working texture ที่ย่อไม่มากและต้องการความเร็ว
    Fast,
}

impl Quality {
    fn alg(self) -> ResizeAlg {
        match self {
            Self::Best => ResizeAlg::Convolution(FilterType::Lanczos3),
            Self::Fast => ResizeAlg::Convolution(FilterType::Bilinear),
        }
    }
}

/// ย่อภาพเป็นจัตุรัสขนาด `side` — คืน RGBA8 ดิบ
///
/// `#[inline(never)]` โดยตั้งใจ: ต้องมีโค้ดชุดเดียวในทั้ง binary (ดูหัวไฟล์)
///
/// คืน `None` เมื่อขนาดเป็นศูนย์หรือ resize ล้มเหลว — **ไม่ panic** ไม่ว่าภาพจะเป็นอะไร
#[inline(never)]
#[must_use]
pub fn to_square(image: &RgbaImage, side: u32, quality: Quality) -> Option<Vec<u8>> {
    if image.width() == 0 || image.height() == 0 || side == 0 {
        return None;
    }

    // ยืม buffer ตรง ๆ ห้าม clone — ภาพ 4000×3000 คือ 48 MB ต่อครั้ง
    let src = FirImageRef::new(
        image.width(),
        image.height(),
        image.as_raw(),
        PixelType::U8x4,
    )
    .ok()?;

    let mut dst = FirImage::new(side, side, PixelType::U8x4);
    Resizer::new()
        .resize(
            &src,
            &mut dst,
            &ResizeOptions::new().resize_alg(quality.alg()),
        )
        .ok()?;
    Some(dst.into_vec())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba(rgba))
    }

    #[test]
    fn produces_exactly_the_requested_square() {
        for side in [1u32, 16, 128, 256] {
            let out = to_square(&solid(300, 200, [10, 20, 30, 255]), side, Quality::Best).unwrap();
            assert_eq!(out.len(), (side * side * 4) as usize, "side {side}");
        }
    }

    /// ★ ย่อภาพสีเดียวต้องได้สีเดิม — จับ resize ที่ "สำเร็จ" แต่คืน buffer ศูนย์
    #[test]
    fn solid_colour_survives_both_qualities() {
        for quality in [Quality::Best, Quality::Fast] {
            let out = to_square(&solid(200, 200, [200, 100, 50, 255]), 64, quality).unwrap();
            for pixel in out.chunks_exact(4) {
                assert!(
                    pixel[0].abs_diff(200) <= 2
                        && pixel[1].abs_diff(100) <= 2
                        && pixel[2].abs_diff(50) <= 2
                        && pixel[3] == 255,
                    "{quality:?} ให้สีเพี้ยน: {pixel:?}"
                );
            }
        }
    }

    #[test]
    fn broken_input_returns_none_not_panic() {
        assert!(to_square(&RgbaImage::new(0, 0), 64, Quality::Best).is_none());
        assert!(to_square(&solid(8, 8, [1, 2, 3, 4]), 0, Quality::Best).is_none());
    }
}
