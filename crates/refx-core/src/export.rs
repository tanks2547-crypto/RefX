//! แผนของการ export ภาพ — **คณิตศาสตร์ล้วน** ไม่มี GPU ไม่มีตัวเข้ารหัส ไม่แตะดิสก์
//!
//! ★★★ **ทำไมอยู่ใน `refx-core`**
//!
//! แผนนี้เป็น *สัญญาระหว่างสองฝั่ง*: ฝั่ง render (`refx-render`) ต้องรู้ว่า tile
//! รูปร่างไหน · ฝั่งเข้ารหัส (`refx-asset`) ต้องรู้ว่าแถบสูงเท่าไหร่ · ถ้าสองฝั่ง
//! คำนวณเองคนละที่ วันที่มีคนแก้ข้างเดียว **บัฟเฟอร์กับสิ่งที่เขียนลงไปจะไม่เท่ากัน
//! แล้วภาพที่ผู้ใช้ได้จะเหลื่อมทีละแถบโดยไม่มี error ที่ไหนเลย** — เป็นรูปแบบเดียว
//! กับบั๊ก `crop_uv` ที่ picker เดินย้อนทางเองแล้วเพี้ยน (ดู `refx-ui/instances.rs`)
//!
//! ทั้งสอง crate พึ่ง `refx-core` อยู่แล้ว และที่นี่ไม่มี dependency ใดเพิ่ม
//!
//! ## ★★ หน่วยความจำคือข้อจำกัดจริง ไม่ใช่ texture limit
//!
//! `16384 × 16384 × RGBA = 1 GB` ในบัฟเฟอร์เดียว — ทะลุงบทุกข้อของโปรเจกต์นี้
//! → ห้ามประกอบภาพเต็มใน RAM · render ทีละ **แถบ** แล้วป้อนตัวเข้ารหัสทันที
//!
//! ★ แถบกิน RAM เป็น **O(ความกว้าง)** ไม่ใช่ค่าคงที่ → ความสูงของแถบต้องคำนวณ
//! จากความกว้างที่ผู้ใช้เลือก ไม่ใช่ค่าคงที่ ไม่งั้นเพดานจะไม่คงที่จริง
//!
//! spec: docs/07-file-format.md §6

use crate::geom::Rect;

/// ด้านที่ยาวที่สุดที่ export ได้ (พิกเซล) — `docs/07 §6`
///
/// ★ นี่คือเพดานของ **ทางเขียน** (RAM/GPU) · เพดานที่ผู้ใช้เจอจริงมักต่ำกว่านี้
/// เพราะถูกจำกัดด้วยพิกเซลที่มีจริง — ดู [`max_export_side`]
pub const MAX_SIDE: u32 = 16384;

/// ด้านยาวสุดที่เล็กที่สุดที่ยังยอมให้เลือก
///
/// ★ ต่ำกว่านี้ภาพเล็กจนไม่มีใครใช้ · board ที่มีภาพใหญ่มากเมื่อเทียบกับผังทั้งหมด
/// จะคำนวณเพดานได้ต่ำกว่านี้ — ในกรณีนั้นเรายอมให้ขยายเล็กน้อยดีกว่าให้กล่อง
/// ที่เลือกอะไรไม่ได้เลย (สภาพที่ผู้ใช้ทำอะไรต่อไม่ได้ แย่กว่าภาพที่นุ่มนิดหน่อย)
pub const MIN_EXPORT_SIDE: u32 = 256;

