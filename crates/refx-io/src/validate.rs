//! ★★★ ตรวจ path ที่ **มาจากไฟล์** ก่อนเอาไปแตะดิสก์ (`docs/06 §4`)
//!
//! ## ทำไมลายเซ็นไม่มี `roots`
//!
//! สเปกเดิมเขียนไว้ว่า `validate_path(candidate, roots)` ต้องบังคับสองข้อ และ
//! **ทั้งสองข้อทำตามจริงไม่ได้** (แก้ 28 ส.ค. 2026):
//!
//! | กฎเดิม | ทำไมไม่ได้ |
//! |---|---|
//! | path ใน `.refx` ต้อง relative | แยกไม่ออกว่าไฟล์ไหน "มาจากคนอื่น" — `.refx` บนดิสก์ก็คือ `.refx` |
//! | ต้องอยู่ใต้ root ที่ผู้ใช้เปิด | linked mode ชี้ไปภาพที่อยู่คนละที่กับเอกสาร**เป็นปกติ** บังคับ root = ภาพทุกใบเป็น `Missing` ทุกครั้งที่เปิด = **พังฟีเจอร์ ไม่ใช่ป้องกัน** |
//!
//! ## สิ่งที่กันจริง ๆ แคบกว่านั้นมาก
//!
//! I-8 ห้าม network อยู่แล้ว จึง**ไม่มีทางส่งข้อมูลออก** การอ่านไฟล์ในเครื่อง
//! ที่ไม่ถูกแสดงผลจึงเสียหายจำกัด · สิ่งที่ต้องปฏิเสธคือ path ที่ทำ **อย่างอื่น
//! นอกจากอ่านไฟล์ภาพในเครื่อง**:
//!
//! | ปฏิเสธ | เหตุผล |
//! |---|---|
//! | UNC (`\\server\share`) | ต่อออกนอกเครื่อง = **ละเมิด I-8 ตรง ๆ** + รั่ว NTLM hash บน Windows |
//! | device namespace (`\\.\`, `\\?\`) · ชื่อสงวน (`CON`, `NUL`, `COM1`…) | ค้างได้ · มี side effect · ไม่ใช่ไฟล์ภาพแน่นอน |
//! | `..` | ไม่มีเหตุผลชอบธรรมใน path ที่มาจากไฟล์ |
//!
//! path ในเครื่องที่เหลือ **อนุญาต** — เดินผ่าน `decode_guarded` ที่มีเกราะครบ
//! และถูก fuzz อยู่แล้ว
//!
//! ★★ **ไม่แตะระบบไฟล์เลย** — ไม่ `canonicalize` (สเปกเดิมสั่งไว้) เพราะ
//! `canonicalize` บน UNC **ต่อออกไปหาเซิร์ฟเวอร์เพื่อจะตอบ** ซึ่งคือสิ่งที่
//! ฟังก์ชันนี้มีไว้กันพอดี · ด่านที่ต้องเปิดการเชื่อมต่อเพื่อตัดสินว่าจะเปิด
//! การเชื่อมต่อดีไหม คือด่านที่แพ้ไปแล้วตั้งแต่ก่อนตอบ
//!
//! ★ เรียกทุกจุดที่ path มาจาก **ไฟล์** ไม่ใช่จากการที่ผู้ใช้เลือกเอง —
//! ผู้ใช้ที่กด "หาไฟล์เอง" แล้วชี้ไปที่ไดรฟ์เครือข่าย **ตั้งใจทำแบบนั้น**
//!
//! spec: docs/06 §4

use std::path::{Component, Path, Prefix};

/// path จากไฟล์ไม่ผ่านด่าน
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SecError {
    /// ชี้ไปเครื่องอื่น — ละเมิด I-8
    #[error("path points at another machine over the network")]
    Unc,
    /// device namespace หรือชื่ออุปกรณ์สงวน
    #[error("path names a device, not a file")]
    Device,
    /// มี `..`
    #[error("path walks up out of its folder")]
    Traversal,
    /// ว่างเปล่า
    #[error("path is empty")]
    Empty,
}

