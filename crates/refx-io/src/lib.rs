//! refx-io — .refx format, journal, autosave, recovery
//!
//! spec: docs/07-file-format.md
//! หน้าที่หลักคือ invariant I-3: ข้อมูลผู้ใช้ห้ามหาย
// lint ทั้งหมดสืบทอดจาก [workspace.lints] ใน Cargo.toml ราก

// ★ เฉพาะตอนเทสต์: นาฬิกาของ "ผู้ใช้จำลอง" ที่เทสต์ฆ่าโปรเซสใช้วัดงานที่หายไป
//   — แยกออกมาเพราะบทเรียนของมันต้องไม่ drift ระหว่างเทสต์สองตัวที่ใช้ร่วมกัน
#[cfg(test)]
mod killclock;

pub mod autosave; // snapshot ทั้ง board กันงานหายตอน crash (P4-3)
pub mod dto; // DTO มีเวอร์ชัน — แยกจาก type ใน refx-core เสมอ
pub mod journal; // append-only command journal + CRC ต่อ record
pub mod packed; // packed mode: ฝังไฟล์ภาพต้นฉบับไว้ในเอกสาร (P4-5)
pub mod recovery; // งานที่ยังไม่เคยบันทึก: <data_dir>/recovery/<session>.refx (P4-4)
pub mod relink; // หาไฟล์ที่หายด้วย hash (5 ขั้นตอน)
pub mod save; // atomic save: tmp -> fsync -> rename -> fsync dir
pub mod spool; // ภาพที่ไม่มีไฟล์ต้นทาง: <data_local_dir>/pasted/<hash>.png (P4-5)
pub mod validate; // validate_path + bound ทุก field ตอน deserialize