/// ★★★ เพดานด้านยาวสุดที่ **พิกเซลที่มีจริงรองรับได้** (`docs/07 §6` · ตัดสิน 9 ก.ย. 2026)
///
/// ## ทำไมต้องมี
///
/// ภาพที่ส่งออกถูกวาดจากแหล่งพิกเซลที่มีความละเอียดจำกัด · ถ้าปล่อยให้เลือก
/// ขนาดใหญ่กว่าที่แหล่งนั้นให้ได้ ผลคือ **การขยายภาพ** ซึ่งนักวาดแยกออกจาก
/// "ต้นฉบับความละเอียดต่ำ" ด้วยตาไม่ได้ · และเขาจะรู้ว่าพลาดหลังรอ export
/// เสร็จแล้วส่งไฟล์ให้ลูกค้าไปแล้ว → **ปุ่มที่กดแล้วได้ผลแย่เสมอ ไม่ควรมีให้กด**
///
/// ## สูตร
///
/// อัตราขยายของ export คือ `s = ด้านยาวของภาพ / ด้านยาวของกรอบ world` ·
/// item ที่กว้างที่สุด `biggest` จะกินพื้นที่ `biggest × s` จุดในภาพปลายทาง
/// ซึ่งต้องไม่เกินพิกเซลที่แหล่งมีให้ (`source_px`)
///
/// ```text
/// biggest × (long / region_long) ≤ source_px
/// long ≤ region_long × source_px / biggest
/// ```
///
/// ★★ **คำนวณ ไม่ใช่ค่าคงที่** — วันที่แหล่งพิกเซลดีขึ้น (P5-4b) เพดานยกขึ้นเอง
/// โดยไม่ต้องแตะ UI เลย เพราะมันเป็นคุณสมบัติของแหล่ง ไม่ใช่ตัวเลขที่เขียนตายไว้
///
/// `biggest_item_side` = ด้านที่ยาวที่สุดของ item ที่ **มีพิกเซลจริง** ใน world
/// (`Missing`/ข้อความไม่นับ — มันเป็นสี่เหลี่ยมสีล้วน ขยายแล้วไม่เสียอะไร)
/// · `0` หรือค่าที่ไม่ finite = ไม่มีอะไรจำกัด → ได้ [`MAX_SIDE`]
#[must_use]
pub fn max_export_side(region_long: f32, biggest_item_side: f32, source_px: u32) -> u32 {
    if !region_long.is_finite()
        || !biggest_item_side.is_finite()
        || region_long <= 0.0
        || biggest_item_side <= 0.0
        || source_px == 0
    {
        return MAX_SIDE;
    }
    let allowed = region_long / biggest_item_side * source_px as f32;
    if !allowed.is_finite() || allowed <= 0.0 {
        return MAX_SIDE;
    }
    // ★ ปัดลง — ปัดขึ้นแปลว่ายอมให้เกินเพดานที่เพิ่งคำนวณมาหนึ่งจุด
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamp เข้าช่วง MIN..=MAX ทันทีหลังแปลง"
    )]
    let allowed = allowed.floor() as u32;
    allowed.clamp(MIN_EXPORT_SIDE, MAX_SIDE)
}

/// งบของบัฟเฟอร์แถบหนึ่งแถบ (ไบต์) — `docs/07 §6`
pub const BAND_BUDGET: usize = 32 << 20;

/// เพดานรวมที่ต้องพิสูจน์: บัฟเฟอร์แถบ + staging ของ readback (ไบต์)
///
/// ★ เพดานนี้ **ไม่ใช่ตัวเลขบนกระดาษ** — `docs/07 §6` บังคับให้พิสูจน์ด้วย RSS จริง
/// ตอน export ด้วย (ดู `refx-ui::export` เทสต์ `what_an_export_really_costs_in_process_memory`)
pub const PEAK_CEILING: usize = 64 << 20;

/// ความกว้างสูงสุดของ tile ที่ render ต่อครั้ง (พิกเซล)
///
/// ★ ไม่ใช่ข้อจำกัดของ RAM แต่เป็นขนาดที่ **การ์ดจอทุกใบที่เรารองรับสร้างได้แน่**
/// (`max_texture_dimension_2d` ขั้นต่ำของ wgpu downlevel คือ 8192 · 4096 จึงเหลือขอบ)
pub const TILE_WIDTH: u32 = 4096;

/// ความสูงต่ำสุดของแถบ — บล็อกของ JPEG สูง 8 แถว
///
/// ★ ในทางปฏิบัติ**ไม่มีวันถูกใช้**: ที่ความกว้างสูงสุด 16384 แถบยังได้ 512 แถว
/// (ดูเทสต์ `the_floor_is_unreachable_at_every_size_we_allow`) · มีไว้เพื่อให้
/// สูตรไม่คืน 0 ถ้าวันหนึ่งเพดานความกว้างถูกยกขึ้น
pub const MIN_BAND_ROWS: u32 = 8;

/// ความสูงสูงสุดของแถบ — สูงกว่านี้ไม่ได้อะไรเพิ่มนอกจาก RAM ที่นอนเปล่า
pub const MAX_BAND_ROWS: u32 = 1024;

/// ไบต์ต่อพิกเซลของภาพที่ render ออกมา (RGBA8)
pub const BYTES_PER_PIXEL: u32 = 4;

