//! refx-ui — egui shell + สอง mode
//!
//! spec: docs/03-modes-and-ui.md
//! หลักสำคัญ: shell เขียนครั้งเดียวใช้ทั้งสอง mode, mode สลับแค่ CentralPanel
// lint ทั้งหมดสืบทอดจาก [workspace.lints] ใน Cargo.toml ราก

pub mod app; // ต่อสายทุก crate + event loop integration
pub mod arrange; // ViewportBehavior ของ Arrange mode
pub mod canvas; // ViewportBehavior ของ Canvas mode
pub mod inspector;
pub mod keymap; // ตาราง data โหลดจาก keymap.toml — ห้าม hard-code (ADR-007)
pub mod shell; // โครง UI กลาง: tabs, toolbar, library, inspector, status bar
pub mod theme;
pub mod tools;

/// handle_input คืน Command ไม่ใช่แก้ board เอง — ทุกการเปลี่ยนแปลงไหลผ่านทางเดียว
pub trait ViewportBehavior {
    /// วาดเนื้อใน CentralPanel
    fn draw(&mut self, ui: &mut egui::Ui, rect: egui::Rect);
}
