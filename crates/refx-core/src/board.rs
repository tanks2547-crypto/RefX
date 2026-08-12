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
///
/// ★★★ **ออกแบบให้ round-trip ได้ตั้งแต่ต้น ไม่ใช่รอไปแก้ตอน P4-1**
///
/// `docs/02 §2.9` บันทึกเคสนี้ไว้เป็นตัวอย่างหลักของกฎ "ทนได้ยังไม่พอ
/// ต้องส่งคืนค่าเดิมได้":
///
/// > ผู้ใช้ติดป้ายสีด้วย RefX รุ่นใหม่ → เปิดด้วยรุ่นเก่า → รุ่นเก่าอ่านค่าไม่รู้จัก
/// > เป็น `None` → ผู้ใช้ขยับภาพใบเดียวแล้วบันทึก → **ป้ายสีหายถาวร**
///
/// กลไกที่สร้างมากัน I-3 กลายเป็นตัวทำให้ข้อมูลหายเสียเอง · ทางแก้คือ
/// **เก็บค่าดิบไว้แล้วเขียนกลับตามเดิม** ซึ่งที่นี่ทำด้วย [`ColorLabel::Unknown`]
///
/// ★ เก็บไว้ใน **ชนิดของโดเมนเอง** ไม่ใช่ใน DTO ตอน P4-1 เพราะถ้าอยู่ใน DTO
/// การ "ลืมเขียนกลับ" เป็นไปได้เสมอ · อยู่ตรงนี้แล้วมันเดินทางไปพร้อม `ItemMeta`
/// ทุกที่โดยไม่ต้องมีใครจำ — และ [`ColorLabel::to_wire`] คือทางเดียวที่ค่าจะออกไปลงไฟล์
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
    /// ★ ป้ายสีที่รุ่นนี้ไม่รู้จัก — **ถือค่าดิบไว้เพื่อเขียนกลับให้เหมือนเดิม**
    ///
    /// UI แสดงเป็น "ไม่รู้จัก" ได้ แต่ **ห้ามแปลงค่าทิ้ง** และผู้ใช้เลือกมันเองไม่ได้
    /// (ไม่มีปุ่มไหนสร้างค่านี้ — มันมาจากไฟล์ทางเดียว)
    ///
    /// `NonZeroU8` เพราะ **0 คือ "ไม่มีป้าย"** ซึ่งแทนด้วย `None` อยู่แล้ว
    /// ถ้าปล่อยให้เป็น `u8` ธรรมดา `Unknown(0)` จะเขียนออกไปเป็น "ไม่มีป้าย"
    /// แล้วป้ายก็หายอยู่ดี — ชนิดข้อมูลปิดรูนั้นแทนที่จะต้องมีคนจำ
    Unknown(std::num::NonZeroU8),
}

impl ColorLabel {
    /// ป้ายทุกสีที่ผู้ใช้ **เลือกได้จริง** — เรียงตามลำดับที่แสดงบน UI
    ///
    /// ไม่มี [`ColorLabel::Unknown`] อยู่ในนี้โดยตั้งใจ
    pub const CHOICES: [Self; 6] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Purple,
    ];

    /// ค่าที่ลงไฟล์ — **ตัวเลขพวกนี้เป็นสัญญาถาวร ห้ามสลับ**
    ///
    /// `0` สงวนไว้ให้ "ไม่มีป้าย" (`None`) จึงไม่มีสีไหนใช้
    #[must_use]
    pub fn to_wire(self) -> u8 {
        match self {
            Self::Red => 1,
            Self::Orange => 2,
            Self::Yellow => 3,
            Self::Green => 4,
            Self::Blue => 5,
            Self::Purple => 6,
            Self::Unknown(raw) => raw.get(),
        }
    }

    /// อ่านค่าจากไฟล์ — `None` = ไม่มีป้าย · ค่าที่ไม่รู้จักตกที่ [`ColorLabel::Unknown`]
    ///
    /// ★ **ไม่มีทางคืน `None` เพราะ "ไม่รู้จัก"** — `None` แปลว่า "ไม่มีป้าย" เท่านั้น
    /// สองอย่างนี้ต่างกัน และการรวมมันเข้าด้วยกันคือบั๊กที่ทำให้ข้อมูลหาย
    #[must_use]
    pub fn from_wire(value: u8) -> Option<Self> {
        Some(match value {
            0 => return None,
            1 => Self::Red,
            2 => Self::Orange,
            3 => Self::Yellow,
            4 => Self::Green,
            5 => Self::Blue,
            6 => Self::Purple,
            // ค่าที่เหลือมาจากรุ่นใหม่กว่า — ถือไว้ให้ครบแล้วเขียนกลับตามเดิม
            other => Self::Unknown(std::num::NonZeroU8::new(other)?),
        })
    }

    /// รุ่นนี้รู้จักป้ายนี้ไหม — UI ใช้ตัดสินว่าจะวาดสีจริงหรือวาดว่า "ไม่รู้จัก"
    #[must_use]
    pub fn is_known(self) -> bool {
        !matches!(self, Self::Unknown(_))
    }

    /// สี RGB สำหรับวาดบน UI — `None` เมื่อรุ่นนี้ไม่รู้จักป้ายนี้
    ///
    /// ★ คืน `None` แทนที่จะเดาสีเทา ๆ ให้ — ชั้น UI ต้องเป็นคนตัดสินว่าจะ
    /// แสดงยังไง และต้องแสดงให้ **ต่างจากป้ายที่รู้จัก** ไม่งั้นผู้ใช้จะคิดว่า
    /// มันเป็นสีจริงแล้วเผลอกดทับ
    #[must_use]
    pub fn rgb(self) -> Option<[u8; 3]> {
        Some(match self {
            Self::Red => [220, 76, 70],
            Self::Orange => [226, 140, 60],
            Self::Yellow => [222, 200, 70],
            Self::Green => [110, 190, 110],
            Self::Blue => [92, 150, 230],
            Self::Purple => [170, 120, 220],
            Self::Unknown(_) => return None,
        })
    }
}

