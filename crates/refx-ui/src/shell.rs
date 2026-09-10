//! โครง UI กลาง — โค้ดชุดเดียวใช้ทั้งสอง mode
//!
//! วาดกรอบทั้งหมด (tabs / toolbar / library / inspector / status bar)
//! แล้วเจาะช่องกลางให้ mode ปัจจุบันวาดเอง
//!
//! > **หมายเหตุ egui 0.34:** `SidePanel` / `TopBottomPanel` / `CentralPanel::show`
//! > ถูก deprecate หมดแล้ว ตัวอย่างใน docs/03 เขียนด้วย API เก่า
//! > ของจริงต้องใช้ `Panel::top/left/right/bottom` + `show_inside(ui)`
//! > (ถ้าใช้ของเก่า clippy `-D warnings` จะตกทันที)
//!
//! spec: docs/03-modes-and-ui.md §1

use refx_core::align::{Align as A, Distribute as D};
use refx_core::board::SortKey;
use refx_core::view::Mode;

use crate::text::{self, Key, Lang, Template};

/// สีของข้อความที่ผู้ใช้ต้องสังเกตเห็นบน**ธีมมืด** — ตัวเดียวกับ RAM/VRAM ตอนใกล้เต็ม
const WARN_COLOR_DARK: egui::Color32 = egui::Color32::from_rgb(230, 160, 60);

/// ★★ สีเดียวกันสำหรับ**ธีมสว่าง** — เข้มกว่ามาก
///
/// ส้มอ่อนบนพื้นขาวแทบอ่านไม่ออก และข้อความที่ใช้สีนี้คือข้อความที่ **สำคัญที่สุด**
/// ทั้งหมด ("ยังไม่ได้บันทึก" · "board เต็มแล้ว" · "ค่าที่ตั้งถูกปรับ") — สีที่
/// อ่านไม่ออกทำให้สิ่งที่เราตั้งใจให้เด่นที่สุดกลายเป็นสิ่งที่มองข้ามง่ายที่สุด
/// (เจอตอนถ่ายภาพหน้าจอธีมสว่างของ P5-3 · ไม่มีเทสต์ไหนถามเรื่องคอนทราสต์ได้)
const WARN_COLOR_LIGHT: egui::Color32 = egui::Color32::from_rgb(150, 85, 0);

/// สีเตือนที่อ่านออกบนธีมที่ใช้อยู่จริง
///
/// ★ อ่านจาก `ui.visuals()` ไม่ใช่จากค่าที่จำไว้ — ผู้ใช้สลับธีมกลางคันได้
/// (`crate::theme`) และค่าที่จำไว้จะค้างเป็นสีของธีมเก่าไปทั้ง session
fn warn_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        WARN_COLOR_DARK
    } else {
        WARN_COLOR_LIGHT
    }
}

/// ★★★ เครื่องหมาย "ยังไม่ถูกบันทึก" ที่นำหน้าชื่อเอกสารบนแท็บ (docs/03 §1)
///
/// ★★ **เคยเป็น `●` (U+25CF) แล้วออกมาเป็นสี่เหลี่ยม tofu บนจอจริง**
/// — ฟอนต์ที่เราฝังไว้ (Ubuntu-Light + Noto Sans Thai) ไม่มี glyph ตัวนั้น
/// และกล่อง tofu ข้างชื่อไฟล์อ่านได้อย่างเดียวว่า "โปรแกรมพัง" ซึ่งแย่กว่า
/// ไม่มีตัวบ่งชี้เลย
///
/// ★ **เทสต์จับไม่ได้โดยธรรมชาติ**: `galley.text()` เก็บอักขระต้นฉบับไว้เสมอ
/// ไม่ว่าจะวาดออกมาเป็น glyph จริงหรือ tofu · เห็นได้ทางเดียวคือถ่ายภาพจอจริง
/// (`docs/08 §3.9` ข้อ 5) → ตอนนี้มีประตู `the_unsaved_mark_has_a_real_glyph`
/// ที่ถาม `Fonts::has_glyph` ตรง ๆ ปิดช่องนั้นแล้ว
const UNSAVED_MARK: char = '*';

/// ★★ สัญลักษณ์บนปุ่มเปิดแผงตั้งค่า (P5-3)
///
/// อยู่ในค่าคงที่ **เพื่อให้ประตู glyph จับมันได้** เหมือน [`UNSAVED_MARK`] —
/// P5-3 เผลอใส่ `●` กลับมาเป็นจุดเตือนแล้วเห็นเป็น tofu บนภาพหน้าจอจริงอีกรอบ
/// ทั้งที่บทเรียนถูกจดไว้แล้วสิบบรรทัดข้างบนนี้เอง · สัญลักษณ์ใหม่ทุกตัวที่
/// โผล่บนจอต้องเข้าประตู `every_symbol_on_screen_has_a_real_glyph`
const SETTINGS_MARK: char = '⚙';

/// ค่าการแสดงผลที่ inspector ปรับได้ — สำเนาของช่องใน `ItemCanvas` ที่เกี่ยวข้อง
///
/// ★ เป็น **สำเนา** ไม่ใช่ `&mut ItemCanvas` โดยตั้งใจ: ทุกการแก้ `Board`
/// ต้องผ่าน `Command` (docs/08 §4 ข้อ 10) ถ้าปล่อย `&mut` เข้ามาถึง widget
/// egui จะเขียนทับ board ตรง ๆ แล้วกฎนั้นก็หายไปโดยไม่มีอะไรบังคับ
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    /// ความทึบ 0..=1
    pub opacity: f32,
    /// ขาวดำเฉพาะภาพนี้
    pub grayscale: bool,
    /// กลับสี
    pub invert: bool,
    /// ความสว่าง -1..=1
    pub brightness: f32,
    /// คอนทราสต์ -1..=1
    pub contrast: f32,
    /// การพลิก
    pub flip: refx_core::board::Flip,
}

/// ข้อมูลฝั่ง Arrange ของสิ่งที่เลือกอยู่ — **ค่าสำหรับแสดงเท่านั้น** (P3-1)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MetaView {
    /// ดาว 0..=5
    pub rating: u8,
    /// ป้ายสี — `Some(Unknown(_))` = ป้ายจากรุ่นใหม่กว่าที่รุ่นนี้ไม่รู้จัก
    pub color_label: Option<refx_core::board::ColorLabel>,
    /// ปักหมุด (arrange จะไม่ย้าย)
    pub pinned: bool,
    /// โน้ตของผู้ใช้ — ★ **คนละตัวกับ `ItemKind::Text`** ตัวนั้นเป็น item บน canvas
    /// ส่วนตัวนี้เป็น metadata ที่ติดกับ item ใบไหนก็ได้ รวมทั้งภาพ
    pub note: String,
    /// ชื่อแท็กของ item นี้ เรียงตาม `TagId` (deterministic)
    pub tags: Vec<String>,
}

/// ★★★ **สภาวะ: ภาพของ board นี้เก็บไว้ที่ไหน** (P4-5 · `docs/07 §2`)
///
/// `docs/07 §2` บังคับว่า *"แถบสถานะบอกเสมอว่า board ปัจจุบันเป็นแบบไหน"* —
/// ซึ่งเป็น **สภาวะ ไม่ใช่เหตุการณ์** (`docs/03 §1`) จึงต้องเป็นตัวบ่งชี้ถาวร
/// ไม่ใช่ข้อความชั่วคราวที่ถูกเขียนทับใน 3 มิลลิวินาที
///
/// ★ คำถามที่ผู้ใช้สนใจจริง ๆ คือ *"ย้ายไฟล์นี้ไปเครื่องอื่นแล้วภาพยังอยู่ไหม"*
/// ตัวเลขจึงเป็น **จำนวนใบที่อยู่ข้างในไฟล์** ไม่ใช่ชื่อโหมดลอย ๆ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StorageView {
    /// โหมดของไฟล์ที่บันทึกไว้ล่าสุด (หรือโหมดที่จะใช้ ถ้ายังไม่เคยบันทึก)
    pub packed: bool,
    /// ภาพทั้งหมดบน board
    pub images: usize,
    /// ★ กี่ใบที่อยู่ **ในไฟล์งาน** แล้วจริง ๆ
    pub inside: usize,
    /// เอกสารนี้มีไฟล์บนดิสก์แล้วหรือยัง
    pub has_file: bool,
}

/// ★★★ **แท็บหนึ่งใบบนแถบบนสุด** (P4-7c) — ค่าสำหรับ *แสดง* เท่านั้น
///
/// ชั้น `app` เติมทุกเฟรมจาก `Docs` · แท็บที่นี่ไม่มี `BoardId` ติดมาด้วยโดยตั้งใจ:
/// widget รู้จักแค่ **ลำดับที่มันวาด** และรายงานกลับเป็นดัชนีของลำดับนั้น
/// (`docs/08 §4` ข้อ 10 — widget ไม่ลงมือกับข้อมูลเอง) ส่วนการแปลงดัชนี → เอกสาร
/// เกิดในเฟรมเดียวกันทันที จึงไม่มีช่วงที่ดัชนีเก่าค้างข้ามเฟรม
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabView {
    /// ชื่อไฟล์ — `None` = ยังไม่เคยบันทึกลงที่ไหน (ขึ้นเป็น "board ที่ยังไม่ได้ตั้งชื่อ")
    pub title: Option<String>,
    /// ★★★ **สภาวะ "ยังไม่ถูกบันทึก" ของแท็บใบนี้เอง**
    ///
    /// ต้องเห็นได้ของ *ทุก* ใบพร้อมกัน ไม่ใช่เฉพาะใบที่อยู่หน้าจอ — ผู้ใช้ที่เห็น
    /// ดาวแค่ใบเดียวจะปิดโปรแกรมโดยเชื่อว่าอีกสามใบสะอาด (`docs/03 §1`)
    pub unsaved: bool,
}

/// สิ่งที่ผู้ใช้แตะบนแถบแท็บในเฟรมนี้ (P4-7c)
///
/// เป็น **คำขอ** ไม่ใช่สถานะ ด้วยเหตุผลเดียวกับ `tool_request`: widget ไม่ปิด
/// เอกสารเอง — การปิดแท็บที่ยังไม่บันทึกต้องผ่านคำถามของชั้น `app` ก่อนเสมอ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabRequest {
    /// สลับไปแท็บลำดับที่ระบุ
    Select(usize),
    /// ปิดแท็บลำดับที่ระบุ (ชั้น `app` ถามก่อนถ้ายังไม่บันทึก)
    Close(usize),
    /// board เปล่าใบใหม่
    New,
}

/// ผู้ใช้ตอบอะไรกับ "จะบันทึกเป็นแบบไหน" (P4-5)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveAsChoice {
    /// ลิงก์ไปไฟล์ของผู้ใช้ (ค่าปริยายตาม `docs/07 §2`)
    ///
    /// ★ ใบที่ไม่มีไฟล์ต้นทางยังถูกฝังอยู่ดี — และ**ห้ามถามผู้ใช้เรื่องนั้น**
    /// (§4 ข้อ 23) กฎนั้นอยู่ที่ `refx-io` ไม่ใช่ที่ปุ่มนี้
    Linked,
    /// ฝังภาพทุกใบไว้ในไฟล์
    Packed,
    /// ไม่บันทึกแล้ว
    Cancel,
}

/// ผู้ใช้ตอบอะไรกับคำถาม "ปิดทั้งที่ยังไม่ได้บันทึก" (P4-2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseChoice {
    /// บันทึกก่อนแล้วค่อยปิด — ★ ปิดจริงเมื่อบันทึก **สำเร็จ** เท่านั้น
    SaveThenClose,
    /// ปิดโดยไม่บันทึก (ผู้ใช้ยืนยันว่าทิ้งงานได้)
    DiscardAndClose,
    /// ไม่ปิดแล้ว กลับไปทำงานต่อ
    Cancel,
}

/// ★★★ ผู้ใช้ตอบอะไรกับ "เจองานที่ยังไม่ได้บันทึกจาก session ก่อน" (P4-4)
///
/// `docs/07 §4` บังคับว่าต้องมี **สามทาง** และเขียนเหตุผลของตัวที่สามไว้ตรง ๆ:
/// *"ผู้ใช้ที่ไม่แน่ใจต้องไม่ถูกบังคับให้ตัดสินใจแบบทำลายข้อมูล"*
///
/// ★★ สองตัวเลือกพอในทางเทคนิค แต่มันบังคับให้คนที่ยัง **จำไม่ได้ว่างานนั้นคืออะไร**
/// ต้องเดา — และครึ่งหนึ่งของการเดาคือการกด "ทิ้ง" ทับงานที่ยังมีค่า
/// · [`Self::Later`] แปลว่า "อย่าแตะไฟล์นั้น ถามฉันใหม่รอบหน้า"
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverChoice {
    /// เอางานนั้นกลับมาบนจอเดี๋ยวนี้
    Restore,
    /// ไม่เอาแล้ว ลบทิ้งได้
    Discard,
    /// ★ **เก็บไว้ก่อน ตัดสินใจทีหลัง** — ไฟล์ไม่ถูกแตะ และจะถูกถามใหม่รอบหน้า
    Later,
}

/// ★★ งานที่กู้ได้มาจากไหน — **สองที่มา คนละเรื่องในสายตาผู้ใช้**
///
/// ปุ่มสามตัวเหมือนกันเป๊ะทั้งสองแบบ (`docs/07 §4`) แต่ประโยคที่ถูกต้องต่างกัน
/// คนละเรื่อง: *"เจองานค้างจากรอบก่อน"* กับ *"ไฟล์นี้มีของที่ยังไม่ได้เขียนลงไป"*
/// — ถามผิดประโยค ผู้ใช้จะตอบผิดปุ่ม
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverScope {
    /// งานที่ **ไม่เคยมีไฟล์เลย** จาก session ก่อน (P4-4 · `recovery/`)
    LastSession,
    /// ★ เอกสารที่เพิ่งเปิดมี `<doc>.refx.autosave` ที่ใหม่กว่าไฟล์ (P4-3)
    ThisDocument,
    /// ★★ ของที่ผู้ใช้เคยกด **"เก็บไว้ก่อน"** ไว้เอง (`<doc>.refx.autosave.kept`)
    ///
    /// ต่างจาก [`Self::ThisDocument`] ตรงที่เขา **เคยเห็นและเคยเลื่อนมันมาแล้ว**
    /// — ประโยคต้องเตือนความจำ ไม่ใช่แจ้งข่าวใหม่
    KeptForLater,
}

/// งานค้างที่เจอตอนเปิดโปรแกรม/เปิดเอกสาร — **ค่าสำหรับแสดงเท่านั้น** (P4-4)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverView {
    /// เขียนไว้เมื่อไหร่ (ข้อความพร้อมแสดงแล้ว) — `None` = ระบบไฟล์ไม่บอก
    pub when: Option<String>,
    /// มีกี่ชิ้นอยู่ในนั้น — ช่วยผู้ใช้จำว่าเป็นงานชิ้นไหน
    pub items: usize,
    /// มาจากไหน — ตัวเลือกที่ผู้ใช้เห็นเหมือนกัน แต่ข้อความต้องตรงกับความจริง
    pub scope: RecoverScope,
}

/// กลุ่มของสิ่งที่เลือกอยู่ — **ค่าสำหรับแสดงเท่านั้น** (P3-7)
///
/// ★ `None` ทั้งก้อน = ไม่ได้เลือกอะไร · `Mixed` = เลือกข้ามหลายกลุ่ม
/// ซึ่งต้องแยกจาก "ไม่ได้อยู่ในกลุ่มไหน" ให้ขาด ไม่งั้นช่องเปลี่ยนชื่อจะโผล่มา
/// แล้วเขียนทับกลุ่มที่ผู้ใช้ไม่ได้ตั้งใจแตะ
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupView {
    /// ทุกใบที่เลือกไม่ได้อยู่ในกลุ่มไหนเลย
    Loose,
    /// ทุกใบที่เลือกอยู่ในกลุ่มเดียวกัน
    One {
        /// คีย์ของกลุ่ม
        id: refx_core::arena::GroupId,
        /// ชื่อที่ผู้ใช้ตั้ง
        name: String,
        /// ยุบอยู่หรือไม่
        collapsed: bool,
        /// จำนวนสมาชิกทั้งหมดของกลุ่ม (ไม่ใช่จำนวนที่เลือก)
        members: usize,
    },
    /// เลือกข้ามหลายกลุ่ม (หรือกลุ่มปนกับใบที่ไม่มีกลุ่ม)
    Mixed,
}

/// ★★★ ภาพที่หาไฟล์ไม่เจอในสิ่งที่เลือกอยู่ — **ค่าสำหรับแสดงเท่านั้น** (P4-6)
///
/// `docs/07 §2` ขั้นที่ 4: item ที่หาไฟล์ไม่เจอ **ต้องไม่หายไปจาก board** และ
/// ต้องบอกผู้ใช้ว่าไฟล์ไหนหาย ไม่ใช่แสดงช่องว่างเฉย ๆ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingView {
    /// ชื่อไฟล์ของใบแรกที่หาย (**ชื่อไฟล์ล้วน ไม่ใช่ path เต็ม** — `docs/08 §5`)
    pub file: String,
    /// เลือกไว้กี่ใบที่หาย
    pub count: usize,
}

/// สิ่งที่ผู้ใช้ขอทำกับกลุ่มในเฟรมนี้ (P3-7)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupRequest {
    /// เปลี่ยนชื่อกลุ่มนี้
    Rename(refx_core::arena::GroupId, String),
    /// ยุบ/กางกลุ่มนี้
    Collapsed(refx_core::arena::GroupId, bool),
}

/// สิ่งที่ผู้ใช้ขอแก้ในเฟรมนี้ — **`None` = ไม่ได้แตะอะไรเลย** (P3-1)
///
/// ★ แยกจาก [`MetaView`] ด้วยเหตุผลเดียวกับ `appearance_edit` / `note_edit`:
/// ค่าที่ค้างอยู่ซึ่งถูกเขียนกลับทุกเฟรมจะทับสิ่งที่ undo เพิ่งคืนมา
/// (`docs/08 §3.9` ข้อ 8.1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaRequest {
    /// ตั้งดาว
    Rating(u8),
    /// ตั้ง/ล้างป้ายสี
    ColorLabel(Option<refx_core::board::ColorLabel>),
    /// ปักหมุด
    Pinned(bool),
    /// แก้โน้ต
    Note(String),
    /// ติดแท็กชื่อนี้
    AddTag(String),
    /// ถอดแท็กชื่อนี้
    RemoveTag(String),
}

/// รูปแบบไฟล์ที่กล่อง export ให้เลือก (P5-4)
///
/// ★ **สำเนาของ `refx_asset::export::ExportFormat` โดยตั้งใจ** — ตัวโน้นถือ
/// ค่าที่ตัวเข้ารหัสต้องใช้ (คุณภาพ · โปร่งใส) ส่วนตัวนี้ถือแค่ *ปุ่มที่ถูกกด*
/// ค่าที่เหลือมีช่องของตัวเองใน [`ExportView`] เพราะผู้ใช้ปรับมันแยกกัน
/// และต้องไม่หายเมื่อสลับรูปแบบไปกลับ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportKind {
    /// PNG — ไม่สูญเสีย · โปร่งใสได้
    #[default]
    Png,
    /// JPEG — ไฟล์เล็กกว่ามาก แต่สูญเสียและไม่มี alpha
    Jpeg,
}

/// สิ่งที่กล่อง export แสดงและให้ผู้ใช้ปรับ (P5-4 · `docs/07 §6`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportView {
    /// ★★ **ด้านที่ยาวที่สุดของภาพปลายทาง — ค่าเดียวที่ผู้ใช้ปรับ**
    ///
    /// ให้ปรับกว้างกับสูงแยกกันได้ = ให้เขาทำภาพที่ยืดผิดสัดส่วนได้โดยไม่ตั้งใจ
    /// ซึ่งไม่มีใครอยากได้ · สัดส่วนมาจาก board ชั้น `app` เป็นคนคำนวณอีกด้านให้
    pub long_side: u32,
    /// ความกว้างที่จะได้จริง — **ค่าสำหรับแสดง** ชั้น `app` เป็นคนเติม
    pub width: u32,
    /// ความสูงที่จะได้จริง — ค่าสำหรับแสดง
    pub height: u32,
    /// ประมาณขนาดไฟล์ (ไบต์) — ค่าสำหรับแสดง
    pub estimate: u64,
    /// ★★★ เพดานด้านยาวสุดที่ **พิกเซลที่มีจริงรองรับได้** (`docs/07 §6`)
    ///
    /// ตัวเลือกที่เกินค่านี้ **ไม่มีให้เลือก** ไม่ใช่เลือกได้แล้วเตือน —
    /// ปุ่มที่กดแล้วได้ผลแย่เสมอ ไม่ควรมีให้กด · ชั้น `app` เป็นคนคำนวณ
    pub max_side: u32,
    /// รูปแบบที่เลือกอยู่
    pub kind: ExportKind,
    /// PNG: เก็บพื้นโปร่งใสไหม
    pub transparent: bool,
    /// JPEG: คุณภาพ 1..=100
    pub quality: u8,
    /// สีพื้นหลัง sRGB — JPEG ไม่มี alpha จึงต้องมีเสมอ
    pub background: [u8; 3],
    /// จำนวน item ที่จะถูกวาด
    pub items: usize,
    /// ★★★ จำนวนใบที่หาไฟล์ไม่เจอ — **ต้องเตือนก่อนกดจริง** (`docs/07 §6`)
    pub missing: usize,
    /// ชื่อไฟล์ปลายทางที่เลือกไว้ — `None` = ยังไม่ได้เลือก
    pub target: Option<String>,
    /// ไฟล์ปลายทางมีอยู่แล้ว → ต้องถามก่อนทับ (`docs/07 §6` ข้อ 4)
    pub overwrite: bool,
    /// กำลังรอผู้ใช้เลือกไฟล์อยู่ (กล่องของ OS เปิดค้าง)
    pub choosing: bool,
    /// เหตุผลที่ export ล่าสุดไม่สำเร็จ (แปลแล้ว) — `None` = ไม่มีอะไรผิด
    pub problem: Option<String>,
}

impl Default for ExportView {
    fn default() -> Self {
        Self {
            long_side: 2048,
            width: 2048,
            height: 2048,
            estimate: 0,
            max_side: refx_core::export::MAX_SIDE,
            kind: ExportKind::default(),
            transparent: false,
            quality: 90,
            // ★ ขาวเป็นค่าปริยาย ไม่ใช่ดำ — นักวาดส่ง mood board ไปให้คนอื่นดู
            //   บนพื้นขาวเป็นส่วนใหญ่ และพื้นดำที่ไม่ได้ตั้งใจคือสิ่งที่
            //   `docs/07 §6` เตือนไว้ตรง ๆ
            background: [255, 255, 255],
            items: 0,
            missing: 0,
            target: None,
            overwrite: false,
            choosing: false,
            problem: None,
        }
    }
}

