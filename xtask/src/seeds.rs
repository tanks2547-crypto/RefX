//! `cargo xtask gen-fuzz-seeds` — corpus ตั้งต้นของ `fuzz_packed` (P4-9)
//!
//! ## ★★★ ทำไมต้องสร้างด้วย **ตัวเขียนของจริง** ไม่ใช่สคริปต์ที่ประกอบไบต์เอง
//!
//! `fuzz/seeds/make_seeds.py` สร้าง seed ของ `fuzz_decode` ด้วย Python ได้เพราะ
//! PNG/JPEG เป็นรูปแบบของคนอื่น — เราไม่ได้เป็นเจ้าของมัน · แต่ `.refx` เป็น
//! **รูปแบบของเราเอง** การเขียนตัวประกอบไบต์ตัวที่สองขึ้นมาในอีกภาษาหนึ่ง
//! คือการทำสิ่งที่ `docs/08 §3.9` ห้ามไว้ตรง ๆ:
//!
//! > รันของจริงเสมอ — สิ่งที่เขียนเลียนแบบจะสะท้อน *ความเข้าใจของคนเขียน*
//! > ไม่ใช่ *พฤติกรรมของของจริง* และความต่างระหว่างสองอย่างนั้นคือที่ที่บั๊กอยู่พอดี
//!
//! ถ้า seed ถูกประกอบด้วยมือแล้วผิดไปหนึ่งช่อง fuzzer จะเริ่มจากไฟล์ที่ถูก
//! ปฏิเสธตั้งแต่ด่านแรกทุกใบ — **corpus ที่ดูเหมือนมี แต่ไม่เคยพาไปถึง parser**
//! ซึ่งเป็นรูปแบบเดียวกับ target ที่เขียวโดยไม่ได้ยิงอะไร
//!
//! → ที่นี่เรียก [`packed::write_packed`] ตัวเดียวกับที่ `Ctrl+S` ของผู้ใช้เรียก
//!
//! ## ★ seed ต้องมีทั้ง "ไฟล์ที่ดี" และ "ไฟล์ที่ถูกประกอบมาอย่างตั้งใจ"
//!
//! `docs/08 §3.9` ข้อ 1b: ไฟล์ที่บิตพลิกเพราะดิสก์เสีย กับไฟล์ที่ถูกดัดแปลง
//! อย่างตั้งใจเป็นคนละภัย และด่าน checksum จะจับอันแรกได้ก่อนเสมอ —
//! **บังหน้าด่านที่เรากำลังจะทดสอบ** · fuzzer กลายพันธุ์ไบต์แล้วเจอแต่แบบแรก
//! ตัวที่มันแทบไม่มีวันสุ่มเจอเองคือ *ตารางที่ crc ถูกต้องแต่ตัวเลขชี้ออกนอกไฟล์*
//! → ใส่ให้เป็นจุดตั้งต้นเลย

use std::io::Write as _;
use std::path::{Path, PathBuf};

use refx_core::arena::{ArenaKey as _, BoardId};
use refx_core::board::{
    AssetRef, Board, BoardParts, ImageFormat, Item, ItemKind, ItemParts, TextNote,
};
use refx_core::hash::ContentHash;
use refx_io::dto;
use refx_io::packed::{self, ENTRY_LEN, PackSource, TABLE_HEADER_LEN};

/// โฟลเดอร์ปลายทาง — commit ไว้ใน repo (ดู `fuzz/README.md`)
const OUT: &str = "fuzz/seeds/fuzz_packed";

