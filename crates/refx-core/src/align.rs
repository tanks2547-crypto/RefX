//! align / distribute + alignment guide ตอนลาก (P2-9)
//!
//! ★ **เป็นฟังก์ชันบริสุทธิ์ทั้งไฟล์** เหมือน [`crate::zorder`] — รับสภาพเข้ามา
//! คืนสภาพใหม่ ไม่แตะ `Board` ชั้น editor เป็นคนห่อเป็น `Command`
//!
//! ★★ **ทำไม guide ไม่ใช่ snap เข้ากริด** (ตัดสินไว้ตั้งแต่ P2-5 · HANDOFF §2.4):
//! mood board คือการวางอิสระ การเด้งเข้ากริดที่ผู้ใช้ไม่ได้ขอทำให้ภาพไปอยู่ที่ที่เขา
//! ไม่ได้ตั้งใจ ซึ่ง**แย่กว่าไม่มี snap เลย** สิ่งที่นักวาดอยากให้ตรงกันจริงคือ
//! **ขอบและกึ่งกลางของภาพอื่น** — นั่นคือสิ่งที่ไฟล์นี้ทำ
//!
//! spec: docs/03-modes-and-ui.md §2, ROADMAP P2-9

use glam::Vec2;

use crate::arena::ItemId;
use crate::board::{Board, ItemCanvas};
use crate::geom::Rect;
use crate::spatial::SpatialIndex;

/// ขอบที่ align ใช้เป็นเป้า (docs/03 §2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// ชิดซ้ายของกรอบรวม
    Left,
    /// กึ่งกลางแนวนอนของกรอบรวม
    CentreX,
    /// ชิดขวาของกรอบรวม
    Right,
    /// ชิดบนของกรอบรวม
    Top,
    /// กึ่งกลางแนวตั้งของกรอบรวม
    CentreY,
    /// ชิดล่างของกรอบรวม
    Bottom,
}

impl Align {
    /// align นี้ทำงานบนแกน x หรือไม่
    #[must_use]
    pub fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::CentreX | Self::Right)
    }
}

/// ทิศของการกระจายระยะ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribute {
    /// กระจายซ้าย→ขวา
    Horizontal,
    /// กระจายบน→ล่าง
    Vertical,
}

/// กรอบรวมของทุกใบที่ส่งมา — `None` ถ้าไม่มีอะไรใช้ได้
fn union_of(items: &[(ItemId, ItemCanvas)]) -> Option<Rect> {
    let mut bounds = Rect::EMPTY;
    for (_, canvas) in items {
        bounds = bounds.union(canvas.world_bounds());
    }
    (bounds.is_finite() && !bounds.is_empty()).then_some(bounds)
}

/// ตำแหน่งใหม่ของทุกใบหลัง align — `None` เมื่อ **ไม่มีอะไรเปลี่ยน**
///
/// ★ ใช้ **กรอบรวมของสิ่งที่เลือก** เป็นเป้า ไม่ใช่ขอบของ board หรือของจอ:
/// ผู้ใช้ที่เลือกห้าใบแล้วกด "ชิดซ้าย" ต้องการให้ห้าใบนั้นตรงกันเอง
/// ไม่ใช่ให้ทั้งกลุ่มกระโดดไปมุมซ้ายของ board
///
/// `None` = ไม่สร้าง `Command` และไม่ขอวาดเฟรม (I-1) — กด align ซ้ำครั้งที่สอง
/// ต้องไม่กิน undo เพิ่มโดยไม่ทำอะไร
#[must_use]
pub fn aligned(items: &[(ItemId, ItemCanvas)], how: Align) -> Option<Vec<(ItemId, ItemCanvas)>> {
    // ★ ใบเดียวจะ align กับตัวเองซึ่งไม่มีความหมาย — ต้องมีอย่างน้อยสอง
    if items.len() < 2 {
        return None;
    }
    let bounds = union_of(items)?;
    let changes: Vec<(ItemId, ItemCanvas)> = items
        .iter()
        .filter_map(|(id, canvas)| {
            let own = canvas.world_bounds();
            let half = own.size() * 0.5;
            let next = match how {
                Align::Left => Vec2::new(bounds.min.x + half.x, canvas.pos.y),
                Align::CentreX => Vec2::new(bounds.center().x, canvas.pos.y),
                Align::Right => Vec2::new(bounds.max.x - half.x, canvas.pos.y),
                Align::Top => Vec2::new(canvas.pos.x, bounds.min.y + half.y),
                Align::CentreY => Vec2::new(canvas.pos.x, bounds.center().y),
                Align::Bottom => Vec2::new(canvas.pos.x, bounds.max.y - half.y),
            };
            // ★ กรอบรวมคิดจาก AABB ของภาพที่หมุนแล้ว ตำแหน่งใหม่จึงต้องชดเชย
            //   ระยะระหว่าง `pos` (กึ่งกลางภาพ) กับกึ่งกลาง AABB ด้วย
            //   สองค่านี้ต่างกันได้เมื่อ crop ไม่สมมาตร
            let drift = canvas.pos - own.center();
            let next = next
                + Vec2::new(
                    if how.is_horizontal() { drift.x } else { 0.0 },
                    if how.is_horizontal() { 0.0 } else { drift.y },
                );
            (next != canvas.pos).then_some((
                *id,
                ItemCanvas {
                    pos: next,
                    ..*canvas
                },
            ))
        })
        .collect();
    (!changes.is_empty()).then_some(changes)
}

