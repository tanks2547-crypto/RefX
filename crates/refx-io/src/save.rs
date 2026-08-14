//! บันทึกไฟล์แบบ **atomic** — tmp → fsync → rename → fsync dir (P4-2)
//!
//! ★★★ **นี่คือจุดที่ I-3 อยู่ทั้งข้อ** `docs/07 §4` เขียนไว้ตรง ๆ ว่า:
//!
//! > **ห้ามเปิดไฟล์เดิมด้วย `File::create` แล้วเขียนทับ** — ถ้าตายกลางทาง
//! > งานของผู้ใช้หายทั้งหมด นี่คือวิธีที่โปรแกรมทำข้อมูลผู้ใช้หายบ่อยที่สุด
//!
//! เหตุผลเชิงกลไก: `File::create` **ตัดไฟล์เดิมเหลือศูนย์ไบต์ทันที** แล้วค่อย
//! เขียนของใหม่ทับ · ช่วงระหว่างนั้นไฟล์ของผู้ใช้อยู่ในสภาพ "ครึ่ง ๆ" ซึ่ง
//! ถ้าโปรแกรมตายตรงนั้น (ไฟดับ, ปิดเครื่อง, crash, End Task) งานหายถาวร
//! ไม่มีทางกู้ · ส่วน `rename` เป็น **atomic ในทุก OS**: ผู้อ่านจะเห็นไฟล์เก่า
//! ทั้งใบ หรือไฟล์ใหม่ทั้งใบ **ไม่มีสภาพระหว่างกลาง**
//!
//! ## ลำดับที่ห้ามสลับ
//!
//! ```text
//! 1. เขียน <doc>.refx.tmp  แล้ว fsync            ← ของใหม่อยู่บนดิสก์จริงแล้ว
//! 2. สำเนาไฟล์เดิมเป็น <doc>.refx.bak (atomic)   ← รุ่นก่อนหน้ายังกลับไปหาได้
//! 3. rename tmp -> <doc>.refx                    ← สลับตัวจริง (atomic)
//! 4. fsync โฟลเดอร์                              ← ให้ตัว rename เองทนไฟดับ
//! ```
//!
//! ★ **fsync ที่ขั้น 1 ต้องมาก่อน rename เสมอ** — ถ้า rename ก่อนแล้วค่อย fsync
//! ไฟฟ้าดับตรงกลางจะได้ชื่อไฟล์ใหม่ที่ชี้ไปยังเนื้อหาที่ยังไม่ลงดิสก์ =
//! ไฟล์ขนาดถูกแต่ข้างในเป็นศูนย์ ซึ่งเป็นอาการคลาสสิกของ ext4/NTFS
//!
//! spec: docs/07-file-format.md §4, ROADMAP P4-2

use std::io::Write as _;
use std::path::{Path, PathBuf};

use refx_core::board::Board;

use crate::dto;

/// นามสกุลของไฟล์ชั่วคราวระหว่างบันทึก
pub const TMP_SUFFIX: &str = "refx.tmp";
/// นามสกุลของไฟล์สำรอง — `CLAUDE.md` อ้างถึงชื่อนี้ในตัวอย่างข้อความ error
pub const BAK_SUFFIX: &str = "refx.bak";
/// ไฟล์ชั่วคราวของตัวสำรอง (ทำให้ `.bak` ถูกสลับแบบ atomic เหมือนกัน)
const BAK_TMP_SUFFIX: &str = "refx.bak.tmp";

/// บันทึกไม่สำเร็จ
///
/// ★ ทุก variant บอก **สิ่งที่เกิดขึ้น + ขั้นที่มันเกิด** เพราะข้อความที่ผู้ใช้เห็น
/// ต้องบอกได้ว่างานของเขายังอยู่ไหม (CLAUDE.md — error ต้องบอกสิ่งที่ทำได้ต่อ)
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    /// ★ ไฟล์ที่จะเขียนทับเป็นของ RefX รุ่นใหม่กว่า — **ไม่แตะอะไรเลย**
    #[error(transparent)]
    Refused(#[from] dto::OpenError),
    /// แปลง `Board` เป็นไบต์ไม่สำเร็จ — ยังไม่ได้แตะดิสก์
    #[error(transparent)]
    Encode(#[from] dto::SaveError),
    /// ล้มระหว่างแตะดิสก์ — บอกด้วยว่าล้มที่ขั้นไหนและไฟล์ไหน
    #[error("could not {step} {}: {source}", path.display())]
    Io {
        /// ขั้นที่ล้ม
        step: &'static str,
        /// ไฟล์ที่กำลังแตะตอนนั้น
        path: PathBuf,
        /// ต้นเหตุจากระบบไฟล์
        source: std::io::Error,
    },
}

impl SaveError {
    fn io(step: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            step,
            path: path.to_path_buf(),
            source,
        }
    }
}

