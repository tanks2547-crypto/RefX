//! Arrange mode — **virtual scrolling** (P3-3)
//!
//! ที่นี่คือที่แรกที่ layout engine ของ P3-2 ถูกเอามาใช้บนจอจริง
//!
//! หน้าที่มีสามอย่างและแยกกันชัด ๆ:
//!
//! 1. **จัดแผ่น** ([`Sheet`]) — เรียก `refx_core::layout` ครั้งเดียวต่อการเปลี่ยนแปลง
//!    แล้วเก็บผลไว้ · 10,000 ใบไม่ได้จัดใหม่ทุกเฟรมที่ผู้ใช้หมุนล้อ
//! 2. **หาว่าใครอยู่ในจอ** — binary search บนดัชนีที่เรียงตามขอบบน แล้วเดินไปข้างหน้า
//!    เท่าที่จำเป็น · **ไม่ไล่ทั้ง 10,000 ใบต่อเฟรม**
//! 3. **นับ** — จำนวนที่ *ตรวจ* และจำนวนที่ *วาด* เป็นตัวเลขที่เห็นได้บน status bar
//!    เพราะเกณฑ์ของ ROADMAP คือ "วาดจริง < 60 ตัว" ซึ่งต้องพิสูจน์ด้วยตัวเลข
//!    ไม่ใช่ด้วยคำว่า "มี culling แล้ว" (docs/08 §3.9 ข้อ 6)
//!
//! ★ **นับแทนการจับเวลา** — ตัวชี้วัดของโมดูลนี้เป็นจำนวนใบที่ถูกเปิดดู
//! ซึ่งเท่ากันทุกเครื่อง ไม่ใช่มิลลิวินาทีที่ขึ้นกับ runner (docs/08 §3.9 ข้อ 5b
//! หลักการเดียวกับ `spatial::hit_testing_a_thousand_items…` ที่นับ `items_examined`)
//!
//! ★★ **ห้ามแตะ `Board`** — การสลับโหมดเป็นการเปลี่ยน *มุมมอง* ล้วน ๆ
//! ไม่สร้าง `Command` ไม่ทำให้ `dirty` (docs/03 §4.3) · แผ่นนี้จึงอ่านอย่างเดียว
//! และตำแหน่งที่คำนวณได้จะลงไปที่ `ItemCanvas` ก็ต่อเมื่อผู้ใช้กด
//! "Apply layout to canvas" ซึ่งเป็น **P3-5** ไม่ใช่ที่นี่
//!
//! spec: docs/03-modes-and-ui.md §3, docs/04-rendering.md §5, ROADMAP P3-3

use glam::Vec2;
use refx_core::arena::ItemId;
use refx_core::layout::{Engine, LayoutParams, Placed};
use refx_core::view::Camera;

/// ขนาดช่องที่อยากได้ (หน่วย point — คูณ `pixels_per_point` ก่อนใช้)
///
/// ★ ใช้กำหนด **จำนวนคอลัมน์** ไม่ใช่กำหนดขนาดตายตัว: ช่องจริงจะกว้าง
/// `(viewport - gap) / columns` ซึ่งพอดีขอบเสมอไม่ว่าหน้าต่างกว้างเท่าไหร่
///
/// ทำไมไม่ปล่อยให้ `LayoutParams::columns = None`: engine จะเดาเป็นตาราง
/// ใกล้จัตุรัส (√n) ซึ่งที่ 10,000 ใบแปลว่า **100 คอลัมน์** — ภาพเล็กกว่าไอคอน
/// และแผ่นกว้างกว่าจอ 20 เท่า จำนวนคอลัมน์ของ contact sheet ต้องมาจาก
/// *ความกว้างของจอ* ไม่ใช่จากจำนวนภาพ
pub const TARGET_CELL_PT: f32 = 200.0;

/// ช่องว่างระหว่างช่อง (point)
pub const GAP_PT: f32 = 12.0;

/// กันชนบน/ล่าง คิดเป็นสัดส่วนของความสูงจอ
///
/// docs/03 §3 ระบุ "แถวที่อยู่ในจอ **+ 1 หน้าจอเป็น buffer บนล่าง**"
/// — เผื่อไว้ให้แถวถัดไปพร้อมอยู่แล้วตอนผู้ใช้สะบัดล้อ ไม่ใช่ค่อยโผล่ตามหลัง
pub const BUFFER_SCREENS: f32 = 1.0;