/// สิ่งที่ผู้ใช้ขอให้ทำกับ export ในเฟรมนี้ — **`None` = ไม่ได้แตะอะไร**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportRequest {
    /// เปิดกล่องส่งออก (ปุ่มบนแถบเครื่องมือ — เส้นทางเดียวกับ `Ctrl+E`)
    Open,
    /// เปิดกล่องของ OS ให้เลือกที่บันทึก
    ChooseTarget,
    /// เริ่ม export ได้แล้ว
    Start,
    /// ปิดกล่อง (ยังไม่ได้เริ่ม)
    Close,
    /// หยุดงานที่กำลังทำอยู่
    Cancel,
}

/// ความคืบหน้าของ export ที่กำลังทำอยู่ (P5-4)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportProgress {
    /// ชื่อไฟล์ที่กำลังเขียน
    pub name: String,
    /// แถบที่เสร็จแล้ว
    pub done: u32,
    /// แถบทั้งหมด
    pub total: u32,
    /// ผู้ใช้กดยกเลิกไปแล้ว กำลังรอให้มันหยุด
    pub cancelling: bool,
}

/// สิ่งที่ปุ่มจัดเรียงขอให้ทำ (P2-9)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrangeRequest {
    /// ชิดขอบ/กึ่งกลางตามที่ระบุ
    Align(refx_core::align::Align),
    /// กระจายระยะให้เท่ากัน
    Distribute(refx_core::align::Distribute),
}

/// สถานะที่ shell ต้องอ่าน/เขียน
///
/// P2+ จะขยายเป็น `App` เต็มที่มี board, selection, history
/// ตอนนี้เก็บเฉพาะที่ P0-8 ต้องใช้จริง — ไม่เดาโครงล่วงหน้า
#[derive(Debug)]
pub struct ShellState {
    /// mode ปัจจุบัน
    pub mode: Mode,
    /// ★ ค่าการแสดงผลของสิ่งที่เลือกอยู่ — inspector อ่านจากที่นี่และเขียนกลับที่นี่
    ///
    /// `None` = ไม่ได้เลือกอะไร · ชั้น `app` เติมค่าก่อนวาดแล้วอ่านกลับหลังวาด
    /// ถ้าต่างจากเดิมแปลว่าผู้ใช้ปรับ แล้วมันจะถูกห่อเป็น `SetFilter` เข้า `History`
    pub appearance: Option<Appearance>,
    /// ★★ **สิ่งที่ผู้ใช้ขอในเฟรมนี้** — `None` = ไม่ได้แตะอะไรเลย
    ///
    /// แยกจาก `appearance` (ที่เป็นแค่ค่าสำหรับ *แสดง*) โดยตั้งใจ เพราะเคยเป็นบั๊กจริง:
    /// เดิมชั้น `app` อ่าน `appearance` กลับไปเขียนลง board ทุกเฟรม ซึ่งแปลว่า
    /// **ค่าที่ค้างอยู่จากเฟรมก่อนจะทับสิ่งที่คีย์ลัดเพิ่งเปลี่ยน** — กด `H` แล้วภาพพลิก
    /// เสี้ยววินาทีแล้วเด้งกลับ โดยไม่มี error ที่ไหนเลย
    ///
    /// ตอนนี้มีเจ้าของเดียว: เขียนตรงนี้ **เฉพาะตอน widget รายงานว่าถูกแตะ**
    /// (`docs/08 §3.9` ข้อ 8.1 — ทำให้ API ถูกได้ทางเดียว ไม่ใช่ต้องใช้ให้ถูก)
    pub appearance_edit: Option<Appearance>,
    /// ผู้ใช้ปล่อยตัวควบคุมในเฟรมนี้ → ปิดหน้าต่าง merge (undo ขั้นใหม่)
    pub appearance_sealed: bool,
    /// ★ ผู้ใช้กดปุ่ม align/distribute ในเฟรมนี้ — ชั้น `app` เป็นคนลงมือ
    ///
    /// เจ้าของเดียวเหมือน `appearance_edit`: widget แค่ **ขอ** ไม่ได้แก้ board เอง
    pub arrange_request: Option<ArrangeRequest>,
    /// ★ grayscale ทั้ง board — **สวิตช์ของการมองเห็น ไม่ใช่ของเอกสาร**
    ///
    /// docs/03 §2 เรียกมันว่า "uniform ตัวเดียว" ซึ่งเป็นสิ่งที่มันเป็นจริงในเส้นทางวาด
    /// ไม่อยู่ใน `Board` จึงไม่กิน undo และไม่ทำให้เอกสาร dirty
    /// (เหตุผลเดียวกับที่ `selection` ถูกย้ายออก — docs/02 §2.9)
    pub board_grayscale: bool,
    /// ★ จำนวน texture upload สะสม — หลักฐานของเกณฑ์ ROADMAP P2-8 ที่ **เห็นได้ด้วยตา**
    ///
    /// สลับ `G` แล้วเลขนี้ต้องไม่ขยับแม้แต่หนึ่ง ไม่ว่าบน board จะมีกี่ภาพ
    pub atlas_uploads: u64,
    /// เครื่องมือที่เลือกอยู่ — **อ่านอย่างเดียว** ชั้นแอปเขียนค่านี้ทุกเฟรม
    ///
    /// ★ เจ้าของจริงคือ `Gfx::tool` ที่เดียว ที่นี่เป็นแค่สำเนาไว้วาดปุ่มให้ถูก
    pub tool: refx_core::interact::Tool,
    /// ★ ผู้ใช้กดปุ่มเครื่องมือบน toolbar — ชั้นแอปมาเก็บไปแล้วล้างทิ้ง
    ///
    /// แยกจาก `tool` โดยตั้งใจ: ถ้าให้ปุ่มเขียน `tool` ตรง ๆ จะมีสองแหล่งความจริง
    /// แล้วต้องพึ่ง**ลำดับ**ว่าใครเขียนก่อน — เคยพลาดมาแล้วตอนทำ P2-7 คือคีย์ลัด
    /// เขียน `Gfx::tool` แล้วโดนค่าเก่าจาก `ShellState` เขียนทับกลับทุกเฟรม
    /// อาการคือ **กด `C` แล้วไม่มีอะไรเกิดขึ้น ส่วนกดปุ่มได้ปกติ** (docs/08 §3.9 ข้อ 8.1)
    pub tool_request: Option<refx_core::interact::Tool>,
    /// ★ ภาษาของ UI — ทุกข้อความที่ผู้ใช้เห็นต้องผ่าน `text::t`/`text::fill` ด้วยค่านี้
    ///
    /// อ่านจาก locale ของ OS ครั้งเดียวตอนเปิดโปรแกรม (docs/03 §0 ข้อ 3)
    pub lang: Lang,
    /// ข้อความสถานะฝั่งซ้ายของ status bar
    pub status: String,
    /// ★ `status` เป็นเรื่องที่ผู้ใช้ต้อง **สังเกตเห็น** ไหม (P3-3)
    ///
    /// status bar เป็นที่รวมของทุกข้อความ ตั้งแต่ "พร้อม" ไปจนถึง "board เต็มแล้ว
    /// อีก 6,928 ใบเข้าไม่ได้" — ถ้าทั้งหมดเป็นสีเดียวกัน ข้อความสำคัญจะกลืนหายไป
    /// กับข้อความประจำวัน · ใช้สีเดียวกับ RAM/VRAM ตอนใกล้เต็ม (เจ้าของโทนเดียวกัน)
    pub status_warn: bool,
    /// จำนวน item บน board (ตอนนี้คือจำนวนสี่เหลี่ยมทดสอบ)
    pub item_count: usize,
    /// ระดับซูมปัจจุบัน — แสดงบน status bar
    pub zoom: f32,
    /// จำนวนเฟรมที่วาดไปแล้ว — ตัวชี้วัด I-1 ที่เห็นได้ด้วยตา
    ///
    /// ปล่อยโปรแกรมทิ้งไว้แล้วตัวเลขนี้ต้อง **หยุดนิ่ง** ถ้ายังไต่ขึ้นเรื่อย ๆ
    /// แปลว่ามีที่ไหนสักแห่งขอวาดทุกเฟรม
    pub frames_drawn: u64,
    /// ★★★ ช่วงที่ **ขอวาดเฟรมต่อเนื่องทั้งที่ไม่มี input** — ตัวจับ I-1 ที่รั่วเป็นครั้งคราว
    ///
    /// `None` = ไม่มีอะไรน่าสงสัย (สภาพปกติ) · `Some` = เกินเพดาน
    /// `refx_platform::redraw::QUIET_ALARM` แล้ว พร้อมชื่อคนที่ขอมากที่สุด
    ///
    /// ★★ **โผล่บนแถบสถานะเฉพาะตอนผิดปกติ** — ภาพหน้าจอในสภาพปกติจึงไม่เปลี่ยนเลย
    /// แต่ครั้งหน้าที่อาการเกิด **ภาพจะบอกสาเหตุของตัวเอง** แทนที่จะต้องไปงมใน log
    /// (`docs/08 §3.9` ข้อ 11 — เจอ ~1.5 Hz สองครั้งแล้วทำซ้ำไม่ได้อีกเลย)
    pub quiet_redraws: Option<(u64, &'static str)>,

    /// ★ I-6: RAM ที่ decode pool ใช้อยู่ / เพดาน (ไบต์)
    ///
    /// spec บังคับให้ตัวเลขนี้ **เห็นได้ด้วยตา** ตลอดเวลา ไม่ใช่ซ่อนใน log
    pub ram_used: usize,
    /// เพดาน RAM รวมทุก worker
    pub ram_limit: usize,
    /// ★ I-6: VRAM ที่ texture ใช้อยู่ / เพดาน (ไบต์)
    pub vram_used: usize,
    /// เพดาน VRAM (คำนวณจากชนิดการ์ดจอ — iGPU ได้น้อยกว่า)
    pub vram_limit: usize,
    /// จำนวน thumbnail ใน cache.sqlite
    pub cache_thumbs: u64,
    /// ขนาด cache.sqlite (ไบต์)
    pub cache_bytes: u64,
    /// งาน decode ที่ยังค้างคิว
    pub decode_queued: usize,
    /// งาน decode ที่ถูกยกเลิกไปแล้ว — หลักฐานว่า cancellation ทำงาน
    pub decode_cancelled: u64,

    /// จำนวน draw call ของภาพในเฟรมล่าสุด
    ///
    /// ชั้น B ทำให้มี draw call ต่อ working texture หนึ่งใบ — ตัวเลขนี้คือสิ่งที่
    /// บอกว่ามันบานออกไปหรือยัง (docs/04 §4 ตั้งเพดานไว้ที่ราว 30)
    pub draw_calls: u32,
    /// VRAM ที่ working texture ใช้ / เพดานของชั้นนั้น (ไบต์)
    pub working_used: usize,
    /// เพดานของ working texture
    pub working_limit: usize,
    /// จำนวน working texture ที่ถูกไล่ออกตาม LRU — หลักฐานว่า LRU ทำงาน
    pub working_evicted: u64,

    /// ★ เนื้อความของโน้ตที่เลือกอยู่ (P2-11) — `None` = ไม่ได้เลือกโน้ต
    ///
    /// **ค่าสำหรับ *แสดง* เท่านั้น** ชั้น `app` เติมให้ทุกเฟรม
    pub note: Option<String>,
    /// ★★ **สิ่งที่ผู้ใช้พิมพ์ในเฟรมนี้** — `None` = ไม่ได้แตะช่องข้อความเลย
    ///
    /// แยกจาก `note` ด้วยเหตุผลเดียวกับ `appearance_edit` เป๊ะ ๆ: ถ้าอ่าน `note`
    /// กลับไปเขียนลง board ทุกเฟรม ค่าที่ค้างจากเฟรมก่อนจะทับสิ่งที่ undo
    /// เพิ่งคืนมา — กด Ctrl+Z แล้วข้อความเด้งกลับทันที (`docs/08 §3.9` ข้อ 8.1)
    pub note_edit: Option<String>,
    /// ผู้ใช้ออกจากช่องข้อความแล้ว → ปิดหน้าต่าง merge (undo ขั้นใหม่)
    pub note_sealed: bool,

    /// ★ วิธีเรียงที่ผู้ใช้เลือก (P3-4) — **widget เป็นเจ้าของ ชั้น `app` อ่านอย่างเดียว**
    ///
    /// ดูเหตุผลที่มันไม่ต้องมีคู่ `_request` แบบ `tool`/`appearance` ใน `arrange_tools`
    pub arrange_sort: SortKey,
    /// เรียงกลับทาง
    pub arrange_descending: bool,
    /// ตัวกรองที่ผู้ใช้ตั้งไว้ (P3-4)
    pub arrange_filter: refx_core::query::Filter,
    /// ★ ผู้ใช้กด "ส่งเข้า canvas" ในเฟรมนี้ (P3-5) — ชั้น `app` มาเก็บไปแล้วล้างทิ้ง
    ///
    /// เป็น **คำขอ** ไม่ใช่สถานะ ด้วยเหตุผลเดียวกับ `tool_request`/`arrange_request`:
    /// widget ไม่แก้ `Board` เอง (docs/08 §4 ข้อ 10)
    pub arrange_apply: bool,
    /// ★★ ตัวเลขของ virtual scrolling ในเฟรมล่าสุด (P3-3)
    ///
    /// เกณฑ์ของ ROADMAP คือ **"10,000 item · วาดจริง < 60 ตัว"** ซึ่งเป็นตัวเลข
    /// ไม่ใช่คำกล่าวอ้าง — มันจึงต้องอยู่บน status bar ให้เห็นด้วยตาเหมือน
    /// `atlas_uploads` ที่เป็นหลักฐานของเกณฑ์ P2-8 (docs/08 §3.9 ข้อ 6)
    pub arrange: crate::arrange::Counts,
    /// ★ ข้อมูลฝั่ง Arrange ของสิ่งที่เลือกอยู่ (P3-1) — `None` = ไม่ได้เลือกอะไร
    pub meta: Option<MetaView>,
    /// ★★ สิ่งที่ผู้ใช้ขอแก้ในเฟรมนี้ — `None` = ไม่ได้แตะ
    pub meta_request: Option<MetaRequest>,
    /// ผู้ใช้ออกจากช่องโน้ตแล้ว → ปิดหน้าต่าง merge
    pub meta_sealed: bool,
    /// ช่องพิมพ์ชื่อแท็กใหม่ — **สถานะของ widget ล้วน ๆ** ไม่ใช่ของเอกสาร
    pub tag_input: String,
    /// ★ กลุ่มของสิ่งที่เลือกอยู่ (P3-7) — `None` = ไม่ได้เลือกอะไร
    pub group: Option<GroupView>,
    /// ★★★ **แท็บทั้งหมดที่เปิดอยู่** (P4-7c) — ค่าสำหรับแสดง ชั้น `app` เติมทุกเฟรม
    pub tabs: Vec<TabView>,
    /// ดัชนีของแท็บที่ผู้ใช้กำลังดูอยู่ (อยู่ในช่วงของ `tabs` เสมอ)
    pub active_tab: usize,
    /// ★ สิ่งที่ผู้ใช้แตะบนแถบแท็บในเฟรมนี้ — `None` = ไม่ได้แตะ
    pub tab_request: Option<TabRequest>,
    /// ★ กำลังถามว่าจะปิดยังไงทั้งที่ยังไม่ได้บันทึก (P4-2)
    ///
    /// ★★ ตั้งแต่ P4-7c ใช้ทั้งกับ "ปิดหน้าต่าง" และ "ปิดแท็บ" (`Ctrl+W`) —
    /// [`Self::close_scope_tab`] บอกว่าคำถามที่ค้างอยู่เป็นเรื่องไหน
    pub close_prompt: bool,
    /// ★ คำถามที่ค้างอยู่เป็นเรื่อง **ปิดแท็บ** ไม่ใช่ปิดทั้งหน้าต่าง
    ///
    /// สองเรื่องนี้มีปุ่มชุดเดียวกันเพราะเป็นคำถามชนิดเดียวกัน — แต่ **สิ่งที่หาย
    /// ถ้าตอบผิดต่างกันมาก** หัวข้อจึงต้องพูดตรงกับเรื่องที่ถาม
    pub close_scope_tab: bool,
    /// ผู้ใช้ตอบแล้วในเฟรมนี้ — `None` = ยังไม่ตอบ
    pub close_choice: Option<CloseChoice>,
    /// ★★★ **สภาวะ: ภาพเก็บไว้ที่ไหน** (P4-5) — ดู [`StorageView`]
    ///
    /// **ค่าสำหรับแสดงเท่านั้น** ชั้น `app` เติมทุกเฟรม
    pub storage: StorageView,
    /// ★ ผู้ใช้กดสลับโหมดบนแถบสถานะในเฟรมนี้ — `Some(true)` = ขอให้ฝังทุกใบ
    ///
    /// เป็น **คำขอ** ไม่ใช่สถานะ ด้วยเหตุผลเดียวกับ `tool_request`:
    /// widget ไม่ลงมือเอง ชั้น `app` เป็นคนตัดสินและลงมือ
    pub storage_request: Option<bool>,
    /// ★★ กำลังถามว่า "บันทึกเป็นแบบไหน" (`Ctrl+Shift+S` — `docs/07 §2`)
    pub save_as_prompt: bool,
    /// ผู้ใช้ตอบแล้วในเฟรมนี้ — `None` = ยังไม่ตอบ
    pub save_as_choice: Option<SaveAsChoice>,
    /// ★★ กล่อง export เปิดอยู่ไหม (P5-4) — `None` = ปิด
    pub export_prompt: Option<ExportView>,
    /// ผู้ใช้กดอะไรในกล่อง export เฟรมนี้ — `None` = ไม่ได้แตะ
    pub export_request: Option<ExportRequest>,
    /// ★ งาน export ที่กำลังทำอยู่บน worker — `None` = ไม่มีงาน
    pub export_progress: Option<ExportProgress>,
    /// ★★★ เจองานที่ยังไม่ได้บันทึกจาก session ก่อน (P4-4) — `None` = ไม่มีอะไรค้าง
    pub recover_prompt: Option<RecoverView>,
    /// ผู้ใช้ตอบแล้วในเฟรมนี้ — `None` = ยังไม่ตอบ
    pub recover_choice: Option<RecoverChoice>,
    /// ★★★ ภาพที่ **หาไฟล์ไม่เจอ** ในสิ่งที่เลือกอยู่ (P4-6) — `None` = ไม่มี
    ///
    /// `docs/07 §2` ขั้นที่ 4 บังคับว่าต้องเห็น **ชื่อไฟล์** ไม่ใช่แค่ช่องว่าง
    /// ผู้ใช้ที่ถอดฮาร์ดดิสก์ออกต้องอ่านออกว่าหายไปเพราะอะไรและไฟล์ไหน
    pub missing: Option<MissingView>,
    /// ผู้ใช้กด "หาไฟล์เอง" ในเฟรมนี้ (ขั้นที่ 5)
    pub relink_request: bool,
    /// ★★ สิ่งที่ผู้ใช้ขอทำกับกลุ่มในเฟรมนี้ — `None` = ไม่ได้แตะ
    pub group_request: Option<GroupRequest>,
    /// ผู้ใช้ออกจากช่องชื่อกลุ่มแล้ว → ปิดหน้าต่าง merge
    pub group_sealed: bool,

    /// ★ สีที่ picker อ่านได้ล่าสุด (P2-10) — `None` = ยังไม่ได้จิ้มอะไร
    ///
    /// **เป็นสีของ pixel ต้นฉบับ** ไม่ได้ผ่าน grayscale/brightness/opacity ใด ๆ
    /// จึงต่างจากสีที่เห็นบนจอได้ และนั่นคือสิ่งที่ถูกต้อง (ROADMAP P2-10)
    pub picked: Option<refx_core::pick::Picked>,
    /// ★ ไม้บรรทัดที่วางอยู่ (P2-10) — ตัวเลขเป็น **world unit** ไม่ใช่พิกเซลบนจอ
    pub measured: Option<refx_core::pick::Measurement>,

    /// ★ ความคืบหน้าการโหลด — `None` เมื่อไม่มีงานค้าง
    ///
    /// docs/05 §6: การรอ 80 วินาทีบน cache เย็นยอมรับได้ **ก็ต่อเมื่อ** ผู้ใช้
    /// เห็นว่ามันคืบหน้าอยู่ ไม่ใช่ค้าง — ถ้าไม่มีตัวนี้ เขาจะคิดว่าโปรแกรมแฮงก์
    /// แล้วปิดทิ้งกลางคัน ซึ่งแย่กว่ารอนาน
    pub loading: Option<LoadProgress>,

    /// ★ แผง Settings เปิดอยู่ไหม (P5-3) — สถานะของ widget ล้วน ๆ
    pub settings_open: bool,
    /// ค่าที่แผงแสดง — **ค่าสำหรับ *แสดง* เท่านั้น** ชั้น `app` เติมทุกเฟรม
    pub settings: SettingsView,
    /// ★★ สิ่งที่ผู้ใช้เปลี่ยนในเฟรมนี้ — `None` = ไม่ได้แตะ
    ///
    /// แยกจาก [`Self::settings`] ด้วยเหตุผลเดียวกับ `appearance_edit` เป๊ะ:
    /// ถ้าอ่านค่าที่แสดงกลับไปเขียนทุกเฟรม ค่าที่ค้างจากเฟรมก่อนจะทับสิ่งที่
    /// เพิ่งถูกตั้ง (`docs/08 §3.9` ข้อ 8.1)
    pub settings_edit: Option<SettingsView>,
    /// ★★★ ผู้ใช้ปล่อยตัวควบคุมแล้ว → **ถึงเวลาเขียนลงไฟล์**
    ///
    /// ★ ถ้าเขียนทุกเฟรมที่ค่าเปลี่ยน การลากแถบเลื่อนหนึ่งครั้งจะเขียนดิสก์
    /// หลายสิบรอบ — สิ้นเปลืองและทำให้ UI thread สะดุด (I-2) · หลักการเดียวกับ
    /// `appearance_sealed` ที่ปิดหน้าต่าง merge ของ undo
    pub settings_sealed: bool,
    /// ★★ เรื่องที่ `settings.toml` ไม่ได้ถูกใช้ตามที่เขียน — ว่าง = ไม่มีอะไรผิด
    pub settings_notes: Vec<refx_io::settings::Note>,
    /// ผู้ใช้กด "รับทราบ" รายการปัญหาในเฟรมนี้
    pub settings_notes_dismissed: bool,
    /// ★★ คีย์ลัดที่ใช้อยู่ — **อ่านอย่างเดียว** (P5-3b ก้อน b)
    pub keymap: KeymapView,
}

/// คีย์ลัดที่ใช้อยู่ ตามที่แผง Settings แสดง (P5-3b ก้อน b)
///
/// ★ **อ่านอย่างเดียวรอบนี้** — แก้คีย์ในแอปยังไม่ทำ · หน้าที่ของมันคือตอบสอง
/// คำถามที่ผู้ใช้ถามก่อนจะเปิด `keymap.toml`: *"ตอนนี้มีปุ่มอะไรบ้าง"* และ
/// *"ต้องพิมพ์ชื่ออะไรลงไฟล์"*
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeymapView {
    /// คู่ (ปุ่ม, ชื่อ action) ที่แสดงได้ — ชั้น `app` เติมให้
    ///
    /// ★★ **ไม่รวม alias อักขระควบคุม** — มันไม่มี glyph ในฟอนต์ที่ฝังไว้
    /// การวาดมันคือสี่เหลี่ยม tofu (ดู `keymap::Chord::display`)
    pub rows: Vec<(String, &'static str)>,
    /// ตารางนี้มาจาก `keymap.toml` ของผู้ใช้ (ไม่ใช่ค่าปริยาย)
    pub from_file: bool,
    /// ★★★ เหตุที่ `keymap.toml` ใช้ไม่ได้ (แปลแล้ว) — `None` = ไม่มีปัญหา
    pub problem: Option<String>,
}

/// ค่าที่แผง Settings แสดงและแก้ได้ (P5-3)
///
/// ★ หน่วยเป็น **MB และจำนวนจุด** ไม่ใช่ไบต์ — ตัวเลขที่ผู้ใช้พิมพ์ต้องเป็น
/// ตัวเลขที่เขาคิดในหัว ส่วนการแปลงเป็นไบต์เป็นเรื่องของชั้น `app`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsView {
    /// เพดาน RAM ของ decode (MB)
    pub ram_limit_mb: u64,
    /// เพดาน VRAM (MB) — `None` = ให้โปรแกรมเลือกตามการ์ดจอ
    pub vram_limit_mb: Option<u64>,
    /// เพดานขนาดภาพ (จำนวนจุด)
    pub max_pixels: u64,
    /// ธีมของ UI
    pub theme: refx_io::settings::Theme,
    /// จังหวะการแสดงเฟรม
    pub present: refx_io::settings::Present,
    /// ★ เพดานของ **เครื่องนี้** — แสดงอย่างเดียว แก้ไม่ได้ (`HANDOFF §4` ข้อ 3)
    pub max_pixels_ceiling: u64,
    /// ★★ มีค่าที่เปลี่ยนแล้วยังไม่มีผลจนกว่าจะเปิดโปรแกรมใหม่
    ///
    /// ต้องบอก ไม่งั้นผู้ใช้ที่เลื่อนเพดาน RAM แล้วเห็นตัวเลขบนแถบสถานะไม่ขยับ
    /// จะสรุปว่าแผงนี้เสีย แล้วไม่กลับมาใช้อีกเลย
    pub needs_restart: bool,
}

impl Default for SettingsView {
    fn default() -> Self {
        Self {
            ram_limit_mb: refx_io::settings::DEFAULT_RAM_LIMIT_MB,
            vram_limit_mb: None,
            max_pixels: refx_asset::decode::MAX_PIXELS_ABS,
            theme: refx_io::settings::Theme::default(),
            present: refx_io::settings::Present::default(),
            max_pixels_ceiling: refx_asset::decode::MAX_PIXELS_ABS,
            needs_restart: false,
        }
    }
}

/// ความคืบหน้าของงาน decode งวดปัจจุบัน
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadProgress {
    /// จำนวนที่จบแล้ว (รวมที่ล้มเหลวและถูกยกเลิก — ไม่ค้างคิวแล้วทั้งคู่)
    pub done: u64,
    /// จำนวนทั้งหมดในงวดนี้
    pub total: u64,
}

impl LoadProgress {
    /// ข้อความที่ผู้ใช้เห็น — รูปแบบตาม docs/05 §6
    #[must_use]
    pub fn label(self, lang: Lang) -> String {
        text::fill(
            lang,
            Template::Loading,
            &[
                ("done", &self.done.to_string()),
                ("total", &self.total.to_string()),
            ],
        )
    }

