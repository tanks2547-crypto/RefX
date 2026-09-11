//! `.refx-meta` ฝั่ง UI — โหลด/เขียน tag ของโฟลเดอร์ที่เปิดดูเฉย ๆ (P5-5)
//!
//! รูปแบบไฟล์ กติกาการจับคู่ และกฎ "ห้ามแตะของที่อ่านไม่ได้" อยู่ที่
//! [`refx_io::sidecar`] · ที่นี่ทำสามอย่างที่ชั้นนั้นทำไม่ได้:
//!
//! 1. **รู้ว่าโฟลเดอร์ไหนอยู่บน board** — และภาพใบไหนอยู่โฟลเดอร์ไหน
//! 2. **แปลง `TagId` ↔ ชื่อแท็ก** เพราะตารางชื่ออยู่ที่ `Board`
//! 3. **เอางาน I/O ออกจาก UI thread** (I-2) และ **ปลุกกลับมาเก็บผล** (`§3.9` ข้อ 18)
//!
//! ## ★★ ทำไมการคืนค่า tag เป็น `Command` (และผลที่ตามมา)
//!
//! `Board::set_meta` เป็น `pub(crate)` — ชั้นนี้แก้ board ตรง ๆ ไม่ได้เลย
//! ซึ่งตรงกับกฎ "ทุก mutation → Command ที่ undo ได้" อยู่แล้ว
//!
//! **ผลที่ตามมาและเป็นสิ่งที่ตั้งใจ:** เปิดโฟลเดอร์ที่มี `.refx-meta` แล้ว
//! ประวัติจะมีขั้น "คืนค่า tag" อยู่หนึ่งขั้น · `Ctrl+Z` ตรงนั้นแปลว่า
//! *"ไม่เอา tag ชุดนั้น"* ซึ่งเป็นความหมายที่อ่านได้ — และดีกว่าทางเลือกอื่น
//! ที่ board ถูกแก้โดยไม่มีใครย้อนได้
//!
//! ## ★★★ สองช่องข้ามเธรด สองวิธีปลุก — ไม่ใช่วิธีเดียวกัน
//!
//! | ช่อง | ใครปลุก | ทำไม |
//! |---|---|---|
//! | โหลด | `Waker` | ผลคือ **tag โผล่บนจอ** ผู้ใช้ต้องเห็นทันที |
//! | เขียน | นาฬิกาของ autosave | ผลคือไฟล์ลงดิสก์ — **ไม่มีอะไรให้วาด** ปลุกให้วาดคือทำให้ `ui-idle-diff` แดงกับพฤติกรรมที่ถูก (`docs/08 §3.9` ข้อ 18) |
//!
//! spec: docs/07-file-format.md §5

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use refx_core::arena::ItemId;
use refx_core::board::{Board, ItemKind, ItemMeta};
use refx_io::settings::SidecarPolicy;
use refx_io::sidecar::{self, Entry, LockReason, Present, Sidecar, WritePermit};

/// สภาพของ `.refx-meta` ของโฟลเดอร์หนึ่ง
#[derive(Debug)]
struct Folder {
    /// ใบอนุญาตเขียน — `None` = **ห้ามแตะ**
    permit: Option<WritePermit>,
    /// เหตุผลที่ห้ามแตะ (ถ้ามี)
    locked: Option<LockReason>,
    /// ★ รายการที่ไม่มีไฟล์ไหนจับคู่ — **ต้องเขียนกลับลงไฟล์เสมอ**
    /// ไฟล์อาจแค่ถูกย้ายออกชั่วคราว (`docs/07 §5`)
    orphans: Vec<Entry>,
    /// มีอะไรเปลี่ยนตั้งแต่เขียนครั้งล่าสุด
    dirty: bool,
    /// บอกผู้ใช้ไปแล้วว่าโฟลเดอร์นี้เขียนไม่ได้ — ★ ห้ามบ่นซ้ำทุกครั้งที่ติดดาว
    told: bool,
}

/// ผลของการโหลดหนึ่งโฟลเดอร์ ที่เดินทางข้ามเธรดกลับมา
#[derive(Debug)]
pub struct Loaded {
    dir: PathBuf,
    load: sidecar::Load,
}

