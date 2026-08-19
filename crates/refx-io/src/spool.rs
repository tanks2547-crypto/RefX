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
//! 3. ที่เหลือใช้เพดานแบบเดียวกับ `recovery/`
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

/// เก็บไฟล์ที่ไม่มีใครอ้างถึงได้กี่ไฟล์ (เพดานแบบเดียวกับ `recovery/` — I-6)
pub const MAX_KEPT: usize = 64;

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
}

/// ★★★ เก็บกวาด spool — **ลบได้เฉพาะไฟล์ที่ไม่มีใครอ้างถึงเลย**
///
/// `referenced` คือ hash ที่ **ห้ามแตะ** ผู้เรียกประกอบจาก board ที่เปิดอยู่
/// รวมกับ [`referenced_by_recovery`] (ดูหัวโมดูลว่าทำไมข้อหลังสำคัญที่สุด)
///
/// ★ `now` ถูกส่งเข้ามา เทสต์จึงเดินเวลาไป 40 วันได้โดยไม่ต้องรอ
/// (`docs/08 §3.9` ข้อ 5b)
pub fn sweep(
    dir: &Path,
    referenced: &BTreeSet<ContentHash>,
    keep: usize,
    max_age: Duration,
    now: SystemTime,
) -> Swept {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Swept::default(); // ยังไม่มีโฟลเดอร์ = ไม่มีอะไรให้กวาด
    };

    let mut found: Vec<(PathBuf, Option<SystemTime>, bool)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == SPOOL_EXT))
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() {
                return None;
            }
            let in_use = hash_of_file_name(&path).is_some_and(|hash| referenced.contains(&hash));
            Some((path, meta.modified().ok(), in_use))
        })
        .collect();
    // ใหม่สุดก่อน · ตัดสินด้วยชื่อไฟล์เมื่อเวลาเท่ากัน (ลำดับต้อง deterministic)
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut result = Swept::default();
    let mut kept = 0usize;
    for (path, written_at, in_use) in found {
        // ★★★ ด่านแรกและด่านเดียวที่สำคัญ: มีคนอ้างถึงอยู่ = ห้ามแตะ
        //     ไม่ว่าจะเก่าแค่ไหนหรือเกินเพดานแค่ไหน
        if in_use {
            result.kept_because_referenced += 1;
            continue;
        }
        let too_old =
            written_at.is_some_and(|at| now.duration_since(at).is_ok_and(|age| age > max_age));
        if kept < keep && !too_old {
            kept += 1;
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => result.removed += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(%err, path = %path.display(), "cannot sweep a spooled image")
            }
        }
    }
    if result.removed > 0 {
        tracing::info!(
            removed = result.removed,
            kept = result.kept_because_referenced,
            "swept the paste spool"
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

    fn spool(dir: &Path, n: u8) -> PathBuf {
        store(dir, hash_of(n), &[n; 128], rename_durable).unwrap()
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

    /// เพดานจำนวน/อายุใช้ได้กับไฟล์ที่ไม่มีใครอ้างถึงเท่านั้น
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

        let swept = sweep(&dir, &referenced, 2, MAX_AGE, SystemTime::now());

        assert!(planted[0].exists() && planted[1].exists(), "ตัวที่ถูกอ้างถึงหาย");
        assert!(planted[5].exists() && planted[4].exists(), "ตัวใหม่สุดหาย");
        assert_eq!(swept.removed, 2, "ต้องลบสองใบกลาง ๆ ที่ไม่มีใครอ้างถึง");
        assert_eq!(swept.kept_because_referenced, 2);
    }

    /// เก่ากว่าเพดานอายุก็ถูกกวาด ถึงจะยังไม่เกินจำนวน — แต่ยังต้องผ่านด่านข้อ 1/2
    #[test]
    fn age_sweeps_even_when_the_count_is_fine() {
        let dir = temp_dir("age");
        let old = spool(&dir, 20);
        let referenced = BTreeSet::new();
        let in_40_days = SystemTime::now() + Duration::from_secs(40 * 24 * 60 * 60);

        let swept = sweep(&dir, &referenced, MAX_KEPT, MAX_AGE, in_40_days);

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
}
