//! refx-ui — egui shell + สอง mode
//!
//! spec: docs/03-modes-and-ui.md
//! หลักสำคัญ: shell เขียนครั้งเดียวใช้ทั้งสอง mode, mode สลับแค่ CentralPanel
// lint ทั้งหมดสืบทอดจาก [workspace.lints] ใน Cargo.toml ราก

pub mod app; // ต่อสายทุก crate + event loop integration
pub mod arrange; // ViewportBehavior ของ Arrange mode
pub mod canvas; // ViewportBehavior ของ Canvas mode
pub mod copy; // ก๊อปข้อความขึ้น clipboard บนเธรดชั่วคราว (P2-10) — ไม่ใช่ UI thread
pub mod export; // ต่อฝั่ง GPU เข้ากับฝั่งตัวเข้ารหัส — งาน export หนึ่งงาน (P5-4)
pub mod fonts; // ฟอนต์ที่ฝังใน binary (docs/03 §0) — ไทย + ละติน ยังไม่มี CJK
pub mod inspector;
pub mod instances; // Board → QuadInstance (ฟังก์ชันบริสุทธิ์ · วัดได้จาก benches/)
pub mod keymap; // ★ ตาราง data ของคีย์ลัด — ห้าม hard-code `if key == …` (docs/03 §5)
pub mod shell; // โครง UI กลาง: tabs, toolbar, library, inspector, status bar
pub mod sidecar; // .refx-meta ฝั่ง UI — โหลด/เขียน tag ของโฟลเดอร์ (P5-5)
pub mod text; // ★ ประตูเดียวของข้อความที่ผู้ใช้เห็น (docs/03 §0) — ห้ามเขียนสตริงตรงใน widget
pub mod theme;
pub mod tools;

/// handle_input คืน Command ไม่ใช่แก้ board เอง — ทุกการเปลี่ยนแปลงไหลผ่านทางเดียว
pub trait ViewportBehavior {
    /// วาดเนื้อใน CentralPanel
    fn draw(&mut self, ui: &mut egui::Ui, rect: egui::Rect);
}
