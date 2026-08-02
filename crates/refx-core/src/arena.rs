//! Generational arena — ตัวเก็บ `Item`/`Group`/`Board` และคีย์ที่อ้างถึงมัน
//!
//! ทำไมไม่ใช้ `Vec<Item>` + index ดิบ: ลบตัวที่ 3 แล้ว index 4 กลายเป็นของคนอื่น
//! index ที่ค้างอยู่ในที่อื่น (selection, z-order, undo stack) จะชี้ไปที่ **ภาพผิดตัว**
//! อย่างเงียบ ๆ — บั๊กแบบนี้จะโผล่เป็น "ลบภาพหนึ่งแล้วอีกภาพหายไปด้วย" ซึ่งผู้ใช้
//! อ่านว่า "โปรแกรมทำงานหาย" (docs/02 §1)
//!
//! ทำไมไม่ใช้ `Rc<RefCell<Item>>`: cache-hostile และ `borrow_mut()` ซ้อนกัน = panic
//! ตอน runtime ซึ่งขัด I-7 โดยตรง
//!
//! ★ **ทำไมเขียนเองแทน `slotmap`** (docs/02 §1 เปิดให้เลือกอย่างใดอย่างหนึ่ง):
//! undo ของ "ลบภาพ" ต้องคืน **`ItemId` ตัวเดิมเป๊ะ** ไม่งั้น `z_order`, `Selection`
//! และ `ItemMeta::group` ที่ยังถือ id เก่าอยู่จะชี้ไปที่ว่าง = ภาพกลับมาแต่ลำดับ
//! และการเลือกหายหมด `slotmap` ไม่มี API ใส่กลับที่คีย์เดิม (คีย์ที่ลบแล้วตายถาวร)
//! จึงทำ I-3 ไม่ได้ — [`Arena::insert_at`] คือเหตุผลทั้งหมดที่ไฟล์นี้มีอยู่
//!
//! spec: docs/02-data-model.md §1

use std::marker::PhantomData;

/// คีย์ของ arena — `index` บอกช่อง `generation` บอกว่าเป็น "รุ่นที่เท่าไหร่" ของช่องนั้น
///
/// ทุกชนิดคีย์ ([`ItemId`], [`GroupId`], [`BoardId`]) มีรูปร่างเดียวกันแต่เป็น
/// **คนละชนิดในสายตาคอมไพเลอร์** — ส่ง `GroupId` ไปที่ที่ต้องการ `ItemId` จะไม่ผ่าน
/// การคอมไพล์ ไม่ใช่ไปคืนของผิดตอน runtime
pub trait ArenaKey: Copy + Eq + std::hash::Hash + std::fmt::Debug {
    /// ประกอบคีย์จากช่องและรุ่น
    #[must_use]
    fn from_parts(index: u32, generation: u32) -> Self;
    /// ช่องที่คีย์นี้ชี้ไป
    #[must_use]
    fn index(self) -> u32;
    /// รุ่นของช่องตอนที่คีย์นี้ถูกแจก
    #[must_use]
    fn generation(self) -> u32;
}

/// ประกาศชนิดคีย์ใหม่ที่ implement [`ArenaKey`]
///
/// เขียนเป็นมาโครเพราะทุกชนิดมีเนื้อเหมือนกันเป๊ะ ต่างแค่ชื่อ — ก๊อปด้วยมือ
/// แปลว่าวันหนึ่งจะมีตัวใดตัวหนึ่งถูกแก้ข้างเดียว
macro_rules! arena_key {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name {
            index: u32,
            generation: u32,
        }

        impl ArenaKey for $name {
            fn from_parts(index: u32, generation: u32) -> Self {
                Self { index, generation }
            }
            fn index(self) -> u32 {
                self.index
            }
            fn generation(self) -> u32 {
                self.generation
            }
        }

        /// ย่อเป็น `ชื่อ(ช่องvรุ่น)` — id ที่อ่านไม่ออกทำให้ log ของ undo ไร้ประโยชน์
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({}v{})", stringify!($name), self.index, self.generation)
            }
        }
    };
}

