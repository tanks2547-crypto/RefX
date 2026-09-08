//! ★★★ ตารางคีย์ลัด — **ที่เดียวที่ตัดสินว่าปุ่มไหนแปลว่าอะไร** (P5-3b ก้อน a)
//!
//! spec: `docs/03-modes-and-ui.md §5` · `ROADMAP` P5-3b
//!
//! ## ก้อน a และ b ทำอะไร และ **ยังไม่** ทำอะไร
//!
//! **ก้อน a** ย้ายการจับคู่ทั้งหมดมาเป็น **ข้อมูล** แล้วให้ฟังก์ชันเดิมใน
//! `app.rs` กลายเป็น wrapper บาง ๆ ที่อ่านตารางนี้ · ★★ **assertion เดิมคือ
//! oracle** ที่พิสูจน์ว่าตารางให้ผลเท่าของเดิมเป๊ะ จึงห้ามเพิ่มคีย์ใหม่ตอนนั้น
//!
//! **ก้อน b** (ที่นี่) รับตารางจาก `keymap.toml` ของผู้ใช้ + **ประตูตรวจการชน**
//! · ไฟล์ถูกอ่านเป็นสตริงดิบที่ `refx_io::keymap` แล้วแปลงเป็นชนิดจริงด้วย
//! [`Keymap::from_rows`]
//!
//! **ก้อน c** เพิ่ม 6 คีย์ที่ `docs/03 §5` สั่งไว้ตั้งแต่วันแรกแต่ไม่เคยมี —
//! `Tab` · `Ctrl+A` · `Esc` · `F` · `1` · `0` — ปิดช่องว่าง "17 คีย์ใน spec
//! vs 11 คีย์ที่ทำจริง" ที่กล่อง ⚠️ ของ `docs/03 §5` บันทึกไว้
//! · ★★ ทั้งหกเข้า **ตารางเดียวกันนี้** ไม่ใช่กิ่ง `if` นอกตาราง มันจึงแก้ได้
//! ด้วย `keymap.toml` และประตูตรวจการชนมองเห็นมันเหมือนคีย์อื่นทุกประการ
//!
//! ## ทำไมไม่ใช่ `HashMap<(Modifiers, Key), Action>` (`docs/03 §5` แก้ 4 ก.ย. 2026)
//!
//! ห้าเรื่องที่ตารางแบนแสดงไม่ได้ และทั้งห้าอยู่ในโค้ดจริงมาก่อนแล้ว:
//!
//! | เรื่อง | ตารางนี้แก้ยังไง |
//! |---|---|
//! | logical → physical fallback (layout ไทย/รัสเซีย/กรีก) | คีย์ของตารางคือ **ผลของ `shortcut_char`** ไม่ใช่ปุ่มดิบ — สองชั้นถูกยุบไปก่อนถึงที่นี่แล้ว |
//! | `Shift+[` มาถึงเป็น `{` | มี binding แยกให้ `'{'` โดยที่ shift เป็น [`Hold::Either`] |
//! | named key ไม่ผ่าน `shortcut_char` เลย | [`Chord::Key`] เป็นคนละกิ่งกับ [`Chord::Char`] |
//! | นโยบาย `repeat` ต่างกันต่อ action | [`Binding::repeat`] เป็นข้อมูล ไม่ใช่ `!event.repeat` ที่กระจายอยู่ 8 จุด |
//! | กฎ modifier ต่างกันต่อ matcher | [`Hold`] มี **สามสถานะ** ไม่ใช่สอง |
//!
//! ## ★ ลำดับ `Char` ก่อน `Key` — **ห้ามสลับ** (`docs/03 §5`)
//!
//! `Char` คือผลของ `shortcut_char` ซึ่งเป็น "logical ก่อน physical เป็นตาข่ายรอง"
//! อยู่แล้ว · `Key` เป็นตาข่ายชั้นนอกสุดสำหรับปุ่มที่ไม่ใช่ตัวอักษรเลย
//! (`Delete` · `Tab`) · ถ้าถาม `Key` ก่อน ปุ่มที่ layout แปลงเป็นอักขระได้จะถูก
//! ตาข่ายชั้นนอกดักไปก่อน แล้วเจตนาของผู้ใช้บน layout ที่ไม่ใช่ QWERTY จะหาย

use refx_core::interact::Tool;
use refx_core::zorder::ZMove;
use winit::keyboard::{ModifiersState, NamedKey};

/// ผู้ใช้ขออะไรกับประวัติ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRequest {
    /// Ctrl+Z
    Undo,
    /// Ctrl+Y หรือ Ctrl+Shift+Z
    Redo,
}

/// ปุ่มที่แตะการแสดงผล
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceKey {
    /// `G` — ขาวดำทั้ง board (การมองเห็น ไม่ใช่เอกสาร)
    ToggleBoardGrayscale,
    /// `H` — พลิกแนวนอนของภาพที่เลือก (เอกสาร → ผ่าน Command)
    FlipHorizontal,
}

/// ผู้ใช้ขออะไรกับกลุ่ม (P3-7)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupRequest {
    /// `Ctrl+G` — รวมสิ่งที่เลือกเป็นกลุ่มใหม่
    Group,
    /// `Ctrl+Shift+G` — เอาสิ่งที่เลือกออกจากกลุ่ม
    Ungroup,
}

/// ผู้ใช้ขออะไรกับการบันทึก (P4-2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveRequest {
    /// `Ctrl+S` — บันทึกลงที่เดิม (ยังไม่เคยบันทึก = ถามที่เก็บก่อน)
    Save,
    /// `Ctrl+Shift+S` — ถามที่เก็บใหม่เสมอ
    SaveAs,
}

/// ผู้ใช้ขออะไรกับระดับซูม (P5-3b ก้อน c · `docs/03 §5`)
///
/// ★★ `0` กับ `F`-ตอนไม่ได้เลือกอะไร **ต้องให้ผลเดียวกัน** ตามที่ตารางใน
/// `docs/03 §5` เขียนไว้ ("ถ้าไม่เลือก = พอดีทั้ง board") — ทำได้ด้วยการให้
/// [`Self::FitSelection`] ตกไปเป็น [`Self::FitBoard`] เองเมื่อไม่มีอะไรเลือก
/// ไม่ใช่ให้ผู้เรียกจำกฎนั้นเอง (`docs/08 §3.9` ข้อ 8.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomRequest {
    /// `1` — ซูม 100% (1 พิกเซล world = 1 พิกเซลจอ) ไม่ขยับจุดกึ่งกลาง
    Actual,
    /// `F` — พอดีกับสิ่งที่เลือก · ไม่ได้เลือกอะไร = พอดีทั้ง board
    FitSelection,
    /// `0` — พอดีทั้ง board เสมอ
    FitBoard,
}

/// ปุ่มของแท็บที่ผู้ใช้เพิ่งกด (P4-7c)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKey {
    /// `Ctrl+T`
    New,
    /// `Ctrl+W`
    Close,
    /// `Ctrl+Tab`
    Next,
}

/// สิ่งที่ปุ่มหนึ่งชุดสั่งได้ — **คำศัพท์ทั้งหมดของคีย์ลัดอยู่ที่นี่**
///
/// ★ ไม่มี variant ไหนที่ยังไม่มีคนทำ · หกตัวสุดท้ายเข้ามาตอนก้อน c ของ P5-3b
/// (`Tab` · `Ctrl+A` · `Esc` · `F` · `1` · `0`) ซึ่งปิดช่องว่าง 17 คีย์ใน spec
/// vs 11 คีย์ที่ทำจริง (`docs/03 §5` กล่อง ⚠️)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// ย้อน/ทำซ้ำ
    History(HistoryRequest),
    /// วางภาพจาก clipboard
    Paste,
    /// ลบสิ่งที่เลือก
    Delete,
    /// ย้ายชั้น
    ZOrder(ZMove),
    /// สลับเครื่องมือ
    Tool(Tool),
    /// สวิตช์การแสดงผล
    Appearance(AppearanceKey),
    /// จัดกลุ่ม / แยกกลุ่ม
    Group(GroupRequest),
    /// บันทึก / บันทึกเป็น
    Save(SaveRequest),
    /// เปิดกระดาน
    OpenBoard,
    /// คีย์ของแท็บ
    Tab(TabKey),

    // ---------- ★ P5-3b ก้อน c: หกตัวที่ `docs/03 §5` สั่งไว้แต่ไม่เคยมี ----------
    /// `Tab` — สลับ Canvas ⇄ Arrange · **เป็นของแท็บ ไม่ใช่ของหน้าต่าง**
    ToggleMode,
    /// `Ctrl+A` — เลือกทุก item บน board ใบนี้
    SelectAll,
    /// `Esc` — ยกเลิกเลือก (หรือปิดแถบที่ค้างอยู่ก่อน ถ้ามี)
    ClearSelection,
    /// `F` / `1` / `0` — ระดับซูม
    Zoom(ZoomRequest),

    /// `Ctrl+E` — เปิดกล่องส่งออกภาพ (P5-4 · `docs/07 §6`)
    Export,
}

