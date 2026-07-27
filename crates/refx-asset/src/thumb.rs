//! สร้าง thumbnail 128×128 + แก้ EXIF orientation + หาสีเด่น
//!
//! ทำงานอยู่บน **decode worker** ไม่ใช่ UI thread (I-2) — ขั้นที่ 5–7 ใน docs/05 §3
//!
//! ส่ง thumbnail กลับ main thread แทนที่จะส่งภาพเต็ม เพราะภาพ 4000² คือ 64 MB
//! ส่วน thumbnail คือ 64 KB — ต่างกัน 1000 เท่า
//!
//! spec: docs/05-memory-and-assets.md §3

use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbaImage;

/// ขนาด thumbnail (ต้องตรงกับ `refx_render::atlas::SLOT_SIZE`)
pub const THUMB_SIZE: u32 = 128;

/// thumbnail ที่พร้อมอัปโหลดขึ้น atlas
#[derive(Debug, Clone)]
pub struct Thumbnail {
    /// RGBA8 ขนาด `THUMB_SIZE × THUMB_SIZE`
    pub pixels: Vec<u8>,
    /// ความกว้างจริงของภาพต้นฉบับ
    pub source_width: u32,
    /// ความสูงจริงของภาพต้นฉบับ
    pub source_height: u32,
    /// สีเด่น (ARGB) ใช้เป็น placeholder ก่อนภาพจริงจะมา
    pub dominant: u32,
}

/// ทิศทางของภาพตาม EXIF (ค่า 1–8)
///
/// กล้องและมือถือหมุนภาพด้วย metadata แทนที่จะหมุน pixel จริง
/// ถ้าไม่แก้ ภาพจากมือถือจะตะแคงทั้งหมด ซึ่งนักวาดสังเกตเห็นทันที
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// ปกติ ไม่ต้องทำอะไร
    Normal,
    /// พลิกแนวนอน
    FlipH,
    /// หมุน 180°
    Rotate180,
    /// พลิกแนวตั้ง
    FlipV,
    /// สลับแกน (transpose)
    Transpose,
    /// หมุน 90° ตามเข็ม
    Rotate90,
    /// สลับแกนแบบกลับด้าน (transverse)
    Transverse,
    /// หมุน 270° ตามเข็ม
    Rotate270,
}

impl Orientation {
    /// แปลงจากค่า EXIF (1–8) — ค่านอกช่วงถือว่าปกติ
    #[must_use]
    pub fn from_exif(value: u32) -> Self {
        match value {
            2 => Self::FlipH,
            3 => Self::Rotate180,
            4 => Self::FlipV,
            5 => Self::Transpose,
            6 => Self::Rotate90,
            7 => Self::Transverse,
            8 => Self::Rotate270,
            // 1 และค่าเพี้ยนอื่น ๆ = ไม่ต้องหมุน (I-4: ข้อมูลจากไฟล์เชื่อไม่ได้)
            _ => Self::Normal,
        }
    }

    /// ทิศทางนี้สลับด้านกว้าง/สูงไหม
    #[must_use]
    pub fn swaps_axes(self) -> bool {
        matches!(
            self,
            Self::Transpose | Self::Rotate90 | Self::Transverse | Self::Rotate270
        )
    }

    /// หมุน/พลิกภาพให้ตั้งตรง
    #[must_use]
    pub fn apply(self, image: RgbaImage) -> RgbaImage {
        use image::imageops;
        match self {
            Self::Normal => image,
            Self::FlipH => imageops::flip_horizontal(&image),
            Self::Rotate180 => imageops::rotate180(&image),
            Self::FlipV => imageops::flip_vertical(&image),
            Self::Transpose => imageops::rotate90(&imageops::flip_horizontal(&image)),
            Self::Rotate90 => imageops::rotate90(&image),
            Self::Transverse => imageops::rotate270(&imageops::flip_horizontal(&image)),
            Self::Rotate270 => imageops::rotate270(&image),
        }
    }
}

