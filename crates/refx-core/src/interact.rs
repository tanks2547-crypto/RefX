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
use crate::board::{CropRect, ItemCanvas};
use crate::command::{Command, SetCrop, TransformItems};
use crate::geom::{Obb, Rect};
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

/// ปุ่มดัดแปลงที่มีผลกับการเลือกและกับ handle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// เพิ่ม/ถอดทีละตัว
    pub ctrl: bool,
    /// เพิ่มเข้าไปในชุดเดิม · ตอนสเกล = คงสัดส่วน · ตอนหมุน = สแนป 15° (docs/03 §2)
    pub shift: bool,
    /// ตอนสเกล = ยืดจากจุดกึ่งกลางแทนมุมตรงข้าม (docs/03 §2)
    pub alt: bool,
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
        /// ★ ปุ่มดัดแปลง **ณ เฟรมนี้** ไม่ใช่ตอนกด
        ///
        /// `Shift`/`Alt` ของ handle ต้องมีผลทันทีที่กดระหว่างลาก — คนกด Shift
        /// หลังเริ่มลากเป็นเรื่องปกติ ถ้าอ่านค่าแค่ตอนกดปุ่มเมาส์ ผู้ใช้จะสรุปว่า
        /// "คงสัดส่วนไม่ทำงาน" · การเลือก/rubber-band ยังใช้ค่าตอนกดเหมือนเดิม
        /// (เปลี่ยนกลางคันจะทำให้ชุดที่เลือกไว้กระโดด)
        modifiers: Modifiers,
    },
    /// ปล่อยปุ่ม
    Release {
        /// ปุ่มที่ปล่อย
        button: CanvasButton,
        /// ตำแหน่งใน world
        world: Vec2,
    },
    /// ★ ดับเบิลคลิก — ตอนอยู่ในเครื่องมือครอปแปลว่า **รีเซ็ตกรอบ crop** (docs/03 §2)
    ///
    /// มาเป็น event ของตัวเองเพราะการรวมคลิกสองครั้งเป็นเรื่องของ OS/ชั้น UI
    /// (ระยะเวลาและระยะทางที่ยอมรับได้ต่างกันไปตามระบบ) `refx-core` ไม่ควรเดาเอง
    DoubleClick {
        /// ตำแหน่งใน world
        world: Vec2,
    },
}

/// สิ่งที่ชั้น UI ต้องเอาไปทำต่อหลังส่ง event เข้ามาหนึ่งตัว
#[derive(Default)]
pub struct Interaction {
    /// กรอบ rubber-band ที่กำลังลากอยู่ (world space) — `None` = ไม่ต้องวาด
    pub rubber_band: Option<Rect>,
    /// มีอะไรเปลี่ยนที่ต้องวาดใหม่ไหม (I-1 — ไม่มีอะไรเปลี่ยนต้องไม่ขอเฟรม)
    pub needs_redraw: bool,
    /// ★ คำสั่งที่ต้องส่งเข้า `History`
    ///
    /// **การย้ายภาพคือการแก้ `Board`** จึงต้องผ่าน `Command` (ต่างจากการเลือก
    /// ที่ไม่ได้อยู่ใน `Board` — docs/02 §2.9) `TransformItems` merge ได้ การลาก
    /// ค้างทั้งครั้งจึงยุบเป็น undo ขั้นเดียว
    pub commands: Vec<Box<dyn Command>>,
    /// ★ ต้องเรียก `History::seal()` หรือไม่ — **จริงตอนปล่อยเมาส์**
    ///
    /// ถ้าไม่ seal การลากสองครั้งติดกันจะกลายเป็น undo เดียว ผู้ใช้จะงง (docs/02 §3)
    pub seal: bool,
}

impl std::fmt::Debug for Interaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Interaction")
            .field("rubber_band", &self.rubber_band)
            .field("needs_redraw", &self.needs_redraw)
            .field("commands", &self.commands.len())
            .field("seal", &self.seal)
            .finish()
    }
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
    /// ครึ่งหนึ่งของ handle มุม ในหน่วย world
    ///
    /// ★ คิดจาก **พิกเซลบนจอ ÷ zoom** เหมือน `drag_threshold` — handle ที่มีขนาด
    /// คงที่ใน world จะเล็กจนจับไม่โดนทันทีที่ซูมออก (HANDOFF §2.4)
    pub handle_reach: f32,
    /// ระยะจากมุมที่ยังนับว่าเป็นวงหมุน ในหน่วย world (ต้องมากกว่า `handle_reach`)
    pub rotate_reach: f32,
    /// เครื่องมือที่ผู้ใช้เลือกอยู่ — ตัดสินว่า handle ทำอะไรและมีกี่ตัว
    pub tool: Tool,
}

/// ระยะเริ่มลากเริ่มต้น (พิกเซลบนจอ) — กันมือสั่นตอนคลิก
pub const DEFAULT_DRAG_THRESHOLD_PX: f32 = 4.0;

/// ครึ่งหนึ่งของพื้นที่กด handle มุม (พิกเซลบนจอ)
///
/// **ใหญ่กว่ารูปที่วาด** ([`HANDLE_DRAW_PX`]) โดยตั้งใจ — พลาดแล้วกลายเป็นย้ายภาพ
/// เจ็บกว่ากดโดนตอนไม่ได้ตั้งใจ
pub const DEFAULT_HANDLE_PX: f32 = 7.0;

/// ความยาวด้านของ handle ที่ **วาด** (พิกเซลบนจอ) — ชั้น UI ใช้ค่านี้
pub const HANDLE_DRAW_PX: f32 = 9.0;

/// ระยะจากมุมที่ยังนับว่าเป็นวงหมุน (พิกเซลบนจอ)
pub const DEFAULT_ROTATE_PX: f32 = 22.0;

/// สแนปการหมุนเมื่อกด `Shift` — 15° ตาม docs/03 §2
const ROTATE_SNAP: f32 = std::f32::consts::TAU / 24.0;

/// ตัวคูณสเกลที่เล็กที่สุดที่ยอมรับ
///
/// ★ **ลากผ่านจุดยึดแล้วต้องไม่กลับด้าน** — `ItemCanvas::sanitized()` clamp `size`
/// ให้ ≥ `MIN_ITEM_SIZE` อยู่แล้ว ค่าติดลบจึงไม่ได้กลายเป็นภาพกลับด้าน
/// แต่กลายเป็นภาพที่ยุบเหลือจุดเดียว **แบบเงียบ ๆ** ซึ่งอ่านได้ว่า "ภาพหาย"
/// การกลับด้านเป็นหน้าที่ของ `Flip` (ปุ่ม `H`) ซึ่งผู้ใช้สั่งอย่างจงใจ
const MIN_SCALE: f32 = 1e-3;

/// ค่าที่เอามาใช้เป็นระยะได้จริง (I-4 — ตัวเลขจากไฟล์เสียต้องไม่ทำให้ hit-test มั่ว)
fn sane_reach(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// กรอบที่ handle เกาะอยู่ — `None` เมื่อไม่มีอะไรที่ขยับได้ถูกเลือก
///
/// * **ใบเดียว** → กรอบเอียงตามภาพ (`ItemCanvas::obb`) handle จึงหมุนตามภาพไปด้วย
///   ซึ่งเป็นสิ่งเดียวที่ทำให้สเกลภาพที่หมุนแล้วยังยืดไปตามแกนของตัวมันเอง
/// * **หลายใบ** → กรอบรวมแนวแกน (HANDOFF §2.4: สเกลกับ*กรอบรวม* ไม่ใช่ทีละใบ)
///
/// ★ **ภาพที่ล็อกไว้ไม่นับ** — handle ที่ลากแล้วไม่มีอะไรเกิดขึ้นแย่กว่าไม่มี handle
/// ผู้ใช้จะสรุปว่าโปรแกรมค้าง ไม่ใช่ว่าภาพถูกล็อก (กรอบเลือกยังวาดตามปกติ
/// การเลือกกับการแก้ได้เป็นคนละเรื่องกัน)
#[must_use]
pub fn selection_frame(board: &Board, selection: &Selection) -> Option<Obb> {
    let mut first: Option<ItemCanvas> = None;
    let mut count = 0usize;
    let mut bounds = Rect::EMPTY;
    for id in selection.iter() {
        let Some(item) = board.item(id) else { continue };
        if item.canvas.locked {
            continue;
        }
        count += 1;
        first.get_or_insert(item.canvas);
        bounds = bounds.union(item.canvas.world_bounds());
    }
    match count {
        0 => None,
        1 => first.map(ItemCanvas::obb),
        _ => {
            if !bounds.is_finite() || bounds.is_empty() {
                return None;
            }
            Some(Obb {
                center: bounds.center(),
                half_size: bounds.size() * 0.5,
                rotation: 0.0,
            })
        }
    }
}

/// เครื่องมือที่ผู้ใช้เลือกอยู่ (docs/03 §2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    /// `V` — เลือก / ย้าย / สเกล / หมุน
    #[default]
    Select,
    /// `C` — ครอปแบบไม่ทำลายต้นฉบับ · ดับเบิลคลิกรีเซ็ต
    Crop,
}

/// ส่วนของกรอบที่เคอร์เซอร์จับอยู่
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    /// handle บนกรอบ — สเกล (เครื่องมือเลือก) หรือครอป (เครื่องมือครอป)
    Edge(HandleDir),
    /// นอก handle มุมแต่ยังใกล้มุม = หมุน
    Rotate,
}

/// ทิศของ handle ในพิกัดท้องถิ่นของกรอบ — แต่ละแกนเป็น `-1`, `0` หรือ `1`
///
/// ★ นิยามเดียวใช้ได้ทั้งมุมและกลางด้าน: มุม = สองแกนไม่เป็นศูนย์ · กลางด้าน =
/// แกนหนึ่งเป็นศูนย์ · ค่านี้บอกตรง ๆ ว่า **ขอบไหนบ้างที่ handle นี้ขยับ**
/// ซึ่งเป็นสิ่งที่การครอปต้องรู้ (สเกลใช้แค่มุม ครอปใช้ครบทั้งแปดตัว)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleDir {
    /// -1 = ขอบซ้าย · 1 = ขอบขวา · 0 = ไม่แตะแกนนี้
    pub x: i8,
    /// -1 = ขอบบน · 1 = ขอบล่าง · 0 = ไม่แตะแกนนี้
    pub y: i8,
}

impl HandleDir {
    /// สี่มุม เรียงตรงกับลำดับของ[`Obb::corners`]
    pub const CORNERS: [Self; 4] = [
        Self { x: -1, y: -1 },
        Self { x: 1, y: -1 },
        Self { x: 1, y: 1 },
        Self { x: -1, y: 1 },
    ];
    /// กลางด้านสี่ตัว — ใช้เฉพาะตอนครอป
    pub const EDGES: [Self; 4] = [
        Self { x: 0, y: -1 },
        Self { x: 1, y: 0 },
        Self { x: 0, y: 1 },
        Self { x: -1, y: 0 },
    ];

    /// ตำแหน่งของ handle ตัวนี้บนกรอบ
    #[must_use]
    pub fn point_on(self, frame: Obb) -> Vec2 {
        let [ax, ay] = frame.axes();
        frame.center
            + ax * (frame.half_size.x * f32::from(self.x))
            + ay * (frame.half_size.y * f32::from(self.y))
    }
}