/// คีย์ของแท็ก — ชื่อจริงอยู่ใน [`TagTable`] ของ board
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TagId(pub u32);

/// ตารางชื่อแท็กของ board หนึ่งใบ (P3-1)
///
/// ★★ **ทำไมอยู่ที่ `Board` ไม่ใช่ `Workspace`** — คอมเมนต์เดิมของ [`TagId`] เขียนว่า
/// "ตารางชื่อแท็กจริงอยู่ระดับ workspace" แต่ `Workspace` ใน `docs/02 §2` มีแค่
/// `boards` / `active` / `order` **ไม่มีตารางแท็ก** และ `Workspace` เองก็ยังไม่ถูก
/// สร้าง (P4-7) — ที่อยู่ของตารางนี้จึงไม่เคยถูกระบุจริง
///
/// เลือก `Board` เพราะ **`Board` คือสิ่งที่ลงไฟล์ `.refx`** (P4-1): ส่งไฟล์ให้เพื่อน
/// แล้วชื่อแท็กไปด้วย · ถ้าตารางอยู่ระดับแอป ไฟล์ที่ส่งไปจะมีแต่ `TagId` เปล่า ๆ
/// ที่ไม่มีความหมาย = **ข้อมูลผู้ใช้หายตอนส่งต่อ** ซึ่งเป็นรูปแบบเดียวกับ I-3
///
/// ตอน P4-7 ทำ multi-board จะเพิ่มตารางระดับ workspace ไว้ *รวม* ชื่อข้าม board ได้
/// — ทิศ board → workspace เป็นการเพิ่ม ส่วน workspace → board ต้อง migrate ไฟล์
///
/// ★ ใช้ `BTreeMap` ไม่ใช่ `HashMap` — **ลำดับต้อง deterministic** (CLAUDE.md)
/// รายการแท็กบน UI ที่สลับที่ทุกครั้งที่เปิดคือสิ่งที่ผู้ใช้อ่านว่าโปรแกรมพัง
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagTable {
    names: std::collections::BTreeMap<TagId, String>,
    /// id ถัดไปที่จะแจก — **ไม่เคยถอยหลัง** แม้แท็กจะถูกลบ
    ///
    /// ★ ถ้าใช้ id ซ้ำ item ที่ยังถือ id เดิมอยู่จะกลายเป็นแท็กใหม่เงียบ ๆ
    /// (เหตุผลเดียวกับที่ `Arena` เป็น generational — HANDOFF §4 ข้อ 19)
    next: u32,
}

/// เพดานความยาวชื่อแท็ก (อักขระ) — ค่าจากไฟล์ต้องถูกตัดก่อนเข้ามา (I-4)
pub const MAX_TAG_LEN: usize = 64;

