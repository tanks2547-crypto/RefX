//! สร้าง thumbnail 128×128 + แก้ EXIF orientation + หาสีเด่น
//!
//! ทำงานอยู่บน **decode worker** ไม่ใช่ UI thread (I-2) — ขั้นที่ 5–7 ใน docs/05 §3
//!
//! ส่ง thumbnail กลับ main thread แทนที่จะส่งภาพเต็ม เพราะภาพ 4000² คือ 64 MB
//! ส่วน thumbnail คือ 64 KB — ต่างกัน 1000 เท่า
//!
//! spec: docs/05-memory-and-assets.md §3

use image::RgbaImage;

use crate::resize::{self, Quality};

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

/// ลายเซ็นเริ่มไฟล์ JPEG (SOI)
const JPEG_SOI: [u8; 2] = [0xFF, 0xD8];
/// ป้ายที่นำหน้าบล็อก EXIF ใน segment APP1
const EXIF_ID: [u8; 6] = *b"Exif\0\0";

/// หา payload ของ EXIF ใน JPEG โดยอ่าน **เฉพาะ segment ส่วนหัว**
///
/// ★ ทำไมต้องเขียนเอง แทนที่จะปล่อยให้ `exif` จัดการทั้งไฟล์:
///
/// `exif::Reader::read_from_container` ไล่หา APP1 ต่อไปเรื่อย ๆ **หลัง SOS ด้วย**
/// ซึ่งแปลว่ามันเดินผ่าน entropy-coded data ทั้งก้อนทีละไบต์ (`read_until(0xFF, ..)`
/// พร้อมจอง `Vec` ใหม่ทุกครั้งที่เจอ `0xFF`) กว่าจะยอมแพ้ตอน EOI
/// บนภาพ 4000×3000 ที่ **ไม่มี EXIF เลย** ต้นทุนนี้วัดได้ **16.2 ms/ไฟล์ = 9.4%**
/// ของเวลาต่อไฟล์ทั้งหมด โดยไม่ได้ข้อมูลอะไรกลับมาสักอย่าง
///
/// สเปก JPEG บังคับให้ APP1 ของ EXIF อยู่ **ก่อน SOS** เสมอ การหยุดที่ SOS
/// จึงไม่ทำให้พลาดภาพที่มี EXIF จริง — ยืนยันด้วยเทสต์ที่ฝัง EXIF จริงลงไฟล์
///
/// ทุกการอ่านผ่าน `get()` ทั้งหมด ไฟล์เพี้ยนได้แค่ `None` ไม่มีทาง panic (I-4)
/// คืน slice ที่ชี้เข้าไปใน `bytes` เลย ไม่คัดลอก
fn jpeg_exif_payload(bytes: &[u8]) -> Option<&[u8]> {
    if !bytes.starts_with(&JPEG_SOI) {
        return None;
    }

    let mut pos = JPEG_SOI.len();
    loop {
        // marker ขึ้นต้นด้วย 0xFF อย่างน้อยหนึ่งตัว (ซ้ำได้ = fill byte)
        let fill_start = pos;
        while bytes.get(pos) == Some(&0xFF) {
            pos = pos.checked_add(1)?;
        }
        if pos == fill_start {
            return None; // ไม่เจอ 0xFF ตรงที่ควรเป็น marker = ไฟล์เพี้ยน
        }

        let code = *bytes.get(pos)?;
        pos = pos.checked_add(1)?;

        match code {
            // marker เดี่ยว ไม่มีความยาวตามหลัง
            0x01 | 0xD0..=0xD7 => continue,
            // SOS = เริ่มข้อมูลภาพ · EOI = จบไฟล์ — EXIF ต้องมาก่อนนี้เสมอ
            0xDA | 0xD9 => return None,
            _ => {}
        }

        // ความยาวนับรวมสองไบต์ของตัวมันเอง
        let len = usize::from(u16::from_be_bytes([
            *bytes.get(pos)?,
            *bytes.get(pos.checked_add(1)?)?,
        ]));
        let end = pos.checked_add(len)?;
        let payload = bytes.get(pos.checked_add(2)?..end)?;

        if code == 0xE1 && payload.starts_with(&EXIF_ID) {
            return payload.get(EXIF_ID.len()..);
        }
        pos = end;
    }
}

