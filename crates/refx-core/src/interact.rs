//! `SelectTool` — เครื่องสถานะของการเลือกบน canvas (P2-4)
//!
//! ★ **การเลือกไม่ใช่ส่วนของเอกสาร** (docs/02 §2.9) `Selection` อยู่นอก `Board`
//! เป็นสถานะชั่วคราวของ editor: ไม่ persist ไม่ undo ไม่ทำให้ `dirty`
//! เครื่องมือนี้จึงแก้ `Selection` ตรง ๆ **ไม่ผ่าน `Command`** — ซึ่งไม่ใช่รูโหว่
//! ของกฎ "ทุก mutation ผ่าน `Command`" เพราะกฎนั้นคุ้ม `Board` และการเลือกไม่ได้
//! อยู่ใน `Board` ตั้งแต่แรก
//!
//! ที่สำคัญกว่า undo: ถ้าการเลือกอยู่ใน `Board` การ**คลิกดูภาพเฉย ๆ จะทำให้เอกสาร
//! dirty** แล้วผู้ใช้ที่เปิดไฟล์มาดูแล้วปิดจะโดนถาม "บันทึกไหม" ทั้งที่ไม่ได้แก้อะไร
//! — เกิดกับทุกคนทุกวัน ไม่ใช่เคสขอบ
//!
//! ★ **ทำไมตรรกะนี้อยู่ `refx-core` ไม่ใช่ `refx-ui`:** การเลือกมีเคสขอบเยอะกว่าที่คิด
//! ถ้าตรรกะอยู่ในชั้นที่ต้องเปิดหน้าต่างจริงถึงจะทดสอบได้ ก็จะไม่มีใครทดสอบมัน
//! ที่นี่ทดสอบครบได้โดยไม่มี GPU — `refx-ui` เหลือหน้าที่แค่แปลง pointer ของ egui
//! เป็น [`CanvasEvent`] แล้วเอา [`Interaction`] ไปวาด
//!
//! spec: docs/02-data-model.md §2.9, docs/03-modes-and-ui.md §1, ROADMAP P2-4

use glam::Vec2;

use crate::arena::ItemId;
use crate::board::Board;
use crate::geom::Rect;
use crate::selection::Selection;
use crate::spatial::SpatialIndex;

/// ปุ่มเมาส์เท่าที่ canvas สนใจ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasButton {
    /// ปุ่มซ้าย — เลือก / ลากกรอบ / (P2-5) ย้าย
    Primary,
    /// ปุ่มกลาง — เลื่อนกล้อง (ชั้น UI จัดการเอง กล้องไม่ใช่การเลือก)
    Middle,
}

/// ปุ่มดัดแปลงที่มีผลกับการเลือก
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// เพิ่ม/ถอดทีละตัว
    pub ctrl: bool,
    /// เพิ่มเข้าไปในชุดเดิม
    pub shift: bool,
}

impl Modifiers {
    /// ผู้ใช้ต้องการ **เพิ่มเข้าไปในชุดเดิม** ไม่ใช่เริ่มใหม่
    #[must_use]
    pub fn is_additive(self) -> bool {
        self.ctrl || self.shift
    }
}

/// เหตุการณ์จาก pointer ที่แปลงเป็นพิกัด world แล้ว
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CanvasEvent {
    /// กดปุ่มลง
    Press {
        /// ปุ่มที่กด
        button: CanvasButton,
        /// ตำแหน่งใน world
        world: Vec2,
        /// ปุ่มดัดแปลงตอนกด
        modifiers: Modifiers,
    },
    /// ขยับขณะกดค้าง
    Move {
        /// ตำแหน่งใน world
        world: Vec2,
    },
    /// ปล่อยปุ่ม
    Release {
        /// ปุ่มที่ปล่อย
        button: CanvasButton,
        /// ตำแหน่งใน world
        world: Vec2,
    },
}

/// สิ่งที่ชั้น UI ต้องเอาไปทำต่อหลังส่ง event เข้ามาหนึ่งตัว
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Interaction {
    /// กรอบ rubber-band ที่กำลังลากอยู่ (world space) — `None` = ไม่ต้องวาด
    pub rubber_band: Option<Rect>,
    /// มีอะไรเปลี่ยนที่ต้องวาดใหม่ไหม (I-1 — ไม่มีอะไรเปลี่ยนต้องไม่ขอเฟรม)
    pub needs_redraw: bool,
}

