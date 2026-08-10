//! Layout engines ทั้งห้าตัวของ Arrange mode (P3-2) — **ฟังก์ชันบริสุทธิ์ทั้งไฟล์**
//!
//! รับรายการ `(ItemId, aspect)` + [`LayoutParams`] คืนตำแหน่ง/ขนาด ไม่แตะ `Board`
//! ชั้น editor เป็นคนห่อผลลัพธ์เป็น `ApplyLayoutCommand` (P3-5)
//!
//! ★★★ **ต้อง deterministic เป็นข้อบังคับ ไม่ใช่ของแถม** (docs/03 §3)
//!
//! input เดิมต้องให้ output เดิม **เป๊ะทุกบิต** ทุกครั้ง · ห้ามใช้ลำดับ iteration
//! ของ `HashMap` ในการคำนวณเด็ดขาด (CLAUDE.md) — อาการของการละเมิดคือ
//! **"ภาพเรียงไม่เหมือนเดิมทุกครั้งที่กด"** ซึ่งผู้ใช้เห็นเป็นโปรแกรมที่เชื่อไม่ได้
//! และหาสาเหตุยากมากเพราะมันถูก *เกือบ* ทุกครั้ง
//!
//! ★★ **ทุกตัวเลขที่ออกจากที่นี่ต้อง finite** — ค่า `NaN`/`inf` ที่หลุดลง
//! `ItemCanvas` จะทำให้ hit-test, culling และ spatial index เพี้ยนทั้งระบบ
//! โดยไม่มี error ที่ไหน · aspect ที่เป็น 0 หรือติดลบมาจากไฟล์ได้จริง (I-4)
//!
//! spec: docs/03-modes-and-ui.md §3, ROADMAP P3-2

use glam::Vec2;

use crate::arena::ItemId;

/// ขนาดเล็กสุดที่ layout ยอมวาง — 0 ทำให้หารศูนย์ในขั้นต่อไปทั้งสาย
///
/// ★ `pub` เพราะเป็น **สัญญาที่ผู้เรียกเชื่อได้** ไม่ใช่รายละเอียดภายใน —
/// `fuzz_layout` ยืนยันสัญญานี้ทุกคืน ถ้าซ่อนไว้ target จะ assert ได้แค่ `> 0.0`
/// ซึ่ง **อ่อนเกินกว่าจะจับการถอดด่านสุดท้ายออก** (ลองแล้ว: 157,000 รอบไม่แดง)
pub const MIN_SIDE: f32 = 1.0;

/// ขนาดใหญ่สุดที่ layout ยอมวาง — กัน aspect สุดโต่งไม่ให้ระเบิดเป็นแถบยาวข้าม board
pub const MAX_SIDE: f32 = 100_000.0;

/// พิกัดไกลสุดที่ layout ยอมวาง — **คนละเรื่องกับ [`MAX_SIDE`] และต้องใหญ่กว่ามาก**
///
/// ★★★ **แยกออกมาตอน P3-3 เพราะของเดิมทำให้เกณฑ์ของ ROADMAP เองเป็นไปไม่ได้**
///
/// เดิม `sane_pos` ใช้ `MAX_SIDE` (100,000) คุมทั้ง *ขนาด* และ *ตำแหน่ง* ซึ่งฟังดูสมเหตุ
/// สมผลจนกระทั่งมีคนเอา engine ไปวาง 10,000 ใบจริง: แผ่น 4 คอลัมน์ ช่องละ ~200 px
/// สูงถึง ~500,000 หน่วย → **ทุกแถวตั้งแต่แถวที่ ~490 ลงไปถูก clamp มาทับกันที่
/// y = 100,000 พอดี** ผลคือภาพหลายพันใบซ้อนกันเป็นกองเดียวโดยไม่มี error ที่ไหนเลย
/// (P3-3 บังคับ 10,000 ใบ ส่วน `MAX_SIDE` เป็นเพดานของ *ด้านหนึ่งด้าน* ไม่ใช่ของแผ่น)
///
/// เลือก 1,000,000 เพราะ f32 ยังมีความละเอียด ~0.06 หน่วยที่ระยะนั้น (ต่ำกว่าหนึ่ง
/// พิกเซลบนจอที่ zoom = 1) — ที่ 1e7 ulp จะเป็น 1.0 เต็ม ๆ แล้วภาพจะกระตุกเป็นขั้น
/// ตอนเลื่อน · ยังกันค่าพังจากไฟล์ได้เหมือนเดิม แค่กันที่ระยะที่ layout จริงไปถึงได้
pub const MAX_COORD: f32 = 1_000_000.0;

/// สัดส่วนกว้าง:สูงที่ยอมรับ — เกินนี้ถือว่าไฟล์โกหก (I-4)
const MAX_ASPECT_RATIO: f32 = 1000.0;

/// ผลของการจัดวางหนึ่งใบ
///
/// ★★ **`top_left` ไม่ใช่ `ItemCanvas::pos`** — `ItemCanvas::pos` คือ **จุดกึ่งกลาง**
/// (docs/02 §2.1) ผู้เรียกต้องบวก `size * 0.5` เอง
///
/// ตั้งชื่อให้ต่างกันโดยตั้งใจ: เคยพลาดเรื่องนี้มาแล้วตอน P2-6 (`quad.transform`
/// ใช้มุมซ้ายบน ส่วน `ItemCanvas::pos` ใช้กึ่งกลาง) ถ้าตั้งชื่อว่า `pos` เฉย ๆ
/// การเผลอเอาไปใส่ตรง ๆ จะทำให้ **ทุกภาพเลื่อนไปครึ่งตัว** ซึ่งดูเหมือน
/// "layout เพี้ยนนิดหน่อย" มากกว่าดูเหมือนบั๊ก
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// item ที่ถูกวาง
    pub id: ItemId,
    /// **มุมซ้ายบน** ในพิกัดของ layout (เริ่มที่ `(0, 0)` เสมอ)
    pub top_left: Vec2,
    /// ขนาดที่จัดให้ — finite และ `>= MIN_SIDE` เสมอ
    pub size: Vec2,
}

