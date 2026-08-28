//! ★★★ `cargo xtask dump-refx` — แปลง `.refx` (binary) เป็น JSON เพื่อ debug (P4-8)
//!
//! ## เครื่องมือนี้มีไว้ใช้กับ **ไฟล์ที่พังแล้ว**
//!
//! เกณฑ์ของ ROADMAP คือ *"debug ไฟล์ผู้ใช้ได้จริง"* — และไฟล์ที่ถูกส่งมาให้เรา
//! คือไฟล์ที่ **เปิดไม่ขึ้น** เสมอ ไม่ใช่ไฟล์ที่ดี · ตัวอ่านปกติ ([`dto::decode`])
//! ปฏิเสธไฟล์พวกนั้นตั้งแต่ด่านแรกตามที่มันควรทำ ที่นี่จึงต้องเดินต่อให้ไกลที่สุด
//! เท่าที่ยังพูดความจริงได้ แล้ว **บอกให้ชัดว่าส่วนไหนเชื่อไม่ได้**
//!
//! | สภาพไฟล์ | dump ทำอะไร |
//! |---|---|
//! | `doc_crc` ไม่ตรง | บอกทั้งค่าที่ไฟล์ประกาศและค่าที่คำนวณได้ · **ยังแตกเอกสารออกมาให้ดู** |
//! | ถูกตัดกลางทาง | บอกว่าหัวไฟล์ประกาศกี่ไบต์ มีจริงกี่ไบต์ |
//! | เวอร์ชันใหม่กว่าที่รู้จัก | **บอกเลขทั้งสองฝั่ง** แล้ว *ไม่* แตะเนื้อเอกสาร (เหตุผลข้างล่าง) |
//! | asset table เสีย แต่ document ดี | แยกให้เห็นเป็นคนละสถานการณ์ (`docs/07 §1`) |
//!
//! ## ★★ ทำไมไฟล์เวอร์ชันใหม่กว่า **ไม่ถูกแตกเนื้อออกมา**
//!
//! `postcard` ไม่ self-describing — อ่านเอกสาร v3 ด้วยโครง v1 จะได้ค่าที่
//! *"อ่านผ่าน"* แต่เพี้ยนทั้งก้อนโดยไม่มีอะไรฟ้อง (`docs/07 §3`) · สำหรับ
//! เครื่องมือที่มีไว้ผลิต **หลักฐาน** นั่นแย่กว่าไม่มีข้อมูลเลย เพราะเราจะไล่บั๊ก
//! ตามตัวเลขที่ไม่เคยมีอยู่ในไฟล์ (`docs/08 §3.9` ข้อ 9)
//!
//! ส่วนที่ยังรายงานได้ครบคือ **หัวไฟล์** ซึ่ง `docs/07 §1` บังคับให้มีรูปร่าง
//! เดียวกันทุกเวอร์ชันตลอดไป — และนั่นคือสิ่งที่ต้องรู้เพื่อบอกผู้ใช้ว่า
//! ให้ไปอัปเดตโปรแกรม
//!
//! ## ★★★ ห้ามโหลดทั้งไฟล์เข้า RAM — packed ใหญ่ระดับ GB ได้
//!
//! สิ่งที่ถูกอ่านเข้ามาคือ **หัวไฟล์ (20 B) + เนื้อ document เท่านั้น** ซึ่งมีเพดาน
//! [`dto::MAX_COMPRESSED_BYTES`] คุมอยู่แล้ว · asset blob ไม่เคยถูกอ่าน เว้นแต่
//! สั่ง `--verify-assets` ซึ่งตอนนั้นก็ยังเป็นการสตรีมทีละ 64 KB ลง
//! [`std::io::sink`] ไม่ใช่การเก็บไว้ · เทสต์
//! `a_packed_file_is_never_slurped_into_memory` นับไบต์ที่ถูกอ่านจริงเพื่อยืนยัน
//!
//! ## ★ ค่าที่พ่นออกมาคือ **ค่าดิบในไฟล์** ไม่ใช่ค่าหลังผ่านเกราะ
//!
//! [`dto::decode_document`] ไม่ผ่าน `Board::load` จึงไม่ผ่าน `sanitized()` —
//! `NaN`, opacity 12.0, rating 200 ที่อยู่ในไฟล์จะโผล่ออกมาตามจริง
//! นั่นคือสิ่งที่คนกำลัง debug ต้องเห็น · ค่าของ enum ก็เขียนเป็นเลขดิบคู่กับชื่อ
//! (`"flip": 7, "flip_name": null`) เพื่อให้ค่าที่มาจากรุ่นใหม่กว่า **มองเห็นได้**
//! ไม่ใช่ถูกกลืนเป็นค่าปริยาย
//!
//! spec: ROADMAP P4-8, docs/07 §1 §3

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use refx_core::board::{ColorLabel, Flip, MissingReason};
use refx_core::view::Mode;
use refx_io::dto::{self, OpenError, v1};
use refx_io::packed::{self, Entry, TABLE_HEADER_LEN};

use crate::json::JsonWriter;

/// ตัวเลือกของคำสั่ง
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// อ่าน asset blob ทุกก้อนแล้วตรวจ crc — **แพง** เพราะต้องอ่านทั้งไฟล์
    pub verify_assets: bool,
}

/// อ่านไฟล์ `.refx` แล้วเขียน JSON ออกไปที่ `out`
///
/// # Errors
/// เมื่อเปิดไฟล์ไม่ได้ หรือเขียนผลลัพธ์ไม่ได้ — **ไฟล์ที่ *พัง* ไม่ใช่ error**
/// เพราะการรายงานว่ามันพังยังไงคือหน้าที่ของเครื่องมือนี้
pub fn dump_file(path: &Path, out: &mut impl Write, options: Options) -> anyhow::Result<()> {
    let mut file = std::fs::File::open(path)
        .map_err(|err| anyhow::anyhow!("เปิด {} ไม่ได้: {err}", path.display()))?;
    let file_bytes = file
        .metadata()
        .map_err(|err| anyhow::anyhow!("อ่านขนาดของ {} ไม่ได้: {err}", path.display()))?
        .len();
    dump(
        &mut file,
        file_bytes,
        &path.display().to_string(),
        out,
        options,
    )
}

