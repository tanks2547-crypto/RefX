//! ★★★ ภาพที่เพิ่งลากเข้ามา **ไปลงที่ไหน และใหญ่เท่าไหร่** (ROADMAP P5-9b · ของใหม่ลงตรงที่ชี้)
//!
//! # ทำไมต้องมีโมดูลนี้
//!
//! ตั้งแต่ P1 ภาพทุกใบถูกย่อให้ด้านยาว **128 หน่วย** แล้ววางเป็นตาราง 16 ช่อง
//! ช่องละ 160 ที่พิกัดตายตัว (2000, 2000) — คอมเมนต์เขียนว่า *"ไปก่อน — layout
//! จริงมาใน P2/P3"* แล้วไม่มีใครกลับมา · เจ้าของโปรเจกต์ลอง rc.1 (27 ก.ย. 2026)
//! แล้วเห็นภาพ ~30 px ที่ซูม 25% · snapshot ของการลากจริงของเขายืนยัน:
//! 2560×3712 → **88 × 128** · 3034×4156 → **93 × 128** · 4000×6641 → **77 × 128**
//!
//! # กติกา
//!
//! | | |
//! |---|---|
//! | ขนาด | **1 หน่วย world = 1 พิกเซลของภาพ ตอนวาง** — 2400 px ที่ซูม 25% = 600 px บนจอ |
//! | ที่วาง | **ตรงที่ผู้ใช้ชี้** ([`Anchor`]) — ใบแรกกึ่งกลางอยู่ที่จุดนั้น · ใบต่อไปไหลไปทางขวา ขึ้นแถวใหม่ที่ขอบขวาของบริเวณที่เห็นตอนวาง |
//! | กล้อง | **ไม่แตะ** — "การขยับมุมมองที่ผู้ใช้ไม่ได้ขอ คือการแย่งงานเขา" (`docs/02 §2.9`) |
//!
//! ★★★ **ตรงที่ชี้** (ROADMAP — ตัดสิน 1 ต.ค. 2026): ลากวาง = จุดที่ปล่อยเมาส์ ·
//! วางจาก clipboard = เคอร์เซอร์ถ้าอยู่เหนือผืนผ้าใบ ไม่งั้นกลางจอ · รุ่นก่อน (P5-9b)
//! ไหลจากมุมบนซ้ายของจอเสมอ แล้ว **ทับของเดิม** ที่อยู่ตรงนั้นโดยผู้ใช้ไม่ได้เลือก
//! · ตอนนี้ถ้ามันทับ นั่นคือที่ผู้ใช้ชี้เอง — **ไม่ขยับของให้** เพราะนักวาดตั้งใจวางซ้อน
//! บ่อย และเครื่องมือที่หาที่ว่างให้เองเดาไม่ได้ว่าของจะไปไหน
//!
//! ★ `docs/02 §2.1` บอกว่า `ItemCanvas.size` **ไม่ใช่** ขนาดพิกเซลต้นฉบับ — ถูก:
//!   มันเป็นอิสระจากกันหลังวาง (ผู้ใช้ย่อ/ขยายได้) · สเปกไม่ได้บอกว่า **เริ่มที่เท่าไหร่**
//!   → ตัดสินที่นี่ว่าเริ่มที่ 1:1 ตามที่เจ้าของโปรเจกต์คาด และตามที่โปรแกรมกลุ่มเดียวกันทำ
//!
//! # ★★ การไหลหนึ่งสาย = การชี้หนึ่งครั้ง
//!
//! ไฟล์ที่ปล่อยพร้อมกันได้ [`Anchor`] เดียวกัน · ภาพ decode เสร็จทีละใบ **ไม่เรียงกัน
//! และปนกับชุดอื่นได้** ถ้าผู้ใช้ลากชุดที่สองไปอีกที่ระหว่างที่ชุดแรกยังโหลด → ผู้เรียก
//! ถือ [`Flow`] แยกตาม [`Anchor`] แล้วหยิบสายที่ตรงกับงานใบนั้น ไม่ใช่ "สายล่าสุด"
//!
//! ★ กล้องขยับหลังวาง **ไม่เปลี่ยนที่วาง** — จุดที่ชี้เป็นพิกัด world แล้ว · กล้องมีผลกับ
//!   เรื่องเดียวคือ fit บนกระดานว่าง ([`Flow::fits`] · [`Flow::belongs_to`])
//!
//! ★ โมดูลนี้ไม่แตะ GPU ไม่แตะ `Board` — คืนแค่ตำแหน่ง/ขนาด ให้ผู้เรียกห่อเป็น
//!   `AddItems` เอง (I-3 · ทุกการเพิ่มผ่าน `Command`)

use refx_core::geom::Rect;
use refx_core::glam::Vec2;
use refx_core::layout::MAX_SIDE;

