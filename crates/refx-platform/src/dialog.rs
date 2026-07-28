//! Native file dialog
//!
//! ★ ยังไม่ implement — วางโครงไว้ตาม ROADMAP P0-2 ของจริงทำใน P1-8 / P4
//!
//! **ข้อควรระวังตอน implement:** `rfd` เปิด dialog แบบ blocking
//! ห้ามเรียกบน UI thread เด็ดขาด (I-2) ต้องใช้ `AsyncFileDialog`
//! หรือ spawn thread แล้วส่งผลกลับทาง `crossbeam-channel`
//! ดู docs/09-crate-versions.md

use std::path::PathBuf;

/// เปิด dialog ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum DialogError {
    /// ผู้ใช้กดยกเลิก
    #[error("file dialog cancelled by user")]
    Cancelled,
}

/// ให้ผู้ใช้เลือกไฟล์ภาพหลายไฟล์
///
/// # Panics
/// ยังไม่ implement — เรียกแล้ว panic ทันที (P1-8)
pub fn pick_images() -> Result<Vec<PathBuf>, DialogError> {
    todo!("P1-8: ต้องรันบน worker thread ห้ามบล็อก UI thread (I-2)")
}

/// ให้ผู้ใช้เลือกที่บันทึกไฟล์ `.refx`
///
/// # Panics
/// ยังไม่ implement — เรียกแล้ว panic ทันที (P4-1)
pub fn pick_save_location() -> Result<PathBuf, DialogError> {
    todo!("P4-1: ต้องรันบน worker thread ห้ามบล็อก UI thread (I-2)")
}

/// แจ้งผู้ใช้ว่าโปรแกรมพัง พร้อมบอกว่าไฟล์ log อยู่ไหน
///
/// เรียกจาก panic hook เท่านั้น — **บล็อกได้** เพราะตอนนั้นโปรแกรมกำลังจะตายอยู่แล้ว
/// (ข้อยกเว้นเดียวของ I-2 ที่ยอมรับได้)
///
/// ห้ามให้ฟังก์ชันนี้ panic ซ้ำเด็ดขาด ไม่งั้นจะได้ panic ซ้อน panic แล้ว abort ทันที
/// จนไม่มีใครได้เห็นข้อความอะไรเลย
pub fn show_crash_dialog(log_path: &std::path::Path) {
    let message = format!(
        "RefX หยุดทำงานกะทันหัน\n\n\
         รายละเอียดถูกบันทึกไว้ที่:\n{}\n\n\
         ถ้าแจ้งปัญหา กรุณาแนบไฟล์นี้มาด้วย",
        log_path.display()
    );

    // rfd อาจล้มได้ถ้าไม่มี display (เช่นรันใน CI) — ห้ามให้ error ตรงนี้ลาม
    let result = std::panic::catch_unwind(|| {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("RefX หยุดทำงาน")
            .set_description(&message)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    });

    if result.is_err() {
        // เหลือทางเดียวคือ stderr
        eprintln!("{message}");
    }
}