/// เส้นทางของไฟล์ชั่วคราว/สำรองที่คู่กับเอกสารนี้
#[must_use]
pub fn tmp_path(doc: &Path) -> PathBuf {
    doc.with_extension(TMP_SUFFIX)
}

/// เส้นทางของไฟล์สำรอง
#[must_use]
pub fn backup_path(doc: &Path) -> PathBuf {
    doc.with_extension(BAK_SUFFIX)
}

/// บันทึก `Board` ลงไฟล์แบบ atomic
///
/// ★★★ **ตรวจเวอร์ชันของไฟล์เดิมก่อนแตะอะไรทั้งสิ้น** — ไฟล์ที่ RefX รุ่นใหม่กว่า
/// เขียนไว้ห้ามถูกทับเด็ดขาด (docs/07 §3) · ยอมให้ผู้ใช้บันทึกไม่ได้ชั่วคราว
/// ดีกว่าปล่อยให้เขาทับแล้วงานที่รุ่นใหม่เก็บไว้หายโดยไม่รู้ตัว
///
/// # Errors
/// [`SaveError`] — เมื่อถูกปฏิเสธเพราะเวอร์ชัน, แปลงไม่ได้, หรือระบบไฟล์ล้ม
/// · **ไฟล์เดิมยังอยู่ครบเสมอ** ไม่ว่าล้มที่ขั้นไหน
pub fn save_atomic(doc: &Path, board: &Board) -> Result<(), SaveError> {
    // ---- 0. ถ้ามีไฟล์เดิมอยู่ ต้องอ่านหัวมันก่อนว่าเราทับได้ไหม ----
    //
    // ★★ อ่าน **แค่หัวไฟล์** ไม่ใช่ทั้งไฟล์ — สองเหตุผล:
    //    1. หัวไฟล์ตอบคำถามนี้ได้ครบอยู่แล้ว (`dto::inspect` ใช้ 22 ไบต์แรก)
    //    2. `.refx` แบบ packed (P4-5) ฝังภาพต้นฉบับไว้ด้วย จึงใหญ่ระดับ GB ได้
    //       — การสูบทั้งไฟล์เข้า RAM เพื่อดู 22 ไบต์คือการแย่ง RAM กับ Photoshop
    //       ตรงตามที่ `CLAUDE.md` ห้ามไว้ (และเป็นเหตุผลที่ `std::fs::read`
    //       ถูกแบนใน `clippy.toml`)
    let existed = match read_header(doc) {
        Some(header) => {
            dto::may_overwrite(&header)?;
            true
        }
        None => false,
    };

    let bytes = dto::encode(board)?;

    // ---- 1. เขียนของใหม่ลงไฟล์ชั่วคราว แล้ว fsync ----
    let tmp = tmp_path(doc);
    write_and_sync(&tmp, &bytes)?;

    // ---- 2. สำรองไฟล์เดิมไว้ (ถ้ามี) ----
    if existed {
        backup(doc)?;
    }

    // ---- 3. สลับตัวจริง — atomic ----
    //
    // ★ `fs::rename` บน Windows ใช้ `MoveFileEx` พร้อม `MOVEFILE_REPLACE_EXISTING`
    //   จึงทับไฟล์ที่มีอยู่ได้และเป็น atomic เหมือนบน Unix
    std::fs::rename(&tmp, doc).map_err(|err| SaveError::io("replace", doc, err))?;

    // ---- 4. ให้ตัว rename เองทนไฟดับ ----
    sync_parent_dir(doc);
    Ok(())
}

/// อ่านเฉพาะหัวไฟล์ของเอกสารที่มีอยู่ — `None` = ไม่มีไฟล์นั้น (หรือเปิดไม่ได้)
///
/// ★ ไฟล์ที่สั้นกว่าหัวไฟล์คืน "เท่าที่มี" ให้ `dto::inspect` เป็นคนตัดสิน
/// — ที่นี่ไม่ตีความอะไรเลย หน้าที่มันคือหยิบไบต์มาให้เท่านั้น
fn read_header(doc: &Path) -> Option<Vec<u8>> {
    use std::io::Read as _;

    let file = std::fs::File::open(doc).ok()?;
    let mut header = Vec::with_capacity(dto::HEADER_LEN);
    // `take` ทำให้อ่านได้ไม่เกินหัวไฟล์เสมอ ไม่ว่าไฟล์จะใหญ่แค่ไหน
    file.take(dto::HEADER_LEN as u64)
        .read_to_end(&mut header)
        .ok()?;
    Some(header)
}

