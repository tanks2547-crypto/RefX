//! ★★★ หาไฟล์ภาพที่หายไป — ตรรกะล้วน ๆ ของ relink 5 ขั้น (P4-6)
//!
//! ## ทำไมตรรกะอยู่ที่นี่ แต่ IO อยู่ที่อื่น
//!
//! ขั้นตอนใน `docs/07 §2` เป็นเรื่องของ **ลำดับการตัดสินใจ** ล้วน ๆ — ส่วนที่
//! แตะดิสก์ (ไฟล์นี้มีอยู่ไหม · cache รู้จัก hash นี้ที่ไหนบ้าง · hash ของไฟล์นี้
//! คืออะไร) ถูกส่งเข้ามาเป็น closure ผู้เรียกจึงรันมันบน worker ได้ตาม I-2
//! และเทสต์รันได้โดยไม่ต้องมีดิสก์จริง
//!
//! ★ ทั้งสองฟังก์ชันในไฟล์นี้คือตัวที่ `refx-ui` เรียกจริง ไม่ใช่ตัวจำลอง
//! (`docs/08 §3.9` ข้อ 9)
//!
//! ## ห้าขั้นตาม `docs/07 §2`
//!
//! | ขั้น | ทำอะไร | อยู่ที่ไหน |
//! |---|---|---|
//! | 1 | ลองที่ path เดิม | [`locate`] |
//! | 2 | ลองในโฟลเดอร์เดียวกับ `.refx` | [`locate`] |
//! | 3 | ค้น `paths` ใน cache ด้วย hash | [`locate`] |
//! | 4 | ยังไม่เจอ → item เป็น `Missing` | [`locate`] คืน `None` |
//! | 5 | ผู้ใช้ชี้ไฟล์เดียว → จับคู่ที่เหลือในโฟลเดอร์นั้น | [`match_folder`] |
//!
//! ★★ เอกสาร **packed** มีสำเนาของภาพอยู่ในตัวมันเอง (P4-5) ผู้เรียกจึงแทรก
//! การแกะ blob ไว้ **ก่อนขั้นที่ 4** แล้วรายงานเป็น [`Step::Embedded`] —
//! ตรรกะนั้นอยู่ที่ผู้เรียกเพราะที่นี่ไม่รู้จักรูปแบบไฟล์ `.refx` เลยสักไบต์
//!
//! spec: docs/07-file-format.md §2, ROADMAP P4-6

use std::path::{Path, PathBuf};

use crate::arena::ItemId;
use crate::hash::ContentHash;

/// ★ เพดานจำนวนไฟล์ที่ยอมส่องในหนึ่งโฟลเดอร์ตอนขั้นที่ 5 (I-4/I-6)
///
/// ผู้ใช้ชี้ไปที่โฟลเดอร์ไหนก็ได้ รวมถึงโฟลเดอร์ที่มีไฟล์เป็นแสน · การ hash
/// ทุกไฟล์โดยไม่มีเพดานคือการเอาเธรดไปนอนอ่านดิสก์ไม่รู้จบ **โดยไม่มีอะไรบอก
/// ผู้ใช้ว่ามันกำลังทำอะไรอยู่** · เลือกให้กว้างกว่าเพดานของ board (3,072)
/// เล็กน้อย เพื่อไม่ให้มันเป็นตัวขวางก่อนเพดานจริง
pub const MAX_FOLDER_SCAN: usize = 4_096;

/// item ที่ต้องการให้ไปตามหาไฟล์ให้
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// item ไหนบน board
    pub id: ItemId,
    /// คีย์ของเนื้อที่เอกสารบันทึกไว้
    ///
    /// ★ ไฟล์ที่บันทึกก่อน 19 ส.ค. 2026 เก็บ hash ของ **path** ไว้ตรงนี้
    /// (`docs/07 §2`) การค้นด้วย hash จึงไม่เจอ — นั่นคือเหตุผลที่ขั้นที่ 2
    /// และการจับคู่ด้วย *ชื่อไฟล์* ใน [`match_folder`] ยังต้องมีอยู่
    pub hash: ContentHash,
    /// ที่อยู่ล่าสุดที่เอกสารจำไว้ (เป็นแค่ hint — `docs/02 §2.3`)
    pub path: PathBuf,
}

impl Wanted {
    /// ชื่อไฟล์ล้วน ๆ ของที่อยู่เดิม
    #[must_use]
    pub fn file_name(&self) -> Option<&std::ffi::OsStr> {
        self.path.file_name()
    }
}