/// ตัวเลขของเฟรมล่าสุด — **หลักฐานของเกณฑ์ P3-3 ที่เห็นได้ด้วยตา**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// จำนวน item ทั้งหมดบนแผ่น
    pub total: usize,
    /// ที่ทับกับ **จอจริง** (ไม่รวมกันชน)
    pub in_view: usize,
    /// ที่ส่งไปวาด = จอ + กันชนบนล่าง (`BUFFER_SCREENS`)
    pub in_band: usize,
    /// กี่ใบที่ต้องเปิดดูเพื่อได้ชุดข้างบน — ★ ตัวชี้วัดที่แทนการจับเวลา
    ///
    /// ถ้าวันหนึ่งมีคนเปลี่ยนการค้นกลับไปเป็นการไล่ทั้งรายการ ตัวเลขนี้จะพุ่ง
    /// เป็น `total` ทันทีและเทสต์จะแดง — ต่างจากการวัดเวลาที่ต้องรอเครื่องช้าพอ
    pub examined: usize,
}

/// แผ่นที่จัดไว้แล้วหนึ่งแผ่น + ดัชนีสำหรับค้นตามแนวตั้ง
///
/// ★ เก็บ `by_top`/`tops` แยกจาก `placed` เพราะ binary search อ่านแต่ `f32`
/// ตัวเดียวต่อใบ — ไล่บน `Vec<Placed>` (24 ไบต์/ใบ) จะกิน cache line ฟรี ๆ
#[derive(Debug, Default)]
pub struct Sheet {
    /// ผลของ layout **ตามลำดับ input** (ผู้เรียกจับคู่กับรายการเดิมได้ตรง ๆ)
    placed: Vec<Placed>,
    /// ดัชนีใน `placed` เรียงตามขอบบน — เสมอกันตัดสินด้วยดัชนี (deterministic)
    by_top: Vec<u32>,
    /// ขอบบนของ `by_top[i]` — เรียงจากน้อยไปมากเสมอ
    tops: Vec<f32>,
    /// ความสูงของใบที่สูงที่สุดบนแผ่น
    ///
    /// ★★ **ตัวนี้คือความถูกต้องของการค้นทั้งหมด**: ใบที่ขอบบนอยู่เหนือจอ
    /// ยังทับจอได้ถ้ามันสูงพอ · ถ้าไม่ถอยหลังไปเท่านี้ ภาพสูง ๆ จะ**หายจากจอ
    /// เฉพาะตอนเลื่อนผ่านครึ่งล่างของมัน** ซึ่งดูเหมือนภาพกะพริบ ไม่เหมือนบั๊ก
    tallest: f32,
    /// ขนาดของแผ่นทั้งใบ (ใช้ clamp การเลื่อน)
    content: Vec2,
}

impl Sheet {
    /// จัดแผ่นใหม่ทั้งใบ
    fn build(&mut self, items: &[(ItemId, Vec2)], engine: Engine, params: LayoutParams) {
        self.placed = refx_core::layout::layout(engine, items, params);

        self.by_top.clear();
        self.by_top
            .extend((0..self.placed.len()).map(|i| u32::try_from(i).unwrap_or(u32::MAX)));
        // ★ เรียงตามขอบบน แล้ว **ตัดสินเสมอด้วยดัชนีเดิม** — ค่าที่เท่ากันเป๊ะ
        //   เป็นเรื่องปกติมาก (ทั้งแถวมีขอบบนเดียวกัน) ถ้าปล่อยให้ตัวเรียงตัดสินเอง
        //   ลำดับการวาดจะสลับกันเองระหว่างเฟรม = ภาพกะพริบสลับที่ (docs/03 §3)
        let placed = &self.placed;
        self.by_top.sort_unstable_by(|a, b| {
            let (ia, ib) = (*a as usize, *b as usize);
            placed[ia]
                .top_left
                .y
                .total_cmp(&placed[ib].top_left.y)
                .then(a.cmp(b))
        });

        self.tops.clear();
        self.tops
            .extend(self.by_top.iter().map(|i| placed[*i as usize].top_left.y));

        self.tallest = placed.iter().map(|p| p.size.y).fold(0.0, f32::max);
        let right = placed
            .iter()
            .map(|p| p.top_left.x + p.size.x)
            .fold(0.0, f32::max);
        let bottom = placed
            .iter()
            .map(|p| p.top_left.y + p.size.y)
            .fold(0.0, f32::max);
        self.content = Vec2::new(right, bottom);
    }

    /// ขนาดของแผ่นทั้งใบ
    #[must_use]
    pub fn content(&self) -> Vec2 {
        self.content
    }

    /// จำนวน item บนแผ่น
    #[must_use]
    pub fn len(&self) -> usize {
        self.placed.len()
    }

