//! `cargo xtask gen-fuzz-seeds` — corpus ตั้งต้นของ `fuzz_packed` · `fuzz_document`
//! · `fuzz_layout`
//!
//! # ★★★ ทำไมสามตัว ไม่ใช่ตัวเดียว (21 ก.ย. 2026)
//!
//! ตอนทำ NC ของ fuzz พบว่า **`fuzz_layout` กับ `fuzz_document` ไม่มี seed สักไฟล์**
//! · ขั้น "วาง corpus ตั้งต้น" ใน `fuzz.yml` ปิดท้ายด้วย `|| true` จึงเงียบสนิท
//! → ทั้งสองยิง 900 วินาที/รอบ **จากศูนย์ทุกครั้ง**
//!
//! ★ `docs/08 §3.9` ข้อ 2: โครงเปล่าต้องไม่เงียบ · corpus เปล่าก็เหมือนกัน
//!
//! ## ★★ seed ไม่ได้แก้ปัญหา "ไปไม่ถึง parser" — มันแก้ปัญหาอื่น
//!
//! สมมติฐานแรกคือ *"ไบต์สุ่มตายที่ด่าน magic ตลอดไป"* · **วัดแล้วไม่จริง**
//! (run `35561900295`, ก่อนมี seed): `fuzz_document` เริ่มจากศูนย์ไฟล์แล้วยัง
//! ไปถึง `cov: 1255` และงอก 621 ไฟล์ใน 900 วินาที เพราะทางที่ 3 ของ target
//! บีบ zstd + ประกอบหัวไฟล์ให้เอง
//!
//! สิ่งที่ seed ให้จริง ๆ มีสามอย่าง และวัดได้ทั้งสาม:
//!
//! 1. **จุดตั้งต้นที่ถูกต้องตั้งแต่วินาทีแรก** — ไม่ต้องใช้เวลาไต่ไปหาเอง
//! 2. **เคสขอบที่เรารู้อยู่แล้ว** — `columns: Some(0)`, `NaN`, เอกสารว่าง ·
//!    ของพวกนี้ fuzzer หาเจอเองได้ แต่ไม่มีเหตุผลให้มันต้องเสียเวลาหา
//! 3. **ครบทุกสาขาที่เลือกด้วยไบต์เดียว** — `fuzz_layout` เลือก engine จาก
//!    `data[0] % 5` · seed การันตีว่าทั้งห้าตัวถูกแตะ ไม่ใช่หวังว่าจะสุ่มถูก
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
const OUT_DOC: &str = "fuzz/seeds/fuzz_document";
const OUT_LAYOUT: &str = "fuzz/seeds/fuzz_layout";

/// สร้าง corpus ตั้งต้นทั้งชุด
///
/// # Errors
/// เมื่อเขียนไฟล์ไม่สำเร็จ
pub fn gen_fuzz_seeds() -> anyhow::Result<()> {
    crate::args::Args::new("cargo xtask gen-fuzz-seeds").finish()?;
    gen_packed_seeds()?;
    gen_document_seeds()?;
    gen_layout_seeds()?;
    Ok(())
}

