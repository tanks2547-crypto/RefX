//! refx-core — โดเมนล้วน ๆ ไม่มี GPU ไม่มีดิสก์ ไม่มี OS
//!
//! กฎของ crate นี้ (บังคับด้วย Cargo.toml — อย่าเพิ่ม dependency นอกรายการ):
//!   * ห้าม depend: wgpu, egui, winit, rusqlite, std::fs
//!   * ทุกอย่างในนี้ต้อง unit-test ได้โดยไม่ต้องมี GPU และไม่แตะดิสก์
//!
//! spec: docs/02-data-model.md
// lint ทั้งหมดสืบทอดจาก [workspace.lints] ใน Cargo.toml ราก

pub mod arena; // generational arena + ItemId/BoardId/GroupId
pub mod board; // Board, Item, ItemCanvas, ItemMeta, AssetRef
pub mod clipboard; // ชนิดข้อมูลกลาง + trait ให้ชั้นบนเสียบตัวอ่านจริง (ไม่มีโค้ด OS)
pub mod command; // Command trait + History (undo/redo + merge/seal)
pub mod hash; // ContentHash — ชนิดล้วน ๆ ส่วนคนคำนวณอยู่ refx-asset
pub mod layout; // layout engines — pure functions, deterministic
pub mod panic_guard; // ธงบอก panic hook ว่า panic นี้ถูกดักไว้แล้ว (I-7)
pub mod selection;
pub mod spatial; // loose uniform grid: culling + hit-test
pub mod view; // Camera, ViewState, Mode
