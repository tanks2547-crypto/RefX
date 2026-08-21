//! ★★★ Packed mode — `.refx` ที่ **ฝังไฟล์ภาพต้นฉบับไว้ข้างใน** (P4-5)
//!
//! ```text
//! ┌────────────────────────────────────────────┐
//! │ header 20 B   magic·version=2·flags(packed)│
//! │               ·doc_len·doc_crc             │
//! ├────────────────────────────────────────────┤
//! │ document      postcard + zstd (เหมือน v1)   │
//! ├────────────────────────────────────────────┤
//! │ table header  [count:u32][table_crc:u32]    │  ← 8 B
//! │ asset table   [hash:32][off:u64][len:u64]   │  ← 52 B × count
//! │               [crc:u32]                     │
//! ├────────────────────────────────────────────┤
//! │ asset blobs   ไฟล์ต้นฉบับ ไม่แปลงอะไรเลย      │
//! └────────────────────────────────────────────┘
//! ```
//!
//! ## ★★★ `version = 2` แปลว่า **"ไฟล์นี้มี asset ฝังอยู่"** ไม่ใช่ "ผู้ใช้เลือก packed"
//!
//! รุ่นเก่าเปิดไฟล์ที่มี blob ได้ "สำเร็จ" โดยบังเอิญ — `decode` ตัดที่ `doc_len`
//! อยู่แล้ว จึงมองข้าม blob ท้ายไฟล์ไปเงียบ ๆ แล้วแสดงภาพเป็น `Missing`
//!
//! **แล้วถ้าผู้ใช้กด Save ทับ รุ่นเก่าจะเขียน v1 ทับลงไป — ภาพที่ฝังไว้หายถาวร**
//! ซึ่งเป็นเคสที่ `docs/07 §2` ห้ามไว้ตรง ๆ (I-3)
//!
//! → ไฟล์ที่ **มี blob จริงอยู่ข้างใน** ประกาศ v2 เพื่อให้รุ่นเก่าปฏิเสธทั้ง
//! การเปิดและการเขียนทับ (`may_overwrite` กันไว้ให้แล้ว) · ไฟล์ที่ไม่มี blob
//! ยังเป็น v1 รุ่นเก่าจึงเปิดงานประจำวันได้ตามปกติ
//! — **เสียความเข้ากันได้เฉพาะไฟล์ที่มีของให้เสีย**
//!
//! ## ★★★ Linked **ก็ฝัง** — เฉพาะใบที่ไม่มีไฟล์ต้นทาง (ตัดสิน 18 ส.ค. 2026)
//!
//! ภาพที่ผู้ใช้วางจาก clipboard **ไม่มีไฟล์ต้นทางเลย** · ถ้าบันทึกเป็น linked
//! ตรง ๆ `AssetRef` จะชี้ไป path ที่ไม่มีอยู่ → เปิดกลับมาได้ `Missing` →
//! **ภาพหายถาวร** ซึ่งเป็นการละเมิด I-3 แบบเงียบที่สุดเท่าที่จะเป็นไปได้
//!
//! | โหมด | ทำอะไร |
//! |---|---|
//! | [`SaveMode::Packed`] | ฝังภาพ **ทุกใบ** |
//! | [`SaveMode::Linked`] | ลิงก์ใบที่มีไฟล์ของผู้ใช้ · **ฝังเฉพาะใบที่ไม่มี** |
//!
//! ★★ **ห้ามแก้ด้วยการเตือนแล้วให้ผู้ใช้เลือก** — คนที่วางภาพจากเบราว์เซอร์
//! ไม่รู้ว่า linked กับ packed ต่างกันยังไง การถามตอนเขากำลังจะบันทึกคือการ
//! โยนการตัดสินใจที่เราควรตัดสินให้ ไปให้คนที่มีข้อมูลน้อยกว่าเรา
//!
//! ได้สามอย่างพร้อมกัน: ไม่มีทางเสียภาพที่วางไว้ · ไฟล์ linked ไม่โตโดยไม่จำเป็น
//! · ภาพที่วางมีที่อยู่ถาวรแล้วจึงขอ working texture ได้ (ปลดหนี้ P1-8)
//!
//! ## ★★ `embedded` ไม่ได้ถูกเก็บลงไฟล์ — มัน **อนุมานจาก asset table**
//!
//! `AssetRef::embedded` เป็นหนึ่งใน "ฟิลด์ที่ไม่มีใครเขียนค่าจริง" มาตลอด และ
//! P4-1 ตัดมันออกจาก DTO ด้วยเหตุผลว่า *"อย่าบันทึกฟิลด์ที่ไม่มีความหมาย"*
//!
//! ตอนนี้มันมีความหมายแล้ว แต่ **ยังไม่ควรอยู่ในไฟล์**: ค่าที่ถูกต้องคือ
//! *"hash นี้อยู่ใน asset table หรือเปล่า"* ซึ่งอ่านได้จากไฟล์ตรง ๆ อยู่แล้ว
//! การเก็บซ้ำอีกที่แปลว่ามันขัดกันเองได้ (ไฟล์บอก embedded แต่ table ไม่มี blob)
//! แล้วเราจะต้องตัดสินว่าจะเชื่ออันไหน — ปัญหาที่ไม่ต้องมีตั้งแต่แรก
//!
//! ## ★★★ ไฟล์ระดับ GB — ห้ามโหลดเข้า RAM ทั้งก้อน
//!
//! packed ของ mood board 500 ภาพ ๆ ละ 20 MB = 10 GB · ทั้งฝั่งเขียนและฝั่งอ่าน
//! จึงเป็น **สตรีม** ทั้งคู่ ([`write_packed`] / [`extract`]) ไม่มีจุดไหนที่
//! `Vec<u8>` โตตามขนาดไฟล์ · `clippy.toml` ที่แบน `fs::read` เคยจับข้อนี้ให้แล้ว
//! รอบ P4-2 — ที่นี่คือที่ที่มันสำคัญที่สุด
//!
//! spec: docs/07-file-format.md §1 §2, ROADMAP P4-5

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use refx_core::board::Board;
use refx_core::hash::ContentHash;

use crate::dto::{self, HEADER_LEN, OpenError};

/// เวอร์ชันที่ไฟล์ packed ประกาศ (ดูหัวโมดูล)
pub const PACKED_VERSION: u16 = 2;

/// ขนาดของหัว asset table — `[count:u32][table_crc:u32]`
pub const TABLE_HEADER_LEN: usize = 4 + 4;

/// ขนาดของหนึ่งแถวใน asset table — `[hash:32][offset:u64][len:u64][crc:u32]`
pub const ENTRY_LEN: usize = 32 + 8 + 8 + 4;

/// ★ เพดานจำนวน asset ในหนึ่งไฟล์ (I-4/I-6)
///
/// board รับได้ 3,072 ใบ (ขนาด atlas) — เผื่อไว้เท่าตัวกว่าเพื่อไม่ให้เพดานนี้
/// เป็นตัวขวางก่อนเพดานจริง · หน้าที่ของมันคือกันไฟล์ที่ประกาศ `count` มหาศาล
/// แล้วพาเราไปจอง `Vec` ก้อนใหญ่ตั้งแต่ยังไม่อ่านอะไร
pub const MAX_ASSETS: u32 = 8_192;