/// ตำแหน่งใหม่หลังกระจายระยะให้เท่ากัน — `None` เมื่อไม่มีอะไรเปลี่ยน
///
/// **"ระยะเท่ากัน" = ช่องว่างระหว่างขอบเท่ากัน** (ไม่ใช่ระยะระหว่างกึ่งกลางเท่ากัน)
/// เพราะภาพขนาดต่างกันมากเป็นเรื่องปกติบน mood board ถ้าใช้กึ่งกลาง ภาพใหญ่จะ
/// ไปเบียดภาพเล็กจนดูไม่เท่ากันเลยทั้งที่เลขเท่ากัน
///
/// ★ **ใบที่อยู่หัวและท้ายไม่ขยับ** — เป็นสมอที่ผู้ใช้เลือกโดยการวางไว้ตรงนั้นแล้ว
#[must_use]
pub fn distributed(
    items: &[(ItemId, ItemCanvas)],
    how: Distribute,
) -> Option<Vec<(ItemId, ItemCanvas)>> {
    // ต้องมีอย่างน้อยสามใบ: สองใบหัวท้ายเป็นสมอ ต้องมีของกลางให้ขยับ
    if items.len() < 3 {
        return None;
    }
    let horizontal = how == Distribute::Horizontal;
    let extent = |rect: Rect| {
        if horizontal {
            (rect.min.x, rect.max.x)
        } else {
            (rect.min.y, rect.max.y)
        }
    };

    let mut order: Vec<(ItemId, ItemCanvas, Rect)> = items
        .iter()
        .map(|(id, canvas)| (*id, *canvas, canvas.world_bounds()))
        .collect();
    if order.iter().any(|(_, _, rect)| !rect.is_finite()) {
        return None;
    }
    // ★ เรียงตามขอบต้น แล้ว **ตัดสินเสมอด้วย `ItemId`** — ภาพที่ขอบตรงกันเป๊ะ
    //   ต้องได้ลำดับเดิมทุกครั้ง ไม่งั้นกดปุ่มเดิมสองครั้งได้ผลคนละแบบ
    order.sort_by(|a, b| {
        extent(a.2)
            .0
            .total_cmp(&extent(b.2).0)
            .then_with(|| a.0.cmp(&b.0))
    });

    let first = extent(order.first()?.2);
    let last = extent(order.last()?.2);
    // ที่ว่างที่เหลือหลังหักตัวภาพทั้งหมดออก
    let occupied: f32 = order
        .iter()
        .map(|(_, _, rect)| {
            let (lo, hi) = extent(*rect);
            hi - lo
        })
        .sum();
    let span = last.1 - first.0;
    let gaps = (order.len() - 1) as f32;
    let gap = (span - occupied) / gaps;
    if !gap.is_finite() {
        return None;
    }

    let mut cursor = first.0;
    let mut changes = Vec::new();
    let count = order.len();
    for (index, (id, canvas, rect)) in order.into_iter().enumerate() {
        let (lo, hi) = extent(rect);
        let size = hi - lo;
        // หัวกับท้ายเป็นสมอ ห้ามขยับ
        let target = if index == 0 {
            first.0
        } else if index + 1 == count {
            last.1 - size
        } else {
            cursor
        };
        cursor = target + size + gap;

        let shift = target - lo;
        if shift != 0.0 {
            let delta = if horizontal {
                Vec2::new(shift, 0.0)
            } else {
                Vec2::new(0.0, shift)
            };
            changes.push((
                id,
                ItemCanvas {
                    pos: canvas.pos + delta,
                    ..canvas
                },
            ));
        }
    }
    (!changes.is_empty()).then_some(changes)
}