impl TagTable {
    /// ชื่อของแท็กนี้ — `None` ถ้าไม่มีในตาราง
    #[must_use]
    pub fn name(&self, id: TagId) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }

    /// ทุกแท็กในตาราง เรียงตาม id (deterministic เสมอ)
    pub fn iter(&self) -> impl Iterator<Item = (TagId, &str)> {
        self.names.iter().map(|(id, name)| (*id, name.as_str()))
    }

    /// จำนวนแท็ก
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// ตารางว่างไหม
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// ทำให้ชื่อเป็นรูปแบบมาตรฐาน — ตัดช่องว่างหัวท้าย + จำกัดความยาว
    ///
    /// คืน `None` เมื่อชื่อว่างเปล่าหลังตัดแล้ว (แท็กชื่อว่างคือแท็กที่กดไม่โดน)
    #[must_use]
    pub fn normalize(name: &str) -> Option<String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }
        // ★ ตัดตาม **อักขระ** ไม่ใช่ไบต์ — ตัดกลาง UTF-8 จะ panic (I-4)
        //   ชื่อไทย/ญี่ปุ่นยาว 64 อักขระเป็นเรื่องปกติ
        Some(trimmed.chars().take(MAX_TAG_LEN).collect())
    }

    /// หา id ของชื่อนี้ถ้ามีอยู่แล้ว — **เทียบแบบไม่สนตัวพิมพ์**
    ///
    /// ★ `Portrait` กับ `portrait` เป็นแท็กเดียวกัน: ผู้ใช้พิมพ์เองทุกครั้ง
    /// การได้แท็กสองอันที่หน้าตาเหมือนกันคือกับดักที่ทำให้ filter หาไม่เจอ
    #[must_use]
    pub fn find(&self, name: &str) -> Option<TagId> {
        let wanted = Self::normalize(name)?;
        self.names
            .iter()
            .find(|(_, existing)| existing.eq_ignore_ascii_case(&wanted))
            .map(|(id, _)| *id)
    }

    /// เพิ่มชื่อใหม่แล้วคืน id — `None` ถ้าชื่อใช้ไม่ได้
    ///
    /// **ไม่เช็คว่าซ้ำ** ผู้เรียกต้องถาม [`TagTable::find`] ก่อน (ตัวที่เรียกจริงคือ
    /// `Command` ซึ่งต้องรู้ด้วยว่า "สร้างใหม่หรือเปล่า" เพื่อจะ undo ได้ถูก)
    pub(crate) fn insert(&mut self, name: &str) -> Option<TagId> {
        let name = Self::normalize(name)?;
        let id = TagId(self.next);
        self.next = self.next.checked_add(1)?;
        self.names.insert(id, name);
        Some(id)
    }

    /// ใส่ชื่อกลับที่ id เดิม — ใช้ตอน undo การลบ
    pub(crate) fn restore(&mut self, id: TagId, name: String) {
        self.next = self.next.max(id.0.saturating_add(1));
        self.names.insert(id, name);
    }

    /// เอาแท็กออกจากตาราง คืนชื่อเดิม — ใช้ตอน undo การสร้าง
    pub(crate) fn remove(&mut self, id: TagId) -> Option<String> {
        self.names.remove(&id)
    }
}

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
    /// ★ เวลาที่ไฟล์ต้นฉบับถูกแก้ครั้งล่าสุด (unix **millis**) ตอนที่ ingest
    ///
    /// ★★ **ไม่ได้อ่านดิสก์เพิ่มเพื่อค่านี้** — มันถูกคำนวณอยู่แล้วตอน ingest
    /// เพราะเป็นส่วนหนึ่งของ cache key `(hash, mtime, size)` (docs/02 §2.9 ข้อ 4)
    /// เดิมถูกทิ้งหลังใช้เสร็จ ทำให้ sort "วันที่แก้ไข" ของ docs/03 §3 ทำไม่ได้
    /// ทั้งที่ข้อมูลอยู่ในมือแล้ว
    ///
    /// `0` = ไม่รู้ (ภาพจาก clipboard ไม่มีไฟล์ · stat ล้มเหลว)
    ///
    /// > เป็นสภาพ **ณ ตอน ingest** ไฟล์ที่ถูกแก้ทีหลังจะไม่ตรง — ยอมรับได้สำหรับ
    /// > การเรียงบน mood board และ cache key ตรวจซ้ำตอนโหลดอยู่แล้ว (docs/02 §2.3)
    pub mtime: i64,
    /// ขนาดไฟล์ต้นฉบับเป็นไบต์ ตอนที่ ingest — `0` = ไม่รู้ (ดู [`AssetRef::mtime`])
    pub file_size: u64,
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
///
/// ★★ **มีเท่าที่ข้อมูลรองรับจริง** — `docs/03 §3` ระบุไว้ 9 ตัว ตอนนี้ทำได้ 7
/// (`date_modified`/`file_size` ปลดล็อกแล้วตอน `AssetRef` เก็บ mtime/ขนาดไฟล์)
/// เหลือสองตัวที่ยังไม่มีข้อมูล: `dominant_hue` อยู่ใน `ItemRender` ของชั้น UI
/// ไม่ใช่ใน `Board` · `canvas_order` คือ **P3-6** ซึ่งมีอัลกอริทึมของตัวเอง
/// (docs/03 §4.2 — อ่านแบบหนังสือ + tolerance 50% ของความสูงเฉลี่ย)
///
/// ไม่ใส่ variant ที่ไม่มีทางทำงาน — กับดักของคนอ่านรอบหน้า (เหตุผลเดียวกับที่
/// P3-2 ไม่ใส่ `respect_pinned` และ P2-9 ลบ `BoardSettings::snap` ทิ้ง)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// ตามเวลาที่เพิ่ม
    #[default]
    AddedAt,
    /// ตามชื่อไฟล์
    Name,
    /// ตามดาว
    Rating,
    /// ตามป้ายสี — ไม่มีป้ายมาก่อน แล้วเรียงตามค่าบนสาย (P3-4)
    ColorLabel,
    /// ตามสัดส่วน กว้าง/สูง — แนวตั้งมาก่อนแนวนอน (P3-4)
    AspectRatio,
    /// ตามเวลาที่ **ไฟล์ต้นฉบับ** ถูกแก้ครั้งล่าสุด (P3-4)
    ///
    /// ★ คนละอย่างกับ [`SortKey::AddedAt`] ซึ่งคือเวลาที่ *ลากเข้ามาบน board*
    /// — นักวาดที่สแกนงานเก่ามาทั้งโฟลเดอร์จะได้ลำดับต่างกันคนละเรื่อง
    ModifiedAt,
    /// ตามขนาดไฟล์ (ไบต์)
    FileSize,
    /// ★ ตามตำแหน่งบน canvas — อ่านแบบหนังสือ บน→ล่าง ซ้าย→ขวา (P3-6)
    ///
    /// ★★ ตัวเดียวในกลุ่มที่ **ไม่ใช่การเปรียบเทียบรายคู่** — การตัดสินว่าสองใบ
    /// อยู่แถวเดียวกันไหมต้องรู้ความสูงเฉลี่ยของทั้งชุดก่อน (docs/03 §4.2)
    /// จึงมีเส้นทางของตัวเองใน `query::select` ไม่ได้อยู่ใน `compare`
    CanvasOrder,
}