/// เจอไฟล์ด้วยวิธีไหน — มีไว้ให้ log/ข้อความบอกผู้ใช้ได้ว่าเกิดอะไรขึ้น
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// ขั้น 1 — อยู่ที่เดิม (เคสปกติ ไม่ใช่การ relink ในสายตาผู้ใช้)
    WhereItWas,
    /// ขั้น 2 — อยู่ข้าง ๆ ไฟล์ `.refx` (เจอบ่อยที่สุดเวลาย้ายเครื่อง)
    NextToDocument,
    /// ขั้น 3 — cache เคยเห็น hash นี้ที่อื่นบนเครื่องนี้
    KnownByHash,
    /// ★★ ไม่มีไฟล์ไหนบนเครื่องนี้ แต่ **เอกสารพกสำเนามาเอง** (packed — P4-5)
    ///
    /// [`locate`] ไม่มีวันคืนค่านี้: มันไม่รู้จักรูปแบบไฟล์ `.refx` เลย ผู้เรียก
    /// เป็นคนแกะ blob ออกมาแล้วสร้าง [`Located`] ใบนี้เอง · แยกจาก
    /// [`Self::KnownByHash`] เพราะสิ่งที่เกิดขึ้นต่างกันคนละเรื่องในสายตาผู้ใช้:
    /// อันนั้นคือ "เจอไฟล์ของคุณที่อื่นบนเครื่อง" ส่วนอันนี้คือ
    /// **"ภาพอยู่ในไฟล์งานอยู่แล้ว ไม่ต้องหาอะไรทั้งนั้น"**
    Embedded,
    /// ขั้น 5 — ผู้ใช้ชี้เอง หรือจับคู่ตามไฟล์ที่ผู้ใช้ชี้
    PickedByUser,
}

/// ผลของการตามหาหนึ่งใบ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// item ไหน
    pub id: ItemId,
    /// ไฟล์ที่จะใช้
    pub path: PathBuf,
    /// เจอด้วยขั้นไหน
    pub step: Step,
}

/// ★★★ ขั้น 1–3: ตามหาไฟล์ของ item หนึ่งใบ — `None` = ขั้น 4 (`Missing`)
///
/// ลำดับสำคัญและ **ห้ามสลับ**:
///
/// 1. **ที่เดิมก่อนเสมอ** — ไฟล์ที่ยังอยู่ที่เดิมต้องไม่ถูกลากไปผูกกับสำเนาอื่น
///    บนเครื่อง แม้สำเนานั้นจะมีเนื้อเหมือนกันเป๊ะ (ผู้ใช้จัดโฟลเดอร์ไว้แบบนั้น
///    ด้วยเหตุผลของเขา และ path ที่เปลี่ยนเองคือสิ่งที่เขาไม่ได้สั่ง)
/// 2. **ข้าง ๆ เอกสาร** — เคสย้ายเครื่อง/ย้ายโฟลเดอร์ทั้งชุด ซึ่งเจอบ่อยที่สุด
///    ★ ขั้นนี้จับคู่ด้วย **ชื่อไฟล์** ไม่ใช่ hash โดยตั้งใจ (`docs/07 §2`) —
///    มันจึงเป็นขั้นเดียวที่ช่วยเอกสารรุ่นเก่าที่คีย์ยังเป็น hash ของ path ได้
/// 3. **cache รู้จัก hash นี้ไหม** — ตอบได้เฉพาะไฟล์ที่เครื่องนี้เคยเปิด
///
/// `exists` ต้องตอบ *"เป็นไฟล์ที่อ่านได้จริงไหม"* ไม่ใช่แค่ "มี entry อยู่"
pub fn locate(
    wanted: &Wanted,
    doc_dir: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
    known_by_hash: impl Fn(ContentHash) -> Vec<PathBuf>,
) -> Option<Located> {
    let found = |path: PathBuf, step: Step| {
        Some(Located {
            id: wanted.id,
            path,
            step,
        })
    };

    // ขั้น 1 — ที่เดิม
    if !wanted.path.as_os_str().is_empty() && exists(&wanted.path) {
        return found(wanted.path.clone(), Step::WhereItWas);
    }

    // ขั้น 2 — โฟลเดอร์เดียวกับเอกสาร
    if let (Some(dir), Some(name)) = (doc_dir, wanted.file_name()) {
        let beside = dir.join(name);
        // ★ เทียบกับที่เดิมก่อน — ถ้าเป็น path เดียวกันแปลว่าขั้น 1 ตอบไปแล้วว่าไม่มี
        if beside != wanted.path && exists(&beside) {
            return found(beside, Step::NextToDocument);
        }
    }

    // ขั้น 3 — cache เคยเห็น hash นี้ที่ไหนบ้าง
    for candidate in known_by_hash(wanted.hash) {
        if candidate != wanted.path && exists(&candidate) {
            return found(candidate, Step::KnownByHash);
        }
    }

    None // ขั้น 4
}

