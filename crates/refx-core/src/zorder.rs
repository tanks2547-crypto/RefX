//! การจัดลำดับชั้นของ item บน canvas (P2-6)
//!
//! ★ **เป็นฟังก์ชันบริสุทธิ์ทั้งไฟล์** — รับลำดับเดิมกับสิ่งที่เลือกไว้ คืนลำดับใหม่
//! ไม่แตะ `Board` เลย ชั้น editor เป็นคนเอาผลไปห่อเป็น [`crate::command::ReorderZ`]
//! เหตุผลเดียวกับที่ layout engine ต้องเป็น pure function (docs/03 §3): ตรรกะที่มี
//! เคสขอบเยอะแบบนี้ต้องทดสอบได้โดยไม่ต้องเปิดหน้าต่าง ไม่งั้นจะไม่มีใครทดสอบมัน
//!
//! ลำดับใน `z_order` คือ **ล่างสุด → บนสุด** (ตัวท้าย = อยู่หน้าสุดบนจอ)
//!
//! spec: docs/03-modes-and-ui.md §5 (`[` `]`), ROADMAP P2-6

use crate::arena::ItemId;

/// ทิศทางที่ผู้ใช้สั่งย้ายชั้น
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZMove {
    /// ขึ้นหนึ่งชั้น (`]`)
    Forward,
    /// ลงหนึ่งชั้น (`[`)
    Backward,
    /// ขึ้นไปบนสุด (`Shift+]`)
    ToFront,
    /// ลงไปล่างสุด (`Shift+[`)
    ToBack,
}

