//! `SpatialIndex` — loose uniform grid สำหรับ culling และ hit-test
//!
//! ทำไม grid ไม่ใช่ quadtree (docs/04 §5): ผู้ใช้ลากภาพตลอดเวลา insert/remove จึงต้อง
//! เป็น O(1) ส่วน quadtree ต้อง rebalance และซับซ้อนกว่าโดยไม่ได้เร็วกว่าในการใช้งานจริง
//!
//! **"loose" หมายถึงอะไรที่นี่:** item ถูกลงทะเบียนไว้ที่ **ช่องเดียว** คือช่องที่
//! จุดกึ่งกลางของมันตกอยู่ (ทำให้ insert/remove เป็น O(1) จริง) แต่ตัวมันยื่นออกนอก
//! ช่องนั้นได้ — แต่ละช่องจึงเก็บ **กรอบจริงของสมาชิก** ไว้ด้วย และการค้นหาขยาย
//! ขอบเขตออกไปตามขนาดของ item ที่ใหญ่ที่สุดในกระดาน
//!
//! ★ **ห้ามให้ผลลัพธ์ขึ้นกับลำดับ iteration ของ `HashMap`** (CLAUDE.md):
//! ในไฟล์นี้ `cells` ถูก *ค้นด้วยคีย์* เป็นหลัก ส่วนเส้นทางที่ไล่ทั้ง map
//! (เมื่อช่วงค้นหาใหญ่กว่าจำนวนช่องที่มีของ) จะ **เรียงผลลัพธ์ก่อนคืนเสมอ**
//! ผลจึงเท่ากันทุกครั้งไม่ว่า hasher จะสุ่มมาแบบไหน — มีเทสต์คุมข้อนี้ไว้
//!
//! spec: docs/04-rendering.md §5, ROADMAP P2-3

use std::collections::HashMap;

use glam::{IVec2, Vec2};
use smallvec::SmallVec;

use crate::arena::{ArenaKey as _, ItemId};
use crate::board::{Board, ItemCanvas};
use crate::geom::Rect;

/// ขนาดช่องเริ่มต้น (world units) — ~2× ขนาดภาพที่พบบ่อย
pub const DEFAULT_CELL_SIZE: f32 = 512.0;

/// ขนาดช่องที่เล็กที่สุดที่ยอมให้ตั้งได้
///
/// ช่องเล็กเกินไปทำให้ board ที่กว้างมากมีช่องเป็นล้าน — กิน RAM โดยไม่ได้เร็วขึ้น
pub const MIN_CELL_SIZE: f32 = 16.0;

/// สิ่งที่อยู่ในช่องหนึ่งช่อง
#[derive(Debug, Clone, PartialEq)]
struct Cell {
    /// `SmallVec` เพราะช่องส่วนใหญ่มีของไม่กี่ชิ้น — ไม่ต้อง alloc เลยในกรณีปกติ
    items: SmallVec<[ItemId; 8]>,
    /// กรอบจริงของสมาชิกทุกตัวรวมกัน — ตัวที่ทำให้ grid นี้ "loose"
    ///
    /// ใช้ตัดช่องที่ไม่มีทางโดนทิ้งก่อนจะไปไล่ทีละ item
    bounds: Rect,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            items: SmallVec::new(),
            // ช่องว่างต้องไม่ทับอะไรเลย — `Rect::default()` ที่เป็นศูนย์ทั้งหมด
            // จะทับกรอบที่พาดผ่าน origin พอดี ซึ่งเป็นบั๊กที่หายาก
            bounds: Rect::EMPTY,
        }
    }
}

/// สิ่งที่ index จำไว้ต่อ item หนึ่งตัว
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    cell: IVec2,
    bounds: Rect,
}

/// ตัวเลขบอกว่าการค้นหาครั้งนั้นทำงานไปเท่าไหร่
///
/// ★ มีไว้ให้ **เทสต์พิสูจน์ว่า index ทำงานจริง** โดยไม่ต้องพึ่งเวลาบนนาฬิกา —
/// เทสต์ที่วัดเป็นมิลลิวินาทีจะแดงสุ่มบน CI ที่เครื่องช้ากว่า (เคยเกิดมาแล้ว
/// จนต้องย้ายไปทดสอบคิวโดยตรง) สิ่งที่ต้องยืนยันคือ **ไม่ได้ไล่ทั้งกระดาน**
/// ซึ่งเป็นคุณสมบัติเชิงอัลกอริทึม ไม่ใช่เชิงความเร็วเครื่อง
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryStats {
    /// จำนวนช่องที่ถูกเปิดดู
    pub cells_visited: usize,
    /// จำนวน item ที่ผ่านด่านหยาบแล้วถูกทดสอบจริง
    pub items_examined: usize,
}

