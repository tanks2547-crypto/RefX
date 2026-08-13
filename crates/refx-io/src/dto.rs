//! `.refx` format v1 — DTO ที่ **แยกจากชนิดใน `refx-core` เสมอ** (P4-1)
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ magic     "REFX"          4 B            │
//! │ version   u16 = 1         2 B            │
//! │ flags     u16             2 B  (bit0 = packed)
//! │ doc_len   u64             8 B            │
//! │ doc_crc   u32             4 B  (crc32 ของ document ที่บีบแล้ว)
//! ├──────────────────────────────────────────┤
//! │ document  (postcard บีบด้วย zstd -3)      │
//! └──────────────────────────────────────────┘
//! ```
//!
//! ★★★ **ทำไม DTO ต้องแยกจาก `refx-core`** (docs/07 §3)
//!
//! ถ้า serialize ชนิดของโดเมนตรง ๆ การ refactor ภายใน (เปลี่ยนชื่อฟิลด์, สลับ
//! ชนิด, ยุบ struct) จะทำให้**ไฟล์เก่าของผู้ใช้เปิดไม่ได้** โดยที่ไม่มีใคร
//! ตั้งใจเปลี่ยน format เลย · แยกแล้วคอมไพเลอร์จะบังคับให้แก้ตัวแปลงตรงนี้
//! ซึ่งเป็นจุดเดียวที่มีคนคิดเรื่องความเข้ากันได้อยู่แล้ว
//!
//! ★★ **สิ่งที่ v1 จงใจ *ไม่* เขียนลงไฟล์** — "อย่าบันทึกฟิลด์ที่ไม่มีความหมาย"
//!
//! | ฟิลด์ | ทำไมไม่เขียน |
//! |---|---|
//! | `AssetRef::format` | **เป็น `ImageFormat::Unknown` เสมอทุกเส้นทาง** (หนี้ §6 — ยังไม่มีใครร้อยค่าจริงจาก `image::guess_format` ผ่าน decode → cache) เขียนลงไปตอนนี้ = ทุกไฟล์ที่สร้างตั้งแต่วันนี้ถือค่าที่ไม่มีความหมายไปตลอด · ต่อค่าจริงเมื่อไหร่ค่อยเพิ่มใน v2 ซึ่งเป็นสิ่งที่ versioning มีไว้ทำ |
//! | `AssetRef::embedded` | packed mode คือ P4-5 · ในโหมด linked มันเป็น `false` เสมอ และ **บิต `packed` ในหัวไฟล์บอกเรื่องนี้อยู่แล้ว** เก็บสองที่ = drift |
//! | `ArrangeState` (`sort` / `descending`) | P3-4 ตัดสินให้การเรียงอยู่ **ชั้น UI** (§2.17) ไม่มีใครเขียนลง `Board` เลย · P4-1 จึงตอบคำถามที่ค้างไว้ว่า **ไม่ persist ใน v1** |
//! | `Board::id` · `dirty` · `revision` | id เป็นคีย์ของ workspace (P4-7) · `dirty` = "ยังไม่บันทึก" ซึ่งไฟล์ที่บันทึกแล้วเป็น `false` เสมอโดยนิยาม · `revision` เป็นของ runtime |
//!
//! ★ **`selection` ไม่อยู่ที่นี่** เพราะมันไม่อยู่ใน `Board` ตั้งแต่ต้น (docs/02 §2.9)
//!
//! spec: docs/07-file-format.md §1 §3, docs/02-data-model.md §2.9, ROADMAP P4-1

use refx_core::arena::BoardId;
use refx_core::board::{
    AssetRef, Board, BoardParts, BoardSettings, ColorLabel, CropRect, Flip, Group, ImageFormat,
    Item, ItemCanvas, ItemFilter, ItemKind, ItemMeta, ItemParts, MissingReason, TagId, TextNote,
};
use refx_core::hash::ContentHash;
use refx_core::view::{Camera, Mode, ViewState};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ค่าคงที่ของ format — **สัญญาถาวร**
// ---------------------------------------------------------------------------

/// ลายเซ็นหัวไฟล์
pub const MAGIC: [u8; 4] = *b"REFX";

/// เวอร์ชันที่รุ่นนี้ **เขียน** และเป็นเพดานของสิ่งที่รุ่นนี้ **อ่าน** ได้
///
/// ★ นี่คือ *major* version ตามความหมายของ `docs/02 §2.9` — การเพิ่ม/ถอดฟิลด์
/// ใด ๆ ต้องบวกเลขนี้ เพราะ postcard **ไม่ self-describing**: ไฟล์ที่มีฟิลด์
/// เกินมาหนึ่งตัวจะถูกอ่านเพี้ยนทั้งก้อนโดยไม่มีอะไรฟ้อง
pub const FORMAT_VERSION: u16 = 1;

/// bit0 ของ `flags` — เอกสารนี้ฝังไฟล์ภาพไว้ด้วย (packed mode · P4-5)
pub const FLAG_PACKED: u16 = 1 << 0;

/// ขนาดหัวไฟล์เป็นไบต์
pub const HEADER_LEN: usize = 4 + 2 + 2 + 8 + 4;

/// เพดานของ document **หลังคลายบีบ** (I-4 + I-6)
///
/// ★★ ไฟล์ที่ถูกดัดแปลงบอกขนาดเท่าไหร่ก็ได้ · zstd ขยายข้อมูลซ้ำ ๆ ได้เป็นพันเท่า
/// (zip bomb) ถ้าไม่มีเพดาน ไฟล์ 2 KB ทำให้จอง RAM หลาย GB ได้ = โปรแกรมตาย
/// ก่อนจะได้ตรวจอะไรเลย · board ที่ใหญ่ที่สุดที่รองรับ (3,072 ใบ) กิน ~2 MB
/// เพดานนี้จึงกว้างกว่าของจริงหลายสิบเท่าแล้ว
pub const MAX_DOCUMENT_BYTES: usize = 64 << 20;

/// เพดานของ document **ก่อนคลายบีบ** — กันไม่ให้ `doc_len` ที่โกหกพาไปจองก้อนใหญ่
pub const MAX_COMPRESSED_BYTES: u64 = MAX_DOCUMENT_BYTES as u64;

/// ระดับการบีบ (docs/07 §1)
const ZSTD_LEVEL: i32 = 3;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// เปิดไฟล์ `.refx` ไม่สำเร็จ
///
/// ★★ **แยก "ไฟล์เสีย" ออกจาก "ไฟล์รุ่นใหม่กว่า" ให้ขาด** (docs/07 §1) —
/// สองอย่างนี้เป็นคนละข้อความสำหรับผู้ใช้โดยสิ้นเชิง: อันแรกบอกให้ไปหาไฟล์สำรอง
/// อันหลังบอกให้อัปเดตโปรแกรม · ผู้ใช้ที่ไฟล์งานเปิดไม่ขึ้นต้องรู้ให้ชัดว่าเกิดอะไร
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    /// สั้นกว่าหัวไฟล์ — ไม่ใช่ `.refx` แน่นอน
    #[error("file is shorter than a .refx header ({len} < {HEADER_LEN} bytes)")]
    TooShort {
        /// ขนาดที่อ่านได้
        len: usize,
    },
    /// ไม่มีลายเซ็น `REFX`
    #[error("not a .refx file (bad magic)")]
    NotRefx,
    /// ★ ไฟล์จากรุ่นใหม่กว่า — **ห้ามเดา ห้ามบันทึกทับ** (docs/07 §3)
    #[error("made by a newer RefX (file format v{found}, this build understands v{understood})")]
    NewerVersion {
        /// เวอร์ชันที่อยู่ในไฟล์
        found: u16,
        /// เวอร์ชันสูงสุดที่รุ่นนี้อ่านได้
        understood: u16,
    },
    /// ความยาวที่หัวไฟล์บอก ไม่ตรงกับไบต์ที่มีจริง
    #[error("truncated document (header says {declared} bytes, {actual} present)")]
    Truncated {
        /// ค่าที่หัวไฟล์ประกาศ
        declared: u64,
        /// ไบต์ที่มีจริง
        actual: u64,
    },
    /// checksum ไม่ตรง — ไฟล์เสียหายจากดิสก์
    #[error("document is damaged (checksum mismatch)")]
    Corrupt,
    /// ใหญ่เกินเพดาน — กัน zip bomb
    #[error("document is too large ({size} bytes, limit {MAX_DOCUMENT_BYTES})")]
    TooLarge {
        /// ขนาดที่ประกาศ/คลายได้
        size: u64,
    },
    /// อ่านเนื้อในไม่ออก (postcard/zstd ปฏิเสธ)
    #[error("document is malformed")]
    Malformed,
}

