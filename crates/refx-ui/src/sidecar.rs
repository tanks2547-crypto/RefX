//! `.refx-meta` ฝั่ง UI — โหลด/เขียน tag ของโฟลเดอร์ที่เปิดดูเฉย ๆ (P5-5)
//!
//! รูปแบบไฟล์ กติกาการจับคู่ และกฎ "ห้ามแตะของที่อ่านไม่ได้" อยู่ที่
//! [`refx_io::sidecar`] · ที่นี่ทำสามอย่างที่ชั้นนั้นทำไม่ได้:
//!
//! 1. **รู้ว่าโฟลเดอร์ไหนอยู่บน board** — และภาพใบไหนอยู่โฟลเดอร์ไหน
//! 2. **แปลง `TagId` ↔ ชื่อแท็ก** เพราะตารางชื่ออยู่ที่ `Board`
//! 3. **เอางาน I/O ออกจาก UI thread** (I-2) และ **ปลุกกลับมาเก็บผล** (`§3.9` ข้อ 18)
//!
//! ## ★★★ การ **โหลด** ไม่ใช่การ **แก้** — ไม่ผ่าน `Command` (`docs/02 §2.9`)
//!
//! รุ่นแรกคืนแท็กผ่าน `EditMeta` เพราะ `Board::set_meta` เป็น `pub(crate)` ·
//! **เหตุผลนั้นเป็นความสะดวกของตัวแปรภาษา ไม่ใช่เหตุผลเชิงออกแบบ** และมันทำให้
//! เปิดโฟลเดอร์เฉย ๆ แล้วเอกสาร `dirty` ทันที · `Ctrl+Z` ครั้งแรกหลังเปิด
//! ไปลบแท็กที่ผู้ใช้บันทึกไว้เอง
//!
//! → ทางที่ถูกคือ [`refx_core::board::Board::restore_meta`] ซึ่งเป็นทางที่
//! **ตั้งใจเปิดไว้** แบบเดียวกับ `set_view` — ไม่ `Command` · ไม่ขึ้นสแตก
//! · **ไม่ `dirty`** · แต่บวก `revision` เพราะดาว/แท็กเป็น input ของ filter
//!
//! > ★ เส้นแบ่ง: กด `Ctrl+Z` แล้วอ่านว่า *"ไม่เอาสิ่งที่ฉันเพิ่งทำ"* = `Command`
//! > · อ่านว่า *"ไม่เอาสิ่งที่ไฟล์บอกมา"* = ไม่ใช่ `Command`
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
use refx_core::board::{Board, ItemKind, RestoredMeta};
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
    /// ★★★ โฟลเดอร์นี้ **มี `.refx-meta` อยู่แล้วตอนเราเปิด**
    ///
    /// ไฟล์ที่มีอยู่ **คือบันทึกการอนุญาต**ของผู้ใช้เอง — เขาเคยตอบตกลงไปแล้ว
    /// จึงไม่ต้องถามซ้ำทุกครั้งที่เปิดโปรแกรม
    had_file: bool,
    /// ★★★ ผู้ใช้เพิ่งตอบตกลงในเซสชันนี้
    granted: bool,
    /// บอกผู้ใช้ไปแล้วว่าโฟลเดอร์นี้เขียนไม่ได้ — ★ ห้ามบ่นซ้ำทุกครั้งที่ติดดาว
    told: bool,
}

impl Folder {
    /// ★★★ เขียนลงโฟลเดอร์นี้ได้หรือยัง — **opt-in คือกฎ ไม่ใช่มารยาท**
    ///
    /// เจอบนแอปจริง 11 ก.ย. 2026: นาฬิกาเขียนไฟล์ไป **ระหว่างที่คำถามยังอยู่บนจอ**
    /// ผู้ใช้กด "ไม่ต้องตอนนี้" แล้วไฟล์ก็อยู่ในโฟลเดอร์เขาแล้ว — คำถามที่ถามไป
    /// หลังลงมือทำ ไม่ใช่การขออนุญาต
    fn may_write(&self, policy: SidecarPolicy) -> bool {
        match policy {
            SidecarPolicy::Never => false,
            SidecarPolicy::Always => true,
            SidecarPolicy::Ask => self.had_file || self.granted,
        }
    }
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
            let mut had_file = false;
            let (permit, locked, sidecar) = match load {
                sidecar::Load::Fresh(permit) => (Some(permit), None, Sidecar::default()),
                sidecar::Load::Opened(sidecar, permit) => {
                    had_file = true;
                    (Some(permit), None, sidecar)
                }
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
                restore.meta.push((
                    *id,
                    RestoredMeta {
                        rating: entry.rating,
                        color_label: entry.color_label,
                        note: entry.note.clone(),
                        pinned: entry.pinned,
                        tags: entry.tags.clone(),
                    },
                ));
            }