/// ★ เพดานขนาดของ asset หนึ่งก้อน — ตรงกับ `refx_asset::Limits::max_file_bytes`
///
/// ไฟล์ที่ใหญ่กว่านี้ decode ไม่ได้อยู่แล้ว การยอมฝังมันเข้าไปคือการสร้างไฟล์
/// ที่เปิดกลับมาแล้วใช้ไม่ได้
pub const MAX_ASSET_BYTES: u64 = 512 << 20;

/// ★ ไฟล์ที่จะถูกฝัง — ผู้เรียกเป็นคนบอกว่า hash ไหนอยู่ที่ path ไหน
///
/// ★★ `refx-io` **ไม่ไปหาไฟล์เอง**: การตัดสินว่า asset ตัวไหนหาเจอ/ตัวไหนหาย
/// เป็นเรื่องของชั้นบน (relink — docs/07 §2) ที่นี่รับรายการมาแล้วฝังให้
#[derive(Debug, Clone)]
pub struct PackSource {
    /// hash ของเนื้อไฟล์ — คีย์ที่ `AssetRef` ใช้อ้าง
    pub hash: ContentHash,
    /// ไฟล์ต้นฉบับบนดิสก์
    pub path: PathBuf,
}

/// โหมดที่ผู้ใช้เลือกตอนบันทึก (`docs/07 §2`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveMode {
    /// ★ ค่าปริยาย — ลิงก์ไปไฟล์ของผู้ใช้ **แต่ยังฝังใบที่ไม่มีไฟล์ต้นทาง**
    #[default]
    Linked,
    /// ฝังทุกใบ — สำหรับส่งต่อ/สำรอง/ย้ายเครื่อง
    Packed,
}

/// ★★★ ไบต์ของ asset ใบหนึ่ง **อยู่ที่ไหน และใครเป็นเจ้าของ**
///
/// การแยก [`Self::UserFile`] ออกจาก [`Self::Ours`] คือทั้งหมดของกฎ
/// "linked ก็ฝัง" (ดูหัวโมดูล) — ไฟล์ของผู้ใช้ยังอยู่ของมันเองได้
/// ส่วนไฟล์ที่มีอยู่เพราะ **เราสร้างมันขึ้นมาเอง** จะหายไปพร้อมเครื่องนี้
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetBytes {
    /// ไฟล์ที่ผู้ใช้เป็นเจ้าของ — ลิงก์ได้ใน linked mode
    UserFile(PathBuf),
    /// ★ ของที่ **มีอยู่เพราะเราสร้างไว้** (ภาพจาก clipboard ที่ถูกพักไว้)
    ///
    /// ผู้ใช้ไม่รู้ว่ามันอยู่ตรงนั้น ไม่ได้ตั้งใจเก็บไว้ และลบมันเมื่อไหร่ก็ได้
    /// → **ต้องฝังเสมอ ไม่ว่าโหมดไหน** ไม่งั้นเปิดกลับมาแล้วภาพหาย
    Ours(PathBuf),
    /// หาไบต์ไม่เจอเลย — item จะเป็น `Missing` (relink เป็นเรื่องของ P4-6)
    Missing,
}

/// ★★★ ตัดสินว่า asset ใบไหนต้องถูกฝัง — **กฎอยู่ที่เดียวในโปรเจกต์**
///
/// วางไว้ที่ชั้น io ไม่ใช่ชั้น UI โดยตั้งใจ: นี่คือกฎที่ปกป้อง I-3 ถ้ามันอยู่
/// ในตัวเรียกใช้ ทุกตัวเรียกใหม่ต้องจำให้ได้เอง แล้ววันหนึ่งจะมีตัวที่ลืม
///
/// ★ ผู้เรียกตอบแค่ว่า "ไบต์ของใบนี้อยู่ที่ไหน" ([`AssetBytes`]) —
/// ส่วนที่ว่า *อะไรควรถูกฝัง* ถูกตัดสินที่นี่
#[must_use]
pub fn plan_embeds(
    board: &Board,
    mode: SaveMode,
    locate: impl Fn(&refx_core::board::AssetRef) -> AssetBytes,
) -> Vec<PackSource> {
    let mut plan = Vec::new();
    for (_, item) in board.items_in_z_order() {
        let refx_core::board::ItemKind::Image(asset) = &item.kind else {
            continue;
        };
        // ★ ภาพเดียวกันวางหลายใบบน board = ฝังครั้งเดียว (คีย์คือเนื้อไฟล์)
        if plan.iter().any(|s: &PackSource| s.hash == asset.hash) {
            continue;
        }
        let path = match (locate(asset), mode) {
            // ของเราเอง → ฝังเสมอ ทั้งสองโหมด (นี่คือหัวใจของกฎ)
            (AssetBytes::Ours(path), _) => path,
            // ไฟล์ของผู้ใช้ → ฝังเฉพาะตอน packed
            (AssetBytes::UserFile(path), SaveMode::Packed) => path,
            (AssetBytes::UserFile(_), SaveMode::Linked) | (AssetBytes::Missing, _) => continue,
        };
        plan.push(PackSource {
            hash: asset.hash,
            path,
        });
    }
    plan
}

/// ฝัง/แกะไฟล์ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    /// เอกสารเองแปลงไม่ได้
    #[error(transparent)]
    Document(#[from] dto::SaveError),
    /// asset เยอะเกินเพดาน
    #[error("too many embedded images ({count}, limit {MAX_ASSETS})")]
    TooManyAssets {
        /// จำนวนที่ขอฝัง
        count: usize,
    },
    /// ไฟล์หนึ่งใหญ่เกินเพดาน
    #[error("{} is too large to embed ({size} bytes, limit {MAX_ASSET_BYTES})", path.display())]
    AssetTooLarge {
        /// ไฟล์ที่ใหญ่เกิน
        path: PathBuf,
        /// ขนาดที่วัดได้
        size: u64,
    },
    /// ★ ไฟล์เปลี่ยนขนาดระหว่างที่กำลังฝัง (โดน sync/แก้ไขอยู่)
    ///
    /// ต้องล้มเสียงดัง ไม่ใช่เขียนต่อ — table จะชี้ผิดตำแหน่งทั้งก้อนหลังจากนั้น
    #[error("{} changed while it was being embedded ({expected} → {actual} bytes)", path.display())]
    AssetChanged {
        /// ไฟล์ที่เปลี่ยน
        path: PathBuf,
        /// ขนาดตอน stat
        expected: u64,
        /// ขนาดที่อ่านได้จริง
        actual: u64,
    },
    /// ระบบไฟล์ล้ม — บอกด้วยว่าล้มกับไฟล์ไหน
    #[error("could not embed {}: {source}", path.display())]
    Io {
        /// ไฟล์ที่กำลังแตะตอนนั้น
        path: PathBuf,
        /// ต้นเหตุ
        source: std::io::Error,
    },
}