/// เขียนไฟล์แล้วบังคับให้ลงดิสก์จริง
///
/// ★★ `sync_all()` คือขั้นที่แยก "เขียนแล้ว" ออกจาก "อยู่บนดิสก์แล้ว" — ถ้าไม่มี
/// ข้อมูลจะค้างอยู่ใน page cache ของ OS แล้วไฟดับตรงนั้นจะได้ไฟล์ที่ **ขนาดถูก
/// แต่ข้างในเป็นศูนย์** ซึ่งเป็นอาการที่พบบ่อยที่สุดของการเขียนไฟล์แบบไม่ fsync
fn write_and_sync(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    let mut file = std::fs::File::create(path).map_err(|err| SaveError::io("create", path, err))?;
    file.write_all(bytes)
        .map_err(|err| SaveError::io("write", path, err))?;
    file.sync_all()
        .map_err(|err| SaveError::io("flush", path, err))?;
    Ok(())
}

/// สำรองไฟล์เดิมเป็น `<doc>.refx.bak` แบบ atomic
///
/// ★ **ทำไมสำเนา ไม่ใช่ rename ไฟล์เดิมไปเป็น `.bak`**
///
/// rename จะทำให้มีช่วงที่ `<doc>.refx` **ไม่มีอยู่เลย** ถ้าโปรแกรมตายตรงนั้น
/// ผู้ใช้จะเปิดโฟลเดอร์มาแล้วไม่เห็นไฟล์งานของตัวเอง ซึ่งอ่านได้อย่างเดียวว่า
/// "งานหาย" ถึงแม้ `.bak` จะอยู่ก็ตาม · การสำเนาแพงกว่าแต่ทำให้ path ของเอกสาร
/// **มีไฟล์ที่ใช้ได้อยู่ตลอดเวลา** ไม่มีหน้าต่างว่างเลยสักจังหวะ
///
/// ★ ตัว `.bak` เองก็ถูกสลับด้วย rename เหมือนกัน — สำเนาที่ถูกตัดกลางทาง
/// จะไม่มีวันปรากฏในชื่อ `.bak` (ไฟล์สำรองที่เสียคือไฟล์สำรองที่หลอกให้วางใจ)
/// ★ สำเนาด้วย `fs::copy` **ไม่ใช่การอ่านทั้งไฟล์เข้า RAM แล้วเขียนออก** —
/// `.refx` แบบ packed (P4-5) ใหญ่ระดับ GB ได้ · `fs::copy` ให้ OS จัดการเอง
/// (บน Windows ใช้ `CopyFileEx` ซึ่งไม่สูบทั้งไฟล์เข้าโปรเซสเรา)
fn backup(doc: &Path) -> Result<(), SaveError> {
    let bak_tmp = doc.with_extension(BAK_TMP_SUFFIX);
    std::fs::copy(doc, &bak_tmp).map_err(|err| SaveError::io("back up", doc, err))?;
    // ★ fsync สำเนาก่อนตั้งชื่อจริง ด้วยเหตุผลเดียวกับไฟล์หลัก
    if let Ok(handle) = std::fs::File::open(&bak_tmp) {
        let _ = handle.sync_all();
    }
    let bak = backup_path(doc);
    std::fs::rename(&bak_tmp, &bak).map_err(|err| SaveError::io("replace", &bak, err))?;
    Ok(())
}