/// สัดส่วนของขอบรอบบริเวณที่เห็น — ภาพไม่ชิดขอบจอตอนขึ้นแถวใหม่
const MARGIN: f32 = 0.05;
/// ช่องว่างระหว่างภาพ เทียบกับความกว้างของบริเวณที่เห็น
const GAP: f32 = 0.02;
/// กรอบของภาพที่เปิดไม่ได้ (`Missing`) กว้างเท่านี้ของบริเวณที่เห็น
///
/// ★ ไม่รู้ขนาดจริงเพราะอ่านหัวไฟล์ไม่ผ่าน · ขนาดตายตัวในหน่วย world จะเล็กจนอ่านป้าย
///   ไม่ออกตอนซูมออก และใหญ่จนบังทั้งจอตอนซูมเข้า → ผูกกับสิ่งที่ผู้ใช้เห็นอยู่
const MISSING_WIDTH: f32 = 0.2;
/// ★ แท็บหนึ่งถือการไหลค้างได้กี่สาย (I-6) — เกินแล้วทิ้งสายเก่าสุด
///
/// สายที่ถูกทิ้งแปลว่าชุดนั้นถูกลากมานานแล้วและยังมีใบค้าง decode อยู่ · ใบที่เหลือ
/// เริ่มสายใหม่ที่จุดเดิม (ทับใบแรก ๆ ของชุดตัวเองได้) — ยอมรับได้เพราะต้องลากเกิน
/// แปดจุดระหว่างที่ชุดแรกยังไม่เสร็จ
pub const MAX_FLOWS: usize = 8;

/// กล้อง ณ จังหวะหนึ่ง — ใช้ตอบว่า "ผู้ใช้แตะกล้องไปแล้วหรือยัง"
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewKey {
    center: Vec2,
    zoom: f32,
}

impl ViewKey {
    /// กล้องหนึ่งจังหวะ
    #[must_use]
    pub fn new(center: Vec2, zoom: f32) -> Self {
        Self { center, zoom }
    }
}

/// ★★★ **ตรงที่ผู้ใช้ชี้** — จุดใน world และบริเวณที่เขาเห็นอยู่ตอนนั้น
///
/// บริเวณที่เห็นถูกจดไว้ด้วยเพราะขนาดช่องว่าง/ขอบขึ้นแถวใหม่/กรอบ `Missing`
/// ต้องผูกกับสิ่งที่ผู้ใช้เห็น **ตอนวาง** ไม่ใช่ตอนที่ภาพใบนั้นบังเอิญ decode เสร็จ
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    point: Vec2,
    view: Rect,
}

impl Anchor {
    /// ชี้ที่ `point` ขณะเห็น `view`
    #[must_use]
    pub fn new(point: Vec2, view: Rect) -> Self {
        Self { point, view }
    }

    /// ไม่มีจุดชี้ (เคอร์เซอร์ไม่อยู่เหนือผืนผ้าใบ · เปิดจากบรรทัดคำสั่ง) → **กลางจอ**
    #[must_use]
    pub fn center_of(view: Rect) -> Self {
        Self::new(view.center(), view)
    }

    /// จุดที่ชี้
    #[must_use]
    pub fn point(&self) -> Vec2 {
        self.point
    }
}

/// การไหลของภาพใหม่จากจุดที่ชี้หนึ่งจุด
#[derive(Debug, Clone, PartialEq)]
pub struct Flow {
    anchor: Anchor,
    /// กล้อง ณ ตอนที่การไหลเริ่ม (หรือหลัง fit ของเราเอง)
    key: ViewKey,
    /// มุมบนซ้ายของช่องถัดไป — `None` = ยังไม่มีภาพใบแรก
    next: Option<Vec2>,
    /// ขอบซ้ายของทุกแถว = ขอบซ้ายของภาพใบแรก
    row_left: f32,
    /// ขอบล่างของแถวปัจจุบัน — แถวถัดไปเริ่มใต้ตรงนี้
    row_bottom: f32,
    /// ★ การไหลนี้เริ่มบน **กระดานว่าง** — กล้อง fit ตามได้ (P5-9b ส่วนที่ 2)
    fits: bool,
}

impl Flow {
    /// เริ่มไหลจากจุดที่ชี้
    #[must_use]
    pub fn new(key: ViewKey, anchor: Anchor) -> Self {
        Self {
            anchor,
            key,
            next: None,
            row_left: anchor.point.x,
            row_bottom: anchor.point.y,
            fits: false,
        }
    }

    /// ★★★ การไหลที่เริ่มบนกระดานว่าง — **กล้อง fit ตามภาพที่เข้ามาได้**
    ///
    /// | สภาพก่อนวาง | กล้อง |
    /// |---|---|
    /// | กระดานว่าง | fit ให้พอดี — ไม่มีมุมมองของผู้ใช้ให้รักษา |
    /// | มีของอยู่แล้ว | **ห้ามขยับ** — การขยับมุมมองที่ผู้ใช้ไม่ได้ขอ คือการแย่งงานเขา |
    #[must_use]
    pub fn on_empty_board(mut self) -> Self {
        self.fits = true;
        self
    }

    /// กล้องควร fit ตามภาพใบที่เพิ่งวางไหม
    #[must_use]
    pub fn fits(&self) -> bool {
        self.fits
    }

