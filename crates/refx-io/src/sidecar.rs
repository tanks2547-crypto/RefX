//! `.refx-meta` — tag/rating ของโฟลเดอร์ที่ผู้ใช้เปิดดูเฉย ๆ (P5-5 · `docs/07 §5`)
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ magic     "RFXM"          4 B            │
//! │ version   u16 = 1         2 B            │
//! │ flags     u16             2 B  (สงวนไว้)  │
//! │ body_len  u64             8 B            │
//! │ body_crc  u32             4 B            │
//! ├──────────────────────────────────────────┤
//! │ body      (postcard บีบด้วย zstd -3)      │
//! └──────────────────────────────────────────┘
//! ```
//!
//! ## ★★★ กุญแจคือ **ชื่อไฟล์** hash เป็น **พยาน** ไม่ใช่กุญแจ
//!
//! | ถ้าใช้ | พังเมื่อ |
//! |---|---|
//! | hash เป็นกุญแจ | แก้ภาพใน Photoshop แล้วเซฟทับ → **tag หายเงียบ ๆ** |
//! | ชื่อเป็นกุญแจอย่างเดียว | เปลี่ยนชื่อ `img1.jpg` → `dragon-pose.jpg` → tag หาย |
//!
//! → [`resolve`] จับคู่เป็น **ชุด** ไม่ใช่ทีละใบ เพราะกติกาข้อ 2 พูดถึง
//!   "hash ที่ตรงและไม่ซ้ำใน**รายการที่เหลือ**" ซึ่งนิยามไม่ได้เลยถ้าถามทีละใบ
//!
//! ## ★★★ ไฟล์ที่อ่านไม่ได้ = **ห้ามแตะ** — บังคับด้วยคอมไพเลอร์
//!
//! `.refx-meta` อยู่ในโฟลเดอร์ของผู้ใช้ **ไม่ใช่ของเรา** · ตัวที่พังหรือมาจาก
//! รุ่นใหม่กว่าอาจเป็นงานที่ RefX รุ่นหน้าเขียนไว้ หรือของเครื่องมืออื่น
//! → ห้ามเขียนทับ ห้ามเปลี่ยนชื่อ ห้ามลบ
//!
//! กฎข้อนี้เขียนเป็นคอมเมนต์แล้ววันหนึ่งจะมีคนลืม จึงทำเป็น **ชนิด**:
//! [`write_atomic`] ขอ [`WritePermit`] ซึ่งมีที่มาทางเดียวคือ [`Load::Fresh`]
//! กับ [`Load::Opened`] · [`Load::HandsOff`] ไม่มีใบให้ — โค้ดที่พยายามเขียนทับ
//! ไฟล์ที่อ่านไม่ได้จะ **คอมไพล์ไม่ผ่าน** ไม่ใช่ผ่าน review ไม่ทัน
//!
//! ## สิ่งที่จงใจ **ไม่** เขียนลงไฟล์
//!
//! | ฟิลด์ | ทำไมไม่เขียน |
//! |---|---|
//! | `TagId` | เป็นดัชนีในตารางของ **board ใบนั้น** — ข้ามเซสชันแล้วไม่มีความหมาย · เก็บ **ชื่อแท็ก** แทน |
//! | `ItemMeta::group` | `GroupId` เป็นของ board เหมือนกัน และกลุ่มเป็นเรื่องของ **การจัดวาง** ไม่ใช่ของไฟล์ |
//! | `ItemMeta::added_at` | "เพิ่มเข้า board เมื่อไหร่" เป็นข้อเท็จจริงของ board ไม่ใช่ของไฟล์บนดิสก์ |
//!
//! spec: docs/07-file-format.md §5

use std::path::{Path, PathBuf};

use refx_core::board::{ColorLabel, ItemMeta};
use refx_core::hash::ContentHash;

use crate::save::RenameFn;

/// ชื่อไฟล์ sidecar — ★ **ห้ามตั้งแอตทริบิวต์ซ่อน** (`docs/07 §5`)
///
/// ขึ้นต้นด้วยจุดเพื่อให้ file manager ส่วนใหญ่จัดกลุ่มไว้ท้าย ๆ แต่ผู้ใช้
/// **ต้องมองเห็นและลบได้** — ไฟล์ที่เราสร้างแล้วเขาหาไม่เจอคือไฟล์ที่ลบไม่ถูก
pub const SIDECAR_NAME: &str = ".refx-meta";

/// ลายเซ็นหัวไฟล์ — ★ ต่างจาก `.refx` (`REFX`) โดยตั้งใจ
///
/// ไฟล์สองชนิดที่ใช้ลายเซ็นเดียวกันแปลว่าวันหนึ่งจะมีคนเปิดผิดตัวแล้วอ่าน
/// โครงผิดจนได้ค่าเพี้ยนโดยไม่มีอะไรฟ้อง (postcard ไม่ self-describing)
pub const MAGIC: [u8; 4] = *b"RFXM";

/// เวอร์ชันที่รุ่นนี้เขียนและอ่านได้
pub const SIDECAR_VERSION: u16 = 1;

/// ขนาดหัวไฟล์เป็นไบต์
pub const HEADER_LEN: usize = 4 + 2 + 2 + 8 + 4;

/// เพดานของ body **หลังคลายบีบ** (I-4 + I-6 — กัน zip bomb)
///
/// โฟลเดอร์ที่ใหญ่ที่สุดที่ UI รองรับคือ 3,072 ใบ · แต่ละรายการมีชื่อไฟล์
/// โน้ต และแท็ก ~1 KB เป็นอย่างมาก → 3 MB · เพดานนี้จึงกว้างกว่าของจริงราว 5 เท่า
pub const MAX_BODY_BYTES: usize = 16 << 20;

/// ★ เพดานของ **ไฟล์บนดิสก์** (ก่อนคลายบีบ) — ถามขนาดก่อนเปิดเสมอ (I-4/I-6)
///
/// ตัวบีบทำให้ไฟล์จริงเล็กกว่า body หลายเท่า · เผื่อไว้เท่ากับ body เพื่อ
/// ไม่ให้ไฟล์ที่บีบไม่ลงเลย (เนื้อสุ่ม) ถูกปฏิเสธทั้งที่ยังอยู่ในเพดานที่แท้จริง
pub const MAX_FILE_BYTES: u64 = MAX_BODY_BYTES as u64;

/// เพดานจำนวนรายการ — ตรวจ**ก่อน**จองอะไรตามตัวเลขในไฟล์
pub const MAX_ENTRIES: usize = 100_000;

/// เพดานความยาวชื่อไฟล์ที่ยอมอ่านกลับ (NTFS/ext4 อยู่ที่ 255)
pub const MAX_NAME_LEN: usize = 255;

/// เพดานความยาวโน้ตต่อรายการ
pub const MAX_NOTE_LEN: usize = 4096;