/// ผลของ hit-test พร้อมตัวเลขว่าทำงานไปเท่าไหร่
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitReport {
    /// item บนสุดที่โดน
    pub hit: Option<ItemId>,
    /// งานที่ใช้ไป
    pub stats: QueryStats,
}

/// loose uniform grid
#[derive(Debug, Clone, Default)]
pub struct SpatialIndex {
    cell_size: f32,
    cells: HashMap<IVec2, Cell>,
    placed: HashMap<ItemId, Placement>,
    /// ครึ่งหนึ่งของด้านที่ยาวที่สุดในบรรดา item ทั้งหมด — ระยะที่การค้นหาต้องเผื่อ
    ///
    /// **โตอย่างเดียว ไม่หดตอนลบ** โดยตั้งใจ: การหาค่าใหม่ตอนลบคือ O(n) ทุกครั้ง
    /// ที่ผู้ใช้ลบภาพ ส่วนค่าที่ใหญ่เกินจริงแค่ทำให้ค้นช้าลง **ไม่ทำให้ผลผิด**
    /// (ผลที่ผิดคือคืนของไม่ครบ ซึ่งจะกลายเป็นภาพหายจากจอ) —
    /// [`SpatialIndex::rebuild`] เป็นคนคืนค่าให้พอดีอีกครั้ง
    max_reach: f32,
}