/// `bytes_per_row` ของ `copy_texture_to_buffer` ต้องหารค่านี้ลงตัว (ข้อบังคับของ wgpu)
///
/// อยู่ที่นี่เพราะ **การคำนวณเพดานต้องรู้ค่านี้** — ถ้าคำนวณเพดานโดยลืม padding
/// ตัวเลขที่ได้จะต่ำกว่าความจริง ซึ่งเป็นตัวเลขที่พาไปตัดสินใจผิด
pub const COPY_ROW_ALIGN: u32 = 256;

/// ขนาดที่ export ไม่ได้
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// ด้านใดด้านหนึ่งเป็นศูนย์ — ไม่มีอะไรให้เขียน
    #[error("the export size {width}x{height} has no pixels")]
    Empty {
        /// ความกว้างที่ขอ
        width: u32,
        /// ความสูงที่ขอ
        height: u32,
    },
    /// เกินเพดาน [`MAX_SIDE`]
    #[error("the export size {width}x{height} is larger than the {MAX_SIDE} px limit")]
    TooLarge {
        /// ความกว้างที่ขอ
        width: u32,
        /// ความสูงที่ขอ
        height: u32,
    },
}

/// แถบหนึ่งแถบของภาพปลายทาง
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    /// แถวแรกของแถบนี้ในภาพปลายทาง
    pub y0: u32,
    /// จำนวนแถวในแถบนี้ — **แถบสุดท้ายเตี้ยกว่าตัวอื่นได้**
    pub rows: u32,
}

/// tile หนึ่งใบภายในแถบ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    /// คอลัมน์แรกของ tile นี้ในภาพปลายทาง
    pub x0: u32,
    /// ความกว้างของ tile นี้ — **ใบสุดท้ายแคบกว่าตัวอื่นได้**
    pub width: u32,
}

/// แผนของ export หนึ่งงาน — ตอบได้ทุกคำถามเรื่องขนาดโดยไม่ต้องมี GPU
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandPlan {
    width: u32,
    height: u32,
    band_rows: u32,
}

impl BandPlan {
    /// วางแผน export ขนาด `width × height`
    ///
    /// # Errors
    /// [`PlanError`] เมื่อขนาดเป็นศูนย์หรือเกิน [`MAX_SIDE`]
    pub fn new(width: u32, height: u32) -> Result<Self, PlanError> {
        if width == 0 || height == 0 {
            return Err(PlanError::Empty { width, height });
        }
        if width > MAX_SIDE || height > MAX_SIDE {
            return Err(PlanError::TooLarge { width, height });
        }
        Ok(Self {
            width,
            height,
            band_rows: band_rows_for(width, height),
        })
    }

    /// ความกว้างของภาพปลายทาง
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// ความสูงของภาพปลายทาง
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// ความสูงของแถบเต็ม (แถบสุดท้ายเตี้ยกว่าได้)
    #[must_use]
    pub fn band_rows(&self) -> u32 {
        self.band_rows
    }

    /// จำนวนแถบทั้งหมด
    #[must_use]
    pub fn band_count(&self) -> u32 {
        self.height.div_ceil(self.band_rows)
    }

    /// แถบลำดับที่ `index` — `None` เมื่อเลยแถบสุดท้ายไปแล้ว
    #[must_use]
    pub fn band(&self, index: u32) -> Option<Band> {
        let y0 = index.checked_mul(self.band_rows)?;
        if y0 >= self.height {
            return None;
        }
        Some(Band {
            y0,
            rows: (self.height - y0).min(self.band_rows),
        })
    }

    /// จำนวน tile ที่ต้อง render ต่อหนึ่งแถบ
    #[must_use]
    pub fn tiles_across(&self) -> u32 {
        self.width.div_ceil(TILE_WIDTH)
    }

    /// tile ลำดับที่ `column` — `None` เมื่อเลยขอบขวาไปแล้ว
    #[must_use]
    pub fn tile(&self, column: u32) -> Option<Tile> {
        let x0 = column.checked_mul(TILE_WIDTH)?;
        if x0 >= self.width {
            return None;
        }
        Some(Tile {
            x0,
            width: (self.width - x0).min(TILE_WIDTH),
        })
    }

    /// ไบต์ของบัฟเฟอร์แถบ (ฝั่ง CPU · ความกว้างเต็ม)
    #[must_use]
    pub fn band_bytes(&self) -> usize {
        (self.width as usize)
            .saturating_mul(self.band_rows as usize)
            .saturating_mul(BYTES_PER_PIXEL as usize)
    }