/// เพดานจำนวนแท็กต่อรายการ
pub const MAX_TAGS_PER_ENTRY: usize = 64;

/// เพดานความยาวชื่อแท็ก
pub const MAX_TAG_LEN: usize = 128;

/// ระดับ zstd — ★ ตัวเดียวกับ `.refx` เพื่อไม่ให้มีสองมาตรฐานในโปรเจกต์เดียว
const ZSTD_LEVEL: i32 = 3;

// ---------------------------------------------------------------------------
// ชนิดที่เห็นจากข้างนอก
// ---------------------------------------------------------------------------

/// meta ของไฟล์ภาพหนึ่งใบ ตามที่เก็บไว้ใน sidecar
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// ชื่อไฟล์ล้วน ไม่มีโฟลเดอร์ — **กุญแจ**
    pub file_name: String,
    /// blake3 ของเนื้อไฟล์ตอนที่เราเขียนรายการนี้ — **พยาน**
    pub hash: ContentHash,
    /// ดาว 0..=5
    pub rating: u8,
    /// ป้ายสี
    pub color_label: Option<ColorLabel>,
    /// โน้ตของผู้ใช้
    pub note: String,
    /// ปักหมุด
    pub pinned: bool,
    /// ชื่อแท็ก (ไม่ใช่ `TagId` — ดูหัวโมดูล)
    pub tags: Vec<String>,
}

impl Entry {
    /// ประกอบรายการจาก meta ของ board หนึ่งใบ
    ///
    /// `tags` ต้องเป็น**ชื่อ**ที่ผู้เรียกแปลงมาจาก `TagId` แล้ว เพราะตารางชื่อ
    /// อยู่ที่ `Board` ซึ่ง `refx-io` ไม่ควรต้องถือไว้เพื่อเขียนไฟล์ใบเดียว
    #[must_use]
    pub fn from_meta(
        file_name: String,
        hash: ContentHash,
        meta: &ItemMeta,
        tags: Vec<String>,
    ) -> Self {
        Self {
            file_name,
            hash,
            rating: meta.rating,
            color_label: meta.color_label,
            note: meta.note.clone(),
            pinned: meta.pinned,
            tags,
        }
    }

    /// ทับค่าลงบน meta ที่มีอยู่ — ★ **ไม่แตะ `group`/`added_at`/`tags`**
    ///
    /// แท็กต้องเดินผ่าน `Board` เพื่อขอ `TagId` จึงเป็นงานของผู้เรียก
    pub fn apply_to(&self, meta: &mut ItemMeta) {
        meta.rating = self.rating.min(ItemMeta::MAX_RATING);
        meta.color_label = self.color_label;
        meta.note.clone_from(&self.note);
        meta.pinned = self.pinned;
    }

    /// รายการนี้ว่างเปล่าหรือไม่ — ★ ของว่างไม่ต้องเขียนลงไฟล์ของผู้ใช้
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.rating == 0
            && self.color_label.is_none()
            && self.note.is_empty()
            && !self.pinned
            && self.tags.is_empty()
    }
}

/// เนื้อของ `.refx-meta` ทั้งไฟล์
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sidecar {
    /// รายการทั้งหมด เรียงตามชื่อไฟล์เสมอ (ผลต้อง deterministic)
    pub entries: Vec<Entry>,
}

impl Sidecar {
    /// สร้างจากรายการ — เรียงและตัดของว่างออกให้เอง
    #[must_use]
    pub fn new(mut entries: Vec<Entry>) -> Self {
        entries.retain(|entry| !entry.is_blank());
        entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        entries.dedup_by(|a, b| a.file_name == b.file_name);
        Self { entries }
    }
}

/// เหตุผลที่ห้ามแตะไฟล์ที่มีอยู่
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockReason {
    /// มาจาก RefX รุ่นใหม่กว่า — เนื้อข้างในเราอ่านไม่ถูกแน่นอน
    NewerVersion {
        /// เวอร์ชันที่เจอในไฟล์
        found: u16,
    },
    /// ลายเซ็นไม่ใช่ของเรา — ไฟล์ของเครื่องมืออื่นที่บังเอิญชื่อเดียวกัน
    NotOurs,
    /// เป็นของเราแต่เนื้อเสีย (crc ไม่ตรง · ถูกตัด · คลายบีบไม่ออก)
    Damaged,
}

/// ★★★ ใบอนุญาตเขียน — สร้างนอกโมดูลนี้ไม่ได้
///
/// ถือ (mtime, size) ที่เห็นตอนอ่านไว้ด้วย เพื่อให้ [`write_atomic`] ปฏิเสธ
/// การเขียนทับของที่ **คนอื่นแก้ไประหว่างทาง** ได้ (`docs/07 §5`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WritePermit {
    /// สภาพไฟล์ที่เราเห็นตอนอ่าน — `None` = ตอนนั้นยังไม่มีไฟล์
    seen: Option<Stamp>,
}

/// ลายพิมพ์ของไฟล์ที่ใช้ตรวจว่ามีใครมาแก้ระหว่างทาง
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    mtime_ms: i64,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0);
        Some(Self {
            len: meta.len(),
            mtime_ms,
        })
    }
}

/// ผลของการเปิด `.refx-meta` ของโฟลเดอร์หนึ่ง
#[derive(Debug)]
pub enum Load {
    /// ยังไม่มีไฟล์ — เขียนได้ (ถ้าผู้ใช้อนุญาต)
    Fresh(WritePermit),
    /// อ่านได้ — ใช้ของเดิมต่อและเขียนทับได้
    Opened(Sidecar, WritePermit),
    /// ★ อ่านไม่ได้ = **ห้ามแตะ** · ไม่มี [`WritePermit`] ให้โดยเจตนา
    HandsOff(LockReason),
}

impl Load {
    /// รายการที่อ่านได้ — `HandsOff`/`Fresh` คืนของว่าง
    #[must_use]
    pub fn sidecar(&self) -> Option<&Sidecar> {
        match self {
            Self::Opened(sidecar, _) => Some(sidecar),
            Self::Fresh(_) | Self::HandsOff(_) => None,
        }
    }

    /// ใบอนุญาตเขียน — `None` แปลว่าห้ามแตะไฟล์นี้
    #[must_use]
    pub fn permit(&self) -> Option<&WritePermit> {
        match self {
            Self::Fresh(permit) | Self::Opened(_, permit) => Some(permit),
            Self::HandsOff(_) => None,
        }
    }
}