impl SpatialIndex {
    /// index ว่างที่ขนาดช่องกำหนดเอง
    #[must_use]
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size: sane_cell_size(cell_size),
            cells: HashMap::new(),
            placed: HashMap::new(),
            max_reach: 0.0,
        }
    }

    /// สร้างใหม่ทั้งชุดจาก board — ขนาดช่องเลือกจากขนาดภาพเฉลี่ย (docs/04 §5)
    #[must_use]
    pub fn from_board(board: &Board) -> Self {
        let mut index = Self::new(suggested_cell_size(board));
        index.rebuild(board);
        index
    }

    /// ขนาดช่องที่ใช้อยู่
    #[must_use]
    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// จำนวน item ที่อยู่ใน index
    #[must_use]
    pub fn len(&self) -> usize {
        self.placed.len()
    }

    /// ว่างหรือไม่
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }

    /// จำนวนช่องที่มีของอยู่ (สำหรับ status bar และเทสต์)
    #[must_use]
    pub fn occupied_cells(&self) -> usize {
        self.cells.len()
    }

    /// ล้างแล้วใส่ทุก item ของ board กลับเข้าไปใหม่
    ///
    /// เป็นจุดเดียวที่ `max_reach` หดกลับได้
    pub fn rebuild(&mut self, board: &Board) {
        self.cells.clear();
        self.placed.clear();
        self.max_reach = 0.0;
        for (id, item) in board.items_in_z_order() {
            self.insert(id, &item.canvas);
        }
    }

    /// ใส่ item เข้า index (หรือย้ายที่ ถ้ามีอยู่แล้ว)
    pub fn insert(&mut self, id: ItemId, canvas: &ItemCanvas) {
        let bounds = canvas.world_bounds();
        if !bounds.is_finite() {
            // ค่าพังจากไฟล์เสีย — ถอนออกดีกว่าปล่อยให้ไปทำให้ทั้งกระดานค้นช้า
            self.remove(id);
            return;
        }
        let cell = self.cell_of(bounds.center());

        if let Some(previous) = self.placed.get(&id).copied() {
            if previous.cell == cell && previous.bounds == bounds {
                return; // ไม่ขยับจริง — ไม่ต้องแตะอะไรเลย
            }
            self.placed.remove(&id);
            self.detach(id, previous.cell);
        }

        let size = bounds.size();
        self.max_reach = self.max_reach.max(size.x.max(size.y) * 0.5);
        self.placed.insert(id, Placement { cell, bounds });

        let slot = self.cells.entry(cell).or_default();
        slot.items.push(id);
        slot.bounds = if slot.items.len() == 1 {
            bounds
        } else {
            slot.bounds.union(bounds)
        };
    }

    /// เอา item ออกจาก index — เรียกซ้ำได้ ไม่มีผลถ้าไม่มีอยู่
    pub fn remove(&mut self, id: ItemId) {
        if let Some(previous) = self.placed.remove(&id) {
            self.detach(id, previous.cell);
        }
    }

    /// item ทั้งหมดที่กรอบทับกับ `rect` — **เรียงตาม `ItemId` เสมอ**
    ///
    /// เรียงเพื่อให้ผลไม่ขึ้นกับลำดับ iteration ของ `HashMap` (CLAUDE.md)
    /// ผู้เรียกที่ต้องการลำดับ render ให้ใช้ `Board::z_order` จัดอีกที
    #[must_use]
    pub fn query(&self, rect: Rect) -> Vec<ItemId> {
        let mut out = Vec::new();
        self.query_into(rect, &mut out);
        out
    }

    /// เหมือน [`SpatialIndex::query`] แต่ใช้บัฟเฟอร์เดิมซ้ำ (culling ทุกเฟรม — I-1/I-6)
    pub fn query_into(&self, rect: Rect, out: &mut Vec<ItemId>) -> QueryStats {
        out.clear();
        let mut stats = QueryStats::default();
        if self.placed.is_empty() || rect.is_empty() || !rect.is_finite() {
            return stats;
        }

        for cell in self.candidate_cells(rect, &mut stats) {
            for &id in &cell.items {
                // กรอบที่จำไว้คือของจริงของ item ตัวนั้น ไม่ใช่ของทั้งช่อง
                let Some(placement) = self.placed.get(&id) else {
                    continue;
                };
                stats.items_examined += 1;
                if placement.bounds.intersects(rect) {
                    out.push(id);
                }
            }
        }

        // ★ ลำดับสุดท้ายต้องไม่ขึ้นกับ hasher
        out.sort_unstable_by_key(|id| (id.index(), id.generation()));
        stats
    }

    /// item บนสุดที่อยู่ใต้จุดนี้ — `None` ถ้าไม่โดนอะไรเลย
    ///
    /// ★ **"บนสุด" ตัดสินจาก `Board::z_order` ไม่ใช่จากลำดับใน index**
    /// นี่คือข้อกำหนดของ P2-3: ทับกัน 50 ชั้นก็ต้องได้ตัวบนสุดเสมอ
    #[must_use]
    pub fn hit_test(&self, board: &Board, world: Vec2) -> Option<ItemId> {
        self.hit_test_reported(board, world).hit
    }

    /// เหมือน [`SpatialIndex::hit_test`] แต่บอกด้วยว่าทำงานไปเท่าไหร่ (สำหรับเทสต์)
    #[must_use]
    pub fn hit_test_reported(&self, board: &Board, world: Vec2) -> HitReport {
        let mut stats = QueryStats::default();
        if !world.is_finite() {
            return HitReport { hit: None, stats };
        }
        let point = Rect::from_corners(world, world);

        // ด่านหยาบ: เอาเฉพาะตัวที่กรอบคลุมจุดนี้ แล้วค่อยทดสอบรูปทรงจริง
        let mut hits: SmallVec<[ItemId; 8]> = SmallVec::new();
        for cell in self.candidate_cells(point, &mut stats) {
            for &id in &cell.items {
                let Some(placement) = self.placed.get(&id) else {
                    continue;
                };
                if !placement.bounds.contains_point(world) {
                    continue;
                }
                stats.items_examined += 1;
                let Some(item) = board.item(id) else {
                    continue; // index ค้างอยู่หลัง board — ข้ามไป ไม่ panic
                };
                // ภาพที่ซ่อนอยู่ต้องคลิกไม่โดน ไม่งั้นผู้ใช้เลือกของที่มองไม่เห็น
                if item.canvas.visible && item.canvas.obb().contains_point(world) {
                    hits.push(id);
                }
            }
        }

        let hit = match hits.len() {
            0 => None,
            1 => Some(hits[0]),
            // ทับกันหลายชั้น — ตัวบนสุดคือตัวที่อยู่ท้ายสุดใน z_order
            _ => board
                .z_order()
                .iter()
                .rev()
                .find(|id| hits.contains(id))
                .copied(),
        };
        HitReport { hit, stats }
    }

    /// item ทุกตัวที่ **รูปทรงจริง** ทับกรอบนี้ เรียงตาม z (ล่างสุด → บนสุด)
    ///
    /// ใช้กับ rubber-band ของ P2-4 — เรียงตาม z เพราะสิ่งที่ผู้ใช้เลือกควรเรียง
    /// เหมือนที่ตาเห็น และ anchor ของ align จะได้เป็นตัวบนสุดอย่างที่คาด
    #[must_use]
    pub fn hit_test_rect(&self, board: &Board, rect: Rect) -> Vec<ItemId> {
        let candidates = self.query(rect);
        if candidates.is_empty() {
            return Vec::new();
        }
        let key = |id: &ItemId| (id.index(), id.generation());
        board
            .z_order()
            .iter()
            .filter(|id| candidates.binary_search_by_key(&key(id), key).is_ok())
            .filter(|id| {
                board.item(**id).is_some_and(|item| {
                    item.canvas.visible && item.canvas.obb().intersects_rect(rect)
                })
            })
            .copied()
            .collect()
    }

    // ---- ภายใน ----

    /// ช่องที่จุดนี้ตกอยู่
    fn cell_of(&self, point: Vec2) -> IVec2 {
        let scaled = point / self.cell_size;
        IVec2::new(floor_to_i32(scaled.x), floor_to_i32(scaled.y))
    }

    /// ถอด item ออกจากช่อง แล้วคำนวณกรอบของช่องนั้นใหม่
    ///
    /// ต้องเรียก **หลัง** เอา `id` ออกจาก `placed` แล้ว
    fn detach(&mut self, id: ItemId, cell: IVec2) {
        let Some(slot) = self.cells.get_mut(&cell) else {
            return;
        };
        slot.items.retain(|other| *other != id);
        if slot.items.is_empty() {
            self.cells.remove(&cell);
            return;
        }
        // กรอบของช่องต้องหดตามเมื่อของออกไป ไม่งั้นช่องที่เคยมีภาพยักษ์
        // จะถูกเปิดดูทุกครั้งไปตลอดกาล
        let items = slot.items.clone();
        let mut bounds = Rect::EMPTY;
        for other in &items {
            if let Some(placement) = self.placed.get(other) {
                bounds = bounds.union(placement.bounds);
            }
        }
        if let Some(slot) = self.cells.get_mut(&cell) {
            slot.bounds = bounds;
        }
    }

    /// ช่องที่มีโอกาสทับ `rect`
    ///
    /// เลือกทางเดินสองแบบตามว่าอันไหนถูกกว่า:
    ///   * ช่วงแคบ → ไล่พิกัดช่องตรง ๆ (ลำดับคงที่ y แล้ว x)
    ///   * ช่วงกว้างกว่าจำนวนช่องที่มีของ → ไล่ช่องที่มีของแทน
    ///
    /// ทางที่สองไล่ `HashMap` ซึ่งลำดับไม่แน่นอน — **ผู้เรียกทุกคนจึงต้องเรียงผล
    /// หรือใช้เกณฑ์ที่ไม่ขึ้นกับลำดับ** (ดูคอมเมนต์หัวไฟล์)
    fn candidate_cells(&self, rect: Rect, stats: &mut QueryStats) -> Vec<&Cell> {
        // เผื่อระยะที่ item ยื่นออกนอกช่องของตัวเอง — นี่คือส่วน "loose" ของ grid
        let search = rect.expand(self.max_reach);
        let min = self.cell_of(search.min);
        let max = self.cell_of(search.max);
        let span = (i64::from(max.x) - i64::from(min.x) + 1).max(0)
            * (i64::from(max.y) - i64::from(min.y) + 1).max(0);

        let mut found = Vec::new();
        if span > 0 && span <= self.cells.len() as i64 {
            for y in min.y..=max.y {
                for x in min.x..=max.x {
                    if let Some(cell) = self.cells.get(&IVec2::new(x, y)) {
                        stats.cells_visited += 1;
                        if cell.bounds.intersects(rect) {
                            found.push(cell);
                        }
                    }
                }
            }
        } else {
            for cell in self.cells.values() {
                stats.cells_visited += 1;
                if cell.bounds.intersects(rect) {
                    found.push(cell);
                }
            }
        }
        found
    }
}