    /// ★★ ผู้ใช้แตะกล้องแล้ว — **เลิก fit ถาวร** สำหรับสายนี้
    ///
    /// ที่วางไม่เปลี่ยน (ผูกกับจุดที่ชี้ใน world แล้ว) แต่กล้องกลับเป็นของผู้ใช้
    pub fn stop_fitting(&mut self) {
        self.fits = false;
    }

    /// เป็นสายของการชี้ครั้งนี้ไหม
    #[must_use]
    pub fn from(&self, anchor: Anchor) -> bool {
        self.anchor == anchor
    }

    /// จุดที่สายนี้เริ่ม
    #[must_use]
    pub fn anchor(&self) -> Anchor {
        self.anchor
    }

    /// กล้องยังเป็นตัวเดียวกับที่สายนี้รู้จักไหม (ผู้ใช้ยังไม่ได้แตะ)
    #[must_use]
    pub fn belongs_to(&self, key: ViewKey) -> bool {
        self.key == key
    }

    /// ★★ กล้องขยับเพราะ **เรา fit เอง** ไม่ใช่ผู้ใช้ — ยังเป็นกล้องของสายนี้
    pub fn rekey(&mut self, key: ViewKey) {
        self.key = key;
    }

    /// ★ ที่วางของภาพขนาด `w × h` พิกเซล — คืน **(จุดกึ่งกลาง, ขนาด)** ในหน่วย world
    ///
    /// จุดกึ่งกลางเพราะ `ItemCanvas::pos` คือกึ่งกลาง (`docs/02 §2.1`)
    pub fn place_image(&mut self, w: u32, h: u32) -> (Vec2, Vec2) {
        // 1:1 · กันศูนย์ (ไฟล์ที่ประกาศขนาด 0) และกันใหญ่เกินเพดานของ layout
        #[expect(
            clippy::cast_precision_loss,
            reason = "ขนาดภาพถูกจำกัดด้วยเพดาน decode ซึ่งต่ำกว่า 2^24 มาก — f32 แทนได้ตรง"
        )]
        let size = Vec2::new(w.max(1) as f32, h.max(1) as f32).min(Vec2::splat(MAX_SIDE));
        self.take(size)
    }

    /// ที่วางของภาพที่เปิดไม่ได้ — ไม่รู้ขนาดจริง ใช้กรอบ 4:3 ตามสัดส่วนของบริเวณที่เห็น
    pub fn place_missing(&mut self) -> (Vec2, Vec2) {
        let width = (self.anchor.view.size().x * MISSING_WIDTH).max(1.0);
        self.take(Vec2::new(width, width * 0.75))
    }

    fn take(&mut self, size: Vec2) -> (Vec2, Vec2) {
        let view = self.anchor.view.size();
        let gap = view.x * GAP;
        let Some(next) = self.next else {
            // ★★★ ใบแรก: **กึ่งกลางอยู่ตรงที่ชี้พอดี** — สิ่งที่ผู้ใช้คาดจากการปล่อยเมาส์
            let top_left = self.anchor.point - size * 0.5;
            self.row_left = top_left.x;
            self.row_bottom = top_left.y + size.y;
            self.next = Some(Vec2::new(top_left.x + size.x + gap, top_left.y));
            return (self.anchor.point, size);
        };
        // ขึ้นแถวใหม่ถ้าล้นขอบขวาของบริเวณที่เห็นตอนวาง · แถวใหม่เริ่มที่ขอบซ้ายของใบแรก
        let right = self.anchor.view.max.x - view.x.min(view.y) * MARGIN;
        let top_left = if next.x + size.x > right {
            Vec2::new(self.row_left, self.row_bottom + gap)
        } else {
            next
        };
        self.next = Some(Vec2::new(top_left.x + size.x + gap, top_left.y));
        self.row_bottom = self.row_bottom.max(top_left.y + size.y);
        (top_left + size * 0.5, size)
    }
}

/// ★ หาสายของการชี้ครั้งนี้ — ไม่มีก็เริ่มใหม่ (เก็บไม่เกิน [`MAX_FLOWS`] สาย · I-6)
///
/// `start` ถูกเรียกเฉพาะตอนต้องเริ่มสายใหม่ — ผู้เรียกตัดสินที่นั่นว่ากระดานว่างไหม
pub fn flow_for(flows: &mut Vec<Flow>, anchor: Anchor, start: impl FnOnce() -> Flow) -> &mut Flow {
    let index = match flows.iter().position(|flow| flow.from(anchor)) {
        Some(index) => index,
        None => {
            if flows.len() >= MAX_FLOWS {
                flows.remove(0);
            }
            flows.push(start());
            flows.len() - 1
        }
    };
    &mut flows[index]
}

/// ★ ใบที่ถือรอใบก่อนหน้าได้มากสุดเท่านี้ต่อหนึ่งชุด (I-6) — ดู [`Arrivals`]
///
/// thumbnail ใบละ 64 KB → 1,024 ใบ = 64 MB · ตัวเลขเดียวกับงบ thumbnail ของ 1,000 ภาพ
/// (`ItemRender::thumb`) ซึ่งใบพวกนี้จะไปอยู่ตรงนั้นอยู่แล้วทันทีที่ถูกวาง
pub const MAX_HELD: usize = 1024;

