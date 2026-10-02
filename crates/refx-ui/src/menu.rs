//! ★★★ แถบเมนู `File / Edit / View / Help` (ROADMAP P5-9c)
//!
//! เหตุผล: ก่อนหน้านี้ **ทุกฟีเจอร์เข้าถึงได้ทางคีย์ลัดเท่านั้น** และผู้ใช้คือนักวาด
//! ไม่ใช่คนที่อ่าน `KEYBOARD-SHORTCUTS.md` ก่อนลองโปรแกรม
//!
//! # ★★★ เมนูสร้างจาก `keymap::builtin()` — ไม่ใช่รายการพิมพ์มือ
//!
//! รายการในเมนู **คือ action ทุกตัวที่ตารางคีย์ลัดรู้จัก** เรียงตามลำดับในตาราง
//! · แต่ละรายการแสดงคีย์ลัดของตัวเองที่อ่านจากตารางที่ใช้อยู่จริง ([`keymap::active`])
//! → เมนูกลายเป็นทางค้นพบคีย์ลัดไปในตัว และ **ผิดพร้อมกันเท่านั้น ไม่มีทางผิดคนละทาง**
//!
//! สองอย่างที่ต้องตัดสินต่อ action — **อยู่ไหน** ([`place_of`]) และ **ชื่ออะไร**
//! ([`label_of`]) — เป็น `match` ที่ไม่มี `_ =>` · เพิ่ม action ใหม่เมื่อไหร่ คอมไพเลอร์
//! บังคับให้มาตัดสินที่นี่ ไม่มีทางหลุดออกจากเมนูเงียบ ๆ
//!
//! ★ กดรายการในเมนู = **ทางเดียวกับกดคีย์ลัดเป๊ะ** (`RefxApp::request`) ไม่ใช่ทางที่สอง
//!
//! ★ `Alt` ไม่เปิดเมนูนี้ (egui ไม่มี mnemonic) · ประตู
//!   `no_builtin_shortcut_needs_alt` กันไว้ว่าวันที่เพิ่มมันจะไม่ชนกับตารางคีย์ลัด

use crate::keymap::{self, Action, AppearanceKey, GroupRequest, SaveRequest, TabKey, ZoomRequest};
use crate::text::{self, Key, Lang};
use refx_core::interact::Tool;
use refx_core::zorder::ZMove;

/// เมนูบนแถบ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// เปิด · บันทึก · ส่งออก · แท็บ
    File,
    /// ย้อน · คลิปบอร์ด · การเลือก · ชั้น · กลุ่ม · เครื่องมือ
    Edit,
    /// โหมด · ซูม · การแสดงผล
    View,
}

impl Menu {
    /// เมนูที่มีรายการจากตารางคีย์ลัด — ตามลำดับบนแถบ
    pub const ALL: [Self; 3] = [Self::File, Self::Edit, Self::View];

    /// ชื่อบนแถบ
    #[must_use]
    pub fn title(self) -> Key {
        match self {
            Self::File => Key::MenuFile,
            Self::Edit => Key::MenuEdit,
            Self::View => Key::MenuView,
        }
    }
}