impl Placed {
    /// จุดกึ่งกลาง — ค่าที่เอาไปใส่ `ItemCanvas::pos` ได้ตรง ๆ
    #[must_use]
    pub fn centre(self) -> Vec2 {
        self.top_left + self.size * 0.5
    }
}

/// ตัวตั้งของการจัดวาง
///
/// ★ `width` **ไม่มีใน `docs/03 §3`** แต่ขาดไม่ได้: `JustifiedRows` ต้องรู้ว่า
/// แถวหนึ่งกว้างเท่าไหร่ถึงจะขึ้นแถวใหม่ และ `Grid` ต้องรู้ว่าช่องกว้างเท่าไหร่
/// — เอกสารระบุแต่ `target_row_height` ซึ่งคุมแค่แกนเดียว
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutParams {
    /// ความกว้างที่มีให้จัด (world unit)
    pub width: f32,
    /// ช่องว่างระหว่างภาพ
    pub gap: f32,
    /// ความสูงแถวที่อยากได้ — `JustifiedRows` ใช้เป็นเป้าก่อนสเกลให้พอดี
    pub target_row_height: f32,
    /// จำนวนคอลัมน์ — `None` = ให้ engine เดาจากจำนวนภาพ
    pub columns: Option<u32>,
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            width: 1600.0,
            gap: 12.0,
            target_row_height: 220.0,
            columns: None,
        }
    }
}

impl LayoutParams {
    /// ค่าที่ใช้ได้จริง — **ทุก engine ต้องเรียกก่อนใช้** (I-4)
    ///
    /// ค่าจากไฟล์/UI เป็น `NaN` ได้ และ `NaN` ตัวเดียวจะไหลไปทุกตำแหน่ง
    #[must_use]
    fn sane(self) -> Self {
        let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        Self {
            width: finite(self.width, 1600.0).clamp(MIN_SIDE, MAX_SIDE),
            // gap ติดลบ = ภาพซ้อนกัน ซึ่งไม่ใช่สิ่งที่ layout engine ควรผลิต
            gap: finite(self.gap, 12.0).clamp(0.0, MAX_SIDE),
            target_row_height: finite(self.target_row_height, 220.0).clamp(MIN_SIDE, MAX_SIDE),
            // 0 คอลัมน์ = หารศูนย์ · เพดานกันรายการที่ทำให้เกิดคอลัมน์เปล่าหลายพัน
            columns: self.columns.map(|c| c.clamp(1, 10_000)),
        }
    }

    /// จำนวนคอลัมน์ที่จะใช้จริง
    ///
    /// ไม่ระบุมา → เดาให้เป็นตารางที่ใกล้จัตุรัสที่สุด ซึ่งเป็นสิ่งที่คนคาดหวัง
    /// เมื่อกด "จัดเป็นตาราง" โดยไม่ตั้งค่าอะไร
    fn columns_for(self, count: usize) -> usize {
        if let Some(columns) = self.columns {
            return columns as usize;
        }
        if count == 0 {
            return 1;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "จำนวนภาพจริงไม่ถึงช่วงที่ f64 เสียความละเอียด"
        )]
        let guess = (count as f64).sqrt().ceil() as usize;
        guess.max(1)
    }
}

/// อัตราส่วน กว้าง/สูง ที่ใช้ได้ — **จุดเดียวที่ aspect ถูกทำให้เชื่อถือได้**
///
/// ★ ภาพจริงมี `px_size` ที่เป็น 0 ได้ (ไฟล์เสีย/cache เพี้ยน) และ `Vec2::NAN`
/// มาจากไฟล์ `.refx` ที่แก้มาได้ · ทุก engine เรียกตัวนี้ตัวเดียว จะได้ไม่มี
/// engine ไหนลืมตรวจแล้วเพี้ยนคนเดียว
fn ratio_of(aspect: Vec2) -> f32 {
    if !aspect.is_finite() || aspect.x <= 0.0 || aspect.y <= 0.0 {
        return 1.0; // จัตุรัส — ค่าที่ปลอดภัยและผู้ใช้เห็นว่ามีของอยู่
    }
    (aspect.x / aspect.y).clamp(1.0 / MAX_ASPECT_RATIO, MAX_ASPECT_RATIO)
}

/// ขนาดที่ผ่านการตรวจแล้ว — ทุก engine ต้องส่งผลผ่านตัวนี้ก่อนคืนออกไป
fn sane_size(size: Vec2) -> Vec2 {
    let side = |v: f32| {
        if v.is_finite() {
            v.clamp(MIN_SIDE, MAX_SIDE)
        } else {
            MIN_SIDE
        }
    };
    Vec2::new(side(size.x), side(size.y))
}

