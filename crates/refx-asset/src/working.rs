//! Working texture — ภาพความละเอียดกลางสำหรับตอนซูมเข้า (docs/04 §4 ชั้น B)
//!
//! thumbnail 128 px ที่ถูกยืดตอนซูมเข้าเบลอชัดเจน ชั้นนี้จึง decode ภาพใหม่
//! ที่ขนาด **power-of-two พอดีกับขนาดบนจอ** แล้วเก็บไว้ใน VRAM ภายใต้ budget + LRU
//!
//! ทำงานบน **decode worker** ไม่ใช่ UI thread (I-2)
//!
//! ### ★ mip chain สร้างบน CPU ไม่ใช่ compute shader
//!
//! `docs/04 §4` เขียนว่า "สร้างด้วย compute shader ตอนอัปโหลด" — ที่นี่สร้างบน CPU
//! ด้วย `fast_image_resize` ที่มีอยู่แล้วแทน ด้วยเหตุผลสามข้อ:
//!
//! 1. compute shader ต้องมี pipeline + bind group layout + storage texture เพิ่มอีกชุด
//!    ซึ่งทั้งหมดผูกกับ device และต้องสร้างใหม่ทุกครั้งที่กู้ device (P0-5)
//! 2. งานนี้เกิดบน worker ที่ว่างอยู่แล้ว ส่วน compute shader จะไปเบียดคิว GPU
//!    ที่ต้องวาดเฟรมให้ทันในจังหวะเดียวกับที่ผู้ใช้กำลังซูม
//! 3. ปริมาณงานเล็ก — mip ทั้งชั้นรวมกันคือ 33% ของ level 0 และ `fast_image_resize`
//!    มี SIMD อยู่แล้ว
//!
//! ถ้าภายหลังพบว่า CPU เป็นคอขวดจริงค่อยย้าย — แต่ต้องวัดก่อน

use image::RgbaImage;

use crate::resize::{self, Quality};

/// ขนาดเล็กสุดของ working texture
///
/// เป็น pow2 ตัวถัดจาก `THUMB_SIZE` (128) พอดี — ผลคือทุกภาพที่บนจอ **ใหญ่กว่า
/// 128 px** จะได้ working texture ตามเกณฑ์ใน docs/04 §4 ส่วนที่ ≤ 128 px
/// ยังใช้ thumbnail ใน atlas ต่อ ซึ่งคมพอแล้วที่ขนาดนั้นและไม่ต้องจ่าย
/// ต้นทุนคงที่ของ texture แยก (bind group + draw call ต่อภาพ)
pub const MIN_WORKING_SIZE: u32 = 256;

/// ขนาดใหญ่สุดของ working texture
///
/// 2048² RGBA + mip = ~21 MB ต่อภาพ · งบครึ่งบน 192 MB จึงถือได้ราว 9 ภาพพร้อมกัน
/// ใหญ่กว่านี้เป็นงานของชั้น C (full-res, สูงสุด 2 ภาพ) ซึ่งยังไม่ทำ
pub const MAX_WORKING_SIZE: u32 = 2048;

/// ภาพความละเอียดกลางพร้อม mip chain — พร้อมอัปโหลดขึ้น GPU
#[derive(Debug, Clone)]
pub struct WorkingImage {
    /// ความกว้าง/สูงของ level 0 (เป็น power of two เสมอ)
    pub size: u32,
    /// pixel ของแต่ละ mip level เรียงจากใหญ่ไปเล็ก จบที่ 1×1
    ///
    /// `levels[i]` มีขนาด `(size >> i)²  × 4` ไบต์
    pub levels: Vec<Vec<u8>>,
}

impl WorkingImage {
    /// จำนวน mip level
    #[must_use]
    pub fn mip_level_count(&self) -> u32 {
        u32::try_from(self.levels.len()).unwrap_or(1)
    }

