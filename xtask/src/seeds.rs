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

/// เขียนไฟล์ที่มีเนื้อไม่ซ้ำกันจริง (บีบไม่ลงง่าย ๆ) แล้วคืน path
fn plant(dir: &Path, name: &str, bytes: usize) -> anyhow::Result<PathBuf> {
    let path = dir.join(name);
    let body: Vec<u8> = (0..bytes)
        .map(|i| (i.wrapping_mul(31).wrapping_add(7) % 251) as u8)
        .collect();
    write(&path, &body)?;
    Ok(path)
}

fn source(n: u8, path: &Path) -> PackSource {
    PackSource {
        hash: ContentHash::from_bytes([n; 32]),
        path: path.to_path_buf(),
    }
}

/// board ที่มีภาพตาม hash ที่ระบุ **บวกโน้ตข้อความหนึ่งใบ**
///
/// ★ `added_at` ถูกตั้งเป็น 0 เพื่อให้ seed ออกมา **ไบต์เท่าเดิมทุกครั้งที่รัน**
/// — ไม่งั้นการรันซ้ำจะสร้าง diff ปลอมใน git ทุกครั้ง และ corpus ที่ต่างกัน
/// ทุกรอบทำให้เทียบผลของสอง session ไม่ได้
fn board(images: &[(u8, &Path)]) -> Board {
    let mut items: Vec<ItemParts> = images
        .iter()
        .map(|(n, path)| {
            let mut item = Item::new(ItemKind::Image(AssetRef {
                hash: ContentHash::from_bytes([*n; 32]),
                path: path.to_path_buf(),
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
}