/// action นี้อยู่ที่ไหนในเมนู: **(เมนู, ก้อน, ลำดับในก้อน)** · ก้อนต่างกัน = มีเส้นคั่น
///
/// ★★ **สร้างจากตาราง ไม่ได้แปลว่าเรียงตามตาราง** (ตัดสิน 2 ต.ค. 2026) — ตารางเรียงตาม
/// ที่คีย์ลัดถูกเพิ่มเข้ามา (`Ctrl+S` มาก่อน `Ctrl+O`) ซึ่งไม่ใช่ลำดับที่คนหาในเมนู
/// · เมนูมีลำดับของตัวเอง (เปิด → บันทึก → ส่งออก) ส่วน **ความครบยังมาจากตาราง**
/// และ `match` นี้ไม่มี `_ =>` — action ใหม่ต้องถูกวางก่อนคอมไพล์ผ่าน
#[must_use]
pub fn place_of(action: Action) -> (Menu, u8, u8) {
    match action {
        Action::OpenBoard => (Menu::File, 0, 0),
        Action::Save(SaveRequest::Save) => (Menu::File, 0, 1),
        Action::Save(SaveRequest::SaveAs) => (Menu::File, 0, 2),
        Action::Export => (Menu::File, 1, 0),
        Action::Tab(TabKey::New) => (Menu::File, 2, 0),
        Action::Tab(TabKey::Close) => (Menu::File, 2, 1),
        Action::Tab(TabKey::Next) => (Menu::File, 2, 2),
        Action::History(keymap::HistoryRequest::Undo) => (Menu::Edit, 0, 0),
        Action::History(keymap::HistoryRequest::Redo) => (Menu::Edit, 0, 1),
        Action::Paste => (Menu::Edit, 1, 0),
        Action::Delete => (Menu::Edit, 1, 1),
        Action::SelectAll => (Menu::Edit, 2, 0),
        Action::ClearSelection => (Menu::Edit, 2, 1),
        Action::ZOrder(ZMove::ToFront) => (Menu::Edit, 3, 0),
        Action::ZOrder(ZMove::Forward) => (Menu::Edit, 3, 1),
        Action::ZOrder(ZMove::Backward) => (Menu::Edit, 3, 2),
        Action::ZOrder(ZMove::ToBack) => (Menu::Edit, 3, 3),
        Action::Group(GroupRequest::Group) => (Menu::Edit, 4, 0),
        Action::Group(GroupRequest::Ungroup) => (Menu::Edit, 4, 1),
        Action::Appearance(AppearanceKey::FlipHorizontal) => (Menu::Edit, 5, 0),
        Action::Tool(Tool::Select) => (Menu::Edit, 6, 0),
        Action::Tool(Tool::Crop) => (Menu::Edit, 6, 1),
        Action::Tool(Tool::Picker) => (Menu::Edit, 6, 2),
        Action::Tool(Tool::Measure) => (Menu::Edit, 6, 3),
        Action::Tool(Tool::Text) => (Menu::Edit, 6, 4),
        Action::ToggleMode => (Menu::View, 0, 0),
        Action::Zoom(ZoomRequest::FitBoard) => (Menu::View, 1, 0),
        Action::Zoom(ZoomRequest::FitSelection) => (Menu::View, 1, 1),
        Action::Zoom(ZoomRequest::Actual) => (Menu::View, 1, 2),
        Action::Appearance(AppearanceKey::ToggleBoardGrayscale) => (Menu::View, 2, 0),
    }
}