    /// ไบต์ของบัฟเฟอร์ staging ที่ใหญ่ที่สุด (readback จาก GPU · หนึ่ง tile)
    ///
    /// ★ นับ padding ของ `bytes_per_row` ด้วย — wgpu บังคับให้หาร
    /// [`COPY_ROW_ALIGN`] ลงตัว ซึ่งทำให้ tile แคบ ๆ กินมากกว่าที่คิดหลายเท่า
    #[must_use]
    pub fn staging_bytes(&self) -> usize {
        let tile_width = self.width.min(TILE_WIDTH);
        let padded = padded_row_bytes(tile_width);
        (padded as usize).saturating_mul(self.band_rows as usize)
    }

    /// เพดาน RAM ที่แผนนี้ตั้งใจใช้ — **ต้องไม่เกิน [`PEAK_CEILING`]**
    ///
    /// ★ นี่คือ *ความตั้งใจ* ไม่ใช่หลักฐาน · หลักฐานคือ RSS จริงตอน export
    #[must_use]
    pub fn peak_bytes(&self) -> usize {
        self.band_bytes().saturating_add(self.staging_bytes())
    }
}

/// `bytes_per_row` หลังปัดขึ้นให้หาร [`COPY_ROW_ALIGN`] ลงตัว
#[must_use]
pub fn padded_row_bytes(width: u32) -> u32 {
    let raw = width.saturating_mul(BYTES_PER_PIXEL);
    raw.div_ceil(COPY_ROW_ALIGN).saturating_mul(COPY_ROW_ALIGN)
}

/// ความสูงของแถบที่ทำให้บัฟเฟอร์อยู่ใน [`BAND_BUDGET`]
///
/// ★★ ปัดลงให้เป็นพหุคูณของ 8 — บล็อกของ JPEG สูง 8 แถว · ตัวเข้ารหัสอ่านภาพ
/// ทีละแถวบล็อก ถ้าขอบแถบตกกลางบล็อก มันจะขอแถวที่อยู่คนละแถบสลับไปมา
/// ทำให้ต้อง render ซ้ำ (ผลยังถูก แต่ช้าเป็นเท่าตัว — ดู `refx-asset::export`
/// ที่นับจำนวนครั้งที่ถูกขอย้อนหลังไว้เป็นหลักฐานถาวร)
fn band_rows_for(width: u32, height: u32) -> u32 {
    let per_row = (width as usize)
        .saturating_mul(BYTES_PER_PIXEL as usize)
        .max(1);
    let raw = (BAND_BUDGET / per_row).min(MAX_BAND_ROWS as usize) as u32;
    let aligned = (raw / 8) * 8;
    aligned.max(MIN_BAND_ROWS).min(height)
}

/// affine `world → clip` ของ tile หนึ่งใบตอน export
///
/// ★★★ **ทำไมไม่ใช้ [`crate::view::Camera::to_clip_affine`] ตรง ๆ**
///
/// `Camera` clamp ซูมไว้ที่ `[MIN_ZOOM, MAX_ZOOM]` ซึ่งถูกต้องสำหรับกล้องบนจอ
/// (ผู้ใช้ซูมเกินนั้นไม่มีประโยชน์) แต่ **ผิดสำหรับ export**: ผู้ใช้เลือกขนาดภาพ
/// ปลายทางเองได้ board เล็ก ๆ ที่ export ที่ 16384 px จะได้อัตราขยายเกิน 64 เท่า
/// แล้วการ clamp จะทำให้ได้ภาพที่**ไม่ตรงกับขนาดที่ขอ** โดยไม่มี error ที่ไหนเลย
///
/// ★ ทั้งสองสูตรถูกยึดเข้าหากันด้วยเทสต์ `the_two_camera_formulas_never_drift`
/// — ถ้ามีใครแก้ข้างเดียว เทสต์นั้นแดงทันที
///
/// `region` คือกรอบใน world ที่จะถูก export · `out` คือขนาดภาพปลายทาง (พิกเซล)
/// · `tile_origin`/`tile_size` คือตำแหน่งและขนาดของ tile ในภาพปลายทางนั้น
#[must_use]
pub fn tile_clip_affine(
    region: Rect,
    out: glam::Vec2,
    tile_origin: glam::Vec2,
    tile_size: glam::Vec2,
) -> [f32; 6] {
    /// ค่าที่ไม่ finite หรือ ≤ 0 ห้ามกลายเป็นตัวหาร — คืน 1.0 แทน (I-4)
    fn positive(value: f32) -> f32 {
        if value.is_finite() && value > 0.0 {
            value
        } else {
            1.0
        }
    }
    fn finite(value: f32) -> f32 {
        if value.is_finite() { value } else { 0.0 }
    }

    let size = region.size();
    let scale_x = positive(out.x) / positive(size.x);
    let scale_y = positive(out.y) / positive(size.y);
    let tile_w = positive(tile_size.x);
    let tile_h = positive(tile_size.y);

    let a = 2.0 * scale_x / tile_w;
    let d = -2.0 * scale_y / tile_h;
    let tx = -2.0 * (scale_x * finite(region.min.x) + finite(tile_origin.x)) / tile_w - 1.0;
    let ty = 2.0 * (scale_y * finite(region.min.y) + finite(tile_origin.y)) / tile_h + 1.0;
    [a, 0.0, 0.0, d, tx, ty]
}

