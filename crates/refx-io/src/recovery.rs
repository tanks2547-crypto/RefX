//! ★★★ Autosave ของงานที่ **ยังไม่เคยบันทึกลง path จริง** (P4-4)
//!
//! ```text
//! <data_dir>/recovery/<session-id>.refx     ← snapshot · DTO เดียวกับ .refx เป๊ะ
//! <data_dir>/recovery/<session-id>.asked    ← ★ "ผู้ใช้เคยเห็นไฟล์นี้แล้ว"
//! ```
//!
//! ## ทำไมต้องมีทั้งที่ P4-3 ทำ autosave ไปแล้ว
//!
//! `<doc>.refx.autosave` ของ [`crate::autosave`] ต้องมี `<doc>` ก่อน — มันวางไฟล์
//! **ข้างเอกสาร** · คนที่จัดงานมาสามชั่วโมงโดยยังไม่เคยกด `Ctrl+S` จึงไม่มี
//! อะไรคุ้มครองเลยสักอย่าง ทั้งที่เขาคือคนที่เสียมากที่สุดถ้าโปรแกรมตาย
//! — และเป็นตัวอย่างที่ `CLAUDE.md` ยกมาตรง ๆ ("mood board ที่จัดมา 3 ชั่วโมง")
//!
//! ★ นโยบายว่า "เมื่อไหร่ควรเขียน" ใช้ [`crate::autosave::Autosaver`] **ตัวเดียวกัน**
//! ที่นี่ต่างแค่ *ที่อยู่ปลายทาง* — สองด่าน (`dirty` + เว้นระยะ) จึงเหมือนกันเป๊ะ
//! และ I-1 ยังจริงเหมือนเดิม (ไม่แก้อะไร = ไม่เขียนดิสก์เลย)
//!
//! ## ★★★ ที่อยู่: `data_dir` ไม่ใช่ `cache_dir`
//!
//! `docs/07 §4` ห้ามวางใน `cache_dir` เด็ดขาด · cache คือที่ของสิ่งที่สร้างใหม่ได้
//! ซึ่งทั้ง OS, เครื่องมือล้างดิสก์ของผู้ใช้ และ eviction ของเราเอง ถือว่าลบได้
//! ตามใจ — งานที่ยังไม่เคยบันทึกคือสิ่งตรงข้ามสนิท
//! · ที่อยู่จริงมาจาก `refx_platform::paths::AppPaths::recovery_dir()` ซึ่ง
//! **ผู้เรียกส่งเข้ามา** เพราะ `refx-io` พึ่ง `refx-platform` ไม่ได้ (ดู
//! [`crate::save::RenameFn`] ว่าทำไม)
//!
//! ## ★★ เพดานที่มี "ห้ามลบสิ่งที่ผู้ใช้ยังไม่เคยเห็น" คร่อมอยู่
//!
//! I-6 บังคับว่าทุกที่เก็บของต้องมีเพดาน · `docs/07 §4` ให้ [`MAX_KEPT`] ไฟล์
//! หรือ [`MAX_AGE`] แล้วแต่อะไรถึงก่อน **แต่ต่อท้ายว่า "ห้ามลบไฟล์ที่ยังไม่เคย
//! ถูกถาม ผู้ใช้ต้องได้เห็นก่อนเสมอ"**
//!
//! เพดานที่ไม่มีข้อยกเว้นนี้จะกลายเป็นตัวทำงานหายเสียเอง: ผู้ใช้เปิด-ปิดโปรแกรม
//! รัว ๆ 11 ครั้ง แล้ว snapshot ของครั้งที่มีงานจริงถูกไล่ออกก่อนที่เขาจะได้เห็น
//! คำถามด้วยซ้ำ → [`prune`] จึงลบได้เฉพาะไฟล์ที่**มีไฟล์ประทับ `.asked`** อยู่
//! (ชั้น UI เขียนตอนแสดง dialog) · ไฟล์ที่ยังไม่เคยถูกถามอยู่ค้างได้ตลอดกาล
//! และนั่นถูกแล้ว
//!
//! spec: docs/07-file-format.md §4, ROADMAP P4-4

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use refx_core::arena::BoardId;
use refx_core::board::Board;

use crate::save::{RenameFn, SaveError, save_atomic};

/// นามสกุลของ snapshot — **เป็นไฟล์ `.refx` ที่ถูกต้องทุกประการ**
///
/// เจตนาเดียวกับ P4-3: ไม่สร้างผิวรูปแบบไฟล์ใหม่แม้แต่ไบต์เดียว
/// `fuzz_document` จึงยิงมันอยู่แล้วโดยปริยาย และ `xtask dump-refx` (P4-8)
/// จะเปิดดูมันได้โดยไม่ต้องรู้จักอะไรเพิ่ม
pub const SNAPSHOT_EXT: &str = "refx";

/// นามสกุลของไฟล์ประทับ "ผู้ใช้เคยเห็นไฟล์นี้แล้ว" (ดูหัวโมดูล)
pub const ASKED_EXT: &str = "asked";

/// เก็บ snapshot ที่ไม่มีเจ้าของได้กี่ไฟล์ (`docs/07 §4`)
pub const MAX_KEPT: usize = 10;

