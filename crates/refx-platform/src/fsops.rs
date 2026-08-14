//! การสลับไฟล์แบบ **ทนไฟดับ** — ส่วนที่ต้องเรียก OS ตรง ๆ (P4-2)
//!
//! ★★★ **ทำไมอยู่ที่นี่** — `docs/07 §4` กำหนดว่าหลัง `rename` ต้อง fsync
//! *โฟลเดอร์* ด้วย ไม่งั้นตัวชื่อไฟล์เองยังไม่ลงดิสก์: rename แก้ directory entry
//! ซึ่งเป็น metadata คนละก้อนกับเนื้อไฟล์ · บน Unix ทำได้ด้วย `File::open(dir)`
//! แล้ว `sync_all()` (std ล้วน) แต่ **บน Windows `File::open` บนโฟลเดอร์ล้มทันที**
//! ต้องขอ `FILE_FLAG_BACKUP_SEMANTICS` หรือใช้ธงของ `MoveFileExW` ซึ่งทั้งคู่
//! ต้องเรียก Win32 ตรง ๆ = ต้องมี `unsafe`
//!
//! I-5 ไม่ได้ห้ามทำ มันบอกว่าต้องทำ **ที่ไหน** — และที่นั่นคือ crate นี้
//! (ที่เดียวกับ `GlobalMemoryStatusEx` ของ P1 และ `GetUserDefaultLocaleName` ของ i18n)
//!
//! ★★ **ข้อจำกัดของหลักประกันที่ได้ — อ่านก่อนอ้างอิง**
//!
//! เอกสารของ Microsoft เขียนถึง `MOVEFILE_WRITE_THROUGH` ว่า *"The function does
//! not return until the file is actually moved on the disk"* แล้วต่อด้วยประโยค
//! ที่ระบุขอบเขตชัดกว่าว่า *"guarantees that a move performed as a **copy and
//! delete** operation is flushed to disk before the function returns"*
//!
//! ประโยคหลังพูดถึงการย้าย **ข้ามไดรฟ์** (ซึ่งวินโดวส์ทำเป็น copy+delete)
//! ส่วนกรณีของเรา — `.tmp` กับไฟล์จริงอยู่โฟลเดอร์เดียวกัน — เป็นการแก้ metadata
//! ในไดรฟ์เดียว ซึ่งเอกสาร **ไม่ได้ระบุตรง ๆ** ว่าครอบด้วย
//!
//! → ธงนี้จึงเป็น **"ดีที่สุดเท่าที่ API เปิดให้"** ไม่ใช่หลักประกันที่พิสูจน์ได้
//!   ว่าปิดช่องสนิท · ที่แน่นอนคือมันไม่ได้แย่ลงกว่าไม่ใส่ และเป็นสิ่งเดียวกับ
//!   ที่โปรแกรมฐานข้อมูลบนวินโดวส์ใช้กัน
//!
//! spec: docs/07-file-format.md §4, ARCHITECTURE §1 I-5

use std::path::Path;

/// สลับ `from` ไปทับ `to` แล้วพยายามให้ตัวการสลับเองลงดิสก์จริง
///
/// เทียบเท่า `std::fs::rename` ทุกประการในแง่ผลลัพธ์ — ต่างกันแค่ความพยายาม
/// เรื่องความทนทาน
///
/// # Errors
/// คืน error ของระบบไฟล์ตามเดิม — ผู้เรียกจัดการเหมือน `std::fs::rename`
pub fn rename_durable(from: &Path, to: &Path) -> std::io::Result<()> {
    platform_rename(from, to)?;
    sync_parent_dir(to);
    Ok(())
}

