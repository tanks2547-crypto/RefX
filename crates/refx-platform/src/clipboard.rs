//! อ่านภาพ/รายชื่อไฟล์จาก clipboard ของระบบ (P1-8)
//!
//! ★ **โมดูลนี้ไม่ตัดสินว่ารับภาพได้ไหม** หน้าที่มันคือ "หยิบของออกมาจาก OS"
//! อย่างเดียว เพดานทั้งหมด (`max_pixels` ที่ผูกกับ RAM เครื่อง, ขนาด, ความสอดคล้อง
//! ของบัฟเฟอร์) อยู่ที่ `refx-asset::decode` **ที่เดียว** เหมือนไฟล์บนดิสก์ทุกประการ
//! ถ้าตรวจสองที่ เกราะสองชุดจะเพี้ยนจากกันวันที่มีคนแก้ข้างเดียว (เหตุผลเดียวกับ
//! ที่ `read_file_guarded` ถูกแยกออกมาให้ decode pool ใช้ร่วมกัน)
//!
//! **ห้ามเรียกบน UI thread** (I-2): การเปิด clipboard รอ OS ได้นานเป็นวินาที
//! ถ้าโปรแกรมอื่นถือมันค้างอยู่ (Windows: `OpenClipboard` ต้องรอเจ้าของเดิมปล่อย
//! และเจ้าของอาจเป็นโปรแกรมที่กำลังค้าง) — ของจริงเรียกจาก decode worker
//!
//! ★ **ชนิดข้อมูลย้ายไป `refx_core::clipboard` แล้ว** (HANDOFF §2.0)
//! `refx-asset` ต้องอ่าน clipboard บน worker แต่ต้องไม่ depend `arboard`/`winit`/`rfd`
//! จึงกลับทิศ: core ถือ trait + DTO ส่วนที่นี่คือตัว implement ที่คุยกับ OS จริง
//! และ `refx-ui` เป็นคนเสียบ [`SystemClipboard`] เข้า decode pool ตอนสร้าง
//!
//! spec: docs/03 §5 (`Ctrl+V`), ARCHITECTURE §1 I-4

use refx_core::clipboard::{
    ClipboardContent, ClipboardError, ClipboardImage, ClipboardReader as CoreClipboardReader,
    ClipboardWriter as CoreClipboardWriter,
};

/// แปลง error ของ `arboard` เป็นของเรา
///
/// แยกเป็นฟังก์ชันเพราะเรียกจากสองจุด และเพื่อให้เห็นชัดว่าเคสไหนแมปไปไหน
fn translate(err: &arboard::Error) -> ClipboardError {
    match err {
        arboard::Error::ContentNotAvailable => ClipboardError::NoImage,
        arboard::Error::ClipboardOccupied => ClipboardError::Busy,
        arboard::Error::ConversionFailure => ClipboardError::Undecodable,
        // ClipboardNotSupported / Unknown / variant ใหม่ในอนาคต — ทั้งหมดคือ
        // "ใช้ clipboard ไม่ได้" จากมุมของผู้ใช้ เก็บคำอธิบายไว้ให้ log
        other => ClipboardError::Unavailable {
            reason: other.to_string(),
        },
    }
}

/// clipboard ของเครื่องจริง — ตัวที่ `refx-ui` เสียบเข้า decode pool
///
/// ไม่ถือสถานะอะไรเลย (`arboard::Clipboard` ถูกสร้างใหม่ทุกครั้งที่อ่าน)
/// จึงแชร์ข้ามเธรดได้โดยไม่ต้องล็อก
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClipboard;

impl CoreClipboardReader for SystemClipboard {
    fn read(&self) -> Result<ClipboardContent, ClipboardError> {
        read()
    }
}

impl CoreClipboardWriter for SystemClipboard {
    fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
        write_text(text)
    }
}