/// ของที่เครื่องสถานะต้องรู้เพื่อตัดสินใจ
#[derive(Debug, Clone, Copy)]
pub struct CanvasContext<'a> {
    /// board ปัจจุบัน — **อ่านอย่างเดียว** การเลือกไม่ได้อยู่ในนี้
    pub board: &'a Board,
    /// index สำหรับ hit-test
    pub index: &'a SpatialIndex,
    /// ระยะที่ถือว่า "เริ่มลากแล้ว" ในหน่วย world
    ///
    /// ชั้น UI คำนวณจาก **พิกเซลบนจอ ÷ zoom** เพื่อให้ความรู้สึกเท่ากันทุกระดับซูม
    /// ถ้าใช้ค่าคงที่ใน world ตอนซูมออกมาก ๆ ผู้ใช้จะลากกรอบไม่ได้เลย
    pub drag_threshold: f32,
}

/// ระยะเริ่มลากเริ่มต้น (พิกเซลบนจอ) — กันมือสั่นตอนคลิก
pub const DEFAULT_DRAG_THRESHOLD_PX: f32 = 4.0;

/// สถานะของการกดค้างหนึ่งครั้ง
#[derive(Debug, Clone)]
struct Press {
    origin: Vec2,
    modifiers: Modifiers,
    /// ขยับเกินระยะจนถือว่าเป็นการลากแล้วหรือยัง
    dragging: bool,
    /// item ที่อยู่ใต้จุดที่กด (`None` = กดที่ว่าง)
    on_item: Option<ItemId>,
    /// สิ่งที่เลือกอยู่ก่อนเริ่มกด — rubber-band แบบเพิ่มต้องบวกจากชุดนี้
    base: Vec<ItemId>,
}

/// เครื่องสถานะของการเลือก
#[derive(Debug, Clone, Default)]
pub struct SelectTool {
    press: Option<Press>,
}