/// handle ทุกตัวที่เครื่องมือนี้ให้จับ — ชั้น UI ใช้วาด และ hit-test ใช้ไล่ตรวจ
///
/// สเกลให้จับได้แค่มุม (docs/03 §2) ส่วนครอปต้องมีกลางด้านด้วย ไม่งั้น "ตัดขอบบน
/// ออกนิดเดียว" ต้องไปลากมุมแล้วอีกแกนหนึ่งเปลี่ยนตามไปโดยไม่ได้ตั้งใจ
#[must_use]
pub fn handles_for(tool: Tool) -> &'static [HandleDir] {
    match tool {
        Tool::Select => &HandleDir::CORNERS,
        Tool::Crop => &ALL_HANDLES,
    }
}

/// มุมสี่ + กลางด้านสี่ (มุมมาก่อนเสมอ — ดู [`handle_at`])
const ALL_HANDLES: [HandleDir; 8] = [
    HandleDir::CORNERS[0],
    HandleDir::CORNERS[1],
    HandleDir::CORNERS[2],
    HandleDir::CORNERS[3],
    HandleDir::EDGES[0],
    HandleDir::EDGES[1],
    HandleDir::EDGES[2],
    HandleDir::EDGES[3],
];

/// handle ที่จุดนี้แตะอยู่ — **ต้องเรียกก่อน hit-test ของภาพเสมอ**
///
/// ไม่งั้นการลาก handle จะกลายเป็นการย้ายภาพ เพราะ handle มุมคร่อมตัวภาพอยู่ครึ่งหนึ่ง
/// (HANDOFF §2.4)
fn handle_at(
    tool: Tool,
    frame: Obb,
    world: Vec2,
    handle_reach: f32,
    rotate_reach: f32,
) -> Option<Handle> {
    if !world.is_finite() || !frame.center.is_finite() {
        return None;
    }
    let handle_reach = sane_reach(handle_reach);
    let rotate_reach = sane_reach(rotate_reach).max(handle_reach);
    let [ax, ay] = frame.axes();
    let near = |point: Vec2, reach: f32| {
        let offset = world - point;
        offset.dot(ax).abs() <= reach && offset.dot(ay).abs() <= reach
    };

    // ★ handle มาก่อนวงหมุน — วงหมุนคลุมมุมอยู่ ถ้าตรวจวงก่อนจะจับ handle ไม่ได้เลย
    //   (มุมมาก่อนกลางด้านด้วย เพราะที่มุมของกรอบแคบ ๆ สองอันซ้อนกันได้)
    if let Some(dir) = handles_for(tool)
        .iter()
        .find(|dir| near(dir.point_on(frame), handle_reach))
    {
        return Some(Handle::Edge(*dir));
    }
    // ★ วงหมุนอยู่ **นอก** กรอบเท่านั้น — ไม่งั้นกดมุมด้านในภาพจะกลายเป็นหมุน
    //   ทั้งที่ผู้ใช้ตั้งใจจะย้าย · ครอปไม่มีการหมุน (เครื่องมือคนละตัว)
    if tool == Tool::Select
        && !frame.contains_point(world)
        && HandleDir::CORNERS
            .iter()
            .any(|dir| near(dir.point_on(frame), rotate_reach))
    {
        return Some(Handle::Rotate);
    }
    None
}

/// สิ่งที่การกดครั้งนี้ "จับ" ไว้ — ตัดสินตอนกดครั้งเดียว ห้ามเปลี่ยนกลางคัน
///
/// ★ ตัดสินตอนกดเพราะระหว่างลาก ภาพเคลื่อนตามเคอร์เซอร์อยู่แล้ว ถ้าประเมินใหม่
/// ทุกเฟรมเคอร์เซอร์จะหลุดเข้า/ออก handle ของตัวเองแล้วสลับโหมดไปมา
#[derive(Debug, Clone, Copy)]
enum Grab {
    /// กดที่ว่าง — จะกลายเป็นกรอบเลือก
    Band,
    /// กดบนภาพ — จะกลายเป็นการย้าย
    Move,
    /// ★ กดบนภาพ **ตอนอยู่ในเครื่องมือครอป** — เลือกได้ แต่ลากแล้วไม่มีอะไรเกิดขึ้น
    ///
    /// การย้ายเป็นหน้าที่ของเครื่องมือเลือก (docs/03 §2) ถ้าเครื่องมือครอปย้ายภาพได้ด้วย
    /// ผู้ใช้ที่ตั้งใจลากขอบแล้วพลาดไปโดนกลางภาพจะย้ายภาพโดยไม่รู้ตัว
    /// — ยังต้องเลือกภาพได้อยู่ ไม่งั้นสลับไปครอปภาพอื่นไม่ได้เลย
    Inert,
    /// จับ handle มุม — สเกล (กรอบ ณ ตอนเริ่มกด)
    Scale { dir: HandleDir, frame: Obb },
    /// จับ handle — ครอป (กรอบ + สภาพ item ณ ตอนเริ่มกด)
    Crop { dir: HandleDir, frame: Obb },
    /// จับนอก handle มุม — หมุน (กรอบ + มุมของเคอร์เซอร์ ณ ตอนเริ่มกด)
    Rotate { frame: Obb, start_angle: f32 },
}

/// สถานะของการกดค้างหนึ่งครั้ง
#[derive(Debug, Clone)]
struct Press {
    origin: Vec2,
    modifiers: Modifiers,
    /// ขยับเกินระยะจนถือว่าเป็นการลากแล้วหรือยัง
    dragging: bool,
    /// สิ่งที่การกดครั้งนี้จับไว้
    grab: Grab,
    /// สิ่งที่เลือกอยู่ก่อนเริ่มกด — rubber-band แบบเพิ่มต้องบวกจากชุดนี้
    base: Vec<ItemId>,
    /// ★ สภาพของ item ที่กำลังจะถูกแปลง **ณ ตอนเริ่มกด**
    ///
    /// คำนวณเป้าหมายจาก "ตอนเริ่ม + ระยะรวม" ไม่ใช่บวกทีละเฟรม — ถ้าบวกสะสม
    /// ความคลาดเคลื่อนของ f32 จะพอกขึ้นเรื่อย ๆ ระหว่างลากยาว ๆ แล้วภาพจะไม่ตรง
    /// กับเคอร์เซอร์ · และมันทำให้ `TransformItems` ที่ merge กันแล้วยังย้อนกลับ
    /// ไปจุดเริ่มลากได้เป๊ะ
    ///
    /// การหมุนยิ่งสำคัญ: มุมสะสมทีละเฟรมจะดริฟต์จนภาพเอียงไม่ตรงที่ปล่อย
    moving: Vec<(ItemId, ItemCanvas)>,
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
            CanvasEvent::Move { world, modifiers } => {
                self.on_move(ctx, selection, world, modifiers)
            }
            CanvasEvent::DoubleClick { world } => self.on_double_click(ctx, selection, world),
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
        let base: Vec<ItemId> = selection.iter().collect();
        let mut out = Interaction::default();

        // ★★ hit-test ของ handle มาก่อน hit-test ของภาพเสมอ (HANDOFF §2.4)
        //    handle มุมคร่อมตัวภาพอยู่ครึ่งหนึ่ง ถ้าถามภาพก่อนจะไม่มีวันจับ handle ติด
        //    การกด handle ยัง **ไม่แตะการเลือก** ด้วย — มันคือการแก้ของที่เลือกไว้แล้ว
        let grab = match grab_handle(ctx, selection, world) {
            Some(grab) => grab,
            None => match ctx.index.hit_test(ctx.board, world) {
                Some(id) if modifiers.is_additive() => {
                    // Ctrl+คลิก = สลับสถานะทีละตัว
                    let mut next = base.clone();
                    let anchor = if let Some(at) = next.iter().position(|other| *other == id) {
                        next.remove(at);
                        next.last().copied()
                    } else {
                        next.push(id);
                        Some(id)
                    };
                    apply_selection(&mut out, selection, next, anchor);
                    on_image(ctx.tool)
                }
                Some(id) => {
                    // คลิกบนภาพที่เลือกอยู่แล้ว = ไม่เปลี่ยนอะไร (จะได้ลากทั้งชุดต่อได้)
                    if !selection.contains(id) {
                        apply_selection(&mut out, selection, vec![id], Some(id));
                    }
                    on_image(ctx.tool)
                }
                // กดที่ว่าง — ยังไม่ล้างทันที รอดูว่าจะกลายเป็นการลากกรอบไหม
                // (ล้างตอนปล่อยแทน ดู `on_release`)
                None => Grab::Band,
            },
        };

        // เก็บสภาพตอนเริ่มไว้ **หลัง**การเลือกถูกตัดสินแล้ว จึงได้ชุดที่ถูกต้อง
        let moving = if matches!(grab, Grab::Band) {
            Vec::new()
        } else {
            selection
                .iter()
                .filter_map(|id| ctx.board.item(id).map(|item| (id, item.canvas)))
                // ★ ภาพที่ล็อกไว้ต้องไม่ขยับ — นั่นคือความหมายทั้งหมดของการล็อก
                .filter(|(_, canvas)| !canvas.locked)
                .collect()
        };

        self.press = Some(Press {
            origin: world,
            modifiers,
            dragging: false,
            grab,
            base,
            moving,
        });
        out
    }

    fn on_move(
        &mut self,
        ctx: CanvasContext<'_>,
        selection: &mut Selection,
        world: Vec2,
        modifiers: Modifiers,
    ) -> Interaction {
        let Some(press) = self.press.as_mut() else {
            return Interaction::default();
        };

        // ★ ทุกอย่างที่ไม่ใช่ `Band` คือการแก้ `Board` → ต้องผ่าน `Command`
        let grab = press.grab;
        if !matches!(grab, Grab::Band) {
            if !started_dragging(press, ctx.drag_threshold, world) {
                return Interaction::default();
            }
            let changes = match grab {
                Grab::Move => move_changes(&press.moving, world - press.origin),
                Grab::Scale { dir, frame } => {
                    scale_changes(&press.moving, frame, dir, world, modifiers)
                }
                Grab::Crop { dir, frame } => {
                    // ★ ครอปเป็น `SetCrop` ไม่ใช่ `TransformItems` — ชื่อในเมนู undo
                    //   ต้องบอกว่าผู้ใช้ทำอะไร และมันต้องไม่ merge ข้ามชนิดกับการย้าย
                    let changes = crop_changes(&press.moving, frame, dir, world);
                    if changes.is_empty() {
                        return Interaction::default();
                    }
                    let mut out = Interaction {
                        needs_redraw: true,
                        ..Interaction::default()
                    };
                    if let Ok(command) = SetCrop::new(changes) {
                        out.commands.push(Box::new(command));
                    }
                    return out;
                }
                Grab::Rotate { frame, start_angle } => rotate_changes(
                    &press.moving,
                    frame,
                    start_angle,
                    world,
                    modifiers,
                    ctx.drag_threshold,
                ),
                Grab::Band | Grab::Inert => Vec::new(),
            };
            return transform(changes);
        }

        if !started_dragging(press, ctx.drag_threshold, world) {
            return Interaction::default(); // ยังนับเป็นคลิก ไม่ใช่ลาก
        }

        let rect = Rect::from_corners(press.origin, world);
        let inside = ctx.index.hit_test_rect(ctx.board, rect);
        let (items, anchor) = combine(&press.base, &inside, press.modifiers);

        let mut out = Interaction {
            rubber_band: Some(rect),
            needs_redraw: true,
            ..Interaction::default()
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

        // จบการย้าย/สเกล/หมุน — **seal เพื่อให้การลากครั้งถัดไปเป็น undo ขั้นใหม่**
        if !matches!(press.grab, Grab::Band) {
            out.seal = press.dragging;
            return out;
        }

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
        if !press.modifiers.is_additive() {
            apply_selection(&mut out, selection, Vec::new(), None);
        }
        out
    }
}

