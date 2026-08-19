//! ★★ เข้ารหัสภาพที่วางจาก clipboard เป็น PNG เพื่อพักลง spool (P4-5)
//!
//! ## ทำไมต้อง encode เลย
//!
//! `arboard` คืน **RGBA ดิบ ไม่ใช่ไบต์ของไฟล์** — ภาพที่ผู้ใช้วางจึงไม่มีอะไร
//! บนดิสก์ที่ชี้ถึงมันได้ · ถ้าไม่เก็บไว้ที่ไหนเลย มันจะหายตอนปิดโปรแกรม
//! และ `AssetRef` จะชี้ไป path ที่ไม่มีอยู่จริง (`docs/07 §2` — I-3)
//!
//! ## ★★★ ต้องไม่หน่วงการที่ภาพขึ้นจอ
//!
//! `docs/07 §2` บังคับข้อนี้ไว้ตรง ๆ · thumbnail ถูกสร้างจาก RGBA ที่อยู่ใน RAM
//! แล้ว — **ภาพขึ้นจอได้โดยไม่ต้องรอ PNG เลย** · การ encode จึงเกิด
//! *หลัง* thumbnail พร้อม และอยู่บน worker เดียวกัน ซึ่งไม่ใช่ UI thread (I-2)
//!
//! ★ ราคาที่วัดได้อยู่ในเทสต์ `what_encoding_a_pasted_image_costs`
//!
//! ★★ ใช้ `CompressionType::Fast` **ไม่ใช่ค่าปริยาย** — ไฟล์นี้เป็นที่พัก
//! ไม่ใช่ของที่ผู้ใช้จะเอาไปส่งต่อ การบีบให้เล็กอีก 10% ไม่คุ้มกับเวลาที่
//! worker ตัวนั้นถูกยึดไว้ไม่ให้ไปถอดรหัสภาพใบถัดไปในคิว
//!
//! spec: docs/07-file-format.md §2, ROADMAP P4-5

use image::RgbaImage;

/// เข้ารหัสไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    /// `image` ปฏิเสธ — ไม่ควรเกิดกับ RGBA ที่ผ่านเกราะมาแล้ว
    #[error("could not encode the pasted image as PNG")]
    Failed,
}

/// ★ เข้ารหัสภาพเป็น PNG สำหรับพักลง spool
///
/// # Errors
/// [`EncodeError`] เมื่อ encoder ปฏิเสธ — ผู้เรียกควร log แล้วไปต่อ
/// **ห้ามทำให้การวางภาพล้มทั้งใบ** (ภาพยังขึ้นจอได้จาก thumbnail ที่มีอยู่แล้ว)
pub fn to_png(image: &RgbaImage) -> Result<Vec<u8>, EncodeError> {
    use image::ImageEncoder as _;

    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        std::io::Cursor::new(&mut out),
        // ★ เร็วก่อนเล็ก — ดูเหตุผลที่หัวโมดูล
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|_| EncodeError::Failed)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// ★★ ภาพทดสอบที่ **บีบยากพอ ๆ กับภาพถ่ายจริง**
    ///
    /// ★ เคยใช้ gradient เรียบ ๆ แล้วมันบีบเหลือ **1%** ซึ่งทำให้ตัวเลขขนาดที่
    /// รายงานต่ำกว่าความจริงหลายสิบเท่า — กับดักเดียวกับที่ P4-3 เจอตอนวัด
    /// ขนาด snapshot ด้วยโน้ตข้อความล้วน (HANDOFF §2.24)
    /// **ตัวเลขที่ต่ำกว่าความจริงสิบเท่าคือตัวเลขที่พาไปตัดสินใจผิด**
    fn sample(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            // ตัวคูณจำนวนเฉพาะ → ค่าที่กระจายตัวคล้ายรายละเอียดของภาพถ่าย
            let n = u64::from(x).wrapping_mul(2_654_435_761) ^ u64::from(y).wrapping_mul(40_503);
            let n = n ^ (n >> 13);
            image::Rgba([n as u8, (n >> 8) as u8, (n >> 16) as u8, 255])
        })
    }

    /// ★★★ **encode แล้ว decode กลับต้องได้พิกเซลเดิมเป๊ะ**
    ///
    /// PNG ไม่มีการสูญเสีย — ถ้าไม่เท่าเดิมแปลว่าเราเลือก format ผิด
    /// และภาพที่ผู้ใช้วางจะเพี้ยนทุกครั้งที่เปิดไฟล์กลับมา
    #[test]
    fn a_pasted_image_survives_the_round_trip_untouched() {
        let original = sample(64, 48);
        let png = to_png(&original).unwrap();

        let back = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), original.dimensions());
        assert_eq!(back.as_raw(), original.as_raw(), "พิกเซลเปลี่ยนไป");
    }

    /// ไบต์ที่ได้ต้องเป็น PNG จริง — ตัวอ่านฝั่งอื่นดูจาก magic
    #[test]
    fn the_bytes_really_are_a_png() {
        let png = to_png(&sample(8, 8)).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "ไม่ใช่ PNG");
    }

    /// ภาพ 1×1 (เล็กที่สุดที่เป็นภาพได้) ต้องไม่ทำให้อะไรพัง
    #[test]
    fn the_smallest_possible_image_still_encodes() {
        let png = to_png(&sample(1, 1)).unwrap();
        assert!(!png.is_empty());
        assert_eq!(image::load_from_memory(&png).unwrap().to_rgba8().len(), 4);
    }

    /// ★★★ **ราคาจริงของการ encode** — ตัวเลขที่ `docs/07 §2` เรียกร้อง
    ///
    /// ★ **พิมพ์ ไม่ assert เวลา** (`docs/08 §3.9` ข้อ 5b) — เวลาเป็นของเครื่อง
    /// สิ่งที่ assert ได้คือ *ขนาดผลลัพธ์* ซึ่งเป็นคุณสมบัติของข้อมูล
    ///
    /// ★★ ตัวเลขนี้คือเหตุผลที่การ encode **ต้องไม่ขวางภาพขึ้นจอ**: ที่
    /// 6000×4000 มันกินเวลาระดับที่ผู้ใช้รู้สึกได้ชัด ถ้าไปอยู่ก่อน thumbnail
    /// ผู้ใช้จะเห็นโปรแกรมค้างหลังกด `Ctrl+V` ทุกครั้ง
    #[test]
    fn what_encoding_a_pasted_image_costs() {
        for (w, h) in [(1920, 1080), (4000, 3000), (6000, 4000)] {
            let image = sample(w, h);
            let start = std::time::Instant::now();
            let png = to_png(&image).unwrap();
            let elapsed = start.elapsed();

            let raw = image.as_raw().len();
            #[expect(clippy::cast_precision_loss, reason = "แค่พิมพ์ให้คนอ่าน")]
            let ratio = png.len() as f64 / raw as f64 * 100.0;
            println!(
                "{w}x{h}: RGBA {} MB → PNG {} MB ({ratio:.0}% ของดิบ) ใน {elapsed:?}",
                raw >> 20,
                png.len() >> 20
            );
            // ★ PNG ต้องไม่ใหญ่กว่า RGBA ดิบ — ถ้าใหญ่กว่าแปลว่าเลือกทางผิด
            //   (เก็บ RGBA ดิบยังจะดีกว่า)
            assert!(
                png.len() < raw,
                "{w}x{h}: PNG ({}) ใหญ่กว่า RGBA ดิบ ({raw})",
                png.len()
            );
        }
    }
}