/// ลำดับใหม่หลังย้ายชั้น — `None` เมื่อ **ไม่มีอะไรเปลี่ยน**
///
/// ★ `None` ไม่ใช่ error แต่เป็นคำตอบที่ถูกต้องของ "กด `]` ตอนภาพอยู่บนสุดแล้ว"
/// ผู้เรียกต้องไม่สร้าง `Command` และต้องไม่ขอวาดเฟรมใหม่ (I-1) — ไม่งั้น undo stack
/// จะเต็มไปด้วยขั้นที่กดแล้วไม่มีอะไรเกิดขึ้น ซึ่งผู้ใช้อ่านว่า "Ctrl+Z ไม่ทำงาน"
///
/// ของที่เลือกไว้หลายใบ **ขยับเป็นก้อน**: ลำดับสัมพัทธ์ระหว่างกันไม่เปลี่ยน
/// และก้อนนั้นหยุดเมื่อชนขอบ ไม่ใช่ต่างคนต่างทะลุกันเอง
#[must_use]
pub fn reordered(order: &[ItemId], selected: &[ItemId], movement: ZMove) -> Option<Vec<ItemId>> {
    if order.is_empty() || selected.is_empty() {
        return None;
    }
    // ★ เทียบด้วย "อยู่ในชุดที่เลือกไหม" ไม่ใช่ index ของ `selected` — ลำดับใน
    //   `Selection` เป็นลำดับที่ผู้ใช้คลิก ซึ่งไม่เกี่ยวกับลำดับชั้นเลย
    let picked = |id: ItemId| selected.contains(&id);
    if !order.iter().copied().any(picked) {
        return None;
    }

    let mut next = order.to_vec();
    match movement {
        ZMove::ToFront => {
            // stable partition: ที่ไม่ได้เลือกอยู่ก่อน แล้วต่อด้วยที่เลือกตามลำดับชั้นเดิม
            let (stay, moved): (Vec<ItemId>, Vec<ItemId>) =
                next.into_iter().partition(|id| !picked(*id));
            next = stay;
            next.extend(moved);
        }
        ZMove::ToBack => {
            let (moved, stay): (Vec<ItemId>, Vec<ItemId>) =
                next.into_iter().partition(|id| picked(*id));
            next = moved;
            next.extend(stay);
        }
        ZMove::Forward => {
            // ★ ไล่จาก **บนลงล่าง** — ตัวบนสุดของก้อนต้องขยับก่อน ไม่งั้นมันจะไปชน
            //   เพื่อนร่วมก้อนที่ยังไม่ได้ขยับ แล้วทั้งก้อนจะติดอยู่กับที่
            for i in (0..next.len().saturating_sub(1)).rev() {
                if picked(next[i]) && !picked(next[i + 1]) {
                    next.swap(i, i + 1);
                }
            }
        }
        ZMove::Backward => {
            // ไล่จากล่างขึ้นบนด้วยเหตุผลกลับกัน
            for i in 1..next.len() {
                if picked(next[i]) && !picked(next[i - 1]) {
                    next.swap(i, i - 1);
                }
            }
        }
    }

    (next != order).then_some(next)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::arena::Arena;

    /// id จริงจาก `Arena` — ห้ามประกอบ `ItemId` เอง มันเป็นคีย์ที่มี generation
    fn ids(n: usize) -> Vec<ItemId> {
        let mut arena: Arena<ItemId, u32> = Arena::new();
        (0..n).map(|i| arena.insert(i as u32)).collect()
    }

    /// แปลงผลลัพธ์เป็นเลขดัชนีของ `all` เพื่อให้ assert อ่านออก
    fn shape(all: &[ItemId], got: &[ItemId]) -> Vec<usize> {
        got.iter()
            .map(|id| all.iter().position(|other| other == id).unwrap())
            .collect()
    }

    #[test]
    fn one_item_moves_one_step_each_way() {
        let all = ids(4);
        let up = reordered(&all, &[all[1]], ZMove::Forward).unwrap();
        assert_eq!(shape(&all, &up), vec![0, 2, 1, 3]);

        let down = reordered(&all, &[all[2]], ZMove::Backward).unwrap();
        assert_eq!(shape(&all, &down), vec![0, 2, 1, 3]);
    }

    #[test]
    fn to_front_and_to_back_go_all_the_way() {
        let all = ids(5);
        let front = reordered(&all, &[all[1]], ZMove::ToFront).unwrap();
        assert_eq!(shape(&all, &front), vec![0, 2, 3, 4, 1]);

        let back = reordered(&all, &[all[3]], ZMove::ToBack).unwrap();
        assert_eq!(shape(&all, &back), vec![3, 0, 1, 2, 4]);
    }

    /// ★ กดตอนอยู่สุดขอบแล้ว = **ไม่มีอะไรเปลี่ยน** ต้องคืน `None`
    ///
    /// ถ้าคืน `Some` ที่เท่าเดิม ผู้เรียกจะสร้าง `Command` ที่ไม่ทำอะไรเข้า undo stack
    /// แล้วผู้ใช้ที่กด `]` รัว ๆ ตอนภาพอยู่บนสุด จะต้องกด Ctrl+Z สิบครั้งกว่าจะย้อน
    /// การแก้ครั้งจริงได้ — อ่านได้ว่า "undo พัง" (I-1 ด้วย: ไม่มีอะไรเปลี่ยน = ไม่วาดใหม่)
    #[test]
    fn hitting_the_edge_reports_no_change_instead_of_a_no_op_command() {
        let all = ids(3);
        assert!(reordered(&all, &[all[2]], ZMove::Forward).is_none());
        assert!(reordered(&all, &[all[2]], ZMove::ToFront).is_none());
        assert!(reordered(&all, &[all[0]], ZMove::Backward).is_none());
        assert!(reordered(&all, &[all[0]], ZMove::ToBack).is_none());
    }

    #[test]
    fn nothing_selected_changes_nothing() {
        let all = ids(3);
        assert!(reordered(&all, &[], ZMove::Forward).is_none());
        assert!(reordered(&[], &[all[0]], ZMove::Forward).is_none());
    }

    /// id ที่ไม่ได้อยู่ในลำดับ (ถูกลบไปแล้ว) ต้องไม่ทำให้ผลเพี้ยน
    ///
    /// ★ ghost ต้องมาจาก **arena เดียวกัน** — arena สองตัวที่เริ่มจากว่างแจกคีย์
    ///   ชุดเดียวกันเป๊ะ (index+generation เท่ากัน) เอามาทำ id ปลอมไม่ได้
    #[test]
    fn a_stale_id_in_the_selection_is_ignored() {
        let pool = ids(5);
        let all = pool[..4].to_vec();
        let ghost = pool[4];
        assert!(reordered(&all, &[ghost], ZMove::Forward).is_none());

        let moved = reordered(&all, &[ghost, all[0]], ZMove::Forward).unwrap();
        assert_eq!(shape(&all, &moved), vec![1, 0, 2, 3]);
    }

    /// ★★ เลือกหลายใบแล้วขยับ = **ก้อนเดียว** ลำดับภายในก้อนห้ามสลับ
    #[test]
    fn a_multi_selection_moves_as_one_block() {
        let all = ids(6);
        let block = [all[1], all[2]];

        let up = reordered(&all, &block, ZMove::Forward).unwrap();
        assert_eq!(shape(&all, &up), vec![0, 3, 1, 2, 4, 5]);

        let down = reordered(&all, &block, ZMove::Backward).unwrap();
        assert_eq!(shape(&all, &down), vec![1, 2, 0, 3, 4, 5]);
    }

    /// ก้อนที่ไม่ติดกันก็ยังต้องเลื่อนทีละหนึ่งโดยไม่ทับกันเอง
    #[test]
    fn a_scattered_selection_never_swallows_its_own_members() {
        let all = ids(6);
        let picked = [all[0], all[2], all[4]];

        let up = reordered(&all, &picked, ZMove::Forward).unwrap();
        assert_eq!(shape(&all, &up), vec![1, 0, 3, 2, 5, 4]);
        // ของที่เลือกยังอยู่ครบและเรียงกันเหมือนเดิม
        assert_eq!(
            up.iter().filter(|id| picked.contains(id)).count(),
            picked.len()
        );
    }

    /// ★ ก้อนที่ชนขอบแล้วต้อง **หยุดทั้งก้อน** ไม่ใช่ให้ตัวล่างไล่ทะลุตัวบน
    #[test]
    fn a_block_already_at_the_top_does_not_shuffle_itself() {
        let all = ids(4);
        assert!(reordered(&all, &[all[2], all[3]], ZMove::Forward).is_none());
        assert!(reordered(&all, &[all[0], all[1]], ZMove::Backward).is_none());
    }

    /// เลือกทุกใบ = ไม่มีอะไรให้ขยับผ่าน
    #[test]
    fn selecting_everything_is_always_a_no_op() {
        let all = ids(4);
        for movement in [
            ZMove::Forward,
            ZMove::Backward,
            ZMove::ToFront,
            ZMove::ToBack,
        ] {
            assert!(reordered(&all, &all, movement).is_none(), "{movement:?}");
        }
    }

    /// ★ ห้ามทำ item หายหรือซ้ำไม่ว่าสั่งอะไร — `ReorderZ::apply` จะปฏิเสธทันที
    /// ถ้าลำดับไม่ครบ แต่ตรวจที่นี่ด้วยเพื่อให้รู้ว่าใครผิดตั้งแต่ต้นทาง
    #[test]
    fn every_move_is_a_permutation_of_the_input() {
        let all = ids(7);
        let picks: [&[ItemId]; 4] = [
            &[all[0]],
            &[all[3], all[4]],
            &[all[0], all[2], all[6]],
            &[all[1], all[5]],
        ];
        for picked in picks {
            for movement in [
                ZMove::Forward,
                ZMove::Backward,
                ZMove::ToFront,
                ZMove::ToBack,
            ] {
                let Some(next) = reordered(&all, picked, movement) else {
                    continue;
                };
                assert_eq!(next.len(), all.len(), "{movement:?}");
                let mut sorted = next.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted.len(), all.len(), "มี id ซ้ำ: {movement:?}");
                assert!(all.iter().all(|id| next.contains(id)), "มี id หาย");
            }
        }
    }

    /// กด `]` ซ้ำ ๆ ต้องไปถึงบนสุดแล้วหยุด ไม่ใช่วนกลับ
    #[test]
    fn repeating_forward_converges_on_the_top_and_stops() {
        let all = ids(5);
        let mut order = all.clone();
        let mut steps = 0;
        while let Some(next) = reordered(&order, &[all[0]], ZMove::Forward) {
            order = next;
            steps += 1;
            assert!(steps <= all.len(), "ไม่ยอมหยุด — วนไม่รู้จบ");
        }
        assert_eq!(steps, 4);
        assert_eq!(order.last(), Some(&all[0]));
    }
}
