//! `Selection` — สิ่งที่ผู้ใช้เลือกอยู่ **พร้อมลำดับที่คลิก**
//!
//! ใช้ `IndexSet` ไม่ใช่ `HashSet` เพราะคำสั่งจัดเรียง/align ต้องรู้ลำดับและตัวอ้างอิง:
//! "จัดชิดซ้ายตามตัวสุดท้ายที่เลือก" ใช้ไม่ได้เลยถ้าไม่รู้ว่าตัวไหนถูกเลือกทีหลังสุด
//! และ `HashSet` iteration order ยังทำให้ผลลัพธ์ไม่ deterministic ซึ่ง CLAUDE.md ห้าม
//!
//! spec: docs/02-data-model.md §4

use indexmap::IndexSet;

use crate::arena::ItemId;

/// สิ่งที่ถูกเลือกอยู่บน board
///
/// ★ `PartialEq` เทียบ **ตามลำดับ** ไม่ใช่แบบเซต — `IndexSet` เองเทียบแบบเซต
/// ซึ่งจะทำให้ `undo_restores_exactly` ผ่านทั้งที่ลำดับคลิก (และ anchor ของ align)
/// เปลี่ยนไปแล้ว นั่นคือสถานะที่ผู้ใช้สัมผัสได้ จึงต้องนับว่าไม่เท่ากัน
#[derive(Debug, Clone, Default, Eq)]
pub struct Selection {
    items: IndexSet<ItemId>,
    anchor: Option<ItemId>,
}

impl PartialEq for Selection {
    fn eq(&self, other: &Self) -> bool {
        self.anchor == other.anchor && self.items.iter().eq(other.items.iter())
    }
}

impl Selection {
    /// ไม่มีอะไรถูกเลือก
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// เลือกตัวเดียว ทิ้งของเดิมทั้งหมด (คลิกธรรมดา)
    pub fn select(&mut self, id: ItemId) {
        self.items.clear();
        self.items.insert(id);
        self.anchor = Some(id);
    }

    /// เพิ่มเข้าไปในสิ่งที่เลือกอยู่ (Ctrl+คลิก) — ตัวที่เพิ่มล่าสุดเป็น anchor
    pub fn add(&mut self, id: ItemId) {
        self.items.insert(id);
        self.anchor = Some(id);
    }

    /// เอาออกจากการเลือก
    ///
    /// ใช้ `shift_remove` ไม่ใช่ `swap_remove` — `swap_remove` ย้ายตัวท้ายมาแทนที่
    /// ทำให้ลำดับคลิกของคนอื่นเพี้ยนไปด้วย ซึ่งเป็นสถานะที่ผู้ใช้สัมผัสได้
    pub fn remove(&mut self, id: ItemId) {
        self.items.shift_remove(&id);
        if self.anchor == Some(id) {
            // anchor ที่ถูกเอาออกต้องตกไปที่ตัวล่าสุดที่ยังเหลือ ไม่ใช่ค้างเป็น id ตาย
            self.anchor = self.items.last().copied();
        }
    }

    /// สลับสถานะการเลือก (Ctrl+คลิกซ้ำ)
    pub fn toggle(&mut self, id: ItemId) {
        if self.items.contains(&id) {
            self.remove(id);
        } else {
            self.add(id);
        }
    }

    /// ล้างการเลือกทั้งหมด
    pub fn clear(&mut self) {
        self.items.clear();
        self.anchor = None;
    }

    /// เลือกทั้งชุดตามลำดับที่ให้มา — anchor ตกไปที่ตัวท้าย
    pub fn set_all(&mut self, ids: impl IntoIterator<Item = ItemId>) {
        self.items = ids.into_iter().collect();
        self.anchor = self.items.last().copied();
    }

    /// ★ คืนสภาพทั้งชุด **รวม anchor** — เส้นทางของ undo
    ///
    /// [`Selection::set_all`] อย่างเดียวไม่พอ เพราะมันเดา anchor เป็นตัวท้ายเสมอ
    /// ซึ่งอาจไม่ใช่ตัวที่ผู้ใช้คลิกไว้จริงก่อน undo — แล้ว align ครั้งถัดไป
    /// จะอ้างอิงภาพผิดตัวโดยไม่มีอะไรส่งเสียง
    ///
    /// anchor ที่ไม่ได้อยู่ในชุดถูกปฏิเสธ (กลายเป็นตัวท้าย) — ไม่มีทางเกิดจาก
    /// เส้นทางปกติ แต่ค่าจากไฟล์เชื่อไม่ได้ (I-4)
    pub fn restore(&mut self, ids: impl IntoIterator<Item = ItemId>, anchor: Option<ItemId>) {
        self.set_all(ids);
        if let Some(anchor) = anchor
            && self.items.contains(&anchor)
        {
            self.anchor = Some(anchor);
        }
    }

    /// ตัวนี้ถูกเลือกอยู่ไหม
    #[must_use]
    pub fn contains(&self, id: ItemId) -> bool {
        self.items.contains(&id)
    }

    /// จำนวนที่เลือกอยู่
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// ไม่มีอะไรถูกเลือกเลยหรือไม่
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// ตัวอ้างอิงของคำสั่ง align — **ตัวสุดท้ายที่เลือก**
    #[must_use]
    pub fn anchor(&self) -> Option<ItemId> {
        self.anchor
    }