/// เขียน `.refx-meta` ไม่สำเร็จ
///
/// ★ ทุกตัวคือ "บันทึกไม่ได้" ซึ่ง **ไม่ใช่เหตุให้ล้มโปรแกรมหรือทิ้ง tag**
/// ผู้เรียกต้องบอกผู้ใช้แล้วทำงานต่อ (`docs/07 §5`: เขียนไม่ได้ ≠ เงียบ)
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// ★ มีคนแก้ไฟล์ระหว่างที่เราถืออยู่ — ปฏิเสธ ไม่ใช่ทับ
    #[error("{} changed since it was read - not overwriting it", path.display())]
    ChangedUnderneath {
        /// ไฟล์ที่เปลี่ยนไป
        path: PathBuf,
    },
    /// เข้ารหัสไม่สำเร็จ หรือใหญ่เกินเพดาน — ยังไม่ได้แตะดิสก์
    #[error("cannot encode the sidecar for {}: {source}", path.display())]
    Encode {
        /// ไฟล์ปลายทาง
        path: PathBuf,
        /// ต้นเหตุ
        source: EncodeError,
    },
    /// ล้มระหว่างแตะดิสก์ (โฟลเดอร์อ่านอย่างเดียว · แผ่นเต็ม · ไม่มีสิทธิ์)
    #[error("could not {step} {}: {source}", path.display())]
    Io {
        /// ขั้นที่ล้ม
        step: &'static str,
        /// ไฟล์ที่กำลังแตะตอนนั้น
        path: PathBuf,
        /// ต้นเหตุจากระบบไฟล์
        source: std::io::Error,
    },
    /// ★ เขียนแล้วแต่ขนาดบนดิสก์ไม่ตรงกับที่ตัวเข้ารหัสบอก (`docs/08 §3.9` ข้อ 14)
    #[error("{} is {found} bytes on disk but the encoder produced {expected}", path.display())]
    SizeMismatch {
        /// ไฟล์ที่ขนาดไม่ตรง
        path: PathBuf,
        /// ขนาดที่ควรเป็น
        expected: u64,
        /// ขนาดที่เจอจริง
        found: u64,
    },
}

impl WriteError {
    fn io(step: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            step,
            path: path.to_path_buf(),
            source,
        }
    }
}

// ---------------------------------------------------------------------------
// เส้นทางของไฟล์
// ---------------------------------------------------------------------------

/// `.refx-meta` ของโฟลเดอร์นี้
#[must_use]
pub fn path_for(dir: &Path) -> PathBuf {
    dir.join(SIDECAR_NAME)
}

/// ★★★ ไฟล์นี้เป็น sidecar ของเราเองไหม — **ทางเข้าภาพทุกเส้นต้องถาม**
///
/// เจอบนแอปจริง 11 ก.ย. 2026: ตั้งแต่วันที่เราเริ่มเขียนไฟล์ลงโฟลเดอร์ภาพ
/// การ "เปิดทั้งโฟลเดอร์" จะ **ดูดไฟล์ของตัวเองกลับเข้ามาเป็นภาพใบที่สี่**
/// แล้วขึ้นเป็นภาพเสียบน board ของผู้ใช้ (3 ไฟล์ → 4 items)
///
/// ★ ทางเข้าไม่ได้กรองนามสกุลโดยตั้งใจ — ไฟล์อะไรก็ลากเข้ามาได้และกลายเป็น
///   `Missing` ถ้าเปิดไม่ออก ซึ่งถูกตาม I-7 · แต่ **ไฟล์ที่เราสร้างเอง**
///   ไม่ใช่ของที่ผู้ใช้ลากเข้ามา และไม่ควรโผล่บน board เลยสักครั้ง
#[must_use]
pub fn is_sidecar(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == SIDECAR_NAME)
}

// ---------------------------------------------------------------------------
// การจับคู่ — หัวใจของ §5
// ---------------------------------------------------------------------------

/// ไฟล์ที่อยู่ในโฟลเดอร์จริง ๆ ตอนนี้
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Present {
    /// ชื่อไฟล์ล้วน
    pub file_name: String,
    /// blake3 ของเนื้อไฟล์ตอนนี้
    pub hash: ContentHash,
}

/// ★★★ จับคู่รายการใน sidecar กับไฟล์ที่มีอยู่จริง — **ชื่อชนะเสมอ**
///
/// คืน index ของ `sidecar.entries` สำหรับไฟล์แต่ละใบใน `present` (ตามลำดับเดิม)
///
/// 1. ชื่อตรง → ใช้เลย **แม้ hash ไม่ตรง** (แก้ภาพใน Photoshop แล้ว tag ต้องไม่หาย)
/// 2. ชื่อไม่ตรง → hash ที่ตรงและ **ไม่ซ้ำ** ในรายการที่ยังไม่มีใครจอง = การเปลี่ยนชื่อ
/// 3. ไม่ตรงทั้งคู่ → ไม่จับคู่ · และรายการนั้น **ยังต้องอยู่ในไฟล์ต่อไป**
///    (ดู [`orphans`]) เพราะไฟล์อาจแค่ถูกย้ายออกชั่วคราว
///
/// ★ ทำไมเป็นชุด: กติกาข้อ 2 พูดถึง "ไม่ซ้ำใน**รายการที่เหลือ**" ซึ่งนิยามได้
/// ก็ต่อเมื่อรู้ว่าใครถูกจองด้วยชื่อไปแล้วบ้าง — ถามทีละใบตอบคำถามนี้ไม่ได้เลย
#[must_use]
pub fn resolve(sidecar: &Sidecar, present: &[Present]) -> Vec<Option<usize>> {
    let mut out = vec![None; present.len()];
    let mut claimed = vec![false; sidecar.entries.len()];

    // --- รอบที่ 1: ชื่อ ---
    for (slot, file) in out.iter_mut().zip(present) {
        if let Some(at) = sidecar
            .entries
            .iter()
            .position(|entry| entry.file_name == file.file_name)
        {
            *slot = Some(at);
            claimed[at] = true;
        }
    }

    // --- รอบที่ 2: hash ที่ไม่ซ้ำ ในรายการที่ยังไม่ถูกจอง ---
    //
    // ★ ไฟล์ที่ hash ซ้ำกันเองก็ต้องไม่จับคู่ด้วย — สองสำเนาของภาพเดียวกันที่
    //   ถูกเปลี่ยนชื่อทั้งคู่ ไม่มีทางรู้ว่าใบไหนเคยเป็นใบไหน การเดาคือการ
    //   ย้าย tag ของผู้ใช้ไปผิดใบ ซึ่งแย่กว่าการไม่ย้ายเลย
    for (i, file) in present.iter().enumerate() {
        if out[i].is_some() {
            continue;
        }
        let twin_in_folder = present
            .iter()
            .enumerate()
            .any(|(j, other)| j != i && other.hash == file.hash);
        if twin_in_folder {
            continue;
        }
        let mut only = None;
        for (at, entry) in sidecar.entries.iter().enumerate() {
            if claimed[at] || entry.hash != file.hash {
                continue;
            }
            if only.is_some() {
                only = None; // ซ้ำ → ไม่จับคู่
                break;
            }
            only = Some(at);
        }
        if let Some(at) = only {
            out[i] = Some(at);
            claimed[at] = true;
        }
    }

    out
}