// ---------------------------------------------------------------------------
// alignment guide ตอนลาก
// ---------------------------------------------------------------------------

/// เส้นไกด์หนึ่งเส้นที่ชั้น UI เอาไปวาด
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guide {
    /// เส้นนี้อยู่บนแกน x (แนวตั้งบนจอ) หรือไม่
    pub vertical: bool,
    /// ตำแหน่งของเส้นใน world (x ถ้า `vertical` มิฉะนั้น y)
    pub at: f32,
    /// ช่วงที่ควรลากเส้น — คลุมทั้งภาพที่ลากและภาพที่มันตรงกับ
    pub from: f32,
    /// ปลายอีกด้านของช่วง
    pub to: f32,
}

/// ผลของการหาไกด์ — ระยะที่ต้องขยับเพิ่มเพื่อให้ตรง + เส้นที่ต้องวาด
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snap {
    /// ระยะที่ต้องบวกเข้าไปในตำแหน่งเป้าหมาย (0 = ไม่ต้องขยับ)
    pub offset: Vec2,
    /// เส้นที่ต้องวาดให้ผู้ใช้เห็นว่ามันตรงกับอะไร
    pub guides: Vec<Guide>,
}

/// สามเส้นที่แต่ละกรอบมีบนแกนหนึ่ง: ขอบต้น · กึ่งกลาง · ขอบท้าย
fn lines(rect: Rect, vertical: bool) -> [f32; 3] {
    if vertical {
        [rect.min.x, rect.center().x, rect.max.x]
    } else {
        [rect.min.y, rect.center().y, rect.max.y]
    }
}