/// ★★★ เขียนไฟล์ packed แบบ **สตรีม** — ไม่มีจุดไหนที่ถือทั้งไฟล์ไว้ใน RAM
///
/// ต้องการ `Seek` เพราะ **crc ของแต่ละ blob รู้ได้ก็ต่อเมื่ออ่านมันจนจบแล้ว**
/// แต่ตำแหน่งของ table อยู่ *ก่อน* blob ตามรูปแบบไฟล์ · ทางเลือกอื่นคืออ่านทุกไฟล์
/// สองรอบ (รอบแรกเพื่อ crc) ซึ่งบน mood board 10 GB แปลว่าอ่านดิสก์ 20 GB
/// → เขียน table เป็นที่ว่างไว้ก่อน แล้วย้อนกลับมาเติมตอนจบ (หนึ่งรอบเท่านั้น)
///
/// # Errors
/// [`PackError`] — ไฟล์ปลายทางถูกทิ้งโดยผู้เรียก (เขียนลง `.tmp` เสมอ)
pub fn write_packed<W: Write + Seek>(
    out: &mut W,
    board: &Board,
    sources: &[PackSource],
) -> Result<(), PackError> {
    if sources.len() > MAX_ASSETS as usize {
        return Err(PackError::TooManyAssets {
            count: sources.len(),
        });
    }

    let body = dto::encode_body(board)?;
    let table_at = HEADER_LEN as u64 + body.len() as u64;
    let blobs_at = table_at + TABLE_HEADER_LEN as u64 + (sources.len() * ENTRY_LEN) as u64;

    // ---- หัวไฟล์ + document ----
    //
    // ★ ความล้มเหลวของ *ปลายทาง* ไม่มี path ให้ชี้ (เป็น `.tmp` ของผู้เรียก)
    //   จึงใช้ป้ายเดียวกันทั้งหมด — ผู้เรียกรู้อยู่แล้วว่ากำลังเขียนไฟล์ไหน
    let sink = || PathBuf::from("<output>");
    let put = |bytes: &[u8], out: &mut W| -> Result<(), PackError> {
        out.write_all(bytes).map_err(|source| PackError::Io {
            path: sink(),
            source,
        })
    };
    put(&dto::MAGIC, out)?;
    put(&PACKED_VERSION.to_le_bytes(), out)?;
    put(&dto::FLAG_PACKED.to_le_bytes(), out)?;
    put(&(body.len() as u64).to_le_bytes(), out)?;
    put(&crc32fast::hash(&body).to_le_bytes(), out)?;
    put(&body, out)?;

    // ---- ที่ว่างของ table (เติมตอนจบ) ----
    let blank = vec![0u8; TABLE_HEADER_LEN + sources.len() * ENTRY_LEN];
    put(&blank, out)?;

    // ---- สตรีม blob ทีละไฟล์ พร้อมเก็บ crc/ความยาวจริง ----
    let mut entries = Vec::with_capacity(sources.len());
    let mut offset = blobs_at;
    for source in sources {
        let (len, crc) = copy_asset(out, &source.path)?;
        entries.push(Entry {
            hash: source.hash,
            offset,
            len,
            crc,
        });
        offset += len;
    }

    // ---- ย้อนกลับไปเติม table ----
    let mut table = Vec::with_capacity(sources.len() * ENTRY_LEN);
    for entry in &entries {
        table.extend_from_slice(entry.hash.as_bytes());
        table.extend_from_slice(&entry.offset.to_le_bytes());
        table.extend_from_slice(&entry.len.to_le_bytes());
        table.extend_from_slice(&entry.crc.to_le_bytes());
    }
    let seek = |out: &mut W, to: SeekFrom| -> Result<(), PackError> {
        out.seek(to).map(drop).map_err(|source| PackError::Io {
            path: sink(),
            source,
        })
    };
    seek(out, SeekFrom::Start(table_at))?;
    // ★ `count` ปลอดภัยเสมอ — ถูกกันด้วย `MAX_ASSETS` ตั้งแต่บรรทัดแรกของฟังก์ชัน
    let count = u32::try_from(entries.len()).unwrap_or(u32::MAX);
    put(&count.to_le_bytes(), out)?;
    put(&crc32fast::hash(&table).to_le_bytes(), out)?;
    put(&table, out)?;
    seek(out, SeekFrom::End(0))?;
    Ok(())
}