/// เก็บได้นานแค่ไหน (`docs/07 §4` — 30 วัน)
pub const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// ★ รหัสประจำการเปิดโปรแกรมหนึ่งครั้ง — สุ่มตอนเปิด ไม่เคยซ้ำกับที่มีชีวิตอยู่
///
/// ต้องไม่ชนกันระหว่างหน้าต่าง/โปรเซสที่เปิดพร้อมกัน (`docs/07 §4`) ไม่งั้น
/// สองหน้าต่างจะเขียนทับ snapshot ของกันและกัน แล้วงานของหน้าต่างหนึ่งหายเงียบ ๆ
///
/// ★★ ประกอบจาก **เวลา + pid + ตัวนับในโปรเซส** ไม่ใช่ตัวสุ่ม เพราะ:
/// - `pid` ไม่ซ้ำกันในบรรดาโปรเซสที่ **มีชีวิตอยู่ตอนนี้** ซึ่งเป็นเงื่อนไขที่
///   ต้องการพอดี (โปรเซสที่ตายไปแล้วไม่ได้เขียนไฟล์แข่งกับใคร)
/// - เวลาแยก pid ที่ถูกใช้ซ้ำหลังโปรเซสเดิมตาย ออกจากกันได้
/// - **ไม่ต้องเพิ่ม dependency** (`rand` ไม่มีใน `docs/09`) และไม่ต้องขออนุญาต
///
/// ★★★ **ทำไมต้องมีตัวนับด้วย ทั้งที่ pid ก็แยกโปรเซสได้แล้ว**
///
/// รุ่นแรกมีแค่ `เวลา + pid` แล้วเทสต์
/// `two_instances_never_share_a_recovery_file` จับได้ทันทีว่าสองตัวที่สร้าง
/// **ในมิลลิวินาทีเดียวกันในโปรเซสเดียวกัน** ได้รหัสเท่ากันเป๊ะ
/// · ตอนนี้มันไม่เกิดเพราะ single-instance lock บังคับให้มีโปรเซสเดียว —
/// แต่นั่นคือ "ถูกเพราะบังเอิญมีคนอื่นกันไว้ให้" ซึ่งจะพังเงียบ ๆ วันที่
/// P4-7 ทำ multi-board tabs แล้วแต่ละ board ขอ session ของตัวเอง
/// (`docs/08 §3.9` ข้อ 8: สัญญาที่ถูกเฉพาะเมื่อเรียกถูกจังหวะคือกับดัก)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// รหัสใหม่สำหรับการเปิดโปรแกรมครั้งนี้
    #[must_use]
    pub fn new_unique() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        /// ตัวนับต่อโปรเซส — ทำให้สองรหัสในโปรเซสเดียวกันต่างกันเสมอ
        /// ไม่ว่านาฬิกาจะละเอียดแค่ไหน
        static NEXT: AtomicU64 = AtomicU64::new(0);

        // เวลาที่ถอยหลัง (ผู้ใช้ปรับนาฬิกา) ไม่ใช่เรื่องที่ต้องล้ม — แค่ตกมาที่ 0
        // แล้ว pid กับตัวนับยังแยกให้อยู่ดี
        let millis = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        let seq = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(format!("{millis:x}-{:x}-{seq:x}", std::process::id()))
    }

    /// อ่านเป็นข้อความ — **ไม่มีจุดและไม่มีตัวคั่น path** จึงใช้เป็นชื่อไฟล์ได้ตรง ๆ
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    /// ★★ "ค่าปริยาย" = **session ใหม่** ไม่ใช่ค่าคงที่ร่วม
    ///
    /// ค่าคงที่จะทำให้ทุกหน้าต่างเขียนไฟล์เดียวกันแล้วทับงานของกันและกัน —
    /// ซึ่งเป็นสิ่งเดียวที่รหัสนี้มีไว้กัน · ตัวที่ derive `Default` มาถึงตรงนี้
    /// (`RefxApp`) ต้องได้ session ที่ใช้ได้จริงเสมอ ไม่ใช่ค่าที่รอให้ใครมาเติม
    fn default() -> Self {
        Self::new_unique()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// ที่อยู่ของ snapshot ของ session นี้
#[must_use]
pub fn snapshot_path(dir: &Path, session: &SessionId) -> PathBuf {
    dir.join(format!("{}.{SNAPSHOT_EXT}", session.as_str()))
}

/// ไฟล์ประทับที่คู่กับ snapshot นี้ (ดูหัวโมดูล)
#[must_use]
pub fn asked_marker(snapshot: &Path) -> PathBuf {
    snapshot.with_extension(ASKED_EXT)
}

/// เขียน snapshot ของงานที่ยังไม่เคยบันทึก
///
/// ★ ใช้ [`save_atomic`] ตัวเดียวกับการบันทึกจริงและกับ [`crate::autosave`] —
/// **ยังไม่มีเส้นทางเขียนไฟล์ที่สองในโปรเจกต์** (tmp → fsync → rename ครบชุด)
///
/// ★★ สร้างโฟลเดอร์ให้ถ้ามันหายไป — ผู้ใช้ (หรือโปรแกรมล้างดิสก์) ลบมันทิ้ง
/// ระหว่างที่ RefX เปิดค้างทั้งวันได้ · การล้มเงียบตรงนี้แปลว่างานทั้งวัน
/// ไม่มีอะไรคุ้มครองโดยไม่มีใครรู้
///
/// # Errors
/// [`SaveError`] เมื่อเขียนไม่สำเร็จ — ผู้เรียกควร log แล้วไปต่อ
pub fn write_snapshot(
    dir: &Path,
    session: &SessionId,
    board: &Board,
    rename: RenameFn,
) -> Result<(), SaveError> {
    let path = snapshot_path(dir, session);
    if let Err(source) = std::fs::create_dir_all(dir) {
        return Err(SaveError::Io {
            step: "create the recovery folder for",
            path,
            source,
        });
    }
    save_atomic(&path, board, rename)
}

/// ★★ ทิ้ง snapshot ของ session นี้ — **เมื่อบันทึกลง path จริงสำเร็จเท่านั้น**
///
/// `docs/07 §4`: *"ลบเมื่อผู้ใช้ Save ลง path จริงสำเร็จ (ย้ายเจ้าของไปเป็น
/// `<doc>.refx.autosave`)"* — ตั้งแต่จังหวะนั้นไปมีคนดูแลงานนี้แล้ว
///
/// ★ ลบ **ทั้งตระกูล** (`.refx` · `.refx.bak` · `.asked`) ไม่ใช่แค่ตัวหลัก —
/// `save_atomic` ทิ้ง `.bak` ของ snapshot รอบก่อนไว้ ถ้าไม่เก็บกวาดพร้อมกัน
/// โฟลเดอร์จะสะสมไฟล์ที่ไม่มีใครเป็นเจ้าของและไม่มีใครลบ
pub fn discard(dir: &Path, session: &SessionId) {
    remove_family(&snapshot_path(dir, session));
}

/// ลบ snapshot กับไฟล์บริวารทั้งหมดของมัน
fn remove_family(snapshot: &Path) {
    let bak = snapshot.with_extension(crate::save::BAK_SUFFIX);
    let tmp = snapshot.with_extension(crate::save::TMP_SUFFIX);
    for path in [snapshot, &bak, &tmp, &asked_marker(snapshot)] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            // ไม่มีไฟล์อยู่แล้วคือผลลัพธ์ที่ต้องการ ไม่ใช่ความล้มเหลว
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(%err, path = %path.display(), "cannot remove a recovery file");
            }
        }
    }
}

