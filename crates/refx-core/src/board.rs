//! `Board` · `Item` · `ItemCanvas` · `ItemMeta` · `AssetRef` — โครงข้อมูลของงานผู้ใช้
//!
//! ★ **หัวใจของดีไซน์สองโหมด:** `ItemCanvas` (ฝั่ง Canvas) กับ `ItemMeta` (ฝั่ง Arrange)
//! อยู่คู่กันตลอดชีวิตของ item สลับโหมดไปมาแล้วข้อมูลอีกฝั่งไม่หาย (ARCHITECTURE §4)
//! นี่คือเหตุผลที่แยกเป็นสอง struct แทนที่จะยัดรวมกัน
//!
//! ★ **z-order ไม่อยู่ใน `ItemCanvas`** แต่เป็น `Vec<ItemId>` ระดับ board —
//! "ส่งไปหลังสุด" ด้วยตัวเลข z ทำให้เลขชนกันแล้วต้อง renumber ทั้งชุด ส่วน `Vec`
//! แค่ย้ายตำแหน่ง และ **ลำดับ render = ลำดับใน Vec** ไม่ต้อง sort ทุกเฟรม (docs/02 §2.1)
//!
//! ★ **ทุกการแก้ `Board` ต้องผ่าน `Command`** (I-3) ตัวที่แก้สถานะจึงเป็น `pub(crate)`
//! ไม่ใช่ `pub` — ชั้นบนอ่านผ่าน accessor ได้ แต่เขียนไม่ได้ตรง ๆ **คอมไพเลอร์บังคับให้**
//! ไม่ใช่กฎที่ต้องจำเอง (docs/08 §4 ข้อ 10)
//!
//! spec: docs/02-data-model.md §2

use std::path::PathBuf;

use glam::{UVec2, Vec2};

use crate::arena::{Arena, ArenaKey as _, BoardId, GroupId, ItemId};
use crate::geom::{Obb, Rect};
use crate::hash::ContentHash;
use crate::view::ViewState;

/// เพดานพิกัด world ทั้งสองแกน (docs/02 §6)
///
/// f32 ให้ความละเอียด ~0.06 unit ที่ 1e6 ซึ่งตากับหน้าจอไม่มีทางเห็น แต่ถ้าปล่อยให้
/// ลากไปไกลไม่จำกัด จะเกิดอาการ "ภาพสั่น/กระตุกตอนซูม" ที่ตามแก้ยากมาก
/// — clamp ตั้งแต่แรกถูกกว่าเปลี่ยนไป f64 ทีหลังหลายเท่า
pub const WORLD_LIMIT: f32 = 1_000_000.0;

/// ขนาดที่แสดงเล็กที่สุด — 0 หรือติดลบทำให้ hit-test และ layout หารศูนย์
pub const MIN_ITEM_SIZE: f32 = 0.01;

/// ขนาดที่ใช้เมื่อค่าจากไฟล์ใช้ไม่ได้เลย (`NaN`/`inf`)
///
/// ★ ไม่ตกไปที่ [`MIN_ITEM_SIZE`] และไม่ตกไปที่ [`WORLD_LIMIT`] — ตัวเล็กสุดคือจุด
/// ที่ผู้ใช้หาไม่เจอ (อ่านว่า "ภาพหาย") ส่วนตัวใหญ่สุดคือสี่เหลี่ยมกว้าง 1e6 หน่วย
/// ที่บังทั้ง board ทั้งสองอย่างแยกไม่ออกจาก "โปรแกรมพัง" ค่ากลางที่ดูเหมือน item
/// ปกติทำให้ผู้ใช้เห็นว่ามีของอยู่และแก้เองได้
pub const DEFAULT_ITEM_SIZE: f32 = 256.0;

// ---------------------------------------------------------------------------
// ชนิดข้อมูลที่ `refx-core` ต้องเป็นเจ้าของเอง (docs/02 §2.2.5)
// ---------------------------------------------------------------------------

/// ชนิดไฟล์ภาพที่ RefX เปิดได้
///
/// ★ **นิยามเอง ไม่ยืมจาก crate `image`** (docs/02 §2.2.5): `refx-core` depend `image`
/// ไม่ได้ (ARCHITECTURE §2) และค่าตัวนี้ต้อง serialize ลง `.refx` ได้ (docs/02 §7)
/// ตัวแปลงระหว่างสองโลกอยู่ที่ `refx-asset` **จุดเดียว**
///
/// รายการต้องตรงกับ `refx_asset::decode::ALLOWED_FORMATS`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    /// PNG
    Png,
    /// JPEG
    Jpeg,
    /// WebP
    WebP,
    /// GIF
    Gif,
    /// BMP
    Bmp,
    /// Targa
    Tga,
    /// TIFF
    Tiff,

    /// ★ **ต้องมี** — สองเหตุผล (docs/02 §2.2.5)
    ///
    /// 1. ไฟล์ที่บันทึกด้วย RefX รุ่นใหม่กว่าอาจมี format ที่รุ่นนี้ไม่รู้จัก
    ///    อ่านเจอค่าที่ไม่รู้จักต้องตกมาที่นี่ **ห้าม error ทิ้งทั้งไฟล์** (I-3)
    /// 2. บางเส้นทางยังไม่รู้ format จริงตอนสร้าง [`AssetRef`] — โดยเฉพาะ **cache hit
    ///    ที่ไม่ได้แตะไบต์ของไฟล์เลย** เขียน `Unknown` ตรง ๆ **ดีกว่าเดาจากนามสกุล**
    ///    เพราะค่านี้ถูก persist ลงไฟล์ และนามสกุลโกหกได้ (I-4)
    Unknown,
}