arena_key! {
    /// คีย์ของ `Item` บน board หนึ่ง ๆ
    ItemId
}
arena_key! {
    /// คีย์ของ `Group`
    GroupId
}
arena_key! {
    /// คีย์ของ `Board` ใน workspace
    BoardId
}

/// ใส่ของกลับที่เดิมไม่ได้
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ArenaError {
    /// ช่องนั้นมีของอยู่แล้ว — ถ้าเขียนทับ ของที่อยู่เดิมจะหายไปเงียบ ๆ (ผิด I-3)
    #[error("arena slot {index} is already occupied")]
    SlotOccupied {
        /// ช่องที่ชนกัน
        index: u32,
    },
}

/// ช่องหนึ่งช่องใน arena
///
/// `Vacant` เก็บรุ่น **ถัดไป** ที่ช่องนี้จะแจก — ตัวที่ทำให้ id เก่าตายทันทีที่ลบ
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot<T> {
    Occupied { generation: u32, value: T },
    Vacant { generation: u32 },
}

/// ตัวเก็บของที่อ้างถึงด้วยคีย์ที่ตายเองเมื่อของถูกลบ
#[derive(Debug, Clone)]
pub struct Arena<K, T> {
    slots: Vec<Slot<T>>,
    /// ช่องว่างที่พร้อมใช้ซ้ำ — เรียงตามลำดับที่ถูกคืน (deterministic)
    free: Vec<u32>,
    len: usize,
    _key: PhantomData<fn() -> K>,
}

/// ★ เทียบ **คีย์กับค่าของสิ่งที่ยังมีชีวิต ตามลำดับช่อง** ไม่ใช่ `slots` ดิบ
///
/// นี่คือนิยามของคำว่า "เท่ากัน" ที่ `undo_restores_exactly` ต้องการ: item ชุดเดิม
/// อยู่ใต้ `ItemId` ชุดเดิม รุ่นอยู่ในคีย์ด้วย **ช่องที่ถูกใช้ซ้ำจึงยังนับว่าไม่เท่ากัน**
///
/// สิ่งที่จงใจ *ไม่* นับ: จำนวนช่องว่างท้าย `slots` และคิว `free` — เพิ่มแล้วลบออก
/// ทิ้งช่องว่างไว้หนึ่งช่องเสมอ ถ้านับด้วย undo จะ "ไม่เท่าเดิม" ตลอดกาลทั้งที่
/// ผู้ใช้และโค้ดส่วนอื่นแยกไม่ออกเลย — นั่นคือการวัดตัวจัดสรร ไม่ใช่วัดงานของผู้ใช้
impl<K: ArenaKey, T: PartialEq> PartialEq for Arena<K, T> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().eq(other.iter())
    }
}

impl<K: ArenaKey, T: Eq> Eq for Arena<K, T> {}

impl<K, T> Default for Arena<K, T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
            _key: PhantomData,
        }
    }
}

impl<K: ArenaKey, T> Arena<K, T> {
    /// arena ว่าง
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// จำนวนของที่อยู่ในนี้
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// ว่างหรือไม่
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// ใส่ของใหม่ คืนคีย์ที่อ้างถึงมัน
    pub fn insert(&mut self, value: T) -> K {
        // ใช้ช่องว่างก่อนเสมอ ไม่งั้น arena จะโตตามจำนวนครั้งที่เพิ่ม/ลบ
        // ไม่ใช่ตามจำนวนของที่มีอยู่จริง (ผู้ใช้เพิ่ม-ลบทั้งวันบน board เดียว)
        while let Some(index) = self.free.pop() {
            let Some(Slot::Vacant { generation }) = self.slots.get(index as usize) else {
                // ช่องใน free ต้องว่างเสมอ ถ้าไม่ว่างแปลว่าบัญชีเพี้ยน — ทิ้งใบนั้น
                // แล้วลองใบถัดไป ดีกว่าเขียนทับของที่มีคนอยู่ (I-3)
                continue;
            };
            let generation = *generation;
            self.slots[index as usize] = Slot::Occupied { generation, value };
            self.len += 1;
            return K::from_parts(index, generation);
        }

        let index = u32::try_from(self.slots.len()).unwrap_or(u32::MAX);
        self.slots.push(Slot::Occupied {
            generation: 0,
            value,
        });
        self.len += 1;
        K::from_parts(index, 0)
    }

