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
use refx_core::board::{Board, SortKey};
use refx_core::layout::{Engine, LayoutParams, Placed};
use refx_core::query::{self, Filter};
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
    /// ★ วิธีเรียง + ตัวกรอง (P3-4)
    ///
    /// ★★ **อยู่ที่ชั้น UI ไม่ใช่ใน `Board`** — `Board::arrange` มี `sort`/`descending`
    /// อยู่จริงและ persist ลง `.refx` แต่ทุกการแก้ `Board` ต้องผ่าน `Command`
    /// (docs/08 §4 ข้อ 10) ซึ่งแปลว่า **กดเปลี่ยนวิธีเรียงแล้วกิน undo หนึ่งขั้น**
    /// — ผู้ใช้ที่สลับไปดูแบบเรียงตามดาวแล้วกด Ctrl+Z ต้องได้ *งาน* คืน ไม่ใช่ได้
    /// วิธีเรียงคืน (เหตุผลเดียวกับที่ `G` ทั้ง board ไม่เข้า undo — §2.7)
    /// · และ **ตัวกรองไม่มีที่เก็บใน `Board` เลย** (docs/02 §2 ไม่มีฟิลด์นั้น)
    /// ซึ่งเป็นหลักฐานว่ามันเป็นสถานะของ *เครื่องมือ* ไม่ใช่ของ *เอกสาร*
    /// → ตัดสินว่าให้ทั้งคู่อยู่ด้วยกันที่นี่ · ถ้าวันหนึ่งต้องการให้ persist
    /// ให้ทำตอน P4-1 พร้อมกับตัดสินเรื่อง undo ไปด้วยกัน
    sort: SortKey,
    descending: bool,
    filter: Filter,
    /// ลำดับของ item หลังกรอง+เรียง — **ผลที่ถูก cache ไว้ตาม `board.revision`**
    order: Vec<ItemId>,
    /// คีย์ของ `order` ที่คำนวณไว้
    query_key: Option<QueryKey>,
    /// รุ่นของ `order` — ขยับทุกครั้งที่มันถูกคำนวณใหม่ (ใช้เป็นส่วนหนึ่งของ `SheetKey`)
    ///
    /// ★ ต้องมี เพราะ "ลำดับเปลี่ยนแต่จำนวนเท่าเดิม" เป็นเรื่องปกติที่สุด
    /// (กดสลับทิศการเรียง) — คีย์ที่ดูแค่จำนวนจะไม่จัดแผ่นใหม่แล้วภาพไม่ขยับเลย
    order_generation: u64,
    /// ★★ จำนวนครั้งที่ **กรอง+เรียงจริง** — หลักฐานของเกณฑ์ ROADMAP P3-4
    /// ("ไม่คำนวณซ้ำเมื่อไม่มีอะไรเปลี่ยน — วัดด้วย counter")
    queries: u64,
    /// ต้องจัดแผ่นใหม่ไหม
    ///
    /// ★ ตั้งจาก `rebuild_quads` · ตั้งแต่ P3-4 มี `board.revision` เป็นตาข่ายหลัก
    /// แล้ว ตัวนี้เหลือไว้เป็นชั้นสองสำหรับการเปลี่ยนที่ไม่ผ่าน `Board`
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
    /// รุ่นของลำดับที่กรอง+เรียงมาแล้ว (ดู `ArrangeView::order_generation`)
    order_generation: u64,
}

/// สิ่งที่ทำให้ต้องกรอง+เรียงใหม่ — **นี่คือ cache ที่ docs/03 §3 สั่งไว้**
///
/// ★ `revision` ตัวเดียวครอบการแก้ board ทุกชนิด (เพิ่ม/ลบ/ติดดาว/ติดแท็ก/ย้ายชั้น)
/// เพราะ `Board` บวกมันในตัวแก้ทุกตัว และมีเทสต์ `every_mutation_bumps_the_revision`
/// บังคับไว้ — ไม่ใช่รายการเงื่อนไขที่คนเขียนต้องจำให้ครบ
#[derive(Debug, Clone, PartialEq)]
struct QueryKey {
    revision: u64,
    sort: SortKey,
    descending: bool,
    filter: Filter,
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

    /// วิธีเรียงที่ใช้อยู่
    #[must_use]
    pub fn sort(&self) -> (SortKey, bool) {
        (self.sort, self.descending)
    }

    /// ตั้งวิธีเรียง — คืน `true` เมื่อ **เปลี่ยนจริง** (ผู้เรียกใช้ตัดสินว่าจะขอเฟรมไหม)
    pub fn set_sort(&mut self, sort: SortKey, descending: bool) -> bool {
        let changed = self.sort != sort || self.descending != descending;
        self.sort = sort;
        self.descending = descending;
        changed
    }