    /// สัดส่วนที่เสร็จแล้ว `0.0..=1.0`
    #[must_use]
    pub fn fraction(self) -> f32 {
        if self.total == 0 {
            return 1.0;
        }
        // ตัดที่ 1.0 เสมอ — ตัวเลขที่เกิน 100% ทำให้ผู้ใช้ไม่เชื่อถือทั้งแถบ
        (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
    }
}

/// แปลงไบต์เป็นข้อความสั้น ๆ ที่คนอ่านรู้เรื่อง
pub(crate) fn human_bytes(bytes: u64) -> String {
    const MB: u64 = 1 << 20;
    const KB: u64 = 1 << 10;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            // ★ ว่างจนกว่าชั้น `app` จะเติมเฟรมแรก — `Docs` ไม่ว่างเสมอ
            tabs: Vec::new(),
            active_tab: 0,
            tab_request: None,
            close_scope_tab: false,
            appearance: None,
            appearance_edit: None,
            missing: None,
            relink_request: false,
            arrange_request: None,
            appearance_sealed: false,
            board_grayscale: false,
            atlas_uploads: 0,
            tool: refx_core::interact::Tool::default(),
            tool_request: None,
            lang: Lang::default(),
            status: text::t(Lang::default(), Key::Ready).to_owned(),
            status_warn: false,
            item_count: 0,
            zoom: 1.0,
            frames_drawn: 0,
            quiet_redraws: None,
            ram_used: 0,
            ram_limit: 0,
            vram_used: 0,
            vram_limit: 0,
            cache_thumbs: 0,
            cache_bytes: 0,
            decode_queued: 0,
            decode_cancelled: 0,
            draw_calls: 0,
            working_used: 0,
            working_limit: 0,
            working_evicted: 0,
            note: None,
            note_edit: None,
            note_sealed: false,
            arrange: crate::arrange::Counts::default(),
            arrange_sort: SortKey::default(),
            arrange_descending: false,
            arrange_filter: refx_core::query::Filter::default(),
            arrange_apply: false,
            meta: None,
            meta_request: None,
            meta_sealed: false,
            tag_input: String::new(),
            group: None,
            close_prompt: false,
            close_choice: None,
            storage: StorageView::default(),
            storage_request: None,
            save_as_prompt: false,
            save_as_choice: None,
            export_prompt: None,
            export_request: None,
            export_progress: None,
            recover_prompt: None,
            recover_choice: None,
            group_request: None,
            group_sealed: false,
            picked: None,
            measured: None,
            loading: None,
            settings_open: false,
            settings: SettingsView::default(),
            settings_edit: None,
            settings_sealed: false,
            settings_notes: Vec::new(),
            settings_notes_dismissed: false,
            keymap: KeymapView::default(),
        }
    }
}

/// วาด shell ทั้งหมด แล้วเรียก `viewport` ให้วาดเนื้อในช่องกลาง
///
/// เรียกจากข้างใน `egui::Context::run_ui` ซึ่งส่ง `&mut Ui` ของ root มาให้
/// (egui 0.34 ไม่มี `Panel::show(ctx)` แล้ว มีแต่ `show_inside(ui)`)
///
/// คืน **rect ของช่องกลาง (หน่วย point)** — ผู้เรียกต้องใช้ค่านี้ตั้ง viewport
/// ของ render pass และเป็นกรอบอ้างอิงของกล้อง ไม่ใช่ขนาดหน้าต่างทั้งบาน
///
/// ★ `viewport` ได้รับ [`Mode`] **ที่จะวาดจริงในเฟรมนี้** ส่งเข้าไปด้วย (P3-3)
/// ปุ่มสลับโหมดอยู่บน toolbar ซึ่งถูกวาด *ก่อน* ช่องกลางเสมอ ผู้เรียกที่อ่าน
/// `state.mode` ไว้ก่อนเรียกฟังก์ชันนี้จะได้ค่า **เก่าไปหนึ่งเฟรม** ในเฟรมที่ผู้ใช้
/// เพิ่งกดสลับ แล้ววาดของประดับของอีกโหมดทับลงไป
#[must_use = "ต้องเอา rect ไปตั้ง viewport ของ canvas ไม่งั้นภาพจะเยื้อง"]
pub fn draw_in_ui(
    ui: &mut egui::Ui,
    state: &mut ShellState,
    viewport: impl FnOnce(&mut egui::Ui, Mode),
) -> egui::Rect {
    // ★ อ่านครั้งเดียวต้นเฟรม — ทุก widget ข้างล่างใช้ค่าเดียวกัน
    let lang = state.lang;

    // ---- แถวบน: board tabs ----
    egui::Panel::top("refx-tabs").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("RefX").strong());
            ui.separator();
            // ★★★ **ตัวบ่งชี้ถาวรของสภาวะ "ยังไม่ถูกบันทึก"** (docs/03 §1, P4-4)
            //
            //   "งานนี้ยังไม่เคยถูกบันทึก" ไม่ใช่ *เหตุการณ์* ที่เกิดแล้วจบ —
            //   เป็น **สภาวะ** ที่คงอยู่จนกว่าผู้ใช้จะกด Save · ข้อความชั่วคราว
            //   บน status bar จึงเป็นเครื่องมือผิดชนิด และมันพิสูจน์ตัวเองแล้ว:
            //   ข้อความ "กู้คืนแล้ว — กด Ctrl+S เพื่อเก็บไว้" ถูกรายงานความคืบหน้า
            //   ของการโหลดภาพเขียนทับใน **~3 มิลลิวินาที** ผู้ใช้ที่เพิ่งได้งานคืน
            //   จึงไม่มีทางรู้ว่างานนั้นยังไม่ถูกบันทึก แล้วปิดโปรแกรมทิ้งได้อีกรอบ
            //   ซึ่งวนกลับไปที่เดิมพอดี
            //
            //   ★ จุดนี้ถูกเลือกเพราะมันคือ **ชื่อของเอกสาร** — ที่ที่คนมองหา
            //     คำตอบว่า "ฉันกำลังแก้ไฟล์ไหนอยู่" อยู่แล้วโดยสัญชาตญาณ
            //     และเป็นที่เดียวกับที่โปรแกรมแก้ไขทุกตัวใส่จุด/ดอกจันไว้
            //
            //   ★★★ P4-7c: ทุกแท็บมีดาวของตัวเอง — ผู้ใช้ที่เห็นดาวแค่ใบที่อยู่
            //     หน้าจอจะปิดโปรแกรมโดยเชื่อว่าอีกสามใบสะอาด ซึ่งเป็นลูปเดิม
            //     ที่ตัวบ่งชี้ถาวรมีไว้แก้พอดี
            for (index, tab) in state.tabs.iter().enumerate() {
                let title = tab
                    .title
                    .as_deref()
                    .unwrap_or_else(|| text::t(lang, Key::UntitledBoard));
                let label = if tab.unsaved {
                    // ★ เครื่องหมายนำหน้า **ไม่ใช่สี** อย่างเดียว — คนตาบอดสีต้องอ่านออกด้วย
                    //   (สีถูกใช้เสริม ไม่ใช่ใช้แทน)
                    egui::RichText::new(format!("{UNSAVED_MARK} {title}")).color(warn_color(ui))
                } else {
                    egui::RichText::new(title.to_owned())
                };
                let hint = if tab.unsaved {
                    text::t(lang, Key::UnsavedHint)
                } else {
                    text::t(lang, Key::SavedHint)
                };
                if ui
                    .selectable_label(index == state.active_tab, label)
                    .on_hover_text(hint)
                    .clicked()
                {
                    state.tab_request = Some(TabRequest::Select(index));
                }
                // ★ กากบาทของแท็บ — ทางเดียวกับ `Ctrl+W` เป๊ะ (ถามก่อนถ้ายังไม่บันทึก)
                if ui
                    .small_button("x")
                    .on_hover_text(text::t(lang, Key::CloseTabHint))
                    .clicked()
                {
                    state.tab_request = Some(TabRequest::Close(index));
                }
                ui.separator();
            }
            if ui
                .button("+")
                .on_hover_text(text::t(lang, Key::NewBoardHint))
                .clicked()
            {
                state.tab_request = Some(TabRequest::New);
            }
        });
    });

    // ---- ★ แถบยืนยันตอนปิดทั้งที่ยังไม่ได้บันทึก (P4-2) ----
    //
    //   วางไว้ **บนสุดใต้แท็บ** เพื่อให้เห็นแน่ ๆ แต่ยังเห็นงานข้างหลังอยู่
    //   — ต่างจาก native dialog ที่บังทุกอย่างและบล็อก UI thread (I-2)
    if state.close_prompt {
        egui::Panel::top("refx-close-confirm").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                // ★ หัวข้อต้องพูดตรงกับ **ขอบเขต** ที่ถาม — ปุ่มชุดเดียวกันแต่
                //   สิ่งที่หายถ้าตอบผิดต่างกัน (ปิดแท็บใบเดียว vs ปิดทั้งโปรแกรม)
                let title = if state.close_scope_tab {
                    Key::CloseTabTitle
                } else {
                    Key::CloseUnsavedTitle
                };
                ui.label(
                    egui::RichText::new(text::t(lang, title))
                        .strong()
                        .color(warn_color(ui)),
                );
                ui.separator();
                // ★ ปุ่มที่ **ปลอดภัยที่สุดมาก่อน** — ผู้ใช้ที่กดเร็วโดยไม่อ่าน
                //   ต้องเจอทางที่ไม่ทำงานหายก่อนเสมอ
                if ui.button(text::t(lang, Key::CloseSaveFirst)).clicked() {
                    state.close_choice = Some(CloseChoice::SaveThenClose);
                }
                if ui.button(text::t(lang, Key::CloseCancel)).clicked() {
                    state.close_choice = Some(CloseChoice::Cancel);
                }
                ui.separator();
                if ui
                    .button(text::t(lang, Key::CloseDiscard))
                    .on_hover_text(text::t(lang, Key::CloseDiscardHint))
                    .clicked()
                {
                    state.close_choice = Some(CloseChoice::DiscardAndClose);
                }
            });
        });
    }

    // ---- ★★ แถบ "บันทึกเป็นแบบไหน" (P4-5 · `docs/07 §2`) ----
    //
    //   native dialog ของ `rfd` ใส่ตัวเลือกของเราเองเข้าไปไม่ได้ · ถามในหน้าต่าง
    //   ก่อนแล้วค่อยเปิด dialog จึงเป็นทางเดียว — และมันไม่บล็อก UI thread (I-2)
    //
    //   ★ ถามเฉพาะ **บันทึกเป็น** เท่านั้น · `Ctrl+S` ใช้โหมดเดิมของเอกสารเงียบ ๆ
    //     (คำถามที่โผล่ทุกครั้งที่กดบันทึก คือคำถามที่คนกดผ่านโดยไม่อ่าน)
    if state.save_as_prompt {
        egui::Panel::top("refx-save-as").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(text::t(lang, Key::SaveModeTitle)).strong());
                ui.separator();
                // ★ ค่าปริยายของ docs/07 §2 มาก่อน — และเป็นตัวที่ผู้ใช้ส่วนใหญ่ต้องการ
                if ui
                    .button(text::t(lang, Key::SaveModeLinked))
                    .on_hover_text(text::t(lang, Key::SaveModeLinkedHint))
                    .clicked()
                {
                    state.save_as_choice = Some(SaveAsChoice::Linked);
                }
                if ui
                    .button(text::t(lang, Key::SaveModePacked))
                    .on_hover_text(text::t(lang, Key::SaveModePackedHint))
                    .clicked()
                {
                    state.save_as_choice = Some(SaveAsChoice::Packed);
                }
                ui.separator();
                if ui.button(text::t(lang, Key::CloseCancel)).clicked() {
                    state.save_as_choice = Some(SaveAsChoice::Cancel);
                }
            });
        });
    }

    // ---- ★★ แถบส่งออกภาพ (P5-4 · `docs/07 §6`) ----
    //
    //   วางที่เดียวกับแถบอื่นด้วยเหตุผลเดียวกัน: เห็นแน่ แต่ยังเห็นกระดานข้างหลัง
    //   ซึ่งสำคัญเป็นพิเศษที่นี่ — ผู้ใช้ต้องเห็นสิ่งที่กำลังจะถูกส่งออก
    // ★ เก็บคำขอไว้ในตัวแปรก่อน แล้วค่อยเขียนกลับหลังจบบล็อก — `view` ยืม
    //   `state` แบบ mut อยู่ การเขียน `state.export_request` ข้างในจึงยืมซ้อน
    let mut export_request = None;
    if let Some(view) = state.export_prompt.as_mut() {
        egui::Panel::top("refx-export").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(text::t(lang, Key::ExportTitle)).strong());
                ui.separator();

                ui.label(text::t(lang, Key::ExportSize));
                // ★ ปรับ "ด้านยาวสุด" ตัวเดียว — สัดส่วนมาจาก board เสมอ
                // ★★★ ช่วงจบที่ **พิกเซลที่มีจริงรองรับได้** ไม่ใช่ที่เพดานของ GPU
                //     (`docs/07 §6`) — ลากเกินไปกว่านั้นไม่ได้เลย ไม่ใช่ลากได้แล้วเตือน
                ui.add(
                    egui::DragValue::new(&mut view.long_side)
                        .speed(16.0)
                        .range(refx_core::export::MIN_EXPORT_SIDE..=view.max_side),
                );
                ui.label(text::fill(
                    lang,
                    Template::ExportPixels,
                    &[
                        ("w", &view.width.to_string()),
                        ("h", &view.height.to_string()),
                    ],
                ));
                ui.label(text::fill(
                    lang,
                    Template::ExportEstimate,
                    &[("size", &human_bytes(view.estimate))],
                ));
                // ★★★ **บอกเหตุผล ไม่ใช่จำกัดเงียบ ๆ** (`docs/07 §6`)
                //     ผู้ใช้ที่ลากตัวเลขแล้วมันหยุด ต้องรู้ว่าหยุดเพราะอะไร
                //     ไม่งั้นเขาจะอ่านว่าโปรแกรมพัง · ขึ้นเฉพาะตอนที่เพดานจริง
                //     ต่ำกว่าเพดานของทางเขียน — board ที่ไม่ติดขีดจะไม่เห็นบรรทัดนี้
                if view.max_side < refx_core::export::MAX_SIDE {
                    ui.label(
                        egui::RichText::new(text::fill(
                            lang,
                            Template::ExportCeiling,
                            &[("n", &view.max_side.to_string())],
                        ))
                        .color(warn_color(ui)),
                    );
                }

                ui.separator();
                ui.label(text::t(lang, Key::ExportFormat));
                if ui
                    .selectable_label(view.kind == ExportKind::Png, text::t(lang, Key::ExportPng))
                    .on_hover_text(text::t(lang, Key::ExportPngHint))
                    .clicked()
                {
                    view.kind = ExportKind::Png;
                }
                if ui
                    .selectable_label(
                        view.kind == ExportKind::Jpeg,
                        text::t(lang, Key::ExportJpeg),
                    )
                    .on_hover_text(text::t(lang, Key::ExportJpegHint))
                    .clicked()
                {
                    view.kind = ExportKind::Jpeg;
                }

                match view.kind {
                    // ★ ช่องติ๊ก "โปร่งใส" มีเฉพาะ PNG — JPEG ไม่มี alpha
                    //   การแสดงตัวเลือกที่ไม่มีผลคือการโกหกผู้ใช้
                    ExportKind::Png => {
                        ui.checkbox(&mut view.transparent, text::t(lang, Key::ExportTransparent));
                    }
                    ExportKind::Jpeg => {
                        ui.label(text::t(lang, Key::ExportQuality));
                        ui.add(egui::DragValue::new(&mut view.quality).range(1..=100));
                    }
                }
                // ★★ สีพื้นยังต้องเลือกได้แม้ตอนติ๊กโปร่งใส — ภาพที่มีส่วนโปร่ง
                //    บางส่วนยังต้องรู้ว่าอีกส่วนผสมลงสีอะไร
                ui.label(text::t(lang, Key::ExportBackground));
                ui.color_edit_button_srgb(&mut view.background);
            });

            ui.horizontal_wrapped(|ui| {
                if view.choosing {
                    ui.label(text::t(lang, Key::ExportChoosing));
                } else if ui.button(text::t(lang, Key::ExportChoose)).clicked() {
                    export_request = Some(ExportRequest::ChooseTarget);
                }
                if let Some(target) = &view.target {
                    ui.label(target);
                }
                ui.separator();

                // ★★★ ปุ่มเริ่มมีได้ก็ต่อเมื่อรู้แล้วว่าจะเขียนลงไฟล์ไหน —
                //     ไม่งั้นผู้ใช้จะกด "ส่งออก" แล้วไม่มีอะไรเกิดขึ้น
                let ready = view.target.is_some() && !view.choosing;
                if view.overwrite {
                    // ★★ "ทับไฟล์ที่มีอยู่ = ถามก่อน" (`docs/07 §6` ข้อ 4)
                    //    ถามด้วยการ **เปลี่ยนสิ่งที่ปุ่มพูด** ไม่ใช่กล่องซ้อนอีกชั้น
                    //    ที่คนกดผ่านโดยไม่อ่าน
                    if ui
                        .add_enabled(
                            ready,
                            egui::Button::new(
                                egui::RichText::new(text::t(lang, Key::ExportOverwrite))
                                    .color(warn_color(ui)),
                            ),
                        )
                        .on_hover_text(text::t(lang, Key::ExportOverwriteHint))
                        .clicked()
                    {
                        export_request = Some(ExportRequest::Start);
                    }
                } else if ui
                    .add_enabled(ready, egui::Button::new(text::t(lang, Key::ExportStart)))
                    .clicked()
                {
                    export_request = Some(ExportRequest::Start);
                }
                if ui.button(text::t(lang, Key::ExportClose)).clicked() {
                    export_request = Some(ExportRequest::Close);
                }
            });

            // ★★★ **คำเตือน `Missing` ต้องอยู่ก่อนปุ่ม ไม่ใช่หลังจากกดไปแล้ว**
            //
            //   export คือสิ่งที่ผู้ใช้ส่งให้คนอื่น · ถ้าภาพหายไปสามใบแล้วรู้ทีหลัง
            //   คือความเสียหายที่ย้อนไม่ได้ (`docs/07 §6`)
            if view.missing > 0 {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(text::fill(
                            lang,
                            Template::ExportMissingCount,
                            &[("n", &view.missing.to_string())],
                        ))
                        .strong()
                        .color(warn_color(ui)),
                    );
                    ui.label(text::t(lang, Key::ExportMissingWarning));
                });
            }
            if let Some(problem) = &view.problem {
                ui.label(
                    egui::RichText::new(problem.as_str())
                        .strong()
                        .color(warn_color(ui)),
                );
            }
        });
    }
    if export_request.is_some() {
        state.export_request = export_request;
    }

    // ---- ★ แถบความคืบหน้าของ export ที่กำลังทำอยู่ (P5-4) ----
    //
    //   ★★ **ไม่บล็อกอะไรเลย** — งานอยู่บน worker ผู้ใช้ยังลากภาพ ซูม
    //   สลับโหมดได้ตามปกติ (I-2 · `docs/07 §6` ข้อ 1)
    if let Some(progress) = state.export_progress.clone() {
        egui::Panel::top("refx-export-progress").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(text::fill(
                    lang,
                    Template::ExportRunning,
                    &[
                        ("name", &progress.name),
                        ("done", &progress.done.to_string()),
                        ("total", &progress.total.to_string()),
                    ],
                ));
                ui.separator();
                if progress.cancelling {
                    ui.label(text::t(lang, Key::ExportCancelling));
                } else if ui.button(text::t(lang, Key::ExportCancel)).clicked() {
                    state.export_request = Some(ExportRequest::Cancel);
                }
            });
        });
    }

    // ---- ★★★ แถบกู้คืนงานที่ยังไม่ได้บันทึกจากรอบก่อน (P4-4) ----
    //
    //   วางไว้ที่เดียวกับแถบยืนยันตอนปิด ด้วยเหตุผลเดียวกัน (เห็นแน่ แต่ยัง
    //   เห็นงานข้างหลัง · ไม่บล็อก UI thread แบบ native dialog — I-2)
    if let Some(found) = state.recover_prompt.clone() {
        egui::Panel::top("refx-recover").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                // ★ ประโยคต้องตรงกับที่มาของงาน ไม่ใช่ประโยคเดียวใช้ทุกกรณี
                let title = match found.scope {
                    RecoverScope::LastSession => Key::RecoverTitle,
                    RecoverScope::ThisDocument => Key::RecoverDocTitle,
                    RecoverScope::KeptForLater => Key::RecoverKeptTitle,
                };
                ui.label(
                    egui::RichText::new(text::t(lang, title))
                        .strong()
                        .color(warn_color(ui)),
                );
                ui.label(text::fill(
                    lang,
                    Template::RecoverFound,
                    &[
                        ("items", &found.items.to_string()),
                        (
                            "when",
                            found
                                .when
                                .as_deref()
                                .unwrap_or_else(|| text::t(lang, Key::RecoverWhenUnknown)),
                        ),
                    ],
                ));
                ui.separator();
                // ★★ เรียงตาม **ความปลอดภัย** เหมือนแถบตอนปิด: ทางที่ไม่ทำงานหาย
                //    ต้องมาก่อนเสมอสำหรับคนที่กดเร็วโดยไม่อ่าน
                if ui.button(text::t(lang, Key::RecoverRestore)).clicked() {
                    state.recover_choice = Some(RecoverChoice::Restore);
                }
                // ★★★ ตัวที่สาม — สำคัญที่สุดตาม docs/07 §4 · **ไม่แตะไฟล์เลย**
                // ★★ คำอธิบายของ "เก็บไว้ก่อน" ต้อง **บอกความจริงของแต่ละกรณี**:
                //    snapshot ของ session ก่อนอยู่คนละไฟล์กับที่เราเขียน จึงรอดแน่
                //    ส่วนของเอกสารอยู่ที่ `<doc>.refx.autosave` ซึ่งเป็นไฟล์เดียวกับ
                //    ที่ autosave ของเราจะเขียนทับเมื่อผู้ใช้แก้อะไรต่อ — ปิดบังข้อนี้
                //    แล้วปุ่มจะกลายเป็นคำโกหกในอีกสิบวินาทีถัดมา
                let later_hint = match found.scope {
                    RecoverScope::LastSession | RecoverScope::KeptForLater => Key::RecoverLaterHint,
                    // ★ ตัวนี้ย้ายไฟล์จริง จึงพูดได้เต็มปากว่าไม่มีอะไรถูกเขียนทับ
                    RecoverScope::ThisDocument => Key::RecoverDocLaterHint,
                };
                if ui
                    .button(text::t(lang, Key::RecoverLater))
                    .on_hover_text(text::t(lang, later_hint))
                    .clicked()
                {
                    state.recover_choice = Some(RecoverChoice::Later);
                }
                ui.separator();
                if ui
                    .button(text::t(lang, Key::RecoverDiscard))
                    .on_hover_text(text::t(lang, Key::RecoverDiscardHint))
                    .clicked()
                {
                    state.recover_choice = Some(RecoverChoice::Discard);
                }
            });
        });
    }

    // ---- แถวสอง: mode switch + tools ----
    egui::Panel::top("refx-toolbar").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            // ★ ส่วน shared: ปุ่มสลับ mode เขียนครั้งเดียว
            for mode in [Mode::Canvas, Mode::Arrange] {
                if ui
                    .selectable_label(state.mode == mode, mode.label())
                    .clicked()
                {
                    state.mode = mode;
                    state.status =
                        text::fill(lang, Template::SwitchedMode, &[("mode", mode.label())]);
                }
            }
            ui.separator();

            // ★ ส่วนที่ต่างกันตาม mode — จุดเดียวที่แยกสองทาง
            match state.mode {
                Mode::Canvas => canvas_tools(ui, state),
                Mode::Arrange => arrange_tools(ui, state),
            }

            // ★ ปุ่มตั้งค่าชิดขวาสุด (P5-3) — ห่างจากเครื่องมือที่ใช้ทุกนาที
            //   เพราะมันเป็นของที่แตะปีละครั้ง การวางไว้ข้าง ๆ ปุ่ม crop
            //   คือการเพิ่มโอกาสกดผิดโดยไม่ได้อะไรตอบแทน
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // ★★ ปุ่มเปลี่ยน**สี** เมื่อ `settings.toml` มีปัญหา แทนที่จะมีจุด
                //    เตือนแยกอีกดวง · ผู้ใช้ที่ปิดรายการปัญหาไปแล้วต้องยังหาทาง
                //    กลับมาดูได้ ไม่ใช่ข้อความที่ผ่านไปแล้วผ่านเลย (บทเรียน
                //    เดียวกับตัวบ่งชี้โหมดของ P4-5)
                //
                //    ★★★ **ห้ามเติมอักขระสัญลักษณ์ตัวใหม่มาเป็นจุดเตือน** —
                //    รุ่นแรกของโค้ดนี้ใช้ `●` ซึ่งเป็นตัวเดียวกับที่เคยออกมาเป็น
                //    สี่เหลี่ยม tofu ตอนทำแท็บ และ `the_unsaved_mark_has_a_real_glyph`
                //    บันทึกไว้แล้วว่ามันไม่มี glyph ในฟอนต์ที่เราฝัง · เห็นอีกครั้ง
                //    บนภาพหน้าจอจริงของ P5-3 (`docs/08 §3.9` ข้อ 5)
                let trouble = !state.settings_notes.is_empty();
                let icon = if trouble {
                    egui::RichText::new(SETTINGS_MARK).color(warn_color(ui))
                } else {
                    egui::RichText::new(SETTINGS_MARK)
                };
                let hint = if trouble {
                    Key::SettingsProblemsStatus
                } else {
                    Key::SettingsHint
                };
                if ui
                    .selectable_label(state.settings_open, icon)
                    .on_hover_text(text::t(lang, hint))
                    .clicked()
                {
                    state.settings_open = !state.settings_open;
                }
                // ★★ ปุ่มส่งออก (P5-4) — **ข้อความล้วน ไม่ใช่สัญลักษณ์**
                //    บทเรียนสามครั้งในไฟล์นี้: สัญลักษณ์ตัวใหม่ทุกตัวคือโอกาส
                //    ได้สี่เหลี่ยม tofu อีกครั้ง · คำว่า "ส่งออก" อ่านออกแน่นอน
                //    และเป็นปุ่มที่กดปีละหลายครั้ง ไม่ใช่ทุกนาที จึงไม่เปลืองที่
                //    ★ ปิดระหว่างมีงานอยู่ — export สองงานพร้อมกันแย่ง VRAM กันเปล่า ๆ
                if ui
                    .add_enabled(
                        state.export_progress.is_none(),
                        egui::Button::new(text::t(lang, Key::ExportTitle)),
                    )
                    .on_hover_text(text::t(lang, Key::ExportHint))
                    .clicked()
                {
                    state.export_request = Some(ExportRequest::Open);
                }
            });
        });
    });

    // ---- ล่างสุด: status bar (ต้องประกาศก่อน panel ซ้าย/ขวาเพื่อให้กินเต็มความกว้าง) ----
    egui::Panel::bottom("refx-status").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            // ★ ความคืบหน้ามาก่อนทุกอย่าง — เป็นสิ่งเดียวที่ผู้ใช้อยากรู้ตอนกำลังโหลด
            //   (เงื่อนไขข้อ 3 ของ docs/05 §6 ที่ทำให้ cache เย็นยอมรับได้)
            if let Some(progress) = state.loading {
                ui.add(
                    egui::ProgressBar::new(progress.fraction())
                        .desired_width(120.0)
                        .text(progress.label(lang)),
                );
            } else if state.status_warn {
                // ★ ข้อความที่บอกว่า "ของที่คุณขอไม่ได้เข้ามาครบ" ต้องเห็นได้
                //   ไม่ใช่กลืนไปกับ "พร้อม" (P3-3)
                ui.colored_label(warn_color(ui), &state.status);
            } else {
                ui.label(&state.status);
            }
            // ★ สีที่จิ้มได้ (P2-10) — ตัวอย่างสีคู่กับ hex เสมอ
            //   ตัวเลขอย่างเดียวอ่านไม่ออกด้วยตา ส่วนสีอย่างเดียวก๊อปไปใช้ไม่ได้
            if let Some(picked) = state.picked {
                ui.separator();
                let (rect, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
                let [r, g, b, _] = picked.rgba;
                ui.painter()
                    .rect_filled(rect, 2.0, egui::Color32::from_rgb(r, g, b));
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(1.0, egui::Color32::from_gray(140)),
                    egui::StrokeKind::Middle,
                );
                ui.label(picked.hex()).on_hover_text(format!(
                    "source pixel {}, {}",
                    picked.source_px.0, picked.source_px.1
                ));
            }
            // ★ ไม้บรรทัด (P2-10) — หน่วยเป็น world ตัวเลขจึงไม่ขยับตอนซูม
            if let Some(m) = state.measured {
                ui.separator();
                let extent = m.extent();
                ui.label(format!(
                    "{:.1} u  ({:.1} x {:.1})  {:.1}°",
                    m.length(),
                    extent.x,
                    extent.y,
                    m.angle_deg()
                ));
            }
            ui.separator();
            ui.label(text::fill(
                lang,
                Template::ItemCount,
                &[("n", &state.item_count.to_string())],
            ));
            ui.separator();
            storage_indicator(ui, state);
            // ★★ หลักฐานของเกณฑ์ P3-3 ที่เห็นได้ด้วยตา — "10,000 ใบ วาดจริง < 60"
            //    โชว์เฉพาะโหมด Arrange เพราะเป็นตัวเลขของ virtual scrolling
            //    (Canvas วาดทั้ง board อยู่แล้ว ตัวเลขจะไม่มีความหมายที่นั่น)
            if state.mode == Mode::Arrange {
                // ★★ กรองอยู่ = ต้องเห็นได้เสมอ ผู้ใช้ที่มองหาภาพที่ "หายไป"
                //   ต้องรู้ทันทีว่ามันถูกกรอง ไม่ใช่หาย (ไม่งั้นอ่านว่าโปรแกรมทำงานหาย)
                if !state.arrange_filter.is_open() {
                    ui.separator();
                    ui.colored_label(
                        warn_color(ui),
                        text::fill(
                            lang,
                            Template::FilterShowing,
                            &[
                                ("shown", &state.arrange.total.to_string()),
                                ("total", &state.item_count.to_string()),
                            ],
                        ),
                    );
                }
                ui.separator();
                let arrange = state.arrange;
                ui.label(text::fill(
                    lang,
                    Template::ArrangeDrawn,
                    &[
                        ("drawn", &arrange.in_band.to_string()),
                        ("view", &arrange.in_view.to_string()),
                        ("total", &arrange.total.to_string()),
                        ("examined", &arrange.examined.to_string()),
                    ],
                ));
            }
            ui.separator();
            ui.label(text::fill(
                lang,
                Template::Zoom,
                &[("pct", &format!("{:.0}", state.zoom * 100.0))],
            ));
            ui.separator();
            // I-1 ให้เห็นกับตา: ตัวเลขนี้ต้องหยุดนิ่งเมื่อไม่แตะอะไร
            ui.label(text::fill(
                lang,
                Template::FramesDrawn,
                &[("n", &state.frames_drawn.to_string())],
            ));
            // ★★★ ตัวจับ I-1: โผล่เฉพาะตอนมีคนขอวาดต่อเนื่องทั้งที่ไม่มี input
            //     ใช้สีเตือนเพราะมันคือ **อาการ** ไม่ใช่ตัวเลขประจำวัน
            if let Some((len, who)) = state.quiet_redraws {
                ui.separator();
                ui.colored_label(
                    warn_color(ui),
                    text::fill(
                        lang,
                        Template::QuietRedraws,
                        &[("n", &len.to_string()), ("who", who)],
                    ),
                );
            }
            ui.separator();

            // ★ I-6 ให้เห็นกับตา: RAM ที่ decode pool ใช้ เทียบกับเพดานรวมทุก worker
            let ram = text::fill(
                lang,
                Template::Ram,
                &[
                    ("used", &human_bytes(state.ram_used as u64)),
                    ("limit", &human_bytes(state.ram_limit as u64)),
                ],
            );
            if state.ram_limit > 0 && state.ram_used * 10 > state.ram_limit * 9 {
                // ใกล้เต็ม — ให้เห็นชัดว่ากำลังตึง
                ui.colored_label(warn_color(ui), ram);
            } else {
                ui.label(ram);
            }

            ui.separator();
            // ★ I-6: VRAM ต้องเห็นด้วยตาเหมือน RAM
            let vram = text::fill(
                lang,
                Template::Vram,
                &[
                    ("used", &human_bytes(state.vram_used as u64)),
                    ("limit", &human_bytes(state.vram_limit as u64)),
                ],
            );
            if state.vram_limit > 0 && state.vram_used * 10 > state.vram_limit * 9 {
                ui.colored_label(warn_color(ui), vram);
            } else {
                ui.label(vram);
            }

            ui.separator();
            ui.label(text::fill(
                lang,
                Template::CacheSummary,
                &[
                    ("n", &state.cache_thumbs.to_string()),
                    ("size", &human_bytes(state.cache_bytes)),
                ],
            ));

            // ★ I-6: ชั้น B มีงบของตัวเอง ต้องเห็นด้วยตาเหมือน RAM/VRAM
            if state.working_limit > 0 && state.working_used > 0 {
                ui.separator();
                ui.label(text::fill(
                    lang,
                    Template::WorkingTextures,
                    &[
                        ("used", &human_bytes(state.working_used as u64)),
                        ("limit", &human_bytes(state.working_limit as u64)),
                        ("calls", &state.draw_calls.to_string()),
                        ("evicted", &state.working_evicted.to_string()),
                    ],
                ));
            }

            // ★ หลักฐานของเกณฑ์ P2-8 ที่เห็นได้ด้วยตา — สลับ `G` แล้วเลขนี้ต้องนิ่ง
            ui.separator();
            ui.label(text::fill(
                lang,
                Template::AtlasUploads,
                &[("uploads", &state.atlas_uploads.to_string())],
            ));

            if state.decode_queued > 0 {
                ui.separator();
                ui.label(text::fill(
                    lang,
                    Template::DecodeQueued,
                    &[("n", &state.decode_queued.to_string())],
                ));
            }
            if state.decode_cancelled > 0 {
                ui.separator();
                ui.label(text::fill(
                    lang,
                    Template::DecodeCancelled,
                    &[("n", &state.decode_cancelled.to_string())],
                ));
            }
        });
    });

    // ---- ซ้าย: library ----
    egui::Panel::left("refx-library")
        .default_size(200.0)
        .show_inside(ui, |ui| {
            ui.heading(text::t(lang, Key::Library));
            ui.separator();
            ui.label(text::t(lang, Key::LibraryPlaceholder));
            ui.small(text::t(lang, Key::LibraryDropHint));
        });

    // ---- ★ ขวาสุด: แผงตั้งค่า (P5-3) ----
    //
    //   วางไว้ **นอกกว่า** inspector เพื่อไม่ให้มันดันเนื้องานหาย: ผู้ใช้ที่เปิด
    //   แผงนี้กำลังตั้งค่า ไม่ได้กำลังจัดภาพ · ปิดแล้วทุกอย่างกลับเหมือนเดิม
    if state.settings_open {
        egui::Panel::right("refx-settings")
            .default_size(260.0)
            .show_inside(ui, |ui| settings_panel(ui, state));
    }

    // ---- ขวา: inspector ----
    egui::Panel::right("refx-inspector")
        .default_size(240.0)
        .show_inside(ui, |ui| {
            ui.heading(text::t(lang, Key::Inspector));
            ui.separator();
            // docs/03 §1: inspector ปรับตัวตาม mode
            match state.mode {
                Mode::Canvas => canvas_inspector(ui, state),
                Mode::Arrange => arrange_inspector(ui, state),
            }
        });

    // ---- กลาง: viewport ของ mode ปัจจุบัน ----
    //
    // ★ `Frame::NONE` สำคัญมาก ห้ามเอาออก
    //   ภาพของผู้ใช้ถูกวาดด้วย wgpu **ใต้** egui อีกที (docs/04 §2 — pass เดียว)
    //   ถ้า CentralPanel ทาพื้นหลังทึบตาม theme (`panel_fill` ซึ่งเป็นค่าปริยาย)
    //   มันจะกลบ quad ทุกอันจนหมด แล้ว canvas จะว่างเปล่าทั้งที่ทุกอย่างทำงานถูก
    //   — อาการนี้เกิดจริงตั้งแต่ P0-6 และไม่มี log ไหนจับได้เลยเพราะการวาดสำเร็จหมด
    //
    //   (egui 0.34: `Frame::none()` ถูก deprecate แล้ว ต้องใช้ `Frame::NONE`)
    let mode = state.mode;
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| viewport(ui, mode))
        .response
        .rect
}

