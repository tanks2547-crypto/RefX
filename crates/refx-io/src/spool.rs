//! ★★★ Spool ของภาพที่ **ไม่มีไฟล์ต้นทาง** — `<data_local_dir>/pasted/<hash>.png`
//!
//! ## ทำไมต้องมี
//!
//! `arboard` คืน RGBA ไม่ใช่ไบต์ของไฟล์ · ภาพที่วางจาก clipboard จึงมีอยู่
//! **แค่ใน RAM กับใน board** — ไม่มีอะไรบนดิสก์ที่ชี้ถึงมันได้
//!
//! เขียนลง spool **ตั้งแต่ตอน paste ไม่ใช่ตอน save** แล้วได้สามอย่างพร้อมกัน
//! (`docs/07 §2`):
//!
//! | ได้อะไร | |
//! |---|---|
//! | ภาพมีไฟล์จริง | ขอ working texture ได้ทันที — **ปลดหนี้ P1-8 ตรงนั้นเลย ไม่ต้องรอ save** |
//! | `plan_embeds` ตอบ `Ours(path)` ได้ | ชั้น io ไม่ต้องรู้จัก encoder |
//! | เปิดไฟล์ packed → แตก blob กลับลง spool | สมมาตรทั้งสองทาง |
//!
//! ★ อยู่ `data_local_dir` **ไม่ใช่ `cache_dir`** ด้วยเหตุผลเดียวกับ `recovery/`
//! เป๊ะ: cache คือที่ของสิ่งที่สร้างใหม่ได้ · ภาพจาก clipboard **สร้างใหม่ไม่ได้
//! จากอะไรเลย** ถ้า eviction ลบมัน ผู้ใช้เสียภาพถาวร
//!
//! ★ ตั้งชื่อด้วย hash → ภาพเดียวกันวางซ้ำสิบครั้งได้ไฟล์เดียว **ฟรี**
//!
//! ## ★★★ กับดักที่ [`sweep`] มีไว้กัน
//!
//! **snapshot ที่กู้คืนได้ มีค่าเท่ากับ asset ที่มันอ้างถึงเท่านั้น**
//!
//! ถ้า spool ถูกเก็บกวาดไปแล้วแต่ recovery snapshot ยังอยู่ → ผู้ใช้กด "เอากลับมา"
//! แล้วได้ board ที่เต็มไปด้วย `Missing` — ซึ่ง **แย่กว่าไม่มี snapshot เลย**
//! เพราะเขาเชื่อไปแล้วว่ากู้สำเร็จ แล้วเลิกมองหางานชุดนั้นต่อ
//!
//! กติกา (`docs/07 §2`):
//! 1. ห้ามลบไฟล์ที่ **board ที่เปิดอยู่** อ้างถึง
//! 2. ★ ห้ามลบไฟล์ที่ **recovery snapshot** อ้างถึง ← ข้อที่มองข้ามง่ายที่สุด
//! 3. ที่เหลือใช้เพดาน — **เป็นไบต์ ไม่ใช่จำนวนไฟล์** ([`MAX_BYTES`])
//!
//! ★★ ข้อ 2 ที่นี่ **กว้างกว่าตัวหนังสือใน spec**: spec เขียนว่า "snapshot ที่ยัง
//! ไม่ถูกตอบ" ส่วนที่นี่คุ้มครอง **snapshot ทุกไฟล์ที่ยังอยู่ในโฟลเดอร์**
//! — เพราะ snapshot ที่ผู้ใช้ตอบว่า "ทิ้ง" ถูกลบไปแล้วจึงไม่อยู่ในรายการอยู่ดี
//! ส่วนที่ตอบว่า **"เก็บไว้ก่อน" ยังกู้ได้** การถือว่ามัน "ตอบแล้ว" จึงทำให้
//! ตัวเลือกที่สามกลายเป็นคำโกหก · เลือกทางที่ปลอดภัยกว่าตาม `CLAUDE.md`
//!
//! spec: docs/07-file-format.md §2, ROADMAP P4-5

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use refx_core::board::{Board, ItemKind};
use refx_core::hash::ContentHash;

use crate::save::{RenameFn, SaveError};

/// นามสกุลของไฟล์ใน spool — PNG เพราะไม่มีการสูญเสีย (ภาพที่วางจะถูกฝังต่อ)
pub const SPOOL_EXT: &str = "png";

/// ★★★ เพดานของไฟล์ที่ **กวาดได้** — เป็น **ไบต์ ไม่ใช่จำนวนไฟล์** (I-6)
///
/// เดิมเขียนเป็น `MAX_KEPT = 64` ไฟล์ ซึ่งตั้งไว้ **ก่อน** จะรู้ว่า PNG ของภาพ
/// 6000×4000 มีขนาด **78 MB** (วัดแล้วใน `refx_asset::encode`) — เพดานจริงจึง
/// กลายเป็น **~5 GB** โดยไม่มีใครตั้งใจ แล้วมันจะเต็มดิสก์ผู้ใช้เงียบ ๆ
///
/// ★ ไฟล์ที่กติกาข้อ 1/2 คุ้มครอง **ไม่นับเข้าเพดานนี้และไม่ถูกลบ ไม่ว่าใหญ่แค่ไหน**
/// — เพดานที่ไล่ของที่กู้ไม่ได้แล้วออกไป คือเพดานที่กลายเป็นตัวทำงานหายเสียเอง
/// ดู [`Swept::protected_over_cap`] สำหรับสภาพที่ต้อง **บอกผู้ใช้ ไม่ใช่ลบ**
pub const MAX_BYTES: u64 = 512 << 20;