/// เขียนข้อความลง clipboard ของระบบ (P2-10 — ก๊อป hex ของสีที่จิ้มได้)
///
/// **ห้ามเรียกบน UI thread** (I-2) — บน Windows clipboard เป็น global lock
/// ของทั้งระบบ `OpenClipboard` รอเจ้าของเดิมปล่อยได้นานเป็นวินาที
///
/// ★ `arboard` บน X11 ต้องมีโปรเซสอยู่ค้างเพื่อ *เสิร์ฟ* ค่าที่เขียนไว้ (X11 ไม่ได้
/// เก็บ clipboard ไว้ที่ server) — `set().text()` ธรรมดาจึงหายทันทีที่ RefX ปิด
/// ยอมรับข้อจำกัดนี้ไปก่อน: ผู้ใช้ก๊อป hex แล้วไปวางใน Photoshop ทันที
/// ซึ่งเป็นตอนที่ RefX ยังเปิดอยู่แน่นอน (`wait()` จะบล็อกเธรดนี้ค้างไว้ตลอด
/// จนกว่าจะมีคนก๊อปทับ ซึ่งแลกไม่คุ้มกับเธรดที่ค้างหนึ่งตัวต่อการก๊อปหนึ่งครั้ง)
///
/// # Errors
/// คืน [`ClipboardError`] เมื่อเปิดหรือเขียน clipboard ไม่ได้ — ไม่ panic
pub fn write_text(text: &str) -> Result<(), ClipboardError> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| translate(&err))?;
    clipboard.set_text(text).map_err(|err| translate(&err))?;
    tracing::debug!(len = text.len(), "wrote text to the clipboard");
    Ok(())
}