/// ทำไมภาพนี้เปิดไม่ได้ — เก็บติด item ไว้เพื่อให้ผู้ใช้ relink ได้
///
/// ★ **ห้ามเก็บเป็น `String`** ถึงจะง่ายกว่า: ข้อความจะถูกบันทึกลง `.refx` ด้วยภาษา
/// ที่ผู้ใช้ตั้งไว้ *ตอนบันทึก* แล้วแปลกลับทีหลังไม่ได้อีก นักวาดญี่ปุ่นที่เปิดไฟล์
/// จากเพื่อนคนไทยจะเห็นข้อความไทยค้างในไฟล์ตลอดไป — ขัด docs/03 §0 ที่บังคับว่า
/// ข้อความผู้ใช้ต้องประกอบขึ้นจากข้อมูลที่มีโครงสร้าง
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MissingReason {
    /// ไฟล์ไม่อยู่ที่เดิมแล้ว (ย้าย/เปลี่ยนชื่อ/ลบ) — เคสที่ relink ช่วยได้
    FileNotFound,
    /// ใหญ่เกินเพดานของเครื่องนี้ (docs/05 §2)
    TooLarge,
    /// ชนิดไฟล์ที่ RefX ไม่รองรับ
    UnsupportedFormat,
    /// เปิดได้แต่เนื้อในเสีย (ไฟล์ถูกตัด / decoder ล้ม)
    Damaged,
    /// อ่านไฟล์ไม่ได้เลย (สิทธิ์, ไดรฟ์เครือข่ายหลุด, ยังไม่ sync ลงเครื่อง)
    Unreadable,

    /// ★ **ต้องมี** — ไฟล์ที่บันทึกด้วย RefX รุ่นใหม่กว่าอาจมีเหตุผลที่รุ่นนี้ไม่รู้จัก
    ///
    /// อ่านเจอค่าที่ไม่รู้จักต้องตกมาที่นี่ **ห้าม error ทิ้งทั้งไฟล์** — board ที่
    /// จัดมาสามชั่วโมงเปิดไม่ได้เพราะ item เดียวมีรหัสเหตุผลแปลก ๆ คืองานหาย (I-3)
    Unknown,
}

/// การพลิกภาพ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Flip {
    /// ไม่พลิก
    #[default]
    None,
    /// พลิกซ้าย-ขวา — ท่าที่นักวาดใช้ตรวจสัดส่วนที่เพี้ยนบ่อยที่สุด
    Horizontal,
    /// พลิกบน-ล่าง
    Vertical,
    /// พลิกทั้งสองแกน
    Both,
}

/// ป้ายสีสำหรับคัดภาพในโหมด Arrange
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorLabel {
    /// แดง
    Red,
    /// ส้ม
    Orange,
    /// เหลือง
    Yellow,
    /// เขียว
    Green,
    /// น้ำเงิน
    Blue,
    /// ม่วง
    Purple,
}

/// คีย์ของแท็ก — ตารางชื่อแท็กจริงอยู่ระดับ workspace (P2-9)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TagId(pub u32);

// ---------------------------------------------------------------------------
// ItemCanvas
// ---------------------------------------------------------------------------

/// กรอบ crop เป็นสัดส่วน 0..1 ของภาพต้นฉบับ
///
/// เก็บเป็นสัดส่วนไม่ใช่พิกเซล เพราะ RefX **ไม่แก้ไฟล์ต้นฉบับ** (ARCHITECTURE §7 ข้อ 4)
/// ค่าที่เป็นสัดส่วนยังถูกต้องแม้ผู้ใช้ relink ไปหาไฟล์ที่ความละเอียดต่างออกไป
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropRect {
    /// มุมซ้ายบน 0..1
    pub min: Vec2,
    /// มุมขวาล่าง 0..1
    pub max: Vec2,
}

impl Default for CropRect {
    fn default() -> Self {
        Self {
            min: Vec2::ZERO,
            max: Vec2::ONE,
        }
    }
}

impl CropRect {
    /// ทำให้ค่าอยู่ในช่วงที่ใช้ได้เสมอ — ค่าจากไฟล์เชื่อไม่ได้ (I-4)
    ///
    /// `NaN` จากไฟล์ที่เสียหายจะทำให้ shader คำนวณ UV ออกมาเป็นภาพว่างเปล่า
    /// โดยไม่มี error ที่ไหนเลย ซึ่งผู้ใช้อ่านว่า "ภาพหาย"
    #[must_use]
    pub fn sanitized(self) -> Self {
        let clamp01 = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let min = Vec2::new(clamp01(self.min.x), clamp01(self.min.y));
        let max = Vec2::new(clamp01(self.max.x), clamp01(self.max.y));
        Self {
            min,
            // ขอบขวา/ล่างต้องไม่มาก่อนขอบซ้าย/บน ไม่งั้นได้กรอบกลับด้าน
            max: max.max(min),
        }
    }

    /// กรอบนี้กินพื้นที่จริงไหม
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.max.x <= self.min.x || self.max.y <= self.min.y
    }
}

