#![no_main]
//! ยิงตัวอ่าน **asset table** ของไฟล์ `.refx` แบบ packed (P4-9)
//!
//! ★★★ **ทำไม target นี้ถึงมาแทน `fuzz_journal`** (ROADMAP P4-9 · แก้ 27 ส.ค. 2026)
//!
//! `refx_io::journal` จะไม่มีวันถูกเขียน — P4-3 เปลี่ยนจาก command journal
//! เป็น snapshot ไปแล้ว (`docs/07 §4`) · ส่วน `packed` คือ **parser ไบนารีตัว
//! เดียวที่เหลือซึ่งยังไม่มี fuzz แตะเลยสักไบต์** ทั้งที่มันคือตัวที่อ่าน
//! `count` / `offset` / `len` **จากไฟล์** แล้ว seek และจองตามตัวเลขเหล่านั้น
//!
//! ★★ ช่องนี้ใหญ่กว่าที่คิดเพราะ `fuzz_document::wrap()` เขียน `flags = 0` เสมอ
//! → **ไม่มี input ไหนเคยตั้งบิต packed เลย** และ `dto::decode` ก็ตัดที่ `doc_len`
//! อยู่แล้วจึงไม่เคยเดินเข้ามาถึงตาราง · ตอนนี้ `xtask dump-refx` (P4-8) ขับ
//! เส้นทางนี้ด้วย เครื่องมือที่มีไว้ debug ไฟล์ผู้ใช้จึงยิ่งต้องไม่ตายกับไฟล์ที่พัง
//!
//! ## สัญญาที่ target นี้บังคับ — **assert ด้วยค่าคงของสัญญา ห้าม hard-code**
//!
//! `HANDOFF §4` ข้อ 21: `fuzz_layout` เคย assert `size > 0.0` แทนสัญญาจริง
//! `size >= MIN_SIDE` แล้ว **157,000 รอบจับการถอดด่านสุดท้ายออกไม่ได้**
//! — assertion ที่อ่อนกว่าสัญญาคือ target ที่เขียวโดยไม่ได้ตรวจอะไร
//!
//! | สัญญา | assert ด้วย |
//! |---|---|
//! | ตารางไม่เกินเพดาน | [`packed::MAX_ASSETS`] |
//! | blob แต่ละก้อนไม่เกินเพดาน | [`packed::MAX_ASSET_BYTES`] |
//! | ทุก entry อยู่ในไฟล์จริง **และไม่ทับหัวไฟล์/ตาราง** | `HEADER_LEN` + `doc_len` + `TABLE_HEADER_LEN` + `ENTRY_LEN` |
//! | `extract` เขียนไม่เกินที่ตารางประกาศ | `entry.len` |
//! | ไฟล์ linked ไม่มีตาราง | `Index::is_empty` |
//!
//! ## ★★★ ไบต์ในไฟล์ต้องกำหนด **ที่อยู่ที่เราจะเขียน** ไม่ได้เลย
//!
//! `spool::unpack` เอา blob ออกมาเขียนลงดิสก์ · ชื่อไฟล์มาจาก `entry.hash`
//! ซึ่ง **มาจากไฟล์** จึงเป็นค่าที่ผู้โจมตีคุมได้ทั้ง 32 ไบต์ · วันนี้ปลอดภัย
//! เพราะ [`spool::spool_path`] เรนเดอร์ hash เป็น hex ล้วน — ไม่มีทางมีตัวคั่น
//! path หรือ `..` โผล่มาได้ · **แต่นั่นเป็นคุณสมบัติที่ต้องมีคนคอยรักษา**
//! ถ้าวันหนึ่งมีใครเปลี่ยนไปอ่านชื่อจากไฟล์ มันจะกลายเป็น path traversal ทันที
//! → target นี้ยืนยันทุกรอบว่าที่อยู่ปลายทาง **ยังอยู่ในโฟลเดอร์ที่เราสั่ง**
//!
//! ★ **ไม่เรียก `spool::unpack` จริง** โดยตั้งใจ: มันมี `create_dir_all` +
//! `File::create` + `sync_all` + `rename` ต่อหนึ่งรอบ ซึ่งลด throughput จาก
//! หลักแสนเหลือหลักร้อยรอบ/วินาที — 15 นาทีจะได้ coverage น้อยกว่าเดิมหลายร้อยเท่า
//! บน *parser* ซึ่งเป็นผิวโจมตีจริง · และไบต์ทุกไบต์ที่ `unpack` อ่านเดินผ่าน
//! [`packed::extract`] ซึ่ง target นี้ยิงอยู่แล้ว · ส่วนกฎ "ห้ามเขียนทับไฟล์
//! ที่มีอยู่แล้ว" (`HANDOFF §4` ข้อ 31) มีเทสต์ของตัวเองใน `spool.rs`
//!
//! spec: ROADMAP P4-9, docs/07 §1, docs/06 §2.5