#[cfg(test)]
mod tests {
    // เทียบ float ตรง ๆ ได้ในเทสต์: ค่าที่ assert คือค่าคงที่หลัง clamp/ประกอบ struct
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use glam::Vec2;

    use super::*;

    // ---------- ★★★ เพดานที่มาจากพิกเซลที่มีจริง (`docs/07 §6`) ----------

    /// ★★★ **NC ของสูตร: ปลอมให้แหล่งพิกเซลดีขึ้น → เพดานต้องขยับตาม**
    ///
    /// ถ้าเพดานไม่ขยับ แปลว่ามันเป็นค่าคงที่ที่แต่งหน้าเป็นสูตร — และวันที่
    /// P5-4b ลงจอด เราจะยังติดอยู่ที่เพดานเดิมโดยไม่มีใครรู้ว่าทำไม
    #[test]
    fn the_ceiling_rises_when_the_pixel_source_gets_better() {
        // board ที่ item ใหญ่สุดกิน 1/10 ของผัง
        let (region_long, biggest) = (5000.0_f32, 500.0_f32);

        let today = max_export_side(region_long, biggest, 128); // atlas 128 px
        let with_512 = max_export_side(region_long, biggest, 512);
        let with_2048 = max_export_side(region_long, biggest, 2048);

        println!("แหล่ง 128 → {today} px · 512 → {with_512} px · 2048 → {with_2048} px");
        assert_eq!(today, 1280, "128 × (5000/500) = 1280");
        assert_eq!(with_512, 5120, "ดีขึ้นสี่เท่า เพดานต้องขึ้นสี่เท่า");
        assert_eq!(with_2048, 16384, "ดีพอจนชนเพดานของทางเขียน");
        assert!(
            with_512 > today && with_2048 > with_512,
            "เพดานไม่ขยับตามแหล่งพิกเซล = เป็นค่าคงที่ที่แต่งหน้าเป็นสูตร"
        );
    }

    /// ★★ ผังของ board ก็เป็นตัวแปร ไม่ใช่แค่ความละเอียดของแหล่ง
    ///
    /// board ที่ภาพใบเดียวกินเต็มผัง ขยายไม่ได้เลย · board ที่ภาพเล็กกระจายกว้าง
    /// ขยายได้มาก — ทั้งที่แหล่งพิกเซลเท่ากันเป๊ะ
    #[test]
    fn the_ceiling_follows_the_layout_too() {
        let fills_everything = max_export_side(1000.0, 1000.0, 128);
        let spread_out = max_export_side(10_000.0, 500.0, 128);
        println!("ภาพเดียวเต็มผัง → {fills_everything} px · กระจายกว้าง → {spread_out} px");
        assert_eq!(fills_everything, MIN_EXPORT_SIDE, "128 ถูกดันขึ้นเป็นพื้นล่าง");
        assert_eq!(spread_out, 2560);
        assert!(spread_out > fills_everything);
    }

    /// ★ ไม่มีอะไรจำกัด (board ที่มีแต่ `Missing`) → ได้เพดานของทางเขียน
    #[test]
    fn a_board_with_no_real_pixels_is_not_limited_by_them() {
        assert_eq!(max_export_side(5000.0, 0.0, 128), MAX_SIDE);
        assert_eq!(max_export_side(5000.0, f32::NAN, 128), MAX_SIDE);
        assert_eq!(max_export_side(f32::INFINITY, 100.0, 128), MAX_SIDE);
        // แหล่งที่ไม่มีพิกเซลเลยเป็นสภาพที่เป็นไปไม่ได้ — อย่าไปจำกัดผู้ใช้เพราะมัน
        assert_eq!(max_export_side(5000.0, 100.0, 0), MAX_SIDE);
    }