impl Action {
    /// ★★★ ชื่อที่ผู้ใช้เขียนใน `keymap.toml` — **สัญญากับไฟล์ ห้ามเปลี่ยนพร่ำเพรื่อ**
    ///
    /// เปลี่ยนชื่อเมื่อไหร่ `keymap.toml` ของผู้ใช้ทุกคนที่ใช้ชื่อเดิมจะกลายเป็น
    /// "action ไม่รู้จัก" แล้ว**คีย์ลัดหายทั้งไฟล์** (ตามกฎ "พังที่ไหนก็ใช้
    /// ค่าปริยายทั้งชุด") · `match` ไม่มี `_ =>` โดยตั้งใจ: เพิ่ม action ใหม่
    /// เมื่อไหร่ คอมไพเลอร์บังคับให้มาตั้งชื่อให้มัน
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::History(HistoryRequest::Undo) => "undo",
            Self::History(HistoryRequest::Redo) => "redo",
            Self::Paste => "paste",
            Self::Delete => "delete",
            Self::ZOrder(ZMove::Backward) => "send-backward",
            Self::ZOrder(ZMove::Forward) => "send-forward",
            Self::ZOrder(ZMove::ToBack) => "send-to-back",
            Self::ZOrder(ZMove::ToFront) => "send-to-front",
            Self::Tool(Tool::Select) => "tool-select",
            Self::Tool(Tool::Crop) => "tool-crop",
            Self::Tool(Tool::Picker) => "tool-picker",
            Self::Tool(Tool::Measure) => "tool-measure",
            Self::Tool(Tool::Text) => "tool-text",
            Self::Appearance(AppearanceKey::ToggleBoardGrayscale) => "toggle-grayscale",
            Self::Appearance(AppearanceKey::FlipHorizontal) => "flip-horizontal",
            Self::Group(GroupRequest::Group) => "group",
            Self::Group(GroupRequest::Ungroup) => "ungroup",
            Self::Save(SaveRequest::Save) => "save",
            Self::Save(SaveRequest::SaveAs) => "save-as",
            Self::OpenBoard => "open-board",
            Self::Tab(TabKey::New) => "new-tab",
            Self::Tab(TabKey::Close) => "close-tab",
            Self::Tab(TabKey::Next) => "next-tab",
            Self::ToggleMode => "toggle-mode",
            Self::SelectAll => "select-all",
            Self::ClearSelection => "clear-selection",
            Self::Export => "export",
            Self::Zoom(ZoomRequest::Actual) => "zoom-100",
            Self::Zoom(ZoomRequest::FitSelection) => "zoom-fit-selection",
            Self::Zoom(ZoomRequest::FitBoard) => "zoom-fit",
        }
    }

    /// ทุก action ที่มี — ★ ใช้ทั้งตอนแปลงชื่อกลับ และตอนแสดงรายการบนแผง
    ///
    /// ★★ **ประกอบจาก [`Self::name`] ไม่ใช่ตารางชื่อชุดที่สอง** — สองรายการ
    /// ที่ต้องตรงกันเองคือรายการที่วันหนึ่งจะไม่ตรงกัน
    pub const ALL: &'static [Self] = &[
        Self::History(HistoryRequest::Undo),
        Self::History(HistoryRequest::Redo),
        Self::Paste,
        Self::Delete,
        Self::ZOrder(ZMove::Backward),
        Self::ZOrder(ZMove::Forward),
        Self::ZOrder(ZMove::ToBack),
        Self::ZOrder(ZMove::ToFront),
        Self::Tool(Tool::Select),
        Self::Tool(Tool::Crop),
        Self::Tool(Tool::Picker),
        Self::Tool(Tool::Measure),
        Self::Tool(Tool::Text),
        Self::Appearance(AppearanceKey::ToggleBoardGrayscale),
        Self::Appearance(AppearanceKey::FlipHorizontal),
        Self::Group(GroupRequest::Group),
        Self::Group(GroupRequest::Ungroup),
        Self::Save(SaveRequest::Save),
        Self::Save(SaveRequest::SaveAs),
        Self::OpenBoard,
        Self::Tab(TabKey::New),
        Self::Tab(TabKey::Close),
        Self::Tab(TabKey::Next),
        Self::ToggleMode,
        Self::SelectAll,
        Self::ClearSelection,
        Self::Export,
        Self::Zoom(ZoomRequest::Actual),
        Self::Zoom(ZoomRequest::FitSelection),
        Self::Zoom(ZoomRequest::FitBoard),
    ];

    /// ชื่อจากไฟล์ → action — `None` ถ้าไม่รู้จัก
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.name() == name)
    }
}

/// ★★ เงื่อนไขของปุ่มค้างหนึ่งตัว — **สามสถานะ ไม่ใช่สอง**
///
/// นี่คือเหตุผลหลักที่ตารางแบนที่คีย์ด้วย `Modifiers` ทำงานแทนโค้ดเดิมไม่ได้:
/// matcher แต่ละตัวใช้กฎคนละอย่างกับ modifier ตัวเดียวกัน และ**ตั้งใจให้ต่างกัน**
///
/// | ตัวอย่างจริง | ctrl | shift | alt |
/// |---|---|---|---|
/// | `Ctrl+Z` = undo | [`Hold::Down`] | [`Hold::Up`] | [`Hold::Either`] |
/// | `Ctrl+Shift+Z` = redo | [`Hold::Down`] | [`Hold::Down`] | [`Hold::Either`] |
/// | `Ctrl+Y` = redo | [`Hold::Down`] | [`Hold::Either`] | [`Hold::Either`] |
/// | `Ctrl+O` = เปิด | [`Hold::Down`] | [`Hold::Up`] | [`Hold::Up`] |
/// | `V` = เครื่องมือเลือก | [`Hold::Up`] | [`Hold::Either`] | [`Hold::Up`] |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// ต้องกดค้างไว้
    Down,
    /// ต้องไม่กด
    Up,
    /// ★ กดหรือไม่กดก็ได้ — **เป็นการตัดสินใจ ไม่ใช่การลืมใส่**
    ///
    /// `Ctrl+Y` ไม่สน shift เพราะ `Y` ไม่มีความหมายอื่นตอนกด Ctrl · ส่วน
    /// `Ctrl+Z` สน เพราะ `Ctrl+Shift+Z` เป็น redo · ความต่างนี้มีมาก่อนตาราง
    /// และตารางต้องแสดงมันได้ ไม่ใช่กลบมันทิ้ง
    Either,
}

impl Hold {
    /// ปุ่มค้างจริงในสถานะนี้ ตรงกับเงื่อนไขไหม
    #[must_use]
    const fn allows(self, down: bool) -> bool {
        match self {
            Self::Down => down,
            Self::Up => !down,
            Self::Either => true,
        }
    }

    /// สองเงื่อนไขนี้ **มีสถานะร่วมกันได้ไหม** — ใช้ตรวจการชนกันของ binding
    ///
    /// ชนกันไม่ได้ก็ต่อเมื่อฝั่งหนึ่งบังคับ `Down` และอีกฝั่งบังคับ `Up`
    ///
    /// ★ ผู้เรียกวันนี้คือประตู `no_single_keypress_can_ever_fire_two_actions`
    /// ซึ่งคุม **ตารางที่ส่งถึงผู้ใช้จริง** · ก้อน b จะใช้ตัวเดียวกันนี้ตรวจ
    /// `keymap.toml` ที่ผู้ใช้แก้เอง แล้วบอกเขาเมื่อคีย์ชนกัน
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        !matches!(
            (self, other),
            (Self::Down, Self::Up) | (Self::Up, Self::Down)
        )
    }
}

/// เงื่อนไขของปุ่มค้างทั้งชุด
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mods {
    /// Ctrl
    pub ctrl: Hold,
    /// Shift
    pub shift: Hold,
    /// Alt
    pub alt: Hold,
}

impl Mods {
    /// เขียนสั้น ๆ ในตาราง — `Mods::new(Down, Up, Either)`
    #[must_use]
    pub const fn new(ctrl: Hold, shift: Hold, alt: Hold) -> Self {
        Self { ctrl, shift, alt }
    }

    /// สถานะปุ่มค้างจริงตรงกับเงื่อนไขนี้ไหม
    #[must_use]
    pub fn matches(self, state: ModifiersState) -> bool {
        self.ctrl.allows(state.control_key())
            && self.shift.allows(state.shift_key())
            && self.alt.allows(state.alt_key())
    }

    /// มีสถานะปุ่มค้างสักชุดที่ตรงกับ **ทั้งสอง** เงื่อนไขไหม
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.ctrl.overlaps(other.ctrl)
            && self.shift.overlaps(other.shift)
            && self.alt.overlaps(other.alt)
    }
}

/// ปุ่มหนึ่งชุดที่ผู้ใช้กด
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    /// อักขระ ASCII ที่ `shortcut_char` คืนมา (**ผ่านชั้น logical→physical มาแล้ว**)
    Char {
        /// ตัวอักษรตัวเล็ก หรืออักขระควบคุมที่บางระบบส่งมาแทน (`Ctrl+Z` = `\u{1a}`)
        ch: char,
        /// เงื่อนไขปุ่มค้าง
        mods: Mods,
    },
    /// ปุ่มที่มีชื่อ — `shortcut_char` มองไม่เห็นเลย (`Delete` · `Tab`)
    Key {
        /// ปุ่มที่มีชื่อตามที่ winit รายงาน
        key: NamedKey,
        /// เงื่อนไขปุ่มค้าง
        mods: Mods,
    },
}

/// ปุ่มที่มีชื่อที่ `keymap.toml` เขียนได้ — ★ รายการสั้นโดยตั้งใจ
///
/// เพิ่มตัวใหม่ได้เสมอ แต่ตัวที่ **ไม่มี action ให้ผูก** จะกลายเป็นแถวที่ผู้ใช้
/// เขียนได้แล้วไม่เกิดอะไรขึ้น ซึ่งอ่านว่าโปรแกรมเสีย
const NAMED_KEYS: &[(&str, NamedKey)] = &[
    ("delete", NamedKey::Delete),
    ("backspace", NamedKey::Backspace),
    ("tab", NamedKey::Tab),
    ("escape", NamedKey::Escape),
    ("enter", NamedKey::Enter),
    ("space", NamedKey::Space),
];