/// ดึงค่า orientation ออกจาก EXIF ที่ parse แล้ว
fn orientation_of(exif: &exif::Exif) -> Orientation {
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|field| field.value.get_uint(0))
        .map_or(Orientation::Normal, Orientation::from_exif)
}

/// อ่าน EXIF orientation จากไบต์ดิบของไฟล์
///
/// คืน [`Orientation::Normal`] ถ้าไม่มี EXIF หรืออ่านไม่ได้ — **ไม่ใช่ error**
/// ภาพส่วนใหญ่ไม่มี EXIF และนั่นเป็นเรื่องปกติ
#[must_use]
pub fn read_orientation(bytes: &[u8]) -> Orientation {
    // ★ JPEG มีทางลัดของตัวเอง — ดูเหตุผลใน [`jpeg_exif_payload`]
    //   format อื่นไม่มีปัญหานี้ (PNG เดินทีละ chunk วัดได้ 0.03 ms/ไฟล์)
    if bytes.starts_with(&JPEG_SOI) {
        let Some(payload) = jpeg_exif_payload(bytes) else {
            return Orientation::Normal;
        };
        return exif::Reader::new()
            .read_raw(payload.to_vec())
            .as_ref()
            .map_or(Orientation::Normal, orientation_of);
    }

    let mut cursor = std::io::Cursor::new(bytes);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut cursor) else {
        return Orientation::Normal;
    };
    orientation_of(&exif)
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
        tracing::warn!(
            source_width,
            source_height,
            "downscale failed — falling back to the dominant colour"
        );
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
///
/// Lanczos3 ตาม docs/05 §3 — คุณภาพดีที่สุดสำหรับการย่อมาก ๆ ซึ่งสำคัญเพราะ
/// thumbnail คือสิ่งที่ผู้ใช้เห็นเกือบตลอดเวลา
fn resize_to_square(image: &RgbaImage) -> Option<Vec<u8>> {
    resize::to_square(image, THUMB_SIZE, Quality::Best)
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

/// ★★★ ตัวสร้างไฟล์ทดสอบที่มี **EXIF จริง** — ใช้ร่วมกันทั้ง `thumb` และ `pool`
///
/// อยู่ตรงนี้ไม่ใช่ใน `mod tests` เพราะ `pool.rs` ต้องใช้ตัวเดียวกัน · สองสำเนา
/// ของตัวเขียน EXIF จะ drift แล้ววันหนึ่งจะมีเทสต์ที่ผ่านเพราะ fixture ของมันเอง
/// ผิดตรงกับโค้ด (`docs/08 §3.9` ข้อ 14)
#[cfg(test)]
pub(crate) mod fixtures {
    // ★ fixture ที่เขียนผิดต้อง **ล้มเสียงดัง** ไม่ใช่เงียบแล้วปล่อยไฟล์พิการ
    //   ให้เทสต์ไปวัด — ไฟล์ที่ decoder ปล่อยผ่านทั้งที่ EXIF เพี้ยนคือของปลอม
    //   ที่ทำให้เทสต์เขียวโดยไม่ได้ตรวจอะไร (`docs/08 §3.9` ข้อ 14)
    #![allow(clippy::unwrap_used)]

    use image::RgbaImage;

    /// บล็อก TIFF/EXIF ที่เล็กที่สุดที่ประกาศ Orientation หนึ่งค่า
    ///
    /// เขียนเองเพราะ `image` ไม่เขียน EXIF ให้ และการเทียบกับ EXIF **จริง**
    /// คือสิ่งเดียวที่พิสูจน์ว่าทางลัดไม่ได้ทำให้ภาพจากมือถือตะแคง
    #[must_use]
    pub(crate) fn exif_block(orientation: u16) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II"); // little-endian
        tiff.extend_from_slice(&42u16.to_le_bytes()); // magic
        tiff.extend_from_slice(&8u32.to_le_bytes()); // offset ของ IFD0
        tiff.extend_from_slice(&1u16.to_le_bytes()); // มี 1 entry
        tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // tag = Orientation
        tiff.extend_from_slice(&3u16.to_le_bytes()); // type = SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes()); // count
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]); // เติมช่องค่าให้ครบ 4 ไบต์
        tiff.extend_from_slice(&0u32.to_le_bytes()); // ไม่มี IFD ถัดไป
        tiff
    }

    /// JPEG จริงจาก `image` (ไม่มี EXIF) — ★ **ลายไม่สมมาตร** เพื่อให้การหมุน
    /// เปลี่ยนพิกเซลจริง · ภาพสีเดียวหมุนแล้วเหมือนเดิมทุกประการ แล้วเทสต์ที่
    /// เทียบพิกเซลจะผ่านทั้งที่ไม่มีใครหมุนอะไรเลย
    #[must_use]
    pub(crate) fn plain_jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, y| {
            // ครึ่งบนสว่าง ครึ่งล่างมืด + ไล่สีตามแกน x → หมุน 90° แล้วต่างแน่นอน
            let top = u8::from(y * 2 < h) * 200;
            image::Rgba([top.saturating_add((x * 3) as u8), 90, 60, 255])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        out
    }

    /// แทรก APP1 ที่มี EXIF เข้าไปหลัง SOI ของ JPEG จริง
    #[must_use]
    pub(crate) fn jpeg_with_exif(orientation: u16, w: u32, h: u32) -> Vec<u8> {
        let base = plain_jpeg(w, h);
        let tiff = exif_block(orientation);
        let len = u16::try_from(2 + super::EXIF_ID.len() + tiff.len()).unwrap();

        let mut out = Vec::with_capacity(base.len() + usize::from(len) + 2);
        out.extend_from_slice(&super::JPEG_SOI);
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&super::EXIF_ID);
        out.extend_from_slice(&tiff);
        out.extend_from_slice(&base[2..]); // ที่เหลือของไฟล์เดิม (ข้าม SOI)
        out
    }

    /// ★★★ PNG ที่มี **`eXIf` chunk จริง** — เส้นทางที่ไม่ใช่ JPEG
    ///
    /// `read_orientation` มีสองทาง: ทางลัดของ JPEG (อ่าน APP1 เอง) กับทาง
    /// container ทั่วไป · **ทางที่สองไม่เคยมีเทสต์เดินผ่านเลย** จนกระทั่ง
    /// ประตู mutation ชี้ให้เห็น 12 ก.ย. 2026
    ///
    /// ตาม PNG spec ตัว chunk `eXIf` เก็บ **TIFF header ตรง ๆ** ไม่มี `Exif\0\0` นำ
    #[must_use]
    pub(crate) fn png_with_exif(orientation: u16, w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, 60, 255])
        });
        let mut base = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut base),
                image::ImageFormat::Png,
            )
            .unwrap();

        let tiff = exif_block(orientation);
        let mut chunk = Vec::with_capacity(12 + tiff.len());
        chunk.extend_from_slice(&u32::try_from(tiff.len()).unwrap().to_be_bytes());
        chunk.extend_from_slice(b"eXIf");
        chunk.extend_from_slice(&tiff);
        let mut crc_over = b"eXIf".to_vec();
        crc_over.extend_from_slice(&tiff);
        chunk.extend_from_slice(&crc32fast::hash(&crc_over).to_be_bytes());

        // แทรกไว้ก่อน IEND (12 ไบต์สุดท้าย)
        let cut = base.len() - 12;
        let mut out = Vec::with_capacity(base.len() + chunk.len());
        out.extend_from_slice(&base[..cut]);
        out.extend_from_slice(&chunk);
        out.extend_from_slice(&base[cut..]);
        out
    }
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

    // ---------- ทางลัด EXIF ของ JPEG ----------
    //
    // ★ ตัวสร้าง fixture อยู่ที่ `super::fixtures` — **ที่เดียว** เพราะ `pool.rs`
    //   ใช้ตัวเดียวกันเพื่อพิสูจน์ว่า orientation ถูกใช้จริงตลอดเส้นทาง
    use super::fixtures::{exif_block, plain_jpeg, png_with_exif};

    fn jpeg_with_exif(orientation: u16) -> Vec<u8> {
        super::fixtures::jpeg_with_exif(orientation, 32, 32)
    }

    /// ★ ทางลัดต้องยังอ่าน EXIF จริงได้ครบ ไม่ใช่แค่เร็วขึ้น
    ///
    /// ถ้าข้อนี้พัง ภาพจากมือถือจะตะแคงหมด ซึ่งนักวาดเห็นทันที
    #[test]
    fn jpeg_with_real_exif_is_still_read() {
        for (value, expected) in [
            (1u16, Orientation::Normal),
            (3, Orientation::Rotate180),
            (6, Orientation::Rotate90),
            (8, Orientation::Rotate270),
        ] {
            let jpeg = jpeg_with_exif(value);
            assert_eq!(
                read_orientation(&jpeg),
                expected,
                "EXIF orientation {value} อ่านไม่ได้"
            );
            // ไฟล์ที่แทรก APP1 แล้วต้องยังเป็น JPEG ที่ decode ได้ปกติ
            assert_eq!(
                image::guess_format(&jpeg).unwrap(),
                image::ImageFormat::Jpeg
            );
        }
    }

    /// ★★★ **EXIF ของไฟล์ที่ไม่ใช่ JPEG** — กิ่งที่ไม่มีใครเคยเดินผ่าน
    ///
    /// `read_orientation` มีสองทาง: ทางลัดของ JPEG (อ่าน APP1 เอง) กับทาง
    /// container ทั่วไปสำหรับ PNG/TIFF/WebP · เทสต์ทั้งหมดที่มีอยู่เดินแต่ทางแรก
    /// → ประตู mutation ชี้ว่า **ทำให้ทางที่สองคืน `Normal` เสมอ แล้วไม่มีอะไรแดง**
    /// (12 ก.ย. 2026)
    ///
    /// อาการถ้าพัง: ภาพ PNG ที่ export จากมือถือ/กล้องมาพร้อม orientation
    /// จะตะแคงบน board ทั้งที่ไฟล์บอกไว้ชัด
    #[test]
    fn a_png_carries_its_orientation_too_not_just_jpeg() {
        for (value, expected) in [
            (1u16, Orientation::Normal),
            (3, Orientation::Rotate180),
            (6, Orientation::Rotate90),
            (8, Orientation::Rotate270),
        ] {
            let png = png_with_exif(value, 24, 16);
            // ★ ประตูของประตู: ไฟล์ต้องยังเป็น PNG ที่ decode ได้ ไม่งั้นเราวัด
            //   "ไฟล์พังแล้วคืน Normal" ซึ่งจริงตลอดกาล
            assert_eq!(
                image::guess_format(&png).unwrap(),
                image::ImageFormat::Png,
                "fixture ไม่ใช่ PNG แล้ว"
            );
            assert!(
                image::load_from_memory(&png).is_ok(),
                "แทรก eXIf แล้ว PNG เปิดไม่ได้ — fixture ผิด ไม่ใช่โค้ดผิด"
            );
            // ★★ และต้อง **ไม่** เดินทางลัดของ JPEG
            assert!(jpeg_exif_payload(&png).is_none());

            assert_eq!(
                read_orientation(&png),
                expected,
                "PNG ที่ประกาศ orientation {value} อ่านไม่ได้"
            );
        }
    }

    #[test]
    fn jpeg_without_exif_reports_normal() {
        let jpeg = plain_jpeg(64, 64);
        assert!(jpeg_exif_payload(&jpeg).is_none(), "ไฟล์นี้ไม่ควรมี APP1/EXIF");
        assert_eq!(read_orientation(&jpeg), Orientation::Normal);
    }

    /// ★ หลักฐานว่าทางลัดหยุดที่ SOS จริง ไม่ได้ไล่ทั้งไฟล์
    ///
    /// ต่อ entropy data ปลอมยาว ๆ ท้ายไฟล์ ถ้ายังเดินทั้งก้อนอยู่ เวลาจะโตตามขนาด
    /// เทียบเป็นอัตราส่วนกับไฟล์เล็ก จึงไม่ผูกกับความเร็วของเครื่องที่รันเทสต์
    #[test]
    fn exif_lookup_does_not_scan_the_whole_file() {
        use std::time::Instant;

        let small = plain_jpeg(64, 64);
        let mut large = small.clone();
        // 0xFF สลับค่าอื่น = กรณีที่แพงที่สุดของตัวสแกนเดิม (จอง Vec ทุกไบต์ที่เจอ 0xFF)
        large.extend(std::iter::repeat_n([0xFFu8, 0x00], 2_000_000).flatten());

        let run = |data: &[u8]| {
            let start = Instant::now();
            for _ in 0..20 {
                assert_eq!(read_orientation(data), Orientation::Normal);
            }
            start.elapsed()
        };

        let small_time = run(&small).max(std::time::Duration::from_nanos(1));
        let large_time = run(&large);
        let ratio = large_time.as_secs_f64() / small_time.as_secs_f64();

        assert!(
            ratio < 10.0,
            "ไฟล์ใหญ่กว่า ~2000 เท่าแต่ใช้เวลามากกว่า {ratio:.1} เท่า — \
             แปลว่ายังไล่สแกนทั้งไฟล์อยู่ ({small_time:?} → {large_time:?})"
        );
    }

    /// I-4: JPEG ที่เพี้ยนทุกแบบต้องได้ `None`/`Normal` ไม่ใช่ panic หรือค้าง
    #[test]
    fn broken_jpeg_headers_never_panic() {
        let full = jpeg_with_exif(6);
        let cases: Vec<Vec<u8>> = vec![
            JPEG_SOI.to_vec(),
            vec![0xFF, 0xD8, 0xFF],
            vec![0xFF, 0xD8, 0xFF, 0xE1],
            vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF], // ความยาวใหญ่กว่าไฟล์
            vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x00], // ความยาว 0 (น้อยกว่า 2)
            vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01], // ความยาว 1
            vec![0xFF, 0xD8, 0x12, 0x34],             // ไม่มี 0xFF ตรงที่ควรมี
            vec![0xFF, 0xD8, 0xFF, 0xD8],             // SOI ซ้อน
        ];
        for (i, case) in cases.iter().enumerate() {
            // สนแค่ว่า "ต้องกลับมาได้" ไม่ panic ไม่วนไม่จบ
            let _ = jpeg_exif_payload(case);
            assert_eq!(read_orientation(case), Orientation::Normal, "เคส {i}");
        }

        // ตัดไฟล์ที่มี EXIF จริงทุกความยาว — เจอ header ครึ่ง ๆ กลาง ๆ ทุกแบบ
        // ยังไม่ครบบล็อก APP1 → ต้องได้ Normal · ครบแล้ว → ต้องอ่านค่าได้ตามปกติ
        // แม้เนื้อภาพข้างหลังจะขาดไปทั้งก้อน (ทางลัดไม่แตะส่วนนั้นอยู่แล้ว)
        let app1_end = JPEG_SOI.len() + 2 + 2 + EXIF_ID.len() + exif_block(6).len();
        for cut in 0..full.len().min(80) {
            let part = &full[..cut];
            let expected = if cut >= app1_end {
                Orientation::Rotate90
            } else {
                Orientation::Normal
            };
            assert_eq!(read_orientation(part), expected, "ตัดที่ {cut} ไบต์");
        }
    }

    /// EXIF ที่อยู่หลัง SOS ถือว่าไม่มี (ผิดสเปก JPEG) แต่ต้องไม่ทำให้พัง
    #[test]
    fn exif_after_sos_is_ignored_not_crashed() {
        let mut jpeg = plain_jpeg(32, 32);
        jpeg.extend_from_slice(&[0xFF, 0xE1]);
        let tiff = exif_block(6);
        let len = u16::try_from(2 + EXIF_ID.len() + tiff.len()).unwrap();
        jpeg.extend_from_slice(&len.to_be_bytes());
        jpeg.extend_from_slice(&EXIF_ID);
        jpeg.extend_from_slice(&tiff);

        assert_eq!(read_orientation(&jpeg), Orientation::Normal);
    }
}
