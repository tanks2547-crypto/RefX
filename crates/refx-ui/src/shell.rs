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

/// สีของข้อความที่ผู้ใช้ต้องสังเกตเห็น — ตัวเดียวกับ RAM/VRAM ตอนใกล้เต็ม
const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(230, 160, 60);

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

/// งานค้างที่เจอตอนเปิดโปรแกรม — **ค่าสำหรับแสดงเท่านั้น** (P4-4)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverView {
    /// เขียนไว้เมื่อไหร่ (ข้อความพร้อมแสดงแล้ว) — `None` = ระบบไฟล์ไม่บอก
    pub when: Option<String>,
    /// มีกี่ชิ้นอยู่ในนั้น — ช่วยผู้ใช้จำว่าเป็นงานชิ้นไหน
    pub items: usize,
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
    /// ★ กำลังถามว่าจะปิดยังไงทั้งที่ยังไม่ได้บันทึก (P4-2)
    pub close_prompt: bool,
    /// ผู้ใช้ตอบแล้วในเฟรมนี้ — `None` = ยังไม่ตอบ
    pub close_choice: Option<CloseChoice>,
    /// ★★★ เจองานที่ยังไม่ได้บันทึกจาก session ก่อน (P4-4) — `None` = ไม่มีอะไรค้าง
    pub recover_prompt: Option<RecoverView>,
    /// ผู้ใช้ตอบแล้วในเฟรมนี้ — `None` = ยังไม่ตอบ
    pub recover_choice: Option<RecoverChoice>,
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
fn human_bytes(bytes: u64) -> String {
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
            appearance: None,
            appearance_edit: None,
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
            recover_prompt: None,
            recover_choice: None,
            group_request: None,
            group_sealed: false,
            picked: None,
            measured: None,
            loading: None,
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
            // P4-7: หลาย board พร้อมกัน
            let _ = ui.selectable_label(true, text::t(lang, Key::UntitledBoard));
            if ui
                .button("+")
                .on_hover_text(text::t(lang, Key::NewBoardHint))
                .clicked()
            {
                state.status = text::fill(
                    lang,
                    Template::NotImplemented,
                    &[("what", text::t(lang, Key::NewBoardHint)), ("when", "P4-7")],
                );
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
                ui.label(
                    egui::RichText::new(text::t(lang, Key::CloseUnsavedTitle))
                        .strong()
                        .color(WARN_COLOR),
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

    // ---- ★★★ แถบกู้คืนงานที่ยังไม่ได้บันทึกจากรอบก่อน (P4-4) ----
    //
    //   วางไว้ที่เดียวกับแถบยืนยันตอนปิด ด้วยเหตุผลเดียวกัน (เห็นแน่ แต่ยัง
    //   เห็นงานข้างหลัง · ไม่บล็อก UI thread แบบ native dialog — I-2)
    if let Some(found) = state.recover_prompt.clone() {
        egui::Panel::top("refx-recover").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(text::t(lang, Key::RecoverTitle))
                        .strong()
                        .color(WARN_COLOR),
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
                if ui
                    .button(text::t(lang, Key::RecoverLater))
                    .on_hover_text(text::t(lang, Key::RecoverLaterHint))
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
                ui.colored_label(WARN_COLOR, &state.status);
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
            // ★★ หลักฐานของเกณฑ์ P3-3 ที่เห็นได้ด้วยตา — "10,000 ใบ วาดจริง < 60"
            //    โชว์เฉพาะโหมด Arrange เพราะเป็นตัวเลขของ virtual scrolling
            //    (Canvas วาดทั้ง board อยู่แล้ว ตัวเลขจะไม่มีความหมายที่นั่น)
            if state.mode == Mode::Arrange {
                // ★★ กรองอยู่ = ต้องเห็นได้เสมอ ผู้ใช้ที่มองหาภาพที่ "หายไป"
                //   ต้องรู้ทันทีว่ามันถูกกรอง ไม่ใช่หาย (ไม่งั้นอ่านว่าโปรแกรมทำงานหาย)
                if !state.arrange_filter.is_open() {
                    ui.separator();
                    ui.colored_label(
                        WARN_COLOR,
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
                ui.colored_label(WARN_COLOR, ram);
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
                ui.colored_label(WARN_COLOR, vram);
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

            // ---- สถานะของ *มุมมอง* — เปลี่ยนได้ตามใจ ไม่แตะเอกสาร ----
            mode: _,
            close_prompt: _,    // บอกแค่ว่าแถบยืนยันโผล่อยู่ไหม ไม่ใช่คำขอแก้อะไร
            recover_prompt: _,  // เหมือนกัน — แค่ "มีอะไรค้างให้ถามไหม"
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
            tag_input: _,          // ข้อความในช่องพิมพ์ ยังไม่ได้กด +
            picked: _,             // สีที่ picker อ่านได้
            measured: _,           // ไม้บรรทัด
            loading: _,

            // ---- ตัวเลขที่โชว์บน status bar ----
            atlas_uploads: _,
            item_count: _,
            zoom: _,
            frames_drawn: _,
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
            || *arrange_apply
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
