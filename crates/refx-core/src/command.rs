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

use crate::arena::{GroupId, ItemId};
use crate::board::{Board, BoardError, Group, Item, ItemCanvas, ItemKind, ItemMeta, TagId};
use crate::geom::Rect;
use crate::layout::Placed;

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
// ApplyLayout
// ---------------------------------------------------------------------------

/// เอาผลของ layout engine ลง canvas ทั้งชุด (P3-5 · docs/03 §4.1)
///
/// ★★★ **นี่คือจุดเดียวที่ผลของ Arrange ไปแตะข้อมูลจริง** — ก่อนหน้านี้ layout
/// เป็นแค่สิ่งที่วาดบนแผ่น contact sheet เท่านั้น (P3-3/P3-4 ไม่แตะ `Board` เลย)
///
/// ★ **ต้องเป็น undo ขั้นเดียวสำหรับทั้งกระดาน** (เกณฑ์ ROADMAP P3-5) — ผู้ใช้กด
/// "จัดลง canvas" ที่ 3,000 ภาพแล้วไม่ชอบ ต้องกด Ctrl+Z ครั้งเดียวได้ของเดิมคืนครบ
/// ไม่ใช่กด 3,000 ครั้ง · `TransformItems` เก็บค่าเดิมของทุกใบไว้อยู่แล้ว
/// จึงยืมมาทั้งดุ้นเหมือนที่ [`SetCrop`] ทำ
///
/// ★★ **`merge` คืน `false` เสมอ** ต่างจากคำสั่งอื่น — การจัดสองครั้งติดกันคือ
/// สองการตัดสินใจของผู้ใช้ (เขาลองแบบ Grid แล้วลองแบบ Masonry) ถ้ายุบเป็นขั้นเดียว
/// กด Ctrl+Z แล้วจะข้ามกลับไปสภาพก่อนจัดครั้งแรกเลย ซึ่งไม่ใช่สิ่งที่เขาขอ
#[derive(Debug)]
pub struct ApplyLayout {
    inner: TransformItems,
}

impl ApplyLayout {
    /// ตั้ง `ItemCanvas` ชุดใหม่ให้ทุกใบที่ layout จัดให้
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง (ไม่มีใบไหนขยับ = ไม่ต้องกิน undo)
    pub fn new(changes: Vec<(ItemId, ItemCanvas)>) -> Result<Self, CmdError> {
        Ok(Self {
            inner: TransformItems::new(changes)?,
        })
    }

    /// สร้างคำสั่งจากผลของ layout engine — `None` เมื่อ **ไม่มีใบไหนขยับจริง**
    ///
    /// คืน `None` แทน `Err` เพราะ "กดแล้วทุกอย่างอยู่ที่เดิมอยู่แล้ว" ไม่ใช่ความ
    /// ผิดพลาด — แค่ต้องไม่กิน undo เปล่า ๆ (หลักการเดียวกับ `zorder::reordered`)
    ///
    /// ★★★ **ใบที่ปักหมุดและใบที่ล็อกไว้ไม่ถูกแตะ**
    ///
    /// `docs/03 §4.1` เขียนว่า "ไม่ขยับภาพที่ `pinned = true` (layout จะจัดรอบมันแทน)"
    /// — **ครึ่งแรกทำแล้ว ครึ่งหลังยังไม่ได้ทำ**: การจัดของที่เหลือให้ *หลบ* ภาพที่
    /// ปักหมุดต้องมีการตรวจการทับกันในตัว engine ซึ่งไม่มีตัวไหนรองรับ (P3-2
    /// จึงตัด `respect_pinned` ทิ้งเพราะ input ของมันไม่มีทั้งธง pin และกรอบจริง)
    /// ตอนนี้ผลคือ **ของที่จัดใหม่อาจไปวางทับใบที่ปักหมุดได้** ซึ่งผู้ใช้แก้เองได้
    /// ด้วยการลาก — ต่างจากการ *ย้ายใบที่เขาสั่งไม่ให้ย้าย* ซึ่งแก้กลับเองไม่ได้เลย
    ///
    /// ★ ใบที่ `locked` ก็ไม่ถูกแตะด้วยเหตุผลเดียวกัน (ล็อก = ห้ามแก้ — `SelectTool`
    /// เคารพอยู่แล้ว การจัดทั้งกระดานต้องไม่เป็นทางลัดที่ข้ามมันไปได้)
    ///
    /// ★★ **กลุ่มอยู่ที่เดิม**: จุดกึ่งกลางของกรอบรวม *หลัง* จัด ถูกเลื่อนให้ตรงกับ
    /// จุดกึ่งกลางของกรอบรวม *ก่อน* จัด · `Placed` เริ่มที่ `(0,0)` เสมอ ถ้าเอาไปใส่
    /// ตรง ๆ ภาพทั้งกระดานจะกระโดดไปมุมซ้ายบนของ world แล้วผู้ใช้จะหาไม่เจอ
    /// (docs/03 §4.1 มีตัวเลือก "กลางจอ canvas" กับ "ต่อท้ายด้านล่าง" ด้วย —
    /// ยังไม่ได้ทำ ดู HANDOFF)
    #[must_use]
    pub fn from_placed(board: &Board, placed: &[Placed]) -> Option<Self> {
        // 1. คัดเฉพาะใบที่ขยับได้จริง
        let movable: Vec<(ItemId, ItemCanvas, Placed)> = placed
            .iter()
            .filter_map(|slot| {
                let item = board.item(slot.id)?;
                (!item.meta.pinned && !item.canvas.locked).then_some((slot.id, item.canvas, *slot))
            })
            .collect();
        if movable.is_empty() {
            return None;
        }

        // 2. กรอบรวมก่อน/หลัง แล้วเลื่อนให้กึ่งกลางตรงกัน
        let mut before = Rect::EMPTY;
        let mut after = Rect::EMPTY;
        for (_, canvas, slot) in &movable {
            before = before.union(canvas.world_bounds());
            after = after.union(Rect::from_corners(slot.top_left, slot.top_left + slot.size));
        }
        let offset = before.center() - after.center();

        // 3. ใบที่ค่าไม่เปลี่ยนเลยไม่ต้องเข้าคำสั่ง — undo ที่ไม่ทำอะไรคือ undo ที่หลอก
        let changes: Vec<(ItemId, ItemCanvas)> = movable
            .into_iter()
            .filter_map(|(id, canvas, slot)| {
                let next = ItemCanvas {
                    pos: slot.centre() + offset,
                    size: slot.size,
                    // ★ การจัดวางเป็นเรื่องของ **ตำแหน่งกับขนาด** เท่านั้น
                    //   crop/flip/filter/opacity/rotation ของผู้ใช้ต้องไม่ถูกล้าง
                    ..canvas
                }
                .sanitized();
                (next != canvas).then_some((id, next))
            })
            .collect();

        Self::new(changes).ok()
    }
}