    /// ★★ เพดานต้องอยู่ในช่วงที่ `BandPlan` รับได้เสมอ ไม่ว่าตัวเลขจะบ้าแค่ไหน
    #[test]
    fn the_ceiling_is_always_a_size_the_writer_can_actually_produce() {
        for (region, biggest) in [
            (1.0_f32, 1_000_000.0_f32),
            (1_000_000.0, 1.0),
            (0.001, 0.001),
            (f32::MAX, f32::MIN_POSITIVE),
        ] {
            let side = max_export_side(region, biggest, 128);
            assert!(
                (MIN_EXPORT_SIDE..=MAX_SIDE).contains(&side),
                "{region} / {biggest} → {side} ซึ่งอยู่นอกช่วงที่เขียนได้"
            );
            assert!(BandPlan::new(side, side).is_ok(), "{side} วางแผนไม่ได้");
        }
    }

    #[test]
    fn a_size_with_no_pixels_is_refused() {
        assert!(matches!(
            BandPlan::new(0, 100),
            Err(PlanError::Empty { .. })
        ));
        assert!(matches!(
            BandPlan::new(100, 0),
            Err(PlanError::Empty { .. })
        ));
    }

    #[test]
    fn a_size_over_the_limit_is_refused() {
        assert!(BandPlan::new(MAX_SIDE, MAX_SIDE).is_ok());
        assert!(matches!(
            BandPlan::new(MAX_SIDE + 1, 100),
            Err(PlanError::TooLarge { .. })
        ));
        assert!(matches!(
            BandPlan::new(100, MAX_SIDE + 1),
            Err(PlanError::TooLarge { .. })
        ));
    }

    /// ★★★ **เพดานต้องคงที่ ไม่ขึ้นกับขนาดที่ผู้ใช้เลือก** (`docs/07 §6`)
    ///
    /// ข้อนี้คือทั้งหมดของ P5-4 ในบรรทัดเดียว · ถ้าใครเปลี่ยนสูตรความสูงแถบเป็น
    /// ค่าคงที่ (ซึ่งดูสมเหตุสมผลมาก) เพดานจะไต่ตามความกว้างทันที
    #[test]
    fn the_memory_ceiling_never_moves_with_the_size_the_user_picks() {
        let mut worst = 0usize;
        for width in [
            1u32, 64, 255, 256, 1000, 4096, 4097, 6000, 8192, 12000, 16384,
        ] {
            for height in [1u32, 8, 1000, 16384] {
                let plan = BandPlan::new(width, height).unwrap();
                let peak = plan.peak_bytes();
                worst = worst.max(peak);
                assert!(
                    peak <= PEAK_CEILING,
                    "{width}x{height}: เพดาน {} MB เกิน {} MB",
                    peak >> 20,
                    PEAK_CEILING >> 20
                );
            }
        }
        println!("เพดานสูงสุดที่พบ: {} MB", worst >> 20);
        // ★ ต้องเข้าใกล้เพดานพอควร ไม่งั้นแปลว่าเราจองน้อยเกินไปจนช้าฟรี ๆ
        assert!(worst > PEAK_CEILING / 4, "ใช้งบไปแค่ {} MB", worst >> 20);
    }

    #[test]
    fn the_band_buffer_stays_inside_its_own_budget() {
        for width in [1u32, 4096, 8192, 16384] {
            let plan = BandPlan::new(width, MAX_SIDE).unwrap();
            assert!(
                plan.band_bytes() <= BAND_BUDGET,
                "{width}: แถบกิน {} MB เกินงบ {} MB",
                plan.band_bytes() >> 20,
                BAND_BUDGET >> 20
            );
        }
    }

    /// ★ พื้นล่าง 8 แถวไม่มีวันถูกใช้ที่ขนาดที่เรารองรับ — บันทึกไว้ให้ชัด
    /// ว่าเป็นเกราะกันสูตรคืน 0 ไม่ใช่ค่าที่ทำงานอยู่จริง
    #[test]
    fn the_floor_is_unreachable_at_every_size_we_allow() {
        let plan = BandPlan::new(MAX_SIDE, MAX_SIDE).unwrap();
        assert_eq!(plan.band_rows(), 512, "ความกว้างสูงสุดยังได้ 512 แถว");
        assert!(plan.band_rows() > MIN_BAND_ROWS);
    }