    /// ★ ใส่ของกลับที่คีย์เดิมเป๊ะ — **เส้นทางของ undo เท่านั้น**
    ///
    /// นี่คือสิ่งที่ทำให้ undo ของ "ลบภาพ" คืนสภาพได้จริง: `z_order`, `Selection`
    /// และ `ItemMeta::group` ที่ยังถือ id เดิมอยู่จะกลับมาใช้ได้ทันทีโดยไม่ต้อง
    /// ไล่แก้ทีละที่ (ซึ่งเป็นการไล่แก้ที่ลืมได้ และลืมเมื่อไหร่คือข้อมูลผู้ใช้เพี้ยน)
    ///
    /// # Errors
    /// [`ArenaError::SlotOccupied`] ถ้าช่องนั้นมีของอยู่ — **ไม่เขียนทับเด็ดขาด**
    /// และ arena ไม่ถูกแตะเลยเมื่อคืน `Err` (ผู้เรียกจึงย้อนกลับได้อย่างปลอดภัย)
    pub fn insert_at(&mut self, key: K, value: T) -> Result<(), ArenaError> {
        let index = key.index() as usize;

        if let Some(Slot::Occupied { .. }) = self.slots.get(index) {
            return Err(ArenaError::SlotOccupied { index: key.index() });
        }

        // ช่องยังไม่มีตัวตน (undo ที่ข้ามช่วงมา) — งอกให้ถึง แล้วยกช่องที่เกิน
        // ระหว่างทางให้เป็นช่องว่างที่ใช้ซ้ำได้ ไม่ใช่หลุมที่ไม่มีใครแตะได้อีก
        while self.slots.len() <= index {
            let hole = u32::try_from(self.slots.len()).unwrap_or(u32::MAX);
            self.slots.push(Slot::Vacant { generation: 0 });
            if hole as usize != index {
                self.free.push(hole);
            }
        }

        // ★ ตั้งรุ่น **ย้อนกลับ** ไปเท่าตอนที่คีย์ถูกแจก — ตรงนี้คือหัวใจ
        //   ตอน remove เราบวกรุ่นไปหนึ่งเพื่อฆ่า id เก่า undo จึงต้องถอยกลับ
        //   ปลอดภัยเพราะช่องนี้ว่าง = ไม่มีของของใครให้ทับ และ id รุ่นระหว่างกลาง
        //   (ถ้ามี) ก็ยังไม่ตรงกับรุ่นที่เพิ่งตั้ง
        self.slots[index] = Slot::Occupied {
            generation: key.generation(),
            value,
        };
        self.free.retain(|&free| free as usize != index);
        self.len += 1;
        Ok(())
    }

    /// เอาของออก คืนของที่เอาออกมา — `None` ถ้าคีย์ตายไปแล้ว
    ///
    /// รุ่นของช่องถูกบวกหนึ่งทันที **id ทุกใบที่ชี้มาที่นี่จึงตายพร้อมกันหมด**
    pub fn remove(&mut self, key: K) -> Option<T> {
        let index = key.index() as usize;
        let slot = self.slots.get_mut(index)?;

        let Slot::Occupied { generation, .. } = slot else {
            return None;
        };
        if *generation != key.generation() {
            return None; // id เก่าที่ชี้มาที่ช่องซึ่งถูกใช้ซ้ำไปแล้ว
        }

        // รุ่นถัดไปของช่องนี้ ถ้าชนเพดาน u32 ให้ **ปลดระวางช่องไปเลย**
        // ไม่เอากลับมาใช้ซ้ำ — ถ้าปล่อยให้ wrap รอบไปเป็น 0 id เก่าที่เก็บไว้
        // ตั้งแต่รุ่นแรกจะกลับมามีชีวิตและชี้ไปที่ของคนอื่นอย่างเงียบ ๆ
        let next = generation.saturating_add(1);
        let taken = std::mem::replace(slot, Slot::Vacant { generation: next });
        if next != u32::MAX {
            self.free.push(key.index());
        }
        self.len -= 1;

        match taken {
            Slot::Occupied { value, .. } => Some(value),
            Slot::Vacant { .. } => None,
        }
    }