/// เหมือน [`dump_file`] แต่รับแหล่งข้อมูลมาเอง — ทำให้เทสต์วัดได้ว่า
/// **อ่านไปกี่ไบต์จริง ๆ** (ซึ่งเป็นสัญญาหลักข้อหนึ่งของเครื่องมือนี้)
///
/// # Errors
/// เมื่อเขียนผลลัพธ์ไม่สำเร็จ
pub fn dump<R: Read + Seek, W: Write>(
    src: &mut R,
    file_bytes: u64,
    name: &str,
    out: &mut W,
    options: Options,
) -> anyhow::Result<()> {
    let report = Report::read(src, file_bytes, options);
    report.write(name, file_bytes, out)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// สิ่งที่อ่านได้จากไฟล์ — เก็บ "ข้อเท็จจริง" ไว้ก่อน แล้วค่อยตัดสินว่ามันแปลว่าอะไร
// ---------------------------------------------------------------------------

/// ผลของการอ่านหัวไฟล์
enum Header {
    /// หัวไฟล์อ่านได้
    Ok(dto::FileInfo),
    /// สั้นกว่าหัวไฟล์ — บอกไปว่ามีกี่ไบต์
    TooShort(usize),
    /// ไม่มีลายเซ็น `REFX` — พก 4 ไบต์แรกไปแสดงให้ดู
    NotRefx([u8; 4]),
}

/// ผลของการอ่าน document
struct Document {
    declared_bytes: u64,
    present_bytes: u64,
    declared_crc: u32,
    computed_crc: Option<u32>,
    /// `None` = ยังไม่ได้คำนวณ (ไม่ได้อ่าน)
    crc_ok: Option<bool>,
    /// เหตุที่ **ไม่แตะเนื้อเอกสาร** — `None` = ลองแล้ว
    skipped: Option<&'static str>,
    decoded: Option<v1::DocumentDto>,
    decode_error: Option<String>,
}

/// ผลของการอ่าน asset table
struct Assets {
    packed_flag: bool,
    /// จุดเริ่มของ table ตามที่ `doc_len` บอก — `None` = ตัวเลขล้น
    table_offset: Option<u64>,
    /// ★ อ่านมาจากไฟล์ตรง ๆ เพื่อให้ **รายงานได้แม้ตัว parser จะปฏิเสธ**
    declared_count: Option<u32>,
    declared_table_crc: Option<u32>,
    entries: Vec<Entry>,
    error: Option<String>,
    /// ผลตรวจ blob ทีละก้อน — ยาวเท่า `entries` เมื่อสั่ง `--verify-assets`
    blobs: Vec<&'static str>,
}

/// ทุกอย่างที่รู้เกี่ยวกับไฟล์นี้
struct Report {
    header: Header,
    document: Option<Document>,
    assets: Option<Assets>,
}

impl Report {
    fn read<R: Read + Seek>(src: &mut R, file_bytes: u64, options: Options) -> Self {
        let header = read_header(src, file_bytes);
        let Header::Ok(info) = header else {
            return Self {
                header,
                document: None,
                assets: None,
            };
        };
        let document = read_document(src, file_bytes, info);
        let assets = read_assets(src, file_bytes, info, options);
        Self {
            header,
            document: Some(document),
            assets: Some(assets),
        }
    }
}

fn read_header<R: Read + Seek>(src: &mut R, file_bytes: u64) -> Header {
    let mut bytes = Vec::with_capacity(dto::HEADER_LEN);
    if src.seek(SeekFrom::Start(0)).is_err() {
        return Header::TooShort(0);
    }
    // `take` ทำให้อ่านได้ไม่เกินหัวไฟล์เสมอ ไม่ว่าไฟล์จะใหญ่แค่ไหน
    if src
        .by_ref()
        .take(dto::HEADER_LEN as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Header::TooShort(bytes.len());
    }
    match dto::inspect(&bytes) {
        Ok(info) => Header::Ok(info),
        Err(OpenError::NotRefx) => {
            let mut magic = [0u8; 4];
            magic.copy_from_slice(&bytes[..4]);
            Header::NotRefx(magic)
        }
        // ที่เหลือมีทางเดียวคือสั้นเกิน — `inspect` ไม่คืน error อื่นจากหัวไฟล์
        Err(_) => Header::TooShort(file_bytes.min(bytes.len() as u64) as usize),
    }
}

fn read_document<R: Read + Seek>(src: &mut R, file_bytes: u64, info: dto::FileInfo) -> Document {
    let mut doc = Document {
        declared_bytes: info.doc_len,
        present_bytes: 0,
        declared_crc: info.doc_crc,
        computed_crc: None,
        crc_ok: None,
        skipped: None,
        decoded: None,
        decode_error: None,
    };

    // ★ เพดานเดียวกับที่ `decode` ใช้ — `doc_len` มาจากไฟล์จึงโกหกได้ (I-4)
    if info.doc_len > dto::MAX_COMPRESSED_BYTES {
        doc.skipped = Some("หัวไฟล์ประกาศขนาด document เกินเพดานที่รุ่นนี้ยอมอ่าน");
        return doc;
    }

    let available = file_bytes.saturating_sub(dto::HEADER_LEN as u64);
    let want = info.doc_len.min(available);
    let mut body = Vec::new();
    if src.seek(SeekFrom::Start(dto::HEADER_LEN as u64)).is_err()
        || src.by_ref().take(want).read_to_end(&mut body).is_err()
    {
        doc.skipped = Some("อ่านเนื้อ document จากไฟล์ไม่สำเร็จ");
        return doc;
    }
    doc.present_bytes = body.len() as u64;
    let computed = crc32fast::hash(&body);
    doc.computed_crc = Some(computed);
    doc.crc_ok = Some(doc.present_bytes == doc.declared_bytes && computed == info.doc_crc);

    if !info.writable {
        // ดูหัวโมดูล — เดาโครงของเวอร์ชันที่ไม่รู้จักคือการผลิตหลักฐานปลอม
        doc.skipped = Some("เอกสารมาจาก format เวอร์ชันที่รุ่นนี้ยังไม่รู้จักโครงของมัน");
        return doc;
    }

    match dto::decode_document(&body) {
        Ok(document) => doc.decoded = Some(document),
        Err(err) => doc.decode_error = Some(err.to_string()),
    }
    doc
}

fn read_assets<R: Read + Seek>(
    src: &mut R,
    file_bytes: u64,
    info: dto::FileInfo,
    options: Options,
) -> Assets {
    let mut assets = Assets {
        packed_flag: info.packed,
        table_offset: (dto::HEADER_LEN as u64).checked_add(info.doc_len),
        declared_count: None,
        declared_table_crc: None,
        entries: Vec::new(),
        error: None,
        blobs: Vec::new(),
    };
    if !info.packed {
        return assets; // linked — ไม่มีตารางให้อ่าน ไม่ใช่ความผิดพลาด
    }

    // ★ อ่านหัวตารางเองก่อน **เพื่อให้รายงานตัวเลขที่ไฟล์ประกาศได้ แม้ตัว parser
    //   จะปฏิเสธทั้งตาราง** — คนที่กำลัง debug ต้องเห็นว่าไฟล์ *อ้างว่า* อะไร
    if let Some(offset) = assets.table_offset {
        let mut head = Vec::with_capacity(TABLE_HEADER_LEN);
        if src.seek(SeekFrom::Start(offset)).is_ok()
            && src
                .by_ref()
                .take(TABLE_HEADER_LEN as u64)
                .read_to_end(&mut head)
                .is_ok()
            && head.len() == TABLE_HEADER_LEN
        {
            assets.declared_count = Some(u32::from_le_bytes([head[0], head[1], head[2], head[3]]));
            assets.declared_table_crc =
                Some(u32::from_le_bytes([head[4], head[5], head[6], head[7]]));
        }
    }

    match packed::read_index(src, file_bytes) {
        Ok(index) => assets.entries = index.entries().to_vec(),
        Err(err) => {
            assets.error = Some(table_error(&err));
            return assets;
        }
    }

    if options.verify_assets {
        assets.blobs = assets
            .entries
            .iter()
            .map(|entry| verify_blob(src, *entry))
            .collect();
    }
    assets
}

/// ★★ แปลง [`OpenError`] ให้เป็นข้อความที่พูดถึง **ตาราง** ไม่ใช่ **เอกสาร**
///
/// `OpenError` ถูกใช้ร่วมกันทั้งสองเส้นทาง ข้อความของมันจึงเขียนจากมุมของเอกสาร
/// (`Malformed` → *"document is malformed"*) · พิมพ์ตรง ๆ ตรงนี้จะได้บรรทัดที่
/// อ่านว่า *"asset table อ่านไม่ได้: document is malformed"* ซึ่ง **ขัดกับสิ่งที่
/// เครื่องมือนี้มีไว้แยกพอดี** (`docs/07 §1`: เอกสารเสีย = งานหาย · ตารางเสีย =
/// ขาดแต่ภาพ) — เจอตอนไล่ดู seed ของ `fuzz_packed` ทีละใบ
fn table_error(err: &OpenError) -> String {
    match err {
        OpenError::Corrupt => "checksum ของตารางไม่ตรง (table_crc)".to_owned(),
        OpenError::Malformed => "entry ในตารางชี้นอกขอบเขตไฟล์ หรือหัวตารางอ่านไม่ออก".to_owned(),
        OpenError::Truncated { declared, actual } => {
            format!("ตารางถูกตัด (ต้องยาวถึงไบต์ที่ {declared} แต่ไฟล์มี {actual})")
        }
        OpenError::TooLarge { size } => {
            format!("ตารางประกาศขนาดเกินเพดาน ({size})")
        }
        // ที่เหลือเป็นเรื่องของหัวไฟล์ ซึ่งรายงานไปแล้วในส่วน `header`
        other => other.to_string(),
    }
}

/// สตรีม blob หนึ่งก้อนผ่าน crc โดย **ทิ้งไบต์ทันที** — ไม่มีอะไรค้างใน RAM
fn verify_blob<R: Read + Seek>(src: &mut R, entry: Entry) -> &'static str {
    match packed::extract(src, entry, &mut std::io::sink()) {
        Ok(()) => "ok",
        Err(OpenError::Corrupt) => "crc_mismatch",
        Err(OpenError::Truncated { .. }) => "truncated",
        Err(_) => "unreadable",
    }
}

