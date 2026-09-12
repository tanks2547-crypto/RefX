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
    accept_language_tag(platform_language_tag())
}

/// ★★★ การ **ตัดสิน** ค่าที่ OS ตอบมา — แยกจากการ **ถาม** OS
///
/// ตอนที่สองอย่างนี้อยู่ในฟังก์ชันเดียวกัน กิ่ง "ค่าใช้ได้" ถูกทดสอบได้เฉพาะ
/// ด้วยภาษาของเครื่องที่รันเทสต์อยู่ — เทสต์เดียวที่มีจึงเขียนเป็น
/// `if let Some(tag) = …` ซึ่งแปลว่า **เครื่องที่ตอบ `None` ทำให้เทสต์ผ่านฟรี**
/// ประตู mutation จับได้ว่าถ้าด่านนี้ทำงานทุกครั้ง (= ไม่มีภาษาไหนผ่านเลย)
/// ไม่มีเทสต์ตัวไหนแดง (`docs/08 §3.9` ข้อ 8 ทางที่ 1 · 12 ก.ย. 2026)
#[must_use]
fn accept_language_tag(raw: Option<String>) -> Option<String> {
    let raw = raw?;
    let tag = raw.trim();
    if !is_real_language(tag) {
        return None;
    }
    tracing::info!(tag, "read the user's language from the OS");
    Some(tag.to_owned())
}

/// ค่านี้เป็น "ภาษาจริง" หรือแค่ locale เปล่าของระบบ
///
/// ★ `C` และ `POSIX` **ไม่ใช่ชื่อภาษา** — มันแปลว่า "ไม่ได้ตั้งภาษาไว้"
/// และต้องตัดรูปที่มีชุดอักขระต่อท้ายด้วย (`C.UTF-8`, `POSIX.UTF-8`)
/// ซึ่งเป็นค่าเริ่มต้นของ container และ CI runner แทบทุกตัวบน Linux
///
/// ถ้าไม่ตัด จะได้รหัสภาษา `"c"` ที่ไม่มีอยู่จริงหลุดเข้าไปในระบบเลือกภาษา
/// แล้ว log จะรายงานว่าผู้ใช้ "ตั้งภาษาไว้" ทั้งที่เขาไม่ได้ตั้ง
/// (เจอเพราะ CI ฝั่ง ubuntu ตั้ง `LANG=C.UTF-8` — 1 ส.ค. 2026)
#[must_use]
fn is_real_language(value: &str) -> bool {
    let primary = primary_language(value);
    !primary.is_empty() && primary != "c" && primary != "posix"
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
    // ตัวที่เป็น locale เปล่า (C / POSIX / C.UTF-8) ถูกข้ามไปหาตัวถัดไป
    // เพราะมันแปลว่า "ไม่ได้ตั้งภาษา" ไม่ใช่ชื่อภาษาจริง — ดู [`is_real_language`]
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| is_real_language(value))
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

    /// ★ locale เปล่าของระบบ **ไม่ใช่ภาษา** — ต้องถือว่า "ไม่ได้ตั้ง"
    ///
    /// `C.UTF-8` คือค่าเริ่มต้นของ container และ CI runner แทบทุกตัวบน Linux
    /// ถ้าไม่ตัด `primary_language` จะให้รหัส `"c"` ที่ไม่มีอยู่จริงหลุดเข้าไป
    /// ในระบบเลือกภาษา แล้ว log จะรายงานว่าผู้ใช้ตั้งภาษาไว้ทั้งที่เขาไม่ได้ตั้ง
    #[test]
    fn empty_system_locales_are_not_languages() {
        for value in ["C", "POSIX", "C.UTF-8", "C.utf8", "POSIX.UTF-8", "c", ""] {
            assert!(!is_real_language(value), "{value:?} ไม่ใช่ภาษา แต่ถูกนับว่าเป็น");
        }
        // ของจริงต้องผ่านตามเดิม
        for value in ["th_TH.UTF-8", "en-US", "th", "ja_JP"] {
            assert!(is_real_language(value), "{value:?} เป็นภาษาจริงแต่ถูกตัดทิ้ง");
        }
    }

    /// ★★★ ค่าที่ **ใช้ได้** ต้องผ่านออกมาจริง ไม่ใช่แค่ค่าที่ใช้ไม่ได้ถูกตัด
    ///
    /// ด่าน `!is_real_language` เคยอยู่ติดกับการเรียก OS → เทสต์ยิงเข้าไปตรง ๆ
    /// ไม่ได้ และตัวที่มีอยู่ (`system_tag_is_usable_when_present`) เขียนเป็น
    /// `if let Some(…)` จึง **ผ่านฟรีบนเครื่องที่ OS ตอบ `None`** · ถ้าด่านนี้
    /// ทำงานทุกครั้ง โปรแกรมจะตกเป็นภาษาอังกฤษให้ผู้ใช้ทุกคนบนโลกเงียบ ๆ
    /// โดยไม่มีเทสต์ตัวไหนแดง — ประตู mutation จับได้ 12 ก.ย. 2026
    #[test]
    fn a_usable_tag_comes_back_out_not_only_the_junk_gets_dropped() {
        // ผ่าน — และต้องคืนแท็กเต็ม ไม่ใช่รหัสภาษาที่ตัดหางแล้ว
        assert_eq!(
            accept_language_tag(Some("th-TH".to_owned())),
            Some("th-TH".to_owned())
        );
        assert_eq!(
            accept_language_tag(Some("en-US".to_owned())),
            Some("en-US".to_owned())
        );
        assert_eq!(
            accept_language_tag(Some("ja_JP.UTF-8".to_owned())),
            Some("ja_JP.UTF-8".to_owned())
        );
        // ช่องว่างรอบ ๆ ต้องถูกตัดก่อนคืน ไม่ใช่ติดไปด้วย
        assert_eq!(
            accept_language_tag(Some("  th-TH \n".to_owned())),
            Some("th-TH".to_owned())
        );

        // ไม่ผ่าน — locale เปล่าของระบบ กับการที่ OS ตอบไม่ได้เลย
        for junk in ["C", "POSIX", "C.UTF-8", "", "   ", "-"] {
            assert_eq!(
                accept_language_tag(Some(junk.to_owned())),
                None,
                "{junk:?} ไม่ใช่ภาษา แต่หลุดเข้าไปในระบบเลือกภาษา"
            );
        }
        assert_eq!(accept_language_tag(None), None, "OS ตอบไม่ได้ ต้องไม่เดาภาษาให้");
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
