//! refx-render — wgpu renderer
//!
//! spec: docs/04-rendering.md
//! ห้าม depend egui/winit — renderer ต้องเทสต์แบบ headless ได้
// lint ทั้งหมดสืบทอดจาก [workspace.lints] ใน Cargo.toml ราก

pub mod atlas; // Texture2DArray สำหรับ thumbnail + free-list
pub mod device; // สร้าง device/surface + กู้จาก device lost (P0-5 — ทำก่อนอย่างอื่น)
pub mod instance; // QuadInstance (64 B) + instance buffer แบบ pre-allocated
pub mod pipeline; // render pipeline + quad.wgsl
pub mod texture; // working / full-res texture + mipmap