    /// อ่านของที่คีย์นี้ชี้ — `None` ถ้าคีย์ตายไปแล้ว
    #[must_use]
    pub fn get(&self, key: K) -> Option<&T> {
        match self.slots.get(key.index() as usize) {
            Some(Slot::Occupied { generation, value }) if *generation == key.generation() => {
                Some(value)
            }
            _ => None,
        }
    }

    /// แก้ของที่คีย์นี้ชี้ — `None` ถ้าคีย์ตายไปแล้ว
    pub fn get_mut(&mut self, key: K) -> Option<&mut T> {
        match self.slots.get_mut(key.index() as usize) {
            Some(Slot::Occupied { generation, value }) if *generation == key.generation() => {
                Some(value)
            }
            _ => None,
        }
    }

    /// คีย์นี้ยังใช้ได้อยู่ไหม
    #[must_use]
    pub fn contains(&self, key: K) -> bool {
        self.get(key).is_some()
    }

    /// ไล่ดูของทั้งหมดพร้อมคีย์ — **เรียงตามช่องเสมอ**
    ///
    /// ลำดับคงที่สำคัญกับ layout และการ save: `HashMap` iteration order ทำให้ผลลัพธ์
    /// ไม่ deterministic ซึ่ง CLAUDE.md ห้ามไว้ตรง ๆ
    pub fn iter(&self) -> impl Iterator<Item = (K, &T)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| match slot {
                Slot::Occupied { generation, value } => Some((
                    K::from_parts(u32::try_from(index).unwrap_or(u32::MAX), *generation),
                    value,
                )),
                Slot::Vacant { .. } => None,
            })
    }

    /// เหมือน [`Arena::iter`] แต่แก้ค่าได้
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (K, &mut T)> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| match slot {
                Slot::Occupied { generation, value } => Some((
                    K::from_parts(u32::try_from(index).unwrap_or(u32::MAX), *generation),
                    value,
                )),
                Slot::Vacant { .. } => None,
            })
    }

    /// คีย์ทั้งหมดที่ยังมีชีวิต เรียงตามช่อง
    pub fn keys(&self) -> impl Iterator<Item = K> {
        self.iter().map(|(key, _)| key)
    }

    /// ของทั้งหมด เรียงตามช่อง
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.iter().map(|(_, value)| value)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    type Bag = Arena<ItemId, &'static str>;

    #[test]
    fn insert_get_remove() {
        let mut arena = Bag::new();
        assert!(arena.is_empty());

        let a = arena.insert("a");
        let b = arena.insert("b");
        assert_eq!(arena.len(), 2);
        assert_eq!(arena.get(a), Some(&"a"));
        assert_eq!(arena.get(b), Some(&"b"));

        assert_eq!(arena.remove(a), Some("a"));
        assert_eq!(arena.len(), 1);
        assert_eq!(arena.get(a), None);
        assert_eq!(arena.get(b), Some(&"b"), "ลบตัวหนึ่งห้ามกระทบอีกตัว");
    }

    /// ★ เหตุผลทั้งหมดที่ arena นี้มีอยู่: id เก่าต้อง **ตาย** ไม่ใช่ไปอ่านของคนอื่น
    ///
    /// ถ้าข้อนี้ไม่จริง การลบภาพหนึ่งจะทำให้ selection/z-order ที่ยังถือ id เดิม
    /// ชี้ไปที่ภาพที่มาแทนที่ — ผู้ใช้เห็นเป็น "ลบภาพหนึ่ง แล้วอีกภาพเพี้ยนตาม"
    #[test]
    fn a_recycled_slot_never_answers_to_the_old_id() {
        let mut arena = Bag::new();
        let old = arena.insert("เก่า");
        arena.remove(old);

        let new = arena.insert("ใหม่");
        assert_eq!(new.index(), old.index(), "ต้องใช้ช่องเดิมซ้ำ ไม่งั้นเปลือง");
        assert_ne!(new.generation(), old.generation());

        assert_eq!(arena.get(new), Some(&"ใหม่"));
        assert_eq!(arena.get(old), None, "id เก่ายังอ่านของใหม่ได้ = บั๊กเงียบ");
        assert!(!arena.contains(old));
        assert_eq!(arena.remove(old), None, "ลบด้วย id เก่าต้องไม่โดนของใหม่");
        assert_eq!(arena.get(new), Some(&"ใหม่"));
    }

    /// ★ หัวใจของ I-3: undo ของ "ลบ" ต้องคืน **id ตัวเดิม** ไม่ใช่ id ใหม่ที่มีค่าเท่ากัน
    #[test]
    fn insert_at_gives_back_the_exact_same_id() {
        let mut arena = Bag::new();
        let a = arena.insert("a");
        let b = arena.insert("b");

        let taken = arena.remove(a).unwrap();
        assert_eq!(arena.get(a), None);

        arena.insert_at(a, taken).expect("ช่องว่างอยู่ ต้องใส่กลับได้");
        assert_eq!(arena.get(a), Some(&"a"), "id เดิมต้องใช้ได้อีกครั้ง");
        assert_eq!(arena.get(b), Some(&"b"));
        assert_eq!(arena.len(), 2);
    }

    /// ใส่กลับที่ช่องที่มีคนอยู่ ต้อง **ไม่ทับ** และต้องไม่แตะ arena เลย
    #[test]
    fn insert_at_refuses_to_overwrite() {
        let mut arena = Bag::new();
        let a = arena.insert("a");
        let before = arena.clone();

        let err = arena.insert_at(a, "ผู้บุกรุก").unwrap_err();
        assert_eq!(err, ArenaError::SlotOccupied { index: a.index() });
        assert_eq!(arena, before, "คืน Err แล้วสถานะต้องไม่ขยับเลย");
        assert_eq!(arena.get(a), Some(&"a"));
    }

    /// undo ที่ข้ามช่วงมาไกล — ช่องระหว่างทางต้องกลายเป็นช่องว่างที่ใช้ซ้ำได้
    /// ไม่ใช่หลุมที่กินหน่วยความจำไปเปล่า ๆ ตลอดอายุ board
    #[test]
    fn insert_at_grows_and_leaves_reusable_holes() {
        let mut arena = Bag::new();
        let far = ItemId::from_parts(4, 0);
        arena.insert_at(far, "ไกล").unwrap();

        assert_eq!(arena.get(far), Some(&"ไกล"));
        assert_eq!(arena.len(), 1);

        // ช่อง 0..3 ต้องถูกแจกกลับมาใช้ ไม่ใช่ต่อท้ายที่ 5
        let next = arena.insert("ถัดไป");
        assert!(next.index() < 4, "ได้ช่อง {} แทนที่จะใช้หลุม", next.index());
        assert_eq!(arena.len(), 2);
    }

    /// ลำดับการไล่ต้องคงที่เสมอ — layout ที่ไม่ deterministic คือสิ่งที่ CLAUDE.md ห้าม
    #[test]
    fn iteration_order_is_stable_and_by_slot() {
        let mut arena = Bag::new();
        let a = arena.insert("a");
        let b = arena.insert("b");
        let c = arena.insert("c");
        arena.remove(b);
        let d = arena.insert("d"); // ใช้ช่องของ b ซ้ำ

        let first: Vec<_> = arena.iter().map(|(k, v)| (k, *v)).collect();
        let second: Vec<_> = arena.iter().map(|(k, v)| (k, *v)).collect();
        assert_eq!(first, second);
        assert_eq!(first, vec![(a, "a"), (d, "d"), (c, "c")]);
        assert_eq!(arena.keys().collect::<Vec<_>>(), vec![a, d, c]);
    }

    /// ★ ช่องที่รุ่นชนเพดาน u32 ต้องถูกปลดระวาง ไม่ใช่ wrap กลับไปเป็น 0
    ///
    /// ถ้า wrap id ที่เก็บไว้ตั้งแต่รุ่นแรกจะกลับมามีชีวิตแล้วชี้ของคนอื่น
    /// (ทำจริงต้องลบ 4 พันล้านครั้ง จึงตั้งรุ่นตรง ๆ ในเทสต์แทน)
    #[test]
    fn an_exhausted_slot_is_retired_instead_of_wrapping() {
        let mut arena = Bag::new();
        arena.insert("สุดท้าย");
        arena.slots[0] = Slot::Occupied {
            generation: u32::MAX - 1,
            value: "สุดท้าย",
        };
        let doomed = ItemId::from_parts(0, u32::MAX - 1);

        assert_eq!(arena.remove(doomed), Some("สุดท้าย"));
        assert!(arena.free.is_empty(), "ช่องที่รุ่นหมดต้องไม่ถูกเอากลับมาใช้");

        let fresh = arena.insert("ใหม่");
        assert_ne!(fresh.index(), doomed.index());
        assert_eq!(arena.get(doomed), None);
    }

    /// คีย์จาก arena คนละตัวที่บังเอิญมีเลขเดียวกัน — เป็นข้อจำกัดที่ยอมรับ
    /// เพราะ arena แต่ละตัวเป็นของ board คนละใบและไม่มีเส้นทางที่คีย์ข้ามกัน
    /// เทสต์นี้บันทึกไว้ว่า **รู้อยู่** ไม่ใช่มองข้าม
    #[test]
    fn keys_are_only_meaningful_within_their_own_arena() {
        let mut one = Bag::new();
        let mut two = Bag::new();
        let a = one.insert("หนึ่ง");
        let b = two.insert("สอง");
        assert_eq!(a, b, "เลขเท่ากันได้ — ความหมายผูกกับ arena ที่มันเกิด");
        assert_eq!(one.get(b), Some(&"หนึ่ง"));
    }

    #[test]
    fn debug_shows_slot_and_generation() {
        assert_eq!(format!("{:?}", ItemId::from_parts(7, 3)), "ItemId(7v3)");
        assert_eq!(format!("{:?}", GroupId::from_parts(0, 0)), "GroupId(0v0)");
        assert_eq!(format!("{:?}", BoardId::from_parts(1, 9)), "BoardId(1v9)");
    }

    /// ★ ของเหมือนกันแต่อยู่ใต้ **คีย์คนละรุ่น** = ไม่เท่ากัน
    ///
    /// สำคัญกับ `undo_restores_exactly`: ถ้าเทียบแค่ค่าข้างใน การที่ undo คืนภาพมา
    /// ด้วย `ItemId` ใหม่ (แทนที่จะเป็นตัวเดิม) จะผ่านเทสต์ไปเงียบ ๆ ทั้งที่
    /// `z_order`/`Selection` ที่ถือ id เก่าอยู่พังไปแล้ว
    #[test]
    fn equality_compares_keys_not_just_values() {
        let mut fresh = Bag::new();
        fresh.insert("x");

        let mut recycled = Bag::new();
        let temp = recycled.insert("ชั่วคราว");
        recycled.remove(temp);
        recycled.insert("x");

        assert_eq!(
            fresh.values().collect::<Vec<_>>(),
            recycled.values().collect::<Vec<_>>(),
            "ค่าข้างในเหมือนกัน"
        );
        assert_ne!(fresh, recycled, "แต่คีย์คนละรุ่น = ไม่เท่ากัน");
    }

    /// ★ กลับกัน: เพิ่มแล้วลบออกทิ้งช่องว่างไว้ **ต้องไม่ทำให้ไม่เท่ากัน**
    ///
    /// ถ้านับช่องว่างท้าย `slots` ด้วย undo ของ "เพิ่มภาพ" จะไม่มีวันคืนสภาพได้เลย
    /// ทั้งที่ผู้ใช้และโค้ดส่วนอื่นแยกไม่ออก — จะกลายเป็นการวัดตัวจัดสรรแทนที่จะวัดงาน
    #[test]
    fn leftover_empty_slots_do_not_make_two_arenas_different() {
        let mut untouched = Bag::new();
        untouched.insert("a");

        let mut churned = Bag::new();
        churned.insert("a");
        let temp = churned.insert("ชั่วคราว");
        churned.remove(temp);

        assert_eq!(untouched, churned);
    }
}