impl SelectTool {
    /// ดับเบิลคลิกในเครื่องมือครอป = **รีเซ็ตกรอบ crop กลับเป็นภาพเต็ม** (docs/03 §2)
    ///
    /// ★ คืนเรขาคณิตให้ตรงกับภาพเต็มด้วย ไม่ใช่แค่ตั้ง `crop` กลับเป็น 0..1 —
    /// ถ้าคืนแต่ `crop` ภาพเต็มจะถูกบีบให้อยู่ในกรอบเล็กที่เหลือจากการครอป
    /// ซึ่งผู้ใช้เห็นเป็น "ภาพหดลง" ไม่ใช่ "ขอบที่ตัดไปกลับมา"
    ///
    /// ★★ **ส่วนที่ยังเห็นอยู่ต้องไม่ขยับเลย** ขอบที่ถูกตัดไปงอกกลับออกมา
    /// **เฉพาะด้านที่มันถูกตัด** — ตัดขอบขวาไป ขอบขวาก็กลับมาทางขวา
    /// (ถ้าคืนโดยยึดจุดกึ่งกลาง ภาพทั้งใบจะเลื่อนไปครึ่งหนึ่งของส่วนที่งอก
    /// ซึ่งผู้ใช้เห็นเป็น "ภาพกระโดด" ทั้งที่เขาแค่กดเลิกครอป — พบตอนดูภาพหน้าจอจริง)
    fn on_double_click(
        &mut self,
        ctx: CanvasContext<'_>,
        selection: &Selection,
        world: Vec2,
    ) -> Interaction {
        // การกดค้างที่ค้างอยู่ใช้ไม่ได้แล้ว — ดับเบิลคลิกจบด้วยการปล่อยเสมอ
        self.press = None;
        if ctx.tool != Tool::Crop {
            return Interaction::default();
        }
        let Some(id) = sole_movable(ctx.board, selection) else {
            return Interaction::default();
        };
        let Some(item) = ctx.board.item(id) else {
            return Interaction::default();
        };
        let start = item.canvas;
        // ต้องดับเบิลคลิก **บนภาพนั้นจริง ๆ** ไม่ใช่ที่ว่างข้าง ๆ
        if !start.obb().contains_point(world) {
            return Interaction::default();
        }
        let crop = start.crop.sanitized();
        let window = crop.max - crop.min;
        if window.x <= f32::EPSILON || window.y <= f32::EPSILON {
            return Interaction::default();
        }
        let full = Vec2::new(start.size.x / window.x, start.size.y / window.y);
        // มุมซ้ายบนของภาพเต็ม ในพิกัดท้องถิ่นเทียบกึ่งกลางปัจจุบัน:
        // ส่วนที่เห็นเริ่มที่ `-size/2` และมันคือช่วง `crop.min` ของภาพเต็ม
        let [ax, ay] = start.obb().axes();
        let centre_local = -start.size * 0.5 - crop.min * full + full * 0.5;
        let restored = ItemCanvas {
            pos: start.pos + ax * centre_local.x + ay * centre_local.y,
            size: full,
            crop: CropRect::default(),
            ..start
        };
        // ไม่มีอะไรให้รีเซ็ต = ไม่สร้างคำสั่งและไม่ขอเฟรม (I-1)
        if restored == start {
            return Interaction::default();
        }
        let mut out = Interaction {
            needs_redraw: true,
            // ★ รีเซ็ตเป็นขั้นเดี่ยวเสมอ ห้ามให้การลากครั้งถัดไปกลืนมันเข้าไป
            seal: true,
            ..Interaction::default()
        };
        if let Ok(command) = SetCrop::new(vec![(id, restored)]) {
            out.commands.push(Box::new(command));
        }
        out
    }
}

/// การลากที่เริ่มจากบนตัวภาพหมายถึงอะไร — ขึ้นกับเครื่องมือที่เลือกอยู่
fn on_image(tool: Tool) -> Grab {
    match tool {
        Tool::Select => Grab::Move,
        Tool::Crop => Grab::Inert,
    }
}

/// item ตัวเดียวที่แก้ได้ในชุดที่เลือก — `None` ถ้าไม่มีหรือมีมากกว่าหนึ่ง
fn sole_movable(board: &Board, selection: &Selection) -> Option<ItemId> {
    let mut only = None;
    for id in selection.iter() {
        let Some(item) = board.item(id) else { continue };
        if item.canvas.locked {
            continue;
        }
        if only.replace(id).is_some() {
            return None; // มากกว่าหนึ่ง
        }
    }
    only
}

/// handle ที่จุดนี้จับอยู่ พร้อมข้อมูลที่การลากต้องใช้ตลอดทาง
fn grab_handle(ctx: CanvasContext<'_>, selection: &Selection, world: Vec2) -> Option<Grab> {
    let frame = selection_frame(ctx.board, selection)?;
    match handle_at(ctx.tool, frame, world, ctx.handle_reach, ctx.rotate_reach)? {
        // ★ ครอปทำได้ **ทีละใบ** — กรอบรวมของหลายใบเป็นแค่กล่องแนวแกน ไม่ได้ผูกกับ
        //   pixel ของภาพไหนเลย การลากขอบมันจึงไม่มีความหมายในหน่วยของภาพต้นฉบับ
        Handle::Edge(dir) if ctx.tool == Tool::Crop => {
            sole_movable(ctx.board, selection)?;
            Some(Grab::Crop { dir, frame })
        }
        Handle::Edge(dir) => Some(Grab::Scale { dir, frame }),
        Handle::Rotate => {
            let offset = world - frame.center;
            // ★ ใกล้จุดหมุนเกินไป มุมจาก atan2 จะกระโดดจากการขยับหนึ่งพิกเซล
            //   ปล่อยผ่านไปจะได้ภาพที่หมุนสุ่มทันทีที่แตะ — ไม่รับดีกว่า
            if !offset.is_finite() || offset.length() <= sane_reach(ctx.drag_threshold) {
                return None;
            }
            Some(Grab::Rotate {
                frame,
                start_angle: offset.y.atan2(offset.x),
            })
        }
    }
}

/// ห่อรายการที่เปลี่ยนให้เป็นคำสั่งหนึ่งตัว — รายการว่าง = ไม่มีอะไรเกิดขึ้น
///
/// I-1: ไม่มีอะไรเปลี่ยนต้องไม่ขอเฟรมใหม่
fn transform(changes: Vec<(ItemId, ItemCanvas)>) -> Interaction {
    if changes.is_empty() {
        return Interaction::default();
    }
    let mut out = Interaction {
        needs_redraw: true,
        ..Interaction::default()
    };
    if let Ok(command) = TransformItems::new(changes) {
        out.commands.push(Box::new(command));
    }
    out
}

/// ย้ายทั้งชุดอย่างแข็ง — ระยะห่างระหว่างภาพในชุดต้องไม่เปลี่ยน
fn move_changes(moving: &[(ItemId, ItemCanvas)], delta: Vec2) -> Vec<(ItemId, ItemCanvas)> {
    if !delta.is_finite() {
        return Vec::new();
    }
    moving
        .iter()
        .map(|(id, start)| {
            (
                *id,
                ItemCanvas {
                    pos: start.pos + delta,
                    ..*start
                },
            )
        })
        .collect()
}

/// สเกลจากมุมตรงข้าม (หรือจากกึ่งกลางเมื่อกด `Alt`) — docs/03 §2
///
/// ★ **หลายใบพร้อมกันบังคับเป็นสัดส่วนเดิมเสมอ** ไม่ว่าจะกด `Shift` หรือไม่:
/// กรอบรวมเป็นแนวแกน แต่ภาพข้างในหมุนได้ การยืดแกนเดียวของกรอบแนวแกนจึงต้องการ
/// **การเฉือน (shear)** ซึ่ง `ItemCanvas` ไม่มีที่เก็บ (มีแค่ pos/size/rotation)
/// ถ้าฝืนทำจะได้ภาพที่ผิดรูปจากที่ผู้ใช้เห็นตอนลาก
fn scale_changes(
    moving: &[(ItemId, ItemCanvas)],
    frame: Obb,
    dir: HandleDir,
    world: Vec2,
    modifiers: Modifiers,
) -> Vec<(ItemId, ItemCanvas)> {
    let grabbed = dir.point_on(frame);
    let anchor = if modifiers.alt {
        frame.center
    } else {
        // มุมตรงข้าม = กลับทิศทั้งสองแกน
        HandleDir {
            x: -dir.x,
            y: -dir.y,
        }
        .point_on(frame)
    };
    let [ax, ay] = frame.axes();
    let to_local = |point: Vec2| {
        let offset = point - anchor;
        Vec2::new(offset.dot(ax), offset.dot(ay))
    };
    let from_local = |local: Vec2| anchor + ax * local.x + ay * local.y;

    let base = to_local(grabbed);
    let now = to_local(world);
    if !base.is_finite() || !now.is_finite() {
        return Vec::new();
    }

    let uniform = modifiers.shift || moving.len() > 1;
    let (sx, sy) = if uniform {
        // ฉายลงบนทิศของมุมเดิม — ได้ค่าที่ลื่นและไม่กระโดดเวลาลากเฉียง
        // (การหยิบ max ของสองแกนจะสะบัดทุกครั้งที่แกนไหนแกนหนึ่งชนะสลับกัน)
        let denominator = base.length_squared();
        let scale = if denominator > 0.0 {
            (now.dot(base) / denominator).max(MIN_SCALE)
        } else {
            1.0
        };
        (scale, scale)
    } else {
        (ratio(now.x, base.x), ratio(now.y, base.y))
    };
    if !sx.is_finite() || !sy.is_finite() {
        return Vec::new();
    }

    moving
        .iter()
        .map(|(id, start)| {
            let local = to_local(start.pos);
            (
                *id,
                ItemCanvas {
                    pos: from_local(Vec2::new(local.x * sx, local.y * sy)),
                    size: Vec2::new(start.size.x * sx, start.size.y * sy),
                    ..*start
                },
            )
        })
        .collect()
}