/// ขนาดช่องที่ยอมรับได้ — กัน 0/ติดลบ/`NaN` จากไฟล์ (I-4)
fn sane_cell_size(requested: f32) -> f32 {
    if requested.is_finite() && requested >= MIN_CELL_SIZE {
        requested
    } else {
        DEFAULT_CELL_SIZE
    }
}

/// ~2× ขนาดภาพเฉลี่ยบน board (docs/04 §5)
fn suggested_cell_size(board: &Board) -> f32 {
    let mut total = 0.0f32;
    let mut count = 0.0f32;
    for (_, item) in board.items_in_z_order() {
        let size = item.canvas.size;
        if size.is_finite() {
            total += size.x.max(size.y);
            count += 1.0;
        }
    }
    if count == 0.0 {
        return DEFAULT_CELL_SIZE;
    }
    sane_cell_size(total / count * 2.0)
}

/// `floor` ที่ไม่ล้นเมื่อเจอค่ามหาศาลหรือ `NaN` (I-4)
fn floor_to_i32(value: f32) -> i32 {
    if value.is_nan() {
        return 0;
    }
    let floored = value.floor();
    if floored >= i32::MAX as f32 {
        i32::MAX
    } else if floored <= i32::MIN as f32 {
        i32::MIN
    } else {
        floored as i32
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

    use super::*;
    use crate::board::tests::image_item;

    /// board ที่มีภาพขนาด 100×100 วางเป็นตาราง `n`×`n` ห่างกัน 200 หน่วย
    fn grid_board(n: u32) -> (Board, Vec<ItemId>) {
        let mut board = Board::default();
        let mut ids = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let mut item = image_item(0);
                item.canvas = ItemCanvas {
                    pos: Vec2::new(x as f32 * 200.0, y as f32 * 200.0),
                    size: Vec2::splat(100.0),
                    ..ItemCanvas::default()
                };
                ids.push(board.insert_item(item));
            }
        }
        (board, ids)
    }

    /// ภาพซ้อนกัน `layers` ชั้นที่จุดเดียวกัน
    fn stack_board(layers: usize) -> (Board, Vec<ItemId>) {
        let mut board = Board::default();
        let ids = (0..layers)
            .map(|i| {
                let mut item = image_item(0);
                item.canvas = ItemCanvas {
                    // เหลื่อมกันทีละนิดเพื่อให้กรอบไม่เหมือนกันเป๊ะ แต่ยังทับจุดกลาง
                    pos: Vec2::splat(i as f32 * 0.5),
                    size: Vec2::splat(100.0),
                    ..ItemCanvas::default()
                };
                board.insert_item(item)
            })
            .collect();
        (board, ids)
    }

    // ---------- พื้นฐาน ----------

    #[test]
    fn insert_query_remove() {
        let (board, ids) = grid_board(3);
        let mut index = SpatialIndex::from_board(&board);
        assert_eq!(index.len(), 9);

        let around_origin = Rect::from_center_size(Vec2::ZERO, Vec2::splat(50.0));
        assert_eq!(index.query(around_origin), vec![ids[0]]);

        index.remove(ids[0]);
        assert_eq!(index.len(), 8);
        assert!(index.query(around_origin).is_empty());

        index.remove(ids[0]); // ลบซ้ำต้องเงียบ ไม่ panic
        assert_eq!(index.len(), 8);
    }

    #[test]
    fn moving_an_item_updates_the_cell_it_lives_in() {
        let (mut board, ids) = grid_board(2);
        let mut index = SpatialIndex::from_board(&board);
        let far = Vec2::new(10_000.0, 10_000.0);

        board
            .set_canvas(
                ids[0],
                ItemCanvas {
                    pos: far,
                    size: Vec2::splat(100.0),
                    ..ItemCanvas::default()
                },
            )
            .unwrap();
        index.insert(ids[0], &board.item(ids[0]).unwrap().canvas);

        assert!(
            index
                .query(Rect::from_center_size(Vec2::ZERO, Vec2::splat(50.0)))
                .is_empty(),
            "ยังค้างอยู่ที่ช่องเดิม"
        );
        assert_eq!(
            index.query(Rect::from_center_size(far, Vec2::splat(50.0))),
            vec![ids[0]]
        );
        assert_eq!(index.len(), 4, "ย้ายที่ต้องไม่ทำให้จำนวนเปลี่ยน");
    }

    /// ใส่ค่าเดิมซ้ำต้องไม่ทำให้ index บวม (ผู้ใช้ลากค้าง = เรียกทุกเฟรม)
    #[test]
    fn reinserting_the_same_position_is_a_no_op() {
        let (board, ids) = grid_board(2);
        let mut index = SpatialIndex::from_board(&board);
        let before = index.occupied_cells();

        for _ in 0..100 {
            index.insert(ids[0], &board.item(ids[0]).unwrap().canvas);
        }
        assert_eq!(index.len(), 4);
        assert_eq!(index.occupied_cells(), before);
        assert_eq!(
            index
                .query(Rect::from_center_size(Vec2::ZERO, Vec2::splat(50.0)))
                .len(),
            1
        );
    }

    // ---------- ข้อกำหนดของ P2-3 ----------

    /// ★ เกณฑ์ผ่านที่ ROADMAP กำหนด: ทับกัน 50 ชั้นก็ต้องได้ตัวบนสุดเสมอ
    #[test]
    fn clicking_a_fifty_deep_stack_always_finds_the_topmost() {
        let (mut board, ids) = stack_board(50);
        let index = SpatialIndex::from_board(&board);
        let point = Vec2::new(12.0, 12.0);

        assert_eq!(index.hit_test(&board, point), Some(*ids.last().unwrap()));

        // ส่งตัวล่างสุดขึ้นบน แล้วต้องได้ตัวนั้นแทนทันที
        let mut order = board.z_order().to_vec();
        let bottom = order.remove(0);
        order.push(bottom);
        board.set_z_order(order);
        assert_eq!(
            index.hit_test(&board, point),
            Some(bottom),
            "hit-test ต้องเชื่อ z_order ไม่ใช่ลำดับที่ index เก็บไว้"
        );
    }

    /// ★ index ต้อง **ไม่ไล่ทั้งกระดาน** — พิสูจน์ด้วยจำนวนงาน ไม่ใช่เวลาบนนาฬิกา
    ///
    /// เทสต์ที่วัดเป็นมิลลิวินาทีจะแดงสุ่มบน CI ที่เครื่องช้ากว่า (เคยเกิดมาแล้ว)
    /// สิ่งที่ต้องยืนยันคือคุณสมบัติเชิงอัลกอริทึม ซึ่งเท่ากันทุกเครื่อง
    #[test]
    fn hit_testing_a_thousand_items_barely_looks_at_any_of_them() {
        let (board, _) = grid_board(32); // 1024 ภาพ
        let index = SpatialIndex::from_board(&board);
        assert_eq!(index.len(), 1024);

        let on_item = index.hit_test_reported(&board, Vec2::new(400.0, 400.0));
        assert!(on_item.hit.is_some());
        assert!(
            on_item.stats.items_examined <= 4,
            "ทดสอบไป {} ตัวจาก 1024 — index ไม่ได้ช่วยอะไร",
            on_item.stats.items_examined
        );

        // คลิกที่ว่างระหว่างภาพ — เคสที่แย่กว่าเพราะไม่มีอะไรให้หยุดเร็ว
        let on_gap = index.hit_test_reported(&board, Vec2::new(100.0, 100.0));
        assert_eq!(on_gap.hit, None);
        assert!(
            on_gap.stats.cells_visited <= 16,
            "เปิดดู {} ช่อง — มากเกินไปสำหรับจุดเดียว",
            on_gap.stats.cells_visited
        );

        // ★ ตัวเลขจริงไว้เทียบรุ่นต่อไป — **ไม่ assert เวลา** (ดูเหตุผลข้างบน)
        //   แยกวัดสองเคสเพราะค่าเฉลี่ยรวมกันจะถูกกลบด้วยเคสที่พลาด ซึ่งเร็วกว่ามาก
        //   เคสที่ต้องเร็วจริงคือ "โดนภาพ" เพราะมันเกิดทุกครั้งที่เมาส์ลอยอยู่บนภาพ
        const ROUNDS: u32 = 10_000;

        let started = std::time::Instant::now();
        for i in 0..ROUNDS {
            let p = Vec2::new((i % 32) as f32 * 200.0, (i / 32 % 32) as f32 * 200.0);
            std::hint::black_box(index.hit_test(&board, p));
        }
        let on_image = started.elapsed() / ROUNDS;

        let started = std::time::Instant::now();
        for i in 0..ROUNDS {
            let p = Vec2::new(
                (i % 32) as f32 * 200.0 + 100.0,
                (i / 32 % 32) as f32 * 200.0,
            );
            std::hint::black_box(index.hit_test(&board, p));
        }
        let on_gap = started.elapsed() / ROUNDS;

        println!("hit_test ที่ 1024 ภาพ — โดนภาพ {on_image:?} · ที่ว่าง {on_gap:?} (ต่อครั้ง)");
    }

    /// ภาพที่ซ่อนอยู่ต้องคลิกไม่โดน
    #[test]
    fn hidden_items_are_not_clickable() {
        let (mut board, ids) = stack_board(2);
        let index = SpatialIndex::from_board(&board);
        let point = Vec2::new(1.0, 1.0);
        assert_eq!(index.hit_test(&board, point), Some(ids[1]));

        let mut canvas = board.item(ids[1]).unwrap().canvas;
        canvas.visible = false;
        board.set_canvas(ids[1], canvas).unwrap();
        assert_eq!(
            index.hit_test(&board, point),
            Some(ids[0]),
            "ต้องทะลุไปโดนตัวที่อยู่ข้างล่างแทน"
        );
    }

    /// ★ hit-test ต้องใช้รูปทรงจริง ไม่ใช่ AABB — ภาพที่หมุนแล้วมีมุมที่คลิกไม่โดน
    #[test]
    fn a_rotated_item_is_not_clickable_at_its_bounding_box_corner() {
        let mut board = Board::default();
        let mut item = image_item(0);
        item.canvas = ItemCanvas {
            pos: Vec2::ZERO,
            size: Vec2::splat(100.0),
            rotation: std::f32::consts::TAU / 8.0,
            ..ItemCanvas::default()
        };
        let id = board.insert_item(item);
        let index = SpatialIndex::from_board(&board);

        assert_eq!(index.hit_test(&board, Vec2::ZERO), Some(id));

        let aabb = board.item(id).unwrap().canvas.world_bounds();
        let corner = aabb.max - Vec2::splat(1.0);
        assert!(aabb.contains_point(corner));
        assert_eq!(
            index.hit_test(&board, corner),
            None,
            "มุมของ AABB ไม่ใช่ตัวภาพ — คลิกตรงนั้นต้องไม่โดน"
        );
    }

    /// item ที่ใหญ่กว่าช่องมาก ๆ ต้องยังหาเจอ — นี่คือส่วน "loose" ของ grid
    #[test]
    fn an_item_far_bigger_than_a_cell_is_still_found() {
        let mut board = Board::default();
        let mut item = image_item(0);
        item.canvas = ItemCanvas {
            pos: Vec2::ZERO,
            size: Vec2::splat(20_000.0), // ใหญ่กว่าช่อง 512 มาก
            ..ItemCanvas::default()
        };
        let id = board.insert_item(item);
        let mut index = SpatialIndex::new(DEFAULT_CELL_SIZE);
        index.insert(id, &board.item(id).unwrap().canvas);

        // จุดที่อยู่ห่างจากจุดกึ่งกลางไปหลายสิบช่อง แต่ยังอยู่บนภาพ
        let far = Vec2::splat(9_000.0);
        assert_eq!(index.hit_test(&board, far), Some(id));
        assert_eq!(
            index.query(Rect::from_center_size(far, Vec2::splat(10.0))),
            vec![id]
        );
    }

    // ---------- rubber-band (ของ P2-4) ----------

    #[test]
    fn rect_selection_returns_items_in_z_order() {
        let (board, ids) = grid_board(3);
        let index = SpatialIndex::from_board(&board);

        // กรอบคลุมสามใบของแถวล่าง
        let rect = Rect::from_corners(Vec2::new(-60.0, -60.0), Vec2::new(460.0, 60.0));
        assert_eq!(
            index.hit_test_rect(&board, rect),
            vec![ids[0], ids[1], ids[2]]
        );
    }

    #[test]
    fn rect_selection_ignores_hidden_items_and_empty_rects() {
        let (mut board, ids) = grid_board(2);
        let index = SpatialIndex::from_board(&board);
        let everything = Rect::from_corners(Vec2::splat(-500.0), Vec2::splat(500.0));
        assert_eq!(index.hit_test_rect(&board, everything).len(), 4);

        let mut canvas = board.item(ids[0]).unwrap().canvas;
        canvas.visible = false;
        board.set_canvas(ids[0], canvas).unwrap();
        assert_eq!(index.hit_test_rect(&board, everything).len(), 3);

        assert!(index.hit_test_rect(&board, Rect::EMPTY).is_empty());
    }

    // ---------- determinism (CLAUDE.md) ----------

    /// ★ ผลลัพธ์ต้องไม่ขึ้นกับลำดับ iteration ของ `HashMap`
    ///
    /// `RandomState` สุ่ม seed ใหม่ทุก `HashMap` ที่สร้าง — index สองตัวที่มีของ
    /// เหมือนกันเป๊ะจึงไล่ภายในคนละลำดับ ถ้าผลต่างกันแม้แต่ลำดับ layout จะไม่ deterministic
    #[test]
    fn results_do_not_depend_on_hash_order() {
        let (board, _) = grid_board(8);
        let rect = Rect::from_corners(Vec2::new(-50.0, -50.0), Vec2::new(1_500.0, 1_500.0));

        let first = SpatialIndex::from_board(&board).query(rect);
        assert!(!first.is_empty());
        for _ in 0..20 {
            assert_eq!(
                SpatialIndex::from_board(&board).query(rect),
                first,
                "ผลเปลี่ยนไปตาม hasher"
            );
        }

        // และเส้นทาง "ไล่ทั้ง map" (ช่วงค้นหากว้างกว่าจำนวนช่อง) ก็ต้องเท่ากัน
        let huge = Rect::from_corners(Vec2::splat(-1e6), Vec2::splat(1e6));
        let wide = SpatialIndex::from_board(&board).query(huge);
        assert_eq!(wide.len(), 64);
        for _ in 0..20 {
            assert_eq!(SpatialIndex::from_board(&board).query(huge), wide);
        }
    }

    // ---------- I-4 ----------

    #[test]
    fn broken_values_never_panic_and_never_poison_the_index() {
        let (board, ids) = grid_board(2);
        let mut index = SpatialIndex::from_board(&board);

        // ค่าที่ผ่าน sanitize ของ ItemCanvas ไม่ได้ ยังเข้ามาทาง index ตรง ๆ ได้
        let broken = ItemCanvas {
            pos: Vec2::new(f32::NAN, 0.0),
            size: Vec2::splat(f32::INFINITY),
            ..ItemCanvas::default()
        };
        index.insert(ids[0], &broken);
        assert_eq!(index.len(), 3, "ตัวที่ค่าพังต้องถูกถอนออก ไม่ใช่เก็บไว้");

        assert_eq!(index.hit_test(&board, Vec2::new(f32::NAN, 1.0)), None);
        assert!(index.query(Rect::EMPTY).is_empty());
        assert!(
            index
                .query(Rect {
                    min: Vec2::new(f32::NAN, 0.0),
                    max: Vec2::splat(10.0),
                })
                .is_empty()
        );

        // ของที่เหลือยังหาเจอตามปกติ
        assert_eq!(
            index.query(Rect::from_center_size(
                Vec2::new(200.0, 0.0),
                Vec2::splat(50.0)
            )),
            vec![ids[1]]
        );
    }

    #[test]
    fn cell_size_rejects_nonsense() {
        assert_eq!(SpatialIndex::new(f32::NAN).cell_size(), DEFAULT_CELL_SIZE);
        assert_eq!(SpatialIndex::new(0.0).cell_size(), DEFAULT_CELL_SIZE);
        assert_eq!(SpatialIndex::new(-5.0).cell_size(), DEFAULT_CELL_SIZE);
        assert_eq!(SpatialIndex::new(1_000.0).cell_size(), 1_000.0);
        assert_eq!(
            SpatialIndex::from_board(&Board::default()).cell_size(),
            DEFAULT_CELL_SIZE,
            "board ว่างต้องไม่หารศูนย์"
        );
    }

    /// ตำแหน่งสุดขอบโลก (±1e6 ตาม docs/02 §6) ต้องไม่ทำให้พิกัดช่องล้น
    #[test]
    fn world_limit_positions_do_not_overflow_the_cell_coordinates() {
        let mut board = Board::default();
        let mut item = image_item(0);
        item.canvas = ItemCanvas {
            pos: Vec2::splat(crate::board::WORLD_LIMIT),
            size: Vec2::splat(100.0),
            ..ItemCanvas::default()
        };
        let id = board.insert_item(item);
        let index = SpatialIndex::from_board(&board);
        assert_eq!(
            index.hit_test(&board, Vec2::splat(crate::board::WORLD_LIMIT)),
            Some(id)
        );
    }

    /// ★ negative control ของตัวชี้วัดเอง: ถ้ากรอบคลุมทั้งกระดาน ตัวเลข
    /// `items_examined` ต้องเท่ากับจำนวนภาพทั้งหมด — พิสูจน์ว่าตัวนับวัดของจริง
    /// ไม่ใช่คืนเลขน้อย ๆ ไปเรื่อย ๆ (docs/08 §3.9 ข้อ 1)
    #[test]
    fn the_examined_counter_actually_reflects_work_done() {
        let (board, _) = grid_board(32);
        let index = SpatialIndex::from_board(&board);

        let narrow = index.hit_test_reported(&board, Vec2::new(400.0, 400.0));
        assert!(narrow.stats.items_examined < 8);

        let mut sink = Vec::new();
        let wide = index.query_into(
            Rect::from_corners(Vec2::splat(-1e5), Vec2::splat(1e5)),
            &mut sink,
        );
        assert_eq!(wide.items_examined, 1024, "กรอบคลุมทั้งกระดานต้องเห็นครบ");
        assert_eq!(sink.len(), 1024);
    }
}
