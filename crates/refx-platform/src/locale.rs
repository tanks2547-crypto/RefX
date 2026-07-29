//! ถาม OS ว่าผู้ใช้ตั้งภาษาอะไรไว้
//!
//! ใช้เลือกภาษาเริ่มต้นของ UI ตอนเปิดโปรแกรม (docs/03 §0)
//! ไม่รู้จัก → ผู้เรียกต้องตกกลับเป็นอังกฤษเสมอ
//!
//! อยู่ใน `refx-platform` ด้วยเหตุผลเดียวกับ [`crate::memory`] — เป็น crate เดียว
//! ที่อนุญาต `unsafe` (I-5) และเป็นที่รวมโค้ดที่ขึ้นกับ OS (ADR-007)
//!
//! **ไม่เพิ่ม dependency ใหม่** — ประกาศ API ของ OS เองตรงนี้เลย

/// แท็กภาษาของผู้ใช้ตามที่ OS บอก เช่น `"th-TH"` `"en-US"`
///
/// คืน `None` เมื่อถามไม่ได้หรือค่าที่ได้ว่างเปล่า — ผู้เรียกต้องใช้อังกฤษ
#[must_use]
pub fn user_language_tag() -> Option<String> {
    let tag = platform_language_tag()?;
    let tag = tag.trim();
    if tag.is_empty() {
        return None;
    }
    tracing::info!(tag, "read the user's language from the OS");
    Some(tag.to_owned())
}

/// รหัสภาษาสองตัวอักษรจากแท็ก เช่น `"th-TH"` → `"th"`, `"th_TH.UTF-8"` → `"th"`
///
/// แยกเป็นฟังก์ชันบริสุทธิ์เพื่อทดสอบรูปแบบแปลก ๆ ได้โดยไม่ต้องพึ่ง OS จริง
/// ค่าจากระบบเป็น input ที่เชื่อไม่ได้เหมือนกัน (I-4) — ผู้ใช้ตั้ง `LANG` เองได้
#[must_use]
pub fn primary_language(tag: &str) -> String {
    tag.split(['-', '_', '.', '@'])
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

#[cfg(target_os = "windows")]
fn platform_language_tag() -> Option<String> {
    /// ความยาวสูงสุดของชื่อ locale ตามเอกสารของ Microsoft (`LOCALE_NAME_MAX_LENGTH`)
    const LOCALE_NAME_MAX_LENGTH: usize = 85;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, name_len: i32) -> i32;
    }

    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    let capacity = i32::try_from(buffer.len()).ok()?;

    // SAFETY: `buffer` เป็นอาร์เรย์บนสแตกที่มีชีวิตอยู่ตลอดการเรียก และเราบอกความจุจริง
    // ของมันไปพร้อมกัน API เขียนได้ไม่เกินจำนวนนั้นตามสัญญาของ Microsoft
    // และไม่เก็บตัวชี้ไว้ใช้ต่อหลังจากคืนค่า
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), capacity) };

    // คืนค่าเป็นจำนวน u16 ที่เขียน **รวมตัวปิดท้าย null** · 0 = ล้มเหลว
    let len = usize::try_from(written).ok()?.checked_sub(1)?;
    let slice = buffer.get(..len)?;
    Some(String::from_utf16_lossy(slice))
}

#[cfg(not(target_os = "windows"))]
fn platform_language_tag() -> Option<String> {
    // POSIX: เรียงตามลำดับความสำคัญที่มาตรฐานกำหนด
    // "C" กับ "POSIX" แปลว่า "ไม่ได้ตั้งภาษา" ไม่ใช่ชื่อภาษาจริง
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn extracts_primary_language_from_every_common_shape() {
        assert_eq!(primary_language("th-TH"), "th"); // Windows / BCP-47
        assert_eq!(primary_language("th_TH.UTF-8"), "th"); // POSIX
        assert_eq!(primary_language("en"), "en");
        assert_eq!(primary_language("en-US"), "en");
        assert_eq!(primary_language("zh-Hans-CN"), "zh");
        assert_eq!(primary_language("sr_RS@latin"), "sr");
        // ตัวพิมพ์ใหญ่ต้องได้ผลเดียวกัน ไม่งั้นการเทียบภาษาจะพลาดแบบเงียบ ๆ
        assert_eq!(primary_language("TH-th"), "th");
    }

    /// ค่าจาก `LANG` ที่ผู้ใช้ตั้งเองเพี้ยนได้ทุกแบบ — ต้องไม่ panic (I-4)
    #[test]
    fn broken_tags_are_handled_not_panicked() {
        for tag in ["", "-", "_", ".", "@", "---", "\u{1F600}", "   "] {
            let primary = primary_language(tag);
            assert!(
                primary.is_empty() || !primary.contains(['-', '_', '.', '@']),
                "แท็ก {tag:?} ให้ผลแปลก: {primary:?}"
            );
        }
    }

    /// ถ้าระบบตอบมา ต้องเป็นแท็กที่ใช้ได้จริง ไม่ใช่ขยะ
    ///
    /// ไม่ยืนยันว่าเป็นภาษาอะไร เพราะขึ้นกับเครื่องที่รันเทสต์
    #[test]
    fn system_tag_is_usable_when_present() {
        if let Some(tag) = user_language_tag() {
            assert!(!tag.trim().is_empty());
            let primary = primary_language(&tag);
            assert!(
                primary.len() >= 2 && primary.chars().all(|c| c.is_ascii_lowercase()),
                "รหัสภาษาที่ได้ใช้ไม่ได้: {primary:?} (จาก {tag:?})"
            );
        }
    }
}
