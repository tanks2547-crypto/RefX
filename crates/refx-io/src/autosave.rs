//! Autosave — **snapshot ทั้ง board** ไม่ใช่ command journal (P4-3)
//!
//! ```text
//! <doc>.refx.autosave     ← snapshot ทั้ง board · DTO เดียวกับ .refx เป๊ะ
//! ```
//!
//! ★★★ **ทำไม snapshot ไม่ใช่ journal** (`docs/07 §4` กล่องบนสุด)
//!
//! `Command` เป็น trait ที่ serialize ไม่ได้ การทำ journal จึงต้องสร้าง
//! `CommandDto` 13 variant ขึ้นมาก่อน = **ผิวรูปแบบไฟล์ใหม่ทั้งชุด** ที่ต้อง
//! version/fuzz/round-trip เอง + ภาระถาวรต่อ command ใหม่ทุกตัว
//! · และถ้าพลาดหนึ่งจุด ผู้ใช้ได้ **board ที่ผิดแบบเงียบ ๆ** ซึ่งแย่กว่า
//! "เสียงาน 10 วินาที" มาก (บน mood board = ลากภาพซ้ำสองใบ)
//!
//! snapshot ใช้ `dto::encode` ตัวเดิม → **ผิวรูปแบบไฟล์ใหม่เป็นศูนย์**
//! ไฟล์ `.refx.autosave` เป็นไฟล์ `.refx` ที่ถูกต้องทุกประการ อ่านด้วย
//! `dto::decode` ตัวเดียวกัน และ `fuzz_document` ก็ยิงมันอยู่แล้วโดยปริยาย
//!
//! ★★ **ราคาที่ต้องคุม** — snapshot หนักกว่า journal ต่อการเขียนหนึ่งครั้ง
//! จึงคุมด้วยสองด่านที่ต้องผ่าน **ทั้งคู่**:
//!
//! 1. `dirty` เท่านั้น — ผู้ใช้ที่เปิดดู reference เฉย ๆ **ไม่มีการเขียนดิสก์เลย**
//!    (สอดคล้อง I-1 ที่บอกว่า idle ต้องไม่ทำงาน)
//! 2. เว้นระยะอย่างน้อย [`Policy::min_interval`] — ลากภาพ 200 เฟรมติดกัน
//!    ต้องไม่กลายเป็นการเขียนดิสก์ 200 ครั้ง
//!
//! ★ **การผูกกับเอกสาร** — ดีไซน์เดิมใช้ `doc_hash` ในหัวไฟล์ journal
//! ที่นี่ใช้ **การอนุมานชื่อจาก path ของเอกสาร** แทน (`autosave_path`)
//! ซึ่งให้ผลเดียวกันคือ "จับคู่ผิดไฟล์ไม่ได้" **โดยไม่ต้องเพิ่มฟิลด์ในไฟล์**
//! — การเพิ่มฟิลด์จะสร้างผิวรูปแบบไฟล์ใหม่ ซึ่งเป็นสิ่งที่ดีไซน์นี้มีไว้เลี่ยงพอดี
//!
//! **สิ่งที่ยอมเสีย:** undo history ข้าม session — หลังกู้คืนแล้ว `Ctrl+Z`
//! ย้อนไปก่อน crash ไม่ได้ (`docs/07 §4`)
//!
//! spec: docs/07-file-format.md §4, ROADMAP P4-3

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use refx_core::board::Board;

use crate::save::{RenameFn, SaveError, write_snapshot_atomic};

/// นามสกุลของไฟล์ autosave
pub const AUTOSAVE_SUFFIX: &str = "refx.autosave";

/// ★★★ นามสกุลของ snapshot ที่ผู้ใช้สั่ง **"เก็บไว้ก่อน ตัดสินใจทีหลัง"**
///
/// `docs/07 §4` (ตัดสิน 22 ส.ค. 2026) — ดู [`keep`] ว่าทำไมต้องเปลี่ยนชื่อไฟล์
pub const KEPT_SUFFIX: &str = "refx.autosave.kept";

/// ระยะเว้นขั้นต่ำระหว่างการเขียนสอง**ครั้ง** (`docs/07 §4` — "อย่างน้อย N วินาที")
///
/// ★ 10 วินาทีคือเพดานของ "เสียงานได้แค่ไหน" ที่ผู้ใช้ยอมรับได้บน mood board
/// — ลากภาพซ้ำสองใบ · ตั้งสั้นกว่านี้ได้แต่จะเขียนดิสก์ถี่ขึ้นตามตรง ๆ
pub const DEFAULT_MIN_INTERVAL: Duration = Duration::from_secs(10);

/// เส้นทางของไฟล์ autosave ที่คู่กับเอกสารนี้
///
/// ★ อนุมานจาก path ของเอกสารเสมอ → **จับคู่ผิดไฟล์ไม่ได้โดยโครงสร้าง**
/// (แทนที่จะต้องมี `doc_hash` ในไฟล์ — ดูหัวโมดูล)
#[must_use]
pub fn autosave_path(doc: &Path) -> PathBuf {
    doc.with_extension(AUTOSAVE_SUFFIX)
}

/// เส้นทางของ snapshot ที่ถูก "เก็บไว้ก่อน" ของเอกสารนี้ — [`KEPT_SUFFIX`]
///
/// ★ อนุมานจาก path ของเอกสารเหมือน [`autosave_path`] → จับคู่ผิดไฟล์ไม่ได้
/// · **มีได้ไฟล์เดียวต่อเอกสาร** กด "เก็บไว้ก่อน" ซ้ำ = ตัวใหม่ทับตัวเก่า
/// (ตัวใหม่ใหม่กว่าเสมอ จึงไม่มีอะไรหาย)
#[must_use]
pub fn kept_path(doc: &Path) -> PathBuf {
    doc.with_extension(KEPT_SUFFIX)
}

/// ★★★ ตัวตัดสินว่า "ตอนนี้ควรเขียน autosave ไหม" — **ฟังก์ชันบริสุทธิ์**
///
/// แยกออกมาจากการเขียนไฟล์เพื่อให้ **เทสต์ได้โดยไม่ต้องแตะดิสก์และไม่ต้อง
/// รอเวลาจริง** (`docs/08 §3.9` ข้อ 5b: ห้าม assert เวลานาฬิกา — ที่นี่เวลา
/// ถูกส่งเข้ามาเป็นพารามิเตอร์ เทสต์จึงเดินนาฬิกาเองได้)
#[derive(Debug, Clone)]
pub struct Autosaver {
    min_interval: Duration,
    last_write: Option<Instant>,
    writes: u64,
    skipped_clean: u64,
    skipped_too_soon: u64,
}

