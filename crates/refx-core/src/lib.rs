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
pub mod command; // Command trait + History (undo/redo + merge/seal)
pub mod layout; // layout engines — pure functions, deterministic
pub mod selection;
pub mod spatial; // loose uniform grid: culling + hit-test
pub mod view; // Camera, ViewState, Mode