impl Chord {
    /// เงื่อนไขปุ่มค้างของ chord นี้ — ใช้ตรวจการชน (ดู [`Mods::overlaps`])
    #[must_use]
    pub const fn mods(self) -> Mods {
        match self {
            Self::Char { mods, .. } | Self::Key { mods, .. } => mods,
        }
    }

    /// ★★★ แปลงข้อความที่ผู้ใช้เขียนเป็น chord — `"ctrl+shift+z"` · `"["` · `"delete"`
    ///
    /// ## ★★ modifier จากไฟล์เป็น **ค่าตรงตัวเสมอ** ไม่มี [`Hold::Either`]
    ///
    /// เขียน `ctrl+z` แปลว่า **ctrl กด shift ไม่กด alt ไม่กด** เป๊ะ ๆ ·
    /// `Either` มีอยู่เพื่ออธิบายควาามเป็นมาของตารางค่าปริยาย (เช่น `Ctrl+Y`
    /// ที่ไม่เคยสน shift มาแต่ไหนแต่ไร) **ไม่ใช่คำศัพท์ที่ผู้ใช้ต้องเรียนรู้** —
    /// ให้เขาเขียนสิ่งที่เขาหมายถึงตรง ๆ แล้วได้สิ่งนั้นกลับไป
    ///
    /// ★ ปฏิเสธอักขระที่ไม่ใช่ ASCII: `shortcut_char` ไม่มีทางผลิตมันออกมาได้เลย
    /// (logical ที่ไม่ใช่ ASCII ตกไปชั้น physical เสมอ) — binding ที่ไม่มีวัน
    /// ถูกจุดคือ binding ที่หลอกผู้ใช้ว่าเขาตั้งค่าสำเร็จแล้ว
    #[must_use]
    pub fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim();
        let mut mods = Mods::new(Up, Up, Up);
        let mut rest = spec;
        // ★ กินทีละ modifier จากซ้าย · หยุดทันทีที่เจอคำที่ไม่ใช่ modifier
        //   ทำให้ `"ctrl++"` อ่านได้ว่า Ctrl + ปุ่ม `+` ไม่ใช่ token ว่าง
        while let Some(plus) = rest.find('+') {
            if plus == 0 {
                break; // ปุ่มคือ `+` เอง
            }
            let (head, tail) = rest.split_at(plus);
            match head.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods.ctrl = Down,
                "shift" => mods.shift = Down,
                "alt" => mods.alt = Down,
                _ => break,
            }
            rest = &tail[1..];
        }
        if rest.is_empty() {
            return None;
        }
        let mut chars = rest.chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            return ch.is_ascii().then_some(Chord::Char {
                ch: ch.to_ascii_lowercase(),
                mods,
            });
        }
        let wanted = rest.to_ascii_lowercase();
        NAMED_KEYS
            .iter()
            .find(|(name, _)| *name == wanted)
            .map(|(_, key)| Chord::Key { key: *key, mods })
    }

    /// ข้อความที่แสดงบนแผง Settings — `None` ถ้าเขียนออกมาแล้วผู้ใช้อ่านไม่ได้
    ///
    /// ★★★ **อักขระควบคุมคืน `None`** — ตารางค่าปริยายมี alias อย่าง
    /// `Ctrl+Z` = `\u{1a}` สำหรับ compositor ที่ส่งมาแบบนั้น · มันไม่มี glyph
    /// ในฟอนต์ที่เราฝัง การวาดมันลงแผงคือ **สี่เหลี่ยม tofu** ซึ่งเป็นบั๊กที่
    /// โปรเจกต์นี้เจอมาแล้วสองครั้ง (`shell::UNSAVED_MARK` · จุดเตือนของ P5-3a)
    /// · มันเป็นรายละเอียดของแพลตฟอร์ม ไม่ใช่คีย์ลัดคนละตัว
    #[must_use]
    pub fn display(self) -> Option<String> {
        let mut out = String::new();
        let mods = self.mods();
        // ★ แสดงเฉพาะตัวที่ **บังคับให้กด** — `Either` แปลว่า "ไม่เกี่ยว"
        //   จึงไม่ควรโผล่มาเป็นเงื่อนไขให้ผู้ใช้เข้าใจผิดว่าต้องกด
        for (hold, name) in [
            (mods.ctrl, "ctrl"),
            (mods.shift, "shift"),
            (mods.alt, "alt"),
        ] {
            if hold == Down {
                out.push_str(name);
                out.push('+');
            }
        }
        match self {
            Self::Char { ch, .. } => {
                if ch.is_control() {
                    return None;
                }
                out.push(ch);
            }
            Self::Key { key, .. } => {
                let name = NAMED_KEYS.iter().find(|(_, k)| *k == key)?;
                out.push_str(name.0);
            }
        }
        Some(out)
    }
}

/// การกดค้างหมายถึงอะไรสำหรับ action นี้
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatPolicy {
    /// ★ กดค้างแล้วสั่งซ้ำได้ — กด `Ctrl+Z` ค้างแล้วย้อนเรื่อย ๆ คือสิ่งที่ทุกคนคาดหวัง
    Allow,
    /// ★★ กดค้างแล้วสั่งซ้ำ **ไม่ได้** — กดค้างหนึ่งวินาทีแล้วลบทั้ง board /
    /// เปิดแท็บสิบใบ / เขียนไฟล์สิบรอบ คือหายนะจากการกดครั้งเดียว
    Once,
}

/// ปุ่มหนึ่งชุด → สิ่งที่มันสั่ง
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// ปุ่มที่ต้องกด
    pub chord: Chord,
    /// สิ่งที่มันสั่ง
    pub action: Action,
    /// กดค้างแล้วซ้ำได้ไหม
    pub repeat: RepeatPolicy,
}

use Action as A;
use Hold::{Down, Either, Up};
use RepeatPolicy::{Allow, Once};

/// เขียน binding แบบอักขระให้สั้นลง — ตารางข้างล่างยาวพออยู่แล้ว
const fn ch(ch: char, mods: Mods, action: Action, repeat: RepeatPolicy) -> Binding {
    Binding {
        chord: Chord::Char { ch, mods },
        action,
        repeat,
    }
}

/// เขียน binding แบบปุ่มที่มีชื่อให้สั้นลง
const fn named(key: NamedKey, mods: Mods, action: Action, repeat: RepeatPolicy) -> Binding {
    Binding {
        chord: Chord::Key { key, mods },
        action,
        repeat,
    }
}