/// เก็บไฟล์ที่ไม่มีใครอ้างถึงได้นานแค่ไหน
pub const MAX_AGE: Duration = crate::recovery::MAX_AGE;

/// ที่อยู่ของภาพนี้ใน spool
///
/// ★ ชื่อมาจาก hash ล้วน ๆ → **ภาพเดียวกันวางซ้ำกี่ครั้งก็ไฟล์เดียว**
/// และไม่มีทางที่ชื่อจะหลุดออกนอกโฟลเดอร์ (hex 64 ตัว ไม่มีตัวคั่น path)
#[must_use]
pub fn spool_path(dir: &Path, hash: ContentHash) -> PathBuf {
    dir.join(format!("{hash}.{SPOOL_EXT}"))
}

/// ★★ เก็บไบต์ของภาพที่วางลง spool — **atomic เหมือนทุกการเขียนในโปรเจกต์นี้**
///
/// ผู้เรียกส่ง **ไบต์ที่ encode แล้ว** เข้ามา (`refx-io` ไม่รู้จัก encoder และ
/// พึ่ง `image` ไม่ได้) · การ encode เป็นงานของ worker ใน `refx-asset`
/// ซึ่ง `docs/07 §2` บังคับว่าต้องไม่หน่วงการที่ภาพขึ้นจอ
///
/// ★ ไฟล์เดิมที่มีอยู่แล้วถือว่า **สำเร็จทันที** — ชื่อคือ hash ของเนื้อ
/// เนื้อจึงเหมือนกันแน่นอน การเขียนทับมีแต่จะเปลืองดิสก์กับเสี่ยงโดยเปล่าประโยชน์
///
/// # Errors
/// [`SaveError`] เมื่อเขียนไม่สำเร็จ
pub fn store(
    dir: &Path,
    hash: ContentHash,
    bytes: &[u8],
    rename: RenameFn,
) -> Result<PathBuf, SaveError> {
    let path = spool_path(dir, hash);
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(dir)
        .map_err(|err| SaveError::io("create the spool folder for", &path, err))?;
    crate::save::write_bytes_atomic(&path, bytes, rename)?;
    Ok(path)
}