/// ครอปแบบไม่ทำลายต้นฉบับ — ลาก handle แล้ว **ขอบที่ handle นั้นคุมขยับเข้า/ออก**
///
/// ★ หัวใจของ "ไม่ทำลายต้นฉบับ": ไม่แตะไฟล์และไม่แตะ pixel เลย เก็บเป็น
/// `CropRect` สัดส่วน 0..1 ของภาพต้นฉบับ (docs/02 §2.1) ซึ่งยังถูกต้องแม้ผู้ใช้
/// relink ไปหาไฟล์ที่ความละเอียดต่างออกไป
///
/// ★ `crop` กับ `pos`/`size` **ต้องเปลี่ยนพร้อมกันเสมอ** — ถ้าแก้แต่ `crop`
/// ส่วนที่เหลือจะถูกยืดให้เต็มขนาดเดิม ผู้ใช้จะเห็นภาพ*ซูมเข้า*แทนที่จะเห็นภาพ*ถูกตัดขอบ*
/// สิ่งที่ถูกคือ pixel ที่ยังเหลืออยู่ต้องอยู่ที่เดิมเป๊ะบนจอ หายไปแค่ส่วนที่ถูกตัด
///
/// ★ ลากกลับออกได้จนสุดขอบภาพต้นฉบับ (ไม่ใช่แค่เข้าอย่างเดียว) — คนที่ตัดเกินไป
/// นิดเดียวต้องดึงกลับได้ทันทีโดยไม่ต้องรีเซ็ตทั้งหมดแล้วเริ่มใหม่
fn crop_changes(
    moving: &[(ItemId, ItemCanvas)],
    frame: Obb,
    dir: HandleDir,
    world: Vec2,
) -> Vec<(ItemId, ItemCanvas)> {
    let [(id, start)] = moving else {
        return Vec::new(); // ครอปทีละใบเท่านั้น
    };
    let (id, start) = (*id, *start);
    let [ax, ay] = frame.axes();
    let offset = world - frame.center;
    if !offset.is_finite() {
        return Vec::new();
    }
    let pointer = Vec2::new(offset.dot(ax), offset.dot(ay));
    let half = frame.half_size.abs();
    let crop = start.crop.sanitized();

    // ขอบปัจจุบันในพิกัดท้องถิ่น (เทียบจุดกึ่งกลางของกรอบตอนเริ่มกด)
    let mut low = -half;
    let mut high = half;
    // ★ ขอบของ **ภาพเต็ม** ในพิกัดเดียวกัน — เพดานของการลากกลับออก
    //   `span` = ระยะที่ภาพเต็มกินบนจอ ณ สเกลปัจจุบัน
    let axis_limits = |visible: f32, from: f32, to: f32| -> (f32, f32) {
        let window = to - from;
        if window <= f32::EPSILON {
            return (-visible, visible);
        }
        let span = (visible * 2.0) / window;
        (-visible - from * span, -visible + (1.0 - from) * span)
    };
    let (min_x, max_x) = axis_limits(half.x, crop.min.x, crop.max.x);
    let (min_y, max_y) = axis_limits(half.y, crop.min.y, crop.max.y);

    // ขอบที่เล็กที่สุดที่ยอมให้เหลือ — กันภาพยุบจนหายและกัน crop ที่กว้างเป็นศูนย์
    let gap = MIN_CROP_SPAN;
    match dir.x {
        -1 => low.x = pointer.x.clamp(min_x, high.x - gap),
        1 => high.x = pointer.x.clamp(low.x + gap, max_x),
        _ => {}
    }
    match dir.y {
        -1 => low.y = pointer.y.clamp(min_y, high.y - gap),
        1 => high.y = pointer.y.clamp(low.y + gap, max_y),
        _ => {}
    }
    if !low.is_finite() || !high.is_finite() {
        return Vec::new();
    }

    // ขอบใหม่ (local) → กรอบ crop ใหม่ (สัดส่วนของภาพต้นฉบับ)
    let to_fraction = |edge: f32, visible: f32, from: f32, to: f32| -> f32 {
        let window = to - from;
        if window <= f32::EPSILON || visible <= 0.0 {
            return from;
        }
        let span = (visible * 2.0) / window;
        from + (edge + visible) / span
    };
    let new_crop = CropRect {
        min: Vec2::new(
            to_fraction(low.x, half.x, crop.min.x, crop.max.x),
            to_fraction(low.y, half.y, crop.min.y, crop.max.y),
        ),
        max: Vec2::new(
            to_fraction(high.x, half.x, crop.min.x, crop.max.x),
            to_fraction(high.y, half.y, crop.min.y, crop.max.y),
        ),
    };

    let size = high - low;
    let centre_local = (low + high) * 0.5;
    vec![(
        id,
        ItemCanvas {
            // ★ กึ่งกลางขยับตามขอบที่หด — pixel ที่เหลือจึงไม่ขยับบนจอเลย
            pos: frame.center + ax * centre_local.x + ay * centre_local.y,
            size,
            crop: new_crop,
            ..start
        },
    )]
}

/// กรอบ crop ที่แคบที่สุดที่ยอมให้ลากเหลือ (หน่วย world)
///
/// ต้องมากกว่า `MIN_ITEM_SIZE` พอสมควร ไม่งั้น `sanitized()` จะ clamp ขนาดขึ้น
/// แล้วภาพกับกรอบ crop จะไม่ตรงกันเงียบ ๆ
const MIN_CROP_SPAN: f32 = 1.0;

/// ตัวคูณของแกนหนึ่ง — มุมที่ทับกับจุดยึดพอดีจะไม่มีทิศให้ยืด จึงคงค่าไว้
fn ratio(now: f32, base: f32) -> f32 {
    if !now.is_finite() || base.abs() <= f32::EPSILON {
        return 1.0;
    }
    (now / base).max(MIN_SCALE)
}

/// หมุนรอบจุดกึ่งกลางของกรอบ — `Shift` = สแนป 15° (docs/03 §2)
///
/// ทั้ง**ตำแหน่ง**และ**มุม**ของทุกใบต้องหมุนตาม ไม่งั้นการหมุนหลายใบจะกลายเป็น
/// "ภาพแต่ละใบหมุนอยู่กับที่" ซึ่งไม่ใช่สิ่งที่ผู้ใช้เห็นตอนลาก
fn rotate_changes(
    moving: &[(ItemId, ItemCanvas)],
    frame: Obb,
    start_angle: f32,
    world: Vec2,
    modifiers: Modifiers,
    dead_zone: f32,
) -> Vec<(ItemId, ItemCanvas)> {
    let offset = world - frame.center;
    if !offset.is_finite() || offset.length() <= sane_reach(dead_zone) {
        return Vec::new();
    }
    let mut delta = offset.y.atan2(offset.x) - start_angle;
    if modifiers.shift {
        // สแนป **มุมสุดท้าย** ไม่ใช่ระยะที่หมุน — ภาพที่เอียง 3° อยู่แล้วต้องลงที่
        // 0°/15°/30° ไม่ใช่ 3°/18°/33° (กรอบรวมหลายใบมี rotation = 0 สูตรเดียวใช้ได้ทั้งคู่)
        let total = frame.rotation + delta;
        delta = (total / ROTATE_SNAP).round() * ROTATE_SNAP - frame.rotation;
    }
    if !delta.is_finite() {
        return Vec::new();
    }
    let (sin, cos) = delta.sin_cos();
    moving
        .iter()
        .map(|(id, start)| {
            let arm = start.pos - frame.center;
            (
                *id,
                ItemCanvas {
                    pos: frame.center
                        + Vec2::new(arm.x * cos - arm.y * sin, arm.x * sin + arm.y * cos),
                    rotation: start.rotation + delta,
                    ..*start
                },
            )
        })
        .collect()
}