/// ★★★ **ตารางค่าปริยาย** — ย้ายมาจาก 11 ฟังก์ชันใน `app.rs` แบบตัวต่อตัว
///
/// ★ อักขระควบคุม (`\u{1a}` ฯลฯ) เป็น **binding แยกใบ** ไม่ใช่ `|` ใน `match`
/// บางระบบ (X11 บาง compositor) ส่ง `Ctrl+<ตัวอักษร>` มาเป็นอักขระควบคุมแทน
/// ตัวอักษรพร้อมธง — ถ้าไม่รับ คีย์ลัดจะตายเฉพาะบนเครื่องพวกนั้นโดยไม่มีใครรู้
///
/// ★★ เรียงตามหน้าที่ ไม่ใช่ตามตัวอักษร — คนที่มาแก้มองหา "คีย์ของแท็บ"
/// ไม่ได้มองหา "ตัว t"
static BUILTIN: &[Binding] = &[
    // ---- ประวัติ (Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y) ----
    //
    // ★ alt เป็น Either โดยตั้งใจ: โค้ดเดิมไม่เคยตรวจ alt ในเส้นทางนี้
    //   (ต่างจาก save/group/open ที่ตรวจ) — ตารางต้องแสดงความต่างนี้ ไม่ใช่กลบ
    ch(
        'z',
        Mods::new(Down, Up, Either),
        A::History(HistoryRequest::Undo),
        Allow,
    ),
    ch(
        '\u{1a}',
        Mods::new(Down, Up, Either),
        A::History(HistoryRequest::Undo),
        Allow,
    ),
    ch(
        'z',
        Mods::new(Down, Down, Either),
        A::History(HistoryRequest::Redo),
        Allow,
    ),
    ch(
        '\u{1a}',
        Mods::new(Down, Down, Either),
        A::History(HistoryRequest::Redo),
        Allow,
    ),
    ch(
        'y',
        Mods::new(Down, Either, Either),
        A::History(HistoryRequest::Redo),
        Allow,
    ),
    ch(
        '\u{19}',
        Mods::new(Down, Either, Either),
        A::History(HistoryRequest::Redo),
        Allow,
    ),
    // ---- วางจาก clipboard (Ctrl+V) ----
    //
    // ★★ `Once`: ภาพจาก clipboard ใหญ่ได้ระดับ 6000×4000 (96 MB) และ `arboard`
    //    จอง RAM ก้อนนั้นก่อนเพดานของเราจะได้ตรวจ — กดค้าง = แย่ง RAM กับ Photoshop
    ch('v', Mods::new(Down, Either, Either), A::Paste, Once),
    ch('\u{16}', Mods::new(Down, Either, Either), A::Paste, Once),
    // ---- ลบ (Delete / Backspace) ----
    //
    // ★ modifier เป็น Either ทั้งชุด: โค้ดเดิม (`is_delete`) ไม่ตรวจเลยสักตัว
    // ★★ `Once`: กดค้างหนึ่งวินาทีแล้วลบทีละชุดจนหมด board คือหายนะที่ undo
    //    ต้องกดกลับหลายสิบครั้ง ทั้งที่ผู้ใช้ตั้งใจกดครั้งเดียว
    named(
        NamedKey::Delete,
        Mods::new(Either, Either, Either),
        A::Delete,
        Once,
    ),
    named(
        NamedKey::Backspace,
        Mods::new(Either, Either, Either),
        A::Delete,
        Once,
    ),
    // ---- ย้ายชั้น (`[` `]` และ `{` `}`) ----
    //
    // ★ ต้องมี `{` `}` แยกใบ: บนคีย์บอร์ดส่วนใหญ่ Shift+`[` **ส่ง `{` มาเลย**
    //   ไม่ได้ส่ง `[` พร้อมธง shift — ดูแต่ธงแล้วปุ่มสุดหัว-สุดท้ายจะไม่ทำงานเลย
    // ★ `Allow`: กด `]` รัว ๆ จนถึงบนสุดคือท่าปกติ
    ch(
        '[',
        Mods::new(Up, Up, Up),
        A::ZOrder(ZMove::Backward),
        Allow,
    ),
    ch(']', Mods::new(Up, Up, Up), A::ZOrder(ZMove::Forward), Allow),
    ch(
        '[',
        Mods::new(Up, Down, Up),
        A::ZOrder(ZMove::ToBack),
        Allow,
    ),
    ch(
        ']',
        Mods::new(Up, Down, Up),
        A::ZOrder(ZMove::ToFront),
        Allow,
    ),
    ch(
        '{',
        Mods::new(Up, Either, Up),
        A::ZOrder(ZMove::ToBack),
        Allow,
    ),
    ch(
        '}',
        Mods::new(Up, Either, Up),
        A::ZOrder(ZMove::ToFront),
        Allow,
    ),
    // ---- เครื่องมือ (docs/03 §2) ----
    ch('v', Mods::new(Up, Either, Up), A::Tool(Tool::Select), Allow),
    ch('c', Mods::new(Up, Either, Up), A::Tool(Tool::Crop), Allow),
    ch('i', Mods::new(Up, Either, Up), A::Tool(Tool::Picker), Allow),
    ch(
        'm',
        Mods::new(Up, Either, Up),
        A::Tool(Tool::Measure),
        Allow,
    ),
    ch('t', Mods::new(Up, Either, Up), A::Tool(Tool::Text), Allow),
    // ---- การแสดงผล (`G` `H`) ----
    //
    // ★ `G` เปล่า ๆ กับ `Ctrl+G` อยู่คนละชั้นกันโดยสิ้นเชิง — แยกด้วย ctrl ตัวเดียว
    //   ทั้งคู่จึงต้องบังคับ ctrl ของตัวเองอย่างเคร่งครัด (ไม่ใช่ `Either`)
    ch(
        'g',
        Mods::new(Up, Either, Up),
        A::Appearance(AppearanceKey::ToggleBoardGrayscale),
        Once,
    ),
    ch(
        'h',
        Mods::new(Up, Either, Up),
        A::Appearance(AppearanceKey::FlipHorizontal),
        Once,
    ),
    // ---- กลุ่ม (Ctrl+G / Ctrl+Shift+G) ----
    ch(
        'g',
        Mods::new(Down, Up, Up),
        A::Group(GroupRequest::Group),
        Once,
    ),
    ch(
        '\u{7}',
        Mods::new(Down, Up, Up),
        A::Group(GroupRequest::Group),
        Once,
    ),
    ch(
        'g',
        Mods::new(Down, Down, Up),
        A::Group(GroupRequest::Ungroup),
        Once,
    ),
    ch(
        '\u{7}',
        Mods::new(Down, Down, Up),
        A::Group(GroupRequest::Ungroup),
        Once,
    ),
    // ---- บันทึก (Ctrl+S / Ctrl+Shift+S) ----
    //
    // ★★ `Once`: กดค้างหนึ่งวินาที = เขียนไฟล์หลายสิบรอบ และเปิด dialog
    //    ซ้อนกันเป็นสิบบานถ้ายังไม่เคยบันทึก
    ch(
        's',
        Mods::new(Down, Up, Up),
        A::Save(SaveRequest::Save),
        Once,
    ),
    ch(
        '\u{13}',
        Mods::new(Down, Up, Up),
        A::Save(SaveRequest::Save),
        Once,
    ),
    ch(
        's',
        Mods::new(Down, Down, Up),
        A::Save(SaveRequest::SaveAs),
        Once,
    ),
    ch(
        '\u{13}',
        Mods::new(Down, Down, Up),
        A::Save(SaveRequest::SaveAs),
        Once,
    ),
    // ---- เปิดกระดาน (Ctrl+O) ----
    //
    // ★ shift เป็น `Up`: `Ctrl+Shift+O` ยังไม่มีความหมาย และปุ่มที่ยังไม่มี
    //   ความหมายต้องเงียบ ไม่ใช่ทำอะไรที่ผู้ใช้ไม่ได้ขอ
    ch('o', Mods::new(Down, Up, Up), A::OpenBoard, Once),
    ch('\u{f}', Mods::new(Down, Up, Up), A::OpenBoard, Once),
    // ---- ★ ส่งออกภาพ (Ctrl+E) — P5-4 ----
    //
    // ★★ `Once` เสมอ: กดค้าง = เปิดกล่อง export ซ้ำ ๆ ทุกเฟรม ซึ่งนอกจากไร้ผล
    //    แล้วยังทำให้ค่าที่ผู้ใช้เพิ่งปรับในกล่องถูกรีเซ็ตทิ้งทุกเฟรม
    ch('e', Mods::new(Down, Up, Up), A::Export, Once),
    ch('\u{5}', Mods::new(Down, Up, Up), A::Export, Once),
    // ---- แท็บ (Ctrl+T / Ctrl+W / Ctrl+Tab) ----
    //
    // ★ `Ctrl+Tab` shift เป็น `Either` — โค้ดเดิมคืน `Next` ก่อนจะไปถึงด่าน shift
    ch('t', Mods::new(Down, Up, Up), A::Tab(TabKey::New), Once),
    ch('\u{14}', Mods::new(Down, Up, Up), A::Tab(TabKey::New), Once),
    ch('w', Mods::new(Down, Up, Up), A::Tab(TabKey::Close), Once),
    ch(
        '\u{17}',
        Mods::new(Down, Up, Up),
        A::Tab(TabKey::Close),
        Once,
    ),
    named(
        NamedKey::Tab,
        Mods::new(Down, Either, Up),
        A::Tab(TabKey::Next),
        Once,
    ),
    // ---- ★★★ P5-3b ก้อน c: หกคีย์ที่ spec สั่งไว้ตั้งแต่วันแรกแต่ไม่เคยมี ----
    //
    // ★ `Tab` เปล่า ๆ vs `Ctrl+Tab`: แยกกันด้วย ctrl ที่บังคับคนละทาง ประตู
    //   `no_single_keypress_can_ever_fire_two_actions` จึงยืนยันได้ว่าไม่ทับกัน
    // ★★ `Once` ทั้งหมด: กด `Tab` ค้าง = โหมดสลับ 30 ครั้งต่อวินาที · `Esc` ค้าง
    //    = ล้างการเลือกซ้ำ ๆ · zoom ค้าง = คำนวณกรอบใหม่ทุกเฟรมเพื่อผลเดิม (I-1)
    named(NamedKey::Tab, Mods::new(Up, Up, Up), A::ToggleMode, Once),
    // ★ `Ctrl+A` — `\u{1}` คือ alias ของ compositor ที่ส่งอักขระควบคุมแทน
    //   (รูปแบบเดียวกับ `Ctrl+Z`/`Ctrl+S`/`Ctrl+G` ข้างบน)
    ch('a', Mods::new(Down, Up, Up), A::SelectAll, Once),
    ch('\u{1}', Mods::new(Down, Up, Up), A::SelectAll, Once),
    // ★★ `Esc` เคร่งครัดกับ modifier: `Ctrl+Esc` เป็นของ Windows (Start menu)
    //    และ `Shift+Esc` ยังไม่มีความหมาย — ปุ่มที่ยังไม่มีความหมายต้องเงียบ
    named(
        NamedKey::Escape,
        Mods::new(Up, Up, Up),
        A::ClearSelection,
        Once,
    ),
    // ★ shift เป็น `Either` แบบเดียวกับปุ่มเครื่องมือ: `Shift+F` มาถึงเป็น `f`
    //   หลัง `to_ascii_lowercase` อยู่แล้ว การบังคับ `Up` จะทำให้มันเงียบโดยไม่มีเหตุ
    ch(
        'f',
        Mods::new(Up, Either, Up),
        A::Zoom(ZoomRequest::FitSelection),
        Once,
    ),
    ch(
        '1',
        Mods::new(Up, Either, Up),
        A::Zoom(ZoomRequest::Actual),
        Once,
    ),
    ch(
        '0',
        Mods::new(Up, Either, Up),
        A::Zoom(ZoomRequest::FitBoard),
        Once,
    ),
];

