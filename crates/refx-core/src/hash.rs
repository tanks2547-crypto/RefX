//! `ContentHash` — คีย์หลักของภาพทั้งระบบ
//!
//! อยู่ใน `refx-core` เพราะ `AssetRef` (docs/02 §2.3) ถือมันไว้ และ `refx-core`
//! depend `blake3` ไม่ได้ **ตัวชนิดจึงอยู่ที่นี่ ส่วนคนที่คำนวณค่าอยู่ `refx-asset`**
//! — ไบต์ 32 ตัวไม่รู้จักว่าตัวเองถูกคำนวณมาอย่างไร
//!
//! `refx-asset::hash` re-export ชนิดนี้กลับออกไป เพื่อไม่ต้องแก้ call site เดิม
//!
//! spec: docs/02-data-model.md §2.2.5, docs/05-memory-and-assets.md §4

/// hash ของเนื้อไฟล์ (blake3-256)
///
/// ใช้ **เนื้อไฟล์** เป็นคีย์ ไม่ใช่ path → ย้ายไฟล์/เปลี่ยนชื่อแล้ว thumbnail ไม่หาย
/// และไฟล์ซ้ำใช้ thumbnail ร่วมกันได้ (ARCHITECTURE §5)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// ไบต์ดิบ 32 ไบต์ — ใช้เป็น BLOB key ใน sqlite
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// สร้างจากไบต์ดิบ (ใช้ตอนอ่านกลับจาก DB และตอนที่ `refx-asset` คำนวณเสร็จ)
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// เลขฐานสิบหกแบบสั้นสำหรับ log (8 ตัวอักษรพอแยกแยะได้ในทางปฏิบัติ)
    #[must_use]
    pub fn short(&self) -> String {
        self.0[..4].iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn raw_bytes_survive_a_round_trip() {
        let raw = [7u8; 32];
        assert_eq!(ContentHash::from_bytes(raw).as_bytes(), &raw);
    }

    #[test]
    fn display_is_full_hex_and_short_is_the_first_four_bytes() {
        let mut raw = [0u8; 32];
        raw[0] = 0xde;
        raw[1] = 0xad;
        raw[2] = 0xbe;
        raw[3] = 0xef;
        let hash = ContentHash::from_bytes(raw);

        assert_eq!(hash.short(), "deadbeef");
        assert_eq!(hash.to_string().len(), 64);
        assert!(hash.to_string().starts_with("deadbeef"));
    }
}
