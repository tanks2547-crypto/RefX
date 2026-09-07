//! refx-asset — decode, thumbnail, hash, cache, memory budget
//!
//! crate นี้ตัดสินว่าโปรแกรมจะกิน RAM 200 MB หรือ 2 GB
//! spec: docs/05-memory-and-assets.md, docs/06-security.md
// lint ทั้งหมดสืบทอดจาก [workspace.lints] ใน Cargo.toml ราก

pub mod budget; // MemoryBudget + TextureAllocator — ทางเดียวที่จองหน่วยความจำได้ (I-6)
pub mod cache; // cache.sqlite (IO thread เดียว)
pub mod decode;
pub mod encode; // PNG ของภาพที่วาง เพื่อพักลง spool (P4-5) // decode_guarded: limit + catch_unwind (I-7)
pub mod export; // เขียนไฟล์ export ทีละแถบ + atomic + ยกเลิกได้ (P5-4)
pub mod hash; // blake3 content hash
pub mod pool; // decode worker pool + priority queue + cancellation
pub mod resize; // ★ ทางเดียวที่เรียก fast_image_resize (ดูเหตุผลในไฟล์)
pub mod thumb;
pub mod working; // working texture ชั้น B — ภาพความละเอียดกลางตอนซูมเข้า (docs/04 §4) // EXIF orientation + resize Lanczos3 (RGBA8 เท่านั้น — ตัด BC7 แล้ว)