/// ★★★ สอง binding นี้ถูกจุดด้วยการกดปุ่มครั้งเดียวกันได้ไหม
///
/// **นี่คือคำถามที่ประตูตรวจการชนถาม** และมันไม่ใช่ *"คีย์ซ้ำเป๊ะไหม"* —
/// สองแถวที่เขียนไม่เหมือนกันเลยก็ทับกันได้ถ้าเงื่อนไข modifier ของมันคาบเกี่ยว
/// (`Ctrl+Z` กับ `Ctrl+Z` ที่ shift เป็น [`Hold::Either`]) · ตอนนั้นผลจะขึ้นกับ
/// **ลำดับในตาราง** ซึ่งไม่มีใครตั้งใจและไม่มีใครสังเกตเห็น
///
/// ★★ เจอจริงตอน negative control ของก้อน a: ปลด ctrl ของ `Ctrl+G` เป็น
/// `Either` แล้ว **เทสต์เดิมทุกตัวยังเขียว** เพราะ `'g'` ของ appearance อยู่
/// ก่อนในตารางจึงชนะไปเงียบ ๆ
///
/// ★ `Char` กับ `Key` ชนกันไม่ได้เพราะ [`Keymap::binding`] ถาม `Char` ให้จบก่อน
/// แล้วค่อยตกไป `Key` — ปุ่มเดียวจึงเดินได้ทางเดียวเสมอ
#[must_use]
pub fn conflict(a: &Binding, b: &Binding) -> bool {
    let same_key = match (a.chord, b.chord) {
        (Chord::Char { ch: x, .. }, Chord::Char { ch: y, .. }) => x == y,
        (Chord::Key { key: x, .. }, Chord::Key { key: y, .. }) => x == y,
        _ => false,
    };
    same_key && a.chord.mods().overlaps(b.chord.mods())
}

/// ตารางคีย์ลัดที่ใช้อยู่
///
/// `Cow` เพราะตารางมีสองที่มา: ค่าปริยายที่คอมไพล์มากับโปรแกรม (`Borrowed`)
/// และตารางที่อ่านจาก `keymap.toml` ของผู้ใช้ (`Owned`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: std::borrow::Cow<'static, [Binding]>,
}

/// ตารางค่าปริยายที่คอมไพล์มากับโปรแกรม
static BUILTIN_MAP: Keymap = Keymap {
    bindings: std::borrow::Cow::Borrowed(BUILTIN),
};

/// ★★ ตารางที่ **ใช้อยู่จริง** ตลอดอายุโปรเซส
///
/// เป็น global เพราะ `shortcut_char` ต้องถาม [`Keymap::binds_char`] และมันเป็น
/// ฟังก์ชันอิสระที่ **assertion เดิม 8 เทสต์เรียกตรง ๆ** (ก้อน a) —
/// การร้อยพารามิเตอร์เพิ่มเข้าไปคือการแก้ลายเซ็นซึ่งทำลาย oracle นั้นทิ้ง
///
/// ★ ตั้งได้ครั้งเดียวตอนเปิดโปรแกรม · เทสต์ **ห้ามเรียก [`install`]**
/// (จะรั่วข้ามเทสต์ในโปรเซสเดียวกัน) — เทสต์ที่ต้องการตารางอื่นให้สร้าง
/// [`Keymap`] แล้วเรียกเมธอดของมันตรง ๆ
static ACTIVE: std::sync::OnceLock<Keymap> = std::sync::OnceLock::new();

/// ตารางค่าปริยาย — **ไม่สนใจว่าผู้ใช้ตั้งอะไรไว้** (เทสต์และการเทียบใช้ตัวนี้)
#[must_use]
pub fn builtin() -> &'static Keymap {
    &BUILTIN_MAP
}

/// ตารางที่ใช้อยู่จริง — ค่าปริยายถ้ายังไม่มีใคร [`install`]
#[must_use]
pub fn active() -> &'static Keymap {
    ACTIVE.get().unwrap_or(&BUILTIN_MAP)
}

/// ตั้งตารางที่ผู้ใช้กำหนด — **เรียกได้ครั้งเดียว** คืน `false` ถ้าตั้งไปแล้ว
///
/// เรียกตอนเปิดโปรแกรมก่อนมีหน้าต่างเท่านั้น (คู่กับ `settings.toml`)
pub fn install(map: Keymap) -> bool {
    ACTIVE.set(map).is_ok()
}

/// เหตุที่ `keymap.toml` ใช้ไม่ได้ — ★ **ทุกตัวชี้แถวที่ผิด** ไม่ใช่แค่ว่าผิด
///
/// ★★ พังข้อเดียวก็ **ใช้ค่าปริยายทั้งชุด** (`ROADMAP` P5-3b ก้อน b) —
/// การใช้ครึ่งเดียวทำให้ผู้ใช้เจอคีย์ลัดบางตัวหายโดยไม่มีอะไรบอกว่าทำไม
/// ซึ่งอ่านได้อย่างเดียวว่าโปรแกรมทำงานหาย
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// อ่านไฟล์/ไวยากรณ์/เพดานไม่ผ่าน — ข้อความมาจาก `refx-io` (อังกฤษ สำหรับ log)
    File {
        /// สิ่งที่ชั้นอ่านไฟล์บ่น
        detail: String,
    },
    /// ปุ่มที่เขียนไว้อ่านไม่ออก
    UnknownKey {
        /// แถวที่ผิด (เริ่มที่ 1)
        row: usize,
        /// สิ่งที่เขาเขียน
        given: String,
    },
    /// ชื่อ action ที่ไม่รู้จัก
    UnknownAction {
        /// แถวที่ผิด (เริ่มที่ 1)
        row: usize,
        /// สิ่งที่เขาเขียน
        given: String,
    },
    /// ★★★ สองแถวถูกจุดด้วยการกดครั้งเดียวกันได้ — **ไม่จำเป็นต้องเขียนเหมือนกัน**
    Conflict {
        /// แถวหลัง
        row: usize,
        /// ปุ่มของแถวหลัง
        keys: String,
        /// แถวแรกที่มันไปชน
        other_row: usize,
        /// ปุ่มของแถวแรก
        other_keys: String,
    },
}

impl Keymap {
    /// ★★★ แปลงแถวดิบจาก `keymap.toml` เป็นตารางจริง พร้อม **ตรวจการชน**
    ///
    /// ## ไฟล์ **แทนที่ตารางทั้งชุด** ไม่ใช่ทับทีละปุ่ม
    ///
    /// เพราะกฎ "พังที่ไหนก็ใช้ค่าปริยายทั้งไฟล์" จะไม่มีความหมายเลยถ้าไฟล์เป็น
    /// แค่ส่วนเสริม — และการทับทีละปุ่มเปิดคำถามที่ยังไม่มีคำตอบ (จะ *ลบ*
    /// ปุ่มค่าปริยายยังไง · ปุ่มที่ผู้ใช้ตั้งชนกับค่าปริยายนับเป็นการชนไหม)
    /// ★ แผง Settings จึงต้องบอกจำนวนปุ่มที่ไฟล์กำหนด ให้เห็นทันทีว่าแทนที่ไปแล้ว
    ///
    /// # Errors
    /// [`Problem`] — **ตัวแรกที่เจอ** · ไฟล์ถูกปฏิเสธทั้งชุดอยู่แล้ว เหตุผลเดียว
    /// จึงพอ และการไล่รายงานทุกข้อพร้อมกันทำให้ผู้ใช้ไม่รู้ว่าจะแก้ตัวไหนก่อน
    pub fn from_rows(rows: &[refx_io::keymap::RawBind]) -> Result<Self, Problem> {
        let mut bindings: Vec<Binding> = Vec::with_capacity(rows.len());
        let mut specs: Vec<(usize, String)> = Vec::with_capacity(rows.len());

        for row in rows {
            let Some(chord) = Chord::parse(&row.keys) else {
                return Err(Problem::UnknownKey {
                    row: row.row,
                    given: row.keys.clone(),
                });
            };
            let Some(action) = Action::from_name(&row.action) else {
                return Err(Problem::UnknownAction {
                    row: row.row,
                    given: row.action.clone(),
                });
            };
            let binding = Binding {
                chord,
                action,
                repeat: if row.repeat { Allow } else { Once },
            };
            // ★★ ตรวจ **ตอนเพิ่ม** ไม่ใช่ตอนจบ — จะได้ชี้ได้ว่าแถวไหนไปชนแถวไหน
            //    ซึ่งเป็นสิ่งเดียวที่ผู้ใช้เอาไปแก้ได้จริง
            if let Some((at, other)) = bindings
                .iter()
                .zip(&specs)
                .find(|(existing, _)| conflict(existing, &binding))
                .map(|(_, (at, spec))| (*at, spec.clone()))
            {
                return Err(Problem::Conflict {
                    row: row.row,
                    keys: row.keys.clone(),
                    other_row: at,
                    other_keys: other,
                });
            }
            bindings.push(binding);
            specs.push((row.row, row.keys.clone()));
        }

        Ok(Self {
            bindings: std::borrow::Cow::Owned(bindings),
        })
    }
}

impl Keymap {
    /// binding ทั้งหมด
    #[must_use]
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// ★★★ ปุ่มที่เพิ่งกด แปลว่าอะไร — **`Char` ก่อน `Key` เป็นตาข่ายรอง**
    ///
    /// `pressed` = ผลของ `shortcut_char` (logical → physical ถูกยุบมาแล้ว) ·
    /// `key` = ปุ่มที่มีชื่อ ถ้าปุ่มนั้นไม่ใช่ตัวอักษร
    ///
    /// ★ ลำดับนี้คือสิ่งที่ `docs/03 §5` เขียนว่า **ห้ามสลับ**
    #[must_use]
    pub fn action(
        &self,
        pressed: Option<char>,
        key: Option<NamedKey>,
        state: ModifiersState,
    ) -> Option<Action> {
        self.binding(pressed, key, state).map(|found| found.action)
    }

