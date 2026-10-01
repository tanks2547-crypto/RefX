//! เปิดโฟลเดอร์ใน file browser ของ OS (`File → เปิดโฟลเดอร์งานที่เก็บไว้` — P5-9c)
//!
//! ★ `docs/07 §4`: *"ต้องมีทางให้ผู้ใช้เดินไปหามันได้ — ไฟล์ที่ไม่มีใครบอกว่าอยู่ไหน
//!   เท่ากับไฟล์ที่ถูกลบ"* · ทางที่สั้นที่สุดคือเปิดโฟลเดอร์ให้เขาเห็นเลย
//!
//! ★★ **รอโปรเซสลูกจบ** — ผู้เรียกต้องอยู่บนเธรดอื่น (I-2) · `explorer.exe` คืนเร็ว
//!   (มันส่งงานต่อให้ explorer ที่รันอยู่แล้ว) · `xdg-open` บน Linux ต้องถูกรอ
//!   ไม่งั้นค้างเป็น zombie จนกว่า RefX จะปิด
//!
//! ไม่มี dependency ใหม่ — เรียกโปรแกรมของ OS เองผ่าน `std::process::Command`

use std::path::Path;

/// เปิด `dir` ใน file browser · ★ บล็อกจนโปรแกรมของ OS ตอบ — เรียกบนเธรดอื่นเท่านั้น
///
/// # Errors
/// เรียกโปรแกรมของ OS ไม่ได้ (ไม่มี `xdg-open` · ระบบปฏิเสธ)
pub fn open_folder(dir: &Path) -> std::io::Result<()> {
    let program = if cfg!(target_os = "windows") {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    // ★ ไม่ตรวจ exit code: `explorer.exe` คืน 1 แม้เปิดสำเร็จ (พฤติกรรมที่รู้กันของมัน)
    //   · สิ่งที่บอกได้แน่คือ "เรียกโปรแกรมไม่ได้เลย" ซึ่งคือ `Err` ของ `status()`
    std::process::Command::new(program)
        .arg(dir)
        .status()
        .map(|_| ())
}