/// ชื่ออุปกรณ์ของ DOS ที่ยังมีผลบน Windows ทุกวันนี้
///
/// ★ เปิด `CON`/`NUL` สำเร็จเสมอและไม่มีวันจบเป็นภาพ · `COM1` บนเครื่องที่มี
/// พอร์ตอนุกรมจริงจะ **ค้างรอฮาร์ดแวร์** · ตรวจแบบไม่สนตัวพิมพ์และไม่สนนามสกุล
/// เพราะ `nul.png` ก็ยังเป็น `NUL`
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// ★★★ path นี้ปลอดภัยพอที่จะเอาไปเปิดอ่านหรือไม่
///
/// # Errors
/// [`SecError`] พร้อมเหตุผลที่แยกได้ว่าโดนด่านไหน — ผู้เรียกเอาไปทำข้อความ
/// บอกผู้ใช้ต่อได้ว่า *ทำไม* ภาพใบนั้นเปิดไม่ได้
pub fn validate_asset_path(candidate: &Path) -> Result<(), SecError> {
    if candidate.as_os_str().is_empty() {
        return Err(SecError::Empty);
    }

    // ★★ ตรวจจากไบต์ดิบ **ก่อน** `components()` เพื่อให้สองแพลตฟอร์มตอบเหมือนกัน
    //
    //   `components()` แยก prefix ของ Windows ออกให้เฉพาะตอนคอมไพล์บน Windows
    //   บน Linux `\\?\C:\x` เป็นชื่อไฟล์ก้อนเดียวที่มีแบ็กสแลชอยู่ข้างใน · ถ้า
    //   ปล่อยให้ผลต่างกันตามแพลตฟอร์ม เทสต์จะเขียวข้างหนึ่งแดงอีกข้างหนึ่ง และ
    //   ไฟล์ `.refx` **เดินทางข้าม OS ได้** (นั่นคือเหตุผลที่ packed mode มีอยู่)
    let raw = candidate.to_string_lossy();
    // ★ `\\?\UNC\server\...` ต้องเป็น Unc ไม่ใช่ Device — ตรวจก่อนกิ่ง device
    if raw.starts_with(r"\\?\UNC\") || raw.starts_with(r"\\.\UNC\") {
        return Err(SecError::Unc);
    }
    if raw.starts_with(r"\\?\") || raw.starts_with(r"\\.\") {
        // device namespace — `\\?\` ข้ามการ normalise ของ Win32 ทั้งชุด
        return Err(SecError::Device);
    }
    if raw.starts_with(r"\\") || raw.starts_with("//") {
        return Err(SecError::Unc);
    }

    // ★★★ แยก segment **เอง** ด้วยตัวคั่นทั้งสองแบบ — ห้ามใช้ `components()`
    //
    //   `components()` ยอมรับ `\` เป็นตัวคั่นเฉพาะตอนคอมไพล์บน Windows · บน Linux
    //   `C:\refs\NUL` เป็นชื่อไฟล์ **ก้อนเดียว** แล้วด่านทั้งสองข้อข้างล่างมองไม่เห็น
    //   อะไรเลย · เหตุผลเดียวกับกิ่ง raw ข้างบนเป๊ะ แต่รอบแรกทำครึ่งเดียว →
    //   `a_path_that_names_a_device_is_refused` **แดงจริงบน ubuntu** (run 33381377345)
    //   ทั้งที่เขียวบน Windows
    // ★ ลำดับเดิม: `..` ทั้ง path ก่อน แล้วค่อยชื่อสงวน — path ที่ผิดสองข้อพร้อมกัน
    //   จึงตอบ `Traversal` เหมือนเดิม (เทสต์ผูกกับคำตอบที่เจาะจง ไม่ใช่แค่ "ถูกปฏิเสธ")
    //   · ไม่เก็บลง `Vec` เพราะด่านนี้ถูกเรียกต่อ **ทุกภาพทุกครั้งที่เปิดเอกสาร**
    if raw.split(['/', '\\']).any(|part| part == "..") {
        return Err(SecError::Traversal);
    }
    if raw.split(['/', '\\']).any(names_a_device) {
        return Err(SecError::Device);
    }

    // ★ ตาข่ายชั้นที่สองสำหรับ Windows ที่ `components()` แยก prefix ให้แล้ว
    for component in candidate.components() {
        if let Component::Prefix(prefix) = component {
            match prefix.kind() {
                Prefix::UNC(..) | Prefix::VerbatimUNC(..) => return Err(SecError::Unc),
                Prefix::DeviceNS(..) | Prefix::Verbatim(..) | Prefix::VerbatimDisk(_) => {
                    return Err(SecError::Device);
                }
                Prefix::Disk(_) => {}
            }
        }
    }
    Ok(())
}