/// ★★★ **ตัวบ่งชี้ถาวรว่าภาพของ board นี้เก็บไว้ที่ไหน** (P4-5 · `docs/07 §2`)
///
/// ## ทำไมเป็น *สภาวะ* ไม่ใช่ *เหตุการณ์*
///
/// "ไฟล์นี้พกภาพไปด้วยหรือเปล่า" เป็นจริงค้างอยู่จนกว่าจะบันทึกใหม่ ไม่ใช่สิ่งที่
/// เกิดแล้วจบ · ข้อความชั่วคราวบนแถบสถานะถูกเขียนทับได้ใน 3 มิลลิวินาที
/// (เกิดจริงสองครั้งแล้วในโปรเจกต์นี้ — §2.24, §2.30) แล้วคนที่กำลังจะส่งไฟล์
/// ให้เพื่อนจะไม่มีทางรู้ว่าเพื่อนจะได้ board ที่ว่างเปล่า
///
/// ## ★★ ทำไมมันกดได้
///
/// การแปลงเอกสารที่มีอยู่แล้วให้พกภาพไปด้วย เป็นสิ่งที่ผู้ใช้ต้องการ *หลัง*
/// ทำงานเสร็จ ("จะส่งให้เพื่อนแล้ว") ไม่ใช่ตอนตั้งชื่อไฟล์ครั้งแรก · การบังคับ
/// ให้เขาไป Save As แล้วตั้งชื่อใหม่ทั้งที่ต้องการไฟล์เดิม คือการสร้างไฟล์ซ้ำสอง
/// ใบที่เขาต้องมาตามลบเอง
fn storage_indicator(ui: &mut egui::Ui, state: &mut ShellState) {
    let lang = state.lang;
    let view = state.storage;
    let label = if view.packed {
        text::t(lang, Key::StoragePacked)
    } else {
        text::t(lang, Key::StorageLinked)
    };
    // ★ บอก **ตัวเลขจริง** ไม่ใช่แค่ชื่อโหมด — "3 จาก 5 ใบอยู่ข้างใน" ตอบคำถาม
    //   ที่ผู้ใช้ถามจริง ๆ ได้ ส่วนชื่อโหมดลอย ๆ ต้องรู้ศัพท์ของเราก่อนถึงจะอ่านออก
    let counts = text::fill(
        lang,
        Template::StorageInside,
        &[
            ("inside", &view.inside.to_string()),
            ("images", &view.images.to_string()),
        ],
    );
    let hint = if view.packed {
        text::t(lang, Key::StoragePackedHint)
    } else if view.has_file {
        text::t(lang, Key::StorageLinkedHint)
    } else {
        // ยังไม่มีไฟล์ = ยังไม่มีอะไรให้แปลง ตัวเลือกมีผลตอนบันทึกครั้งแรก
        text::t(lang, Key::StorageUnsavedHint)
    };
    if ui
        .selectable_label(view.packed, label)
        .on_hover_text(format!("{counts}\n{hint}"))
        .clicked()
    {
        state.storage_request = Some(!view.packed);
    }
}