    /// แผ่นว่างหรือยัง
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }

    /// ทุกใบที่ทับแถบ `top..=bottom` — เขียนลง `out` แล้วคืนจำนวนใบที่ **เปิดดู**
    ///
    /// ★ นี่คือหัวใจของ virtual scrolling: `partition_point` หาใบแรกที่เป็นไปได้
    /// (`top - tallest`) แล้วเดินไปข้างหน้าจนขอบบนเลย `bottom` — ต้นทุนเป็น
    /// `O(log n + จำนวนใบในแถบ)` **ไม่ใช่ `O(n)`**
    ///
    /// ผลลัพธ์เรียงตามขอบบนเสมอ (บน → ล่าง) ซึ่งเป็นลำดับที่ดีต่อการวาดด้วย
    fn band(&self, top: f32, bottom: f32, out: &mut Vec<Placed>) -> usize {
        out.clear();
        if self.placed.is_empty() || !top.is_finite() || !bottom.is_finite() {
            return 0;
        }
        // ใบที่ขอบบนอยู่เหนือ `top - tallest` ไม่มีทางยาวลงมาถึงแถบนี้ได้
        let start = self.tops.partition_point(|t| *t < top - self.tallest);
        let mut examined = 0usize;
        for slot in start..self.tops.len() {
            if self.tops[slot] > bottom {
                break; // เรียงแล้ว — ตัวถัดไปยิ่งอยู่ต่ำลงไปอีก
            }
            examined += 1;
            let placed = self.placed[self.by_top[slot] as usize];
            if placed.top_left.y + placed.size.y >= top {
                out.push(placed);
            }
        }
        examined
    }

    /// จำนวนใบที่ทับแถบนี้ (ไม่เก็บผล) — ใช้นับ "อยู่ในจอจริงกี่ใบ"
    fn count_in(&self, top: f32, bottom: f32) -> usize {
        if self.placed.is_empty() || !top.is_finite() || !bottom.is_finite() {
            return 0;
        }
        let start = self.tops.partition_point(|t| *t < top - self.tallest);
        let mut hits = 0usize;
        for slot in start..self.tops.len() {
            if self.tops[slot] > bottom {
                break;
            }
            let placed = self.placed[self.by_top[slot] as usize];
            if placed.top_left.y + placed.size.y >= top {
                hits += 1;
            }
        }
        hits
    }
}

/// มุมมอง Arrange ทั้งก้อน — แผ่น + ตำแหน่งที่เลื่อนไป + ตัวเลขของเฟรมล่าสุด
#[derive(Debug, Default)]
pub struct ArrangeView {
    engine: Engine,
    sheet: Sheet,
    /// ระยะที่เลื่อนลงมาแล้ว (world unit = physical pixel ที่ zoom 1)
    scroll: f32,
    /// กรอบของ viewport ตอน [`ArrangeView::plan`] ครั้งล่าสุด (physical pixel)
    viewport: Vec2,
    /// ต้องจัดแผ่นใหม่ไหม
    ///
    /// ★ ตั้งจาก `rebuild_quads` ซึ่งเป็น **ประตูเดียว** ที่รู้ว่า board เปลี่ยน
    /// — P3-4 จะเปลี่ยนไปเทียบ `board.revision` ตามที่ docs/03 §3 เขียนไว้
    /// (ตอนนี้ `Board` ยังไม่มีฟิลด์นั้น และการเพิ่มมันคือของ P3-4 ไม่ใช่ที่นี่)
    dirty: bool,
    /// คีย์ของแผ่นที่จัดไว้ — เปลี่ยนเมื่อไหร่ต้องจัดใหม่
    key: Option<SheetKey>,
    /// จำนวนครั้งที่จัดแผ่นจริง — หลักฐานว่า cache ทำงาน (เทสต์อ่านค่านี้)
    rebuilds: u64,
    /// ชุดที่ต้องวาดเฟรมนี้ — ★ ถือเป็นฟิลด์เพื่อ **ไม่จองหน่วยความจำใหม่ทุกเฟรม**
    visible: Vec<Placed>,
    counts: Counts,
}

/// สิ่งที่ทำให้ต้องจัดแผ่นใหม่
#[derive(Debug, Clone, Copy, PartialEq)]
struct SheetKey {
    engine: Engine,
    params: LayoutParams,
    count: usize,
}

impl ArrangeView {
    /// มุมมองเปล่า
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// engine ที่ใช้อยู่
    #[must_use]
    pub fn engine(&self) -> Engine {
        self.engine
    }

    /// เปลี่ยน engine — แผ่นจะถูกจัดใหม่ในเฟรมถัดไป
    pub fn set_engine(&mut self, engine: Engine) {
        self.engine = engine;
    }

    /// board เปลี่ยน → แผ่นเดิมใช้ไม่ได้แล้ว
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// ตัวเลขของเฟรมล่าสุด
    #[must_use]
    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// จำนวนครั้งที่ layout ถูกคำนวณจริง (หลักฐานของ cache)
    #[must_use]
    pub fn rebuilds(&self) -> u64 {
        self.rebuilds
    }