    /// ไล่ตามลำดับที่คลิก
    pub fn iter(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.items.iter().copied()
    }
}

impl<'a> IntoIterator for &'a Selection {
    type Item = ItemId;
    type IntoIter = std::iter::Copied<indexmap::set::Iter<'a, ItemId>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::arena::ArenaKey as _;

    fn id(n: u32) -> ItemId {
        ItemId::from_parts(n, 0)
    }

    #[test]
    fn select_replaces_and_add_accumulates() {
        let mut selection = Selection::new();
        assert!(selection.is_empty());

        selection.select(id(1));
        selection.add(id(2));
        assert_eq!(selection.len(), 2);
        assert!(selection.contains(id(1)) && selection.contains(id(2)));

        selection.select(id(3));
        assert_eq!(selection.len(), 1, "คลิกธรรมดาต้องทิ้งของเดิม");
        assert!(selection.contains(id(3)));
    }

    /// ★ เหตุผลที่ใช้ `IndexSet`: align ต้องรู้ว่าใครถูกเลือก **ทีหลังสุด**
    #[test]
    fn the_last_click_becomes_the_anchor() {
        let mut selection = Selection::new();
        selection.select(id(1));
        assert_eq!(selection.anchor(), Some(id(1)));

        selection.add(id(2));
        selection.add(id(3));
        assert_eq!(selection.anchor(), Some(id(3)));
        assert_eq!(
            selection.iter().collect::<Vec<_>>(),
            vec![id(1), id(2), id(3)]
        );
    }

    /// เอา anchor ออกแล้วต้องไม่ค้างเป็น id ที่ไม่ได้ถูกเลือกแล้ว
    /// ไม่งั้น align จะอ้างอิงภาพที่ผู้ใช้ไม่ได้เลือก = ภาพกระเด็นไปคนละที่
    #[test]
    fn removing_the_anchor_moves_it_to_the_newest_survivor() {
        let mut selection = Selection::new();
        selection.select(id(1));
        selection.add(id(2));
        selection.add(id(3));

        selection.remove(id(3));
        assert_eq!(selection.anchor(), Some(id(2)));

        selection.remove(id(2));
        selection.remove(id(1));
        assert_eq!(selection.anchor(), None);
        assert!(selection.is_empty());
    }

    /// เอาตัวกลางออกแล้วลำดับของที่เหลือต้องไม่สลับ (`shift_remove` ไม่ใช่ `swap_remove`)
    #[test]
    fn removing_from_the_middle_keeps_click_order() {
        let mut selection = Selection::new();
        selection.select(id(1));
        selection.add(id(2));
        selection.add(id(3));
        selection.add(id(4));

        selection.remove(id(2));
        assert_eq!(
            selection.iter().collect::<Vec<_>>(),
            vec![id(1), id(3), id(4)]
        );
        assert_eq!(selection.anchor(), Some(id(4)), "anchor ที่ไม่ได้ถูกลบต้องไม่ขยับ");
    }

    #[test]
    fn toggle_flips_membership() {
        let mut selection = Selection::new();
        selection.toggle(id(1));
        assert!(selection.contains(id(1)));
        selection.toggle(id(1));
        assert!(!selection.contains(id(1)));
    }

    /// ★ สองชุดที่มีสมาชิกเหมือนกันแต่ลำดับต่างกัน **ต้องไม่เท่ากัน**
    ///
    /// `IndexSet` เองเทียบแบบเซตจึงจะบอกว่าเท่ากัน ถ้าปล่อยไว้แบบนั้น
    /// `undo_restores_exactly` จะผ่านทั้งที่ anchor ของ align เปลี่ยนไปแล้ว
    #[test]
    fn equality_is_order_sensitive_unlike_a_plain_set() {
        let mut one = Selection::new();
        one.select(id(1));
        one.add(id(2));

        let mut two = Selection::new();
        two.select(id(2));
        two.add(id(1));

        assert_eq!(one.len(), two.len());
        assert_ne!(one, two, "ลำดับคลิกต่างกัน = สถานะต่างกัน");
    }

    #[test]
    fn set_all_restores_order_and_anchor() {
        let mut selection = Selection::new();
        selection.set_all([id(5), id(6), id(7)]);
        assert_eq!(
            selection.iter().collect::<Vec<_>>(),
            vec![id(5), id(6), id(7)]
        );
        assert_eq!(selection.anchor(), Some(id(7)));
    }

    /// ★ undo ต้องคืน anchor ตัวจริง ไม่ใช่เดาเป็นตัวท้าย
    #[test]
    fn restore_brings_back_the_original_anchor() {
        let mut selection = Selection::new();
        selection.restore([id(5), id(6), id(7)], Some(id(6)));
        assert_eq!(selection.anchor(), Some(id(6)));

        // anchor ที่ไม่ได้อยู่ในชุด (ค่าจากไฟล์ที่เสียหาย) ต้องไม่ถูกรับไว้
        selection.restore([id(5), id(6)], Some(id(99)));
        assert_eq!(selection.anchor(), Some(id(6)));
    }
}