    /// แถบสุดท้ายเตี้ยกว่าตัวอื่นได้ และรวมกันต้องได้ความสูงพอดี **ไม่ขาดไม่เกิน**
    #[test]
    fn the_bands_cover_the_image_exactly_once() {
        for (width, height) in [(16384u32, 16384u32), (100, 1), (4096, 1025), (7, 4097)] {
            let plan = BandPlan::new(width, height).unwrap();
            let mut covered = 0u32;
            let mut index = 0u32;
            while let Some(band) = plan.band(index) {
                assert_eq!(band.y0, covered, "{width}x{height}: แถบไม่ต่อกัน");
                assert!(band.rows > 0);
                covered += band.rows;
                index += 1;
            }
            assert_eq!(covered, height, "{width}x{height}: ครอบไม่ครบ");
            assert_eq!(index, plan.band_count(), "{width}x{height}: นับแถบผิด");
        }
    }

    #[test]
    fn the_tiles_cover_each_band_exactly_once() {
        for width in [1u32, 4096, 4097, 16384] {
            let plan = BandPlan::new(width, 100).unwrap();
            let mut covered = 0u32;
            let mut column = 0u32;
            while let Some(tile) = plan.tile(column) {
                assert_eq!(tile.x0, covered);
                assert!(tile.width > 0 && tile.width <= TILE_WIDTH);
                covered += tile.width;
                column += 1;
            }
            assert_eq!(covered, width);
            assert_eq!(column, plan.tiles_across());
        }
    }

    /// ★★ ขอบของแถบต้องตกที่ขอบบล็อกของ JPEG เสมอ — ไม่งั้นตัวเข้ารหัสจะขอ
    /// แถวข้ามแถบไปมาแล้วต้อง render ซ้ำทั้งแถบ
    #[test]
    fn every_full_band_ends_on_a_jpeg_block_boundary() {
        for width in [1u32, 100, 4096, 9000, 16384] {
            let plan = BandPlan::new(width, MAX_SIDE).unwrap();
            assert_eq!(
                plan.band_rows() % 8,
                0,
                "{width}: แถบสูง {} แถว ตกกลางบล็อก",
                plan.band_rows()
            );
        }
    }

    /// ภาพที่เตี้ยกว่าหนึ่งแถบต้องไม่จองบัฟเฟอร์เผื่อแถวที่ไม่มีอยู่จริง
    #[test]
    fn a_short_image_does_not_reserve_rows_it_will_never_use() {
        let plan = BandPlan::new(4096, 10).unwrap();
        assert_eq!(plan.band_rows(), 10);
        assert_eq!(plan.band_count(), 1);
    }

    #[test]
    fn padded_rows_follow_the_gpu_alignment_rule() {
        assert_eq!(padded_row_bytes(1), 256, "4 ไบต์ต้องถูกดันขึ้นเป็น 256");
        assert_eq!(padded_row_bytes(64), 256, "256 ไบต์พอดี ไม่ต้องดัน");
        assert_eq!(padded_row_bytes(65), 512);
        assert_eq!(padded_row_bytes(4096), 16384);
        for width in 1..=512u32 {
            assert_eq!(padded_row_bytes(width) % COPY_ROW_ALIGN, 0, "{width}");
        }
    }

    // ---------- ★ กล้องของ export ----------

    fn project(affine: [f32; 6], p: Vec2) -> Vec2 {
        Vec2::new(
            affine[0] * p.x + affine[2] * p.y + affine[4],
            affine[1] * p.x + affine[3] * p.y + affine[5],
        )
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-4
    }

    /// tile ที่กินภาพทั้งใบ: มุมของ `region` ต้องไปอยู่ที่มุมของ clip space พอดี
    #[test]
    fn a_single_tile_maps_the_whole_region_onto_the_target() {
        let region = Rect::from_corners(Vec2::new(-50.0, 20.0), Vec2::new(150.0, 120.0));
        let out = Vec2::new(800.0, 400.0);
        let affine = tile_clip_affine(region, out, Vec2::ZERO, out);

        assert!(
            close(project(affine, region.min), Vec2::new(-1.0, 1.0)),
            "ซ้ายบนผิด: {:?}",
            project(affine, region.min)
        );
        assert!(
            close(project(affine, region.max), Vec2::new(1.0, -1.0)),
            "ขวาล่างผิด: {:?}",
            project(affine, region.max)
        );
    }