impl Default for Autosaver {
    fn default() -> Self {
        Self::new(DEFAULT_MIN_INTERVAL)
    }
}

impl Autosaver {
    /// ตัวใหม่พร้อมระยะเว้นที่กำหนด
    #[must_use]
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_write: None,
            writes: 0,
            skipped_clean: 0,
            skipped_too_soon: 0,
        }
    }

    /// ★★ ควรเขียนตอนนี้ไหม — **ต้องผ่านทั้งสองด่าน**
    ///
    /// ★ ลำดับสำคัญ: ตรวจ `dirty` **ก่อน** เสมอ · ถ้าตรวจเวลาก่อน ตัวนับ
    /// `skipped_clean` จะไม่มีความหมาย และเราจะแยกไม่ออกว่า "ไม่เขียนเพราะ
    /// ไม่มีอะไรแก้" กับ "ไม่เขียนเพราะเพิ่งเขียนไป" ซึ่งเป็นสองเรื่องคนละอย่าง
    /// ตอนอ่านตัวเลขที่วัดได้
    pub fn should_write(&mut self, dirty: bool, now: Instant) -> bool {
        if !dirty {
            self.skipped_clean += 1;
            return false;
        }
        if let Some(last) = self.last_write
            && now.duration_since(last) < self.min_interval
        {
            self.skipped_too_soon += 1;
            return false;
        }
        true
    }

    /// บันทึกว่าเพิ่งเขียนไปเมื่อ `now`
    pub fn record_write(&mut self, now: Instant) {
        self.last_write = Some(now);
        self.writes += 1;
    }

    /// ★ ลืมเวลาครั้งล่าสุด — เรียกหลัง **บันทึกจริง** สำเร็จ
    ///
    /// ทำให้การแก้ครั้งถัดไปหลังกด `Ctrl+S` ได้ autosave ทันทีโดยไม่ต้องรอ
    /// ครบรอบ · ช่วงหลัง save คือช่วงที่ผู้ใช้มักแก้ต่อทันที
    pub fn reset(&mut self) {
        self.last_write = None;
    }

    /// ★★★ เวลาที่ควรถูกปลุกมาเขียนรอบถัดไป — `None` = ไม่มีอะไรค้าง
    ///
    /// **ขาดข้อนี้ autosave จะไม่เกิดเลยตอนที่มันจำเป็นที่สุด**: ผู้ใช้ลากภาพ
    /// เสร็จแล้วลุกไปชงกาแฟ · board `dirty` แต่ไม่มี input เข้ามาอีก →
    /// `ControlFlow::Wait` ทำให้เธรดหลับสนิท (I-1 ทำงานถูกต้อง) → ไม่มีเฟรม
    /// ไหนถูกวาด → ไม่มีใครถาม `should_write` → **snapshot ไม่ถูกเขียนจนกว่า
    /// เขาจะกลับมาขยับเมาส์** ซึ่งคือช่วงเวลาที่ crash แล้วเสียงานพอดี
    ///
    /// ★ คืนเป็น **deadline** ให้ชั้นบนเอาไปรวมกับนาฬิกาตัวอื่นแล้วตั้ง
    /// `ControlFlow::WaitUntil` — เธรดยังหลับสนิทจนถึงเวลานั้น **ไม่ใช่ `Poll`**
    /// จึงไม่ขัด I-1 (หลักการเดียวกับที่ egui ใช้ขอเวลาสำหรับเคอร์เซอร์กะพริบ)
    #[must_use]
    pub fn next_deadline(&self, dirty: bool) -> Option<Instant> {
        if !dirty {
            return None;
        }
        Some(
            self.last_write
                .map_or_else(Instant::now, |last| last + self.min_interval),
        )
    }

    /// จำนวนครั้งที่เขียนจริง — **ตัวเลขที่ใช้วัดว่าราคาหนักแค่ไหน**
    #[must_use]
    pub fn writes(&self) -> u64 {
        self.writes
    }

    /// กี่ครั้งที่ไม่เขียนเพราะ **ไม่มีอะไรแก้**
    #[must_use]
    pub fn skipped_clean(&self) -> u64 {
        self.skipped_clean
    }

    /// กี่ครั้งที่ไม่เขียนเพราะ **เพิ่งเขียนไป**
    #[must_use]
    pub fn skipped_too_soon(&self) -> u64 {
        self.skipped_too_soon
    }
}

/// เขียน snapshot ลงไฟล์ autosave ของเอกสารนี้
///
/// ★ ใช้ [`save_atomic`] ตัวเดียวกับการบันทึกจริง จึงได้ tmp → fsync → rename
/// และ `rename_durable` มาฟรีทั้งชุด — **ไม่มีเส้นทางเขียนไฟล์ที่สองในโปรเจกต์**
///
/// # Errors
/// [`SaveError`] เมื่อเขียนไม่สำเร็จ — ผู้เรียกควร log แล้วไปต่อ ไม่ใช่หยุดโปรแกรม
/// (autosave ที่ล้มไม่ได้แปลว่างานที่ผู้ใช้เห็นอยู่หายไปไหน)
pub fn write_snapshot(doc: &Path, board: &Board, rename: RenameFn) -> Result<(), SaveError> {
    write_snapshot_atomic(&autosave_path(doc), board, rename)
}

/// ทิ้ง autosave ของเอกสารนี้ — **เรียกเมื่อ save สำเร็จเท่านั้น** (`docs/07 §4`)
///
/// ★★ "เท่านั้น" คือคำที่สำคัญ: ถ้าลบตอนอื่น (เช่นตอนปิดโปรแกรมปกติ)
/// ผู้ใช้ที่กด "ปิดโดยไม่บันทึก" เพราะเข้าใจผิด จะไม่เหลืออะไรให้กู้เลย
///
/// ลบไม่สำเร็จ **ไม่ใช่ error ที่ต้องบอกผู้ใช้** — ผลที่แย่ที่สุดคือรอบหน้า
/// เขาถูกถามว่าจะกู้คืนไหมทั้งที่ไม่จำเป็น ซึ่งน่ารำคาญแต่ไม่ทำงานหาย
/// ★★ **ลบไฟล์บริวารด้วย** — [`write_snapshot`] ใช้ `save_atomic` ซึ่งทิ้ง
/// `.bak` ของ snapshot รอบก่อนไว้ข้าง ๆ เสมอ · ถ้าลบแต่ตัวหลัก จะเหลือสำเนา
/// ที่ **ไม่มีใครอ่านและไม่มีใครเก็บกวาด** นอนอยู่ในโฟลเดอร์งานของผู้ใช้ตลอดไป
/// — และตอนที่ผู้ใช้กด *"ทิ้งไป"* มันคือของที่เขาเพิ่งสั่งให้ทิ้งพอดี
/// (หลักการเดียวกับ `remove_recovery_file` ของ P4-4 ที่ลบทั้งชุดอยู่แล้ว)
pub fn discard(doc: &Path) {
    for path in [
        autosave_path(doc),
        crate::save::backup_path(&autosave_path(doc)),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => tracing::debug!(file = %path.display(), "removed the autosave snapshot"),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => tracing::warn!(%err, "cannot remove the autosave snapshot"),
        }
    }
}