use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use refx_io::dto;
use refx_io::packed::{self, ENTRY_LEN, MAX_ASSET_BYTES, MAX_ASSETS, TABLE_HEADER_LEN};
use refx_io::spool;

/// ปลายทางที่ **นับอย่างเดียว ไม่เก็บ** — `extract` สตรีมไฟล์ระดับ GB ได้
/// การเก็บไว้ใน `Vec` จะทำให้ target นี้เองกลายเป็นตัวที่ OOM
#[derive(Default)]
struct Counting {
    written: u64,
}

impl Write for Counting {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.written += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// เอกสารเปล่าที่ถูกต้องหนึ่งชุด — คิดครั้งเดียวต่อโปรเซส
///
/// ★ ถ้าคำนวณใหม่ทุกรอบ เวลาส่วนใหญ่ของ 15 นาทีจะหมดไปกับ zstd ไม่ใช่กับ parser
fn empty_document() -> &'static [u8] {
    static BODY: OnceLock<Vec<u8>> = OnceLock::new();
    BODY.get_or_init(|| {
        dto::encode_body(&refx_core::board::Board::default()).unwrap_or_default()
    })
}

/// ★★ ประกอบไฟล์ packed ที่ **หัวไฟล์และเอกสารถูกต้องทุกช่อง** แล้วเอาไบต์ของ
/// fuzzer ไปวางตรงตำแหน่งของ **asset table** พอดี
///
/// ถ้ายิงแต่ไบต์ดิบ fuzzer จะติดอยู่ที่ด่าน magic + `doc_crc` (สุ่มเจอแทบไม่ได้)
/// แล้วเกือบทุก input จะจบตั้งแต่ 20 ไบต์แรก = target ที่ดูเหมือนทำงานแต่ไม่เคย
/// แตะตารางเลย — รูปแบบเดียวกับ `fuzz_decode` ที่เป็น stub อยู่ 5 session
fn wrap_table(table_and_blobs: &[u8]) -> Vec<u8> {
    let body = empty_document();
    let mut out = Vec::with_capacity(dto::HEADER_LEN + body.len() + table_and_blobs.len());
    out.extend_from_slice(&dto::MAGIC);
    out.extend_from_slice(&packed::PACKED_VERSION.to_le_bytes());
    out.extend_from_slice(&dto::FLAG_PACKED.to_le_bytes());
    out.extend_from_slice(&(body.len() as u64).to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(table_and_blobs);
    out
}

/// อ่านตารางแล้วบังคับสัญญาทุกข้อ — **ห้าม panic ไม่ว่าไบต์จะเป็นอะไร**
fn check(bytes: &[u8]) {
    let file_len = bytes.len() as u64;
    let mut source = Cursor::new(bytes);

    let Ok(index) = packed::read_index(&mut source, file_len) else {
        return; // ปฏิเสธคือคำตอบที่ถูกต้องเสมอ — ที่ห้ามคือ panic
    };

    // ไฟล์ที่ไม่ได้ประกาศ packed ต้องไม่มีตาราง — ไม่งั้นไฟล์ linked ธรรมดา
    // จะพาเราไปอ่านท้ายไฟล์ตามตัวเลขที่ไม่มีใครตั้งใจให้เป็นตาราง
    let Ok(info) = dto::inspect(bytes) else {
        assert!(index.is_empty(), "อ่านตารางได้จากไฟล์ที่หัวไฟล์ใช้ไม่ได้");
        return;
    };
    if !info.packed {
        assert!(index.is_empty(), "ไฟล์ linked ไม่ควรมี entry สักตัว");
        return;
    }

    assert!(
        index.len() <= MAX_ASSETS as usize,
        "ตารางมี {} entry เกินเพดาน {MAX_ASSETS}",
        index.len()
    );

    // ★ จุดเริ่มของ blob คำนวณจาก **ค่าคงของสัญญาล้วน ๆ** ไม่ใช่เลขที่คัดมาเอง
    //   entry ที่ชี้ก่อนหน้านี้แปลว่ามันชี้ทับหัวไฟล์ เอกสาร หรือตัวตารางเอง
    let blobs_at = (dto::HEADER_LEN as u64)
        .saturating_add(info.doc_len)
        .saturating_add(TABLE_HEADER_LEN as u64)
        .saturating_add((index.len() as u64).saturating_mul(ENTRY_LEN as u64));

    let suffix = format!(".{}", spool::SPOOL_EXT);
    for entry in index.entries() {
        assert!(
            entry.len <= MAX_ASSET_BYTES,
            "blob ประกาศ {} ไบต์ เกินเพดาน {MAX_ASSET_BYTES}",
            entry.len
        );
        let end = entry
            .offset
            .checked_add(entry.len)
            .expect("offset + len ล้น u64 — ด่านขอบเขตปล่อยผ่าน");
        assert!(
            entry.offset >= blobs_at,
            "entry ชี้ที่ {} ซึ่งอยู่ก่อนบล็อก blob ({blobs_at}) — ทับหัวไฟล์หรือตาราง",
            entry.offset
        );
        assert!(
            end <= file_len,
            "entry กิน {}..{end} แต่ไฟล์ยาว {file_len} — ชี้ออกนอกไฟล์",
            entry.offset
        );

        // ★★★ ที่อยู่ที่ `spool::unpack` จะเขียน **ต้องไม่ถูกไบต์ในไฟล์กำหนด**
        //     `entry.hash` มาจากไฟล์ทั้ง 32 ไบต์ ผู้โจมตีจึงคุมมันได้ทั้งหมด
        let dir = Path::new("spool");
        let target = spool::spool_path(dir, entry.hash);
        assert_eq!(
            target.parent(),
            Some(dir),
            "ชื่อจาก hash พาไฟล์ออกนอกโฟลเดอร์ที่สั่งไว้: {}",
            target.display()
        );
        let name = target
            .file_name()
            .and_then(|name| name.to_str())
            .expect("ชื่อไฟล์ใน spool ต้องเป็น UTF-8 เสมอ");
        // ★ ตัดนามสกุลที่ **เราเป็นคนตั้ง** ออก แล้วที่เหลือต้องเป็น hex 64 ตัวเป๊ะ
        //   — ตรวจแบบนี้ไม่ได้พึ่ง `Display` ของ `ContentHash` ถ้าวันหนึ่งมันเปลี่ยน
        //   ไปพ่นอะไรที่มาจากไฟล์ ข้อนี้จะแดงทันที (เขียนเทียบกับ `{hash}.png`
        //   ตรง ๆ จะกลายเป็นการทดสอบตัวเองแล้วทั้งสองฝั่งเปลี่ยนพร้อมกัน)
        let stem = name
            .strip_suffix(&suffix)
            .expect("ชื่อไฟล์ใน spool ต้องลงท้ายด้วยนามสกุลที่เราตั้งเอง");
        assert_eq!(
            stem.len(),
            64,
            "ชื่อไฟล์ใน spool ไม่ใช่ hash 32 ไบต์: {name:?}"
        );
        assert!(
            stem.chars().all(|ch| ch.is_ascii_hexdigit()),
            "ชื่อไฟล์ใน spool มีอักขระที่ไม่ใช่ hex: {name:?}"
        );

        // `extract` ต้องไม่เขียนเกินที่ตารางประกาศไว้ ไม่ว่าไฟล์จะบอกอะไร
        let mut sink = Counting::default();
        let _ = packed::extract(&mut source, *entry, &mut sink);
        assert!(
            sink.written <= entry.len,
            "extract เขียน {} ไบต์ ทั้งที่ตารางประกาศ {}",
            sink.written,
            entry.len
        );
    }
}

fuzz_target!(|data: &[u8]| {
    // ---- 1. ทั้งไฟล์เป็นของ fuzzer — ยิงด่านหัวไฟล์และตารางพร้อมกัน ----
    check(data);

    // ---- 2. หัวไฟล์+เอกสารถูกต้อง · ไบต์ของ fuzzer = ตาราง + blob ----
    //
    // ทางนี้คือทางที่ทำให้ fuzzer แก้ `count` / `table_crc` / `offset` / `len`
    // ได้จริง โดยไม่ต้องเดา crc ของเอกสารให้ถูกก่อน
    check(&wrap_table(data));
});