    /// เหมือน [`Self::action`] แต่คืนทั้งใบ — ผู้เรียกที่ต้องรู้ `repeat` ด้วย
    #[must_use]
    pub fn binding(
        &self,
        pressed: Option<char>,
        key: Option<NamedKey>,
        state: ModifiersState,
    ) -> Option<&Binding> {
        if let Some(pressed) = pressed
            && let Some(found) = self.bindings.iter().find(|binding| {
                matches!(binding.chord, Chord::Char { ch, mods }
                    if ch == pressed && mods.matches(state))
            })
        {
            return Some(found);
        }
        let key = key?;
        self.bindings.iter().find(|binding| {
            matches!(binding.chord, Chord::Key { key: wanted, mods }
                if wanted == key && mods.matches(state))
        })
    }

    /// ★★★ **อักขระนี้ผูกกับอะไรสักอย่างในตารางไหม** (ไม่สนใจ modifier)
    ///
    /// เป็นคำถามที่ `shortcut_char` ใช้ตัดสินว่าจะรับอักขระที่ layout ผลิต
    /// หรือจะตกไปชั้น physical (`docs/03 §5` — แก้ 4 ก.ย. 2026) ·
    /// **ก่อนมีตารางนี้ ถามแบบนี้ไม่ได้** จึงต้องเดาจากหน้าตาของอักขระแทน
    /// แล้ว `Ctrl+Shift+Z` ก็ตายบน layout ไทยอยู่หลายเฟส
    ///
    /// ★★ **ไม่สนใจ modifier โดยตั้งใจ** — ถ้าถามแบบตรงทั้งชุด (`ch` + `mods`)
    /// การกดตัวอักษรเปล่า ๆ บน layout ที่ไม่ใช่ QWERTY จะตกไปจุดเครื่องมือที่
    /// ตำแหน่ง physical นั้นแทน ซึ่งคือ *"ถาม physical ก่อนแล้ว Dvorak พัง"*
    /// (`HANDOFF §2.12`) กลับมาในรูปที่ช้าลงหนึ่งจังหวะ
    ///
    /// คำถามที่ถูกคือ **"ตัวอักษรนี้เป็นภาษาของคีย์ลัดเราไหม"** — ถ้าใช่
    /// เจ้าของคือ layout ของผู้ใช้ · ถ้าไม่ใช่เลย มันไม่ใช่เจตนาของเขาแน่
    #[must_use]
    pub fn binds_char(&self, ch: char) -> bool {
        self.bindings
            .iter()
            .any(|binding| matches!(binding.chord, Chord::Char { ch: bound, .. } if bound == ch))
    }