    /// ★★★ tile ที่อยู่กลางภาพต้องวาด **ส่วนของตัวเองเท่านั้น** ให้เต็มเป้า
    ///
    /// ถ้าข้อนี้ผิด ภาพที่ผู้ใช้ได้จะเป็นภาพเต็มซ้ำกันทุก tile ซึ่งเป็นความผิดพลาด
    /// ที่ดูไม่ออกจนกว่าจะ export ภาพที่กว้างเกิน 4096
    #[test]
    fn a_tile_in_the_middle_only_draws_its_own_slice() {
        let region = Rect::from_corners(Vec2::ZERO, Vec2::new(1000.0, 1000.0));
        let out = Vec2::new(8192.0, 8192.0);
        let tile_origin = Vec2::new(4096.0, 512.0);
        let tile_size = Vec2::new(4096.0, 512.0);
        let affine = tile_clip_affine(region, out, tile_origin, tile_size);

        // world ที่ตรงกับมุมซ้ายบนของ tile นี้ = out px (4096, 512) → world (500, 62.5)
        let world_tl = Vec2::new(500.0, 62.5);
        assert!(
            close(project(affine, world_tl), Vec2::new(-1.0, 1.0)),
            "มุมซ้ายบนของ tile ผิด: {:?}",
            project(affine, world_tl)
        );
        // มุมขวาล่างของ tile = out px (8192, 1024) → world (1000, 125)
        let world_br = Vec2::new(1000.0, 125.0);
        assert!(
            close(project(affine, world_br), Vec2::new(1.0, -1.0)),
            "มุมขวาล่างของ tile ผิด: {:?}",
            project(affine, world_br)
        );
    }

    /// ★★★ **สองสูตรกล้องต้องไม่มีวันเพี้ยนออกจากกัน**
    ///
    /// `Camera::to_clip_affine` (บนจอ) กับ [`tile_clip_affine`] (ตอน export) ต้อง
    /// ให้ผลเดียวกันเป๊ะเมื่อ region คือสิ่งที่กล้องเห็นพอดี · ถ้าไม่มีเทสต์นี้
    /// วันที่มีคนแก้ข้างเดียว **ภาพที่ export จะไม่ตรงกับภาพบนจอ** ซึ่งเป็นการ
    /// ผิดสัญญา WYSIWYG ที่ `docs/07 §6` เขียนไว้ โดยไม่มี error ที่ไหนเลย
    #[test]
    fn the_two_camera_formulas_never_drift() {
        for (center, zoom, viewport) in [
            (Vec2::ZERO, 1.0_f32, Vec2::new(800.0, 600.0)),
            (Vec2::new(123.0, -456.0), 2.5, Vec2::new(1920.0, 1080.0)),
            (Vec2::new(-7.5, 3.25), 0.25, Vec2::new(640.0, 480.0)),
        ] {
            let camera = crate::view::Camera::new(center, zoom);
            let on_screen = camera.to_clip_affine(viewport);
            let region = Rect::from_center_size(center, viewport / zoom);
            let exported = tile_clip_affine(region, viewport, Vec2::ZERO, viewport);

            for (index, (a, b)) in on_screen.iter().zip(exported).enumerate() {
                assert!(
                    (a - b).abs() < 1e-4,
                    "ช่อง {index} ต่างกัน: บนจอ {a} · export {b} (zoom {zoom})"
                );
            }
        }
    }

    /// ค่าจากไฟล์เป็น `NaN`/`inf` ได้ (I-4) — ห้ามหลุดไปถึง GPU
    #[test]
    fn no_broken_value_ever_reaches_the_gpu() {
        let bad = f32::NAN;
        for region in [
            Rect::from_corners(Vec2::new(bad, 0.0), Vec2::new(10.0, 10.0)),
            Rect::from_corners(Vec2::ZERO, Vec2::ZERO), // กว้าง 0 → หารศูนย์
            Rect::EMPTY,
        ] {
            let affine = tile_clip_affine(region, Vec2::new(100.0, 100.0), Vec2::ZERO, Vec2::ONE);
            assert!(
                affine.iter().all(|v| v.is_finite()),
                "ค่าเสียหลุดออกไป: {affine:?}"
            );
        }
    }
}