/// snapshot ที่ค้างอยู่ของเอกสารนี้ พร้อมเวลาที่มันถูกเขียน
#[derive(Debug, Clone)]
pub struct Pending {
    /// board ที่กู้คืนได้
    pub board: Board,
    /// เขียนเมื่อไหร่ — เอาไปบอกผู้ใช้ว่า "งานจาก <เวลา>"
    pub written_at: Option<std::time::SystemTime>,
}

/// หา snapshot ที่ค้างอยู่ของเอกสารนี้ — `None` = ไม่มีอะไรให้กู้
///
/// ★★ **ไฟล์ที่อ่านไม่ออกถือว่าไม่มี ไม่ใช่ error** — autosave ที่เขียนค้าง
/// ตอนไฟดับเป็นเรื่องปกติ (`save_atomic` กันไว้แล้วแต่ไม่ 100% — ดู P4-2)
/// การทำให้ผู้ใช้เปิดโปรแกรมไม่ได้เพราะไฟล์กู้คืนเสีย คือการเอาเกราะมาทำร้ายเขาเอง
///
/// ★ ผู้เรียกต้องเทียบกับเอกสารจริงเองว่าต่างกันไหมก่อนถาม — ไฟล์ที่เหมือนกัน
/// เป๊ะไม่มีอะไรให้กู้ (เกิดได้ถ้าโปรแกรมตายหลัง save แต่ก่อนลบ autosave)
#[must_use]
pub fn find_pending(doc: &Path, id: refx_core::arena::BoardId) -> Option<Pending> {
    read_snapshot(&autosave_path(doc), id)
}

/// ★★★ snapshot ที่ผู้ใช้สั่ง **"เก็บไว้ก่อน"** ของเอกสารนี้ — `None` = ไม่มี
///
/// ถูกเสนอกลับ **ทุกครั้งที่เปิดเอกสารนี้** จนกว่าเขาจะกู้หรือทิ้ง (`docs/07 §4`)
#[must_use]
pub fn find_kept(doc: &Path, id: refx_core::arena::BoardId) -> Option<Pending> {
    read_snapshot(&kept_path(doc), id)
}

/// ★★★ **ย้าย snapshot ออกจากทางของ autosave** — ทางที่ทำให้ปุ่ม "เก็บไว้ก่อน" จริง
///
/// ## ปัญหาที่มันแก้ (`docs/07 §4` — ตัดสิน 22 ส.ค. 2026)
///
/// snapshot ของเอกสารอยู่ที่ `<doc>.refx.autosave` ซึ่งเป็น **ไฟล์เดียวกับที่
/// autosave จะเขียนทับในไม่กี่วินาที** · ผู้ใช้ที่กด *"เก็บไว้ก่อน ตัดสินใจทีหลัง"*
/// แล้วแก้งานต่ออีกนิดเดียว จะเสียของที่เพิ่งสั่งให้เก็บไว้โดยไม่มีอะไรเตือน —
/// ปุ่มนั้นกลายเป็น *"ทิ้งใน 10 วินาที"*
///
/// ★★ ทางที่ตกไป: **หยุด autosave จนกว่าจะตอบ** (เอางานใหม่ไปเสี่ยงแทนงานเก่า
/// = แลกของหายกับของหาย) และ **เตือนบน tooltip แล้วปล่อยให้ทับ** (คนที่ไม่อ่าน
/// ยังเสียของอยู่ดี · คำเตือนไม่ใช่การป้องกัน)
///
/// → **การเก็บทั้งสองอย่างดีกว่าการเลือกว่าจะเสียอันไหน** ราคาคือชื่อไฟล์เพิ่มหนึ่งชื่อ
///
/// ★ ใช้ `rename` ตัวเดียวกับ `save_atomic` จึงเป็นการสลับแบบ atomic —
/// ไม่มีจังหวะไหนที่ snapshot หายไปจากดิสก์ทั้งสองชื่อพร้อมกัน
///
/// คืน `false` เมื่อไม่มี snapshot ให้ย้าย (ไม่ใช่ error — ผู้ใช้อาจตอบช้ากว่า
/// ที่ไฟล์ถูกลบไปด้วยเหตุอื่น)
///
/// # Errors
/// [`SaveError`] เมื่อย้ายไม่สำเร็จ — **ของเดิมยังอยู่ที่เดิมครบ**
pub fn keep(doc: &Path, rename: RenameFn) -> Result<bool, SaveError> {
    let from = autosave_path(doc);
    if !from.exists() {
        return Ok(false);
    }
    let to = kept_path(doc);
    rename(&from, &to).map_err(|err| SaveError::io("keep", &to, err))?;
    tracing::info!(file = %to.display(), "kept the snapshot the user has not answered yet");
    Ok(true)
}