    /// ระยะที่เลื่อนลงมาแล้ว
    #[must_use]
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// ชุดที่ต้องวาดเฟรมนี้ (เรียงจากบนลงล่าง)
    #[must_use]
    pub fn visible(&self) -> &[Placed] {
        &self.visible
    }

    /// ระยะเลื่อนได้มากสุด — `0` เมื่อแผ่นสั้นกว่าจอ
    #[must_use]
    pub fn max_scroll(&self) -> f32 {
        (self.sheet.content().y - self.viewport.y).max(0.0)
    }

    /// เลื่อนตามล้อ — คืน `true` เมื่อ **ตำแหน่งเปลี่ยนจริง**
    ///
    /// ★ ค่าที่คืนคือสิ่งที่ตัดสินว่าจะขอเฟรมใหม่ไหม (I-1) — หมุนล้อค้างไว้ตอน
    /// สุดขอบแล้วต้องไม่วาดใหม่ทุกเฟรม ไม่งั้นโปรแกรมกิน CPU ตอนที่ภาพนิ่งสนิท
    pub fn scroll_by(&mut self, delta: f32) -> bool {
        if !delta.is_finite() {
            return false;
        }
        let want = (self.scroll - delta).clamp(0.0, self.max_scroll());
        // ค่าเท่ากันเป๊ะ = ไม่มีอะไรเปลี่ยน (เทียบ bit ตรง ๆ ถูกต้องกว่า epsilon ที่นี่
        // เพราะเราถามว่า "ต้องวาดใหม่ไหม" ไม่ได้ถามว่า "ใกล้เคียงกันไหม")
        let moved = want.to_bits() != self.scroll.to_bits();
        self.scroll = want;
        moved
    }

    /// เตรียมเฟรม: จัดแผ่นถ้าจำเป็น → หาชุดที่ต้องวาด → อัปเดตตัวเลข
    ///
    /// `items` ถูกเรียก **เฉพาะตอนที่ต้องจัดแผ่นใหม่จริง ๆ** — ที่ 10,000 ใบ
    /// การสร้าง `Vec` ของ aspect ทุกเฟรมคือการเผา CPU ทิ้งระหว่างที่ผู้ใช้แค่เลื่อน
    ///
    /// ★★ **`count` เข้ามาเป็นพารามิเตอร์เพราะสัญญาต้องไม่พึ่งจังหวะของผู้เรียก**
    /// (docs/08 §3.9 ข้อ 8) — ถ้าอาศัย [`ArrangeView::invalidate`] อย่างเดียว
    /// วันที่มีคนเพิ่มเส้นทางที่แก้ board แล้วลืมเรียก แผ่นจะค้างอยู่รุ่นเก่า
    /// **เงียบ ๆ** · จำนวนที่ไม่ตรงถูกจับได้ที่นี่โดยไม่ต้องมีใครจำ
    ///
    /// `viewport` เป็น physical pixel · `ppp` คือ `pixels_per_point` ของจอ
    pub fn plan<F>(&mut self, viewport: Vec2, ppp: f32, count: usize, items: F)
    where
        F: FnOnce() -> Vec<(ItemId, Vec2)>,
    {
        if !viewport.is_finite() || viewport.x < 1.0 || viewport.y < 1.0 {
            return;
        }
        self.viewport = viewport;
        let params = params_for(viewport, ppp);

        // ---- 1. จัดแผ่น (เฉพาะเมื่อจำเป็น) ----
        let key = SheetKey {
            engine: self.engine,
            params,
            count,
        };
        if self.dirty || self.key != Some(key) {
            let list = items();
            self.sheet.build(&list, self.engine, params);
            // เชื่อจำนวนจริงที่ได้มา ไม่ใช่ที่ผู้เรียกบอก — ไม่งั้นความไม่ตรงกัน
            // จะทำให้จัดใหม่ทุกเฟรมโดยไม่มีใครรู้
            self.key = Some(SheetKey {
                count: list.len(),
                ..key
            });
            self.dirty = false;
            self.rebuilds += 1;
        }

        // ---- 2. หาชุดที่ต้องวาด ----
        // เลื่อนใหม่ให้อยู่ในระยะเสมอ — หน้าต่างที่ถูกย่อลงทำให้ค่าเดิมเกินขอบได้
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        let (top, bottom) = (self.scroll, self.scroll + viewport.y);
        let buffer = viewport.y * BUFFER_SCREENS;
        let examined = self
            .sheet
            .band(top - buffer, bottom + buffer, &mut self.visible);

        self.counts = Counts {
            total: self.sheet.len(),
            in_view: self.sheet.count_in(top, bottom),
            in_band: self.visible.len(),
            examined,
        };
    }