/// ★ ประทับว่า "ผู้ใช้เห็นไฟล์นี้แล้ว" — เรียกตอนแสดง dialog กู้คืน
///
/// ตั้งแต่จังหวะนี้ [`prune`] ถึงจะแตะมันได้ · ก่อนหน้านั้นมันอยู่ค้างตลอดกาล
/// ต่อให้เกินเพดานก็ตาม (ดูหัวโมดูล)
///
/// เขียนไม่สำเร็จ = ไฟล์นั้นถูกถือว่า "ยังไม่เคยถาม" ต่อไป ซึ่งเป็นทางที่
/// **ปลอดภัยกว่า** (อย่างมากคือถูกถามซ้ำรอบหน้า ไม่ใช่งานหาย)
pub fn mark_asked(snapshot: &Path) {
    let marker = asked_marker(snapshot);
    if let Err(err) = std::fs::write(&marker, b"asked") {
        tracing::warn!(%err, path = %marker.display(), "cannot mark a recovery file as seen");
    }
}

/// ผู้ใช้เคยเห็นไฟล์นี้แล้วหรือยัง
#[must_use]
pub fn was_asked(snapshot: &Path) -> bool {
    asked_marker(snapshot).exists()
}

/// snapshot ที่ค้างอยู่จาก session ที่จบไปแล้ว
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    /// ที่อยู่ของไฟล์
    pub path: PathBuf,
    /// เขียนเมื่อไหร่ — เอาไปบอกผู้ใช้ว่า "งานจาก <เวลา>" · `None` = ระบบไฟล์ไม่บอก
    pub written_at: Option<SystemTime>,
    /// ผู้ใช้เคยเห็นไฟล์นี้แล้วหรือยัง
    pub asked: bool,
}

/// ★ ไล่ดูโฟลเดอร์ recovery — **ใหม่สุดก่อน** · ไม่รวม session ปัจจุบัน
///
/// ★★ **แตะดิสก์ ห้ามเรียกบน UI thread** (I-2) — ผู้เรียกต้องพามันไป worker
///
/// ★ เรียงด้วย `(เวลา, ชื่อไฟล์)` ไม่ใช่ลำดับที่ระบบไฟล์คืนมา — ลำดับของ
/// `read_dir` ไม่ถูกกำหนดไว้ในทุก OS และผลของ [`prune`] ขึ้นกับลำดับนี้ตรง ๆ
/// (หลักการเดียวกับที่ `CLAUDE.md` ห้าม HashMap iteration order ในการคำนวณ layout)
///
/// ไฟล์ที่อ่านไม่ออก/ไม่ใช่ `.refx` ถูกข้ามเงียบ ๆ — **ไม่ใช่ error**
/// (I-4: ทุกไฟล์คือ input ที่ไม่น่าไว้ใจ · ใครก็วางไฟล์อะไรไว้ตรงนั้นได้)
#[must_use]
pub fn scan(dir: &Path, current: &SessionId) -> Vec<Orphan> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new(); // ยังไม่เคยมีโฟลเดอร์ = ไม่มีอะไรค้าง
    };
    let mine = snapshot_path(dir, current);

    let mut found: Vec<Orphan> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == SNAPSHOT_EXT) && *path != mine)
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() {
                return None;
            }
            Some(Orphan {
                written_at: meta.modified().ok(),
                asked: was_asked(&path),
                path,
            })
        })
        .collect();

    // ใหม่สุดก่อน · ไฟล์ที่ไม่มีเวลาถูกถือว่าเก่าที่สุด (ปลอดภัยกว่าในแง่การถาม
    // — มันจะไปอยู่ท้ายรายการ ไม่แย่งที่ของไฟล์ที่รู้เวลาแน่ ๆ)
    found.sort_by(|a, b| {
        b.written_at
            .cmp(&a.written_at)
            .then_with(|| a.path.cmp(&b.path))
    });
    found
}