/// ★ Windows — `MoveFileExW` พร้อม `MOVEFILE_WRITE_THROUGH`
///
/// `std::fs::rename` เรียก `MoveFileExW` ด้วย `MOVEFILE_REPLACE_EXISTING`
/// อย่างเดียว **ไม่ได้ใส่ `MOVEFILE_WRITE_THROUGH`** จึงคืนค่าได้ก่อนที่การสลับ
/// จะลงดิสก์ · ที่นี่ใส่ธงนั้นเพิ่ม (ดูข้อจำกัดในหัวโมดูล)
#[cfg(windows)]
fn platform_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    /// ทับไฟล์ปลายทางที่มีอยู่แล้ว (ค่าเดียวกับที่ `std::fs::rename` ใช้)
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    /// ไม่คืนค่าจนกว่าการย้ายจะลงดิสก์
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    // Win32 ต้องการสตริงที่ปิดท้ายด้วย NUL — `encode_wide` ไม่ใส่ให้
    let wide = |path: &Path| -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let (src, dst) = (wide(from), wide(to));

    // SAFETY: `src`/`dst` เป็น `Vec<u16>` ที่ปิดท้ายด้วย NUL และมีชีวิตอยู่ตลอด
    // การเรียก (ผูกกับ binding ข้างบน ไม่ใช่ temporary) · MoveFileExW อ่านอย่างเดียว
    // ไม่เก็บพอยน์เตอร์ไว้ใช้ต่อ และคืนค่าเป็น BOOL ซึ่งเราตรวจทันที
    let ok = unsafe {
        MoveFileExW(
            src.as_ptr(),
            dst.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// แพลตฟอร์มอื่น — `std::fs::rename` เป็น `rename(2)` ซึ่ง atomic อยู่แล้ว
/// ส่วนความทนทานมาจากการ fsync โฟลเดอร์ข้างล่าง
#[cfg(not(windows))]
fn platform_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

/// fsync โฟลเดอร์ที่ไฟล์อยู่ — ทำได้จริงเฉพาะบน Unix
///
/// ★ ล้มแล้ว **ไม่คืน error** โดยตั้งใจ: ถึงจุดนี้ไฟล์อยู่ในตำแหน่งที่ถูกแล้ว
/// การรายงานว่า "บันทึกไม่สำเร็จ" ทั้งที่งานอยู่ครบจะทำให้ผู้ใช้คิดว่างานหาย
///
/// บน Windows การสลับถูกทำผ่าน `MOVEFILE_WRITE_THROUGH` ไปแล้ว (ดูหัวโมดูล)
fn sync_parent_dir(doc: &Path) {
    #[cfg(unix)]
    {
        if let Some(dir) = doc.parent()
            && let Ok(handle) = std::fs::File::open(dir)
        {
            let _ = handle.sync_all();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = doc;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-fsops-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// สลับไฟล์ได้จริง และ **ทับปลายทางที่มีอยู่แล้วได้**
    ///
    /// ข้อหลังคือสิ่งที่ `save_atomic` พึ่งอยู่ — ถ้าทับไม่ได้ การบันทึกครั้งที่สอง
    /// จะล้มทุกครั้ง (และเป็นพฤติกรรมที่ต่างกันระหว่าง Windows กับ Unix ถ้าใช้ API ผิด)
    #[test]
    fn renaming_replaces_an_existing_destination() {
        let dir = temp_dir("replace");
        let from = dir.join("new.tmp");
        let to = dir.join("doc.refx");
        std::fs::write(&from, b"new content").unwrap();
        std::fs::write(&to, b"old content").unwrap();

        rename_durable(&from, &to).unwrap();

        assert!(!from.exists(), "ไฟล์ต้นทางต้องหายไปหลังสลับ");
        let mut back = Vec::new();
        std::io::Read::read_to_end(&mut std::fs::File::open(&to).unwrap(), &mut back).unwrap();
        assert_eq!(back, b"new content");
    }

    /// ปลายทางที่ยังไม่มีไฟล์อยู่ ก็ต้องสลับได้
    #[test]
    fn renaming_works_when_the_destination_is_new() {
        let dir = temp_dir("fresh");
        let from = dir.join("new.tmp");
        let to = dir.join("doc.refx");
        std::fs::write(&from, b"content").unwrap();

        rename_durable(&from, &to).unwrap();
        assert!(to.exists());
    }

    /// ต้นทางที่ไม่มีอยู่ต้องคืน `Err` ไม่ใช่ panic
    #[test]
    fn a_missing_source_is_an_error_not_a_panic() {
        let dir = temp_dir("missing");
        let err = rename_durable(&dir.join("nope.tmp"), &dir.join("doc.refx"));
        assert!(err.is_err());
    }

    /// ★ ราคาของ `MOVEFILE_WRITE_THROUGH` — **พิมพ์ไว้ ไม่ assert**
    ///
    /// `docs/08 §3.9` ข้อ 5b ห้าม assert เวลานาฬิกา · ตัวเลขนี้มีไว้ให้คนอ่าน
    /// ตัดสินว่าการบันทึกจะรู้สึกช้าไหม ไม่ใช่เกณฑ์ผ่าน/ไม่ผ่าน
    ///
    /// ที่ต้องรู้: การบันทึกเกิดตอนผู้ใช้กด `Ctrl+S` ซึ่งเป็นการกระทำที่ตั้งใจ
    /// ทีละครั้ง ไม่ใช่ทุกเฟรม — ราคาระดับมิลลิวินาทีจึงรับได้สบาย
    #[test]
    fn how_long_a_durable_rename_takes() {
        let dir = temp_dir("timing");
        let to = dir.join("doc.refx");
        let payload = vec![7u8; 256 * 1024];

        let mut total = std::time::Duration::ZERO;
        const ROUNDS: u32 = 20;
        for round in 0..ROUNDS {
            let from = dir.join(format!("{round}.tmp"));
            std::fs::write(&from, &payload).unwrap();
            let start = std::time::Instant::now();
            rename_durable(&from, &to).unwrap();
            total += start.elapsed();
        }
        println!(
            "rename_durable (256 KB, {ROUNDS} รอบ): รวม {:?} · เฉลี่ย {:?}",
            total,
            total / ROUNDS
        );
    }
}