/// รายการที่ไม่มีไฟล์ไหนจับคู่ด้วย — ★ **ต้องเขียนกลับลงไฟล์เหมือนเดิม**
///
/// ไฟล์อาจแค่ถูกย้ายออกชั่วคราว (เสียบ USB คนละวัน · ย้ายไปโฟลเดอร์ย่อยแล้ว
/// ย้ายกลับ) · ลบรายการทิ้งคือการทำงานของผู้ใช้หาย ซึ่งเป็น I-3
/// — รูปเดียวกับ "ไฟล์เสียต้องกลายเป็น `Missing` ไม่ใช่หายไปจาก board"
#[must_use]
pub fn orphans(sidecar: &Sidecar, matched: &[Option<usize>]) -> Vec<Entry> {
    let mut taken = vec![false; sidecar.entries.len()];
    for at in matched.iter().flatten() {
        if let Some(slot) = taken.get_mut(*at) {
            *slot = true;
        }
    }
    sidecar
        .entries
        .iter()
        .enumerate()
        .filter(|(at, _)| !taken[*at])
        .map(|(_, entry)| entry.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// อ่าน
// ---------------------------------------------------------------------------

/// เปิด `.refx-meta` ของโฟลเดอร์นี้
///
/// ★★ **ทุกไบต์คือ input ที่ไม่น่าไว้ใจ** (I-4) — ห้าม panic ห้ามจอง memory
/// ตามตัวเลขที่อยู่ในไฟล์ · ไม่มีทางคืน `Err` เพราะทุกความล้มเหลวมีความหมาย
/// ที่ผู้เรียกต้องทำต่างกัน และ [`Load`] บอกเรื่องนั้นตรง ๆ
#[must_use]
pub fn load(path: &Path) -> Load {
    let stamp = Stamp::of(path);
    let Some(stamp) = stamp else {
        // ★ stat ไม่ได้ = ไม่มีไฟล์ **หรือ** ไม่มีสิทธิ์อ่านโฟลเดอร์
        //   ทั้งสองกรณีปลอดภัยที่จะถือว่า "ยังไม่มี" เพราะการเขียนจริงจะล้ม
        //   เองพร้อมข้อความที่บอกสาเหตุจริง (`WriteError::Io`)
        return Load::Fresh(WritePermit { seen: None });
    };
    // ★ ถามขนาดแล้วอ่านผ่าน `take` — รูปแบบเดียวกับ `read_file_guarded`
    //   (`clippy.toml` แบน `fs::read` ไว้ด้วยเหตุผลนี้: ไฟล์บอกขนาดเท่าไหร่ก็ได้)
    //
    //   ★★ ไฟล์ที่ใหญ่เกินเพดานคือ **`HandsOff` ไม่ใช่ `Fresh`** — มันอาจเป็น
    //      ของเครื่องมืออื่นที่บังเอิญชื่อเดียวกัน การถือว่า "ยังไม่มี" แล้ว
    //      เขียนทับคือการลบไฟล์ของคนอื่นทิ้ง
    if stamp.len > MAX_FILE_BYTES {
        return Load::HandsOff(LockReason::Damaged);
    }
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|file| {
        use std::io::Read as _;
        std::io::Read::take(file, MAX_FILE_BYTES).read_to_end(&mut bytes)
    });
    if read.is_err() {
        return Load::HandsOff(LockReason::Damaged);
    }
    match decode(&bytes) {
        Ok(sidecar) => Load::Opened(sidecar, WritePermit { seen: Some(stamp) }),
        Err(reason) => Load::HandsOff(reason),
    }
}

/// อ่านไบต์ของ `.refx-meta` กลับมาเป็น [`Sidecar`]
///
/// # Errors
/// [`LockReason`] เมื่อไฟล์ไม่ใช่ของเรา มาจากรุ่นใหม่กว่า หรือเนื้อเสีย
pub fn decode(bytes: &[u8]) -> Result<Sidecar, LockReason> {
    if bytes.len() < HEADER_LEN || bytes[..4] != MAGIC {
        return Err(LockReason::NotOurs);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version > SIDECAR_VERSION {
        return Err(LockReason::NewerVersion { found: version });
    }
    let declared = u64::from_le_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]);
    let crc = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);

    let body = &bytes[HEADER_LEN..];
    // ★ เทียบกับไบต์ที่ **มีจริง** ก่อนเสมอ — `declared` มาจากไฟล์จึงโกหกได้
    let declared = usize::try_from(declared).map_err(|_| LockReason::Damaged)?;
    if body.len() < declared {
        return Err(LockReason::Damaged);
    }
    let body = &body[..declared];
    if crc32fast::hash(body) != crc {
        return Err(LockReason::Damaged);
    }

    let raw = zstd::bulk::Decompressor::new()
        .and_then(|mut d| d.decompress(body, MAX_BODY_BYTES))
        .map_err(|_| LockReason::Damaged)?;
    let dto: v1::SidecarDto = postcard::from_bytes(&raw).map_err(|_| LockReason::Damaged)?;
    Ok(dto.into_sidecar())
}

// ---------------------------------------------------------------------------
// เขียน
// ---------------------------------------------------------------------------

/// เข้ารหัส `.refx-meta` ไม่สำเร็จ — ★ ยังไม่ได้แตะดิสก์เลย
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    /// ใหญ่เกิน [`MAX_BODY_BYTES`]
    #[error("the sidecar would be {size} bytes, over the {MAX_BODY_BYTES} limit")]
    TooLarge {
        /// ขนาดที่ได้
        size: usize,
    },
    /// postcard หรือ zstd ล้มเหลว — ★ ไม่ควรเกิดกับข้อมูลที่เราสร้างเอง
    #[error("cannot encode the sidecar")]
    Encode,
}