/// ฟิลเตอร์ที่คำนวณใน shader — ไม่แตะ pixel ต้นฉบับ
///
/// grayscale/invert ไม่ใช่ของเล่น: นักวาดใช้ตรวจค่าน้ำหนัก (value) ทุกวัน
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemFilter {
    /// แปลงเป็นขาวดำ
    pub grayscale: bool,
    /// กลับสี
    pub invert: bool,
    /// ความสว่าง -1.0..=1.0 (0 = ไม่เปลี่ยน)
    pub brightness: f32,
    /// คอนทราสต์ -1.0..=1.0 (0 = ไม่เปลี่ยน)
    pub contrast: f32,
}

impl Default for ItemFilter {
    fn default() -> Self {
        Self {
            grayscale: false,
            invert: false,
            brightness: 0.0,
            contrast: 0.0,
        }
    }
}

impl ItemFilter {
    /// clamp ค่าให้อยู่ในช่วงที่ shader รับได้ (I-4)
    #[must_use]
    pub fn sanitized(self) -> Self {
        let clamp = |v: f32| {
            if v.is_finite() {
                v.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        Self {
            brightness: clamp(self.brightness),
            contrast: clamp(self.contrast),
            ..self
        }
    }
}

/// สถานะฝั่ง Canvas mode — ตำแหน่ง/สเกล/หมุน/crop/opacity
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemCanvas {
    /// ตำแหน่งใน world space (จุดกึ่งกลางภาพ)
    pub pos: Vec2,
    /// ขนาดที่แสดงเป็น world units — **ไม่ใช่ขนาด pixel ต้นฉบับ**
    pub size: Vec2,
    /// การหมุน (เรเดียน)
    pub rotation: f32,
    /// การพลิก
    pub flip: Flip,
    /// ความทึบ 0.0..=1.0
    pub opacity: f32,
    /// กรอบ crop เป็นสัดส่วนของภาพต้นฉบับ
    pub crop: CropRect,
    /// ล็อกไม่ให้ขยับ
    pub locked: bool,
    /// แสดงบน canvas หรือไม่
    pub visible: bool,
    /// ฟิลเตอร์ที่คำนวณใน shader
    pub filter: ItemFilter,
}

impl Default for ItemCanvas {
    fn default() -> Self {
        Self {
            pos: Vec2::ZERO,
            size: Vec2::splat(DEFAULT_ITEM_SIZE),
            rotation: 0.0,
            flip: Flip::None,
            opacity: 1.0,
            crop: CropRect::default(),
            locked: false,
            visible: true,
            filter: ItemFilter::default(),
        }
    }
}

impl ItemCanvas {
    /// ★ บังคับให้ทุกค่าอยู่ในช่วงที่ปลอดภัย — **ประตูเดียวที่ค่าตัวเลขเข้ามาได้**
    ///
    /// เรียกจาก `Command` ทุกตัวที่แตะ `ItemCanvas` และจากตัวอ่านไฟล์
    /// ถ้ามีเส้นทางไหนข้ามไปได้ `NaN` จากไฟล์ที่เสียหายจะเข้าไปถึง shader
    /// แล้วภาพจะหายทั้งจอโดยไม่มี error ที่ไหนเลย (I-4, docs/02 §6)
    #[must_use]
    pub fn sanitized(self) -> Self {
        let coord = |v: f32| {
            if v.is_finite() {
                v.clamp(-WORLD_LIMIT, WORLD_LIMIT)
            } else {
                0.0
            }
        };
        let extent = |v: f32| {
            if v.is_finite() {
                v.clamp(MIN_ITEM_SIZE, WORLD_LIMIT)
            } else {
                DEFAULT_ITEM_SIZE
            }
        };
        Self {
            pos: Vec2::new(coord(self.pos.x), coord(self.pos.y)),
            size: Vec2::new(extent(self.size.x), extent(self.size.y)),
            rotation: if self.rotation.is_finite() {
                // ห่อเข้าช่วงเดียว ไม่งั้นการหมุนสะสมนาน ๆ จะได้เลขใหญ่จนความละเอียด
                // ของ f32 หายไป แล้วการหมุนทีละน้อยจะเริ่มกระตุกเป็นขั้น
                self.rotation.rem_euclid(std::f32::consts::TAU)
            } else {
                0.0
            },
            opacity: if self.opacity.is_finite() {
                self.opacity.clamp(0.0, 1.0)
            } else {
                1.0
            },
            crop: self.crop.sanitized(),
            filter: self.filter.sanitized(),
            ..self
        }
    }

    /// ค่าทุกตัวอยู่ในช่วงที่ปลอดภัยแล้วหรือยัง
    #[must_use]
    pub fn is_sane(self) -> bool {
        self == self.sanitized()
    }

    /// รูปทรงจริงบน canvas (หมุนแล้ว) — **ตัวที่ hit-test ใช้ตัดสิน**
    ///
    /// `flip` ไม่มีผลกับรูปทรง มันสลับแค่ทิศการอ่าน texture
    #[must_use]
    pub fn obb(self) -> Obb {
        Obb {
            center: self.pos,
            half_size: self.size * 0.5,
            rotation: self.rotation,
        }
    }

    /// กรอบแนวแกนที่คลุมรูปทรงจริง — ด่านหยาบของ culling และคีย์ของ `SpatialIndex`
    #[must_use]
    pub fn world_bounds(self) -> Rect {
        self.obb().aabb()
    }
}

// ---------------------------------------------------------------------------
// ItemMeta
// ---------------------------------------------------------------------------

/// สถานะฝั่ง Arrange mode — แท็ก/เรตติ้ง/ป้ายสี/โน้ต
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemMeta {
    /// แท็ก — `SmallVec` เพราะภาพส่วนใหญ่มีไม่เกิน 4 แท็ก จึงไม่ต้อง alloc เลย
    pub tags: smallvec::SmallVec<[TagId; 4]>,
    /// ดาว 0..=5
    pub rating: u8,
    /// ป้ายสี
    pub color_label: Option<ColorLabel>,
    /// กลุ่มที่สังกัด
    pub group: Option<GroupId>,
    /// โน้ตของผู้ใช้
    pub note: String,
    /// เวลาที่เพิ่มเข้ามา (unix millis)
    pub added_at: i64,
    /// ปักหมุด — arrange จะไม่ย้ายภาพที่ปักไว้
    pub pinned: bool,
}

impl ItemMeta {
    /// เพดานดาว
    pub const MAX_RATING: u8 = 5;

    /// clamp ค่าที่มาจากไฟล์ (I-4)
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.rating = self.rating.min(Self::MAX_RATING);
        self
    }
}

// ---------------------------------------------------------------------------
// Item
// ---------------------------------------------------------------------------

/// ภาพต้นทางหนึ่งใบที่ item อ้างถึง
///
/// ภาพเดียวกันวางซ้ำ 10 ครั้ง = 10 `Item` แต่ **1 `AssetRef` 1 texture** เพราะคีย์
/// คือ `hash` ไม่ใช่ `path` (docs/02 §2.3)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRef {
    /// blake3-256 ของไฟล์ต้นฉบับ — **คีย์หลัก**
    pub hash: ContentHash,
    /// เส้นทางล่าสุดที่เจอ — เป็นแค่ hint ไฟล์ย้ายแล้วยังหาเจอด้วย hash
    pub path: PathBuf,
    /// ขนาดจริงหลังแก้ EXIF orientation แล้ว
    pub px_size: UVec2,
    /// ชนิดไฟล์
    pub format: ImageFormat,
    /// ตัวไฟล์ฝังอยู่ใน `.refx` หรือไม่ (packed mode — P4-5)
    pub embedded: bool,
}