/// ทิ้ง snapshot ที่ถูกเก็บไว้ — **เรียกเมื่อผู้ใช้สั่งเท่านั้น**
///
/// ★★★ ต่างจาก [`discard`] ตรงที่ **ไม่มีอะไรลบไฟล์นี้อัตโนมัติเลย** ไม่ว่าจะ
/// บันทึกสำเร็จหรือปิดโปรแกรม — มันคือของที่ผู้ใช้ **สั่งให้เก็บไว้เอง**
/// การลบมันโดยเขาไม่ได้ตอบคือการทำในสิ่งที่ตัวเลือกที่สามมีไว้ห้ามพอดี
pub fn discard_kept(doc: &Path) {
    let path = kept_path(doc);
    match std::fs::remove_file(&path) {
        Ok(()) => tracing::debug!(file = %path.display(), "removed the kept snapshot"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(%err, "cannot remove the kept snapshot"),
    }
}

/// อ่าน snapshot หนึ่งไฟล์ — ตัวร่วมของ [`find_pending`] และ [`find_kept`]
fn read_snapshot(path: &Path, id: refx_core::arena::BoardId) -> Option<Pending> {
    use std::io::Read as _;

    let mut bytes = Vec::new();
    let mut file = std::fs::File::open(path).ok()?;
    // ★ เพดานเดียวกับตัวอ่านเอกสาร — ไฟล์ที่โตผิดปกติต้องไม่ถูกสูบเข้า RAM ทั้งก้อน
    file.by_ref()
        .take(crate::dto::MAX_COMPRESSED_BYTES + crate::dto::HEADER_LEN as u64)
        .read_to_end(&mut bytes)
        .ok()?;

    match crate::dto::decode(&bytes, id) {
        Ok(board) => Some(Pending {
            board,
            written_at: file.metadata().ok().and_then(|meta| meta.modified().ok()),
        }),
        Err(err) => {
            // อ่านไม่ออก = ไม่มีอะไรให้กู้ · บอกใน log ไว้ให้ตามได้
            tracing::warn!(%err, "the autosave snapshot could not be read");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use refx_core::arena::{ArenaKey as _, BoardId};
    use refx_core::board::{BoardParts, Item, ItemKind, ItemParts, TextNote};
    use refx_platform::fsops::rename_durable;

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

    /// board ที่เป็น **ภาพจริง** — hash/path/ขนาด ครบเหมือนของผู้ใช้
    fn board_of_images(n: usize) -> Board {
        use refx_core::board::{AssetRef, ImageFormat};
        use refx_core::hash::ContentHash;
        Board::load(
            board_id(),
            BoardParts {
                name: "full".to_owned(),
                items: (0..n)
                    .map(|i| {
                        // hash ที่ต่างกันจริงทุกใบ — บีบไม่ลงเหมือนของจริง
                        let mut raw = [0u8; 32];
                        for (slot, byte) in raw.iter_mut().enumerate() {
                            *byte = (i.wrapping_mul(31).wrapping_add(slot * 7)) as u8;
                        }
                        ItemParts {
                            item: Item::new(ItemKind::Image(AssetRef {
                                hash: ContentHash::from_bytes(raw),
                                path: PathBuf::from(format!(
                                    "C:/references/set-{}/plate_{i:05}.jpg",
                                    i / 100
                                )),
                                px_size: glam::UVec2::new(4000, 3000),
                                format: ImageFormat::Unknown,
                                embedded: false,
                                mtime: 1_760_000_000_000 + i as i64,
                                file_size: 2_400_000 + i as u64,
                            })),
                            group: None,
                        }
                    })
                    .collect(),
                ..BoardParts::default()
            },
        )
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-autosave-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ---------- นโยบาย ----------

    /// ★★★ **ไม่แก้อะไร = ไม่เขียนดิสก์เลย** ไม่ใช่ "เขียนน้อยลง"
    ///
    /// นี่คือข้อที่ทำให้ autosave ไม่ขัด I-1 · ผู้ใช้ที่เปิด RefX ค้างไว้ทั้งวัน
    /// ข้าง Photoshop เพื่อ *ดู* reference ต้องไม่มีการแตะดิสก์แม้แต่ครั้งเดียว
    #[test]
    fn a_board_nobody_touched_is_never_written_to_disk() {
        let mut saver = Autosaver::new(Duration::from_secs(10));
        let start = Instant::now();

        // ปล่อยไว้ครึ่งชั่วโมง (เดินนาฬิกาเอง — ไม่ได้รอจริง ตาม §3.9 ข้อ 5b)
        for minute in 0..30 {
            let now = start + Duration::from_secs(minute * 60);
            assert!(
                !saver.should_write(false, now),
                "นาทีที่ {minute}: board ที่ไม่มีใครแตะกลับขอเขียนดิสก์"
            );
        }
        assert_eq!(saver.writes(), 0, "เขียนดิสก์ทั้งที่ไม่มีอะไรแก้");
        assert_eq!(saver.skipped_clean(), 30);
        assert_eq!(
            saver.skipped_too_soon(),
            0,
            "ต้องข้ามเพราะสะอาด ไม่ใช่เพราะเวลา"
        );
    }

    /// ★★ ลากภาพรัว ๆ ต้องไม่กลายเป็นการเขียนดิสก์ทุกเฟรม
    ///
    /// 200 เฟรมใน ~3.3 วินาที (60 fps) ที่ `dirty` ตลอด ต้องได้การเขียน
    /// **ครั้งเดียว** เพราะระยะเว้นคือ 10 วินาที
    #[test]
    fn dragging_for_hundreds_of_frames_writes_once() {
        let mut saver = Autosaver::new(Duration::from_secs(10));
        let start = Instant::now();

        for frame in 0..200u64 {
            let now = start + Duration::from_millis(frame * 16);
            if saver.should_write(true, now) {
                saver.record_write(now);
            }
        }
        assert_eq!(
            saver.writes(),
            1,
            "ลาก 200 เฟรมแล้วเขียน {} ครั้ง — rate limit ไม่ทำงาน",
            saver.writes()
        );
        assert_eq!(saver.skipped_too_soon(), 199);
    }

    /// ระยะเว้นครบแล้วต้องเขียนอีกครั้ง — ไม่ใช่เขียนครั้งเดียวแล้วเงียบตลอดกาล
    #[test]
    fn the_next_window_writes_again() {
        let mut saver = Autosaver::new(Duration::from_secs(10));
        let start = Instant::now();

        for step in 0..7u64 {
            let now = start + Duration::from_secs(step * 5);
            if saver.should_write(true, now) {
                saver.record_write(now);
            }
        }
        // t=0 เขียน · t=10 · t=20 · t=30 (t=5,15,25 เร็วไป)
        assert_eq!(saver.writes(), 4);
    }

    /// ★ หลังบันทึกจริง การแก้ครั้งถัดไปต้องได้ autosave ทันที ไม่ต้องรอครบรอบ
    #[test]
    fn saving_lets_the_next_edit_be_captured_immediately() {
        let mut saver = Autosaver::new(Duration::from_secs(10));
        let start = Instant::now();
        assert!(saver.should_write(true, start));
        saver.record_write(start);

        let soon = start + Duration::from_secs(1);
        assert!(!saver.should_write(true, soon), "ยังไม่ครบรอบ ต้องไม่เขียน");

        saver.reset(); // ผู้ใช้กด Ctrl+S
        assert!(
            saver.should_write(true, soon),
            "หลังบันทึกจริงแล้วแก้ต่อ ต้อง autosave ได้ทันที"
        );
    }

    /// ★★★ board ที่ `dirty` แล้ว **ปล่อยไว้เฉย ๆ** ต้องยังถูกปลุกมาเขียน
    ///
    /// นี่คือช่วงที่ autosave จำเป็นที่สุด (ผู้ใช้ลุกจากโต๊ะ) และเป็นช่วงที่
    /// I-1 ทำให้ไม่มีเฟรมไหนเกิดขึ้นเลย — ถ้าไม่มี deadline ก็ไม่มีใครถาม
    #[test]
    fn a_dirty_board_left_alone_still_asks_to_be_woken() {
        let mut saver = Autosaver::new(Duration::from_secs(10));
        let start = Instant::now();

        // ยังไม่เคยเขียน + dirty → ต้องขอให้ปลุกทันที
        assert!(saver.next_deadline(true).is_some());
        // สะอาด → ไม่ต้องปลุกเลย (I-1: idle ต้องหลับยาว)
        assert!(saver.next_deadline(false).is_none());

        saver.record_write(start);
        let due = saver.next_deadline(true).expect("dirty อยู่ ต้องมีรอบถัดไป");
        assert_eq!(
            due.duration_since(start),
            Duration::from_secs(10),
            "รอบถัดไปต้องห่างจากครั้งล่าสุดเท่ากับระยะเว้นพอดี"
        );
        assert!(saver.next_deadline(false).is_none(), "บันทึกแล้วต้องหลับยาว");
    }

    // ---------- ไฟล์จริง ----------

    /// snapshot ที่เขียนออกไปต้องอ่านกลับได้เท่าเดิม — **DTO เดียวกับ `.refx`**
    #[test]
    fn a_snapshot_round_trips_through_the_same_format_as_the_document() {
        let dir = temp_dir("roundtrip");
        let doc = dir.join("work.refx");
        let board = board_named("in progress", 5);

        write_snapshot(&doc, &board, rename_durable).unwrap();

        let pending = find_pending(&doc, board_id()).expect("ต้องเจอ snapshot");
        assert_eq!(pending.board, board);
        assert!(pending.written_at.is_some());

        // ★ และมันเป็นไฟล์ `.refx` ที่ถูกต้องทุกประการ — ผิวรูปแบบใหม่เป็นศูนย์
        //   หัวไฟล์ต้องผ่าน `inspect` ตัวเดียวกับเอกสารจริง และเขียนทับได้
        let info = crate::dto::inspect(&read_bytes(&autosave_path(&doc))).unwrap();
        // ★ `LINKED_VERSION` **ไม่ใช่ `FORMAT_VERSION`** — ตัวหลังคือ *เพดานที่อ่านได้*
        //   ซึ่งขยับเป็น 2 ตอน P4-5 · snapshot เขียนด้วยเส้นทาง linked จึงยังเป็น v1
        assert_eq!(info.version, crate::dto::LINKED_VERSION);
        assert!(info.writable);
        assert!(!info.packed);
    }

    fn read_bytes(path: &Path) -> Vec<u8> {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    }

    /// ★★ ทิ้ง snapshot **เมื่อ save สำเร็จเท่านั้น** — และทิ้งซ้ำต้องไม่ล้ม
    #[test]
    fn discarding_removes_it_and_is_safe_to_repeat() {
        let dir = temp_dir("discard");
        let doc = dir.join("work.refx");
        write_snapshot(&doc, &board_named("x", 1), rename_durable).unwrap();
        assert!(autosave_path(&doc).exists());

        discard(&doc);
        assert!(!autosave_path(&doc).exists());
        discard(&doc); // ไม่มีไฟล์แล้ว — ต้องเงียบ ไม่ใช่ล้ม
        assert!(find_pending(&doc, board_id()).is_none());
    }

    /// ★★★ **snapshot ต้องไม่สร้าง `.bak` ตั้งแต่ต้น** (`docs/07 §4` — 22 ส.ค. 2026)
    ///
    /// `.bak` ถูกต้องสำหรับเอกสารของผู้ใช้ แต่ผิดสำหรับ snapshot ซึ่งถูกเขียนทับ
    /// ทุกสิบวินาทีอยู่แล้ว — **สำเนาของสำเนาไม่มีค่า** · ผลของเวอร์ชันก่อนคือไฟล์
    /// ชื่อ `work.refx.refx.bak` (เห็นของจริงข้างเอกสารตอนยืนยัน P4-7a) ที่ไม่มี
    /// ใครอ่านและไม่มีใครกวาด
    ///
    /// ★ ก่อนหน้านี้แก้ด้วยการ **ตามลบทีหลัง** ใน `discard` ซึ่งยังทิ้งไฟล์ไว้
    /// ตลอดช่วงที่ผู้ใช้ยังไม่บันทึก — ตอนนี้แก้ที่ต้นทางแล้ว
    #[test]
    fn a_snapshot_never_leaves_a_backup_beside_the_document() {
        let dir = temp_dir("no-bak");
        let doc = dir.join("work.refx");
        // เขียนสองรอบ — รอบที่สองคือรอบที่ `save_atomic` จะทำ `.bak` ของรอบแรก
        write_snapshot(&doc, &board_named("x", 1), rename_durable).unwrap();
        write_snapshot(&doc, &board_named("x", 2), rename_durable).unwrap();

        let backup = crate::save::backup_path(&autosave_path(&doc));
        assert!(
            !backup.exists(),
            "snapshot สร้าง `.bak` ทิ้งไว้: {}",
            backup.display()
        );
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(left.len(), 1, "เหลือไฟล์เกิน snapshot ตัวเดียว: {left:?}");

        // ★ snapshot ล่าสุดต้องยังอ่านได้ครบ — ความทนทานไม่ได้ลดลงเพราะไม่มี `.bak`
        let pending = find_pending(&doc, board_id()).expect("อ่าน snapshot ไม่ได้");
        assert_eq!(pending.board.len(), 2);
    }

    /// ★★ `.bak` ของ **รุ่นเก่า** ที่ค้างอยู่บนดิสก์ผู้ใช้ต้องถูกเก็บกวาดตอนทิ้ง
    ///
    /// รุ่นก่อน 22 ส.ค. 2026 สร้างมันไว้จริง (เห็นของจริงชื่อ `edit.refx.refx.bak`)
    /// · ไฟล์พวกนั้นไม่หายไปเองเมื่อเราแก้ที่ต้นทาง — ตอนผู้ใช้กด "ทิ้งไป"
    /// คือจังหวะเดียวที่เรามีสิทธิ์ลบมัน
    #[test]
    fn discarding_also_sweeps_a_backup_left_by_an_older_build() {
        let dir = temp_dir("legacy-bak");
        let doc = dir.join("work.refx");
        write_snapshot(&doc, &board_named("x", 1), rename_durable).unwrap();
        // จำลองของที่รุ่นเก่าทิ้งไว้
        let legacy = crate::save::backup_path(&autosave_path(&doc));
        std::fs::write(&legacy, b"snapshot from an older build").unwrap();

        discard(&doc);

        assert!(!autosave_path(&doc).exists());
        assert!(!legacy.exists(), "ของที่รุ่นเก่าทิ้งไว้ยังอยู่: {}", legacy.display());
    }

    /// ★★★ **"เก็บไว้ก่อน" ต้องรอดจาก autosave รอบถัดไป** (`docs/07 §4`)
    ///
    /// นี่คือทั้งหมดของเหตุผลที่ `.kept` มีอยู่: ก่อนหน้านี้ snapshot ที่ผู้ใช้
    /// สั่งเก็บไว้อยู่ที่ `<doc>.refx.autosave` ซึ่งเป็น **ไฟล์เดียวกับที่ autosave
    /// เขียนทับทุกสิบวินาที** ปุ่ม "เก็บไว้ก่อน" จึงเท่ากับ "ทิ้งใน 10 วินาที"
    ///
    /// ★ เทสต์นี้เดินเรื่องจริงทั้งเส้น: ผู้ใช้ตอบ "เก็บไว้ก่อน" → ทำงานต่อ →
    /// autosave เขียนรอบใหม่ → **ของเก่าต้องยังอยู่และเนื้อไม่เปลี่ยน**
    #[test]
    fn work_the_user_kept_survives_the_next_autosave() {
        let dir = temp_dir("kept");
        let doc = dir.join("work.refx");

        // งานที่ค้างจาก session ก่อน (3 ใบ)
        write_snapshot(&doc, &board_named("before the crash", 3), rename_durable).unwrap();
        assert!(keep(&doc, rename_durable).unwrap(), "ไม่มีอะไรถูกย้าย");
        assert!(!autosave_path(&doc).exists(), "ของเดิมยังขวางทาง autosave อยู่");
        assert!(kept_path(&doc).exists());

        // ผู้ใช้ทำงานต่อ — autosave เขียนรอบใหม่ทับที่เดิมสองรอบ
        write_snapshot(&doc, &board_named("new work", 1), rename_durable).unwrap();
        write_snapshot(&doc, &board_named("new work", 2), rename_durable).unwrap();

        let kept = find_kept(&doc, board_id()).expect("ของที่สั่งเก็บไว้หายไป");
        assert_eq!(kept.board.len(), 3, "ของที่สั่งเก็บไว้ถูกเขียนทับด้วยงานใหม่");
        assert_eq!(kept.board.name(), "before the crash");
        // ★ และงานใหม่ก็ไม่ได้เสี่ยงอะไรเลย — มันมี snapshot ของตัวเองตามปกติ
        let pending = find_pending(&doc, board_id()).expect("งานใหม่ไม่มี snapshot");
        assert_eq!(pending.board.len(), 2);
    }

    /// ★★ กด "เก็บไว้ก่อน" ซ้ำ = ตัวใหม่ทับตัวเก่า · และไม่มีอะไรให้ย้ายก็ต้องเงียบ
    #[test]
    fn keeping_twice_leaves_one_file_and_the_newer_content() {
        let dir = temp_dir("kept-twice");
        let doc = dir.join("work.refx");

        write_snapshot(&doc, &board_named("older", 1), rename_durable).unwrap();
        keep(&doc, rename_durable).unwrap();
        write_snapshot(&doc, &board_named("newer", 4), rename_durable).unwrap();
        keep(&doc, rename_durable).unwrap();

        let kept = find_kept(&doc, board_id()).unwrap();
        assert_eq!(kept.board.len(), 4, "ตัวใหม่ไม่ได้ทับตัวเก่า");
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(files.len(), 1, "มีได้ไฟล์เดียวต่อเอกสาร: {files:?}");

        // ไม่มี snapshot ให้ย้าย = เงียบ ไม่ใช่ error
        assert!(!keep(&doc, rename_durable).unwrap());
        assert!(
            find_kept(&doc, board_id()).is_some(),
            "การเรียกซ้ำลบของที่เก็บไว้"
        );
    }

    /// ★★★ **บันทึกสำเร็จต้องไม่ลบของที่ผู้ใช้สั่งเก็บไว้** (`docs/07 §4`)
    ///
    /// `discard` ถูกเรียกทุกครั้งที่ save สำเร็จ · ถ้ามันกวาด `.kept` ไปด้วย
    /// ตัวเลือกที่สามจะตายเงียบ ๆ อีกทางหนึ่ง — ผู้ใช้ที่ยังไม่ตัดสินใจแล้วเผลอ
    /// กด `Ctrl+S` จะไม่มีวันได้ถูกถามอีก
    #[test]
    fn saving_never_removes_what_the_user_asked_to_keep() {
        let dir = temp_dir("kept-vs-save");
        let doc = dir.join("work.refx");
        write_snapshot(&doc, &board_named("kept", 2), rename_durable).unwrap();
        keep(&doc, rename_durable).unwrap();
        write_snapshot(&doc, &board_named("current", 5), rename_durable).unwrap();

        discard(&doc); // ← สิ่งที่เกิดขึ้นตอนกด Ctrl+S สำเร็จ

        assert!(!autosave_path(&doc).exists(), "snapshot ของงานปัจจุบันต้องถูกทิ้ง");
        assert!(kept_path(&doc).exists(), "ของที่ผู้ใช้สั่งเก็บไว้ถูกลบตอนบันทึก");
        assert_eq!(find_kept(&doc, board_id()).unwrap().board.len(), 2);

        // ทางเดียวที่มันหายคือผู้ใช้สั่งเอง
        discard_kept(&doc);
        assert!(!kept_path(&doc).exists());
        discard_kept(&doc); // ซ้ำแล้วต้องเงียบ
    }

    /// ★★★ snapshot ที่เสียหายต้องอ่านเป็น "ไม่มีอะไรให้กู้" **ไม่ใช่เปิดโปรแกรมไม่ได้**
    ///
    /// การทำให้ผู้ใช้เปิดโปรแกรมไม่ได้เพราะไฟล์กู้คืนเสีย คือการเอาเกราะที่สร้าง
    /// มากันงานหาย มาทำให้เขาเข้าถึงงานไม่ได้เสียเอง
    #[test]
    fn a_damaged_snapshot_reads_as_nothing_to_recover() {
        let dir = temp_dir("damaged");
        let doc = dir.join("work.refx");
        write_snapshot(&doc, &board_named("x", 3), rename_durable).unwrap();

        // พลิกไบต์ท้ายไฟล์ — checksum ของ `.refx` จับได้
        let path = autosave_path(&doc);
        let mut bytes = read_bytes(&path);
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();

        assert!(find_pending(&doc, board_id()).is_none());
    }

    /// ★ autosave ผูกกับเอกสาร **ด้วยการอนุมานชื่อ** — จับคู่ผิดไฟล์ไม่ได้
    ///
    /// แทน `doc_hash` ในหัวไฟล์ของดีไซน์เดิม · ให้ผลเดียวกันโดยไม่ต้องเพิ่ม
    /// ฟิลด์ใหม่ (ซึ่งจะสร้างผิวรูปแบบไฟล์ที่ดีไซน์นี้มีไว้เลี่ยงพอดี)
    #[test]
    fn each_document_only_ever_sees_its_own_snapshot() {
        let dir = temp_dir("pairing");
        let mine = dir.join("mine.refx");
        let yours = dir.join("yours.refx");

        write_snapshot(&mine, &board_named("mine", 2), rename_durable).unwrap();

        assert!(find_pending(&mine, board_id()).is_some());
        assert!(
            find_pending(&yours, board_id()).is_none(),
            "เอกสารอื่นเห็น snapshot ที่ไม่ใช่ของตัวเอง"
        );
        assert_ne!(autosave_path(&mine), autosave_path(&yours));
    }

    /// ★★★ **ราคาจริงบน board ที่ใหญ่ที่สุดที่รองรับ (3,072 ใบ)**
    ///
    /// snapshot หนักกว่า journal ต่อการเขียนหนึ่งครั้ง — นี่คือตัวเลขที่บอกว่า
    /// "หนักเกินไปไหม" · ถ้าวันหนึ่งมันเกินรับได้ นี่คือเงื่อนไขที่พาเรากลับไป
    /// คุยเรื่อง journal (`docs/07 §4` เขียนไว้ตรง ๆ)
    ///
    /// ★ **พิมพ์ ไม่ assert เวลา** (§3.9 ข้อ 5b) — แต่ assert *ขนาด* ได้
    /// เพราะขนาดไฟล์เป็นคุณสมบัติของข้อมูล ไม่ใช่ของเครื่อง
    #[test]
    fn what_a_snapshot_of_the_biggest_board_costs() {
        let dir = temp_dir("cost");
        let doc = dir.join("work.refx");
        // ★★ ต้องเป็น **ภาพจริง** ไม่ใช่โน้ตข้อความ — `AssetRef` มี hash 32 ไบต์
        //    ที่บีบไม่ลงเลย + path ของแต่ละไฟล์ · board ที่เป็น text ล้วนจะบีบ
        //    เหลือไม่กี่ KB แล้วตัวเลขที่รายงานจะต่ำกว่าความจริงหลายสิบเท่า
        // 3,072 = เพดานจริงของ board (ขนาด atlas — HANDOFF §6)
        let board = board_of_images(3_072);

        let start = Instant::now();
        write_snapshot(&doc, &board, rename_durable).unwrap();
        let elapsed = start.elapsed();

        let bytes = read_bytes(&autosave_path(&doc)).len();
        #[expect(clippy::cast_precision_loss, reason = "แค่พิมพ์ให้คนอ่าน ไม่ใช่ค่าที่ assert")]
        let kb = bytes as f64 / 1024.0;
        println!(
            "snapshot 3,072 ใบ: {bytes} ไบต์ ({kb:.1} KB) · เขียนใน {elapsed:?} ·              ที่ระยะเว้น 10 วินาทีคือ ~{:.1} KB/s ตอนแก้งานต่อเนื่อง",
            kb / 10.0
        );

        // ★ ที่ 3,072 ใบยังต้องเล็กกว่า 8 MB — เกินกว่านี้แปลว่ามีอะไรผิดปกติ
        //   ใน DTO (เช่นเผลอฝังอะไรที่ไม่ควรฝัง) ไม่ใช่แค่ "board ใหญ่"
        assert!(
            bytes < 8 << 20,
            "snapshot ใหญ่ผิดปกติ ({bytes} ไบต์) — ตรวจว่ามีอะไรหลุดเข้า DTO"
        );
    }

    // ---------- ★★★ วัดว่าฆ่าโปรเซสแล้วเสียงานไปกี่วินาทีจริง ----------

    /// env ที่บอกให้ process ลูกทำตัวเป็น "คนกำลังแก้งาน" ที่จะถูกฆ่า
    const EDITOR_ENV: &str = "REFX_AUTOSAVE_KILL_TARGET";
    /// ชื่อเต็มของ helper — `--exact` ไม่จับชื่อสั้น (บทเรียนจาก P4-2)
    const EDITOR_TEST_PATH: &str = "autosave::tests::edit_victim_helper";
    /// เพิ่ม item หนึ่งใบทุกกี่มิลลิวินาที — "ความเร็วในการทำงาน" ของผู้ใช้จำลอง
    const EDIT_PERIOD: Duration = Duration::from_millis(20);
    /// ระยะเว้น autosave ที่ใช้ในเทสต์ — สั้นกว่าของจริงเพื่อให้เทสต์จบเร็ว
    const TEST_INTERVAL: Duration = Duration::from_secs(1);

    fn ready_marker(doc: &Path) -> PathBuf {
        doc.with_extension("ready")
    }

    /// ★ ชื่อใน [`EDITOR_TEST_PATH`] ต้องตรงกับของจริง ไม่งั้นลูกจะได้
    /// "running 0 tests" แล้วเทสต์วัดจะรอจนหมดเวลาโดยไม่รู้สาเหตุ (เจอมาแล้วใน P4-2)
    #[test]
    fn the_editor_test_path_matches_the_real_one() {
        assert!(EDITOR_TEST_PATH.ends_with("edit_victim_helper"));
        assert!(EDITOR_TEST_PATH.starts_with("autosave::tests::"));
    }

    /// ★★★ **เหยื่อ** — แก้งานไปเรื่อย ๆ พร้อม autosave ตามนโยบายจริง
    ///
    /// เพิ่ม item ทีละใบทุก [`EDIT_PERIOD`] · จำนวน item ใน snapshot จึงเป็น
    /// **นาฬิกาที่อ่านย้อนหลังได้**: `item ที่หายไป × EDIT_PERIOD` = เวลาที่เสีย
    #[test]
    fn edit_victim_helper() {
        let Ok(target) = std::env::var(EDITOR_ENV) else {
            println!("ข้าม: ตัวช่วยของเทสต์วัดเวลาที่เสีย (ไม่ได้ตั้ง {EDITOR_ENV})");
            return;
        };
        let doc = PathBuf::from(target);
        let dir = doc.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut saver = Autosaver::new(TEST_INTERVAL);
        let mut progress = crate::killclock::Progress::new(&crate::killclock::progress_path(&dir))
            .expect("เปิดไฟล์ประวัติไม่ได้");
        let mut items = 0usize;

        std::fs::write(ready_marker(&doc), b"ready").expect("เขียนไฟล์สัญญาณไม่ได้");
        loop {
            items += 1;
            let board = board_named("editing", items);
            // ★★ จด **ก่อน** ตัดสินใจ snapshot — ประวัติจึงครอบงานที่ยังไม่ถูกเก็บ
            //    เสมอ ไม่ใช่ตามหลังมัน (ดู `killclock`: เราต้องวัด ไม่ใช่หาร)
            progress.record(items);
            // ★ เส้นทางเดียวกับของจริงทุกขั้น: นโยบายเดิม · `write_snapshot` เดิม
            //   · `rename_durable` เดิม — ไม่ใช่ตัวจำลอง (docs/08 §3.9 ข้อ 9)
            let now = Instant::now();
            if saver.should_write(true, now) {
                let _ = write_snapshot(&doc, &board, rename_durable);
                saver.record_write(now);
            }
            std::thread::sleep(EDIT_PERIOD);
        }
    }

    /// ★★★ **เกณฑ์ P4-3: ฆ่าโปรเซสระหว่างแก้งาน → วัดว่าเสียไปกี่วินาทีจริง**
    ///
    /// ★★ **วัด ไม่ใช่อนุมานจากค่า N ที่ตั้งไว้** — จังหวะที่ snapshot ถูก flush
    /// เทียบกับจังหวะที่โปรเซสตาย เป็นสิ่งที่รู้ได้ทางเดียวคือลองฆ่าจริง
    ///
    /// วิธีวัด: เหยื่อเพิ่ม item ทีละใบแล้ว **จดลงไฟล์ประวัติว่าใบที่ N เกิดตอนไหน**
    /// → เทียบจำนวน item ใน snapshot กับประวัตินั้น ได้ "เสียไปกี่วินาที" ที่เป็น
    /// ของจริง
    ///
    /// ★★★ **เคยหารเอาจาก `เวลาที่มีชีวิต ÷ EDIT_PERIOD` แล้วแดงบน CI**
    /// (17 ส.ค. 2026) เพราะ runner 2 core ทำได้ช้ากว่า 20 ms ต่อรอบจริง
    /// ตัวหารจึงบอกว่าเหยื่อทำไป 163 ใบทั้งที่ทำน้อยกว่านั้นมาก — เหตุผลเต็ม
    /// และทางแก้อยู่ใน [`crate::killclock`] · **อย่าเอาการหารกลับมา**
    ///
    /// ★ ค่าที่ยอมรับได้คือ **ไม่เกินระยะเว้น + ค่าเผื่อ** — เกินกว่านั้นแปลว่า
    /// snapshot ไม่ได้ลงดิสก์ตามที่นโยบายบอก ซึ่งเป็นคนละเรื่องกับ "นโยบายหลวม"
    #[test]
    fn killing_the_editor_loses_no_more_than_one_interval() {
        const ROUNDS: usize = 12;

        let dir = temp_dir("kill-window");
        let doc = dir.join("work.refx");
        let marker = ready_marker(&doc);
        let exe = std::env::current_exe().expect("หา test binary ของตัวเองไม่เจอ");

        let mut worst = Duration::ZERO;
        let mut measured = 0usize;
        let mut nothing_yet = 0usize;

        for round in 0..ROUNDS {
            let _ = std::fs::remove_file(&marker);
            let _ = std::fs::remove_file(autosave_path(&doc));
            // ประวัติของรอบก่อนต้องไม่ปนมา ไม่งั้นจะวัดงานของคนละโปรเซส
            let _ = std::fs::remove_file(crate::killclock::progress_path(&dir));

            let mut child = std::process::Command::new(&exe)
                .args([EDITOR_TEST_PATH, "--exact", "--nocapture"])
                .env(EDITOR_ENV, &doc)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("spawn เหยื่อไม่สำเร็จ");

            // รอให้เหยื่อพร้อม — ตาข่ายจับค้าง ตั้งหลวม ๆ (§3.9 ข้อ 5b)
            let deadline = Instant::now() + Duration::from_secs(30);
            while !marker.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(marker.exists(), "รอบ {round}: เหยื่อไม่เคยพร้อม");

            // ★ นาฬิกาเริ่มนับ **ตอนเหยื่อบอกว่าพร้อม** ไม่ใช่ตอน spawn
            //   (เวลาสตาร์ต process ไม่ใช่เวลาที่ผู้ใช้ทำงาน)
            let started = Instant::now();
            let alive = Duration::from_millis(900 + (round as u64 * 337) % 2600);
            std::thread::sleep(alive);
            let _ = child.kill();
            let _ = child.wait();
            let lived = started.elapsed();

            // ---- อ่าน snapshot ที่รอดมา ----
            let Some(pending) = find_pending(&doc, board_id()) else {
                // ยังไม่ทันเขียนรอบแรก — เป็นไปได้ถ้าฆ่าเร็วมาก
                nothing_yet += 1;
                continue;
            };
            let saved_items = pending.board.len();
            // ★★★ ถามประวัติที่เหยื่อจดไว้เอง **ห้ามหารจาก `lived`** (ดู `killclock`)
            let timeline = crate::killclock::read(&crate::killclock::progress_path(&dir));
            let (Some(done_items), Some(lost)) =
                (timeline.done(), timeline.lost_after(saved_items))
            else {
                nothing_yet += 1;
                continue;
            };
            worst = worst.max(lost);
            measured += 1;
            println!(
                "รอบ {round}: มีชีวิต {lived:?} · ทำไป {done_items} ใบ · \
                 snapshot มี {saved_items} ใบ · เสีย {lost:?}"
            );
        }

        println!(
            "\n★ วัด {measured} รอบ (อีก {nothing_yet} รอบฆ่าก่อนเขียนรอบแรก) — \
             เสียมากสุด {worst:?} · ระยะเว้นที่ตั้งไว้ {TEST_INTERVAL:?}"
        );

        assert!(
            measured > 0,
            "ไม่มีรอบไหนวัดได้เลย — ฆ่าเร็วเกินไปทุกครั้งจนไม่เคยมี snapshot"
        );
        // ★ เผื่อครึ่งหนึ่งของระยะเว้น: ระหว่างที่ snapshot กำลังเขียน เหยื่อยัง
        //   เพิ่ม item ต่อ ใบพวกนั้นจึงไม่อยู่ในไฟล์ทั้งที่ประวัติจดไว้แล้ว
        //   ★ ค่าเผื่อนี้ **ไม่ได้มีไว้กลบความช้าของเครื่อง** อีกต่อไป —
        //     ตัวเลขที่วัดได้มาจากประวัติของเหยื่อเอง จึงเป็นของจริงบนทุกเครื่อง
        let allowed = TEST_INTERVAL + TEST_INTERVAL / 2;
        assert!(
            worst <= allowed,
            "เสียงานมากสุด {worst:?} เกินระยะเว้น {TEST_INTERVAL:?} (+ค่าเผื่อ) — \
             แปลว่า snapshot ไม่ได้ลงดิสก์ตามที่นโยบายบอก"
        );
    }
}