/// อ่านสิ่งที่อยู่ใน clipboard ตอนนี้
///
/// ลองรายชื่อไฟล์ก่อนเสมอ เพราะถ้าผู้ใช้ก๊อปไฟล์ภาพมา การเปิดจากไฟล์จริง
/// ให้ผลดีกว่าทุกทาง (ผ่าน cache, อ่าน EXIF orientation, ขอภาพคมตอนซูมได้)
///
/// **ห้ามเรียกบน UI thread** (I-2)
///
/// # Errors
/// คืน [`ClipboardError`] เสมอเมื่อไม่มีอะไรที่ใช้ได้ — ไม่ panic ไม่ว่าใน
/// clipboard จะมีอะไรอยู่
pub fn read() -> Result<ClipboardContent, ClipboardError> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| translate(&err))?;

    // 1. ไฟล์ก่อน — รายการว่างถือว่าไม่มี (บาง OS คืน Ok(vec![]) แทนที่จะเป็น Err)
    match clipboard.get().file_list() {
        Ok(files) if !files.is_empty() => {
            tracing::debug!(count = files.len(), "clipboard holds a file list");
            return Ok(ClipboardContent::Files(files));
        }
        // ไม่มีไฟล์ = เรื่องปกติ ไปลองภาพต่อ ไม่ใช่ error ของผู้ใช้
        Ok(_) | Err(_) => {}
    }

    // 2. ภาพดิบ
    let image = clipboard.get().image().map_err(|err| translate(&err))?;
    let width = u32::try_from(image.width).unwrap_or(u32::MAX);
    let height = u32::try_from(image.height).unwrap_or(u32::MAX);
    let rgba = image.bytes.into_owned();

    tracing::debug!(
        width,
        height,
        bytes = rgba.len(),
        "clipboard holds an image"
    );
    Ok(ClipboardContent::Image(ClipboardImage {
        // ค่าที่ใหญ่เกิน u32 ไม่มีทางผ่านเพดาน `max_dimension` (65535) อยู่แล้ว
        // จึงย่อเป็น u32::MAX เพื่อให้เกราะฝั่ง refx-asset ปฏิเสธด้วยเหตุผลเดียวกัน
        // แทนที่จะต้องมีเส้นทาง error ซ้ำอีกชุดที่นี่
        width,
        height,
        rgba,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// ★ ทุก error ของ `arboard` ต้องมีที่ลง — ห้ามมีเคสที่หลุดไปเป็น panic
    #[test]
    fn every_arboard_error_maps_to_something_useful() {
        let cases = [
            arboard::Error::ContentNotAvailable,
            arboard::Error::ClipboardOccupied,
            arboard::Error::ConversionFailure,
            arboard::Error::ClipboardNotSupported,
            arboard::Error::Unknown {
                description: "boom".to_owned(),
            },
        ];
        for case in &cases {
            let mapped = translate(case);
            assert!(
                !mapped.to_string().trim().is_empty(),
                "{case:?} แมปแล้วได้ข้อความว่าง"
            );
        }

        assert!(matches!(
            translate(&arboard::Error::ContentNotAvailable),
            ClipboardError::NoImage
        ));
        assert!(matches!(
            translate(&arboard::Error::ClipboardOccupied),
            ClipboardError::Busy
        ));
        // ★ ขยะใน clipboard = Undecodable ไม่ใช่ "ไม่มีภาพ" — ผู้ใช้ต้องรู้ว่า
        //   มีอะไรอยู่จริงแต่พัง จะได้ก๊อปใหม่ ไม่ใช่นั่งงงว่าทำไมไม่มีอะไรเกิดขึ้น
        assert!(matches!(
            translate(&arboard::Error::ConversionFailure),
            ClipboardError::Undecodable
        ));
        assert!(matches!(
            translate(&arboard::Error::ClipboardNotSupported),
            ClipboardError::Unavailable { .. }
        ));
    }

    /// ชื่อ env ที่ CI ใช้บอกว่า **job นี้ต้องมี clipboard จริง**
    ///
    /// คู่ขนานกับ `REFX_REQUIRE_GPU` ของ `refx-render` โดยตั้งใจ — กติกาการ
    /// ข้าม/บังคับควรมีรูปเดียวทั้งโปรเจกต์ คนอ่าน log จะได้ไม่ต้องเรียนใหม่
    const REQUIRE_CLIPBOARD_ENV: &str = "REFX_REQUIRE_CLIPBOARD";

    /// ค่าใน env นี้แปลว่า "ต้องมี clipboard" หรือไม่
    ///
    /// ★ ว่างเปล่าต้องแปลว่า **ไม่บังคับ** — GitHub Actions ตั้ง env เป็นสตริงว่าง
    /// เมื่อ expression ไม่เข้าเงื่อนไข ถ้าตีความว่า "มีค่า = บังคับ" job ที่ไม่ควร
    /// บังคับจะแดงทันทีโดยไม่มีใครเข้าใจว่าทำไม (เหตุผลเดียวกับ `gpu_required_from`)
    fn clipboard_required_from(value: Option<&str>) -> bool {
        matches!(value, Some(v) if !v.is_empty() && v != "0")
    }

    /// เครื่องนี้ไม่มี clipboard ให้ใช้ — จะข้ามหรือจะล้ม
    ///
    /// ★★★ **นี่คือหัวใจของการแก้รอบนี้** (`docs/08 §3.9` ข้อ 2)
    ///
    /// เดิมสองเทสต์ข้างล่างรับ `Err` **ทุกชนิด** ว่าผ่าน เจตนาเดิมคือ "ต้องไม่
    /// panic บนเครื่องที่ไม่มี display" ซึ่งถูกต้องในตัวมันเอง — แต่ผลข้างเคียง
    /// คือบน runner headless มันเขียวโดย **ไม่ได้แตะ clipboard จริงสักครั้ง**
    /// แล้วบรรทัด "645/645 ผ่าน" ในทุกรายงานก็อ่านได้ว่า clipboard ถูกตรวจแล้ว
    /// ทั้งที่ไม่เคยมีใครตรวจเลย = **หลักฐานปลอมที่ผลิตซ้ำทุกรอบ**
    ///
    /// คำเตือนที่เขียนไว้ในรายงานของ session หายไปพร้อม session
    /// ส่วนการข้ามที่พิมพ์เหตุผลอยู่ใน log **ทุกครั้งที่รัน**
    ///
    /// # Panics
    /// panic เมื่อ `required` เป็นจริง — นั่นคือพฤติกรรมที่ต้องการบน job ที่บังคับ
    fn no_clipboard_available(required: bool, why: &ClipboardError) {
        assert!(
            !required,
            "ตั้ง {REQUIRE_CLIPBOARD_ENV}=1 ไว้แต่เปิด clipboard ของระบบไม่ได้ ({why}) — \
             job นี้ถูกกำหนดให้เป็น job ที่ตรวจ clipboard จริง ถ้า runner ไม่มีให้ใช้ \
             ให้แก้ที่ CI ไม่ใช่ปลดการบังคับทิ้ง ไม่งั้นจะไม่เหลือใครตรวจกลุ่มนี้เลย"
        );
        println!(
            "ข้าม: เครื่องนี้ไม่มี clipboard ที่ใช้ได้ ({why}) \
             (ไม่ได้ตั้ง {REQUIRE_CLIPBOARD_ENV}) — เส้นทาง clipboard **ไม่ถูกตรวจ** ในรอบนี้"
        );
    }

    /// ข้ามได้ไหมสำหรับ error ตัวนี้ — `true` = ข้ามแล้ว ผู้เรียกจบเทสต์ได้เลย
    ///
    /// ★★ **แยก "ไม่มี clipboard" ออกจาก "clipboard ใช้ได้แต่ไม่มีของที่ต้องการ"**
    ///
    /// | error | แปลว่า | ทำ |
    /// |---|---|---|
    /// | `Unavailable` | เปิด clipboard ของ OS ไม่ได้เลย (headless / ไม่รองรับ) | **ข้ามพร้อมเหตุผล** |
    /// | `NoImage` · `Busy` · `Undecodable` | clipboard **ใช้ได้จริง** แค่ไม่มีภาพอยู่ | ผ่าน — เส้นทางเดินครบแล้ว |
    ///
    /// การรวมสองอย่างนี้เข้าด้วยกันคือบั๊กทั้งหมดของเทสต์ชุดเดิม
    fn skipped_because_unavailable(err: &ClipboardError) -> bool {
        if matches!(err, ClipboardError::Unavailable { .. }) {
            no_clipboard_available(
                clipboard_required_from(std::env::var(REQUIRE_CLIPBOARD_ENV).ok().as_deref()),
                err,
            );
            return true;
        }
        false
    }

    /// ★ negative control ของประตูนี้เอง (`docs/08 §3.9` ข้อ 1)
    ///
    /// ถ้าสาขา "บังคับแล้วไม่มี" ไม่ล้ม การตั้ง `REFX_REQUIRE_CLIPBOARD` บน CI
    /// ก็ไม่มีความหมายอะไรเลย — ทดสอบได้โดยไม่ต้องถอด display จริง
    #[test]
    #[should_panic(expected = "REFX_REQUIRE_CLIPBOARD")]
    fn requiring_a_clipboard_that_is_missing_is_a_failure_not_a_skip() {
        no_clipboard_available(
            true,
            &ClipboardError::Unavailable {
                reason: "จำลองว่า runner ไม่มี clipboard".to_owned(),
            },
        );
    }

    /// ★★★ **การจำแนกว่า error ไหน "ข้าม" และไหน "ตรวจแล้ว" คือทั้งหมดของการแก้รอบนี้**
    ///
    /// ถ้าเส้นแบ่งนี้เลื่อน เทสต์จะกลับไปเขียวแบบว่างเปล่าเหมือนเดิมทันที
    /// จึงต้องมีเทสต์เป็นเจ้าของ ไม่ใช่ปล่อยให้เป็น `matches!` ที่ไม่มีใครเฝ้า
    ///
    /// ★ เทสต์นี้จำเป็นเป็นพิเศษเพราะบนเครื่องที่ **มี** clipboard (เช่นเครื่อง
    /// พัฒนาและ runner Windows) สาขา `Unavailable` ไม่มีวันถูกเดินถึงเลย
    /// — ถ้าไม่ยิงตรง ๆ แบบนี้ ทางข้ามจะไม่เคยถูกตรวจจนถึงวันที่ต้องใช้จริง
    #[test]
    fn only_a_missing_clipboard_counts_as_a_skip() {
        // ไม่มี clipboard เลย → ข้าม (ไม่ panic เพราะไม่ได้บังคับ)
        assert!(skipped_because_unavailable(&ClipboardError::Unavailable {
            reason: "headless".to_owned(),
        }));

        // ★ clipboard **ใช้ได้จริง** แค่ไม่มีของที่ขอ — ห้ามนับเป็นการข้าม
        //   ไม่งั้นเครื่องที่ clipboard ว่างจะรายงานว่า "ไม่ได้ตรวจ" ทั้งที่ตรวจครบแล้ว
        for reachable in [
            ClipboardError::NoImage,
            ClipboardError::Busy,
            ClipboardError::Undecodable,
        ] {
            assert!(
                !skipped_because_unavailable(&reachable),
                "{reachable:?} แปลว่า clipboard เปิดได้แล้ว ต้องไม่ถูกนับเป็นการข้าม"
            );
        }
    }

    /// ★ ค่าว่างของ env ต้องแปลว่า "ไม่บังคับ" (GitHub Actions ส่งสตริงว่างมา)
    #[test]
    fn an_empty_requirement_flag_means_not_required() {
        assert!(!clipboard_required_from(None));
        assert!(!clipboard_required_from(Some("")));
        assert!(!clipboard_required_from(Some("0")));
        assert!(clipboard_required_from(Some("1")));
        assert!(clipboard_required_from(Some("true")));
    }

    /// ★ เขียน clipboard จริงบนเครื่องที่รันเทสต์
    ///
    /// เรียกผ่าน **trait** ด้วยเหตุผลเดียวกับตัวอ่าน: สิ่งที่ชั้น UI ใช้จริงคือ
    /// `SystemClipboard as ClipboardWriter` ถ้าเทสต์ตรวจแต่ `write_text()`
    /// การเสียบ trait ที่พังจะไม่มีใครจับได้ (docs/08 §3.9 ข้อ 1)
    ///
    /// ★★ **เขียนค่าที่ไม่มีความหมายลงไปโดยตั้งใจ** — เทสต์นี้ทับ clipboard ของ
    /// คนที่รันมันอยู่ ซึ่งเป็นผลข้างเคียงที่หลีกไม่ได้ถ้าจะตรวจเส้นทางจริง
    /// จึงเขียนสตริงที่บอกตัวเองว่ามาจากไหน ไม่ใช่ค่าที่ดูเหมือนของจริง
    ///
    /// ★★★ **ต่างจากรุ่นก่อน: `Err` ไม่ใช่ "ผ่าน" อีกต่อไป**
    /// มีทางเดียวที่ยอมได้คือ `Unavailable` ซึ่งถูก**ข้ามพร้อมพิมพ์เหตุผล**
    /// · การเขียนข้อความธรรมดาแล้วได้ `Busy`/`Undecodable` คือปัญหาจริงที่ต้องแดง
    #[test]
    fn writing_to_the_real_clipboard_lands_without_an_error() {
        let writer: &dyn CoreClipboardWriter = &SystemClipboard;
        match writer.write_text("refx-test-clipboard-write") {
            Ok(()) => println!("ตรวจแล้ว: เขียน clipboard ของระบบสำเร็จจริง"),
            Err(err) => {
                if skipped_because_unavailable(&err) {
                    return;
                }
                panic!("เขียนข้อความธรรมดาลง clipboard ที่ใช้งานได้ แต่ล้มเหลว: {err}");
            }
        }
    }

    /// อ่าน clipboard จริงบนเครื่องที่รันเทสต์
    ///
    /// เทสต์นี้พิสูจน์ว่าเส้นทางจริงเดินได้ทั้งเส้น (สร้าง `Clipboard` → ถามไฟล์ →
    /// ถามภาพ → แมป error)
    ///
    /// ★ เรียกผ่าน **trait** ไม่ใช่ `read()` ตรง ๆ เพราะสิ่งที่ decode pool ใช้จริง
    /// คือ `SystemClipboard as ClipboardReader` — ถ้าเทสต์ตรวจแต่ `read()`
    /// การเสียบ trait ที่พังจะไม่มีใครจับได้ (docs/08 §3.9 ข้อ 1)
    ///
    /// ★★ `NoImage` **ยังนับว่าผ่าน** เพราะมันแปลว่า clipboard เปิดได้จริงแล้ว
    /// แค่ไม่มีภาพอยู่ข้างใน — ต่างจาก `Unavailable` ที่แปลว่าไม่มี clipboard เลย
    /// ซึ่งจะถูกข้ามพร้อมเหตุผล ไม่ใช่นับเป็นการตรวจ
    #[test]
    fn reading_the_real_clipboard_walks_the_whole_path() {
        let reader: &dyn CoreClipboardReader = &SystemClipboard;
        match reader.read() {
            Ok(ClipboardContent::Files(files)) => {
                assert!(!files.is_empty());
                println!(
                    "ตรวจแล้ว: อ่านรายชื่อไฟล์จาก clipboard ได้ {} รายการ",
                    files.len()
                );
            }
            Ok(ClipboardContent::Image(image)) => {
                // ห้ามยืนยันว่า len ตรงกับ w×h×4 ตรงนี้ — โมดูลนี้ไม่ตรวจโดยตั้งใจ
                assert!(image.width > 0 || image.rgba.is_empty());
                println!(
                    "ตรวจแล้ว: อ่านภาพจาก clipboard ได้ {}x{}",
                    image.width, image.height
                );
            }
            Err(err) => {
                if skipped_because_unavailable(&err) {
                    return;
                }
                // clipboard เปิดได้ แต่ไม่มีภาพ/ไฟล์อยู่ — เส้นทางเดินครบแล้ว
                assert!(!err.to_string().trim().is_empty());
                println!("ตรวจแล้ว: clipboard เปิดได้จริง แต่ไม่มีภาพอยู่ ({err})");
            }
        }
    }
}