/// สตรีมไฟล์หนึ่งก้อนลงปลายทาง — คืน `(ความยาวจริง, crc)`
///
/// ★ บัฟเฟอร์คงที่ 64 KB · ไม่โตตามขนาดไฟล์ ไม่ว่าไฟล์จะใหญ่แค่ไหน
fn copy_asset<W: Write>(out: &mut W, path: &Path) -> Result<(u64, u32), PackError> {
    let meta = std::fs::metadata(path).map_err(|source| PackError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let declared = meta.len();
    if declared > MAX_ASSET_BYTES {
        return Err(PackError::AssetTooLarge {
            path: path.to_path_buf(),
            size: declared,
        });
    }

    let mut file = std::fs::File::open(path).map_err(|source| PackError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut buf = vec![0u8; 64 << 10];
    let mut hasher = crc32fast::Hasher::new();
    let mut written = 0u64;
    loop {
        let read = file.read(&mut buf).map_err(|source| PackError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        // ★ ไฟล์โตขึ้นระหว่างทาง (โดนเขียนอยู่) — หยุดที่ขนาดที่ประกาศไว้
        //   ไม่งั้น offset ของทุก entry ที่ตามมาจะเลื่อนทั้งก้อน
        if written + read as u64 > declared {
            return Err(PackError::AssetChanged {
                path: path.to_path_buf(),
                expected: declared,
                actual: written + read as u64,
            });
        }
        hasher.update(&buf[..read]);
        out.write_all(&buf[..read])
            .map_err(|source| PackError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        written += read as u64;
    }
    if written != declared {
        // ไฟล์หดลงระหว่างทาง — table จะชี้ผิดเช่นกัน
        return Err(PackError::AssetChanged {
            path: path.to_path_buf(),
            expected: declared,
            actual: written,
        });
    }
    Ok((written, hasher.finalize()))
}

/// หนึ่งแถวใน asset table
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// hash ของเนื้อไฟล์
    pub hash: ContentHash,
    /// ตำแหน่งของ blob นับจากต้นไฟล์
    pub offset: u64,
    /// ความยาวของ blob
    pub len: u64,
    /// crc32 ของ blob
    pub crc: u32,
}

/// asset table ที่อ่านมาแล้ว — **ไม่มีเนื้อ blob อยู่ในนี้**
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index(Vec<Entry>);

impl Index {
    /// รายการทั้งหมด
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.0
    }

    /// ★ hash นี้ถูกฝังไว้ในไฟล์หรือไม่ — **คำตอบของ `AssetRef::embedded`**
    #[must_use]
    pub fn find(&self, hash: ContentHash) -> Option<Entry> {
        self.0.iter().copied().find(|entry| entry.hash == hash)
    }

    /// จำนวน asset ที่ฝังไว้
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// ไม่มี asset ฝังไว้เลย
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// ★★ อ่าน asset table โดย **ไม่แตะ blob เลยสักไบต์**
///
/// ★★★ **ทุกตัวเลขในตารางมาจากไฟล์ จึงโกหกได้ทั้งหมด** (I-4) — ทุก entry ถูก
/// ตรวจว่าอยู่ในขอบเขตไฟล์จริงก่อนถูกคืนออกไป · การปล่อยให้ผู้เรียกไป seek
/// ตามตัวเลขดิบ ๆ แปลว่าไฟล์ที่ถูกดัดแปลงพาเราไปอ่านที่ไหนก็ได้
///
/// # Errors
/// [`OpenError`] เมื่อหัวไฟล์/ตารางไม่สมเหตุสมผล
pub fn read_index<R: Read + Seek>(source: &mut R, file_len: u64) -> Result<Index, OpenError> {
    let mut header = [0u8; HEADER_LEN];
    source
        .seek(SeekFrom::Start(0))
        .and_then(|_| source.read_exact(&mut header))
        .map_err(|_| OpenError::TooShort {
            len: file_len.min(usize::MAX as u64) as usize,
        })?;
    let info = dto::inspect(&header)?;
    if !info.packed {
        return Ok(Index::default()); // linked — ไม่มีตารางให้อ่าน ไม่ใช่ error
    }
    let doc_len = u64::from_le_bytes([
        header[8], header[9], header[10], header[11], header[12], header[13], header[14],
        header[15],
    ]);

    let table_at = (HEADER_LEN as u64)
        .checked_add(doc_len)
        .ok_or(OpenError::Malformed)?;
    let mut table_header = [0u8; TABLE_HEADER_LEN];
    source
        .seek(SeekFrom::Start(table_at))
        .and_then(|_| source.read_exact(&mut table_header))
        .map_err(|_| OpenError::Truncated {
            declared: table_at + TABLE_HEADER_LEN as u64,
            actual: file_len,
        })?;
    let count = u32::from_le_bytes([
        table_header[0],
        table_header[1],
        table_header[2],
        table_header[3],
    ]);
    let table_crc = u32::from_le_bytes([
        table_header[4],
        table_header[5],
        table_header[6],
        table_header[7],
    ]);
    if count > MAX_ASSETS {
        return Err(OpenError::TooLarge {
            size: u64::from(count),
        });
    }

    // ★ จองตามตัวเลขที่ **ผ่านเพดานแล้วเท่านั้น** (กันไฟล์ที่ประกาศ count มหาศาล)
    let table_bytes = count as usize * ENTRY_LEN;
    let mut table = vec![0u8; table_bytes];
    source
        .read_exact(&mut table)
        .map_err(|_| OpenError::Truncated {
            declared: table_at + TABLE_HEADER_LEN as u64 + table_bytes as u64,
            actual: file_len,
        })?;
    if crc32fast::hash(&table) != table_crc {
        return Err(OpenError::Corrupt);
    }

    let blobs_at = table_at + TABLE_HEADER_LEN as u64 + table_bytes as u64;
    let mut entries = Vec::with_capacity(count as usize);
    for row in table.chunks_exact(ENTRY_LEN) {
        let mut raw = [0u8; 32];
        raw.copy_from_slice(&row[..32]);
        let offset = u64::from_le_bytes(row[32..40].try_into().map_err(|_| OpenError::Malformed)?);
        let len = u64::from_le_bytes(row[40..48].try_into().map_err(|_| OpenError::Malformed)?);
        let crc = u32::from_le_bytes(row[48..52].try_into().map_err(|_| OpenError::Malformed)?);

        // ★★★ ด่านที่ทำให้ตัวเลขจากไฟล์ปลอดภัย — ทั้งสามข้อต้องผ่าน
        if len > MAX_ASSET_BYTES {
            return Err(OpenError::TooLarge { size: len });
        }
        let end = offset.checked_add(len).ok_or(OpenError::Malformed)?;
        if offset < blobs_at || end > file_len {
            return Err(OpenError::Malformed);
        }
        entries.push(Entry {
            hash: ContentHash::from_bytes(raw),
            offset,
            len,
            crc,
        });
    }
    Ok(Index(entries))
}

/// ★★ แกะ asset หนึ่งก้อนออกมาแบบ **สตรีม** พร้อมตรวจ crc
///
/// ★ ตรวจ crc **หลังเขียนครบ** แล้วคืน `Corrupt` ถ้าไม่ตรง — ผู้เรียกต้องทิ้ง
/// ของที่เพิ่งเขียนไป · ตรวจก่อนเขียนต้องอ่านสองรอบ ซึ่งบนไฟล์ระดับ GB
/// แปลว่าอ่านดิสก์สองเท่าเพื่อความสะดวกของเราเอง
///
/// # Errors
/// [`OpenError::Corrupt`] เมื่อ checksum ไม่ตรง · [`OpenError::Truncated`] เมื่อไฟล์สั้น
pub fn extract<R: Read + Seek, W: Write>(
    source: &mut R,
    entry: Entry,
    out: &mut W,
) -> Result<(), OpenError> {
    source
        .seek(SeekFrom::Start(entry.offset))
        .map_err(|_| OpenError::Malformed)?;
    let mut left = entry.len;
    let mut buf = vec![0u8; 64 << 10];
    let mut hasher = crc32fast::Hasher::new();
    while left > 0 {
        let want = usize::try_from(left.min(buf.len() as u64)).unwrap_or(buf.len());
        let read = source
            .read(&mut buf[..want])
            .map_err(|_| OpenError::Malformed)?;
        if read == 0 {
            return Err(OpenError::Truncated {
                declared: entry.len,
                actual: entry.len - left,
            });
        }
        hasher.update(&buf[..read]);
        out.write_all(&buf[..read])
            .map_err(|_| OpenError::Malformed)?;
        left -= read as u64;
    }
    if hasher.finalize() != entry.crc {
        return Err(OpenError::Corrupt);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use refx_core::arena::{ArenaKey as _, BoardId};
    use refx_core::board::{AssetRef, BoardParts, ImageFormat, Item, ItemKind, ItemParts};

    fn board_id() -> BoardId {
        BoardId::from_parts(0, 0)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-packed-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// อ่านไฟล์ทั้งก้อนในเทสต์ — `std::fs::read` ถูกแบนใน `clippy.toml`
    ///
    /// ★ ข้อห้ามนั้นมีไว้กับ **เส้นทางจริง** (ต้องมีเพดาน + อยู่บน worker) ·
    /// ในเทสต์เราคุมไฟล์เองอยู่แล้ว · ★★ และมันจับผมได้จริงตอนเขียนเทสต์ชุดนี้
    /// ซึ่งเป็นเหตุผลที่ข้อห้ามนั้นถูกตั้งไว้แต่แรก
    fn read_all(path: &Path) -> Vec<u8> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    }

    fn hash_of(n: u8) -> ContentHash {
        ContentHash::from_bytes([n; 32])
    }

    /// เขียนไฟล์ภาพปลอมที่มีเนื้อต่างกันจริง
    fn plant_image(dir: &Path, n: u8, bytes: usize) -> PathBuf {
        let path = dir.join(format!("img{n}.bin"));
        let body: Vec<u8> = (0..bytes).map(|i| (i as u8).wrapping_mul(n | 1)).collect();
        std::fs::write(&path, body).unwrap();
        path
    }

    fn board_of(paths: &[(u8, &Path)]) -> Board {
        Board::load(
            board_id(),
            BoardParts {
                name: "packed".to_owned(),
                items: paths
                    .iter()
                    .map(|(n, path)| ItemParts {
                        item: Item::new(ItemKind::Image(AssetRef {
                            hash: hash_of(*n),
                            path: path.to_path_buf(),
                            px_size: glam::UVec2::new(64, 64),
                            format: ImageFormat::Unknown,
                            embedded: false,
                            mtime: 0,
                            file_size: 0,
                        })),
                        group: None,
                    })
                    .collect(),
                ..BoardParts::default()
            },
        )
    }

    fn write_to_file(dir: &Path, board: &Board, sources: &[PackSource]) -> PathBuf {
        let out = dir.join("packed.refx");
        let mut file = std::fs::File::create(&out).unwrap();
        write_packed(&mut file, board, sources).unwrap();
        file.sync_all().unwrap();
        out
    }

    fn open(path: &Path) -> (std::fs::File, u64) {
        let file = std::fs::File::open(path).unwrap();
        let len = file.metadata().unwrap().len();
        (file, len)
    }

    // ---------- ★★★ round-trip ----------

    /// ★★★ **ฝังภาพแล้วแกะกลับมาต้องได้ไบต์เดิมเป๊ะ** — เกณฑ์หลักของ packed
    #[test]
    fn embedded_images_come_back_byte_for_byte() {
        let dir = temp_dir("roundtrip");
        let a = plant_image(&dir, 1, 5_000);
        let b = plant_image(&dir, 2, 130_000); // ใหญ่กว่าบัฟเฟอร์ 64 KB หลายรอบ
        let board = board_of(&[(1, &a), (2, &b)]);
        let sources = vec![
            PackSource {
                hash: hash_of(1),
                path: a.clone(),
            },
            PackSource {
                hash: hash_of(2),
                path: b.clone(),
            },
        ];

        let out = write_to_file(&dir, &board, &sources);
        let (mut file, len) = open(&out);
        let index = read_index(&mut file, len).unwrap();
        assert_eq!(index.len(), 2);

        for (n, original) in [(1u8, &a), (2u8, &b)] {
            let entry = index.find(hash_of(n)).expect("ต้องเจอ asset ที่ฝังไว้");
            let mut got = Vec::new();
            extract(&mut file, entry, &mut got).unwrap();
            let want = read_all(original);
            assert_eq!(got, want, "ภาพที่ {n} แกะกลับมาแล้วไม่เหมือนเดิม");
        }
    }

    /// เอกสารข้างในต้องอ่านได้ด้วย `dto::decode` ตัวเดิม (แค่ตัด blob ทิ้ง)
    #[test]
    fn the_document_inside_a_packed_file_still_reads() {
        let dir = temp_dir("doc");
        let a = plant_image(&dir, 3, 1_000);
        let board = board_of(&[(3, &a)]);
        let out = write_to_file(
            &dir,
            &board,
            &[PackSource {
                hash: hash_of(3),
                path: a,
            }],
        );

        let bytes = read_all(&out);
        let info = dto::inspect(&bytes).unwrap();
        assert!(info.packed, "flag packed ต้องติด");
        assert_eq!(info.version, PACKED_VERSION);
        assert_eq!(dto::decode(&bytes, board_id()).unwrap(), board);
    }

    /// ★★★ **รุ่นที่เข้าใจแค่ v1 ต้องปฏิเสธทั้งการเปิดและการเขียนทับ**
    ///
    /// นี่คือเหตุผลทั้งหมดที่ packed เป็น v2 (ดูหัวโมดูล): รุ่นเก่าที่เปิดได้
    /// จะเห็นภาพเป็น `Missing` แล้วถ้าผู้ใช้ save ทับ **ภาพต้นฉบับที่ฝังไว้
    /// หายถาวรทั้งหมด** ซึ่ง `docs/07 §2` ห้ามไว้ตรง ๆ (I-3)
    #[test]
    fn an_older_build_refuses_a_packed_file_instead_of_destroying_it() {
        let dir = temp_dir("v2gate");
        let a = plant_image(&dir, 4, 2_048);
        let board = board_of(&[(4, &a)]);
        let out = write_to_file(
            &dir,
            &board,
            &[PackSource {
                hash: hash_of(4),
                path: a,
            }],
        );
        let bytes = read_all(&out);

        // จำลอง "รุ่นที่เข้าใจแค่ v1" ด้วยการอ่านหัวไฟล์ตามกฎเดียวกับที่ `inspect` ใช้
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        assert_eq!(version, 2, "ไฟล์ packed ต้องประกาศ v2");
        assert!(
            version > 1,
            "รุ่น v1 ต้องเห็นว่านี่ใหม่กว่าตัวเอง จึงปฏิเสธทั้งเปิดและเขียนทับ"
        );

        // ★ negative control: ไฟล์ linked ยังเป็น v1 — รุ่นเก่ายังเปิดงานประจำวันได้
        let linked = dto::encode(&board).unwrap();
        assert_eq!(
            u16::from_le_bytes([linked[4], linked[5]]),
            1,
            "ไฟล์ linked ถูกดันขึ้น v2 ไปด้วย — รุ่นเก่าจะเปิดงานประจำวันไม่ได้ทั้งที่ไม่จำเป็น"
        );
    }

    /// ★★ `embedded` **อนุมานจากตาราง ไม่ได้เก็บในไฟล์** (ดูหัวโมดูล)
    #[test]
    fn whether_an_asset_is_embedded_is_answered_by_the_table() {
        let dir = temp_dir("embedded");
        let a = plant_image(&dir, 5, 700);
        let board = board_of(&[(5, &a), (6, Path::new("C:/ที่ไม่มีอยู่จริง.jpg"))]);
        let out = write_to_file(
            &dir,
            &board,
            &[PackSource {
                hash: hash_of(5),
                path: a,
            }],
        );

        let (mut file, len) = open(&out);
        let index = read_index(&mut file, len).unwrap();
        assert!(index.find(hash_of(5)).is_some(), "ตัวที่ฝังต้องเจอ");
        assert!(
            index.find(hash_of(6)).is_none(),
            "ตัวที่ไม่ได้ฝังต้องไม่เจอ — ไม่งั้น `embedded` จะโกหก"
        );
    }

    /// ไฟล์ linked ไม่มีตาราง — ต้องคืนตารางว่าง ไม่ใช่ error
    #[test]
    fn a_linked_file_simply_has_no_table() {
        let dir = temp_dir("linked");
        let out = dir.join("linked.refx");
        std::fs::write(&out, dto::encode(&Board::default()).unwrap()).unwrap();
        let (mut file, len) = open(&out);
        assert!(read_index(&mut file, len).unwrap().is_empty());
    }

    // ---------- ★★★ กฎ "linked ก็ฝัง" ----------

    /// ★★★ **ภาพที่วางจาก clipboard ต้องถูกฝังแม้ในโหมด linked**
    ///
    /// ถ้าไม่ฝัง `AssetRef` จะชี้ไปที่ที่ผู้ใช้ไม่รู้ว่ามีอยู่และลบเมื่อไหร่ก็ได้
    /// → เปิดกลับมาได้ `Missing` → **ภาพหายถาวร** = I-3 ถูกละเมิดแบบเงียบที่สุด
    #[test]
    fn a_pasted_image_is_embedded_even_in_linked_mode() {
        let dir = temp_dir("linked-embed");
        let mine = plant_image(&dir, 20, 900); // ไฟล์ของผู้ใช้
        let ours = plant_image(&dir, 21, 800); // ของที่เราพักไว้เอง (clipboard)
        let board = board_of(&[(20, &mine), (21, &ours)]);

        let plan = plan_embeds(&board, SaveMode::Linked, |asset| {
            if asset.hash == hash_of(21) {
                AssetBytes::Ours(ours.clone())
            } else {
                AssetBytes::UserFile(mine.clone())
            }
        });

        assert_eq!(plan.len(), 1, "linked ต้องฝังเฉพาะใบที่ไม่มีไฟล์ต้นทาง");
        assert_eq!(plan[0].hash, hash_of(21), "ฝังผิดใบ");
    }

    /// ★ board ที่ไม่มีภาพวางเลย → linked → **ไม่ฝังอะไร → ไฟล์ยังเป็น v1**
    ///
    /// ยืนยันจาก **ไบต์ในไฟล์** ไม่ใช่จากค่าที่เราตั้งเอง — รุ่นเก่าต้องยังเปิด
    /// งานประจำวันได้ ไฟล์ที่ไม่มีอะไรฝังอยู่ไม่มีเหตุให้ตัดเขาออก
    #[test]
    fn a_board_with_nothing_to_embed_stays_v1() {
        let dir = temp_dir("stays-v1");
        let mine = plant_image(&dir, 22, 1_200);
        let board = board_of(&[(22, &mine)]);
        let plan = plan_embeds(&board, SaveMode::Linked, |_| {
            AssetBytes::UserFile(mine.clone())
        });
        assert!(plan.is_empty());

        let doc = dir.join("work.refx");
        crate::save::save_document(&doc, &board, &plan, refx_platform::fsops::rename_durable)
            .unwrap();

        let bytes = read_all(&doc);
        assert_eq!(
            u16::from_le_bytes([bytes[4], bytes[5]]),
            1,
            "ไฟล์ที่ไม่มี asset ฝังอยู่ ถูกดันขึ้น v2 โดยไม่จำเป็น"
        );
        assert!(!dto::inspect(&bytes).unwrap().packed);
    }

    /// ★★★ negative control ของข้อบน — **มีของฝังเมื่อไหร่ ต้องเป็น v2 ทันที**
    #[test]
    fn a_board_with_something_embedded_becomes_v2() {
        let dir = temp_dir("becomes-v2");
        let ours = plant_image(&dir, 23, 640);
        let board = board_of(&[(23, &ours)]);
        let plan = plan_embeds(&board, SaveMode::Linked, |_| AssetBytes::Ours(ours.clone()));
        assert_eq!(plan.len(), 1);

        let doc = dir.join("work.refx");
        crate::save::save_document(&doc, &board, &plan, refx_platform::fsops::rename_durable)
            .unwrap();

        let bytes = read_all(&doc);
        assert_eq!(
            u16::from_le_bytes([bytes[4], bytes[5]]),
            PACKED_VERSION,
            "ไฟล์ที่มี blob จริงอยู่ข้างในต้องประกาศ v2 ให้รุ่นเก่าปฏิเสธ"
        );
    }

    /// ★★★ **packed → เปิดใหม่ได้ภาพครบ แม้ลบโฟลเดอร์ต้นฉบับไปแล้ว**
    ///
    /// นี่คือเหตุผลทั้งหมดที่ packed มีอยู่ (`docs/07 §2`: "ส่งให้คนอื่น /
    /// ย้ายเครื่อง") — ถ้ายังต้องมีไฟล์ต้นฉบับอยู่ มันก็ไม่ต่างจาก linked
    #[test]
    fn a_packed_file_survives_losing_every_original() {
        let dir = temp_dir("survives");
        let src = dir.join("originals");
        std::fs::create_dir_all(&src).unwrap();
        let a = plant_image(&src, 24, 3_000);
        let b = plant_image(&src, 25, 70_000);
        let want_a = read_all(&a);
        let want_b = read_all(&b);
        let board = board_of(&[(24, &a), (25, &b)]);

        let plan = plan_embeds(&board, SaveMode::Packed, |asset| {
            AssetBytes::UserFile(if asset.hash == hash_of(24) {
                a.clone()
            } else {
                b.clone()
            })
        });
        assert_eq!(plan.len(), 2, "packed ต้องฝังทุกใบ");
        let doc = dir.join("packed.refx");
        crate::save::save_document(&doc, &board, &plan, refx_platform::fsops::rename_durable)
            .unwrap();

        // ★ ลบต้นฉบับทิ้งทั้งโฟลเดอร์ — เหมือนส่งไฟล์ไปเครื่องอื่น
        std::fs::remove_dir_all(&src).unwrap();
        assert!(!a.exists() && !b.exists());

        let (mut file, len) = open(&doc);
        let index = read_index(&mut file, len).unwrap();
        for (n, want) in [(24u8, &want_a), (25u8, &want_b)] {
            let entry = index.find(hash_of(n)).expect("ต้องเจอใน asset table");
            let mut got = Vec::new();
            extract(&mut file, entry, &mut got).unwrap();
            assert_eq!(&got, want, "ภาพที่ {n} ไม่ครบหลังลบต้นฉบับ");
        }
        assert_eq!(dto::decode(&read_all(&doc), board_id()).unwrap(), board);
    }

    /// ★★ linked → packed → linked แล้วต้องได้ **board เท่าเดิม**
    ///
    /// การแปลงโหมดเป็นเรื่องของ *ที่เก็บไบต์* ไม่ใช่ของเนื้อหา — ถ้าเนื้อหาเปลี่ยน
    /// แปลว่ามีอะไรรั่วจากชั้นเก็บขึ้นมาถึงชั้นเอกสาร
    #[test]
    fn converting_between_modes_never_changes_the_board() {
        let dir = temp_dir("convert");
        let a = plant_image(&dir, 26, 2_500);
        let board = board_of(&[(26, &a)]);
        let rename = refx_platform::fsops::rename_durable;

        let linked = dir.join("a.refx");
        let plan = plan_embeds(&board, SaveMode::Linked, |_| {
            AssetBytes::UserFile(a.clone())
        });
        crate::save::save_document(&linked, &board, &plan, rename).unwrap();
        let after_linked = dto::decode(&read_all(&linked), board_id()).unwrap();

        let packed = dir.join("b.refx");
        let plan = plan_embeds(&after_linked, SaveMode::Packed, |_| {
            AssetBytes::UserFile(a.clone())
        });
        crate::save::save_document(&packed, &after_linked, &plan, rename).unwrap();
        let after_packed = dto::decode(&read_all(&packed), board_id()).unwrap();

        let back = dir.join("c.refx");
        let plan = plan_embeds(&after_packed, SaveMode::Linked, |_| {
            AssetBytes::UserFile(a.clone())
        });
        crate::save::save_document(&back, &after_packed, &plan, rename).unwrap();
        let after_back = dto::decode(&read_all(&back), board_id()).unwrap();

        assert_eq!(after_linked, board, "linked เปลี่ยนเนื้อหา");
        assert_eq!(after_packed, board, "packed เปลี่ยนเนื้อหา");
        assert_eq!(after_back, board, "แปลงกลับแล้วไม่เท่าเดิม");
        // ★ และเวอร์ชันเดินตามของที่อยู่ข้างในจริง ๆ
        let ver = |path: &Path| {
            let b = read_all(path);
            u16::from_le_bytes([b[4], b[5]])
        };
        assert_eq!(ver(&linked), 1);
        assert_eq!(ver(&packed), PACKED_VERSION);
        assert_eq!(ver(&back), 1);
    }

    /// ★ ภาพเดียวกันวางหลายใบบน board = ฝังครั้งเดียว (คีย์คือเนื้อไฟล์)
    #[test]
    fn the_same_image_used_twice_is_embedded_once() {
        let dir = temp_dir("dedup");
        let a = plant_image(&dir, 27, 512);
        let board = board_of(&[(27, &a), (27, &a), (27, &a)]);
        let plan = plan_embeds(&board, SaveMode::Packed, |_| {
            AssetBytes::UserFile(a.clone())
        });
        assert_eq!(plan.len(), 1, "ฝังซ้ำ {} ครั้ง", plan.len());
    }

    /// ★ ใบที่หาไบต์ไม่เจอเลยต้องไม่ทำให้การบันทึกล้ม — มันเป็น `Missing`
    /// ซึ่ง `docs/07 §2` บอกว่าต้อง save กลับได้ครบ ไม่ใช่ทำให้บันทึกไม่ได้
    #[test]
    fn an_image_with_no_bytes_anywhere_does_not_block_saving() {
        let dir = temp_dir("missing");
        let board = board_of(&[(28, Path::new("C:/หายไปแล้ว.jpg"))]);
        let plan = plan_embeds(&board, SaveMode::Packed, |_| AssetBytes::Missing);
        assert!(plan.is_empty());

        let doc = dir.join("work.refx");
        crate::save::save_document(&doc, &board, &plan, refx_platform::fsops::rename_durable)
            .unwrap();
        assert_eq!(dto::decode(&read_all(&doc), board_id()).unwrap(), board);
    }

    // ---------- ★★★ I-4: ทุกตัวเลขในไฟล์โกหกได้ ----------

    /// ★★★ **entry ที่ชี้ออกนอกไฟล์ต้องถูกปฏิเสธ ไม่ใช่พาเราไป seek ตามมัน**
    ///
    /// ★★ **ต้องคำนวณ `table_crc` ใหม่หลังแก้ด้วย** ไม่งั้นไฟล์จะถูกปฏิเสธที่ด่าน
    /// checksum ก่อนถึงด่านขอบเขต แล้วเทสต์จะเขียวโดยพิสูจน์คนละเรื่องกับชื่อของมัน
    ///
    /// รุ่นแรกของเทสต์นี้พลาดตรงนั้นจริง — **negative control จับได้**: ถอดด่าน
    /// ขอบเขตออกแล้วมันยัง**เขียว** เพราะ crc เป็นตัวที่ล้มอยู่ · นี่คือรูปแบบ
    /// "assertion ที่อ่อนกว่าสัญญา" ของ `HANDOFF §4` ข้อ 21 เป๊ะ ๆ
    ///
    /// ไฟล์ที่ถูกประกอบมาอย่างตั้งใจ (crc ตรง แต่ตัวเลขชี้ออกนอกไฟล์) คือสิ่งที่
    /// I-4 พูดถึงจริง ๆ — ไม่ใช่ไฟล์ที่บิตพลิกเพราะดิสก์เสีย
    #[test]
    fn an_entry_pointing_outside_the_file_is_rejected() {
        let dir = temp_dir("oob");
        let a = plant_image(&dir, 7, 1_500);
        let board = board_of(&[(7, &a)]);
        let out = write_to_file(
            &dir,
            &board,
            &[PackSource {
                hash: hash_of(7),
                path: a,
            }],
        );

        let mut bytes = read_all(&out);
        let doc_len = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
        let table_at = HEADER_LEN + doc_len;
        let entry_at = table_at + TABLE_HEADER_LEN;

        for (label, offset, len) in [
            // ชี้เลยท้ายไฟล์ไปไกล ๆ
            ("offset นอกไฟล์", u64::MAX / 2, 16u64),
            // ความยาวลากเลยท้ายไฟล์
            ("len เลยท้ายไฟล์", 0, u64::MAX / 2),
            // ชี้ย้อนกลับเข้าไปทับ header/document
            ("offset ทับหัวไฟล์", 0, 8),
        ] {
            bytes[entry_at + 32..entry_at + 40].copy_from_slice(&offset.to_le_bytes());
            bytes[entry_at + 40..entry_at + 48].copy_from_slice(&len.to_le_bytes());
            // ★ ประกอบ crc ใหม่ให้ตรง — ไฟล์นี้ "ถูกต้อง" ทุกอย่างยกเว้นเจตนา
            let table_end = entry_at + ENTRY_LEN;
            let fixed = crc32fast::hash(&bytes[entry_at..table_end]);
            bytes[table_at + 4..table_at + 8].copy_from_slice(&fixed.to_le_bytes());

            let broken = dir.join("crafted.refx");
            std::fs::write(&broken, &bytes).unwrap();
            let (mut file, file_len) = open(&broken);
            let err = read_index(&mut file, file_len).unwrap_err();
            assert!(
                matches!(err, OpenError::Malformed | OpenError::TooLarge { .. }),
                "{label}: ต้องถูกปฏิเสธด้วยด่านขอบเขต แต่ได้ {err}"
            );
        }
    }

    /// ★ blob ที่ถูกแก้ไบต์ต้องถูกจับได้ตอนแกะ — ไม่ใช่คืนภาพที่เพี้ยน
    #[test]
    fn a_tampered_blob_is_caught_by_its_checksum() {
        let dir = temp_dir("tamper");
        let a = plant_image(&dir, 8, 4_096);
        let board = board_of(&[(8, &a)]);
        let out = write_to_file(
            &dir,
            &board,
            &[PackSource {
                hash: hash_of(8),
                path: a,
            }],
        );

        let mut bytes = read_all(&out);
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        let broken = dir.join("tampered.refx");
        std::fs::write(&broken, &bytes).unwrap();

        let (mut file, len) = open(&broken);
        let index = read_index(&mut file, len).unwrap();
        let entry = index.find(hash_of(8)).unwrap();
        let mut got = Vec::new();
        assert!(
            matches!(extract(&mut file, entry, &mut got), Err(OpenError::Corrupt)),
            "blob ที่ถูกแก้ไบต์หลุดด่าน checksum"
        );
    }

    /// ★ ตารางที่ถูกแก้ต้องถูกจับด้วย `table_crc` — คนละตัวกับ `doc_crc`
    ///
    /// สองอันนี้ต้องแยกกัน ไม่งั้นแยกไม่ออกว่า "ตารางเสีย" กับ "เอกสารเสีย"
    /// ซึ่งเป็นคนละข้อความสำหรับผู้ใช้ (เอกสารเสีย = งานหาย · ตารางเสีย = ภาพหาย)
    #[test]
    fn a_tampered_table_is_caught_separately_from_the_document() {
        let dir = temp_dir("table");
        let a = plant_image(&dir, 9, 900);
        let board = board_of(&[(9, &a)]);
        let out = write_to_file(
            &dir,
            &board,
            &[PackSource {
                hash: hash_of(9),
                path: a,
            }],
        );

        let mut bytes = read_all(&out);
        let doc_len = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
        let entry_at = HEADER_LEN + doc_len + TABLE_HEADER_LEN;
        bytes[entry_at] ^= 0x01; // พลิกไบต์แรกของ hash
        let broken = dir.join("badtable.refx");
        std::fs::write(&broken, &bytes).unwrap();

        let (mut file, len) = open(&broken);
        assert!(matches!(
            read_index(&mut file, len),
            Err(OpenError::Corrupt)
        ));
        // ★ แต่ **เอกสารยังอ่านได้** — งานของผู้ใช้ไม่ได้หายไปด้วย
        assert!(dto::decode(&bytes, board_id()).is_ok());
    }

    /// ★★ `count` ที่โกหกเป็นเลขมหาศาลต้องไม่พาไปจอง RAM ก้อนใหญ่
    #[test]
    fn a_lying_count_never_reserves_a_huge_buffer() {
        let dir = temp_dir("count");
        let a = plant_image(&dir, 10, 512);
        let board = board_of(&[(10, &a)]);
        let out = write_to_file(
            &dir,
            &board,
            &[PackSource {
                hash: hash_of(10),
                path: a,
            }],
        );

        let mut bytes = read_all(&out);
        let doc_len = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
        let table_at = HEADER_LEN + doc_len;
        bytes[table_at..table_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let broken = dir.join("bigcount.refx");
        std::fs::write(&broken, &bytes).unwrap();

        let (mut file, len) = open(&broken);
        assert!(matches!(
            read_index(&mut file, len),
            Err(OpenError::TooLarge { .. })
        ));
    }

    /// ★ เพดานจำนวน asset ต้องเป็นด่านจริง — assert ด้วย **ค่าคงของสัญญา**
    /// ไม่ใช่เลขที่คัดมาเอง (HANDOFF §4 ข้อ 21)
    #[test]
    fn too_many_assets_is_refused_before_anything_is_written() {
        let dir = temp_dir("many");
        let sources: Vec<PackSource> = (0..=MAX_ASSETS as usize)
            .map(|i| PackSource {
                hash: hash_of((i % 251) as u8),
                path: dir.join("nope.bin"),
            })
            .collect();
        let mut sink = std::io::Cursor::new(Vec::new());
        let err = write_packed(&mut sink, &Board::default(), &sources).unwrap_err();
        assert!(matches!(err, PackError::TooManyAssets { .. }), "{err}");
        assert!(
            sink.into_inner().is_empty(),
            "ปฏิเสธแล้วต้องไม่มีไบต์ไหนถูกเขียนออกไปเลย"
        );
    }

    /// ★ ไม่มี asset เลย = ไฟล์ packed ที่มีตารางว่าง (ไม่ใช่ error)
    #[test]
    fn packing_nothing_gives_an_empty_table() {
        let dir = temp_dir("empty");
        let out = write_to_file(&dir, &Board::default(), &[]);
        let (mut file, len) = open(&out);
        let index = read_index(&mut file, len).unwrap();
        assert!(index.is_empty());
        assert!(dto::inspect(&read_all(&out)).unwrap().packed);
    }

    /// ★★★ **ราคาที่วัดได้: RAM ที่ใช้ต้องไม่โตตามขนาดไฟล์**
    ///
    /// นี่คือข้อที่ `docs/07` เตือนไว้ว่าไฟล์ packed ใหญ่ระดับ GB ได้ ·
    /// เทสต์นี้ฝังไฟล์ 8 MB แล้ววัดว่า **บัฟเฟอร์คงที่** ถูกใช้จริง โดยนับจำนวน
    /// ครั้งที่เขียนลงปลายทางแทนการวัด RAM (ซึ่งวัดข้ามเครื่องไม่ได้ — §3.9 ข้อ 5b)
    #[test]
    fn writing_a_big_asset_streams_instead_of_buffering_it() {
        /// ปลายทางที่นับว่าถูกเขียนกี่ครั้ง และก้อนใหญ่สุดเท่าไร
        struct Counting {
            chunks: usize,
            largest: usize,
            total: u64,
            cursor: std::io::Cursor<Vec<u8>>,
        }
        impl Write for Counting {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.chunks += 1;
                self.largest = self.largest.max(buf.len());
                self.total += buf.len() as u64;
                self.cursor.write(buf)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.cursor.flush()
            }
        }
        impl Seek for Counting {
            fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
                self.cursor.seek(pos)
            }
        }

        let dir = temp_dir("stream");
        let big = plant_image(&dir, 11, 8 << 20); // 8 MB
        let board = board_of(&[(11, &big)]);
        let mut out = Counting {
            chunks: 0,
            largest: 0,
            total: 0,
            cursor: std::io::Cursor::new(Vec::new()),
        };
        write_packed(
            &mut out,
            &board,
            &[PackSource {
                hash: hash_of(11),
                path: big,
            }],
        )
        .unwrap();

        println!(
            "ฝังไฟล์ 8 MB: เขียน {} ครั้ง · ก้อนใหญ่สุด {} ไบต์ · รวม {} ไบต์",
            out.chunks, out.largest, out.total
        );
        // ★ ก้อนใหญ่สุดต้องเป็นบัฟเฟอร์คงที่ ไม่ใช่ขนาดไฟล์
        assert!(
            out.largest <= 64 << 10,
            "เขียนทีเดียว {} ไบต์ — แปลว่าถือทั้งไฟล์ไว้ใน RAM",
            out.largest
        );
        assert!(out.chunks > 100, "ไฟล์ 8 MB ต้องถูกสตรีมเป็นหลายก้อน");
    }
}