/// แกะ blob ออกจากเอกสาร packed ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum UnpackError {
    /// ตารางหรือ blob ในไฟล์ไม่สมเหตุสมผล (รวม checksum ไม่ตรง)
    #[error(transparent)]
    Read(#[from] crate::dto::OpenError),
    /// เขียนลง spool ไม่ได้ (ดิสก์เต็ม / ไม่มีสิทธิ์)
    #[error(transparent)]
    Write(#[from] SaveError),
}

/// ★★★ แกะ blob จากเอกสาร packed **กลับลง spool** — ทางกลับของ [`store`]
///
/// `docs/07 §2` บังคับให้สองทิศสมมาตรกัน: ภาพที่วางถูก encode ลง spool แล้ว
/// ฝังเข้า `.refx` ตอนบันทึก · เปิดกลับมาบนเครื่องที่ไม่มีไฟล์ต้นฉบับเลย
/// blob ต้องกลายเป็นไฟล์จริงอีกครั้งเพื่อให้ **ขอ working texture ได้**
/// (ชั้น decode รับ *ไฟล์* ไม่ใช่ไบต์ในเอกสาร)
///
/// ★★ **สตรีมทั้งเส้น** — เอกสาร packed ใหญ่ระดับ GB ได้ ไม่มีจุดไหนที่ถือ
/// blob ทั้งก้อนไว้ใน RAM (เหตุผลเดียวกับ [`crate::packed::write_packed`])
///
/// ★★★ **ไฟล์ที่มีอยู่แล้ว ถือว่าสำเร็จทันที ไม่เขียนทับเด็ดขาด** — เหมือน
/// [`store`] แต่ที่นี่มันเป็น *เกราะ* ไม่ใช่แค่การประหยัด: `hash` ในตารางมาจาก
/// ไฟล์ ซึ่ง I-4 บอกว่าโกหกได้ · ถ้ายอมเขียนทับ เอกสารที่ถูกดัดแปลงจะแทนที่
/// ภาพที่ผู้ใช้วางไว้เอง (ซึ่งไม่มีต้นฉบับอยู่ที่อื่นแล้ว) ได้ด้วยการอ้าง hash
/// ของมัน · เขียนใหม่เฉพาะชื่อที่ยังว่างจึงไม่มีของใครถูกแทนที่ได้เลย
///
/// # Errors
/// [`UnpackError`] — blob เสีย (crc ไม่ตรง) หรือเขียนไฟล์ไม่ได้
/// · **ของที่เขียนค้างถูกลบทิ้ง** ไม่มีไฟล์ครึ่ง ๆ ค้างใน spool
pub fn unpack<R: std::io::Read + std::io::Seek>(
    dir: &Path,
    entry: crate::packed::Entry,
    source: &mut R,
    rename: RenameFn,
) -> Result<PathBuf, UnpackError> {
    let path = spool_path(dir, entry.hash);
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(dir)
        .map_err(|err| SaveError::io("create the spool folder for", &path, err))?;

    // ★ tmp → fsync → rename เหมือนทุกการเขียนในโปรเจกต์นี้ · ผู้อ่านคนอื่น
    //   (sweep / decode pool) จึงไม่มีวันเห็นไฟล์ที่เขียนค้างอยู่
    let tmp = path.with_extension("tmp");
    let outcome = (|| -> Result<(), UnpackError> {
        let mut file =
            std::fs::File::create(&tmp).map_err(|err| SaveError::io("create", &tmp, err))?;
        crate::packed::extract(source, entry, &mut file)?;
        file.sync_all()
            .map_err(|err| SaveError::io("flush", &tmp, err))?;
        Ok(())
    })();
    if let Err(err) = outcome {
        // ★ crc ไม่ตรง = ไบต์ที่เขียนไปแล้วเชื่อไม่ได้ ต้องไม่เหลือไว้ให้ใครหยิบไปใช้
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    rename(&tmp, &path).map_err(|err| SaveError::io("replace", &path, err))?;
    Ok(path)
}

/// hash ทุกตัวที่ board นี้อ้างถึง
#[must_use]
pub fn hashes_of(board: &Board) -> BTreeSet<ContentHash> {
    board
        .items_in_z_order()
        .filter_map(|(_, item)| match &item.kind {
            ItemKind::Image(asset) => Some(asset.hash),
            _ => None,
        })
        .collect()
}

/// ผลของการเก็บกวาด — ตัวเลขที่บอกว่ากติกาทำงานจริงไหม
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Swept {
    /// ลบไปกี่ไฟล์
    pub removed: usize,
    /// ★ กี่ไฟล์ที่รอดเพราะยังมีคนอ้างถึง (board ที่เปิดอยู่ หรือ snapshot)
    ///
    /// แยกจาก `removed` เพื่อให้อ่านออกว่า "ไม่มีอะไรให้ลบ" ต่างจาก
    /// "มีของแต่ห้ามแตะ" — สองสภาพนี้บอกคนละเรื่องตอนตามปัญหา
    pub kept_because_referenced: usize,
    /// ★★ ไบต์รวมของไฟล์ที่ถูกคุ้มครอง — **ไม่ถูกนับเข้าเพดาน**
    ///
    /// มีไว้ตอบคำถามเดียว: *เพดานไม่ได้ทำงานเพราะไม่มีอะไรให้ทำ หรือเพราะ
    /// ทุกอย่างห้ามแตะ?* ([`Self::protected_over_cap`])
    pub protected_bytes: u64,
    /// ไบต์รวมของไฟล์ที่กวาดได้แต่เก็บไว้ (ยังไม่ชนเพดาน)
    pub kept_bytes: u64,
    /// ไบต์รวมที่ลบออกไปจริง
    pub removed_bytes: u64,
}

impl Swept {
    /// ★★★ ไฟล์ที่ **ห้ามลบ** อย่างเดียวก็เกินเพดานแล้ว → ต้อง **บอกผู้ใช้**
    ///
    /// `docs/07 §2`: *"ถ้าไฟล์ที่ถูกคุ้มครองอย่างเดียวก็เกิน 512 MB แล้ว →
    /// บอกผู้ใช้ ห้ามลบ"* รูปแบบเดียวกับ "board เต็ม" ใน `ROADMAP P3-3` —
    /// เงียบไว้แล้วลบทิ้งคือการทำงานของผู้ใช้หายโดยที่เขาไม่มีทางรู้ว่าเกิดอะไร
    #[must_use]
    pub fn protected_over_cap(&self, cap: u64) -> bool {
        self.protected_bytes > cap
    }
}

/// ★★★ เก็บกวาด spool — **ลบได้เฉพาะไฟล์ที่ไม่มีใครอ้างถึงเลย**
///
/// `referenced` คือ hash ที่ **ห้ามแตะ** ผู้เรียกประกอบจาก board ที่เปิดอยู่
/// รวมกับ [`referenced_by_recovery`] (ดูหัวโมดูลว่าทำไมข้อหลังสำคัญที่สุด)
///
/// ★ `now` ถูกส่งเข้ามา เทสต์จึงเดินเวลาไป 40 วันได้โดยไม่ต้องรอ
/// (`docs/08 §3.9` ข้อ 5b)
///
/// ★★ `max_bytes` เป็น **ไบต์รวมของไฟล์ที่กวาดได้** (ดู [`MAX_BYTES`]) —
/// ไฟล์ที่ [`referenced`](sweep) คุ้มครองไม่ถูกนับและไม่ถูกลบไม่ว่าจะใหญ่แค่ไหน
pub fn sweep(
    dir: &Path,
    referenced: &BTreeSet<ContentHash>,
    max_bytes: u64,
    max_age: Duration,
    now: SystemTime,
) -> Swept {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Swept::default(); // ยังไม่มีโฟลเดอร์ = ไม่มีอะไรให้กวาด
    };

    let mut found: Vec<(PathBuf, Option<SystemTime>, u64, bool)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == SPOOL_EXT))
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() {
                return None;
            }
            let in_use = hash_of_file_name(&path).is_some_and(|hash| referenced.contains(&hash));
            Some((path, meta.modified().ok(), meta.len(), in_use))
        })
        .collect();
    // ใหม่สุดก่อน · ตัดสินด้วยชื่อไฟล์เมื่อเวลาเท่ากัน (ลำดับต้อง deterministic)
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut result = Swept::default();
    for (path, written_at, size, in_use) in found {
        // ★★★ ด่านแรกและด่านเดียวที่สำคัญ: มีคนอ้างถึงอยู่ = ห้ามแตะ
        //     ไม่ว่าจะเก่าแค่ไหนหรือใหญ่แค่ไหน — และ **ไม่นับเข้าเพดาน**
        //     ถ้ามันนับ ไฟล์ที่กู้ไม่ได้แล้วจะไล่ไฟล์ที่ยังกู้ได้ออกไปแทน
        if in_use {
            result.kept_because_referenced += 1;
            result.protected_bytes = result.protected_bytes.saturating_add(size);
            continue;
        }
        let too_old =
            written_at.is_some_and(|at| now.duration_since(at).is_ok_and(|age| age > max_age));
        // ★ เพดานนับ **ไบต์** — ไฟล์เดียวขนาด 78 MB กินเพดานเท่ากับไฟล์เล็ก 78,000 ใบ
        let fits = result.kept_bytes.saturating_add(size) <= max_bytes;
        if fits && !too_old {
            result.kept_bytes = result.kept_bytes.saturating_add(size);
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                result.removed += 1;
                result.removed_bytes = result.removed_bytes.saturating_add(size);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(%err, path = %path.display(), "cannot sweep a spooled image")
            }
        }
    }
    if result.removed > 0 {
        tracing::info!(
            removed = result.removed,
            removed_bytes = result.removed_bytes,
            kept = result.kept_because_referenced,
            protected_bytes = result.protected_bytes,
            "swept the paste spool"
        );
    }
    // ★ ไม่ลบอะไรเลยเพราะทุกอย่างห้ามแตะ = สภาพที่ผู้ใช้ต้องรู้ ไม่ใช่ความเงียบ
    if result.protected_over_cap(max_bytes) {
        tracing::warn!(
            protected_bytes = result.protected_bytes,
            cap = max_bytes,
            "the paste spool is over its cap but every file in it is still referenced"
        );
    }
    result
}