impl SortKey {
    /// ★ ทุกวิธีเรียงที่มี — **ชั้น UI ต้องให้ผู้ใช้เลือกได้ครบทุกตัว**
    ///
    /// วิธีเรียงที่มีใน enum แต่ไม่มีในรายการของ toolbar = ฟีเจอร์ที่ไม่มีอยู่จริง
    /// สำหรับผู้ใช้ · เทสต์ `every_sort_key_can_be_picked_from_the_toolbar`
    /// (`refx-ui`) เทียบรายการนี้กับปุ่มจริง และ `all_lists_every_sort_key`
    /// ข้างล่างบังคับว่ารายการนี้เองต้องครบ
    pub const ALL: [Self; 8] = [
        Self::AddedAt,
        Self::Name,
        Self::Rating,
        Self::ColorLabel,
        Self::AspectRatio,
        Self::ModifiedAt,
        Self::FileSize,
        Self::CanvasOrder,
    ];
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
#[derive(Debug, Clone)]
pub struct Board {
    /// คีย์ของตัวเองใน workspace
    pub id: BoardId,
    pub(crate) name: String,
    pub(crate) items: Arena<ItemId, Item>,
    /// ล่างสุด → บนสุด **source of truth ของ z**
    pub(crate) z_order: Vec<ItemId>,
    pub(crate) groups: Arena<GroupId, Group>,
    /// ★ ชื่อของแท็กทั้งหมดบน board นี้ (P3-1) — ดู [`TagTable`] ว่าทำไมอยู่ที่นี่
    pub(crate) tags: TagTable,
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
    /// ★★ นับทุกครั้งที่ **เนื้อหา** ของ board เปลี่ยน (P3-4)
    ///
    /// มีไว้ให้ชั้นบน cache ผลที่คำนวณจาก board ได้โดยไม่ต้องเทียบ board ทั้งก้อน
    /// — docs/03 §3 บังคับว่า filter ต้อง cache ผลไว้ "ตราบใดที่ `board.revision`
    /// ไม่เปลี่ยน" เพราะที่ 1000+ ภาพ การกรองใหม่ทุกเฟรมคือการเผา CPU ฟรี
    ///
    /// ★ **ไม่นับรวมใน [`PartialEq`]** โดยตั้งใจ — undo ต้องคืนสภาพให้ "เท่าเดิม"
    /// ในสายตาผู้ใช้ ส่วนเลขรุ่นเดินหน้าอย่างเดียวเสมอ ถ้านับด้วย เทสต์ที่ยืนยันว่า
    /// undo คืนสภาพได้จะไม่มีวันผ่าน (เหตุผลเดียวกับ `Arena`/`Selection` — §4 ข้อ 20)
    ///
    /// ★★ **แก้ board ที่ไหน ต้องบวกที่นั่น** — `every_mutation_bumps_the_revision`
    /// เรียกตัวแก้ทุกตัวแล้วบังคับข้อนี้ ไม่ใช่ความจำของคนเขียน
    pub(crate) revision: u64,
}

/// ★ เทียบเฉพาะสิ่งที่ผู้ใช้สัมผัสได้ — **`revision` ไม่นับ** (§4 ข้อ 20)
///
/// destructure ครบทุกฟิลด์ไม่มี `..` โดยตั้งใจ: เพิ่มฟิลด์ใหม่เมื่อไหร่ ตรงนี้
/// **คอมไพล์ไม่ผ่าน** จนกว่าจะมีคนตัดสินว่ามันเป็นส่วนหนึ่งของ "เท่าเดิม" หรือไม่
impl PartialEq for Board {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            id,
            name,
            items,
            z_order,
            groups,
            tags,
            view,
            arrange,
            settings,
            dirty,
            revision: _,
        } = self;
        id == &other.id
            && name == &other.name
            && items == &other.items
            && z_order == &other.z_order
            && groups == &other.groups
            && tags == &other.tags
            && view == &other.view
            && arrange == &other.arrange
            && settings == &other.settings
            && dirty == &other.dirty
    }
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
            tags: TagTable::default(),
            view: ViewState::default(),
            arrange: ArrangeState::default(),
            settings: BoardSettings::default(),
            dirty: false,
            revision: 0,
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

    /// ★ เลขรุ่นของ **เนื้อหา** board — เปลี่ยนทุกครั้งที่มีการแก้ (P3-4)
    ///
    /// ชั้นบนใช้เป็นคีย์ของ cache: เท่าเดิม = ไม่ต้องคำนวณใหม่ (docs/03 §3)
    /// · ห้ามใช้เดาว่า "แก้ไปกี่ครั้ง" — undo ก็บวก และค่าจะ wrap ที่ `u64::MAX`
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
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
        self.touch();
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
        self.touch();
        Ok(())
    }

    /// เอา item ออก คืนตัวมันกับชั้น z เดิม (ไว้ให้ undo ใส่กลับ)
    pub(crate) fn remove_item(&mut self, id: ItemId) -> Option<(Item, usize)> {
        let z_index = self.z_order.iter().position(|&other| other == id)?;
        let item = self.items.remove(id)?;
        self.z_order.remove(z_index);
        self.touch();
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
        let previous = std::mem::replace(&mut item.canvas, canvas.sanitized());
        self.touch();
        Ok(previous)
    }

    /// ตารางชื่อแท็กของ board นี้ — **อ่านอย่างเดียว** (P3-1)
    #[must_use]
    pub fn tags(&self) -> &TagTable {
        &self.tags
    }

    /// เพิ่มชื่อแท็กใหม่ คืน id — `None` ถ้าชื่อใช้ไม่ได้
    pub(crate) fn insert_tag(&mut self, name: &str) -> Option<TagId> {
        let id = self.tags.insert(name)?;
        self.dirty = true;
        self.touch();
        Some(id)
    }

    /// เอาแท็กออกจากตาราง คืนชื่อเดิม — ใช้ตอน undo การสร้างแท็ก
    pub(crate) fn remove_tag(&mut self, id: TagId) -> Option<String> {
        let name = self.tags.remove(id)?;
        self.dirty = true;
        self.touch();
        Some(name)
    }

    /// ใส่ชื่อแท็กกลับที่ id เดิม — ใช้ตอน redo/undo
    pub(crate) fn restore_tag(&mut self, id: TagId, name: String) {
        self.tags.restore(id, name);
        self.dirty = true;
        self.touch();
    }

    /// แก้เนื้อความของโน้ต คืนข้อความเดิม (P2-11)
    ///
    /// ★ คืน [`BoardError::NoSuchItem`] เมื่อ item **ไม่ใช่โน้ต** ด้วย ไม่ใช่แค่ตอน id ตาย
    /// — การเขียนข้อความทับภาพจะทำให้ `AssetRef` หายไปทั้งก้อน ซึ่งคือการทำงาน
    /// ของผู้ใช้หายแบบที่ I-3 ห้ามไว้ตรง ๆ
    ///
    /// # Errors
    /// [`BoardError::NoSuchItem`] ถ้า id ตายไปแล้ว หรือ item นั้นไม่ใช่ `ItemKind::Text`
    pub(crate) fn set_text(&mut self, id: ItemId, text: String) -> Result<String, BoardError> {
        let item = self
            .items
            .get_mut(id)
            .ok_or(BoardError::NoSuchItem { id })?;
        let ItemKind::Text(note) = &mut item.kind else {
            return Err(BoardError::NoSuchItem { id });
        };
        let previous = std::mem::replace(&mut note.text, text);
        self.touch();
        Ok(previous)
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
        let previous = std::mem::replace(&mut item.meta, meta.sanitized());
        self.touch();
        Ok(previous)
    }

    /// เขียน z-order ทั้งชุด คืนของเดิม
    ///
    /// เก็บทั้งชุดแทน diff ตามที่ docs/02 §3 กำหนด — ถูกกว่าและไม่มีบั๊ก
    pub(crate) fn set_z_order(&mut self, order: Vec<ItemId>) -> Vec<ItemId> {
        let previous = std::mem::replace(&mut self.z_order, order);
        self.touch();
        previous
    }

    /// ทำเครื่องหมายว่ามีการแก้ที่ยังไม่ได้บันทึก
    ///
    /// ★ **ไม่บวก `revision`** — "บันทึกแล้ว/ยังไม่บันทึก" ไม่ใช่การเปลี่ยนเนื้อหา
    /// ถ้าบวกด้วย การกด Save จะทำให้ทุก cache ที่ผูกกับ board ถูกทิ้งฟรี ๆ
    pub(crate) fn mark_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    /// ★ บวกเลขรุ่น — เรียกจากตัวแก้ทุกตัวที่เปลี่ยน **เนื้อหา** ของ board
    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
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

    // ---------- ColorLabel: round-trip (P3-1) ----------

    /// ★★★ **ค่าป้ายสีทุกค่าที่เป็นไปได้ต้องเขียนกลับได้เหมือนเดิมเป๊ะ**
    ///
    /// นี่คือเทสต์ที่ทำให้เคสใน `docs/02 §2.9` เกิดขึ้นไม่ได้: ผู้ใช้ติดป้ายด้วย
    /// รุ่นใหม่ → เปิดด้วยรุ่นเก่า → ขยับภาพใบเดียวแล้วบันทึก → **ป้ายสีหายถาวร**
    ///
    /// ไล่ครบทั้ง 256 ค่าเพราะโดเมนมันเล็กพอที่จะไล่หมดได้จริง — ไม่ต้องสุ่ม
    #[test]
    fn every_possible_colour_label_survives_a_round_trip() {
        for raw in 0..=u8::MAX {
            let parsed = ColorLabel::from_wire(raw);
            let written = parsed.map_or(0, ColorLabel::to_wire);
            assert_eq!(
                written, raw,
                "ค่า {raw} อ่านเป็น {parsed:?} แล้วเขียนกลับได้ {written} — ข้อมูลเพี้ยน"
            );
        }
    }

    /// ★★ **"ไม่มีป้าย" กับ "ป้ายที่ไม่รู้จัก" ต้องไม่ใช่สิ่งเดียวกัน**
    ///
    /// การรวมสองอย่างนี้เข้าด้วยกันคือบั๊กทั้งหมดของเคสนั้น: ถ้าค่าที่ไม่รู้จัก
    /// ตกเป็น `None` มันจะกลายเป็น "ผู้ใช้ตั้งใจไม่ติดป้าย" แล้วถูกเขียนทับด้วย 0
    #[test]
    fn an_unknown_label_is_never_confused_with_having_no_label() {
        assert_eq!(ColorLabel::from_wire(0), None, "0 = ไม่มีป้าย");
        for raw in 7..=u8::MAX {
            let parsed = ColorLabel::from_wire(raw);
            assert!(parsed.is_some(), "ค่า {raw} หายไปเป็น None");
            assert!(!parsed.unwrap().is_known(), "ค่า {raw} ไม่ควรถูกอ้างว่ารู้จัก");
        }
    }

    /// ★ ค่าบนสายเป็น **สัญญาถาวร** — สลับตัวเลขเมื่อไหร่ ไฟล์เก่าจะอ่านผิดสีทั้งหมด
    #[test]
    fn the_wire_numbers_are_pinned_forever() {
        assert_eq!(ColorLabel::Red.to_wire(), 1);
        assert_eq!(ColorLabel::Orange.to_wire(), 2);
        assert_eq!(ColorLabel::Yellow.to_wire(), 3);
        assert_eq!(ColorLabel::Green.to_wire(), 4);
        assert_eq!(ColorLabel::Blue.to_wire(), 5);
        assert_eq!(ColorLabel::Purple.to_wire(), 6);
        // ★ ทุกตัวที่ผู้ใช้เลือกได้ต้องมีเลขไม่ซ้ำกันและไม่ใช่ 0
        let mut seen: Vec<u8> = ColorLabel::CHOICES.iter().map(|c| c.to_wire()).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), ColorLabel::CHOICES.len(), "เลขบนสายซ้ำกัน");
        assert!(!seen.contains(&0), "0 สงวนไว้ให้ \"ไม่มีป้าย\"");
    }

    /// ★ ป้ายที่ผู้ใช้เลือกได้ต้องมีสีให้วาด · ป้ายที่ไม่รู้จักต้อง **ไม่มี**
    ///
    /// ถ้า `Unknown` เดาสีให้ ผู้ใช้จะคิดว่ามันเป็นสีจริงแล้วเผลอกดทับ
    /// ซึ่งคือการทำลายค่าที่เราอุตส่าห์ถือไว้
    #[test]
    fn only_labels_this_build_knows_have_a_colour() {
        for label in ColorLabel::CHOICES {
            assert!(label.is_known());
            assert!(label.rgb().is_some(), "{label:?} ไม่มีสีให้วาด");
        }
        let unknown = ColorLabel::from_wire(200).unwrap();
        assert!(unknown.rgb().is_none(), "ป้ายที่ไม่รู้จักต้องไม่เดาสีให้");
    }

    /// ★ `ItemMeta` ที่ถือป้ายไม่รู้จักต้องผ่าน `sanitized()` ไปได้โดยไม่ถูกล้าง
    ///
    /// `sanitized()` คือด่านที่ค่าจากไฟล์ทุกค่าต้องผ่าน (I-4) — ถ้ามันล้างป้าย
    /// ที่ไม่รู้จักทิ้ง เกราะที่สร้างมากันข้อมูลเสียจะกลายเป็นตัวทำข้อมูลหายเสียเอง
    #[test]
    fn sanitizing_meta_keeps_a_label_it_does_not_understand() {
        let raw = 199;
        let meta = ItemMeta {
            color_label: ColorLabel::from_wire(raw),
            rating: 99, // ตัวนี้ต้องถูก clamp
            ..ItemMeta::default()
        }
        .sanitized();
        assert_eq!(meta.rating, ItemMeta::MAX_RATING, "rating ต้องถูก clamp");
        assert_eq!(
            meta.color_label.map_or(0, ColorLabel::to_wire),
            raw,
            "ป้ายที่ไม่รู้จักต้องรอดจาก sanitize"
        );
    }

    /// ★★★ ตัวแก้ **ทุกตัว** ต้องบวก `revision` — ไม่ใช่ความจำของคนเขียน
    ///
    /// cache ของชั้นบนผูกกับเลขนี้ (docs/03 §3) · ตัวแก้ที่ลืมบวกคือ cache ที่
    /// **ค้างอยู่รุ่นเก่าโดยไม่มีอะไรส่งเสียง** — ผู้ใช้ติดดาวแล้วรายการไม่ขยับ
    /// แล้วเขาจะสรุปว่าโปรแกรมไม่รับคำสั่ง ซึ่งหาสาเหตุยากมากเพราะทุกอย่าง "ถูก"
    ///
    /// เทสต์นี้เรียกตัวแก้ทุกตัวที่มีจริง ๆ ทีละตัวแล้วเทียบเลขก่อน/หลัง
    #[test]
    fn every_mutation_bumps_the_revision() {
        let mut board = Board::default();
        let mut last = board.revision();
        let check = |board: &Board, what: &str, last: &mut u64| {
            assert!(
                board.revision() > *last,
                "{what} ไม่ได้บวก revision — cache ของชั้นบนจะค้างรุ่นเก่าเงียบ ๆ"
            );
            *last = board.revision();
        };

        let id = board.insert_item(image_item(1));
        check(&board, "insert_item", &mut last);

        board.set_canvas(id, ItemCanvas::default()).unwrap();
        check(&board, "set_canvas", &mut last);

        board.set_meta(id, ItemMeta::default()).unwrap();
        check(&board, "set_meta", &mut last);

        let order = board.z_order().to_vec();
        board.set_z_order(order);
        check(&board, "set_z_order", &mut last);

        let tag = board.insert_tag("แท็ก").unwrap();
        check(&board, "insert_tag", &mut last);
        let name = board.remove_tag(tag).unwrap();
        check(&board, "remove_tag", &mut last);
        board.restore_tag(tag, name);
        check(&board, "restore_tag", &mut last);

        let note = board.insert_item(Item::new(ItemKind::Text(TextNote::default())));
        last = board.revision();
        board.set_text(note, "ข้อความ".to_owned()).unwrap();
        check(&board, "set_text", &mut last);

        let (item, z) = board.remove_item(id).unwrap();
        check(&board, "remove_item", &mut last);
        board.restore_item(id, item, z).unwrap();
        check(&board, "restore_item", &mut last);

        // ★ ตรงข้าม: "บันทึกแล้ว" ไม่ใช่การเปลี่ยนเนื้อหา จึงต้อง **ไม่** บวก
        let before = board.revision();
        board.mark_dirty(false);
        assert_eq!(
            board.revision(),
            before,
            "mark_dirty บวก revision — การกด Save จะทิ้ง cache ทุกตัวฟรี ๆ"
        );
    }

    /// ★★ `revision` ต้องไม่ทำให้ "undo คืนสภาพเป๊ะ" เป็นไปไม่ได้
    ///
    /// เลขรุ่นเดินหน้าอย่างเดียว ถ้ามันอยู่ใน `PartialEq` ด้วย เทสต์ทุกตัวที่
    /// ยืนยันว่า undo คืนสภาพได้จะแดงทันทีและถาวร (§4 ข้อ 20)
    #[test]
    fn the_revision_is_not_part_of_being_equal() {
        let mut board = Board::default();
        let snapshot = board.clone();
        let id = board.insert_item(image_item(7));
        board.remove_item(id).unwrap();
        assert_ne!(board.revision(), snapshot.revision(), "เลขรุ่นต้องขยับ");
        assert_eq!(board, snapshot, "เนื้อหากลับมาเท่าเดิมแล้วแต่ยังไม่เท่ากัน");
    }

    /// ★★ `SortKey::ALL` ต้องครบทุก variant — บังคับด้วย `match` ที่ไม่มี `_`
    ///
    /// เพิ่ม variant ใหม่แล้ว **คอมไพล์ไม่ผ่านที่นี่** จนกว่าจะมีคนมาดู แล้วเขาจะ
    /// เห็น `ALL` อยู่ข้าง ๆ พอดี · ถ้าใช้แค่ `assert_eq!(len, 7)` การเพิ่ม variant
    /// แล้วแก้เลขให้ผ่านเป็นเรื่องที่ทำได้โดยไม่ต้องคิดอะไรเลย
    #[test]
    fn all_lists_every_sort_key() {
        for key in SortKey::ALL {
            match key {
                SortKey::AddedAt
                | SortKey::Name
                | SortKey::Rating
                | SortKey::ColorLabel
                | SortKey::AspectRatio
                | SortKey::ModifiedAt
                | SortKey::FileSize
                | SortKey::CanvasOrder => {}
            }
        }
        let mut seen = SortKey::ALL.to_vec();
        seen.dedup();
        assert_eq!(seen.len(), SortKey::ALL.len(), "มี variant ซ้ำในรายการ");
    }

    /// ใครเป็นคนเขียนค่าจริงลงฟิลด์นี้
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Writer {
        /// มีเส้นทางจริงในโปรแกรมที่เขียนค่าที่ไม่ใช่ค่าปริยายลงไป
        Real,
        /// **ยังไม่มีใครเขียนเลย** — ค่าคงเป็นค่าปริยายตลอดอายุโปรแกรม
        ///
        /// ไม่ใช่บั๊กเสมอไป (ของที่รอคิวอยู่) แต่ต้องเป็นสิ่งที่มีคนเซ็นรับรองไว้
        /// ไม่ใช่สิ่งที่ไม่มีใครรู้ — เหตุผลอยู่ในสตริง
        NoneYet(&'static str),
    }

    /// ★★★ ฟิลด์ที่ **ไม่มีโค้ดไหนเขียนค่าจริงลงไปเลย** ต้องเป็นของที่มีคนรู้
    ///
    /// รูปแบบนี้เกิดมาแล้วสามครั้งและทุกครั้งเงียบสนิท:
    ///
    /// | ฟิลด์ | อาการ |
    /// |---|---|
    /// | คอลัมน์ `format` ใน `cache.sqlite` | เขียน `0` ตายตัวมาตลอด |
    /// | `AssetRef::format` | เป็น `Unknown` เสมอ |
    /// | `ItemMeta::added_at` | เป็น `0` ทุกใบ → เรียง "เวลาที่เพิ่ม" ไม่มีความหมาย |
    ///
    /// ต่างจาก audit สองรอบก่อน (enum ที่ขาดทางออก §2.2b · ฟิลด์ที่ไม่ถึง shader
    /// §2.2d) ตรงที่ **คอมไพเลอร์ช่วยไม่ได้เลย**: ฟิลด์มีอยู่ อ่านได้ ใช้งานได้
    /// แค่ค่าที่อยู่ข้างในไม่เคยมาจากผู้ใช้
    ///
    /// ประตูนี้ปิดสองชั้นแบบเดียวกับ `no_item_canvas_field_reaches_the_gpu_without_us_knowing`:
    ///
    /// 1. **destructure ครบทุกฟิลด์ ไม่มี `..`** → เพิ่มฟิลด์ใหม่เมื่อไหร่
    ///    **คอมไพล์ไม่ผ่าน** จนกว่าจะมีคนตัดสินว่ามันมีคนเขียนหรือยัง
    /// 2. ตารางอยู่ในเทสต์เป็นข้อมูล ไม่ใช่ในคอมเมนต์ที่ลอยอยู่เฉย ๆ
    ///
    /// ★ ที่ทำไม่ได้และต้องรู้ไว้: **มันตรวจไม่ได้ว่าโค้ดยัง*เขียน*อยู่จริงไหม**
    /// ถ้าวันหนึ่งมีคนลบเส้นทางที่เขียน `rating` ทิ้ง ตารางนี้จะยังบอกว่า `Real`
    /// — กันได้แค่ฟิลด์ **ใหม่** ที่โผล่มาโดยไม่มีใครถามว่าใครจะเขียนมัน
    #[test]
    fn no_field_stays_unwritten_without_us_knowing() {
        use Writer::{NoneYet, Real};

        let ItemCanvas {
            pos: _,
            size: _,
            rotation: _,
            flip: _,
            opacity: _,
            crop: _,
            locked: _,
            visible: _,
            filter: _,
        } = ItemCanvas::default();
        let canvas = [
            ("pos", Real),      // ลากย้าย (P2-5)
            ("size", Real),     // handle สเกล (P2-5)
            ("rotation", Real), // ลากนอกมุม (P2-5)
            ("flip", Real),     // ปุ่ม H + inspector (P2-8)
            ("opacity", Real),  // inspector (P2-8)
            ("crop", Real),     // เครื่องมือครอป (P2-7)
            ("filter", Real),   // inspector (P2-8)
            (
                "locked",
                NoneYet("ยังไม่มีปุ่มล็อกภาพ — โค้ดที่ *อ่าน* มีครบแล้ว (SelectTool, ApplyLayout) รอ UI"),
            ),
            (
                "visible",
                NoneYet("ยังไม่มีปุ่มซ่อนภาพ — `.refx` จะพาค่ามาตอน P4-1 (§2.2d)"),
            ),
        ];

        let ItemMeta {
            tags: _,
            rating: _,
            color_label: _,
            group: _,
            note: _,
            added_at: _,
            pinned: _,
        } = ItemMeta::default();
        let meta = [
            ("tags", Real),        // แผง Arrange (P3-1)
            ("rating", Real),      // ดาวในแผง Arrange (P3-1)
            ("color_label", Real), // ป้ายสีในแผง Arrange (P3-1)
            ("note", Real),        // ช่องโน้ตในแผง Arrange (P3-1)
            ("pinned", Real),      // checkbox ปักหมุด (P3-1)
            ("added_at", Real),    // ตั้งตอนสร้าง item (P3-4 — ก่อนหน้านี้เป็น 0 ทุกใบ)
            ("group", NoneYet("กลุ่มยังไม่มีใครสร้างได้ — P3-7")),
        ];

        let AssetRef {
            hash: _,
            path: _,
            px_size: _,
            format: _,
            embedded: _,
            mtime: _,
            file_size: _,
        } = match image_item(0).kind {
            ItemKind::Image(asset) => asset,
            _ => unreachable!("image_item สร้าง ItemKind::Image เสมอ"),
        };
        let asset = [
            ("hash", Real),
            ("path", Real),
            ("px_size", Real),
            ("mtime", Real),     // stat บน worker ตอน ingest (P3-4)
            ("file_size", Real), // เหมือนกัน
            (
                "format",
                NoneYet(
                    "เป็น Unknown เสมอ — ต้องร้อยจาก image::guess_format ผ่าน decode → Thumbnail → ThumbEntry พร้อมตัดสินคอลัมน์ `format` ใน cache.sqlite (§6)",
                ),
            ),
            ("embedded", NoneYet("packed mode — P4-5")),
        ];

        let BoardSettings { background: _ } = BoardSettings::default();
        let settings = [(
            "background",
            NoneYet(
                "ยังไม่มี UI ให้เปลี่ยนสีพื้น — แต่ **มีคนอ่านแล้ว** ตั้งแต่ 12 ส.ค. (render pass ใช้ค่านี้แทนค่าคงที่)",
            ),
        )];

        let ArrangeState {
            sort: _,
            descending: _,
        } = ArrangeState::default();
        let arrange = [
            (
                "sort",
                NoneYet(
                    "P3-4 ตัดสินให้การเรียงอยู่ที่ชั้น UI ไม่ใช่ในเอกสาร (§2.17) — ตัวนี้จะมีคนเขียนก็ต่อเมื่อ P4-1 ตัดสินว่าต้อง persist",
                ),
            ),
            ("descending", NoneYet("เหตุผลเดียวกับ `sort`")),
        ];

        let Group {
            name: _,
            collapsed: _,
        } = Group::default();
        let group = [
            ("name", NoneYet("ยังไม่มีใครสร้างกลุ่มได้ — P3-7")),
            ("collapsed", NoneYet("P3-7")),
        ];

        // ★ รายงานออกมาเสมอ ไม่ว่าเทสต์จะผ่านหรือไม่ (`--nocapture`) — ตัวเลขที่
        //   ต้องไปเปิดโค้ดอ่านถึงจะรู้ คือตัวเลขที่ไม่มีใครดู
        let all = [
            ("ItemCanvas", &canvas[..]),
            ("ItemMeta", &meta[..]),
            ("AssetRef", &asset[..]),
            ("BoardSettings", &settings[..]),
            ("ArrangeState", &arrange[..]),
            ("Group", &group[..]),
        ];
        let mut waiting = 0;
        for (owner, fields) in all {
            for (field, writer) in fields {
                if let NoneYet(why) = writer {
                    waiting += 1;
                    println!("ยังไม่มีใครเขียน: {owner}::{field} — {why}");
                }
            }
        }
        println!("รวมฟิลด์ที่ยังไม่มีใครเขียน: {waiting}");
        assert!(
            waiting <= 10,
            "ฟิลด์ที่ไม่มีใครเขียนเพิ่มขึ้นเป็น {waiting} — เพิ่มฟิลด์ใหม่ต้องมีคนเขียน \
             หรือมีเหตุผลว่าทำไมยัง"
        );
    }

    pub(crate) fn image_item(tag: u8) -> Item {
        Item::new(ItemKind::Image(AssetRef {
            hash: ContentHash::from_bytes([tag; 32]),
            path: PathBuf::from(format!("{tag}.png")),
            px_size: UVec2::new(100, 80),
            format: ImageFormat::Png,
            embedded: false,
            mtime: 0,
            file_size: 0,
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