/// เขียนไฟล์ไม่สำเร็จ
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SaveError {
    /// เอกสารใหญ่เกินเพดาน
    #[error("document is too large to save ({size} bytes, limit {MAX_DOCUMENT_BYTES})")]
    TooLarge {
        /// ขนาดที่ได้
        size: u64,
    },
    /// บีบอัดล้ม (ไม่ควรเกิด — เก็บไว้เพื่อไม่ให้ต้อง unwrap)
    #[error("could not encode the document")]
    Encode,
}

// ---------------------------------------------------------------------------
// หัวไฟล์
// ---------------------------------------------------------------------------

/// สิ่งที่อ่านได้จาก **หัวไฟล์อย่างเดียว** โดยไม่แตะ document เลย
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileInfo {
    /// เวอร์ชัน format ที่อยู่ในไฟล์
    pub version: u16,
    /// ฝังไฟล์ภาพไว้ด้วยหรือไม่ (P4-5)
    pub packed: bool,
    /// ★ รุ่นนี้ **เขียนทับไฟล์นี้ได้ไหม** — `false` = ไฟล์จากรุ่นใหม่กว่า
    pub writable: bool,
}

/// อ่านหัวไฟล์อย่างเดียว — ถูกที่สุดและปลอดภัยที่สุด
///
/// ★ ใช้ตัดสินใจก่อนทำอย่างอื่นเสมอ: ไฟล์รุ่นใหม่กว่าต้องรู้ตัวตั้งแต่ 22 ไบต์แรก
/// ไม่ใช่หลังจากพยายามคลายบีบ document ที่เราอ่านไม่เป็นอยู่แล้ว
///
/// # Errors
/// [`OpenError::TooShort`] / [`OpenError::NotRefx`] เมื่อไม่ใช่ไฟล์ `.refx`
pub fn inspect(bytes: &[u8]) -> Result<FileInfo, OpenError> {
    if bytes.len() < HEADER_LEN {
        return Err(OpenError::TooShort { len: bytes.len() });
    }
    if bytes[0..4] != MAGIC {
        return Err(OpenError::NotRefx);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
    Ok(FileInfo {
        version,
        packed: flags & FLAG_PACKED != 0,
        writable: version <= FORMAT_VERSION,
    })
}

/// ★★★ เขียนทับไฟล์ที่มีอยู่แล้วตรงนี้ได้ไหม — **ต้องถามก่อนบันทึกทับเสมอ**
///
/// `docs/07 §3` + `docs/02 §2.9`: ไฟล์ที่ major version สูงกว่าที่รุ่นนี้เข้าใจ
/// **ห้ามบันทึกทับเด็ดขาด** · ยอมให้ผู้ใช้แก้ไม่ได้ชั่วคราวดีกว่าปล่อยให้เขา
/// บันทึกทับแล้วงานที่รุ่นใหม่เก็บไว้หายโดยไม่รู้ตัว — อันแรกน่ารำคาญ
/// อันหลังคือสิ่งที่ `CLAUDE.md` บอกว่า "เลิกใช้ทันที ไม่มีโอกาสที่สอง"
///
/// ★ ไบต์ที่อ่านไม่ออกเลย (ไม่ใช่ `.refx`, สั้นเกิน) **ไม่ใช่เหตุห้ามเขียน** —
/// นั่นคือไฟล์ของโปรแกรมอื่นหรือไฟล์ขยะ ซึ่งเป็นการตัดสินใจของชั้นบน
/// (จะถามผู้ใช้ก่อนทับหรือไม่) ไม่ใช่ของ format · ที่นี่ตอบคำถามเดียวคือ
/// **"นี่เป็นงานของ RefX รุ่นใหม่กว่าหรือเปล่า"**
///
/// # Errors
/// [`OpenError::NewerVersion`] เมื่อไฟล์เดิมมาจากรุ่นที่ใหม่กว่า
pub fn may_overwrite(existing: &[u8]) -> Result<(), OpenError> {
    match inspect(existing) {
        Ok(info) if !info.writable => Err(OpenError::NewerVersion {
            found: info.version,
            understood: FORMAT_VERSION,
        }),
        // อ่านหัวไม่ออก = ไม่ใช่ไฟล์ของเรา — ไม่ใช่หน้าที่ของ format ที่จะห้าม
        Ok(_) | Err(_) => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// เข้ารหัส / ถอดรหัส
// ---------------------------------------------------------------------------

/// แปลง `Board` เป็นไบต์ของไฟล์ `.refx` (โหมด linked)
///
/// # Errors
/// [`SaveError`] เมื่อเอกสารใหญ่เกินเพดานหรือบีบอัดไม่สำเร็จ
pub fn encode(board: &Board) -> Result<Vec<u8>, SaveError> {
    let document = v1::DocumentDto::from_parts(&board.to_parts());
    let raw = postcard::to_stdvec(&document).map_err(|_| SaveError::Encode)?;
    if raw.len() > MAX_DOCUMENT_BYTES {
        return Err(SaveError::TooLarge {
            size: raw.len() as u64,
        });
    }
    let packed = zstd::encode_all(raw.as_slice(), ZSTD_LEVEL).map_err(|_| SaveError::Encode)?;

    let mut out = Vec::with_capacity(HEADER_LEN + packed.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // flags — linked mode
    out.extend_from_slice(&(packed.len() as u64).to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(&packed).to_le_bytes());
    out.extend_from_slice(&packed);
    Ok(out)
}

/// อ่านไฟล์ `.refx` กลับมาเป็น `Board`
///
/// ★★ **ทุกไบต์ในนี้คือ input ที่ไม่น่าไว้ใจ** (I-4) — ไฟล์อาจถูกดัดแปลง
/// หรือเสียหายจากดิสก์ · ห้าม panic ห้ามจอง memory ตามตัวเลขที่อยู่ในไฟล์
/// นี่คือสิ่งที่ `fuzz_document` ยิงอยู่
///
/// # Errors
/// [`OpenError`] พร้อมเหตุผลที่แยกได้ว่าไฟล์เสีย หรือมาจากรุ่นใหม่กว่า
pub fn decode(bytes: &[u8], id: BoardId) -> Result<Board, OpenError> {
    let info = inspect(bytes)?;
    if !info.writable {
        // ★ ไม่พยายามเดาเนื้อใน: postcard ไม่ self-describing การอ่าน v2 ด้วย
        //   โครง v1 จะได้ค่าที่ "อ่านผ่าน" แต่เพี้ยนทั้งก้อน ซึ่งอันตรายกว่าอ่านไม่ได้
        return Err(OpenError::NewerVersion {
            found: info.version,
            understood: FORMAT_VERSION,
        });
    }

    let declared = u64::from_le_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]);
    let crc = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);

    if declared > MAX_COMPRESSED_BYTES {
        return Err(OpenError::TooLarge { size: declared });
    }
    let body = &bytes[HEADER_LEN..];
    // ★ เทียบกับไบต์ที่ **มีจริง** ก่อนเสมอ — `declared` มาจากไฟล์จึงโกหกได้
    let declared_usize =
        usize::try_from(declared).map_err(|_| OpenError::TooLarge { size: declared })?;
    if body.len() < declared_usize {
        return Err(OpenError::Truncated {
            declared,
            actual: body.len() as u64,
        });
    }
    let body = &body[..declared_usize];

    if crc32fast::hash(body) != crc {
        return Err(OpenError::Corrupt);
    }

    // ★★ คลายบีบแบบมีเพดาน — `decode_all` ไม่มีเพดานในตัวเอง จึงต้องอ่านผ่าน
    //    `take()` แล้วเช็คว่าอ่านจนหมดจริงไหม (อ่านได้เต็มเพดาน = ยังมีต่อ = เกิน)
    let raw = decompress_bounded(body)?;
    let document: v1::DocumentDto = postcard::from_bytes(&raw).map_err(|_| OpenError::Malformed)?;
    Ok(Board::load(id, document.into_parts()))
}

/// คลาย zstd โดยมีเพดาน — กัน zip bomb (I-4)
fn decompress_bounded(body: &[u8]) -> Result<Vec<u8>, OpenError> {
    use std::io::Read as _;

    let mut decoder = zstd::stream::Decoder::new(body).map_err(|_| OpenError::Malformed)?;
    let mut raw = Vec::new();
    // อ่านได้มากสุด "เพดาน + 1" ไบต์ — ถ้าได้ครบเท่านั้นแปลว่ายังมีต่อ = เกินเพดาน
    let limit = MAX_DOCUMENT_BYTES as u64 + 1;
    decoder
        .by_ref()
        .take(limit)
        .read_to_end(&mut raw)
        .map_err(|_| OpenError::Malformed)?;
    if raw.len() > MAX_DOCUMENT_BYTES {
        return Err(OpenError::TooLarge {
            size: raw.len() as u64,
        });
    }
    Ok(raw)
}

// ---------------------------------------------------------------------------
// v1
// ---------------------------------------------------------------------------

/// DTO ของ format เวอร์ชัน 1
///
/// ★ **ห้ามแก้ struct ในนี้หลังปล่อยรุ่นแล้ว** — ไฟล์ของผู้ใช้ผูกกับรูปร่างนี้
/// เปลี่ยนอะไรให้เพิ่ม `mod v2` แล้วเขียน `migrate_v1_to_v2` (docs/07 §3)
pub mod v1 {
    use super::{
        AssetRef, Board, BoardParts, BoardSettings, Camera, ColorLabel, ContentHash, CropRect,
        Deserialize, Flip, Group, ImageFormat, Item, ItemCanvas, ItemFilter, ItemKind, ItemMeta,
        ItemParts, MissingReason, Mode, Serialize, TagId, TextNote, ViewState,
    };

    /// เอกสารทั้งก้อน
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct DocumentDto {
        /// ชื่อ board
        pub name: String,
        /// กลุ่มทั้งหมด — `ItemDto::group` อ้างด้วย **ดัชนี** ในรายการนี้
        pub groups: Vec<GroupDto>,
        /// ชื่อแท็กพร้อม id เดิม
        pub tags: Vec<TagDto>,
        /// item เรียง **ล่างสุด → บนสุด** — ลำดับในรายการ *คือ* z-order
        pub items: Vec<ItemDto>,
        /// ตั้งค่าระดับ board
        pub settings: SettingsDto,
        /// กล้อง/โหมดที่บันทึกไว้
        pub view: ViewDto,
    }

    /// กลุ่มหนึ่งกลุ่ม
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct GroupDto {
        /// ชื่อที่ผู้ใช้ตั้ง
        pub name: String,
        /// ยุบอยู่ไหม
        pub collapsed: bool,
    }

    /// ชื่อแท็กหนึ่งชื่อ
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct TagDto {
        /// id เดิม — ต้องคงไว้เพราะ `ItemDto::tags` ถืออยู่
        pub id: u32,
        /// ชื่อ
        pub name: String,
    }

    /// item หนึ่งใบ
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ItemDto {
        /// เป็นภาพ/ข้อความ/ของที่หายไป
        pub kind: KindDto,
        /// สถานะฝั่ง Canvas
        pub canvas: CanvasDto,
        /// สถานะฝั่ง Arrange
        pub meta: MetaDto,
        /// ดัชนีของกลุ่มใน `DocumentDto::groups`
        pub group: Option<u32>,
    }

    /// ชนิดของ item
    ///
    /// ★ **ไม่มี variant `Unknown`** โดยตั้งใจ (docs/02 §2.2b): kind ที่ไม่รู้จัก
    /// ควรตกเป็น `Missing` ซึ่งมีความหมายถูกต้องอยู่แล้ว (item ยังอยู่บน board
    /// ผู้ใช้เห็นว่ามีของ) · และในทางปฏิบัติ variant ใหม่ = format ใหม่ = v2
    /// ซึ่งถูกด่านเวอร์ชันปฏิเสธไปก่อนแล้ว
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub enum KindDto {
        /// ภาพ
        Image(AssetDto),
        /// โน้ตข้อความ
        Text(String),
        /// ภาพที่เปิดไม่ได้
        Missing {
            /// path เดิมสำหรับ relink
            path: String,
            /// รหัสเหตุผล (`MissingReason::to_wire`)
            reason: u8,
        },
    }

    /// ภาพต้นทาง
    ///
    /// ★ ไม่มี `format` และ `embedded` — ดูตารางในหัวโมดูล
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct AssetDto {
        /// blake3-256 ของไฟล์ต้นฉบับ
        pub hash: [u8; 32],
        /// เส้นทางล่าสุดที่เจอ
        pub path: String,
        /// ขนาดจริงหลังแก้ EXIF orientation
        pub px_size: [u32; 2],
        /// mtime ตอน ingest (unix millis) — `0` = ไม่รู้
        pub mtime: i64,
        /// ขนาดไฟล์ตอน ingest — `0` = ไม่รู้
        pub file_size: u64,
    }

    /// สถานะฝั่ง Canvas
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub struct CanvasDto {
        /// ตำแหน่ง world
        pub pos: [f32; 2],
        /// ขนาดที่แสดง
        pub size: [f32; 2],
        /// การหมุน (เรเดียน)
        pub rotation: f32,
        /// การพลิก (`Flip::to_wire`)
        pub flip: u8,
        /// ความทึบ 0..=1
        pub opacity: f32,
        /// กรอบครอป normalized `[x0, y0, x1, y1]`
        pub crop: [f32; 4],
        /// ล็อกไม่ให้แก้
        pub locked: bool,
        /// มองเห็นไหม
        pub visible: bool,
        /// ฟิลเตอร์
        pub filter: FilterDto,
    }

    /// ฟิลเตอร์ที่คำนวณใน shader
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub struct FilterDto {
        /// ขาวดำ
        pub grayscale: bool,
        /// กลับสี
        pub invert: bool,
        /// ความสว่าง -1..=1
        pub brightness: f32,
        /// คอนทราสต์ -1..=1
        pub contrast: f32,
    }

    /// สถานะฝั่ง Arrange
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct MetaDto {
        /// id ของแท็ก
        pub tags: Vec<u32>,
        /// ดาว 0..=5
        pub rating: u8,
        /// ป้ายสี (`ColorLabel::to_wire` · `0` = ไม่มีป้าย)
        pub color_label: u8,
        /// โน้ตของผู้ใช้
        pub note: String,
        /// เวลาที่เพิ่ม (unix millis)
        pub added_at: i64,
        /// ปักหมุด
        pub pinned: bool,
    }

    /// ตั้งค่าระดับ board
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub struct SettingsDto {
        /// สีพื้นหลัง (linear)
        pub background: [f32; 3],
    }

    /// กล้องหนึ่งตัว
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub struct CameraDto {
        /// จุดกลางจอใน world
        pub center: [f32; 2],
        /// ระดับซูม
        pub zoom: f32,
    }

    /// กล้อง/โหมดที่บันทึกไว้
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub struct ViewDto {
        /// กล้องของโหมด Canvas
        pub canvas: CameraDto,
        /// กล้องของโหมด Arrange
        pub arrange: CameraDto,
        /// โหมดที่เปิดค้างไว้ (`Mode::to_wire`)
        pub mode: u8,
    }

    impl DocumentDto {
        /// `Board` → DTO
        #[must_use]
        pub fn from_parts(parts: &BoardParts) -> Self {
            Self {
                name: parts.name.clone(),
                groups: parts
                    .groups
                    .iter()
                    .map(|group| GroupDto {
                        name: group.name.clone(),
                        collapsed: group.collapsed,
                    })
                    .collect(),
                tags: parts
                    .tags
                    .iter()
                    .map(|(id, name)| TagDto {
                        id: id.0,
                        name: name.clone(),
                    })
                    .collect(),
                items: parts.items.iter().map(ItemDto::from_parts).collect(),
                settings: SettingsDto {
                    background: parts.settings.background,
                },
                view: ViewDto {
                    canvas: CameraDto::from_camera(parts.view.canvas),
                    arrange: CameraDto::from_camera(parts.view.arrange),
                    mode: parts.view.mode.to_wire(),
                },
            }
        }

        /// DTO → `Board`
        ///
        /// ★ ค่าที่เพี้ยนถูก **ตัดให้อยู่ในช่วง** ไม่ใช่ปฏิเสธทั้งไฟล์ (I-3 + I-4)
        /// — `Board::load` เรียก `sanitized()` ให้ทุกใบเหมือนเส้นทางปกติ
        #[must_use]
        pub fn into_parts(self) -> BoardParts {
            BoardParts {
                name: self.name,
                groups: self
                    .groups
                    .into_iter()
                    .map(|group| Group {
                        name: group.name,
                        collapsed: group.collapsed,
                    })
                    .collect(),
                tags: self
                    .tags
                    .into_iter()
                    .map(|tag| (TagId(tag.id), tag.name))
                    .collect(),
                items: self.items.into_iter().map(ItemDto::into_parts).collect(),
                settings: BoardSettings {
                    background: self.settings.background,
                },
                view: ViewState {
                    canvas: self.view.canvas.into_camera(),
                    arrange: self.view.arrange.into_camera(),
                    mode: Mode::from_wire(self.view.mode),
                },
            }
        }
    }

    impl CameraDto {
        fn from_camera(camera: Camera) -> Self {
            Self {
                center: camera.center().to_array(),
                zoom: camera.zoom(),
            }
        }

        /// ★ `Camera::new` clamp ให้เองทั้ง `center` และ `zoom` — ค่าจากไฟล์
        /// (รวม `NaN`/`inf`) จึงถูกกรองที่จุดเดียวกับทุกเส้นทางอื่น (I-4)
        fn into_camera(self) -> Camera {
            Camera::new(glam::Vec2::from_array(self.center), self.zoom)
        }
    }

    impl ItemDto {
        fn from_parts(parts: &ItemParts) -> Self {
            let item = &parts.item;
            Self {
                kind: match &item.kind {
                    ItemKind::Image(asset) => KindDto::Image(AssetDto {
                        hash: *asset.hash.as_bytes(),
                        path: asset.path.to_string_lossy().into_owned(),
                        px_size: [asset.px_size.x, asset.px_size.y],
                        mtime: asset.mtime,
                        file_size: asset.file_size,
                    }),
                    ItemKind::Text(note) => KindDto::Text(note.text.clone()),
                    ItemKind::Missing {
                        original_path,
                        reason,
                    } => KindDto::Missing {
                        path: original_path.to_string_lossy().into_owned(),
                        reason: reason.to_wire(),
                    },
                },
                canvas: CanvasDto {
                    pos: item.canvas.pos.to_array(),
                    size: item.canvas.size.to_array(),
                    rotation: item.canvas.rotation,
                    flip: item.canvas.flip.to_wire(),
                    opacity: item.canvas.opacity,
                    crop: [
                        item.canvas.crop.min.x,
                        item.canvas.crop.min.y,
                        item.canvas.crop.max.x,
                        item.canvas.crop.max.y,
                    ],
                    locked: item.canvas.locked,
                    visible: item.canvas.visible,
                    filter: FilterDto {
                        grayscale: item.canvas.filter.grayscale,
                        invert: item.canvas.filter.invert,
                        brightness: item.canvas.filter.brightness,
                        contrast: item.canvas.filter.contrast,
                    },
                },
                meta: MetaDto {
                    tags: item.meta.tags.iter().map(|tag| tag.0).collect(),
                    rating: item.meta.rating,
                    color_label: item.meta.color_label.map_or(0, ColorLabel::to_wire),
                    note: item.meta.note.clone(),
                    added_at: item.meta.added_at,
                    pinned: item.meta.pinned,
                },
                group: parts.group.and_then(|index| u32::try_from(index).ok()),
            }
        }

        fn into_parts(self) -> ItemParts {
            let kind = match self.kind {
                KindDto::Image(asset) => ItemKind::Image(AssetRef {
                    hash: ContentHash::from_bytes(asset.hash),
                    path: std::path::PathBuf::from(asset.path),
                    px_size: glam::UVec2::new(asset.px_size[0], asset.px_size[1]),
                    // ★ v1 ไม่เก็บ format — ดูตารางในหัวโมดูล
                    format: ImageFormat::Unknown,
                    // ★ linked mode เสมอใน v1 (บิต packed ในหัวไฟล์เป็นของ P4-5)
                    embedded: false,
                    mtime: asset.mtime,
                    file_size: asset.file_size,
                }),
                KindDto::Text(text) => ItemKind::Text(TextNote { text }),
                KindDto::Missing { path, reason } => ItemKind::Missing {
                    original_path: std::path::PathBuf::from(path),
                    reason: MissingReason::from_wire(reason),
                },
            };
            let mut item = Item::new(kind);
            item.canvas = ItemCanvas {
                pos: glam::Vec2::from_array(self.canvas.pos),
                size: glam::Vec2::from_array(self.canvas.size),
                rotation: self.canvas.rotation,
                flip: Flip::from_wire(self.canvas.flip),
                opacity: self.canvas.opacity,
                // ★ ไม่ต้อง sanitize ที่นี่ — `Board::load` เรียก `insert_item`
                //   ซึ่ง sanitize ทั้ง `ItemCanvas` (รวม `crop`) ให้เหมือนเส้นทางปกติ
                //   ทุกประการ · sanitize สองที่ = เกราะสองชุดที่จะเพี้ยนจากกัน
                crop: CropRect {
                    min: glam::Vec2::new(self.canvas.crop[0], self.canvas.crop[1]),
                    max: glam::Vec2::new(self.canvas.crop[2], self.canvas.crop[3]),
                },
                locked: self.canvas.locked,
                visible: self.canvas.visible,
                filter: ItemFilter {
                    grayscale: self.canvas.filter.grayscale,
                    invert: self.canvas.filter.invert,
                    brightness: self.canvas.filter.brightness,
                    contrast: self.canvas.filter.contrast,
                },
            };
            item.meta = ItemMeta {
                tags: self.meta.tags.into_iter().map(TagId).collect(),
                rating: self.meta.rating,
                color_label: ColorLabel::from_wire(self.meta.color_label),
                // ★ ค่าจริงมาจาก `ItemParts::group` — ฟิลด์นี้ถูกเขียนทับใน `Board::load`
                group: None,
                note: self.meta.note,
                added_at: self.meta.added_at,
                pinned: self.meta.pinned,
            };
            ItemParts {
                item,
                group: self.group.and_then(|index| usize::try_from(index).ok()),
            }
        }
    }

    /// ทำให้ `Board` ที่ประกอบเสร็จแล้วเทียบกันได้ในเทสต์
    #[must_use]
    pub fn round_trip(board: &Board) -> DocumentDto {
        DocumentDto::from_parts(&board.to_parts())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp
    )]

    use super::*;
    use proptest::prelude::*;
    use refx_core::arena::ArenaKey as _;

    fn board_id() -> BoardId {
        BoardId::from_parts(0, 0)
    }

    // ---------- ตัวสร้าง board แบบสุ่ม ----------

    /// ★ สุ่มค่าที่ **ผ่านด่าน `sanitized()` มาแล้ว** เท่านั้น
    ///
    /// ★★ เหตุผลสำคัญ: `Board::load` เรียก `sanitized()` ให้ทุกใบ (I-4) ถ้าเทสต์
    /// ป้อน `NaN`/ค่าเกินช่วงเข้าไป ค่าที่ออกมาจะ *ต่าง* จากที่ป้อนโดยถูกต้อง
    /// แล้ว round-trip จะแดงเพราะเกราะทำงาน ไม่ใช่เพราะ format พัง —
    /// เทสต์ที่แดงด้วยเหตุผลผิดคือเทสต์ที่จะถูกปิดทิ้งในที่สุด
    /// · ค่าที่ **ไม่** ผ่านด่านมีเทสต์ของตัวเองแยกไว้ข้างล่าง
    fn sane_f32(range: std::ops::RangeInclusive<f32>) -> impl Strategy<Value = f32> {
        range.prop_map(|v| if v.is_finite() { v } else { 0.0 })
    }

    prop_compose! {
        fn any_canvas()(
            x in sane_f32(-5000.0..=5000.0),
            y in sane_f32(-5000.0..=5000.0),
            w in sane_f32(1.0..=4000.0),
            h in sane_f32(1.0..=4000.0),
            rotation in sane_f32(0.0..=std::f32::consts::TAU),
            flip_raw in 0u8..=u8::MAX,
            opacity in sane_f32(0.0..=1.0),
            crop_x in sane_f32(0.0..=0.4),
            crop_y in sane_f32(0.0..=0.4),
            locked in any::<bool>(),
            visible in any::<bool>(),
            grayscale in any::<bool>(),
            invert in any::<bool>(),
            brightness in sane_f32(-1.0..=1.0),
            contrast in sane_f32(-1.0..=1.0),
        ) -> ItemCanvas {
            ItemCanvas {
                pos: glam::Vec2::new(x, y),
                size: glam::Vec2::new(w, h),
                rotation,
                flip: Flip::from_wire(flip_raw),
                opacity,
                crop: CropRect {
                    min: glam::Vec2::new(crop_x, crop_y),
                    max: glam::Vec2::new(1.0 - crop_x, 1.0 - crop_y),
                },
                locked,
                visible,
                filter: ItemFilter { grayscale, invert, brightness, contrast },
            }.sanitized()
        }
    }

    prop_compose! {
        fn any_meta(tag_count: u32)(
            rating in 0u8..=5,
            label_raw in 0u8..=u8::MAX,
            note in ".{0,40}",
            added_at in 0i64..=4_000_000_000_000,
            pinned in any::<bool>(),
            tags in prop::collection::vec(0u32..tag_count.max(1), 0..4),
        ) -> ItemMeta {
            let mut tags: smallvec::SmallVec<[TagId; 4]> =
                tags.into_iter().map(TagId).collect();
            tags.sort_unstable();
            tags.dedup();
            ItemMeta {
                tags,
                rating,
                color_label: ColorLabel::from_wire(label_raw),
                group: None,
                note,
                added_at,
                pinned,
            }.sanitized()
        }
    }

    prop_compose! {
        fn any_kind()(
            which in 0u8..3,
            tag in any::<u8>(),
            name in "[a-z]{1,12}",
            reason_raw in 0u8..=u8::MAX,
            px_w in 1u32..=20000,
            px_h in 1u32..=20000,
            mtime in 0i64..=4_000_000_000_000,
            file_size in 0u64..=(1u64 << 40),
        ) -> ItemKind {
            match which {
                0 => ItemKind::Image(AssetRef {
                    hash: ContentHash::from_bytes([tag; 32]),
                    path: std::path::PathBuf::from(format!("{name}.png")),
                    px_size: glam::UVec2::new(px_w, px_h),
                    format: ImageFormat::Unknown,
                    embedded: false,
                    mtime,
                    file_size,
                }),
                1 => ItemKind::Text(TextNote { text: name }),
                _ => ItemKind::Missing {
                    original_path: std::path::PathBuf::from(format!("{name}.jpg")),
                    reason: MissingReason::from_wire(reason_raw),
                },
            }
        }
    }

    prop_compose! {
        /// board แบบสุ่มที่ประกอบ **ผ่าน `BoardParts` เหมือนเส้นทางเปิดไฟล์จริง**
        fn any_board()(
            name in "[a-z ]{0,20}",
            group_names in prop::collection::vec("[a-z]{1,8}", 0..4),
            tag_names in prop::collection::vec("[a-z]{1,8}", 0..5),
            background in prop::array::uniform3(sane_f32(0.0..=1.0)),
            mode_raw in 0u8..=u8::MAX,
            cam_x in sane_f32(-2000.0..=2000.0),
            cam_y in sane_f32(-2000.0..=2000.0),
            zoom in sane_f32(0.01..=64.0),
            arr_y in sane_f32(0.0..=10000.0),
            item_count in 0usize..12,
        )(
            name in Just(name),
            groups in Just(group_names.clone()),
            tags in Just(tag_names.clone()),
            background in Just(background),
            mode_raw in Just(mode_raw),
            cam_x in Just(cam_x), cam_y in Just(cam_y), zoom in Just(zoom), arr_y in Just(arr_y),
            items in prop::collection::vec(
                (any_kind(), any_canvas(), any_meta(tag_names.len().max(1) as u32),
                 prop::option::of(0usize..group_names.len().max(1))),
                item_count..=item_count),
        ) -> Board {
            let groups: Vec<Group> = groups.into_iter()
                .map(|name| Group { name, collapsed: false })
                .collect();
            let parts = BoardParts {
                name,
                items: items.into_iter().map(|(kind, canvas, meta, group)| {
                    let mut item = Item::new(kind);
                    item.canvas = canvas;
                    item.meta = meta;
                    ItemParts {
                        item,
                        // ดัชนีที่ชี้นอกช่วงต้องกลายเป็น "ไม่มีกลุ่ม" ไม่ใช่พัง
                        group: group.filter(|i| *i < groups.len()),
                    }
                }).collect(),
                tags: tags.into_iter().enumerate()
                    .map(|(i, name)| (TagId(i as u32), name))
                    .collect(),
                groups,
                settings: BoardSettings { background },
                view: ViewState {
                    canvas: Camera::new(glam::Vec2::new(cam_x, cam_y), zoom),
                    arrange: Camera::new(glam::Vec2::new(0.0, arr_y), 1.0),
                    mode: Mode::from_wire(mode_raw),
                },
            };
            Board::load(board_id(), parts)
        }
    }

    // ---------- เกณฑ์หลักของ ROADMAP: round-trip ----------

    proptest! {
        /// ★★★ **เกณฑ์ผ่านของ P4-1** — สุ่ม board แล้ว save/load ต้องได้เท่าเดิมเป๊ะ
        ///
        /// เทียบด้วย `Board::PartialEq` ซึ่ง destructure ครบทุกฟิลด์ไม่มี `..`
        /// (§4 ข้อ 20) → เพิ่มฟิลด์ใหม่ใน `Board` เมื่อไหร่ เทสต์นี้จะเริ่มเทียบมัน
        /// ให้เองโดยอัตโนมัติ ไม่ต้องมีใครจำมาเติม
        #[test]
        fn a_board_survives_a_round_trip_through_the_file_format(board in any_board()) {
            let bytes = encode(&board).expect("เขียนไม่สำเร็จ");
            let back = decode(&bytes, board_id()).expect("อ่านกลับไม่สำเร็จ");
            prop_assert_eq!(&back, &board);
            // ★ ไฟล์ที่เพิ่งเปิดต้องไม่ dirty — ไม่งั้นผู้ใช้โดนถาม "บันทึกไหม" ทันที
            prop_assert!(!back.is_dirty());
        }

        /// ★★ **ไบต์ที่เขียนออกมาต้องเหมือนเดิมทุกครั้ง** (deterministic)
        ///
        /// ถ้าไม่ deterministic การบันทึกซ้ำโดยไม่ได้แก้อะไรจะได้ไฟล์ที่ต่างกัน
        /// ซึ่งทำให้ระบบสำรอง/ซิงค์ (Dropbox, git) เห็นว่ามีการเปลี่ยนแปลงตลอดเวลา
        /// — และเป็นสัญญาณว่ามี `HashMap` หลุดเข้ามาในเส้นทาง (CLAUDE.md ห้าม)
        #[test]
        fn encoding_the_same_board_twice_gives_the_same_bytes(board in any_board()) {
            let a = encode(&board).expect("เขียนไม่สำเร็จ");
            let b = encode(&board).expect("เขียนไม่สำเร็จ");
            prop_assert_eq!(a, b);
        }

        /// ★★★ **ไบต์ที่ถูกดัดแปลงต้องคืน `Err` — ห้าม panic** (I-4, I-7)
        ///
        /// เส้นทางเดียวกับที่ `fuzz_document` ยิง แต่รันทุกครั้งที่ `cargo test`
        /// ไม่ใช่แค่คืนวันจันทร์/พุธ/ศุกร์
        #[test]
        fn a_corrupted_file_is_rejected_instead_of_crashing(
            board in any_board(),
            at in 0usize..400,
            xor in 1u8..=u8::MAX,
        ) {
            let mut bytes = encode(&board).expect("เขียนไม่สำเร็จ");
            let at = at % bytes.len();
            bytes[at] ^= xor;
            // ผลเป็นอะไรก็ได้ **ยกเว้น panic** — พลิกบิตในหัวไฟล์อาจได้ error
            // คนละชนิดกับพลิกใน document และทั้งคู่ถูกต้อง
            let _ = decode(&bytes, board_id());
        }
    }

    // ---------- ★ ค่าที่รุ่นนี้ไม่รู้จัก ต้องเขียนกลับได้ครบ ----------

    /// ★★★ **"ทนได้" ยังไม่พอ ต้องส่งคืนค่าเดิมได้** (docs/02 §2.9)
    ///
    /// เคสที่กฎข้อนี้เกิดมาจาก:
    ///
    /// > ผู้ใช้ติดป้ายสีด้วย RefX รุ่นใหม่ → เปิดด้วยรุ่นเก่า → รุ่นเก่าอ่านค่า
    /// > ไม่รู้จักเป็น `None` → ผู้ใช้ขยับภาพใบเดียวแล้วบันทึก →
    /// > **ป้ายสีหายถาวร** โดยไม่มีอะไรเตือน
    ///
    /// เทสต์นี้เดินเคสนั้นทั้งเส้น: สร้างไฟล์ที่มีค่าซึ่งรุ่นนี้ไม่รู้จักสามตัว
    /// → เปิด → **บันทึกใหม่โดยไม่แก้อะไร** → เปิดอีกครั้ง → ค่าต้องอยู่ครบ
    #[test]
    fn values_this_build_does_not_understand_survive_being_rewritten() {
        let unknown_label = 200u8;
        let unknown_flip = 77u8;
        let unknown_reason = 111u8;

        let mut image = Item::new(ItemKind::Image(AssetRef {
            hash: ContentHash::from_bytes([7; 32]),
            path: std::path::PathBuf::from("a.png"),
            px_size: glam::UVec2::new(100, 80),
            format: ImageFormat::Unknown,
            embedded: false,
            mtime: 0,
            file_size: 0,
        }));
        image.canvas.flip = Flip::from_wire(unknown_flip);
        image.meta.color_label = ColorLabel::from_wire(unknown_label);

        let missing = Item::new(ItemKind::Missing {
            original_path: std::path::PathBuf::from("gone.png"),
            reason: MissingReason::from_wire(unknown_reason),
        });

        let board = Board::load(
            board_id(),
            BoardParts {
                name: "unknown values".to_owned(),
                items: vec![
                    ItemParts {
                        item: image,
                        group: None,
                    },
                    ItemParts {
                        item: missing,
                        group: None,
                    },
                ],
                ..BoardParts::default()
            },
        );

        // เปิด → บันทึกใหม่โดยไม่แก้อะไร → เปิดอีกครั้ง (เคสของผู้ใช้เป๊ะ ๆ)
        let once = decode(&encode(&board).unwrap(), board_id()).unwrap();
        let twice = decode(&encode(&once).unwrap(), board_id()).unwrap();

        let ids: Vec<_> = twice.z_order().to_vec();
        let item = twice.item(ids[0]).unwrap();
        assert_eq!(
            item.canvas.flip.to_wire(),
            unknown_flip,
            "การพลิกที่รุ่นนี้ไม่รู้จักถูกเขียนทับ = ทำงานของผู้ใช้หาย"
        );
        assert_eq!(
            item.meta.color_label.map_or(0, ColorLabel::to_wire),
            unknown_label,
            "ป้ายสีที่รุ่นนี้ไม่รู้จักหายไปตอนบันทึกทับ"
        );
        assert!(!item.canvas.flip.is_known());
        assert!(!item.meta.color_label.unwrap().is_known());

        let ItemKind::Missing { reason, .. } = &twice.item(ids[1]).unwrap().kind else {
            panic!("item ที่สองต้องยังเป็น Missing");
        };
        assert_eq!(reason.to_wire(), unknown_reason);

        // ★ และ board ทั้งก้อนต้องเท่าเดิม ไม่ใช่แค่สามฟิลด์ที่ตรวจข้างบน
        assert_eq!(once, twice, "บันทึกทับรอบสองทำให้ board เปลี่ยน");
    }

    // ---------- ★ ไฟล์จากรุ่นใหม่กว่า ----------

    /// สร้างไฟล์ที่หัวบอกว่าเป็น format เวอร์ชันอื่น
    fn with_version(bytes: &[u8], version: u16) -> Vec<u8> {
        let mut out = bytes.to_vec();
        out[4..6].copy_from_slice(&version.to_le_bytes());
        out
    }

    /// ★★★ ไฟล์จากรุ่นใหม่กว่า: **อ่านไม่ได้ และห้ามเขียนทับ** (docs/07 §3 + §2.9)
    ///
    /// ★★ ทำไมไม่ "เปิดอ่านอย่างเดียวแล้วโชว์เนื้อใน" — postcard **ไม่
    /// self-describing** การอ่าน v2 ด้วยโครง v1 ไม่ได้ให้ข้อมูลบางส่วน
    /// แต่ให้ค่าที่ *เพี้ยนทั้งก้อน* โดยไม่มีอะไรฟ้อง ซึ่งอันตรายกว่าอ่านไม่ได้
    /// · สิ่งที่ปกป้องผู้ใช้จริงคือครึ่งหลัง: **ห้ามบันทึกทับ**
    #[test]
    fn a_file_from_a_newer_build_is_never_overwritten() {
        let board = Board::load(board_id(), BoardParts::default());
        let v1_bytes = encode(&board).unwrap();

        for newer in [FORMAT_VERSION + 1, 7, u16::MAX] {
            let bytes = with_version(&v1_bytes, newer);

            // อ่านไม่ได้ และบอกเลขทั้งสองฝั่งให้ผู้ใช้เห็น
            assert_eq!(
                decode(&bytes, board_id()),
                Err(OpenError::NewerVersion {
                    found: newer,
                    understood: FORMAT_VERSION,
                }),
                "v{newer} ต้องถูกปฏิเสธพร้อมบอกเวอร์ชัน"
            );

            // ★ หัวใจของข้อนี้ — เขียนทับไม่ได้
            assert_eq!(
                may_overwrite(&bytes),
                Err(OpenError::NewerVersion {
                    found: newer,
                    understood: FORMAT_VERSION,
                }),
                "v{newer} ต้องห้ามเขียนทับ"
            );
            assert!(!inspect(&bytes).unwrap().writable);
        }

        // ★ negative control ของตัวเอง: เวอร์ชันที่เราเข้าใจต้อง **ไม่** ถูกห้าม
        //   ไม่งั้นประตูนี้จะห้ามทุกอย่างแล้วดูเหมือนทำงานถูก
        assert!(may_overwrite(&v1_bytes).is_ok());
        assert!(inspect(&v1_bytes).unwrap().writable);
        assert!(decode(&v1_bytes, board_id()).is_ok());
    }

    /// ★ ไฟล์ที่ไม่ใช่ `.refx` เลย **ไม่ใช่** เหตุห้ามเขียน
    ///
    /// ห้ามเขียนทับสงวนไว้ให้ "งานของ RefX รุ่นใหม่กว่า" เท่านั้น · การห้ามทับ
    /// ไฟล์ขยะด้วยจะทำให้ผู้ใช้ Save As ทับไฟล์เก่าของตัวเองไม่ได้เลย
    #[test]
    fn a_file_that_is_not_refx_at_all_does_not_block_saving() {
        assert!(may_overwrite(b"").is_ok());
        assert!(may_overwrite(b"not a refx file at all, just text").is_ok());
        assert_eq!(inspect(b"short"), Err(OpenError::TooShort { len: 5 }));
        assert_eq!(
            inspect(
                b"XXXX\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
            ),
            Err(OpenError::NotRefx)
        );
    }

    // ---------- ★ ไฟล์เสีย vs ไฟล์รุ่นใหม่ — คนละข้อความ ----------

    /// ★★ `doc_crc` มีไว้เพื่อ **แยกไฟล์เสียออกจากไฟล์ที่อ่านไม่เป็น** (docs/07 §1)
    ///
    /// ผู้ใช้ที่ไฟล์งานเปิดไม่ขึ้นต้องรู้ว่าจะไปหาไฟล์สำรอง หรือไปอัปเดตโปรแกรม
    /// — สองอย่างนี้เป็นคนละการกระทำโดยสิ้นเชิง
    #[test]
    fn a_damaged_file_reads_differently_from_one_we_cannot_understand() {
        let board = Board::load(board_id(), BoardParts::default());
        let good = encode(&board).unwrap();

        // พลิกไบต์ใน document → checksum จับได้
        let mut damaged = good.clone();
        let last = damaged.len() - 1;
        damaged[last] ^= 0xFF;
        assert_eq!(decode(&damaged, board_id()), Err(OpenError::Corrupt));

        // ตัดท้ายทิ้ง → รู้ตั้งแต่ความยาว ไม่ต้องรอ checksum
        let truncated = &good[..good.len() - 3];
        assert!(matches!(
            decode(truncated, board_id()),
            Err(OpenError::Truncated { .. })
        ));

        // เวอร์ชันใหม่กว่า → คนละ error กับสองอันบน
        let newer = with_version(&good, FORMAT_VERSION + 1);
        assert!(matches!(
            decode(&newer, board_id()),
            Err(OpenError::NewerVersion { .. })
        ));
    }

    /// ★★★ `doc_len` ที่โกหกต้องไม่พาไปจอง memory ตามตัวเลขในไฟล์ (I-4)
    #[test]
    fn a_lying_length_field_never_allocates_what_it_asks_for() {
        let board = Board::load(board_id(), BoardParts::default());
        let good = encode(&board).unwrap();

        let mut huge = good.clone();
        huge[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(
            decode(&huge, board_id()),
            Err(OpenError::TooLarge { size: u64::MAX })
        );

        // ค่าที่อยู่ใต้เพดานแต่ยังมากกว่าไบต์ที่มีจริง → Truncated ไม่ใช่ panic
        let mut lying = good.clone();
        lying[8..16].copy_from_slice(&(MAX_COMPRESSED_BYTES - 1).to_le_bytes());
        assert!(matches!(
            decode(&lying, board_id()),
            Err(OpenError::Truncated { .. })
        ));
    }

    /// ★★ zip bomb — ข้อมูลซ้ำ ๆ ที่บีบแล้วเล็กมากแต่คลายออกมหาศาล
    ///
    /// ไม่มีเพดานตอนคลาย = ไฟล์ไม่กี่ KB ทำให้จอง RAM หลาย GB ได้
    #[test]
    fn a_compression_bomb_is_refused_by_the_ceiling() {
        // ศูนย์ล้วนบีบได้แน่นมาก — คลายออกเกินเพดานแน่นอน
        let bomb = vec![0u8; MAX_DOCUMENT_BYTES + 4096];
        let packed = zstd::encode_all(bomb.as_slice(), ZSTD_LEVEL).unwrap();
        assert!(
            packed.len() < 1 << 20,
            "ตัวอย่างต้องบีบได้เล็กจริง ไม่งั้นไม่ได้ทดสอบสิ่งที่ตั้งใจ (ได้ {} ไบต์)",
            packed.len()
        );

        let mut bytes = Vec::with_capacity(HEADER_LEN + packed.len());
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&(packed.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&crc32fast::hash(&packed).to_le_bytes());
        bytes.extend_from_slice(&packed);

        assert!(matches!(
            decode(&bytes, board_id()),
            Err(OpenError::TooLarge { .. })
        ));
    }

    // ---------- ★ view: หนี้ §6 ----------

    /// ★★★ กล้องที่บันทึกไว้ต้องกลับมาจริง — **ไม่ใช่กล้องค่าปริยาย**
    ///
    /// `HANDOFF §6` แถวแรกเตือนกับดักนี้ไว้ตรง ๆ: เขียน DTO ให้ `ViewState`
    /// แล้วเทสต์ round-trip ผ่านหมด **จะดู "เสร็จ" ทั้งที่สิ่งที่ถูกบันทึกคือ
    /// กล้องค่าปริยายเสมอ** เพราะไม่มีใครเคยเขียน `Board::view` เลย
    ///
    /// เทสต์นี้จึงยืนยันสองชั้น: (ก) ค่าที่ใส่ไปกลับมาครบ **และ**
    /// (ข) ค่านั้น **ต่างจากค่าปริยาย** — ข้อ (ข) คือข้อที่จับกับดักได้
    #[test]
    fn the_saved_camera_comes_back_and_is_not_the_default_one() {
        let view = ViewState {
            canvas: Camera::new(glam::Vec2::new(1234.5, -678.25), 3.5),
            arrange: Camera::new(glam::Vec2::new(0.0, 4096.0), 1.0),
            mode: Mode::Arrange,
        };
        assert_ne!(view, ViewState::default(), "ตัวอย่างต้องต่างจากค่าปริยาย");

        let board = Board::load(
            board_id(),
            BoardParts {
                view,
                ..BoardParts::default()
            },
        );
        let back = decode(&encode(&board).unwrap(), board_id()).unwrap();

        assert_eq!(
            back.view().canvas.center(),
            glam::Vec2::new(1234.5, -678.25)
        );
        assert_eq!(back.view().canvas.zoom(), 3.5);
        assert_eq!(back.view().arrange.center().y, 4096.0);
        assert_eq!(back.view().mode, Mode::Arrange);
        assert_ne!(
            *back.view(),
            ViewState::default(),
            "อ่านกลับมาได้กล้องค่าปริยาย = ไม่ได้บันทึกกล้องจริง"
        );
    }

    // ---------- ★ โครงสร้างที่อ้างกันเอง ----------

    /// ★★ กลุ่มถูกอ้างด้วย **ดัชนี** ไม่ใช่ `GroupId` — และต้องผูกกลับถูกใบ
    #[test]
    fn group_membership_survives_the_trip_by_index_not_by_arena_key() {
        let mut a = Item::new(ItemKind::Text(TextNote { text: "a".into() }));
        a.meta.note = "first".into();
        let b = Item::new(ItemKind::Text(TextNote { text: "b".into() }));
        let c = Item::new(ItemKind::Text(TextNote { text: "c".into() }));

        let board = Board::load(
            board_id(),
            BoardParts {
                groups: vec![
                    Group {
                        name: "one".into(),
                        collapsed: false,
                    },
                    Group {
                        name: "two".into(),
                        collapsed: true,
                    },
                ],
                items: vec![
                    ItemParts {
                        item: a,
                        group: Some(1),
                    },
                    ItemParts {
                        item: b,
                        group: Some(0),
                    },
                    ItemParts {
                        item: c,
                        group: None,
                    },
                ],
                ..BoardParts::default()
            },
        );
        let back = decode(&encode(&board).unwrap(), board_id()).unwrap();
        assert_eq!(back, board);

        let ids = back.z_order().to_vec();
        let group_of = |i: usize| back.item(ids[i]).unwrap().meta.group;
        let name_of = |i: usize| {
            group_of(i)
                .and_then(|g| back.group(g))
                .map(|g| g.name.clone())
        };
        assert_eq!(name_of(0).as_deref(), Some("two"));
        assert_eq!(name_of(1).as_deref(), Some("one"));
        assert_eq!(group_of(2), None);
        assert!(back.group(group_of(0).unwrap()).unwrap().collapsed);
    }

    /// ★ ดัชนีกลุ่มที่ชี้นอกช่วง = "ไม่มีกลุ่ม" **ไม่ใช่เปิดไฟล์ไม่ได้** (I-3)
    ///
    /// board ที่จัดมาสามชั่วโมงต้องไม่เปิดไม่ได้เพราะเลขเดียวเพี้ยน
    #[test]
    fn a_group_index_pointing_nowhere_loses_the_group_not_the_board() {
        let item = Item::new(ItemKind::Text(TextNote { text: "x".into() }));
        let board = Board::load(
            board_id(),
            BoardParts {
                groups: vec![Group {
                    name: "only".into(),
                    collapsed: false,
                }],
                items: vec![ItemParts {
                    item,
                    group: Some(99),
                }],
                ..BoardParts::default()
            },
        );
        assert_eq!(board.len(), 1, "item ต้องยังอยู่");
        assert_eq!(board.item(board.z_order()[0]).unwrap().meta.group, None);
    }

    /// ★ id ของแท็กต้องคงเดิม — `ItemMeta::tags` ถือค่าเหล่านั้นอยู่
    ///
    /// ถ้า id เลื่อน ภาพจะโผล่ใต้แท็กผิดชื่อ ซึ่งอ่านว่า "โปรแกรมสลับข้อมูล"
    #[test]
    fn tag_ids_keep_pointing_at_the_same_names() {
        let mut item = Item::new(ItemKind::Text(TextNote { text: "x".into() }));
        item.meta.tags = smallvec::smallvec![TagId(3), TagId(9)];

        let board = Board::load(
            board_id(),
            BoardParts {
                tags: vec![
                    (TagId(3), "portrait".to_owned()),
                    (TagId(9), "lighting".to_owned()),
                ],
                items: vec![ItemParts { item, group: None }],
                ..BoardParts::default()
            },
        );
        let back = decode(&encode(&board).unwrap(), board_id()).unwrap();
        assert_eq!(back.tags().name(TagId(3)), Some("portrait"));
        assert_eq!(back.tags().name(TagId(9)), Some("lighting"));
        let names: Vec<&str> = back
            .item(back.z_order()[0])
            .unwrap()
            .meta
            .tags
            .iter()
            .filter_map(|id| back.tags().name(*id))
            .collect();
        assert_eq!(names, vec!["portrait", "lighting"]);
    }

    /// ★ ลำดับ z คือลำดับในไฟล์ — สลับเมื่อไหร่ผู้ใช้เห็นภาพซ้อนกันผิดชั้น
    #[test]
    fn the_stacking_order_is_the_order_in_the_file() {
        let items: Vec<ItemParts> = ["bottom", "middle", "top"]
            .into_iter()
            .map(|text| ItemParts {
                item: Item::new(ItemKind::Text(TextNote {
                    text: text.to_owned(),
                })),
                group: None,
            })
            .collect();
        let board = Board::load(
            board_id(),
            BoardParts {
                items,
                ..BoardParts::default()
            },
        );
        let back = decode(&encode(&board).unwrap(), board_id()).unwrap();

        let order: Vec<String> = back
            .items_in_z_order()
            .map(|(_, item)| match &item.kind {
                ItemKind::Text(note) => note.text.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(order, vec!["bottom", "middle", "top"]);
        assert!(back.z_order_is_consistent());
    }
}