/// ★★★ ขั้น 5: ผู้ใช้ชี้ไฟล์มาหนึ่งไฟล์ → จับคู่ใบที่เหลือในโฟลเดอร์เดียวกัน
///
/// `candidates` คือไฟล์ในโฟลเดอร์นั้นพร้อม hash ของมัน (ผู้เรียก hash ให้ บน
/// worker · ดู [`MAX_FOLDER_SCAN`])
///
/// ## ลำดับการจับคู่
///
/// | ลำดับ | จับด้วย | ทำไม |
/// |---|---|---|
/// | 1 | **hash ของเนื้อ** | ตรงตาม `docs/07 §2` · ผิดตัวไม่ได้เลยโดยนิยาม |
/// | 2 | **ชื่อไฟล์ตรงเป๊ะ** | ทางเดียวที่เอกสารรุ่นเก่า (คีย์เป็น hash ของ path) จะกู้ได้ |
///
/// ★★ **ข้อ 2 ไม่ใช่การขยายกฎ** — ขั้นที่ 2 ของ `docs/07 §2` จับคู่ด้วยชื่อไฟล์
/// อยู่แล้ว (*"ลองในโฟลเดอร์เดียวกับ `.refx`"*) ที่นี่คือหลักการเดียวกันกับ
/// โฟลเดอร์ที่ **ผู้ใช้ชี้มาเอง** ซึ่งเป็นเจตนาที่ชัดกว่าอีก
///
/// ★ ความเสี่ยงที่ยอมรับ: โฟลเดอร์นั้นอาจมีไฟล์ชื่อเดียวกันแต่เป็นภาพคนละใบ
/// ผลคือผู้ใช้เห็นภาพผิดใบ **ซึ่งเห็นได้ทันทีบนจอและ undo ได้** ต่างจากการ
/// ไม่จับคู่ให้เลย ที่บังคับให้เขาชี้ทีละใบ 200 ครั้ง
///
/// ★ ไฟล์หนึ่งไฟล์ถูกผูกได้กับ item หลายใบ (ภาพเดียวกันวางหลายที่บน board)
/// แต่ item หนึ่งใบผูกได้กับไฟล์เดียว
#[must_use]
pub fn match_folder(wanted: &[Wanted], candidates: &[(PathBuf, ContentHash)]) -> Vec<Located> {
    let mut out = Vec::new();
    let mut matched = vec![false; wanted.len()];

    // รอบที่ 1 — hash
    for (index, want) in wanted.iter().enumerate() {
        if let Some((path, _)) = candidates.iter().find(|(_, hash)| *hash == want.hash) {
            matched[index] = true;
            out.push(Located {
                id: want.id,
                path: path.clone(),
                step: Step::PickedByUser,
            });
        }
    }

    // รอบที่ 2 — ชื่อไฟล์ (เฉพาะใบที่รอบแรกไม่ได้)
    for (index, want) in wanted.iter().enumerate() {
        if matched[index] {
            continue;
        }
        let Some(name) = want.file_name() else {
            continue;
        };
        if let Some((path, _)) = candidates
            .iter()
            .find(|(path, _)| path.file_name() == Some(name))
        {
            out.push(Located {
                id: want.id,
                path: path.clone(),
                step: Step::PickedByUser,
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::arena::ArenaKey as _;

    fn id(n: u32) -> ItemId {
        ItemId::from_parts(n, 0)
    }

    fn hash(n: u8) -> ContentHash {
        ContentHash::from_bytes([n; 32])
    }

    fn want(n: u32, h: u8, path: &str) -> Wanted {
        Wanted {
            id: id(n),
            hash: hash(h),
            path: PathBuf::from(path),
        }
    }

    /// ไม่มีไฟล์ไหนอยู่จริงเลย
    fn nothing_exists(_: &Path) -> bool {
        false
    }

    /// cache ไม่รู้จักอะไรเลย
    fn nothing_known(_: ContentHash) -> Vec<PathBuf> {
        Vec::new()
    }

    // ---------- ขั้น 1–4 ----------

    /// ★★★ **ไฟล์ที่ยังอยู่ที่เดิมต้องไม่ถูกลากไปผูกกับสำเนาอื่น**
    ///
    /// เทสต์นี้ล้มเป็น: สลับลำดับให้ขั้น 3 มาก่อนขั้น 1 แล้วมันจะเลือกสำเนา
    /// · ผลที่ผู้ใช้เจอคือ path ในเอกสารเปลี่ยนเองโดยเขาไม่ได้สั่ง
    #[test]
    fn a_file_that_never_moved_is_never_relinked_to_a_copy() {
        let w = want(1, 7, "/work/cat.png");
        let found = locate(
            &w,
            Some(Path::new("/work")),
            |_| true,
            |_| vec![PathBuf::from("/backup/cat.png")],
        )
        .unwrap();
        assert_eq!(found.path, PathBuf::from("/work/cat.png"));
        assert_eq!(found.step, Step::WhereItWas);
    }

    /// ★★ ขั้น 2 — ย้ายทั้งโฟลเดอร์ไปพร้อมเอกสาร (เคสที่เจอบ่อยที่สุด)
    #[test]
    fn an_image_that_travelled_with_the_document_is_found_beside_it() {
        let w = want(2, 8, "/old/machine/cat.png");
        let found = locate(
            &w,
            Some(Path::new("/new/place")),
            |p| p == Path::new("/new/place/cat.png"),
            nothing_known,
        )
        .unwrap();
        assert_eq!(found.path, PathBuf::from("/new/place/cat.png"));
        assert_eq!(found.step, Step::NextToDocument);
    }

    /// ★★★ ขั้น 2 จับคู่ด้วย **ชื่อไฟล์** — เอกสารรุ่นเก่าจึงยังกู้ได้
    ///
    /// คีย์ของเอกสารรุ่นเก่าเป็น hash ของ *path* ขั้นที่ 3 จึงไม่มีวันเจอ
    /// ขั้นนี้เป็นขั้นเดียวที่ยังทำงานให้เขาได้
    #[test]
    fn step_two_works_even_when_the_stored_key_is_meaningless() {
        let w = want(3, 200, "/gone/cat.png"); // hash ที่ไม่ตรงกับอะไรเลย
        let found = locate(
            &w,
            Some(Path::new("/here")),
            |p| p == Path::new("/here/cat.png"),
            nothing_known,
        )
        .unwrap();
        assert_eq!(found.step, Step::NextToDocument);
    }

    /// ขั้น 3 — cache เคยเห็น hash นี้ที่อื่นบนเครื่อง
    #[test]
    fn an_image_the_cache_has_seen_elsewhere_is_found_by_hash() {
        let w = want(4, 9, "/gone/cat.png");
        let found = locate(
            &w,
            None,
            |p| p == Path::new("/elsewhere/moved.png"),
            |h| {
                assert_eq!(h, hash(9));
                vec![PathBuf::from("/elsewhere/moved.png")]
            },
        )
        .unwrap();
        assert_eq!(found.path, PathBuf::from("/elsewhere/moved.png"));
        assert_eq!(found.step, Step::KnownByHash);
    }

    /// ★ cache ตอบ path ที่ **ถูกลบไปแล้ว** — ต้องข้ามไปตัวถัดไป ไม่ใช่เชื่อ
    ///
    /// `paths` เก็บสิ่งที่เคยเห็น ไม่ใช่สิ่งที่ยังอยู่ · ถ้าเชื่อโดยไม่ตรวจ
    /// item จะถูกผูกกับไฟล์ที่เปิดไม่ได้ แล้วผู้ใช้เห็น error แทน placeholder
    #[test]
    fn a_stale_row_in_the_cache_never_wins_over_a_file_that_exists() {
        let w = want(5, 10, "/gone/cat.png");
        let found = locate(
            &w,
            None,
            |p| p == Path::new("/real/cat.png"),
            |_| {
                vec![
                    PathBuf::from("/deleted/cat.png"),
                    PathBuf::from("/real/cat.png"),
                ]
            },
        )
        .unwrap();
        assert_eq!(found.path, PathBuf::from("/real/cat.png"));
    }

    /// ขั้น 4 — หมดทุกทางแล้วยังไม่เจอ
    #[test]
    fn an_image_that_is_really_gone_becomes_missing() {
        let w = want(6, 11, "/gone/cat.png");
        assert_eq!(
            locate(
                &w,
                Some(Path::new("/nowhere")),
                nothing_exists,
                nothing_known
            ),
            None
        );
    }

    /// item ที่ไม่มี path เลย (ภาพที่วางแล้ว spool เขียนไม่สำเร็จ) ต้องไม่ระเบิด
    #[test]
    fn an_item_with_no_path_at_all_is_simply_missing() {
        let w = want(7, 12, "");
        assert_eq!(
            locate(&w, Some(Path::new("/x")), |_| true, nothing_known),
            None
        );
    }

    // ---------- ขั้น 5 ----------

    /// ★★★ ชี้ไฟล์เดียว → ที่เหลือในโฟลเดอร์จับคู่เองด้วย **hash**
    #[test]
    fn pointing_at_one_file_matches_the_rest_by_hash() {
        let wanted = vec![
            want(1, 1, "/gone/a.png"),
            want(2, 2, "/gone/b.png"),
            want(3, 3, "/gone/c.png"),
        ];
        // ★ ชื่อไฟล์ในโฟลเดอร์ใหม่ **เปลี่ยนไปหมดแล้ว** — จับได้ด้วย hash เท่านั้น
        let candidates = vec![
            (PathBuf::from("/new/renamed-1.png"), hash(1)),
            (PathBuf::from("/new/renamed-2.png"), hash(2)),
            (PathBuf::from("/new/renamed-3.png"), hash(3)),
        ];

        let out = match_folder(&wanted, &candidates);

        assert_eq!(out.len(), 3);
        assert_eq!(out[0].path, PathBuf::from("/new/renamed-1.png"));
        assert_eq!(out[2].path, PathBuf::from("/new/renamed-3.png"));
        assert!(out.iter().all(|l| l.step == Step::PickedByUser));
    }

    /// ★★ hash มาก่อนชื่อเสมอ — ชื่อที่ตรงแต่เนื้อคนละใบต้องแพ้
    #[test]
    fn the_bytes_win_over_the_name() {
        let wanted = vec![want(1, 1, "/gone/cat.png")];
        let candidates = vec![
            (PathBuf::from("/new/cat.png"), hash(99)), // ชื่อตรง เนื้อคนละใบ
            (PathBuf::from("/new/whatever.png"), hash(1)), // เนื้อตรง
        ];

        let out = match_folder(&wanted, &candidates);

        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].path,
            PathBuf::from("/new/whatever.png"),
            "จับคู่ด้วยชื่อทั้งที่ hash ตรงอยู่ตัวหนึ่ง"
        );
    }

    /// ★★★ เอกสารรุ่นเก่า (คีย์เป็น hash ของ path) — **ชื่อไฟล์คือทางเดียวที่เหลือ**
    ///
    /// ถ้าไม่มีรอบที่สอง ผู้ใช้ที่มี board 200 ใบต้องกด "หาไฟล์เอง" 200 ครั้ง
    #[test]
    fn an_old_document_still_matches_by_name() {
        let wanted = vec![want(1, 250, "/gone/a.png"), want(2, 251, "/gone/b.png")];
        // hash ในเอกสารเป็นของ path จึงไม่ตรงกับเนื้อของไฟล์ไหนเลย
        let candidates = vec![
            (PathBuf::from("/new/a.png"), hash(1)),
            (PathBuf::from("/new/b.png"), hash(2)),
        ];

        let out = match_folder(&wanted, &candidates);

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].path, PathBuf::from("/new/a.png"));
        assert_eq!(out[1].path, PathBuf::from("/new/b.png"));
    }

    /// ใบที่จับคู่ไม่ได้เลยต้องไม่ถูกยัดให้มั่ว — ไม่มีในผลลัพธ์ = ยังเป็น `Missing`
    #[test]
    fn an_image_that_is_not_in_that_folder_stays_missing() {
        let wanted = vec![want(1, 1, "/gone/a.png"), want(2, 2, "/gone/b.png")];
        let candidates = vec![(PathBuf::from("/new/a.png"), hash(1))];

        let out = match_folder(&wanted, &candidates);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, id(1));
    }

    /// ★ ภาพเดียวกันวางหลายใบบน board → ทุกใบต้องได้ไฟล์เดียวกัน
    #[test]
    fn every_copy_of_one_picture_is_relinked_together() {
        let wanted = vec![want(1, 5, "/gone/a.png"), want(2, 5, "/gone/a.png")];
        let candidates = vec![(PathBuf::from("/new/a.png"), hash(5))];

        let out = match_folder(&wanted, &candidates);

        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|l| l.path == Path::new("/new/a.png")));
    }

    /// โฟลเดอร์ว่าง = ไม่จับคู่อะไรเลย ไม่ใช่ error
    #[test]
    fn an_empty_folder_matches_nothing() {
        assert!(match_folder(&[want(1, 1, "/gone/a.png")], &[]).is_empty());
    }
}