// ---------------------------------------------------------------------------
// ★ การวินิจฉัย — แปลข้อเท็จจริงเป็นประโยคเดียวที่คนอ่านก่อนอย่างอื่น
// ---------------------------------------------------------------------------

impl Report {
    /// ประโยคเดียวที่บอกว่า *เกิดอะไรขึ้นกับไฟล์นี้*
    ///
    /// ★★ `docs/07 §1` บังคับให้แยก **"เอกสารเสีย"** ออกจาก **"ตารางเสีย"** ให้ขาด
    /// เพราะสองอย่างนี้เป็นคนละสถานการณ์สำหรับผู้ใช้โดยสิ้นเชิง: อันแรกคืองานหาย
    /// อันหลังคือโครงงานยังอยู่ครบ ขาดแต่ภาพ ซึ่งกู้ได้ด้วย relink
    fn diagnosis(&self) -> String {
        match &self.header {
            Header::TooShort(len) => {
                return format!(
                    "ไฟล์สั้นกว่าหัวไฟล์ ({len} ไบต์ จากที่ต้องมีอย่างน้อย {} ไบต์) — ไม่ใช่ไฟล์ .refx หรือถูกตัดตั้งแต่ต้น",
                    dto::HEADER_LEN
                );
            }
            Header::NotRefx(magic) => {
                return format!("ไม่ใช่ไฟล์ .refx — 4 ไบต์แรกคือ {} ไม่ใช่ \"REFX\"", hex(magic));
            }
            Header::Ok(info) => {
                if !info.writable {
                    return format!(
                        "ไฟล์นี้สร้างจาก RefX รุ่นใหม่กว่า (format v{}) รุ่นนี้อ่านได้ถึง v{} — เนื้อเอกสารจึงไม่ถูกแตกออกมา เพราะการเดาโครงของเวอร์ชันที่ไม่รู้จักจะได้ค่าที่เพี้ยนทั้งก้อน",
                        info.version,
                        dto::FORMAT_VERSION
                    );
                }
            }
        }

        let Some(doc) = &self.document else {
            return "อ่านหัวไฟล์ได้ แต่ไม่ได้อ่านส่วนอื่นเลย".to_owned();
        };
        if doc.present_bytes < doc.declared_bytes {
            return format!(
                "ไฟล์ถูกตัดกลางทาง — หัวไฟล์บอกว่า document ยาว {} ไบต์ แต่มีอยู่จริง {} ไบต์ · ลองไฟล์สำรอง .refx.bak ในโฟลเดอร์เดียวกัน",
                doc.declared_bytes, doc.present_bytes
            );
        }
        if let Some(reason) = doc.skipped {
            return format!("ไม่ได้อ่านเนื้อเอกสาร: {reason}");
        }
        if let Some(err) = &doc.decode_error {
            return format!(
                "เอกสารข้างในอ่านไม่ออก ({err}) — งานในไฟล์นี้กู้จากตัวมันเองไม่ได้ ลองไฟล์สำรอง .refx.bak หรือ .refx.autosave ในโฟลเดอร์เดียวกัน"
            );
        }
        if doc.crc_ok == Some(false) {
            return "checksum ของเอกสารไม่ตรงกับที่หัวไฟล์ประกาศ — เนื้อที่ dump ออกมาอ่านได้ แต่เชื่อไม่ได้ทั้งหมด ให้เทียบกับไฟล์สำรอง .refx.bak".to_owned();
        }

        // มาถึงตรงนี้ = เอกสารดีครบ · ที่เหลือคือเรื่องของภาพอย่างเดียว
        match &self.assets {
            Some(assets) if assets.error.is_some() => {
                let err = assets.error.as_deref().unwrap_or("");
                format!(
                    "★ เอกสารอ่านได้ครบ แต่ asset table เสีย ({err}) — โครงงานยังอยู่ทั้งหมด ขาดแต่ภาพที่ฝังไว้ กู้ภาพได้ด้วย relink"
                )
            }
            Some(assets) if assets.blobs.iter().any(|status| *status != "ok") => {
                let bad = assets.blobs.iter().filter(|s| **s != "ok").count();
                format!(
                    "เอกสารและตารางดีครบ แต่ภาพที่ฝังไว้ {bad} จาก {} ใบ checksum ไม่ตรง — งานยังอยู่ ภาพที่เสียต้องหาไฟล์ต้นฉบับมา relink",
                    assets.blobs.len()
                )
            }
            _ => "ไฟล์นี้อ่านได้ครบทุกส่วน ไม่พบความเสียหาย".to_owned(),
        }
    }

