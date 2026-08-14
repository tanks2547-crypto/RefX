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

/// ให้ผู้ใช้เลือกที่บันทึกไฟล์ `.refx` — **ไม่บล็อก UI thread** (P4-2)
///
/// ★★★ `rfd` เปิด dialog แบบ blocking และการรอผู้ใช้เลือกไฟล์กินเวลา
/// **เป็นนาที** ได้สบาย ๆ (เขาอาจไปเปิดโฟลเดอร์หา หรือลุกไปชงกาแฟ)
/// เรียกบน UI thread = โปรแกรมค้างทั้งตัว ซึ่งผิด I-2 ตรง ๆ
///
/// จึงย้ายไปเธรดของตัวเองแล้วคืน `Receiver` ให้ผู้เรียก **ถามทีหลัง** —
/// ชั้น UI เช็คทุกเฟรมด้วย `try_recv()` ซึ่งไม่บล็อกเลย
///
/// `None` ที่ส่งกลับมา = ผู้ใช้กดยกเลิก (ไม่ใช่ error — เป็นสิ่งที่เขาตั้งใจทำ)
///
/// ★ เธรดนี้ **ไม่ถูก join** โดยตั้งใจ: ถ้าผู้ใช้ปิดโปรแกรมทั้งที่ dialog
/// ยังเปิดอยู่ เราไม่อยากให้การปิดค้างรอเขากดปุ่ม · `Receiver` ที่ถูก drop
/// ทำให้ `send` ฝั่งโน้นล้มเงียบ ๆ ซึ่งเป็นพฤติกรรมที่ต้องการพอดี
#[must_use]
pub fn pick_save_location(suggested_name: &str) -> crossbeam_channel::Receiver<Option<PathBuf>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let name = suggested_name.to_owned();
    std::thread::Builder::new()
        .name("refx-save-dialog".to_owned())
        .spawn(move || {
            // rfd ล้มได้ถ้าไม่มี display (เช่นรันใน CI) — ห้ามให้ลามเป็น panic
            let picked = std::panic::catch_unwind(|| {
                rfd::FileDialog::new()
                    .set_title("บันทึกกระดานเป็น")
                    .set_file_name(&name)
                    .add_filter("RefX board", &["refx"])
                    .save_file()
            })
            .unwrap_or(None);
            let _ = tx.send(picked);
        })
        // spawn ล้ม = ระบบไม่มีเธรดให้แล้ว ซึ่งใหญ่กว่าเรื่องบันทึกไฟล์
        // — ผู้เรียกจะเห็นว่า channel ปิดทันที แล้วรายงานว่าเปิด dialog ไม่ได้
        .map_or_else(
            |err| tracing::error!(%err, "cannot spawn the save dialog thread"),
            drop,
        );
    rx
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