/// ★★★ แผงตั้งค่า (P5-3) — memory budget · theme · present mode
///
/// ## กติกาที่ทำให้แผงนี้ไม่ทำให้เปิดโปรแกรมไม่ขึ้น
///
/// ตัวควบคุมทุกตัวเป็น **ช่วงปิด** (`range` ของ `DragValue`) จึงพิมพ์ค่าที่อยู่นอก
/// ช่วงลงไปไม่ได้ตั้งแต่ต้น — ด่านใน `refx_io::settings` ยังอยู่ครบสำหรับคนที่แก้
/// ไฟล์เอง แต่ทางนี้ **ไม่มีทางผลิตไฟล์ที่ตัวเองอ่านกลับไม่ได้**
///
/// ★ เพดานของ `max_pixels` ผูกกับ **RAM ของเครื่อง** (`HANDOFF §4` ข้อ 3) จึงเป็น
/// เพดานของตัวควบคุมเลย ไม่ใช่แค่ข้อความเตือน — ผู้ใช้จะเลื่อนเกินไม่ได้
///
/// ## ทำไมต้องมี `settings_sealed`
///
/// การลากตัวเลขหนึ่งครั้งผลิตค่าใหม่ทุกเฟรม ถ้าเขียนไฟล์ตามนั้นจะได้การเขียน
/// ดิสก์หลายสิบรอบต่อการลากหนึ่งครั้ง — บน UI thread ซึ่ง I-2 ห้ามไว้
/// จึงเขียนตอน **ปล่อย** เท่านั้น (หลักการเดียวกับ `appearance_sealed`)
fn settings_panel(ui: &mut egui::Ui, state: &mut ShellState) {
    use refx_io::settings::{Present, Theme};

    let lang = state.lang;
    let view = state.settings;
    let mut edit = view;
    // ★ "ปล่อยแล้ว" ของทั้งแผง — ปุ่มถือว่าปล่อยทันที ส่วนตัวเลขรอ `drag_stopped`
    let mut sealed = false;

    ui.horizontal(|ui| {
        ui.heading(text::t(lang, Key::SettingsTitle));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(text::t(lang, Key::SettingsClose)).clicked() {
                state.settings_open = false;
            }
        });
    });
    ui.separator();

    // ---- ★★ เรื่องที่ settings.toml ไม่ได้ถูกใช้ตามที่เขียน ----
    //
    //   อยู่ **บนสุด** เพราะเป็นเหตุผลเดียวที่แผงนี้เปิดขึ้นมาเองได้ · ผู้ใช้ที่
    //   ตั้งค่าผิดต้องเห็นคำอธิบายก่อนตัวควบคุม ไม่ใช่ต้องเลื่อนหา
    if !state.settings_notes.is_empty() {
        ui.label(
            egui::RichText::new(text::t(lang, Key::SettingsProblemsTitle))
                .strong()
                .color(warn_color(ui)),
        );
        for note in &state.settings_notes {
            ui.label(egui::RichText::new(text::settings_note(lang, note)).color(warn_color(ui)));
        }
        if ui
            .button(text::t(lang, Key::SettingsProblemsDismiss))
            .clicked()
        {
            state.settings_notes_dismissed = true;
        }
        ui.separator();
    }

    // ---- ธีม — ★ ตัวเดียวในแผงนี้ที่เห็นผลทันที ----
    ui.label(egui::RichText::new(text::t(lang, Key::SettingsTheme)).strong());
    ui.horizontal(|ui| {
        for (theme, key) in [
            (Theme::Dark, Key::SettingsThemeDark),
            (Theme::Light, Key::SettingsThemeLight),
        ] {
            if ui
                .selectable_label(edit.theme == theme, text::t(lang, key))
                .clicked()
            {
                edit.theme = theme;
                sealed = true;
            }
        }
    });
    ui.add_space(6.0);

    // ---- จังหวะการแสดงเฟรม ----
    ui.label(egui::RichText::new(text::t(lang, Key::SettingsPresent)).strong());
    ui.horizontal(|ui| {
        for (present, key, hint) in [
            (Present::Vsync, Key::SettingsPresentVsync, None),
            (
                Present::Uncapped,
                Key::SettingsPresentUncapped,
                Some(Key::SettingsPresentUncappedHint),
            ),
        ] {
            let mut button = ui.selectable_label(edit.present == present, text::t(lang, key));
            if let Some(hint) = hint {
                button = button.on_hover_text(text::t(lang, hint));
            }
            if button.clicked() {
                edit.present = present;
                sealed = true;
            }
        }
    });
    ui.add_space(6.0);

    // ---- หน่วยความจำ (docs/05 §2) ----
    ui.label(egui::RichText::new(text::t(lang, Key::SettingsMemory)).strong());

    ui.horizontal(|ui| {
        ui.label(text::t(lang, Key::SettingsRamLimit));
        let response = ui.add(
            egui::DragValue::new(&mut edit.ram_limit_mb)
                .range(refx_io::settings::RAM_LIMIT_MB)
                .speed(8.0)
                .suffix(" MB"),
        );
        if response.drag_stopped() || response.lost_focus() {
            sealed = true;
        }
        response.on_hover_text(text::t(lang, Key::SettingsRamLimitHint));
    });

    ui.horizontal(|ui| {
        ui.label(text::t(lang, Key::SettingsVramLimit));
        // ★ "อัตโนมัติ" เป็น **ค่าหนึ่งของช่องนี้** ไม่ใช่ช่องแยก — ผู้ใช้ที่เคย
        //   ตั้งเองแล้วอยากกลับไปให้โปรแกรมเลือก ต้องมีทางกลับที่ชัดเจน
        //   (ไม่งั้นเขาจะเดาว่าต้องพิมพ์ 0 แล้วได้เพดาน 0 MB ซึ่งใช้ไม่ได้)
        let auto = edit.vram_limit_mb.is_none();
        if ui
            .selectable_label(auto, text::t(lang, Key::SettingsVramAuto))
            .clicked()
        {
            edit.vram_limit_mb = if auto {
                Some(*refx_io::settings::VRAM_LIMIT_MB.start().max(&384))
            } else {
                None
            };
            sealed = true;
        }
        if let Some(mb) = edit.vram_limit_mb.as_mut() {
            let response = ui.add(
                egui::DragValue::new(mb)
                    .range(refx_io::settings::VRAM_LIMIT_MB)
                    .speed(8.0)
                    .suffix(" MB"),
            );
            if response.drag_stopped() || response.lost_focus() {
                sealed = true;
            }
        }
    })
    .response
    .on_hover_text(text::t(lang, Key::SettingsVramLimitHint));

    ui.horizontal(|ui| {
        ui.label(text::t(lang, Key::SettingsMaxPixels));
        // ★★★ เพดานบนของตัวควบคุมคือเพดานของ **เครื่องนี้** ไม่ใช่ค่าคงที่
        //     (`HANDOFF §4` ข้อ 3) — เลื่อนเกินไม่ได้ ไม่ใช่เลื่อนได้แล้วค่อยบ่น
        let ceiling = view
            .max_pixels_ceiling
            .max(refx_io::settings::MIN_MAX_PIXELS);
        let response = ui.add(
            egui::DragValue::new(&mut edit.max_pixels)
                .range(refx_io::settings::MIN_MAX_PIXELS.min(ceiling)..=ceiling)
                .speed(100_000.0),
        );
        if response.drag_stopped() || response.lost_focus() {
            sealed = true;
        }
        response.on_hover_text(text::t(lang, Key::SettingsMaxPixelsHint));
    });
    // ★ จำนวนจุดเป็นตัวเลขที่ไม่มีใครคิดในหัว — บอกด้านเทียบเท่าไว้ด้วย
    let side = (view.max_pixels_ceiling as f64).sqrt() as u64;
    ui.small(text::fill(
        lang,
        Template::SettingsCeiling,
        &[
            ("side", &side.to_string()),
            ("pixels", &view.max_pixels_ceiling.to_string()),
        ],
    ));

    // ---- ★★ ค่าที่ยังไม่มีผล ----
    //
    //   เพดานหน่วยความจำถูกอ่านตอนสร้าง pool/allocator เท่านั้น การเปลี่ยนกลางคัน
    //   ต้องรื้อ threading model ซึ่ง `CLAUDE.md` บอกให้ถามก่อน — **บอกความจริง
    //   ดีกว่าแกล้งทำเป็นว่ามันมีผลแล้ว** ผู้ใช้ที่เห็นตัวเลขบนแถบสถานะไม่ขยับ
    //   จะเชื่อว่าแผงนี้เสียทั้งแผง
    if view.needs_restart {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(text::t(lang, Key::SettingsNeedsRestart))
                .small()
                .color(warn_color(ui)),
        );
    }

    // ---- ★★ คีย์ลัด (P5-3b ก้อน b) — **อ่านอย่างเดียว** ----
    ui.add_space(8.0);
    ui.separator();
    keymap_section(ui, state);

    if edit != view {
        state.settings_edit = Some(edit);
    }
    if sealed {
        state.settings_sealed = true;
    }
}

/// ★★ คีย์ลัดที่ใช้อยู่ — **แสดงอย่างเดียว ยังแก้ในแอปไม่ได้** (P5-3b ก้อน b)
///
/// ตอบสองคำถามที่ผู้ใช้ถามก่อนจะเปิด `keymap.toml`:
/// *"ตอนนี้มีปุ่มอะไรบ้าง"* และ *"ต้องพิมพ์ชื่ออะไรลงไฟล์"*
///
/// ★★★ **ต้องบอกว่าตารางมาจากไหน** — ไฟล์ของผู้ใช้ *แทนที่ทั้งชุด* คนที่เขียน
/// ไปสามบรรทัดแล้วเหลือคีย์ลัดสามปุ่มต้องเห็นทันทีว่านั่นคือสิ่งที่เกิดขึ้น
/// ไม่ใช่ว่าโปรแกรมทำงานหาย
fn keymap_section(ui: &mut egui::Ui, state: &ShellState) {
    let lang = state.lang;
    let view = &state.keymap;

    ui.label(egui::RichText::new(text::t(lang, Key::SettingsKeymap)).strong());

    // ★ เหตุที่ keymap.toml ใช้ไม่ได้ มาก่อนรายการ — ผู้ใช้ที่ไฟล์พังต้องเห็น
    //   คำอธิบายก่อน ไม่ใช่ต้องเลื่อนผ่านตาราง 27 แถวไปหา
    if let Some(problem) = view.problem.as_deref() {
        ui.label(
            egui::RichText::new(text::t(lang, Key::KeymapFellBackToDefaults))
                .strong()
                .color(warn_color(ui)),
        );
        ui.label(egui::RichText::new(problem).color(warn_color(ui)));
    }

    let source = if view.from_file {
        Key::SettingsKeymapFromFile
    } else {
        Key::SettingsKeymapBuiltin
    };
    ui.small(format!(
        "{} · {}",
        text::t(lang, source),
        text::fill(
            lang,
            Template::SettingsKeymapCount,
            &[("n", &view.rows.len().to_string())],
        )
    ));
    ui.small(text::t(lang, Key::SettingsKeymapHint));

    // ★ รายการยาว 27 แถว — ต้องเลื่อนได้ ไม่งั้นมันดันตัวควบคุมข้างบนหายจากจอ
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (chord, action) in &view.rows {
                ui.horizontal(|ui| {
                    ui.monospace(chord);
                    // ★★★ ASCII เท่านั้น — `→` (U+2192) **ไม่มี glyph** ในฟอนต์ที่
                    //     ฝังไว้ และออกมาเป็นสี่เหลี่ยม tofu บนภาพหน้าจอจริงของ
                    //     P5-3b · เป็นครั้งที่สามของบั๊กเดิม (`UNSAVED_MARK` ·
                    //     จุดเตือนของ P5-3a · ตัวนี้) และสองครั้งแรกมีค่าคงที่
                    //     คอยคุมไว้แล้ว — **ตัวอักษรที่เขียนสด ๆ ในโค้ดคือรูที่เหลือ**
                    //     ปิดด้วย `every_character_the_shell_draws_has_a_glyph`
                    ui.monospace("->");
                    ui.monospace(*action);
                });
            }
        });
}

/// ปุ่มเครื่องมือของ Canvas mode
fn canvas_tools(ui: &mut egui::Ui, state: &mut ShellState) {
    use refx_core::interact::Tool;

    let lang = state.lang;
    // ★ เครื่องมือจริงเป็นปุ่มสลับที่ชี้ `state.tool` ตัวเดียวกับคีย์ลัด
    //   (`ToolMove` ไม่ได้อยู่ที่นี่แล้ว — docs/03 §2 ระบุว่าการย้ายเป็นส่วนหนึ่งของ
    //   Select/Move ปุ่ม `V` ตัวเดียว การมีปุ่มแยกทำให้เข้าใจผิดว่าเป็นคนละโหมด)
    for (tool, key, hint) in [
        (Tool::Select, Key::ToolSelect, "V"),
        (Tool::Crop, Key::ToolCrop, "C"),
        (Tool::Picker, Key::ToolPicker, "I"),
        (Tool::Measure, Key::ToolMeasure, "M"),
        (Tool::Text, Key::ToolText, "T"),
    ] {
        let label = text::t(lang, key);
        if ui
            .selectable_label(state.tool == tool, label)
            .on_hover_text(hint)
            .clicked()
        {
            state.tool_request = Some(tool);
        }
    }
    // ★ grayscale ทั้ง board — เป็นปุ่มสลับจริงตั้งแต่ P2-8 (ชี้ค่าเดียวกับปุ่ม `G`)
    if ui
        .selectable_label(state.board_grayscale, text::t(lang, Key::ToolGrayscale))
        .on_hover_text("G")
        .clicked()
    {
        state.board_grayscale = !state.board_grayscale;
    }

    ui.separator();
    // ★ จัดเรียง (P2-9) — ป้ายบนปุ่มอยู่ใน `ARRANGE_BUTTONS` คำอธิบายอยู่ใน tooltip
    for (request, glyph, key) in ARRANGE_BUTTONS {
        if ui.button(glyph).on_hover_text(text::t(lang, key)).clicked() {
            state.arrange_request = Some(request);
        }
    }
}

/// ★★ สัญลักษณ์ทุกตัวที่ Arrange inspector วาด (P3-1)
///
/// อยู่รวมกันที่นี่เพื่อให้เทสต์ `arrange_inspector_glyphs_all_exist` ไล่ตรวจได้
/// **ทุกครั้งที่ build** — เพิ่ม/เปลี่ยนสัญลักษณ์เมื่อไหร่ เทสต์จะแดงทันที
/// ถ้าฟอนต์ที่เราฝังไม่มี glyph นั้น (บทเรียนจาก P2-9)
///
/// ★ `#[cfg(test)]` เพราะโค้ดจริงใช้ค่าคงที่แต่ละตัวโดยตรง รายการนี้มีไว้ให้เทสต์
/// ไล่เท่านั้น · **ข้อจำกัดที่ต้องรู้:** มันกันการ *เปลี่ยนค่า* ของสัญลักษณ์ที่มีอยู่
/// ได้แน่นอน แต่กัน "เพิ่มสัญลักษณ์ใหม่แล้วลืมมาต่อท้ายที่นี่" ไม่ได้
/// (ต่างจาก `ARRANGE_BUTTONS` ที่ตัวมันเองคือข้อมูลที่ UI วาด จึงลืมไม่ได้เชิงโครงสร้าง)
#[cfg(test)]
pub(crate) const META_GLYPHS: [&str; 3] = [STAR_FULL, STAR_EMPTY, LABEL_NONE];

/// ★ ดาวที่ให้แล้ว (U+2605) — ★ ตรวจแล้วว่าฟอนต์ที่ฝังมีจริง
const STAR_FULL: &str = "\u{2605}";
/// ดาวที่ยังไม่ให้ (U+2606)
const STAR_EMPTY: &str = "\u{2606}";
/// ล้างป้ายสี (U+00D7)
///
/// ★ ตรวจแล้วว่า U+2713 (check) กับ U+25CF (วงกลมทึบ) **ไม่มี** ในฟอนต์ที่ฝัง
/// จึงใช้ไม่ได้ — เดาไม่ได้ ต้องตรวจ (บทเรียนจาก P2-9)
const LABEL_NONE: &str = "\u{00D7}";

// ★ ปักหมุดใช้ `checkbox` ของ egui — egui วาดเครื่องหมายเอง
// จึงไม่ต้องพึ่ง glyph ในฟอนต์เลย (U+2713 ไม่มีในฟอนต์ที่เราฝัง)

/// ★★ ปุ่มจัดเรียงทั้งแปด — **ป้ายต้องเป็นตัวอักษรที่ฟอนต์ที่เราฝังมี glyph จริง**
///
/// รอบแรกใช้สัญลักษณ์เส้นตาราง ซึ่งดูเหมาะที่สุด แต่ทั้งฟอนต์ละตินของ egui และ
/// Noto Sans Thai **ไม่มี glyph พวกนั้น** ผลคือปุ่มขึ้นเป็นกล่องสี่เหลี่ยมว่างบนจอจริง
/// โดยไม่มี error ที่ไหนเลย (docs/03 §0: ไม่ฝัง CJK เพราะเพดาน binary)
///
/// `↔`/`↕` ใช้ได้เพราะ Noto Sans Thai มีให้ ส่วน `← → ↑ ↓` **ไม่มี** — เดาไม่ได้
/// ต้องตรวจ · เทสต์ `arrange_button_labels_all_have_glyphs` ยืนยันให้ทุกครั้งที่ build
pub(crate) const ARRANGE_BUTTONS: [(ArrangeRequest, &str, Key); 8] = [
    (ArrangeRequest::Align(A::Left), "L", Key::AlignLeft),
    (ArrangeRequest::Align(A::CentreX), "C", Key::AlignCentreX),
    (ArrangeRequest::Align(A::Right), "R", Key::AlignRight),
    (ArrangeRequest::Align(A::Top), "T", Key::AlignTop),
    (ArrangeRequest::Align(A::CentreY), "M", Key::AlignCentreY),
    (ArrangeRequest::Align(A::Bottom), "B", Key::AlignBottom),
    (
        ArrangeRequest::Distribute(D::Horizontal),
        "↔",
        Key::DistributeX,
    ),
    (
        ArrangeRequest::Distribute(D::Vertical),
        "↕",
        Key::DistributeY,
    ),
];

/// ★ inspector ของ Canvas mode — ช่องที่ docs/03 §1 กำหนดไว้ (opacity, filter)
///
/// ตัวควบคุมเขียนลง `state.appearance` เท่านั้น **ไม่แตะ `Board` เลย** ชั้น `app`
/// เป็นคนเทียบกับค่าเดิมแล้วห่อเป็น `SetFilter` เข้า `History` — ทางเดียวที่กฎ
/// "ทุก mutation ผ่าน Command" ยังบังคับได้จริงเมื่อ widget เป็นคนแก้ค่า
fn canvas_inspector(ui: &mut egui::Ui, state: &mut ShellState) {
    use refx_core::board::Flip;

    let lang = state.lang;
    ui.label(text::t(lang, Key::InspectorCanvasGeometry));
    ui.separator();

    // ★★★ ภาพที่หาไฟล์ไม่เจอ (P4-6 ขั้นที่ 4/5) — มาก่อนทุกช่อง เพราะถ้าภาพหาย
    //   สิ่งที่ผู้ใช้อยากทำคือหามันให้เจอ ไม่ใช่ปรับ opacity ของช่องว่าง
    if let Some(missing) = state.missing.clone() {
        ui.label(text::fill(
            lang,
            text::Template::MissingImage,
            &[("file", &missing.file), ("n", &missing.count.to_string())],
        ));
        if ui.button(text::t(lang, Key::FindFile)).clicked() {
            state.relink_request = true;
        }
        ui.separator();
    }

    // ★ โน้ตข้อความ (P2-11) — ช่องนี้มาก่อนเพราะเป็นทั้งหมดที่โน้ตมีให้แก้
    //   ★★ egui กิน keyboard ให้เองเมื่อช่องนี้มี focus และชั้น `app` หยุด
    //   คีย์ลัดทุกตัวตาม `egui_wants_keyboard_input()` — space จึงเป็นอักขระจริง
    if let Some(note) = state.note.as_mut() {
        ui.label(text::t(lang, Key::Note));
        let response = ui.add(
            egui::TextEdit::multiline(note)
                .desired_width(f32::INFINITY)
                .desired_rows(6)
                .hint_text(text::t(lang, Key::NoteHint)),
        );
        let wanted = note.clone();
        // เขียน "สิ่งที่ผู้ใช้ขอ" เฉพาะตอนมีคนพิมพ์จริง — เจ้าของเดียว ไม่มีการทับกัน
        state.note_edit = response.changed().then_some(wanted);
        // ออกจากช่องแล้ว = จบหนึ่งขั้น undo · ระหว่างพิมพ์อยู่ยังยุบเป็นขั้นเดียว
        state.note_sealed = response.lost_focus();
        ui.separator();
    } else {
        state.note_edit = None;
        state.note_sealed = false;
    }

    let Some(appearance) = state.appearance.as_mut() else {
        if state.note.is_none() {
            ui.label(text::t(lang, Key::InspectorNoSelection));
        }
        return;
    };

    // ★ `touched` = ผู้ใช้แตะตัวควบคุมในเฟรมนี้จริง ๆ · `released` = ปล่อยแล้ว (seal)
    //   ถ้าไม่แยกสองอย่างนี้ ค่าที่ค้างอยู่จะถูกเขียนกลับลง board ทุกเฟรม
    //   แล้วมันจะทับสิ่งที่คีย์ลัดเพิ่งเปลี่ยน (เคยเป็นบั๊กจริงตอนทำ P2-8)
    let mut touched = false;
    let mut released = false;
    let mut note = |response: &egui::Response| {
        touched |= response.changed();
        released |= response.drag_stopped() || response.lost_focus();
    };

    ui.label(text::t(lang, Key::Opacity));
    note(&ui.add(egui::Slider::new(&mut appearance.opacity, 0.0..=1.0).show_value(true)));

    ui.separator();
    note(&ui.checkbox(&mut appearance.grayscale, text::t(lang, Key::ToolGrayscale)));
    note(&ui.checkbox(&mut appearance.invert, text::t(lang, Key::Invert)));

    ui.label(text::t(lang, Key::Brightness));
    note(&ui.add(egui::Slider::new(&mut appearance.brightness, -1.0..=1.0)));

    ui.label(text::t(lang, Key::Contrast));
    note(&ui.add(egui::Slider::new(&mut appearance.contrast, -1.0..=1.0)));

    ui.separator();
    ui.label(text::t(lang, Key::Flip));
    ui.horizontal(|ui| {
        for (flip, label) in [
            (Flip::None, "—"),
            (Flip::Horizontal, "↔"),
            (Flip::Vertical, "↕"),
            (Flip::Both, "⇄⇅"),
        ] {
            if ui
                .selectable_label(appearance.flip == flip, label)
                .clicked()
            {
                appearance.flip = flip;
                touched = true;
                // ปุ่มเป็นการกดครั้งเดียว ต้อง seal ทันที ไม่งั้นการกดถัดไปถูกกลืน
                released = true;
            }
        }
    });

    let wanted = *appearance;
    // ★★ เขียน "สิ่งที่ผู้ใช้ขอ" **เฉพาะตอนมีคนแตะจริง** — เจ้าของเดียว ไม่มีการทับกัน
    state.appearance_edit = touched.then_some(wanted);
    state.appearance_sealed = released;
}