    /// ตัวกรองที่ใช้อยู่
    #[must_use]
    pub fn filter(&self) -> &Filter {
        &self.filter
    }

    /// ตั้งตัวกรอง — คืน `true` เมื่อเปลี่ยนจริง
    pub fn set_filter(&mut self, filter: Filter) -> bool {
        let changed = self.filter != filter;
        if changed {
            self.filter = filter;
        }
        changed
    }

    /// กลับไปบนสุดของแผ่น — ใช้ตอนลำดับเปลี่ยน (เรียง/กรองใหม่)
    ///
    /// ★ ถ้าไม่ทำ ผู้ใช้ที่เลื่อนอยู่กลางแผ่นแล้วกดเรียงใหม่จะยังเห็นกลางแผ่นเหมือนเดิม
    /// ซึ่งอ่านว่า "กดแล้วไม่มีอะไรเกิดขึ้น" ทั้งที่ลำดับเปลี่ยนไปหมดแล้ว
    pub fn scroll_to_top(&mut self) {
        self.scroll = 0.0;
    }

    /// ★★ จำนวนครั้งที่ **กรอง+เรียงจริง** — เกณฑ์ ROADMAP P3-4 วัดด้วยตัวเลขนี้
    #[must_use]
    pub fn queries(&self) -> u64 {
        self.queries
    }