/// ผลของการเขียนหนึ่งโฟลเดอร์
#[derive(Debug)]
pub struct Written {
    dir: PathBuf,
    /// ใบอนุญาตใบใหม่ (ไฟล์เพิ่งเปลี่ยน ใบเก่าจึงหมดอายุ) หรือเหตุผลที่ล้ม
    result: Result<Option<WritePermit>, String>,
}

/// สิ่งที่ต้องบอกผู้ใช้ — ★ ชั้นนี้ไม่ประกอบประโยคเอง (`docs/03 §0`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Say {
    /// คืนค่า tag ของ `n` ใบจาก `.refx-meta` แล้ว
    Restored {
        /// กี่ใบ
        items: usize,
    },
    /// เขียนไม่ได้ — tag ยังใช้ได้ในเซสชันนี้ แต่จะไม่ถูกบันทึก
    CannotWrite {
        /// โฟลเดอร์ที่เขียนไม่ได้
        dir: PathBuf,
    },
    /// ★ มี `.refx-meta` ที่เราอ่านไม่ได้อยู่ — **ไม่แตะ** และบอกให้รู้
    HandsOff {
        /// โฟลเดอร์นั้น
        dir: PathBuf,
        /// เพราะอะไร
        reason: LockReason,
    },
}

/// ทุกโฟลเดอร์ที่ board ใบนี้แตะอยู่
#[derive(Debug, Default)]
pub struct Folders {
    known: BTreeMap<PathBuf, Folder>,
    loading: Option<crossbeam_channel::Receiver<Vec<Loaded>>>,
    writing: Option<crossbeam_channel::Receiver<Vec<Written>>>,
    /// โฟลเดอร์ที่กำลังรอคำตอบ "จะเขียนไหม" — `Some` = แผงคำถามเปิดอยู่
    pub asking: Option<PathBuf>,
}