/// ขยับเกินระยะจนนับเป็น "การลาก" แล้วหรือยัง — จำสถานะไว้ใน `press`
///
/// ระยะนี้กันมือสั่น: คลิกแล้วเมาส์ขยับสองพิกเซลต้องยังเป็นคลิก ไม่ใช่การลาก
/// ที่ย้ายภาพของผู้ใช้ไปโดยไม่ตั้งใจ
fn started_dragging(press: &mut Press, threshold: f32, world: Vec2) -> bool {
    if press.dragging {
        return true;
    }
    let threshold = if threshold.is_finite() {
        threshold.max(0.0)
    } else {
        0.0
    };
    if (world - press.origin).length() < threshold {
        return false;
    }
    press.dragging = true;
    true
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
    // เทียบ float ตรง ๆ ได้: ค่าที่ assert คือผลของการบวกเวกเตอร์ครั้งเดียว
    // จากค่าคงที่ ไม่ใช่ผลสะสมจากการคำนวณทศนิยมหลายรอบ
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp
    )]

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
        handle_reach: f32,
        rotate_reach: f32,
        active_tool: Tool,
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
                    handle_reach: HANDLE_REACH,
                    rotate_reach: ROTATE_REACH,
                    active_tool: Tool::Select,
                },
                ids,
            )
        }

        fn feed(&mut self, event: CanvasEvent) {
            let ctx = CanvasContext {
                board: &self.board,
                index: &self.index,
                drag_threshold: 4.0,
                handle_reach: self.handle_reach,
                rotate_reach: self.rotate_reach,
                tool: self.active_tool,
            };
            let outcome = self.tool.handle(ctx, &mut self.selection, event);
            self.last = outcome.rubber_band;
            let moved: Vec<ItemId> = self.selection.iter().collect();
            for command in outcome.commands {
                self.history.apply(&mut self.board, command).unwrap();
            }
            // ★ index ต้องตามตำแหน่งใหม่ทันที ไม่งั้นการกดครั้งถัดไปจะ hit-test
            //   กับตำแหน่ง *เก่า* แล้วคลิกไม่โดนภาพที่เพิ่งย้ายไป
            //   (เทสต์ `sealing_on_release_keeps_two_drags_separate` จับข้อนี้ได้
            //    ตอนเขียนครั้งแรก — ชั้น UI ต้องทำเหมือนกันเป๊ะ)
            for id in moved {
                if let Some(item) = self.board.item(id) {
                    self.index.insert(id, &item.canvas);
                }
            }
            if outcome.seal {
                self.history.seal();
            }
        }

        fn canvas_of(&self, id: ItemId) -> ItemCanvas {
            self.board.item(id).unwrap().canvas
        }

        fn press(&mut self, at: Vec2, modifiers: Modifiers) {
            self.feed(CanvasEvent::Press {
                button: CanvasButton::Primary,
                world: at,
                modifiers,
            });
        }

        fn drag_to(&mut self, at: Vec2) {
            self.drag_to_with(at, Modifiers::default());
        }

        fn drag_to_with(&mut self, at: Vec2, modifiers: Modifiers) {
            self.feed(CanvasEvent::Move {
                world: at,
                modifiers,
            });
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

    /// ระยะของ handle ที่ใช้ในเทสต์ — เล็กกว่าครึ่งภาพ 100×100 มาก
    /// จึงไม่มีทางกลืนการคลิกกลางภาพไปโดยบังเอิญ
    const HANDLE_REACH: f32 = 6.0;
    const ROTATE_REACH: f32 = 20.0;

    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        shift: false,
        alt: false,
    };

    const SHIFT: Modifiers = Modifiers {
        ctrl: false,
        shift: true,
        alt: false,
    };

    const ALT: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: true,
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

    // ---------- ย้ายภาพ (P2-5) ----------

    /// ★ เกณฑ์ของ ROADMAP: **ลากค้าง = 1 undo · ปล่อยแล้ว seal**
    ///
    /// ลาก 200 เฟรมแล้ว undo ครั้งเดียวต้องกลับไปจุดเริ่มลากเป๊ะ — ไม่ใช่ถอย
    /// ทีละเฟรม (ซึ่งจะกิน undo stack ทั้งก้อนแล้วผู้ใช้ย้อนงานจริงไม่ถึง)
    #[test]
    fn a_long_drag_is_one_undo_that_lands_exactly_where_it_started() {
        let (mut h, ids) = Harness::new(3);
        h.click(Vec2::ZERO);
        let start = h.canvas_of(ids[0]);

        h.press(Vec2::ZERO, Modifiers::default());
        for step in 1..=200 {
            h.drag_to(Vec2::new(step as f32 * 2.0, step as f32));
        }
        h.release(Vec2::new(400.0, 200.0));

        assert_eq!(h.canvas_of(ids[0]).pos, Vec2::new(400.0, 200.0));
        assert_eq!(h.history.undo_depth(), 1, "ลากค้างต้องเป็นขั้นเดียว");

        h.history.undo(&mut h.board).unwrap();
        assert_eq!(
            h.canvas_of(ids[0]),
            start,
            "ย้อนครั้งเดียวต้องกลับไปจุดเริ่มลาก ไม่ใช่เฟรมก่อนหน้า"
        );
    }

    /// ปล่อยแล้วลากใหม่ = **สองขั้น** — ถ้าไม่ seal ผู้ใช้จะย้อนทีเดียวแล้วภาพ
    /// กระโดดข้ามไปสองที่ ซึ่งงงมาก
    #[test]
    fn sealing_on_release_keeps_two_drags_separate() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);

        h.press(Vec2::ZERO, Modifiers::default());
        h.drag_to(Vec2::new(100.0, 0.0));
        h.release(Vec2::new(100.0, 0.0));

        h.press(Vec2::new(100.0, 0.0), Modifiers::default());
        h.drag_to(Vec2::new(100.0, 80.0));
        h.release(Vec2::new(100.0, 80.0));

        assert_eq!(h.history.undo_depth(), 2);
        h.history.undo(&mut h.board).unwrap();
        assert_eq!(h.canvas_of(ids[0]).pos, Vec2::new(100.0, 0.0));
    }

    /// ★ ลากภาพที่เลือกไว้หลายใบ = ย้ายทั้งชุดพร้อมกัน โดยระยะห่างระหว่างกันคงเดิม
    #[test]
    fn dragging_one_of_many_moves_the_whole_selection_rigidly() {
        let (mut h, ids) = Harness::new(3);
        h.press(Vec2::ZERO, CTRL);
        h.release(Vec2::ZERO);
        h.press(Vec2::new(200.0, 0.0), CTRL);
        h.release(Vec2::new(200.0, 0.0));
        let gap = h.canvas_of(ids[1]).pos - h.canvas_of(ids[0]).pos;

        h.press(Vec2::ZERO, Modifiers::default());
        h.drag_to(Vec2::new(50.0, 90.0));
        h.release(Vec2::new(50.0, 90.0));

        assert_eq!(h.canvas_of(ids[0]).pos, Vec2::new(50.0, 90.0));
        assert_eq!(
            h.canvas_of(ids[1]).pos - h.canvas_of(ids[0]).pos,
            gap,
            "ระยะห่างระหว่างภาพในชุดต้องไม่เปลี่ยน"
        );
        assert_eq!(h.canvas_of(ids[2]).pos.x, 400.0, "ตัวที่ไม่ได้เลือกต้องอยู่นิ่ง");
        assert_eq!(h.history.undo_depth(), 1);
    }

    /// ★ ภาพที่ล็อกไว้ต้องไม่ขยับ — นั่นคือความหมายทั้งหมดของการล็อก
    #[test]
    fn a_locked_item_never_moves() {
        let (mut h, ids) = Harness::new(2);
        let mut canvas = h.canvas_of(ids[0]);
        canvas.locked = true;
        h.board.set_canvas(ids[0], canvas).unwrap();
        h.board.mark_dirty(false);

        h.click(Vec2::ZERO);
        h.press(Vec2::ZERO, Modifiers::default());
        h.drag_to(Vec2::new(300.0, 300.0));
        h.release(Vec2::new(300.0, 300.0));

        assert_eq!(h.canvas_of(ids[0]).pos, Vec2::ZERO, "ล็อกแล้วต้องไม่ขยับ");
        assert_eq!(h.history.undo_depth(), 0, "ไม่มีอะไรเปลี่ยน = ไม่ควรมีขั้น undo");
    }

    /// ตำแหน่งคำนวณจาก **จุดเริ่ม + ระยะรวม** ไม่ใช่บวกทีละเฟรม
    ///
    /// ถ้าบวกสะสม ความคลาดเคลื่อนจะพอกขึ้นระหว่างลากยาว ๆ แล้วภาพจะไม่ตรงเคอร์เซอร์
    #[test]
    fn the_target_is_absolute_so_a_wobbly_drag_still_lands_exactly() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);

        h.press(Vec2::ZERO, Modifiers::default());
        // ลากส่าย ๆ ไปมาแล้วจบที่จุดเดิมพอดี
        for step in 0..50 {
            let wobble = (step as f32 * 0.7).sin() * 40.0;
            h.drag_to(Vec2::new(120.0 + wobble, wobble));
        }
        h.drag_to(Vec2::new(120.0, 0.0));
        h.release(Vec2::new(120.0, 0.0));

        assert_eq!(h.canvas_of(ids[0]).pos, Vec2::new(120.0, 0.0));
    }

    /// มือสั่นตอนคลิกบนภาพต้องไม่กลายเป็นการย้าย
    #[test]
    fn a_wobble_on_an_item_does_not_move_it() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);

        h.press(Vec2::ZERO, Modifiers::default());
        h.drag_to(Vec2::new(1.5, 1.0)); // ต่ำกว่าระยะ 4.0
        h.release(Vec2::new(1.5, 1.0));

        assert_eq!(h.canvas_of(ids[0]).pos, Vec2::ZERO);
        assert_eq!(h.history.undo_depth(), 0);
        assert!(!h.board.is_dirty());
    }

    /// ย้ายแล้ว hit-test ต้องตามไปที่ตำแหน่งใหม่ (index ต้องถูก rebuild โดยผู้เรียก)
    #[test]
    fn hit_testing_follows_the_item_after_a_move() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        h.press(Vec2::ZERO, Modifiers::default());
        h.drag_to(Vec2::new(600.0, 0.0));
        h.release(Vec2::new(600.0, 0.0));

        h.index.rebuild(&h.board);
        assert_eq!(
            h.index.hit_test(&h.board, Vec2::new(600.0, 0.0)),
            Some(ids[0])
        );
        assert_eq!(h.index.hit_test(&h.board, Vec2::ZERO), None);
    }

    // ---------- handle: scale · rotate (P2-5) ----------

    /// สี่มุมของกรอบที่ handle เกาะอยู่ (ลำดับเดียวกับ `Obb::corners`)
    fn frame_corners(h: &Harness) -> [Vec2; 4] {
        selection_frame(&h.board, &h.selection)
            .expect("ต้องมีกรอบให้จับ")
            .corners()
    }

    /// จุดที่ใช้ "ลาก**นอก** handle มุม" — เลยมุมออกไปตามแนวทแยงของกรอบ
    ///
    /// คำนวณจากกรอบจริงเสมอ เพราะกรอบเอียงตามภาพที่หมุนแล้ว การเขียนพิกัดตายตัว
    /// จะหลุดออกนอกวงหมุนทันทีที่ภาพเอียงไปนิดเดียว
    fn outside_corner(h: &Harness, corner: usize) -> Vec2 {
        let frame = selection_frame(&h.board, &h.selection).expect("ต้องมีกรอบให้จับ");
        let point = frame.corners()[corner];
        point + (point - frame.center).normalize_or_zero() * 12.0
    }

    /// ★★ กฎข้อแรกของ handle: **hit-test ของ handle มาก่อน hit-test ของภาพ**
    ///
    /// handle มุมคร่อมตัวภาพอยู่ครึ่งหนึ่ง — ถ้าถามภาพก่อน การลาก handle จะกลายเป็น
    /// การย้ายภาพทุกครั้ง แล้วจะไม่มีทางสเกลอะไรได้เลย (HANDOFF §2.4)
    #[test]
    fn a_corner_handle_wins_over_the_image_underneath_it() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let start = h.canvas_of(ids[0]);
        // จุดนี้อยู่ **ในภาพ** (ภาพกิน -50..50) และเป็นมุมของ handle พอดี
        let corner = frame_corners(&h)[2];
        assert!(start.obb().contains_point(corner - Vec2::splat(0.5)));

        h.press(corner, Modifiers::default());
        h.drag_to(corner + Vec2::splat(50.0));
        h.release(corner + Vec2::splat(50.0));

        assert_ne!(
            h.canvas_of(ids[0]).size,
            start.size,
            "ลาก handle แล้วขนาดต้องเปลี่ยน — ถ้าเท่าเดิมแปลว่ากลายเป็นการย้าย"
        );
    }

    /// ลากมุมล่างขวา = ยืดจากมุมบนซ้าย (จุดยึดต้องอยู่นิ่งเป๊ะ)
    #[test]
    fn scaling_a_corner_keeps_the_opposite_corner_pinned() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let anchor = frame_corners(&h)[0];
        let grabbed = frame_corners(&h)[2];

        h.press(grabbed, Modifiers::default());
        h.drag_to(Vec2::new(150.0, 150.0));
        h.release(Vec2::new(150.0, 150.0));

        let canvas = h.canvas_of(ids[0]);
        assert_eq!(canvas.size, Vec2::splat(200.0), "ยืดสองเท่าทั้งสองแกน");
        assert_eq!(canvas.pos, Vec2::new(50.0, 50.0));
        assert_eq!(
            canvas.obb().corners()[0],
            anchor,
            "มุมตรงข้ามคือจุดยึด ต้องไม่ขยับเลย"
        );
    }

    /// ลากเฉียง ๆ ต้องยืดคนละอัตราสองแกนได้ (ไม่งั้นครอปภาพให้พอดีกรอบไม่ได้)
    #[test]
    fn scaling_without_shift_stretches_each_axis_on_its_own() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let grabbed = frame_corners(&h)[2];

        h.press(grabbed, Modifiers::default());
        h.drag_to(Vec2::new(150.0, 50.0));
        h.release(Vec2::new(150.0, 50.0));

        assert_eq!(h.canvas_of(ids[0]).size, Vec2::new(200.0, 100.0));
    }

    /// docs/03 §2: `Shift` ตอนสเกล = คงสัดส่วน
    #[test]
    fn shift_while_scaling_keeps_the_aspect_ratio() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let grabbed = frame_corners(&h)[2];
        let before = h.canvas_of(ids[0]).size;

        h.press(grabbed, Modifiers::default());
        h.drag_to_with(Vec2::new(150.0, 50.0), SHIFT);
        h.release(Vec2::new(150.0, 50.0));

        let after = h.canvas_of(ids[0]).size;
        assert!(after.x > before.x, "ต้องโตขึ้นจริง ไม่ใช่ค้างที่เดิม");
        assert!(
            (after.x / after.y - before.x / before.y).abs() < 1e-4,
            "สัดส่วนต้องเท่าเดิม: {before:?} → {after:?}"
        );
    }

    /// ★ `Shift` ต้องมีผลแม้กดกลางคัน — คนกด Shift หลังเริ่มลากเป็นเรื่องปกติ
    #[test]
    fn shift_pressed_mid_drag_still_locks_the_aspect() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let grabbed = frame_corners(&h)[2];

        h.press(grabbed, Modifiers::default());
        h.drag_to(Vec2::new(150.0, 50.0)); // ยังไม่กด — ยืดไม่เท่ากัน
        assert_eq!(h.canvas_of(ids[0]).size, Vec2::new(200.0, 100.0));

        h.drag_to_with(Vec2::new(150.0, 50.0), SHIFT);
        let after = h.canvas_of(ids[0]).size;
        assert!(
            (after.x - after.y).abs() < 1e-4,
            "กด Shift แล้วต้องพอดีทันที: {after:?}"
        );
    }

    /// docs/03 §2: `Alt` ตอนสเกล = ยืดจากจุดกึ่งกลาง (กึ่งกลางต้องไม่ขยับ)
    #[test]
    fn alt_while_scaling_grows_from_the_centre() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let grabbed = frame_corners(&h)[2];

        h.press(grabbed, Modifiers::default());
        h.drag_to_with(Vec2::new(100.0, 100.0), ALT);
        h.release(Vec2::new(100.0, 100.0));

        let canvas = h.canvas_of(ids[0]);
        assert_eq!(canvas.pos, Vec2::ZERO, "ยืดจากกึ่งกลาง กึ่งกลางต้องอยู่นิ่ง");
        assert_eq!(canvas.size, Vec2::splat(200.0));
    }

    /// ★ ลากผ่านจุดยึดไปอีกฝั่ง **ต้องไม่กลับด้านและต้องไม่ยุบหาย**
    ///
    /// `sanitized()` clamp `size` ให้เป็นบวกอยู่แล้ว ค่าติดลบจึงไม่ได้กลายเป็นภาพ
    /// กลับด้าน แต่กลายเป็นภาพที่เล็กจนมองไม่เห็น — ซึ่งผู้ใช้อ่านว่า "ภาพหาย"
    /// การกลับด้านเป็นหน้าที่ของ `Flip` ที่ผู้ใช้สั่งอย่างจงใจ
    #[test]
    fn dragging_past_the_anchor_never_mirrors_the_image() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let grabbed = frame_corners(&h)[2];

        h.press(grabbed, Modifiers::default());
        h.drag_to(Vec2::new(-400.0, -400.0)); // เลยจุดยึดไปไกล
        h.release(Vec2::new(-400.0, -400.0));

        let canvas = h.canvas_of(ids[0]);
        assert!(canvas.size.x > 0.0 && canvas.size.y > 0.0, "{canvas:?}");
        assert!(canvas.is_sane());
    }

    /// เกณฑ์ ROADMAP ข้อเดียวกับการย้าย: **ลากค้าง = 1 undo กลับที่เดิมเป๊ะ**
    #[test]
    fn a_long_scale_drag_is_one_undo_that_restores_exactly() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let start = h.canvas_of(ids[0]);
        let grabbed = frame_corners(&h)[2];

        h.press(grabbed, Modifiers::default());
        for step in 1..=200 {
            h.drag_to(grabbed + Vec2::splat(step as f32 * 0.5));
        }
        h.release(grabbed + Vec2::splat(100.0));

        assert_eq!(h.history.undo_depth(), 1, "ลากค้างต้องเป็นขั้นเดียว");
        h.history.undo(&mut h.board).unwrap();
        assert_eq!(h.canvas_of(ids[0]), start, "ย้อนครั้งเดียวต้องคืนสภาพเป๊ะ");
    }

    /// ปล่อยแล้วลากใหม่ = สองขั้น (seal ทำงานกับ handle เหมือนกับการย้าย)
    #[test]
    fn releasing_a_handle_seals_the_step() {
        let (mut h, _) = Harness::new(2);
        h.click(Vec2::ZERO);

        for _ in 0..2 {
            let grabbed = frame_corners(&h)[2];
            h.press(grabbed, Modifiers::default());
            h.drag_to(grabbed + Vec2::splat(20.0));
            h.release(grabbed + Vec2::splat(20.0));
        }
        assert_eq!(h.history.undo_depth(), 2, "สองครั้งต้องย้อนได้สองขั้น");
    }

    /// ★ handle ต้องมีขนาดคงที่ **บนจอ** — ชั้น UI ส่ง `พิกเซล ÷ zoom` เข้ามา
    ///
    /// ข้อนี้พิสูจน์ว่าระยะจับมาจากพารามิเตอร์จริง ไม่ใช่ค่าคงที่ที่ฝังใน world
    /// (ถ้าฝังไว้ ซูมออกแล้ว handle จะเล็กลงบนจอจนจับไม่โดน)
    #[test]
    fn the_handle_reach_follows_the_zoom_it_is_given() {
        let grabbed = Vec2::new(50.0, 50.0);
        let nearby = grabbed + Vec2::splat(9.0);

        // ซูมเข้า: 9 หน่วย world ห่างเกิน handle → ตกไปเป็นการย้าย
        let (mut zoomed_in, ids) = Harness::new(2);
        zoomed_in.click(Vec2::ZERO);
        zoomed_in.press(nearby, Modifiers::default());
        zoomed_in.drag_to(nearby + Vec2::splat(40.0));
        zoomed_in.release(nearby + Vec2::splat(40.0));
        assert_eq!(
            zoomed_in.canvas_of(ids[0]).size,
            Vec2::splat(100.0),
            "นอกระยะ handle ต้องไม่สเกล"
        );

        // ซูมออก: พิกเซลเท่าเดิมบนจอ = ระยะ world กว้างขึ้น → จุดเดิมกลายเป็น handle
        let (mut zoomed_out, ids) = Harness::new(2);
        zoomed_out.handle_reach = HANDLE_REACH * 4.0;
        zoomed_out.rotate_reach = ROTATE_REACH * 4.0;
        zoomed_out.click(Vec2::ZERO);
        zoomed_out.press(nearby, Modifiers::default());
        zoomed_out.drag_to(nearby + Vec2::splat(40.0));
        zoomed_out.release(nearby + Vec2::splat(40.0));
        assert_ne!(
            zoomed_out.canvas_of(ids[0]).size,
            Vec2::splat(100.0),
            "ซูมออกแล้วต้องยังจับ handle ได้"
        );
    }

    /// docs/03 §2: ลาก **นอก** handle มุม = หมุน
    #[test]
    fn dragging_outside_a_corner_rotates_the_item() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let outside = Vec2::new(60.0, -60.0); // นอกภาพ ใกล้มุมขวาบน

        h.press(outside, Modifiers::default());
        h.drag_to(Vec2::new(60.0, 60.0)); // หมุนไปอีก 90°
        h.release(Vec2::new(60.0, 60.0));

        let canvas = h.canvas_of(ids[0]);
        assert!(
            (canvas.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-3,
            "ต้องหมุน 90°: ได้ {} rad",
            canvas.rotation
        );
        assert_eq!(canvas.size, Vec2::splat(100.0), "หมุนต้องไม่เปลี่ยนขนาด");
    }

    /// docs/03 §2: `Shift` ตอนหมุน = สแนป 15°
    #[test]
    fn shift_while_rotating_snaps_to_fifteen_degrees() {
        let step = std::f32::consts::TAU / 24.0;
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let outside = Vec2::new(60.0, -60.0);
        let start_angle = outside.y.atan2(outside.x);

        // ขยับไป 20° — ต้องลงที่ 15° ไม่ใช่ 20°
        let target = Vec2::from_angle(start_angle + 20.0_f32.to_radians()) * 85.0;
        h.press(outside, Modifiers::default());
        h.drag_to_with(target, SHIFT);
        h.release(target);

        let rotation = h.canvas_of(ids[0]).rotation;
        assert!(
            (rotation - step).abs() < 1e-3,
            "ต้องสแนปไปที่ 15° (={step} rad) ได้ {rotation} rad"
        );
    }

    /// ★ สแนปต้องลงที่**มุมสุดท้าย** ที่หารด้วย 15° ลงตัว ไม่ใช่ "เดิม + 15°"
    ///
    /// ภาพที่เอียง 4° อยู่ก่อน แล้วผู้ใช้กด Shift หมุนไปอีก 8° ต้องได้ **15° พอดี**
    /// (12° ปัดเข้าช่องที่ใกล้ที่สุด) ถ้าสแนประยะที่หมุนแทน จะได้ 4° หรือ 19°
    /// ซึ่งแปลว่า Shift ไม่ได้ช่วยจัดภาพให้ตรงเลย
    #[test]
    fn snapping_lands_on_absolute_angles_not_relative_ones() {
        let step = std::f32::consts::TAU / 24.0;
        let (mut h, ids) = Harness::new(2);
        let mut canvas = h.canvas_of(ids[0]);
        canvas.rotation = 4.0_f32.to_radians();
        h.board.set_canvas(ids[0], canvas).unwrap();
        h.board.mark_dirty(false);
        h.index.rebuild(&h.board);

        h.click(Vec2::ZERO);
        let outside = outside_corner(&h, 1);
        let start_angle = outside.y.atan2(outside.x);
        let target = Vec2::from_angle(start_angle + 8.0_f32.to_radians()) * outside.length();

        h.press(outside, Modifiers::default());
        h.drag_to_with(target, SHIFT);
        h.release(target);

        let rotation = h.canvas_of(ids[0]).rotation;
        assert!(
            (rotation - step).abs() < 1e-3,
            "4° + 8° ต้องลงที่ 15° พอดี ได้ {} °",
            rotation.to_degrees()
        );
    }

    /// ★ ภาพที่หมุนแล้ว handle ต้องหมุนตาม แล้ว hit-test ยังตรง (P2-3 ใช้ OBB)
    ///
    /// ถ้า handle ยังอยู่ที่มุมของ AABB ผู้ใช้จะกดที่ที่ *เห็น* handle แล้วไม่โดน
    #[test]
    fn handles_follow_a_rotated_item_instead_of_its_aabb() {
        let (mut h, ids) = Harness::new(2);
        let mut canvas = h.canvas_of(ids[0]);
        canvas.rotation = std::f32::consts::FRAC_PI_4; // 45°
        h.board.set_canvas(ids[0], canvas).unwrap();
        h.board.mark_dirty(false);
        h.index.rebuild(&h.board);
        h.click(Vec2::ZERO);

        let corners = frame_corners(&h);
        let aabb = h.canvas_of(ids[0]).world_bounds();
        assert!(
            corners.iter().all(|c| (c.length() - 70.71).abs() < 0.1),
            "มุมต้องอยู่บนตัวภาพที่หมุนแล้ว: {corners:?}"
        );
        assert!(
            !corners.iter().any(|c| (*c - aabb.max).length() < 1.0),
            "ต้องไม่ใช่มุมของ AABB"
        );

        // จับมุมที่หมุนแล้วจริง ๆ แล้วยืดออกตามแนวแกนของภาพ
        let grabbed = corners[2];
        let anchor = corners[0];
        h.press(grabbed, Modifiers::default());
        h.drag_to(grabbed * 2.0);
        h.release(grabbed * 2.0);

        let after = h.canvas_of(ids[0]);
        assert!(
            (after.size - Vec2::splat(150.0)).length() < 1e-2,
            "ต้องยืดตามแกนของภาพเอง 1.5 เท่า: {:?}",
            after.size
        );
        assert!(
            (after.obb().corners()[0] - anchor).length() < 1e-2,
            "จุดยึดของภาพที่หมุนต้องยังอยู่ที่เดิม"
        );
        assert!(
            (after.rotation - std::f32::consts::FRAC_PI_4).abs() < 1e-4,
            "สเกลต้องไม่แตะมุมหมุน"
        );
    }

    /// ★ เลือกหลายใบแล้วสเกล = ทำกับ **กรอบรวม** ไม่ใช่ทีละใบ (HANDOFF §2.4)
    ///
    /// และบังคับสัดส่วนเดิมเสมอ: `ItemCanvas` ไม่มีที่เก็บการเฉือน การยืดแกนเดียว
    /// ของกรอบแนวแกนกับภาพที่หมุนอยู่ข้างในจึงแทนค่าไม่ได้
    #[test]
    fn scaling_a_multi_selection_works_on_the_group_box() {
        let (mut h, ids) = Harness::new(3);
        h.press(Vec2::ZERO, CTRL);
        h.release(Vec2::ZERO);
        h.press(Vec2::new(200.0, 0.0), CTRL);
        h.release(Vec2::new(200.0, 0.0));

        let corners = frame_corners(&h);
        assert_eq!(corners[0], Vec2::new(-50.0, -50.0), "กรอบรวมของสองใบ");
        assert_eq!(corners[2], Vec2::new(250.0, 50.0));

        let anchor = corners[0];
        h.press(corners[2], Modifiers::default());
        // ลากเฉียงแบบไม่เท่ากันสองแกน — ผลต้องยังคงสัดส่วน
        h.drag_to(Vec2::new(550.0, 50.0));
        h.release(Vec2::new(550.0, 50.0));

        let first = h.canvas_of(ids[0]);
        let second = h.canvas_of(ids[1]);
        assert!(
            (first.size.x - first.size.y).abs() < 1e-3,
            "หลายใบต้องคงสัดส่วนเสมอ: {:?}",
            first.size
        );
        assert!(first.size.x > 100.0, "ต้องโตขึ้นจริง");
        assert!(
            (first.obb().corners()[0] - anchor).length() < 1e-3,
            "มุมยึดของกรอบรวมต้องอยู่นิ่ง"
        );
        assert!(
            (second.pos - first.pos).length() > 200.0,
            "ระยะห่างต้องขยายตามกรอบรวม ไม่ใช่ต่างคนต่างโต"
        );
        assert_eq!(
            h.canvas_of(ids[2]).size,
            Vec2::splat(100.0),
            "ตัวที่ไม่ได้เลือกต้องนิ่ง"
        );
    }

    /// หมุนหลายใบ = **ตำแหน่ง**ของทุกใบต้องโคจรรอบกึ่งกลางกรอบรวมด้วย
    /// ไม่ใช่ต่างคนต่างหมุนอยู่กับที่
    #[test]
    fn rotating_a_multi_selection_swings_every_item_around_the_group_centre() {
        let (mut h, ids) = Harness::new(3);
        h.press(Vec2::ZERO, CTRL);
        h.release(Vec2::ZERO);
        h.press(Vec2::new(200.0, 0.0), CTRL);
        h.release(Vec2::new(200.0, 0.0));

        let centre = Vec2::new(100.0, 0.0);
        let outside = Vec2::new(260.0, -60.0);
        let arm = outside - centre;
        let start_angle = arm.y.atan2(arm.x);
        let target =
            centre + Vec2::from_angle(start_angle + std::f32::consts::FRAC_PI_2) * arm.length();

        h.press(outside, Modifiers::default());
        h.drag_to(target);
        h.release(target);

        let first = h.canvas_of(ids[0]);
        assert!(
            (first.pos - Vec2::new(100.0, -100.0)).length() < 1e-2,
            "ต้องโคจรรอบกึ่งกลางกรอบรวม: {:?}",
            first.pos
        );
        assert!((first.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-3);
        assert!(
            (h.canvas_of(ids[1]).pos - Vec2::new(100.0, 100.0)).length() < 1e-2,
            "ใบที่สองต้องไปอยู่ฝั่งตรงข้าม"
        );
        assert_eq!(h.history.undo_depth(), 1, "หมุนทั้งกลุ่มคือขั้นเดียว");
    }

    /// ภาพที่ล็อกไว้ต้องไม่มี handle เลย — handle ที่ลากแล้วไม่มีอะไรเกิดขึ้น
    /// ผู้ใช้จะอ่านว่าโปรแกรมค้าง ไม่ใช่ว่าภาพถูกล็อก
    #[test]
    fn a_locked_item_shows_no_handles_at_all() {
        let (mut h, ids) = Harness::new(2);
        let mut canvas = h.canvas_of(ids[0]);
        canvas.locked = true;
        h.board.set_canvas(ids[0], canvas).unwrap();
        h.board.mark_dirty(false);

        h.click(Vec2::ZERO);
        assert!(selection_frame(&h.board, &h.selection).is_none());

        h.press(Vec2::new(50.0, 50.0), Modifiers::default());
        h.drag_to(Vec2::new(200.0, 200.0));
        h.release(Vec2::new(200.0, 200.0));
        assert_eq!(h.canvas_of(ids[0]), canvas, "ล็อกแล้วต้องไม่ถูกแตะเลย");
        assert_eq!(h.history.undo_depth(), 0);
    }

    /// ยังไม่ได้เลือกอะไร = ไม่มีกรอบ = ทุกอย่างเป็นการเลือกตามเดิม
    #[test]
    fn nothing_selected_means_no_frame_and_no_handles() {
        let (h, _) = Harness::new(3);
        assert!(selection_frame(&h.board, &h.selection).is_none());
    }

    /// กด handle เฉย ๆ (ไม่ลาก) ต้องไม่แตะทั้งการเลือกและเอกสาร
    #[test]
    fn tapping_a_handle_changes_nothing() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        h.board.mark_dirty(false);
        let before = h.board.clone();
        let grabbed = frame_corners(&h)[2];

        h.press(grabbed, Modifiers::default());
        h.drag_to(grabbed + Vec2::splat(1.0)); // ต่ำกว่าระยะเริ่มลาก
        h.release(grabbed + Vec2::splat(1.0));

        assert_eq!(h.selected(), vec![ids[0]], "ต้องไม่ล้างการเลือก");
        assert_eq!(h.board, before);
        assert_eq!(h.history.undo_depth(), 0);
    }

    /// ลาก handle แล้ว hit-test ต้องตามขนาดใหม่ (index ถูกอัปเดตโดยผู้เรียก)
    ///
    /// ★ เคยพลาดมาแล้วตอนทำการย้าย — อาการคือลากได้ครั้งเดียวแล้วไม่ตอบสนอง
    #[test]
    fn hit_testing_follows_the_new_size_after_a_scale() {
        let (mut h, ids) = Harness::new(2);
        h.click(Vec2::ZERO);
        let far = Vec2::new(120.0, 120.0);
        assert_eq!(h.index.hit_test(&h.board, far), None, "ยังไม่โตต้องไม่โดน");

        let grabbed = frame_corners(&h)[2];
        h.press(grabbed, Modifiers::default());
        h.drag_to(Vec2::new(150.0, 150.0));
        h.release(Vec2::new(150.0, 150.0));

        assert_eq!(
            h.index.hit_test(&h.board, far),
            Some(ids[0]),
            "ขยายแล้วต้องคลิกโดนพื้นที่ใหม่ทันที"
        );
    }

    /// I-4: พิกัดพังระหว่างลาก handle ต้องไม่ทำให้ item กลายเป็นค่าที่ไม่ใช่ตัวเลข
    #[test]
    fn non_finite_input_never_corrupts_a_handle_drag() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let (mut h, ids) = Harness::new(2);
            h.click(Vec2::ZERO);
            let grabbed = frame_corners(&h)[2];

            h.press(grabbed, Modifiers::default());
            h.drag_to(Vec2::new(bad, 30.0));
            h.drag_to(Vec2::new(30.0, bad));
            h.release(Vec2::new(bad, bad));

            let canvas = h.canvas_of(ids[0]);
            assert!(canvas.is_sane(), "{bad}: {canvas:?}");
            assert!(canvas.pos.is_finite() && canvas.size.is_finite());
        }
    }

    // ---------- crop (P2-7) ----------

    /// ครอปได้ต้องเข้าเครื่องมือครอปก่อน — เทสต์กลุ่มนี้เลือกภาพเดียวแล้วสลับเครื่องมือ
    fn cropping(n: u32) -> (Harness, Vec<ItemId>) {
        let (mut h, ids) = Harness::new(n);
        h.click(Vec2::ZERO);
        h.active_tool = Tool::Crop;
        (h, ids)
    }

    /// ★★ หัวใจของ "ไม่ทำลายต้นฉบับ": ลากขอบขวาเข้ามา แล้ว
    /// **pixel ที่เหลือต้องอยู่ที่เดิมเป๊ะ** หายไปแค่ส่วนที่ถูกตัด
    ///
    /// ถ้าแก้แต่ `crop` โดยไม่หด `size`/`pos` ผู้ใช้จะเห็นภาพ *ซูมเข้า* แทนที่จะเห็น
    /// ภาพ *ถูกตัดขอบ* ซึ่งเป็นคนละอย่างกันโดยสิ้นเชิง
    #[test]
    fn cropping_an_edge_trims_it_without_moving_what_is_left() {
        let (mut h, ids) = cropping(2);
        let before = h.canvas_of(ids[0]);
        let left_edge = before.obb().corners()[0].x;

        // ภาพกิน -50..50 — ลากขอบขวาเข้ามาที่ x = 0 (ตัดครึ่งขวาทิ้ง)
        h.press(Vec2::new(50.0, 0.0), Modifiers::default());
        h.drag_to(Vec2::new(0.0, 0.0));
        h.release(Vec2::new(0.0, 0.0));

        let after = h.canvas_of(ids[0]);
        assert_eq!(after.size, Vec2::new(50.0, 100.0), "กว้างหายครึ่ง สูงเท่าเดิม");
        assert_eq!(after.pos, Vec2::new(-25.0, 0.0), "กึ่งกลางขยับตามขอบที่หด");
        assert!(
            (after.obb().corners()[0].x - left_edge).abs() < 1e-4,
            "ขอบซ้ายต้องไม่ขยับเลย — pixel ที่เหลืออยู่ที่เดิม"
        );
        // crop เก็บเป็นสัดส่วนของภาพต้นฉบับ (docs/02 §2.1)
        assert_eq!(after.crop.min, Vec2::ZERO);
        assert!((after.crop.max.x - 0.5).abs() < 1e-4, "{:?}", after.crop);
        assert!((after.crop.max.y - 1.0).abs() < 1e-4);
    }

    /// ครอปสองครั้งต้องทบกันถูก — สัดส่วนรอบสองคิดจากภาพ**ต้นฉบับ** ไม่ใช่จากที่เหลือ
    #[test]
    fn cropping_twice_composes_against_the_original_image() {
        let (mut h, ids) = cropping(2);

        h.press(Vec2::new(50.0, 0.0), Modifiers::default());
        h.drag_to(Vec2::ZERO);
        h.release(Vec2::ZERO);
        // ตอนนี้ภาพกิน -50..0 และ crop.max.x = 0.5

        h.press(Vec2::new(0.0, 0.0), Modifiers::default());
        h.drag_to(Vec2::new(-25.0, 0.0));
        h.release(Vec2::new(-25.0, 0.0));

        let after = h.canvas_of(ids[0]);
        assert_eq!(after.size.x, 25.0);
        assert!(
            (after.crop.max.x - 0.25).abs() < 1e-4,
            "ครอปเหลือหนึ่งในสี่ของ**ต้นฉบับ** ไม่ใช่ครึ่งของที่เหลือ: {:?}",
            after.crop
        );
    }

    /// ★ ลากกลับออกได้จนสุดขอบภาพต้นฉบับ — ตัดเกินไปนิดเดียวต้องดึงคืนได้ทันที
    #[test]
    fn an_edge_can_be_dragged_back_out_but_never_past_the_original() {
        let (mut h, ids) = cropping(2);

        h.press(Vec2::new(50.0, 0.0), Modifiers::default());
        h.drag_to(Vec2::ZERO); // ตัดครึ่งขวา
        h.drag_to(Vec2::new(25.0, 0.0)); // ดึงกลับออกครึ่งหนึ่งของที่ตัด
        h.release(Vec2::new(25.0, 0.0));
        let back = h.canvas_of(ids[0]);
        assert_eq!(back.size.x, 75.0);
        assert!((back.crop.max.x - 0.75).abs() < 1e-4);

        // ลากออกไปไกลกว่าภาพต้นฉบับ — ต้องหยุดที่ขอบภาพ ไม่ใช่ยืดภาพออก
        h.press(Vec2::new(25.0, 0.0), Modifiers::default());
        h.drag_to(Vec2::new(9_000.0, 0.0));
        h.release(Vec2::new(9_000.0, 0.0));
        let full = h.canvas_of(ids[0]);
        assert_eq!(full.size.x, 100.0, "หยุดที่ความกว้างของภาพต้นฉบับ");
        assert!((full.crop.max.x - 1.0).abs() < 1e-4);
        assert_eq!(full.pos, Vec2::ZERO, "กลับมาเต็มใบแล้วต้องอยู่ที่เดิม");
    }

    /// กลางด้านขยับแกนเดียว · มุมขยับสองแกน
    #[test]
    fn edge_handles_move_one_axis_and_corner_handles_move_two() {
        let (mut h, ids) = cropping(2);
        h.press(Vec2::new(0.0, -50.0), Modifiers::default()); // กลางด้านบน
        h.drag_to(Vec2::new(0.0, -20.0));
        h.release(Vec2::new(0.0, -20.0));
        let after = h.canvas_of(ids[0]);
        assert_eq!(after.size, Vec2::new(100.0, 70.0), "กว้างต้องไม่เปลี่ยน");

        let (mut h, ids) = cropping(2);
        h.press(Vec2::new(50.0, 50.0), Modifiers::default()); // มุมขวาล่าง
        h.drag_to(Vec2::new(20.0, 30.0));
        h.release(Vec2::new(20.0, 30.0));
        let after = h.canvas_of(ids[0]);
        assert_eq!(after.size, Vec2::new(70.0, 80.0), "มุมตัดสองแกนพร้อมกัน");
    }

    /// ★ ครอปแล้ว hit-test ต้องใช้กรอบใหม่ — คลิกในส่วนที่ถูกตัดออกไปต้องไม่โดน
    #[test]
    fn hit_testing_ignores_the_part_that_was_cropped_away() {
        let (mut h, ids) = cropping(2);
        assert_eq!(
            h.index.hit_test(&h.board, Vec2::new(40.0, 0.0)),
            Some(ids[0])
        );

        h.press(Vec2::new(50.0, 0.0), Modifiers::default());
        h.drag_to(Vec2::ZERO);
        h.release(Vec2::ZERO);
        h.index.rebuild(&h.board);

        assert_eq!(
            h.index.hit_test(&h.board, Vec2::new(40.0, 0.0)),
            None,
            "ส่วนที่ถูกตัดไปแล้วต้องกดไม่โดน"
        );
        assert_eq!(
            h.index.hit_test(&h.board, Vec2::new(-40.0, 0.0)),
            Some(ids[0]),
            "ส่วนที่ยังเหลือยังกดโดนตามปกติ"
        );
    }

    /// docs/03 §2: ดับเบิลคลิกรีเซ็ต — และต้องคืน**เรขาคณิต**ด้วย ไม่ใช่แค่ค่า `crop`
    #[test]
    fn double_clicking_resets_the_crop_and_the_geometry_together() {
        let (mut h, ids) = cropping(2);
        let original = h.canvas_of(ids[0]);

        h.press(Vec2::new(50.0, 50.0), Modifiers::default());
        h.drag_to(Vec2::new(-20.0, -10.0));
        h.release(Vec2::new(-20.0, -10.0));
        let cropped = h.canvas_of(ids[0]);
        assert_ne!(cropped.size, original.size);

        h.feed(CanvasEvent::DoubleClick {
            world: cropped.pos, // กลางภาพที่ครอปแล้ว
        });

        let reset = h.canvas_of(ids[0]);
        assert_eq!(reset.crop, CropRect::default(), "กรอบ crop กลับเป็นภาพเต็ม");
        // ★ ส่วนที่ยังเห็นอยู่ต้องไม่ขยับ — มุมซ้ายบนของภาพที่ครอปแล้วยังอยู่ที่เดิม
        //   เทียบกับตำแหน่งเดียวกันในภาพเต็ม (ครอปจากมุมขวาล่าง ขอบซ้ายบนจึงไม่ขยับ)
        assert!(
            (reset.obb().corners()[0] - cropped.obb().corners()[0]).length() < 1e-3,
            "ขอบที่ไม่ได้ถูกตัดต้องอยู่ที่เดิม: {:?} vs {:?}",
            reset.obb().corners()[0],
            cropped.obb().corners()[0]
        );
        // ขนาดคืนมาจากการหารด้วยสัดส่วน จึงคลาดเคลื่อนระดับ f32 ได้ ไม่ใช่เท่าเป๊ะ
        assert!(
            (reset.size - original.size).length() < 1e-3,
            "ขนาดต้องกลับมาเท่าภาพเต็ม: {:?} vs {:?}",
            reset.size,
            original.size
        );
    }

    /// ดับเบิลคลิกตอนไม่ได้อยู่ในเครื่องมือครอป ต้องไม่แตะอะไรเลย
    #[test]
    fn double_clicking_outside_the_crop_tool_changes_nothing() {
        let (mut h, ids) = cropping(2);
        h.press(Vec2::new(50.0, 50.0), Modifiers::default());
        h.drag_to(Vec2::new(0.0, 0.0));
        h.release(Vec2::new(0.0, 0.0));
        let cropped = h.canvas_of(ids[0]);

        h.active_tool = Tool::Select;
        h.feed(CanvasEvent::DoubleClick { world: cropped.pos });
        assert_eq!(h.canvas_of(ids[0]), cropped);

        // และดับเบิลคลิกที่ว่างตอนอยู่ในเครื่องมือครอปก็ต้องไม่รีเซ็ต
        h.active_tool = Tool::Crop;
        h.feed(CanvasEvent::DoubleClick {
            world: Vec2::new(600.0, 600.0),
        });
        assert_eq!(h.canvas_of(ids[0]), cropped);
    }

    /// ★ เกณฑ์เดียวกับ P2-5: ลากค้าง = 1 undo กลับที่เดิมเป๊ะ · ปล่อยแล้ว seal
    #[test]
    fn a_long_crop_drag_is_one_undo_and_two_drags_stay_separate() {
        let (mut h, ids) = cropping(2);
        let original = h.canvas_of(ids[0]);

        h.press(Vec2::new(50.0, 0.0), Modifiers::default());
        for step in 1..=200 {
            h.drag_to(Vec2::new(50.0 - step as f32 * 0.2, 0.0));
        }
        h.release(Vec2::new(10.0, 0.0));
        assert_eq!(h.history.undo_depth(), 1, "ลากค้างต้องเป็นขั้นเดียว");

        h.press(Vec2::new(10.0, 0.0), Modifiers::default());
        h.drag_to(Vec2::new(0.0, 0.0));
        h.release(Vec2::new(0.0, 0.0));
        assert_eq!(h.history.undo_depth(), 2, "ปล่อยแล้วลากใหม่ = คนละขั้น");

        h.history.undo(&mut h.board).unwrap();
        h.history.undo(&mut h.board).unwrap();
        assert_eq!(h.canvas_of(ids[0]), original, "ย้อนครบต้องคืนสภาพเป๊ะ");
    }

    /// ★ ครอปกับการย้าย **ห้าม merge ข้ามกัน** แม้จะแตะ item ตัวเดียวกัน
    ///
    /// ผู้ใช้ที่ครอปเสร็จแล้วสลับไปย้ายต่อ ต้องกด Ctrl+Z แล้วได้การย้ายคืนอย่างเดียว
    /// ไม่ใช่เสียการครอปไปด้วยทั้งที่ไม่ได้ขอ
    #[test]
    fn a_crop_never_merges_with_a_move() {
        let (mut h, ids) = cropping(2);
        h.press(Vec2::new(50.0, 0.0), Modifiers::default());
        h.drag_to(Vec2::new(20.0, 0.0));
        // ★ จงใจไม่ปล่อยเมาส์ (ไม่ seal) แล้วสลับเครื่องมือ — หน้าต่าง merge ยังเปิดอยู่
        h.active_tool = Tool::Select;
        let cropped = h.canvas_of(ids[0]);

        h.press(cropped.pos, Modifiers::default());
        h.drag_to(cropped.pos + Vec2::new(200.0, 0.0));
        h.release(cropped.pos + Vec2::new(200.0, 0.0));

        assert_eq!(h.history.undo_depth(), 2, "ต้องเป็นสองขั้น ไม่ใช่ขั้นเดียว");
        h.history.undo(&mut h.board).unwrap();
        let after_undo = h.canvas_of(ids[0]);
        assert_eq!(after_undo.crop, cropped.crop, "ย้อนการย้ายต้องไม่เสียการครอป");
        assert_eq!(after_undo.size, cropped.size);
    }

    /// เลือกหลายใบแล้วครอปไม่ได้ — กรอบรวมไม่ได้ผูกกับ pixel ของภาพไหนเลย
    #[test]
    fn cropping_needs_exactly_one_item() {
        let (mut h, ids) = Harness::new(3);
        h.press(Vec2::ZERO, CTRL);
        h.release(Vec2::ZERO);
        h.press(Vec2::new(200.0, 0.0), CTRL);
        h.release(Vec2::new(200.0, 0.0));
        h.active_tool = Tool::Crop;
        let before = (h.canvas_of(ids[0]), h.canvas_of(ids[1]));

        let corner = selection_frame(&h.board, &h.selection).unwrap().corners()[2];
        h.press(corner, Modifiers::default());
        h.drag_to(corner - Vec2::splat(40.0));
        h.release(corner - Vec2::splat(40.0));

        assert_eq!((h.canvas_of(ids[0]), h.canvas_of(ids[1])), before);
        assert_eq!(h.history.undo_depth(), 0);
    }

    /// เครื่องมือครอปมี handle แปดตัว เครื่องมือเลือกมีสี่ — และมุมมาก่อนกลางด้านเสมอ
    #[test]
    fn the_crop_tool_offers_edge_handles_that_select_does_not() {
        assert_eq!(handles_for(Tool::Select).len(), 4);
        assert_eq!(handles_for(Tool::Crop).len(), 8);
        assert!(
            handles_for(Tool::Crop)[..4]
                .iter()
                .all(|dir| dir.x != 0 && dir.y != 0),
            "สี่ตัวแรกต้องเป็นมุม ไม่งั้นกลางด้านจะแย่งการกดที่มุมของกรอบแคบ ๆ"
        );
    }

    /// I-4: พิกัดพังระหว่างครอปต้องไม่ทำให้ค่าใน `ItemCanvas` เพี้ยน
    #[test]
    fn non_finite_input_never_corrupts_a_crop() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let (mut h, ids) = cropping(2);
            h.press(Vec2::new(50.0, 50.0), Modifiers::default());
            h.drag_to(Vec2::new(bad, 10.0));
            h.drag_to(Vec2::new(10.0, bad));
            h.release(Vec2::new(bad, bad));

            let canvas = h.canvas_of(ids[0]);
            assert!(canvas.is_sane(), "{bad}: {canvas:?}");
            assert!(canvas.crop.min.is_finite() && canvas.crop.max.is_finite());
            assert!(canvas.crop.max.x >= canvas.crop.min.x);
        }
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
            handle_reach: HANDLE_REACH,
            rotate_reach: ROTATE_REACH,
            tool: Tool::Select,
        };

        let outcome = tool.handle(
            ctx,
            &mut selection,
            CanvasEvent::Move {
                world: Vec2::ZERO,
                modifiers: Modifiers::default(),
            },
        );
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