/// ตำแหน่งที่ผ่านการตรวจแล้ว — เพดานคือ [`MAX_COORD`] **ไม่ใช่** [`MAX_SIDE`]
fn sane_pos(pos: Vec2) -> Vec2 {
    let axis = |v: f32| {
        if v.is_finite() {
            v.clamp(-MAX_COORD, MAX_COORD)
        } else {
            0.0
        }
    };
    Vec2::new(axis(pos.x), axis(pos.y))
}

/// engine ที่จะใช้จัด
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Engine {
    /// ช่องเท่ากันทุกช่อง — คัดภาพ/ดูรวม
    #[default]
    Grid,
    /// คอลัมน์คงที่ วางลงคอลัมน์ที่เตี้ยสุด — ภาพสัดส่วนต่างกันมาก
    Masonry,
    /// เติมแถวจนเกินความกว้างแล้วสเกลทั้งแถวให้พอดี (แบบ Flickr/Google Photos)
    JustifiedRows,
    /// อัดให้แน่นที่สุด — first-fit-decreasing-height
    ShelfPack,
    /// วางเป็นวงรอบภาพแรก — เทียบภาพรอบภาพหลัก
    Radial,
}

/// จัดวางตาม engine ที่เลือก
///
/// รายการว่าง → ผลว่าง (ไม่ใช่ error) · ผลลัพธ์เรียงตามลำดับ input เสมอ
#[must_use]
pub fn layout(engine: Engine, items: &[(ItemId, Vec2)], params: LayoutParams) -> Vec<Placed> {
    if items.is_empty() {
        return Vec::new();
    }
    let params = params.sane();
    let placed = match engine {
        Engine::Grid => grid(items, params),
        Engine::Masonry => masonry(items, params),
        Engine::JustifiedRows => justified_rows(items, params),
        Engine::ShelfPack => shelf_pack(items, params),
        Engine::Radial => radial(items, params),
    };
    // ★★ ด่านสุดท้ายด่านเดียวสำหรับทุก engine — ไม่มีทางที่ค่าพังจะหลุดออกไป
    //    แม้ engine ใหม่ที่ใครเพิ่มทีหลังจะลืมตรวจเอง
    //
    //    ยืนยันว่าล้มเป็นแล้ว **สองทาง**: เอาบล็อกนี้ออกแล้ว
    //    (ก) unit test `every_engine_survives_broken_input` แดงทันที
    //    (ข) `fuzz_layout` แดงภายในไม่กี่วินาที: `Vec2(1.0, 0.001)` จาก Masonry
    placed
        .into_iter()
        .map(|p| Placed {
            id: p.id,
            top_left: sane_pos(p.top_left),
            size: sane_size(p.size),
        })
        .collect()
}

/// ช่องเท่ากันทุกช่อง — ภาพถูก fit ลงในช่องโดยคงสัดส่วน
fn grid(items: &[(ItemId, Vec2)], params: LayoutParams) -> Vec<Placed> {
    let columns = params.columns_for(items.len());
    #[expect(clippy::cast_precision_loss, reason = "จำนวนคอลัมน์เล็ก")]
    let columns_f = columns as f32;
    // ความกว้างที่เหลือหลังหัก gap ระหว่างช่องออก
    let cell_w = ((params.width - params.gap * (columns_f - 1.0)) / columns_f).max(MIN_SIDE);
    let cell_h = cell_w; // ช่องจัตุรัส — "ช่องเท่ากัน" ตามที่ docs/03 §3 ระบุ

    items
        .iter()
        .enumerate()
        .map(|(index, (id, aspect))| {
            let (col, row) = (index % columns, index / columns);
            let ratio = ratio_of(*aspect);
            // fit ในช่อง: ด้านที่ยาวกว่าชนขอบช่อง อีกด้านหดตาม
            let size = if ratio >= 1.0 {
                Vec2::new(cell_w, cell_w / ratio)
            } else {
                Vec2::new(cell_h * ratio, cell_h)
            };
            #[expect(clippy::cast_precision_loss, reason = "ดัชนีในช่วงที่ f32 แทนได้ตรง")]
            let cell = Vec2::new(
                col as f32 * (cell_w + params.gap),
                row as f32 * (cell_h + params.gap),
            );
            // ★ จัดกึ่งกลางช่อง ไม่ใช่ชิดมุม — ภาพแนวตั้งกับแนวนอนคละกันแล้ว
            //   ชิดมุมจะอ่านว่า "เรียงไม่ตรง" ทั้งที่ช่องตรงกันหมด
            Placed {
                id: *id,
                top_left: cell + (Vec2::new(cell_w, cell_h) - size) * 0.5,
                size,
            }
        })
        .collect()
}

/// คอลัมน์คงที่ วางลงคอลัมน์ที่เตี้ยที่สุด
fn masonry(items: &[(ItemId, Vec2)], params: LayoutParams) -> Vec<Placed> {
    let columns = params.columns_for(items.len());
    #[expect(clippy::cast_precision_loss, reason = "จำนวนคอลัมน์เล็ก")]
    let columns_f = columns as f32;
    let col_w = ((params.width - params.gap * (columns_f - 1.0)) / columns_f).max(MIN_SIDE);
    let mut heights = vec![0.0f32; columns];

    items
        .iter()
        .map(|(id, aspect)| {
            // ★ เลือกคอลัมน์เตี้ยสุด และ **ตัดสินเสมอด้วยดัชนีที่น้อยกว่า**
            //   ถ้าใช้ `min_by` เฉย ๆ กับค่าที่เท่ากัน ผลจะขึ้นกับรายละเอียดของ
            //   การเปรียบเทียบ — ที่นี่บังคับให้ซ้ายสุดชนะเสมอ (deterministic)
            let mut best = 0usize;
            for (index, height) in heights.iter().enumerate() {
                if *height < heights[best] {
                    best = index;
                }
            }
            let size = Vec2::new(col_w, col_w / ratio_of(*aspect));
            #[expect(clippy::cast_precision_loss, reason = "จำนวนคอลัมน์เล็ก")]
            let x = best as f32 * (col_w + params.gap);
            let placed = Placed {
                id: *id,
                top_left: Vec2::new(x, heights[best]),
                size,
            };
            heights[best] += size.y + params.gap;
            placed
        })
        .collect()
}