impl SelectTool {
    /// เครื่องมือที่ยังไม่มีอะไรค้าง
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// กำลังลากกรอบอยู่หรือไม่ (ชั้น UI ใช้เลือกเคอร์เซอร์)
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.press.as_ref().is_some_and(|press| press.dragging)
    }

    /// ทิ้งสถานะที่ค้างอยู่ — เรียกเมื่อหน้าต่างเสีย focus หรือ board เปลี่ยน
    ///
    /// ถ้าไม่ล้าง ผู้ใช้ที่สลับไปโปรแกรมอื่นแล้วปล่อยเมาส์ที่นั่นจะกลับมาเจอ
    /// กรอบ rubber-band ค้างอยู่กลางจอโดยกดอะไรก็ไม่หาย
    pub fn cancel(&mut self) {
        self.press = None;
    }

    /// ป้อน event หนึ่งตัว — แก้ `selection` ให้ตรงตามที่ผู้ใช้สั่ง
    #[must_use]
    pub fn handle(
        &mut self,
        ctx: CanvasContext<'_>,
        selection: &mut Selection,
        event: CanvasEvent,
    ) -> Interaction {
        match event {
            CanvasEvent::Press {
                button: CanvasButton::Primary,
                world,
                modifiers,
            } => self.on_press(ctx, selection, world, modifiers),
            CanvasEvent::Move { world } => self.on_move(ctx, selection, world),
            CanvasEvent::Release {
                button: CanvasButton::Primary,
                world,
            } => self.on_release(ctx, selection, world),
            // ปุ่มกลางเป็นเรื่องของกล้อง ไม่แตะการเลือก
            CanvasEvent::Press {
                button: CanvasButton::Middle,
                ..
            }
            | CanvasEvent::Release {
                button: CanvasButton::Middle,
                ..
            } => Interaction::default(),
        }
    }

    fn on_press(
        &mut self,
        ctx: CanvasContext<'_>,
        selection: &mut Selection,
        world: Vec2,
        modifiers: Modifiers,
    ) -> Interaction {
        let hit = ctx.index.hit_test(ctx.board, world);
        let base: Vec<ItemId> = selection.iter().collect();

        self.press = Some(Press {
            origin: world,
            modifiers,
            dragging: false,
            on_item: hit,
            base: base.clone(),
        });

        let mut out = Interaction::default();
        match hit {
            Some(id) if modifiers.is_additive() => {
                // Ctrl+คลิก = สลับสถานะทีละตัว
                let mut next = base;
                let anchor = if let Some(at) = next.iter().position(|other| *other == id) {
                    next.remove(at);
                    next.last().copied()
                } else {
                    next.push(id);
                    Some(id)
                };
                apply_selection(&mut out, selection, next, anchor);
            }
            Some(id) => {
                // คลิกบนภาพที่เลือกอยู่แล้ว = ไม่เปลี่ยนอะไร (จะได้ลากทั้งชุดต่อได้ — P2-5)
                if !selection.contains(id) {
                    apply_selection(&mut out, selection, vec![id], Some(id));
                }
            }
            // กดที่ว่าง — ยังไม่ล้างทันที รอดูว่าจะกลายเป็นการลากกรอบไหม
            // (ล้างตอนปล่อยแทน ดู `on_release`)
            None => {}
        }
        out
    }

    fn on_move(
        &mut self,
        ctx: CanvasContext<'_>,
        selection: &mut Selection,
        world: Vec2,
    ) -> Interaction {
        let Some(press) = self.press.as_mut() else {
            return Interaction::default();
        };

        // ★ กดโดนภาพแล้วลาก = การย้าย ซึ่งเป็นงานของ P2-5 ไม่ใช่ rubber-band
        //   ตรงนี้จึงไม่ทำอะไร แต่ก็ต้องไม่ไปเริ่มลากกรอบทับด้วย
        if press.on_item.is_some() {
            return Interaction::default();
        }

        if !press.dragging {
            let threshold = if ctx.drag_threshold.is_finite() {
                ctx.drag_threshold.max(0.0)
            } else {
                0.0
            };
            if (world - press.origin).length() < threshold {
                return Interaction::default(); // ยังนับเป็นคลิก ไม่ใช่ลาก
            }
            press.dragging = true;
        }

        let rect = Rect::from_corners(press.origin, world);
        let inside = ctx.index.hit_test_rect(ctx.board, rect);
        let (items, anchor) = combine(&press.base, &inside, press.modifiers);

        let mut out = Interaction {
            rubber_band: Some(rect),
            needs_redraw: true,
        };
        apply_selection(&mut out, selection, items, anchor);
        out
    }

    fn on_release(
        &mut self,
        ctx: CanvasContext<'_>,
        selection: &mut Selection,
        world: Vec2,
    ) -> Interaction {
        let Some(press) = self.press.take() else {
            return Interaction::default();
        };
        // อย่างน้อยกรอบ rubber-band ต้องหายไป จึงต้องวาดใหม่เสมอ
        let mut out = Interaction {
            needs_redraw: true,
            ..Interaction::default()
        };

        if press.dragging {
            // จบการลากกรอบ — ยืนยันชุดสุดท้ายอีกครั้งด้วยกรอบตอนปล่อย
            let rect = Rect::from_corners(press.origin, world);
            let inside = ctx.index.hit_test_rect(ctx.board, rect);
            let (items, anchor) = combine(&press.base, &inside, press.modifiers);
            apply_selection(&mut out, selection, items, anchor);
            return out;
        }

        // คลิกเปล่า ๆ ที่ว่าง (ไม่ได้ลาก) = ล้างการเลือก
        // ★ ล้างตอน **ปล่อย** ไม่ใช่ตอนกด: ถ้าล้างตอนกด ผู้ใช้ที่เริ่มลากกรอบ
        //   จะเห็นสิ่งที่เลือกไว้กะพริบหายไปหนึ่งเฟรมก่อนกรอบจะขึ้น
        if press.on_item.is_none() && !press.modifiers.is_additive() {
            apply_selection(&mut out, selection, Vec::new(), None);
        }
        out
    }
}

/// รวมชุดเดิมกับสิ่งที่อยู่ในกรอบ ตามปุ่มดัดแปลง
///
/// คงลำดับเดิมไว้ก่อนแล้วต่อท้ายด้วยตัวใหม่ — ลำดับคือสถานะที่ผู้ใช้สัมผัสได้
/// (anchor ของ align) จึงต้องไม่สลับไปมาระหว่างลาก
fn combine(
    base: &[ItemId],
    inside: &[ItemId],
    modifiers: Modifiers,
) -> (Vec<ItemId>, Option<ItemId>) {
    if !modifiers.is_additive() {
        return (inside.to_vec(), inside.last().copied());
    }
    let mut items = base.to_vec();
    for id in inside {
        if !items.contains(id) {
            items.push(*id);
        }
    }
    let anchor = items.last().copied();
    (items, anchor)
}