/// ★ inspector ของ Arrange mode — tag / rating / color label / pinned / note (P3-1)
///
/// ★★ **ทุกตัวควบคุมเขียนลง `state.meta_request` เท่านั้น ไม่แตะ `Board` เลย**
/// ชั้น `app` เป็นคนห่อเป็น `EditMeta` / `TagItems` เข้า `History` — ทางเดียวที่กฎ
/// "ทุก mutation ผ่าน Command" ยังบังคับได้จริงเมื่อ widget เป็นคนแก้ค่า
///
/// ★ เขียนคำขอ **ทีละหนึ่ง** ต่อเฟรม (enum ไม่ใช่ struct) เพราะ `EditMeta`
/// merge **ต่อ field**: ให้ดาวแล้วใส่แท็กต้องเป็นคนละขั้น undo ถ้าส่งทั้งก้อน
/// ทุกเฟรม เราจะแยกไม่ออกว่าผู้ใช้เพิ่งแตะอะไร (docs/02 §3)
fn arrange_inspector(ui: &mut egui::Ui, state: &mut ShellState) {
    use refx_core::board::ColorLabel;

    let lang = state.lang;
    ui.label(text::t(lang, Key::InspectorArrangeMeta));
    ui.separator();

    let Some(meta) = state.meta.clone() else {
        ui.label(text::t(lang, Key::InspectorNoSelection));
        state.meta_request = None;
        state.meta_sealed = false;
        // ★ ต้องล้างคำขอของกลุ่มด้วย ไม่งั้นคำขอของเฟรมก่อนค้างอยู่แล้วถูกเขียนซ้ำ
        //   ทุกเฟรม → ทับสิ่งที่ undo เพิ่งคืนมา (docs/08 §3.9 ข้อ 8.1)
        state.group_request = None;
        state.group_sealed = false;
        return;
    };
    let mut request = None;
    let mut sealed = false;

    // ---- ดาว ----
    ui.label(text::t(lang, Key::Rating));
    ui.horizontal(|ui| {
        for star in 1..=refx_core::board::ItemMeta::MAX_RATING {
            let filled = meta.rating >= star;
            let glyph = if filled { STAR_FULL } else { STAR_EMPTY };
            if ui.selectable_label(filled, glyph).clicked() {
                // ★ กดดาวที่ให้อยู่แล้ว = ล้างเป็น 0 — ไม่งั้นลดดาวไม่ได้เลย
                request = Some(MetaRequest::Rating(if meta.rating == star {
                    0
                } else {
                    star
                }));
                sealed = true;
            }
        }
        ui.label(format!("{}", meta.rating));
    });

    // ---- ป้ายสี ----
    ui.separator();
    ui.label(text::t(lang, Key::ColorLabelTitle));
    ui.horizontal(|ui| {
        // ล้างป้าย
        if ui
            .selectable_label(meta.color_label.is_none(), LABEL_NONE)
            .on_hover_text(text::t(lang, Key::ColorLabelNone))
            .clicked()
        {
            request = Some(MetaRequest::ColorLabel(None));
            sealed = true;
        }
        for label in ColorLabel::CHOICES {
            let picked = meta.color_label == Some(label);
            let (rect, response) =
                ui.allocate_exact_size(egui::Vec2::splat(18.0), egui::Sense::click());
            if let Some([r, g, b]) = label.rgb() {
                ui.painter()
                    .rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
            }
            if picked {
                ui.painter().rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                    egui::StrokeKind::Middle,
                );
            }
            if response.clicked() {
                request = Some(MetaRequest::ColorLabel(Some(label)));
                sealed = true;
            }
        }
    });
    // ★★ ป้ายที่รุ่นนี้ไม่รู้จัก — บอกตรง ๆ **ห้ามวาดเป็นสีใดสีหนึ่ง**
    //    ถ้าเดาสีให้ ผู้ใช้จะคิดว่ามันเป็นสีจริงแล้วเผลอกดทับ ซึ่งคือการทำลาย
    //    ค่าที่เราอุตส่าห์ถือไว้เพื่อเขียนกลับ (docs/02 §2.9)
    if meta.color_label.is_some_and(|label| !label.is_known()) {
        ui.small(text::t(lang, Key::ColorLabelUnknown));
    }

    // ---- ปักหมุด ----
    ui.separator();
    let mut pinned = meta.pinned;
    if ui
        .checkbox(&mut pinned, text::t(lang, Key::Pinned))
        .changed()
    {
        request = Some(MetaRequest::Pinned(pinned));
        sealed = true;
    }

    // ---- แท็ก ----
    ui.separator();
    ui.label(text::t(lang, Key::Tags));
    ui.horizontal_wrapped(|ui| {
        for name in &meta.tags {
            // กดที่แท็ก = ถอดออก · tooltip บอกไว้เพราะเดาเองไม่ได้
            if ui
                .button(name)
                .on_hover_text(text::t(lang, Key::RemoveTagHint))
                .clicked()
            {
                request = Some(MetaRequest::RemoveTag(name.clone()));
                sealed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut state.tag_input)
                .desired_width(120.0)
                .hint_text(text::t(lang, Key::AddTagHint)),
        );
        // Enter หรือกดปุ่ม — ทั้งสองทางต้องได้ผลเดียวกัน
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (entered || ui.button("+").clicked()) && !state.tag_input.trim().is_empty() {
            request = Some(MetaRequest::AddTag(std::mem::take(&mut state.tag_input)));
            sealed = true;
        }
    });

    // ---- โน้ต ----
    ui.separator();
    ui.label(text::t(lang, Key::MetaNote));
    let mut note = meta.note.clone();
    let response = ui.add(
        egui::TextEdit::multiline(&mut note)
            .desired_width(f32::INFINITY)
            .desired_rows(4)
            .hint_text(text::t(lang, Key::NoteHint)),
    );
    if response.changed() {
        request = Some(MetaRequest::Note(note));
    }
    sealed |= response.lost_focus();

    state.meta_request = request;
    state.meta_sealed = sealed;

    // ---- กลุ่ม (P3-7) ----
    ui.separator();
    group_section(ui, state);
}

/// ★ ส่วนของกลุ่มในแผง Arrange (P3-7)
///
/// ★★ **ช่องเปลี่ยนชื่อโผล่เฉพาะตอนที่เลือกอยู่ในกลุ่มเดียวกันทั้งหมด**
/// — ถ้าโผล่ตอน `Mixed` ด้วย การพิมพ์ครั้งเดียวจะเขียนทับชื่อของกลุ่มที่ผู้ใช้
/// แค่บังเอิญเลือกติดมา ซึ่งเป็นการแก้ข้อมูลที่เขาไม่ได้สั่ง
///
/// เขียนลง `state.group_request` เท่านั้น ไม่แตะ `Board` เลย — เหมือนทุกแผงอื่น
fn group_section(ui: &mut egui::Ui, state: &mut ShellState) {
    let lang = state.lang;
    ui.label(text::t(lang, Key::GroupTitle));

    let mut request = None;
    let mut sealed = false;
    match state.group.clone() {
        None | Some(GroupView::Loose) => {
            ui.small(text::t(lang, Key::GroupNone));
        }
        Some(GroupView::Mixed) => {
            ui.small(text::t(lang, Key::GroupMixed));
        }
        Some(GroupView::One {
            id,
            name,
            collapsed,
            members,
        }) => {
            let mut edited = name.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut edited)
                    .desired_width(f32::INFINITY)
                    .hint_text(text::t(lang, Key::GroupRenameHint)),
            );
            if response.changed() {
                request = Some(GroupRequest::Rename(id, edited));
            }
            sealed |= response.lost_focus();

            let mut folded = collapsed;
            if ui
                .checkbox(&mut folded, text::t(lang, Key::GroupCollapsed))
                .on_hover_text(text::t(lang, Key::GroupCollapsedHint))
                .changed()
            {
                request = Some(GroupRequest::Collapsed(id, folded));
                sealed = true;
            }
            ui.small(text::fill(
                lang,
                Template::GroupMembers,
                &[("n", &members.to_string())],
            ));
        }
    }

    state.group_request = request;
    state.group_sealed = sealed;
}

/// วิธีเรียงทั้งหมดที่ผู้ใช้เลือกได้ + ชื่อของมัน (P3-4)
///
/// ★ อยู่รวมกันที่นี่เหมือน `ARRANGE_BUTTONS` — เพิ่ม `SortKey` ใหม่แล้วคอมไพเลอร์
/// ไม่ฟ้อง แต่เทสต์ `every_sort_key_can_be_picked_from_the_toolbar` ฟ้องแทน
/// (ตัวเลือกที่มีในโค้ดแต่กดไม่ได้ = ฟีเจอร์ที่ไม่มีอยู่จริงสำหรับผู้ใช้)
pub(crate) const SORT_CHOICES: [(SortKey, Key); 8] = [
    (SortKey::AddedAt, Key::SortAddedAt),
    (SortKey::Name, Key::SortName),
    (SortKey::Rating, Key::SortRating),
    (SortKey::ColorLabel, Key::SortColorLabel),
    (SortKey::AspectRatio, Key::SortAspect),
    (SortKey::ModifiedAt, Key::SortModifiedAt),
    (SortKey::FileSize, Key::SortFileSize),
    (SortKey::CanvasOrder, Key::SortCanvasOrder),
];