    /// กดค้างแล้วสั่งซ้ำได้ไหม — `false` ถ้า action นี้ไม่มีในตาราง
    ///
    /// ★★ อ่านจาก **ตาราง** ไม่ใช่จาก `match` ที่เขียนซ้ำใน `on_input` —
    /// ไม่งั้น `RepeatPolicy` จะเป็นข้อมูลที่ไม่มีใครใช้ ซึ่ง `docs/08 §3.9`
    /// ข้อ 2 เรียกว่า "โครงเปล่า" และห้ามไว้ตรง ๆ
    #[must_use]
    pub fn repeats(&self, action: Action) -> bool {
        self.bindings
            .iter()
            .find(|binding| binding.action == action)
            .is_some_and(|binding| binding.repeat == RepeatPolicy::Allow)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// ★★ เรียก [`conflict`] **ตัวจริงที่ production ใช้** ไม่ใช่สำเนาในเทสต์
    ///
    /// รุ่นแรกของเทสต์ชุดนี้เขียนตรรกะซ้ำไว้เอง ซึ่งแปลว่ามันพิสูจน์ได้แค่ว่า
    /// *สำเนาในเทสต์พอใจ* ไม่ใช่ว่าประตูของจริงพอใจ — รูปแบบเดียวกับที่
    /// `docs/08 §3.9` ข้อ 9 ห้ามไว้ (ยืนยันประตูด้วยสิ่งที่เขียนเลียนแบบประตู)
    fn can_both_fire(a: &Binding, b: &Binding) -> bool {
        conflict(a, b)
    }

    /// ★★★ **ไม่มีปุ่มไหนสั่งสองอย่างพร้อมกัน** — ทั้งตาราง
    ///
    /// เคยเป็นบั๊กจริงคนละรูป: `G` กับ `Ctrl+G` อยู่คนละชั้นกันสิ้นเชิง (การมองเห็น
    /// vs เอกสาร) ถ้าตัวใดตัวหนึ่งไม่บังคับ modifier ของตัวเอง การกดปุ่มเดียว
    /// จะทำทั้งสองอย่าง แล้วผู้ใช้ที่ตั้งใจเช็ค value จะได้กลุ่มใหม่แถมมา
    ///
    /// ★ เทสต์เดิม (`grayscale_and_grouping_never_fire_on_the_same_keypress`)
    /// ถามคู่เดียว · ตัวนี้ถาม **ทุกคู่ในตาราง** ซึ่งเป็นสิ่งที่ทำได้ก็ต่อเมื่อ
    /// การจับคู่เป็นข้อมูล · และเป็นฐานของ "keymap ที่ชนกันเองต้องบอก" ในก้อน b
    #[test]
    fn no_single_keypress_can_ever_fire_two_actions() {
        let table = builtin().bindings();
        for (i, a) in table.iter().enumerate() {
            for b in &table[i + 1..] {
                assert!(!can_both_fire(a, b), "ปุ่มเดียวสั่งสองอย่าง:\n  {a:?}\n  {b:?}");
            }
        }
    }

    /// ★ ประตูของประตู: ตัวตรวจการชนต้องจับของที่ชนกันจริงได้
    ///
    /// ถ้า `can_both_fire` ตอบ `false` กับทุกอย่าง เทสต์ข้างบนจะเขียวตลอดกาล
    /// โดยไม่ได้ตรวจอะไรเลย (`docs/08 §3.9` ข้อ 1)
    #[test]
    fn the_conflict_check_can_actually_see_a_conflict() {
        let undo = ch(
            'z',
            Mods::new(Down, Up, Either),
            A::History(HistoryRequest::Undo),
            Allow,
        );
        // ปุ่มเดียวกัน · shift ไม่สนทั้งคู่ → กด Ctrl+Z ทีเดียวติดทั้งสองใบ
        let clash = ch('z', Mods::new(Down, Either, Either), A::Paste, Once);
        assert!(can_both_fire(&undo, &clash));

        // ต่างกันที่ shift แบบบังคับคนละทาง → ไม่มีทางติดพร้อมกัน
        let redo = ch(
            'z',
            Mods::new(Down, Down, Either),
            A::History(HistoryRequest::Redo),
            Allow,
        );
        assert!(!can_both_fire(&undo, &redo));

        // คนละปุ่ม
        let paste = ch('v', Mods::new(Down, Either, Either), A::Paste, Once);
        assert!(!can_both_fire(&undo, &paste));
    }

    /// ★★ ทุก binding ของ action เดียวกันต้องมีนโยบาย `repeat` เหมือนกัน
    ///
    /// [`Keymap::repeats`] หาใบแรกที่เจอ · ถ้าสองใบของ `Ctrl+Z` ตอบไม่ตรงกัน
    /// พฤติกรรมของการกดค้างจะขึ้นกับ **ลำดับในตาราง** ซึ่งเป็นสิ่งที่ไม่มีใคร
    /// ตั้งใจและไม่มีใครสังเกตเห็นจนกว่าจะมีคนสลับสองบรรทัด
    #[test]
    fn every_binding_of_one_action_agrees_on_holding_the_key_down() {
        let table = builtin().bindings();
        for a in table {
            for b in table {
                assert!(
                    a.action != b.action || a.repeat == b.repeat,
                    "action เดียวกันแต่คนละนโยบายกดค้าง:\n  {a:?}\n  {b:?}"
                );
            }
        }
    }

    /// ★ นโยบายกดค้างต้องตรงกับที่ `on_input` เคยทำ — ตัวเลือกที่ตั้งใจทั้งคู่
    #[test]
    fn holding_a_key_repeats_only_where_it_should() {
        let map = builtin();
        // ย้อนเรื่อย ๆ / เลื่อนชั้นรัว ๆ / สลับเครื่องมือ = ท่าปกติ
        assert!(map.repeats(A::History(HistoryRequest::Undo)));
        assert!(map.repeats(A::History(HistoryRequest::Redo)));
        assert!(map.repeats(A::ZOrder(ZMove::Forward)));
        assert!(map.repeats(A::Tool(Tool::Crop)));
        // วาง/ลบ/บันทึก/เปิด/แท็บ/กลุ่ม = กดค้างแล้วเป็นหายนะ
        for action in [
            A::Paste,
            A::Delete,
            A::Save(SaveRequest::Save),
            A::OpenBoard,
            A::Tab(TabKey::New),
            A::Group(GroupRequest::Group),
            A::Appearance(AppearanceKey::FlipHorizontal),
        ] {
            assert!(!map.repeats(action), "{action:?} ไม่ควรซ้ำตอนกดค้าง");
        }
    }

    /// ★★★ **ช่องว่างระหว่าง spec กับของจริงปิดแล้ว** — 17 คีย์ใน `docs/03 §5`
    ///
    /// ## ประวัติของเทสต์ตัวนี้
    ///
    /// ก้อน a ใช้มันบังคับว่า **ห้ามมีคีย์ใหม่โผล่มา** เพราะการพิสูจน์ว่าตาราง
    /// ให้ผลเท่าของเดิมเป๊ะทำได้ก็ต่อเมื่อไม่มีอะไรงอกระหว่างทาง · ตัวมันเอง
    /// เขียนไว้ว่า *"ตัวเลขนี้จะเปลี่ยนตอนก้อน c เท่านั้น และตอนนั้นต้องมีคนมา
    /// แก้ตัวเลขพร้อมกับอ่านเหตุผลนี้"* — ก้อน c คือตอนนี้
    ///
    /// ★ มันจึงกลับด้าน: จาก "หกตัวนี้ต้อง **ไม่** มี" เป็น "หกตัวนี้ต้อง **มี**
    /// และผูกกับ action ที่ถูกต้อง" · ตัวเลขยังคุมการงอกโดยไม่ตั้งใจเหมือนเดิม
    #[test]
    fn the_table_holds_exactly_what_the_old_functions_held() {
        let table = builtin().bindings();
        // 6 ประวัติ · 2 วาง · 2 ลบ · 6 ย้ายชั้น · 5 เครื่องมือ · 2 การแสดงผล ·
        // 4 กลุ่ม · 4 บันทึก · 2 เปิด · 5 แท็บ  = 38 (ก้อน a)
        // + ก้อน c: Tab · Ctrl+A (+alias) · Esc · F · 1 · 0 = 7
        assert_eq!(table.len(), 47, "จำนวน binding เปลี่ยน — เพิ่มคีย์ต้องมาแก้ที่นี่ด้วย");

        // ★★ หกคีย์ที่ `docs/03 §5` สั่งไว้ตั้งแต่วันแรก **และไม่เคยมีจนถึงก้อน c**
        //    ตรวจว่ามันเดินผ่านเส้นทางเดียวกับที่ผู้ใช้กดจริง ไม่ใช่แค่มีอยู่ในตาราง
        let map = builtin();
        let none = ModifiersState::empty();
        assert_eq!(
            map.action(None, Some(NamedKey::Tab), none),
            Some(A::ToggleMode),
            "`Tab` เปล่า ๆ ต้องสลับโหมด"
        );
        assert_eq!(
            map.action(Some('a'), None, ModifiersState::CONTROL),
            Some(A::SelectAll)
        );
        assert_eq!(
            map.action(None, Some(NamedKey::Escape), none),
            Some(A::ClearSelection)
        );
        assert_eq!(
            map.action(Some('f'), None, none),
            Some(A::Zoom(ZoomRequest::FitSelection))
        );
        assert_eq!(
            map.action(Some('1'), None, none),
            Some(A::Zoom(ZoomRequest::Actual))
        );
        assert_eq!(
            map.action(Some('0'), None, none),
            Some(A::Zoom(ZoomRequest::FitBoard))
        );

        // ★★★ `Tab` เปล่า ๆ กับ `Ctrl+Tab` **ต้องไม่ปนกัน** — ปุ่มเดียวกันเป๊ะ
        //     ต่างกันแค่ ctrl · ประตู `no_single_keypress_can_ever_fire_two_actions`
        //     พิสูจน์ว่าไม่ทับกัน แต่ตรงนี้พิสูจน์ว่าแต่ละอันไป **ที่ถูก**
        assert_eq!(
            map.action(None, Some(NamedKey::Tab), ModifiersState::CONTROL),
            Some(A::Tab(TabKey::Next))
        );
    }

    /// ★★★ **หกคีย์ใหม่ต้องรอดบน layout ที่ไม่ใช่ QWERTY** — กลุ่มเสี่ยงที่สุด
    ///
    /// `F` `1` `0` เป็น binding **ไม่มี modifier** ซึ่งคือรูปที่บั๊ก layout ไทย
    /// (`§2.40ก`) เกิดขึ้นพอดี · เทสต์นี้อยู่ที่ระดับตารางจึงตอบได้แค่ครึ่งเดียว
    /// — ครึ่งที่เหลือ (`shortcut_char` ต้องแปลงปุ่มไทยให้ถูกก่อนถึงตาราง) อยู่ที่
    /// `app::tests::the_six_new_keys_survive_a_thai_and_a_dvorak_layout`
    ///
    /// ★ ที่นี่ตอบข้อเดียวแต่สำคัญ: **อักขระที่ตารางต้องการต้องเป็น ASCII ล้วน**
    /// ถ้ามีตัวไหนไม่ใช่ `shortcut_char` จะผลิตมันไม่ได้เลย (`logical_ascii`
    /// รับเฉพาะ ASCII · `physical_char` คืน ASCII เท่านั้น) แล้ว binding นั้น
    /// จะเป็นปุ่มที่ไม่มีวันถูกกด
    #[test]
    fn every_character_the_table_waits_for_can_actually_be_produced() {
        for binding in builtin().bindings() {
            if let Chord::Char { ch, .. } = binding.chord {
                assert!(
                    ch.is_ascii(),
                    "{ch:?} ไม่ใช่ ASCII — `shortcut_char` ผลิตมันไม่ได้ ปุ่มนี้จะตายเงียบ"
                );
            }
        }
    }

    // ---------- ก้อน b: keymap.toml ----------

    fn row(at: usize, keys: &str, action: &str) -> refx_io::keymap::RawBind {
        refx_io::keymap::RawBind {
            row: at,
            keys: keys.to_owned(),
            action: action.to_owned(),
            repeat: false,
        }
    }

    /// ★★★ **ประตูตรวจการชนต้องจับแถวที่ *ทับกัน* ไม่ใช่แค่แถวที่ *ซ้ำเป๊ะ***
    ///
    /// นี่คือรูปที่ negative control ของก้อน a เผยให้เห็น: `'g'` ที่ ctrl เป็น
    /// `Either` ไม่ได้ "ซ้ำ" กับ `'g'` ที่ ctrl เป็น `Up` เลยสักตัวอักษร
    /// แต่กด `G` เปล่า ๆ ทีเดียวติดทั้งคู่ แล้วผลขึ้นกับลำดับในตาราง
    ///
    /// ★ ไฟล์ของผู้ใช้เขียน `Either` ไม่ได้ (ดู [`Chord::parse`]) การชนจากไฟล์
    /// จึงมาในรูป **modifier ซ้อนกัน** เช่น `z` กับ `z` · หรือ `ctrl+z` สองแถว
    /// · เทสต์นี้ยิงทั้งรูปที่เขียนเหมือนกันและรูปที่เขียนไม่เหมือนกัน
    #[test]
    fn two_rows_that_one_keypress_can_both_trigger_are_reported_by_row() {
        // เขียนเหมือนกันเป๊ะ
        let same = [row(1, "ctrl+z", "undo"), row(2, "ctrl+z", "redo")];
        assert_eq!(
            Keymap::from_rows(&same),
            Err(Problem::Conflict {
                row: 2,
                keys: "ctrl+z".to_owned(),
                other_row: 1,
                other_keys: "ctrl+z".to_owned(),
            })
        );

        // ★★ เขียนไม่เหมือนกันเลย แต่ทับกัน — `CTRL+Z` กับ `control+z`
        //    ตัวจับคู่แบบ "สตริงซ้ำ" มองไม่เห็นคู่นี้
        let spelled = [row(1, "CTRL+Z", "undo"), row(2, "control+z", "paste")];
        assert!(matches!(
            Keymap::from_rows(&spelled),
            Err(Problem::Conflict {
                row: 2,
                other_row: 1,
                ..
            })
        ));

        // ★ ต่างกันที่ shift แบบบังคับคนละทาง = ไม่มีทางติดพร้อมกัน → ต้องผ่าน
        let fine = [
            row(1, "ctrl+z", "undo"),
            row(2, "ctrl+shift+z", "redo"),
            row(3, "z", "tool-select"),
        ];
        assert_eq!(Keymap::from_rows(&fine).unwrap().bindings().len(), 3);
    }

    /// ★★★ **ขอบเขตที่การชนจากไฟล์ไปไม่ถึง** — บันทึกไว้ ไม่ใช่ปล่อยให้คนเดา
    ///
    /// `docs/08 §3.9` ข้อ 1b: ก่อนเชื่อว่าเทสต์คุมกิ่งไหน ต้องรู้ว่า input ของมัน
    /// **ไปถึงกิ่งนั้นได้** · [`Chord::parse`] ไม่มีทางผลิต [`Hold::Either`]
    /// เลย (ดูเหตุผลที่นั่น) → การชนที่มาจาก `keymap.toml` จึงเป็น
    /// **chord ที่เท่ากันเป๊ะ** เสมอ และกิ่งที่น่าสนใจของ[`Mods::overlaps`]
    /// (`Either` คาบกับ `Down`/`Up`) **ไม่มีทางถูกยิงจากไฟล์**
    ///
    /// กิ่งนั้นถูกคุมที่ระดับ [`Binding`] แทน โดย
    /// `the_conflict_check_can_actually_see_a_conflict` และ
    /// `no_single_keypress_can_ever_fire_two_actions` ซึ่งยิงตารางค่าปริยาย
    /// ที่ **มี `Either` อยู่จริง** — ถ้าวันหนึ่งไฟล์เขียน `Either` ได้
    /// เทสต์นี้จะแดงแล้วคนแก้จะรู้ว่าต้องไปเติมเคสที่ระดับไฟล์ด้วย
    #[test]
    fn a_file_can_never_write_the_modifier_state_that_makes_overlap_interesting() {
        for spec in [
            "z",
            "ctrl+z",
            "shift+z",
            "alt+z",
            "ctrl+shift+z",
            "ctrl+shift+alt+z",
            "delete",
            "ctrl+tab",
            "[",
            "{",
        ] {
            let mods = Chord::parse(spec).unwrap().mods();
            for (hold, which) in [
                (mods.ctrl, "ctrl"),
                (mods.shift, "shift"),
                (mods.alt, "alt"),
            ] {
                assert_ne!(
                    hold, Either,
                    "{spec:?} ผลิต Either ที่ {which} — ไฟล์เขียน Either ได้แล้ว \
                     ต้องเพิ่มเคสการชนแบบคาบเกี่ยวที่ระดับไฟล์"
                );
            }
        }
        // ★ และตารางค่าปริยาย **มี** `Either` อยู่จริง — ถ้าไม่มี กิ่งนั้นก็ไม่มีใครยิง
        assert!(
            builtin()
                .bindings()
                .iter()
                .any(|b| b.chord.mods().shift == Either),
            "ตารางค่าปริยายไม่มี Either แล้ว — ทบทวนว่ากิ่ง overlaps ยังมีคนยิงไหม"
        );
    }

    /// ★ ปุ่มที่อ่านไม่ออก และ action ที่ไม่รู้จัก ต้องบอก **แถว** และ **สิ่งที่เขาเขียน**
    #[test]
    fn a_row_that_cannot_be_understood_names_itself_and_what_was_written() {
        assert_eq!(
            Keymap::from_rows(&[row(1, "ctrl+z", "undo"), row(2, "ctrl+นก", "redo")]),
            Err(Problem::UnknownKey {
                row: 2,
                given: "ctrl+นก".to_owned(),
            })
        );
        assert_eq!(
            Keymap::from_rows(&[row(4, "ctrl+z", "unbdo")]),
            Err(Problem::UnknownAction {
                row: 4,
                given: "unbdo".to_owned(),
            })
        );
        // ★★ อักขระที่ไม่ใช่ ASCII ตัวเดียวก็รับไม่ได้ — `shortcut_char` ไม่มีทาง
        //    ผลิตมันออกมา binding นั้นจึงไม่มีวันถูกจุด = หลอกผู้ใช้ว่าตั้งสำเร็จ
        assert!(matches!(
            Keymap::from_rows(&[row(1, "ผ", "undo")]),
            Err(Problem::UnknownKey { row: 1, .. })
        ));
    }

    /// ★★ ชื่อ action ต้อง round-trip ได้ทุกตัว — มันคือสัญญากับไฟล์ของผู้ใช้
    ///
    /// ชื่อซ้ำกันสองตัวจะทำให้ `from_name` คืนตัวแรกเสมอ แล้ว action อีกตัว
    /// **ผูกจากไฟล์ไม่ได้เลยตลอดกาล** โดยไม่มีอะไรบ่น
    #[test]
    fn every_action_name_round_trips_and_none_of_them_collide() {
        for action in Action::ALL {
            assert_eq!(
                Action::from_name(action.name()),
                Some(*action),
                "{} ไม่ round-trip",
                action.name()
            );
        }
        let mut names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "มีชื่อ action ซ้ำกัน");
        assert_eq!(before, 30, "จำนวน action เปลี่ยน — เพิ่ม action ต้องมาแก้ที่นี่ด้วย");

        // ★ ทุก action ในตารางค่าปริยายต้องเขียนลงไฟล์ได้ ไม่งั้นผู้ใช้ทำ
        //   keymap.toml ที่ได้พฤติกรรมเท่าค่าปริยายไม่ได้เลย
        for binding in builtin().bindings() {
            assert!(Action::from_name(binding.action.name()).is_some());
        }
    }