/// เขียนการเลือกชุดใหม่ **เฉพาะเมื่อมันต่างจากของเดิมจริง**
///
/// I-1: การขยับเมาส์ระหว่างลากกรอบที่ผลไม่เปลี่ยน ต้องไม่ขอวาดเฟรมใหม่
fn apply_selection(
    out: &mut Interaction,
    selection: &mut Selection,
    items: Vec<ItemId>,
    anchor: Option<ItemId>,
) {
    if selection.anchor() == anchor && selection.iter().eq(items.iter().copied()) {
        return;
    }
    selection.restore(items, anchor);
    out.needs_redraw = true;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::board::ItemCanvas;
    use crate::board::tests::image_item;
    use crate::command::{History, RemoveItems};

    /// board ที่มีภาพ 100×100 เรียงเป็นแถวห่างกัน 200 หน่วย
    fn row_board(n: u32) -> (Board, Vec<ItemId>, SpatialIndex) {
        let mut board = Board::default();
        let ids = (0..n)
            .map(|i| {
                let mut item = image_item(0);
                item.canvas = ItemCanvas {
                    pos: Vec2::new(i as f32 * 200.0, 0.0),
                    size: Vec2::splat(100.0),
                    ..ItemCanvas::default()
                };
                board.insert_item(item)
            })
            .collect();
        board.mark_dirty(false);
        let index = SpatialIndex::from_board(&board);
        (board, ids, index)
    }

    /// editor จำลอง — board + selection ที่อยู่ **นอก** board + history
    struct Harness {
        board: Board,
        index: SpatialIndex,
        selection: Selection,
        history: History,
        tool: SelectTool,
        last: Option<Rect>,
    }

    impl Harness {
        fn new(n: u32) -> (Self, Vec<ItemId>) {
            let (board, ids, index) = row_board(n);
            (
                Self {
                    board,
                    index,
                    selection: Selection::new(),
                    history: History::default(),
                    tool: SelectTool::new(),
                    last: None,
                },
                ids,
            )
        }

        fn feed(&mut self, event: CanvasEvent) {
            let ctx = CanvasContext {
                board: &self.board,
                index: &self.index,
                drag_threshold: 4.0,
            };
            let outcome = self.tool.handle(ctx, &mut self.selection, event);
            self.last = outcome.rubber_band;
        }

        fn press(&mut self, at: Vec2, modifiers: Modifiers) {
            self.feed(CanvasEvent::Press {
                button: CanvasButton::Primary,
                world: at,
                modifiers,
            });
        }

        fn drag_to(&mut self, at: Vec2) {
            self.feed(CanvasEvent::Move { world: at });
        }

        fn release(&mut self, at: Vec2) {
            self.feed(CanvasEvent::Release {
                button: CanvasButton::Primary,
                world: at,
            });
        }

        fn click(&mut self, at: Vec2) {
            self.press(at, Modifiers::default());
            self.release(at);
        }

        fn selected(&self) -> Vec<ItemId> {
            self.selection.iter().collect()
        }
    }

    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        shift: false,
    };

    // ---------- ★ docs/02 §2.9: การเลือกต้องไม่แตะเอกสาร ----------

    /// ★★ เหตุผลที่ `selection` ถูกย้ายออกจาก `Board`
    ///
    /// ผู้ใช้เปิดไฟล์ คลิกดูภาพสองสามใบ ปิด แล้ว **ต้องไม่โดนถาม "บันทึกไหม"**
    /// นี่คือรายละเอียดเล็ก ๆ ที่ทำลายความรู้สึกเชื่อถือได้ และเกิดกับทุกคนทุกวัน
    #[test]
    fn clicking_around_never_makes_the_document_dirty() {
        let (mut h, _) = Harness::new(4);
        let before = h.board.clone();

        h.click(Vec2::ZERO);
        h.click(Vec2::new(200.0, 0.0));
        h.press(Vec2::new(400.0, 0.0), CTRL);
        h.release(Vec2::new(400.0, 0.0));
        h.press(Vec2::new(-60.0, -60.0), Modifiers::default());
        h.drag_to(Vec2::new(460.0, 60.0));
        h.release(Vec2::new(460.0, 60.0));

        assert!(!h.selected().is_empty(), "ต้องมีของถูกเลือกจริง");
        assert!(!h.board.is_dirty(), "คลิกเลือกทำให้เอกสาร dirty");
        assert_eq!(h.board, before, "เอกสารต้องไม่ถูกแตะเลยแม้แต่ฟิลด์เดียว");
    }

    /// ★ การเลือกต้องไม่กิน undo stack เลยแม้แต่ขั้นเดียว
    #[test]
    fn selecting_never_touches_the_undo_stack() {
        let (mut h, _) = Harness::new(6);
        h.press(Vec2::new(-60.0, -60.0), Modifiers::default());
        for step in 1..=100 {
            h.drag_to(Vec2::new(step as f32 * 10.0, 60.0));
        }
        h.release(Vec2::new(1_000.0, 60.0));
        for _ in 0..20 {
            h.click(Vec2::new(100.0, 0.0));
        }

        assert_eq!(h.history.undo_depth(), 0);
        assert_eq!(h.history.redo_depth(), 0);
        assert!(h.history.undo(&mut h.board).unwrap().is_none());
    }

    /// ★ แต่ undo ของ **การลบ** ยังต้องคืนการเลือกได้ (docs/02 §2.9)
    ///
    /// การเลือกไม่ได้ถูก undo — มันตามผลลัพธ์ที่คำสั่งรายงานกลับมา
    #[test]
    fn undoing_a_delete_reselects_what_came_back() {
        let (mut h, ids) = Harness::new(4);
        let doomed = vec![ids[1], ids[2]];

        h.history
            .apply(
                &mut h.board,
                Box::new(RemoveItems::new(doomed.clone()).unwrap()),
            )
            .unwrap();
        h.selection.clear();
        assert_eq!(h.board.len(), 2);

        let affected = h
            .history
            .undo(&mut h.board)
            .unwrap()
            .expect("ต้องมีอะไรให้ย้อน");
        // ชั้น editor เป็นคนตั้ง selection จากสิ่งที่คำสั่งบอกว่าแตะ
        h.selection
            .restore(affected.clone(), affected.last().copied());

        assert_eq!(h.board.len(), 4);
        assert_eq!(h.selected(), doomed, "ภาพที่กลับมาต้องถูกเลือกอยู่");
    }

    // ---------- คลิกเดี่ยว ----------

    #[test]
    fn clicking_an_item_selects_only_it() {
        let (mut h, ids) = Harness::new(3);
        h.click(Vec2::ZERO);
        assert_eq!(h.selected(), vec![ids[0]]);
        assert_eq!(h.selection.anchor(), Some(ids[0]));

        h.click(Vec2::new(200.0, 0.0));
        assert_eq!(h.selected(), vec![ids[1]], "คลิกธรรมดาต้องทิ้งของเดิม");
    }

    #[test]
    fn clicking_empty_space_clears_the_selection() {
        let (mut h, ids) = Harness::new(3);
        h.click(Vec2::ZERO);
        assert_eq!(h.selected(), vec![ids[0]]);

        h.click(Vec2::new(100.0, 0.0)); // ระหว่างภาพ
        assert!(h.selected().is_empty());
        assert_eq!(h.selection.anchor(), None);
    }

    /// ★ ล้างการเลือกต้องเกิดตอน **ปล่อย** ไม่ใช่ตอนกด
    ///
    /// ถ้าล้างตอนกด ผู้ใช้ที่เริ่มลากกรอบจะเห็นสิ่งที่เลือกไว้กะพริบหายไปหนึ่งเฟรม
    #[test]
    fn pressing_on_empty_space_does_not_clear_until_release() {
        let (mut h, ids) = Harness::new(3);
        h.click(Vec2::ZERO);

        h.press(Vec2::new(100.0, 0.0), Modifiers::default());
        assert_eq!(h.selected(), vec![ids[0]], "กดค้างยังไม่ควรล้าง");

        h.release(Vec2::new(100.0, 0.0));
        assert!(h.selected().is_empty());
    }

    /// คลิกบนภาพที่เลือกอยู่แล้วต้องไม่เปลี่ยนชุด — ไม่งั้นลากทั้งชุดไม่ได้ (P2-5)
    #[test]
    fn clicking_an_already_selected_item_keeps_the_whole_group() {
        let (mut h, ids) = Harness::new(3);
        h.press(Vec2::ZERO, CTRL);
        h.release(Vec2::ZERO);
        h.press(Vec2::new(200.0, 0.0), CTRL);
        h.release(Vec2::new(200.0, 0.0));
        assert_eq!(h.selected(), vec![ids[0], ids[1]]);

        h.click(Vec2::ZERO); // คลิกธรรมดาบนตัวที่เลือกอยู่แล้ว
        assert_eq!(
            h.selected(),
            vec![ids[0], ids[1]],
            "ต้องไม่ยุบเหลือตัวเดียว ไม่งั้นลากทั้งชุดไม่ได้"
        );
    }

    // ---------- multi-select ----------

    #[test]
    fn ctrl_click_toggles_one_item_at_a_time() {
        let (mut h, ids) = Harness::new(3);
        for i in 0..3 {
            h.press(Vec2::new(i as f32 * 200.0, 0.0), CTRL);
            h.release(Vec2::new(i as f32 * 200.0, 0.0));
        }
        assert_eq!(h.selected(), vec![ids[0], ids[1], ids[2]]);
        assert_eq!(h.selection.anchor(), Some(ids[2]));

        // ถอดตัวกลางออก — ลำดับของที่เหลือต้องไม่สลับ
        h.press(Vec2::new(200.0, 0.0), CTRL);
        h.release(Vec2::new(200.0, 0.0));
        assert_eq!(h.selected(), vec![ids[0], ids[2]]);
        assert_eq!(
            h.selection.anchor(),
            Some(ids[2]),
            "anchor ต้องตกไปที่ตัวท้ายที่ยังเหลือ ไม่ใช่ค้างที่ตัวที่เพิ่งถอด"
        );
    }

    // ---------- rubber-band ----------

    #[test]
    fn dragging_a_band_selects_everything_it_touches() {
        let (mut h, ids) = Harness::new(4);
        h.press(Vec2::new(-60.0, -60.0), Modifiers::default());
        h.drag_to(Vec2::new(260.0, 60.0));

        assert!(h.last.is_some(), "ต้องมีกรอบให้วาด");
        assert_eq!(h.selected(), vec![ids[0], ids[1]]);

        h.drag_to(Vec2::new(460.0, 60.0));
        assert_eq!(h.selected(), vec![ids[0], ids[1], ids[2]]);

        h.release(Vec2::new(460.0, 60.0));
        assert_eq!(h.selected(), vec![ids[0], ids[1], ids[2]]);
        assert!(h.last.is_none(), "ปล่อยแล้วกรอบต้องหาย");
    }

    #[test]
    fn a_band_drawn_backwards_works_the_same() {
        let (mut h, ids) = Harness::new(3);
        h.press(Vec2::new(260.0, 60.0), Modifiers::default());
        h.drag_to(Vec2::new(-60.0, -60.0));
        h.release(Vec2::new(-60.0, -60.0));
        assert_eq!(h.selected(), vec![ids[0], ids[1]]);
    }

    #[test]
    fn ctrl_dragging_a_band_adds_to_the_existing_selection() {
        let (mut h, ids) = Harness::new(4);
        h.click(Vec2::new(600.0, 0.0));
        assert_eq!(h.selected(), vec![ids[3]]);

        h.press(Vec2::new(-60.0, -60.0), CTRL);
        h.drag_to(Vec2::new(260.0, 60.0));
        h.release(Vec2::new(260.0, 60.0));

        assert_eq!(
            h.selected(),
            vec![ids[3], ids[0], ids[1]],
            "ของเดิมต้องอยู่ครบและอยู่ก่อน"
        );
    }

    /// ★ ขยับไม่ถึงระยะ = ยังเป็นคลิก ไม่ใช่การลาก
    #[test]
    fn a_tiny_wobble_is_still_a_click() {
        let (mut h, ids) = Harness::new(3);
        h.click(Vec2::ZERO);

        h.press(Vec2::new(100.0, 0.0), Modifiers::default());
        h.drag_to(Vec2::new(101.5, 0.5)); // ต่ำกว่าระยะ 4.0
        assert!(h.last.is_none(), "ยังไม่ควรขึ้นกรอบ");
        assert_eq!(h.selected(), vec![ids[0]], "ยังไม่ควรแตะการเลือก");

        h.release(Vec2::new(101.5, 0.5));
        assert!(h.selected().is_empty(), "จบแล้วต้องนับเป็นคลิกที่ว่าง");
    }

    /// กดบนภาพแล้วลาก = การย้าย (P2-5) ต้องไม่กลายเป็น rubber-band ทับ
    #[test]
    fn dragging_from_an_item_never_starts_a_band() {
        let (mut h, ids) = Harness::new(3);
        h.press(Vec2::ZERO, Modifiers::default());
        h.drag_to(Vec2::new(400.0, 0.0));

        assert!(h.last.is_none(), "ลากจากบนภาพต้องไม่ขึ้นกรอบเลือก");
        assert_eq!(h.selected(), vec![ids[0]]);
        h.release(Vec2::new(400.0, 0.0));
        assert_eq!(h.selected(), vec![ids[0]]);
    }

    // ---------- I-1 / ความทนทาน ----------

    /// ★ I-1: event ที่ไม่ได้เปลี่ยนอะไรต้องไม่ขอวาดเฟรมใหม่
    #[test]
    fn a_move_that_changes_nothing_does_not_ask_for_a_redraw() {
        let (board, _, index) = row_board(3);
        let mut tool = SelectTool::new();
        let mut selection = Selection::new();
        let ctx = CanvasContext {
            board: &board,
            index: &index,
            drag_threshold: 4.0,
        };

        let outcome = tool.handle(ctx, &mut selection, CanvasEvent::Move { world: Vec2::ZERO });
        assert!(!outcome.needs_redraw);

        let outcome = tool.handle(
            ctx,
            &mut selection,
            CanvasEvent::Release {
                button: CanvasButton::Primary,
                world: Vec2::ZERO,
            },
        );
        assert!(!outcome.needs_redraw);
    }

    /// ลากกรอบต่อไปโดยที่ชุดที่เลือกไม่เปลี่ยน ต้องไม่แตะ selection ซ้ำ ๆ
    #[test]
    fn dragging_within_the_same_result_stops_rewriting_the_selection() {
        let (mut h, _) = Harness::new(2);
        h.press(Vec2::new(-60.0, -60.0), Modifiers::default());
        h.drag_to(Vec2::new(60.0, 60.0));
        let settled = h.selection.clone();

        // ขยับต่ออีกนิดโดยยังคลุมภาพเดิมตัวเดียว
        h.drag_to(Vec2::new(70.0, 60.0));
        assert_eq!(h.selection, settled);
    }

    /// ปุ่มกลางเป็นเรื่องของกล้อง ต้องไม่แตะการเลือกเลย
    #[test]
    fn the_middle_button_never_touches_the_selection() {
        let (mut h, ids) = Harness::new(3);
        h.click(Vec2::ZERO);

        h.feed(CanvasEvent::Press {
            button: CanvasButton::Middle,
            world: Vec2::new(200.0, 0.0),
            modifiers: Modifiers::default(),
        });
        h.feed(CanvasEvent::Release {
            button: CanvasButton::Middle,
            world: Vec2::new(400.0, 0.0),
        });
        assert_eq!(h.selected(), vec![ids[0]]);
    }

    /// เสีย focus ระหว่างลาก แล้วปล่อยเมาส์ที่โปรแกรมอื่น — กรอบต้องไม่ค้าง
    #[test]
    fn cancelling_mid_drag_leaves_nothing_stuck() {
        let (mut h, _) = Harness::new(3);
        h.press(Vec2::new(-60.0, -60.0), Modifiers::default());
        h.drag_to(Vec2::new(260.0, 60.0));
        assert!(h.tool.is_dragging());

        h.tool.cancel();
        assert!(!h.tool.is_dragging());

        h.drag_to(Vec2::new(500.0, 60.0));
        h.release(Vec2::new(500.0, 60.0));
        assert!(h.last.is_none());
    }

    /// I-4: พิกัดที่ไม่ใช่ตัวเลขต้องไม่ทำให้เลือกมั่วหรือ panic
    #[test]
    fn non_finite_pointer_positions_are_harmless() {
        let (mut h, ids) = Harness::new(3);
        h.click(Vec2::ZERO);

        h.press(Vec2::new(f32::NAN, 0.0), Modifiers::default());
        h.drag_to(Vec2::new(f32::INFINITY, 0.0));
        h.release(Vec2::new(f32::NAN, f32::NAN));

        assert!(h.selected().is_empty() || h.selected() == vec![ids[0]]);
        assert!(h.board.z_order_is_consistent());
        assert!(!h.board.is_dirty());
    }
}