/// ปุ่มเครื่องมือของ Arrange mode — เรียง + กรอง (P3-4)
///
/// ★★ **widget เขียนลง `state` ตรง ๆ ได้ที่นี่** ต่างจาก inspector ที่ต้องแยก
/// "ค่าที่แสดง" ออกจาก "สิ่งที่ผู้ใช้ขอ" — เพราะการเรียง/กรองเป็นสถานะของ
/// *เครื่องมือ* ที่ **ไม่มีใครอื่นเขียนเลย** (ไม่มีคีย์ลัด ไม่มี undo ไม่ได้มาจาก
/// board) จึงไม่มีลำดับการเขียนให้ผิดได้ · ชั้น `app` อ่านไปเทียบกับของเดิม
/// แล้วค่อยลงมือ ซึ่งทำให้ "ตั้งค่าเดิมซ้ำ" ไม่ขอเฟรมใหม่ (I-1)
fn arrange_tools(ui: &mut egui::Ui, state: &mut ShellState) {
    use refx_core::board::ColorLabel;
    use refx_core::query::{Filter, LabelFilter};

    let lang = state.lang;

    // ---- เรียง ----
    ui.label(text::t(lang, Key::ToolSort));
    let current = SORT_CHOICES
        .iter()
        .find(|(key, _)| *key == state.arrange_sort)
        .map_or(Key::SortAddedAt, |(_, label)| *label);
    egui::ComboBox::from_id_salt("refx-sort")
        .selected_text(text::t(lang, current))
        .show_ui(ui, |ui| {
            for (key, label) in SORT_CHOICES {
                ui.selectable_value(&mut state.arrange_sort, key, text::t(lang, label));
            }
        });
    // ★ ทิศทางเป็น **คำ** ไม่ใช่ลูกศร — ฟอนต์ที่ฝังไม่มี U+2191/2193
    //   (ตรวจแล้วตอน P2-9: มีแต่ลูกศรสองหัว) และคำอ่านออกทันทีว่าทางไหน
    let direction = if state.arrange_descending {
        Key::SortDescending
    } else {
        Key::SortAscending
    };
    if ui.button(text::t(lang, direction)).clicked() {
        state.arrange_descending = !state.arrange_descending;
    }

    ui.separator();

    // ---- กรอง ----
    ui.label(text::t(lang, Key::ToolFilter));

    // ดาวขั้นต่ำ: กดดาวดวงที่ N = "อย่างน้อย N ดาว" · กดซ้ำดวงเดิม = ล้าง
    for stars in 1..=refx_core::board::ItemMeta::MAX_RATING {
        let on = state.arrange_filter.min_rating >= stars;
        let glyph = if on { STAR_FULL } else { STAR_EMPTY };
        if ui
            .selectable_label(on, glyph)
            .on_hover_text(text::t(lang, Key::FilterMinRating))
            .clicked()
        {
            state.arrange_filter.min_rating = if state.arrange_filter.min_rating == stars {
                0
            } else {
                stars
            };
        }
    }

    // ป้ายสี: ช่องสีเหมือนใน inspector — ★ ไม่ใช้ ComboBox เพราะป้ายสีไม่มี "ชื่อ"
    // ในโปรแกรมนี้ (มีแต่สี) การตั้งชื่อให้มันที่นี่ที่เดียวจะเป็นคำที่ผู้ใช้ไม่เคยเห็น
    if ui
        .selectable_label(
            state.arrange_filter.label == LabelFilter::Unlabelled,
            LABEL_NONE,
        )
        .on_hover_text(text::t(lang, Key::ColorLabelNone))
        .clicked()
    {
        state.arrange_filter.label = if state.arrange_filter.label == LabelFilter::Unlabelled {
            LabelFilter::Any
        } else {
            LabelFilter::Unlabelled
        };
    }
    for label in ColorLabel::CHOICES {
        let picked = state.arrange_filter.label == LabelFilter::Is(label);
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::splat(16.0), egui::Sense::click());
        if let Some([r, g, b]) = label.rgb() {
            ui.painter()
                .rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
        }
        if picked {
            ui.painter().rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(2.0, egui::Color32::WHITE),
                egui::StrokeKind::Middle,
            );
        }
        if response.clicked() {
            // กดซ้ำสีเดิม = เลิกกรอง (ไม่ต้องหาปุ่มล้าง)
            state.arrange_filter.label = if picked {
                LabelFilter::Any
            } else {
                LabelFilter::Is(label)
            };
        }
    }

    ui.separator();

    // ---- ส่งผลการจัดลง canvas (P3-5) ----
    if ui
        .button(text::t(lang, Key::ToolSendToCanvas))
        .on_hover_text(text::t(lang, Key::SendToCanvasHint))
        .clicked()
    {
        state.arrange_apply = true;
    }

    ui.add(
        egui::TextEdit::singleline(&mut state.arrange_filter.text)
            .desired_width(130.0)
            .hint_text(text::t(lang, Key::FilterSearchHint)),
    );

    // ★ ปุ่มล้างโผล่เฉพาะตอนกรองอยู่จริง — ปุ่มที่กดแล้วไม่มีอะไรเกิดขึ้น
    //   สอนผู้ใช้ว่าปุ่มบนแถบนี้เชื่อถือไม่ได้
    if !state.arrange_filter.is_open() && ui.button(text::t(lang, Key::FilterClear)).clicked() {
        state.arrange_filter = Filter::default();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// ★★ inspector ต้อง **ไม่ขออะไร** ถ้าผู้ใช้ไม่ได้แตะมันในเฟรมนั้น
    ///
    /// เคยเป็นบั๊กจริงตอน P2-8: `appearance` ถูกอ่านกลับไปเขียนลง board ทุกเฟรม
    /// ค่าที่ค้างจากเฟรมก่อนจึง**ทับสิ่งที่คีย์ลัด `H` เพิ่งเปลี่ยน** — ภาพพลิกแล้วเด้งกลับ
    /// ทันทีโดยไม่มี error ที่ไหนเลย และ unit test ของแต่ละชิ้นก็ผ่านหมด
    /// เพราะแต่ละชิ้นถูกจริง ๆ สิ่งที่ผิดอยู่ระหว่างชิ้น (`docs/08 §3.9` ข้อ 8)
    #[test]
    fn the_inspector_asks_for_nothing_when_nobody_touches_it() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            appearance: Some(Appearance {
                opacity: 0.5,
                grayscale: true,
                invert: false,
                brightness: 0.25,
                contrast: -0.25,
                flip: refx_core::board::Flip::Horizontal,
            }),
            ..ShellState::default()
        };

        // วาดหลายเฟรมโดยไม่มี pointer/keyboard เลย
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let _ = draw_in_ui(ui, &mut state, |_, _| {});
            });
            assert!(
                state.appearance_edit.is_none(),
                "ไม่มีใครแตะ แต่ inspector กลับขอให้เขียนค่าลง board"
            );
        }
    }

    /// ★★ ช่องโน้ตต้อง **ไม่ขออะไร** ถ้าผู้ใช้ไม่ได้พิมพ์ในเฟรมนั้น (P2-11)
    ///
    /// เหตุผลเดียวกับ `the_inspector_asks_for_nothing_when_nobody_touches_it`:
    /// ถ้าค่าที่ค้างอยู่ถูกเขียนกลับลง board ทุกเฟรม มันจะทับสิ่งที่ undo เพิ่งคืนมา
    /// อาการคือ **กด Ctrl+Z แล้วข้อความเด้งกลับ** โดยไม่มี error ที่ไหนเลย
    #[test]
    fn the_note_field_asks_for_nothing_when_nobody_types() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            note: Some("เขียนไว้แล้ว".to_owned()),
            ..ShellState::default()
        };

        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let _ = draw_in_ui(ui, &mut state, |_, _| {});
            });
            assert!(
                state.note_edit.is_none(),
                "ไม่มีใครพิมพ์ แต่ช่องโน้ตกลับขอให้เขียนลง board"
            );
            assert!(!state.note_sealed);
        }
    }

    /// ไม่ได้เลือกโน้ต = ต้องไม่มีคำขอค้างจากรอบก่อน
    #[test]
    fn deselecting_a_note_clears_any_pending_edit() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            note: None,
            note_edit: Some("ค้างจากรอบก่อน".to_owned()),
            note_sealed: true,
            ..ShellState::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let _ = draw_in_ui(ui, &mut state, |_, _| {});
        });
        assert!(state.note_edit.is_none());
        assert!(!state.note_sealed);
    }

    /// ★★ แผง Arrange ต้อง **ไม่ขออะไร** ถ้าผู้ใช้ไม่ได้แตะมันในเฟรมนั้น (P3-1)
    ///
    /// เหตุผลเดียวกับแผง Canvas: ค่าที่ค้างแล้วถูกเขียนกลับทุกเฟรมจะทับสิ่งที่
    /// undo เพิ่งคืนมา — กด Ctrl+Z แล้วดาว/แท็กเด้งกลับทันที
    #[test]
    fn the_arrange_panel_asks_for_nothing_when_nobody_touches_it() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            mode: Mode::Arrange,
            meta: Some(MetaView {
                rating: 3,
                color_label: Some(refx_core::board::ColorLabel::Blue),
                pinned: true,
                note: "จดไว้".to_owned(),
                tags: vec!["portrait".to_owned()],
            }),
            ..ShellState::default()
        };

        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let _ = draw_in_ui(ui, &mut state, |_, _| {});
            });
            assert!(
                state.meta_request.is_none(),
                "ไม่มีใครแตะ แต่แผง Arrange กลับขอให้เขียนลง board"
            );
        }
    }

    /// ไม่ได้เลือกอะไร = ต้องไม่มีคำขอค้างจากรอบก่อน
    #[test]
    fn deselecting_clears_a_pending_meta_request() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            mode: Mode::Arrange,
            meta: None,
            meta_request: Some(MetaRequest::Rating(5)),
            meta_sealed: true,
            ..ShellState::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let _ = draw_in_ui(ui, &mut state, |_, _| {});
        });
        assert!(state.meta_request.is_none());
        assert!(!state.meta_sealed);
    }

    /// ★★ ป้ายสีที่รุ่นนี้ไม่รู้จักต้อง **ไม่ถูกวาดเป็นสีใดสีหนึ่ง**
    ///
    /// ถ้าเดาสีให้ ผู้ใช้จะคิดว่ามันเป็นสีจริงแล้วเผลอกดทับ ซึ่งทำลายค่าที่
    /// `ColorLabel::Unknown` อุตส่าห์ถือไว้เพื่อเขียนกลับ (docs/02 §2.9)
    #[test]
    fn an_unknown_colour_label_is_never_drawn_as_a_real_colour() {
        let unknown = refx_core::board::ColorLabel::from_wire(200).unwrap();
        assert!(unknown.rgb().is_none());
        // และไม่มีปุ่มไหนบน UI สร้างค่านี้ได้
        assert!(
            !refx_core::board::ColorLabel::CHOICES
                .iter()
                .any(|choice| !choice.is_known())
        );
    }

    /// ★★ ทุกวิธีเรียงที่มีในโค้ด ต้อง **เลือกได้จริงบน toolbar**
    ///
    /// วิธีเรียงที่ไม่มีปุ่มให้กด = ฟีเจอร์ที่ไม่มีอยู่จริงสำหรับผู้ใช้
    /// (รูปแบบเดียวกับ `arrange_button_labels_all_have_glyphs` ของ P2-9)
    #[test]
    fn every_sort_key_can_be_picked_from_the_toolbar() {
        for key in SortKey::ALL {
            assert!(
                SORT_CHOICES.iter().any(|(choice, _)| *choice == key),
                "{key:?} ไม่มีในรายการบน toolbar — ผู้ใช้เลือกไม่ได้"
            );
        }
        assert_eq!(SORT_CHOICES.len(), SortKey::ALL.len(), "มีตัวเลือกเกินมา");
    }

    #[test]
    fn default_state_starts_in_canvas_mode() {
        let state = ShellState::default();
        assert_eq!(state.mode, Mode::Canvas);
    }

    #[test]
    fn mode_labels_are_distinct() {
        assert_ne!(Mode::Canvas.label(), Mode::Arrange.label());
    }

    // ---------- ★ canvas ต้องโปร่ง ----------
    //
    // egui รันได้โดยไม่มี GPU (มันแค่ผลิตรูปทรงออกมา) จึงทดสอบเรื่องนี้ได้จริง
    // ไม่ใช่แค่ตรวจว่ามีโค้ด `.frame(...)` อยู่

    const SCREEN: egui::Vec2 = egui::Vec2::new(1280.0, 800.0);

    /// รัน shell แบบไม่มีหน้าต่างจริง คืน (rect ของ canvas, รูปทรงที่วาด)
    ///
    /// ต้องรันสองรอบ: egui เป็น immediate mode ที่ใช้ layout ของรอบก่อนหน้า
    /// รอบแรกจึงยังได้ขนาด panel ที่ยังไม่นิ่ง
    fn run_shell() -> (egui::Rect, Vec<egui::epaint::ClippedShape>) {
        let ctx = egui::Context::default();
        let mut state = ShellState::default();
        let mut canvas = egui::Rect::NOTHING;
        let mut shapes = Vec::new();

        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                canvas = draw_in_ui(ui, &mut state, |ui, _mode| {
                    ui.allocate_space(ui.available_size());
                });
            });
            shapes = output.shapes;
        }
        (canvas, shapes)
    }

    /// ★ บั๊กที่ทำให้ canvas ว่างเปล่ามาตั้งแต่ P0-6
    ///
    /// ภาพของผู้ใช้ถูกวาดด้วย wgpu **ใต้** egui ถ้า `CentralPanel` ทาพื้นหลังทึบ
    /// (ค่าปริยายของ egui) มันจะกลบภาพทุกใบโดยที่ไม่มี log ไหนจับได้เลย
    /// เพราะทุกขั้นตอน "สำเร็จ" หมด
    #[test]
    fn nothing_opaque_is_painted_over_the_canvas() {
        let (canvas, shapes) = run_shell();
        let center = canvas.center();

        for clipped in &shapes {
            let egui::Shape::Rect(rect) = &clipped.shape else {
                continue;
            };
            assert!(
                !(rect.rect.contains(center) && rect.fill.a() > 0),
                "มีสี่เหลี่ยมทึบ (alpha {}) ทับกลาง canvas ที่ {center:?} — \
                 ภาพของผู้ใช้จะถูกกลบทั้งหมด (ต้องใช้ Frame::NONE)",
                rect.fill.a()
            );
        }
    }

    /// rect ที่คืนออกไปต้องเป็นช่องกลางจริง ๆ ไม่ใช่ทั้งหน้าต่าง
    ///
    /// ผู้เรียกเอาไปตั้ง `set_viewport` และเป็นกรอบอ้างอิงของกล้อง
    /// ถ้าคืนขนาดหน้าต่างทั้งบาน ภาพจะเยื้องแล้วขอบไปอยู่ใต้ panel
    #[test]
    fn canvas_rect_excludes_the_side_panels() {
        let (canvas, _) = run_shell();

        assert!(
            canvas.width() > 100.0 && canvas.height() > 100.0,
            "{canvas:?}"
        );
        assert!(canvas.min.x > 0.0, "ต้องเว้นที่ให้ Library ทางซ้าย: {canvas:?}");
        assert!(
            canvas.max.x < SCREEN.x,
            "ต้องเว้นที่ให้ Inspector ทางขวา: {canvas:?}"
        );
        assert!(
            canvas.min.y > 0.0,
            "ต้องเว้นที่ให้ tabs/toolbar ด้านบน: {canvas:?}"
        );
        assert!(
            canvas.max.y < SCREEN.y,
            "ต้องเว้นที่ให้ status bar ด้านล่าง: {canvas:?}"
        );
    }
    // ---------- P3-8: สลับ mode แล้วข้อมูลต้องไม่เปลี่ยน ----------

    /// ★★★ **แถบเดียวถามได้สองเรื่อง — ประโยคต้องตรงกับเรื่องที่ถาม**
    ///
    /// "เจองานค้างจากรอบก่อน" กับ "ไฟล์นี้มีของที่ยังไม่ได้เขียนลงไป" เป็นคนละ
    /// สถานการณ์: อันแรกคืองานที่ไม่เคยมีไฟล์ อันหลังคือไฟล์ที่ผู้ใช้เพิ่งสั่งเปิด
    /// · ถามผิดประโยค ผู้ใช้จะตอบผิดปุ่ม แล้วปุ่มหนึ่งในสามลบงานของเขาถาวร
    #[test]
    fn the_recovery_bar_says_which_work_it_is_asking_about() {
        let ctx = egui::Context::default();
        let ask = |scope: RecoverScope| {
            let mut state = ShellState {
                recover_prompt: Some(RecoverView {
                    when: Some("2 h ago".to_owned()),
                    items: 7,
                    scope,
                }),
                ..ShellState::default()
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let _ = draw_in_ui(ui, &mut state, |_, _| {});
            });
            shell_text(&output)
        };

        let session = ask(RecoverScope::LastSession);
        let document = ask(RecoverScope::ThisDocument);
        let lang = Lang::default();
        assert!(
            session.contains(text::t(lang, Key::RecoverTitle)),
            "งานกำพร้าจาก session ก่อนต้องถูกถามด้วยประโยคของมันเอง"
        );
        assert!(
            document.contains(text::t(lang, Key::RecoverDocTitle)),
            "งานของเอกสารที่เปิดอยู่ถูกถามด้วยประโยคของ session ก่อน"
        );
        // ★ ทั้งสองแบบต้องมีสามทางเลือกครบเสมอ (`docs/07 §4`) — และตัวเลข
        //   "7 ชิ้น จาก 2 h ago" คือสิ่งเดียวที่ช่วยผู้ใช้จำได้ว่างานชิ้นไหน
        for (name, shown) in [("session", &session), ("document", &document)] {
            for key in [Key::RecoverRestore, Key::RecoverLater, Key::RecoverDiscard] {
                assert!(
                    shown.contains(text::t(lang, key)),
                    "{name}: ตัวเลือก {key:?} หายไปจากแถบ"
                );
            }
            assert!(shown.contains('7'), "{name}: ไม่บอกว่ามีกี่ชิ้น");
        }
    }

    /// ★ ช่องทางทั้งหมดที่ shell ใช้ "ขอให้เขียนลง `Board`" — คืน `true` ถ้ามีอันไหนดังอยู่
    ///
    /// ★★★ **destructure `ShellState` ครบทุกฟิลด์ ไม่มี `..` โดยตั้งใจ**
    ///
    /// docs/03 §4.3 บังคับว่า "สลับ mode ห้ามแก้ข้อมูล" ซึ่งเป็นข้อกำหนดหลัก
    /// ของดีไซน์สองโหมดทั้งหมด · แต่กฎแบบนั้นพังได้ทุกครั้งที่มีคนเพิ่ม
    /// **ช่องทางใหม่** ระหว่าง shell กับ `app` แล้วลืมคิดถึงการสลับโหมด
    ///
    /// การไล่ชื่อฟิลด์เอาไว้เฉย ๆ กันไม่ได้ เพราะฟิลด์ที่ 45 จะไม่มีใครมาเติม
    /// → ตรงนี้จึงบังคับตอน **คอมไพล์**: เพิ่มฟิลด์ใหม่ใน `ShellState` เมื่อไหร่
    /// ไฟล์นี้จะไม่ผ่านจนกว่าจะมีคนตัดสินว่ามันเป็น "คำขอแก้เอกสาร" หรือ
    /// "สถานะของมุมมอง" (หลักการเดียวกับ §4 ข้อ 16 และประตูของ `ItemCanvas`)
    fn asks_to_touch_the_document(state: &ShellState) -> bool {
        let ShellState {
            // ---- ช่องทางที่ทำให้ `Board` เปลี่ยนได้จริง ----
            //      (แต่ละตัวมี `App::apply_*` ของตัวเองที่ห่อเป็น `Command`)
            appearance_edit,
            appearance_sealed: _, // แค่ปิดหน้าต่าง merge ไม่ได้แก้อะไรเอง
            arrange_request,
            tool_request: _, // เปลี่ยนเครื่องมือ ไม่ใช่เอกสาร
            note_edit,
            note_sealed: _,
            arrange_apply,
            meta_request,
            meta_sealed: _,
            group_request,
            group_sealed: _,
            // ★ ปุ่มในแถบยืนยันตอนปิด (P4-2) — นำไปสู่ `mark_saved` ซึ่งแตะธง
            //   `dirty` ของ `Board` จึงนับเป็นช่องทางที่แก้เอกสารได้
            close_choice,
            // ★★ ปุ่มในแถบกู้คืน (P4-4) — "กู้คืน" **เปลี่ยน board ทั้งก้อน**
            //    ซึ่งแรงกว่าทุกช่องทางในรายการนี้ · ต้องนับเป็นการแก้เอกสารแน่นอน
            recover_choice,
            // ★★ ปุ่ม "หาไฟล์เอง" (P4-6) — นำไปสู่ `RelinkAssets` ซึ่งเปลี่ยน
            //    `ItemKind` ของ item จริง ๆ จึงเป็นช่องทางที่แก้เอกสารได้
            relink_request,
            // ★★ ปุ่มตอบ "บันทึกเป็นแบบไหน" และปุ่มสลับโหมดบนแถบสถานะ (P4-5)
            //    — ทั้งคู่นำไปสู่การ **เขียนไฟล์จริง** แล้ว `mark_saved` ตามมา
            //    (แตะธง `dirty` ของ `Board`) เหมือน `close_choice` เป๊ะ
            save_as_choice,
            storage_request,
            // ★★★ ปุ่มบนแถบแท็บ (P4-7c) — **แรงที่สุดในรายการนี้**: `Close` เอา
            //    เอกสารทั้งฉบับออกจากจอ · `New` เพิ่มเอกสารใหม่เข้ามา · ทั้งคู่
            //    เปลี่ยนว่า "เอกสารที่ผู้ใช้กำลังแก้อยู่คือใบไหน" ซึ่งเป็นสิ่งที่
            //    การสลับ *โหมด* ต้องไม่ทำเด็ดขาด (`docs/03 §4.3`)
            tab_request,

            // ---- สถานะของ *มุมมอง* — เปลี่ยนได้ตามใจ ไม่แตะเอกสาร ----
            mode: _,
            tabs: _,            // รายชื่อแท็บสำหรับแสดง — อ่านจาก `Docs` ไม่ได้เขียนกลับ
            active_tab: _,      // ดัชนีที่แสดงว่าใบไหนถูกเน้น — คำขอสลับอยู่ที่ `tab_request`
            close_scope_tab: _, // คำถามที่ค้างอยู่พูดถึงแท็บหรือทั้งหน้าต่าง
            close_prompt: _,    // บอกแค่ว่าแถบยืนยันโผล่อยู่ไหม ไม่ใช่คำขอแก้อะไร
            recover_prompt: _,  // เหมือนกัน — แค่ "มีอะไรค้างให้ถามไหม"
            save_as_prompt: _,  // แถบถามโหมดโผล่อยู่ไหม — ไม่ใช่คำขอแก้อะไร
            // ★★ กล่อง export (P5-4) — **ไม่แตะเอกสารเลยแม้แต่ทางเดียว**
            //    มันผลิต *ไฟล์ภาพใบใหม่* ไม่ได้แก้ `Board` ไม่แตะธง `dirty`
            //    และไม่ผ่าน `Command` · ★ ถ้าวันหนึ่งมีคนเพิ่ม "export แล้วจำ
            //    ค่าที่เลือกไว้ในเอกสาร" ต้องย้ายสามช่องนี้ขึ้นไปกลุ่มบน
            export_prompt: _,
            export_request: _,
            export_progress: _,
            storage: _,         // ★ ตัวบ่งชี้สภาวะของ **ไฟล์** ไม่ใช่ของเอกสารในหน่วยความจำ
            appearance: _,      // ค่าสำหรับแสดงของ inspector
            board_grayscale: _, // สวิตช์การมองเห็นทั้ง board (P2-8) ไม่ลงไฟล์
            tool: _,
            lang: _,
            status: _,
            status_warn: _,
            arrange_sort: _,       // P3-4 ตัดสินให้การเรียงอยู่ชั้น UI (§2.17)
            arrange_descending: _, // เหมือนกัน
            arrange_filter: _,     // เหมือนกัน
            arrange: _,            // ตัวนับของ virtual scrolling
            note: _,               // ค่าสำหรับแสดง
            meta: _,               // ค่าสำหรับแสดง
            group: _,              // ค่าสำหรับแสดง
            missing: _,            // ค่าสำหรับแสดง (ชื่อไฟล์ที่หาย)
            tag_input: _,          // ข้อความในช่องพิมพ์ ยังไม่ได้กด +
            picked: _,             // สีที่ picker อ่านได้
            measured: _,           // ไม้บรรทัด
            loading: _,

            // ---- ★ แผงตั้งค่า (P5-3) — **ไม่มีตัวไหนแตะเอกสารเลย** ----
            //
            //   ค่าที่ตั้งเป็นของ *โปรแกรม* ทั้งหมด (เพดานหน่วยความจำ · ธีม ·
            //   จังหวะเฟรม) ไม่มีตัวไหนลง `.refx` และไม่มีตัวไหนผ่าน `Command`
            //   · `settings_sealed` สั่งเขียน **`settings.toml`** ซึ่งเป็นไฟล์
            //   ของโปรแกรม ไม่ใช่ไฟล์งานของผู้ใช้
            //
            //   ★ สีพื้นหลัง canvas เป็นคนละเรื่องและ **จะแตะเอกสาร** เพราะมัน
            //     คือ `BoardSettings::background` — ถ้าวันหนึ่งมีคนเอามาไว้ในแผงนี้
            //     ต้องย้ายมันขึ้นไปอยู่กลุ่มบนพร้อมกับ `Command` ของตัวเอง
            settings_open: _,
            settings: _,
            settings_edit: _,
            settings_sealed: _,
            settings_notes: _,
            settings_notes_dismissed: _,
            // ★ คีย์ลัด: **อ่านอย่างเดียว** ในก้อน b · ไม่มีทางกลับไปแตะเอกสาร
            //   วันที่แก้คีย์ในแอปได้ ให้ทบทวนอีกครั้ง — การเปลี่ยนคีย์เองก็ยัง
            //   ไม่แตะ `Board` แต่คำขอที่มันผลิตจะแตะ ต้องแยกให้ชัดตอนนั้น
            keymap: _,

            // ---- ตัวเลขที่โชว์บน status bar ----
            atlas_uploads: _,
            item_count: _,
            zoom: _,
            frames_drawn: _,
            quiet_redraws: _, // ตัวจับ I-1 — เป็นตัวเลขที่รายงาน ไม่ใช่คำขอแก้อะไร
            ram_used: _,
            ram_limit: _,
            vram_used: _,
            vram_limit: _,
            cache_thumbs: _,
            cache_bytes: _,
            decode_queued: _,
            decode_cancelled: _,
            draw_calls: _,
            working_used: _,
            working_limit: _,
            working_evicted: _,
        } = state;

        appearance_edit.is_some()
            || arrange_request.is_some()
            || note_edit.is_some()
            || meta_request.is_some()
            || group_request.is_some()
            || close_choice.is_some()
            || recover_choice.is_some()
            || save_as_choice.is_some()
            || storage_request.is_some()
            || *arrange_apply
            || *relink_request
            || tab_request.is_some()
    }

    /// ★★★ **ตัวบ่งชี้ "ยังไม่บันทึก" ต้องอยู่ตราบเท่าที่สภาวะยังอยู่** (docs/03 §1)
    ///
    /// นี่คือความต่างทั้งหมดระหว่าง *ข้อความชั่วคราว* กับ *ตัวบ่งชี้ถาวร* —
    /// ข้อความ "กู้คืนแล้ว — กด Ctrl+S เพื่อเก็บไว้" ของรุ่นก่อนถูกรายงาน
    /// ความคืบหน้าการโหลดภาพเขียนทับใน **~3 มิลลิวินาที** ผู้ใช้ที่เพิ่งได้งานคืน
    /// จึงไม่มีทางรู้ว่างานนั้นยังไม่ถูกบันทึก แล้วปิดโปรแกรมทิ้งได้อีกรอบ
    ///
    /// ★ เทสต์จึงวาดหลายเฟรม **แล้วเขียนทับ `status` ระหว่างทาง** เลียนแบบสิ่งที่
    /// ของจริงทำ — ตัวบ่งชี้ต้องรอด (`docs/08 §3.9` ข้อ 8: สัญญาที่ถูกเฉพาะตอน
    /// เรียกถูกจังหวะคือกับดัก · ที่นี่คือ "ถูกเฉพาะเฟรมแรก")
    #[test]
    fn the_unsaved_marker_survives_every_status_message_that_lands_on_top() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            // ★ ตัวบ่งชี้ย้ายมาอยู่บน **แท็บของมันเอง** ตั้งแต่ P4-7c —
            //   แหล่งความจริงคือ `Doc::board.is_dirty()` ของแต่ละใบ
            tabs: vec![TabView {
                title: None,
                unsaved: true,
            }],
            ..ShellState::default()
        };

        for frame in 0..30 {
            // ของจริงเขียน status ทับตลอดเวลา: โหลดเสร็จ · ก๊อปสี · สลับโหมด
            state.status = format!("เหตุการณ์ที่ {frame}");
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let _ = draw_in_ui(ui, &mut state, |_, _| {});
            });
            let shown = shell_text(&output);
            assert!(
                shown.contains(UNSAVED_MARK),
                "เฟรมที่ {frame}: ตัวบ่งชี้ 'ยังไม่บันทึก' หายไปจากจอ"
            );
        }

        // ★ negative control — บันทึกแล้วจุดต้องหายไป ไม่ใช่ค้างอยู่ตลอดกาล
        //   (ตัวบ่งชี้ที่ไม่เคยดับก็ไร้ความหมายเท่ากับตัวที่ไม่เคยติด)
        state.tabs = vec![TabView {
            title: Some("moodboard.refx".to_owned()),
            unsaved: false,
        }];
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            let _ = draw_in_ui(ui, &mut state, |_, _| {});
        });
        let shown = shell_text(&output);
        assert!(!shown.contains(UNSAVED_MARK), "บันทึกแล้วแต่เครื่องหมายยังอยู่");
        assert!(
            shown.contains("moodboard.refx"),
            "บันทึกแล้วต้องเห็นชื่อไฟล์ที่กำลังแก้อยู่ — ไม่งั้นผู้ใช้ไม่รู้ว่าแก้ไฟล์ไหน"
        );
    }

    /// ★★★ **ทุกแท็บโชว์สภาวะของตัวเอง ไม่ใช่ของใบที่ผู้ใช้กำลังดู** (P4-7c)
    ///
    /// ผู้ใช้ที่เห็นดาวแค่ใบเดียวจะปิดโปรแกรมโดยเชื่อว่าอีกสามใบสะอาด ซึ่งเป็น
    /// ลูปเดิมที่ตัวบ่งชี้ถาวรมีไว้แก้พอดี (`docs/03 §1`) — แค่ย้ายมาโผล่ตอนที่
    /// มีหลายเอกสารพร้อมกัน
    #[test]
    fn every_tab_shows_its_own_unsaved_state() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            tabs: vec![
                TabView {
                    title: Some("clean.refx".to_owned()),
                    unsaved: false,
                },
                TabView {
                    // ★ ใบที่ค้างอยู่คือใบที่ **ไม่ได้** ถูกเลือก
                    title: Some("dirty.refx".to_owned()),
                    unsaved: true,
                },
            ],
            active_tab: 0,
            ..ShellState::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input.clone(), |ui| {
            let _ = draw_in_ui(ui, &mut state, |_, _| {});
        });
        let shown = shell_text(&output);
        assert!(shown.contains("clean.refx"), "แท็บที่บันทึกแล้วไม่ขึ้นบนจอ");
        assert!(shown.contains("dirty.refx"), "แท็บที่ค้างอยู่ไม่ขึ้นบนจอ");
        assert!(
            shown.contains(&format!("{UNSAVED_MARK} dirty.refx")),
            "แท็บที่ไม่ได้ถูกเลือกไม่มีตัวบ่งชี้ 'ยังไม่บันทึก' ของตัวเอง"
        );
        assert!(
            !shown.contains(&format!("{UNSAVED_MARK} clean.refx")),
            "แท็บที่บันทึกแล้วกลับมีดาว — ตัวบ่งชี้ที่ไม่เคยดับก็ไร้ความหมาย"
        );

        // ★ negative control ของ *ที่มา*: ธงอยู่ที่แท็บ ไม่ใช่ที่ตัวแปรรวม
        state.tabs[1].unsaved = false;
        let output = ctx.run_ui(input, |ui| {
            let _ = draw_in_ui(ui, &mut state, |_, _| {});
        });
        assert!(
            !shell_text(&output).contains(UNSAVED_MARK),
            "ทุกใบสะอาดแล้วยังมีดาวค้าง — ประตูนี้ไม่ล้มเป็น"
        );
    }

    /// ★★ ปุ่มบนแถบแท็บเป็น **คำขอ** ไม่ใช่การลงมือ (docs/08 §4 ข้อ 10)
    ///
    /// widget ปิดเอกสารเองไม่ได้ — การปิดแท็บที่ยังไม่บันทึกต้องผ่านคำถามของ
    /// ชั้น `app` ก่อนเสมอ · ที่นี่พิสูจน์ว่ามันรายงาน **ดัชนีที่กดจริง**
    #[test]
    fn the_tab_strip_reports_which_tab_was_touched() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            tabs: vec![
                TabView {
                    title: Some("a.refx".to_owned()),
                    unsaved: false,
                },
                TabView {
                    title: Some("b.refx".to_owned()),
                    unsaved: false,
                },
            ],
            active_tab: 0,
            ..ShellState::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let _ = draw_in_ui(ui, &mut state, |_, _| {});
        });
        assert_eq!(
            state.tab_request, None,
            "แค่วาดเฉย ๆ แล้วมีคำขอโผล่มาเอง — แท็บจะสลับ/ปิดตัวเองทุกเฟรม"
        );
        // ★ และมันถูกนับเป็นช่องทางที่แตะเอกสารได้ (ปิดแท็บ = เอาเอกสารออกจากจอ)
        state.tab_request = Some(TabRequest::Close(1));
        assert!(
            asks_to_touch_the_document(&state),
            "ปุ่มปิดแท็บไม่ถูกนับเป็นช่องทางที่แตะเอกสาร"
        );
    }

    /// ★★★ **เครื่องหมายบนแท็บต้องมี glyph จริงในฟอนต์ที่เราฝังไว้**
    ///
    /// รุ่นแรกใช้ `●` (U+25CF) แล้ว **ออกมาเป็นสี่เหลี่ยม tofu บนจอจริง** —
    /// กล่องเปล่าข้างชื่อไฟล์อ่านได้อย่างเดียวว่า "โปรแกรมพัง" ซึ่งแย่กว่า
    /// ไม่มีตัวบ่งชี้เลย
    ///
    /// ★★ เทสต์ที่ดูข้อความ (`galley.text()`) จับข้อนี้ **ไม่ได้โดยธรรมชาติ**
    /// เพราะ galley เก็บอักขระต้นฉบับไว้เสมอไม่ว่าจะวาดเป็น glyph หรือ tofu
    /// — ตอนนั้นเทสต์เขียวและภาพหน้าจอเป็นสิ่งเดียวที่เห็น (`docs/08 §3.9` ข้อ 5)
    ///
    /// ★ ประตูนี้ถาม `Fonts::has_glyph` ตรง ๆ จึงเป็นสิ่งที่**เทสต์ทำแทนตาได้**
    /// สำหรับข้อนี้โดยเฉพาะ · negative control: เปลี่ยนกลับเป็น `●` แล้วมันแดง
    ///
    /// ★★★ **ขยายให้ครอบทุกสัญลักษณ์ ไม่ใช่แค่ตัวเดียว (P5-3)** — ประตูรุ่นแรก
    /// ถามเฉพาะ [`UNSAVED_MARK`] แล้ว P5-3 ก็เผลอใส่ `●` กลับเข้ามาเป็นจุดเตือน
    /// ข้างปุ่มตั้งค่า **ทั้งที่บทเรียนอยู่ในไฟล์เดียวกัน** และเห็นเป็น tofu บน
    /// ภาพหน้าจอจริงอีกครั้ง · ประตูที่คุมของชิ้นเดียวไม่ได้คุมรูปแบบของบั๊ก
    #[test]
    fn every_symbol_on_screen_has_a_real_glyph() {
        let ctx = egui::Context::default();
        // ★★ ฟอนต์ชุดที่แอปใช้จริง — เดิมเทสต์นี้ถามฟอนต์ของ egui เอง
        //    ข้อสรุปบังเอิญตรงกัน แต่มันตอบคนละคำถามกับที่ชื่อมันอ้าง
        crate::fonts::install(&ctx);
        // ต้องวาดหนึ่งเฟรมก่อน ฟอนต์ถึงถูกโหลดจริง
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let mut state = ShellState::default();
        let _ = ctx.run_ui(input, |ui| {
            let _ = draw_in_ui(ui, &mut state, |_, _| {});
        });

        let font = egui::FontId::proportional(14.0);
        for mark in [UNSAVED_MARK, SETTINGS_MARK] {
            assert!(
                ctx.fonts_mut(|fonts| fonts.has_glyph(&font, mark)),
                "เครื่องหมาย {mark:?} ไม่มี glyph ในฟอนต์ที่ฝังไว้ — \
                 ผู้ใช้จะเห็นสี่เหลี่ยม tofu ซึ่งอ่านว่า 'โปรแกรมพัง'"
            );
        }
        // ★ และ `●` ที่เคยใช้ **ไม่มีจริง** — พิสูจน์ว่าประตูนี้แยกสองกรณีออกจากกัน
        //   ไม่ใช่ประตูที่ตอบ true กับทุกอักขระ (ประตูที่ผ่านทุกอย่าง = ไม่มีประตู)
        assert!(
            !ctx.fonts_mut(|fonts| fonts.has_glyph(&font, '\u{25CF}')),
            "ถ้า ● มี glyph แล้ว ประตูนี้ก็ไม่ได้พิสูจน์อะไร — ทบทวนว่ายังจำเป็นไหม"
        );
    }

    /// วาด shell หนึ่งเฟรมแล้วคืนข้อความที่ขึ้นจอ
    fn draw_once(state: &mut ShellState, ctx: &egui::Context) -> String {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            let _ = draw_in_ui(ui, state, |_, _| {});
        });
        shell_text(&output)
    }

    /// ★★★ ค่าที่ตั้งผิดต้อง **ขึ้นจอพร้อมเหตุผล** ไม่ใช่แค่ไม่ทำให้โปรแกรมล้ม (P5-3)
    ///
    /// `refx_io::settings` พิสูจน์แล้วว่าไฟล์ที่พังยังให้ค่าที่ใช้ได้ — แต่ครึ่งนั้น
    /// เขียวได้แม้ไม่มีใครบอกผู้ใช้เลยสักคำ · ครึ่งที่เหลืออยู่ตรงนี้: ตัวเลขที่เขา
    /// เขียน ตัวเลขที่ได้จริง และ **เหตุผล** ต้องอ่านได้จากบนจอ
    #[test]
    fn a_setting_that_was_adjusted_says_so_on_screen_with_its_numbers() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            settings_open: true,
            settings_notes: vec![
                refx_io::settings::Note::MaxPixelsCappedByRam {
                    asked: 268_435_456,
                    used: 134_217_728,
                    ram_gb: 8,
                },
                refx_io::settings::Note::UnknownKey {
                    name: "ram_limit_md".to_owned(),
                },
            ],
            ..ShellState::default()
        };

        let shown = draw_once(&mut state, &ctx);
        for needle in ["268435456", "134217728", "8", "ram_limit_md"] {
            assert!(
                shown.contains(needle),
                "แผงไม่ได้บอก {needle:?} ให้ผู้ใช้เห็น:\n{shown}"
            );
        }
        assert!(
            shown.contains(text::t(Lang::En, Key::SettingsProblemsTitle)),
            "ไม่มีหัวข้อบอกว่าไฟล์ไม่ได้ถูกใช้ตามที่เขียน"
        );
    }

    /// ★★ แผงตั้งค่าต้อง **ไม่ขออะไร** ถ้าผู้ใช้ไม่ได้แตะมัน
    ///
    /// เหตุผลเดียวกับ inspector และช่องโน้ต — ถ้าค่าที่แสดงถูกอ่านกลับไปเขียนทุกเฟรม
    /// มันจะสั่งเขียน `settings.toml` ทุกเฟรมที่แผงเปิดอยู่ ซึ่งคือการเขียนดิสก์
    /// ตลอดเวลาโดยไม่มีใครสั่ง (`docs/08 §3.9` ข้อ 8.1)
    #[test]
    fn the_settings_panel_asks_for_nothing_when_nobody_touches_it() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            settings_open: true,
            settings: SettingsView {
                ram_limit_mb: 512,
                vram_limit_mb: Some(1024),
                max_pixels: 100_000_000,
                theme: refx_io::settings::Theme::Light,
                present: refx_io::settings::Present::Uncapped,
                max_pixels_ceiling: 268_435_456,
                needs_restart: true,
            },
            ..ShellState::default()
        };

        for _ in 0..3 {
            let _ = draw_once(&mut state, &ctx);
            assert!(state.settings_edit.is_none(), "ไม่มีใครแตะ แต่แผงกลับขอเปลี่ยนค่า");
            assert!(!state.settings_sealed, "ไม่มีใครแตะ แต่แผงกลับสั่งเขียนไฟล์");
        }
    }

    /// ★★★ `keymap.toml` ที่ใช้ไม่ได้ ต้องบอก **แถวที่ผิด** บนจอ (P5-3b ก้อน b)
    ///
    /// ชั้น `keymap` พิสูจน์แล้วว่ามันตรวจเจอ — แต่ครึ่งนั้นเขียวได้แม้ไม่มีใคร
    /// บอกผู้ใช้เลยสักคำ · ครึ่งที่เหลืออยู่ตรงนี้: เลขแถวทั้งสอง ปุ่มทั้งสอง
    /// และคำว่า "กลับไปใช้ค่าปริยาย" ต้องอ่านได้จากบนจอ
    #[test]
    fn a_keymap_file_that_conflicts_with_itself_says_which_rows_on_screen() {
        let ctx = egui::Context::default();
        let problem = crate::keymap::Problem::Conflict {
            row: 7,
            keys: "ctrl+z".to_owned(),
            other_row: 3,
            other_keys: "CTRL+Z".to_owned(),
        };
        let mut state = ShellState {
            settings_open: true,
            keymap: KeymapView {
                rows: vec![("ctrl+s".to_owned(), "save")],
                from_file: false,
                problem: Some(text::keymap_problem(Lang::En, &problem)),
            },
            ..ShellState::default()
        };

        let shown = draw_once(&mut state, &ctx);
        for needle in ["7", "3", "ctrl+z", "CTRL+Z"] {
            assert!(
                shown.contains(needle),
                "แผงไม่ได้บอก {needle:?} ให้ผู้ใช้เห็น:\n{shown}"
            );
        }
        assert!(
            shown.contains(text::t(Lang::En, Key::KeymapFellBackToDefaults)),
            "ไม่มีข้อความบอกว่ากลับไปใช้ค่าปริยายแล้ว"
        );
        // ★ และต้องบอกว่าตารางที่ใช้อยู่มาจากไหน — ไฟล์แทนที่ทั้งชุด คนที่เขียน
        //   ไปสามบรรทัดต้องเห็นทันทีว่าทำไมเหลือสามปุ่ม
        assert!(shown.contains(text::t(Lang::En, Key::SettingsKeymapBuiltin)));
    }

    /// ★★ ชื่อ action ที่แผงแสดง ต้องเป็นชื่อที่ **พิมพ์ลงไฟล์ได้จริง**
    ///
    /// แผงนี้มีไว้ตอบคำถาม *"ต้องพิมพ์อะไรลงใน keymap.toml"* · ถ้ามันแสดงชื่อ
    /// ที่ตัวอ่านไฟล์ไม่รู้จัก มันจะพาผู้ใช้ไปเขียนไฟล์ที่ถูกปฏิเสธทั้งชุด
    #[test]
    fn the_names_the_panel_shows_are_the_names_the_file_accepts() {
        let ctx = egui::Context::default();
        let rows: Vec<(String, &'static str)> = crate::keymap::builtin()
            .bindings()
            .iter()
            .filter_map(|b| Some((b.chord.display()?, b.action.name())))
            .collect();
        assert!(!rows.is_empty());

        let mut state = ShellState {
            settings_open: true,
            keymap: KeymapView {
                rows: rows.clone(),
                from_file: true,
                problem: None,
            },
            ..ShellState::default()
        };
        let shown = draw_once(&mut state, &ctx);

        // ---- 1. ทุกแถวต้องเขียนกลับลงไฟล์ได้ — ตรวจที่ **ข้อมูล** ไม่ใช่ที่จอ ----
        //
        // ★★ แยกจากการตรวจบนจอโดยตั้งใจ: รายการอยู่ใน `ScrollArea` ซึ่งวาดเฉพาะ
        //    แถวที่อยู่ในกรอบ · ถ้าเอาไปผูกกับ `shown` แถวท้าย ๆ จะ "ไม่ผ่าน"
        //    เพราะมันแค่ยังไม่ถูกเลื่อนมาให้เห็น ซึ่งเป็นคนละเรื่องกับความถูกต้อง
        for (chord, action) in &rows {
            assert!(
                crate::keymap::Action::from_name(action).is_some(),
                "{action:?} แสดงบนแผงแต่ตัวอ่านไฟล์ไม่รู้จัก"
            );
            assert!(
                crate::keymap::Chord::parse(chord).is_some(),
                "{chord:?} แสดงบนแผงแต่เขียนกลับลงไฟล์ไม่ได้"
            );
        }

        // ---- 2. บนจอ: แถวแรก ๆ ต้องเห็นจริง และต้องบอกว่าตารางมาจากไหน ----
        let (chord, action) = &rows[0];
        assert!(shown.contains(chord.as_str()), "{chord:?} ไม่ได้ขึ้นจอ");
        assert!(shown.contains(*action), "{action:?} ไม่ได้ขึ้นจอ");
        assert!(shown.contains(text::t(Lang::En, Key::SettingsKeymapFromFile)));
    }

    /// ★ ค่าที่เปลี่ยนแล้วยังไม่มีผลต้องบอก — ไม่ใช่ปล่อยให้ผู้ใช้เดาว่าปุ่มเสีย
    #[test]
    fn a_value_that_needs_a_restart_says_so() {
        let ctx = egui::Context::default();
        let notice = text::t(Lang::En, Key::SettingsNeedsRestart);
        let shown = |needs_restart: bool| {
            let mut state = ShellState {
                settings_open: true,
                settings: SettingsView {
                    needs_restart,
                    ..SettingsView::default()
                },
                ..ShellState::default()
            };
            draw_once(&mut state, &ctx).contains(notice)
        };

        assert!(shown(true), "เปลี่ยนค่าแล้วไม่มีอะไรบอกว่าต้องเปิดใหม่");
        // ★ negative control: ข้อความที่ขึ้นตลอดเวลาไม่ได้บอกอะไรใครเลย
        assert!(!shown(false), "ข้อความ 'ต้องเปิดใหม่' ขึ้นทั้งที่ยังไม่มีอะไรรออยู่");
    }

    /// ★★★ **ทุกอักขระที่ shell วาด ต้องมี glyph จริง** — ประตูที่ครอบตัวอักษรสด
    ///
    /// ## ทำไมประตูเดิมยังไม่พอ
    ///
    /// `every_symbol_on_screen_has_a_real_glyph` ถาม `has_glyph` ให้ **ค่าคงที่**
    /// ที่เราจดชื่อไว้ (`UNSAVED_MARK` · `SETTINGS_MARK`) · แต่บั๊กครั้งที่สาม
    /// (P5-3b) มาในรูป **`ui.monospace("→")` ที่เขียนสดในโค้ด** — ไม่มีค่าคงที่
    /// ให้จด ประตูจึงมองไม่เห็น และมันโผล่เป็นสี่เหลี่ยม tofu บนภาพหน้าจอจริง
    /// เป็นครั้งที่สามของบั๊กเดิมกันในไฟล์เดียวกัน
    ///
    /// ## วิธีที่ครอบได้จริง
    ///
    /// วาด shell ด้วยสถานะที่ **มีของให้วาดครบ** แล้วเดินทุกอักขระที่ galley
    /// ผลิตออกมา · ★ `galley.text()` เก็บอักขระต้นฉบับไว้เสมอไม่ว่าจะวาดเป็น
    /// glyph จริงหรือ tofu — เทสต์ที่ดูแต่ข้อความจึงจับไม่ได้ ต้องถาม
    /// `Fonts::has_glyph` ทีละตัว
    ///
    /// ★★ ข้อความในเทสต์นี้เป็นของเราทั้งหมด (ไม่มี input ของผู้ใช้จริง)
    /// อักขระที่หลุดออกมาจึงเป็นความผิดของโค้ดเสมอ ไม่ใช่ของข้อมูล
    #[test]
    fn every_character_the_shell_draws_has_a_glyph() {
        let ctx = egui::Context::default();
        // ★★★ **ต้องเป็นฟอนต์ชุดที่แอปใช้จริง** — `Context::default()` มีฟอนต์ของ
        //     egui เอง ซึ่งไม่ใช่สิ่งที่ผู้ใช้เห็น · ประตูที่ตรวจฟอนต์คนละชุดกับ
        //     production คือประตูที่ตอบคนละคำถาม (`docs/08 §3.9` ข้อ 1b)
        crate::fonts::install(&ctx);
        let mut state = ShellState {
            lang: Lang::Th,
            settings_open: true,
            keymap: KeymapView {
                rows: crate::keymap::builtin()
                    .bindings()
                    .iter()
                    .filter_map(|b| Some((b.chord.display()?, b.action.name())))
                    .collect(),
                from_file: true,
                problem: Some(text::keymap_problem(
                    Lang::Th,
                    &crate::keymap::Problem::UnknownAction {
                        row: 2,
                        given: "undoo".to_owned(),
                    },
                )),
            },
            settings_notes: vec![refx_io::settings::Note::MaxPixelsCappedByRam {
                asked: 268_435_456,
                used: 134_217_728,
                ram_gb: 8,
            }],
            // ★ ตัวจับ I-1 โผล่เฉพาะตอนผิดปกติ — ต้องบังคับให้มันถูกวาดที่นี่
            //   ไม่งั้นข้อความของมันจะไม่เคยผ่านประตู tofu เลยจนถึงวันที่อาการเกิด
            quiet_redraws: Some((82, "EguiRepaint")),
            // ★★★ กล่องส่งออก (P5-4) — **บังคับให้ทุกสถานะที่นาน ๆ โผล่ ถูกวาด**
            //     (`docs/03 §0`) · คำเตือน `Missing` · ปุ่ม "ทับไฟล์เดิม" ·
            //     ข้อความ error — สามอย่างนี้ผู้ใช้เห็นเฉพาะวันที่มีอะไรผิด
            //     ซึ่งเป็นวันที่แย่ที่สุดที่จะเจอสี่เหลี่ยม tofu
            export_prompt: Some(ExportView {
                target: Some("moodboard.png".to_owned()),
                overwrite: true,
                missing: 3,
                // ★ เพดานที่ต่ำกว่าเพดานของทางเขียน — บังคับให้บรรทัดบอกเหตุผล
                //   ถูกวาด · board ปกติจะไม่ติดขีด แล้วประตูจะไม่เคยเห็นข้อความนี้
                max_side: 2760,
                long_side: 2048,
                problem: Some(text::t(Lang::Th, Key::ExportBadTarget).to_owned()),
                ..ExportView::default()
            }),
            export_progress: Some(ExportProgress {
                name: "moodboard.png".to_owned(),
                done: 7,
                total: 32,
                cancelling: false,
            }),
            status_warn: true,
            ..ShellState::default()
        };

        // ★★ ข้อความสถานะที่ **ไม่มีทางโผล่พร้อมกัน** ต้องถูกวาดคนละรอบ — ไม่งั้น
        //    ประตูจะตรวจแค่ตัวที่บังเอิญค้างอยู่ตอนเขียนเทสต์ · หกคีย์ของก้อน c
        //    เพิ่มสามข้อความใหม่เข้ามาที่นี่ ทั้งอังกฤษและไทย
        let statuses: Vec<String> = [
            Key::SettingsProblemsStatus,
            Key::SelectionCleared,
            Key::NothingToFit,
            Key::ZoomIsCanvasOnly,
        ]
        .into_iter()
        .flat_map(|key| {
            [Lang::En, Lang::Th]
                .into_iter()
                .map(move |lang| text::t(lang, key).to_owned())
        })
        .chain([
            text::fill(Lang::En, Template::ZoomSet, &[("percent", "250")]),
            text::fill(Lang::Th, Template::ZoomSet, &[("percent", "250")]),
            text::fill(Lang::En, Template::SelectedAll, &[("n", "42")]),
            text::fill(Lang::Th, Template::SelectedAll, &[("n", "42")]),
            text::fill(Lang::En, Template::SwitchedMode, &[("mode", "Arrange")]),
            text::fill(Lang::Th, Template::SwitchedMode, &[("mode", "Arrange")]),
        ])
        .collect();

        // สองเฟรม: egui ใช้ layout ของรอบก่อน รอบแรกขนาด panel ยังไม่นิ่ง
        //
        // ★★★ **วาดทั้งสองภาษา** — รุ่นก่อนตรึง `lang` ไว้ที่ไทยแล้ววนแต่ข้อความ
        //     สถานะ · ผลคือ **ข้อความบนแผงทุกตัวถูกตรวจเฉพาะภาษาไทย** ส่วน
        //     ภาษาอังกฤษไม่เคยผ่านประตูนี้เลย (เจอตอนเพิ่มบรรทัดเพดานของ P5-4a)
        let mut drawn: Vec<(egui::FontId, String)> = Vec::new();
        for lang in [Lang::En, Lang::Th] {
            state.lang = lang;
            // ★ `problem` เก็บข้อความที่ **แปลแล้ว** — ต้องเปลี่ยนตามภาษาที่กำลังวาด
            //   ไม่งั้นครึ่งหนึ่งของภาษาไม่เคยผ่านประตู
            if let Some(view) = state.export_prompt.as_mut() {
                view.problem = Some(text::t(lang, Key::ExportBadTarget).to_owned());
            }
            for status in &statuses {
                state.status = status.clone();
                let mut last = Vec::new();
                for _ in 0..2 {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        ..Default::default()
                    };
                    let output = ctx.run_ui(input, |ui| {
                        let _ = draw_in_ui(ui, &mut state, |_, _| {});
                    });
                    last = drawn_runs(&output);
                }
                assert!(
                    last.iter().any(|(_, text)| text.contains(status.as_str())),
                    "ข้อความ {status:?} ไม่ได้ถูกวาดจริง — ประตูจะเขียวโดยไม่ได้ตรวจมัน                  (docs/08 §3.9 ข้อ 1b)"
                );
                drawn.extend(last);
            }
        }
        assert!(!drawn.is_empty(), "ไม่มีอะไรถูกวาดเลย — ประตูนี้ไม่ได้ตรวจอะไร");
        assert!(
            drawn.iter().any(|(_, text)| text.contains("EguiRepaint")),
            "ช่องของตัวจับ I-1 ไม่ได้ถูกวาด — ข้อความของมันจะไม่เคยผ่านประตูนี้"
        );
        // ★★ ยืนยันว่าสามสถานะของกล่องส่งออกถูกวาดจริง ไม่ใช่แค่ตั้งค่าไว้
        //    (`docs/08 §3.9` ข้อ 1b — input ต้องไปถึงกิ่งที่กำลังตรวจ)
        for lang in [Lang::En, Lang::Th] {
            for expected in [
                text::t(lang, Key::ExportOverwrite),
                text::t(lang, Key::ExportMissingWarning),
                text::t(lang, Key::ExportBadTarget),
                text::t(lang, Key::ExportCancel),
            ] {
                assert!(
                    drawn.iter().any(|(_, text)| text.contains(expected)),
                    "{expected:?} ไม่ได้ถูกวาด — ประตูจะเขียวโดยไม่ได้ตรวจมัน"
                );
            }
        }
        // ★★ บรรทัดบอกเหตุผลของเพดานขึ้นเฉพาะตอน board ติดขีด — ต้องบังคับให้วาด
        //    ไม่งั้นข้อความนี้จะไม่เคยผ่านประตู tofu เลย (`docs/03 §0`)
        for lang in [Lang::En, Lang::Th] {
            let ceiling = text::fill(lang, Template::ExportCeiling, &[("n", "2760")]);
            assert!(
                drawn.iter().any(|(_, text)| text.contains(&ceiling)),
                "{ceiling:?} ไม่ได้ถูกวาด"
            );
        }

        // ★★★ ถามด้วย **ฟอนต์ที่ข้อความท่อนนั้นถูกจัดหน้าด้วยจริง ๆ**
        //
        //     รุ่นแรกของประตูนี้ถามแบบ `proportional || monospace` แล้ว **ปล่อย
        //     `→` ผ่าน** เพราะฟอนต์ proportional มีมัน ส่วนของจริงถูกวาดด้วย
        //     `ui.monospace()` ซึ่งไม่มี — ประตูตอบคนละคำถามกับที่มันอ้าง
        //     พิสูจน์ด้วย NC: เอา `→` กลับเข้าไปแล้วประตูรุ่นแรก **ยังเขียว**
        //     (`docs/08 §3.9` ข้อ 1b)
        let mut missing: Vec<(String, char)> = Vec::new();
        for (font, text) in &drawn {
            for ch in text.chars() {
                // ช่องว่าง/ขึ้นบรรทัดไม่ใช่ glyph ที่ต้องมี และไม่มีทางเป็น tofu
                if ch.is_whitespace() || ch.is_control() {
                    continue;
                }
                if !ctx.fonts_mut(|fonts| fonts.has_glyph(font, ch)) {
                    missing.push((format!("{:?}", font.family), ch));
                }
            }
        }
        missing.sort_unstable();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "อักขระที่ไม่มี glyph ในฟอนต์ที่ใช้วาดมันจริง ๆ \
             (จะเห็นเป็นสี่เหลี่ยม tofu): {missing:?}"
        );
    }

    /// ข้อความที่ถูกวาด **พร้อมฟอนต์ที่ใช้จัดหน้าท่อนนั้น**
    ///
    /// `LayoutJob` แบ่งข้อความเป็น section ที่มี `font_id` ของตัวเอง — ตัวเดียว
    /// ที่บอกได้ว่าอักขระตัวนี้ถูกวาดด้วยฟอนต์ตระกูลไหน
    fn drawn_runs(output: &egui::FullOutput) -> Vec<(egui::FontId, String)> {
        let mut runs = Vec::new();
        for shape in &output.shapes {
            collect_runs(&shape.shape, &mut runs);
        }
        runs
    }

    fn collect_runs(shape: &egui::Shape, out: &mut Vec<(egui::FontId, String)>) {
        match shape {
            egui::Shape::Text(text) => {
                let job = &text.galley.job;
                for section in &job.sections {
                    if let Some(part) = job.text.get(section.byte_range.clone()) {
                        out.push((section.format.font_id.clone(), part.to_owned()));
                    }
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_runs(shape, out);
                }
            }
            _ => {}
        }
    }

    /// ข้อความทั้งหมดที่ egui วาดออกมาในเฟรมนั้น
    fn shell_text(output: &egui::FullOutput) -> String {
        let mut found = String::new();
        for shape in &output.shapes {
            collect_text(&shape.shape, &mut found);
        }
        found
    }

    fn collect_text(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                out.push_str(text.galley.text());
                out.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, out);
                }
            }
            _ => {}
        }
    }

    /// ★★★ **สลับ mode 100 ครั้งแล้วต้องไม่มีคำขอแก้เอกสารสักครั้งเดียว** (P3-8)
    ///
    /// docs/03 §4.3: "การกด `Canvas ⇄ Arrange` เป็นการเปลี่ยน view ล้วน ๆ
    /// ไม่สร้าง Command ไม่ทำให้ `dirty = true`" — ข้อมูลจะเปลี่ยนก็ต่อเมื่อผู้ใช้
    /// กดปุ่มสะพาน (`Send to Canvas` / `Sort by canvas order`) อย่างจงใจเท่านั้น
    ///
    /// ★★ **นี่คือด่านที่ `Board` เปลี่ยนได้เท่านั้น** — `ArrangeView::plan` และ
    /// `query::select` รับ `&Board` คอมไพเลอร์จึงกันการเขียนให้แล้ว ทางเดียว
    /// ที่เหลือคือ shell เขียน "คำขอ" ค้างไว้แล้ว `app` ห่อเป็น `Command`
    /// ในเฟรมถัดไป · ถ้าไม่มีคำขอ ก็ไม่มี `Command` ก็ไม่มีอะไรเปลี่ยน
    ///
    /// ★ วาดด้วย **สถานะที่มีของให้แตะครบ** (เลือกอยู่ · มีกลุ่ม · มีโน้ต)
    /// ไม่ใช่ `default()` เปล่า ๆ — แผงที่ไม่มีอะไรให้วาดพิสูจน์อะไรไม่ได้เลย
    #[test]
    fn switching_mode_a_hundred_times_never_asks_to_change_the_document() {
        use refx_core::arena::ArenaKey as _;
        let ctx = egui::Context::default();
        let mut state = ShellState {
            mode: Mode::Canvas,
            meta: Some(MetaView {
                rating: 3,
                color_label: Some(refx_core::board::ColorLabel::Blue),
                pinned: true,
                note: "จดไว้".to_owned(),
                tags: vec!["portrait".to_owned()],
            }),
            note: Some("โน้ตบน canvas".to_owned()),
            group: Some(GroupView::One {
                id: refx_core::arena::GroupId::from_parts(0, 0),
                name: "Group 1".to_owned(),
                collapsed: true,
                members: 3,
            }),
            ..ShellState::default()
        };

        for round in 0..100 {
            state.mode = if round % 2 == 0 {
                Mode::Arrange
            } else {
                Mode::Canvas
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let _ = draw_in_ui(ui, &mut state, |_, _| {});
            });
            assert!(
                !asks_to_touch_the_document(&state),
                "รอบที่ {round} ({:?}) แผงขอให้เขียนลง board ทั้งที่ผู้ใช้แค่สลับโหมด",
                state.mode
            );
        }

        // ★ และสิ่งที่แผงแสดงต้องยังอยู่ครบ — "ไม่ขอแก้" ต้องไม่ได้มาจากการที่
        //   แผงเงียบไปเพราะมันลืมของที่เลือกไว้
        assert!(state.meta.is_some(), "ของที่เลือกหายไประหว่างสลับโหมด");
        assert!(state.group.is_some(), "กลุ่มหายไประหว่างสลับโหมด");
    }
}