fn gen_packed_seeds() -> anyhow::Result<()> {
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

/// corpus ตั้งต้นของ `fuzz_document`
///
/// # ★★ ทำไมต้องมี **สองชนิด** ในโฟลเดอร์เดียวกัน
///
/// target ยิงสามทางจากไบต์ชุดเดียวกัน (ดูหัวไฟล์ `fuzz_document.rs`):
///
/// | ทาง | สิ่งที่ `data` เป็น | seed ที่พาไปถึง |
/// |---|---|---|
/// | 1 | ไฟล์ `.refx` ทั้งใบ | `doc_*.refx` |
/// | 2 | body ที่ถูก `wrap` ให้ | (ได้จากทั้งสองชนิดโดยปริยาย) |
/// | 3 | **postcard ดิบ** แล้ว target บีบ+ประกอบหัวให้เอง | `postcard_*.bin` |
///
/// ★★★ ทางที่ 3 คือทางที่ลึกที่สุด — มันข้ามทั้งด่าน magic และด่าน zstd
/// ไปโผล่ที่ **โครงของเอกสารโดยตรง** ซึ่งเป็นที่ที่ fuzzer แก้อะไรที่มีความหมายได้
/// · ถ้ามีแต่ไฟล์ทั้งใบเป็น seed การกลายพันธุ์เกือบทุกครั้งจะพัง crc แล้วตาย
/// ตั้งแต่ด่านแรก — *corpus ที่ดูเหมือนมี แต่ไม่เคยพาไปถึง parser*
fn gen_document_seeds() -> anyhow::Result<()> {
    let out = PathBuf::from(OUT_DOC);
    std::fs::create_dir_all(&out)?;
    for (name, bytes) in document_seeds()? {
        write(&out.join(&name), &bytes)?;
    }
    println!("เขียน seed ลง {OUT_DOC} แล้ว");
    Ok(())
}

/// ★ ตัวผลิตล้วน — ไม่แตะดิสก์เลย จึงเป็นตัวเดียวกับที่เทสต์ใช้เทียบกับ
/// ไฟล์ที่ commit ไว้ · ถ้าตัวผลิตกับตัวเขียนเป็นคนละเส้นทาง เทสต์จะยืนยัน
/// ได้แค่ว่า "ตัวเทสต์ตรงกับตัวเทสต์" ซึ่งไม่มีความหมาย
fn document_seeds() -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    // ★ ไม่ต้อง `plant` — เอกสารบันทึกแค่ `logical` ส่วนไฟล์จริงถูกอ่านโดย
    //   `PackSource` เท่านั้น ซึ่ง seed ชุดนี้ไม่ได้ใช้
    let small = logical_only("small.bin");
    let big = logical_only("big.bin");
    let tiny = logical_only("tiny.bin");

    let mut out = Vec::new();
    for (name, board) in [
        ("empty", board(&[])),
        ("one_asset", board(&[(1, &small)])),
        ("three_assets", board(&[(1, &small), (2, &big), (3, &tiny)])),
    ] {
        // ทางที่ 1 — ไฟล์ทั้งใบที่ `Ctrl+S` ของผู้ใช้เขียนออกมาจริง ๆ
        out.push((format!("doc_{name}.refx"), dto::encode(&board)?));
        // ทางที่ 3 — postcard ดิบ
        out.push((format!("postcard_{name}.bin"), postcard_of(&board)?));
    }
    Ok(out)
}

/// `Blob` ที่มีแต่ที่อยู่เชิงตรรกะ — ใช้กับ seed ที่ไม่ต้องอ่านไฟล์จริง
fn logical_only(name: &str) -> Blob {
    Blob {
        real: PathBuf::new(),
        logical: PathBuf::from(format!("seed-blobs/{name}")),
    }
}

/// postcard ดิบของเอกสาร — **คลายกลับด้วยตัวเดียวกับที่ `encode_body` ใช้บีบ**
///
/// ★ ไม่เรียก `postcard::to_stdvec` เอง · ถ้าวันหนึ่งรูปแบบของเอกสารเปลี่ยน
/// (v1 → v2, เปลี่ยน serializer) seed จะเปลี่ยนตามโดยอัตโนมัติ · ตัวประกอบ
/// ตัวที่สองจะ drift เงียบ ๆ แล้ว seed จะกลายเป็นของที่ parser ปฏิเสธทุกใบ
fn postcard_of(board: &Board) -> anyhow::Result<Vec<u8>> {
    let squeezed = dto::encode_body(board)?;
    Ok(zstd::decode_all(squeezed.as_slice())?)
}

