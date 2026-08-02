//! ชนิดข้อมูลกลางของ clipboard + **ประตูที่ให้ชั้นบนเสียบตัวอ่านจริงเข้ามา**
//!
//! ★ ทำไมชนิดข้อมูลอยู่ที่นี่ ทั้งที่ `refx-core` เป็นโดเมนล้วน:
//! `refx-asset::pool` ต้องอ่าน clipboard **บน worker** (การเปิด clipboard รอ OS ได้
//! นานเป็นวินาที ทำบน UI thread = ค้างทั้งบาน — I-2) แต่ `refx-asset` ต้องไม่รู้จัก
//! `arboard`/`winit`/`rfd` (ARCHITECTURE §2) ทางออกคือกลับทิศ dependency:
//! **ชนิดข้อมูลกับ trait อยู่ชั้นล่างสุด ส่วนตัวที่คุยกับ OS จริงอยู่ `refx-platform`**
//! แล้ว `refx-ui` เป็นคนเสียบให้ตอนสร้าง pool
//!
//! รูปแบบเดียวกับ [`refx_asset::pool::WakeHandle`] ที่ทำให้ `refx-asset` ไม่ต้อง
//! รู้จัก `winit` — ที่นั่นใช้ closure ที่นี่ใช้ trait เพราะมี error type ที่ต้องแชร์
//!
//! **โมดูลนี้ไม่ตัดสินว่ารับภาพได้ไหม** เพดานทั้งหมด (`max_pixels` ที่ผูกกับ RAM
//! เครื่อง, ขนาด, ความสอดคล้องของบัฟเฟอร์) อยู่ที่ `refx-asset::decode` **ที่เดียว**
//! เหมือนไฟล์บนดิสก์ทุกประการ ถ้าตรวจสองที่ เกราะสองชุดจะเพี้ยนจากกันวันที่มีคนแก้ข้างเดียว
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

/// ตัวอ่าน clipboard ของจริง — ตัวที่ implement อยู่ `refx-platform`
///
/// `Send + Sync` เพราะ decode worker หลายตัวถือร่วมกันผ่าน `Arc`
///
/// **ห้ามเรียกบน UI thread** (I-2): การเปิด clipboard รอ OS ได้นานเป็นวินาที
/// ถ้าโปรแกรมอื่นถือมันค้างอยู่ (Windows: `OpenClipboard` ต้องรอเจ้าของเดิมปล่อย
/// และเจ้าของอาจเป็นโปรแกรมที่กำลังค้าง)
pub trait ClipboardReader: Send + Sync {
    /// อ่านสิ่งที่อยู่ใน clipboard ตอนนี้
    ///
    /// # Errors
    /// คืน [`ClipboardError`] เสมอเมื่อไม่มีอะไรที่ใช้ได้ — **ห้าม panic**
    /// ไม่ว่าใน clipboard จะมีอะไรอยู่ (I-4)
    fn read(&self) -> Result<ClipboardContent, ClipboardError>;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// ทุก variant ต้องมีข้อความสำหรับ log — variant ที่ `Display` ว่าง
    /// จะกลายเป็นบรรทัด log ที่บอกอะไรไม่ได้เลยตอนผู้ใช้ส่ง log มาให้ดู
    #[test]
    fn every_error_variant_says_something() {
        let cases = [
            ClipboardError::NoImage,
            ClipboardError::Busy,
            ClipboardError::Unavailable {
                reason: "no display".to_owned(),
            },
            ClipboardError::Undecodable,
        ];
        for case in &cases {
            assert!(!case.to_string().trim().is_empty(), "{case:?} ไม่มีข้อความ");
        }
    }

    /// ★ trait นี้มีค่าก็ต่อเมื่อ **ส่งข้ามเธรดได้จริง** — ถ้าไม่ใช่
    /// decode worker จะเสียบตัวอ่านจริงไม่ได้เลย แล้วทั้งดีไซน์นี้ก็ไร้ความหมาย
    #[test]
    fn a_reader_can_be_shared_with_worker_threads() {
        #[derive(Debug)]
        struct Fake;
        impl ClipboardReader for Fake {
            fn read(&self) -> Result<ClipboardContent, ClipboardError> {
                Ok(ClipboardContent::Files(vec![PathBuf::from("a.png")]))
            }
        }

        let reader: std::sync::Arc<dyn ClipboardReader> = std::sync::Arc::new(Fake);
        let moved = std::sync::Arc::clone(&reader);
        let seen = std::thread::spawn(move || match moved.read() {
            Ok(ClipboardContent::Files(files)) => files.len(),
            _ => 0,
        })
        .join()
        .unwrap();
        assert_eq!(seen, 1);
    }
}
