//! ★★★ ที่พักของภาพที่ **ไม่มีไฟล์ต้นทาง** — trait ที่กลับทิศไว้ (P4-5)
//!
//! ## ทำไม trait อยู่ที่นี่ ไม่ใช่ที่ `refx-io`
//!
//! คนที่ *มีไบต์* คือ worker ใน `refx-asset` (มันเพิ่ง decode clipboard เสร็จ)
//! คนที่ *รู้ว่าไฟล์ต้องไปไหนและเขียนยังไงให้ atomic* คือ `refx-io::spool`
//! แต่ **`refx-asset` พึ่ง `refx-io` ไม่ได้** — ARCHITECTURE §2 วางสองตัวนี้ไว้
//! เป็นพี่น้องกัน ทั้งคู่ขึ้นตรงกับ `refx-ui` ไม่ใช่ต่อกันเอง
//!
//! → กลับทิศด้วย trait ใน `refx-core` แล้วให้ `refx-ui` เสียบตัวจริงให้
//! **หลักการเดียวกับ [`crate::clipboard::ClipboardReader`] เป๊ะ** ซึ่งเกิดจาก
//! ปัญหาเดียวกันเมื่อ 2 ส.ค. (ตอนนั้น `refx-asset` พึ่ง `refx-platform`
//! แล้วลาก `rfd`/`winit`/`arboard` เข้า fuzz จนคอมไพล์ nightly ไม่ผ่าน)
//!
//! spec: docs/07-file-format.md §2, ARCHITECTURE §2

use std::path::PathBuf;

use crate::hash::ContentHash;

/// ที่พักของภาพที่วางจาก clipboard — ตัวจริงเขียนลง `<data_local_dir>/pasted/`
///
/// ★ คืน `None` เมื่อเก็บไม่สำเร็จ · **ไม่ใช่ error ที่ต้องหยุดงาน**: ภาพยังขึ้นจอ
/// ได้ตามปกติจาก thumbnail ที่อยู่ใน RAM แล้ว สิ่งที่เสียไปคือความคมตอนซูม
/// กับความสามารถในการกู้คืน ซึ่งควรถูก log ไว้ ไม่ใช่ทำให้การวางภาพล้มทั้งใบ
pub trait PastedImageStore: Send + Sync {
    /// เก็บไบต์ (PNG) ของภาพนี้ แล้วคืน path ที่มันไปอยู่
    fn store(&self, hash: ContentHash, png: &[u8]) -> Option<PathBuf>;
}
