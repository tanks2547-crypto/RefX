//! refx-core — โดเมนล้วน ๆ ไม่มี GPU ไม่มีดิสก์ ไม่มี OS
//!
//! กฎของ crate นี้ (บังคับด้วย Cargo.toml — อย่าเพิ่ม dependency นอกรายการ):
//!   * ห้าม depend: wgpu, egui, winit, rusqlite, std::fs
//!   * ทุกอย่างในนี้ต้อง unit-test ได้โดยไม่ต้องมี GPU และไม่แตะดิสก์
//!
//! spec: docs/02-data-model.md
// lint ทั้งหมดสืบทอดจาก [workspace.lints] ใน Cargo.toml ราก

/// ★ re-export `glam` — ชนิดของมัน (`Vec2`) อยู่ใน API สาธารณะของ crate นี้
///
/// ผู้เรียกที่อยู่**นอก workspace** (เช่น `fuzz/` ซึ่งอยู่ใน `exclude`) จึงประกอบ
/// ค่าส่งเข้ามาได้โดยไม่ต้องประกาศ `glam` เองแล้วเสี่ยงว่าเวอร์ชันจะไม่ตรงกัน
/// — สองเวอร์ชันของชนิดเดียวกันคือ error ตอนคอมไพล์ที่อ่านไม่รู้เรื่องเลย
pub use glam;

pub mod align; // align / distribute + alignment guide — pure functions
pub mod arena; // generational arena + ItemId/BoardId/GroupId
pub mod board; // Board, Item, ItemCanvas, ItemMeta, AssetRef
pub mod clipboard; // ชนิดข้อมูลกลาง + trait ให้ชั้นบนเสียบตัวอ่านจริง (ไม่มีโค้ด OS)
pub mod command; // Command trait + History (undo/redo + merge/seal)
pub mod geom; // Rect / Obb — รูปทรงสำหรับ culling, hit-test, rubber-band
pub mod hash; // ContentHash — ชนิดล้วน ๆ ส่วนคนคำนวณอยู่ refx-asset
pub mod interact; // เครื่องสถานะของการเลือกบน canvas (คืน Command ไม่แก้ board เอง)
pub mod layout; // layout engines — pure functions, deterministic
pub mod panic_guard; // ธงบอก panic hook ว่า panic นี้ถูกดักไว้แล้ว (I-7)
pub mod pick; // color picker (world → pixel ต้นฉบับ) + measure — pure functions
pub mod selection;
pub mod spatial; // loose uniform grid: culling + hit-test
pub mod view; // Camera, ViewState, Mode
pub mod zorder; // ย้ายชั้น (`[` `]`) — pure function คืนลำดับใหม่ ไม่แตะ board