/// ★★★ **ช่องในผังมาจากลำดับของไฟล์ในชุด ไม่ใช่ลำดับที่ decode เสร็จ** (ตัดสิน 8 ต.ค. 2026)
///
/// # ทำไม
///
/// [`Flow`] ให้ช่องตามลำดับที่ถูกเรียก และช่องถัดไปขึ้นกับขนาดของ **ทุกใบก่อนหน้า** ·
/// worker หลายตัว decode พร้อมกันแล้วเสร็จไม่เรียงกัน → เดิมลากชุดเดิมสองครั้งได้ผัง
/// ต่างกัน (เห็นบนภาพจอของรอบ z-order: ภาพเล็กอยู่คนละช่องในสองรอบ) · ชนชั้นเดียวกับ
/// z-order ที่ขึ้นกับ texture (`docs/04`) — **ผลที่ทำซ้ำไม่ได้แย่กว่าผลที่ผิดสม่ำเสมอ**
///
/// # กติกา
///
/// ผลที่กลับมาก่อนใบก่อนหน้า **ถูกถือไว้** แล้วปล่อยเมื่อทุกใบก่อนหน้ามีคำตอบแล้ว
/// (สำเร็จ · เปิดไม่ได้ · ถูกยกเลิก — ทุกงานได้คำตอบหนึ่งครั้งเสมอ ซึ่งเป็นสิ่งเดียวกับที่
/// `DropBatch::settled` พึ่งพาอยู่แล้ว) · ราคาคือภาพใบหลังขึ้นจอช้าลงได้เท่ากับเวลาที่
/// ใบก่อนหน้ายัง decode อยู่ — pool หยิบงานตามลำดับไฟล์ (`priority = i`) จึงมักแค่ไม่กี่ ms
///
/// ★ เกิน [`MAX_HELD`] ใบที่ถือรอ = ใบที่ยังไม่มาถูก **ข้ามไปก่อน** แล้วลงท้ายเมื่อมันมา
///   (I-6 ชนะลำดับ — ต้องมีใบหนึ่งช้ากว่าอีกพันใบ) · ข้ามแล้วไม่ทิ้ง: ไม่มีใบไหนหาย
///
/// ★ ไม่รู้จักคีย์ (งานที่ไม่สร้างใบใหม่ เช่น ภาพของเอกสารที่เปิดจากไฟล์) → คืนกลับให้
///   ผู้เรียกทำทันที · คีย์เดียวกันอยู่ในสองชุดได้ (ลากไฟล์เดิมซ้ำระหว่างที่ชุดแรกยังโหลด)
///   → ชุดที่เก่ากว่าได้ก่อน · เนื้อไฟล์เดียวกันจึงไม่สำคัญว่าผลไหนตกชุดไหน
#[derive(Debug)]
pub struct Arrivals<K, T> {
    batches: Vec<Batch<K, T>>,
    ready: Vec<(Anchor, T)>,
}

#[derive(Debug)]
struct Batch<K, T> {
    anchor: Anchor,
    /// หนึ่งช่องต่อหนึ่งไฟล์ ตามลำดับในชุด
    slots: Vec<(K, Slot<T>)>,
    /// ช่องแรกที่ยังไม่ถูกปล่อย
    next: usize,
    /// จำนวนช่อง `Held` — เทียบกับ [`MAX_HELD`]
    held: usize,
}

#[derive(Debug)]
enum Slot<T> {
    Waiting,
    Held(T),
    Released,
    /// ถูกข้ามเพราะถือรอเกินเพดาน — มาเมื่อไหร่ปล่อยทันที
    Late,
}

impl<K, T> Default for Arrivals<K, T> {
    fn default() -> Self {
        Self {
            batches: Vec::new(),
            ready: Vec::new(),
        }
    }
}

impl<K: PartialEq, T> Arrivals<K, T> {
    /// ชุดใหม่จากการชี้หนึ่งครั้ง — `keys` ตามลำดับไฟล์ในชุด
    pub fn open(&mut self, anchor: Anchor, keys: impl IntoIterator<Item = K>) {
        let slots: Vec<(K, Slot<T>)> = keys.into_iter().map(|k| (k, Slot::Waiting)).collect();
        if !slots.is_empty() {
            self.batches.push(Batch {
                anchor,
                slots,
                next: 0,
                held: 0,
            });
        }
    }