/// อ่าน EXIF orientation จากไบต์ดิบของไฟล์
///
/// คืน [`Orientation::Normal`] ถ้าไม่มี EXIF หรืออ่านไม่ได้ — **ไม่ใช่ error**
/// ภาพส่วนใหญ่ไม่มี EXIF และนั่นเป็นเรื่องปกติ
#[must_use]
pub fn read_orientation(bytes: &[u8]) -> Orientation {
    let mut cursor = std::io::Cursor::new(bytes);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut cursor) else {
        return Orientation::Normal;
    };
    let Some(field) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) else {
        return Orientation::Normal;
    };
    field
        .value
        .get_uint(0)
        .map_or(Orientation::Normal, Orientation::from_exif)
}

/// ย่อภาพเป็น thumbnail 128×128
///
/// ใช้ Lanczos3 ตาม docs/05 §3 — คุณภาพดีที่สุดสำหรับการย่อมาก ๆ
/// ซึ่งสำคัญเพราะ thumbnail คือสิ่งที่ผู้ใช้เห็นเกือบตลอดเวลา
#[must_use]
pub fn make_thumbnail(image: &RgbaImage) -> Thumbnail {
    let (source_width, source_height) = (image.width(), image.height());
    let dominant = dominant_color(image);

    let pixels = resize_to_square(image).unwrap_or_else(|| {
        // resize ล้มเหลว (ขนาด 0 ฯลฯ) — คืนสี่เหลี่ยมสีเด่นแทน ไม่ใช่ล้ม
        tracing::warn!(source_width, source_height, "ย่อภาพไม่สำเร็จ — ใช้สีเด่นแทน");
        solid_square(dominant)
    });

    Thumbnail {
        pixels,
        source_width,
        source_height,
        dominant,
    }
}

/// ย่อเป็นสี่เหลี่ยมจัตุรัสขนาด `THUMB_SIZE`
fn resize_to_square(image: &RgbaImage) -> Option<Vec<u8>> {
    if image.width() == 0 || image.height() == 0 {
        return None;
    }

    let src = FirImage::from_vec_u8(
        image.width(),
        image.height(),
        image.as_raw().clone(),
        PixelType::U8x4,
    )
    .ok()?;

    let mut dst = FirImage::new(THUMB_SIZE, THUMB_SIZE, PixelType::U8x4);
    let mut resizer = Resizer::new();
    resizer
        .resize(
            &src,
            &mut dst,
            &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(
                fast_image_resize::FilterType::Lanczos3,
            )),
        )
        .ok()?;

    Some(dst.into_vec())
}

/// สี่เหลี่ยมทึบสีเดียว (fallback)
fn solid_square(argb: u32) -> Vec<u8> {
    let [a, r, g, b] = argb.to_be_bytes();
    let mut out = Vec::with_capacity((THUMB_SIZE * THUMB_SIZE * 4) as usize);
    for _ in 0..(THUMB_SIZE * THUMB_SIZE) {
        out.extend_from_slice(&[r, g, b, a]);
    }
    out
}