/// corpus ตั้งต้นของ `fuzz_layout`
///
/// # ★★★ ที่นี่ "ของจริง" คืออะไร
///
/// สองตัวบนมีรูปแบบไฟล์ของตัวเอง จึงเรียกตัวเขียนจริงได้ · **`fuzz_layout`
/// ไม่มีรูปแบบไฟล์** — สิ่งที่ target อ่านคือการตีความไบต์ที่มันนิยามขึ้นเอง
/// ในตัวมัน (byte 0 = engine, แล้ว f32 สามตัว, แล้ว 2 ไบต์ของ `columns`,
/// แล้วคู่ f32 ไปเรื่อย ๆ)
///
/// → ตัวเขียนที่นี่จึงเป็น **ตัวประกอบตัวที่สองโดยเลี่ยงไม่ได้** ซึ่งคือสิ่งที่
///   หัวไฟล์นี้เตือนไว้เอง · ทางแก้ไม่ใช่การแกล้งว่าไม่มีปัญหา แต่คือ
///   `the_layout_target_still_reads_bytes_the_way_these_seeds_write_them`
///   ที่ **อ่าน `fuzz_layout.rs` ตัวจริง** แล้วล้มถ้าลำดับช่องเปลี่ยน
///   (รูปเดียวกับ `xtask/tests/ui_drive_step_lists.rs`)
fn gen_layout_seeds() -> anyhow::Result<()> {
    let out = PathBuf::from(OUT_LAYOUT);
    std::fs::create_dir_all(&out)?;
    for (name, bytes) in layout_seeds() {
        write(&out.join(&name), &bytes)?;
    }
    println!("เขียน seed ลง {OUT_LAYOUT} แล้ว");
    Ok(())
}

/// หนึ่ง seed ของ `fuzz_layout` — ช่องเรียงตามลำดับที่ target อ่านไบต์พอดี
struct LayoutCase {
    name: &'static str,
    /// ไบต์ 0 · target ทำ `% 5` เอง
    engine: u8,
    width: f32,
    gap: f32,
    row_height: f32,
    columns: Option<u32>,
    items: usize,
}