/// ชื่อของ action ในเมนู — ผ่าน `text::` เสมอ (สองภาษา · ประตู tofu)
#[must_use]
pub fn label_of(action: Action) -> Key {
    match action {
        Action::History(keymap::HistoryRequest::Undo) => Key::ActUndo,
        Action::History(keymap::HistoryRequest::Redo) => Key::ActRedo,
        Action::Paste => Key::ActPaste,
        Action::Delete => Key::ActDelete,
        Action::ZOrder(ZMove::Backward) => Key::ActSendBackward,
        Action::ZOrder(ZMove::Forward) => Key::ActBringForward,
        Action::ZOrder(ZMove::ToBack) => Key::ActSendToBack,
        Action::ZOrder(ZMove::ToFront) => Key::ActBringToFront,
        Action::Tool(Tool::Select) => Key::ToolSelect,
        Action::Tool(Tool::Crop) => Key::ToolCrop,
        Action::Tool(Tool::Picker) => Key::ToolPicker,
        Action::Tool(Tool::Measure) => Key::ToolMeasure,
        Action::Tool(Tool::Text) => Key::ToolText,
        Action::Appearance(AppearanceKey::ToggleBoardGrayscale) => Key::ToolGrayscale,
        Action::Appearance(AppearanceKey::FlipHorizontal) => Key::ActFlipHorizontal,
        Action::Group(GroupRequest::Group) => Key::ActGroup,
        Action::Group(GroupRequest::Ungroup) => Key::ActUngroup,
        Action::Save(SaveRequest::Save) => Key::ActSave,
        Action::Save(SaveRequest::SaveAs) => Key::ActSaveAs,
        Action::OpenBoard => Key::ActOpen,
        Action::Tab(TabKey::New) => Key::ActNewTab,
        Action::Tab(TabKey::Close) => Key::ActCloseTab,
        Action::Tab(TabKey::Next) => Key::ActNextTab,
        Action::ToggleMode => Key::ActToggleMode,
        Action::SelectAll => Key::ActSelectAll,
        Action::ClearSelection => Key::ActClearSelection,
        Action::Export => Key::ExportTitle,
        Action::Zoom(ZoomRequest::Actual) => Key::ActZoomActual,
        Action::Zoom(ZoomRequest::FitSelection) => Key::ActZoomFitSelection,
        Action::Zoom(ZoomRequest::FitBoard) => Key::ActZoomFitBoard,
    }
}

/// ★★★ action ทุกตัวที่ตารางคีย์ลัด **ที่มากับโปรแกรม** รู้จัก — ไม่ซ้ำ เรียงตามตาราง
///
/// สร้างจาก [`keymap::builtin`] ไม่ใช่จาก `Action::ALL` — ตารางคือแหล่งความจริงที่
/// คู่มือใช้อยู่แล้ว (`docs/KEYBOARD-SHORTCUTS.md`) เมนูจึงเป็นมุมมองที่สามของตาราง
/// เดียวกัน · ★ ไม่ใช้ตารางของผู้ใช้ (`keymap.toml`) เป็นที่มาของ *รายการ* — ผู้ใช้
/// ที่ลบคีย์ลัดของคำสั่งหนึ่งทิ้งยังต้องสั่งมันจากเมนูได้
#[must_use]
pub fn actions() -> Vec<Action> {
    let mut seen = Vec::new();
    for binding in keymap::builtin().bindings() {
        if !seen.contains(&binding.action) {
            seen.push(binding.action);
        }
    }
    seen
}

/// คีย์ลัดที่ใช้อยู่จริงของ action นี้ (ตารางของผู้ใช้ถ้ามี) — ตัวแรกที่แสดงผลได้
#[must_use]
pub fn shortcut_of(action: Action) -> Option<String> {
    keymap::active()
        .bindings()
        .iter()
        .filter(|binding| binding.action == action)
        .find_map(|binding| binding.chord.display())
}

/// รายการหนึ่งบรรทัดของเมนู
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// สั่งอะไร
    pub action: Action,
    /// ก้อนที่อยู่ — เปลี่ยนก้อน = เส้นคั่น
    pub group: u8,
    /// ลำดับในก้อน
    pub rank: u8,
    /// ข้อความคีย์ลัดที่แสดงทางขวา
    pub shortcut: Option<String>,
}

/// รายการของเมนูนี้ เรียงตาม [`place_of`] (ก้อน แล้วลำดับในก้อน)
#[must_use]
pub fn entries(menu: Menu) -> Vec<Entry> {
    let mut out: Vec<Entry> = actions()
        .into_iter()
        .filter_map(|action| {
            let (at, group, rank) = place_of(action);
            (at == menu).then(|| Entry {
                action,
                group,
                rank,
                shortcut: shortcut_of(action),
            })
        })
        .collect();
    out.sort_by_key(|entry| (entry.group, entry.rank));
    out
}

/// สิ่งที่ผู้ใช้กดในเมนูในเฟรมนี้
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// คำสั่งจากตารางคีย์ลัด — ทางเดียวกับกดคีย์
    Action(Action),
    /// `File → เปิดโฟลเดอร์งานที่เก็บไว้` (`docs/07 §4` — ทางเดินไปหา `recovery/kept/`)
    OpenKeptFolder,
}

