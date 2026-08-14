//! refx-platform — crate เดียวในโปรเจกต์ที่อนุญาต `unsafe`
//!
//! ทุกบล็อก unsafe ต้องมีคอมเมนต์ `// SAFETY:` อธิบายว่าทำไมถึงถูกต้อง
//! โค้ดที่ขึ้นกับ OS ต้องอยู่ที่นี่ที่เดียว — เพื่อให้ P6 (macOS) ไม่ต้องรื้อทั้งโปรเจกต์ (ADR-007)
#![warn(clippy::all)]

pub mod clipboard; // อ่าน/เขียนภาพจาก clipboard
pub mod dialog; // native file dialog (ต้องไม่บล็อก UI thread — I-2)
pub mod fsops; // สลับไฟล์แบบทนไฟดับ (fsync dir / MOVEFILE_WRITE_THROUGH — P4-2)
pub mod locale; // ภาษาที่ผู้ใช้ตั้งไว้ใน OS (เลือกภาษาเริ่มต้นของ UI — docs/03 §0)
pub mod memory; // ถาม OS ว่าเครื่องมี RAM เท่าไหร่ (ตั้งเพดาน max_pixels)
pub mod paths; // cache/config/log dir ตามมาตรฐานแต่ละ OS
pub mod redraw; // RedrawTracker — ประตูเดียวที่ขอวาดเฟรมได้ (I-1)
pub mod single_instance;
pub mod window; // winit ApplicationHandler wrapper
