//! Native file dialog
//!
//! ★ ยังไม่ implement — วางโครงไว้ตาม ROADMAP P0-2 ของจริงทำใน P1-8 / P4
//!
//! **ข้อควรระวังตอน implement:** `rfd` เปิด dialog แบบ blocking
//! ห้ามเรียกบน UI thread เด็ดขาด (I-2) ต้องใช้ `AsyncFileDialog`
//! หรือ spawn thread แล้วส่งผลกลับทาง `crossbeam-channel`
//! ดู docs/09-crate-versions.md

use std::path::PathBuf;

/// ★★★ ปลุก event loop **หลังส่งผลแล้ว** — `docs/08 §3.9` ข้อ 18
///
/// แอปหลับด้วย `ControlFlow::Wait` ตาม I-1 · ผลที่ส่งกลับมาโดยไม่มีใครปลุก
/// จะนอนอยู่ในช่องจนกว่าผู้ใช้จะบังเอิญขยับเมาส์ — อาการคือ *"เลือกไฟล์เสร็จ
/// แล้วไม่มีอะไรเกิดขึ้น"* ซึ่งเกิดจริงมาแล้วสามครั้งในโปรเจกต์นี้
///
/// ★ **ต้องปลุกหลัง `send` เสมอ** — ปลุกก่อนส่ง เฟรมที่ตื่นมาจะยังไม่เห็นอะไร
/// แล้วก็หลับต่อ กลายเป็นการปลุกที่ไม่ได้ผลอะไรเลย
///
/// `None` = ผู้เรียกยอมรับความหน่วงนั้น (เทสต์ที่ไม่มี event loop)
fn wake_after_send(waker: Option<&crate::window::Waker>) {
    if let Some(waker) = waker {
        waker.wake();
    }
}

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
///
/// ★★★ `waker` — ดู [`wake_after_send`] · **ไม่มีตัวปลุก = ผลนอนรอ**
#[must_use]
pub fn pick_save_location(
    suggested_name: &str,
    waker: Option<crate::window::Waker>,
) -> crossbeam_channel::Receiver<Option<PathBuf>> {
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
            wake_after_send(waker.as_ref());
        })
        // spawn ล้ม = ระบบไม่มีเธรดให้แล้ว ซึ่งใหญ่กว่าเรื่องบันทึกไฟล์
        // — ผู้เรียกจะเห็นว่า channel ปิดทันที แล้วรายงานว่าเปิด dialog ไม่ได้
        .map_or_else(
            |err| tracing::error!(%err, "cannot spawn the save dialog thread"),
            drop,
        );
    rx
}

/// ให้ผู้ใช้เลือกไฟล์ `.refx` ที่จะเปิด — **ไม่บล็อก UI thread** (P4-4)
///
/// เหตุผลของรูปร่าง API ทั้งหมดเหมือน [`pick_save_location`] เป๊ะ (ดูที่นั่น)
/// — ต่างแค่มันเปิดไฟล์ที่มีอยู่แล้วแทนที่จะตั้งชื่อไฟล์ใหม่
///
/// `None` = ผู้ใช้กดยกเลิก ซึ่งไม่ใช่ error
#[must_use]
pub fn pick_document_to_open(
    waker: Option<crate::window::Waker>,
) -> crossbeam_channel::Receiver<Option<PathBuf>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("refx-open-dialog".to_owned())
        .spawn(move || {
            let picked = std::panic::catch_unwind(|| {
                rfd::FileDialog::new()
                    .set_title("เปิดกระดาน")
                    .add_filter("RefX board", &["refx"])
                    .pick_file()
            })
            .unwrap_or(None);
            let _ = tx.send(picked);
            wake_after_send(waker.as_ref());
        })
        .map_or_else(
            |err| tracing::error!(%err, "cannot spawn the open dialog thread"),
            drop,
        );
    rx
}