/// สร้าง corpus ตั้งต้นทั้งชุด
///
/// # Errors
/// เมื่อเขียนไฟล์ไม่สำเร็จ
pub fn gen_fuzz_seeds() -> anyhow::Result<()> {
    crate::args::Args::new("cargo xtask gen-fuzz-seeds").finish()?;
    let out = PathBuf::from(OUT);
    std::fs::create_dir_all(&out)?;
    let work = std::env::temp_dir().join(format!("refx-seed-blobs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;

    let small = plant(&work, "small.bin", 512)?;
    let big = plant(&work, "big.bin", 70_000)?; // ใหญ่กว่าบัฟเฟอร์ 64 KB ของ `extract`
    let tiny = plant(&work, "tiny.bin", 3)?;

    let one = pack(
        &board(&[(1, &small)]),
        &[source(1, &small)],
        &out,
        "packed_one_asset.refx",
    )?;
    let three = pack(
        &board(&[(1, &small), (2, &big), (3, &tiny)]),
        &[source(1, &small), source(2, &big), source(3, &tiny)],
        &out,
        "packed_three_assets.refx",
    )?;
    pack(&board(&[]), &[], &out, "packed_empty_table.refx")?;

    // ★ ไฟล์ linked — ทางที่ `read_index` ต้องคืน "ตารางว่าง" ไม่ใช่ error
    write(
        &out.join("linked_v1.refx"),
        &dto::encode(&board(&[(1, &small)]))?,
    )?;

    // ★ ถูกตัดกลาง blob — หัวไฟล์และตารางยังถูกต้องทุกช่อง แต่ไบต์ไม่ครบ
    write(
        &out.join("packed_truncated.refx"),
        &three[..three.len() / 2],
    )?;

    // ★★★ ตัวที่ fuzzer แทบไม่มีวันสุ่มเจอเอง: **crc ถูกต้อง แต่ตัวเลขชี้ออกนอกไฟล์**
    write(
        &out.join("packed_offset_past_eof.refx"),
        &crafted_out_of_range(&one),
    )?;

    let _ = std::fs::remove_dir_all(&work);
    println!("เขียน seed ลง {OUT} แล้ว");
    Ok(())
}

/// ไฟล์ตัวอย่างหนึ่งก้อน — **ที่อยู่จริงบนดิสก์ กับที่อยู่ที่ถูกบันทึกลงเอกสาร
/// เป็นคนละอย่างกันโดยตั้งใจ**
///
/// # ★★★ ทำไมต้องแยก (แก้ 20 ก.ย. 2026)
///
/// เดิม `plant()` คืน path จริงตัวเดียว แล้วมันถูกบันทึกลง `AssetRef.path`
/// ตรง ๆ · ผลคือ seed ที่ commit ไว้มีข้อความ
/// **`C:\Users\<ชื่อผู้ใช้>\AppData\Local\Temp\refx-seed-blobs-<pid>`** ฝังอยู่ข้างใน
///
/// เสียสองชั้น:
///
/// 1. **ชื่อผู้ใช้ของเครื่องที่สร้าง หลุดไปอยู่ในไฟล์ที่ commit**
/// 2. **seed สร้างซ้ำข้ามเครื่องไม่ได้** — คนละเครื่องได้ไบต์คนละชุด
///    ทั้งที่ input เหมือนกันทุกอย่าง · `docs/08 §3.9` ข้อ 13
///
/// `real` ใช้อ่านไฟล์ตอนแพ็ก (ไม่ถูกบันทึกลงไฟล์เลย — `PackSource.path`
/// ถูกใช้โดย `copy_asset` อย่างเดียว) · `logical` คือสิ่งที่ถูกบันทึก
struct Blob {
    /// ที่อยู่จริงบนดิสก์ชั่วคราว — ต่างกันทุกเครื่อง และ **ห้ามหลุดลงไฟล์**
    real: PathBuf,
    /// ที่อยู่ที่ถูกบันทึกลงเอกสาร — คงที่ ไม่ขึ้นกับเครื่องที่สร้าง
    logical: PathBuf,
}

/// เขียนไฟล์ที่มีเนื้อไม่ซ้ำกันจริง (บีบไม่ลงง่าย ๆ) แล้วคืนที่อยู่ทั้งสองแบบ
fn plant(dir: &Path, name: &str, bytes: usize) -> anyhow::Result<Blob> {
    let real = dir.join(name);
    let body: Vec<u8> = (0..bytes)
        .map(|i| (i.wrapping_mul(31).wrapping_add(7) % 251) as u8)
        .collect();
    write(&real, &body)?;
    Ok(Blob {
        real,
        // ★ เครื่องหมาย `/` ตรง ๆ ไม่ใช่ `Path::join` — ไม่งั้น Windows จะได้
        //   `seed-blobs\small.bin` ส่วน Linux ได้ `seed-blobs/small.bin`
        //   แล้วไบต์ก็ยังต่างกันข้ามเครื่องอยู่ดี แค่เนียนกว่าเดิม
        logical: PathBuf::from(format!("seed-blobs/{name}")),
    })
}

fn source(n: u8, blob: &Blob) -> PackSource {
    PackSource {
        hash: ContentHash::from_bytes([n; 32]),
        path: blob.real.clone(),
    }
}

/// board ที่มีภาพตาม hash ที่ระบุ **บวกโน้ตข้อความหนึ่งใบ**
///
/// ★ `added_at` ถูกตั้งเป็น 0 เพื่อให้ seed ออกมา **ไบต์เท่าเดิมทุกครั้งที่รัน**
/// — ไม่งั้นการรันซ้ำจะสร้าง diff ปลอมใน git ทุกครั้ง และ corpus ที่ต่างกัน
/// ทุกรอบทำให้เทียบผลของสอง session ไม่ได้
fn board(images: &[(u8, &Blob)]) -> Board {
    let mut items: Vec<ItemParts> = images
        .iter()
        .map(|(n, blob)| {
            let mut item = Item::new(ItemKind::Image(AssetRef {
                hash: ContentHash::from_bytes([*n; 32]),
                // ★ ที่อยู่เชิงตรรกะเท่านั้น — ดู `Blob` ว่าทำไม
                path: blob.logical.clone(),
                px_size: glam::UVec2::new(64, 64),
                format: ImageFormat::Unknown,
                embedded: false,
                mtime: 0,
                file_size: 0,
            }));
            item.meta.added_at = 0;
            ItemParts { item, group: None }
        })
        .collect();
    let mut note = Item::new(ItemKind::Text(TextNote {
        text: "seed".to_owned(),
    }));
    note.meta.added_at = 0;
    items.push(ItemParts {
        item: note,
        group: None,
    });

    Board::load(
        BoardId::from_parts(0, 0),
        BoardParts {
            name: "seed".to_owned(),
            items,
            ..BoardParts::default()
        },
    )
}

/// เขียนไฟล์ packed ด้วย **ตัวเขียนของจริง** แล้วคืนไบต์ที่ได้
fn pack(board: &Board, sources: &[PackSource], out: &Path, name: &str) -> anyhow::Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    packed::write_packed(&mut cursor, board, sources)?;
    let bytes = cursor.into_inner();
    write(&out.join(name), &bytes)?;
    Ok(bytes)
}

/// ★★★ ไฟล์ที่ **ถูกต้องทุกอย่างยกเว้นเจตนา** — entry ชี้เลยท้ายไฟล์ โดยที่
/// `table_crc` ถูกคำนวณใหม่ให้ตรง
///
/// ถ้าไม่คำนวณ crc ใหม่ ไฟล์จะตกที่ด่าน checksum **ก่อน** ถึงด่านขอบเขต
/// แล้ว seed นี้จะไม่เคยพา fuzzer ไปถึงสิ่งที่มันมีไว้ทดสอบเลย
/// (`docs/08 §3.9` ข้อ 1b — เคสจริงที่เคยพลาดมาแล้วตอน P4-5)
fn crafted_out_of_range(good: &[u8]) -> Vec<u8> {
    let mut bytes = good.to_vec();
    let Ok(info) = dto::inspect(&bytes) else {
        return bytes;
    };
    let table_at = dto::HEADER_LEN + info.doc_len as usize;
    let entry_at = table_at + TABLE_HEADER_LEN;
    if bytes.len() < entry_at + ENTRY_LEN {
        return bytes;
    }
    // offset ที่ใหญ่กว่าไฟล์มาก + len ที่ยังอยู่ใต้เพดาน → ต้องตกที่ด่านขอบเขต
    bytes[entry_at + 32..entry_at + 40].copy_from_slice(&(u64::MAX / 2).to_le_bytes());
    let table_end = entry_at + ENTRY_LEN;
    let fixed = crc32fast::hash(&bytes[entry_at..table_end]);
    bytes[table_at + 4..table_at + 8].copy_from_slice(&fixed.to_le_bytes());
    bytes
}

fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(path)
        .map_err(|err| anyhow::anyhow!("เขียน {} ไม่ได้: {err}", path.display()))?;
    file.write_all(bytes)?;
    println!("  {:<34} {:>8} ไบต์", display_name(path), bytes.len());
    Ok(())
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    // I-2 คุมเธรดของแอป · เทสต์ของ xtask ไม่มี UI thread และงานของมันคือ
    // การอ่านไฟล์ seed ที่ commit ไว้ในทรีเพื่อเทียบไบต์
    #![expect(
        clippy::disallowed_methods,
        reason = "เทสต์ของ xtask — ไม่มี UI thread ให้บล็อก"
    )]

    use super::*;

    /// ★★★ **seed ทุกใบต้องเป็นไฟล์ที่ parser ของเราเดินเข้าไปได้จริง**
    ///
    /// seed ที่ถูกปฏิเสธตั้งแต่ด่านแรกคือ corpus ที่ *ดูเหมือนมี* แต่ไม่เคยพา
    /// fuzzer ไปถึงตาราง — เทสต์นี้จึงเช็คว่าไฟล์ที่เราสร้างพาไปถึงจริง
    #[test]
    fn every_generated_seed_reaches_the_table_parser() {
        let dir = std::env::temp_dir().join(format!("refx-seedtest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blob = plant(&dir, "b.bin", 900).unwrap();

        let bytes = pack(&board(&[(1, &blob)]), &[source(1, &blob)], &dir, "one.refx").unwrap();

        // ไฟล์ที่ดี → อ่านตารางได้ครบ และเอกสารข้างในยังอ่านได้
        let mut cursor = std::io::Cursor::new(bytes.clone());
        let index = packed::read_index(&mut cursor, bytes.len() as u64).unwrap();
        assert_eq!(index.len(), 1, "seed ไม่พาไปถึงตาราง");
        assert!(dto::decode(&bytes, BoardId::from_parts(0, 0)).is_ok());

        // ★ ตัวที่ถูกประกอบมา: ต้องผ่าน crc แล้ว **ตกที่ด่านขอบเขต** ไม่ใช่ที่ checksum
        let crafted = crafted_out_of_range(&bytes);
        let mut cursor = std::io::Cursor::new(crafted.clone());
        let err = packed::read_index(&mut cursor, crafted.len() as u64).unwrap_err();
        assert!(
            matches!(err, dto::OpenError::Malformed),
            "seed ที่ประกอบไว้ตกที่ด่านผิดตัว ({err}) — ถ้าตกที่ checksum แปลว่า \
             ไม่เคยพา fuzzer ไปถึงด่านขอบเขตเลย"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ seed ต้องออกมา **ไบต์เท่าเดิมทุกครั้ง** ไม่งั้นรันซ้ำจะได้ diff ปลอม
    ///
    /// ★★ เทสต์นี้ตอบได้แค่ "รันสองครั้งบนเครื่องเดียวกันเหมือนกันไหม" ซึ่ง
    /// **ไม่พอ** — ดู `the_committed_seeds_are_what_this_machine_would_write`
    #[test]
    fn generating_seeds_twice_gives_identical_bytes() {
        let dir = std::env::temp_dir().join(format!("refx-seeddet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blob = plant(&dir, "b.bin", 700).unwrap();

        let a = pack(&board(&[(1, &blob)]), &[source(1, &blob)], &dir, "a.refx").unwrap();
        let b = pack(&board(&[(1, &blob)]), &[source(1, &blob)], &dir, "b.refx").unwrap();
        assert_eq!(a, b, "seed ไม่ deterministic — น่าจะมีนาฬิกาหลุดเข้ามา");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ **ไม่มีที่อยู่ของเครื่องไหนฝังอยู่ใน seed ที่ commit ไว้**
    ///
    /// 20 ก.ย. 2026 พบว่า seed ที่ commit ไว้มี
    /// `C:\Users\<ชื่อผู้ใช้>\AppData\Local\Temp\refx-seed-blobs-<pid>` อยู่ข้างใน
    /// — ชื่อผู้ใช้หลุด **และ** seed สร้างซ้ำข้ามเครื่องไม่ได้
    ///
    /// ★★ เทสต์เดิม (`generating_seeds_twice_...`) มองไม่เห็นเลย เพราะมัน
    /// **เทียบผลของตัวเองกับตัวเองบนเครื่องเดียวกัน** ซึ่งเหมือนกันเสมอ
    /// ไม่ว่าจะมี path ของเครื่องฝังอยู่หรือไม่ (`docs/08 §3.9` ข้อ 14)
    #[test]
    fn no_seed_carries_a_path_from_the_machine_that_made_it() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask อยู่ใต้รากของ workspace")
            .join(OUT);
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("อ่าน {} ไม่ได้: {err}", dir.display()));

        let mut checked = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("refx") {
                continue;
            }
            let bytes = std::fs::read(&path).expect("อ่าน seed ไม่ได้");
            let text = String::from_utf8_lossy(&bytes);
            for mark in [
                "\\Users\\",
                "/home/",
                "/Users/",
                "AppData",
                "Temp\\",
                "/tmp/",
            ] {
                assert!(
                    !text.contains(mark),
                    "{} มี {mark:?} อยู่ข้างใน — ที่อยู่ของเครื่องที่สร้างหลุดลงไฟล์ที่ commit",
                    path.display()
                );
            }
            checked += 1;
        }
        assert!(checked >= 5, "ตรวจ seed ได้แค่ {checked} ไฟล์ — น้อยเกินจะเชื่อ");
    }

    /// ★★★ **seed ที่ commit ไว้ ต้องตรงกับสิ่งที่เครื่องนี้จะเขียนออกมา**
    ///
    /// นี่คือข้อที่ทำให้ "สร้างซ้ำได้ข้ามเครื่อง" เป็นคำที่ตรวจได้ ไม่ใช่ความหวัง —
    /// ถ้าใครเผลอใส่อะไรที่ขึ้นกับเครื่องกลับเข้ามา CI บนอีกแพลตฟอร์มจะแดงทันที
    /// แทนที่จะเงียบจนกว่าจะมีคนไปเปิดไฟล์ดูเอง
    #[test]
    fn the_committed_seeds_are_what_this_machine_would_write() {
        let dir = std::env::temp_dir().join(format!("refx-seedcmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blob = plant(&dir, "small.bin", 512).unwrap();
        let fresh = pack(&board(&[(1, &blob)]), &[source(1, &blob)], &dir, "x.refx").unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        let committed = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask อยู่ใต้รากของ workspace")
            .join(OUT)
            .join("packed_one_asset.refx");
        let want = std::fs::read(&committed)
            .unwrap_or_else(|err| panic!("อ่าน {} ไม่ได้: {err}", committed.display()));

        assert_eq!(
            fresh, want,
            "seed ที่ commit ไว้ไม่ตรงกับที่เครื่องนี้สร้าง — รัน `cargo xtask gen-fuzz-seeds` \
             แล้วดูว่า diff เป็นสิ่งที่ตั้งใจจริงไหม"
        );
    }
}