/// ★★★ hash ที่ **recovery snapshot ทุกไฟล์ที่ยังอยู่** อ้างถึง
///
/// นี่คือกติกาข้อ 2 (ดูหัวโมดูล) · ต้องเรียกแล้วรวมเข้ากับ hash ของ board
/// ที่เปิดอยู่ก่อนส่งให้ [`sweep`] เสมอ
///
/// ★ อ่านทุก snapshot ในโฟลเดอร์ — งานดิสก์จริง จึงต้องอยู่บน worker (I-2)
/// · snapshot ที่อ่านไม่ออกถูกข้าม (มันกู้อะไรไม่ได้อยู่แล้ว จึงไม่มี asset
/// ที่ต้องคุ้มครองแทนมัน)
#[must_use]
pub fn referenced_by_recovery(
    recovery_dir: &Path,
    id: refx_core::arena::BoardId,
) -> BTreeSet<ContentHash> {
    let Ok(entries) = std::fs::read_dir(recovery_dir) else {
        return BTreeSet::new();
    };
    let mut all = BTreeSet::new();
    for path in entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == crate::recovery::SNAPSHOT_EXT)
        })
    {
        if let Some(board) = crate::recovery::load(&path, id) {
            all.extend(hashes_of(&board));
        }
    }
    all
}

