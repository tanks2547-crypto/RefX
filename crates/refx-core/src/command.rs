//! `Command` + `History` — **ประตูเดียวที่แก้ `Board` ได้** (I-3)
//!
//! ทั้ง invariant "ข้อมูลผู้ใช้ห้ามหาย" พิงอยู่บนไฟล์นี้ทั้งข้อ ถ้าโครงตรงนี้ผิด
//! ทุกอย่างที่สร้างทับมันจะผิดตาม จึงมีกฎสามข้อที่ห้ามยืดหยุ่น:
//!
//! 1. **`apply` ที่คืน `Err` ต้องไม่แตะ board เลย** — ไม่ใช่ "แตะไปครึ่งหนึ่งแล้วบอกว่าพัง"
//!    board ที่ค้างครึ่ง ๆ กลาง ๆ คือข้อมูลผู้ใช้ที่เพี้ยนโดยไม่มีทางกู้ (I-3)
//!    คำสั่งที่แตะหลาย item จึงต้อง **ย้อนสิ่งที่ทำไปแล้วคืน** ก่อนคืน `Err`
//! 2. **`undo` ต้องคืนสภาพ *เป๊ะ*** ไม่ใช่ "ดูเหมือนเดิม" — รวมถึง `ItemId` ตัวเดิม
//!    และชั้น z เดิม (`selection` **ไม่อยู่ใน `Board`** แล้ว — docs/02 §2.9)
//! 3. **redo ต้องได้ `ItemId` ชุดเดิม** ไม่งั้นคำสั่งถัดไปใน redo stack ที่อ้าง id
//!    เหล่านั้นจะชี้ไปที่ว่าง (นี่คือเหตุผลที่ `Arena::insert_at` มีอยู่)
//!
//! ★ **ต่างจาก docs/02 §3 หนึ่งจุด:** ที่นั่นเขียน `fn undo(&mut self, board: &mut Board);`
//! แบบล้มไม่ได้ ที่นี่คืน `Result` ตาม CLAUDE.md ("ทุกอย่างที่ล้มเหลวได้ → `Result`")
//! ในระบบที่ถูกต้อง undo ไม่มีทางล้ม แต่ "ไม่มีทางล้ม" คือสมมติฐานที่พังเงียบที่สุด
//! ถ้ามันล้มจริงเราต้องรู้ ไม่ใช่ปล่อยให้ board เพี้ยนไปเฉย ๆ
//!
//! spec: docs/02-data-model.md §3, ROADMAP P2-2

use std::any::Any;
use std::collections::VecDeque;

use crate::arena::ItemId;
use crate::board::{Board, BoardError, Item, ItemCanvas, ItemMeta};