/// ให้ผู้ใช้เลือกที่จะ export ภาพลง — **ไม่บล็อก UI thread** (P5-4)
///
/// เหตุผลของรูปร่าง API เหมือน [`pick_save_location`] เป๊ะ · ต่างแค่ตัวกรอง
/// และ **ตัวปลุก**
///
/// ★ ตัวกรองมีนามสกุลเดียวตามรูปแบบที่ผู้ใช้เลือกไว้ในกล่อง export แล้ว —
/// ให้เลือกได้สองแบบตรงนี้จะกลายเป็นสองที่ที่ตัดสินรูปแบบไฟล์ แล้ววันหนึ่ง
/// ผู้ใช้จะเลือก JPEG ในกล่องแต่ตั้งชื่อ `.png` แล้วได้ไฟล์ที่ชื่อโกหก
///
/// ★★★ **`waker` ไม่ใช่ของประดับ** — แอปหลับด้วย `ControlFlow::Wait` (I-1)
/// ถ้าไม่ปลุก ผลที่ส่งกลับมาจะไม่มีใครอ่านจนกว่าผู้ใช้จะขยับเมาส์ · เห็นบนแอป
/// จริงตอน P5-4: เลือกไฟล์เสร็จแล้วกล่องยังเขียนว่า "กำลังรอให้เลือกที่บันทึก"
/// อยู่อย่างนั้นจนขยับเมาส์ · `None` = ผู้เรียกยอมรับความหน่วงนั้น (เทสต์)
#[must_use]
pub fn pick_export_location(
    suggested_name: &str,
    extension: &'static str,
    waker: Option<crate::window::Waker>,
) -> crossbeam_channel::Receiver<Option<PathBuf>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let name = suggested_name.to_owned();
    std::thread::Builder::new()
        .name("refx-export-dialog".to_owned())
        .spawn(move || {
            let picked = std::panic::catch_unwind(|| {
                rfd::FileDialog::new()
                    .set_title("ส่งออกภาพเป็น")
                    .set_file_name(&name)
                    .add_filter(extension.to_uppercase(), &[extension])
                    .save_file()
            })
            .unwrap_or(None);
            let _ = tx.send(picked);
            wake_after_send(waker.as_ref());
        })
        .map_or_else(
            |err| tracing::error!(%err, "cannot spawn the export dialog thread"),
            drop,
        );
    rx
}

/// ★★★ ให้ผู้ใช้ชี้ไฟล์ภาพที่หายไป — ขั้นที่ 5 ของ relink (`docs/07 §2`, P4-6)
///
/// ★ **ไม่บล็อก UI thread** ด้วยเหตุผลเดียวกับ [`pick_document_to_open`] เป๊ะ:
/// ผู้ใช้ที่กำลังไล่หาโฟลเดอร์ที่เขาย้ายภาพไปเมื่อเดือนก่อน อาจใช้เวลาเป็นนาที
///
/// ★★ ตัวกรองเป็นนามสกุลชุดเดียวกับที่ `decode_guarded` รับได้ — ถ้ากว้างกว่านั้น
/// ผู้ใช้จะชี้ไฟล์ที่โปรแกรมเปิดไม่ได้แล้วได้ error ที่เขาทำอะไรกับมันไม่ได้
#[must_use]
pub fn pick_missing_image(
    file_name: &str,
    waker: Option<crate::window::Waker>,
) -> crossbeam_channel::Receiver<Option<PathBuf>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let title = if file_name.is_empty() {
        "หาไฟล์ภาพที่หายไป".to_owned()
    } else {
        format!("หาไฟล์: {file_name}")
    };
    std::thread::Builder::new()
        .name("refx-relink-dialog".to_owned())
        .spawn(move || {
            let picked = std::panic::catch_unwind(|| {
                rfd::FileDialog::new()
                    .set_title(&title)
                    .add_filter(
                        "Images",
                        &[
                            "png", "jpg", "jpeg", "webp", "gif", "bmp", "tga", "tiff", "tif",
                        ],
                    )
                    .pick_file()
            })
            .unwrap_or(None);
            let _ = tx.send(picked);
            wake_after_send(waker.as_ref());
        })
        .map_or_else(
            |err| tracing::error!(%err, "cannot spawn the relink dialog thread"),
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