/// ไบต์ของไฟล์ `.refx-meta`
///
/// # Errors
/// [`EncodeError`] เมื่อเข้ารหัสไม่สำเร็จหรือใหญ่เกิน [`MAX_BODY_BYTES`]
pub fn encode(sidecar: &Sidecar) -> Result<Vec<u8>, EncodeError> {
    let dto = v1::SidecarDto::from_sidecar(sidecar);
    let raw = postcard::to_stdvec(&dto).map_err(|_| EncodeError::Encode)?;
    if raw.len() > MAX_BODY_BYTES {
        return Err(EncodeError::TooLarge { size: raw.len() });
    }
    let body = zstd::encode_all(raw.as_slice(), ZSTD_LEVEL).map_err(|_| EncodeError::Encode)?;

    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&SIDECAR_VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // flags — สงวนไว้
    out.extend_from_slice(&(body.len() as u64).to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(&body).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// เขียน `.refx-meta` แบบ atomic — tmp → fsync → rename → **ตรวจขนาด**
///
/// ★ ต้องมี [`WritePermit`] ซึ่งได้จาก [`load`] เท่านั้น — ไฟล์ที่อ่านไม่ได้
/// ไม่มีใบให้ จึงเขียนทับไม่ได้ตั้งแต่ตอนคอมไพล์
///
/// # Errors
/// [`WriteError`] — ★ ทุกตัวแปลว่า "บันทึกไม่ได้" ไม่ใช่ "โปรแกรมพัง"
/// **ไฟล์เดิม (ถ้ามี) ยังอยู่ครบเสมอ**
pub fn write_atomic(
    path: &Path,
    sidecar: &Sidecar,
    permit: &WritePermit,
    rename: RenameFn,
) -> Result<(), WriteError> {
    // ★ มีคนแก้ไฟล์ระหว่างที่เราถืออยู่ → ปฏิเสธ ไม่ใช่ทับ (`docs/07 §5`)
    //   เทียบ `Option` ตรง ๆ: เราจำว่า "ตอนนั้นไม่มีไฟล์" แล้วตอนนี้มี
    //   ก็คือมีคนมาสร้างระหว่างทาง ซึ่งต้องปฏิเสธเหมือนกัน
    if Stamp::of(path) != permit.seen {
        return Err(WriteError::ChangedUnderneath {
            path: path.to_path_buf(),
        });
    }

    let bytes = encode(sidecar).map_err(|source| WriteError::Encode {
        path: path.to_path_buf(),
        source,
    })?;
    let expected = bytes.len() as u64;

    let tmp = path.with_extension("tmp");
    {
        use std::io::Write as _;
        let mut file =
            std::fs::File::create(&tmp).map_err(|err| WriteError::io("create", &tmp, err))?;
        file.write_all(&bytes)
            .map_err(|err| WriteError::io("write", &tmp, err))?;
        // ★ sync_all คือขั้นที่แยก "เขียนแล้ว" ออกจาก "อยู่บนดิสก์แล้ว"
        file.sync_all()
            .map_err(|err| WriteError::io("flush", &tmp, err))?;
    }
    rename(&tmp, path).map_err(|err| {
        let _ = std::fs::remove_file(&tmp); // ★ ไม่ทิ้ง .tmp ไว้ในโฟลเดอร์ของผู้ใช้
        WriteError::io("replace", path, err)
    })?;

    // ★★ ยืนยันขนาดด้วยตัวตั้งจาก **ตัวเข้ารหัส** ไม่ใช่จากตัวเราเองที่เพิ่งเขียน
    //    (`docs/08 §3.9` ข้อ 14 — ด่านที่เทียบของกับตัวมันเองผ่านตลอดกาล)
    let found = std::fs::metadata(path)
        .map_err(|err| WriteError::io("check", path, err))?
        .len();
    if found != expected {
        return Err(WriteError::SizeMismatch {
            path: path.to_path_buf(),
            expected,
            found,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// DTO — แยกจากชนิดของโดเมนเสมอ (เหตุผลเดียวกับ `dto.rs`)
// ---------------------------------------------------------------------------

mod v1 {
    use super::{
        ColorLabel, ContentHash, Entry, MAX_ENTRIES, MAX_NAME_LEN, MAX_NOTE_LEN, MAX_TAG_LEN,
        MAX_TAGS_PER_ENTRY, Sidecar,
    };
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    pub(super) struct SidecarDto {
        pub entries: Vec<EntryDto>,
    }

    #[derive(Serialize, Deserialize)]
    pub(super) struct EntryDto {
        pub file_name: String,
        pub hash: [u8; 32],
        pub rating: u8,
        /// `ColorLabel::to_wire` · `0` = ไม่มีป้าย (เหมือน `dto.rs`)
        pub color_label: u8,
        pub note: String,
        pub pinned: bool,
        pub tags: Vec<String>,
    }

    impl SidecarDto {
        pub(super) fn from_sidecar(sidecar: &Sidecar) -> Self {
            Self {
                entries: sidecar
                    .entries
                    .iter()
                    .map(|entry| EntryDto {
                        file_name: entry.file_name.clone(),
                        hash: *entry.hash.as_bytes(),
                        rating: entry.rating,
                        color_label: entry.color_label.map_or(0, ColorLabel::to_wire),
                        note: entry.note.clone(),
                        pinned: entry.pinned,
                        tags: entry.tags.clone(),
                    })
                    .collect(),
            }
        }

        /// ★ bound ทุกคอลเลกชันตอนอ่านกลับ (I-4) — ไฟล์บอกอะไรมาก็ตัดตามเพดาน
        pub(super) fn into_sidecar(self) -> Sidecar {
            let entries = self
                .entries
                .into_iter()
                .take(MAX_ENTRIES)
                .filter(|dto| {
                    // ชื่อที่มีโฟลเดอร์ปนมาคือ path traversal — ทิ้งทั้งรายการ
                    !dto.file_name.is_empty()
                        && dto.file_name.len() <= MAX_NAME_LEN
                        && !dto.file_name.contains(['/', '\\'])
                        && dto.file_name != "."
                        && dto.file_name != ".."
                })
                .map(|dto| {
                    let mut note = dto.note;
                    note.truncate(floor_char_boundary(&note, MAX_NOTE_LEN));
                    let mut tags: Vec<String> = dto
                        .tags
                        .into_iter()
                        .take(MAX_TAGS_PER_ENTRY)
                        .map(|mut tag| {
                            tag.truncate(floor_char_boundary(&tag, MAX_TAG_LEN));
                            tag
                        })
                        .filter(|tag| !tag.trim().is_empty())
                        .collect();
                    tags.sort_unstable();
                    tags.dedup();
                    Entry {
                        file_name: dto.file_name,
                        hash: ContentHash::from_bytes(dto.hash),
                        rating: dto.rating.min(refx_core::board::ItemMeta::MAX_RATING),
                        color_label: ColorLabel::from_wire(dto.color_label),
                        note,
                        pinned: dto.pinned,
                        tags,
                    }
                })
                .collect();
            Sidecar::new(entries)
        }
    }

    /// ตัดสตริงที่ขอบอักขระ — ★ `String::truncate` panic ถ้าตัดกลาง UTF-8
    /// และข้อความไทยกินตัวละ 3 ไบต์ จึงตกกลางแทบทุกครั้งถ้าตัดดิบ ๆ
    fn floor_char_boundary(text: &str, limit: usize) -> usize {
        if text.len() <= limit {
            return text.len();
        }
        let mut at = limit;
        while at > 0 && !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    }
}

#[cfg(test)]
mod tests {
    // ★ `fs::read` ถูกแบนเพราะ **ดิสก์ I/O บน UI thread** (I-2) และการอ่านไฟล์
    //   ที่ขนาดไม่รู้จบ · ที่นี่อ่านไฟล์ที่เทสต์เพิ่งเขียนเอง เพื่อพิสูจน์ว่า
    //   **ไบต์ไม่เปลี่ยน** — ทางที่มีเพดานจะอ่านได้ไม่ครบแล้วพิสูจน์ไม่ได้
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::disallowed_methods
    )]

    use super::*;
    use refx_platform::fsops::rename_durable;

    /// hash ปลอมที่ **ขึ้นกับเนื้อ** — พอสำหรับที่นี่ และตั้งใจไม่ใช่ blake3 จริง
    ///
    /// สิ่งที่เทสต์ชุดนี้ตรวจคือ **การจับคู่** ซึ่งสนใจแค่ว่าสองค่าเท่ากันไหม
    /// ลาก blake3 เข้ามาเป็น dependency ของ `refx-io` เพื่อเรื่องนี้คือการเพิ่ม
    /// ของที่ `fuzz/` ต้องคอมไพล์ตามโดยไม่ได้อะไรกลับมา
    fn hash_of(bytes: &[u8]) -> ContentHash {
        let mut out = [0u8; 32];
        let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
        for (i, byte) in bytes.iter().enumerate() {
            acc = (acc ^ u64::from(*byte)).wrapping_mul(0x1000_0000_01b3);
            out[i % 32] ^= (acc >> ((i % 8) * 8)) as u8;
        }
        out[0] ^= bytes.len() as u8;
        ContentHash::from_bytes(out)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-sidecar-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(name: &str, content: &[u8], rating: u8) -> Entry {
        Entry {
            file_name: name.to_owned(),
            hash: hash_of(content),
            rating,
            color_label: None,
            note: String::new(),
            pinned: false,
            tags: vec!["ref".to_owned()],
        }
    }

    fn present(name: &str, content: &[u8]) -> Present {
        Present {
            file_name: name.to_owned(),
            hash: hash_of(content),
        }
    }

    // ---------- รูปแบบไฟล์ ----------

    #[test]
    fn a_sidecar_survives_a_round_trip() {
        let sidecar = Sidecar::new(vec![
            Entry {
                file_name: "ภาพมังกร.png".to_owned(),
                hash: hash_of(b"dragon"),
                rating: 5,
                color_label: Some(ColorLabel::Red),
                note: "ท่าทางที่อยากได้".to_owned(),
                pinned: true,
                tags: vec!["pose".to_owned(), "แสง".to_owned()],
            },
            entry("cat.jpg", b"cat", 3),
        ]);
        let bytes = encode(&sidecar).unwrap();
        assert_eq!(&bytes[..4], &MAGIC, "ลายเซ็นต้องไม่ใช่ของ .refx");
        assert_eq!(decode(&bytes).unwrap(), sidecar);
    }

    #[test]
    fn a_file_that_is_not_ours_is_left_alone() {
        assert_eq!(decode(b"REFX\x01\x00").unwrap_err(), LockReason::NotOurs);
        assert_eq!(decode(b"").unwrap_err(), LockReason::NotOurs);
        assert_eq!(decode(b"hello there").unwrap_err(), LockReason::NotOurs);
    }

    /// ★★★ ไฟล์จากรุ่นใหม่กว่าต้องแยกออกจาก "ไฟล์เสีย" — คนละการกระทำต่อ
    #[test]
    fn a_newer_version_is_recognised_as_newer_not_as_damage() {
        let mut bytes = encode(&Sidecar::new(vec![entry("a.png", b"a", 1)])).unwrap();
        bytes[4..6].copy_from_slice(&(SIDECAR_VERSION + 1).to_le_bytes());
        assert_eq!(
            decode(&bytes).unwrap_err(),
            LockReason::NewerVersion {
                found: SIDECAR_VERSION + 1
            }
        );
    }

    #[test]
    fn flipped_bytes_are_caught_by_the_checksum() {
        let mut bytes = encode(&Sidecar::new(vec![entry("a.png", b"a", 1)])).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        assert_eq!(decode(&bytes).unwrap_err(), LockReason::Damaged);
    }

    #[test]
    fn a_truncated_file_is_damage_not_a_panic() {
        let bytes = encode(&Sidecar::new(vec![entry("a.png", b"a", 1)])).unwrap();
        for cut in 0..bytes.len() {
            // ★ ห้าม panic ที่ความยาวไหนเลย (I-4)
            let _ = decode(&bytes[..cut]);
        }
        assert_eq!(
            decode(&bytes[..bytes.len() - 1]).unwrap_err(),
            LockReason::Damaged
        );
    }

    /// ★ ชื่อไฟล์ที่มีโฟลเดอร์ปนมาคือ path traversal — ต้องถูกทิ้งตอนอ่าน
    #[test]
    fn a_name_with_a_folder_in_it_never_survives_the_read() {
        let evil = Sidecar {
            entries: vec![
                Entry {
                    file_name: "../../../etc/passwd".to_owned(),
                    ..entry("x", b"x", 4)
                },
                Entry {
                    file_name: r"..\..\windows\system32\evil.png".to_owned(),
                    ..entry("x", b"x", 4)
                },
                entry("ok.png", b"ok", 2),
            ],
        };
        let back = decode(&encode(&evil).unwrap()).unwrap();
        assert_eq!(back.entries.len(), 1);
        assert_eq!(back.entries[0].file_name, "ok.png");
    }

    /// ★ โน้ตภาษาไทยยาวเกินเพดานต้องถูกตัดที่ขอบอักขระ ไม่ใช่ panic
    #[test]
    fn an_over_long_thai_note_is_trimmed_not_a_panic() {
        let long = "ก".repeat(MAX_NOTE_LEN); // 3 ไบต์ต่อตัว = เกินเพดานแน่
        let sidecar = Sidecar::new(vec![Entry {
            note: long,
            ..entry("a.png", b"a", 1)
        }]);
        let back = decode(&encode(&sidecar).unwrap()).unwrap();
        assert!(back.entries[0].note.len() <= MAX_NOTE_LEN);
        assert!(!back.entries[0].note.is_empty(), "ตัดจนหมดก็ผิด");
    }

    /// ★★★ ไฟล์ของเราเองต้องไม่โผล่บน board ของผู้ใช้ (เจอบนแอปจริง)
    #[test]
    fn our_own_file_is_never_mistaken_for_a_picture() {
        assert!(is_sidecar(Path::new("E:/ภาพ/.refx-meta")));
        assert!(is_sidecar(&path_for(Path::new("E:/ภาพ"))));
        // ★ ประตูของประตู: ต้องปฏิเสธได้จริง ไม่ใช่ตอบ true เสมอ
        assert!(!is_sidecar(Path::new("E:/ภาพ/cat.png")));
        assert!(!is_sidecar(Path::new("E:/ภาพ/.refx-meta.bak")));
        assert!(!is_sidecar(Path::new("E:/ภาพ/my.refx")));
        assert!(
            !is_sidecar(Path::new("E:/.refx-meta/cat.png")),
            "โฟลเดอร์ชื่อนี้ไม่ใช่ไฟล์นี้"
        );
    }

    // ---------- การจับคู่: หัวใจของ §5 ----------

    /// ★★★ NC: **เปลี่ยนชื่อไฟล์ → tag ตามไป**
    #[test]
    fn renaming_a_file_carries_its_tags_along() {
        let sidecar = Sidecar::new(vec![entry("img1.jpg", b"dragon pixels", 5)]);
        let now = [present("dragon-pose.jpg", b"dragon pixels")];

        let matched = resolve(&sidecar, &now);
        assert_eq!(matched, vec![Some(0)], "hash ตรงและไม่ซ้ำ = การเปลี่ยนชื่อ");
        assert!(orphans(&sidecar, &matched).is_empty());
    }

    /// ★★★ NC: **แก้เนื้อไฟล์ → tag อยู่ที่เดิม**
    ///
    /// ผู้ใช้เปิดใน Photoshop แล้วเซฟทับ · hash เปลี่ยนทั้งใบ แต่ "ช่องนี้"
    /// ยังเป็นของเดิมสำหรับเขา · ถ้า hash เป็นกุญแจ ดาว 5 ดวงจะหายเงียบ ๆ
    #[test]
    fn editing_a_file_in_photoshop_keeps_its_tags() {
        let sidecar = Sidecar::new(vec![entry("dragon.png", b"before", 5)]);
        let now = [present("dragon.png", b"after an edit")];

        let matched = resolve(&sidecar, &now);
        assert_eq!(matched, vec![Some(0)], "ชื่อตรง = ใช้เลย แม้ hash ไม่ตรง");
    }

    /// ★★ ชื่อชนะเสมอเมื่อสองกติกาขัดกัน — สลับชื่อไฟล์สองใบ
    #[test]
    fn when_the_two_rules_disagree_the_name_wins() {
        let sidecar = Sidecar::new(vec![
            Entry {
                rating: 1,
                ..entry("a.png", b"pixels of A", 1)
            },
            Entry {
                rating: 2,
                ..entry("b.png", b"pixels of B", 2)
            },
        ]);
        // ผู้ใช้สลับชื่อ: ไฟล์ที่ชื่อ a.png ตอนนี้มีเนื้อของ B
        let now = [
            present("a.png", b"pixels of B"),
            present("b.png", b"pixels of A"),
        ];

        let matched = resolve(&sidecar, &now);
        assert_eq!(matched, vec![Some(0), Some(1)], "ชื่อต้องชนะ hash");
        assert_eq!(sidecar.entries[matched[0].unwrap()].rating, 1);
    }

    /// ★★ hash ที่ซ้ำกันต้องไม่จับคู่ — เดาผิดคือย้าย tag ไปผิดใบ
    #[test]
    fn two_copies_of_the_same_picture_are_never_guessed_at() {
        let sidecar = Sidecar::new(vec![
            entry("one.png", b"identical", 5),
            entry("two.png", b"identical", 1),
        ]);
        let now = [
            present("renamed-a.png", b"identical"),
            present("renamed-b.png", b"identical"),
        ];

        let matched = resolve(&sidecar, &now);
        assert_eq!(matched, vec![None, None], "เดาไม่ได้ = ไม่เดา");
        assert_eq!(orphans(&sidecar, &matched).len(), 2, "ทั้งคู่ต้องยังอยู่");
    }

    /// ★★★ ไม่ตรงทั้งคู่ = **ห้ามลบรายการ** — ไฟล์อาจแค่ถูกย้ายออกชั่วคราว
    #[test]
    fn a_file_that_stepped_out_keeps_its_row() {
        let sidecar = Sidecar::new(vec![
            entry("still-here.png", b"here", 3),
            entry("on-a-usb-stick.png", b"elsewhere", 5),
        ]);
        let now = [present("still-here.png", b"here")];

        let matched = resolve(&sidecar, &now);
        let left = orphans(&sidecar, &matched);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].file_name, "on-a-usb-stick.png");
        assert_eq!(left[0].rating, 5, "ดาวต้องไม่หายไปกับไฟล์ที่ถูกถอดออก");
    }

    /// ★ ประตูของประตู: ตัวจับคู่ต้องตอบ `None` ได้จริง ไม่ใช่ตอบ `Some` เสมอ
    /// (`docs/08 §3.9` ข้อ 1 — ด่านที่ไม่เคยปฏิเสธคือด่านที่ไม่มีอยู่)
    #[test]
    fn a_brand_new_file_matches_nothing() {
        let sidecar = Sidecar::new(vec![entry("old.png", b"old", 4)]);
        let now = [present("brand-new.png", b"never seen before")];
        assert_eq!(resolve(&sidecar, &now), vec![None]);
    }

    // ---------- เขียน / อ่านจากดิสก์จริง ----------

    #[test]
    fn a_folder_with_no_sidecar_is_fresh_and_writable() {
        let dir = temp_dir("fresh");
        let path = path_for(&dir);
        let opened = load(&path);
        assert!(opened.sidecar().is_none());
        let permit = *opened.permit().expect("โฟลเดอร์ว่างต้องเขียนได้");

        let sidecar = Sidecar::new(vec![entry("a.png", b"a", 4)]);
        write_atomic(&path, &sidecar, &permit, rename_durable).unwrap();
        assert!(path.exists());

        match load(&path) {
            Load::Opened(back, _) => assert_eq!(back, sidecar),
            other => panic!("เขียนแล้วต้องอ่านกลับได้ แต่ได้ {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ ไฟล์ `.tmp` ต้องไม่ค้างในโฟลเดอร์ของผู้ใช้
    #[test]
    fn writing_leaves_nothing_but_the_sidecar_behind() {
        let dir = temp_dir("tidy");
        let path = path_for(&dir);
        let permit = *load(&path).permit().unwrap();
        write_atomic(
            &path,
            &Sidecar::new(vec![entry("a.png", b"a", 4)]),
            &permit,
            rename_durable,
        )
        .unwrap();

        let left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect();
        assert_eq!(left, vec![SIDECAR_NAME.to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ NC ของ §5: **`.refx-meta` ที่ใหม่กว่าต้อง byte-identical หลังจบเซสชัน**
    ///
    /// ไม่มี `WritePermit` ให้ = โค้ดที่พยายามเขียนทับ **คอมไพล์ไม่ผ่าน**
    /// เทสต์นี้จึงยืนยันสองอย่าง: ตัวโหลดจัดชั้นถูก และไฟล์ไม่ถูกแตะจริง
    #[test]
    fn a_sidecar_from_a_newer_build_is_never_touched() {
        let dir = temp_dir("newer");
        let path = path_for(&dir);

        let mut bytes = encode(&Sidecar::new(vec![entry("theirs.png", b"theirs", 5)])).unwrap();
        bytes[4..6].copy_from_slice(&(SIDECAR_VERSION + 1).to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let before = std::fs::read(&path).unwrap();

        let opened = load(&path);
        match &opened {
            Load::HandsOff(LockReason::NewerVersion { found }) => {
                assert_eq!(*found, SIDECAR_VERSION + 1);
            }
            other => panic!("ต้องจัดเป็น HandsOff แต่ได้ {other:?}"),
        }
        assert!(opened.permit().is_none(), "ห้ามมีใบอนุญาตเขียนให้");

        // จบเซสชัน — ไฟล์ต้องเหมือนเดิมทุกไบต์
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            !path.with_extension("tmp").exists(),
            "ห้ามทิ้งร่องรอยไว้ในโฟลเดอร์ของผู้ใช้แม้แต่ไฟล์ชั่วคราว"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ ไฟล์เสียก็ห้ามแตะเหมือนกัน — มันอาจเป็นงานของผู้ใช้ที่กู้ได้ด้วยมือ
    #[test]
    fn a_damaged_sidecar_is_never_overwritten_either() {
        let dir = temp_dir("damaged");
        let path = path_for(&dir);
        let mut bytes = encode(&Sidecar::new(vec![entry("a.png", b"a", 5)])).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();
        let before = std::fs::read(&path).unwrap();

        let opened = load(&path);
        assert!(matches!(opened, Load::HandsOff(LockReason::Damaged)));
        assert!(opened.permit().is_none());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ มีคนแก้ไฟล์ระหว่างที่เราถืออยู่ → **ปฏิเสธ ไม่ใช่ทับ**
    #[test]
    fn someone_else_editing_it_first_stops_the_write() {
        let dir = temp_dir("race");
        let path = path_for(&dir);
        let permit = *load(&path).permit().unwrap();

        // คนอื่น (เครื่องมืออื่น · RefX อีกหน้าต่าง) เขียนของเขาลงไปก่อน
        std::fs::write(
            &path,
            encode(&Sidecar::new(vec![entry("theirs.png", b"t", 5)])).unwrap(),
        )
        .unwrap();
        let theirs = std::fs::read(&path).unwrap();

        let err = write_atomic(
            &path,
            &Sidecar::new(vec![entry("mine.png", b"m", 1)]),
            &permit,
            rename_durable,
        )
        .unwrap_err();
        assert!(
            matches!(err, WriteError::ChangedUnderneath { .. }),
            "ได้ {err:?}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), theirs, "ของเขาต้องยังอยู่ครบ");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ NC ของ §5: **โฟลเดอร์อ่านอย่างเดียว → ไม่ล้ม และบอกได้ว่าเพราะอะไร**
    ///
    /// ★ ทำให้โฟลเดอร์เขียนไม่ได้จริงด้วย ACL ของ Windows ไม่ใช่แค่ธง read-only
    /// ซึ่ง Windows ไม่บังคับกับการสร้างไฟล์ในโฟลเดอร์ (`docs/08 §3.9` ข้อ 9:
    /// เครื่องมือที่ผลิตหลักฐานต้องพิสูจน์ก่อนว่าตัวมันเองไม่โกหก)
    #[cfg(windows)]
    #[test]
    fn a_read_only_folder_reports_why_instead_of_crashing() {
        let dir = temp_dir("readonly");
        let path = path_for(&dir);
        let permit = *load(&path).permit().unwrap();

        let who = std::env::var("USERNAME").unwrap_or_else(|_| "Users".to_owned());
        let deny = std::process::Command::new("icacls")
            .arg(&dir)
            .arg("/deny")
            .arg(format!("{who}:(WD,AD)"))
            .output();
        let denied = deny.map(|o| o.status.success()).unwrap_or(false);
        if !denied {
            // ★ ข้ามต้องไม่เงียบ (`docs/08 §3.9` ข้อ 2)
            eprintln!("ข้าม: ตั้ง ACL ปฏิเสธการเขียนไม่สำเร็จบนเครื่องนี้");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        // ★ ยืนยันว่าด่านที่เราสร้างเองใช้ได้จริงก่อนจะเชื่อผลข้างล่าง
        assert!(
            std::fs::write(dir.join("probe.txt"), b"x").is_err(),
            "ACL ไม่ได้ผล — ผลของเทสต์นี้จะไม่มีความหมาย"
        );

        let err = write_atomic(
            &path,
            &Sidecar::new(vec![entry("a.png", b"a", 4)]),
            &permit,
            rename_durable,
        )
        .unwrap_err();
        // ต้องเป็น "บันทึกไม่ได้" ที่บอกไฟล์และขั้นตอน ไม่ใช่ panic
        match &err {
            WriteError::Io { step, path: at, .. } => {
                assert!(!step.is_empty());
                assert!(at.to_string_lossy().contains(SIDECAR_NAME) || at.ends_with("tmp"));
            }
            other => panic!("ต้องเป็น Io แต่ได้ {other:?}"),
        }
        assert!(!err.to_string().is_empty(), "ข้อความต้องบอกผู้ใช้ได้");

        let _ = std::process::Command::new("icacls")
            .arg(&dir)
            .arg("/remove:d")
            .arg(&who)
            .output();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ รายการว่างเปล่าไม่ต้องกินที่ในไฟล์ของผู้ใช้
    #[test]
    fn blank_rows_are_not_written_at_all() {
        let sidecar = Sidecar::new(vec![
            Entry {
                tags: Vec::new(),
                ..entry("nothing.png", b"n", 0)
            },
            entry("something.png", b"s", 4),
        ]);
        assert_eq!(sidecar.entries.len(), 1);
        assert_eq!(sidecar.entries[0].file_name, "something.png");
    }

    /// ★ ผลต้อง deterministic — ไฟล์ที่เนื้อเท่ากันต้องได้ไบต์เท่ากันเสมอ
    /// ไม่งั้น `.refx-meta` จะถูกเขียนใหม่ทุกครั้งที่เปิดโปรแกรม
    #[test]
    fn the_same_rows_always_encode_to_the_same_bytes() {
        let one = Sidecar::new(vec![entry("b.png", b"b", 2), entry("a.png", b"a", 1)]);
        let two = Sidecar::new(vec![entry("a.png", b"a", 1), entry("b.png", b"b", 2)]);
        assert_eq!(encode(&one).unwrap(), encode(&two).unwrap());
    }
}