    /// ผลของงาน `key` กลับมาแล้ว · `Err` = ไม่ใช่งานของชุดไหน → ผู้เรียกทำเองทันที
    ///
    /// # Errors
    /// คืน `value` กลับเมื่อไม่มีชุดไหนรอคีย์นี้อยู่
    pub fn arrive(&mut self, key: &K, value: T) -> Result<(), T> {
        let found = self.batches.iter().enumerate().find_map(|(b, batch)| {
            batch
                .slots
                .iter()
                .position(|(k, slot)| k == key && matches!(slot, Slot::Waiting | Slot::Late))
                .map(|s| (b, s))
        });
        let Some((b, s)) = found else {
            return Err(value);
        };
        let batch = &mut self.batches[b];
        if matches!(batch.slots[s].1, Slot::Late) {
            batch.slots[s].1 = Slot::Released;
            self.ready.push((batch.anchor, value));
        } else {
            batch.slots[s].1 = Slot::Held(value);
            batch.held += 1;
        }
        self.advance(b);
        Ok(())
    }

    /// ปล่อยทุกช่องที่ใบก่อนหน้าครบแล้ว · ถือเกินเพดานเมื่อไหร่ข้ามช่องที่ยังรอ
    fn advance(&mut self, b: usize) {
        let batch = &mut self.batches[b];
        while batch.next < batch.slots.len() {
            let slot = &mut batch.slots[batch.next].1;
            match std::mem::replace(slot, Slot::Released) {
                Slot::Held(value) => {
                    batch.held -= 1;
                    self.ready.push((batch.anchor, value));
                }
                Slot::Waiting if batch.held > MAX_HELD => *slot = Slot::Late,
                Slot::Waiting => {
                    *slot = Slot::Waiting;
                    break;
                }
                done @ (Slot::Released | Slot::Late) => *slot = done,
            }
            batch.next += 1;
        }
        // ★ ชุดที่ทุกช่องได้คำตอบแล้วต้องหายไป ไม่งั้นรายการโตตลอดอายุโปรแกรม (I-6)
        if batch.next == batch.slots.len()
            && batch
                .slots
                .iter()
                .all(|(_, slot)| matches!(slot, Slot::Released))
        {
            self.batches.remove(b);
        }
    }

    /// ผลที่ถึงคิวแล้ว **ตามลำดับไฟล์** — พร้อมจุดชี้ของชุดมัน
    pub fn take_ready(&mut self) -> Vec<(Anchor, T)> {
        std::mem::take(&mut self.ready)
    }