impl Command for ApplyLayout {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        self.inner.apply(board)
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        self.inner.undo(board)
    }

    /// ★ ไม่รวมกับอะไรเลย — ดูเหตุผลที่หัวโครงสร้าง
    fn merge(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn affected(&self) -> Vec<ItemId> {
        self.inner.affected()
    }

    fn label(&self) -> &'static str {
        "Apply layout"
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
// RelinkAssets  (P4-6)
// ---------------------------------------------------------------------------

/// ★★★ ผูก item เข้ากับที่มาของพิกเซลใหม่ — relink (`docs/07 §2`)
///
/// คำสั่งเดียวครอบทั้งสามทิศ เพราะทั้งสามคือเรื่องเดียวกัน (*"พิกเซลของ item นี้
/// มาจากไหน"*) และผู้ใช้เห็นมันเป็นการกระทำเดียว:
///
/// | จาก | เป็น | เกิดตอนไหน |
/// |---|---|---|
/// | `Missing` | `Image` | หาไฟล์เจอ (ขั้น 1–3 หรือผู้ใช้ชี้เอง) |
/// | `Image` | `Missing` | เปิดไฟล์มาแล้วภาพหายจากเครื่อง |
/// | `Image` | `Image` | ซ่อมคีย์ที่เป็น hash ของ *path* ให้เป็นของ *เนื้อ* |
///
/// ★★ **merge ได้** เพราะผลลัพธ์ทยอยกลับมาทีละใบข้ามหลายเฟรม · ถ้าไม่ merge
/// การเปิดไฟล์ที่มีภาพหาย 200 ใบจะดัน undo stack 200 ขั้นที่ผู้ใช้ต้องกด Ctrl+Z
/// สองร้อยครั้งเพื่อย้อนสิ่งที่เขาเห็นเป็นการกระทำเดียว
///
/// ★ ตำแหน่ง/ขนาด/หมุน/ครอป/แท็ก **ไม่ถูกแตะ** — ดู [`Board::set_source`]
#[derive(Debug)]
pub struct RelinkAssets {
    changes: Vec<Change<ItemKind>>,
}

impl RelinkAssets {
    /// สร้างคำสั่งจากรายการ (id, ที่มาใหม่)
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง หรือ id ไหนไม่มีอยู่/เป็นโน้ตข้อความ
    pub fn new(board: &Board, targets: Vec<(ItemId, ItemKind)>) -> Result<Self, CmdError> {
        let mut changes = Vec::with_capacity(targets.len());
        for (id, after) in targets {
            let Some(item) = board.item(id) else {
                continue; // item ถูกลบไประหว่างที่งานค้นหาเดินอยู่
            };
            if matches!(item.kind, ItemKind::Text(_)) || matches!(after, ItemKind::Text(_)) {
                continue;
            }
            // ★ ไม่มีอะไรเปลี่ยน = ไม่ต้องมีขั้น undo (เคสปกติของไฟล์ที่ยังอยู่ที่เดิม)
            if item.kind == after {
                continue;
            }
            changes.push(Change {
                id,
                before: Some(item.kind.clone()),
                after,
            });
        }
        if changes.is_empty() {
            return Err(CmdError::Empty);
        }
        Ok(Self { changes })
    }
}

impl Command for RelinkAssets {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let mut done: Vec<(ItemId, ItemKind)> = Vec::with_capacity(self.changes.len());
        for change in &self.changes {
            match board.set_source(change.id, change.after.clone()) {
                Ok(previous) => done.push((change.id, previous)),
                Err(err) => {
                    // ★ ล้มกลางคัน — คืนของที่แก้ไปแล้วให้ครบก่อนรายงาน
                    for (id, previous) in done.into_iter().rev() {
                        let _ = board.set_source(id, previous);
                    }
                    return Err(err.into());
                }
            }
        }
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let mut done: Vec<(ItemId, ItemKind)> = Vec::with_capacity(self.changes.len());
        for change in &self.changes {
            // `before` ถูกเติมตั้งแต่ตอนสร้าง — ไม่มีทางว่างในคำสั่งนี้
            let Some(before) = change.before.clone() else {
                continue;
            };
            match board.set_source(change.id, before) {
                Ok(previous) => done.push((change.id, previous)),
                Err(err) => {
                    for (id, previous) in done.into_iter().rev() {
                        let _ = board.set_source(id, previous);
                    }
                    return Err(err.into());
                }
            }
        }
        Ok(())
    }

    /// รวมงวดที่ทยอยกลับมาให้เป็นขั้นเดียว
    ///
    /// ★ item ที่โผล่ในทั้งสองงวดใช้ `after` ของงวดใหม่ แต่เก็บ `before` ของ
    /// **งวดแรก** ไว้ — undo ต้องกลับไปที่สภาพก่อนเริ่มทั้งชุด ไม่ใช่สภาพกลางทาง
    fn merge(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        for incoming in &next.changes {
            if let Some(mine) = self.changes.iter_mut().find(|c| c.id == incoming.id) {
                mine.after = incoming.after.clone();
            } else {
                self.changes.push(incoming.clone());
            }
        }
        true
    }

    fn affected(&self) -> Vec<ItemId> {
        self.changes.iter().map(|change| change.id).collect()
    }

    fn label(&self) -> &'static str {
        "Relink images"
    }

    fn heap_size(&self) -> usize {
        self.changes
            .iter()
            .map(|change| {
                std::mem::size_of::<Change<ItemKind>>()
                    + change.before.as_ref().map_or(0, kind_heap_size)
                    + kind_heap_size(&change.after)
            })
            .sum()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// ไบต์บน heap ที่ `ItemKind` ตัวหนึ่งถือไว้ (I-6 — เพดานของ `History`)
#[must_use]
fn kind_heap_size(kind: &ItemKind) -> usize {
    match kind {
        ItemKind::Image(asset) => asset.path.as_os_str().len(),
        ItemKind::Text(note) => note.text.len(),
        ItemKind::Missing { original_path, .. } => original_path.as_os_str().len(),
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
// TagItems
// ---------------------------------------------------------------------------

/// ติด/ถอดแท็กให้ item หลายใบ พร้อม **สร้างชื่อแท็กใหม่ถ้ายังไม่มี** (P3-1)
///
/// ★★ **ทำไมไม่ใช้ `EditMeta` เฉย ๆ** — การติดแท็กแตะ **สองที่**: `ItemMeta.tags`
/// ของแต่ละ item **และ** `TagTable` ของ board (ถ้าเป็นชื่อใหม่) ทั้งคู่อยู่ใน `Board`
/// จึงต้องย้อนพร้อมกันเป็นก้อนเดียว · ถ้าแยกเป็นสองคำสั่ง ผู้ใช้กด Ctrl+Z ครั้งเดียว
/// จะได้ item ที่ถือ `TagId` ซึ่งไม่มีชื่อในตารางแล้ว — แท็กที่กดดูแล้วว่างเปล่า
///
/// ★ **ชื่อที่ถูกสร้างในคำสั่งนี้เท่านั้นที่ถูกลบตอน undo** — ถ้าลบทุกครั้ง
/// แท็กที่ผู้ใช้ใช้อยู่กับภาพอื่นจะหายไปด้วย
#[derive(Debug)]
pub struct TagItems {
    /// ชื่อที่ผู้ใช้พิมพ์ (ยังไม่ normalize)
    name: String,
    /// ติด (`true`) หรือถอด (`false`)
    attach: bool,
    targets: Vec<ItemId>,
    /// meta เดิมของแต่ละ item — เก็บตอน apply ครั้งแรก
    before: Vec<(ItemId, ItemMeta)>,
    /// แท็กที่ **คำสั่งนี้เป็นคนสร้าง** — `None` = ใช้ของที่มีอยู่แล้ว
    created: Option<(TagId, String)>,
}

impl TagItems {
    /// ติดแท็กชื่อนี้ให้ทุก id ที่ส่งมา
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง หรือชื่อว่างเปล่าหลังตัดช่องว่าง
    pub fn attach(name: &str, targets: Vec<ItemId>) -> Result<Self, CmdError> {
        Self::new(name, true, targets)
    }

    /// ถอดแท็กชื่อนี้ออกจากทุก id ที่ส่งมา
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง หรือชื่อว่างเปล่า
    pub fn detach(name: &str, targets: Vec<ItemId>) -> Result<Self, CmdError> {
        Self::new(name, false, targets)
    }

    fn new(name: &str, attach: bool, targets: Vec<ItemId>) -> Result<Self, CmdError> {
        if targets.is_empty() {
            return Err(CmdError::Empty);
        }
        let name = crate::board::TagTable::normalize(name).ok_or(CmdError::Empty)?;
        Ok(Self {
            name,
            attach,
            targets,
            before: Vec::new(),
            created: None,
        })
    }
}

impl Command for TagItems {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        // ★ หา id ของแท็กก่อน — สร้างใหม่เฉพาะตอน "ติด" เท่านั้น
        //   การถอดแท็กที่ไม่มีในตารางคือ no-op ไม่ใช่เหตุให้สร้างชื่อขึ้นมา
        let tag: TagId = match board.tags().find(&self.name) {
            Some(id) => id,
            None if self.attach => {
                // redo ต้องได้ **id เดิม** ไม่ใช่ id ใหม่ ไม่งั้น item ที่คำสั่งถัดไป
                // ในสาย redo อ้างถึงจะชี้ไปที่แท็กที่ไม่มีอยู่ (หลักการเดียวกับ
                // `Arena::insert_at` — HANDOFF §4 ข้อ 19)
                match self.created.as_ref() {
                    Some((id, name)) => {
                        board.restore_tag(*id, name.clone());
                        *id
                    }
                    None => {
                        let id = board.insert_tag(&self.name).ok_or(CmdError::Empty)?;
                        self.created = Some((id, self.name.clone()));
                        id
                    }
                }
            }
            None => return Ok(()), // ถอดแท็กที่ไม่มี = ไม่มีอะไรเกิดขึ้น
        };

        // ★ เก็บของเดิมครั้งแรกครั้งเดียว — redo เรียก `apply` ซ้ำ
        let first_time = self.before.is_empty();
        let mut done: Vec<(ItemId, ItemMeta)> = Vec::new();
        for id in &self.targets {
            let Some(item) = board.item(*id) else {
                continue;
            };
            let previous = item.meta.clone();
            let mut next = previous.clone();
            if self.attach {
                if next.tags.contains(&tag) {
                    continue; // ติดอยู่แล้ว
                }
                next.tags.push(tag);
                // เรียงเสมอ — ลำดับแท็กต้องไม่ขึ้นกับลำดับที่ผู้ใช้กด
                next.tags.sort_unstable();
            } else if let Some(at) = next.tags.iter().position(|other| *other == tag) {
                next.tags.remove(at);
            } else {
                continue; // ไม่ได้ติดอยู่
            }
            // ★ ล้มกลางคันต้องย้อนสิ่งที่ทำไปแล้วคืน — `apply` ที่คืน `Err`
            //   ห้ามแตะ board เลย (HANDOFF §2.1 ข้อ 1)
            if let Err(err) = board.set_meta(*id, next) {
                for (undo_id, undo_meta) in done {
                    let _ = board.set_meta(undo_id, undo_meta);
                }
                if let Some((tag_id, _)) = self.created.as_ref()
                    && first_time
                {
                    board.remove_tag(*tag_id);
                    self.created = None;
                }
                return Err(err.into());
            }
            done.push((*id, previous));
        }
        if first_time {
            self.before = done;
        }
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        for (id, meta) in &self.before {
            board.set_meta(*id, meta.clone())?;
        }
        // ★ ลบชื่อแท็กออก **เฉพาะตัวที่คำสั่งนี้สร้างเอง** — ตัวที่มีอยู่ก่อนแล้ว
        //   ยังถูกใช้กับภาพอื่นอยู่ ลบไปด้วยจะทำให้แท็กของภาพเหล่านั้นว่างเปล่า
        if let Some((id, _)) = self.created.as_ref() {
            board.remove_tag(*id);
        }
        Ok(())
    }

    /// **ไม่ merge** — ติดแท็กหนึ่งครั้ง = undo หนึ่งขั้นเสมอ
    ///
    /// ต่างจากการลากสไลเดอร์ที่ผู้ใช้ขยับรัว ๆ โดยมองว่าเป็นการกระทำเดียว
    fn merge(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn affected(&self) -> Vec<ItemId> {
        self.targets.clone()
    }

    fn label(&self) -> &'static str {
        if self.attach { "Add tag" } else { "Remove tag" }
    }

    fn heap_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.capacity()
            + self.targets.capacity() * std::mem::size_of::<ItemId>()
            + self
                .before
                .iter()
                .map(|(_, meta)| std::mem::size_of::<(ItemId, ItemMeta)>() + meta.note.capacity())
                .sum::<usize>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// EditText
// ---------------------------------------------------------------------------

/// แก้เนื้อความของโน้ต (P2-11)
///
/// ★★ **ทำไมต้องเป็น `Command`** — `TextNote.text` อยู่ใน `Board` จึงเป็นเอกสาร
/// (`docs/08 §4` ข้อ 10) · โน้ตที่พิมพ์ไปสิบบรรทัดแล้วหายเพราะ Ctrl+Z ย้อนไม่ถึง
/// คือการทำงานหายแบบเดียวกับภาพหาย (I-3)
///
/// ★ **merge ได้เฉพาะ item เดียวกัน** — พิมพ์รัว ๆ ในโน้ตหนึ่งใบยุบเป็น undo
/// ขั้นเดียว (ไม่งั้นการพิมพ์ประโยคเดียวกิน undo stack ทั้งสแตกจนคำสั่งอื่นถูกตัดทิ้ง
/// ตามเพดาน 200 ขั้น) แต่ **ห้ามข้ามใบ** ไม่งั้นแก้โน้ต A แล้วแก้ B
/// จะย้อนทีเดียวเสียทั้งสองใบ
#[derive(Debug)]
pub struct EditText {
    id: ItemId,
    after: String,
    /// ข้อความเดิม — เก็บ **ตอน apply ครั้งแรกเท่านั้น** เพื่อให้ redo ไม่เขียนทับ
    before: Option<String>,
}

impl EditText {
    /// ตั้งเนื้อความใหม่ให้โน้ตหนึ่งใบ
    #[must_use]
    pub fn new(id: ItemId, text: String) -> Self {
        Self {
            id,
            after: text,
            before: None,
        }
    }
}

impl Command for EditText {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let previous = board.set_text(self.id, self.after.clone())?;
        // ★ เก็บของเดิมครั้งแรกครั้งเดียว — redo เรียก `apply` ซ้ำ ถ้าเขียนทับทุกครั้ง
        //   ค่าเดิมจะกลายเป็นค่าที่คำสั่งนี้เพิ่งเขียนไป แล้ว undo จะคืนอะไรไม่ได้เลย
        if self.before.is_none() {
            self.before = Some(previous);
        }
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let Some(before) = self.before.clone() else {
            return Ok(()); // ยังไม่เคย apply — ไม่มีอะไรให้คืน
        };
        board.set_text(self.id, before)?;
        Ok(())
    }

    fn merge(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if next.id != self.id {
            return false;
        }
        self.after.clone_from(&next.after);
        true
    }

    fn affected(&self) -> Vec<ItemId> {
        vec![self.id]
    }

    fn label(&self) -> &'static str {
        "Edit note"
    }

    fn heap_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.after.capacity()
            + self.before.as_ref().map_or(0, String::capacity)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// GroupItems / Ungroup / SetGroup  (P3-7)
// ---------------------------------------------------------------------------

/// ★ เก็บกวาดกลุ่มที่ไม่มีสมาชิกเหลือแล้ว คืนรายการที่ถูกเอาออก (พร้อมตัวมันเอง)
///
/// ★★ **ทำไมต้องเก็บกวาด** — กลุ่มไม่มีรายชื่อสมาชิกของตัวเอง (ดู [`Group`])
/// กลุ่มที่สมาชิกย้ายออกหมดจึงเป็นแถวที่ผู้ใช้กดแล้วไม่มีอะไรอยู่ข้างใน และ
/// ไม่มีทางลบมันได้เลยเพราะไม่มีสมาชิกให้เลือกไปสั่ง ungroup — ขยะที่สะสมทุกครั้ง
/// ที่ผู้ใช้จัดกลุ่มใหม่ และถูก persist ลง `.refx` ไปด้วย
///
/// คืนค่าเพื่อให้ `undo` ใส่กลับที่ **คีย์เดิม** ได้ (ดู `Board::restore_group`)
fn sweep_empty_groups(board: &mut Board, candidates: &[GroupId]) -> Vec<(GroupId, Group)> {
    let mut swept = Vec::new();
    for id in candidates {
        if board.group(*id).is_none() || board.group_members(*id).next().is_some() {
            continue;
        }
        if let Some(group) = board.remove_group(*id) {
            swept.push((*id, group));
        }
    }
    swept
}

/// ใส่กลุ่มที่ถูกเก็บกวาดไปกลับคืนที่คีย์เดิม — คู่ของ [`sweep_empty_groups`]
fn restore_swept(board: &mut Board, swept: &[(GroupId, Group)]) {
    // ★ ย้อนลำดับที่เอาออก เพื่อให้ผลเหมือนเดิมเป๊ะเมื่อมีหลายกลุ่ม
    for (id, group) in swept.iter().rev() {
        let _ = board.restore_group(*id, group.clone());
    }
}

/// รวม item ที่เลือกไว้เป็นกลุ่มใหม่หนึ่งกลุ่ม (`Ctrl+G` — docs/03 §5)
///
/// ★★ **ทำไมไม่ใช่ `EditMeta` เฉย ๆ** — เหตุผลเดียวกับ [`TagItems`]: การจัดกลุ่ม
/// แตะ **สองที่** คือ `ItemMeta.group` ของแต่ละใบ **และ** `Board::groups`
/// ทั้งคู่อยู่ใน `Board` จึงต้องย้อนพร้อมกันเป็นก้อนเดียว · ถ้าแยกเป็นสองคำสั่ง
/// ผู้ใช้กด Ctrl+Z ครั้งเดียวจะได้ item ที่ถือ `GroupId` ซึ่งไม่มีกลุ่มอยู่แล้ว
///
/// ★ **ย้ายเข้ากลุ่มใหม่ ไม่ใช่ซ้อนกลุ่ม** — `ItemMeta::group` เป็น `Option<GroupId>`
/// ตัวเดียว ไม่ใช่ต้นไม้ · item อยู่ได้ทีละกลุ่มตามที่ `docs/02 §2.2` กำหนดไว้
#[derive(Debug)]
pub struct GroupItems {
    targets: Vec<ItemId>,
    /// ชื่อที่จะใช้ตอนสร้าง — คำนวณจาก board ตอน apply ครั้งแรก
    name: String,
    /// กลุ่มที่ **คำสั่งนี้เป็นคนสร้าง** — เก็บไว้ให้ redo ใช้ id เดิม
    created: Option<(GroupId, Group)>,
    /// meta เดิมของแต่ละใบ — เก็บตอน apply ครั้งแรก
    before: Vec<(ItemId, ItemMeta)>,
    /// กลุ่มเดิมที่ว่างลงเพราะการย้ายครั้งนี้ แล้วถูกเก็บกวาด
    swept: Vec<(GroupId, Group)>,
}

impl GroupItems {
    /// รวม item เหล่านี้เป็นกลุ่มใหม่ · `name_base` คือคำตั้งต้นของชื่ออัตโนมัติ
    /// (ชั้น UI ส่งมาเป็นภาษาของผู้ใช้ — ดู `Board::unused_group_name`)
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง
    pub fn new(targets: Vec<ItemId>, name_base: &str) -> Result<Self, CmdError> {
        if targets.is_empty() {
            return Err(CmdError::Empty);
        }
        Ok(Self {
            targets,
            name: name_base.to_owned(),
            created: None,
            before: Vec::new(),
            swept: Vec::new(),
        })
    }

    /// id ของกลุ่มที่สร้าง — ใช้ได้หลัง `apply` แล้วเท่านั้น
    #[must_use]
    pub fn group_id(&self) -> Option<GroupId> {
        self.created.as_ref().map(|(id, _)| *id)
    }

    /// ★ การจัดกลุ่มครั้งนี้เปลี่ยนอะไรจริงหรือไม่
    ///
    /// "ทุกใบอยู่ในกลุ่มเดียวกันอยู่แล้ว **และ** กลุ่มนั้นไม่มีสมาชิกอื่น" =
    /// กด `Ctrl+G` ซ้ำบนสิ่งที่จัดกลุ่มไว้แล้ว · ถ้ายอมให้ผ่าน ผู้ใช้จะได้กลุ่มใหม่
    /// ที่หน้าตาเหมือนเดิมทุกอย่าง + undo stack ที่มีขั้นซึ่งกดแล้วไม่มีอะไรขยับ
    fn changes_anything(&self, board: &Board) -> bool {
        let mut existing: Option<GroupId> = None;
        for id in &self.targets {
            let Some(item) = board.item(*id) else {
                continue;
            };
            match (item.meta.group, existing) {
                (None, _) => return true,
                (Some(group), None) => existing = Some(group),
                (Some(group), Some(seen)) if group != seen => return true,
                (Some(_), Some(_)) => {}
            }
        }
        let Some(group) = existing else {
            return false; // ไม่มี target ไหนอยู่บน board เลย
        };
        // กลุ่มเดิมมีสมาชิกที่ไม่ได้ถูกเลือกอยู่ด้วย → การจัดกลุ่มแยกออกมามีความหมาย
        board
            .group_members(group)
            .any(|member| !self.targets.contains(&member))
    }
}

impl Command for GroupItems {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let first_time = self.created.is_none();
        if first_time && !self.changes_anything(board) {
            return Err(CmdError::Empty);
        }

        // ★ redo ต้องได้ **id เดิม** ไม่งั้น `ItemMeta::group` ที่คำสั่งถัดไปในสาย
        //   redo เขียนไว้จะชี้ไปที่กลุ่มที่ไม่มีอยู่ (§4 ข้อ 19)
        let group_id = match self.created.as_ref() {
            Some((id, group)) => {
                board.restore_group(*id, group.clone())?;
                *id
            }
            None => {
                let group = Group {
                    name: board.unused_group_name(&self.name),
                    collapsed: false,
                };
                let id = board.insert_group(group.clone());
                self.created = Some((id, group));
                id
            }
        };

        // กลุ่มเดิมของทุกใบ — ผู้สมัครให้เก็บกวาดหลังย้ายเสร็จ
        let mut vacated: Vec<GroupId> = Vec::new();
        let mut done: Vec<(ItemId, ItemMeta)> = Vec::new();
        for id in &self.targets {
            let Some(item) = board.item(*id) else {
                continue;
            };
            let previous = item.meta.clone();
            if previous.group == Some(group_id) {
                continue;
            }
            if let Some(old) = previous.group
                && !vacated.contains(&old)
            {
                vacated.push(old);
            }
            let mut next = previous.clone();
            next.group = Some(group_id);
            // ★ ล้มกลางคันต้องคืนทุกอย่างที่ทำไปแล้ว — `apply` ที่คืน `Err`
            //   ห้ามแตะ board เลย (§2.1 ข้อ 1)
            if let Err(err) = board.set_meta(*id, next) {
                for (undo_id, undo_meta) in done {
                    let _ = board.set_meta(undo_id, undo_meta);
                }
                board.remove_group(group_id);
                if first_time {
                    self.created = None;
                }
                return Err(err.into());
            }
            done.push((*id, previous));
        }

        self.swept = sweep_empty_groups(board, &vacated);
        if first_time {
            self.before = done;
        }
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        // ★ ลำดับสำคัญ: ใส่กลุ่มเดิมกลับ **ก่อน** คืน meta ที่ชี้ไปหามัน
        //   ไม่งั้นระหว่างสองขั้นจะมี item ที่ถือ id ของกลุ่มที่ยังไม่มีอยู่
        restore_swept(board, &self.swept);
        for (id, meta) in &self.before {
            board.set_meta(*id, meta.clone())?;
        }
        if let Some((id, _)) = self.created.as_ref() {
            board.remove_group(*id);
        }
        Ok(())
    }

    /// **ไม่ merge** — จัดกลุ่มหนึ่งครั้ง = undo หนึ่งขั้นเสมอ (เหมือน [`TagItems`])
    fn merge(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn affected(&self) -> Vec<ItemId> {
        self.targets.clone()
    }

    fn label(&self) -> &'static str {
        "Group items"
    }

    fn heap_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.capacity()
            + self.targets.capacity() * std::mem::size_of::<ItemId>()
            + self
                .created
                .as_ref()
                .map_or(0, |(_, group)| group.name.capacity())
            + self
                .before
                .iter()
                .map(|(_, meta)| std::mem::size_of::<(ItemId, ItemMeta)>() + meta.note.capacity())
                .sum::<usize>()
            + self
                .swept
                .iter()
                .map(|(_, group)| std::mem::size_of::<(GroupId, Group)>() + group.name.capacity())
                .sum::<usize>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// เอา item ที่เลือกไว้ออกจากกลุ่มของมัน (`Ctrl+Shift+G` — docs/03 §5)
///
/// ★ กลุ่มที่ไม่เหลือสมาชิกถูกเก็บกวาดทิ้ง และ undo ใส่กลับที่คีย์เดิม
#[derive(Debug)]
pub struct Ungroup {
    targets: Vec<ItemId>,
    before: Vec<(ItemId, ItemMeta)>,
    swept: Vec<(GroupId, Group)>,
}

impl Ungroup {
    /// เอาออกจากกลุ่ม
    ///
    /// # Errors
    /// [`CmdError::Empty`] ถ้ารายการว่าง
    pub fn new(targets: Vec<ItemId>) -> Result<Self, CmdError> {
        if targets.is_empty() {
            return Err(CmdError::Empty);
        }
        Ok(Self {
            targets,
            before: Vec::new(),
            swept: Vec::new(),
        })
    }
}

impl Command for Ungroup {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let first_time = self.before.is_empty();
        let mut vacated: Vec<GroupId> = Vec::new();
        let mut done: Vec<(ItemId, ItemMeta)> = Vec::new();
        for id in &self.targets {
            let Some(item) = board.item(*id) else {
                continue;
            };
            let previous = item.meta.clone();
            let Some(old) = previous.group else {
                continue; // ไม่ได้อยู่ในกลุ่มไหนอยู่แล้ว
            };
            if !vacated.contains(&old) {
                vacated.push(old);
            }
            let mut next = previous.clone();
            next.group = None;
            if let Err(err) = board.set_meta(*id, next) {
                for (undo_id, undo_meta) in done {
                    let _ = board.set_meta(undo_id, undo_meta);
                }
                return Err(err.into());
            }
            done.push((*id, previous));
        }
        // ★ ไม่มีใครอยู่ในกลุ่มเลย = ไม่มีอะไรให้ทำ — กัน undo stack ที่มีขั้นเปล่า
        if first_time && done.is_empty() {
            return Err(CmdError::Empty);
        }
        self.swept = sweep_empty_groups(board, &vacated);
        if first_time {
            self.before = done;
        }
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        restore_swept(board, &self.swept);
        for (id, meta) in &self.before {
            board.set_meta(*id, meta.clone())?;
        }
        Ok(())
    }

    fn affected(&self) -> Vec<ItemId> {
        self.targets.clone()
    }

    fn label(&self) -> &'static str {
        "Ungroup items"
    }

    fn heap_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.targets.capacity() * std::mem::size_of::<ItemId>()
            + self
                .before
                .iter()
                .map(|(_, meta)| std::mem::size_of::<(ItemId, ItemMeta)>() + meta.note.capacity())
                .sum::<usize>()
            + self
                .swept
                .iter()
                .map(|(_, group)| std::mem::size_of::<(GroupId, Group)>() + group.name.capacity())
                .sum::<usize>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// ช่องของ [`Group`] ที่คำสั่งกำลังแก้ — ใช้ตัดสินว่า merge ได้ไหม
///
/// เหตุผลเดียวกับ [`MetaField`]: พิมพ์ชื่อกลุ่มรัว ๆ ควรเป็น undo ขั้นเดียว
/// แต่ "เปลี่ยนชื่อ" แล้ว "ยุบ" ต้องแยกขั้น ไม่งั้นย้อนการยุบแล้วชื่อหายไปด้วย
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupField {
    /// ชื่อกลุ่ม
    Name,
    /// ยุบ/กาง
    Collapsed,
}

/// เปลี่ยนชื่อหรือยุบ/กางกลุ่ม (P3-7)
///
/// ★★ **ทำไมการยุบต้องผ่าน `Command`** — `Group::collapsed` อยู่ใน `Board` จึงเป็น
/// *เอกสาร* ตาม `docs/08 §4` ข้อ 10 · ข้อยกเว้นที่โปรเจกต์นี้ยอมมีแค่สองข้อคือ
/// `view` กับ `dirty` และ HANDOFF สั่งไว้ว่าเจอข้อที่สามให้ **หยุดถาม**
/// ไม่ใช่เจาะรูเพิ่มเอง · ผลข้างเคียงที่ยอมรับ: กดยุบแล้วเอกสาร dirty และ
/// Ctrl+Z ย้อนการยุบได้ ซึ่งสม่ำเสมอกับทุกอย่างอื่นที่ persist ลงไฟล์
#[derive(Debug)]
pub struct SetGroup {
    id: GroupId,
    field: GroupField,
    after: Group,
    /// ค่าเดิม — เก็บ **ตอน apply ครั้งแรกเท่านั้น** เพื่อให้ redo ไม่เขียนทับ
    before: Option<Group>,
}

impl SetGroup {
    /// ตั้งชื่อใหม่
    #[must_use]
    pub fn rename(id: GroupId, current: &Group, name: String) -> Self {
        Self {
            id,
            field: GroupField::Name,
            after: Group {
                name,
                collapsed: current.collapsed,
            },
            before: None,
        }
    }

    /// ยุบหรือกาง
    #[must_use]
    pub fn set_collapsed(id: GroupId, current: &Group, collapsed: bool) -> Self {
        Self {
            id,
            field: GroupField::Collapsed,
            after: Group {
                name: current.name.clone(),
                collapsed,
            },
            before: None,
        }
    }
}

impl Command for SetGroup {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let previous = board.set_group(self.id, self.after.clone())?;
        if self.before.is_none() {
            self.before = Some(previous);
        }
        Ok(())
    }

    fn undo(&mut self, board: &mut Board) -> Result<(), CmdError> {
        let Some(before) = self.before.clone() else {
            return Ok(()); // ยังไม่เคย apply — ไม่มีอะไรให้คืน
        };
        board.set_group(self.id, before)?;
        Ok(())
    }

    fn merge(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };
        if next.id != self.id || next.field != self.field {
            return false;
        }
        self.after.clone_from(&next.after);
        true
    }

    /// ★ **ไม่แตะ item สักใบ** — การเปลี่ยนชื่อ/ยุบกลุ่มไม่ควรไปตั้ง selection ใหม่
    fn affected(&self) -> Vec<ItemId> {
        Vec::new()
    }

    fn label(&self) -> &'static str {
        match self.field {
            GroupField::Name => "Rename group",
            GroupField::Collapsed => "Collapse group",
        }
    }

    fn heap_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.after.name.capacity()
            + self
                .before
                .as_ref()
                .map_or(0, |group| group.name.capacity())
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

    /// ★★★ board ที่ถืออยู่ **ยังไม่เคยอยู่ในไฟล์** — ใช้ตอนกู้คืน snapshot
    ///
    /// [`History::new`] ถือว่า board ที่รับมาคือ "สถานะที่บันทึกไว้แล้ว" ซึ่งถูก
    /// สำหรับการเปิดไฟล์ปกติ · แต่ **ผิดเสมอสำหรับ snapshot ที่กู้คืนมา**:
    /// เนื้อของมันไม่เหมือนอะไรที่อยู่บนดิสก์เลยตามนิยาม
    ///
    /// ถ้าไม่มีตัวนี้ ตัวบ่งชี้ "ยังไม่ถูกบันทึก" (`docs/03 §1`) จะ **ดับ** ทันที
    /// ที่ผู้ใช้กดกู้คืน แล้วเขาจะปิดโปรแกรมโดยเชื่อว่างานอยู่ในไฟล์แล้ว
    /// — วนกลับไปที่เดิมพอดีกับสิ่งที่กลไกกู้คืนทั้งชุดมีไว้กัน
    ///
    /// ★ ไม่มีทางกลับเป็น "บันทึกแล้ว" นอกจาก [`History::mark_saved`] จริง ๆ
    /// (`undo` จนสุดก็ยังนับว่ายังไม่บันทึก เพราะจุดอ้างอิงไม่ได้อยู่ในประวัตินี้)
    pub fn mark_unsaved(&mut self, board: &mut Board) {
        self.saved_depth = None;
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
    use crate::board::{ColorLabel, SortKey, TagId};

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

    // ---------- P4-6: RelinkAssets ----------

    fn missing_kind(path: &str) -> ItemKind {
        ItemKind::Missing {
            original_path: std::path::PathBuf::from(path),
            reason: crate::board::MissingReason::FileNotFound,
        }
    }

    /// ★★★ **relink เปลี่ยนแค่ที่มาของพิกเซล ไม่แตะสิ่งที่ผู้ใช้จัดไว้**
    ///
    /// ตำแหน่ง/ขนาด/หมุน/ครอป/ฟิลเตอร์/แท็ก/ดาว/โน้ต คือ *งาน* ของผู้ใช้
    /// การหาไฟล์เจอไม่ควรแตะมันแม้แต่ค่าเดียว (I-3)
    #[test]
    fn relinking_never_touches_what_the_user_arranged() {
        let mut board = Board::default();
        let id = board.insert_item(image_item(1));
        board
            .set_canvas(id, moved_to(123.0, 456.0))
            .expect("ตั้งตำแหน่งไม่ได้");
        let meta = ItemMeta {
            rating: 4,
            note: "ใช้ใบนี้เป็นหลัก".to_owned(),
            ..ItemMeta::default()
        };
        board.set_meta(id, meta.clone()).expect("ตั้ง meta ไม่ได้");
        let canvas_before = board.item(id).unwrap().canvas;

        let mut history = History::default();
        let command = RelinkAssets::new(&board, vec![(id, missing_kind("1.png"))]).unwrap();
        history.apply(&mut board, Box::new(command)).unwrap();

        let item = board.item(id).unwrap();
        assert!(matches!(item.kind, ItemKind::Missing { .. }), "ที่มาต้องเปลี่ยน");
        assert_eq!(item.canvas, canvas_before, "ตำแหน่ง/ขนาดถูกแตะ");
        assert_eq!(item.meta.rating, 4, "ดาวหาย");
        assert_eq!(item.meta.note, "ใช้ใบนี้เป็นหลัก", "โน้ตหาย");
    }

    /// ★★★ undo ต้องคืน **สภาพก่อนหน้าเป๊ะ** ทั้งสองทิศ
    #[test]
    fn undo_puts_the_old_source_back_exactly() {
        let mut board = Board::default();
        let id = board.insert_item(image_item(3));
        let before = board.item(id).unwrap().kind.clone();
        let mut history = History::default();

        let command = RelinkAssets::new(&board, vec![(id, missing_kind("3.png"))]).unwrap();
        history.apply(&mut board, Box::new(command)).unwrap();
        history.undo(&mut board).unwrap();

        assert_eq!(board.item(id).unwrap().kind, before);

        // และ redo ต้องกลับไปสภาพหลังได้ด้วย
        history.redo(&mut board).unwrap();
        assert!(matches!(
            board.item(id).unwrap().kind,
            ItemKind::Missing { .. }
        ));
    }

    /// ★★ ผลลัพธ์ที่ทยอยกลับมาหลายงวด = **undo ขั้นเดียว**
    ///
    /// เปิดไฟล์ที่มีภาพหาย 200 ใบแล้วผลกลับมาคนละเฟรม ถ้าไม่ merge ผู้ใช้ต้อง
    /// กด `Ctrl+Z` สองร้อยครั้งเพื่อย้อนสิ่งที่เขาเห็นเป็นการกระทำเดียว
    #[test]
    fn results_that_trickle_in_over_many_frames_are_one_undo_step() {
        let mut board = Board::default();
        let a = board.insert_item(image_item(1));
        let b = board.insert_item(image_item(2));
        let before_a = board.item(a).unwrap().kind.clone();
        let before_b = board.item(b).unwrap().kind.clone();
        let mut history = History::default();

        for id in [a, b] {
            let command = RelinkAssets::new(&board, vec![(id, missing_kind("x.png"))]).unwrap();
            history.apply(&mut board, Box::new(command)).unwrap();
        }
        assert_eq!(history.undo_depth(), 1, "ควรยุบเป็นขั้นเดียว");

        history.undo(&mut board).unwrap();
        assert_eq!(board.item(a).unwrap().kind, before_a, "ใบแรกไม่ได้ถูกย้อน");
        assert_eq!(board.item(b).unwrap().kind, before_b, "ใบที่สองไม่ได้ถูกย้อน");
    }

    /// ★★★ **สอง "งวด" ที่คนละเวลา ต้องเป็นคนละขั้น undo** (docs/02 §3 — `seal`)
    ///
    /// การผูกไฟล์ตอนเปิดเอกสาร กับการที่ผู้ใช้กด "หาไฟล์เอง" อีกสิบนาทีต่อมา
    /// เป็นการกระทำคนละครั้งในสายตาเขา · ถ้ารวมกัน `Ctrl+Z` ครั้งเดียวจะย้อน
    /// ทั้งสองเรื่องพร้อมกัน ซึ่งเป็นกับดักเดียวกับที่ `seal` มีไว้กันตอนลากเมาส์
    ///
    /// ★ เจอตอนยืนยันบนแอปจริง (21 ส.ค. 2026): กด `Ctrl+Z` หลัง relink แล้ว
    /// ภาพไม่กลับไปเป็น `Missing` เพราะมันย้อนข้ามไปถึงสภาพตอนเปิดไฟล์
    #[test]
    fn a_sealed_run_is_never_merged_into_the_one_before_it() {
        let mut board = Board::default();
        let id = board.insert_item(image_item(1));
        let mut history = History::default();

        let first = RelinkAssets::new(&board, vec![(id, missing_kind("1.png"))]).unwrap();
        history.apply(&mut board, Box::new(first)).unwrap();
        history.seal(); // ← งวดแรกจบแล้ว (ของจริงเรียกตอนเริ่มงวดถัดไป)
        let after_first = board.item(id).unwrap().kind.clone();

        let second = RelinkAssets::new(&board, vec![(id, image_item(9).kind)]).unwrap();
        history.apply(&mut board, Box::new(second)).unwrap();

        assert_eq!(history.undo_depth(), 2, "สองงวดถูกยุบเป็นขั้นเดียว");
        history.undo(&mut board).unwrap();
        assert_eq!(
            board.item(id).unwrap().kind,
            after_first,
            "undo ย้อนข้ามไปไกลกว่าหนึ่งงวด"
        );
    }

    /// ★★★ merge แล้ว undo ต้องกลับไป **สภาพก่อนเริ่มทั้งชุด** ไม่ใช่สภาพกลางทาง
    ///
    /// item ใบเดียวถูกแก้สองงวด (เจอ → ซ่อมคีย์) · ถ้า merge เก็บ `before` ของ
    /// งวดหลัง การ undo จะคืนสภาพกลางทางที่ผู้ใช้ไม่เคยเห็น
    #[test]
    fn merging_keeps_the_state_from_before_the_whole_run() {
        let mut board = Board::default();
        let id = board.insert_item(image_item(1));
        let original = board.item(id).unwrap().kind.clone();
        let mut history = History::default();

        let first = RelinkAssets::new(&board, vec![(id, missing_kind("1.png"))]).unwrap();
        history.apply(&mut board, Box::new(first)).unwrap();
        let second = RelinkAssets::new(&board, vec![(id, image_item(9).kind)]).unwrap();
        history.apply(&mut board, Box::new(second)).unwrap();

        history.undo(&mut board).unwrap();
        assert_eq!(
            board.item(id).unwrap().kind,
            original,
            "undo คืนสภาพกลางทาง ไม่ใช่สภาพก่อนเริ่ม"
        );
    }

    /// ★★ ไม่มีอะไรเปลี่ยน = **ไม่มีขั้น undo** (เคสปกติของไฟล์ที่ยังอยู่ที่เดิม)
    ///
    /// ถ้าปล่อยผ่าน การเปิดไฟล์ทุกครั้งจะดันขั้นเปล่าเข้า undo stack และทำให้
    /// เอกสาร dirty ทั้งที่ไม่มีอะไรเปลี่ยน — ซึ่ง `docs/02 §2.9` เตือนไว้ตรง ๆ
    #[test]
    fn relinking_a_file_that_did_not_move_is_not_a_change_at_all() {
        let mut board = Board::default();
        let id = board.insert_item(image_item(1));
        let same = board.item(id).unwrap().kind.clone();
        board.mark_dirty(false);

        assert!(matches!(
            RelinkAssets::new(&board, vec![(id, same)]),
            Err(CmdError::Empty)
        ));
        assert!(!board.is_dirty(), "ไม่มีอะไรเปลี่ยนแต่เอกสาร dirty");
    }

    /// ★★★ **โน้ตข้อความห้ามถูก relink ทับ** — ข้อความที่ผู้ใช้พิมพ์จะหายทั้งก้อน
    #[test]
    fn a_text_note_is_never_overwritten_by_a_relink() {
        let mut board = Board::default();
        let note = board.insert_item(Item::new(ItemKind::Text(crate::board::TextNote {
            text: "อย่าลบฉัน".to_owned(),
        })));

        assert!(matches!(
            RelinkAssets::new(&board, vec![(note, image_item(1).kind)]),
            Err(CmdError::Empty)
        ));
        let ItemKind::Text(kept) = &board.item(note).unwrap().kind else {
            panic!("โน้ตถูกเขียนทับ");
        };
        assert_eq!(kept.text, "อย่าลบฉัน");
    }

    /// item ที่ถูกลบไประหว่างที่งานค้นหาเดินอยู่ — ข้ามไป ไม่ใช่ล้มทั้งชุด
    #[test]
    fn an_item_deleted_while_the_search_was_running_is_skipped() {
        let mut board = Board::default();
        let alive = board.insert_item(image_item(1));
        let gone = board.insert_item(image_item(2));
        board.remove_item(gone);

        let command = RelinkAssets::new(
            &board,
            vec![
                (gone, missing_kind("2.png")),
                (alive, missing_kind("1.png")),
            ],
        )
        .unwrap();
        assert_eq!(command.affected(), vec![alive]);
    }

    // ---------- P3-5: ApplyLayout (Arrange → Canvas) ----------

    /// board ที่มีภาพ `n` ใบวางกระจัดกระจาย
    fn scattered(n: u32) -> (Board, History, Vec<ItemId>) {
        let mut board = Board::default();
        let mut history = History::default();
        let items: Vec<Item> = (0..n)
            .map(|i| {
                #[expect(clippy::cast_precision_loss, reason = "จำนวนน้อยในเทสต์")]
                let f = i as f32;
                crate::board::tests::image_item(u8::try_from(i % 250).unwrap_or(0))
                    .at(Vec2::new(f * 37.0, f * 11.0), Vec2::new(80.0, 60.0))
            })
            .collect();
        history
            .apply(&mut board, Box::new(AddItems::new(items).unwrap()))
            .unwrap();
        let ids = board.z_order().to_vec();
        (board, history, ids)
    }

    fn grid_for(board: &Board) -> Vec<Placed> {
        let items: Vec<(ItemId, Vec2)> = board
            .z_order()
            .iter()
            .map(|id| (*id, Vec2::new(4.0, 3.0)))
            .collect();
        crate::layout::layout(
            crate::layout::Engine::Grid,
            &items,
            crate::layout::LayoutParams {
                width: 800.0,
                gap: 12.0,
                columns: Some(4),
                target_row_height: 200.0,
            },
        )
    }

    /// ★★★ เกณฑ์ ROADMAP P3-5: **undo ครั้งเดียวคืนสภาพเดิมครบ**
    ///
    /// จัด 40 ใบใหม่ทั้งกระดานแล้วกด Ctrl+Z หนึ่งครั้ง ต้องได้ board ที่ *เท่ากันทุก
    /// ตัวอักษร* กับก่อนจัด — ไม่ใช่ "ใกล้เคียง" (`Board: PartialEq` เทียบทุกอย่าง
    /// ที่ผู้ใช้สัมผัสได้ · `revision` ไม่นับเพราะมันเดินหน้าอย่างเดียว — §4 ข้อ 20)
    #[test]
    fn one_undo_puts_every_item_back_exactly() {
        let (mut board, mut history, _) = scattered(40);
        let snapshot = board.clone();

        let command = ApplyLayout::from_placed(&board, &grid_for(&board)).unwrap();
        history.apply(&mut board, Box::new(command)).unwrap();
        assert_ne!(board, snapshot, "จัดแล้วต้องมีอะไรขยับจริง");
        assert_eq!(history.undo_depth(), 2, "การจัดต้องเป็น undo ขั้นเดียว");

        history.undo(&mut board).unwrap();
        assert_eq!(board, snapshot, "undo ครั้งเดียวแล้วยังไม่เหมือนเดิม");
    }

    /// ★★ ใบที่ **ปักหมุด** และใบที่ **ล็อก** ต้องไม่ขยับแม้แต่หน่วยเดียว
    #[test]
    fn pinned_and_locked_items_are_never_moved() {
        let (mut board, mut history, ids) = scattered(6);
        let pinned = ids[1];
        let locked = ids[3];
        let meta = ItemMeta {
            pinned: true,
            ..board.item(pinned).unwrap().meta.clone()
        };
        history
            .apply(
                &mut board,
                Box::new(EditMeta::new(MetaField::Pinned, vec![(pinned, meta)]).unwrap()),
            )
            .unwrap();
        let locked_canvas = ItemCanvas {
            locked: true,
            ..board.item(locked).unwrap().canvas
        };
        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(locked, locked_canvas)]).unwrap()),
            )
            .unwrap();

        let before: Vec<ItemCanvas> = ids
            .iter()
            .map(|id| board.item(*id).unwrap().canvas)
            .collect();
        let command = ApplyLayout::from_placed(&board, &grid_for(&board)).unwrap();
        history.apply(&mut board, Box::new(command)).unwrap();

        for (index, id) in ids.iter().enumerate() {
            let now = board.item(*id).unwrap().canvas;
            if *id == pinned || *id == locked {
                assert_eq!(now, before[index], "ใบที่ปักหมุด/ล็อกถูกย้าย");
            } else {
                assert_ne!(now, before[index], "ใบปกติต้องถูกจัดใหม่");
            }
        }
    }

    /// ★ กลุ่มต้องอยู่ที่เดิม — ไม่กระโดดไปมุมซ้ายบนของ world
    ///
    /// `Placed` เริ่มที่ `(0,0)` เสมอ ถ้าเอาไปใส่ตรง ๆ ภาพทั้งกระดานจะย้ายไปที่
    /// ที่ผู้ใช้ไม่ได้มองอยู่ แล้วเขาจะอ่านว่า "กดแล้วภาพหายหมด"
    #[test]
    fn the_group_keeps_the_place_it_already_occupied() {
        let (mut board, mut history, ids) = scattered(9);
        let centre_of = |board: &Board| {
            ids.iter().fold(Rect::EMPTY, |acc, id| {
                acc.union(board.item(*id).unwrap().canvas.world_bounds())
            })
        };
        let before = centre_of(&board).center();

        let command = ApplyLayout::from_placed(&board, &grid_for(&board)).unwrap();
        history.apply(&mut board, Box::new(command)).unwrap();
        let after = centre_of(&board).center();

        assert!(
            (before - after).length() < 0.5,
            "กรอบรวมย้ายจาก {before:?} ไป {after:?}"
        );
    }

    /// ★ การจัดวางแตะแค่ตำแหน่ง/ขนาด — ของที่ผู้ใช้แต่งไว้ต้องไม่ถูกล้าง
    #[test]
    fn applying_a_layout_keeps_crop_flip_and_filters() {
        let (mut board, mut history, ids) = scattered(4);
        let id = ids[0];
        let dressed = ItemCanvas {
            opacity: 0.5,
            flip: crate::board::Flip::Horizontal,
            rotation: 0.3,
            ..board.item(id).unwrap().canvas
        };
        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(id, dressed)]).unwrap()),
            )
            .unwrap();

        let command = ApplyLayout::from_placed(&board, &grid_for(&board)).unwrap();
        history.apply(&mut board, Box::new(command)).unwrap();

        let now = board.item(id).unwrap().canvas;
        assert!((now.opacity - 0.5).abs() < 1e-6, "opacity หาย");
        assert_eq!(now.flip, crate::board::Flip::Horizontal, "flip หาย");
        assert!((now.rotation - 0.3).abs() < 1e-6, "การหมุนหาย");
    }

    /// ★ จัดแล้วทุกใบอยู่ที่เดิมอยู่แล้ว = **ไม่มีคำสั่ง** (ไม่กิน undo เปล่า)
    #[test]
    fn a_layout_that_changes_nothing_produces_no_command() {
        let (mut board, mut history, _) = scattered(5);
        let placed = grid_for(&board);
        let command = ApplyLayout::from_placed(&board, &placed).unwrap();
        history.apply(&mut board, Box::new(command)).unwrap();
        let depth = history.undo_depth();

        // จัดซ้ำด้วยผลเดิมเป๊ะ — ไม่มีอะไรขยับแล้ว
        assert!(
            ApplyLayout::from_placed(&board, &placed).is_none(),
            "จัดซ้ำแล้วยังสร้างคำสั่งที่ไม่ทำอะไร"
        );
        assert_eq!(history.undo_depth(), depth);
    }

    /// board ที่มีแต่ใบที่ปักหมุด → ไม่มีอะไรให้จัด
    #[test]
    fn a_board_of_pinned_items_yields_no_command() {
        let (mut board, mut history, ids) = scattered(3);
        for id in &ids {
            let meta = ItemMeta {
                pinned: true,
                ..board.item(*id).unwrap().meta.clone()
            };
            history
                .apply(
                    &mut board,
                    Box::new(EditMeta::new(MetaField::Pinned, vec![(*id, meta)]).unwrap()),
                )
                .unwrap();
        }
        assert!(ApplyLayout::from_placed(&board, &grid_for(&board)).is_none());
    }

    /// ★★ จัดสองครั้งติดกัน = **สอง** ขั้น undo ไม่ใช่ขั้นเดียว
    ///
    /// ผู้ใช้ลอง Grid แล้วลอง Masonry ต่อ = สองการตัดสินใจ · ถ้ายุบเป็นขั้นเดียว
    /// กด Ctrl+Z จะข้ามกลับไปสภาพก่อนจัดครั้งแรกเลย ซึ่งเขาไม่ได้ขอ
    #[test]
    fn two_layouts_in_a_row_are_two_undo_steps() {
        let (mut board, mut history, _) = scattered(8);
        let before = history.undo_depth();

        let grid = ApplyLayout::from_placed(&board, &grid_for(&board)).unwrap();
        history.apply(&mut board, Box::new(grid)).unwrap();

        let items: Vec<(ItemId, Vec2)> = board
            .z_order()
            .iter()
            .map(|id| (*id, Vec2::new(3.0, 4.0)))
            .collect();
        let masonry = crate::layout::layout(
            crate::layout::Engine::Masonry,
            &items,
            crate::layout::LayoutParams::default(),
        );
        let second = ApplyLayout::from_placed(&board, &masonry).unwrap();
        history.apply(&mut board, Box::new(second)).unwrap();

        assert_eq!(history.undo_depth(), before + 2, "สองการจัดถูกยุบเป็นขั้นเดียว");
    }

    /// id ที่ตายแล้วต้องไม่ทำให้ล้ม — layout อาจมาจากเฟรมก่อนที่ผู้ใช้เพิ่งลบภาพ
    #[test]
    fn a_layout_that_mentions_a_dead_id_still_works() {
        let (mut board, mut history, ids) = scattered(4);
        let mut placed = grid_for(&board);
        history
            .apply(
                &mut board,
                Box::new(RemoveItems::new(vec![ids[0]]).unwrap()),
            )
            .unwrap();
        placed.push(Placed {
            id: ids[0],
            top_left: Vec2::ZERO,
            size: Vec2::splat(10.0),
        });
        let command = ApplyLayout::from_placed(&board, &placed).expect("ที่เหลือยังจัดได้");
        history.apply(&mut board, Box::new(command)).unwrap();
        assert!(board.item(ids[0]).is_none(), "ใบที่ลบไปแล้วต้องไม่ฟื้น");
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

    /// ★★★ **งานที่กู้คืนมาต้องไม่ถูกนับว่าบันทึกแล้ว** (`docs/03 §1` · I-3)
    ///
    /// `History::new` ถือว่า board ที่รับมา = สถานะที่บันทึกไว้แล้ว ซึ่งถูกสำหรับ
    /// การเปิดไฟล์ แต่ **ผิดเสมอสำหรับ snapshot**: เนื้อของมันไม่เหมือนอะไรบนดิสก์
    /// เลยตามนิยาม · ถ้าธงนี้ดับ ตัวบ่งชี้ถาวรบนแท็บจะบอกว่า "ทุกอย่างอยู่ในไฟล์แล้ว"
    /// แล้วผู้ใช้จะปิดโปรแกรมทิ้งอีกรอบ — วนกลับไปที่เดิมพอดี
    #[test]
    fn work_brought_back_from_a_snapshot_is_never_reported_as_saved() {
        let (mut board, ids) = board_with(1);
        let mut history = History::default();
        // สภาพตอนเพิ่งโหลด snapshot ขึ้นมา: ประวัติว่าง ธงยังสะอาด (นี่คือกับดัก)
        assert!(!board.is_dirty(), "จุดเริ่มต้นของเทสต์ต้องคือสภาพที่ดูสะอาด");

        history.mark_unsaved(&mut board);
        assert!(board.is_dirty(), "งานที่กู้คืนมาถูกนับว่าบันทึกแล้ว");

        // ★ แก้แล้วย้อนจนสุดก็ยัง **ไม่** สะอาด — จุดอ้างอิงไม่ได้อยู่ในประวัตินี้
        history
            .apply(
                &mut board,
                Box::new(TransformItems::new(vec![(ids[0], moved_to(10.0, 0.0))]).unwrap()),
            )
            .unwrap();
        history.undo(&mut board).unwrap();
        assert!(
            board.is_dirty(),
            "ย้อนจนสุดแล้วธงดับ — ผู้ใช้จะปิดโปรแกรมโดยเชื่อว่างานอยู่ในไฟล์"
        );

        // ทางเดียวที่ธงดับได้คือบันทึกจริง
        history.mark_saved(&mut board);
        assert!(!board.is_dirty(), "บันทึกจริงแล้วต้องสะอาด");
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

    // ---------- TagItems (P3-1) ----------

    fn tag_board() -> (Board, Vec<ItemId>, History) {
        let mut board = Board::default();
        let ids = (0..3)
            .map(|i| board.insert_item(crate::board::tests::image_item(i)))
            .collect();
        board.mark_dirty(false);
        (board, ids, History::default())
    }

    fn tags_of(board: &Board, id: ItemId) -> Vec<String> {
        board.item(id).map_or_else(Vec::new, |item| {
            item.meta
                .tags
                .iter()
                .filter_map(|tag| board.tags().name(*tag).map(str::to_owned))
                .collect()
        })
    }

    /// ★★ ติดแท็กใหม่ = สร้างชื่อ + ติดให้ทุกใบ **ในขั้น undo เดียว**
    ///
    /// ถ้าแยกเป็นสองคำสั่ง กด Ctrl+Z ครั้งเดียวจะได้ item ที่ถือ `TagId`
    /// ซึ่งไม่มีชื่อในตารางแล้ว — ผู้ใช้เห็นแท็กว่างเปล่าที่กดดูแล้วไม่มีอะไร
    #[test]
    fn tagging_creates_the_name_and_undoes_both_together() {
        let (mut board, ids, mut history) = tag_board();
        history
            .apply(
                &mut board,
                Box::new(TagItems::attach("Portrait", ids.clone()).unwrap()),
            )
            .unwrap();

        assert_eq!(board.tags().len(), 1);
        for id in &ids {
            assert_eq!(tags_of(&board, *id), vec!["Portrait".to_owned()]);
        }

        history.undo(&mut board).unwrap();
        assert_eq!(board.tags().len(), 0, "ชื่อแท็กต้องหายไปพร้อมกัน");
        for id in &ids {
            assert!(tags_of(&board, *id).is_empty());
        }
    }

    /// ★★ undo ต้อง **ไม่** ลบแท็กที่มีอยู่ก่อนแล้ว — ภาพอื่นยังใช้มันอยู่
    #[test]
    fn undoing_never_deletes_a_tag_that_existed_before() {
        let (mut board, ids, mut history) = tag_board();
        history
            .apply(
                &mut board,
                Box::new(TagItems::attach("Ref", vec![ids[0]]).unwrap()),
            )
            .unwrap();
        history.seal();
        history
            .apply(
                &mut board,
                Box::new(TagItems::attach("Ref", vec![ids[1]]).unwrap()),
            )
            .unwrap();

        // ย้อนครั้งที่สอง — ชื่อต้องยังอยู่เพราะใบแรกยังใช้
        history.undo(&mut board).unwrap();
        assert_eq!(board.tags().len(), 1, "แท็กที่ใบอื่นยังใช้อยู่ต้องไม่ถูกลบ");
        assert_eq!(tags_of(&board, ids[0]), vec!["Ref".to_owned()]);
        assert!(tags_of(&board, ids[1]).is_empty());
    }

    /// ★ redo ต้องได้ **`TagId` ตัวเดิม** ไม่ใช่ id ใหม่
    ///
    /// หลักการเดียวกับ `Arena::insert_at` (HANDOFF §4 ข้อ 19): ถ้า redo แจก id ใหม่
    /// คำสั่งอื่นในสายที่ถือ id เดิมจะชี้ไปที่ว่าง
    #[test]
    fn redoing_reuses_the_same_tag_id() {
        let (mut board, ids, mut history) = tag_board();
        history
            .apply(
                &mut board,
                Box::new(TagItems::attach("Lighting", ids.clone()).unwrap()),
            )
            .unwrap();
        let first = board.tags().find("Lighting").unwrap();

        history.undo(&mut board).unwrap();
        history.redo(&mut board).unwrap();

        assert_eq!(board.tags().find("Lighting"), Some(first), "id ต้องเป็นตัวเดิม");
        for id in &ids {
            assert_eq!(tags_of(&board, *id), vec!["Lighting".to_owned()]);
        }
    }

    /// ★ ชื่อเดียวกันคนละตัวพิมพ์ = แท็กเดียวกัน
    ///
    /// ผู้ใช้พิมพ์เองทุกครั้ง แท็กสองอันที่หน้าตาเหมือนกันคือกับดักที่ทำให้ filter หาไม่เจอ
    #[test]
    fn tag_names_are_matched_without_case() {
        let (mut board, ids, mut history) = tag_board();
        history
            .apply(
                &mut board,
                Box::new(TagItems::attach("Portrait", vec![ids[0]]).unwrap()),
            )
            .unwrap();
        history.seal();
        history
            .apply(
                &mut board,
                Box::new(TagItems::attach("  portrait  ", vec![ids[1]]).unwrap()),
            )
            .unwrap();
        assert_eq!(board.tags().len(), 1, "ต้องไม่สร้างแท็กซ้ำ");
        assert_eq!(tags_of(&board, ids[1]), vec!["Portrait".to_owned()]);
    }

    /// ถอดแท็กที่ไม่มีอยู่ต้องเงียบ ไม่ใช่สร้างชื่อขึ้นมา
    #[test]
    fn detaching_a_tag_that_does_not_exist_creates_nothing() {
        let (mut board, ids, mut history) = tag_board();
        history
            .apply(
                &mut board,
                Box::new(TagItems::detach("nope", ids.clone()).unwrap()),
            )
            .unwrap();
        assert_eq!(board.tags().len(), 0);
    }

    /// ★ ติดแท็กซ้ำต้องไม่ทำให้มีสองอันในใบเดียว
    #[test]
    fn attaching_twice_does_not_duplicate() {
        let (mut board, ids, mut history) = tag_board();
        for _ in 0..3 {
            history
                .apply(
                    &mut board,
                    Box::new(TagItems::attach("dup", vec![ids[0]]).unwrap()),
                )
                .unwrap();
            history.seal();
        }
        assert_eq!(tags_of(&board, ids[0]), vec!["dup".to_owned()]);
    }

    /// I-4: ชื่อที่มาจากไฟล์ต้องถูกตัดความยาวและห้ามตัดกลางอักขระ UTF-8
    #[test]
    fn tag_names_are_clamped_without_splitting_characters() {
        let long: String = "ก".repeat(500);
        let normalized = crate::board::TagTable::normalize(&long).unwrap();
        assert_eq!(normalized.chars().count(), crate::board::MAX_TAG_LEN);
        assert!(crate::board::TagTable::normalize("   ").is_none());
        assert!(crate::board::TagTable::normalize("").is_none());
    }

    /// ★ รายการแท็กต้องเรียงเหมือนเดิมทุกครั้ง (CLAUDE.md: ห้าม HashMap order)
    #[test]
    fn the_tag_list_is_always_in_the_same_order() {
        let (mut board, ids, mut history) = tag_board();
        for name in ["zebra", "alpha", "middle"] {
            history
                .apply(
                    &mut board,
                    Box::new(TagItems::attach(name, vec![ids[0]]).unwrap()),
                )
                .unwrap();
            history.seal();
        }
        let first: Vec<String> = board.tags().iter().map(|(_, n)| n.to_owned()).collect();
        for _ in 0..20 {
            let again: Vec<String> = board.tags().iter().map(|(_, n)| n.to_owned()).collect();
            assert_eq!(again, first);
        }
        // เรียงตาม id = ลำดับที่ผู้ใช้สร้าง ไม่ใช่ลำดับตัวอักษร
        assert_eq!(first, vec!["zebra", "alpha", "middle"]);
    }
    // ---------- P3-7: group / ungroup ----------

    /// board สามใบ + `History` สะอาด
    fn group_board() -> (Board, Vec<ItemId>, History) {
        let (board, ids) = board_with(3);
        (board, ids, History::default())
    }

    /// จัดกลุ่มผ่าน `History` แล้วคืน `GroupId` ที่เพิ่งสร้าง
    fn grouped(
        board: &mut Board,
        history: &mut History,
        targets: Vec<ItemId>,
    ) -> crate::arena::GroupId {
        let first = targets[0];
        history
            .apply(board, Box::new(GroupItems::new(targets, "Group").unwrap()))
            .unwrap();
        history.seal();
        board.item(first).unwrap().meta.group.unwrap()
    }

    /// ★★★ จัดกลุ่มแล้ว undo ต้องคืน **ทั้ง board** ให้เท่าเดิมเป๊ะ
    ///
    /// ไม่ใช่แค่ `ItemMeta::group` — กลุ่มที่ถูกสร้างต้องหายไปจาก `Board::groups`
    /// ด้วย ไม่งั้นผู้ใช้กด Ctrl+Z แล้วยังเหลือกลุ่มเปล่าค้างอยู่ในแผงตลอดไป
    #[test]
    fn undoing_a_group_leaves_no_trace_of_it() {
        let (mut board, ids, mut history) = group_board();
        let before = board.clone();

        history
            .apply(
                &mut board,
                Box::new(GroupItems::new(vec![ids[0], ids[1]], "Group").unwrap()),
            )
            .unwrap();
        assert_eq!(board.groups().len(), 1, "ต้องมีกลุ่มเดียวหลังจัดกลุ่ม");
        assert!(board.item(ids[0]).unwrap().meta.group.is_some());

        history.undo(&mut board).unwrap();
        assert_eq!(board, before, "undo ต้องคืนสภาพเป๊ะ รวมถึงกลุ่มที่สร้างขึ้น");
        assert_eq!(board.groups().len(), 0);
    }

    /// ★★ redo ต้องได้ **`GroupId` เดิม** ไม่ใช่ id ใหม่
    ///
    /// เหตุผลเดียวกับ `Arena::insert_at` ของ item (§4 ข้อ 19): `ItemMeta::group`
    /// ถือคีย์นี้อยู่ ถ้า redo แจกคีย์ใหม่ สมาชิกจะชี้ไปที่กลุ่มที่ไม่มีอยู่
    #[test]
    fn redo_puts_the_group_back_at_the_same_key() {
        let (mut board, ids, mut history) = group_board();
        history
            .apply(
                &mut board,
                Box::new(GroupItems::new(vec![ids[0], ids[1]], "Group").unwrap()),
            )
            .unwrap();
        let first = board.item(ids[0]).unwrap().meta.group.unwrap();

        history.undo(&mut board).unwrap();
        history.redo(&mut board).unwrap();

        let again = board.item(ids[0]).unwrap().meta.group.unwrap();
        assert_eq!(again, first, "redo แจก GroupId ใหม่ = สมาชิกห้อย");
        assert!(board.group(again).is_some(), "กลุ่มต้องมีอยู่จริงหลัง redo");
        assert_eq!(board.item(ids[1]).unwrap().meta.group, Some(first));
    }

    /// ★★★ ย้ายสมาชิกออกจนกลุ่มเดิมว่าง → กลุ่มเดิมต้องถูกเก็บกวาด
    /// **และ undo ต้องเอามันกลับมาที่คีย์เดิม**
    ///
    /// กลุ่มเปล่าลบเองไม่ได้เลย (ไม่มีสมาชิกให้เลือกไปสั่ง ungroup) จึงเป็นขยะ
    /// ถาวรที่ถูก persist ลง `.refx` ไปด้วย
    #[test]
    fn a_group_left_empty_is_swept_and_comes_back_on_undo() {
        let (mut board, ids, mut history) = group_board();
        let first = grouped(&mut board, &mut history, vec![ids[0], ids[1]]);
        let before = board.clone();

        // ★ ต้องดึงใบที่สาม (ยังไม่มีกลุ่ม) เข้ามาด้วย ไม่งั้นคำสั่งถูกปฏิเสธเป็น
        //   Empty เพราะชุดนี้ *เป็น* กลุ่มเดิมอยู่แล้วพอดี (ดู `changes_anything`)
        //   — ผลข้างเคียงที่ตั้งใจ และเป็นเหตุผลที่เทสต์นี้ใช้ทั้งสามใบ
        history
            .apply(
                &mut board,
                Box::new(GroupItems::new(vec![ids[0], ids[1], ids[2]], "Group").unwrap()),
            )
            .unwrap();
        assert_eq!(board.groups().len(), 1, "กลุ่มเดิมที่ว่างต้องถูกเก็บกวาด");
        assert!(board.group(first).is_none());

        history.undo(&mut board).unwrap();
        assert_eq!(board, before, "undo ต้องคืนกลุ่มที่ถูกเก็บกวาดกลับมาทั้งก้อน");
        assert!(board.group(first).is_some(), "ต้องกลับมาที่ GroupId เดิม");
    }

    /// กลุ่มเดิมที่ยัง **เหลือสมาชิกคนอื่น** ต้องไม่ถูกเก็บกวาด
    #[test]
    fn a_group_that_still_has_members_survives() {
        let (mut board, ids, mut history) = group_board();
        let first = grouped(&mut board, &mut history, vec![ids[0], ids[1], ids[2]]);

        history
            .apply(
                &mut board,
                Box::new(GroupItems::new(vec![ids[0]], "Group").unwrap()),
            )
            .unwrap();

        assert!(board.group(first).is_some(), "ยังเหลือสองใบ ห้ามลบ");
        assert_eq!(board.groups().len(), 2);
        assert_eq!(board.item(ids[1]).unwrap().meta.group, Some(first));
    }

    /// ★ กด `Ctrl+G` ซ้ำบนสิ่งที่จัดกลุ่มไว้แล้ว = ไม่มีอะไรเปลี่ยน = ไม่สร้างคำสั่ง
    ///
    /// ถ้าปล่อยผ่าน undo stack จะมีขั้นที่กดแล้วผู้ใช้ไม่เห็นอะไรขยับเลย
    #[test]
    fn regrouping_the_exact_same_group_is_refused() {
        let (mut board, ids, mut history) = group_board();
        grouped(&mut board, &mut history, vec![ids[0], ids[1]]);

        let mut cmd = GroupItems::new(vec![ids[0], ids[1]], "Group").unwrap();
        assert_eq!(cmd.apply(&mut board), Err(CmdError::Empty));

        // ★ แต่ถ้ากลุ่มเดิมมีสมาชิกคนอื่นอยู่ด้วย การแยกออกมามีความหมายจริง
        let mut split = GroupItems::new(vec![ids[0]], "Group").unwrap();
        assert!(split.apply(&mut board).is_ok());
    }

    /// ungroup แล้ว undo ต้องคืนทั้งสมาชิกและกลุ่มที่ถูกเก็บกวาด
    #[test]
    fn ungrouping_then_undoing_restores_everything() {
        let (mut board, ids, mut history) = group_board();
        grouped(&mut board, &mut history, vec![ids[0], ids[1]]);
        let before = board.clone();

        history
            .apply(
                &mut board,
                Box::new(Ungroup::new(vec![ids[0], ids[1]]).unwrap()),
            )
            .unwrap();
        assert_eq!(board.item(ids[0]).unwrap().meta.group, None);
        assert_eq!(board.groups().len(), 0, "กลุ่มที่ว่างลงต้องถูกเก็บกวาด");

        history.undo(&mut board).unwrap();
        assert_eq!(board, before, "undo ของ ungroup ต้องคืนสภาพเป๊ะ");
    }

    /// ungroup สิ่งที่ไม่ได้อยู่ในกลุ่มไหนเลย = ไม่มีอะไรให้ทำ
    #[test]
    fn ungrouping_loose_items_is_refused() {
        let (mut board, ids, _history) = group_board();
        let mut cmd = Ungroup::new(vec![ids[0], ids[1]]).unwrap();
        assert_eq!(cmd.apply(&mut board), Err(CmdError::Empty));
    }

    /// ★ เปลี่ยนชื่อรัว ๆ ยุบเป็น undo ขั้นเดียว แต่ **ห้ามยุบข้ามช่อง**
    ///
    /// ถ้า merge ข้ามช่อง การกดยุบหลังเปลี่ยนชื่อจะทำให้ Ctrl+Z ครั้งเดียว
    /// เสียชื่อที่พิมพ์ไปด้วย ทั้งที่ผู้ใช้ตั้งใจย้อนแค่การยุบ
    #[test]
    fn renaming_merges_but_never_across_fields() {
        let (mut board, ids, mut history) = group_board();
        let id = grouped(&mut board, &mut history, vec![ids[0], ids[1]]);
        let depth = history.undo_depth();

        for name in ["a", "ab", "abc"] {
            let current = board.group(id).unwrap().clone();
            history
                .apply(
                    &mut board,
                    Box::new(SetGroup::rename(id, &current, name.to_owned())),
                )
                .unwrap();
        }
        assert_eq!(history.undo_depth(), depth + 1, "พิมพ์ชื่อรัว ๆ = undo ขั้นเดียว");
        assert_eq!(board.group(id).unwrap().name, "abc");

        // ยุบ = ขั้นใหม่ ไม่ใช่ขั้นเดิม
        let current = board.group(id).unwrap().clone();
        history
            .apply(
                &mut board,
                Box::new(SetGroup::set_collapsed(id, &current, true)),
            )
            .unwrap();
        assert_eq!(history.undo_depth(), depth + 2, "ยุบต้องเป็นคนละขั้นกับเปลี่ยนชื่อ");

        history.undo(&mut board).unwrap();
        let after = board.group(id).unwrap();
        assert!(!after.collapsed, "ย้อนการยุบ");
        assert_eq!(after.name, "abc", "ย้อนการยุบต้องไม่กินชื่อไปด้วย");
    }

    /// ★★★ กลุ่มที่ยุบอยู่โผล่ในแผ่น Arrange **ใบเดียว** และกางแล้วกลับมาครบ
    ///
    /// นี่คือสิ่งที่ทำให้ `Group::collapsed` เป็นฟิลด์ที่ *ทำงาน* ไม่ใช่ฟิลด์ที่
    /// ถูกเขียนแล้วไม่มีใครอ่าน — ปุ่มที่กดแล้วไม่มีอะไรเกิดขึ้นโกหกผู้ใช้
    #[test]
    fn a_collapsed_group_shows_exactly_one_tile() {
        use crate::query::{Filter, select};

        let (mut board, ids, mut history) = group_board();
        let id = grouped(&mut board, &mut history, vec![ids[0], ids[1]]);
        let filter = Filter::default();

        let open = select(&board, &filter, SortKey::AddedAt, false);
        assert_eq!(open.len(), 3, "ยังไม่ยุบ = เห็นครบทุกใบ");

        let current = board.group(id).unwrap().clone();
        history
            .apply(
                &mut board,
                Box::new(SetGroup::set_collapsed(id, &current, true)),
            )
            .unwrap();

        let folded = select(&board, &filter, SortKey::AddedAt, false);
        assert_eq!(folded.len(), 2, "ยุบแล้วสมาชิกสองใบเหลือหน้ากลุ่มใบเดียว");
        assert!(folded.contains(&ids[2]), "ใบนอกกลุ่มห้ามหาย");
        assert_eq!(
            folded.iter().filter(|id| ids[..2].contains(id)).count(),
            1,
            "ต้องเหลือสมาชิกใบเดียวพอดี"
        );

        history.undo(&mut board).unwrap();
        assert_eq!(
            select(&board, &filter, SortKey::AddedAt, false).len(),
            3,
            "กางแล้วต้องกลับมาครบ"
        );
    }

    /// ★ `GroupId` ที่ห้อยอยู่ (กลุ่มถูกลบไปแล้ว) ต้อง **ไม่ทำให้ภาพหาย**
    ///
    /// ข้อมูลไม่ครบต้องแปลว่า "แสดงตามปกติ" ไม่ใช่ "ซ่อน" — ภาพที่หายไปเงียบ ๆ
    /// อ่านได้อย่างเดียวว่างานหาย (I-3)
    /// ★★ **ต้องมีกลุ่มที่ยุบอยู่จริงอีกกลุ่มค้างไว้ด้วย** ไม่งั้นเทสต์นี้จับอะไรไม่ได้เลย
    ///
    /// `fold_collapsed_groups` มีทางลัดที่ออกทันทีเมื่อ **ไม่มีกลุ่มไหนยุบอยู่เลย**
    /// — ถ้าเทสต์ลบกลุ่มเดียวที่มีทิ้งไป ทางลัดจะทำงานแล้วผลลัพธ์จะถูก "โดยบังเอิญ"
    /// ไม่ว่าโค้ดตัดสินใจเรื่อง id ที่ห้อยอยู่ถูกหรือผิด · negative control ยืนยันแล้ว:
    /// รุ่นที่ลบกลุ่มเดียวทิ้งยัง**เขียว**ทั้งที่ใส่บั๊กเข้าไปแล้ว รุ่นนี้แดงทันที
    /// (docs/08 §3.9 ข้อ 1 — "เทสต์อ่อน" ไม่ใช่ "ดีไซน์ทำให้พังแบบนั้นไม่ได้")
    #[test]
    fn a_dangling_group_id_never_hides_an_item() {
        use crate::query::{Filter, select};

        let (mut board, ids, mut history) = group_board();
        let doomed = grouped(&mut board, &mut history, vec![ids[0], ids[1]]);
        let survivor = grouped(&mut board, &mut history, vec![ids[2]]);
        for id in [doomed, survivor] {
            let current = board.group(id).unwrap().clone();
            history
                .apply(
                    &mut board,
                    Box::new(SetGroup::set_collapsed(id, &current, true)),
                )
                .unwrap();
            history.seal();
        }
        // ลบกลุ่มทิ้งโดยที่สมาชิกยังถือ id เดิมอยู่ — สภาพที่ไฟล์เสียหายพามาได้
        board.remove_group(doomed);

        let shown = select(&board, &Filter::default(), SortKey::AddedAt, false);
        assert_eq!(
            shown.len(),
            3,
            "กลุ่มที่ไม่มีอยู่ห้ามซ่อนสมาชิก — ต้องเห็นทั้งสองใบที่ห้อย + ใบของกลุ่มที่ยังยุบอยู่"
        );
        assert!(shown.contains(&ids[0]) && shown.contains(&ids[1]));
    }

    /// ★ ชื่ออัตโนมัติต้องไม่ชนกับกลุ่มที่ยังอยู่ แม้จะลบกลุ่มกลางทิ้งไปแล้ว
    #[test]
    fn the_generated_name_never_collides_with_a_living_group() {
        let (mut board, ids, mut history) = group_board();
        let a = grouped(&mut board, &mut history, vec![ids[0]]);
        let b = grouped(&mut board, &mut history, vec![ids[1]]);
        assert_eq!(board.group(a).unwrap().name, "Group 1");
        assert_eq!(board.group(b).unwrap().name, "Group 2");

        // ลบ "Group 1" ทิ้ง แล้วถามชื่อว่าง — ต้องได้ 1 คืน ไม่ใช่ 3
        board.remove_group(a);
        assert_eq!(board.unused_group_name("Group"), "Group 1");
        // และห้ามคืนชื่อที่กลุ่มที่ยังอยู่ใช้อยู่
        assert_ne!(board.unused_group_name("Group"), "Group 2");
    }

    /// สมาชิกของกลุ่มต้องเรียงตามลำดับ z เสมอ ไม่ใช่ลำดับที่ผู้ใช้กดเลือก
    #[test]
    fn group_members_follow_z_order() {
        let (mut board, ids, mut history) = group_board();
        let id = grouped(&mut board, &mut history, vec![ids[2], ids[0]]);
        let members: Vec<ItemId> = board.group_members(id).collect();
        assert_eq!(members, vec![ids[0], ids[2]], "ต้องเป็นลำดับ z ไม่ใช่ลำดับที่กด");
    }
}