/// เติมแถวจนเกินความกว้าง แล้วสเกลทั้งแถวให้พอดีขอบ (Flickr/Google Photos)
fn justified_rows(items: &[(ItemId, Vec2)], params: LayoutParams) -> Vec<Placed> {
    let mut out = Vec::with_capacity(items.len());
    let mut row: Vec<(ItemId, f32)> = Vec::new(); // (id, ratio)
    let mut y = 0.0f32;

    // ★ แยกเป็นคลอเชอร์เพื่อให้ "ปิดแถว" มีที่เดียว — แถวสุดท้ายกับแถวที่เต็ม
    //   ต้องคิดความสูงคนละแบบ แต่การวางต้องเหมือนกันเป๊ะ
    let mut flush = |row: &mut Vec<(ItemId, f32)>, y: &mut f32, height: f32| {
        let mut x = 0.0f32;
        for (id, ratio) in row.iter() {
            let size = Vec2::new(height * ratio, height);
            out.push(Placed {
                id: *id,
                top_left: Vec2::new(x, *y),
                size,
            });
            x += size.x + params.gap;
        }
        *y += height + params.gap;
        row.clear();
    };

    for (id, aspect) in items {
        row.push((*id, ratio_of(*aspect)));
        let ratio_sum: f32 = row.iter().map(|(_, r)| *r).sum();
        #[expect(clippy::cast_precision_loss, reason = "จำนวนภาพต่อแถวน้อย")]
        let gaps = params.gap * (row.len() - 1) as f32;
        // ความสูงที่ทำให้แถวนี้กว้างพอดีขอบ
        let fitted = if ratio_sum > 0.0 {
            (params.width - gaps) / ratio_sum
        } else {
            params.target_row_height
        };
        // แถวเต็มเมื่อการยัดเพิ่มทำให้ต้องหดต่ำกว่าความสูงเป้าหมาย
        if fitted <= params.target_row_height {
            flush(&mut row, &mut y, fitted.max(MIN_SIDE));
        }
    }
    // ★ แถวสุดท้ายที่ยังไม่เต็ม **ห้ามยืดให้เต็มขอบ** — ภาพสองใบท้ายจะกลายเป็น
    //   แบนเนอร์ยักษ์ ซึ่งเป็นข้อผิดพลาดคลาสสิกของ layout แบบนี้
    if !row.is_empty() {
        flush(&mut row, &mut y, params.target_row_height);
    }
    out
}

/// first-fit-decreasing-height — อัดให้แน่นที่สุด
fn shelf_pack(items: &[(ItemId, Vec2)], params: LayoutParams) -> Vec<Placed> {
    // ทุกใบสูงเท่า target ก่อน แล้วกว้างตามสัดส่วน — ชั้นจึงเรียบเสมอกัน
    let mut order: Vec<(usize, ItemId, Vec2)> = items
        .iter()
        .enumerate()
        .map(|(index, (id, aspect))| {
            let ratio = ratio_of(*aspect);
            let height = params.target_row_height;
            (index, *id, Vec2::new(height * ratio, height))
        })
        .collect();
    // ★ เรียงจากสูงไปเตี้ย (decreasing height) แล้ว **ตัดสินเสมอด้วยดัชนีเดิม**
    //   ที่นี่ความสูงเท่ากันหมดโดยธรรมชาติของการตั้งค่าข้างบน ตัวตัดสินจริงจึงเป็น
    //   ความกว้าง — เรียงกว้างไปแคบทำให้ชั้นเต็มพอดีกว่า และ `then` ที่ท้าย
    //   ทำให้ผลเหมือนเดิมทุกครั้งแม้ค่าจะเท่ากันเป๊ะ (docs/03 §3: deterministic)
    order.sort_by(|a, b| {
        b.2.y
            .total_cmp(&a.2.y)
            .then_with(|| b.2.x.total_cmp(&a.2.x))
            .then_with(|| a.0.cmp(&b.0))
    });

    /// ชั้นหนึ่งชั้น — ความกว้างที่ใช้ไปแล้วและความสูงของชั้น
    struct Shelf {
        y: f32,
        used: f32,
        height: f32,
    }
    let mut shelves: Vec<Shelf> = Vec::new();
    let mut next_y = 0.0f32;
    let mut placed: Vec<(usize, Placed)> = Vec::with_capacity(items.len());

    for (index, id, size) in order {
        let need = size.x;
        // first-fit: ชั้นแรกที่ยังใส่ได้
        let mut chosen = None;
        for (slot, shelf) in shelves.iter().enumerate() {
            let start = if shelf.used > 0.0 {
                shelf.used + params.gap
            } else {
                0.0
            };
            if start + need <= params.width {
                chosen = Some(slot);
                break;
            }
        }
        let slot = match chosen {
            Some(slot) => slot,
            None => {
                shelves.push(Shelf {
                    y: next_y,
                    used: 0.0,
                    height: size.y,
                });
                next_y += size.y + params.gap;
                shelves.len() - 1
            }
        };
        let shelf = &mut shelves[slot];
        let x = if shelf.used > 0.0 {
            shelf.used + params.gap
        } else {
            0.0
        };
        shelf.used = x + need;
        shelf.height = shelf.height.max(size.y);
        placed.push((
            index,
            Placed {
                id,
                top_left: Vec2::new(x, shelf.y),
                size,
            },
        ));
    }

    // คืนตามลำดับ input เดิม — ผู้เรียกจับคู่กับรายการที่ส่งมาได้ตรง ๆ
    placed.sort_by_key(|(index, _)| *index);
    placed.into_iter().map(|(_, p)| p).collect()
}