    /// ไบต์รวมทุก level (ใช้ประเมิน VRAM)
    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.levels.iter().map(Vec::len).sum()
    }
}

/// ขนาด working texture ที่เหมาะกับภาพขนาด `on_screen_px` บนจอ
///
/// ปัดขึ้นเป็น power of two เพื่อให้จำนวนขนาดที่เป็นไปได้มีจำกัด — ไม่งั้นการขยับ
/// ซูมทีละนิดจะสั่ง decode ใหม่ทุกครั้งแล้วเผา CPU ทิ้งโดยผู้ใช้ไม่ได้อะไรเพิ่ม
///
/// คืน `None` เมื่อไม่ต้องใช้ working texture (เล็กเกินไป หรือภาพต้นฉบับเล็กกว่า
/// thumbnail อยู่แล้ว) — ผู้เรียกใช้ atlas ต่อไป
#[must_use]
pub fn working_size_for(on_screen_px: f32, source_max_side: u32) -> Option<u32> {
    if !on_screen_px.is_finite() || on_screen_px <= 0.0 {
        return None;
    }
    // ปัดขึ้นเป็น pow2 · `next_power_of_two` ของ 0 คือ 1 จึง clamp ก่อน
    let wanted = (on_screen_px.ceil() as u32).max(1).next_power_of_two();

    // ห้ามขยายเกินภาพต้นฉบับ — ได้แต่ความเบลอกับ VRAM ที่เสียเปล่า
    let cap = source_max_side.min(MAX_WORKING_SIZE);
    let size = wanted.min(cap);

    (size >= MIN_WORKING_SIZE).then_some(size)
}

/// ย่อภาพเป็น working texture ขนาด `size` พร้อม mip chain
///
/// ใช้ Triangle ตาม docs/05 §3 (Lanczos3 สงวนไว้ให้ thumbnail ซึ่งย่อแรงกว่ามาก
/// และเป็นสิ่งที่ผู้ใช้เห็นตลอดเวลา) — ที่ขนาดใกล้เคียงต้นฉบับ Triangle เร็วกว่า
/// และต่างกันแทบไม่เห็น
///
/// คืน `None` เมื่อภาพต้นทางว่างเปล่าหรือย่อไม่สำเร็จ
#[must_use]
pub fn build(image: &RgbaImage, size: u32) -> Option<WorkingImage> {
    if image.width() == 0 || image.height() == 0 || size == 0 {
        return None;
    }

    // ---- level 0: ย่อจากภาพต้นฉบับผ่านเส้นทางเดียวกับ thumbnail ----
    let mut levels = vec![resize::to_square(image, size, Quality::Fast)?];

    // ---- mip ที่เหลือ: box filter 2×2 เขียนเอง ----
    //
    // ★ ไม่เรียก `fast_image_resize` ตรงนี้โดยตั้งใจ — วัดแล้วว่าการเรียกมันเพิ่ม
    //   ในเส้นทางนี้ทำให้ binary โต **2.6 MB** (15.40 → 17.99 MB) ซึ่งเป็น 10%
    //   ของงบทั้งก้อน แลกกับงานที่ box filter ทำได้ดีพอ ๆ กันอยู่แล้ว
    //
    //   การเฉลี่ย 2×2 คือวิธีมาตรฐานของการสร้าง mip (เป็นสิ่งที่ GPU ทำให้เอง
    //   ตอน generate mipmap) เพราะย่อครึ่งพอดีทุกครั้ง ไม่ต้องใช้ filter kernel
    //   ที่ซับซ้อนกว่านี้ และให้ผลที่คาดเดาได้แน่นอน
    let mut current = size;
    while current > 1 {
        let next = current / 2;
        let previous = levels.last()?;
        levels.push(halve_box(previous, current, next)?);
        current = next;
    }

    Some(WorkingImage { size, levels })
}