/// อ่าน board กลับจาก snapshot — `None` = ไม่มีอะไรให้กู้
///
/// ★★ **ไฟล์ที่อ่านไม่ออกถือว่าไม่มี ไม่ใช่ error** ด้วยเหตุผลเดียวกับ
/// [`crate::autosave::find_pending`]: การทำให้ผู้ใช้เปิดโปรแกรมไม่ได้เพราะ
/// ไฟล์กู้คืนเสีย คือการเอาเกราะมาทำร้ายเขาเอง
#[must_use]
pub fn load(path: &Path, id: BoardId) -> Option<Board> {
    use std::io::Read as _;

    let mut bytes = Vec::new();
    let mut file = std::fs::File::open(path).ok()?;
    // เพดานเดียวกับตัวอ่านเอกสาร — ไฟล์ที่โตผิดปกติต้องไม่ถูกสูบเข้า RAM ทั้งก้อน
    file.by_ref()
        .take(crate::dto::MAX_COMPRESSED_BYTES + crate::dto::HEADER_LEN as u64)
        .read_to_end(&mut bytes)
        .ok()?;

    match crate::dto::decode(&bytes, id) {
        Ok(board) => Some(board),
        Err(err) => {
            tracing::warn!(%err, path = %path.display(), "a recovery snapshot could not be read");
            None
        }
    }
}

/// ผลของการเก็บกวาด — **ตัวเลขที่บอกว่าเพดานทำงานจริงไหม**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pruned {
    /// ลบไปกี่ไฟล์
    pub removed: usize,
    /// ★ กี่ไฟล์ที่เข้าเกณฑ์ลบแล้ว **แต่รอดเพราะผู้ใช้ยังไม่เคยเห็น**
    ///
    /// ตัวเลขนี้แยก "เพดานไม่ทำงาน" ออกจาก "เพดานทำงานแต่ยอมถอยให้ข้อยกเว้น"
    /// ซึ่งเป็นสองเรื่องคนละอย่างตอนอ่านผล
    pub kept_because_unasked: usize,
}