/// โน้ตข้อความบน canvas (P2-11)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextNote {
    /// เนื้อความ
    pub text: String,
}

/// item บน board เป็นอะไรได้บ้าง
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    /// ภาพ
    Image(AssetRef),
    /// โน้ตข้อความ
    Text(TextNote),
    /// ★ ภาพที่โหลดไม่สำเร็จ — **ยังอยู่ใน board ไม่หายไปไหน** ผู้ใช้ relink ได้
    ///
    /// ถ้าลบทิ้งเงียบ ๆ ผู้ใช้ที่ถอดฮาร์ดดิสก์ออกแล้วเปิดไฟล์จะพบว่างานหายไปครึ่ง board
    /// ซึ่งเป็นสิ่งที่ I-3 ห้ามตรง ๆ
    Missing {
        /// path เดิม — ใช้เดา/ค้นหาไฟล์ตอน relink
        original_path: PathBuf,
        /// เหตุผลที่เปิดไม่ได้ (สำหรับให้ `refx-ui` แปลเป็นข้อความ)
        reason: MissingReason,
    },
}

/// หนึ่ง item บน board
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// คีย์ของตัวเอง — สะดวกตอนส่งต่อ `&Item` โดยไม่ต้องพก id คู่ไปด้วย
    pub id: ItemId,
    /// เป็นภาพ/ข้อความ/ของที่หายไป
    pub kind: ItemKind,
    /// ★ สถานะฝั่ง Canvas
    pub canvas: ItemCanvas,
    /// ★ สถานะฝั่ง Arrange — อยู่คู่กับ `canvas` เสมอ สลับโหมดแล้วไม่หาย
    pub meta: ItemMeta,
}

impl Item {
    /// สร้าง item ใหม่จาก kind — `id` ถูกเติมให้ตอนใส่ลง board
    #[must_use]
    pub fn new(kind: ItemKind) -> Self {
        Self {
            // ค่าชั่วคราว: `Board::insert_item` เขียนทับด้วยคีย์จริงเสมอ
            id: ItemId::from_parts(0, 0),
            kind,
            canvas: ItemCanvas::default(),
            meta: ItemMeta::default(),
        }
    }

    /// ตั้งตำแหน่ง/ขนาดเริ่มต้นบน canvas
    #[must_use]
    pub fn at(mut self, pos: Vec2, size: Vec2) -> Self {
        self.canvas.pos = pos;
        self.canvas.size = size;
        self.canvas = self.canvas.sanitized();
        self
    }
}

// ---------------------------------------------------------------------------
// Group / ArrangeState / BoardSettings
// ---------------------------------------------------------------------------

/// กลุ่มของ item ในโหมด Arrange
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Group {
    /// ชื่อกลุ่มที่ผู้ใช้ตั้ง
    pub name: String,
    /// ยุบอยู่หรือไม่
    pub collapsed: bool,
}

/// วิธีเรียงในโหมด Arrange
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// ตามเวลาที่เพิ่ม
    #[default]
    AddedAt,
    /// ตามชื่อไฟล์
    Name,
    /// ตามดาว
    Rating,
}

/// สถานะของโหมด Arrange
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArrangeState {
    /// เรียงด้วยอะไร
    pub sort: SortKey,
    /// กลับลำดับ
    pub descending: bool,
}