/// วาดแถบเมนู — คืนสิ่งที่ถูกกดในเฟรมนี้ (ถ้ามี)
///
/// `Help → คีย์ลัดทั้งหมด` เปิดแผงตั้งค่า (ซึ่งมีตารางคีย์ลัดอยู่แล้ว) ผ่าน `settings_open`
pub fn bar(ui: &mut egui::Ui, lang: Lang, settings_open: &mut bool) -> Option<Pick> {
    let mut picked = None;
    for menu in Menu::ALL {
        ui.menu_button(text::t(lang, menu.title()), |ui| {
            let mut last_group = None;
            for entry in entries(menu) {
                if last_group.is_some_and(|group| group != entry.group) {
                    ui.separator();
                }
                last_group = Some(entry.group);
                let mut button = egui::Button::new(text::t(lang, label_of(entry.action)));
                if let Some(shortcut) = entry.shortcut {
                    button = button.shortcut_text(shortcut);
                }
                if ui.add(button).clicked() {
                    picked = Some(Pick::Action(entry.action));
                    ui.close();
                }
            }
            if menu == Menu::File {
                ui.separator();
                if ui
                    .button(text::t(lang, Key::MenuOpenKept))
                    .on_hover_text(text::t(lang, Key::MenuOpenKeptHint))
                    .clicked()
                {
                    picked = Some(Pick::OpenKeptFolder);
                    ui.close();
                }
            }
        });
    }
    ui.menu_button(text::t(lang, Key::MenuHelp), |ui| {
        if ui.button(text::t(lang, Key::MenuShortcuts)).clicked() {
            *settings_open = true;
            ui.close();
        }
        ui.separator();
        ui.label(format!("RefX {}", env!("CARGO_PKG_VERSION")));
    });
    picked
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// ★★★ **ประตู: เมนูกับตาราง `Binding` ตรงกันเสมอ** (ROADMAP P5-9c)
    ///
    /// ทุก action ที่ตารางรู้จักอยู่ในเมนู **ครั้งเดียวพอดี** · ทุกรายการในเมนูมีอยู่
    /// ในตาราง · คีย์ลัดที่แสดง = `Chord::display` ของ binding ตัวจริง
    #[test]
    fn the_menus_hold_exactly_the_actions_of_the_shortcut_table() {
        let mut in_menus: Vec<Action> = Menu::ALL
            .iter()
            .flat_map(|menu| entries(*menu))
            .map(|entry| entry.action)
            .collect();
        let table = actions();
        assert_eq!(in_menus.len(), table.len(), "จำนวนในเมนูไม่เท่ากับในตาราง");
        for action in &table {
            assert_eq!(
                in_menus.iter().filter(|a| *a == action).count(),
                1,
                "{} ไม่ได้อยู่ในเมนูครั้งเดียวพอดี",
                action.name()
            );
        }
        in_menus.retain(|a| !table.contains(a));
        assert!(in_menus.is_empty(), "มีรายการที่ตารางไม่รู้จัก: {in_menus:?}");

        // ★ ตารางครอบทุก action ที่โปรแกรมมี — ไม่งั้นคำสั่งที่ไม่มีคีย์ลัดจะไม่มีทางเข้า
        for action in Action::ALL {
            assert!(
                table.contains(action),
                "{} ไม่มีในตาราง → ไม่มีในเมนู",
                action.name()
            );
        }

        for menu in Menu::ALL {
            for entry in entries(menu) {
                let want = keymap::builtin()
                    .bindings()
                    .iter()
                    .filter(|b| b.action == entry.action)
                    .find_map(|b| b.chord.display());
                assert_eq!(
                    entry.shortcut,
                    want,
                    "{}: คีย์ลัดในเมนูไม่ตรงตาราง",
                    entry.action.name()
                );
            }
        }
    }

    /// ★★ **เมนูมีลำดับของตัวเอง** — เปิด → บันทึก → บันทึกเป็น → ส่งออก (ตัดสิน 2 ต.ค. 2026)
    ///
    /// ตารางคีย์ลัดมี `Ctrl+S` ก่อน `Ctrl+O` · ถ้าเมนูเรียงตามตาราง "บันทึก" จะมาก่อน
    /// "เปิด" ซึ่งคือสิ่งที่เกิดในรอบก่อน (เห็นบนภาพจอจริง)
    #[test]
    fn the_file_menu_reads_open_then_save_then_export() {
        let file: Vec<Action> = entries(Menu::File).iter().map(|e| e.action).collect();
        assert_eq!(
            file,
            [
                Action::OpenBoard,
                Action::Save(SaveRequest::Save),
                Action::Save(SaveRequest::SaveAs),
                Action::Export,
                Action::Tab(TabKey::New),
                Action::Tab(TabKey::Close),
                Action::Tab(TabKey::Next),
            ]
        );
        // NC — ตารางเองเรียงคนละแบบ ไม่งั้นข้อนี้ผ่านเพราะบังเอิญ
        let table = actions();
        let pos = |a: Action| table.iter().position(|x| *x == a).unwrap();
        assert!(
            pos(Action::Save(SaveRequest::Save)) < pos(Action::OpenBoard),
            "ตารางเรียงเปิดก่อนบันทึกอยู่แล้ว — ข้อนี้ไม่ได้พิสูจน์ว่าเมนูมีลำดับของตัวเอง"
        );
    }

    /// ★ ไม่มีสองรายการไหนในเมนูเดียวกันที่ได้ตำแหน่งเดียวกัน — ลำดับต้องไม่ขึ้นกับตาราง
    #[test]
    fn no_two_entries_share_a_place_in_the_same_menu() {
        for menu in Menu::ALL {
            let mut places: Vec<(u8, u8)> =
                entries(menu).iter().map(|e| (e.group, e.rank)).collect();
            let total = places.len();
            places.sort_unstable();
            places.dedup();
            assert_eq!(places.len(), total, "{menu:?}: มีรายการที่ตำแหน่งซ้ำกัน");
        }
    }

    /// ★ ทุกเมนูมีของ และไม่มีรายการไหนชื่อซ้ำกันในเมนูเดียวกัน (ทั้งสองภาษา)
    #[test]
    fn every_menu_has_items_with_distinct_names_in_both_languages() {
        for lang in [Lang::En, Lang::Th] {
            for menu in Menu::ALL {
                let names: Vec<&str> = entries(menu)
                    .iter()
                    .map(|e| text::t(lang, label_of(e.action)))
                    .collect();
                assert!(!names.is_empty(), "{menu:?} ว่าง");
                let mut unique = names.clone();
                unique.sort_unstable();
                unique.dedup();
                assert_eq!(
                    unique.len(),
                    names.len(),
                    "{lang:?} {menu:?}: ชื่อซ้ำ {names:?}"
                );
            }
        }
    }

    /// ★★ `Alt` เป็นของระบบเมนูของ OS — ตารางที่มากับโปรแกรมห้ามมีคีย์ลัดที่ต้องกด `Alt`
    ///
    /// วันที่เพิ่ม mnemonic (`Alt+F` เปิด File) จะไม่มีทางชนกับคีย์ลัดเดิม
    #[test]
    fn no_builtin_shortcut_needs_alt() {
        for binding in keymap::builtin().bindings() {
            let (keymap::Chord::Char { mods, .. } | keymap::Chord::Key { mods, .. }) =
                binding.chord;
            assert_ne!(
                mods.alt,
                keymap::Hold::Down,
                "{} ต้องกด Alt — จะชนกับการเปิดเมนู",
                binding.action.name()
            );
        }
    }
}