/// ★ ตัวผลิตล้วน — เหตุผลเดียวกับ [`document_seeds`]
fn layout_seeds() -> Vec<(String, Vec<u8>)> {
    let case = |name, engine, width, gap, row_height, columns, items| LayoutCase {
        name,
        engine,
        width,
        gap,
        row_height,
        columns,
        items,
    };
    let cases = [
        case("grid_sane", 0, 1280.0, 8.0, 220.0, None, 12),
        case("masonry_sane", 1, 1600.0, 12.0, 240.0, Some(4), 40),
        case("justified_sane", 2, 1024.0, 6.0, 180.0, None, 25),
        case("shelf_sane", 3, 1920.0, 10.0, 200.0, None, 30),
        case("radial_sane", 4, 900.0, 4.0, 160.0, None, 16),
        // ★ ขอบที่เรารู้ว่าอันตราย — ใส่เป็นจุดตั้งต้น ไม่ต้องรอ fuzzer สุ่มเจอ
        case("grid_zero_width", 0, 0.0, 0.0, 0.0, Some(0), 8),
        case("masonry_nan", 1, f32::NAN, f32::NAN, f32::NAN, Some(1), 8),
        case("justified_inf", 2, f32::INFINITY, -1.0, f32::MIN, None, 8),
        case("shelf_one_item", 3, 800.0, 5.0, 150.0, None, 1),
        case("radial_no_items", 4, 800.0, 5.0, 150.0, None, 0),
        case("grid_many_items", 0, 2400.0, 3.0, 120.0, Some(9), 600),
    ];

    let mut out = Vec::new();
    for LayoutCase {
        name,
        engine,
        width,
        gap,
        row_height,
        columns,
        items,
    } in cases
    {
        let mut bytes = vec![engine];
        for value in [width, gap, row_height] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        // `columns`: ไบต์แรกคี่ = Some(ไบต์ที่สอง) · คู่ = None
        match columns {
            None => bytes.extend_from_slice(&[0, 0]),
            Some(n) => bytes.extend_from_slice(&[1, u8::try_from(n).unwrap_or(u8::MAX)]),
        }
        // คู่ f32 = aspect ของแต่ละภาพ · ไล่ค่าให้ต่างกันจริงเพื่อให้แต่ละ engine
        // เดินคนละเส้น ไม่ใช่ภาพจัตุรัสเหมือนกันหมดซึ่งเป็นเคสง่ายที่สุด
        for i in 0..items {
            #[expect(
                clippy::cast_precision_loss,
                reason = "ค่า aspect ของ seed — ความละเอียดระดับ f32 พอเกินพอ"
            )]
            let t = i as f32;
            let w = 40.0 + (t * 37.0) % 610.0;
            let h = 30.0 + (t * 53.0) % 430.0;
            bytes.extend_from_slice(&w.to_le_bytes());
            bytes.extend_from_slice(&h.to_le_bytes());
        }
        out.push((format!("layout_{name}.bin"), bytes));
    }
    out
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
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask อยู่ใต้รากของ workspace");
        // ★ ทั้งสามโฟลเดอร์ ไม่ใช่แค่ `fuzz_packed` · โฟลเดอร์ที่ไม่ถูกตรวจ
        //   คือโฟลเดอร์ที่ path หลุดลงไปได้โดยไม่มีใครรู้
        let entries: Vec<_> = [OUT, OUT_DOC, OUT_LAYOUT]
            .iter()
            .flat_map(|dir| {
                let dir = root.join(dir);
                std::fs::read_dir(&dir)
                    .unwrap_or_else(|err| panic!("อ่าน {} ไม่ได้: {err}", dir.display()))
                    .flatten()
                    .collect::<Vec<_>>()
            })
            .collect();

        let mut checked = 0;
        for entry in entries {
            let path = entry.path();
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
        assert!(checked >= 20, "ตรวจ seed ได้แค่ {checked} ไฟล์ — น้อยเกินจะเชื่อ");
    }

    /// ★★★ **ไม่มี target ไหนที่ commit target ไว้แต่ไม่ commit seed ไว้**
    ///
    /// นี่คือข้อที่ถ้ามีมาตั้งแต่แรก จะจับได้ตั้งแต่วันที่ `fuzz_layout` กับ
    /// `fuzz_document` ถูกต่อ — แทนที่จะต้องรอให้มีคนไปทำ NC ของ fuzz
    /// แล้วบังเอิญเห็น `|| true` (21 ก.ย. 2026)
    #[test]
    fn every_wired_target_has_a_seed_folder_with_files_in_it() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask อยู่ใต้รากของ workspace");
        let targets = std::fs::read_dir(root.join("fuzz/fuzz_targets"))
            .expect("อ่านโฟลเดอร์ fuzz_targets ไม่ได้")
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                (p.extension().and_then(|x| x.to_str()) == Some("rs"))
                    .then(|| p.file_stem()?.to_str().map(str::to_owned))?
            })
            .collect::<Vec<_>>();
        assert!(targets.len() >= 4, "เจอ target แค่ {} ตัว", targets.len());

        let mut starving = Vec::new();
        for target in &targets {
            let dir = root.join("fuzz/seeds").join(target);
            let count = std::fs::read_dir(&dir)
                .map(|it| it.flatten().count())
                .unwrap_or(0);
            if count == 0 {
                starving.push(target.clone());
            }
        }
        assert!(
            starving.is_empty(),
            "target ที่ไม่มี seed สักไฟล์: {starving:?} — มันจะยิง 900 วินาทีจากศูนย์ \
             ทุกรอบ · เพิ่มตัวสร้างใน xtask/src/seeds.rs แล้วรัน \
             `cargo xtask gen-fuzz-seeds`"
        );
    }

    /// ★★★ **seed ของ `fuzz_document` ทุกใบต้องเดินถึง parser จริง**
    ///
    /// นี่คือข้อที่ทำให้ seed ต่างจาก "ไฟล์ที่มีอยู่" · seed ที่ตกด่านแรก
    /// มีค่าเท่ากับไม่มี seed เลย แต่**หน้าตาเหมือนมี** ซึ่งแย่กว่า เพราะ
    /// `fuzz.yml` จะพิมพ์ว่าวาง corpus ตั้งต้นสำเร็จ
    ///
    /// ★ เทสต์เดินทาง **ทางเดียวกับที่ target เดิน** ไม่ใช่ทางที่สะดวกกว่า:
    /// `postcard_*.bin` ถูกบีบก่อนแล้วค่อยส่งให้ `decode_document` เหมือนที่
    /// `fuzz_document.rs` ทางที่ 3 ทำ
    #[test]
    fn every_document_seed_reaches_the_real_parser() {
        let id = BoardId::from_parts(0, 0);
        let mut docs = 0;
        let mut cards = 0;
        for (name, bytes) in document_seeds().expect("สร้าง seed ของเอกสารไม่ได้")
        {
            if name.ends_with(".refx") {
                let got = dto::decode(&bytes, id);
                assert!(
                    got.is_ok(),
                    "{name} เปิดไม่ได้ ({:?}) — seed ที่ตกด่านแรกไม่เคยพา fuzzer ไปถึง parser",
                    got.err()
                );
                docs += 1;
            } else {
                let squeezed = zstd::encode_all(bytes.as_slice(), 3).expect("บีบ seed ไม่ได้");
                let got = dto::decode_document(&squeezed);
                assert!(
                    got.is_ok(),
                    "{name} ไม่ใช่ postcard ที่อ่านได้ ({:?}) — ทางที่ 3 ของ target จะตายทันที",
                    got.err()
                );
                cards += 1;
            }
        }
        assert!(
            docs >= 3 && cards >= 3,
            "ได้ {docs} ไฟล์ .refx กับ {cards} postcard — น้อยเกินจะเชื่อ"
        );
    }

    /// ★★★ **`fuzz_layout.rs` ต้องยังอ่านไบต์ด้วยลำดับที่ seed เขียน**
    ///
    /// ต่างจากอีกสองตัว: `fuzz_layout` ไม่มีรูปแบบไฟล์ของตัวเอง ตัวเขียน seed
    /// จึงเป็น **ตัวประกอบตัวที่สอง** ซึ่งหัวไฟล์นี้เตือนไว้เองว่าเป็นที่ที่บั๊กอยู่
    ///
    /// วันที่ใครสลับ `width` กับ `gap` หรือย้าย `columns` ไปอยู่ข้างหน้า
    /// seed ทุกใบจะยังเป็นไฟล์ที่ "ใช้ได้" แต่จะสื่อความหมายคนละอย่าง —
    /// **ไม่มีอะไรแดง มีแต่ corpus ที่เงียบ ๆ เลิกตรงประเด็น**
    ///
    /// → เทสต์นี้อ่าน target ตัวจริง แล้วยืนยันลำดับที่เราพึ่งพา
    ///   (รูปเดียวกับ `xtask/tests/ui_drive_step_lists.rs`)
    #[test]
    fn the_layout_target_still_reads_bytes_the_way_these_seeds_write_them() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask อยู่ใต้รากของ workspace")
            .join("fuzz/fuzz_targets/fuzz_layout.rs");
        let text = std::fs::read_to_string(&src)
            .unwrap_or_else(|err| panic!("อ่าน {} ไม่ได้: {err}", src.display()));

        // ลำดับที่ตัวเขียน seed พึ่งพา — ต้องปรากฏใน target ตามลำดับนี้เป๊ะ
        let order = [
            "data[at] % 5",                // ไบต์ 0 = engine
            "at += 1",                     //
            "width: take_f32",             // f32 #1
            "gap: take_f32",               // f32 #2
            "target_row_height: take_f32", // f32 #3
            "columns:",                    // 2 ไบต์
            "at += 2",                     //
            "at + 8 <= data.len()",        // จากนี้เป็นคู่ f32
        ];
        let mut from = 0usize;
        for needle in order {
            let found = text[from..].find(needle).unwrap_or_else(|| {
                panic!(
                    "ไม่เจอ {needle:?} ใน fuzz_layout.rs (หลังตำแหน่ง {from}) — \
                     target เปลี่ยนวิธีอ่านไบต์แล้ว แต่ตัวเขียน seed ใน \
                     xtask/src/seeds.rs ยังเขียนแบบเดิม · seed ทั้งชุดกำลังสื่อ \
                     ความหมายคนละอย่างกับที่ตั้งใจ"
                )
            });
            from += found + needle.len();
        }
    }

    /// ★ seed ของ layout ต้องมีครบทั้งห้า engine
    ///
    /// engine ที่ไม่มี seed = engine ที่ fuzzer ต้องสุ่มไบต์แรกให้ตรงเอง
    /// ซึ่งมันทำได้ แต่เสียเวลาไปกับสิ่งที่เรารู้คำตอบอยู่แล้ว
    #[test]
    fn the_layout_seeds_cover_every_engine() {
        let engines: std::collections::BTreeSet<u8> = layout_seeds()
            .iter()
            .filter_map(|(_, bytes)| bytes.first().map(|b| b % 5))
            .collect();
        assert_eq!(
            engines.len(),
            5,
            "seed ครอบ engine ได้แค่ {:?} จากห้าตัว",
            engines
        );
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

    /// ★★★ เหมือนข้างบน แต่ **ทุกไฟล์ ไม่ใช่ตัวแทนใบเดียว**
    ///
    /// สองตัวใหม่ทำได้เพราะตัวผลิตเป็นฟังก์ชันล้วน — เทียบได้ทั้งชุดโดยไม่ต้อง
    /// แตะดิสก์ · `fuzz_packed` ยังเทียบใบเดียวเพราะมันต้องมีไฟล์จริงให้แพ็ก
    #[test]
    fn the_committed_document_and_layout_seeds_are_byte_for_byte_what_we_generate() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask อยู่ใต้รากของ workspace");

        let mut all: Vec<(&str, String, Vec<u8>)> = Vec::new();
        for (name, bytes) in document_seeds().expect("สร้าง seed ของเอกสารไม่ได้")
        {
            all.push((OUT_DOC, name, bytes));
        }
        for (name, bytes) in layout_seeds() {
            all.push((OUT_LAYOUT, name, bytes));
        }

        for (dir, name, fresh) in &all {
            let path = root.join(dir).join(name);
            let want = std::fs::read(&path)
                .unwrap_or_else(|err| panic!("อ่าน {} ไม่ได้: {err}", path.display()));
            assert_eq!(
                fresh,
                &want,
                "{} ไม่ตรงกับที่เครื่องนี้สร้าง — รัน `cargo xtask gen-fuzz-seeds`",
                path.display()
            );
        }

        // ★ ทิศกลับ: ไฟล์ที่อยู่ในโฟลเดอร์แต่ไม่มีใครสร้างแล้ว = ของค้างที่
        //   fuzzer ยังโหลดอยู่ทุกรอบโดยไม่มีใครรู้ว่ามันมาจากไหน
        for dir in [OUT_DOC, OUT_LAYOUT] {
            let on_disk: std::collections::BTreeSet<String> = std::fs::read_dir(root.join(dir))
                .expect("อ่านโฟลเดอร์ seed ไม่ได้")
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(str::to_owned))
                .collect();
            let generated: std::collections::BTreeSet<String> = all
                .iter()
                .filter(|(d, _, _)| *d == dir)
                .map(|(_, n, _)| n.clone())
                .collect();
            let stale: Vec<_> = on_disk.difference(&generated).collect();
            assert!(
                stale.is_empty(),
                "{dir} มีไฟล์ที่ไม่มีตัวสร้างแล้ว: {stale:?} — ลบทิ้งหรือเพิ่มตัวสร้างกลับมา"
            );
        }
    }
}