/// แปลงชื่อไฟล์กลับเป็น hash — `None` = ชื่อไม่ใช่ hex 64 ตัว (ไฟล์ของคนอื่น)
fn hash_of_file_name(path: &Path) -> Option<ContentHash> {
    let stem = path.file_stem()?.to_str()?;
    if stem.len() != 64 {
        return None;
    }
    let mut raw = [0u8; 32];
    for (slot, pair) in raw.iter_mut().zip(stem.as_bytes().chunks_exact(2)) {
        let text = std::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(text, 16).ok()?;
    }
    Some(ContentHash::from_bytes(raw))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use refx_core::arena::{ArenaKey as _, BoardId};
    use refx_core::board::{AssetRef, BoardParts, ImageFormat, Item, ItemParts};
    use refx_platform::fsops::rename_durable;

    fn board_id() -> BoardId {
        BoardId::from_parts(0, 0)
    }

    fn hash_of(n: u8) -> ContentHash {
        ContentHash::from_bytes([n; 32])
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-spool-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn board_of(hashes: &[u8]) -> Board {
        Board::load(
            board_id(),
            BoardParts {
                name: "spool".to_owned(),
                items: hashes
                    .iter()
                    .map(|n| ItemParts {
                        item: Item::new(ItemKind::Image(AssetRef {
                            hash: hash_of(*n),
                            path: PathBuf::from(format!("pasted-{n}.png")),
                            px_size: glam::UVec2::new(32, 32),
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

    /// ไฟล์ขนาด 128 ไบต์ — ขนาดคือสิ่งที่เพดานนับ จึงเขียนไว้ตรงนี้ที่เดียว
    const SPOOLED_BYTES: u64 = 128;

    fn spool(dir: &Path, n: u8) -> PathBuf {
        let path = store(
            dir,
            hash_of(n),
            &[n; SPOOLED_BYTES as usize],
            rename_durable,
        )
        .unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), SPOOLED_BYTES);
        path
    }

    /// เพดานที่กว้างพอสำหรับ `n` ไฟล์ของ [`spool`] พอดี ๆ
    fn cap_for(n: u64) -> u64 {
        SPOOLED_BYTES * n
    }

    // ---------- ชื่อไฟล์ ----------

    /// ★ ภาพเดียวกันวางซ้ำต้องได้ไฟล์เดียว — คีย์คือเนื้อ ไม่ใช่จำนวนครั้งที่วาง
    #[test]
    fn pasting_the_same_image_ten_times_uses_one_file() {
        let dir = temp_dir("dedup");
        let mut paths = Vec::new();
        for _ in 0..10 {
            paths.push(store(&dir, hash_of(1), b"same bytes", rename_durable).unwrap());
        }
        assert!(paths.windows(2).all(|w| w[0] == w[1]));
        let count = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, 1, "ได้ {count} ไฟล์แทนที่จะเป็นไฟล์เดียว");
    }

    /// ชื่อไฟล์ต้องแปลงกลับเป็น hash ได้ — ไม่งั้น `sweep` แยกไม่ออกว่าใครเป็นใคร
    #[test]
    fn a_spooled_name_round_trips_back_to_its_hash() {
        let dir = Path::new("/tmp/pasted");
        for n in [0u8, 1, 0x7f, 0xff] {
            let path = spool_path(dir, hash_of(n));
            assert_eq!(hash_of_file_name(&path), Some(hash_of(n)));
            assert_eq!(path.parent(), Some(dir), "ชื่อหลุดออกนอกโฟลเดอร์");
        }
        // ไฟล์ของคนอื่นในโฟลเดอร์เดียวกันต้องไม่ถูกตีความเป็น hash
        assert_eq!(hash_of_file_name(Path::new("/tmp/pasted/notes.png")), None);
        assert_eq!(hash_of_file_name(Path::new("/tmp/pasted/zz.png")), None);
    }

    // ---------- ★★★ กติกาเก็บกวาด ----------

    /// ★★★ **ห้ามลบไฟล์ที่ board ที่เปิดอยู่อ้างถึง** (กติกาข้อ 1)
    #[test]
    fn an_image_on_the_open_board_is_never_swept() {
        let dir = temp_dir("open-board");
        let used = spool(&dir, 2);
        let stale = spool(&dir, 3);

        let referenced = hashes_of(&board_of(&[2]));
        // เพดาน 0 + อายุ 0 = กวาดทุกอย่างที่กวาดได้
        let swept = sweep(&dir, &referenced, 0, Duration::ZERO, SystemTime::now());

        assert!(used.exists(), "ลบภาพที่อยู่บน board ที่เปิดอยู่");
        assert!(!stale.exists(), "ไฟล์ที่ไม่มีใครอ้างถึงต้องถูกกวาด");
        assert_eq!(swept.removed, 1);
        assert_eq!(swept.kept_because_referenced, 1);
        // ★ ไฟล์ที่ถูกคุ้มครองอยู่นอกเพดาน — ทั้งที่เพดานคือ 0 มันก็ยังอยู่
        assert_eq!(swept.protected_bytes, SPOOLED_BYTES);
        assert_eq!(swept.kept_bytes, 0);
        assert_eq!(swept.removed_bytes, SPOOLED_BYTES);
    }

    /// ★★★ **ห้ามลบไฟล์ที่ recovery snapshot อ้างถึง** (กติกาข้อ 2)
    ///
    /// ข้อที่มองข้ามง่ายที่สุด และผลของมันคือ **"กู้คืนสำเร็จแต่ภาพหายหมด"** —
    /// ผู้ใช้เชื่อไปแล้วว่าได้งานคืน แล้วเลิกมองหางานชุดนั้นต่อ ซึ่งแย่กว่า
    /// ไม่มี snapshot เลย
    #[test]
    fn an_image_only_a_recovery_snapshot_knows_about_is_never_swept() {
        let dir = temp_dir("snapshot-ref");
        let spool_dir = dir.join("pasted");
        let recovery_dir = dir.join("recovery");
        std::fs::create_dir_all(&spool_dir).unwrap();
        std::fs::create_dir_all(&recovery_dir).unwrap();

        // ภาพที่วางไว้แล้วยังไม่เคย save — อยู่ใน snapshot เท่านั้น
        let pasted = store(&spool_dir, hash_of(4), &[4; 64], rename_durable).unwrap();
        let session = crate::recovery::SessionId::new_unique();
        crate::recovery::write_snapshot(&recovery_dir, &session, &board_of(&[4]), rename_durable)
            .unwrap();

        // board ที่เปิดอยู่ตอนนี้ **ว่างเปล่า** (เพิ่งเปิดโปรแกรมใหม่)
        let mut referenced = hashes_of(&Board::default());
        referenced.extend(referenced_by_recovery(&recovery_dir, board_id()));

        let swept = sweep(
            &spool_dir,
            &referenced,
            0,
            Duration::ZERO,
            SystemTime::now(),
        );

        assert!(
            pasted.exists(),
            "ลบภาพที่มีแต่ snapshot รู้จัก — กู้คืนแล้วจะได้ board ที่เต็มไปด้วย Missing"
        );
        assert_eq!(swept.removed, 0);
        assert_eq!(swept.kept_because_referenced, 1);
    }

    /// ★ negative control ของข้อบน — **ลืมถาม `referenced_by_recovery` แล้วภาพหาย**
    ///
    /// เขียนไว้เพื่อให้เห็นด้วยตาว่าข้อ 2 มีผลจริง ไม่ใช่ข้อที่เขียนไว้เฉย ๆ
    /// (ถ้าไม่มีเทสต์นี้ ข้อบนจะเขียวเท่ากันแม้ `referenced_by_recovery` คืนเซตว่าง)
    #[test]
    fn forgetting_the_snapshot_rule_is_what_loses_the_image() {
        let dir = temp_dir("forgot");
        let spool_dir = dir.join("pasted");
        let recovery_dir = dir.join("recovery");
        std::fs::create_dir_all(&spool_dir).unwrap();
        std::fs::create_dir_all(&recovery_dir).unwrap();

        let pasted = store(&spool_dir, hash_of(5), &[5; 64], rename_durable).unwrap();
        let session = crate::recovery::SessionId::new_unique();
        crate::recovery::write_snapshot(&recovery_dir, &session, &board_of(&[5]), rename_durable)
            .unwrap();

        // ★ จงใจ "ลืม" ข้อ 2 — ถามแค่ board ที่เปิดอยู่
        let referenced = hashes_of(&Board::default());
        sweep(
            &spool_dir,
            &referenced,
            0,
            Duration::ZERO,
            SystemTime::now(),
        );

        assert!(
            !pasted.exists(),
            "ลืมข้อ 2 แล้วภาพยังอยู่ — แปลว่าเทสต์ข้อ 2 ไม่ได้พิสูจน์อะไร"
        );
        // และนี่คือสภาพที่ผู้ใช้จะเจอ: snapshot ยังอยู่ แต่ของที่มันอ้างถึงหายแล้ว
        assert!(
            crate::recovery::snapshot_path(&recovery_dir, &session).exists(),
            "snapshot ยังอยู่ ทั้งที่ของที่มันอ้างถึงถูกลบไปแล้ว"
        );
    }

    /// ★ snapshot ที่ผู้ใช้ตอบ "เก็บไว้ก่อน" ยังกู้ได้ → asset ของมันต้องรอดด้วย
    ///
    /// spec เขียนว่า "snapshot ที่ยังไม่ถูกตอบ" แต่ที่นี่คุ้มครองทุกไฟล์ที่ยังอยู่
    /// — ตัวที่ถูกตอบว่า "ทิ้ง" ถูกลบไปแล้วจึงไม่อยู่ในรายการอยู่ดี
    /// ส่วนตัวที่ตอบว่า "เก็บไว้ก่อน" **ยังกู้ได้** การถือว่ามันจบแล้วจะทำให้
    /// ตัวเลือกที่สามของ P4-4 กลายเป็นคำโกหก
    #[test]
    fn a_snapshot_the_user_kept_for_later_still_protects_its_images() {
        let dir = temp_dir("later");
        let spool_dir = dir.join("pasted");
        let recovery_dir = dir.join("recovery");
        std::fs::create_dir_all(&spool_dir).unwrap();
        std::fs::create_dir_all(&recovery_dir).unwrap();

        let pasted = store(&spool_dir, hash_of(6), &[6; 64], rename_durable).unwrap();
        let session = crate::recovery::SessionId::new_unique();
        crate::recovery::write_snapshot(&recovery_dir, &session, &board_of(&[6]), rename_durable)
            .unwrap();
        // ผู้ใช้เห็นแล้วและเลือก "เก็บไว้ก่อน" → มีไฟล์ประทับ แต่ snapshot ยังอยู่
        crate::recovery::mark_asked(&crate::recovery::snapshot_path(&recovery_dir, &session));

        let referenced = referenced_by_recovery(&recovery_dir, board_id());
        sweep(
            &spool_dir,
            &referenced,
            0,
            Duration::ZERO,
            SystemTime::now(),
        );

        assert!(pasted.exists(), "ตอบ 'เก็บไว้ก่อน' แล้วภาพของมันถูกลบ");
    }

    /// เพดานไบต์/อายุใช้ได้กับไฟล์ที่ไม่มีใครอ้างถึงเท่านั้น
    #[test]
    fn the_cap_only_ever_applies_to_unreferenced_files() {
        let dir = temp_dir("cap");
        let mut planted = Vec::new();
        for n in 10..16u8 {
            planted.push(spool(&dir, n));
            std::thread::sleep(Duration::from_millis(12));
        }
        // อ้างถึงตัวที่เก่าที่สุดสองใบ — มันต้องรอดทั้งที่อยู่ท้ายรายการ
        let referenced = hashes_of(&board_of(&[10, 11]));

        let swept = sweep(&dir, &referenced, cap_for(2), MAX_AGE, SystemTime::now());

        assert!(planted[0].exists() && planted[1].exists(), "ตัวที่ถูกอ้างถึงหาย");
        assert!(planted[5].exists() && planted[4].exists(), "ตัวใหม่สุดหาย");
        assert_eq!(swept.removed, 2, "ต้องลบสองใบกลาง ๆ ที่ไม่มีใครอ้างถึง");
        assert_eq!(swept.kept_because_referenced, 2);
        // ★ ที่คุ้มครองสองใบ **ไม่ถูกนับ** — ไม่งั้นเพดาน 2 ใบจะเต็มไปแล้วตั้งแต่
        //   ยังไม่ถึงไฟล์ที่กวาดได้สักใบ แล้วมันจะลบตัวใหม่สุดทิ้งหมด
        assert_eq!(swept.protected_bytes, cap_for(2));
        assert_eq!(swept.kept_bytes, cap_for(2));
    }

    /// ★★★ **เพดานนับไบต์ ไม่ใช่จำนวนไฟล์** (แก้ 19 ส.ค. 2026)
    ///
    /// เทสต์นี้ล้มเป็นกับโค้ดรุ่นก่อน: `MAX_KEPT = 64` **ไฟล์** ปล่อยให้ทั้งสอง
    /// ใบอยู่ต่อ เพราะ 2 < 64 · ของจริงที่ตัวเลขนั้นแปลว่าอะไรคือ 64 × 78 MB
    /// = **~5 GB** ซึ่งเต็มดิสก์ผู้ใช้โดยไม่มีใครตั้งใจ (`docs/07 §2`)
    #[test]
    fn the_cap_counts_bytes_not_files() {
        let dir = temp_dir("bytes");
        let older = spool(&dir, 40);
        std::thread::sleep(Duration::from_millis(12));
        let newer = spool(&dir, 41);

        // เพดานกว้างพอสำหรับ **ไฟล์เดียว** ทั้งที่มีสองไฟล์
        let swept = sweep(
            &dir,
            &BTreeSet::new(),
            cap_for(1),
            MAX_AGE,
            SystemTime::now(),
        );

        assert!(newer.exists(), "ตัวใหม่สุดต้องอยู่ก่อนเสมอ");
        assert!(!older.exists(), "เพดานนับไฟล์อยู่ — สองไฟล์เล็ก ๆ ผ่านไปได้ทั้งคู่");
        assert_eq!(swept.removed, 1);
        assert_eq!(swept.kept_bytes, cap_for(1));
        assert_eq!(swept.removed_bytes, SPOOLED_BYTES);
    }

    /// ★★★ ทุกไฟล์ถูกคุ้มครอง + เกินเพดาน → **บอกผู้ใช้ ห้ามลบ**
    ///
    /// `docs/07 §2`: *"ถ้าไฟล์ที่ถูกคุ้มครองอย่างเดียวก็เกิน 512 MB แล้ว →
    /// บอกผู้ใช้ ห้ามลบ"* — เพดานต้องไม่กลายเป็นตัวทำงานหาย (`ROADMAP P3-3`)
    #[test]
    fn a_spool_full_of_protected_files_is_reported_never_deleted() {
        let dir = temp_dir("protected-over-cap");
        let planted: Vec<PathBuf> = (50..53u8).map(|n| spool(&dir, n)).collect();
        let referenced = hashes_of(&board_of(&[50, 51, 52]));

        // เพดานแคบกว่าของที่มีอยู่ทั้งหมด
        let cap = cap_for(1);
        let swept = sweep(&dir, &referenced, cap, Duration::ZERO, SystemTime::now());

        assert!(planted.iter().all(|p| p.exists()), "ลบไฟล์ที่ห้ามลบเพราะเพดาน");
        assert_eq!(swept.removed, 0);
        assert_eq!(swept.protected_bytes, cap_for(3));
        assert!(
            swept.protected_over_cap(cap),
            "ไม่มีอะไรบอกผู้ใช้ว่า spool เกินเพดานแล้วแต่แตะอะไรไม่ได้"
        );
    }

    /// ★ negative control ของข้อบน — ไม่มีของที่ถูกคุ้มครอง = ไม่ต้องไปกวนผู้ใช้
    #[test]
    fn a_spool_the_sweep_can_actually_trim_never_bothers_the_user() {
        let dir = temp_dir("under-cap");
        for n in 60..63u8 {
            spool(&dir, n);
        }

        let cap = cap_for(1);
        let swept = sweep(&dir, &BTreeSet::new(), cap, MAX_AGE, SystemTime::now());

        assert_eq!(swept.removed, 2, "เพดานไม่ได้ทำงาน");
        assert!(
            !swept.protected_over_cap(cap),
            "ไม่มีไฟล์ที่ถูกคุ้มครองสักใบ แต่ยังเตือนผู้ใช้"
        );
    }

    /// เพดานที่ประกาศไว้ต้องเป็น **ไบต์** และใหญ่พอสำหรับ PNG จริงหลายใบ
    ///
    /// ★ อ้างค่าคงของสัญญาโดยตรง ไม่ hard-code เลขซ้ำ (`HANDOFF §4` ข้อ 21)
    #[test]
    fn the_declared_cap_is_the_one_the_spec_decided() {
        assert_eq!(MAX_BYTES, 512 << 20, "docs/07 §2 ตัดสินไว้ที่ 512 MB");
        // PNG ของภาพ 6000×4000 = 78 MB (วัดไว้ใน refx_asset::encode)
        const {
            assert!(MAX_BYTES > 78 << 20, "เพดานเล็กกว่าภาพที่วางได้หนึ่งใบ")
        }
    }

    /// เก่ากว่าเพดานอายุก็ถูกกวาด ถึงจะยังไม่เกินจำนวน — แต่ยังต้องผ่านด่านข้อ 1/2
    #[test]
    fn age_sweeps_even_when_the_count_is_fine() {
        let dir = temp_dir("age");
        let old = spool(&dir, 20);
        let referenced = BTreeSet::new();
        let in_40_days = SystemTime::now() + Duration::from_secs(40 * 24 * 60 * 60);

        let swept = sweep(&dir, &referenced, MAX_BYTES, MAX_AGE, in_40_days);

        assert!(!old.exists());
        assert_eq!(swept.removed, 1);
    }

    /// ไฟล์ของคนอื่นในโฟลเดอร์ต้องไม่ถูกแตะ และไม่ทำให้การกวาดล้ม (I-4)
    #[test]
    fn files_that_are_not_ours_are_left_alone() {
        let dir = temp_dir("junk");
        spool(&dir, 30);
        let note = dir.join("readme.txt");
        std::fs::write(&note, "ผู้ใช้วางอะไรไว้ก็ได้").unwrap();

        sweep(&dir, &BTreeSet::new(), 0, Duration::ZERO, SystemTime::now());

        assert!(note.exists(), "ลบไฟล์ที่ไม่ใช่ของเรา");
    }

    /// กวาดโฟลเดอร์ที่ยังไม่มี = เงียบ ไม่ใช่ error
    #[test]
    fn sweeping_a_folder_that_does_not_exist_is_quiet() {
        let dir = temp_dir("missing").join("never-created");
        assert_eq!(
            sweep(&dir, &BTreeSet::new(), 0, Duration::ZERO, SystemTime::now()),
            Swept::default()
        );
    }

    // ---------- ★★★ ทางกลับ: เอกสาร packed → spool ----------

    /// สร้างเอกสาร packed ที่ฝังไฟล์เดียว แล้วคืน `(path ของเอกสาร, ไบต์ที่ฝัง)`
    fn packed_document(dir: &Path, n: u8, bytes: usize) -> (PathBuf, Vec<u8>) {
        let body: Vec<u8> = (0..bytes).map(|i| (i as u8).wrapping_mul(n | 1)).collect();
        let source = dir.join(format!("source-{n}.png"));
        std::fs::write(&source, &body).unwrap();
        let doc = dir.join(format!("packed-{n}.refx"));
        let mut file = std::fs::File::create(&doc).unwrap();
        crate::packed::write_packed(
            &mut file,
            &board_of(&[n]),
            &[crate::packed::PackSource {
                hash: hash_of(n),
                path: source,
            }],
        )
        .unwrap();
        file.sync_all().unwrap();
        (doc, body)
    }

    fn index_of(doc: &Path) -> (std::fs::File, crate::packed::Index) {
        let mut file = std::fs::File::open(doc).unwrap();
        let len = file.metadata().unwrap().len();
        let index = crate::packed::read_index(&mut file, len).unwrap();
        (file, index)
    }

    fn read_all(path: &Path) -> Vec<u8> {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    }

    /// ★★★ **ภาพที่ฝังไว้ต้องกลายเป็นไฟล์จริงอีกครั้ง ไบต์ต่อไบต์**
    ///
    /// นี่คือครึ่งที่ทำให้ *"ลบโฟลเดอร์ต้นฉบับทิ้งแล้วยังเปิดได้"* เป็นจริง —
    /// ถ้าไบต์ไม่ตรง ภาพจะ decode ไม่ออกหรือได้ภาพผิดใบ
    #[test]
    fn an_embedded_image_becomes_a_real_file_again() {
        let dir = temp_dir("unpack");
        // ★ ใหญ่กว่าบัฟเฟอร์ 64 KB หลายรอบ — เส้นทางสตรีมต้องถูกเดินจริง
        let (doc, body) = packed_document(&dir, 7, 150_000);
        let (mut file, index) = index_of(&doc);
        let entry = index.find(hash_of(7)).expect("ไม่มี entry ของภาพที่ฝังไว้");

        let spool_dir = dir.join("pasted");
        let path = unpack(&spool_dir, entry, &mut file, rename_durable).unwrap();

        assert_eq!(path, spool_path(&spool_dir, hash_of(7)), "ชื่อไฟล์ไม่ใช่ hash");
        assert_eq!(read_all(&path), body, "ไบต์ที่แกะออกมาไม่ตรงกับที่ฝังไว้");
        // ★ เรียกซ้ำต้องเงียบและได้ที่เดิม (เปิดเอกสารเดิมสองครั้งเป็นเรื่องปกติ)
        let again = unpack(&spool_dir, entry, &mut file, rename_durable).unwrap();
        assert_eq!(again, path);
    }

    /// ★★★ **blob ที่เสียต้องไม่ทิ้งไฟล์ครึ่ง ๆ ไว้ให้ใครหยิบไปใช้**
    ///
    /// ★★ input ถูกต้องทุกอย่างยกเว้นสิ่งที่กำลังทดสอบ (`docs/08 §3.9` ข้อ 1b):
    /// พลิกไบต์ใน **blob** ซึ่ง `table_crc` ไม่ได้ครอบอยู่แล้ว — ตารางจึงยัง
    /// ผ่านทุกด่าน และของที่ล้มคือ crc ของ blob ตัวเดียว ซึ่งคือด่านที่ทดสอบพอดี
    #[test]
    fn a_corrupted_blob_leaves_nothing_behind() {
        use std::io::{Seek as _, SeekFrom, Write as _};

        let dir = temp_dir("unpack-corrupt");
        let (doc, _) = packed_document(&dir, 9, 4_096);
        let (mut file, index) = index_of(&doc);
        let entry = index.find(hash_of(9)).unwrap();
        drop(file);

        // พลิกไบต์กลาง blob — ตารางยังถูกต้องครบทุกช่อง
        let mut writable = std::fs::OpenOptions::new().write(true).open(&doc).unwrap();
        writable
            .seek(SeekFrom::Start(entry.offset + entry.len / 2))
            .unwrap();
        writable.write_all(&[0xAB]).unwrap();
        writable.sync_all().unwrap();
        drop(writable);

        file = std::fs::File::open(&doc).unwrap();
        let spool_dir = dir.join("pasted");
        let err = unpack(&spool_dir, entry, &mut file, rename_durable)
            .expect_err("blob ที่ถูกแก้ไบต์กลางต้องไม่ผ่าน");
        assert!(matches!(
            err,
            UnpackError::Read(crate::dto::OpenError::Corrupt)
        ));
        let left: Vec<_> = std::fs::read_dir(&spool_dir)
            .map(|entries| entries.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "เหลือไฟล์ค้างไว้: {left:?}");
    }

    /// ★★★ **เอกสารห้ามเขียนทับภาพที่อยู่ใน spool อยู่แล้ว** (I-4)
    ///
    /// `hash` ในตารางมาจากไฟล์ จึงอ้างเป็นอะไรก็ได้ · ถ้าการแกะยอมเขียนทับ
    /// เอกสารที่ถูกดัดแปลงจะแทนที่ภาพที่ผู้ใช้วางไว้เอง — ซึ่ง**ไม่มีต้นฉบับ
    /// อยู่ที่อื่นแล้ว** (I-3 เงียบที่สุด)
    #[test]
    fn the_document_never_replaces_an_image_already_in_the_spool() {
        let dir = temp_dir("unpack-poison");
        let (doc, body) = packed_document(&dir, 11, 2_048);
        let (mut file, index) = index_of(&doc);
        let entry = index.find(hash_of(11)).unwrap();

        // ภาพของผู้ใช้ที่อยู่ก่อนแล้วภายใต้คีย์เดียวกัน
        let spool_dir = dir.join("pasted");
        let mine = store(
            &spool_dir,
            hash_of(11),
            b"the image I pasted",
            rename_durable,
        )
        .unwrap();

        let path = unpack(&spool_dir, entry, &mut file, rename_durable).unwrap();

        assert_eq!(path, mine);
        assert_eq!(
            read_all(&mine),
            b"the image I pasted",
            "เอกสารเขียนทับภาพที่ผู้ใช้วางไว้"
        );
        assert_ne!(read_all(&mine), body);
    }
}
