//! อ่านภาพ/รายชื่อไฟล์จาก clipboard ของระบบ (P1-8)
//!
//! ★ **โมดูลนี้ไม่ตัดสินว่ารับภาพได้ไหม** หน้าที่มันคือ "หยิบของออกมาจาก OS"
//! อย่างเดียว เพดานทั้งหมด (`max_pixels` ที่ผูกกับ RAM เครื่อง, ขนาด, ความสอดคล้อง
//! ของบัฟเฟอร์) อยู่ที่ `refx-asset::decode` **ที่เดียว** เหมือนไฟล์บนดิสก์ทุกประการ
//! ถ้าตรวจสองที่ เกราะสองชุดจะเพี้ยนจากกันวันที่มีคนแก้ข้างเดียว (เหตุผลเดียวกับ
//! ที่ `read_file_guarded` ถูกแยกออกมาให้ decode pool ใช้ร่วมกัน)
//!
//! **ห้ามเรียกบน UI thread** (I-2): การเปิด clipboard รอ OS ได้นานเป็นวินาที
//! ถ้าโปรแกรมอื่นถือมันค้างอยู่ (Windows: `OpenClipboard` ต้องรอเจ้าของเดิมปล่อย
//! และเจ้าของอาจเป็นโปรแกรมที่กำลังค้าง) — ของจริงเรียกจาก decode worker
//!
//! spec: docs/03 §5 (`Ctrl+V`), ARCHITECTURE §1 I-4

use std::path::PathBuf;

/// อ่าน clipboard ไม่สำเร็จ
///
/// ★ ข้อความใน `#[error(…)]` เป็น **อังกฤษสำหรับ log/นักพัฒนา**
/// ข้อความที่ผู้ใช้เห็นประกอบที่ `refx-ui::text::clipboard_error` แล้วแปลตามภาษา
/// (docs/03 §0) — variant จึงต้องแยกให้ละเอียดพอที่ผู้ใช้จะรู้ว่าต้องทำอะไรต่อ
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    /// ใน clipboard ไม่มีภาพและไม่มีไฟล์ — เคสที่พบบ่อยที่สุดคือ **ก๊อปข้อความมา**
    #[error("clipboard holds no image and no file")]
    NoImage,

    /// โปรแกรมอื่นถือ clipboard อยู่ ลองใหม่อีกครั้งได้
    #[error("clipboard is held by another program")]
    Busy,

    /// เครื่องนี้ไม่มี clipboard ให้ใช้ (เช่นรันแบบไม่มี display)
    #[error("no usable clipboard on this system: {reason}")]
    Unavailable {
        /// เหตุผลจาก OS (อังกฤษ — ไม่มีข้อมูลของผู้ใช้อยู่ในนั้น)
        reason: String,
    },

    /// มีภาพอยู่จริงแต่แปลงเป็น pixel ไม่ได้ — ข้อมูลใน clipboard เสียหรือเป็นขยะ
    #[error("the image in the clipboard could not be decoded")]
    Undecodable,
}

/// แปลง error ของ `arboard` เป็นของเรา
///
/// แยกเป็นฟังก์ชันเพราะเรียกจากสองจุด และเพื่อให้เห็นชัดว่าเคสไหนแมปไปไหน
fn translate(err: &arboard::Error) -> ClipboardError {
    match err {
        arboard::Error::ContentNotAvailable => ClipboardError::NoImage,
        arboard::Error::ClipboardOccupied => ClipboardError::Busy,
        arboard::Error::ConversionFailure => ClipboardError::Undecodable,
        // ClipboardNotSupported / Unknown / variant ใหม่ในอนาคต — ทั้งหมดคือ
        // "ใช้ clipboard ไม่ได้" จากมุมของผู้ใช้ เก็บคำอธิบายไว้ให้ log
        other => ClipboardError::Unavailable {
            reason: other.to_string(),
        },
    }
}

/// ภาพดิบจาก clipboard — **ค่าตามที่ OS ให้มา ยังไม่ผ่านเกราะ**
///
/// ห้ามเชื่อว่า `rgba.len() == width * height * 4` ตรงนี้ (I-4)
/// `refx-asset::decode::accept_rgba_guarded` เป็นคนตรวจ
#[derive(Debug, Clone)]
pub struct ClipboardImage {
    /// ความกว้างที่ OS ประกาศ (pixel)
    pub width: u32,
    /// ความสูงที่ OS ประกาศ (pixel)
    pub height: u32,
    /// ข้อมูล RGBA ที่ OS ให้มา
    pub rgba: Vec<u8>,
}

/// สิ่งที่อยู่ใน clipboard ตอนนี้ เท่าที่ RefX เอาไปใช้ได้
#[derive(Debug, Clone)]
pub enum ClipboardContent {
    /// รายชื่อไฟล์ (ก๊อปไฟล์จาก Explorer/Finder)
    ///
    /// ★ เคสนี้ต้องเดินเส้นทางเดียวกับ drag & drop เป๊ะ ๆ — มี cache, มี EXIF
    /// และขอภาพคมตอนซูมได้ ต่างจากภาพดิบที่ไม่มีไฟล์ให้กลับไปอ่านใหม่
    Files(Vec<PathBuf>),
    /// ภาพดิบที่ OS ถอดรหัสมาให้แล้ว (screenshot, ก๊อปจากเบราว์เซอร์/โปรแกรมวาด)
    Image(ClipboardImage),
}