    /// กล้องที่วาดแผ่นนี้ — **zoom คงที่ 1.0**
    ///
    /// Arrange เป็น contact sheet ไม่ใช่ระนาบอิสระ: ขนาดภาพมาจากจำนวนคอลัมน์
    /// ไม่ใช่จากการซูม · ล้อจึงเป็น "เลื่อน" ไม่ใช่ "ซูม" เหมือนใน Canvas
    #[must_use]
    pub fn camera(&self) -> Camera {
        Camera::new(
            Vec2::new(self.viewport.x * 0.5, self.scroll + self.viewport.y * 0.5),
            1.0,
        )
    }
}

/// พารามิเตอร์ของแผ่นที่ได้จากขนาด viewport
///
/// ★ คอลัมน์มาจาก **ความกว้างของจอ** ไม่ใช่จำนวนภาพ (ดู [`TARGET_CELL_PT`])
#[must_use]
pub fn params_for(viewport: Vec2, ppp: f32) -> LayoutParams {
    let scale = if ppp.is_finite() && ppp > 0.0 {
        ppp
    } else {
        1.0
    };
    let cell = TARGET_CELL_PT * scale;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "หารด้วยค่าคงที่บวกแล้ว clamp ก่อนแปลง — อยู่ในช่วง u32 เสมอ"
    )]
    let columns = (viewport.x / cell).round().clamp(1.0, 64.0) as u32;
    LayoutParams {
        width: viewport.x,
        gap: GAP_PT * scale,
        // ช่องของ Grid เป็นจัตุรัสอยู่แล้ว ค่านี้มีผลกับ engine ที่คิดเป็นแถว
        target_row_height: cell,
        columns: Some(columns),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        // เทียบ f32 ตรง ๆ ถูกต้องที่นี่: เราถามว่า "สุดขอบพอดีไหม" ซึ่งเป็นค่าที่
        // `clamp` ผลิตออกมาเป๊ะ ไม่ใช่ผลของการคำนวณสะสม
        clippy::float_cmp
    )]

    use super::*;
    use refx_core::arena::ArenaKey as _;

    fn ids(n: u32) -> Vec<(ItemId, Vec2)> {
        (0..n)
            .map(|i| {
                let aspect = match i % 3 {
                    0 => Vec2::new(4.0, 3.0),
                    1 => Vec2::new(3.0, 4.0),
                    _ => Vec2::new(1.0, 1.0),
                };
                (ItemId::from_parts(i, 0), aspect)
            })
            .collect()
    }

    /// viewport ที่ใช้ในเทสต์ทั้งไฟล์ — ใกล้เคียงช่อง canvas จริงบนจอ 1280×800
    const VIEW: Vec2 = Vec2::new(840.0, 600.0);

    fn planned(count: u32) -> ArrangeView {
        let mut view = ArrangeView::new();
        view.plan(VIEW, 1.0, count as usize, || ids(count));
        view
    }

    /// ★★★ เกณฑ์ของ ROADMAP: 10,000 ใบ แล้ว **วาดจริงไม่ถึง 60**
    #[test]
    fn ten_thousand_items_draw_fewer_than_sixty() {
        let view = planned(10_000);
        let counts = view.counts();
        // ★ พิมพ์ตัวเลขไว้เสมอ — เกณฑ์ข้อนี้เป็น *ตัวเลข* ไม่ใช่คำว่า "มี culling"
        //   (`cargo test -- --nocapture` หรือ `cargo nextest run --no-capture`)
        println!(
            "P3-3 @ {}x{} : total {} · ในจอ {} · วาด {} · ตรวจ {} · แผ่นสูง {:.0}",
            VIEW.x,
            VIEW.y,
            counts.total,
            counts.in_view,
            counts.in_band,
            counts.examined,
            view.sheet.content().y
        );
        assert_eq!(counts.total, 10_000);
        assert!(
            counts.in_band < 60,
            "วาดจริง {} ใบ — เกณฑ์ ROADMAP P3-3 คือน้อยกว่า 60 \
             (ในจอ {} ใบ · กันชน {BUFFER_SCREENS} หน้าจอ)",
            counts.in_band,
            counts.in_view
        );
        assert!(counts.in_view > 0, "จอต้องมีอะไรให้เห็นบ้าง");
    }

    /// ★★ ต้นทุนของการหาว่าใครอยู่ในจอ **ไม่ขึ้นกับจำนวนภาพบน board**
    ///
    /// นับใบที่เปิดดูแทนการจับเวลา (docs/08 §3.9 ข้อ 5b) — เลขนี้เท่ากันทุกเครื่อง
    /// ส่วนมิลลิวินาทีไม่เท่า · ถ้าใครเปลี่ยนการค้นกลับไปไล่ทั้งรายการ
    /// `examined` จะกลายเป็นหลักพันแล้วเทสต์นี้แดงทันที
    ///
    /// ★★★ **ต้องเลื่อนลงไปก่อน ไม่งั้นเทสต์นี้ไม่ได้ตรวจอะไรเลย** — เวอร์ชันแรก
    /// วัดที่ `scroll = 0` ซึ่ง "ใบที่อยู่เหนือแถบ" มีศูนย์ใบพอดี การไล่ทั้งรายการ
    /// จึงให้ตัวเลขเท่ากับ binary search เป๊ะ · negative control จับได้:
    /// เปลี่ยน `partition_point` เป็น `start = 0` แล้ว **เทสต์ยังเขียว**
    /// (docs/08 §3.9 ข้อ 1 — negative control ที่ไม่แดงแปลว่าเทสต์อ่อน)
    #[test]
    fn finding_the_visible_band_does_not_scan_the_whole_board() {
        let mut small = planned(200);
        small.scroll = small.max_scroll();
        small.plan(VIEW, 1.0, 200, || ids(200));
        let small = small.counts();

        let mut big = planned(10_000);
        big.scroll = big.max_scroll();
        big.plan(VIEW, 1.0, 10_000, || ids(10_000));
        let big = big.counts();

        assert!(
            big.examined <= small.examined + 4,
            "ตรวจ {} ใบที่ 10,000 ภาพ เทียบกับ {} ใบที่ 200 ภาพ — การค้นไม่ควรโตตาม board",
            big.examined,
            small.examined
        );
        assert!(
            big.examined < big.total / 50,
            "ตรวจ {} จาก {} ใบ — นี่คือการไล่ทั้งรายการ ไม่ใช่ virtual scrolling",
            big.examined,
            big.total
        );
    }

    /// เลื่อนไปตรงไหนก็ต้องเห็นของ และจำนวนที่วาดต้องไม่บานตามตำแหน่ง
    #[test]
    fn scrolling_anywhere_keeps_the_drawn_count_bounded() {
        let mut view = planned(10_000);
        let max = view.max_scroll();
        assert!(max > 0.0, "แผ่น 10,000 ใบต้องยาวกว่าจอ");
        for step in 0..=10 {
            #[expect(clippy::cast_precision_loss, reason = "0..=10")]
            let target = max * (step as f32) / 10.0;
            view.scroll = target;
            view.plan(VIEW, 1.0, 10_000, || ids(10_000));
            let counts = view.counts();
            assert!(
                counts.in_band < 60,
                "ที่ scroll {target}: วาด {} ใบ",
                counts.in_band
            );
            assert!(
                counts.in_view > 0,
                "ที่ scroll {target}: จอว่างเปล่า — แถวหายไประหว่างทาง"
            );
        }
    }

    /// ★ ชุดที่วาดต้องเป็น **ทุกใบ** ที่ทับแถบจริง ๆ — ไม่ขาดสักใบ
    ///
    /// เทียบกับการไล่ตรวจทั้งรายการแบบตรงไปตรงมา (ช้าแต่ถูกแน่นอน)
    /// ถ้า binary search ถอยหลังไม่พอ ใบสูง ๆ จะหลุด แล้วผู้ใช้เห็นภาพกะพริบหาย
    #[test]
    fn the_fast_search_finds_exactly_what_a_full_scan_would() {
        let mut view = ArrangeView::new();
        let list = ids(2_000);
        for scroll in [0.0f32, 137.0, 999.0, 5_000.0, 12_345.0] {
            view.plan(VIEW, 1.0, list.len(), || list.clone());
            view.scroll = scroll.min(view.max_scroll());
            view.plan(VIEW, 1.0, list.len(), || list.clone());

            let buffer = VIEW.y * BUFFER_SCREENS;
            let (top, bottom) = (view.scroll - buffer, view.scroll + VIEW.y + buffer);
            let mut want: Vec<ItemId> = view
                .sheet
                .placed
                .iter()
                .filter(|p| p.top_left.y <= bottom && p.top_left.y + p.size.y >= top)
                .map(|p| p.id)
                .collect();
            let mut got: Vec<ItemId> = view.visible().iter().map(|p| p.id).collect();
            want.sort_unstable();
            got.sort_unstable();
            assert_eq!(want, got, "ชุดที่วาดไม่ตรงกับการไล่ทั้งรายการ ที่ scroll {scroll}");
        }
    }

    /// ★★ ใบที่สูงกว่าทั้งจอต้องไม่หายตอนเลื่อนผ่านครึ่งล่างของมัน
    ///
    /// นี่คือเคสที่ `Sheet::tallest` มีอยู่เพราะมัน — ขอบบนของภาพอยู่เหนือแถบไปแล้ว
    /// แต่ตัวภาพยังพาดลงมาถึง · ถ้าไม่ถอยหลังไปเท่าความสูงสูงสุด ภาพจะหายเงียบ ๆ
    #[test]
    fn a_very_tall_item_is_still_found_when_its_top_is_far_above() {
        let mut view = ArrangeView::new();
        // ★ ต้องเป็น Masonry: Grid บีบทุกใบให้พอดีช่องจัตุรัส จึงไม่มีใบไหนสูงเกินจอ
        //   ได้เลย — เคสที่ `tallest` มีไว้แก้จะไม่มีทางเกิดถ้าทดสอบด้วย Grid
        view.set_engine(Engine::Masonry);
        // ใบแรกสูงมาก (aspect ผอมสุด ๆ) ที่เหลือปกติ
        let list: Vec<(ItemId, Vec2)> =
            std::iter::once((ItemId::from_parts(0, 0), Vec2::new(1.0, 40.0)))
                .chain((1..300).map(|i| (ItemId::from_parts(i, 0), Vec2::new(4.0, 3.0))))
                .collect();
        view.plan(VIEW, 1.0, list.len(), || list.clone());
        let tall = view.sheet.placed[0];
        assert!(
            tall.size.y > VIEW.y,
            "เคสนี้ต้องมีใบที่สูงกว่าจอจริง ๆ ถึงจะตรวจสิ่งที่ตั้งใจ (สูง {})",
            tall.size.y
        );

        // เลื่อนไปให้ขอบบนของมันอยู่เหนือแถบกันชนไปแล้ว แต่ตัวมันยังพาดถึงจอ
        let inside = tall.top_left.y + tall.size.y - 10.0;
        view.scroll = inside.min(view.max_scroll());
        view.plan(VIEW, 1.0, list.len(), || list.clone());
        assert!(
            view.visible().iter().any(|p| p.id == tall.id),
            "ใบที่สูงกว่าจอหายไปตอนเลื่อนผ่านครึ่งล่างของมัน"
        );
    }

    /// ★ กันชนบน/ล่างมีจริง — ไม่ใช่วาดเฉพาะที่ตาเห็น (docs/03 §3)
    #[test]
    fn the_band_reaches_one_screen_above_and_below_the_view() {
        let mut view = planned(2_000);
        view.scroll = view.max_scroll() * 0.5;
        view.plan(VIEW, 1.0, 2_000, || ids(2_000));
        let counts = view.counts();
        assert!(
            counts.in_band > counts.in_view,
            "กันชนหายไป: วาด {} ใบ เท่ากับที่อยู่ในจอพอดี",
            counts.in_band
        );
        // แถวเหนือจอหนึ่งหน้าจอต้องอยู่ในชุดที่วาด
        let above = view
            .visible()
            .iter()
            .filter(|p| p.top_left.y + p.size.y < view.scroll)
            .count();
        assert!(above > 0, "ไม่มีแถวไหนอยู่เหนือจอเลย — กันชนบนไม่ทำงาน");
    }

    /// ★★ แผ่นต้องไม่ถูกจัดใหม่ถ้าไม่มีอะไรเปลี่ยน — เลื่อน 100 ครั้ง = จัด 1 ครั้ง
    ///
    /// ที่ 10,000 ใบการจัดใหม่ทุกเฟรมคือการเผา CPU ทิ้งขณะที่ผู้ใช้แค่หมุนล้อ
    /// (docs/03 §3 บังคับ cache ไว้กับ filter อยู่แล้วด้วยเหตุผลเดียวกัน)
    #[test]
    fn scrolling_never_recomputes_the_layout() {
        let mut view = ArrangeView::new();
        view.plan(VIEW, 1.0, 10_000, || ids(10_000));
        assert_eq!(view.rebuilds(), 1);
        for _ in 0..100 {
            view.scroll_by(-50.0);
            view.plan(VIEW, 1.0, 10_000, || panic!("จัดแผ่นใหม่ทั้งที่แค่เลื่อน"));
        }
        assert_eq!(view.rebuilds(), 1, "แผ่นถูกจัดใหม่ระหว่างเลื่อน");
    }

    /// เปลี่ยนจำนวน item / ขนาดจอ / engine แล้วต้องจัดใหม่จริง
    #[test]
    fn the_sheet_is_rebuilt_when_something_that_matters_changes() {
        let mut view = ArrangeView::new();
        view.plan(VIEW, 1.0, 100, || ids(100));
        assert_eq!(view.rebuilds(), 1);

        // จอกว้างขึ้น → จำนวนคอลัมน์เปลี่ยน
        view.plan(Vec2::new(1_600.0, 600.0), 1.0, 100, || ids(100));
        assert_eq!(view.rebuilds(), 2, "จอกว้างขึ้นแล้วแผ่นต้องจัดใหม่");

        // board เปลี่ยน
        view.invalidate();
        view.plan(Vec2::new(1_600.0, 600.0), 1.0, 101, || ids(101));
        assert_eq!(view.rebuilds(), 3, "board เปลี่ยนแล้วแผ่นต้องจัดใหม่");

        // engine เปลี่ยน
        view.set_engine(Engine::Masonry);
        view.plan(Vec2::new(1_600.0, 600.0), 1.0, 101, || ids(101));
        assert_eq!(view.rebuilds(), 4, "เปลี่ยน engine แล้วแผ่นต้องจัดใหม่");
    }

    /// ★ I-1: เลื่อนจนสุดแล้วหมุนต่อ ต้องไม่รายงานว่า "มีอะไรเปลี่ยน"
    #[test]
    fn scrolling_past_the_end_asks_for_no_redraw() {
        let mut view = planned(300);
        assert!(view.scroll_by(-1_000.0), "เลื่อนลงครั้งแรกต้องขยับ");
        while view.scroll_by(-1_000.0) {}
        assert!(
            !view.scroll_by(-1_000.0),
            "สุดขอบล่างแล้วยังบอกว่าขยับ — เฟรมจะถูกขอใหม่ไม่รู้จบ (ผิด I-1)"
        );
        while view.scroll_by(1_000.0) {}
        assert!(!view.scroll_by(1_000.0), "สุดขอบบนแล้วยังบอกว่าขยับ");
        assert_eq!(view.scroll(), 0.0);
    }

    /// แผ่นที่สั้นกว่าจอ เลื่อนไม่ได้เลย
    #[test]
    fn a_sheet_shorter_than_the_screen_does_not_scroll() {
        let mut view = planned(3);
        assert_eq!(view.max_scroll(), 0.0);
        assert!(!view.scroll_by(-500.0));
    }

    /// board ว่าง = ไม่มีอะไรวาด และต้องไม่ panic
    #[test]
    fn an_empty_board_draws_nothing() {
        let view = planned(0);
        assert_eq!(view.counts(), Counts::default());
        assert!(view.visible().is_empty());
    }

    /// ★ ผลต้องเหมือนเดิมเป๊ะทุกครั้ง รวมทั้ง **ลำดับ** ที่ส่งไปวาด
    ///
    /// ลำดับที่สลับไปมาระหว่างเฟรมทำให้ภาพที่ซ้อนกันสลับหน้า/หลังเอง
    /// ซึ่งผู้ใช้เห็นเป็นภาพกะพริบ และหาสาเหตุยากมาก (docs/03 §3)
    #[test]
    fn the_same_board_always_yields_the_same_window_in_the_same_order() {
        let first: Vec<(ItemId, Vec2)> = planned(5_000)
            .visible()
            .iter()
            .map(|p| (p.id, p.top_left))
            .collect();
        for _ in 0..3 {
            let again: Vec<(ItemId, Vec2)> = planned(5_000)
                .visible()
                .iter()
                .map(|p| (p.id, p.top_left))
                .collect();
            assert_eq!(first, again, "ผลไม่คงที่ระหว่างการเรียกซ้ำ");
        }
    }

    /// ★ จำนวนคอลัมน์มาจากความกว้างของจอ ไม่ใช่จากจำนวนภาพ
    ///
    /// ปล่อยให้ engine เดา (`columns = None`) ที่ 10,000 ใบจะได้ 100 คอลัมน์
    /// ซึ่งแปลว่าภาพเล็กกว่าไอคอนและแผ่นกว้างกว่าจอ 20 เท่า
    #[test]
    fn the_column_count_follows_the_window_not_the_item_count() {
        let narrow = params_for(Vec2::new(840.0, 600.0), 1.0);
        let wide = params_for(Vec2::new(2_400.0, 600.0), 1.0);
        assert!(
            wide.columns > narrow.columns,
            "จอกว้างขึ้นแล้วคอลัมน์ต้องมากขึ้น: {:?} vs {:?}",
            narrow.columns,
            wide.columns
        );
        // ★ และแผ่นต้องไม่กว้างเกินจอ ไม่งั้นจะต้องเลื่อนแนวนอนซึ่ง Arrange ไม่มี
        let view = planned(10_000);
        assert!(
            view.sheet.content().x <= VIEW.x + 1.0,
            "แผ่นกว้าง {} เกินจอ {} — จะมีคอลัมน์ที่มองไม่เห็นตลอดกาล",
            view.sheet.content().x,
            VIEW.x
        );
    }

    /// จอ DPI สูงต้องได้ช่องขนาดเท่ากันเมื่อวัดเป็น point
    #[test]
    fn a_hidpi_screen_gets_the_same_grid_measured_in_points() {
        let at_100 = params_for(Vec2::new(840.0, 600.0), 1.0);
        let at_150 = params_for(Vec2::new(1_260.0, 900.0), 1.5);
        assert_eq!(at_100.columns, at_150.columns);
        assert!((at_150.gap / at_100.gap - 1.5).abs() < 1e-3);
    }
}
