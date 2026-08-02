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
//! ★ **ชนิดข้อมูลย้ายไป `refx_core::clipboard` แล้ว** (HANDOFF §2.0)
//! `refx-asset` ต้องอ่าน clipboard บน worker แต่ต้องไม่ depend `arboard`/`winit`/`rfd`
//! จึงกลับทิศ: core ถือ trait + DTO ส่วนที่นี่คือตัว implement ที่คุยกับ OS จริง
//! และ `refx-ui` เป็นคนเสียบ [`SystemClipboard`] เข้า decode pool ตอนสร้าง
//!
//! spec: docs/03 §5 (`Ctrl+V`), ARCHITECTURE §1 I-4

use refx_core::clipboard::{
    ClipboardContent, ClipboardError, ClipboardImage, ClipboardReader as CoreClipboardReader,
};

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

/// clipboard ของเครื่องจริง — ตัวที่ `refx-ui` เสียบเข้า decode pool
///
/// ไม่ถือสถานะอะไรเลย (`arboard::Clipboard` ถูกสร้างใหม่ทุกครั้งที่อ่าน)
/// จึงแชร์ข้ามเธรดได้โดยไม่ต้องล็อก
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClipboard;

impl CoreClipboardReader for SystemClipboard {
    fn read(&self) -> Result<ClipboardContent, ClipboardError> {
        read()
    }
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
    ///
    /// ★ เรียกผ่าน **trait** ไม่ใช่ `read()` ตรง ๆ เพราะสิ่งที่ decode pool ใช้จริง
    /// คือ `SystemClipboard as ClipboardReader` — ถ้าเทสต์ตรวจแต่ `read()`
    /// การเสียบ trait ที่พังจะไม่มีใครจับได้ (docs/08 §3.9 ข้อ 1)
    #[test]
    fn reading_the_real_clipboard_never_panics() {
        let reader: &dyn CoreClipboardReader = &SystemClipboard;
        match reader.read() {
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