/// ตั้งค่าระดับ board
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoardSettings {
    /// สีพื้นหลัง (linear RGB)
    pub background: [f32; 3],
    // ★ **เคยมี `snap: f32` ตรงนี้ — ลบทิ้งแล้ว (P2-9)**
    //
    // มันถูกนิยามว่าเป็น "ระยะ snap เข้ากริด เป็น world units" และเป็น `0.0`
    // มาตลอดเพราะไม่มีใครเซ็ต · P2-5 ตัดสินไปแล้วว่า **การลากย้ายไม่มี snap เข้ากริด**
    // โดยตั้งใจ (HANDOFF §2.4) และ P2-9 ทำสิ่งที่นักวาดอยากได้จริงแทน คือ
    // **ไกด์ที่ snap เข้าขอบและกึ่งกลางของภาพอื่น**
    //
    // ระยะของไกด์นั้นเป็น **พิกเซลบนจอ** (คงที่ทุกระดับซูม เหมือน `drag_threshold`)
    // ไม่ใช่ world units และเป็นค่าความรู้สึกของเครื่องมือ ไม่ใช่ข้อมูลของเอกสาร
    // จึงอยู่กับค่าคงที่ของ interaction ไม่ใช่ใน `BoardSettings`
    //
    // เก็บฟิลด์ที่ชื่อและหน่วยไม่ตรงกับสิ่งที่ระบบทำจริงไว้ = กับดักของคนอ่านรอบหน้า
}

impl Default for BoardSettings {
    fn default() -> Self {
        Self {
            // เทากลาง: พื้นขาวทำให้ประเมินค่าน้ำหนักของภาพผิด ซึ่งเป็นสิ่งที่
            // นักวาดใช้ board นี้ทำอยู่ทุกวัน
            background: [0.13, 0.13, 0.14],
        }
    }
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

/// แก้ board ไม่สำเร็จ
///
/// ★ ทุก variant ต้องหมายถึง "**ไม่มีอะไรเปลี่ยน**" — `Command` ที่ล้มกลางคัน
/// ห้ามทิ้ง board ไว้ครึ่ง ๆ กลาง ๆ (I-3)
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BoardError {
    /// อ้าง item ที่ไม่มีอยู่ (หรือ id ตายไปแล้ว)
    #[error("no such item: {id:?}")]
    NoSuchItem {
        /// id ที่หาไม่เจอ
        id: ItemId,
    },
    /// ใส่ item กลับที่เดิมไม่ได้เพราะช่องมีคนอยู่
    #[error(transparent)]
    Arena(#[from] crate::arena::ArenaError),
}

/// กระดานหนึ่งใบ — หน่วยที่ผู้ใช้เรียกว่า "งานของฉัน"
///
/// ★ ตัวที่แก้สถานะเป็น `pub(crate)` ทั้งหมด: ชั้นบนอ่านผ่าน accessor ได้ แต่เขียน
/// ต้องผ่าน `Command` เท่านั้น (I-3, docs/08 §4 ข้อ 10) — `&mut Board` ไม่หลุดออกไปที่อื่น
#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    /// คีย์ของตัวเองใน workspace
    pub id: BoardId,
    pub(crate) name: String,
    pub(crate) items: Arena<ItemId, Item>,
    /// ล่างสุด → บนสุด **source of truth ของ z**
    pub(crate) z_order: Vec<ItemId>,
    pub(crate) groups: Arena<GroupId, Group>,
    /// ★ **`selection` ไม่อยู่ที่นี่โดยตั้งใจ** (docs/02 §2.9) — มันเป็นสถานะชั่วคราว
    /// ของ editor ไม่ใช่ของเอกสาร ถ้าอยู่ใน `Board` การคลิกดูภาพเฉย ๆ จะทำให้
    /// เอกสาร dirty แล้วผู้ใช้จะโดนถาม "บันทึกไหม" ทั้งที่ไม่ได้แก้อะไร
    ///
    /// `view` **อยู่** ที่นี่และ persist ลงไฟล์ (เปิดมาแล้วกลับมุมมองเดิม)
    /// แต่เป็น **ข้อยกเว้นที่ตั้งใจ**: ไม่ผ่าน `Command` และไม่ทำให้ `dirty`
    /// — ลาก pan 200 เฟรมแล้วกด Ctrl+Z ต้องย้อนการแก้ครั้งล่าสุด ไม่ใช่ย้อนกล้อง
    /// (docs/08 §4 — มีข้อยกเว้นสองข้อนี้เท่านั้น เจอข้อที่สามให้หยุดถาม)
    pub(crate) view: ViewState,
    pub(crate) arrange: ArrangeState,
    pub(crate) settings: BoardSettings,
    pub(crate) dirty: bool,
}

impl Default for Board {
    fn default() -> Self {
        Self::new(BoardId::from_parts(0, 0), "Board 1")
    }
}

impl Board {
    /// board ว่างเปล่า
    #[must_use]
    pub fn new(id: BoardId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            items: Arena::new(),
            z_order: Vec::new(),
            groups: Arena::new(),
            view: ViewState::default(),
            arrange: ArrangeState::default(),
            settings: BoardSettings::default(),
            dirty: false,
        }
    }

    // ---- อ่านอย่างเดียว ----

    /// ชื่อ board
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// จำนวน item
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// ว่างหรือไม่
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// อ่าน item — `None` ถ้า id ตายไปแล้ว
    #[must_use]
    pub fn item(&self, id: ItemId) -> Option<&Item> {
        self.items.get(id)
    }

