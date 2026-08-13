#![no_main]
//! ยิงตัวอ่านไฟล์ `.refx` ด้วยไบต์ที่ไม่มีใครควบคุม (P4-1)
//!
//! ★★★ **สิ่งที่ target นี้ปกป้อง** — ไฟล์เอกสารคือ input ที่ไม่น่าไว้ใจที่สุด
//! ที่โปรแกรมนี้รับ (I-4): มันมาจากดิสก์ที่เสียได้ จากไดรฟ์เครือข่ายที่หลุดกลางทาง
//! จากเพื่อนที่ส่งมาให้ และจากรุ่นของโปรแกรมที่เรายังไม่มีอยู่
//!
//! สองข้อที่ห้ามพลาด (docs/07 §2, T3):
//!
//! 1. **ห้าม panic ไม่ว่าไบต์จะเป็นอะไร** — `.refx` ที่เปิดแล้วโปรแกรมตาย
//!    คือผู้ใช้ที่เปิดงานตัวเองไม่ได้อีกเลย ซึ่งเป็น I-3 ตรง ๆ
//! 2. **ห้ามจอง memory ตามตัวเลขที่อยู่ในไฟล์** — `doc_len` โกหกได้ และ zstd
//!    ขยายข้อมูลซ้ำ ๆ ได้เป็นพันเท่า (zip bomb) ไฟล์ 2 KB ต้องไม่ทำให้จอง RAM
//!    หลาย GB · เพดานอยู่ที่ `MAX_DOCUMENT_BYTES` และ target นี้คือสิ่งที่ยืนยันมัน
//!
//! ★★ **ยิงสามทางโดยตั้งใจ** — ถ้ายิงแต่ไบต์ดิบ fuzzer จะติดอยู่ที่ด่าน magic
//! (`"REFX"` + crc32 ที่ถูกต้อง สุ่มเจอแทบไม่ได้เลย) แล้วเกือบทุก input จะจบ
//! ตั้งแต่ 4 ไบต์แรก = target ที่ดูเหมือนทำงานแต่ไม่เคยแตะ parser จริง
//! ซึ่งเป็นรูปแบบเดียวกับ `fuzz_decode` ที่เป็น stub อยู่ 5 session
//! → จึง **ประกอบหัวไฟล์ที่ถูกต้องให้** ในทางที่ 2 และ 3 เพื่อให้ไบต์ของ fuzzer
//! ไปถึง zstd และ postcard จริง ๆ

use libfuzzer_sys::fuzz_target;
use refx_core::arena::{ArenaKey as _, BoardId};
use refx_io::dto;

/// ประกอบไฟล์ `.refx` ที่หัวถูกต้องทุกช่อง โดยเอา `body` เป็น document
///
/// crc/ความยาวถูกคำนวณให้ตรง — fuzzer จึงไม่ต้องเดาสองอย่างนั้นเอง
/// และไบต์ที่มันคุมได้จะไปโผล่ที่ตัวคลาย zstd โดยตรง
fn wrap(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dto::HEADER_LEN + body.len());
    out.extend_from_slice(&dto::MAGIC);
    out.extend_from_slice(&dto::FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(body.len() as u64).to_le_bytes());
    out.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
    out.extend_from_slice(body);
    out
}

fuzz_target!(|data: &[u8]| {
    let id = BoardId::from_parts(0, 0);

    // ---- 1. ไบต์ดิบ — ยิงด่านหัวไฟล์ (magic / version / len / crc) ----
    //
    // ★ `inspect` กับ `may_overwrite` ต้องไม่ panic ด้วย: ทั้งคู่ถูกเรียก
    //   **ก่อน** ตัดสินใจเขียนทับไฟล์ของผู้ใช้ ถ้ามันตายตรงนั้นคือตายตอนที่
    //   อันตรายที่สุด
    let _ = dto::inspect(data);
    let _ = dto::may_overwrite(data);
    let _ = dto::decode(data, id);

    // ---- 2. หัวถูกต้อง + body ที่ fuzzer คุม → ยิงตัวคลาย zstd ----
    let _ = dto::decode(&wrap(data), id);

    // ---- 3. หัวถูกต้อง + body ที่บีบมาถูกต้อง → ยิง postcard โดยตรง ----
    //
    // ทางนี้คือทางที่ทำให้ fuzzer แก้ *โครงของ document* ได้จริง
    // (ทาง 2 มันต้องผลิต stream ของ zstd ที่ถูกต้องเองก่อน ซึ่งยากกว่ามาก)
    if let Ok(packed) = zstd::encode_all(data, 3) {
        let _ = dto::decode(&wrap(&packed), id);
    }
});