/// ย่อภาพจัตุรัส RGBA8 ลงครึ่งหนึ่งด้วยการเฉลี่ย 2×2
///
/// คืน `None` ถ้า buffer ต้นทางสั้นกว่าที่ `side` ประกาศไว้ (I-4)
fn halve_box(src: &[u8], side: u32, next: u32) -> Option<Vec<u8>> {
    let side = side as usize;
    let next = (next as usize).max(1);
    if src.len() < side * side * 4 {
        return None;
    }

    let mut out = vec![0u8; next * next * 4];
    for y in 0..next {
        for x in 0..next {
            // สี่ pixel ต้นทางที่ยุบเป็น pixel เดียว
            let (sx, sy) = (x * 2, y * 2);
            let corners = [
                (sy * side + sx) * 4,
                (sy * side + (sx + 1).min(side - 1)) * 4,
                ((sy + 1).min(side - 1) * side + sx) * 4,
                ((sy + 1).min(side - 1) * side + (sx + 1).min(side - 1)) * 4,
            ];
            let target = (y * next + x) * 4;
            for channel in 0..4 {
                // บวกใน u16 แล้วหาร — บวกใน u8 จะล้นตั้งแต่สอง pixel แรก
                let sum: u16 = corners
                    .iter()
                    .map(|offset| u16::from(src[offset + channel]))
                    .sum();
                out[target + channel] = u8::try_from(sum / 4).unwrap_or(u8::MAX);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn gradient(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
        })
    }

    // ---------- การเลือกขนาด ----------

    #[test]
    fn size_rounds_up_to_power_of_two() {
        assert_eq!(working_size_for(300.0, 4000), Some(512));
        assert_eq!(working_size_for(512.0, 4000), Some(512));
        assert_eq!(working_size_for(513.0, 4000), Some(1024));
        assert_eq!(working_size_for(1025.0, 4000), Some(2048));
    }

    /// ★ เกณฑ์ของ docs/04 §4: ใหญ่กว่า thumbnail 128 px เมื่อไหร่ถึงขอ texture แยก
    ///
    /// การปัดขึ้นเป็น pow2 ทำให้ทุกอย่างที่ > 128 px ไปโผล่ที่ 256 พอดี
    /// ส่วน ≤ 128 px ยังใช้ atlas ต่อ เพราะ thumbnail คมพอแล้วที่ขนาดนั้น
    #[test]
    fn threshold_matches_the_thumbnail_size() {
        assert_eq!(working_size_for(128.0, 4000), None, "เท่า thumbnail พอดี");
        assert_eq!(working_size_for(100.0, 4000), None, "เล็กกว่า thumbnail");
        assert_eq!(working_size_for(129.0, 4000), Some(256), "ใหญ่กว่าแล้ว");
        assert_eq!(working_size_for(256.0, 4000), Some(256));
    }

    /// ★ ห้ามขยายเกินภาพต้นฉบับ — ได้แต่ความเบลอกับ VRAM ที่เสียเปล่า
    #[test]
    fn never_upscales_beyond_the_source() {
        // ภาพต้นฉบับ 400 px แต่ถูกซูมจนกินจอ 1500 px
        assert_eq!(working_size_for(1500.0, 400), Some(400));
        // ต้นฉบับเล็กกว่าเพดานล่าง → ไม่ต้องมี working texture เลย
        assert_eq!(working_size_for(1500.0, 200), None);
    }

    #[test]
    fn caps_at_max_working_size() {
        assert_eq!(working_size_for(9000.0, 16_384), Some(MAX_WORKING_SIZE));
    }

    /// ค่าที่ไม่ควรเกิดต้องไม่ทำให้ panic (I-4 — ค่ามาจาก transform ที่โหลดจากไฟล์ได้)
    #[test]
    fn broken_sizes_are_rejected() {
        for px in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(working_size_for(px, 4000), None, "px = {px}");
        }
        assert_eq!(working_size_for(500.0, 0), None);
    }

    /// ★ ซูมทีละนิดต้องไม่สั่ง decode ใหม่ทุกครั้ง
    ///
    /// นี่คือเหตุผลทั้งหมดที่ปัดเป็น power of two — ถ้าใช้ขนาดตรง ๆ การขยับซูม
    /// 1 % จะทำให้ทุกภาพบนจอถูก decode ใหม่หมด แล้วเผา CPU ทิ้งโดยไม่ได้อะไรเพิ่ม
    #[test]
    fn nearby_zoom_levels_reuse_the_same_size() {
        let sizes: Vec<Option<u32>> = (600..=700)
            .step_by(5)
            .map(|px| working_size_for(px as f32, 4000))
            .collect();
        assert!(
            sizes.iter().all(|s| *s == Some(1024)),
            "ขนาดเปลี่ยนระหว่างซูมทีละนิด: {sizes:?}"
        );
    }

    // ---------- mip chain ----------

    #[test]
    fn builds_a_complete_mip_chain_down_to_one_pixel() {
        let working = build(&gradient(1000, 800), 512).unwrap();
        assert_eq!(working.size, 512);
        // 512, 256, 128, 64, 32, 16, 8, 4, 2, 1
        assert_eq!(working.levels.len(), 10);
        assert_eq!(working.mip_level_count(), 10);

        let mut expected = 512u32;
        for (index, level) in working.levels.iter().enumerate() {
            assert_eq!(
                level.len(),
                (expected as usize) * (expected as usize) * 4,
                "level {index} ขนาดไม่ตรงกับ {expected}×{expected}"
            );
            expected /= 2;
        }
    }

    /// mip ทั้งชั้นต้องกินเพิ่มราว 1/3 ของ level 0 — ตัวเลขที่ใช้คำนวณงบ VRAM
    #[test]
    fn mip_chain_costs_about_a_third_extra() {
        let working = build(&gradient(600, 600), 256).unwrap();
        let level0 = working.levels[0].len();
        let rest: usize = working.levels[1..].iter().map(Vec::len).sum();
        let ratio = rest as f64 / level0 as f64;
        assert!(
            (0.30..=0.34).contains(&ratio),
            "mip เพิ่ม {ratio:.3} เท่าของ level 0"
        );
        assert_eq!(working.total_bytes(), level0 + rest);
    }

    /// ★ ภาพที่ย่อแล้วต้องยังมีเนื้อภาพจริง ไม่ใช่พื้นดำ
    ///
    /// resize ที่ล้มเหลวเงียบ ๆ จะได้ buffer ศูนย์ทั้งก้อน ซึ่งดู "สำเร็จ" ทุกอย่าง
    /// แต่ผู้ใช้เห็นสี่เหลี่ยมดำแทนภาพตอนซูมเข้า
    #[test]
    fn resized_levels_contain_real_pixels() {
        let working = build(&gradient(800, 600), 256).unwrap();
        for (index, level) in working.levels.iter().enumerate() {
            assert!(
                level.iter().any(|&byte| byte != 0),
                "level {index} เป็นศูนย์ทั้งก้อน"
            );
            // alpha ต้องทึบเหมือนต้นฉบับ
            assert!(
                level.chunks_exact(4).all(|px| px[3] > 200),
                "level {index} โปร่งใส"
            );
        }
    }

    #[test]
    fn tiny_and_broken_inputs_do_not_panic() {
        assert!(build(&gradient(1, 1), 256).is_some());
        assert!(build(&RgbaImage::new(0, 0), 256).is_none());
        assert!(build(&gradient(10, 10), 0).is_none());
    }

    /// ขนาด 1×1 มี mip เดียว — ลูปต้องจบ ไม่ใช่วนไม่รู้จบ
    #[test]
    fn single_pixel_target_terminates() {
        let working = build(&gradient(64, 64), 1).unwrap();
        assert_eq!(working.levels.len(), 1);
        assert_eq!(working.levels[0].len(), 4);
    }
}