    /// item ทั้งหมดเรียง **ล่างสุด → บนสุด** (ลำดับ render)
    pub fn items_in_z_order(&self) -> impl Iterator<Item = (ItemId, &Item)> {
        self.z_order
            .iter()
            .filter_map(|&id| self.items.get(id).map(|item| (id, item)))
    }

    /// ลำดับ z ปัจจุบัน (ล่างสุด → บนสุด)
    #[must_use]
    pub fn z_order(&self) -> &[ItemId] {
        &self.z_order
    }

    /// กล้องของแต่ละโหมด
    #[must_use]
    pub fn view(&self) -> &ViewState {
        &self.view
    }

    /// สถานะโหมด Arrange
    #[must_use]
    pub fn arrange(&self) -> &ArrangeState {
        &self.arrange
    }

    /// ตั้งค่าระดับ board
    #[must_use]
    pub fn settings(&self) -> BoardSettings {
        self.settings
    }

    /// กลุ่มทั้งหมด
    #[must_use]
    pub fn groups(&self) -> &Arena<GroupId, Group> {
        &self.groups
    }

    /// มีการแก้ที่ยังไม่ได้บันทึกหรือไม่
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// ★ `z_order` กับ `items` ตรงกันเป๊ะหรือไม่ — invariant ที่ทุก `Command` ต้องรักษา
    ///
    /// "ทุก item อยู่ใน `z_order` ครั้งเดียวพอดี และไม่มีคีย์ตายค้างอยู่"
    /// ถ้าข้อนี้พัง ภาพจะหายจากจอทั้งที่ยังอยู่ใน board (หรือถูกวาดซ้ำสองครั้ง)
    /// — เทสต์ทุกตัวที่แตะ board ควรยืนยันข้อนี้หลังทำงาน
    #[must_use]
    pub fn z_order_is_consistent(&self) -> bool {
        if self.z_order.len() != self.items.len() {
            return false;
        }
        // ใช้ `HashSet` ตรวจความซ้ำเท่านั้น **ไม่ได้ไล่ลำดับจากมัน** จึงไม่กระทบ
        // ความ deterministic ที่ CLAUDE.md บังคับ
        let mut seen = std::collections::HashSet::with_capacity(self.z_order.len());
        self.z_order
            .iter()
            .all(|&id| self.items.contains(id) && seen.insert(id))
    }

    // ---- เขียน: `pub(crate)` เท่านั้น — ทางเข้าคือ `Command` ----

    /// ใส่ item ใหม่ วางไว้ **บนสุด** ของ z-order
    pub(crate) fn insert_item(&mut self, mut item: Item) -> ItemId {
        item.canvas = item.canvas.sanitized();
        item.meta = item.meta.sanitized();
        let id = self.items.insert(item);
        if let Some(slot) = self.items.get_mut(id) {
            slot.id = id;
        }
        self.z_order.push(id);
        id
    }

    /// ใส่ item กลับที่ **คีย์เดิมและชั้น z เดิม** — เส้นทางของ undo
    ///
    /// # Errors
    /// [`BoardError::Arena`] ถ้าช่องนั้นมีคนอยู่ — board ไม่ถูกแตะเลย
    pub(crate) fn restore_item(
        &mut self,
        id: ItemId,
        mut item: Item,
        z_index: usize,
    ) -> Result<(), BoardError> {
        item.id = id;
        self.items.insert_at(id, item)?;
        self.z_order.insert(z_index.min(self.z_order.len()), id);
        Ok(())
    }

    /// เอา item ออก คืนตัวมันกับชั้น z เดิม (ไว้ให้ undo ใส่กลับ)
    pub(crate) fn remove_item(&mut self, id: ItemId) -> Option<(Item, usize)> {
        let z_index = self.z_order.iter().position(|&other| other == id)?;
        let item = self.items.remove(id)?;
        self.z_order.remove(z_index);
        Some((item, z_index))
    }

    /// แก้ `ItemCanvas` คืนค่าเดิม — ค่าใหม่ถูก sanitize ให้เสมอ
    ///
    /// # Errors
    /// [`BoardError::NoSuchItem`] ถ้า id ตายไปแล้ว **โดยไม่แตะอะไรเลย**
    pub(crate) fn set_canvas(
        &mut self,
        id: ItemId,
        canvas: ItemCanvas,
    ) -> Result<ItemCanvas, BoardError> {
        let item = self
            .items
            .get_mut(id)
            .ok_or(BoardError::NoSuchItem { id })?;
        Ok(std::mem::replace(&mut item.canvas, canvas.sanitized()))
    }

    /// แก้ `ItemMeta` คืนค่าเดิม
    ///
    /// # Errors
    /// [`BoardError::NoSuchItem`] ถ้า id ตายไปแล้ว
    pub(crate) fn set_meta(&mut self, id: ItemId, meta: ItemMeta) -> Result<ItemMeta, BoardError> {
        let item = self
            .items
            .get_mut(id)
            .ok_or(BoardError::NoSuchItem { id })?;
        Ok(std::mem::replace(&mut item.meta, meta.sanitized()))
    }

    /// เขียน z-order ทั้งชุด คืนของเดิม
    ///
    /// เก็บทั้งชุดแทน diff ตามที่ docs/02 §3 กำหนด — ถูกกว่าและไม่มีบั๊ก
    pub(crate) fn set_z_order(&mut self, order: Vec<ItemId>) -> Vec<ItemId> {
        std::mem::replace(&mut self.z_order, order)
    }

