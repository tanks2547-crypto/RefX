//! refx-io — .refx format, journal, autosave, recovery
//!
//! spec: docs/07-file-format.md
//! หน้าที่หลักคือ invariant I-3: ข้อมูลผู้ใช้ห้ามหาย
// lint ทั้งหมดสืบทอดจาก [workspace.lints] ใน Cargo.toml ราก

pub mod autosave; // snapshot ทั้ง board กันงานหายตอน crash (P4-3)
pub mod dto; // DTO มีเวอร์ชัน — แยกจาก type ใน refx-core เสมอ
pub mod journal; // append-only command journal + CRC ต่อ record
pub mod recovery; // replay journal ตอนเปิดโปรแกรม
pub mod relink; // หาไฟล์ที่หายด้วย hash (5 ขั้นตอน)
pub mod save; // atomic save: tmp -> fsync -> rename -> fsync dir
pub mod validate; // validate_path + bound ทุก field ตอน deserialize