impl Folders {
    /// โฟลเดอร์ที่มีภาพอยู่บน board แต่เรายังไม่เคยเปิด `.refx-meta` ของมัน
    fn unseen(&self, board: &Board) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for dir in folders_of(board) {
            if !self.known.contains_key(&dir) && !out.contains(&dir) {
                out.push(dir);
            }
        }
        out
    }

    /// เริ่มโหลด `.refx-meta` ของโฟลเดอร์ที่ยังไม่เคยเปิด — **บน worker** (I-2)
    ///
    /// ★★★ ปลุก event loop หลังส่งผลเสมอ ไม่งั้น tag จะไม่โผล่จนกว่าผู้ใช้จะขยับเมาส์
    /// (`docs/08 §3.9` ข้อ 18)
    pub fn start_load(&mut self, board: &Board, waker: Option<&refx_platform::window::Waker>) {
        if self.loading.is_some() {
            return; // รอบก่อนยังไม่กลับ — อย่าซ้อน
        }
        let dirs = self.unseen(board);
        if dirs.is_empty() {
            return;
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        let waker = waker.cloned();
        let spawned = std::thread::Builder::new()
            .name("refx-sidecar-load".to_owned())
            .spawn(move || {
                let loaded: Vec<Loaded> = dirs
                    .into_iter()
                    .map(|dir| {
                        let load = sidecar::load(&sidecar::path_for(&dir));
                        Loaded { dir, load }
                    })
                    .collect();
                let _ = tx.send(loaded);
                // ★★★ ผลนี้ต้องขึ้นจอ — ปลุก (`docs/08 §3.9` ข้อ 18)
                if let Some(waker) = waker {
                    waker.wake();
                }
            });
        match spawned {
            Ok(_) => self.loading = Some(rx),
            Err(err) => tracing::warn!(%err, "cannot start the sidecar reader"),
        }
    }

    /// เก็บผลการโหลดแล้วคืนคำสั่งที่ต้องเดินผ่าน `History`
    ///
    /// คืน `None` เมื่อยังไม่มีผลกลับมา — ★ ไม่บล็อก UI thread เด็ดขาด
    pub fn collect_load(&mut self, board: &Board) -> Option<Restore> {
        let rx = self.loading.as_ref()?;
        let batch = match rx.try_recv() {
            Ok(batch) => batch,
            Err(crossbeam_channel::TryRecvError::Empty) => return None,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                self.loading = None;
                return None;
            }
        };
        self.loading = None;

        let mut restore = Restore::default();
        for Loaded { dir, load } in batch {
            let (permit, locked, sidecar) = match load {
                sidecar::Load::Fresh(permit) => (Some(permit), None, Sidecar::default()),
                sidecar::Load::Opened(sidecar, permit) => (Some(permit), None, sidecar),
                sidecar::Load::HandsOff(reason) => {
                    restore.say.push(Say::HandsOff {
                        dir: dir.clone(),
                        reason,
                    });
                    (None, Some(reason), Sidecar::default())
                }
            };

            let here = images_in(board, &dir);
            let present: Vec<Present> = here
                .iter()
                .map(|(_, name, hash)| Present {
                    file_name: name.clone(),
                    hash: *hash,
                })
                .collect();
            let matched = sidecar::resolve(&sidecar, &present);

            for ((id, _, _), at) in here.iter().zip(&matched) {
                let Some(entry) = at.and_then(|at| sidecar.entries.get(at)) else {
                    continue;
                };
                let Some(item) = board.item(*id) else {
                    continue;
                };
                let mut next = item.meta.clone();
                entry.apply_to(&mut next);
                let next = next.sanitized();
                if next != item.meta {
                    restore.meta.push((*id, next));
                }
                for tag in &entry.tags {
                    restore.tags.entry(tag.clone()).or_default().push(*id);
                }
            }

            self.known.insert(
                dir,
                Folder {
                    permit,
                    locked,
                    orphans: sidecar::orphans(&sidecar, &matched),
                    dirty: false,
                    told: false,
                },
            );
        }
        Some(restore)
    }

    /// ★ โฟลเดอร์ของภาพเหล่านี้มีอะไรเปลี่ยน — ต้องเขียนรอบหน้า
    ///
    /// คืนสิ่งที่ต้องบอกผู้ใช้: โฟลเดอร์ที่ **เขียนไม่ได้** ต้องไม่เงียบ
    pub fn touched(&mut self, board: &Board, ids: &[ItemId]) -> Vec<Say> {
        let mut say = Vec::new();
        for dir in ids
            .iter()
            .filter_map(|id| board.item(*id))
            .filter_map(|item| match &item.kind {
                ItemKind::Image(asset) => asset.path.parent().map(Path::to_path_buf),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>()
        {
            let Some(folder) = self.known.get_mut(&dir) else {
                continue;
            };
            folder.dirty = true;
            if folder.permit.is_none() && !folder.told {
                folder.told = true;
                let reason = folder.locked;
                say.push(match reason {
                    Some(reason) => Say::HandsOff {
                        dir: dir.clone(),
                        reason,
                    },
                    None => Say::CannotWrite { dir: dir.clone() },
                });
            }
        }
        say
    }

    /// โฟลเดอร์แรกที่ยังไม่เคยมี `.refx-meta` และมีของรอเขียน — ใช้ตั้งคำถาม
    #[must_use]
    pub fn first_needing_an_answer(&self) -> Option<&Path> {
        self.known
            .iter()
            .find(|(_, folder)| folder.dirty && folder.permit.is_some())
            .map(|(dir, _)| dir.as_path())
    }

    /// เริ่มเขียนโฟลเดอร์ที่ค้างอยู่ — **บน worker** (I-2)
    ///
    /// ★★★ **ไม่มี `Waker`** โดยเจตนา — ผลคือไฟล์ลงดิสก์ ไม่มีอะไรให้วาด
    /// ผลถูกเก็บโดยนาฬิกาของ autosave เหมือน snapshot (`docs/08 §3.9` ข้อ 18)
    pub fn start_write(&mut self, board: &Board, policy: SidecarPolicy) {
        if self.writing.is_some() || policy == SidecarPolicy::Never {
            return;
        }
        let mut jobs: Vec<(PathBuf, Sidecar, WritePermit)> = Vec::new();
        for (dir, folder) in &mut self.known {
            if !folder.dirty {
                continue;
            }
            let Some(permit) = folder.permit else {
                continue; // ห้ามแตะ — และผู้ใช้ถูกบอกไปแล้วที่ `touched`
            };
            folder.dirty = false;
            let mut rows = rows_for(board, dir);
            rows.extend(folder.orphans.iter().cloned()); // ★ ของที่ย้ายออกต้องไม่หาย
            jobs.push((dir.clone(), Sidecar::new(rows), permit));
        }
        if jobs.is_empty() {
            return;
        }

        let (tx, rx) = crossbeam_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("refx-sidecar-write".to_owned())
            .spawn(move || {
                let done: Vec<Written> = jobs
                    .into_iter()
                    .map(|(dir, sidecar, permit)| {
                        let path = sidecar::path_for(&dir);
                        let result = sidecar::write_atomic(
                            &path,
                            &sidecar,
                            &permit,
                            refx_platform::fsops::rename_durable,
                        )
                        .map_err(|err| err.to_string())
                        // ★ ไฟล์เพิ่งเปลี่ยน ใบอนุญาตใบเก่าจึงหมดอายุทันที —
                        //   ขอใบใหม่ตรงนี้ ไม่ใช่รอบหน้า ไม่งั้นการเขียนครั้งที่สอง
                        //   จะถูกปฏิเสธด้วย `ChangedUnderneath` ที่เราเป็นคนทำเอง
                        .map(|()| sidecar::load(&path).permit().copied());
                        Written { dir, result }
                    })
                    .collect();
                let _ = tx.send(done);
                // ★★★ **ไม่ปลุก** — ดูตารางที่หัวโมดูล (`§3.9` ข้อ 18 ชนิด `OwnTimer`)
            });
        match spawned {
            Ok(_) => self.writing = Some(rx),
            Err(err) => tracing::warn!(%err, "cannot start the sidecar writer"),
        }
    }

    /// เก็บผลการเขียน — คืนสิ่งที่ต้องบอกผู้ใช้
    pub fn collect_write(&mut self) -> Vec<Say> {
        let Some(rx) = self.writing.as_ref() else {
            return Vec::new();
        };
        let batch = match rx.try_recv() {
            Ok(batch) => batch,
            Err(crossbeam_channel::TryRecvError::Empty) => return Vec::new(),
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                self.writing = None;
                return Vec::new();
            }
        };
        self.writing = None;

        let mut say = Vec::new();
        for Written { dir, result } in batch {
            let Some(folder) = self.known.get_mut(&dir) else {
                continue;
            };
            match result {
                Ok(permit) => folder.permit = permit,
                Err(err) => {
                    tracing::warn!(%err, "cannot write the sidecar");
                    // ★ เขียนไม่ได้ = **ห้ามเงียบ** · และห้ามลองรัวไม่จบ
                    folder.permit = None;
                    if !folder.told {
                        folder.told = true;
                        say.push(Say::CannotWrite { dir });
                    }
                }
            }
        }
        say
    }

    /// มีงานเขียนค้างอยู่ไหม — นาฬิกาของ autosave ใช้ตัดสินว่าต้องตื่นมาเก็บผล
    #[must_use]
    pub fn has_work_outstanding(&self) -> bool {
        self.writing.is_some()
    }

    /// จำคำตอบของผู้ใช้สำหรับโฟลเดอร์ที่ถามค้างไว้
    pub fn answer(&mut self, write_it: bool) {
        let Some(dir) = self.asking.take() else {
            return;
        };
        if !write_it && let Some(folder) = self.known.get_mut(&dir) {
            // ★ "ไม่" ถูกจำไว้ที่ `settings.toml` — **ห้ามเขียนคำว่าไม่ลงโฟลเดอร์นั้น**
            folder.permit = None;
            folder.dirty = false;
            folder.told = true;
        }
    }

    /// ★ ลืมทุกอย่าง — ใช้ตอนเอกสารถูกบันทึกเป็น `.refx` (เจ้าของเดียว)
    pub fn disown(&mut self) {
        self.known.clear();
        self.loading = None;
        self.writing = None;
        self.asking = None;
    }

    /// รู้จักโฟลเดอร์นี้อยู่ไหม (สำหรับเทสต์และ status bar)
    #[must_use]
    pub fn knows(&self, dir: &Path) -> bool {
        self.known.contains_key(dir)
    }
}