/// ★★ เก็บกวาดตามเพดาน — **ลบได้เฉพาะไฟล์ที่ผู้ใช้เคยเห็นแล้ว**
///
/// เกินจำนวน [`MAX_KEPT`] **หรือ** เก่ากว่า [`MAX_AGE`] อย่างใดอย่างหนึ่งก็เข้าเกณฑ์
/// (`docs/07 §4`: "แล้วแต่อะไรถึงก่อน") แต่ต้องผ่านด่าน `.asked` ก่อนเสมอ
///
/// ★ `now` ถูก **ส่งเข้ามา** ไม่ใช่อ่านนาฬิกาเอง — เทสต์จึงเดินเวลาไป 40 วัน
/// ได้โดยไม่ต้องรอ และไม่มี assert ที่ผูกกับนาฬิกาของเครื่อง (`docs/08 §3.9` ข้อ 5b)
///
/// ★ นับเฉพาะ snapshot ที่ **ไม่มีเจ้าของแล้ว** — ของ session ที่กำลังทำงานอยู่
/// ไม่ถูกนับและไม่ถูกแตะ (มันยังถูกเขียนทับอยู่ทุกรอบ autosave)
pub fn prune(
    dir: &Path,
    current: &SessionId,
    keep: usize,
    max_age: Duration,
    now: SystemTime,
) -> Pruned {
    let mut result = Pruned::default();
    for (index, orphan) in scan(dir, current).into_iter().enumerate() {
        let too_old = orphan
            .written_at
            .is_some_and(|at| now.duration_since(at).is_ok_and(|age| age > max_age));
        if index < keep && !too_old {
            continue;
        }
        // ★★★ ด่านสุดท้าย: ผู้ใช้ต้องได้เห็นก่อนเสมอ (`docs/07 §4`)
        if !orphan.asked {
            result.kept_because_unasked += 1;
            continue;
        }
        remove_family(&orphan.path);
        result.removed += 1;
    }
    if result.removed > 0 || result.kept_because_unasked > 0 {
        tracing::info!(
            removed = result.removed,
            kept = result.kept_because_unasked,
            "swept the recovery folder"
        );
    }
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use refx_core::arena::{ArenaKey as _, BoardId};
    use refx_core::board::{BoardParts, Item, ItemKind, ItemParts, TextNote};
    use refx_platform::fsops::rename_durable;
    use std::time::Instant;

    fn board_id() -> BoardId {
        BoardId::from_parts(0, 0)
    }

    fn board_named(name: &str, items: usize) -> Board {
        Board::load(
            board_id(),
            BoardParts {
                name: name.to_owned(),
                items: (0..items)
                    .map(|i| ItemParts {
                        item: Item::new(ItemKind::Text(TextNote {
                            text: format!("{name}-{i}"),
                        })),
                        group: None,
                    })
                    .collect(),
                ..BoardParts::default()
            },
        )
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-recovery-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// เขียน snapshot ปลอมของ session ชื่อหนึ่ง แล้วตั้งเวลาแก้ไขย้อนหลังได้
    fn plant(dir: &Path, name: &str, items: usize) -> PathBuf {
        let session = SessionId(name.to_owned());
        write_snapshot(dir, &session, &board_named(name, items), rename_durable).unwrap();
        snapshot_path(dir, &session)
    }

    // ---------- รหัส session ----------

    /// ★★ สองหน้าต่างต้องไม่เขียนทับกัน — รหัสที่ซ้ำกันคือการทำงานหายเงียบ ๆ
    ///
    /// ★ ยิง **ติด ๆ กันในมิลลิวินาทีเดียว** โดยตั้งใจ · การหน่วงเวลาก่อนขอ
    /// ตัวที่สองจะทำให้เทสต์ผ่านด้วยความละเอียดของนาฬิกา แทนที่จะด้วยดีไซน์
    /// — และรุ่นแรกของ `new_unique()` ก็พังตรงนี้จริง ๆ (ดูคอมเมนต์ที่ตัวมัน)
    #[test]
    fn two_sessions_never_share_a_file() {
        let dir = temp_dir("unique");
        let ids: Vec<SessionId> = (0..1_000).map(|_| SessionId::new_unique()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "รหัส session ซ้ำกัน");
        assert_ne!(snapshot_path(&dir, &ids[0]), snapshot_path(&dir, &ids[1]));
    }

    /// รหัสต้องใช้เป็นชื่อไฟล์ได้ตรง ๆ — ไม่มีตัวคั่น path และไม่มีจุด
    ///
    /// ★ จุดสำคัญเป็นพิเศษ: `asked_marker` ใช้ `with_extension` ซึ่งจะกิน
    /// ส่วนหลังจุดสุดท้ายไปเป็นนามสกุล แล้วไฟล์ประทับจะไปตกที่ชื่ออื่น
    #[test]
    fn a_session_id_is_a_safe_file_name() {
        let id = SessionId::new_unique();
        assert!(
            id.as_str()
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == '-'),
            "รหัสมีอักขระที่ใช้เป็นชื่อไฟล์ไม่ได้: {id}"
        );
        let dir = Path::new("/tmp/recovery");
        let snapshot = snapshot_path(dir, &id);
        assert_eq!(snapshot.parent(), Some(dir), "รหัสหลุดออกนอกโฟลเดอร์");
        assert_eq!(
            asked_marker(&snapshot).file_stem(),
            snapshot.file_stem(),
            "ไฟล์ประทับไม่ได้คู่กับ snapshot"
        );
    }

    // ---------- ★★★ เกณฑ์หลัก: งานที่ไม่เคยบันทึกต้องกลับมาได้ ----------

    /// ★★★ **ไม่มี `<doc>` เลยตั้งแต่ต้นจนจบ แล้วยังกู้ได้**
    ///
    /// นี่คือช่องที่ P4-3 เปิดค้างไว้ทั้งหมด: [`crate::autosave`] ต้องมี path
    /// ของเอกสารก่อนถึงจะทำงาน · ที่นี่ไม่มี path ไหนถูกเอ่ยถึงเลยสักครั้ง
    #[test]
    fn work_that_was_never_saved_anywhere_still_comes_back() {
        let dir = temp_dir("never-saved");
        let session = SessionId::new_unique();
        let board = board_named("สามชั่วโมงที่ยังไม่เคยกด Ctrl+S", 42);

        write_snapshot(&dir, &session, &board, rename_durable).unwrap();

        // เปิดโปรแกรมใหม่ = session ใหม่ ซึ่งไม่รู้อะไรเกี่ยวกับ session เก่าเลย
        let next = SessionId::new_unique();
        let orphans = scan(&dir, &next);
        assert_eq!(orphans.len(), 1, "เปิดใหม่แล้วไม่เห็นงานที่ค้างอยู่");
        let back = load(&orphans[0].path, board_id()).expect("อ่าน snapshot ไม่ได้");
        assert_eq!(back, board, "กู้กลับมาแล้วไม่เท่าเดิม");
        assert!(orphans[0].written_at.is_some());
        assert!(!orphans[0].asked, "ไฟล์ใหม่ต้องยังไม่ถูกถือว่าเคยถาม");
    }

    /// snapshot ต้องเป็นไฟล์ `.refx` ที่ถูกต้อง — ผิวรูปแบบไฟล์ใหม่เป็นศูนย์
    #[test]
    fn a_snapshot_is_a_valid_refx_file() {
        use std::io::Read as _;
        let dir = temp_dir("valid");
        let session = SessionId::new_unique();
        write_snapshot(&dir, &session, &board_named("x", 3), rename_durable).unwrap();

        let mut bytes = Vec::new();
        std::fs::File::open(snapshot_path(&dir, &session))
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let info = crate::dto::inspect(&bytes).unwrap();
        assert_eq!(info.version, crate::dto::FORMAT_VERSION);
        assert!(info.writable);
    }

    /// ★ session ปัจจุบันต้องไม่เห็นไฟล์ของตัวเองเป็น "งานค้างจากรอบก่อน"
    ///
    /// ไม่งั้นผู้ใช้จะถูกถามว่าจะกู้งานที่เขากำลังทำอยู่ตรงหน้าหรือไม่
    #[test]
    fn a_session_never_sees_its_own_file_as_something_to_recover() {
        let dir = temp_dir("self");
        let session = SessionId::new_unique();
        write_snapshot(&dir, &session, &board_named("mine", 2), rename_durable).unwrap();
        assert!(scan(&dir, &session).is_empty());
    }

    /// บันทึกลง path จริงสำเร็จ = ทิ้ง snapshot **พร้อมบริวารทั้งหมด**
    #[test]
    fn saving_for_real_removes_the_whole_family() {
        let dir = temp_dir("handover");
        let session = SessionId::new_unique();
        let board = board_named("x", 2);
        // เขียนสองรอบ — รอบที่สองทำให้เกิด `.bak` ของรอบแรก
        write_snapshot(&dir, &session, &board, rename_durable).unwrap();
        write_snapshot(&dir, &session, &board, rename_durable).unwrap();
        let snapshot = snapshot_path(&dir, &session);
        mark_asked(&snapshot);
        assert!(snapshot.with_extension(crate::save::BAK_SUFFIX).exists());

        discard(&dir, &session);

        assert!(!snapshot.exists());
        assert!(!snapshot.with_extension(crate::save::BAK_SUFFIX).exists());
        assert!(!asked_marker(&snapshot).exists());
        assert!(scan(&dir, &SessionId::new_unique()).is_empty());
        discard(&dir, &session); // ทิ้งซ้ำต้องเงียบ ไม่ใช่ล้ม
    }

    /// ไฟล์เสียต้องอ่านเป็น "ไม่มีอะไรให้กู้" ไม่ใช่ทำให้เปิดโปรแกรมไม่ได้
    #[test]
    fn a_damaged_snapshot_reads_as_nothing_to_recover() {
        use std::io::Read as _;
        let dir = temp_dir("damaged");
        let path = plant(&dir, "broken", 3);

        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();

        assert!(load(&path, board_id()).is_none());
        // ★ แต่ยังต้อง **โผล่ในรายการ** — ผู้เรียกเป็นคนตัดสินว่าจะทำอะไรกับมัน
        //   การหายไปเงียบ ๆ แปลว่าไฟล์เสียจะค้างอยู่ตลอดกาลโดยไม่มีใครเก็บกวาด
        assert_eq!(scan(&dir, &SessionId::new_unique()).len(), 1);
    }

    /// ขยะที่ไม่ใช่ `.refx` ในโฟลเดอร์ต้องถูกข้าม ไม่ใช่ทำให้ทั้งการสแกนล้ม (I-4)
    #[test]
    fn junk_in_the_folder_is_ignored() {
        let dir = temp_dir("junk");
        plant(&dir, "real", 1);
        // ผู้ใช้วางไฟล์อะไรไว้ตรงนั้นก็ได้ — โฟลเดอร์นี้ไม่ใช่ของเราคนเดียว
        std::fs::write(dir.join("notes.txt"), "ข้อความของผู้ใช้").unwrap();
        std::fs::write(dir.join("no-extension"), b"...").unwrap();
        std::fs::create_dir_all(dir.join("weird.refx")).unwrap(); // โฟลเดอร์ที่ชื่อเหมือนไฟล์

        let found = scan(&dir, &SessionId::new_unique());
        assert_eq!(found.len(), 1, "สแกนได้ของที่ไม่ควรได้: {found:#?}");
        assert!(
            found[0]
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("real")
        );
    }

    /// สแกนโฟลเดอร์ที่ยังไม่มี = ไม่มีอะไรค้าง ไม่ใช่ error
    #[test]
    fn scanning_a_folder_that_does_not_exist_is_quiet() {
        let dir = temp_dir("missing").join("never-created");
        assert!(scan(&dir, &SessionId::new_unique()).is_empty());
    }

    /// ★ เขียนได้แม้โฟลเดอร์ถูกลบทิ้งระหว่างโปรแกรมเปิดค้างอยู่
    #[test]
    fn writing_recreates_a_folder_that_vanished() {
        let dir = temp_dir("vanished");
        let session = SessionId::new_unique();
        std::fs::remove_dir_all(&dir).unwrap();

        write_snapshot(&dir, &session, &board_named("x", 1), rename_durable).unwrap();
        assert!(snapshot_path(&dir, &session).exists());
    }

    // ---------- ★★ เพดาน + ข้อยกเว้นที่คร่อมมันอยู่ ----------

    /// ★★★ **เพดานห้ามแตะไฟล์ที่ผู้ใช้ยังไม่เคยเห็น** (`docs/07 §4`)
    ///
    /// วางไว้ 15 ไฟล์เกินเพดาน 10 · ไม่มีไฟล์ไหนถูกถามเลย → **ต้องไม่หายสักไฟล์**
    /// นี่คือเคสของคนที่เปิด-ปิดโปรแกรมรัว ๆ แล้ว snapshot ที่มีงานจริงถูกไล่ออก
    /// ก่อนที่เขาจะได้เห็นคำถามด้วยซ้ำ
    #[test]
    fn the_cap_never_touches_files_the_user_has_not_seen() {
        let dir = temp_dir("cap-unasked");
        for i in 0..15 {
            plant(&dir, &format!("s{i:02}"), 1);
        }
        let current = SessionId::new_unique();

        let pruned = prune(&dir, &current, MAX_KEPT, MAX_AGE, SystemTime::now());

        assert_eq!(pruned.removed, 0, "ลบไฟล์ที่ผู้ใช้ยังไม่เคยเห็น");
        assert_eq!(pruned.kept_because_unasked, 5);
        assert_eq!(scan(&dir, &current).len(), 15);
    }

    /// ★ negative control ของข้อบน — ไฟล์ที่ **เคยถามแล้ว** ต้องถูกเก็บกวาดจริง
    ///
    /// ถ้าไม่มีข้อนี้ ประตูที่ไม่เคยลบอะไรเลยจะดูเหมือนทำงานถูกต้องสมบูรณ์
    /// (`docs/08 §3.9` ข้อ 1)
    #[test]
    fn the_cap_does_sweep_files_the_user_already_saw() {
        let dir = temp_dir("cap-asked");
        for i in 0..15 {
            let path = plant(&dir, &format!("s{i:02}"), 1);
            mark_asked(&path);
        }
        let current = SessionId::new_unique();

        let pruned = prune(&dir, &current, MAX_KEPT, MAX_AGE, SystemTime::now());

        assert_eq!(pruned.removed, 5, "เพดานไม่ทำงาน");
        assert_eq!(pruned.kept_because_unasked, 0);
        assert_eq!(scan(&dir, &current).len(), MAX_KEPT);
    }

    /// ★ เก่ากว่า 30 วันก็ถูกเก็บกวาด ถึงจะยังไม่เกินจำนวนก็ตาม
    /// ("แล้วแต่อะไรถึงก่อน") — และ **ยังต้องผ่านด่าน `.asked` เหมือนกัน**
    #[test]
    fn age_sweeps_even_when_the_count_is_fine() {
        let dir = temp_dir("cap-age");
        let old_asked = plant(&dir, "old-asked", 1);
        mark_asked(&old_asked);
        let old_unasked = plant(&dir, "old-unasked", 1);
        let current = SessionId::new_unique();

        // ★ เดินนาฬิกาเอง ไม่ได้รอ 30 วันจริง (§3.9 ข้อ 5b)
        let in_40_days = SystemTime::now() + Duration::from_secs(40 * 24 * 60 * 60);
        let pruned = prune(&dir, &current, MAX_KEPT, MAX_AGE, in_40_days);

        assert_eq!(pruned.removed, 1, "ไฟล์เก่าที่เคยถามแล้วต้องถูกลบ");
        assert_eq!(pruned.kept_because_unasked, 1);
        assert!(!old_asked.exists());
        assert!(old_unasked.exists(), "ไฟล์เก่าที่ยังไม่เคยถามหายไป");
    }

    /// เพดานต้องไล่ **ตัวเก่าสุด** ออก ไม่ใช่ตัวไหนก็ได้ที่ระบบไฟล์คืนมาก่อน
    #[test]
    fn the_newest_files_are_the_ones_that_survive() {
        let dir = temp_dir("order");
        let mut planted = Vec::new();
        for i in 0..6 {
            let path = plant(&dir, &format!("s{i}"), 1);
            mark_asked(&path);
            // ★ ต้องให้ mtime ต่างกันจริง ไม่งั้นลำดับมาจากชื่อไฟล์อย่างเดียว
            std::thread::sleep(Duration::from_millis(12));
            planted.push(path);
        }
        let current = SessionId::new_unique();

        let pruned = prune(&dir, &current, 2, MAX_AGE, SystemTime::now());

        assert_eq!(pruned.removed, 4);
        assert!(planted[5].exists() && planted[4].exists(), "ตัวใหม่สุดหายไป");
        for old in &planted[..4] {
            assert!(!old.exists(), "ตัวเก่ายังอยู่: {}", old.display());
        }
    }

    /// เก็บกวาดแล้วต้องไม่ทิ้งไฟล์ประทับ/ไฟล์สำรองไว้เป็นขยะกำพร้า
    #[test]
    fn sweeping_leaves_no_orphaned_companions() {
        let dir = temp_dir("companions");
        let path = plant(&dir, "gone", 1);
        plant(&dir, "gone", 1); // รอบสอง → เกิด `.bak`
        mark_asked(&path);

        let pruned = prune(
            &dir,
            &SessionId::new_unique(),
            0,
            MAX_AGE,
            SystemTime::now(),
        );

        assert_eq!(pruned.removed, 1);
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect();
        assert!(left.is_empty(), "เหลือขยะไว้ในโฟลเดอร์: {left:?}");
    }

    // ---------- ★★★ ฆ่าโปรเซสระหว่างแก้งานที่ยังไม่เคยบันทึก ----------

    /// env ที่บอกให้ process ลูกทำตัวเป็น "คนกำลังจัด mood board โดยไม่เคยกด Ctrl+S"
    const EDITOR_ENV: &str = "REFX_RECOVERY_KILL_TARGET";
    /// ชื่อเต็มของ helper — `--exact` ไม่จับชื่อสั้น (บทเรียนจาก P4-2)
    const EDITOR_TEST_PATH: &str = "recovery::tests::unsaved_victim_helper";
    /// เพิ่ม item หนึ่งใบทุกกี่มิลลิวินาที
    const EDIT_PERIOD: Duration = Duration::from_millis(20);
    /// ระยะเว้น autosave ที่ใช้ในเทสต์ — สั้นกว่าของจริงเพื่อให้เทสต์จบเร็ว
    const TEST_INTERVAL: Duration = Duration::from_secs(1);

    fn ready_marker(dir: &Path) -> PathBuf {
        dir.join("ready")
    }

    /// ★ ชื่อใน [`EDITOR_TEST_PATH`] ต้องตรงกับของจริง ไม่งั้นลูกจะได้
    /// "running 0 tests" แล้วเทสต์วัดจะรอจนหมดเวลาโดยไม่รู้สาเหตุ (เจอมาแล้วใน P4-2)
    #[test]
    fn the_editor_test_path_matches_the_real_one() {
        assert!(EDITOR_TEST_PATH.ends_with("unsaved_victim_helper"));
        assert!(EDITOR_TEST_PATH.starts_with("recovery::tests::"));
    }

    /// ★★★ **เหยื่อ** — จัด board ไปเรื่อย ๆ โดย**ไม่เคยบันทึกลง path จริงเลย**
    ///
    /// เดินเส้นทางเดียวกับของจริงทุกขั้น: [`crate::autosave::Autosaver`] ตัวเดิม
    /// · [`write_snapshot`] ตัวเดิม · `rename_durable` ตัวเดิม
    /// (`docs/08 §3.9` ข้อ 9 — เทสต์ที่เรียกตัวจำลองพิสูจน์ได้แค่ว่าตัวจำลองทำงาน)
    #[test]
    fn unsaved_victim_helper() {
        let Ok(target) = std::env::var(EDITOR_ENV) else {
            println!("ข้าม: ตัวช่วยของเทสต์กู้งานที่ไม่เคยบันทึก (ไม่ได้ตั้ง {EDITOR_ENV})");
            return;
        };
        let dir = PathBuf::from(target);
        // ★ session ใหม่ทุกครั้งที่เปิดโปรแกรม — เหมือนของจริงเป๊ะ
        let session = SessionId::new_unique();
        let mut saver = crate::autosave::Autosaver::new(TEST_INTERVAL);
        let mut progress = crate::killclock::Progress::new(&crate::killclock::progress_path(&dir))
            .expect("เปิดไฟล์ประวัติไม่ได้");
        let mut items = 0usize;

        std::fs::write(ready_marker(&dir), session.as_str()).expect("เขียนไฟล์สัญญาณไม่ได้");
        loop {
            items += 1;
            let board = board_named("ยังไม่เคยบันทึก", items);
            // ★★ จดความคืบหน้าของ "ผู้ใช้" ลงไฟล์ — พ่อจะได้ **วัด** ว่าเสียไปเท่าไร
            //    ไม่ใช่หารจากนาฬิกา ซึ่งเป็นสิ่งที่ทำให้เทสต์คู่แฝดแดงบน CI
            //    (เหตุผลเต็มใน `killclock`)
            progress.record(items);
            let now = Instant::now();
            if saver.should_write(true, now) {
                let _ = write_snapshot(&dir, &session, &board, rename_durable);
                saver.record_write(now);
            }
            std::thread::sleep(EDIT_PERIOD);
        }
    }

    /// ★★★ **เกณฑ์ผ่านของ `docs/07 §4`**: เปิดโปรแกรมใหม่ · ลากภาพ ·
    /// **ไม่กด `Ctrl+S` เลย** · ฆ่าโปรเซส → เปิดใหม่ต้องได้งานคืน
    ///
    /// ★★ วัดจริงเหมือน P4-3 ไม่ใช่แค่ดูว่ามีไฟล์: เทียบจำนวน item ใน snapshot
    /// กับ **ประวัติที่เหยื่อจดไว้เองว่าใบที่ N เกิดตอนไหน** ([`crate::killclock`])
    /// → ตอบได้ว่า "กู้ได้กี่ %" และ "เสียไปกี่วินาที" ที่เป็นของจริงบนทุกเครื่อง
    ///
    /// ★ ไม่มี path ของเอกสารเข้ามาเกี่ยวข้องเลยสักจังหวะ — ถ้ามี แปลว่า
    /// เทสต์นี้กำลังวัดเส้นทางของ P4-3 ซ้ำแทนที่จะวัดช่องที่มันเปิดค้างไว้
    #[test]
    fn killing_an_unsaved_session_still_leaves_the_work_recoverable() {
        const ROUNDS: usize = 8;

        let dir = temp_dir("kill-unsaved");
        let marker = ready_marker(&dir);
        let exe = std::env::current_exe().expect("หา test binary ของตัวเองไม่เจอ");

        let mut worst_lost = Duration::ZERO;
        let mut worst_kept = 100.0_f64;
        let mut measured = 0usize;
        let mut nothing_yet = 0usize;

        for round in 0..ROUNDS {
            // โฟลเดอร์สะอาดทุกรอบ — แต่ละรอบคือ "เปิดโปรแกรมใหม่" ครั้งหนึ่ง
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();

            let mut child = std::process::Command::new(&exe)
                .args([EDITOR_TEST_PATH, "--exact", "--nocapture"])
                .env(EDITOR_ENV, &dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("spawn เหยื่อไม่สำเร็จ");

            let deadline = Instant::now() + Duration::from_secs(30);
            while !marker.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(marker.exists(), "รอบ {round}: เหยื่อไม่เคยพร้อม");

            let started = Instant::now();
            let alive = Duration::from_millis(900 + (round as u64 * 411) % 2300);
            std::thread::sleep(alive);
            // ★ `kill()` = `TerminateProcess` บน Windows — เทียบเท่า End Task
            //   ซึ่งเป็นเกณฑ์ที่ ROADMAP P4-4 เขียนไว้ตรง ๆ
            let _ = child.kill();
            let _ = child.wait();
            let lived = started.elapsed();

            // ---- เปิดโปรแกรมใหม่: session ใหม่ที่ไม่รู้อะไรเกี่ยวกับรอบก่อนเลย ----
            let next = SessionId::new_unique();
            let orphans = scan(&dir, &next);
            let Some(orphan) = orphans.first() else {
                nothing_yet += 1;
                continue;
            };
            assert_eq!(orphans.len(), 1, "รอบ {round}: เจอ snapshot มากกว่าหนึ่ง");
            let board = load(&orphan.path, board_id())
                .unwrap_or_else(|| panic!("รอบ {round}: snapshot อ่านไม่ออก — งานหาย"));

            let saved_items = board.len();
            // ★★★ ถามประวัติที่เหยื่อจดไว้เอง **ห้ามหารจาก `lived`** — ดู `killclock`
            //     (การหารทำให้เทสต์คู่แฝดของ P4-3 แดงบน runner 2 core)
            let timeline = crate::killclock::read(&crate::killclock::progress_path(&dir));
            let (Some(done_items), Some(lost)) =
                (timeline.done(), timeline.lost_after(saved_items))
            else {
                nothing_yet += 1;
                continue;
            };
            #[expect(clippy::cast_precision_loss, reason = "แค่พิมพ์ให้คนอ่าน")]
            let kept = (saved_items as f64 / done_items.max(1) as f64 * 100.0).min(100.0);
            worst_lost = worst_lost.max(lost);
            worst_kept = worst_kept.min(kept);
            measured += 1;
            println!(
                "รอบ {round}: มีชีวิต {lived:?} · ทำไป {done_items} ใบ · \
                 กู้ได้ {saved_items} ใบ ({kept:.0}%) · เสีย {lost:?}"
            );
        }

        println!(
            "\n★ วัด {measured} รอบ (อีก {nothing_yet} รอบฆ่าก่อนเขียนรอบแรก) — \
             กู้ได้น้อยสุด {worst_kept:.0}% · เสียมากสุด {worst_lost:?} · \
             ระยะเว้นที่ตั้งไว้ {TEST_INTERVAL:?}"
        );
        assert!(
            measured > 0,
            "ไม่มีรอบไหนกู้ได้เลย — ฆ่าเร็วเกินไปทุกครั้งจนไม่เคยมี snapshot"
        );
        let allowed = TEST_INTERVAL + TEST_INTERVAL / 2;
        assert!(
            worst_lost <= allowed,
            "เสียงานมากสุด {worst_lost:?} เกินระยะเว้น {TEST_INTERVAL:?} (+ค่าเผื่อ)"
        );
    }
}