/// คำสั่งทำงานไม่สำเร็จ — **board ไม่ถูกแตะเลยเมื่อได้ค่านี้**
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CmdError {
    /// board ปฏิเสธ (id ตายไปแล้ว / ช่องมีคนอยู่)
    #[error(transparent)]
    Board(#[from] BoardError),

    /// ลำดับ z ที่ส่งมาไม่ใช่การสลับที่ของ item ชุดเดิม
    ///
    /// ยอมรับไม่ได้เพราะจะทำให้ภาพหายจากจอทั้งที่ยังอยู่ใน board
    #[error("the new z-order is not a permutation of the current items")]
    ZOrderMismatch,

    /// สั่งงานที่ไม่มีอะไรให้ทำ — กันไม่ให้ undo stack เต็มไปด้วยรายการว่าง
    #[error("nothing to do")]
    Empty,
}

/// การแก้ `Board` หนึ่งครั้งที่ย้อนกลับได้
///
/// `Any` เป็น supertrait เพราะ [`Command::merge`] ต้องรู้ว่าคำสั่งถัดไปเป็นชนิดเดียวกัน
/// หรือไม่ (ลากเมาส์ต่อเนื่อง = `TransformItems` ต่อ `TransformItems`)
pub trait Command: Send + std::fmt::Debug + Any {
    /// ลงมือแก้ board
    ///
    /// เรียกซ้ำได้ (redo) และ **ต้องให้ผลเหมือนครั้งแรกทุกประการ** รวมถึง `ItemId`
    ///
    /// # Errors
    /// คืน [`CmdError`] เมื่อทำไม่ได้ — และเมื่อคืน `Err` **board ต้องเหมือนก่อนเรียกเป๊ะ**
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError>;

    /// ย้อนสิ่งที่ [`Command::apply`] ทำไว้
    ///
    /// # Errors
    /// คืน [`CmdError`] เมื่อย้อนไม่ได้ — ในระบบที่ถูกต้องไม่ควรเกิด
    /// ถ้าเกิดคือสัญญาณว่ามีใครแก้ board นอกเส้นทาง `Command` (ผิด I-3)
    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError>;

    /// รวมคำสั่งถัดไปเข้ากับตัวเอง — `true` = รวมแล้ว ผู้เรียกทิ้ง `next` ได้
    ///
    /// ★ ใช้กับการลากเมาส์ค้าง: ถ้าไม่รวม การลากหนึ่งครั้งจะกลายเป็น undo หลายร้อยขั้น
    /// แต่ต้อง **ปิดหน้าต่างทันทีที่ปล่อยเมาส์** ([`History::seal`]) ไม่งั้นการลาก
    /// สองครั้งติดกันจะกลายเป็น undo เดียว ซึ่งผู้ใช้จะงง (docs/02 §3)
    ///
    /// ค่าเริ่มต้นคือ "ไม่รวม" — คำสั่งที่ไม่ได้ตั้งใจให้ merge จึงปลอดภัยโดยอัตโนมัติ
    fn merge(&mut self, _next: &dyn Command) -> bool {
        false
    }

    /// item ที่คำสั่งนี้แตะ — ชั้น editor เอาไปตั้ง selection หลัง undo/redo
    ///
    /// ★ **การเลือกไม่ใช่สิ่งที่ถูก undo มันแค่ตามผลลัพธ์** (docs/02 §2.9):
    /// `selection` ไม่อยู่ใน `Board` แล้ว แต่ผู้ใช้ที่กด Ctrl+Z หลังลบภาพ 5 ใบ
    /// ยังต้องได้ภาพนั้นกลับมา **พร้อมถูกเลือกอยู่** คำสั่งจึงบอกได้ว่าตัวเองแตะอะไร
    ///
    /// §2.9 เขียนว่าให้ `undo` เป็นคนคืนค่านี้ — ทำเป็นเมธอดแยกแทนเพราะ **redo
    /// ต้องการค่าเดียวกัน** และ redo เดินผ่าน `apply` ถ้าผูกไว้กับ `undo` อย่างเดียว
    /// การ redo จะไม่มีทางตั้ง selection ได้ กลไกเดียวใช้ได้ทั้งสองทาง
    fn affected(&self) -> Vec<ItemId>;

    /// ชื่อที่แสดงในเมนู undo (อังกฤษ — `refx-ui` แปลจากค่านี้)
    fn label(&self) -> &'static str;

    /// หน่วยความจำที่คำสั่งนี้ถือไว้โดยประมาณ (ไบต์)
    ///
    /// ★ **ไม่มีค่าเริ่มต้นโดยตั้งใจ** (I-6): ถ้าให้ default เป็น 0 คำสั่งที่เขียนใหม่
    /// จะไม่ถูกนับเงียบ ๆ แล้วเพดาน 64 MB ของ [`History`] จะไม่มีวันทำงาน —
    /// เพดานที่ไม่เคยทำงานคือเพดานที่ไม่มีอยู่ (docs/08 §3.9)
    fn heap_size(&self) -> usize;

    /// สำหรับ [`Command::merge`] ตรวจชนิด
    fn as_any(&self) -> &dyn Any;
}

/// ขนาดโดยประมาณของ `Item` หนึ่งใบที่คำสั่งถือไว้
///
/// ประมาณพอให้เพดานทำงาน ไม่ต้องเป๊ะ — สิ่งที่กินจริงคือ `PathBuf` กับ `String`
#[must_use]
fn item_heap_size(item: &Item) -> usize {
    use crate::board::ItemKind;
    let kind = match &item.kind {
        ItemKind::Image(asset) => asset.path.as_os_str().len(),
        ItemKind::Text(note) => note.text.len(),
        ItemKind::Missing { original_path, .. } => original_path.as_os_str().len(),
    };
    std::mem::size_of::<Item>() + kind + item.meta.note.len() + item.meta.tags.len() * 4
}

// ---------------------------------------------------------------------------
// AddItems
// ---------------------------------------------------------------------------

/// เพิ่ม item เข้า board (ลากไฟล์เข้ามา / วางจาก clipboard)
///
/// ★ **redo ต้องได้ `ItemId` ชุดเดิม** — ถ้าแจก id ใหม่ คำสั่งถัดไปใน redo stack
/// ที่อ้าง id เก่าจะชี้ไปที่ว่าง แล้ว redo ทั้งสายจะพังแบบเงียบ ๆ
#[derive(Debug)]
pub struct AddItems {
    /// item ที่ยังไม่ได้อยู่บน board (ตอนสร้าง และหลัง undo)
    pending: Vec<Item>,
    /// id + ชั้น z ที่ได้ตอน apply ครั้งแรก — เรียงจากชั้นล่างขึ้นบน
    placed: Vec<(ItemId, usize)>,
}

impl AddItems {
    /// เพิ่ม item ตามลำดับที่ให้มา (ตัวแรกอยู่ล่างสุด)
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง — ไม่ยอมให้ undo stack มีรายการที่ไม่ทำอะไร
    pub fn new(items: Vec<Item>) -> Result<Self, CmdError> {
        if items.is_empty() {
            return Err(CmdError::Empty);
        }
        Ok(Self {
            pending: items,
            placed: Vec::new(),
        })
    }

    /// id ของ item ที่เพิ่มเข้าไป — ใช้ได้หลัง `apply` แล้วเท่านั้น
    #[must_use]
    pub fn ids(&self) -> Vec<ItemId> {
        self.placed.iter().map(|&(id, _)| id).collect()
    }
}

impl Command for AddItems {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let items = std::mem::take(&mut self.pending);
        if items.is_empty() {
            return Err(CmdError::Empty); // apply ซ้อนโดยไม่ undo ก่อน
        }

        if self.placed.is_empty() {
            // ครั้งแรก — ให้ board แจก id
            for item in items {
                let id = board.insert_item(item);
                let z = board.z_order().len() - 1;
                self.placed.push((id, z));
            }
            return Ok(());
        }

        // redo — ต้องคืนที่เดิมเป๊ะ เรียงจากชั้นล่างขึ้นบนเพื่อให้ index ถูกต้อง
        let mut restored: Vec<ItemId> = Vec::with_capacity(items.len());
        for (item, &(id, z)) in items.iter().zip(&self.placed) {
            if let Err(err) = board.restore_item(id, item.clone(), z) {
                // ★ ล้มกลางคัน — เก็บกวาดให้หมดก่อนคืน Err (กฎข้อ 1)
                for id in restored.iter().rev() {
                    board.remove_item(*id);
                }
                self.pending = items;
                return Err(err.into());
            }
            restored.push(id);
        }
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        // ถอดจากชั้นบนลงล่าง แล้วเรียงกลับให้ตรงลำดับเดิมตอนเก็บ
        let mut taken = Vec::with_capacity(self.placed.len());
        for &(id, _) in self.placed.iter().rev() {
            let Some((item, _)) = board.remove_item(id) else {
                // ใส่ที่ถอดไปแล้วกลับ ก่อนรายงานว่าย้อนไม่ได้
                for (item, &(id, z)) in taken.iter().rev().zip(self.placed.iter().rev()) {
                    let _ = board.restore_item(id, Item::clone(item), z);
                }
                return Err(CmdError::Board(BoardError::NoSuchItem { id }));
            };
            taken.push(item);
        }
        taken.reverse();
        self.pending = taken;
        Ok(())
    }

    fn affected(&self) -> Vec<ItemId> {
        self.ids()
    }

    fn label(&self) -> &'static str {
        "Add items"
    }

    fn heap_size(&self) -> usize {
        self.pending.iter().map(item_heap_size).sum::<usize>()
            + self.placed.len() * std::mem::size_of::<(ItemId, usize)>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// RemoveItems
// ---------------------------------------------------------------------------

/// ลบ item ออกจาก board — เก็บตัวที่ลบไว้ในคำสั่งเพื่อ undo (docs/02 §3)
///
/// ★ การเลือกไม่ได้ถูกเก็บไว้ที่นี่ — `selection` ไม่อยู่ใน `Board` แล้ว (docs/02 §2.9)
/// แต่ [`Command::affected`] บอกได้ว่าลบอะไรไป ชั้น editor จึงเลือกของที่กลับมาให้เองได้
#[derive(Debug)]
pub struct RemoveItems {
    /// id ที่สั่งลบ
    targets: Vec<ItemId>,
    /// ของที่ถอดออกมา (id, item, ชั้น z) เรียงจากชั้นล่างขึ้นบน
    removed: Vec<(ItemId, Item, usize)>,
}

impl RemoveItems {
    /// ลบตาม id ที่ให้มา
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง
    pub fn new(targets: Vec<ItemId>) -> Result<Self, CmdError> {
        if targets.is_empty() {
            return Err(CmdError::Empty);
        }
        Ok(Self {
            targets,
            removed: Vec::new(),
        })
    }
}

impl Command for RemoveItems {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let mut taken: Vec<(ItemId, Item, usize)> = Vec::with_capacity(self.targets.len());
        for &id in &self.targets {
            let Some((item, z)) = board.remove_item(id) else {
                // ★ ล้มกลางคัน — ใส่ทุกอย่างกลับก่อนคืน Err (กฎข้อ 1)
                //   เรียงจากชั้นล่างขึ้นบนเพื่อให้ index เดิมยังถูกต้อง
                taken.sort_by_key(|&(_, _, z)| z);
                for (id, item, z) in taken {
                    let _ = board.restore_item(id, item, z);
                }
                return Err(CmdError::Board(BoardError::NoSuchItem { id }));
            };
            taken.push((id, item, z));
        }

        taken.sort_by_key(|&(_, _, z)| z);
        self.removed = taken;
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let removed = std::mem::take(&mut self.removed);
        let mut restored = Vec::with_capacity(removed.len());

        for (id, item, z) in &removed {
            if let Err(err) = board.restore_item(*id, item.clone(), *z) {
                for id in restored.iter().rev() {
                    board.remove_item(*id);
                }
                self.removed = removed;
                return Err(err.into());
            }
            restored.push(*id);
        }
        Ok(())
    }

    /// ★ id ที่ถูกลบ — ชั้น editor เอาไปตั้ง selection หลัง undo
    /// ผู้ใช้ที่ลบ 5 ภาพแล้วกด Ctrl+Z ต้องได้ 5 ภาพนั้นกลับมา **พร้อมถูกเลือกอยู่**
    fn affected(&self) -> Vec<ItemId> {
        self.targets.clone()
    }

    fn label(&self) -> &'static str {
        "Remove items"
    }

    fn heap_size(&self) -> usize {
        self.removed
            .iter()
            .map(|(_, item, _)| item_heap_size(item))
            .sum::<usize>()
            + self.targets.len() * std::mem::size_of::<ItemId>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// TransformItems
// ---------------------------------------------------------------------------

/// การเปลี่ยนค่าหนึ่งช่องพร้อมค่าเดิมสำหรับ undo
#[derive(Debug, Clone, PartialEq)]
struct Change<T> {
    id: ItemId,
    /// ค่าก่อนหน้า — เติมตอน `apply` ครั้งแรก
    before: Option<T>,
    after: T,
}

/// ย้าย/สเกล/หมุน item — **คำสั่งที่ merge ได้** (ลากเมาส์ค้าง)
#[derive(Debug)]
pub struct TransformItems {
    changes: Vec<Change<ItemCanvas>>,
}

impl TransformItems {
    /// ตั้งค่า `ItemCanvas` ใหม่ให้ item ตามที่ระบุ
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง
    pub fn new(changes: Vec<(ItemId, ItemCanvas)>) -> Result<Self, CmdError> {
        if changes.is_empty() {
            return Err(CmdError::Empty);
        }
        Ok(Self {
            changes: changes
                .into_iter()
                .map(|(id, after)| Change {
                    id,
                    before: None,
                    after,
                })
                .collect(),
        })
    }

    /// แตะ item ชุดเดียวกันตามลำดับเดียวกันหรือไม่ — เงื่อนไขของการ merge
    fn same_targets(&self, other: &Self) -> bool {
        self.changes.len() == other.changes.len()
            && self
                .changes
                .iter()
                .zip(&other.changes)
                .all(|(a, b)| a.id == b.id)
    }
}

impl Command for TransformItems {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let mut done: Vec<(ItemId, ItemCanvas)> = Vec::with_capacity(self.changes.len());
        for change in &mut self.changes {
            match board.set_canvas(change.id, change.after) {
                Ok(previous) => {
                    done.push((change.id, previous));
                    // เก็บค่าเดิมไว้ครั้งเดียว — redo ต้องย้อนกลับไปจุดเดียวกับ undo แรก
                    change.before.get_or_insert(previous);
                }
                Err(err) => {
                    for (id, previous) in done.into_iter().rev() {
                        let _ = board.set_canvas(id, previous);
                    }
                    return Err(err.into());
                }
            }
        }
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let mut done: Vec<(ItemId, ItemCanvas)> = Vec::with_capacity(self.changes.len());
        for change in &self.changes {
            let Some(before) = change.before else {
                continue; // ยังไม่เคย apply — ไม่มีอะไรให้ย้อน
            };
            match board.set_canvas(change.id, before) {
                Ok(previous) => done.push((change.id, previous)),
                Err(err) => {
                    for (id, previous) in done.into_iter().rev() {
                        let _ = board.set_canvas(id, previous);
                    }
                    return Err(err.into());
                }
            }
        }
        Ok(())
    }

    /// รวมการลากต่อเนื่อง: เก็บ **จุดเริ่ม** ของตัวเก่าไว้ แล้วรับ **จุดจบ** ของตัวใหม่
    fn merge(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        // ลากคนละชุด = คนละการกระทำ ต้องแยก undo (ไม่งั้นย้อนทีเดียวไปโดนภาพอื่นด้วย)
        if !self.same_targets(next) {
            return false;
        }
        for (mine, theirs) in self.changes.iter_mut().zip(&next.changes) {
            mine.after = theirs.after;
        }
        true
    }

    fn affected(&self) -> Vec<ItemId> {
        self.changes.iter().map(|change| change.id).collect()
    }

    fn label(&self) -> &'static str {
        "Transform items"
    }

    fn heap_size(&self) -> usize {
        self.changes.len() * std::mem::size_of::<Change<ItemCanvas>>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// SetCrop
// ---------------------------------------------------------------------------

/// ครอปภาพแบบไม่ทำลายต้นฉบับ (P2-7) — **merge ได้** เหมือนการลากอื่น ๆ
///
/// ★ **ทำไมไม่ใช้ `TransformItems` ไปเลย ทั้งที่แก้ `ItemCanvas` เหมือนกัน:**
///
/// 1. `docs/02 §3` กำหนดให้ `SetCrop` เป็นคำสั่งของตัวเอง
/// 2. **ชื่อในเมนู undo ต้องบอกว่าผู้ใช้ทำอะไร** — "Crop" ไม่ใช่ "Transform items"
/// 3. ★ **merge ต้องแยกตามชนิด** — ครอปเสร็จแล้วลากย้ายต่อโดยไม่ปล่อยเมาส์
///    (เปลี่ยนเครื่องมือกลางคัน) ต้องเป็นคนละขั้น ถ้าใช้คำสั่งเดียวกันมันจะกลืนกัน
///    แล้วกด Ctrl+Z ทีเดียวจะย้อนทั้งการครอปและการย้าย ซึ่งผู้ใช้ไม่ได้ขอ
///
/// ตัวเนื้อในยืม [`TransformItems`] ทั้งดุ้น เพราะการครอปคือการเขียน `ItemCanvas`
/// ชุดใหม่จริง ๆ (`crop` + `pos` + `size` เปลี่ยนพร้อมกันเสมอ — ดู `refx-core::interact`)
/// การก๊อปตรรกะ apply/undo มาไว้สองที่มีแต่จะทำให้มันเพี้ยนจากกันวันหนึ่ง
#[derive(Debug)]
pub struct SetCrop {
    inner: TransformItems,
}

impl SetCrop {
    /// ตั้ง `ItemCanvas` ชุดใหม่ที่มีทั้งกรอบ crop และเรขาคณิตที่หดตามแล้ว
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง
    pub fn new(changes: Vec<(ItemId, ItemCanvas)>) -> Result<Self, CmdError> {
        Ok(Self {
            inner: TransformItems::new(changes)?,
        })
    }
}

impl Command for SetCrop {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        self.inner.apply(board)
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        self.inner.undo(board)
    }

    /// รวมได้เฉพาะกับ `SetCrop` ด้วยกัน — ดูเหตุผลข้อ 3 ที่หัวโครงสร้าง
    fn merge(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        self.inner.merge(&next.inner)
    }

    fn affected(&self) -> Vec<ItemId> {
        self.inner.affected()
    }

    fn label(&self) -> &'static str {
        "Crop"
    }

    fn heap_size(&self) -> usize {
        self.inner.heap_size()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// SetFilter
// ---------------------------------------------------------------------------

/// เปลี่ยนลักษณะการแสดงผลของภาพ — opacity / flip / filter (P2-8)
///
/// ★ **แยกจาก `TransformItems` ด้วยเหตุผลเดียวกับ [`SetCrop`]**: ชื่อในเมนู undo
/// ต้องบอกว่าผู้ใช้ทำอะไร และการลากสไลเดอร์ opacity ต้องไม่ merge เข้ากับการลากย้าย
/// ที่เพิ่งทำไปก่อนหน้า ถ้าใช้คำสั่งเดียวกันมันจะกลืนกันแล้วย้อนทีเดียวเสียทั้งสองอย่าง
///
/// `docs/02 §3` ไม่ได้ระบุคำสั่งชื่อนี้ไว้ (ตารางมีแค่ `SetCrop`/`EditMeta`) —
/// ตัดสินใจเพิ่มเพราะ `ItemFilter`/`opacity`/`flip` อยู่ใน `ItemCanvas` ซึ่งเป็น
/// เอกสาร จึงต้องผ่าน `Command` ตาม `docs/08 §4` ข้อ 10 และไม่มีคำสั่งไหนที่เหมาะกว่า
#[derive(Debug)]
pub struct SetFilter {
    inner: TransformItems,
}

impl SetFilter {
    /// ตั้ง `ItemCanvas` ชุดใหม่ที่ต่างกันเฉพาะช่องการแสดงผล
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง
    pub fn new(changes: Vec<(ItemId, ItemCanvas)>) -> Result<Self, CmdError> {
        Ok(Self {
            inner: TransformItems::new(changes)?,
        })
    }
}

impl Command for SetFilter {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        self.inner.apply(board)
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        self.inner.undo(board)
    }

    /// รวมได้เฉพาะกับ `SetFilter` ด้วยกัน — ลากสไลเดอร์รัว ๆ = undo ขั้นเดียว
    fn merge(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        self.inner.merge(&next.inner)
    }

    fn affected(&self) -> Vec<ItemId> {
        self.inner.affected()
    }

    fn label(&self) -> &'static str {
        "Filter"
    }

    fn heap_size(&self) -> usize {
        self.inner.heap_size()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// ReorderZ
// ---------------------------------------------------------------------------

/// เปลี่ยนลำดับชั้น (ส่งไปหน้าสุด/หลังสุด)
///
/// เก็บ `Vec<ItemId>` เดิม **ทั้งชุด** ตามที่ docs/02 §3 กำหนด — ถูกกว่า diff
/// และไม่มีบั๊กที่ diff มักมี (ลำดับที่เพี้ยนทีละหนึ่งแล้วสะสม)
#[derive(Debug)]
pub struct ReorderZ {
    order: Vec<ItemId>,
    previous: Option<Vec<ItemId>>,
}

impl ReorderZ {
    /// ตั้งลำดับใหม่ทั้งชุด (ล่างสุด → บนสุด)
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง
    pub fn new(order: Vec<ItemId>) -> Result<Self, CmdError> {
        if order.is_empty() {
            return Err(CmdError::Empty);
        }
        Ok(Self {
            order,
            previous: None,
        })
    }
}

impl Command for ReorderZ {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        // ★ ตรวจ **ก่อน** เขียน: ลำดับที่ไม่ครบจะทำให้ภาพหายจากจอทั้งที่ยังอยู่ใน board
        //   ตรวจทีหลังแล้วค่อยย้อนก็ได้ แต่ตรวจก่อนทำให้ "ไม่แตะเลย" เป็นจริงโดยโครงสร้าง
        if self.order.len() != board.len() {
            return Err(CmdError::ZOrderMismatch);
        }
        let mut seen = std::collections::HashSet::with_capacity(self.order.len());
        if !self
            .order
            .iter()
            .all(|&id| board.item(id).is_some() && seen.insert(id))
        {
            return Err(CmdError::ZOrderMismatch);
        }

        let previous = board.set_z_order(self.order.clone());
        self.previous.get_or_insert(previous);
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        if let Some(previous) = self.previous.clone() {
            board.set_z_order(previous);
        }
        Ok(())
    }

    fn affected(&self) -> Vec<ItemId> {
        // การเรียงชั้นไม่ได้ "แตะ" ตัวไหนเป็นพิเศษ — ปล่อยให้ selection เดิมอยู่ต่อ
        Vec::new()
    }

    fn label(&self) -> &'static str {
        "Reorder"
    }

    fn heap_size(&self) -> usize {
        (self.order.len() + self.previous.as_ref().map_or(0, Vec::len))
            * std::mem::size_of::<ItemId>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// EditMeta
// ---------------------------------------------------------------------------

/// ช่องของ `ItemMeta` ที่คำสั่งกำลังแก้ — ใช้ตัดสินว่า merge ได้ไหม
///
/// docs/02 §3 กำหนดว่า `EditMeta` merge ได้ **ต่อ field**: ลากแถบดาวรัว ๆ ควรเป็น
/// undo เดียว แต่ "ให้ดาว" แล้ว "ใส่แท็ก" ต้องเป็นคนละขั้น ไม่งั้นย้อนทีเดียว
/// จะเสียทั้งสองอย่างทั้งที่ผู้ใช้ตั้งใจย้อนแค่อันหลัง
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaField {
    /// แท็ก
    Tags,
    /// ดาว
    Rating,
    /// ป้ายสี
    ColorLabel,
    /// โน้ต
    Note,
    /// กลุ่ม
    Group,
    /// ปักหมุด
    Pinned,
}

/// แก้ข้อมูลฝั่ง Arrange
#[derive(Debug)]
pub struct EditMeta {
    field: MetaField,
    changes: Vec<Change<ItemMeta>>,
}

impl EditMeta {
    /// ตั้งค่า `ItemMeta` ใหม่ พร้อมบอกว่ากำลังแก้ช่องไหน (ใช้ตัดสินการ merge)
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง
    pub fn new(field: MetaField, changes: Vec<(ItemId, ItemMeta)>) -> Result<Self, CmdError> {
        if changes.is_empty() {
            return Err(CmdError::Empty);
        }
        Ok(Self {
            field,
            changes: changes
                .into_iter()
                .map(|(id, after)| Change {
                    id,
                    before: None,
                    after,
                })
                .collect(),
        })
    }
}

impl Command for EditMeta {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let mut done: Vec<(ItemId, ItemMeta)> = Vec::with_capacity(self.changes.len());
        for change in &mut self.changes {
            match board.set_meta(change.id, change.after.clone()) {
                Ok(previous) => {
                    if change.before.is_none() {
                        change.before = Some(previous.clone());
                    }
                    done.push((change.id, previous));
                }
                Err(err) => {
                    for (id, previous) in done.into_iter().rev() {
                        let _ = board.set_meta(id, previous);
                    }
                    return Err(err.into());
                }
            }
        }
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let mut done: Vec<(ItemId, ItemMeta)> = Vec::with_capacity(self.changes.len());
        for change in &self.changes {
            let Some(before) = change.before.clone() else {
                continue;
            };
            match board.set_meta(change.id, before) {
                Ok(previous) => done.push((change.id, previous)),
                Err(err) => {
                    for (id, previous) in done.into_iter().rev() {
                        let _ = board.set_meta(id, previous);
                    }
                    return Err(err.into());
                }
            }
        }
        Ok(())
    }

    fn merge(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if self.field != next.field || self.changes.len() != next.changes.len() {
            return false;
        }
        if !self
            .changes
            .iter()
            .zip(&next.changes)
            .all(|(a, b)| a.id == b.id)
        {
            return false;
        }
        for (mine, theirs) in self.changes.iter_mut().zip(&next.changes) {
            mine.after = theirs.after.clone();
        }
        true
    }

    fn affected(&self) -> Vec<ItemId> {
        self.changes.iter().map(|change| change.id).collect()
    }

    fn label(&self) -> &'static str {
        "Edit metadata"
    }

    fn heap_size(&self) -> usize {
        self.changes
            .iter()
            .map(|change| {
                std::mem::size_of::<Change<ItemMeta>>()
                    + change.after.note.len()
                    + change.before.as_ref().map_or(0, |meta| meta.note.len())
            })
            .sum()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

/// จำนวนขั้น undo สูงสุด (docs/02 §3)
pub const DEFAULT_MAX_ENTRIES: usize = 200;
/// เพดานหน่วยความจำของ undo stack (I-6 — ไม่มี cache ไหนโตได้ไม่จำกัด)
pub const DEFAULT_MAX_BYTES: usize = 64 << 20;

/// undo/redo stack พร้อมเพดานและหน้าต่าง merge
///
/// ★ **เจ้าของ `&mut Board` แต่เพียงผู้เดียว** ทุกการแก้ผ่านที่นี่ (I-3)
#[derive(Debug)]
pub struct History {
    undo: VecDeque<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
    max_entries: usize,
    max_bytes: usize,
    /// รวม `heap_size()` ของทุกใบใน `undo`
    bytes: usize,
    /// หน้าต่าง merge ปิดแล้วหรือยัง (`seal()` เป็นคนปิด)
    sealed: bool,
    /// ความลึกของ stack ตอนบันทึกล่าสุด — `None` = กลับไปสถานะ "บันทึกแล้ว" ไม่ได้อีก
    saved_depth: Option<usize>,
    /// ★ id ที่หลุดออกจากประวัติ **ถาวร** แล้ว รอผู้เรียกมาเก็บกวาด
    ///
    /// ดู [`History::take_forgotten`] — มีไว้เพื่อให้ทรัพยากรที่ผูกกับ item
    /// (thumbnail ใน RAM, ช่องใน atlas) มีวันตาย ไม่ใช่ค้างจนปิดโปรแกรม
    forgotten: Vec<ItemId>,
}

impl Default for History {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ENTRIES, DEFAULT_MAX_BYTES)
    }
}

