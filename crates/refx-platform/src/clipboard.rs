//! อ่าน/เขียนภาพจาก clipboard ของระบบ
//!
//! ★ ยังไม่ implement — วางโครงไว้ตาม ROADMAP P0-2 ของจริงทำใน P1-8
//!
//! **ข้อควรระวังตอน implement:** ภาพจาก clipboard คือ input ที่ไม่น่าไว้ใจ (I-4)
//! ต้องผ่าน limit เรื่องขนาด/จำนวน pixel เหมือนไฟล์บนดิสก์ทุกประการ
//! และการอ่าน clipboard อาจบล็อกได้ → ต้องไม่อยู่บน UI thread (I-2)

/// อ่าน/เขียน clipboard ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    /// ใน clipboard ไม่มีภาพ
    ///
    /// ข้อความเป็นอังกฤษเพราะเป็นของ log/นักพัฒนา — ข้อความที่ผู้ใช้เห็น
    /// ประกอบที่ `refx-ui::text` แล้วแปลตามภาษา (docs/03 §0)
    #[error("clipboard holds no image")]
    NoImage,
}

/// ภาพดิบจาก clipboard (RGBA 8 บิตต่อช่อง)
#[derive(Debug, Clone)]
pub struct ClipboardImage {
    /// ความกว้าง (pixel)
    pub width: u32,
    /// ความสูง (pixel)
    pub height: u32,
    /// ข้อมูล RGBA เรียงตามแถว ความยาวต้องเท่ากับ `width * height * 4`
    pub rgba: Vec<u8>,
}

/// อ่านภาพจาก clipboard
///
/// # Panics
/// ยังไม่ implement — เรียกแล้ว panic ทันที (P1-8)
pub fn read_image() -> Result<ClipboardImage, ClipboardError> {
    todo!("P1-8: ต้องตรวจ limit ของภาพก่อนรับเข้ามา (I-4) และห้ามบล็อก UI thread (I-2)")
}