/// วางเป็นวงรอบภาพแรก
fn radial(items: &[(ItemId, Vec2)], params: LayoutParams) -> Vec<Placed> {
    let centre_size = Vec2::splat(params.target_row_height * 1.5);
    let satellite = params.target_row_height;
    // รัศมีต้องกว้างพอให้ดาวบริวารไม่ทับภาพกลาง
    let radius = (centre_size.x * 0.5 + satellite * 0.75 + params.gap).max(MIN_SIDE);
    let origin = Vec2::splat(radius + satellite);

    let count = items.len().saturating_sub(1);
    items
        .iter()
        .enumerate()
        .map(|(index, (id, aspect))| {
            let ratio = ratio_of(*aspect);
            if index == 0 {
                // ★ ภาพแรกคือภาพหลักที่อยู่ตรงกลาง — docs/03 §3 เขียนว่า "วางเป็นวง
                //   รอบ item ที่ pin ไว้" แต่ input ไม่มีธง pin มาให้ (ดู §ท้ายไฟล์)
                //   จึงใช้ **ใบแรกในรายการ** ซึ่งผู้เรียกเลือกเองได้ว่าจะส่งใบไหนมาก่อน
                let size = if ratio >= 1.0 {
                    Vec2::new(centre_size.x, centre_size.x / ratio)
                } else {
                    Vec2::new(centre_size.y * ratio, centre_size.y)
                };
                return Placed {
                    id: *id,
                    top_left: origin - size * 0.5,
                    size,
                };
            }
            let size = if ratio >= 1.0 {
                Vec2::new(satellite, satellite / ratio)
            } else {
                Vec2::new(satellite * ratio, satellite)
            };
            #[expect(clippy::cast_precision_loss, reason = "จำนวนภาพในวงไม่มาก")]
            let step = std::f32::consts::TAU / count.max(1) as f32;
            #[expect(clippy::cast_precision_loss, reason = "ดัชนีเล็ก")]
            let angle = step * (index - 1) as f32;
            // ★ เริ่มที่ 12 นาฬิกาแล้วเดินตามเข็ม — ตรงกับที่ตาคาดหวังเวลาอ่านวง
            let centre = origin + Vec2::new(angle.sin(), -angle.cos()) * radius;
            Placed {
                id: *id,
                top_left: centre - size * 0.5,
                size,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// ★ สิ่งที่ spec ระบุแต่ไฟล์นี้ยังไม่ทำ — `respect_pinned`
// ---------------------------------------------------------------------------
//
// `docs/03 §3` ใส่ `respect_pinned: bool` ไว้ใน `LayoutParams` และ §4.1 เขียนว่า
// "ไม่ขยับภาพที่ `pinned = true` (layout จะจัดรอบมันแทน)"
//
// แต่ **input ที่ spec กำหนดเองคือ `&[(ItemId, Vec2 aspect)]` ซึ่งไม่มีธง pin
// และไม่มีตำแหน่ง/ขนาดจริงของภาพที่ pin ไว้** — engine จึงมองไม่เห็นสิ่งที่
// ต้องจัดรอบ ธงตัวนั้นจึงเป็นค่าที่ไม่มีทางมีผล
//
// เก็บฟิลด์ที่ไม่มีผลไว้ = กับดักของคนอ่านรอบหน้า (เหตุผลเดียวกับที่ `BoardSettings::snap`
// ถูกลบตอน P2-9) จึง **ไม่ใส่** และแยกงานเป็นสองส่วนตามความจริง:
//
//   * "ไม่ขยับภาพที่ pin" → ผู้เรียก **กรองออกก่อนส่งมา** ทำได้เลยตั้งแต่ P3-5
//   * "จัดรอบภาพที่ pin"  → เป็นปัญหาคนละข้อ (ต้องรู้กรอบจริงของภาพที่ pin แล้ว
//     หลบมัน) ซึ่งต้องเปลี่ยน input ของทุก engine → **ตัดสินตอน P3-5** ที่นั่นมี
//     `Board` อยู่ในมือและเห็นว่าจำเป็นแค่ไหน

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const ALL: [Engine; 5] = [
        Engine::Grid,
        Engine::Masonry,
        Engine::JustifiedRows,
        Engine::ShelfPack,
        Engine::Radial,
    ];

    fn id(n: u32) -> ItemId {
        use crate::arena::ArenaKey as _;
        ItemId::from_parts(n, 0)
    }

    /// รายการ `n` ใบ สัดส่วนคละกันแบบที่ mood board จริงเป็น
    fn items(n: u32) -> Vec<(ItemId, Vec2)> {
        (0..n)
            .map(|i| {
                let aspect = match i % 4 {
                    0 => Vec2::new(4.0, 3.0),
                    1 => Vec2::new(3.0, 4.0),
                    2 => Vec2::new(16.0, 9.0),
                    _ => Vec2::new(1.0, 1.0),
                };
                (id(i), aspect)
            })
            .collect()
    }

    // ---------- property: ต้องมีเสมอทั้งสองข้อ (ROADMAP P3-2) ----------

    /// ★★★ **input เดิมต้องได้ output เดิมเป๊ะ** — ทุก engine
    ///
    /// ละเมิดข้อนี้แล้วผู้ใช้เห็น "ภาพเรียงไม่เหมือนเดิมทุกครั้งที่กด" ซึ่งเป็น
    /// บั๊กที่หาสาเหตุยากที่สุดแบบหนึ่ง เพราะมันถูก *เกือบ* ทุกครั้ง
    #[test]
    fn every_engine_is_deterministic() {
        let list = items(37);
        let params = LayoutParams::default();
        for engine in ALL {
            let first = layout(engine, &list, params);
            assert_eq!(first.len(), list.len(), "{engine:?} วางไม่ครบ");
            for _ in 0..20 {
                assert_eq!(layout(engine, &list, params), first, "{engine:?} ไม่คงที่");
            }
        }
    }

    /// ★★★ **ทุกตัวเลขที่ออกไปต้อง finite** — ต่อให้ input จะพังแค่ไหน
    ///
    /// `NaN` ที่หลุดลง `ItemCanvas` ทำให้ hit-test/culling/spatial index เพี้ยน
    /// ทั้งระบบโดยไม่มี error ที่ไหนเลย (I-4)
    #[test]
    fn every_engine_survives_broken_input() {
        let broken = vec![
            (id(0), Vec2::new(f32::NAN, 1.0)),
            (id(1), Vec2::new(0.0, 0.0)),
            (id(2), Vec2::new(-5.0, 2.0)),
            (id(3), Vec2::new(f32::INFINITY, f32::INFINITY)),
            (id(4), Vec2::new(1e30, 1e-30)),
            (id(5), Vec2::new(1.0, f32::NEG_INFINITY)),
        ];
        let nasty = [
            LayoutParams::default(),
            LayoutParams {
                width: f32::NAN,
                gap: f32::NAN,
                target_row_height: f32::NAN,
                columns: Some(0),
            },
            LayoutParams {
                width: 0.0,
                gap: -100.0,
                target_row_height: -1.0,
                columns: Some(u32::MAX),
            },
            LayoutParams {
                width: f32::INFINITY,
                gap: f32::INFINITY,
                target_row_height: f32::INFINITY,
                columns: None,
            },
        ];
        for engine in ALL {
            for params in nasty {
                let out = layout(engine, &broken, params);
                assert_eq!(out.len(), broken.len(), "{engine:?} วางไม่ครบ");
                for placed in out {
                    assert!(
                        placed.top_left.is_finite() && placed.size.is_finite(),
                        "{engine:?} คืนค่าที่ไม่ finite: {placed:?}"
                    );
                    assert!(
                        placed.size.x >= MIN_SIDE && placed.size.y >= MIN_SIDE,
                        "{engine:?} คืนขนาดที่เล็กจนหารศูนย์ได้: {placed:?}"
                    );
                }
            }
        }
    }

    /// ★★★ 10,000 ใบ (เกณฑ์ของ P3-3) ต้อง **ไม่ถูก clamp มาทับกันที่แถวเดียว**
    ///
    /// นี่คือเทสต์ที่ [`MAX_COORD`] มีอยู่เพราะมัน: ตอน `sane_pos` ใช้ `MAX_SIDE`
    /// (100,000) แผ่น 4 คอลัมน์ที่ช่องละ ~200 หน่วยจะสูง ~500,000 → แถวที่ ~490
    /// ลงไป **ทุกแถว** ถูกกดมาอยู่ที่ y = 100,000 เท่ากันหมด ภาพหลายพันใบซ้อนกัน
    /// เป็นกองเดียว **โดยไม่มี error ที่ไหน** — ผู้ใช้เห็นเป็น "เลื่อนลงไปแล้วภาพหาย"
    ///
    /// เทสต์นี้ตรวจสิ่งที่ผู้ใช้สังเกตจริง (แถวสุดท้ายอยู่ต่ำกว่าแถวก่อนหน้า
    /// และจำนวน y ที่ต่างกันต้องเท่ากับจำนวนแถว) ไม่ใช่ตรวจว่า "ค่าเท่ากับเลขนี้"
    #[test]
    fn ten_thousand_items_do_not_pile_up_on_one_row() {
        let list = items(10_000);
        let params = LayoutParams {
            width: 800.0,
            gap: 12.0,
            columns: Some(4),
            target_row_height: 200.0,
        };
        let out = layout(Engine::Grid, &list, params);
        assert_eq!(out.len(), 10_000);

        // แถวละ 4 ใบ → ต้องได้ค่า y ที่ต่างกัน 2,500 ค่า
        // ★ ใช้ **จุดกึ่งกลาง** เพราะ Grid จัดภาพกึ่งกลางช่อง ขอบบนจึงต่างกันตามสัดส่วน
        //   ของแต่ละใบ ส่วนกึ่งกลางช่องเป็นของแถว = สิ่งที่เราหมายถึงจริง ๆ
        let mut tops: Vec<f32> = out.iter().map(|p| p.centre().y).collect();
        tops.sort_by(f32::total_cmp);
        tops.dedup();
        assert_eq!(
            tops.len(),
            2_500,
            "แถวถูกยุบมาทับกัน — มี y ที่ต่างกันแค่ {} ค่า",
            tops.len()
        );

        // และใบสุดท้ายต้องอยู่ต่ำกว่าเพดานเก่าไปไกล ๆ (พิสูจน์ว่าเพดานใหม่ถูกใช้จริง)
        let lowest = out.iter().map(|p| p.top_left.y).fold(0.0f32, f32::max);
        assert!(
            lowest > MAX_SIDE,
            "แผ่น 10,000 ใบต้องสูงเกิน MAX_SIDE จริง ไม่งั้นเทสต์นี้ไม่ได้ตรวจอะไร \
             (สูงสุด {lowest})"
        );
        assert!(lowest <= MAX_COORD, "ตำแหน่งต้องยังอยู่ใต้เพดานใหม่");
    }

    /// รายการว่างต้องได้ผลว่าง ไม่ใช่ panic
    #[test]
    fn an_empty_board_lays_out_to_nothing() {
        for engine in ALL {
            assert!(layout(engine, &[], LayoutParams::default()).is_empty());
        }
    }

    /// ★ ทุก engine ต้องคืน **ครบทุกใบ ไม่ซ้ำ ไม่หาย และเรียงตาม input**
    ///
    /// `ShelfPack` เรียงภายในใหม่เพื่อจัดวาง ถ้าลืมเรียงกลับ ผู้เรียกที่จับคู่
    /// ผลลัพธ์กับรายการเดิมตามดัชนีจะสลับภาพกันทั้งกระดาน
    #[test]
    fn every_engine_returns_each_item_once_in_input_order() {
        let list = items(23);
        for engine in ALL {
            let out = layout(engine, &list, LayoutParams::default());
            let got: Vec<ItemId> = out.iter().map(|p| p.id).collect();
            let want: Vec<ItemId> = list.iter().map(|(id, _)| *id).collect();
            assert_eq!(got, want, "{engine:?} คืนลำดับ/ชุดไม่ตรงกับ input");
        }
    }

    // ---------- คุณสมบัติเฉพาะตัวของแต่ละ engine ----------

    /// Grid: ช่องต้องตรงกันเป็นตารางจริง และไม่ล้นความกว้าง
    #[test]
    fn grid_columns_line_up_and_stay_inside_the_width() {
        let list = items(10);
        let params = LayoutParams {
            width: 1000.0,
            gap: 10.0,
            columns: Some(4),
            ..LayoutParams::default()
        };
        let out = layout(Engine::Grid, &list, params);
        // แถวแรกสี่ใบ แถวสองสี่ใบ แถวสามสองใบ
        let row_of = |i: usize| out[i].centre().y;
        assert!(
            (row_of(0) - row_of(3)).abs() < 1e-3,
            "แถวแรกต้องอยู่ระดับเดียวกัน"
        );
        assert!(row_of(4) > row_of(0), "แถวสองต้องอยู่ต่ำกว่าแถวแรก");
        for placed in &out {
            assert!(
                placed.top_left.x >= -1e-3 && placed.top_left.x + placed.size.x <= 1000.0 + 1e-3,
                "ล้นความกว้าง: {placed:?}"
            );
        }
    }

    /// Masonry: ทุกใบกว้างเท่ากัน และคอลัมน์เตี้ยสุดได้ใบถัดไป
    #[test]
    fn masonry_fills_the_shortest_column() {
        let list = items(12);
        let params = LayoutParams {
            width: 900.0,
            gap: 10.0,
            columns: Some(3),
            ..LayoutParams::default()
        };
        let out = layout(Engine::Masonry, &list, params);
        let width = out[0].size.x;
        for placed in &out {
            assert!((placed.size.x - width).abs() < 1e-3, "คอลัมน์ต้องกว้างเท่ากัน");
        }
        // สามใบแรกต้องลงคนละคอลัมน์ (ทุกคอลัมน์ยังสูง 0 เท่ากัน → ซ้ายสุดชนะไล่ไป)
        let xs: Vec<f32> = out.iter().take(3).map(|p| p.top_left.x).collect();
        assert!(
            xs[0] < xs[1] && xs[1] < xs[2],
            "สามใบแรกต้องกระจายคนละคอลัมน์: {xs:?}"
        );
    }

    /// ★ JustifiedRows: แถวที่ **เต็ม** ต้องกว้างพอดีขอบ
    #[test]
    fn justified_rows_fill_the_width_exactly() {
        let list = items(40);
        let params = LayoutParams {
            width: 1200.0,
            gap: 8.0,
            target_row_height: 200.0,
            columns: None,
        };
        let out = layout(Engine::JustifiedRows, &list, params);

        // จัดกลุ่มตาม y แล้วดูแถวที่ไม่ใช่แถวสุดท้าย
        let mut rows: Vec<Vec<&Placed>> = Vec::new();
        for placed in &out {
            match rows
                .last_mut()
                .filter(|row| (row[0].top_left.y - placed.top_left.y).abs() < 1e-3)
            {
                Some(row) => row.push(placed),
                None => rows.push(vec![placed]),
            }
        }
        assert!(rows.len() >= 3, "ต้องได้หลายแถว: {}", rows.len());
        for row in &rows[..rows.len() - 1] {
            let right = row.last().unwrap();
            let end = right.top_left.x + right.size.x;
            assert!(
                (end - 1200.0).abs() < 0.5,
                "แถวเต็มต้องจบพอดีขอบ 1200 แต่จบที่ {end}"
            );
        }
    }

    /// ★★ แถวสุดท้ายที่ยังไม่เต็ม **ห้ามถูกยืดให้เต็มขอบ**
    ///
    /// ข้อผิดพลาดคลาสสิกของ layout แบบนี้: ภาพสองใบท้ายกลายเป็นแบนเนอร์ยักษ์
    #[test]
    fn a_short_last_row_is_not_stretched() {
        // สองใบที่รวมกันแล้วไม่ถึงความกว้าง
        let list = vec![(id(0), Vec2::new(1.0, 1.0)), (id(1), Vec2::new(1.0, 1.0))];
        let params = LayoutParams {
            width: 2000.0,
            gap: 10.0,
            target_row_height: 200.0,
            columns: None,
        };
        let out = layout(Engine::JustifiedRows, &list, params);
        for placed in &out {
            assert!(
                (placed.size.y - 200.0).abs() < 1e-3,
                "แถวสุดท้ายต้องสูงเท่าเป้าหมาย ไม่ใช่ถูกยืด: {placed:?}"
            );
        }
    }

    /// ShelfPack: ไม่มีใบไหนล้นความกว้าง และภาพไม่ทับกัน
    #[test]
    fn shelf_pack_never_overflows_and_never_overlaps() {
        let list = items(25);
        let params = LayoutParams {
            width: 1000.0,
            gap: 6.0,
            target_row_height: 120.0,
            columns: None,
        };
        let out = layout(Engine::ShelfPack, &list, params);
        for placed in &out {
            assert!(
                placed.top_left.x + placed.size.x <= 1000.0 + 1e-3,
                "ล้นขอบ: {placed:?}"
            );
        }
        for (i, a) in out.iter().enumerate() {
            for b in out.iter().skip(i + 1) {
                let overlap_x = a.top_left.x < b.top_left.x + b.size.x
                    && b.top_left.x < a.top_left.x + a.size.x;
                let overlap_y = a.top_left.y < b.top_left.y + b.size.y
                    && b.top_left.y < a.top_left.y + a.size.y;
                assert!(!(overlap_x && overlap_y), "ทับกัน: {a:?} กับ {b:?}");
            }
        }
    }

    /// ★ Radial: ใบแรกอยู่กลาง ที่เหลืออยู่บนวงรัศมีเดียวกันและไม่ทับใบกลาง
    #[test]
    fn radial_puts_the_first_item_in_the_middle_of_a_ring() {
        let list = items(9);
        let out = layout(Engine::Radial, &list, LayoutParams::default());
        let centre = out[0].centre();
        let radii: Vec<f32> = out[1..]
            .iter()
            .map(|p| (p.centre() - centre).length())
            .collect();
        let first = radii[0];
        for r in &radii {
            assert!((r - first).abs() < 1e-2, "ต้องอยู่บนวงเดียวกัน: {radii:?}");
        }
        // ★ ดาวบริวารต้องไม่ทับภาพกลาง ไม่งั้น "ภาพหลัก" ถูกบัง
        for placed in &out[1..] {
            let gap = (placed.centre() - centre).length();
            let touching = out[0].size.length() * 0.5 + placed.size.length() * 0.5;
            assert!(gap > touching * 0.5, "ดาวบริวารทับภาพกลาง: {placed:?}");
        }
    }

    /// ภาพเดียวต้องไม่ทำให้ radial หารศูนย์
    #[test]
    fn radial_with_one_item_does_not_divide_by_zero() {
        let out = layout(
            Engine::Radial,
            &[(id(0), Vec2::new(4.0, 3.0))],
            LayoutParams::default(),
        );
        assert_eq!(out.len(), 1);
        assert!(out[0].top_left.is_finite() && out[0].size.is_finite());
    }

    /// ★ `centre()` ต้องเป็นค่าที่ใส่ `ItemCanvas::pos` ได้ตรง ๆ
    ///
    /// ป้องกันความสับสน "มุมซ้ายบน vs กึ่งกลาง" ที่เคยทำให้ภาพเลื่อนครึ่งตัวตอน P2-6
    #[test]
    fn centre_is_half_a_size_away_from_the_corner() {
        let placed = Placed {
            id: id(0),
            top_left: Vec2::new(10.0, 20.0),
            size: Vec2::new(100.0, 50.0),
        };
        assert_eq!(placed.centre(), Vec2::new(60.0, 45.0));
    }

    /// เพิ่มภาพเข้าไปแล้วผลของภาพเดิมต้องไม่กระโดดแบบสุ่ม (เสถียรพอจะดูรู้เรื่อง)
    #[test]
    fn adding_one_item_keeps_the_earlier_ones_where_they_were_for_streaming_engines() {
        let params = LayoutParams::default();
        // Masonry กับ JustifiedRows วางไล่ไปข้างหน้า ใบก่อนหน้าจึงไม่ควรขยับ
        // เมื่อมีใบใหม่ต่อท้าย (ยกเว้นแถวสุดท้ายของ JustifiedRows ที่ยังไม่ปิด)
        let short = items(8);
        let long = items(9);
        let a = layout(Engine::Masonry, &short, params);
        let b = layout(Engine::Masonry, &long, params);
        for (before, after) in a.iter().zip(b.iter()) {
            assert_eq!(before, after, "Masonry ขยับใบเดิมตอนมีใบใหม่");
        }
    }
}