/// อ่านสิ่งที่อยู่ใน clipboard ตอนนี้
///
/// ลองรายชื่อไฟล์ก่อนเสมอ เพราะถ้าผู้ใช้ก๊อปไฟล์ภาพมา การเปิดจากไฟล์จริง
/// ให้ผลดีกว่าทุกทาง (ผ่าน cache, อ่าน EXIF orientation, ขอภาพคมตอนซูมได้)
///
/// **ห้ามเรียกบน UI thread** (I-2)
///
/// # Errors
/// คืน [`ClipboardError`] เสมอเมื่อไม่มีอะไรที่ใช้ได้ — ไม่ panic ไม่ว่าใน
/// clipboard จะมีอะไรอยู่
pub fn read() -> Result<ClipboardContent, ClipboardError> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| translate(&err))?;

    // 1. ไฟล์ก่อน — รายการว่างถือว่าไม่มี (บาง OS คืน Ok(vec![]) แทนที่จะเป็น Err)
    match clipboard.get().file_list() {
        Ok(files) if !files.is_empty() => {
            tracing::debug!(count = files.len(), "clipboard holds a file list");
            return Ok(ClipboardContent::Files(files));
        }
        // ไม่มีไฟล์ = เรื่องปกติ ไปลองภาพต่อ ไม่ใช่ error ของผู้ใช้
        Ok(_) | Err(_) => {}
    }

    // 2. ภาพดิบ
    let image = clipboard.get().image().map_err(|err| translate(&err))?;
    let width = u32::try_from(image.width).unwrap_or(u32::MAX);
    let height = u32::try_from(image.height).unwrap_or(u32::MAX);
    let rgba = image.bytes.into_owned();

    tracing::debug!(
        width,
        height,
        bytes = rgba.len(),
        "clipboard holds an image"
    );
    Ok(ClipboardContent::Image(ClipboardImage {
        // ค่าที่ใหญ่เกิน u32 ไม่มีทางผ่านเพดาน `max_dimension` (65535) อยู่แล้ว
        // จึงย่อเป็น u32::MAX เพื่อให้เกราะฝั่ง refx-asset ปฏิเสธด้วยเหตุผลเดียวกัน
        // แทนที่จะต้องมีเส้นทาง error ซ้ำอีกชุดที่นี่
        width,
        height,
        rgba,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// ★ ทุก error ของ `arboard` ต้องมีที่ลง — ห้ามมีเคสที่หลุดไปเป็น panic
    #[test]
    fn every_arboard_error_maps_to_something_useful() {
        let cases = [
            arboard::Error::ContentNotAvailable,
            arboard::Error::ClipboardOccupied,
            arboard::Error::ConversionFailure,
            arboard::Error::ClipboardNotSupported,
            arboard::Error::Unknown {
                description: "boom".to_owned(),
            },
        ];
        for case in &cases {
            let mapped = translate(case);
            assert!(
                !mapped.to_string().trim().is_empty(),
                "{case:?} แมปแล้วได้ข้อความว่าง"
            );
        }

        assert!(matches!(
            translate(&arboard::Error::ContentNotAvailable),
            ClipboardError::NoImage
        ));
        assert!(matches!(
            translate(&arboard::Error::ClipboardOccupied),
            ClipboardError::Busy
        ));
        // ★ ขยะใน clipboard = Undecodable ไม่ใช่ "ไม่มีภาพ" — ผู้ใช้ต้องรู้ว่า
        //   มีอะไรอยู่จริงแต่พัง จะได้ก๊อปใหม่ ไม่ใช่นั่งงงว่าทำไมไม่มีอะไรเกิดขึ้น
        assert!(matches!(
            translate(&arboard::Error::ConversionFailure),
            ClipboardError::Undecodable
        ));
        assert!(matches!(
            translate(&arboard::Error::ClipboardNotSupported),
            ClipboardError::Unavailable { .. }
        ));
    }

    /// อ่าน clipboard จริงบนเครื่องที่รันเทสต์ — ผลเป็นอะไรก็ได้ **ยกเว้น panic**
    ///
    /// เทสต์นี้พิสูจน์ว่าเส้นทางจริงเดินได้ทั้งเส้น (สร้าง `Clipboard` → ถามไฟล์ →
    /// ถามภาพ → แมป error) บนเครื่องที่ไม่มี display ก็ต้องได้ `Err` ไม่ใช่ล้ม
    #[test]
    fn reading_the_real_clipboard_never_panics() {
        match read() {
            Ok(ClipboardContent::Files(files)) => assert!(!files.is_empty()),
            Ok(ClipboardContent::Image(image)) => {
                // ห้ามยืนยันว่า len ตรงกับ w×h×4 ตรงนี้ — โมดูลนี้ไม่ตรวจโดยตั้งใจ
                assert!(image.width > 0 || image.rgba.is_empty());
            }
            Err(err) => {
                assert!(!err.to_string().trim().is_empty());
            }
        }
    }
}