/// สิ่งที่ต้องเดินผ่าน `History` เพื่อคืนค่า tag ที่อ่านมาได้
#[derive(Debug, Default)]
pub struct Restore {
    /// meta ที่ต้องทับ (rating · ป้ายสี · โน้ต · ปักหมุด)
    pub meta: Vec<(ItemId, ItemMeta)>,
    /// ชื่อแท็ก → ภาพที่ต้องติดแท็กนั้น
    pub tags: BTreeMap<String, Vec<ItemId>>,
    /// สิ่งที่ต้องบอกผู้ใช้
    pub say: Vec<Say>,
}

impl Restore {
    /// ไม่มีอะไรต้องทำเลยไหม — ★ ไม่มี = **ไม่ขอเฟรม** (I-1)
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.meta.is_empty() && self.tags.is_empty() && self.say.is_empty()
    }

    /// จำนวนภาพที่ได้ของกลับคืน
    #[must_use]
    pub fn items(&self) -> usize {
        let mut ids: std::collections::BTreeSet<ItemId> =
            self.meta.iter().map(|(id, _)| *id).collect();
        for targets in self.tags.values() {
            ids.extend(targets.iter().copied());
        }
        ids.len()
    }
}

// ---------------------------------------------------------------------------
// ตัวช่วยที่อ่าน board (ไม่มี I/O — เทสต์ได้โดยไม่ต้องมีดิสก์)
// ---------------------------------------------------------------------------