/// หาเส้นที่กรอบ `moving` ควร snap เข้าไป โดยดูจากภาพอื่นที่อยู่ใกล้
///
/// ★★ **ใช้ `SpatialIndex` ไม่ใช่ไล่ทั้ง board** — ที่ 1000 ภาพ การไล่ทุกใบทุกเฟรม
/// ระหว่างลากคือการเผา CPU ฟรี และจะทำให้เฟรมตกทันที (ROADMAP P2-9)
///
/// ★ ขอบเขตที่ค้นคือ **แถบบาง ๆ สองแถบ** (แนวตั้งกับแนวนอน) ที่พาดผ่านกรอบที่ลาก
/// แล้ว **ตัดด้วย `viewport`** — ไม่ใช่แค่กรอบที่ลากขยายออก
///
/// เหตุผล: ภาพที่ควรจะตรงกันมัก **ไม่ทับกัน** เลย (ลากภาพลงมาให้ขอบซ้ายตรงกับ
/// ภาพที่อยู่ข้างบน) ถ้าค้นแค่รอบกรอบที่ลาก จะไม่มีวันเจอมัน
/// และ **ไกด์ที่มองไม่เห็นก็ไม่มีประโยชน์** จึงตัดด้วย viewport ทั้งเพื่อความถูกต้อง
/// และเพื่อไม่ให้ต้นทุนโตตามขนาด board
///
/// `exclude` = id ที่กำลังลากอยู่ (ห้าม snap เข้าหาตัวเอง)
#[must_use]
pub fn snap_to_neighbours(
    board: &Board,
    index: &SpatialIndex,
    moving: Rect,
    viewport: Rect,
    exclude: &[ItemId],
    threshold: f32,
) -> Snap {
    let mut snap = Snap::default();
    if !moving.is_finite() || !threshold.is_finite() || threshold <= 0.0 {
        return snap;
    }
    // viewport ที่ใช้ไม่ได้ = ตกกลับไปใช้กรอบที่ลากขยายออก (ยังทำงานได้ แค่เจอน้อยลง)
    let viewport = if viewport.is_finite() && !viewport.is_empty() {
        viewport
    } else {
        moving.expand(threshold)
    };

    // แถบแนวตั้ง (หาไกด์ของแกน x) และแถบแนวนอน (แกน y) — ตัดด้วย viewport ทั้งคู่
    let strip_x = Rect {
        min: Vec2::new(moving.min.x - threshold, viewport.min.y),
        max: Vec2::new(moving.max.x + threshold, viewport.max.y),
    };
    let strip_y = Rect {
        min: Vec2::new(viewport.min.x, moving.min.y - threshold),
        max: Vec2::new(viewport.max.x, moving.max.y + threshold),
    };
    let mut nearby = index.query(strip_x);
    nearby.extend(index.query(strip_y));
    nearby.sort_unstable();
    nearby.dedup();

    // เก็บคู่ที่ดีที่สุดของแต่ละแกนไว้ก่อน แล้วค่อยตัดสินตอนท้าย
    // (ถ้า snap ทีละแกนทันที เส้นที่สองจะคิดจากตำแหน่งที่ยังไม่ขยับ)
    let mut best: [Option<(f32, Guide)>; 2] = [None, None];

    for id in nearby {
        if exclude.contains(&id) {
            continue;
        }
        let Some(item) = board.item(id) else { continue };
        if !item.canvas.visible {
            continue;
        }
        let other = item.canvas.world_bounds();
        if !other.is_finite() {
            continue;
        }
        for (axis, vertical) in [(0usize, true), (1usize, false)] {
            for mine in lines(moving, vertical) {
                for theirs in lines(other, vertical) {
                    let delta = theirs - mine;
                    if delta.abs() > threshold {
                        continue;
                    }
                    let closer = best[axis].is_none_or(|(current, _)| delta.abs() < current.abs());
                    if !closer {
                        continue;
                    }
                    // ช่วงของเส้นต้องคลุมทั้งสองกรอบ ผู้ใช้จึงเห็นว่ามันตรงกับใบไหน
                    let (from, to) = if vertical {
                        (moving.min.y.min(other.min.y), moving.max.y.max(other.max.y))
                    } else {
                        (moving.min.x.min(other.min.x), moving.max.x.max(other.max.x))
                    };
                    best[axis] = Some((
                        delta,
                        Guide {
                            vertical,
                            at: theirs,
                            from,
                            to,
                        },
                    ));
                }
            }
        }
    }

    if let Some((delta, guide)) = best[0] {
        snap.offset.x = delta;
        snap.guides.push(guide);
    }
    if let Some((delta, guide)) = best[1] {
        snap.offset.y = delta;
        snap.guides.push(guide);
    }
    snap
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

    /// board ที่มีภาพตามตำแหน่ง/ขนาดที่กำหนด
    fn board_of(specs: &[(Vec2, Vec2)]) -> (Board, Vec<ItemId>, SpatialIndex) {
        let mut board = Board::default();
        let ids = specs
            .iter()
            .enumerate()
            .map(|(i, (pos, size))| {
                let mut item = image_item(u8::try_from(i).unwrap_or(0));
                item.canvas = ItemCanvas {
                    pos: *pos,
                    size: *size,
                    ..ItemCanvas::default()
                };
                board.insert_item(item)
            })
            .collect();
        board.mark_dirty(false);
        let index = SpatialIndex::from_board(&board);
        (board, ids, index)
    }

    /// viewport กว้าง ๆ สำหรับเทสต์ — ของจริงมาจากกล้อง
    const VIEW: Rect = Rect {
        min: Vec2::new(-10_000.0, -10_000.0),
        max: Vec2::new(10_000.0, 10_000.0),
    };

    fn snapshot(board: &Board, ids: &[ItemId]) -> Vec<(ItemId, ItemCanvas)> {
        ids.iter()
            .filter_map(|id| board.item(*id).map(|item| (*id, item.canvas)))
            .collect()
    }

    // ---------- align ----------

    #[test]
    fn aligning_left_lines_up_the_left_edges() {
        let (board, ids, _) = board_of(&[
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),   // ซ้าย -50
            (Vec2::new(200.0, 50.0), Vec2::splat(40.0)), // ซ้าย 180
        ]);
        let out = aligned(&snapshot(&board, &ids), Align::Left).unwrap();
        let moved: Vec<Vec2> = out.iter().map(|(_, canvas)| canvas.pos).collect();
        // ขอบซ้ายของกรอบรวมคือ -50 → ใบที่สอง (กว้าง 40) ต้องมีกึ่งกลางที่ -30
        assert_eq!(moved, vec![Vec2::new(-30.0, 50.0)]);
        assert_eq!(out.len(), 1, "ใบที่ชิดอยู่แล้วต้องไม่ถูกแตะ");
    }

    #[test]
    fn every_align_direction_lines_up_the_edge_it_promises() {
        let specs = [
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),
            (Vec2::new(300.0, 200.0), Vec2::splat(60.0)),
            (Vec2::new(150.0, 400.0), Vec2::splat(20.0)),
        ];
        let (board, ids, _) = board_of(&specs);
        let before = snapshot(&board, &ids);
        let union = union_of(&before).unwrap();

        for (how, pick) in [
            (Align::Left, 0usize),
            (Align::CentreX, 1),
            (Align::Right, 2),
            (Align::Top, 3),
            (Align::CentreY, 4),
            (Align::Bottom, 5),
        ] {
            let out = aligned(&before, how).unwrap();
            // ประกอบสภาพหลัง align ขึ้นมาใหม่
            let mut after = before.clone();
            for (id, canvas) in &out {
                for slot in &mut after {
                    if slot.0 == *id {
                        slot.1 = *canvas;
                    }
                }
            }
            let edges: Vec<f32> = after
                .iter()
                .map(|(_, canvas)| {
                    let rect = canvas.world_bounds();
                    match pick {
                        0 => rect.min.x,
                        1 => rect.center().x,
                        2 => rect.max.x,
                        3 => rect.min.y,
                        4 => rect.center().y,
                        _ => rect.max.y,
                    }
                })
                .collect();
            let want = match pick {
                0 => union.min.x,
                1 => union.center().x,
                2 => union.max.x,
                3 => union.min.y,
                4 => union.center().y,
                _ => union.max.y,
            };
            for edge in &edges {
                assert!(
                    (edge - want).abs() < 1e-3,
                    "{how:?}: ได้ {edges:?} ควรตรงกันที่ {want}"
                );
            }
        }
    }

    /// ★ กด align ซ้ำครั้งที่สองต้องไม่กิน undo เพิ่ม (I-1 + undo stack ที่อ่านได้)
    #[test]
    fn aligning_something_already_aligned_changes_nothing() {
        let (board, ids, _) = board_of(&[
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),
            (Vec2::new(0.0, 300.0), Vec2::splat(100.0)),
        ]);
        assert!(aligned(&snapshot(&board, &ids), Align::Left).is_none());
        assert!(aligned(&snapshot(&board, &ids), Align::CentreX).is_none());
    }

    #[test]
    fn aligning_needs_at_least_two_items() {
        let (board, ids, _) = board_of(&[(Vec2::ZERO, Vec2::splat(50.0))]);
        assert!(aligned(&snapshot(&board, &ids), Align::Left).is_none());
        assert!(aligned(&[], Align::Left).is_none());
    }

    /// align แนวนอนต้องไม่แตะแกน y เลย (และกลับกัน)
    #[test]
    fn aligning_one_axis_never_touches_the_other() {
        let (board, ids, _) = board_of(&[
            (Vec2::new(0.0, 10.0), Vec2::splat(100.0)),
            (Vec2::new(200.0, 90.0), Vec2::splat(40.0)),
        ]);
        let before = snapshot(&board, &ids);
        for (id, canvas) in aligned(&before, Align::Left).unwrap() {
            let was = before.iter().find(|slot| slot.0 == id).unwrap().1;
            assert_eq!(canvas.pos.y, was.pos.y, "align ซ้ายต้องไม่ขยับแกน y");
        }
        for (id, canvas) in aligned(&before, Align::Top).unwrap() {
            let was = before.iter().find(|slot| slot.0 == id).unwrap().1;
            assert_eq!(canvas.pos.x, was.pos.x, "align บนต้องไม่ขยับแกน x");
        }
    }

    // ---------- distribute ----------

    /// ★ ช่องว่างระหว่างขอบต้องเท่ากัน และหัวท้ายต้องไม่ขยับ
    #[test]
    fn distributing_makes_the_gaps_between_edges_equal() {
        let (board, ids, _) = board_of(&[
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),  //
            (Vec2::new(120.0, 0.0), Vec2::splat(20.0)), // ใบเล็กเบียดอยู่ใกล้ ๆ
            (Vec2::new(500.0, 0.0), Vec2::splat(60.0)),
        ]);
        let before = snapshot(&board, &ids);
        let out = distributed(&before, Distribute::Horizontal).unwrap();

        let mut after = before.clone();
        for (id, canvas) in &out {
            for slot in &mut after {
                if slot.0 == *id {
                    slot.1 = *canvas;
                }
            }
        }
        let mut rects: Vec<Rect> = after.iter().map(|(_, c)| c.world_bounds()).collect();
        rects.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
        let gaps: Vec<f32> = rects.windows(2).map(|w| w[1].min.x - w[0].max.x).collect();
        assert!((gaps[0] - gaps[1]).abs() < 1e-3, "ช่องว่างต้องเท่ากัน: {gaps:?}");
        // หัวท้ายไม่ขยับ
        assert_eq!(rects.first().unwrap().min.x, -50.0);
        assert_eq!(rects.last().unwrap().max.x, 530.0);
    }

    #[test]
    fn distributing_needs_at_least_three_items() {
        let (board, ids, _) = board_of(&[
            (Vec2::ZERO, Vec2::splat(50.0)),
            (Vec2::new(100.0, 0.0), Vec2::splat(50.0)),
        ]);
        assert!(distributed(&snapshot(&board, &ids), Distribute::Horizontal).is_none());
    }

    /// ★ ผลต้อง deterministic — ภาพที่ขอบตรงกันเป๊ะต้องได้ลำดับเดิมทุกครั้ง
    /// (CLAUDE.md: ห้ามให้ผลขึ้นกับลำดับ iteration)
    #[test]
    fn distributing_is_deterministic_when_edges_tie() {
        let (board, ids, _) = board_of(&[
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),
            (Vec2::new(400.0, 0.0), Vec2::splat(100.0)),
        ]);
        let before = snapshot(&board, &ids);
        let first = distributed(&before, Distribute::Horizontal);
        for _ in 0..20 {
            assert_eq!(distributed(&before, Distribute::Horizontal), first);
        }
    }

    // ---------- alignment guide ----------

    /// ★★ snap เข้าหา **ขอบของภาพอื่น** ไม่ใช่กริด
    #[test]
    fn dragging_near_another_edge_snaps_to_it_and_reports_a_guide() {
        let (board, ids, index) = board_of(&[
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),   // ขอบซ้าย -50
            (Vec2::new(0.0, 400.0), Vec2::splat(100.0)), // ตัวที่กำลังลาก
        ]);
        // ลากมาจนขอบซ้ายอยู่ที่ -47 (ห่างจาก -50 อยู่ 3 หน่วย)
        let moving = Rect::from_center_size(Vec2::new(3.0, 400.0), Vec2::splat(100.0));
        let snap = snap_to_neighbours(&board, &index, moving, VIEW, &[ids[1]], 8.0);

        assert_eq!(snap.offset.x, -3.0, "ต้องดึงกลับให้ขอบตรงกันเป๊ะ");
        assert_eq!(snap.offset.y, 0.0, "แกน y ไม่มีอะไรใกล้ ต้องไม่ขยับ");
        assert_eq!(snap.guides.len(), 1);
        assert!(snap.guides[0].vertical);
        assert_eq!(snap.guides[0].at, -50.0);
        // เส้นต้องคลุมทั้งสองใบ ผู้ใช้จึงเห็นว่ามันตรงกับใบไหน
        assert!(snap.guides[0].from <= -50.0 && snap.guides[0].to >= 450.0);
    }

    /// กึ่งกลางก็ต้อง snap ได้ ไม่ใช่แค่ขอบ
    #[test]
    fn centres_snap_too_not_only_edges() {
        let (board, ids, index) = board_of(&[
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),
            (Vec2::new(0.0, 400.0), Vec2::splat(40.0)),
        ]);
        // กึ่งกลางของตัวที่ลากอยู่ห่างจากกึ่งกลางของใบแรก 2 หน่วย
        let moving = Rect::from_center_size(Vec2::new(2.0, 400.0), Vec2::splat(40.0));
        let snap = snap_to_neighbours(&board, &index, moving, VIEW, &[ids[1]], 8.0);
        assert_eq!(snap.offset.x, -2.0);
        assert_eq!(snap.guides[0].at, 0.0, "ตรงกับกึ่งกลาง ไม่ใช่ขอบ");
    }

    /// ★ ไกลเกินระยะ = ไม่ snap และ **ไม่มีเส้น** (ไม่งั้นจอเต็มไปด้วยเส้นตลอดเวลา)
    #[test]
    fn nothing_snaps_when_everything_is_far_away() {
        let (board, ids, index) = board_of(&[
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),
            (Vec2::new(0.0, 400.0), Vec2::splat(100.0)),
        ]);
        let moving = Rect::from_center_size(Vec2::new(900.0, 400.0), Vec2::splat(100.0));
        let snap = snap_to_neighbours(&board, &index, moving, VIEW, &[ids[1]], 8.0);
        assert_eq!(snap, Snap::default());
        assert!(snap.guides.is_empty());
    }

    /// ★ ห้าม snap เข้าหาตัวเอง — ไม่งั้นภาพที่ลากจะล็อกอยู่กับที่ตลอด
    #[test]
    fn the_dragged_items_never_snap_to_themselves() {
        let (board, ids, index) = board_of(&[(Vec2::new(0.0, 0.0), Vec2::splat(100.0))]);
        let moving = Rect::from_center_size(Vec2::new(2.0, 0.0), Vec2::splat(100.0));
        let snap = snap_to_neighbours(&board, &index, moving, VIEW, &ids, 8.0);
        assert_eq!(snap, Snap::default(), "ตัวเดียวบน board ต้องไม่มีอะไรให้ snap");
    }

    /// เลือกทั้งสองแกนได้พร้อมกัน (มุมตรงกับมุม)
    #[test]
    fn both_axes_can_snap_at_once() {
        let (board, ids, index) = board_of(&[
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),
            (Vec2::new(500.0, 500.0), Vec2::splat(100.0)),
        ]);
        let moving = Rect::from_center_size(Vec2::new(3.0, -3.0), Vec2::splat(100.0));
        let snap = snap_to_neighbours(&board, &index, moving, VIEW, &[ids[1]], 8.0);
        assert_eq!(snap.offset, Vec2::new(-3.0, 3.0));
        assert_eq!(snap.guides.len(), 2);
        assert!(snap.guides.iter().any(|g| g.vertical));
        assert!(snap.guides.iter().any(|g| !g.vertical));
    }

    /// ★★ ที่ 1000 ภาพ การหาไกด์ต้องดูแค่ภาพรอบ ๆ ไม่ใช่ทั้ง board
    ///
    /// วัดด้วย **จำนวน item ที่ index คืนมา** ไม่ใช่จับเวลา (docs/08 §3.9 ข้อ 5b)
    #[test]
    fn finding_guides_looks_at_neighbours_not_the_whole_board() {
        let specs: Vec<(Vec2, Vec2)> = (0..1000)
            .map(|i| {
                let (col, row) = (i % 40, i / 40);
                #[expect(clippy::cast_precision_loss, reason = "ดัชนีเล็กกว่า 1000")]
                let grid = Vec2::new(col as f32 * 300.0, row as f32 * 300.0);
                (grid, Vec2::splat(100.0))
            })
            .collect();
        let (board, _, index) = board_of(&specs);
        assert_eq!(board.len(), 1000);

        let moving = Rect::from_center_size(Vec2::new(150.0, 150.0), Vec2::splat(100.0));
        let examined = index
            .query(Rect {
                min: Vec2::new(moving.min.x - 8.0, VIEW.min.y),
                max: Vec2::new(moving.max.x + 8.0, VIEW.max.y),
            })
            .len();
        assert!(
            examined < 20,
            "ดูไป {examined} ใบจาก 1000 — ต้องเป็นแค่เพื่อนบ้าน ไม่ใช่ทั้ง board"
        );
        // และผลลัพธ์ต้องยังถูก
        let snap = snap_to_neighbours(&board, &index, moving, VIEW, &[], 8.0);
        assert!(snap.guides.is_empty(), "ตรงกลางช่องว่าง ไม่มีอะไรใกล้");
    }

    /// I-4: ค่าที่พังต้องไม่ทำให้ snap คืนขยะ
    #[test]
    fn non_finite_input_produces_no_snap() {
        let (board, ids, index) = board_of(&[
            (Vec2::new(0.0, 0.0), Vec2::splat(100.0)),
            (Vec2::new(0.0, 400.0), Vec2::splat(100.0)),
        ]);
        let broken = Rect {
            min: Vec2::new(f32::NAN, 0.0),
            max: Vec2::new(10.0, 10.0),
        };
        assert_eq!(
            snap_to_neighbours(&board, &index, broken, VIEW, &[ids[1]], 8.0),
            Snap::default()
        );
        let fine = Rect::from_center_size(Vec2::new(3.0, 400.0), Vec2::splat(100.0));
        assert_eq!(
            snap_to_neighbours(&board, &index, fine, VIEW, &[ids[1]], f32::NAN),
            Snap::default()
        );
    }
}