impl History {
    /// ประวัติว่างเปล่าที่ถือว่า board ปัจจุบัน = สถานะที่บันทึกไว้แล้ว
    #[must_use]
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            max_entries: max_entries.max(1),
            max_bytes,
            bytes: 0,
            sealed: true,
            saved_depth: Some(0),
            forgotten: Vec::new(),
        }
    }

    /// ★ id ที่ประวัติ **ลืมถาวรแล้ว** — เอาไปแล้วรายการในนี้ถูกล้าง
    ///
    /// ตราบใดที่คำสั่งยังอยู่ในสาย undo/redo item ที่มันถือไว้ยัง**กลับมาได้**
    /// ทรัพยากรของ item นั้น (thumbnail ใน RAM) จึงต้องไม่ถูกทิ้ง ไม่งั้น undo
    /// ของการลบจะต้อง decode ใหม่ ซึ่งช้าและอาจล้มถ้าไฟล์ต้นทางหายไปแล้ว
    ///
    /// พอคำสั่งหลุดออกจากประวัติ (ถูกตัดตามเพดาน หรือสาย redo ถูกล้างเพราะทำอะไรใหม่)
    /// item ของมันกลับมาไม่ได้อีกแล้ว **ตรงนั้นคือจุดตายที่ชัดเจน** — คำตอบของคำถาม
    /// "ใครเป็นเจ้าของอายุของ thumbnail" คือ **`History` เป็นคนถือ** (P2-6)
    ///
    /// ★★ **กรองตั้งแต่ตอนลืม ไม่ใช่ตอนเอาไปใช้** — id ในรายการนี้คือตัวที่
    /// "ไม่อยู่บน board **แล้ว ณ วินาทีที่คำสั่งตาย**" ซึ่งแปลว่ากลับมาไม่ได้อีกจริง ๆ
    ///
    /// เคยเป็นบั๊กจริง (พบ 4 ส.ค. 2026 ตอนรันของจริง 400 ภาพ): เดิมรายงานทุก id
    /// แล้วให้ผู้เรียกไปเช็คเอง — แต่ผู้เรียกเช็ค**ทีหลัง** พอถึงตอนนั้นภาพชุดนั้น
    /// ถูกลบไปพอดี (ชั่วคราว ยัง undo ได้) จึงถูกตัดสินว่าตายแล้ว → thumbnail ถูกทิ้ง
    /// → **กด Ctrl+Z แล้วภาพกลับมาใน board แต่ไม่ขึ้นจอเลย** ทั้งที่ทุกอย่าง
    /// "ทำงานถูก" ตามตัวอักษรของสัญญา
    ///
    /// บทเรียน: สัญญาที่ถูกต้องเฉพาะ "ถ้าเรียกทันที" คือกับดัก — ทำให้มันถูกต้อง
    /// โดยไม่ขึ้นกับเวลาที่เรียกดีกว่า `History` มี `board` อยู่ในมือตอนนั้นพอดี
    pub fn take_forgotten(&mut self) -> Vec<ItemId> {
        std::mem::take(&mut self.forgotten)
    }

    /// จดว่าคำสั่งที่เพิ่งตายทำให้ item ตัวไหน "กลับมาไม่ได้อีก"
    ///
    /// ★★ เงื่อนไขที่ถูกต้องมี **สองข้อ** ทั้งคู่ต้องจริงพร้อมกัน:
    ///
    /// 1. ไม่อยู่บน board แล้ว — ถ้ายังอยู่ ผู้ใช้เห็นมันอยู่บนจอ ทรัพยากรต้องอยู่ต่อ
    /// 2. **ไม่มีคำสั่งไหนที่เหลืออยู่พามันกลับมาได้** — ข้อนี้คือข้อที่พลาดง่ายที่สุด
    ///
    /// เคสที่ข้อ 2 มีไว้ดัก: `AddItems(X)` ถูกตัดตามเพดาน **หลัง** `X` ถูกลบไปแล้ว
    /// ด้วย `RemoveItems(X)` ที่ยังอยู่ในสแตก — ข้อ 1 จริง (X ไม่อยู่บน board)
    /// แต่ผู้ใช้ยังกด Ctrl+Z เอา X กลับมาได้อยู่ ถ้าทิ้ง thumbnail ตรงนี้
    /// **ภาพจะกลับมาใน board แต่ไม่ขึ้นจอ** ซึ่งผู้ใช้อ่านว่า "งานหาย"
    ///
    /// ต้องเรียก **หลัง** คำสั่งนั้นถูกถอดออกจาก `undo`/`redo` แล้ว
    /// ไม่งั้นมันจะเจอตัวเองแล้วสรุปว่ายังกลับมาได้
    fn forget(&mut self, board: &Board, dropped: &dyn Command) {
        for id in dropped.affected() {
            if board.item(id).is_some() || self.forgotten.contains(&id) {
                continue;
            }
            if self.can_still_restore(id) {
                continue;
            }
            self.forgotten.push(id);
        }
    }

    /// ยังมีคำสั่งที่เหลืออยู่ตัวไหนพา item นี้กลับมาได้ไหม
    ///
    /// สแกนทั้งสองสาย — เพดาน 200 ขั้นทำให้ต้นทุนคงที่ และสแกนเฉพาะตอนเจอ id
    /// ที่หลุดจาก board แล้วเท่านั้น การเพิ่มภาพตามปกติจึงไม่จ่ายค่านี้เลย
    fn can_still_restore(&self, id: ItemId) -> bool {
        self.undo
            .iter()
            .chain(self.redo.iter())
            .any(|command| command.affected().contains(&id))
    }

    /// ทำคำสั่งแล้วเก็บลงประวัติ
    ///
    /// ถ้าคำสั่งก่อนหน้ารับ merge ได้และหน้าต่างยังเปิดอยู่ คำสั่งนี้จะถูกกลืนเข้าไป
    /// ไม่กลายเป็นขั้น undo ใหม่ (ลากเมาส์ค้าง = 1 undo)
    ///
    /// # Errors
    /// คืน [`CmdError`] จากคำสั่ง — เมื่อคืน `Err` **ทั้ง board และประวัติไม่ถูกแตะเลย**
    pub fn apply(
        &mut self,
        board: &mut Board,
        mut command: Box<dyn Command>,
    ) -> Result<(), CmdError> {
        command.apply(board)?;

        // ทำอะไรใหม่ = เส้นทาง redo เดิมใช้ไม่ได้อีกแล้ว
        // ★ item ที่คำสั่งพวกนั้นถือไว้กลับมาไม่ได้อีกแล้ว — จดไว้ให้ผู้เรียกเก็บกวาด
        let dropped: Vec<Box<dyn Command>> = self.redo.drain(..).collect();
        for command in &dropped {
            self.forget(board, command.as_ref());
        }

        if !self.sealed
            && let Some(previous) = self.undo.back_mut()
        {
            let before = previous.heap_size();
            if previous.merge(command.as_ref()) {
                self.bytes = self.bytes + previous.heap_size() - before;
                self.trim(board);
                self.sync_dirty(board);
                return Ok(());
            }
        }

        self.bytes += command.heap_size();
        self.undo.push_back(command);
        // หน้าต่าง merge เปิดขึ้นอีกครั้งหลังมีขั้นใหม่ — ปิดด้วย `seal()` ตอนปล่อยเมาส์
        self.sealed = false;
        self.trim(board);
        self.sync_dirty(board);
        Ok(())
    }

    /// ★ ปิดหน้าต่าง merge — **เรียกตอนปล่อยปุ่มเมาส์**
    ///
    /// ถ้าลืมเรียก การลากสองครั้งติดกันจะกลายเป็น undo เดียว ผู้ใช้จะงงว่า
    /// ทำไมกด Ctrl+Z ทีเดียวแล้วภาพกระโดดข้ามไปสองที่ (docs/02 §3)
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// ย้อนหนึ่งขั้น — `None` ถ้าไม่มีอะไรให้ย้อน
    ///
    /// คืน **id ที่คำสั่งนั้นแตะ** ให้ชั้น editor เอาไปตั้ง selection (docs/02 §2.9)
    /// — การเลือกไม่ได้ถูก undo มันแค่ตามผลลัพธ์
    ///
    /// # Errors
    /// คืน [`CmdError`] ถ้าคำสั่งย้อนไม่ได้ — คำสั่งนั้นถูกเก็บไว้ในประวัติตามเดิม
    /// เพื่อให้ยังเห็นว่ามีอะไรค้างอยู่ ไม่ใช่หายไปเงียบ ๆ
    pub fn undo(&mut self, board: &mut Board) -> Result<Option<Vec<ItemId>>, CmdError> {
        let Some(mut command) = self.undo.pop_back() else {
            return Ok(None);
        };
        if let Err(err) = command.undo(board) {
            self.undo.push_back(command);
            return Err(err);
        }
        let affected = command.affected();
        self.bytes = self.bytes.saturating_sub(command.heap_size());
        self.redo.push(command);
        // ย้อนแล้วต้องไม่มีการ merge ข้ามการย้อน
        self.sealed = true;
        self.sync_dirty(board);
        Ok(Some(affected))
    }

    /// ทำซ้ำหนึ่งขั้น — `None` ถ้าไม่มีอะไรให้ทำซ้ำ
    ///
    /// คืน id ที่แตะเหมือน [`History::undo`] เพื่อให้ selection ตามผลลัพธ์ทั้งสองทาง
    ///
    /// # Errors
    /// คืน [`CmdError`] ถ้าคำสั่งทำซ้ำไม่ได้ — คำสั่งนั้นยังอยู่ในสาย redo ตามเดิม
    pub fn redo(&mut self, board: &mut Board) -> Result<Option<Vec<ItemId>>, CmdError> {
        let Some(mut command) = self.redo.pop() else {
            return Ok(None);
        };
        if let Err(err) = command.apply(board) {
            self.redo.push(command);
            return Err(err);
        }
        let affected = command.affected();
        self.bytes += command.heap_size();
        self.undo.push_back(command);
        self.sealed = true;
        self.trim(board);
        self.sync_dirty(board);
        Ok(Some(affected))
    }

    /// บันทึกแล้ว — จำความลึกปัจจุบันไว้เป็นจุดอ้างอิงของธง `dirty`
    pub fn mark_saved(&mut self, board: &mut Board) {
        self.saved_depth = Some(self.undo.len());
        self.sync_dirty(board);
    }

    /// จำนวนขั้นที่ย้อนได้
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// จำนวนขั้นที่ทำซ้ำได้
    #[must_use]
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// หน่วยความจำที่ประวัติถืออยู่ (ไบต์) — สำหรับ status bar (I-6)
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// ชื่อของขั้นที่จะถูกย้อนถ้ากด Ctrl+Z ตอนนี้
    #[must_use]
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.back().map(|command| command.label())
    }

    /// ชื่อของขั้นที่จะถูกทำซ้ำถ้ากด Ctrl+Y ตอนนี้
    #[must_use]
    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(|command| command.label())
    }

    /// หน้าต่าง merge ปิดอยู่หรือไม่ (สำหรับเทสต์และ debug)
    #[must_use]
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// ตัดขั้นเก่าสุดทิ้งเมื่อเกินเพดาน (I-6)
    ///
    /// ★ **เหลือไว้อย่างน้อยหนึ่งขั้นเสมอ** ถึงขั้นนั้นจะใหญ่กว่าเพดานก็ตาม —
    /// คำสั่งเดียวที่กินเกิน 64 MB (เช่นลบภาพ 5000 ใบ) ต้องยังย้อนได้
    /// ไม่งั้นเพดานที่ตั้งไว้กันหน่วยความจำจะกลายเป็นตัวทำให้ **งานหาย** เสียเอง
    fn trim(&mut self, board: &Board) {
        while self.undo.len() > 1
            && (self.undo.len() > self.max_entries || self.bytes > self.max_bytes)
        {
            let Some(dropped) = self.undo.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(dropped.heap_size());
            // ย้อนไปไกลกว่านี้ไม่ได้แล้ว — จดตัวที่ไม่มีใครพากลับมาได้อีก
            self.forget(board, dropped.as_ref());
            match self.saved_depth {
                // จุดที่บันทึกไว้เพิ่งถูกตัดทิ้ง — กลับไปหาไม่ได้อีกแล้ว
                Some(0) => self.saved_depth = None,
                Some(depth) => self.saved_depth = Some(depth - 1),
                None => {}
            }
        }
    }

    /// ธง `dirty` ของ board = "ความลึกตอนนี้ต่างจากตอนบันทึกล่าสุด"
    ///
    /// คิดจากความลึกแทนที่จะตั้ง `true` ทิ้งไว้ เพราะผู้ใช้ที่กด Ctrl+Z กลับมาจนสุด
    /// คาดหวังว่าจะปิดโปรแกรมได้โดยไม่โดนถาม — และการถามทั้งที่ไม่มีอะไรเปลี่ยน
    /// จะสอนให้ผู้ใช้กด "ไม่บันทึก" โดยไม่อ่าน ซึ่งวันหนึ่งจะทำให้งานหายจริง
    fn sync_dirty(&self, board: &mut Board) {
        board.mark_dirty(self.saved_depth != Some(self.undo.len()));
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

    use glam::Vec2;
    use proptest::prelude::*;

    use super::*;
    use crate::board::tests::image_item;
    use crate::board::{ColorLabel, TagId};

    fn board_with(n: u8) -> (Board, Vec<ItemId>) {
        let mut board = Board::default();
        let ids = (0..n).map(|i| board.insert_item(image_item(i))).collect();
        // เริ่มจากสถานะสะอาด เหมือน board ที่เพิ่งเปิดไฟล์มา
        board.mark_dirty(false);
        (board, ids)
    }

    fn moved_to(x: f32, y: f32) -> ItemCanvas {
        ItemCanvas {
            pos: Vec2::new(x, y),
            ..ItemCanvas::default()
        }
    }

    // ---------- พื้นฐาน ----------

    #[test]
    fn add_then_undo_leaves_the_board_exactly_as_before() {
        let (mut board, _) = board_with(2);
        let before = board.clone();
        let mut history = History::default();

        history
            .apply(
                &mut board,
                Box::new(AddItems::new(vec![image_item(9)]).unwrap()),
            )
            .unwrap();
        assert_eq!(board.len(), 3);
        assert!(board.is_dirty());

        assert!(history.undo(&mut board).unwrap().is_some());
        assert_eq!(board, before, "undo ต้องคืนสภาพเป๊ะ รวมถึงธง dirty");
        assert!(board.z_order_is_consistent());
    }

    /// ★ redo ของ `AddItems` ต้องได้ **`ItemId` ชุดเดิม** ไม่งั้นคำสั่งถัดไป
    /// ในสาย redo ที่อ้าง id เหล่านั้นจะชี้ไปที่ว่าง
    #[test]
    fn redoing_an_add_gives_back_the_very_same_ids() {
        let (mut board, _) = board_with(1);
        let mut history = History::default();

        let mut command = AddItems::new(vec![image_item(7), image_item(8)]).unwrap();
        command.apply(&mut board).unwrap();
        let first_ids = command.ids();
        command.undo(&mut board).unwrap();
        command.apply(&mut board).unwrap();

        assert_eq!(command.ids(), first_ids, "redo แจก id ชุดใหม่ = สาย redo พัง");
        for id in first_ids {
            assert!(board.item(id).is_some());
        }
        assert!(board.z_order_is_consistent());

        // ผ่าน History ก็ต้องได้ผลเดียวกัน
        history
            .apply(
                &mut board,
                Box::new(AddItems::new(vec![image_item(3)]).unwrap()),
            )
            .unwrap();
        let after_apply = board.clone();
        history.undo(&mut board).unwrap();
        history.redo(&mut board).unwrap();
        assert_eq!(board, after_apply);
    }

    /// ★ undo ของการลบต้องคืนภาพ **พร้อมชั้น z เดิม**
    #[test]
    fn undoing_a_remove_restores_the_item_at_its_old_depth() {
        let (mut board, ids) = board_with(3);
        let before = board.clone();

        let mut history = History::default();
        history
            .apply(
                &mut board,
                Box::new(RemoveItems::new(vec![ids[1]]).unwrap()),
            )
            .unwrap();
        assert_eq!(board.len(), 2);

        let affected = history.undo(&mut board).unwrap().expect("ต้องมีอะไรให้ย้อน");
        assert_eq!(board, before, "z-order กับ id ต้องกลับมาเป๊ะ");

        // ★ คำสั่งต้องบอกได้ว่าแตะอะไร เพื่อให้ชั้น editor เลือกของที่กลับมาให้ผู้ใช้
        //   (การเลือกไม่ได้ถูก undo — มันตามผลลัพธ์ ดู docs/02 §2.9)
        assert_eq!(affected, vec![ids[1]]);
    }

    // ---------- merge / seal ----------

    /// ★ ลากค้าง = **1 undo** (ข้อกำหนดของ P2-5)
    #[test]
    fn a_continuous_drag_collapses_into_a_single_undo_step() {
        let (mut board, ids) = board_with(1);
        let start = board.item(ids[0]).unwrap().canvas;
        let mut history = History::default();

        // 200 เฟรมของการลาก
        for step in 1..=200 {
            let target = moved_to(step as f32, 0.0);
            history
                .apply(
                    &mut board,
                    Box::new(TransformItems::new(vec![(ids[0], target)]).unwrap()),
                )
                .unwrap();
        }

        assert_eq!(history.undo_depth(), 1, "ลากค้างต้องเป็นขั้นเดียว");
        assert_eq!(board.item(ids[0]).unwrap().canvas.pos.x, 200.0);

        history.undo(&mut board).unwrap();
        assert_eq!(
            board.item(ids[0]).unwrap().canvas,
            start,
            "ย้อนทีเดียวต้องกลับไปจุดเริ่มลาก ไม่ใช่เฟรมก่อนหน้า"
        );
    }

    /// ★ ปล่อยเมาส์แล้วลากใหม่ = **2 undo** — ถ้าไม่ seal ผู้ใช้จะงง
    #[test]
    fn sealing_between_drags_keeps_them_as_separate_steps() {
        let (mut board, ids) = board_with(1);
        let mut history = History::default();

        for step in 1..=5 {
            history
                .apply(
                    &mut board,
                    Box::new(
                        TransformItems::new(vec![(ids[0], moved_to(step as f32, 0.0))]).unwrap(),
                    ),
                )
                .unwrap();
        }
        history.seal(); // ปล่อยเมาส์
        assert!(history.is_sealed());

        for step in 1..=5 {
            history
                .apply(
                    &mut board,
                    Box::new(
                        TransformItems::new(vec![(ids[0], moved_to(100.0, step as f32))]).unwrap(),
                    ),
                )
                .unwrap();
        }

        assert_eq!(history.undo_depth(), 2, "การลากสองครั้งต้องเป็นสองขั้น");
        history.undo(&mut board).unwrap();
        assert_eq!(board.item(ids[0]).unwrap().canvas.pos, Vec2::new(5.0, 0.0));
    }

    /// ลากคนละชุด ห้าม merge — ไม่งั้นย้อนทีเดียวจะไปโดนภาพที่ผู้ใช้ไม่ได้แตะ
    #[test]
    fn transforms_on_different_items_never_merge() {
        let (mut board, ids) = board_with(2);
        let mut history = History::default();

        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[0], moved_to(1.0, 0.0))]).unwrap()),
            )
            .unwrap();
        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[1], moved_to(2.0, 0.0))]).unwrap()),
            )
            .unwrap();

        assert_eq!(history.undo_depth(), 2);
    }

    /// คนละชนิดคำสั่งห้าม merge กัน แม้หน้าต่างจะเปิดอยู่
    #[test]
    fn different_command_types_never_merge() {
        let (mut board, ids) = board_with(1);
        let mut history = History::default();

        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[0], moved_to(1.0, 0.0))]).unwrap()),
            )
            .unwrap();
        history
            .apply(
                &mut board,
                Box::new(AddItems::new(vec![image_item(5)]).unwrap()),
            )
            .unwrap();

        assert_eq!(history.undo_depth(), 2);
    }

    /// `EditMeta` merge **ต่อ field**: ให้ดาวรัว ๆ = ขั้นเดียว แต่ดาวแล้วแท็ก = สองขั้น
    #[test]
    fn edit_meta_merges_within_a_field_but_not_across_fields() {
        let (mut board, ids) = board_with(1);
        let mut history = History::default();

        for rating in 1..=5u8 {
            let meta = ItemMeta {
                rating,
                ..ItemMeta::default()
            };
            history
                .apply(
                    &mut board,
                    Box::new(EditMeta::new(MetaField::Rating, vec![(ids[0], meta)]).unwrap()),
                )
                .unwrap();
        }
        assert_eq!(history.undo_depth(), 1);

        let meta = ItemMeta {
            rating: 5,
            tags: smallvec::smallvec![TagId(1)],
            ..ItemMeta::default()
        };
        history
            .apply(
                &mut board,
                Box::new(EditMeta::new(MetaField::Tags, vec![(ids[0], meta)]).unwrap()),
            )
            .unwrap();
        assert_eq!(history.undo_depth(), 2, "คนละ field ต้องเป็นคนละขั้น");

        history.undo(&mut board).unwrap();
        assert_eq!(
            board.item(ids[0]).unwrap().meta.rating,
            5,
            "ย้อนแท็กห้ามเสียดาวไปด้วย"
        );
    }

    // ---------- ล้มกลางคันต้องไม่ทิ้งของครึ่ง ๆ กลาง ๆ ----------

    /// ★ เกณฑ์ผ่านของ P2-2: คำสั่งที่ล้มกลางคันต้องไม่ทิ้ง board ไว้ครึ่งทาง
    #[test]
    fn a_command_that_fails_halfway_leaves_the_board_untouched() {
        let (mut board, ids) = board_with(3);

        // id ตัวที่สองตายไปแล้ว → RemoveItems ต้องล้ม **หลังจากถอดตัวแรกไปแล้ว**
        let dead = ids[1];
        board.remove_item(dead);
        board.mark_dirty(false);
        let before = board.clone();

        let mut history = History::default();
        let err = history
            .apply(
                &mut board,
                Box::new(RemoveItems::new(vec![ids[0], dead, ids[2]]).unwrap()),
            )
            .unwrap_err();

        assert_eq!(err, CmdError::Board(BoardError::NoSuchItem { id: dead }));
        assert_eq!(board, before, "ตัวแรกที่ถอดไปแล้วต้องถูกใส่กลับ");
        assert_eq!(history.undo_depth(), 0, "คำสั่งที่ล้มห้ามเข้าประวัติ");
        assert!(board.z_order_is_consistent());
    }

    /// `TransformItems` ก็ต้องเป็นแบบเดียวกัน
    #[test]
    fn a_failed_transform_rolls_back_the_items_it_already_moved() {
        let (mut board, ids) = board_with(3);
        let dead = ids[1];
        board.remove_item(dead);
        board.mark_dirty(false);
        let before = board.clone();

        let mut history = History::default();
        let err = history
            .apply(
                &mut board,
                Box::new(
                    TransformItems::new(vec![
                        (ids[0], moved_to(50.0, 50.0)),
                        (dead, moved_to(60.0, 60.0)),
                    ])
                    .unwrap(),
                ),
            )
            .unwrap_err();

        assert!(matches!(
            err,
            CmdError::Board(BoardError::NoSuchItem { .. })
        ));
        assert_eq!(board, before, "ตัวแรกที่ย้ายไปแล้วต้องถูกย้ายกลับ");
    }

    /// ลำดับ z ที่ไม่ครบต้องถูกปฏิเสธ **ก่อน** เขียน ไม่ใช่เขียนแล้วค่อยย้อน
    #[test]
    fn an_incomplete_z_order_is_rejected_before_anything_changes() {
        let (mut board, ids) = board_with(3);
        let before = board.clone();
        let mut history = History::default();

        let err = history
            .apply(
                &mut board,
                Box::new(ReorderZ::new(vec![ids[0], ids[1]]).unwrap()),
            )
            .unwrap_err();
        assert_eq!(err, CmdError::ZOrderMismatch);
        assert_eq!(board, before);

        // ซ้ำก็ไม่ได้ — จะทำให้ภาพหนึ่งถูกวาดสองครั้งและอีกภาพหายไป
        let err = history
            .apply(
                &mut board,
                Box::new(ReorderZ::new(vec![ids[0], ids[0], ids[1]]).unwrap()),
            )
            .unwrap_err();
        assert_eq!(err, CmdError::ZOrderMismatch);
        assert_eq!(board, before);

        // ของที่ถูกต้องต้องผ่าน ไม่งั้นเทสต์ข้างบนไม่ได้พิสูจน์อะไร
        history
            .apply(
                &mut board,
                Box::new(ReorderZ::new(vec![ids[2], ids[0], ids[1]]).unwrap()),
            )
            .unwrap();
        assert_eq!(board.z_order(), &[ids[2], ids[0], ids[1]]);
        history.undo(&mut board).unwrap();
        assert_eq!(board, before);
    }

    // ---------- ★ ใครเป็นเจ้าของอายุของทรัพยากรที่ผูกกับ item (P2-6) ----------

    /// ★★ ตราบใดที่ยัง undo ได้ item ที่ถูกลบ **ยังกลับมาได้** ห้ามรายงานว่าลืมแล้ว
    ///
    /// ชั้น UI ใช้ค่านี้ตัดสินว่าจะทิ้ง thumbnail ใน RAM ได้เมื่อไหร่ ถ้ารายงานเร็วไป
    /// undo ของการลบจะต้อง decode ใหม่ — ช้า และ **ล้มถาวรถ้าไฟล์ต้นทางหายไปแล้ว**
    /// (ผู้ใช้ลบภาพในโปรแกรม แล้วลบไฟล์ใน Explorer แล้วค่อยกด Ctrl+Z) = ผิด I-3
    #[test]
    fn nothing_is_forgotten_while_it_can_still_come_back() {
        let (mut board, ids) = board_with(3);
        let mut history = History::default();

        history
            .apply(
                &mut board,
                Box::new(RemoveItems::new(vec![ids[1]]).unwrap()),
            )
            .unwrap();
        assert!(history.take_forgotten().is_empty(), "ลบแล้วยังย้อนได้ = ยังไม่ลืม");

        history.undo(&mut board).unwrap();
        assert!(history.take_forgotten().is_empty(), "ย้อนกลับมาแล้วยิ่งไม่ลืม");

        history.redo(&mut board).unwrap();
        assert!(history.take_forgotten().is_empty());
        assert_eq!(board.len(), 2);
    }

    /// ตัดตามเพดานแล้ว = ย้อนไปถึงไม่ได้อีก → ต้องรายงานว่าลืม
    #[test]
    fn trimming_past_the_cap_reports_what_can_never_return() {
        let (mut board, ids) = board_with(4);
        let mut history = History::new(2, DEFAULT_MAX_BYTES);

        for id in &ids[..3] {
            history.seal();
            history
                .apply(&mut board, Box::new(RemoveItems::new(vec![*id]).unwrap()))
                .unwrap();
        }

        let forgotten = history.take_forgotten();
        assert_eq!(history.undo_depth(), 2, "เพดาน 2 ขั้น");
        assert_eq!(forgotten, vec![ids[0]], "ตัวที่ถูกตัดออกคือตัวแรกเท่านั้น");
        assert!(board.item(ids[0]).is_none(), "และมันไม่ได้อยู่บน board แล้วจริง ๆ");
        assert!(history.take_forgotten().is_empty(), "เอาไปแล้วต้องไม่ซ้ำ");
    }

    /// ★ ทำอะไรใหม่ทับสาย redo = คำสั่งในสายนั้นตายถาวร
    ///
    /// แต่ **`RemoveItems` ที่ถูก undo ไว้แล้วโดนล้าง ไม่ได้แปลว่าภาพตาย** —
    /// ตรงกันข้าม ภาพกลับมาอยู่บน board ถาวรเลย เพราะไม่มีใครลบมันได้อีก
    /// (เทสต์นี้เคยเขียนกลับด้าน แล้วมันคือบั๊กจริง — ดู regression ด้านล่าง)
    #[test]
    fn clearing_the_redo_path_does_not_kill_images_that_came_back() {
        let (mut board, ids) = board_with(3);
        let mut history = History::default();

        history
            .apply(
                &mut board,
                Box::new(RemoveItems::new(vec![ids[2]]).unwrap()),
            )
            .unwrap();
        history.undo(&mut board).unwrap();
        assert_eq!(board.len(), 3, "ภาพกลับมาแล้ว");
        let _ = history.take_forgotten();

        // ทำอย่างอื่นทับ — สาย redo (ที่ถือ RemoveItems อยู่) ตายตรงนี้
        history.seal();
        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[0], moved_to(5.0, 5.0))]).unwrap()),
            )
            .unwrap();

        assert!(
            history.take_forgotten().is_empty(),
            "ภาพยังอยู่บนจอ ห้ามบอกให้ไปทิ้งทรัพยากรของมัน"
        );
        assert!(board.item(ids[2]).is_some());
    }

    /// สาย redo ที่ถือ **`AddItems`** ต่างหากที่ตายจริง — ภาพนั้นกลับมาไม่ได้อีก
    #[test]
    fn clearing_the_redo_path_reports_adds_that_can_never_replay() {
        let (mut board, ids) = board_with(1);
        let mut history = History::default();

        history
            .apply(
                &mut board,
                Box::new(AddItems::new(vec![image_item(7)]).unwrap()),
            )
            .unwrap();
        let added = *board.z_order().last().unwrap();
        history.undo(&mut board).unwrap(); // ภาพหายจาก board แต่ redo ยังพากลับมาได้
        assert!(history.take_forgotten().is_empty(), "ยัง redo ได้ = ยังไม่ตาย");

        history.seal();
        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[0], moved_to(5.0, 5.0))]).unwrap()),
            )
            .unwrap();

        assert_eq!(history.take_forgotten(), vec![added]);
        assert!(board.item(added).is_none());
    }

    /// ★ id ที่ยังอยู่บน board ต้อง **ไม่ถูกรายงาน** แม้คำสั่งที่เพิ่มมันจะถูกตัดทิ้ง
    ///
    /// เคสจริง: ลากภาพ 400 ใบเข้ามา = 400 `AddItems` แต่เพดานคือ 200 ขั้น
    /// → 200 ตัวแรกถูกตัดทันทีตั้งแต่ยังไม่มีใครแตะอะไร ทั้งที่ภาพทั้ง 400 อยู่บนจอครบ
    #[test]
    fn an_id_that_is_still_on_the_board_is_never_reported() {
        let (mut board, _) = board_with(1);
        let mut history = History::new(1, DEFAULT_MAX_BYTES);

        history
            .apply(
                &mut board,
                Box::new(AddItems::new(vec![image_item(7)]).unwrap()),
            )
            .unwrap();
        let added = *board.z_order().last().unwrap();

        history.seal();
        history
            .apply(
                &mut board,
                Box::new(AddItems::new(vec![image_item(8)]).unwrap()),
            )
            .unwrap();

        assert!(
            history.take_forgotten().is_empty(),
            "ภาพยังอยู่บนจอ — ทิ้ง thumbnail แล้วมันจะกลายเป็นสี่เหลี่ยมสีทันที"
        );
        assert!(board.item(added).is_some());
    }

    /// ★★★ regression ของบั๊กที่เจอตอนรันจริงด้วย 400 ภาพ (4 ส.ค. 2026)
    ///
    /// ลำดับเหตุการณ์: `AddItems(X)` ถูกตัดตามเพดาน **หลัง** `X` ถูกลบไปแล้ว
    /// ด้วย `RemoveItems(X)` ที่ยังอยู่ในสแตก
    ///
    /// ตอนนั้น `X` ไม่อยู่บน board จริง (เงื่อนไข "ไม่อยู่บน board" ผ่าน)
    /// **แต่ผู้ใช้ยังกด Ctrl+Z เอามันกลับมาได้อยู่** ถ้ารายงานว่าลืมแล้ว
    /// ชั้น UI จะทิ้ง thumbnail → **กด Ctrl+Z แล้วภาพกลับเข้า board แต่ไม่ขึ้นจอเลย**
    /// ซึ่งผู้ใช้แยกไม่ออกจาก "งานหาย" (ผิด I-3)
    ///
    /// ★ เทสต์ชุดแรกจับไม่ได้เพราะมันเรียก `take_forgotten` **ทันที** ส่วนของจริง
    /// เรียกทีหลัง — **สัญญาที่ถูกเฉพาะเมื่อเรียกทันทีคือกับดัก** เงื่อนไขที่ถูกจริง
    /// คือ "ไม่มีคำสั่งไหนที่เหลืออยู่พามันกลับมาได้" ซึ่งไม่ขึ้นกับเวลาที่เรียก
    #[test]
    fn an_item_a_pending_undo_can_restore_is_never_forgotten() {
        let (mut board, ids) = board_with(2);
        let mut history = History::new(2, DEFAULT_MAX_BYTES);

        // 1) เพิ่มภาพ
        history
            .apply(
                &mut board,
                Box::new(AddItems::new(vec![image_item(7)]).unwrap()),
            )
            .unwrap();
        let added = *board.z_order().last().unwrap();

        // 2) ลบมัน — คำสั่งลบยังอยู่ในสแตก ผู้ใช้ยัง Ctrl+Z ได้
        history.seal();
        history
            .apply(&mut board, Box::new(RemoveItems::new(vec![added]).unwrap()))
            .unwrap();
        assert!(board.item(added).is_none());

        // 3) ทำอย่างอื่นจน `AddItems` ตัวแรกถูกตัดออกตามเพดาน
        history.seal();
        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[0], moved_to(1.0, 1.0))]).unwrap()),
            )
            .unwrap();

        assert!(
            history.take_forgotten().is_empty(),
            "ยัง undo เอาภาพกลับมาได้ ห้ามบอกให้ทิ้ง thumbnail"
        );

        // และพิสูจน์ว่ามันกลับมาได้จริง
        history.undo(&mut board).unwrap();
        history.undo(&mut board).unwrap();
        assert!(board.item(added).is_some(), "ภาพต้องกลับมาได้");
    }
    /// ★★ `docs/08 §3.9` ข้อ 8.2 — **เทสต์ที่ drain แบบเลื่อนออกไปตามที่ของจริงทำ**
    ///
    /// เทสต์อื่นในไฟล์นี้เรียก `take_forgotten()` ทันทีหลังทุกคำสั่ง ซึ่ง
    /// **ไม่ใช่สิ่งที่ของจริงทำ**: ingest ภาพเป็นร้อยใบไม่ได้ drain เลยสักครั้ง
    /// แล้วค่อยไป drain ตอนผู้ใช้ลบ — ตรงนั้นคือจุดที่บั๊กเกิด
    ///
    /// รูปร่างนี้คือเคสจริงย่อส่วน: เพิ่มเกินเพดาน → ลบทั้งหมด → drain ครั้งเดียว
    /// ทุกใบยัง undo กลับมาได้ จึงต้อง**ไม่มีใครถูกรายงานว่าลืม**
    #[test]
    fn draining_late_like_the_real_app_does_reports_nothing_that_can_return() {
        let mut board = Board::default();
        let mut history = History::new(3, DEFAULT_MAX_BYTES);

        // เพิ่ม 6 ใบทีละคำสั่ง (เพดาน 3 → ครึ่งหนึ่งถูกตัดทันที) **ไม่ drain เลย**
        for i in 0..6u8 {
            history.seal();
            history
                .apply(
                    &mut board,
                    Box::new(AddItems::new(vec![image_item(i)]).unwrap()),
                )
                .unwrap();
        }
        let all: Vec<ItemId> = board.z_order().to_vec();
        assert_eq!(all.len(), 6);

        // ลบทั้งหมดในคำสั่งเดียว — ตอนนี้ทุกใบหลุดจาก board แต่ยัง undo ได้
        history.seal();
        history
            .apply(&mut board, Box::new(RemoveItems::new(all.clone()).unwrap()))
            .unwrap();

        // แล้วค่อย drain ทีเดียวตอนนี้ เหมือนที่ชั้น UI ทำ
        assert!(
            history.take_forgotten().is_empty(),
            "ทุกใบยังกด Ctrl+Z กลับมาได้ ห้ามบอกให้ทิ้ง thumbnail"
        );

        history.undo(&mut board).unwrap();
        assert_eq!(board.len(), 6, "ต้องกลับมาครบทุกใบ");
    }

    /// `ReorderZ::affected()` ว่างโดยตั้งใจ → ถูกตัดทิ้งก็ไม่มีอะไรให้ลืม
    #[test]
    fn reordering_never_forgets_anything() {
        let (mut board, ids) = board_with(3);
        let mut history = History::new(1, DEFAULT_MAX_BYTES);
        let flipped: Vec<ItemId> = ids.iter().rev().copied().collect();

        history
            .apply(&mut board, Box::new(ReorderZ::new(flipped).unwrap()))
            .unwrap();
        history.seal();
        history
            .apply(&mut board, Box::new(ReorderZ::new(ids).unwrap()))
            .unwrap();

        assert!(history.take_forgotten().is_empty());
    }

    // ---------- เพดาน (I-6) ----------

    #[test]
    fn the_stack_never_grows_past_max_entries() {
        let (mut board, ids) = board_with(1);
        let mut history = History::new(5, DEFAULT_MAX_BYTES);

        for step in 1..=50 {
            history.seal(); // บังคับให้เป็นคนละขั้นทุกครั้ง
            history
                .apply(
                    &mut board,
                    Box::new(
                        TransformItems::new(vec![(ids[0], moved_to(step as f32, 0.0))]).unwrap(),
                    ),
                )
                .unwrap();
        }
        assert_eq!(history.undo_depth(), 5);
    }

    /// ★ เพดานหน่วยความจำห้ามกลายเป็นตัวทำให้ **งานหาย** เสียเอง —
    /// คำสั่งเดียวที่ใหญ่เกินเพดานต้องยังย้อนได้
    #[test]
    fn a_single_oversized_command_is_still_undoable() {
        let (mut board, _) = board_with(1);
        let before = board.clone();
        let mut history = History::new(DEFAULT_MAX_ENTRIES, 1); // เพดาน 1 ไบต์

        history
            .apply(
                &mut board,
                Box::new(AddItems::new(vec![image_item(4)]).unwrap()),
            )
            .unwrap();
        assert_eq!(history.undo_depth(), 1, "ตัดจนย้อนไม่ได้ = ทำงานหายเสียเอง");

        history.undo(&mut board).unwrap();
        assert_eq!(board, before);
    }

    #[test]
    fn bytes_are_released_when_entries_are_dropped() {
        let (mut board, _) = board_with(1);
        let mut history = History::new(2, DEFAULT_MAX_BYTES);

        for tag in 0..10u8 {
            history.seal();
            history
                .apply(
                    &mut board,
                    Box::new(AddItems::new(vec![image_item(tag)]).unwrap()),
                )
                .unwrap();
        }
        assert_eq!(history.undo_depth(), 2);
        assert!(history.bytes() > 0);
        assert!(
            history.bytes() < 10 * item_heap_size(&image_item(0)) * 2,
            "ตัวเลขไบต์ไม่ถูกหักออกตอนตัดขั้นเก่าทิ้ง: {}",
            history.bytes()
        );
    }

    // ---------- ธง dirty ----------

    #[test]
    fn undoing_back_to_the_saved_point_clears_the_dirty_flag() {
        let (mut board, ids) = board_with(1);
        let mut history = History::default();
        assert!(!board.is_dirty());

        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[0], moved_to(10.0, 0.0))]).unwrap()),
            )
            .unwrap();
        assert!(board.is_dirty());

        history.undo(&mut board).unwrap();
        assert!(!board.is_dirty(), "ย้อนกลับมาจุดที่บันทึกแล้ว ต้องไม่ dirty");

        history.redo(&mut board).unwrap();
        assert!(board.is_dirty());

        history.mark_saved(&mut board);
        assert!(!board.is_dirty(), "บันทึกแล้วต้องสะอาด");
    }

    // ---------- redo ----------

    #[test]
    fn doing_something_new_throws_away_the_redo_branch() {
        let (mut board, ids) = board_with(1);
        let mut history = History::default();

        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[0], moved_to(1.0, 0.0))]).unwrap()),
            )
            .unwrap();
        history.undo(&mut board).unwrap();
        assert_eq!(history.redo_depth(), 1);

        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[0], moved_to(2.0, 0.0))]).unwrap()),
            )
            .unwrap();
        assert_eq!(history.redo_depth(), 0);
        assert!(history.redo(&mut board).unwrap().is_none());
    }

    #[test]
    fn undo_and_redo_on_an_empty_history_are_quiet_no_ops() {
        let (mut board, _) = board_with(1);
        let before = board.clone();
        let mut history = History::default();

        assert!(history.undo(&mut board).unwrap().is_none());
        assert!(history.redo(&mut board).unwrap().is_none());
        assert_eq!(board, before);
        assert_eq!(history.undo_label(), None);
    }

    /// ★ เกณฑ์ผ่านของ P2-2: สลับ undo/redo 1000 รอบแล้วต้องเท่าเดิม **เป๊ะ**
    /// ไม่ใช่ "ดูเหมือนเดิม" — `Board: PartialEq` เทียบถึงรุ่นของทุกช่องใน arena
    #[test]
    fn a_thousand_undo_redo_cycles_land_on_exactly_the_same_state() {
        let (mut board, ids) = board_with(4);
        let mut history = History::default();

        // ผสมคำสั่งหลายชนิดให้สายมีความหลากหลายจริง
        history
            .apply(
                &mut board,
                Box::new(AddItems::new(vec![image_item(9)]).unwrap()),
            )
            .unwrap();
        history.seal();
        history
            .apply(
                &mut board,
                Box::new(RemoveItems::new(vec![ids[1]]).unwrap()),
            )
            .unwrap();
        history.seal();
        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[0], moved_to(33.0, 44.0))]).unwrap()),
            )
            .unwrap();
        history.seal();
        history
            .apply(
                &mut board,
                Box::new(
                    EditMeta::new(
                        MetaField::ColorLabel,
                        vec![(
                            ids[2],
                            ItemMeta {
                                color_label: Some(ColorLabel::Green),
                                ..ItemMeta::default()
                            },
                        )],
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        history.seal();

        let settled = board.clone();
        let depth = history.undo_depth();

        for round in 0..1000 {
            for _ in 0..depth {
                history.undo(&mut board).unwrap();
            }
            for _ in 0..depth {
                history.redo(&mut board).unwrap();
            }
            assert_eq!(board, settled, "เพี้ยนที่รอบที่ {round}");
            assert!(board.z_order_is_consistent());
        }
    }

    // ---------- property test ----------

    /// คำสั่งที่ property test สุ่มออกมา — จงใจให้บางตัว "ผิด" ได้ด้วย
    #[derive(Debug, Clone)]
    enum AnyCommand {
        Add,
        Remove(usize),
        Transform(usize, f32, f32),
        Reorder(usize),
        Rate(usize, u8),
    }

    fn any_command() -> impl Strategy<Value = AnyCommand> {
        prop_oneof![
            Just(AnyCommand::Add),
            (0usize..6).prop_map(AnyCommand::Remove),
            (0usize..6, -2000.0f32..2000.0, -2000.0f32..2000.0)
                .prop_map(|(i, x, y)| AnyCommand::Transform(i, x, y)),
            (0usize..6).prop_map(AnyCommand::Reorder),
            (0usize..6, 0u8..8).prop_map(|(i, r)| AnyCommand::Rate(i, r)),
        ]
    }

    /// แปลงคำสั่งที่สุ่มมาเป็นคำสั่งจริง — คืน `None` ถ้าประกอบไม่ได้บน board ตอนนี้
    fn build(board: &Board, command: &AnyCommand) -> Option<Box<dyn Command>> {
        let live: Vec<ItemId> = board.z_order().to_vec();
        match *command {
            AnyCommand::Add => Some(Box::new(AddItems::new(vec![image_item(200)]).ok()?)),
            AnyCommand::Remove(i) => {
                let id = *live.get(i % live.len().max(1))?;
                Some(Box::new(RemoveItems::new(vec![id]).ok()?))
            }
            AnyCommand::Transform(i, x, y) => {
                let id = *live.get(i % live.len().max(1))?;
                Some(Box::new(
                    TransformItems::new(vec![(id, moved_to(x, y))]).ok()?,
                ))
            }
            AnyCommand::Reorder(i) => {
                if live.len() < 2 {
                    return None;
                }
                let mut order = live;
                let shift = i % order.len();
                order.rotate_left(shift);
                Some(Box::new(ReorderZ::new(order).ok()?))
            }
            AnyCommand::Rate(i, rating) => {
                let id = *live.get(i % live.len().max(1))?;
                let mut meta = board.item(id)?.meta.clone();
                meta.rating = rating;
                Some(Box::new(
                    EditMeta::new(MetaField::Rating, vec![(id, meta)]).ok()?,
                ))
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// ★★ `undo_restores_exactly` — หัวใจของ I-3 (ROADMAP P2-2, docs/08 §1)
        ///
        /// ทำคำสั่งมั่ว ๆ กี่ตัวก็ได้ แล้วย้อนให้หมด สถานะต้องกลับมา **เท่ากันเป๊ะ**
        /// รวมถึงรุ่นของทุกช่องใน arena, ลำดับ z และธง dirty
        #[test]
        fn undo_restores_exactly(commands in prop::collection::vec(any_command(), 0..40)) {
            let (mut board, _ids) = board_with(4);
            let before = board.clone();

            let mut history = History::default();
            for command in &commands {
                // seal ทุกครั้ง เพื่อให้ทุกคำสั่งเป็นขั้นของตัวเอง — merge ถูกตรวจแยกไปแล้ว
                history.seal();
                if let Some(command) = build(&board, command) {
                    // คำสั่งที่ล้มไม่ควรเปลี่ยนอะไร จึงปล่อยผ่านได้
                    let _ = history.apply(&mut board, command);
                }
                prop_assert!(board.z_order_is_consistent());
            }

            while history.undo(&mut board)?.is_some() {}
            prop_assert_eq!(&board, &before, "ย้อนหมดแล้วไม่กลับมาเท่าเดิม");
        }

        /// ย้อนจนสุดแล้วทำซ้ำจนสุด ต้องกลับมาที่สถานะปลายทางเป๊ะเช่นกัน
        #[test]
        fn redo_returns_to_the_same_place(commands in prop::collection::vec(any_command(), 0..30)) {
            let (mut board, _) = board_with(4);
            let mut history = History::default();
            for command in &commands {
                history.seal();
                if let Some(command) = build(&board, command) {
                    let _ = history.apply(&mut board, command);
                }
            }
            let settled = board.clone();

            while history.undo(&mut board)?.is_some() {}
            while history.redo(&mut board)?.is_some() {}
            prop_assert_eq!(&board, &settled);
        }
    }
}