    /// จุดที่ผิดทั้งหมด เรียงตามที่เจอ — ว่าง = ไม่พบอะไรเลย
    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        match &self.header {
            Header::TooShort(len) => problems.push(format!("หัวไฟล์ไม่ครบ ({len} ไบต์)")),
            Header::NotRefx(magic) => {
                problems.push(format!("magic ไม่ใช่ \"REFX\" (ได้ {})", hex(magic)));
            }
            Header::Ok(info) => {
                if !info.writable {
                    problems.push(format!(
                        "format v{} ใหม่กว่าที่รุ่นนี้อ่านได้ (v{})",
                        info.version,
                        dto::FORMAT_VERSION
                    ));
                }
            }
        }
        if let Some(doc) = &self.document {
            if doc.present_bytes < doc.declared_bytes {
                problems.push(format!(
                    "document ถูกตัด (ประกาศ {} ไบต์ มีจริง {} ไบต์)",
                    doc.declared_bytes, doc.present_bytes
                ));
            }
            if doc.crc_ok == Some(false) && doc.present_bytes == doc.declared_bytes {
                problems.push(format!(
                    "doc_crc ไม่ตรง (ไฟล์บอก {:#010x} คำนวณได้ {:#010x})",
                    doc.declared_crc,
                    doc.computed_crc.unwrap_or(0)
                ));
            }
            if let Some(err) = &doc.decode_error {
                problems.push(format!("แตกเนื้อเอกสารไม่สำเร็จ: {err}"));
            }
        }
        if let Some(assets) = &self.assets {
            if let Some(err) = &assets.error {
                problems.push(format!("asset table อ่านไม่ได้: {err}"));
            }
            for (entry, status) in assets.entries.iter().zip(&assets.blobs) {
                if *status != "ok" {
                    problems.push(format!(
                        "blob {} เสีย ({status})",
                        short_hash(entry.hash.as_bytes())
                    ));
                }
            }
        }
        problems
    }
}

// ---------------------------------------------------------------------------
// การเขียน JSON
// ---------------------------------------------------------------------------

impl Report {
    fn write<W: Write>(&self, name: &str, file_bytes: u64, out: &mut W) -> std::io::Result<()> {
        let mut w = JsonWriter::new(out);
        w.begin_object()?;
        w.field_str("tool", "xtask dump-refx")?;
        w.field_str("file", name)?;
        w.field_u64("file_bytes", file_bytes)?;
        // ★ วินิจฉัยอยู่บนสุด — คนที่เปิดไฟล์ dump ยาวหมื่นบรรทัดต้องเห็นก่อนอย่างอื่น
        w.field_str("diagnosis", &self.diagnosis())?;
        w.key("problems")?;
        w.begin_array()?;
        for problem in self.problems() {
            w.next()?;
            w.string(&problem)?;
        }
        w.end_array()?;

        w.key("header")?;
        self.write_header(&mut w)?;

        w.key("document")?;
        match &self.document {
            Some(doc) => write_document_section(&mut w, doc)?,
            None => w.null()?,
        }

        w.key("assets")?;
        match &self.assets {
            Some(assets) => write_assets_section(&mut w, assets)?,
            None => w.null()?,
        }

        w.end_object()?;
        w.finish()?;
        Ok(())
    }

    fn write_header<W: Write>(&self, w: &mut JsonWriter<W>) -> std::io::Result<()> {
        w.begin_object()?;
        match &self.header {
            Header::Ok(info) => {
                w.field_str("status", "ok")?;
                w.field_str("magic", "REFX")?;
                w.field_u64("version", u64::from(info.version))?;
                w.field_u64("understood_version", u64::from(dto::FORMAT_VERSION))?;
                w.field_bool("this_build_can_read_it", info.writable)?;
                w.field_u64("flags", u64::from(info.flags))?;
                w.field_bool("packed", info.packed)?;
                w.field_u64("doc_len", info.doc_len)?;
                w.key("doc_crc")?;
                w.string(&format!("{:#010x}", info.doc_crc))?;
            }
            Header::TooShort(len) => {
                w.field_str("status", "too_short")?;
                w.field_u64("bytes_present", *len as u64)?;
                w.field_u64("bytes_needed", dto::HEADER_LEN as u64)?;
            }
            Header::NotRefx(magic) => {
                w.field_str("status", "not_refx")?;
                w.field_str("magic", &hex(magic))?;
            }
        }
        w.end_object()
    }
}

fn write_document_section<W: Write>(w: &mut JsonWriter<W>, doc: &Document) -> std::io::Result<()> {
    w.begin_object()?;
    w.field_u64("declared_bytes", doc.declared_bytes)?;
    w.field_u64("present_bytes", doc.present_bytes)?;
    w.field_bool("truncated", doc.present_bytes < doc.declared_bytes)?;
    w.field_str("declared_crc", &format!("{:#010x}", doc.declared_crc))?;
    w.key("computed_crc")?;
    match doc.computed_crc {
        Some(crc) => w.string(&format!("{crc:#010x}"))?,
        None => w.null()?,
    }
    w.field_opt_bool("crc_ok", doc.crc_ok)?;
    w.field_opt_str("not_decoded_because", doc.skipped)?;
    w.field_opt_str("decode_error", doc.decode_error.as_deref())?;
    w.key("board")?;
    match &doc.decoded {
        Some(document) => write_board(w, document)?,
        None => w.null()?,
    }
    w.end_object()
}

fn write_assets_section<W: Write>(w: &mut JsonWriter<W>, assets: &Assets) -> std::io::Result<()> {
    w.begin_object()?;
    w.field_bool("packed_flag", assets.packed_flag)?;
    if !assets.packed_flag {
        w.field_str("note", "ไฟล์ linked ไม่มี asset table — ไม่ใช่ความผิดพลาด")?;
    }
    w.field_opt_u64("table_offset", assets.table_offset)?;
    w.field_opt_u64("declared_count", assets.declared_count.map(u64::from))?;
    w.key("declared_table_crc")?;
    match assets.declared_table_crc {
        Some(crc) => w.string(&format!("{crc:#010x}"))?,
        None => w.null()?,
    }
    w.field_opt_str("error", assets.error.as_deref())?;
    w.field_bool("blobs_verified", !assets.blobs.is_empty())?;
    w.key("entries")?;
    w.begin_array()?;
    for (index, entry) in assets.entries.iter().enumerate() {
        w.next()?;
        w.begin_object()?;
        w.field_str("hash", &hex(entry.hash.as_bytes()))?;
        w.field_u64("offset", entry.offset)?;
        w.field_u64("length", entry.len)?;
        w.field_str("crc", &format!("{:#010x}", entry.crc))?;
        w.field_str(
            "blob",
            assets.blobs.get(index).copied().unwrap_or("not_checked"),
        )?;
        w.end_object()?;
    }
    w.end_array()?;
    w.end_object()
}

// ---------------------------------------------------------------------------
// เอกสาร → JSON  (ค่าดิบตามที่อยู่ในไฟล์ ดูหัวโมดูล)
// ---------------------------------------------------------------------------

