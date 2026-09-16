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
    title: &str,
    suggested_name: &str,
    waker: Option<crate::window::Waker>,
) -> crossbeam_channel::Receiver<Option<PathBuf>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let title = title.to_owned();
    let name = suggested_name.to_owned();
    std::thread::Builder::new()
        .name("refx-save-dialog".to_owned())
        .spawn(move || {
            // rfd ล้มได้ถ้าไม่มี display (เช่นรันใน CI) — ห้ามให้ลามเป็น panic
            let picked = std::panic::catch_unwind(|| {
                rfd::FileDialog::new()
                    .set_title(&title)
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
    title: &str,
    waker: Option<crate::window::Waker>,
) -> crossbeam_channel::Receiver<Option<PathBuf>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let title = title.to_owned();
    std::thread::Builder::new()
        .name("refx-open-dialog".to_owned())
        .spawn(move || {
            let picked = std::panic::catch_unwind(|| {
                rfd::FileDialog::new()
                    .set_title(&title)
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
    title: &str,
    suggested_name: &str,
    extension: &'static str,
    waker: Option<crate::window::Waker>,
) -> crossbeam_channel::Receiver<Option<PathBuf>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let title = title.to_owned();
    let name = suggested_name.to_owned();
    std::thread::Builder::new()
        .name("refx-export-dialog".to_owned())
        .spawn(move || {
            let picked = std::panic::catch_unwind(|| {
                rfd::FileDialog::new()
                    .set_title(&title)
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
    title: &str,
    waker: Option<crate::window::Waker>,
) -> crossbeam_channel::Receiver<Option<PathBuf>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let title = title.to_owned();
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
/// ★★★ ภาษาอังกฤษ **และไม่ผ่านระบบแปล** โดยตั้งใจ (`docs/03 §0`: ภาษาหลักคืออังกฤษ)
///
/// ข้อความนี้ถูกสร้างขึ้นระหว่าง panic — ตอนนั้นสถานะของโปรแกรมเชื่อไม่ได้แล้ว
/// การไปหยิบตารางคำแปล (ซึ่งอยู่ในชั้น UI ที่อาจเป็นตัวที่เพิ่ง panic ไป)
/// คือการเพิ่มโอกาส **panic ซ้อน panic** ซึ่งจะ abort ทันทีและผู้ใช้จะไม่ได้เห็น
/// ข้อความอะไรเลย · ค่าคงที่ที่ไม่พึ่งอะไรเลยคือสิ่งเดียวที่เชื่อได้บนเส้นทางนี้
pub fn show_crash_dialog(log_path: &std::path::Path) {
    let message = crash_message(log_path);

    // rfd อาจล้มได้ถ้าไม่มี display (เช่นรันใน CI) — ห้ามให้ error ตรงนี้ลาม
    let result = std::panic::catch_unwind(|| {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("RefX stopped")
            .set_description(&message)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    });

    if result.is_err() {
        // เหลือทางเดียวคือ stderr
        eprintln!("{message}");
    }
}

/// ★★★ บอกผู้ใช้ว่า **เปิดโปรแกรมไม่ขึ้น** — และทำไม
///
/// ## ทำไมต้องมี ทั้งที่มี `show_crash_dialog` อยู่แล้ว
///
/// อันนั้นต่อไว้กับ **panic hook** เท่านั้น · ความล้มเหลวตอนเปิด (ไม่มี GPU ที่ใช้ได้
/// · ไดรเวอร์เพิ่งอัปเดตแล้วพัง · `WGPU_BACKEND` ตั้งผิด) ไม่ใช่ panic — มันเป็น
/// `Err` ที่ไหลกลับไปถึง `main` แล้วถูกพิมพ์ลง **stderr**
///
/// ★★ และ RefX ถูก build เป็น **GUI application** ซึ่งบน Windows แปลว่า
/// **ไม่มี stderr ให้พิมพ์** (`KNOWN-LIMITATIONS` ข้อ 2) · ผลที่ผู้ใช้เห็นจริง
/// คือ **ดับเบิลคลิกแล้วไม่เกิดอะไรขึ้นเลย** ซึ่งอ่านไม่ออกว่าเป็นอะไร
/// และแยกไม่ออกจาก "โปรแกรมไม่ทำงาน"
///
/// เจอ 19 ก.ย. 2026 ระหว่างทดสอบ `WGPU_BACKEND` ที่ตั้งผิด — ข้อความที่เขียนไว้
/// อย่างดีลง log ไม่มีค่าอะไรเลยถ้าผู้ใช้ไม่รู้ว่ามี log ให้เปิด
///
/// **บล็อกได้** ด้วยเหตุผลเดียวกับ [`show_crash_dialog`]: ไม่มีหน้าต่าง ไม่มี
/// event loop และโปรแกรมกำลังจะจบอยู่แล้ว (ข้อยกเว้นของ I-2 ที่ยอมรับได้)
pub fn show_startup_failure(details: &str, log_path: &std::path::Path) {
    let message = startup_failure_message(details, log_path);
    let result = std::panic::catch_unwind(|| {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("RefX cannot start")
            .set_description(&message)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    });
    if result.is_err() {
        eprintln!("{message}");
    }
}

/// ข้อความของ [`show_startup_failure`] — แยกออกมาเพื่อให้เทสต์อ่านได้
///
/// ★ ภาษาอังกฤษและไม่ผ่านระบบแปล ด้วยเหตุผลเดียวกับ [`crash_message`]:
/// ชั้น UI อาจเป็นชั้นที่เพิ่งล้มไป การไปหยิบตารางคำแปลจากตรงนั้นคือการพึ่ง
/// สิ่งที่เชื่อไม่ได้แล้ว
fn startup_failure_message(details: &str, log_path: &std::path::Path) -> String {
    format!(
        "RefX could not start.\n\n\
         {details}\n\n\
         This usually means the graphics driver refused to start, or was updated \
         while RefX was closed. Restarting the machine fixes most cases.\n\n\
         The full log is here:\n{}",
        log_path.display()
    )
}

/// ข้อความที่ผู้ใช้จะได้อ่านตอนโปรแกรมพัง
///
/// แยกออกมาเพราะ `show_crash_dialog` เปิดหน้าต่างจริง — เทสต์เรียกไม่ได้
/// แล้วข้อความที่ผู้ใช้เห็นในนาทีที่แย่ที่สุดก็จะไม่เคยถูกตรวจเลย
fn crash_message(log_path: &std::path::Path) -> String {
    format!(
        "RefX stopped unexpectedly.\n\n\
         The details were written to:\n{}\n\n\
         Please attach that file if you report this.",
        log_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★★★ ข้อความตอนโปรแกรมพังต้องเป็นภาษาอังกฤษ (`docs/03 §0`) และต้อง **บอกที่อยู่
    /// ของไฟล์ log** — ไม่งั้นผู้ใช้ได้แค่ "โปรแกรมพัง" ซึ่งทำอะไรต่อไม่ได้เลย
    #[test]
    fn the_crash_message_is_english_and_says_where_the_log_is() {
        let message = crash_message(std::path::Path::new("C:/logs/refx.log"));
        assert!(message.is_ascii(), "ข้อความตอน crash ไม่ใช่ ASCII: {message}");
        assert!(
            message.contains("C:/logs/refx.log"),
            "ไม่ได้บอกว่า log อยู่ไหน — ผู้ใช้แนบไฟล์มาให้ไม่ได้: {message}"
        );
    }

    /// ★★★ เปิดไม่ขึ้นต้อง **บอกด้วยหน้าต่าง** ไม่ใช่แค่ลง log
    ///
    /// RefX เป็น GUI application ซึ่งบน Windows ไม่มี stderr — ข้อความที่เขียน
    /// ไว้อย่างดีจึงหายไปทั้งหมด และผู้ใช้เห็นแค่ "ดับเบิลคลิกแล้วไม่เกิดอะไร"
    #[test]
    fn the_cannot_start_message_names_the_reason_and_the_log() {
        let message = startup_failure_message(
            "WGPU_BACKEND=\"gl\": this build does not contain gl",
            std::path::Path::new("C:/logs/refx.log"),
        );
        assert!(message.is_ascii(), "ข้อความตอนเปิดไม่ขึ้นไม่ใช่ ASCII: {message}");
        assert!(
            message.contains("WGPU_BACKEND"),
            "ไม่ได้บอกเหตุผลที่แท้จริง มันจึงเป็นแค่ 'พังนะ': {message}"
        );
        assert!(
            message.contains("C:/logs/refx.log"),
            "ไม่ได้บอกว่า log อยู่ไหน: {message}"
        );
        // ★ ต้องบอก **สิ่งที่ทำได้ต่อ** ไม่ใช่แค่สิ่งที่เกิดขึ้น (`CLAUDE.md`)
        assert!(
            message.to_lowercase().contains("restart"),
            "ไม่ได้บอกว่าทำอะไรต่อได้: {message}"
        );
    }
}