    /// ★★★ ไฟล์ของผู้ใช้ **แทนที่ตารางทั้งชุด** และ `Either` ไม่หลุดเข้ามา
    #[test]
    fn what_the_user_writes_is_exactly_what_they_get() {
        let map = Keymap::from_rows(&[row(1, "ctrl+z", "undo")]).unwrap();
        assert_eq!(map.bindings().len(), 1, "ไฟล์แทนที่ทั้งชุด ไม่ใช่ส่วนเสริม");

        let ctrl = ModifiersState::CONTROL;
        assert_eq!(
            map.action(Some('z'), None, ctrl),
            Some(A::History(HistoryRequest::Undo))
        );
        // ★ เขียน `ctrl+z` แปลว่า ctrl กด · shift ไม่กด · alt ไม่กด **เป๊ะ ๆ**
        assert_eq!(
            map.action(Some('z'), None, ctrl | ModifiersState::SHIFT),
            None,
            "modifier จากไฟล์ต้องตรงตัว ไม่ใช่ Either"
        );
        assert_eq!(
            map.action(Some('z'), None, ctrl | ModifiersState::ALT),
            None
        );
        // ปุ่มที่ไฟล์ไม่ได้พูดถึงต้องเงียบ — ไม่ใช่ตกกลับไปค่าปริยาย
        assert_eq!(map.action(Some('v'), None, ctrl), None);
    }

    /// ★ ปุ่มที่มีชื่อ และปุ่ม `+` เอง — สองรูปที่ตัวแยก modifier พลาดได้ง่ายที่สุด
    #[test]
    fn the_chord_parser_handles_the_shapes_that_look_ambiguous() {
        assert_eq!(
            Chord::parse("delete"),
            Some(Chord::Key {
                key: NamedKey::Delete,
                mods: Mods::new(Up, Up, Up),
            })
        );
        assert_eq!(
            Chord::parse("ctrl+tab"),
            Some(Chord::Key {
                key: NamedKey::Tab,
                mods: Mods::new(Down, Up, Up),
            })
        );
        // ★ `ctrl++` = Ctrl กับปุ่ม `+` ไม่ใช่ token ว่าง
        assert_eq!(
            Chord::parse("ctrl++"),
            Some(Chord::Char {
                ch: '+',
                mods: Mods::new(Down, Up, Up),
            })
        );
        assert_eq!(
            Chord::parse(" Shift+[ "),
            Some(Chord::Char {
                ch: '[',
                mods: Mods::new(Up, Down, Up),
            })
        );
        // อ่านไม่ออก
        assert_eq!(Chord::parse(""), None);
        assert_eq!(Chord::parse("ctrl+"), None);
        assert_eq!(Chord::parse("f13"), None, "ปุ่มที่ยังไม่รองรับต้องบอกว่าไม่รู้จัก");
    }

    /// ★★★ **อักขระควบคุมห้ามขึ้นแผง** — มันไม่มี glyph แล้วจะเป็นสี่เหลี่ยม tofu
    ///
    /// บทเรียนนี้โปรเจกต์นี้เจอมาแล้วสองครั้ง (`shell::UNSAVED_MARK` และจุดเตือน
    /// ของ P5-3a) · ตารางค่าปริยายมี alias อย่าง `Ctrl+Z` = `\u{1a}` อยู่ **12 ใบ**
    /// — อักขระควบคุม 9 ตัวที่ต่างกัน แต่สามตัว (`Ctrl+Z`/`Ctrl+G`/`Ctrl+S`)
    /// มีสองใบเพราะแยกตามธง shift · ตัวที่เก้าคือ `Ctrl+A` (ก้อน c)
    #[test]
    fn control_character_aliases_never_reach_the_panel() {
        let hidden = builtin()
            .bindings()
            .iter()
            .filter(|b| b.chord.display().is_none())
            .count();
        assert_eq!(hidden, 13, "จำนวน alias อักขระควบคุมเปลี่ยนไป");

        for binding in builtin().bindings() {
            if let Some(shown) = binding.chord.display() {
                assert!(
                    !shown.chars().any(char::is_control),
                    "{shown:?} มีอักขระควบคุมหลุดขึ้นแผง"
                );
            }
        }
        // ★ สิ่งที่แผงแสดงต้องเป็นสิ่งที่ผู้ใช้ก๊อปไปเขียนในไฟล์ได้จริง
        for binding in builtin().bindings() {
            if let Some(shown) = binding.chord.display() {
                assert!(
                    Chord::parse(&shown).is_some(),
                    "{shown:?} แสดงได้แต่เขียนกลับลงไฟล์ไม่ได้"
                );
            }
        }
    }

    /// ★ `Char` ต้องถูกถามก่อน `Key` เสมอ (`docs/03 §5` — ห้ามสลับ)
    ///
    /// ★★ สร้างสถานะที่ **แยกสองลำดับออกจากกันได้จริง**: ปุ่มที่รายงานทั้ง
    /// อักขระและชื่อพร้อมกัน · ของจริงเกิดยากมาก แต่ถ้าวันหนึ่งมีใครสลับลำดับ
    /// เทสต์นี้คือตัวเดียวที่บอกได้ — เทียบกับการอ่านโค้ดแล้วเชื่อคอมเมนต์
    #[test]
    fn the_character_net_is_asked_before_the_named_net() {
        let map = builtin();
        let ctrl = ModifiersState::CONTROL;
        // ปุ่มที่ให้ทั้ง `t` (อักขระ) และ `Tab` (ชื่อ) — `Char` ต้องชนะ
        assert_eq!(
            map.action(Some('t'), Some(NamedKey::Tab), ctrl),
            Some(A::Tab(TabKey::New)),
            "ตาข่ายชั้นนอกดักไปก่อน — layout ที่ไม่ใช่ QWERTY จะเสียเจตนาของผู้ใช้"
        );
        // ไม่มีอักขระเลย → ตกไปที่ตาข่ายรองตามที่ตั้งใจ
        assert_eq!(
            map.action(None, Some(NamedKey::Tab), ctrl),
            Some(A::Tab(TabKey::Next))
        );
        // อักขระที่ไม่มีในตาราง → ยังต้องตกไปที่ตาข่ายรอง ไม่ใช่จบที่ None
        assert_eq!(
            map.action(Some('q'), Some(NamedKey::Delete), ModifiersState::empty()),
            Some(A::Delete),
            "อักขระที่ไม่ตรงต้องไม่กลืนปุ่มที่มีชื่อไปด้วย"
        );
    }
}