/// fsync โฟลเดอร์ที่ไฟล์อยู่ — ทำให้ตัว **rename** เองทนไฟดับ
///
/// ★★ ไฟล์ที่ fsync แล้วยังหายได้ ถ้า *ชื่อ* ของมันยังไม่ลงดิสก์: rename เป็น
/// การแก้ directory entry ซึ่งเป็นคนละ metadata กับตัวไฟล์ (docs/07 §4)
///
/// ★ **บน Windows ทำไม่ได้ผ่าน std** — `File::open` บนโฟลเดอร์ล้มทันที
/// (ต้องใช้ `FILE_FLAG_BACKUP_SEMANTICS` ซึ่งต้องเรียก Win32 ตรง ๆ = ต้องมี
/// `unsafe` ที่ I-5 ห้ามนอก `refx-platform`) · NTFS บันทึกการเปลี่ยน metadata
/// ผ่าน journal ของตัวเองอยู่แล้ว จึงยอมรับความเสี่ยงที่เหลือไปก่อน
/// — **บันทึกไว้ว่าเป็นช่องที่รู้ตัว ไม่ใช่ช่องที่ลืม**
///
/// ล้มแล้ว **ไม่คืน error** โดยตั้งใจ: ถึงจุดนี้ไฟล์ใหม่อยู่ในตำแหน่งที่ถูกแล้ว
/// การบอกผู้ใช้ว่า "บันทึกไม่สำเร็จ" ทั้งที่งานอยู่ครบจะทำให้เขากดบันทึกซ้ำ
/// หรือแย่กว่านั้นคือคิดว่างานหาย
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
    use refx_core::arena::{ArenaKey as _, BoardId};
    use refx_core::board::{BoardParts, Group, Item, ItemKind, ItemParts, TextNote};

    fn board_id() -> BoardId {
        BoardId::from_parts(0, 0)
    }

    /// board ที่แยกออกจากกันได้ด้วยชื่อ — ใช้ดูว่าไฟล์เป็นรุ่นไหน
    fn board_named(name: &str, items: usize) -> Board {
        Board::load(
            board_id(),
            BoardParts {
                name: name.to_owned(),
                groups: vec![Group {
                    name: name.to_owned(),
                    collapsed: false,
                }],
                items: (0..items)
                    .map(|i| ItemParts {
                        item: Item::new(ItemKind::Text(TextNote {
                            text: format!("{name}-{i}"),
                        })),
                        group: Some(0),
                    })
                    .collect(),
                ..BoardParts::default()
            },
        )
    }

    /// อ่านไฟล์ทั้งก้อนในเทสต์ — `std::fs::read` ถูกแบนใน `clippy.toml`
    /// (เหตุผลของมันคือเส้นทางจริงต้องมีเพดานขนาด · ในเทสต์เราคุมไฟล์เองอยู่แล้ว)
    fn read_all(path: impl AsRef<Path>) -> std::io::Result<Vec<u8>> {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-save-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ---------- พื้นฐาน ----------

    /// บันทึกแล้วอ่านกลับได้เท่าเดิม และ **ไม่ทิ้งไฟล์ชั่วคราวไว้**
    #[test]
    fn saving_then_reading_back_gives_the_same_board() {
        let dir = temp_dir("basic");
        let doc = dir.join("work.refx");
        let board = board_named("first", 3);

        save_atomic(&doc, &board).unwrap();

        let back = dto::decode(&read_all(&doc).unwrap(), board_id()).unwrap();
        assert_eq!(back, board);
        assert!(
            !tmp_path(&doc).exists(),
            "ไฟล์ชั่วคราวต้องไม่ค้างอยู่ — ผู้ใช้จะเห็นขยะในโฟลเดอร์งานของตัวเอง"
        );
        // บันทึกครั้งแรกยังไม่มีของเดิมให้สำรอง
        assert!(!backup_path(&doc).exists());
    }

    /// ★ บันทึกทับ → `.refx.bak` ต้องเป็น **รุ่นก่อนหน้า** ไม่ใช่รุ่นปัจจุบัน
    ///
    /// `CLAUDE.md` ยกตัวอย่างข้อความ error ที่ชี้ผู้ใช้ไปหา `reference.refx.bak`
    /// — ไฟล์นั้นจะมีประโยชน์ก็ต่อเมื่อมันคือ *ของเก่า* จริง ๆ
    #[test]
    fn overwriting_leaves_the_previous_version_in_the_backup() {
        let dir = temp_dir("backup");
        let doc = dir.join("work.refx");

        let old = board_named("old", 2);
        let new = board_named("new", 5);
        save_atomic(&doc, &old).unwrap();
        save_atomic(&doc, &new).unwrap();

        let current = dto::decode(&read_all(&doc).unwrap(), board_id()).unwrap();
        let backup = dto::decode(&read_all(backup_path(&doc)).unwrap(), board_id()).unwrap();
        assert_eq!(current, new, "ไฟล์หลักต้องเป็นรุ่นล่าสุด");
        assert_eq!(backup, old, "ไฟล์สำรองต้องเป็นรุ่นก่อนหน้า");
        assert!(!doc.with_extension(BAK_TMP_SUFFIX).exists());
    }

    // ---------- ★ เทสต์ที่ค้างมาจาก P4-1 ----------

    /// ★★★ ไฟล์ของรุ่นใหม่กว่า — **ไบต์บนดิสก์ต้องไม่ถูกแตะแม้แต่ไบต์เดียว**
    ///
    /// ที่ชั้น `dto` ข้อนี้จริงโดยโครงสร้าง (ทุกฟังก์ชันรับ `&[u8]`) ซึ่ง
    /// **คนละอย่างกับการมีตาข่าย** — ที่นี่คือชั้นที่เขียนดิสก์จริง จึงเป็นที่ของมัน
    ///
    /// ตรวจครบทั้งสามอย่าง: ไบต์เดิมเท่าเดิมเป๊ะ · ไม่มีไฟล์ชั่วคราวโผล่ ·
    /// ไม่มี `.bak` ถูกสร้าง (ถ้ามี แปลว่าเราเริ่มลงมือไปแล้วก่อนตรวจ)
    #[test]
    fn a_file_from_a_newer_build_is_left_untouched_on_disk() {
        let dir = temp_dir("newer");
        let doc = dir.join("work.refx");

        // ไฟล์ v1 ปกติ แล้วแก้หัวให้เป็นเวอร์ชันที่เราอ่านไม่เป็น
        let mut bytes = dto::encode(&board_named("from the future", 4)).unwrap();
        bytes[4..6].copy_from_slice(&(dto::FORMAT_VERSION + 1).to_le_bytes());
        std::fs::write(&doc, &bytes).unwrap();
        let before = read_all(&doc).unwrap();

        let err = save_atomic(&doc, &board_named("mine", 1)).unwrap_err();
        assert!(
            matches!(err, SaveError::Refused(dto::OpenError::NewerVersion { .. })),
            "ต้องถูกปฏิเสธเพราะเวอร์ชัน ไม่ใช่เหตุอื่น: {err}"
        );

        assert_eq!(
            read_all(&doc).unwrap(),
            before,
            "ไฟล์ของรุ่นใหม่กว่าถูกแตะ — นี่คือการทำลายงานที่รุ่นใหม่เก็บไว้"
        );
        assert!(!tmp_path(&doc).exists(), "ไม่ควรมีไฟล์ชั่วคราวเกิดขึ้นเลย");
        assert!(
            !backup_path(&doc).exists(),
            "มี .bak แปลว่าเราลงมือไปแล้วก่อนจะตรวจเวอร์ชัน"
        );
    }

    /// ★ negative control ของข้อบน — ไฟล์เวอร์ชันที่เราเข้าใจต้องเขียนทับ**ได้**
    ///
    /// ถ้าไม่มีข้อนี้ ประตูที่ห้ามทุกอย่างจะดูเหมือนทำงานถูกต้องสมบูรณ์
    #[test]
    fn a_file_this_build_understands_is_replaced_normally() {
        let dir = temp_dir("replace");
        let doc = dir.join("work.refx");
        save_atomic(&doc, &board_named("old", 1)).unwrap();
        save_atomic(&doc, &board_named("new", 2)).unwrap();
        let back = dto::decode(&read_all(&doc).unwrap(), board_id()).unwrap();
        assert_eq!(back.name(), "new");
    }

    /// ไฟล์ที่ไม่ใช่ `.refx` เลย (ขยะ/ของโปรแกรมอื่น) ทับได้ตามปกติ
    ///
    /// ข้อห้ามสงวนไว้ให้ "งานของ RefX รุ่นใหม่กว่า" เท่านั้น — ไม่งั้นผู้ใช้
    /// Save As ทับไฟล์เก่าของตัวเองไม่ได้เลย · และของเดิมยังถูกสำรองไว้ให้
    #[test]
    fn a_file_that_is_not_refx_can_still_be_replaced() {
        let dir = temp_dir("junk");
        let doc = dir.join("work.refx");
        std::fs::write(&doc, b"this was never a refx file").unwrap();

        save_atomic(&doc, &board_named("mine", 1)).unwrap();

        assert!(dto::decode(&read_all(&doc).unwrap(), board_id()).is_ok());
        assert_eq!(
            read_all(backup_path(&doc)).unwrap(),
            b"this was never a refx file",
            "ของเดิมต้องยังถูกสำรองไว้ ถึงจะอ่านไม่ออกก็ตาม"
        );
    }

    // ---------- ★★★ ฆ่าโปรเซสกลาง save ----------

    /// ชื่อ env ที่บอกให้ process ลูกทำตัวเป็น "เหยื่อ" ที่จะถูกฆ่า
    const VICTIM_ENV: &str = "REFX_SAVE_KILL_TARGET";

    /// ★★★ **เหยื่อ** — บันทึกวนไม่รู้จบจนกว่าจะถูกฆ่า
    ///
    /// เป็น `#[test]` เพราะต้องถูกเรียกผ่าน test binary ตัวเดียวกัน (ไม่ต้องมี
    /// binary พิเศษให้ CI ต้องรู้จัก) · **ไม่มี env = ออกทันที** จึงไม่กระทบ
    /// การรันปกติ แต่ก็ไม่เงียบ: มันพิมพ์บอกว่าตัวเองข้าม
    #[test]
    fn save_victim_helper() {
        let Ok(target) = std::env::var(VICTIM_ENV) else {
            println!("ข้าม: ตัวช่วยของเทสต์ฆ่าโปรเซส (ไม่ได้ตั้ง {VICTIM_ENV})");
            return;
        };
        let doc = PathBuf::from(target);
        let old = board_named("old", VICTIM_ITEMS);
        let new = board_named("new", VICTIM_ITEMS);

        // ★★★ บอกพ่อว่า "พร้อมแล้ว" — **ขาดข้อนี้เทสต์จะไม่ได้ทดสอบอะไรเลย**
        //
        //   การ spawn โปรเซส + สตาร์ต test harness ใช้เวลานานกว่าช่วงหน่วงที่พ่อ
        //   รออยู่มาก · รุ่นแรกจึงถูกฆ่า **ก่อนจะเริ่มเขียนไฟล์ด้วยซ้ำ** แล้ว
        //   เทสต์ก็ "ผ่าน" ทุกรอบโดยไม่เคยแตะเส้นทาง rename เลยสักครั้ง
        //   — ตัวที่จับได้คือ `assert!(survived_as_new > 0)` ท้ายเทสต์ ซึ่งเป็น
        //   เหตุผลที่ต้องมีมัน (docs/08 §3.9 ข้อ 2: เขียวแบบว่างเปล่าห้ามเกิด)
        std::fs::write(ready_marker(&doc), b"ready").expect("เขียนไฟล์สัญญาณไม่ได้");

        // ★ สลับเขียนสองรุ่นไปเรื่อย ๆ จนกว่าจะถูกฆ่า — ทำให้ไฟล์ต้องเป็น
        //   "รุ่นใดรุ่นหนึ่งทั้งใบ" เสมอ ซึ่งเป็นสมบัติที่พ่อตรวจ
        loop {
            // ล้มก็ช่างมัน — หน้าที่ของมันคือ "เขียนไปเรื่อย ๆ จนโดนฆ่า"
            // สิ่งที่ถูกตรวจคือ *ไฟล์บนดิสก์* ไม่ใช่ค่าที่ฟังก์ชันนี้คืน
            let _ = save_atomic(&doc, &new);
            let _ = save_atomic(&doc, &old);
        }
    }

    /// ★ ต้องใหญ่พอให้การเขียนหนึ่งรอบกินเวลาจริง — ไม่งั้นการฆ่าจะตกระหว่างรอบ
    /// แทนที่จะตกกลาง I/O แล้วเทสต์จะไม่เคยเจอสภาพที่มันมีไว้ตรวจ
    const VICTIM_ITEMS: usize = 900;

    /// ไฟล์สัญญาณว่าเหยื่อพร้อมแล้ว
    fn ready_marker(doc: &Path) -> PathBuf {
        doc.with_extension("ready")
    }

    /// ชื่อเต็มของ [`save_victim_helper`] ที่ใช้เรียกผ่าน test binary
    const VICTIM_TEST_PATH: &str = "save::tests::save_victim_helper";

    /// ★ ชื่อใน [`VICTIM_TEST_PATH`] ต้องตรงกับของจริง ไม่งั้นเทสต์ฆ่าโปรเซส
    /// จะ spawn โปรเซสที่ **ไม่รันอะไรเลย** ("running 0 tests") แล้วรอจนหมดเวลา
    ///
    /// ย้ายเทสต์ไปคนละ module เมื่อไหร่ ตัวนี้จะแดงทันที แทนที่จะไปแดงที่
    /// เทสต์ฆ่าโปรเซสด้วยข้อความที่ชี้ไปผิดทาง
    #[test]
    fn the_victim_test_path_matches_the_real_one() {
        assert!(
            VICTIM_TEST_PATH.ends_with("save_victim_helper"),
            "ชื่อ helper เปลี่ยนไปแล้ว"
        );
        assert!(
            VICTIM_TEST_PATH.starts_with("save::tests::"),
            "module path ของ helper เปลี่ยนไปแล้ว — เทสต์ฆ่าโปรเซสจะเรียกไม่เจอ"
        );
    }

    /// ★★★ **เกณฑ์ของ ROADMAP: ฆ่าโปรเซสกลาง save 100 ครั้ง → ไฟล์เดิมไม่เสียสักครั้ง**
    ///
    /// ★★ **สิ่งที่เทสต์นี้พิสูจน์จริง ๆ และสิ่งที่มันพิสูจน์ไม่ได้** — ต้องอ่านให้ครบ
    /// ก่อนอ้างอิงมัน (docs/08 §3.9 ข้อ 1: negative control ที่ไม่แดงมีสองความหมาย)
    ///
    /// | ภัย | ครอบไหม | เพราะ |
    /// |---|---|---|
    /// | **โปรเซสตายกลางเขียน** (crash, End Task, ปิดโปรแกรม) | ✅ **ครอบ** | นี่คือสิ่งที่ `TerminateProcess`/`SIGKILL` จำลองได้ตรง ๆ · ตัวที่ปกป้องคือ **tmp + rename** |
    /// | **ไฟดับ / ถอดปลั๊ก** | ❌ **ไม่ครอบ** | ข้อมูลใน page cache ของ OS ยังถูกเขียนลงดิสก์ต่อแม้โปรเซสตาย · ตัวที่ปกป้องคือ **fsync** ซึ่ง**เทสต์นี้จับไม่ได้โดยธรรมชาติ** ต้องตัดไฟจริง หรือใช้ตัวฉีดความผิดพลาดระดับ block device (เช่น `dm-flakey` บน Linux) |
    ///
    /// ★★★ **negative control วัดจริงแล้ว ไม่ใช่การคาดเดา** (13 ส.ค. 2026):
    ///
    /// | ถอดอะไรออก | ผล |
    /// |---|---|
    /// | **tmp + rename** (เขียนทับไฟล์เดิมตรง ๆ) | **แดงทันที** — นี่คือสิ่งที่เทสต์นี้ปกป้องจริง |
    /// | **`sync_all()`** (เหลือ tmp+rename ครบ) | **ยังเขียว** — 100 ครั้ง เสียหาย 0 |
    ///
    /// ตัวหลังคือ *"ดีไซน์ทำให้พังแบบนั้นไม่ได้"* ไม่ใช่ *"เทสต์อ่อน"*
    /// (docs/08 §3.9 ข้อ 1 แยกสองความหมายนี้ไว้) — การฆ่าโปรเซสไม่ทำให้ข้อมูล
    /// ที่ OS รับไปแล้วหาย จึงไม่มีทางที่มันจะเห็นความต่างของการมี/ไม่มี fsync
    ///
    /// → **ห้ามอ้างว่าเทสต์นี้พิสูจน์ `fsync`** · `fsync` อยู่ในโค้ดเพราะ
    /// `docs/07 §4` กำหนดจาก threat model ที่เป็นไฟดับ **ไม่ใช่เพราะมีเทสต์คุม**
    /// — และตอนนี้มีคนเซ็นรับรองว่ารู้ แทนที่จะเป็นช่องที่ไม่มีใครรู้
    ///
    /// ★ ตัวเลข 100 มาจาก ROADMAP · เวลาที่รอก่อนฆ่าถูกสุ่มให้กระจายทั่วช่วงของ
    /// การเขียนหนึ่งรอบ เพื่อให้การฆ่าตกลงไปตรงกลาง I/O จริง ๆ ไม่ใช่ตกระหว่างรอบ
    #[test]
    fn killing_the_process_mid_save_never_damages_the_document() {
        // ★ นับเป็นเทสต์ที่ช้าโดยธรรมชาติ — spawn 100 โปรเซส
        //   `.config/nextest.toml` ฆ่าที่ 4 นาทีซึ่งเป็นเพดานจริง
        const ROUNDS: usize = 100;

        let dir = temp_dir("kill");
        let doc = dir.join("work.refx");

        // ไฟล์ตั้งต้น = "งานเมื่อวาน" ที่ห้ามเสียไม่ว่าอะไรจะเกิดขึ้น
        let old = board_named("old", VICTIM_ITEMS);
        let new = board_named("new", VICTIM_ITEMS);
        save_atomic(&doc, &old).unwrap();

        let exe = std::env::current_exe().expect("หา test binary ของตัวเองไม่เจอ");
        let marker = ready_marker(&doc);
        let mut survived_as_old = 0usize;
        let mut survived_as_new = 0usize;

        for round in 0..ROUNDS {
            let _ = std::fs::remove_file(&marker);
            let mut child = std::process::Command::new(&exe)
                // ★ ต้องเป็น **ชื่อเต็มพร้อม module path** — `--exact` ไม่จับชื่อสั้น
                //   แล้วจะได้ "running 0 tests" ซึ่งดูเหมือนสำเร็จทุกอย่างจากฝั่งพ่อ
                //   (เจอจริงตอนเขียนเทสต์นี้ — ตัวที่จับได้คือด่าน "เหยื่อไม่เคยพร้อม")
                .args([VICTIM_TEST_PATH, "--exact", "--nocapture"])
                .env(VICTIM_ENV, &doc)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("spawn เหยื่อไม่สำเร็จ");

            // ★★ รอให้เหยื่อ "พร้อม" ก่อน — การ spawn + สตาร์ต test harness ใช้เวลา
            //    นานกว่าช่วงหน่วงข้างล่างมาก ถ้าไม่รอ ทุกรอบจะฆ่าก่อนเหยื่อเริ่ม
            //    เขียนด้วยซ้ำ (เกิดขึ้นจริงตอนเขียนเทสต์นี้รอบแรก)
            //
            //    ★ เป็น **ตาข่ายจับค้าง** ไม่ใช่การวัดความเร็ว จึงตั้งหลวม ๆ
            //      ตาม docs/08 §3.9 ข้อ 5b — เพดานจริงคือ nextest ที่ฆ่าที่ 4 นาที
            let ready_by = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !marker.exists() && std::time::Instant::now() < ready_by {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(
                marker.exists(),
                "รอบ {round}: เหยื่อไม่เคยพร้อม — เทสต์นี้จะไม่ได้ตรวจอะไรเลย"
            );

            // ★ กระจายเวลาให้ทั่วช่วงของการเขียนหนึ่งรอบ — ฆ่าเวลาเดิมทุกครั้ง
            //   จะตกจุดเดิมทุกครั้ง แล้วเราจะทดสอบแค่จังหวะเดียวโดยไม่รู้ตัว
            let delay = (round as u64 * 7) % 37;
            std::thread::sleep(std::time::Duration::from_millis(delay));
            let _ = child.kill();
            let _ = child.wait();

            // ---- ไฟล์ต้องอ่านได้ และเป็น "รุ่นเก่าทั้งใบ" หรือ "รุ่นใหม่ทั้งใบ" ----
            let bytes = read_all(&doc).unwrap_or_else(|err| {
                panic!("รอบ {round}: ไฟล์เอกสารหายไปจากดิสก์ ({err}) — งานของผู้ใช้หาย")
            });
            let board = dto::decode(&bytes, board_id()).unwrap_or_else(|err| {
                panic!(
                    "รอบ {round}: ไฟล์เสียหาย ({err}) — ถูกจับได้ในสภาพเขียนค้างครึ่งทาง \
                     ({} ไบต์)",
                    bytes.len()
                )
            });
            if board == old {
                survived_as_old += 1;
            } else if board == new {
                survived_as_new += 1;
            } else {
                panic!("รอบ {round}: ไฟล์อ่านได้แต่เนื้อหาไม่ใช่ทั้งรุ่นเก่าและรุ่นใหม่");
            }
        }

        println!(
            "ฆ่าโปรเซส {ROUNDS} ครั้ง — ไฟล์เป็นรุ่นเก่า {survived_as_old} ครั้ง · \
             รุ่นใหม่ {survived_as_new} ครั้ง · เสียหาย 0 ครั้ง"
        );
        assert_eq!(survived_as_old + survived_as_new, ROUNDS);

        // ★★ **ต้องเคยถูกฆ่าทั้งก่อนและหลังการสลับไฟล์จริง** ไม่งั้นเทสต์อาจ
        //    ฆ่าเร็วเกินไปทุกครั้งจนไม่เคยแตะเส้นทาง rename เลย แล้วเขียวโดย
        //    ไม่ได้ตรวจอะไร (รูปแบบเดียวกับ fuzz target ที่ตายตั้งแต่ด่านแรก)
        assert!(
            survived_as_new > 0,
            "ไม่มีรอบไหนที่บันทึกสำเร็จเลย — การฆ่าเร็วเกินไปจนไม่เคยถึงขั้น rename"
        );
    }
}