/// หาสีเด่นแบบเร็ว — เฉลี่ยจากการสุ่มตัวอย่าง
///
/// ใช้เป็น placeholder ก่อนภาพจริงมา (docs/04 §8) จึงไม่ต้องแม่นมาก
/// แค่ต้อง **เร็ว** และ **deterministic**
#[must_use]
pub fn dominant_color(image: &RgbaImage) -> u32 {
    let raw = image.as_raw();
    if raw.len() < 4 {
        return 0xFF80_8080; // เทากลาง
    }

    // สุ่มตัวอย่างไม่เกิน ~4096 pixel — คงที่ทุกครั้งเพราะ step คำนวณจากขนาด
    let pixel_count = raw.len() / 4;
    let step = (pixel_count / 4096).max(1);

    let (mut sum_r, mut sum_g, mut sum_b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for i in (0..pixel_count).step_by(step) {
        let p = i * 4;
        // ข้าม pixel โปร่งใส — ไม่งั้นภาพที่มีขอบโปร่งจะได้สีเด่นเป็นดำ
        if raw[p + 3] < 16 {
            continue;
        }
        sum_r += u64::from(raw[p]);
        sum_g += u64::from(raw[p + 1]);
        sum_b += u64::from(raw[p + 2]);
        n += 1;
    }

    if n == 0 {
        return 0x0080_8080; // โปร่งทั้งภาพ
    }
    let r = u32::try_from(sum_r / n).unwrap_or(128);
    let g = u32::try_from(sum_g / n).unwrap_or(128);
    let b = u32::try_from(sum_b / n).unwrap_or(128);
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba(rgba))
    }

    #[test]
    fn thumbnail_is_always_128_square() {
        for (w, h) in [(1, 1), (4000, 3000), (37, 512), (2, 999)] {
            let thumb = make_thumbnail(&solid(w, h, [10, 20, 30, 255]));
            assert_eq!(
                thumb.pixels.len(),
                (THUMB_SIZE * THUMB_SIZE * 4) as usize,
                "ภาพ {w}×{h} ให้ thumbnail ผิดขนาด"
            );
            assert_eq!((thumb.source_width, thumb.source_height), (w, h));
        }
    }

    #[test]
    fn thumbnail_size_matches_atlas_slot() {
        // ถ้าสองค่านี้ไม่ตรงกัน write_texture จะพังตอน runtime
        assert_eq!(THUMB_SIZE, 128);
    }

    #[test]
    fn dominant_color_of_solid_image_is_that_color() {
        let argb = dominant_color(&solid(64, 64, [200, 100, 50, 255]));
        assert_eq!(argb, 0xFFC8_6432, "ได้ {argb:#010x}");
    }

    #[test]
    fn dominant_color_ignores_transparent_pixels() {
        let mut img = solid(32, 32, [255, 0, 0, 255]);
        // ครึ่งบนโปร่งใสสนิท (ค่าสีเป็นดำ) — ต้องไม่ถูกนับ
        for y in 0..16 {
            for x in 0..32 {
                img.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
            }
        }
        assert_eq!(dominant_color(&img), 0xFFFF_0000, "ต้องได้แดง ไม่ใช่เทา");
    }

    #[test]
    fn dominant_color_is_deterministic() {
        let img = solid(500, 500, [12, 34, 56, 255]);
        let first = dominant_color(&img);
        for _ in 0..20 {
            assert_eq!(dominant_color(&img), first);
        }
    }

    #[test]
    fn tiny_image_does_not_panic() {
        let thumb = make_thumbnail(&solid(1, 1, [7, 7, 7, 255]));
        assert_eq!(thumb.pixels.len(), (THUMB_SIZE * THUMB_SIZE * 4) as usize);
    }

    // ---------- EXIF orientation ----------

    #[test]
    fn exif_values_map_correctly() {
        assert_eq!(Orientation::from_exif(1), Orientation::Normal);
        assert_eq!(Orientation::from_exif(6), Orientation::Rotate90);
        assert_eq!(Orientation::from_exif(8), Orientation::Rotate270);
        // ค่าเพี้ยนจากไฟล์เสีย ต้องไม่ทำให้ภาพหมุนมั่ว (I-4)
        assert_eq!(Orientation::from_exif(0), Orientation::Normal);
        assert_eq!(Orientation::from_exif(99), Orientation::Normal);
        assert_eq!(Orientation::from_exif(u32::MAX), Orientation::Normal);
    }

    #[test]
    fn rotation_swaps_dimensions() {
        let img = solid(40, 10, [1, 2, 3, 255]);
        let rotated = Orientation::Rotate90.apply(img.clone());
        assert_eq!((rotated.width(), rotated.height()), (10, 40));
        assert!(Orientation::Rotate90.swaps_axes());

        let flipped = Orientation::FlipH.apply(img);
        assert_eq!((flipped.width(), flipped.height()), (40, 10));
        assert!(!Orientation::FlipH.swaps_axes());
    }

    /// หมุนแล้วหมุนกลับต้องได้ภาพเดิมเป๊ะ
    #[test]
    fn rotate_then_rotate_back_is_identity() {
        let mut img = solid(8, 4, [0, 0, 0, 255]);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255])); // จุดอ้างอิงมุมซ้ายบน

        let there = Orientation::Rotate90.apply(img.clone());
        let back = Orientation::Rotate270.apply(there);
        assert_eq!(back, img, "หมุนไปกลับแล้วภาพต้องเหมือนเดิม");
    }

    #[test]
    fn no_exif_is_normal_not_error() {
        assert_eq!(
            read_orientation(b"not an image at all"),
            Orientation::Normal
        );
        assert_eq!(read_orientation(&[]), Orientation::Normal);
    }
}