/// โฟลเดอร์ทั้งหมดที่ภาพบน board อยู่
#[must_use]
pub fn folders_of(board: &Board) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = board
        .items_in_z_order()
        .filter_map(|(_, item)| match &item.kind {
            // ★ ภาพที่ฝังอยู่ในเอกสารไม่มีโฟลเดอร์ให้เขียน sidecar ลงไป
            ItemKind::Image(asset) if !asset.embedded => asset.path.parent(),
            _ => None,
        })
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// ภาพที่อยู่ในโฟลเดอร์นี้ — `(id, ชื่อไฟล์, hash)`
fn images_in(board: &Board, dir: &Path) -> Vec<(ItemId, String, refx_core::hash::ContentHash)> {
    board
        .items_in_z_order()
        .filter_map(|(id, item)| match &item.kind {
            ItemKind::Image(asset) if !asset.embedded && asset.path.parent() == Some(dir) => {
                let name = asset.path.file_name()?.to_str()?.to_owned();
                Some((id, name, asset.hash))
            }
            _ => None,
        })
        .collect()
}

/// รายการที่จะเขียนลง `.refx-meta` ของโฟลเดอร์นี้
fn rows_for(board: &Board, dir: &Path) -> Vec<Entry> {
    images_in(board, dir)
        .into_iter()
        .filter_map(|(id, name, hash)| {
            let item = board.item(id)?;
            let tags: Vec<String> = item
                .meta
                .tags
                .iter()
                .filter_map(|tag| board.tags().name(*tag).map(str::to_owned))
                .collect();
            Some(Entry::from_meta(name, hash, &item.meta, tags))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use refx_core::board::{AssetRef, ImageFormat, Item, ItemKind};
    use refx_core::command::{AddItems, History};
    use refx_core::glam::UVec2;
    use refx_core::hash::ContentHash;

    /// ★ ใส่ภาพเข้า board ผ่าน `Command` เหมือนของจริง — `insert_item` เป็น
    /// `pub(crate)` และนั่นถูกต้อง (`docs/08 §4` ข้อ 10)
    fn put(board: &mut Board, history: &mut History, items: Vec<Item>) -> Vec<ItemId> {
        let command = AddItems::new(items).unwrap();
        history.apply(board, Box::new(command)).unwrap();
        // ★ `ItemId` ถูกแจกตอน `apply` ไม่ใช่ตอนสร้างคำสั่ง — อ่านจาก board
        //   หลังจากนั้นจึงเป็นทางเดียวที่ได้ id จริง
        board.items_in_z_order().map(|(id, _)| id).collect()
    }

    fn history() -> History {
        History::new(64, 1 << 20)
    }

    fn hash_of(seed: u8) -> ContentHash {
        ContentHash::from_bytes([seed; 32])
    }

    fn image(path: &str, seed: u8) -> Item {
        Item::new(ItemKind::Image(AssetRef {
            hash: hash_of(seed),
            path: PathBuf::from(path),
            px_size: UVec2::new(64, 64),
            format: ImageFormat::Unknown,
            embedded: false,
            mtime: 0,
            file_size: 0,
        }))
    }

    #[test]
    fn every_folder_on_the_board_is_found_once() {
        let mut board = Board::default();
        let mut h = history();
        put(
            &mut board,
            &mut h,
            vec![
                image("/photos/a.png", 1),
                image("/photos/b.png", 2),
                image("/other/c.png", 3),
            ],
        );

        let dirs = folders_of(&board);
        assert_eq!(
            dirs,
            vec![PathBuf::from("/other"), PathBuf::from("/photos")],
            "โฟลเดอร์เดียวกันต้องนับครั้งเดียว และลำดับต้อง deterministic"
        );
    }

    /// ★ ภาพที่ฝังอยู่ในเอกสาร (packed) ไม่มีโฟลเดอร์ให้เขียน
    #[test]
    fn an_embedded_image_has_no_folder_to_write_into() {
        let mut board = Board::default();
        let mut h = history();
        let mut item = image("/photos/a.png", 1);
        if let ItemKind::Image(asset) = &mut item.kind {
            asset.embedded = true;
        }
        put(&mut board, &mut h, vec![item]);
        assert!(folders_of(&board).is_empty());
    }

    /// ★ ภาพจาก clipboard ไม่มี path จริง — ต้องไม่ทำให้เกิดโฟลเดอร์ว่าง
    #[test]
    fn a_pasted_image_never_invents_a_folder() {
        let mut board = Board::default();
        let mut h = history();
        put(&mut board, &mut h, vec![image("pasted.png", 9)]);
        assert!(folders_of(&board).is_empty(), "ได้ {:?}", folders_of(&board));
    }

    /// ★★ แถวที่จะเขียนต้องมี **ชื่อแท็ก** ไม่ใช่ `TagId`
    #[test]
    fn the_rows_carry_tag_names_not_board_local_ids() {
        let mut board = Board::default();
        let mut h = history();
        let ids = put(&mut board, &mut h, vec![image("/photos/a.png", 1)]);
        h.apply(
            &mut board,
            Box::new(refx_core::command::TagItems::attach("มังกร", ids).unwrap()),
        )
        .unwrap();

        let rows = rows_for(&board, Path::new("/photos"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file_name, "a.png");
        assert_eq!(rows[0].tags, vec!["มังกร".to_owned()]);
    }

    /// ★★★ โฟลเดอร์ที่ยังไม่รู้จักต้องไม่ถูกทำเครื่องหมายว่าต้องเขียน
    ///
    /// ไม่งั้นการติดดาวก่อนที่ `.refx-meta` จะโหลดเสร็จ จะทำให้เราเขียนทับ
    /// ไฟล์ที่ยังไม่ได้อ่าน = **ลบ tag ของผู้ใช้ทิ้งทั้งโฟลเดอร์**
    #[test]
    fn a_folder_we_have_not_read_yet_is_never_written_over() {
        let mut board = Board::default();
        let mut h = history();
        let ids = put(&mut board, &mut h, vec![image("/photos/a.png", 1)]);
        let mut folders = Folders::default();
        assert!(folders.touched(&board, &ids).is_empty());
        folders.start_write(&board, SidecarPolicy::Always);
        assert!(
            !folders.has_work_outstanding(),
            "ยังไม่ได้อ่านไฟล์เลยแต่จะเขียนทับแล้ว"
        );
    }

    /// ★ `Never` ต้องไม่แตะดิสก์เลยแม้แต่ครั้งเดียว
    #[test]
    fn the_never_policy_starts_no_writer_at_all() {
        let board = Board::default();
        let mut folders = Folders::default();
        folders.start_write(&board, SidecarPolicy::Never);
        assert!(!folders.has_work_outstanding());
    }

    /// ★ นับ "กี่ใบได้ของคืน" ต้องไม่นับซ้ำเมื่อใบเดียวได้ทั้ง meta และ tag
    #[test]
    fn one_picture_that_got_both_back_is_counted_once() {
        let mut board = Board::default();
        let mut h = history();
        let id = put(&mut board, &mut h, vec![image("/photos/a.png", 1)])[0];
        let mut restore = Restore::default();
        restore.meta.push((id, ItemMeta::default()));
        restore.tags.insert("a".to_owned(), vec![id]);
        restore.tags.insert("b".to_owned(), vec![id]);
        assert_eq!(restore.items(), 1);
    }
}
