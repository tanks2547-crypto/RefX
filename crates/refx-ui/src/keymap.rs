//! ★★★ ตารางคีย์ลัด — **ที่เดียวที่ตัดสินว่าปุ่มไหนแปลว่าอะไร** (P5-3b ก้อน a)
//!
//! spec: `docs/03-modes-and-ui.md §5` · `ROADMAP` P5-3b
//!
//! ## ก้อน a ทำอะไร และ **ไม่** ทำอะไร
//!
//! ทำ: ย้ายการจับคู่ทั้งหมดมาเป็น **ข้อมูล** แล้วให้ฟังก์ชันเดิมใน `app.rs`
//! กลายเป็น wrapper บาง ๆ ที่อ่านตารางนี้
//!
//! ไม่ทำ: **ไม่อ่าน `keymap.toml`** · **ไม่เพิ่มคีย์ใหม่สักตัว** ·
//! ไม่แตะเทสต์เดิมสักบรรทัด
//!
//! ★★ เหตุผลที่ `ROADMAP` แยกก้อนไว้แบบนี้: **assertion เดิมคือ oracle**
//! ที่พิสูจน์ว่าตารางให้ผลเท่าของเดิมเป๊ะ · การเพิ่มคีย์ใหม่ระหว่างทางทำให้
//! พิสูจน์ความเท่ากันไม่ได้อีกต่อไป เพราะไม่มีของเดิมให้เทียบ
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
/// ★ ไม่มี variant ไหนที่ยังไม่มีคนทำ · การเพิ่ม action ใหม่คือก้อน c ของ P5-3b
/// (`Tab` · `Ctrl+A` · `Esc` · `F` · `1` · `0`) — ก้อน a **ห้ามเพิ่ม**
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

impl Chord {
    /// เงื่อนไขปุ่มค้างของ chord นี้ — ใช้ตรวจการชน (ดู [`Mods::overlaps`])
    #[must_use]
    pub const fn mods(self) -> Mods {
        match self {
            Self::Char { mods, .. } | Self::Key { mods, .. } => mods,
        }
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
];

/// ตารางคีย์ลัดที่ใช้อยู่
#[derive(Debug, Clone, Copy)]
pub struct Keymap {
    bindings: &'static [Binding],
}

/// ตารางค่าปริยาย — **ยังไม่มีทางให้ผู้ใช้ทับ** (นั่นคือก้อน b)
#[must_use]
pub const fn builtin() -> Keymap {
    Keymap { bindings: BUILTIN }
}

impl Keymap {
    /// binding ทั้งหมด (เทสต์และก้อน b ใช้)
    #[must_use]
    pub const fn bindings(&self) -> &'static [Binding] {
        self.bindings
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
    ) -> Option<&'static Binding> {
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

    /// สอง binding นี้ถูกจุดด้วยการกดปุ่มครั้งเดียวกันได้ไหม
    fn can_both_fire(a: &Binding, b: &Binding) -> bool {
        let same_key = match (a.chord, b.chord) {
            (Chord::Char { ch: x, .. }, Chord::Char { ch: y, .. }) => x == y,
            (Chord::Key { key: x, .. }, Chord::Key { key: y, .. }) => x == y,
            // ★ `Char` กับ `Key` ชนกันไม่ได้เพราะ `binding()` ถาม `Char` ให้จบก่อน
            //   แล้วค่อยตกไป `Key` — ปุ่มเดียวจึงเดินได้ทางเดียวเสมอ
            _ => false,
        };
        same_key && a.chord.mods().overlaps(b.chord.mods())
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

    /// ★★ **ก้อน a ห้ามเพิ่มคีย์ใหม่** — ประตูที่บังคับข้อนั้น
    ///
    /// `ROADMAP` P5-3b เขียนว่าก้อน a พิสูจน์ความเท่ากันกับของเดิมได้ก็ต่อเมื่อ
    /// ไม่มีคีย์ใหม่โผล่มาระหว่างทาง · ตัวเลขนี้จะเปลี่ยนตอนก้อน c เท่านั้น
    /// และตอนนั้นต้องมีคนมาแก้ตัวเลขพร้อมกับอ่านเหตุผลนี้
    #[test]
    fn the_table_holds_exactly_what_the_old_functions_held() {
        let table = builtin().bindings();
        // 6 ประวัติ · 2 วาง · 2 ลบ · 6 ย้ายชั้น · 5 เครื่องมือ · 2 การแสดงผล ·
        // 4 กลุ่ม · 4 บันทึก · 2 เปิด · 5 แท็บ
        assert_eq!(table.len(), 38, "จำนวน binding เปลี่ยน — ก้อน a ห้ามเพิ่มคีย์ใหม่");

        // ★ 6 คีย์ที่ `docs/03 §5` สั่งไว้แต่ **ยังไม่เคยมี** ต้องยังไม่มีในก้อน a
        for missing in ['f', '1', '0', 'a'] {
            assert!(
                !table.iter().any(|binding| matches!(
                    binding.chord,
                    Chord::Char { ch, mods }
                        if ch == missing && mods.ctrl == Up
                )),
                "{missing:?} ถูกเพิ่มเข้ามาในก้อน a — ต้องรอก้อน c"
            );
        }
        assert!(
            !table.iter().any(|binding| matches!(
                binding.chord,
                Chord::Key {
                    key: NamedKey::Escape,
                    ..
                }
            )),
            "Esc ถูกเพิ่มเข้ามาในก้อน a — ต้องรอก้อน c"
        );
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