fn write_board<W: Write>(w: &mut JsonWriter<W>, doc: &v1::DocumentDto) -> std::io::Result<()> {
    w.begin_object()?;
    w.field_str("name", &doc.name)?;
    w.field_u64("item_count", doc.items.len() as u64)?;

    w.key("groups")?;
    w.begin_array()?;
    for group in &doc.groups {
        w.next()?;
        w.begin_object()?;
        w.field_str("name", &group.name)?;
        w.field_bool("collapsed", group.collapsed)?;
        w.end_object()?;
    }
    w.end_array()?;

    w.key("tags")?;
    w.begin_array()?;
    for tag in &doc.tags {
        w.next()?;
        w.begin_object()?;
        w.field_u64("id", u64::from(tag.id))?;
        w.field_str("name", &tag.name)?;
        w.end_object()?;
    }
    w.end_array()?;

    w.key("settings")?;
    w.begin_object()?;
    w.field_f32s("background", &doc.settings.background)?;
    w.end_object()?;

    w.key("view")?;
    w.begin_object()?;
    w.key("canvas")?;
    write_camera(w, &doc.view.canvas)?;
    w.key("arrange")?;
    write_camera(w, &doc.view.arrange)?;
    w.field_u64("mode", u64::from(doc.view.mode))?;
    w.field_opt_str("mode_name", mode_name(doc.view.mode))?;
    w.end_object()?;

    // ★ ลำดับในรายการ *คือ* z-order (ล่างสุด → บนสุด) — พิมพ์ดัชนีกำกับไว้
    //   เพื่อให้อ้างถึงใบใดใบหนึ่งได้เวลาคุยกับผู้ใช้
    w.key("items")?;
    w.begin_array()?;
    for (index, item) in doc.items.iter().enumerate() {
        w.next()?;
        write_item(w, index, item)?;
    }
    w.end_array()?;
    w.end_object()
}

fn write_camera<W: Write>(w: &mut JsonWriter<W>, camera: &v1::CameraDto) -> std::io::Result<()> {
    w.begin_object()?;
    w.field_f32s("center", &camera.center)?;
    w.key("zoom")?;
    w.f32(camera.zoom)?;
    w.end_object()
}

fn write_item<W: Write>(
    w: &mut JsonWriter<W>,
    index: usize,
    item: &v1::ItemDto,
) -> std::io::Result<()> {
    w.begin_object()?;
    w.field_u64("z", index as u64)?;
    w.field_opt_u64("group", item.group.map(u64::from))?;

    w.key("kind")?;
    w.begin_object()?;
    match &item.kind {
        v1::KindDto::Image(asset) => {
            w.field_str("type", "image")?;
            w.field_str("hash", &hex(&asset.hash))?;
            w.field_str("path", &asset.path)?;
            w.key("px_size")?;
            w.begin_array()?;
            for side in asset.px_size {
                w.next()?;
                w.u64(u64::from(side))?;
            }
            w.end_array()?;
            w.key("mtime")?;
            w.i64(asset.mtime)?;
            w.field_u64("file_size", asset.file_size)?;
        }
        v1::KindDto::Text(text) => {
            w.field_str("type", "text")?;
            w.field_str("text", text)?;
        }
        v1::KindDto::Missing { path, reason } => {
            w.field_str("type", "missing")?;
            w.field_str("path", path)?;
            w.field_u64("reason", u64::from(*reason))?;
            w.field_opt_str("reason_name", missing_reason_name(*reason))?;
        }
    }
    w.end_object()?;

    w.key("canvas")?;
    let canvas = &item.canvas;
    w.begin_object()?;
    w.field_f32s("pos", &canvas.pos)?;
    w.field_f32s("size", &canvas.size)?;
    w.key("rotation")?;
    w.f32(canvas.rotation)?;
    w.field_u64("flip", u64::from(canvas.flip))?;
    w.field_opt_str("flip_name", flip_name(canvas.flip))?;
    w.key("opacity")?;
    w.f32(canvas.opacity)?;
    w.field_f32s("crop", &canvas.crop)?;
    w.field_bool("locked", canvas.locked)?;
    w.field_bool("visible", canvas.visible)?;
    w.key("filter")?;
    w.begin_object()?;
    w.field_bool("grayscale", canvas.filter.grayscale)?;
    w.field_bool("invert", canvas.filter.invert)?;
    w.key("brightness")?;
    w.f32(canvas.filter.brightness)?;
    w.key("contrast")?;
    w.f32(canvas.filter.contrast)?;
    w.end_object()?;
    w.end_object()?;

    w.key("meta")?;
    let meta = &item.meta;
    w.begin_object()?;
    w.key("tags")?;
    w.begin_array()?;
    for tag in &meta.tags {
        w.next()?;
        w.u64(u64::from(*tag))?;
    }
    w.end_array()?;
    w.field_u64("rating", u64::from(meta.rating))?;
    w.field_u64("color_label", u64::from(meta.color_label))?;
    w.field_opt_str("color_label_name", color_label_name(meta.color_label))?;
    w.field_str("note", &meta.note)?;
    w.key("added_at")?;
    w.i64(meta.added_at)?;
    w.field_bool("pinned", meta.pinned)?;
    w.end_object()?;

    w.end_object()
}

// ---------------------------------------------------------------------------
// ★ ชื่อของค่า enum — **ผ่าน `from_wire` ของจริงเสมอ ห้ามเขียนตัวเลขซ้ำที่นี่**
//
// ตัวเลขในไฟล์เป็นสัญญาถาวรที่ `refx-core` เป็นเจ้าของ · ถ้า dump ถือตารางแปลง
// ของตัวเอง วันหนึ่งสองฝั่งจะไม่ตรงกัน แล้วเครื่องมือ debug จะรายงานชื่อผิด
// ซึ่งแย่กว่าไม่รายงานชื่อเลย · `None` = ค่านี้รุ่นนี้ไม่รู้จัก (มาจากรุ่นใหม่กว่า)
// ---------------------------------------------------------------------------

fn flip_name(raw: u8) -> Option<&'static str> {
    match Flip::from_wire(raw) {
        Flip::None => Some("none"),
        Flip::Horizontal => Some("horizontal"),
        Flip::Vertical => Some("vertical"),
        Flip::Both => Some("both"),
        Flip::Unknown(_) => None,
    }
}

fn color_label_name(raw: u8) -> Option<&'static str> {
    if raw == 0 {
        return Some("none"); // 0 = ไม่มีป้าย ซึ่งต่างจาก "ป้ายที่ไม่รู้จัก"
    }
    match ColorLabel::from_wire(raw)? {
        ColorLabel::Red => Some("red"),
        ColorLabel::Orange => Some("orange"),
        ColorLabel::Yellow => Some("yellow"),
        ColorLabel::Green => Some("green"),
        ColorLabel::Blue => Some("blue"),
        ColorLabel::Purple => Some("purple"),
        ColorLabel::Unknown(_) => None,
    }
}

fn missing_reason_name(raw: u8) -> Option<&'static str> {
    match MissingReason::from_wire(raw) {
        MissingReason::FileNotFound => Some("file_not_found"),
        MissingReason::TooLarge => Some("too_large"),
        MissingReason::UnsupportedFormat => Some("unsupported_format"),
        MissingReason::Damaged => Some("damaged"),
        // ★ `0` ก็ตกมาที่ `Unreadable` เหมือนกัน แต่มันไม่ใช่รหัสของใคร —
        //   รายงานว่า "ไม่รู้จัก" ตามความจริง ไม่ใช่ตามค่าที่เกราะแปลงให้
        MissingReason::Unreadable => (raw == 5).then_some("unreadable"),
        MissingReason::Unknown(_) => None,
    }
}