    /// ชุดที่ยังมีช่องรออยู่ — เทสต์และแถบสถานะใช้
    #[must_use]
    pub fn open_batches(&self) -> usize {
        self.batches.len()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::float_cmp)]

    use super::*;

    /// หน้าต่าง 1280×800 เหมือนภาพหน้าจอที่ใช้วัด
    fn view(center: Vec2, zoom: f32) -> (ViewKey, Rect) {
        (
            ViewKey::new(center, zoom),
            Rect::from_center_size(center, Vec2::new(1280.0, 800.0) / zoom),
        )
    }

    fn overlap(a: Rect, b: Rect) -> bool {
        a.min.x < b.max.x && b.min.x < a.max.x && a.min.y < b.max.y && b.min.y < a.max.y
    }

    /// ★★★ **ภาพของจริงของเจ้าของโปรเจกต์** — ต้องได้ขนาดพิกเซลของมัน ไม่ใช่ 128
    ///
    /// ตัวเลขจาก snapshot การลากจริง 27 ก.ย. 2026 (กล้องปริยาย ซูม 0.25)
    #[test]
    fn the_images_from_the_real_drag_land_at_their_own_pixel_size() {
        let (key, rect) = view(Vec2::splat(2000.0), 0.25);
        let mut flow = Flow::new(key, Anchor::center_of(rect));
        for (w, h) in [(2560, 3712), (3034, 4156), (4000, 6641)] {
            let (_, size) = flow.place_image(w, h);
            #[expect(clippy::cast_precision_loss, reason = "ขนาดทดสอบเล็ก")]
            let want = Vec2::new(w as f32, h as f32);
            assert_eq!(size, want, "{w}×{h} ถูกย่อ/ขยาย — ควรเป็น 1:1");
            // ที่ซูม 25% ด้านยาวบนจอต้องเป็นหลักร้อยพิกเซล ไม่ใช่ ~30
            assert!(size.y * 0.25 > 500.0, "{w}×{h} บนจอยังเล็กเกินไป");
        }
    }

    /// ★★★ **ใบแรกกึ่งกลางอยู่ตรงที่ชี้** — ที่ไหนก็ได้บนโลก ทุกระดับซูม
    #[test]
    fn the_first_image_is_centred_where_the_user_pointed() {
        for (center, zoom, point) in [
            (Vec2::splat(2000.0), 0.25, Vec2::new(1500.0, 2600.0)),
            (
                Vec2::new(-50_000.0, 7_000.0),
                1.0,
                Vec2::new(-50_400.0, 6_900.0),
            ),
            (Vec2::ZERO, 4.0, Vec2::new(10.0, -20.0)),
        ] {
            let (key, rect) = view(center, zoom);
            let mut flow = Flow::new(key, Anchor::new(point, rect));
            let (pos, _) = flow.place_image(100, 80);
            assert_eq!(pos, point, "ภาพใบแรกไม่ได้อยู่ตรงที่ชี้");
            let mut missing = Flow::new(key, Anchor::new(point, rect));
            assert_eq!(missing.place_missing().0, point, "ใบที่เปิดไม่ได้ไม่ได้อยู่ตรงที่ชี้");
        }
    }

    /// ★ ไม่มีจุดชี้ = กลางจอ (วางจาก clipboard ตอนเคอร์เซอร์อยู่นอกผืนผ้าใบ)
    #[test]
    fn with_nothing_pointed_at_it_lands_in_the_middle_of_the_view() {
        let (key, rect) = view(Vec2::new(300.0, -40.0), 0.5);
        let mut flow = Flow::new(key, Anchor::center_of(rect));
        assert_eq!(flow.place_image(640, 480).0, rect.center());
    }

    /// ★★ ภาพในชุดเดียวกัน **ไม่ทับกันเอง** — ทั้งในแถวเดียวกันและตอนขึ้นแถวใหม่
    ///
    /// (ทับ **ของเดิม** บนกระดานได้ — นั่นคือที่ผู้ใช้ชี้เอง · ข้อนี้คุมแค่ชุดเดียวกัน)
    #[test]
    fn images_in_one_flow_never_overlap_each_other() {
        for point in [
            Vec2::ZERO,
            Vec2::new(2000.0, 0.0),
            Vec2::new(-2400.0, 1500.0),
        ] {
            let (key, rect) = view(Vec2::ZERO, 0.25);
            let mut flow = Flow::new(key, Anchor::new(point, rect));
            let placed: Vec<Rect> = [
                (2400, 1600),
                (1200, 3000),
                (800, 800),
                (5000, 400),
                (300, 300),
                (4000, 6641),
            ]
            .iter()
            .map(|&(w, h)| {
                let (pos, size) = flow.place_image(w, h);
                Rect::from_center_size(pos, size)
            })
            .collect();
            for (i, a) in placed.iter().enumerate() {
                for b in &placed[i + 1..] {
                    assert!(!overlap(*a, *b), "ชี้ที่ {point}: ภาพทับกัน {a:?} กับ {b:?}");
                }
            }
        }
    }

    /// ★★ ใบต่อไปไหลไปทางขวาของใบแรก — ไม่กระโดดกลับไปที่มุมจอ
    #[test]
    fn the_rest_of_the_batch_flows_on_from_the_pointed_spot() {
        let (key, rect) = view(Vec2::ZERO, 1.0);
        let mut flow = Flow::new(key, Anchor::new(Vec2::new(-300.0, -200.0), rect));
        let (a, sa) = flow.place_image(100, 100);
        let (b, _) = flow.place_image(100, 100);
        assert!(b.x > a.x + sa.x * 0.5, "ใบที่สองไม่ได้อยู่ทางขวาของใบแรก");
        assert_eq!(b.y, a.y, "ใบที่สองไม่ได้อยู่แถวเดียวกัน");
    }

    /// ★★★ **สองการชี้ = สองสาย** แม้ผลจะกลับมาสลับกัน
    ///
    /// ลากชุด A ไปซ้าย แล้วลากชุด B ไปขวาระหว่างที่ A ยังโหลด · ผลกลับมา A B A B
    /// → ทุกใบของ A ต้องอยู่ฝั่งซ้าย ทุกใบของ B ฝั่งขวา · ถ้าผู้เรียกใช้ "สายล่าสุด"
    /// ใบที่สองของ A จะไปต่อท้าย B
    #[test]
    fn two_drops_at_two_places_keep_their_own_flows_while_results_interleave() {
        let (key, rect) = view(Vec2::ZERO, 1.0);
        let left = Anchor::new(Vec2::new(-400.0, 0.0), rect);
        let right = Anchor::new(Vec2::new(400.0, 0.0), rect);
        let mut flows = Vec::new();
        let mut placed = Vec::new();
        for anchor in [left, right, left, right, left] {
            let flow = flow_for(&mut flows, anchor, || Flow::new(key, anchor));
            placed.push((anchor, flow.place_image(50, 50).0));
        }
        assert_eq!(flows.len(), 2);
        for (anchor, pos) in placed {
            if anchor == left {
                assert!(pos.x < 0.0, "ใบของชุดซ้ายไปตกฝั่งขวา: {pos}");
            } else {
                assert!(pos.x > 0.0, "ใบของชุดขวาไปตกฝั่งซ้าย: {pos}");
            }
        }
        // NC — ใช้สายเดียวสำหรับทุกใบ แล้วใบของชุดซ้ายต้องไหลไปต่อท้ายจนข้ามฝั่ง
        let mut one = Flow::new(key, left);
        let xs: Vec<f32> = (0..5).map(|_| one.place_image(50, 50).0.x).collect();
        assert!(xs.iter().any(|x| *x > -300.0), "NC ไม่แสดงความต่าง: {xs:?}");
    }

    /// ★ เก็บสายไม่เกินเพดาน (I-6) — สายเก่าสุดถูกทิ้ง
    #[test]
    fn a_tab_never_holds_more_than_the_cap_of_flows() {
        let (key, rect) = view(Vec2::ZERO, 1.0);
        let mut flows = Vec::new();
        for i in 0..MAX_FLOWS * 3 {
            #[expect(clippy::cast_precision_loss, reason = "ตัวเลขเล็ก")]
            let anchor = Anchor::new(Vec2::new(i as f32 * 10.0, 0.0), rect);
            let _ = flow_for(&mut flows, anchor, || Flow::new(key, anchor));
        }
        assert_eq!(flows.len(), MAX_FLOWS);
    }

    /// ★ `Missing` ผูกกับบริเวณที่เห็น — อ่านป้ายออกได้ทุกระดับซูม
    #[test]
    fn a_missing_image_frame_scales_with_the_view() {
        for zoom in [0.05, 1.0, 16.0] {
            let (key, rect) = view(Vec2::ZERO, zoom);
            let mut flow = Flow::new(key, Anchor::center_of(rect));
            let (_, size) = flow.place_missing();
            let on_screen = size.x * zoom;
            assert!(
                (on_screen - 1280.0 * MISSING_WIDTH).abs() < 0.5,
                "ซูม {zoom}: กรอบ Missing กว้าง {on_screen} px บนจอ"
            );
        }
    }

    /// ★★ fit ของเราเองไม่ทำให้สายเลิก fit · ผู้ใช้แตะกล้องแล้วเลิกถาวร
    #[test]
    fn our_own_fit_keeps_fitting_and_the_users_touch_stops_it() {
        let (key, rect) = view(Vec2::ZERO, 0.25);
        let mut flow = Flow::new(key, Anchor::center_of(rect)).on_empty_board();
        assert!(flow.fits());
        let (a, _) = flow.place_image(2400, 1600);
        let fitted = ViewKey::new(a, 0.4);
        flow.rekey(fitted);
        assert!(flow.belongs_to(fitted));
        assert!(!flow.belongs_to(ViewKey::new(a + Vec2::X, 0.4)));
        flow.stop_fitting();
        assert!(!flow.fits());
        // ค่าปริยาย: ไม่ fit (มีของอยู่แล้ว = ห้ามขยับกล้อง)
        assert!(!Flow::new(key, Anchor::center_of(rect)).fits());
    }

    /// ไฟล์ทดสอบห้าใบ ขนาดต่างกันมากพอให้ช่องของใบหลังขยับตามใบก่อน
    const SIZES: [(u32, u32); 5] = [
        (2400, 1600),
        (300, 300),
        (1200, 3000),
        (5000, 400),
        (800, 800),
    ];

    /// ทุกลำดับที่ห้าใบเสร็จได้ (120 แบบ)
    fn every_order(n: usize) -> Vec<Vec<usize>> {
        if n == 0 {
            return vec![Vec::new()];
        }
        let mut out = Vec::new();
        for rest in every_order(n - 1) {
            for at in 0..=rest.len() {
                let mut order = rest.clone();
                order.insert(at, n - 1);
                out.push(order);
            }
        }
        out
    }

    /// จำลองการลากหนึ่งชุด: ผลกลับมาตาม `order` · `frame` ใบต่อเฟรม · คืนกรอบตามดัชนีไฟล์
    fn drop_batch(order: &[usize], frame: usize, sequenced: bool) -> Vec<Rect> {
        let (key, rect) = view(Vec2::ZERO, 0.25);
        let anchor = Anchor::new(Vec2::new(-1000.0, -500.0), rect);
        let mut arrivals: Arrivals<usize, usize> = Arrivals::default();
        arrivals.open(anchor, 0..SIZES.len());
        let mut flows = Vec::new();
        let mut placed = vec![Rect::from_center_size(Vec2::ZERO, Vec2::ZERO); SIZES.len()];
        for chunk in order.chunks(frame) {
            let released: Vec<(Anchor, usize)> = if sequenced {
                for &file in chunk {
                    arrivals.arrive(&file, file).unwrap();
                }
                arrivals.take_ready()
            } else {
                chunk.iter().map(|&file| (anchor, file)).collect()
            };
            for (anchor, file) in released {
                let flow = flow_for(&mut flows, anchor, || Flow::new(key, anchor));
                let (w, h) = SIZES[file];
                let (pos, size) = flow.place_image(w, h);
                placed[file] = Rect::from_center_size(pos, size);
            }
        }
        placed
    }

    /// ★★★ **ประตูของเจ้าของโปรเจกต์:** ลากชุดเดิมโดยบังคับให้ decode เสร็จคนละลำดับ
    /// → **ผังต้องเท่ากัน** · ทุกลำดับ 120 แบบ × ผลมาทีละ 1/2/5 ใบต่อเฟรม
    #[test]
    fn the_layout_of_a_batch_does_not_depend_on_which_file_finished_decoding_first() {
        let want = drop_batch(&[0, 1, 2, 3, 4], 1, true);
        for order in every_order(SIZES.len()) {
            for frame in [1, 2, 5] {
                assert_eq!(
                    drop_batch(&order, frame, true),
                    want,
                    "เสร็จตามลำดับ {order:?} ({frame} ใบ/เฟรม) ได้ผังต่างจากลำดับไฟล์"
                );
            }
        }
        // NC — วางตามลำดับที่เสร็จ (ของเดิม) ต้องได้ผังต่างกัน ไม่งั้นประตูนี้ล้มไม่เป็น
        let mut layouts: Vec<Vec<Rect>> = Vec::new();
        for order in every_order(SIZES.len()) {
            let layout = drop_batch(&order, 1, false);
            if !layouts.contains(&layout) {
                layouts.push(layout);
            }
        }
        assert!(
            layouts.len() > 100,
            "NC: ลำดับที่เสร็จให้ผังเดียว ({})",
            layouts.len()
        );
    }

    /// ★★ ใบที่ไม่สร้างภาพ (ถูกยกเลิก) ก็นับเป็นคำตอบ — ใบหลังมันต้องไม่ค้าง
    ///    และชุดที่ทุกใบตอบแล้วต้องหายไป (I-6)
    #[test]
    fn every_answer_moves_the_queue_and_a_finished_batch_is_forgotten() {
        let (_, rect) = view(Vec2::ZERO, 1.0);
        let anchor = Anchor::center_of(rect);
        let mut arrivals: Arrivals<u8, &str> = Arrivals::default();
        arrivals.open(anchor, [1, 2, 3]);
        arrivals.arrive(&3, "three").unwrap();
        arrivals.arrive(&2, "cancelled").unwrap();
        assert!(arrivals.take_ready().is_empty(), "ปล่อยก่อนใบแรกมา");
        arrivals.arrive(&1, "one").unwrap();
        let out: Vec<&str> = arrivals.take_ready().into_iter().map(|(_, v)| v).collect();
        assert_eq!(out, ["one", "cancelled", "three"]);
        assert_eq!(arrivals.open_batches(), 0, "ชุดที่จบแล้วยังค้างอยู่");
        // คีย์ที่ไม่มีชุดไหนรอ → คืนให้ผู้เรียกทำเอง (ภาพของเอกสารที่เปิดจากไฟล์)
        assert_eq!(arrivals.arrive(&9, "relink"), Err("relink"));
    }

    /// ★★ สองชุดสลับกัน + **ไฟล์เดียวกันอยู่ทั้งสองชุด** — ไม่มีชุดไหนรอคำตอบของอีกชุด
    #[test]
    fn two_batches_wait_only_for_their_own_files() {
        let (_, rect) = view(Vec2::ZERO, 1.0);
        let left = Anchor::new(Vec2::new(-400.0, 0.0), rect);
        let right = Anchor::new(Vec2::new(400.0, 0.0), rect);
        let mut arrivals: Arrivals<&str, &str> = Arrivals::default();
        arrivals.open(left, ["a", "same"]);
        arrivals.open(right, ["same", "b"]);
        arrivals.arrive(&"b", "b").unwrap();
        arrivals.arrive(&"same", "same#1").unwrap();
        // ชุดซ้ายเก่ากว่า ได้ `same` ไปก่อน · ยังรอ `a` อยู่ · ชุดขวายังรอ `same` ของมัน
        assert!(arrivals.take_ready().is_empty());
        arrivals.arrive(&"same", "same#2").unwrap();
        assert_eq!(arrivals.take_ready(), [(right, "same#2"), (right, "b")]);
        arrivals.arrive(&"a", "a").unwrap();
        assert_eq!(arrivals.take_ready(), [(left, "a"), (left, "same#1")]);
        assert_eq!(arrivals.open_batches(), 0);
    }

    /// ★ ถือเกินเพดาน → ข้ามใบที่ช้า **แต่ไม่ทิ้ง** · ทุกใบออกมาครั้งเดียวพอดี (I-6 · I-3)
    #[test]
    fn a_very_slow_file_is_skipped_past_the_cap_but_never_lost() {
        let (_, rect) = view(Vec2::ZERO, 1.0);
        let anchor = Anchor::center_of(rect);
        let n = MAX_HELD + 10;
        let mut arrivals: Arrivals<usize, usize> = Arrivals::default();
        arrivals.open(anchor, 0..n);
        let mut out = Vec::new();
        for file in 1..n {
            arrivals.arrive(&file, file).unwrap();
            out.extend(arrivals.take_ready().into_iter().map(|(_, v)| v));
        }
        assert!(!out.is_empty(), "ถือเกินเพดานแล้วยังไม่ปล่อย — RAM โตไม่มีที่สิ้นสุด");
        assert!(
            out.windows(2).all(|w| w[0] < w[1]),
            "ที่ปล่อยออกมาไม่เรียงตามไฟล์"
        );
        arrivals.arrive(&0, 0).unwrap();
        out.extend(arrivals.take_ready().into_iter().map(|(_, v)| v));
        out.sort_unstable();
        assert_eq!(out, (0..n).collect::<Vec<_>>(), "มีใบหายหรือซ้ำ");
        assert_eq!(arrivals.open_batches(), 0);
    }
}