/// segment นี้เป็นชื่ออุปกรณ์สงวนหรือไม่ — `nul.png` ยังเป็น `NUL`
fn names_a_device(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("");
    RESERVED
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    /// ★★★ **UNC = ต่อออกนอกเครื่อง = ผิด I-8** — ด่านที่สำคัญที่สุดในไฟล์นี้
    #[test]
    fn a_path_that_reaches_another_machine_is_refused() {
        for evil in [
            r"\\server\share\payload.png",
            r"\\192.0.2.1\share\payload.png",
            r"\\?\UNC\server\share\x.png",
            "//server/share/x.png",
        ] {
            assert_eq!(
                validate_asset_path(Path::new(evil)),
                Err(SecError::Unc),
                "{evil} ควรถูกปฏิเสธเพราะมันต่อออกนอกเครื่อง"
            );
        }
    }

    /// device namespace และชื่อสงวน — เปิดได้แต่ไม่มีวันเป็นภาพ และค้างได้
    #[test]
    fn a_path_that_names_a_device_is_refused() {
        for evil in [
            r"\\.\PhysicalDrive0",
            r"\\?\C:\Windows\win.ini",
            r"C:\refs\NUL",
            r"C:\refs\nul.png",
            r"C:\refs\COM1.jpg",
            "CON",
        ] {
            assert_eq!(
                validate_asset_path(Path::new(evil)),
                Err(SecError::Device),
                "{evil} ควรถูกปฏิเสธ"
            );
        }
    }

    /// ★★★ **คำตอบของด่านต้องไม่ขึ้นกับแพลตฟอร์มที่คอมไพล์**
    ///
    /// `.refx` เดินทางข้าม OS ได้โดยตั้งใจ (นั่นคือเหตุผลที่ packed mode มีอยู่)
    /// path ในไฟล์จึงเป็น path ของ Windows ได้เสมอแม้เปิดบน Linux
    ///
    /// รอบแรกด่านนี้ตรวจ `..`/ชื่อสงวนด้วย `Path::components()` ซึ่งยอมรับ `\`
    /// เป็นตัวคั่น **เฉพาะตอนคอมไพล์บน Windows** → บน Linux ทั้ง `C:\refs\NUL`
    /// และ `C:\refs\..\..\Windows\win.ini` เป็นชื่อไฟล์ก้อนเดียวที่ผ่านด่านฉลุย
    /// · เขียวบนเครื่องพัฒนา **แดงจริงบน ubuntu** (CI run `33381377345`)
    ///
    /// ★ เทสต์นี้ไม่ได้ `cfg` แยกแพลตฟอร์มโดยตั้งใจ — มันจะมีความหมายก็ต่อเมื่อ
    /// รันเหมือนกันทั้งสองฝั่ง
    #[test]
    fn a_windows_path_is_judged_the_same_on_every_platform() {
        for (evil, expected) in [
            (r"C:\refs\NUL", SecError::Device),
            (r"C:\refs\nul.png", SecError::Device),
            (r"C:\refs\..\..\Windows\win.ini", SecError::Traversal),
            (r"..\..\Windows\System32\config\SAM", SecError::Traversal),
        ] {
            assert_eq!(
                validate_asset_path(Path::new(evil)),
                Err(expected),
                "{evil} ต้องได้คำตอบเดียวกันทุกแพลตฟอร์ม — `\\` เป็นตัวคั่นเสมอ"
            );
        }
        // negative control: ตัวคั่นทั้งสองแบบต้องไม่ทำให้ path ปกติถูกปฏิเสธ
        for ok in [r"C:\refs\CONCEPT\hero.png", "/home/a/refs/COM10.png"] {
            assert_eq!(validate_asset_path(Path::new(ok)), Ok(()), "{ok}");
        }
    }

    /// `..` ไม่มีเหตุผลชอบธรรมใน path ที่มาจากไฟล์
    #[test]
    fn walking_up_out_of_the_folder_is_refused() {
        for evil in [
            "../../../etc/passwd",
            r"..\..\..\Windows\System32\config\SAM",
            r"C:\refs\..\..\Windows\win.ini",
        ] {
            assert_eq!(
                validate_asset_path(Path::new(evil)),
                Err(SecError::Traversal),
                "{evil} ควรถูกปฏิเสธ"
            );
        }
    }

    /// ★★★ **negative control ที่สำคัญที่สุด: linked mode ต้องไม่พัง**
    ///
    /// ถ้าด่านนี้แน่นเกินไป ภาพทุกใบของผู้ใช้จะกลายเป็น `Missing` ทุกครั้งที่เปิด
    /// ซึ่ง**แย่กว่าไม่มีด่านเลย** — นั่นคือเหตุผลทั้งหมดที่กฎ "ต้องอยู่ใต้ root"
    /// ถูกถอดออกจากสเปก
    #[test]
    fn the_paths_real_users_actually_have_are_allowed() {
        for ok in [
            r"C:\Users\kitsa\Pictures\refs\pose01.jpg",
            r"D:\งานอ้างอิง\ท่าทาง\01.png",
            "/home/kitsa/refs/pose01.jpg",
            "refs/pose01.jpg",
            r"C:\refs\CONCEPT\hero.png", // ขึ้นต้นด้วย CON แต่ไม่ใช่ CON
            r"C:\refs\NULL_island.png",  // ขึ้นต้นด้วย NUL แต่ไม่ใช่ NUL
            "COM10.png",                 // COM10 ไม่ใช่ชื่อสงวน (มีถึง COM9)
        ] {
            assert_eq!(
                validate_asset_path(Path::new(ok)),
                Ok(()),
                "{ok} เป็น path ปกติของผู้ใช้ ห้ามถูกปฏิเสธ"
            );
        }
    }

    #[test]
    fn an_empty_path_is_refused() {
        assert_eq!(validate_asset_path(Path::new("")), Err(SecError::Empty));
    }

    /// ★ ด่านนี้ **ห้ามแตะระบบไฟล์** — สเปกเดิมสั่งให้ `canonicalize` ซึ่งบน UNC
    /// **ต่อออกไปหาเซิร์ฟเวอร์เพื่อจะตอบ** คือทำสิ่งที่เรากำลังจะห้ามเสียเอง
    ///
    /// พิสูจน์ด้วยการยิง path ที่ไม่มีอยู่จริงเลย: ถ้าฟังก์ชันแตะดิสก์ มันจะ
    /// ตอบต่างจากตอนที่ไฟล์มีอยู่ — ที่นี่ต้องตอบเหมือนกันทุกครั้ง
    #[test]
    fn the_gate_never_touches_the_filesystem() {
        let nowhere = Path::new(r"C:\this\does\not\exist\at\all\x.png");
        assert_eq!(validate_asset_path(nowhere), Ok(()));
        assert_eq!(
            validate_asset_path(Path::new(r"\\nowhere.invalid\share\x.png")),
            Err(SecError::Unc),
            "ต้องตอบได้โดยไม่ต้องไปถามเซิร์ฟเวอร์"
        );
    }
}