fn mode_name(raw: u8) -> Option<&'static str> {
    // `Mode::from_wire` ตกกลับเป็น `Canvas` ให้ทุกค่าที่ไม่รู้จักโดยตั้งใจ
    // (`docs/02 §2.2b`) — เทียบ `to_wire()` กลับจึงเป็นทางเดียวที่แยกออกว่า
    // ไฟล์บอก 0 จริง ๆ หรือบอกค่าที่เราไม่รู้จัก
    let mode = Mode::from_wire(raw);
    (mode.to_wire() == raw).then_some(match mode {
        Mode::Canvas => "canvas",
        Mode::Arrange => "arrange",
    })
}

// ---------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 8 ตัวแรกของ hash — พอให้คนอ้างถึงได้ในข้อความสั้น ๆ
fn short_hash(bytes: &[u8]) -> String {
    hex(&bytes[..bytes.len().min(4)])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use refx_core::arena::{ArenaKey as _, BoardId};
    use refx_core::board::{
        AssetRef, Board, BoardParts, ImageFormat, Item, ItemKind, ItemParts, TextNote,
    };
    use refx_core::hash::ContentHash;
    use refx_io::packed::PackSource;
    use std::io::Cursor;

    fn board_id() -> BoardId {
        BoardId::from_parts(0, 0)
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-dump-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_board() -> Board {
        let mut note = Item::new(ItemKind::Text(TextNote {
            text: "โน้ต \"ทดสอบ\"".to_owned(),
        }));
        note.meta.rating = 3;
        let image = Item::new(ItemKind::Image(AssetRef {
            hash: ContentHash::from_bytes([7; 32]),
            path: std::path::PathBuf::from("C:/ref/a.png"),
            px_size: glam::UVec2::new(1920, 1080),
            format: ImageFormat::Unknown,
            embedded: false,
            mtime: 1_700_000_000_000,
            file_size: 4096,
        }));
        Board::load(
            board_id(),
            BoardParts {
                name: "งานของผู้ใช้".to_owned(),
                items: vec![
                    ItemParts {
                        item: note,
                        group: None,
                    },
                    ItemParts {
                        item: image,
                        group: None,
                    },
                ],
                ..BoardParts::default()
            },
        )
    }

    fn dump_bytes(bytes: &[u8]) -> String {
        let mut out = Vec::new();
        let len = bytes.len() as u64;
        dump(
            &mut Cursor::new(bytes.to_vec()),
            len,
            "test.refx",
            &mut out,
            Options::default(),
        )
        .unwrap();
        String::from_utf8(out).unwrap()
    }

    /// เขียนไฟล์ packed จริงลง `Vec<u8>` (ต้องใช้ `Seek` — เหมือนของจริง)
    fn packed_bytes(board: &Board, sources: &[PackSource]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        packed::write_packed(&mut cursor, board, sources).unwrap();
        cursor.into_inner()
    }

    // ---------- ★★★ ไฟล์ที่ดี ----------

    /// ★ เนื้อของ board ต้องออกมาครบ ไม่ใช่แค่หัวไฟล์
    #[test]
    fn a_healthy_file_dumps_its_whole_board() {
        let text = dump_bytes(&dto::encode(&sample_board()).unwrap());
        assert!(
            text.contains("\"diagnosis\": \"ไฟล์นี้อ่านได้ครบทุกส่วน"),
            "{text}"
        );
        assert!(text.contains("\"problems\": []"), "{text}");
        assert!(text.contains("\"name\": \"งานของผู้ใช้\""), "{text}");
        assert!(text.contains("\"item_count\": 2"), "{text}");
        assert!(text.contains("\"type\": \"text\""), "{text}");
        assert!(text.contains("\"type\": \"image\""), "{text}");
        assert!(text.contains("\"path\": \"C:/ref/a.png\""), "{text}");
        assert!(text.contains("\"rating\": 3"), "{text}");
        // ★ สตริงที่มีเครื่องหมายคำพูดต้องถูก escape ไม่ใช่ทำให้ JSON พัง
        assert!(text.contains("\\\"ทดสอบ\\\""), "{text}");
        assert!(text.contains("\"crc_ok\": true"), "{text}");
    }

    // ---------- ★★★ ไฟล์ที่พัง — เหตุผลทั้งหมดที่เครื่องมือนี้มีอยู่ ----------

    /// ★★★ **checksum ไม่ตรง → บอกว่าส่วนไหนพัง แล้วยัง dump ส่วนที่เหลือ**
    ///
    /// ★★ ไบต์ที่พลิกอยู่ใน **หัวไฟล์** ไม่ใช่ในเนื้อเอกสาร — เนื้อจึงยังดีทุกไบต์
    /// และสิ่งที่ dump ออกมาคือของจริง ไม่ใช่ของที่เดาเอา (`docs/08 §3.9` ข้อ 1b)
    #[test]
    fn a_file_with_a_bad_checksum_still_gives_up_its_board() {
        let mut bytes = dto::encode(&sample_board()).unwrap();
        bytes[16] ^= 0xFF;
        let text = dump_bytes(&bytes);

        assert!(text.contains("\"crc_ok\": false"), "{text}");
        assert!(text.contains("doc_crc ไม่ตรง"), "{text}");
        assert!(text.contains("checksum ของเอกสารไม่ตรง"), "{text}");
        // ★ ยังต้องได้เนื้อ — ไม่งั้นเครื่องมือนี้ไม่มีประโยชน์กับไฟล์ที่ผู้ใช้ส่งมา
        assert!(text.contains("\"name\": \"งานของผู้ใช้\""), "{text}");
        assert!(text.contains("\"item_count\": 2"), "{text}");
    }

    /// ★★★ **เวอร์ชันใหม่กว่า → บอกเลขทั้งสองฝั่ง ไม่ใช่ปฏิเสธเงียบ ๆ**
    #[test]
    fn a_file_from_a_newer_build_reports_both_version_numbers() {
        let mut bytes = dto::encode(&sample_board()).unwrap();
        bytes[4..6].copy_from_slice(&99u16.to_le_bytes());
        let text = dump_bytes(&bytes);

        assert!(text.contains("\"version\": 99"), "{text}");
        assert!(text.contains("\"understood_version\": 2"), "{text}");
        assert!(text.contains("\"this_build_can_read_it\": false"), "{text}");
        assert!(text.contains("format v99 ใหม่กว่าที่รุ่นนี้อ่านได้ (v2)"), "{text}");
        // ★ และ **ต้องไม่เดาเนื้อใน** — postcard ไม่ self-describing (docs/07 §3)
        assert!(text.contains("\"board\": null"), "{text}");
        assert!(text.contains("\"not_decoded_because\":"), "{text}");
        assert!(!text.contains("\"item_count\""), "{text}");
    }

    /// ★★★ **asset table เสีย แต่ document ดี = คนละสถานการณ์** (`docs/07 §1`)
    ///
    /// ผู้ใช้ต้องรู้ว่านี่คือ *"ขาดแต่ภาพ"* ไม่ใช่ *"งานหาย"* — สองอย่างนี้
    /// ต่างกันคนละขั้วและมีทางแก้คนละทาง
    #[test]
    fn a_broken_asset_table_is_reported_apart_from_a_broken_document() {
        let dir = temp_dir("badtable");
        let blob = dir.join("a.bin");
        std::fs::write(&blob, vec![9u8; 2048]).unwrap();
        let board = sample_board();
        let mut bytes = packed_bytes(
            &board,
            &[PackSource {
                hash: ContentHash::from_bytes([7; 32]),
                path: blob,
            }],
        );

        // พลิกไบต์แรกของ hash ในตาราง — `table_crc` จึงไม่ตรง แต่ document ไม่ถูกแตะ
        let doc_len = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
        let entry_at = dto::HEADER_LEN + doc_len + TABLE_HEADER_LEN;
        bytes[entry_at] ^= 0x01;
        let text = dump_bytes(&bytes);

        assert!(text.contains("เอกสารอ่านได้ครบ แต่ asset table เสีย"), "{text}");
        assert!(text.contains("โครงงานยังอยู่ทั้งหมด ขาดแต่ภาพ"), "{text}");
        assert!(text.contains("asset table อ่านไม่ได้"), "{text}");
        // ★ และข้อความของตาราง **ต้องไม่พูดถึง "document"** — `OpenError` ถูกใช้
        //   ร่วมกันสองเส้นทาง ข้อความดิบของมันเขียนจากมุมของเอกสาร การพิมพ์ตรง ๆ
        //   จะได้ "asset table อ่านไม่ได้: document is malformed" ซึ่งลบล้าง
        //   ความแตกต่างที่เทสต์ตัวนี้ทั้งตัวมีไว้รักษา (เกิดขึ้นจริง เจอตอนไล่ดู seed)
        assert!(
            !text.contains("asset table อ่านไม่ได้: document"),
            "ข้อความของตารางไปยืมคำของเอกสารมาใช้:\n{text}"
        );
        // ★ document ต้องยังออกมาครบ พร้อม crc ที่ผ่าน — นี่คือครึ่งที่แยกสองเรื่องออกจากกัน
        assert!(text.contains("\"crc_ok\": true"), "{text}");
        assert!(text.contains("\"name\": \"งานของผู้ใช้\""), "{text}");
        // ★ และตัวเลขที่ไฟล์ *อ้างว่า* ต้องยังรายงานได้ ทั้งที่ parser ปฏิเสธทั้งตาราง
        assert!(text.contains("\"declared_count\": 1"), "{text}");
    }

    /// ★ negative control ของข้อบน — ตารางที่ **ไม่ได้ถูกแก้** ต้องไม่ถูกฟ้อง
    ///
    /// ถ้าไม่มีข้อนี้ เทสต์ข้างบนจะเขียวได้แม้ dump จะบ่นว่าตารางเสียทุกไฟล์
    #[test]
    fn an_intact_asset_table_is_not_reported_as_broken() {
        let dir = temp_dir("goodtable");
        let blob = dir.join("a.bin");
        std::fs::write(&blob, vec![9u8; 2048]).unwrap();
        let bytes = packed_bytes(
            &sample_board(),
            &[PackSource {
                hash: ContentHash::from_bytes([7; 32]),
                path: blob,
            }],
        );
        let text = dump_bytes(&bytes);

        assert!(text.contains("ไฟล์นี้อ่านได้ครบทุกส่วน"), "{text}");
        assert!(text.contains("\"problems\": []"), "{text}");
        assert!(text.contains("\"packed_flag\": true"), "{text}");
        assert!(text.contains("\"length\": 2048"), "{text}");
        assert!(
            text.contains(&format!("\"hash\": \"{}\"", hex(&[7u8; 32]))),
            "{text}"
        );
    }

    /// ★★ ไฟล์ที่ถูกตัดกลางทาง — บอกทั้งขนาดที่ประกาศและขนาดที่มีจริง
    #[test]
    fn a_truncated_file_says_how_much_is_missing() {
        let bytes = dto::encode(&sample_board()).unwrap();
        let cut = bytes.len() - 40;
        let text = dump_bytes(&bytes[..cut]);

        assert!(text.contains("\"truncated\": true"), "{text}");
        assert!(text.contains("ไฟล์ถูกตัดกลางทาง"), "{text}");
        assert!(text.contains(".refx.bak"), "{text}");
    }

    /// ★ ไฟล์ที่ไม่ใช่ `.refx` เลย — ต้องบอกว่าเห็นอะไรแทน ไม่ใช่แค่ "ผิดพลาด"
    #[test]
    fn a_file_that_is_not_refx_says_what_it_saw_instead() {
        let text = dump_bytes(b"PK\x03\x04 this is a zip file, not a board");
        assert!(text.contains("\"status\": \"not_refx\""), "{text}");
        assert!(text.contains("ไม่ใช่ไฟล์ .refx"), "{text}");
        assert!(text.contains("\"document\": null"), "{text}");
    }

    /// ★ ไฟล์ว่าง/สั้นเกิน — ต้องไม่ panic และต้องบอกจำนวนไบต์ที่มี
    #[test]
    fn an_empty_file_does_not_crash_the_tool() {
        let text = dump_bytes(b"");
        assert!(text.contains("\"status\": \"too_short\""), "{text}");
        assert!(text.contains("\"bytes_present\": 0"), "{text}");
    }

    /// ★★★ **`doc_len` ที่โกหกเป็นเลขมหาศาลต้องไม่พาไปจอง RAM ก้อนใหญ่** (I-4)
    ///
    /// เครื่องมือ debug รับไฟล์ที่ตั้งใจโจมตีเหมือนกันทุกประการ
    #[test]
    fn a_lying_doc_len_never_reserves_what_it_asks_for() {
        let mut bytes = dto::encode(&sample_board()).unwrap();
        bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        let text = dump_bytes(&bytes);

        assert!(text.contains("เกินเพดาน"), "{text}");
        assert!(text.contains("\"board\": null"), "{text}");
        assert!(text.contains("\"present_bytes\": 0"), "{text}");
    }

    // ---------- ★★★ ราคาที่วัดได้: ไบต์ที่ถูกอ่านจริง ----------

    /// แหล่งข้อมูลที่นับว่าถูกอ่านไปกี่ไบต์
    struct Counting {
        inner: Cursor<Vec<u8>>,
        read: u64,
    }
    impl Read for Counting {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.read += n as u64;
            Ok(n)
        }
    }
    impl Seek for Counting {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    fn count_reads(bytes: Vec<u8>, options: Options) -> (u64, u64) {
        let len = bytes.len() as u64;
        let mut src = Counting {
            inner: Cursor::new(bytes),
            read: 0,
        };
        let mut out = Vec::new();
        dump(&mut src, len, "big.refx", &mut out, options).unwrap();
        (src.read, len)
    }

    /// ★★★ **ไฟล์ packed ระดับ GB ต้องไม่ถูกสูบเข้า RAM** (`docs/07 §1`)
    ///
    /// วัดด้วย **จำนวนไบต์ที่ถูกอ่านจริง** ไม่ใช่ด้วยการวัด RAM (ซึ่งวัดข้ามเครื่อง
    /// ไม่ได้ — `docs/08 §3.9` ข้อ 5b) · คู่กับ negative control ข้างล่างที่
    /// พิสูจน์ว่าตัวนับนี้ไม่ได้เสีย
    #[test]
    fn a_packed_file_is_never_slurped_into_memory() {
        let dir = temp_dir("stream");
        let blob = dir.join("big.bin");
        std::fs::write(&blob, vec![3u8; 8 << 20]).unwrap(); // 8 MB
        let bytes = packed_bytes(
            &sample_board(),
            &[PackSource {
                hash: ContentHash::from_bytes([7; 32]),
                path: blob,
            }],
        );

        let (read, total) = count_reads(bytes, Options::default());
        println!("ไฟล์ {total} ไบต์ · dump อ่านไปจริง {read} ไบต์");
        assert!(
            read < 64 << 10,
            "อ่านไป {read} ไบต์จากไฟล์ {total} ไบต์ — blob ถูกอ่านเข้ามาด้วย"
        );
    }

    /// ★★ negative control ของข้อบน — `--verify-assets` **ต้อง** อ่าน blob จริง
    ///
    /// ถ้าตัวนับเสีย (หรือ dump ไม่เคยแตะ blob เลยไม่ว่าสั่งอะไร) ข้อนี้จะแดง
    /// · ไม่มีข้อนี้ เทสต์ข้างบนจะเขียวได้แม้ `--verify-assets` จะไม่ทำงาน
    #[test]
    fn verifying_assets_really_does_read_every_blob() {
        let dir = temp_dir("verify");
        let blob = dir.join("big.bin");
        std::fs::write(&blob, vec![3u8; 8 << 20]).unwrap();
        let bytes = packed_bytes(
            &sample_board(),
            &[PackSource {
                hash: ContentHash::from_bytes([7; 32]),
                path: blob,
            }],
        );

        let (read, total) = count_reads(
            bytes,
            Options {
                verify_assets: true,
            },
        );
        println!("--verify-assets: ไฟล์ {total} ไบต์ · อ่านไปจริง {read} ไบต์");
        assert!(
            read >= 8 << 20,
            "สั่งตรวจ blob แล้วแต่อ่านไปแค่ {read} ไบต์ — แปลว่าไม่ได้ตรวจจริง"
        );
    }

    /// ★★ blob ที่ถูกแก้ไบต์ต้องถูกจับได้ตอนสั่งตรวจ — และต้องบอกว่าใบไหน
    #[test]
    fn a_tampered_blob_is_named_when_asked_to_verify() {
        let dir = temp_dir("tamper");
        let blob = dir.join("a.bin");
        std::fs::write(&blob, vec![9u8; 4096]).unwrap();
        let mut bytes = packed_bytes(
            &sample_board(),
            &[PackSource {
                hash: ContentHash::from_bytes([7; 32]),
                path: blob,
            }],
        );
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;

        let len = bytes.len() as u64;
        let mut out = Vec::new();
        dump(
            &mut Cursor::new(bytes),
            len,
            "t.refx",
            &mut out,
            Options {
                verify_assets: true,
            },
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("\"blob\": \"crc_mismatch\""), "{text}");
        assert!(text.contains("checksum ไม่ตรง"), "{text}");
        // ★ negative control: ไม่สั่งตรวจ = ต้องไม่แอบอ้างว่าตรวจแล้ว
        let quiet = dump_bytes(&packed_bytes(&sample_board(), &[]));
        assert!(quiet.contains("\"blobs_verified\": false"), "{quiet}");
    }

    // ---------- ★ ค่าที่รุ่นนี้ไม่รู้จัก ต้องมองเห็นได้ ----------

    /// ★★★ **ค่า enum จากรุ่นใหม่กว่าต้องโผล่เป็นเลขดิบ ไม่ใช่ถูกกลืน**
    ///
    /// นี่คือครึ่งหนึ่งของเหตุผลที่เครื่องมือนี้ dump **DTO ดิบ** แทนที่จะ dump
    /// `Board` — `Board` ผ่าน `sanitized()` มาแล้วจึงบอกไม่ได้ว่าในไฟล์มีอะไร
    #[test]
    fn values_this_build_does_not_understand_are_visible_in_the_dump() {
        let mut item = Item::new(ItemKind::Text(TextNote {
            text: "x".to_owned(),
        }));
        item.canvas.flip = Flip::from_wire(200);
        item.meta.color_label = ColorLabel::from_wire(201);
        let board = Board::load(
            board_id(),
            BoardParts {
                items: vec![ItemParts { item, group: None }],
                ..BoardParts::default()
            },
        );
        let text = dump_bytes(&dto::encode(&board).unwrap());

        assert!(text.contains("\"flip\": 200"), "{text}");
        assert!(text.contains("\"flip_name\": null"), "{text}");
        assert!(text.contains("\"color_label\": 201"), "{text}");
        assert!(text.contains("\"color_label_name\": null"), "{text}");
    }

    /// ★ negative control ของข้อบน — ค่าที่รุ่นนี้ **รู้จัก** ต้องมีชื่อกำกับ
    #[test]
    fn values_this_build_understands_get_their_name_printed() {
        let mut item = Item::new(ItemKind::Text(TextNote {
            text: "x".to_owned(),
        }));
        item.canvas.flip = Flip::Horizontal;
        item.meta.color_label = Some(ColorLabel::Blue);
        let board = Board::load(
            board_id(),
            BoardParts {
                items: vec![ItemParts { item, group: None }],
                ..BoardParts::default()
            },
        );
        let text = dump_bytes(&dto::encode(&board).unwrap());

        assert!(text.contains("\"flip_name\": \"horizontal\""), "{text}");
        assert!(text.contains("\"color_label_name\": \"blue\""), "{text}");
        assert!(text.contains("\"mode_name\": \"canvas\""), "{text}");
    }

    /// ★ ชื่อของค่า enum ต้องมาจาก `from_wire` ของจริง ไม่ใช่ตารางที่ dump จำเอง
    #[test]
    fn enum_names_follow_the_wire_contract_in_refx_core() {
        for raw in 0u8..=u8::MAX {
            let known = Flip::from_wire(raw).is_known();
            assert_eq!(
                flip_name(raw).is_some(),
                known,
                "flip {raw}: dump กับ refx-core ไม่ตรงกัน"
            );
            assert_eq!(
                missing_reason_name(raw).is_some(),
                MissingReason::from_wire(raw).is_known() && raw != 0,
                "reason {raw}: dump กับ refx-core ไม่ตรงกัน"
            );
        }
        assert_eq!(mode_name(0), Some("canvas"));
        assert_eq!(mode_name(1), Some("arrange"));
        assert_eq!(mode_name(2), None, "ค่าที่ไม่รู้จักต้องไม่ถูกกลืนเป็น canvas");
    }
}