            // ★★ แท็กที่บันทึกไว้แต่ไม่มีไฟล์ไหนรับ — **ของยังอยู่ ต้องบอกให้รู้**
            let stranded = sidecar::orphans(&sidecar, &matched);
            restore.unmatched += stranded.len();

            // ★★★ **มีใบไหนถูกจับคู่ด้วย hash ไม่ใช่ด้วยชื่อ = ผู้ใช้เปลี่ยนชื่อไฟล์**
            //
            //   ถ้าไม่เขียนกลับ ชื่อใน `.refx-meta` จะค้างของเก่าไปเรื่อย ๆ แล้ว
            //   วันที่ผู้ใช้ **แก้เนื้อไฟล์ด้วย** (ชื่อไม่ตรง + hash ไม่ตรง)
            //   แท็กจะหายทั้งที่แต่ละอย่างแยกกันยังหาเจอ
            //
            //   ★ ทำเฉพาะโฟลเดอร์ที่ **มีไฟล์อยู่แล้ว** (เขาเคยตอบตกลงไปแล้ว) —
            //     โฟลเดอร์เปล่าต้องไม่ถูกทำเครื่องหมายว่าต้องเขียน ไม่งั้น
            //     แค่เปิดโฟลเดอร์ก็จะมีคำถามเด้งขึ้นมาโดยที่ผู้ใช้ยังไม่ได้ทำอะไร
            //   ★★ และ **hash ก็เหมือนกัน**: ผู้ใช้ที่แก้ภาพใน Photoshop วันนี้
            //     แล้วเปลี่ยนชื่อมันเดือนหน้า จะเสียแท็กถ้าเราปล่อยพยานให้เก่าค้าง
            //     — สองกติกาของ `docs/07 §5` ต้องใช้ได้ทั้งคู่เสมอ ไม่ใช่ทีละอย่าง
            let moved = !sidecar.entries.is_empty()
                && here.iter().zip(&matched).any(|((_, name, hash), at)| {
                    at.is_some_and(|at| {
                        sidecar
                            .entries
                            .get(at)
                            .is_some_and(|e| &e.file_name != name || &e.hash != hash)
                    })
                });