    /// ลำดับที่ผ่านการกรอง+เรียงแล้ว (ผลที่ cache ไว้)
    #[must_use]
    pub fn order(&self) -> &[ItemId] {
        &self.order
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

    /// ★ ผลของ layout **ทั้งแผ่น** ไม่ใช่แค่ที่อยู่ในจอ (P3-5)
    ///
    /// ปุ่ม "ส่งเข้า canvas" ต้องได้ทุกใบที่ผ่านตัวกรอง ไม่ใช่เฉพาะที่ตาเห็นตอนกด
    /// — ผู้ใช้ที่เลื่อนอยู่กลางแผ่นแล้วกดปุ่ม ย่อมหมายถึงทั้งแผ่น
    #[must_use]
    pub fn placed(&self) -> &[Placed] {
        &self.sheet.placed
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

    /// เตรียมเฟรม: กรอง+เรียง (ถ้าจำเป็น) → จัดแผ่น (ถ้าจำเป็น) → หาชุดที่ต้องวาด
    ///
    /// ★★ **ทั้งสองขั้นมี cache ของตัวเอง** เพราะมันเปลี่ยนคนละจังหวะกัน:
    /// ย่อหน้าต่างทำให้ต้องจัดแผ่นใหม่แต่ไม่ต้องกรองใหม่ · ติดดาวเพิ่มหนึ่งใบ
    /// ทำให้ต้องกรองใหม่และจัดแผ่นใหม่ · เลื่อนอย่างเดียวไม่ต้องทำทั้งคู่
    ///
    /// `aspect` ถูกเรียก **เฉพาะตอนจัดแผ่นใหม่จริง ๆ** — ที่หลายพันใบ การสร้าง
    /// `Vec` ของ aspect ทุกเฟรมคือการเผา CPU ทิ้งระหว่างที่ผู้ใช้แค่เลื่อน
    ///
    /// `viewport` เป็น physical pixel · `ppp` คือ `pixels_per_point` ของจอ
    pub fn plan<F>(&mut self, board: &Board, viewport: Vec2, ppp: f32, aspect: F)
    where
        F: Fn(ItemId) -> Vec2,
    {
        if !viewport.is_finite() || viewport.x < 1.0 || viewport.y < 1.0 {
            return;
        }
        self.viewport = viewport;
        let params = params_for(viewport, ppp);

        // ---- 1. กรอง + เรียง (เฉพาะเมื่อ board หรือเงื่อนไขเปลี่ยน) ----
        //
        // ★ `revision` เปลี่ยนเมื่อ **เนื้อหา** board เปลี่ยนเท่านั้น — เลื่อน/ซูม/
        //   ย่อหน้าต่างไม่ทำให้มันขยับ จึงไม่มีการกรองใหม่ระหว่างที่ผู้ใช้แค่เลื่อน
        let query_key = QueryKey {
            revision: board.revision(),
            sort: self.sort,
            descending: self.descending,
            filter: self.filter.clone(),
        };
        if self.query_key.as_ref() != Some(&query_key) {
            self.order = query::select(board, &self.filter, self.sort, self.descending);
            self.query_key = Some(query_key);
            self.order_generation = self.order_generation.wrapping_add(1);
            self.queries += 1;
        }

        // ---- 2. จัดแผ่น (เฉพาะเมื่อจำเป็น) ----
        let key = SheetKey {
            engine: self.engine,
            params,
            count: self.order.len(),
            order_generation: self.order_generation,
        };
        if self.dirty || self.key != Some(key) {
            let list: Vec<(ItemId, Vec2)> =
                self.order.iter().map(|id| (*id, aspect(*id))).collect();
            self.sheet.build(&list, self.engine, params);
            self.key = Some(key);
            self.dirty = false;
            self.rebuilds += 1;
        }

        // ---- 3. หาชุดที่ต้องวาด ----
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
    use crate::shell::SORT_CHOICES;
    use refx_core::board::{AssetRef, ImageFormat, Item, ItemKind, ItemMeta};
    use refx_core::command::{AddItems, EditMeta, GroupItems, History, MetaField, SetGroup};
    use refx_core::hash::ContentHash;
    use refx_core::query::LabelFilter;

    /// viewport ที่ใช้ในเทสต์ทั้งไฟล์ — ใกล้เคียงช่อง canvas จริงบนจอ 1280×800
    const VIEW: Vec2 = Vec2::new(840.0, 600.0);

    /// สัดส่วนของใบที่ `index` — วนสามแบบให้แผ่นมีทั้งแนวตั้ง/นอน/จัตุรัส
    fn px_size(index: u32) -> glam::UVec2 {
        match index % 3 {
            0 => glam::UVec2::new(400, 300),
            1 => glam::UVec2::new(300, 400),
            _ => glam::UVec2::new(300, 300),
        }
    }

    fn image(index: u32) -> Item {
        Item::new(ItemKind::Image(AssetRef {
            hash: ContentHash::from_bytes([(index % 251) as u8; 32]),
            path: std::path::PathBuf::from(format!("img{index:05}.png")),
            px_size: px_size(index),
            format: ImageFormat::Png,
            embedded: false,
            mtime: 0,
            file_size: 0,
        }))
    }

    /// board ที่มี `n` ภาพ — ผ่าน `AddItems` เหมือนเส้นทางจริง (I-3)
    fn board_of(n: u32) -> Board {
        let mut board = Board::default();
        let mut history = History::default();
        let items: Vec<Item> = (0..n).map(image).collect();
        if let Ok(command) = AddItems::new(items) {
            history.apply(&mut board, Box::new(command)).unwrap();
        }
        board
    }

    /// aspect ที่ชั้น UI ส่งให้ — อ่านจาก `AssetRef` เหมือนของจริง
    fn aspect_of(board: &Board) -> impl Fn(ItemId) -> Vec2 + '_ {
        move |id| {
            board.item(id).map_or(Vec2::ONE, |item| match &item.kind {
                ItemKind::Image(asset) => Vec2::new(asset.px_size.x as f32, asset.px_size.y as f32),
                _ => Vec2::ONE,
            })
        }
    }

    fn planned(board: &Board) -> ArrangeView {
        let mut view = ArrangeView::new();
        view.plan(board, VIEW, 1.0, aspect_of(board));
        view
    }

    /// ให้ดาวกับ item ผ่าน `Command` เหมือนที่ inspector ทำจริง
    fn set_rating(board: &mut Board, history: &mut History, id: ItemId, rating: u8) {
        let before = board.item(id).map(|item| item.meta.clone()).unwrap();
        let after = ItemMeta { rating, ..before };
        let command = EditMeta::new(MetaField::Rating, vec![(id, after)]).unwrap();
        history.apply(board, Box::new(command)).unwrap();
    }

    // ---------- P3-3: virtual scrolling ----------

    /// ★★★ เกณฑ์ของ ROADMAP: 3,072 ใบ (เพดานจริงของ board) แล้ว **วาดจริงไม่ถึง 60**
    #[test]
    fn a_full_board_draws_fewer_than_sixty() {
        let board = board_of(3_072);
        let view = planned(&board);
        let counts = view.counts();
        // ★ พิมพ์ตัวเลขไว้เสมอ — เกณฑ์ข้อนี้เป็น *ตัวเลข* ไม่ใช่คำว่า "มี culling"
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
        assert_eq!(counts.total, 3_072);
        assert!(
            counts.in_band < 60,
            "วาดจริง {} ใบ — เกณฑ์ ROADMAP P3-3 คือน้อยกว่า 60 (ในจอ {} ใบ)",
            counts.in_band,
            counts.in_view
        );
        assert!(counts.in_view > 0, "จอต้องมีอะไรให้เห็นบ้าง");
    }

    /// ★★ กลไกต้องรับ 10,000 ใบได้ แม้ board จริงจะตันที่ 3,072 (ROADMAP P3-3)
    #[test]
    fn ten_thousand_items_draw_fewer_than_sixty() {
        let board = board_of(10_000);
        let counts = planned(&board).counts();
        assert_eq!(counts.total, 10_000);
        assert!(counts.in_band < 60, "วาดจริง {} ใบ", counts.in_band);
    }

    /// ★★ ต้นทุนของการหาว่าใครอยู่ในจอ **ไม่ขึ้นกับจำนวนภาพบน board**
    ///
    /// นับใบที่เปิดดูแทนการจับเวลา (docs/08 §3.9 ข้อ 5b) — เลขนี้เท่ากันทุกเครื่อง
    ///
    /// ★★★ **ต้องเลื่อนลงไปก่อน ไม่งั้นเทสต์นี้ไม่ได้ตรวจอะไรเลย** — เวอร์ชันแรก
    /// วัดที่ `scroll = 0` ซึ่ง "ใบที่อยู่เหนือแถบ" มีศูนย์ใบพอดี การไล่ทั้งรายการ
    /// จึงให้ตัวเลขเท่ากับ binary search เป๊ะ (negative control ไม่แดง = เทสต์อ่อน)
    #[test]
    fn finding_the_visible_band_does_not_scan_the_whole_board() {
        let small_board = board_of(200);
        let mut small = planned(&small_board);
        small.scroll = small.max_scroll();
        small.plan(&small_board, VIEW, 1.0, aspect_of(&small_board));

        let big_board = board_of(10_000);
        let mut big = planned(&big_board);
        big.scroll = big.max_scroll();
        big.plan(&big_board, VIEW, 1.0, aspect_of(&big_board));

        let (small, big) = (small.counts(), big.counts());
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
        let board = board_of(3_072);
        let mut view = planned(&board);
        let max = view.max_scroll();
        assert!(max > 0.0, "แผ่น 3,072 ใบต้องยาวกว่าจอ");
        for step in 0..=10 {
            #[expect(clippy::cast_precision_loss, reason = "0..=10")]
            let target = max * (step as f32) / 10.0;
            view.scroll = target;
            view.plan(&board, VIEW, 1.0, aspect_of(&board));
            let counts = view.counts();
            assert!(
                counts.in_band < 60,
                "ที่ scroll {target}: วาด {} ใบ",
                counts.in_band
            );
            assert!(counts.in_view > 0, "ที่ scroll {target}: จอว่างเปล่า");
        }
    }

    /// ★ ชุดที่วาดต้องเป็น **ทุกใบ** ที่ทับแถบจริง ๆ — เทียบกับการไล่ทั้งรายการ
    #[test]
    fn the_fast_search_finds_exactly_what_a_full_scan_would() {
        let board = board_of(2_000);
        let mut view = planned(&board);
        for scroll in [0.0f32, 137.0, 999.0, 5_000.0, 12_345.0] {
            view.scroll = scroll.min(view.max_scroll());
            view.plan(&board, VIEW, 1.0, aspect_of(&board));

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
    #[test]
    fn a_very_tall_item_is_still_found_when_its_top_is_far_above() {
        // ★ ต้องเป็น Masonry: Grid บีบทุกใบให้พอดีช่องจัตุรัส จึงไม่มีใบไหนสูงเกินจอ
        let mut board = Board::default();
        let mut history = History::default();
        let mut tall = Item::new(ItemKind::Image(AssetRef {
            hash: ContentHash::from_bytes([9; 32]),
            path: std::path::PathBuf::from("tall.png"),
            px_size: glam::UVec2::new(10, 400),
            format: ImageFormat::Png,
            embedded: false,
            mtime: 0,
            file_size: 0,
        }));
        tall.meta = ItemMeta::default();
        let mut items = vec![tall];
        items.extend((1..300).map(image));
        history
            .apply(&mut board, Box::new(AddItems::new(items).unwrap()))
            .unwrap();

        let mut view = ArrangeView::new();
        view.set_engine(Engine::Masonry);
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        let first = view.sheet.placed[0];
        assert!(
            first.size.y > VIEW.y,
            "เคสนี้ต้องมีใบที่สูงกว่าจอจริง ๆ ถึงจะตรวจสิ่งที่ตั้งใจ (สูง {})",
            first.size.y
        );

        let inside = first.top_left.y + first.size.y - 10.0;
        view.scroll = inside.min(view.max_scroll());
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        assert!(
            view.visible().iter().any(|p| p.id == first.id),
            "ใบที่สูงกว่าจอหายไปตอนเลื่อนผ่านครึ่งล่างของมัน"
        );
    }

    /// ★ กันชนบน/ล่างมีจริง — ไม่ใช่วาดเฉพาะที่ตาเห็น (docs/03 §3)
    #[test]
    fn the_band_reaches_one_screen_above_and_below_the_view() {
        let board = board_of(2_000);
        let mut view = planned(&board);
        view.scroll = view.max_scroll() * 0.5;
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        let counts = view.counts();
        assert!(
            counts.in_band > counts.in_view,
            "กันชนหายไป: วาด {} ใบ เท่ากับที่อยู่ในจอพอดี",
            counts.in_band
        );
        let above = view
            .visible()
            .iter()
            .filter(|p| p.top_left.y + p.size.y < view.scroll)
            .count();
        assert!(above > 0, "ไม่มีแถวไหนอยู่เหนือจอเลย — กันชนบนไม่ทำงาน");
    }

    /// ★★ แผ่นต้องไม่ถูกจัดใหม่ถ้าไม่มีอะไรเปลี่ยน — เลื่อน 100 ครั้ง = จัด 1 ครั้ง
    #[test]
    fn scrolling_never_recomputes_the_layout() {
        let board = board_of(3_072);
        let mut view = planned(&board);
        assert_eq!(view.rebuilds(), 1);
        for _ in 0..100 {
            view.scroll_by(-50.0);
            view.plan(&board, VIEW, 1.0, |_| panic!("จัดแผ่นใหม่ทั้งที่แค่เลื่อน"));
        }
        assert_eq!(view.rebuilds(), 1, "แผ่นถูกจัดใหม่ระหว่างเลื่อน");
    }

    /// เปลี่ยนขนาดจอ / engine แล้วต้องจัดใหม่จริง
    #[test]
    fn the_sheet_is_rebuilt_when_something_that_matters_changes() {
        let board = board_of(100);
        let mut view = planned(&board);
        assert_eq!(view.rebuilds(), 1);

        let wide = Vec2::new(1_600.0, 600.0);
        view.plan(&board, wide, 1.0, aspect_of(&board));
        assert_eq!(view.rebuilds(), 2, "จอกว้างขึ้นแล้วแผ่นต้องจัดใหม่");

        view.set_engine(Engine::Masonry);
        view.plan(&board, wide, 1.0, aspect_of(&board));
        assert_eq!(view.rebuilds(), 3, "เปลี่ยน engine แล้วแผ่นต้องจัดใหม่");
    }

    /// ★ I-1: เลื่อนจนสุดแล้วหมุนต่อ ต้องไม่รายงานว่า "มีอะไรเปลี่ยน"
    #[test]
    fn scrolling_past_the_end_asks_for_no_redraw() {
        let board = board_of(300);
        let mut view = planned(&board);
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
        let board = board_of(3);
        let mut view = planned(&board);
        assert_eq!(view.max_scroll(), 0.0);
        assert!(!view.scroll_by(-500.0));
    }

    /// board ว่าง = ไม่มีอะไรวาด และต้องไม่ panic
    #[test]
    fn an_empty_board_draws_nothing() {
        let board = Board::default();
        let view = planned(&board);
        assert_eq!(view.counts(), Counts::default());
        assert!(view.visible().is_empty());
    }

    /// ★ ผลต้องเหมือนเดิมเป๊ะทุกครั้ง รวมทั้ง **ลำดับ** ที่ส่งไปวาด
    #[test]
    fn the_same_board_always_yields_the_same_window_in_the_same_order() {
        let board = board_of(2_000);
        let first: Vec<(ItemId, Vec2)> = planned(&board)
            .visible()
            .iter()
            .map(|p| (p.id, p.top_left))
            .collect();
        for _ in 0..3 {
            let again: Vec<(ItemId, Vec2)> = planned(&board)
                .visible()
                .iter()
                .map(|p| (p.id, p.top_left))
                .collect();
            assert_eq!(first, again, "ผลไม่คงที่ระหว่างการเรียกซ้ำ");
        }
    }

    /// ★ จำนวนคอลัมน์มาจากความกว้างของจอ ไม่ใช่จากจำนวนภาพ
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
        let board = board_of(3_072);
        let view = planned(&board);
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

    // ---------- P3-4: sort + filter + cache ----------

    /// ★★★ เกณฑ์ของ ROADMAP P3-4: **ไม่คำนวณซ้ำเมื่อไม่มีอะไรเปลี่ยน**
    ///
    /// วัดด้วย counter ที่นับ "กรอง+เรียงจริงกี่ครั้ง" — 200 เฟรมที่ไม่มีอะไรเปลี่ยน
    /// (รวมทั้งการเลื่อน ซึ่งเป็นสิ่งที่ผู้ใช้ทำถี่ที่สุด) ต้องได้ **1 ครั้ง**
    #[test]
    fn nothing_changing_means_nothing_is_recomputed() {
        let board = board_of(3_072);
        let mut view = planned(&board);
        assert_eq!(view.queries(), 1, "รอบแรกต้องคำนวณหนึ่งครั้ง");
        for _ in 0..200 {
            view.scroll_by(-30.0);
            view.plan(&board, VIEW, 1.0, aspect_of(&board));
        }
        assert_eq!(
            view.queries(),
            1,
            "กรอง+เรียงใหม่ {} ครั้งทั้งที่ไม่มีอะไรเปลี่ยน",
            view.queries()
        );
    }

    /// ★★★ negative control ของ counter — **มันต้องขยับเมื่อมีอะไรเปลี่ยนจริง**
    ///
    /// counter ที่คืนเลขน้อย ๆ เสมอ (เช่นลืมเรียกเลย) จะผ่านเทสต์ข้างบนได้สบาย
    /// ทุกทางที่ทำให้ผลเปลี่ยนได้ต้องมีตัวอย่างอยู่ที่นี่
    #[test]
    fn the_counter_moves_for_every_way_the_result_can_change() {
        let mut board = board_of(20);
        let mut history = History::default();
        let mut view = planned(&board);
        let mut last = view.queries();
        assert_eq!(last, 1);

        let check = |view: &ArrangeView, what: &str, last: &mut u64| {
            assert!(
                view.queries() > *last,
                "{what} แล้ว counter ไม่ขยับ — cache ค้างอยู่รุ่นเก่า"
            );
            *last = view.queries();
        };

        // 1. เปลี่ยนวิธีเรียง
        assert!(view.set_sort(SortKey::Rating, false));
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        check(&view, "เปลี่ยนวิธีเรียง", &mut last);

        // 2. สลับทิศ
        assert!(view.set_sort(SortKey::Rating, true));
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        check(&view, "สลับทิศการเรียง", &mut last);

        // 3. เปลี่ยนตัวกรอง
        assert!(view.set_filter(Filter {
            min_rating: 3,
            ..Filter::default()
        }));
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        check(&view, "เปลี่ยนตัวกรอง", &mut last);

        // 4. board เปลี่ยน (ติดดาวหนึ่งใบ) — ★ ตัวที่ `revision` มีไว้เพื่อจับ
        let id = board.z_order()[0];
        set_rating(&mut board, &mut history, id, 5);
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        check(&view, "ติดดาวหนึ่งใบ", &mut last);

        // 5. undo ก็เปลี่ยนเนื้อหาเหมือนกัน
        history.undo(&mut board).unwrap();
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        check(&view, "undo", &mut last);
    }

    /// ★★ ตัวกรองต้อง **กรองจริง** และแผ่นต้องหดตาม ไม่ใช่แค่ตัวเลขบน status bar
    #[test]
    fn filtering_actually_shrinks_the_sheet() {
        let mut board = board_of(60);
        let mut history = History::default();
        for (index, id) in board.z_order().to_vec().into_iter().enumerate() {
            if index % 4 == 0 {
                set_rating(&mut board, &mut history, id, 5);
            }
        }
        let mut view = planned(&board);
        let before = view.counts().total;
        assert_eq!(before, 60);

        assert!(view.set_filter(Filter {
            min_rating: 5,
            ..Filter::default()
        }));
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        assert_eq!(view.counts().total, 15, "ควรเหลือเฉพาะใบที่ 5 ดาว");
        assert!(
            view.order()
                .iter()
                .all(|id| board.item(*id).is_some_and(|item| item.meta.rating == 5))
        );

        // ★ negative control: ล้างตัวกรองแล้วต้องกลับมาครบ ไม่ใช่หายถาวร
        assert!(view.set_filter(Filter::default()));
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        assert_eq!(view.counts().total, before, "ล้างตัวกรองแล้วภาพไม่กลับมา");
    }

    /// ★ เรียงแล้ว **ลำดับบนแผ่นต้องเปลี่ยนจริง** ไม่ใช่แค่ `order` เปลี่ยน
    ///
    /// แผ่นถูก cache แยกจากการกรอง/เรียง — ถ้าคีย์ของแผ่นดูแค่ *จำนวน* item
    /// การสลับทิศจะไม่ทำให้จัดใหม่ แล้วภาพบนจอจะไม่ขยับเลยทั้งที่ `order` ถูกแล้ว
    #[test]
    fn changing_the_sort_moves_the_images_not_just_the_list() {
        let board = board_of(40);
        let mut view = planned(&board);
        let first_up = view.visible().first().map(|p| p.id);

        assert!(view.set_sort(SortKey::Name, true));
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        let first_down = view.visible().first().map(|p| p.id);

        assert_ne!(
            first_up, first_down,
            "สลับทิศแล้วใบแรกบนแผ่นยังเป็นใบเดิม — แผ่นไม่ได้ถูกจัดใหม่"
        );
        assert_eq!(view.rebuilds(), 2, "แผ่นต้องถูกจัดใหม่พอดีหนึ่งครั้ง");
    }

    /// ★ ตั้งค่าเดิมซ้ำต้องไม่นับว่าเปลี่ยน (I-1: ไม่ขอเฟรมใหม่ฟรี ๆ)
    #[test]
    fn setting_the_same_sort_or_filter_again_changes_nothing() {
        let board = board_of(10);
        let mut view = planned(&board);
        assert!(!view.set_sort(SortKey::default(), false));
        assert!(!view.set_filter(Filter::default()));
        view.plan(&board, VIEW, 1.0, aspect_of(&board));
        assert_eq!(view.queries(), 1, "ตั้งค่าเดิมซ้ำแล้วยังคำนวณใหม่");
    }

    /// ตัวกรองที่ไม่กรองอะไรต้องบอกตัวเองได้ — UI ใช้ตัดสินว่าจะเตือนผู้ใช้ไหม
    #[test]
    fn an_open_filter_knows_it_is_open() {
        assert!(Filter::default().is_open());
        assert!(
            !Filter {
                label: LabelFilter::Unlabelled,
                ..Filter::default()
            }
            .is_open()
        );
    }
    // ---------- P3-8: สลับ mode 100 ครั้งแล้ว board ต้องเท่าเดิมเป๊ะ ----------

    /// ★★★ **สลับโหมด 100 ครั้ง แล้ว `Board` ทั้งก้อนต้องเหมือนเดิมเป๊ะ** (P3-8)
    ///
    /// docs/03 §4.3 เรียกข้อนี้ว่า "หลักที่ห้ามละเมิด" และ docs/02 §2.2 เรียกการที่
    /// `ItemCanvas` กับ `ItemMeta` อยู่คู่กันตลอดชีวิตของ item ว่า **ข้อกำหนดหลัก
    /// ของดีไซน์สองโหมดทั้งหมด** — เทสต์นี้คือตัวที่พิสูจน์ทั้งสองข้อพร้อมกัน
    ///
    /// ★★ **เทียบ `Board` ทั้งก้อน ไม่ใช่เช็คแค่ `dirty`** · `dirty` เป็นแค่ธง
    /// ที่ `History` คำนวณจากความลึกของ stack — มันจะยัง `false` อยู่ดีถ้ามีใคร
    /// แก้ board **นอกเส้นทาง `Command`** ซึ่งคือสภาพที่แย่ที่สุดที่เป็นไปได้
    /// (ข้อมูลเปลี่ยนโดยไม่มีทางย้อน และไม่มีอะไรบอกว่าต้องบันทึก) การเช็ค
    /// แค่ธงจึงเขียวได้ทั้งที่งานของผู้ใช้เพี้ยนไปแล้ว
    ///
    /// ★★★ **และเทียบ `revision` ด้วย ซึ่ง `PartialEq` ของ `Board` ไม่นับ**
    /// (§4 ข้อ 20 — undo ต้องคืนสภาพให้ "เท่าเดิม" ในสายตาผู้ใช้ ส่วนเลขรุ่น
    /// เดินหน้าอย่างเดียว) · นั่นทำให้ `revision` เป็นเครื่องมือที่ **แรงกว่า**
    /// `PartialEq` ตรงนี้พอดี: มันจับแม้แต่การเขียนที่ถูกเขียนกลับเป็นค่าเดิม
    /// ซึ่ง `==` มองไม่เห็นเลย ทุกตัวแก้ของ `Board` บวกมันหมด (`touch()`)
    #[test]
    fn a_hundred_mode_switches_leave_the_board_exactly_as_it_was() {
        let mut board = board_of(40);
        let mut history = History::default();

        // ให้ board มีของครบทุกฝั่ง ไม่ใช่ board เปล่า ๆ ที่พิสูจน์อะไรไม่ได้:
        // ฝั่ง Arrange (ดาว) + ฝั่ง Canvas (ตำแหน่ง/ขนาดจาก AddItems) + กลุ่ม
        let ids: Vec<ItemId> = board.z_order().to_vec();
        for (n, id) in ids.iter().take(6).enumerate() {
            set_rating(
                &mut board,
                &mut history,
                *id,
                u8::try_from(n % 6).unwrap_or(0),
            );
        }
        history
            .apply(
                &mut board,
                Box::new(GroupItems::new(ids[..4].to_vec(), "Group").unwrap()),
            )
            .unwrap();
        history.seal();
        // ยุบกลุ่มไว้ด้วย — เส้นทางที่ `select` ต้องทำงานเพิ่ม (P3-7)
        let group_id = board.item(ids[0]).unwrap().meta.group.unwrap();
        let current = board.group(group_id).unwrap().clone();
        history
            .apply(
                &mut board,
                Box::new(SetGroup::set_collapsed(group_id, &current, true)),
            )
            .unwrap();
        history.seal();
        // จุดเริ่ม = "เพิ่งบันทึกเสร็จ" ตามที่ ROADMAP บรรยาย (`dirty` ยัง false)
        history.mark_saved(&mut board);
        assert!(!board.is_dirty(), "จุดเริ่มต้องสะอาด ไม่งั้นเทสต์ไม่ได้พิสูจน์อะไร");

        let before = board.clone();
        let revision_before = board.revision();

        // ★ ของจริงถือ `ArrangeView` ตัวเดิมข้ามการสลับโหมด (มันอยู่ใน `Gfx`)
        //   สร้างใหม่ทุกครอบจะพลาด cache ที่เป็นตัวเสี่ยงจริง
        let mut view = ArrangeView::new();

        for round in 0..100 {
            if round % 2 == 0 {
                // ---- เข้าโหมด Arrange: กรอง + เรียง + จัดแผ่น + หาชุดที่ต้องวาด ----
                //     วนวิธีเรียงไปด้วย เพื่อให้ผ่านทุกเส้นทางของ `query::select`
                //     รวมทั้ง `CanvasOrder` ที่อ่านเรขาคณิตฝั่ง Canvas มาใช้ (P3-6)
                let (key, _) = SORT_CHOICES[round / 2 % SORT_CHOICES.len()];
                view.set_sort(key, round % 4 == 0);
                view.plan(&board, VIEW, 1.0, aspect_of(&board));
                let _ = view.visible();
            } else {
                // ---- กลับโหมด Canvas: สิ่งที่ `rebuild_quads` อ่านทุกเฟรม ----
                let _: Vec<ItemId> = board.items_in_z_order().map(|(id, _)| id).collect();
            }
        }

        // ★ สามข้อ เรียงจาก **แข็งไปอ่อน** — ตัวแรกคือตัวที่จับได้กว้างที่สุด
        //
        // ★★★ ทำไม `revision` ถึงต้องมา *ก่อน* และทำไม `dirty` กับ `==` ไม่พอ:
        //     `apply` แล้ว `undo` ในเฟรมเดียว จะคืน `dirty` เป็น false และคืน
        //     ทุกค่าจน `==` ผ่าน — **แต่ `revision` ขยับสองครั้ง**
        //
        //     ยืนยันด้วย negative control แล้ว (ฉีดคู่ apply+undo เข้าไปในลูป):
        //     `revision` แดง · `dirty` กับ `==` **เขียวทั้งคู่** · และเมื่อปิด
        //     `revision` ทิ้ง ตัวที่จับได้เป็นตัวถัดไปคือ `redo_depth` ข้างล่าง
        //     ซึ่งจับได้เพราะ *การฉีดนั้นใช้ `undo`* เท่านั้น
        //
        //     ★ ที่ยังพิสูจน์ไม่ได้จากในเครทนี้: การเขียนที่ **ไม่ผ่าน `History`
        //       เลย** ซึ่งจะรอดทั้ง `dirty` และ `redo_depth` เหลือ `revision`
        //       เป็นตาข่ายเดียว · เขียนเทสต์ให้ทำแบบนั้นไม่ได้เพราะตัวแก้ของ
        //       `Board` เป็น `pub(crate)` ของ `refx-core` — ซึ่งก็คือกำแพงที่
        //       ทำให้สภาพนั้นเกิดยากอยู่แล้ว จึงบันทึกไว้ว่ารู้ ไม่ใช่อ้างว่าตรวจแล้ว
        assert_eq!(
            board.revision(),
            revision_before,
            "ไม่มีตัวแก้ของ Board ตัวไหนควรถูกเรียกเลยระหว่างสลับโหมด — \
             revision ที่ขยับแปลว่ามีคนเขียน แม้จะเขียนแล้วย้อนกลับจนธง dirty \
             และ `==` มองไม่เห็นก็ตาม"
        );
        assert!(!board.is_dirty(), "สลับโหมดแล้ว dirty กลายเป็น true");
        assert_eq!(board, before, "สลับโหมดแล้ว board ไม่เหมือนเดิม");

        // ★★ และข้อมูลของ **ทั้งสองฝั่ง** ต้องยังอยู่คู่กัน (docs/02 §2.2)
        //    — "เท่าเดิม" ต้องไม่ได้มาจากการที่ทั้งสองฝั่งว่างเปล่าเหมือนกัน
        for id in &ids {
            let item = board.item(*id).expect("item หายไประหว่างสลับโหมด");
            let was = before.item(*id).unwrap();
            assert_eq!(item.canvas, was.canvas, "ฝั่ง Canvas ของ {id:?} เปลี่ยน");
            assert_eq!(item.meta, was.meta, "ฝั่ง Arrange ของ {id:?} เปลี่ยน");
        }
        assert_eq!(board.groups().len(), 1, "กลุ่มหายหรือถูกสร้างเพิ่มระหว่างสลับโหมด");
        assert!(
            board.group(group_id).is_some_and(|g| g.collapsed),
            "สถานะยุบของกลุ่มต้องอยู่เหมือนเดิม"
        );
        // ★ และ history ต้องไม่โตขึ้นเลย — คำสั่งที่เกิดใหม่คือหลักฐานตรง ๆ
        //   ว่ามีการแก้เอกสารระหว่างสลับโหมด
        assert_eq!(history.redo_depth(), 0);
    }
}