    /// ทำเครื่องหมายว่ามีการแก้ที่ยังไม่ได้บันทึก
    pub(crate) fn mark_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp
    )]

    use super::*;

    pub(crate) fn image_item(tag: u8) -> Item {
        Item::new(ItemKind::Image(AssetRef {
            hash: ContentHash::from_bytes([tag; 32]),
            path: PathBuf::from(format!("{tag}.png")),
            px_size: UVec2::new(100, 80),
            format: ImageFormat::Png,
            embedded: false,
        }))
    }

    #[test]
    fn inserting_fills_in_the_id_and_puts_the_item_on_top() {
        let mut board = Board::default();
        let a = board.insert_item(image_item(1));
        let b = board.insert_item(image_item(2));

        assert_eq!(board.item(a).unwrap().id, a, "item ต้องรู้จัก id ของตัวเอง");
        assert_eq!(board.z_order(), &[a, b], "ตัวใหม่ต้องอยู่บนสุด");
        assert!(board.z_order_is_consistent());
        assert_eq!(board.len(), 2);
    }

    /// ★ ลบแล้ว z-order ต้องตามไปด้วย ไม่ทิ้ง id ตายค้างไว้
    ///
    /// (`selection` ไม่อยู่ใน `Board` แล้ว — ชั้น editor เป็นคนล้าง id ที่ตาย
    /// ออกจากการเลือกเอง ดู docs/02 §2.9)
    #[test]
    fn removing_cleans_up_the_z_order() {
        let mut board = Board::default();
        let a = board.insert_item(image_item(1));
        let b = board.insert_item(image_item(2));

        let (item, z_index) = board.remove_item(a).unwrap();
        assert_eq!(item.id, a);
        assert_eq!(z_index, 0);
        assert_eq!(board.z_order(), &[b]);
        assert!(board.z_order_is_consistent());
    }

    /// ★ หัวใจของ I-3 ที่ระดับ board: undo ของการลบต้องคืน **ทั้ง id และชั้น z**
    #[test]
    fn restoring_puts_the_item_back_with_the_same_id_and_depth() {
        let mut board = Board::default();
        let a = board.insert_item(image_item(1));
        let b = board.insert_item(image_item(2));
        let c = board.insert_item(image_item(3));

        let (item, z_index) = board.remove_item(b).unwrap();
        assert_eq!(board.z_order(), &[a, c]);

        board.restore_item(b, item, z_index).unwrap();
        assert_eq!(board.z_order(), &[a, b, c], "ต้องกลับมาอยู่ชั้นเดิม");
        assert_eq!(board.item(b).unwrap().id, b, "ต้องเป็น id เดิมเป๊ะ");
        assert!(board.z_order_is_consistent());
    }

    #[test]
    fn removing_an_id_twice_is_a_no_op_not_a_panic() {
        let mut board = Board::default();
        let a = board.insert_item(image_item(1));
        assert!(board.remove_item(a).is_some());
        assert!(board.remove_item(a).is_none());
        assert!(board.z_order_is_consistent());
    }

    #[test]
    fn setting_canvas_on_a_dead_id_changes_nothing() {
        let mut board = Board::default();
        let a = board.insert_item(image_item(1));
        board.remove_item(a);
        let before = board.clone();

        let err = board.set_canvas(a, ItemCanvas::default()).unwrap_err();
        assert_eq!(err, BoardError::NoSuchItem { id: a });
        assert_eq!(board, before, "คืน Err แล้ว board ต้องไม่ขยับเลย");
    }

    // ---------- I-4: ค่าจากไฟล์เชื่อไม่ได้ ----------

    /// ★ `NaN` ที่หลุดเข้า shader = ภาพหายทั้งจอโดยไม่มี error ที่ไหนเลย
    #[test]
    fn insane_canvas_values_are_clamped_on_the_way_in() {
        let mut board = Board::default();
        let mut item = image_item(1);
        item.canvas = ItemCanvas {
            pos: Vec2::new(f32::NAN, 9e9),
            size: Vec2::new(-5.0, f32::INFINITY),
            rotation: f32::NAN,
            opacity: 42.0,
            crop: CropRect {
                min: Vec2::new(0.8, f32::NAN),
                max: Vec2::new(0.2, 2.0),
            },
            filter: ItemFilter {
                brightness: f32::NEG_INFINITY,
                contrast: 7.0,
                ..ItemFilter::default()
            },
            ..ItemCanvas::default()
        };

        let id = board.insert_item(item);
        let canvas = board.item(id).unwrap().canvas;

        assert_eq!(canvas.pos.x, 0.0, "NaN → 0");
        assert_eq!(canvas.pos.y, WORLD_LIMIT, "เกินเพดาน → clamp");
        assert_eq!(canvas.size.x, MIN_ITEM_SIZE, "ขนาดติดลบ → ขนาดเล็กสุด");
        assert_eq!(
            canvas.size.y, DEFAULT_ITEM_SIZE,
            "inf → ค่ากลางที่ผู้ใช้เห็นและแก้ได้"
        );
        assert_eq!(canvas.rotation, 0.0);
        assert_eq!(canvas.opacity, 1.0);
        assert!(canvas.crop.max.x >= canvas.crop.min.x, "crop ห้ามกลับด้าน");
        assert_eq!(canvas.filter.brightness, 0.0);
        assert_eq!(canvas.filter.contrast, 1.0);
        assert!(canvas.is_sane());
    }

    /// sanitize ซ้ำต้องได้ค่าเดิม — ไม่งั้น undo/redo จะค่อย ๆ ดริฟต์ทีละนิด
    /// จนสถานะ "เหมือนเดิม" กลายเป็นไม่เหมือนเดิมโดยไม่มีใครเห็น
    #[test]
    fn sanitizing_is_idempotent() {
        let wild = ItemCanvas {
            pos: Vec2::new(1e9, -1e9),
            size: Vec2::new(f32::NAN, 3.5),
            rotation: 100.0,
            opacity: -2.0,
            crop: CropRect {
                min: Vec2::new(0.9, 0.1),
                max: Vec2::new(0.3, 0.7),
            },
            ..ItemCanvas::default()
        };
        let once = wild.sanitized();
        assert_eq!(once, once.sanitized());
        assert!(once.is_sane());
    }

    #[test]
    fn rotation_is_wrapped_into_one_turn() {
        let canvas = ItemCanvas {
            rotation: std::f32::consts::TAU * 3.25,
            ..ItemCanvas::default()
        };
        let wrapped = canvas.sanitized().rotation;
        assert!(
            (0.0..std::f32::consts::TAU).contains(&wrapped),
            "ได้ {wrapped}"
        );
    }

    #[test]
    fn rating_above_five_is_clamped() {
        let meta = ItemMeta {
            rating: 200,
            ..ItemMeta::default()
        };
        assert_eq!(meta.sanitized().rating, ItemMeta::MAX_RATING);
    }

    #[test]
    fn an_inverted_crop_becomes_empty_not_negative() {
        let crop = CropRect {
            min: Vec2::new(0.7, 0.7),
            max: Vec2::new(0.2, 0.2),
        }
        .sanitized();
        assert!(crop.is_empty());
        assert!(crop.max.x >= crop.min.x && crop.max.y >= crop.min.y);
    }

    // ---------- ดีไซน์สองโหมด ----------

    /// ★ ข้อกำหนดหลักของ ARCHITECTURE §4: แก้ฝั่งหนึ่งห้ามลบข้อมูลอีกฝั่ง
    #[test]
    fn canvas_and_meta_live_side_by_side_for_the_whole_life_of_an_item() {
        let mut board = Board::default();
        let id = board.insert_item(image_item(1));

        let meta = ItemMeta {
            tags: smallvec::smallvec![TagId(7), TagId(9)],
            rating: 4,
            color_label: Some(ColorLabel::Blue),
            note: "ท่ายืน".to_owned(),
            pinned: true,
            ..ItemMeta::default()
        };
        board.set_meta(id, meta.clone()).unwrap();

        // แก้ฝั่ง canvas ล้วน ๆ
        board
            .set_canvas(
                id,
                ItemCanvas {
                    pos: Vec2::new(50.0, 60.0),
                    ..ItemCanvas::default()
                },
            )
            .unwrap();

        assert_eq!(board.item(id).unwrap().meta, meta, "ข้อมูลฝั่ง Arrange หายไป");
        assert_eq!(board.item(id).unwrap().canvas.pos, Vec2::new(50.0, 60.0));
    }

    /// ภาพที่โหลดไม่ได้ต้อง **ยังอยู่บน board** พร้อม path เดิมไว้ relink
    #[test]
    fn a_missing_image_stays_on_the_board() {
        let mut board = Board::default();
        let id = board.insert_item(Item::new(ItemKind::Missing {
            original_path: PathBuf::from("D:/ref/แมว.png"),
            reason: MissingReason::FileNotFound,
        }));

        assert_eq!(board.len(), 1);
        match &board.item(id).unwrap().kind {
            ItemKind::Missing {
                original_path,
                reason,
            } => {
                assert_eq!(*reason, MissingReason::FileNotFound);
                assert!(original_path.ends_with("แมว.png"), "ต้องเก็บ path ไว้ relink");
            }
            other => panic!("ต้องเป็น Missing แต่ได้ {other:?}"),
        }
    }

    /// ★ ต้องมี `Unknown` — ไฟล์จากรุ่นใหม่กว่ามีเหตุผลที่รุ่นนี้ไม่รู้จักได้
    /// อ่านเจอแล้วต้องตกมาที่นี่ ห้ามทิ้งทั้งไฟล์ (I-3)
    #[test]
    fn missing_reason_has_a_landing_spot_for_values_from_newer_versions() {
        let mut board = Board::default();
        board.insert_item(Item::new(ItemKind::Missing {
            original_path: PathBuf::new(),
            reason: MissingReason::Unknown,
        }));
        assert_eq!(board.len(), 1);
        assert_ne!(MissingReason::Unknown, MissingReason::FileNotFound);
    }

    /// ตัวตรวจ invariant ต้องจับได้จริงทั้งสองทิศ ไม่ใช่คืน `true` เสมอ
    /// (docs/08 §3.9 ข้อ 1 — negative control)
    #[test]
    fn z_order_inconsistency_is_actually_detectable() {
        let mut board = Board::default();
        let a = board.insert_item(image_item(1));
        assert!(board.z_order_is_consistent());

        board.z_order.push(a);
        assert!(!board.z_order_is_consistent(), "ซ้ำแล้วต้องจับได้");

        board.z_order.clear();
        assert!(!board.z_order_is_consistent(), "ขาดแล้วต้องจับได้");
    }
}