            self.known.insert(
                dir,
                Folder {
                    permit,
                    locked,
                    orphans: stranded,
                    dirty: moved,
                    had_file,
                    granted: false,
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

    /// โฟลเดอร์แรกที่มีของรอเขียนแต่ **ยังไม่ได้รับอนุญาต** — ใช้ตั้งคำถาม
    ///
    /// ★ โฟลเดอร์ที่มี `.refx-meta` อยู่แล้วไม่ถูกถามซ้ำ — ไฟล์นั้นคือคำตอบ
    /// ที่ผู้ใช้เคยให้ไว้ · ถามทุกครั้งที่เปิดโปรแกรมคือการไม่ฟังคำตอบเดิม
    #[must_use]
    pub fn first_needing_an_answer(&self, policy: SidecarPolicy) -> Option<&Path> {
        if policy != SidecarPolicy::Ask {
            return None;
        }
        self.known
            .iter()
            .find(|(_, folder)| {
                folder.dirty && folder.permit.is_some() && !folder.may_write(policy)
            })
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
            // ★★★ ยังไม่ได้รับอนุญาต = **ไม่เขียน และไม่ล้างธง** — รอคำตอบ
            //     ล้างธงตรงนี้จะทำให้การติดดาวระหว่างรอคำตอบหายไปเงียบ ๆ
            if !folder.may_write(policy) {
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
        let Some(folder) = self.known.get_mut(&dir) else {
            return;
        };
        if write_it {
            folder.granted = true;
        } else {
            // ★ "ไม่" ถูกจำไว้ **ในหน่วยความจำของเซสชันนี้เท่านั้น** —
            //   ห้ามเขียนคำว่าไม่ลงโฟลเดอร์นั้น (`docs/07 §5`) · ค่าถาวรอยู่ที่
            //   `settings.toml` ซึ่งผู้ใช้ตั้งเองจากแผง Settings
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
    /// meta ที่อ่านมาได้ — ★ ถือ **ชื่อแท็ก** ตัวแปลงเป็น `TagId` อยู่ที่ `Board`
    pub meta: Vec<(ItemId, RestoredMeta)>,
    /// ★★ รายการใน `.refx-meta` ที่ **ไม่มีไฟล์ไหนจับคู่ด้วย** (`docs/07 §5`)
    ///
    /// ของไม่ได้หาย แค่ไม่ติด — รายการยังอยู่ในไฟล์และกลับมาเองถ้าไฟล์กลับมา
    /// · ผู้ใช้ต้องรู้ว่ามันยังอยู่ **ห้ามเดาแทนเขา** และห้ามเงียบ
    pub unmatched: usize,
    /// สิ่งที่ต้องบอกผู้ใช้
    pub say: Vec<Say>,
}

impl Restore {
    /// ไม่มีอะไรต้องทำเลยไหม — ★ ไม่มี = **ไม่ขอเฟรม** (I-1)
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.meta.is_empty() && self.unmatched == 0 && self.say.is_empty()
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

    fn entry_of(name: &str, seed: u8, rating: u8) -> Entry {
        Entry {
            file_name: name.to_owned(),
            hash: hash_of(seed),
            rating,
            color_label: None,
            note: String::new(),
            pinned: false,
            tags: Vec::new(),
        }
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

    /// ★★★ ชื่อที่เปลี่ยนไปต้องถูกเขียนกลับ — ไม่งั้นแท็กจะหายในวันที่
    /// ผู้ใช้ทั้งเปลี่ยนชื่อ **และ** แก้เนื้อไฟล์
    #[test]
    fn a_rename_is_written_back_so_the_name_stops_being_stale() {
        let mut board = Board::default();
        let mut h = history();
        put(&mut board, &mut h, vec![image("/photos/new-name.png", 7)]);

        let stored = Sidecar::new(vec![Entry {
            file_name: "old-name.png".to_owned(),
            hash: hash_of(7),
            rating: 5,
            color_label: None,
            note: String::new(),
            pinned: false,
            tags: Vec::new(),
        }]);
        let present = [Present {
            file_name: "new-name.png".to_owned(),
            hash: hash_of(7),
        }];
        let matched = sidecar::resolve(&stored, &present);
        assert_eq!(matched, vec![Some(0)], "hash ต้องจับคู่ให้ได้ก่อน");

        // ★ ประตูของประตู: ชื่อที่ **ไม่ได้** เปลี่ยน ต้องไม่ถูกนับว่าต้องเขียน
        let same = [Present {
            file_name: "old-name.png".to_owned(),
            hash: hash_of(7),
        }];
        let matched_same = sidecar::resolve(&stored, &same);
        let changed = |names: &[Present], m: &[Option<usize>]| {
            names.iter().zip(m).any(|(p, at)| {
                at.is_some_and(|at| {
                    stored
                        .entries
                        .get(at)
                        .is_some_and(|e| e.file_name != p.file_name)
                })
            })
        };
        assert!(changed(&present, &matched), "เปลี่ยนชื่อแล้วต้องรู้ว่าต้องเขียนกลับ");
        assert!(
            !changed(&same, &matched_same),
            "ชื่อเดิมต้องไม่ทำให้เขียนไฟล์ทุกครั้งที่เปิด"
        );

        // ★★ เนื้อไฟล์ที่เปลี่ยน (Photoshop) ก็ต้องทำให้พยานถูกเขียนใหม่ —
        //    ไม่งั้นวันที่เขาเปลี่ยนชื่อมันเดือนหน้า แท็กจะหาย
        let edited = [Present {
            file_name: "old-name.png".to_owned(),
            hash: hash_of(99),
        }];
        let matched_edited = sidecar::resolve(&stored, &edited);
        assert_eq!(matched_edited, vec![Some(0)], "ชื่อตรงต้องจับคู่แม้ hash ไม่ตรง");
        let witness_stale = edited.iter().zip(&matched_edited).any(|(p, at)| {
            at.is_some_and(|at| stored.entries.get(at).is_some_and(|e| e.hash != p.hash))
        });
        assert!(witness_stale, "พยานที่เก่าแล้วต้องถูกเขียนใหม่");
    }

    /// ★★★ **ตอบว่าไม่ แล้วต้องไม่มีไฟล์เกิดขึ้นเลย** — และระหว่างที่ยังไม่ตอบ
    /// ก็ต้องไม่เขียน · เจอบนแอปจริง 11 ก.ย. 2026: นาฬิกาเขียนไปก่อนผู้ใช้ตอบ
    /// แล้วคำถามกลายเป็นการแจ้งให้ทราบหลังลงมือทำ
    #[test]
    fn nothing_is_written_until_the_user_has_actually_said_yes() {
        let waiting = Folder {
            permit: None,
            locked: None,
            orphans: Vec::new(),
            dirty: true,
            had_file: false,
            granted: false,
            told: false,
        };
        assert!(
            !waiting.may_write(SidecarPolicy::Ask),
            "เขียนทั้งที่ยังไม่ได้ถาม — opt-in พัง"
        );
        assert!(!waiting.may_write(SidecarPolicy::Never));
        // ★ ประตูของประตู: ต้องอนุญาตได้จริงด้วย ไม่ใช่ห้ามทุกกรณี
        assert!(waiting.may_write(SidecarPolicy::Always), "Always ต้องเขียนได้");

        let answered = Folder {
            granted: true,
            ..waiting
        };
        assert!(
            answered.may_write(SidecarPolicy::Ask),
            "ตอบตกลงแล้วต้องเขียนได้"
        );

        // ★★ ไฟล์ที่มีอยู่แล้ว **คือคำตอบเดิมของผู้ใช้** — ห้ามถามซ้ำทุกครั้งที่เปิด
        let known_folder = Folder {
            had_file: true,
            granted: false,
            dirty: true,
            permit: None,
            locked: None,
            orphans: Vec::new(),
            told: false,
        };
        assert!(known_folder.may_write(SidecarPolicy::Ask));
        assert!(
            !known_folder.may_write(SidecarPolicy::Never),
            "Never ต้องชนะทุกอย่าง"
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

    /// ★★★ NC ของ `docs/02 §2.9`: **เปิดโฟลเดอร์ที่มีแท็กบันทึกไว้ ต้องไม่ใช่การแก้**
    ///
    /// สามอย่าง ไม่ใช่อย่างเดียว — แต่ละอย่างพังคนละแบบ:
    ///
    /// | ต้องเป็น | ถ้าไม่เป็น ผู้ใช้เจออะไร |
    /// |---|---|
    /// | `dirty == false` | ปิดโปรแกรมแล้วโดนถาม "บันทึกไหม" ทั้งที่ไม่ได้แตะอะไร |
    /// | ประวัติว่าง | ตัวนับ undo ขึ้นเองตั้งแต่เปิด |
    /// | `Ctrl+Z` ไม่แตะแท็ก | **กด undo ครั้งแรกแล้วแท็กที่เขาบันทึกไว้หายไป** |
    #[test]
    fn opening_a_folder_with_saved_tags_is_not_an_edit() {
        use refx_core::command::{EditMeta, MetaField};

        let mut board = Board::default();
        let mut h = history();
        let ids = put(
            &mut board,
            &mut h,
            vec![image("/photos/a.png", 1), image("/photos/b.png", 2)],
        );

        // ★★ ผู้ใช้ทำอะไรจริง ๆ ไว้หนึ่งอย่าง — **ขั้นที่ `Ctrl+Z` ควรย้อน**
        //    ถ้าไม่มีของจริงให้ย้อน เทสต์นี้พิสูจน์ไม่ได้ว่า undo ไปโดนอะไรผิด
        let mut pinned = board.item(ids[1]).unwrap().meta.clone();
        pinned.pinned = true;
        h.apply(
            &mut board,
            Box::new(EditMeta::new(MetaField::Pinned, vec![(ids[1], pinned)]).unwrap()),
        )
        .unwrap();
        h.mark_saved(&mut board);
        let depth_before = h.undo_depth();
        assert!(!board.is_dirty(), "จุดตั้งต้นต้องสะอาด ไม่งั้นเทสต์นี้พิสูจน์อะไรไม่ได้");

        let changed = board.restore_meta(vec![(
            ids[0],
            RestoredMeta {
                rating: 4,
                note: "จากไฟล์".to_owned(),
                tags: vec!["มังกร".to_owned()],
                ..RestoredMeta::default()
            },
        )]);

        // ★ ประตูของประตู: ถ้าไม่มีอะไรเปลี่ยนจริง สามข้อข้างล่างเป็นจริงฟรี ๆ
        assert_eq!(changed, 1, "ไม่ได้คืนอะไรเลย — ข้อที่เหลือจะผ่านโดยไม่ได้ตรวจ");
        assert_eq!(board.item(ids[0]).unwrap().meta.rating, 4);
        assert_eq!(board.item(ids[0]).unwrap().meta.tags.len(), 1);

        // (1) ไม่ dirty
        assert!(!board.is_dirty(), "เปิดโฟลเดอร์เฉย ๆ แล้วเอกสาร dirty");
        // (2) ไม่ขึ้นสแตก
        assert_eq!(h.undo_depth(), depth_before, "การโหลดขึ้นสแตก undo");

        // (3) ★★★ `Ctrl+Z` ต้องย้อน **สิ่งที่ผู้ใช้ทำ** ไม่ใช่สิ่งที่ไฟล์บอกมา
        h.undo(&mut board).unwrap();
        assert!(
            !board.item(ids[1]).unwrap().meta.pinned,
            "undo ไม่ได้ย้อนสิ่งที่ผู้ใช้ทำ — เทสต์นี้กำลังวัดของผิด"
        );
        assert_eq!(
            board.item(ids[0]).unwrap().meta.rating,
            4,
            "กด undo แล้วแท็กที่มาจากไฟล์หายไปด้วย = โปรแกรมกินงานผู้ใช้"
        );
        assert_eq!(board.item(ids[0]).unwrap().meta.tags.len(), 1);
    }

    /// ★★ แท็กที่ไม่มีไฟล์ไหนรับ **ต้องถูกนับ** — ของไม่ได้หาย แค่ไม่ติด
    ///
    /// `docs/07 §5` ข้อ 3: ห้ามลบรายการ และ **ห้ามเงียบ** · ผู้ใช้ที่เปลี่ยนชื่อ
    /// ไฟล์แล้วแก้เนื้อพร้อมกันต้องรู้ว่าของยังอยู่ ไม่ใช่เดาเอง
    #[test]
    fn tags_that_match_nothing_are_counted_not_hidden() {
        let stored = Sidecar::new(vec![
            entry_of("still-here.png", 1, 4),
            entry_of("on-a-usb-stick.png", 2, 5),
            entry_of("renamed-and-edited.png", 3, 3),
        ]);
        let present = [Present {
            file_name: "still-here.png".to_owned(),
            hash: hash_of(1),
        }];
        let matched = sidecar::resolve(&stored, &present);
        // ★ `Sidecar::new` เรียงตามชื่อ — ถามด้วย **ชื่อ** ไม่ใช่ด้วยตำแหน่ง
        //   (เทสต์ที่ผูกกับลำดับภายในจะแดงวันที่มีคนเพิ่มรายการ ไม่ใช่วันที่โค้ดผิด)
        let hit = matched[0].and_then(|at| stored.entries.get(at));
        assert_eq!(
            hit.map(|e| e.file_name.as_str()),
            Some("still-here.png"),
            "ใบที่ยังอยู่ต้องจับคู่ได้"
        );
        assert_eq!(
            sidecar::orphans(&stored, &matched).len(),
            2,
            "สองชุดที่ไม่มีไฟล์รับต้องถูกนับ ไม่ใช่หายไปเงียบ ๆ"
        );

        // ★ ประตูของประตู: ทุกใบจับคู่ได้ = ต้องนับเป็นศูนย์ ไม่ใช่นับเกินตลอด
        let all = [
            Present {
                file_name: "still-here.png".to_owned(),
                hash: hash_of(1),
            },
            Present {
                file_name: "on-a-usb-stick.png".to_owned(),
                hash: hash_of(2),
            },
            Present {
                file_name: "renamed-and-edited.png".to_owned(),
                hash: hash_of(3),
            },
        ];
        let matched_all = sidecar::resolve(&stored, &all);
        assert!(sidecar::orphans(&stored, &matched_all).is_empty());
    }
}