/// ★★★ เทสต์ที่ผูกด่านนี้เข้ากับ **I-8** โดยตรง
///
/// `docs/06 §4` บังคับว่าต้องมีเทสต์ว่า `.refx` ที่มี UNC path **ไม่ทำให้เกิด
/// การต่อออกนอกเครื่อง** · การสังเกตด้วยมือครั้งเดียว ("ยิงแล้วไม่เห็น TCP 445")
/// เป็นภาพถ่าย ณ เวลานั้น ไม่ใช่ตาข่าย — วันที่มีคนเปลี่ยนลำดับให้ `is_file()`
/// มาก่อนด่าน จะไม่มีอะไรส่งเสียงเลย
#[cfg(test)]
mod i8 {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// ทุก path ที่เอกสารจะพาไปแตะดิสก์ได้ — ชุดเดียวกับที่ `relink::locate` ลอง
    const FROM_A_DOCUMENT: [&str; 6] = [
        r"\\server\share\payload.png",
        r"\\192.0.2.1\share\payload.png",
        r"\\?\UNC\server\share\payload.png",
        "//server/share/payload.png",
        r"\\.\PhysicalDrive0",
        "../../../etc/passwd",
    ];

    /// ★★★ **ไม่มี path ไหนจากเอกสารได้แตะระบบไฟล์เลยสักครั้ง**
    ///
    /// จำลอง `exists` ของ `relink::locate` ด้วยตัวนับ: ถ้าด่านปล่อยผ่าน ตัวนับจะขึ้น
    /// ซึ่งบนของจริงแปลว่า `Path::is_file()` ถูกเรียก และบน UNC **นั่นคือการต่อ
    /// ออกนอกเครื่อง** (Windows ไปถาม SMB ก่อนถึงจะตอบได้ว่าไฟล์มีไหม)
    #[test]
    fn no_path_from_a_document_ever_reaches_the_filesystem() {
        static TOUCHED: AtomicUsize = AtomicUsize::new(0);
        TOUCHED.store(0, Ordering::SeqCst);

        // ★ รูปเดียวกับใน `app.rs` เป๊ะ: ตรวจก่อน แล้วค่อยแตะ
        let exists = |path: &Path| match validate_asset_path(path) {
            Ok(()) => {
                TOUCHED.fetch_add(1, Ordering::SeqCst);
                true
            }
            Err(_) => false,
        };

        for evil in FROM_A_DOCUMENT {
            assert!(!exists(Path::new(evil)), "{evil} ผ่านด่านออกไปได้");
        }
        assert_eq!(
            TOUCHED.load(Ordering::SeqCst),
            0,
            "มี path จากเอกสารที่ได้แตะระบบไฟล์ — บน UNC นั่นคือการต่อออกนอกเครื่อง (I-8)"
        );
    }

    /// ★★ negative control — ถ้า path ปกติ **ไม่** ถูกแตะเลย แปลว่าด่านแน่นเกินไป
    /// จนฟีเจอร์พัง ซึ่งเป็นความล้มเหลวคนละแบบแต่ร้ายแรงพอกัน
    #[test]
    fn ordinary_paths_do_reach_the_filesystem() {
        static TOUCHED: AtomicUsize = AtomicUsize::new(0);
        TOUCHED.store(0, Ordering::SeqCst);
        let exists = |path: &Path| match validate_asset_path(path) {
            Ok(()) => {
                TOUCHED.fetch_add(1, Ordering::SeqCst);
                true
            }
            Err(_) => false,
        };

        for ok in [r"C:\Users\a\refs\x.png", "/home/a/refs/x.png"] {
            assert!(exists(Path::new(ok)), "{ok} ถูกปฏิเสธทั้งที่เป็น path ปกติ");
        }
        assert_eq!(TOUCHED.load(Ordering::SeqCst), 2);
    }
}
