#![no_main]
//! ยิง layout engine ทั้งห้าตัวด้วย input ที่ไม่มีใครควบคุม (P3-2)
//!
//! ★★ **สิ่งที่ target นี้ปกป้อง** — `docs/03 §3` บังคับสองข้อกับทุก engine:
//!
//! 1. **ห้าม panic ไม่ว่า input จะเป็นอะไร** — ค่า aspect มาจาก `px_size` ที่
//!    อ่านจากไฟล์ผู้ใช้ (I-4) และ `LayoutParams` มาจาก UI/ไฟล์ตั้งค่า
//!    การหารศูนย์ ดัชนีหลุดขอบ หรือ `Vec` ที่โตจนหมด RAM ต้องเป็นไปไม่ได้
//! 2. **ผลลัพธ์ต้อง finite เสมอ** — `NaN` ที่หลุดลง `ItemCanvas` ทำให้ hit-test
//!    culling และ spatial index เพี้ยนทั้งระบบ **โดยไม่มี error ที่ไหนเลย**
//!    ซึ่งแย่กว่า crash เพราะผู้ใช้จะเจอ "โปรแกรมแปลก ๆ" แทนที่จะเจอข้อความ
//!
//! ★ unit test ใน `layout.rs` ยิงค่าพังชุดที่เรา *คิดออก* — target นี้ยิงชุดที่
//! เราคิดไม่ออก โดยเฉพาะการผสมกันของ aspect สุดโต่งกับ params สุดโต่ง

use libfuzzer_sys::fuzz_target;
use refx_core::arena::{ArenaKey as _, ItemId};
use refx_core::glam::Vec2;
use refx_core::layout::{Engine, LayoutParams, MAX_SIDE, MIN_SIDE, layout};

/// อ่าน f32 หนึ่งตัวจากไบต์ดิบ — **ตั้งใจให้ได้ `NaN`/`inf` บ่อย ๆ**
///
/// การแปลงไบต์เป็น f32 ตรง ๆ ทำให้ fuzzer เจอ bit pattern ที่เป็น NaN ได้เอง
/// ซึ่งคือสิ่งที่เราอยากยิงที่สุด
fn take_f32(data: &[u8], at: &mut usize) -> f32 {
    let mut bytes = [0u8; 4];
    for slot in &mut bytes {
        *slot = data.get(*at).copied().unwrap_or(0);
        *at += 1;
    }
    f32::from_le_bytes(bytes)
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let mut at = 0usize;

    let engine = match data[at] % 5 {
        0 => Engine::Grid,
        1 => Engine::Masonry,
        2 => Engine::JustifiedRows,
        3 => Engine::ShelfPack,
        _ => Engine::Radial,
    };
    at += 1;

    let params = LayoutParams {
        width: take_f32(data, &mut at),
        gap: take_f32(data, &mut at),
        target_row_height: take_f32(data, &mut at),
        // สลับระหว่าง "ให้ engine เดาเอง" กับตัวเลขที่ fuzzer เลือก (รวมทั้ง 0)
        columns: if data.get(at).copied().unwrap_or(0) % 2 == 0 {
            None
        } else {
            Some(u32::from(data.get(at + 1).copied().unwrap_or(0)))
        },
    };
    at += 2;

    // ★ เพดานจำนวนภาพ — fuzzer ไม่ควรทำให้ OOM แทนที่จะหาบั๊กจริง
    //   1024 พอที่จะเจอเส้นทางทุกเส้น (หลายแถว หลายชั้น หลายคอลัมน์)
    let mut items = Vec::new();
    while at + 8 <= data.len() && items.len() < 1024 {
        let x = take_f32(data, &mut at);
        let y = take_f32(data, &mut at);
        let n = u32::try_from(items.len()).unwrap_or(u32::MAX);
        items.push((ItemId::from_parts(n, 0), Vec2::new(x, y)));
    }

    let placed = layout(engine, &items, params);

    // ---- ข้อบังคับที่ต้องจริงเสมอ ----
    assert_eq!(placed.len(), items.len(), "layout ต้องคืนครบทุกใบ");
    for (index, p) in placed.iter().enumerate() {
        assert_eq!(p.id, items[index].0, "ลำดับผลลัพธ์ต้องตรงกับ input");
        assert!(
            p.top_left.is_finite(),
            "ตำแหน่งไม่ finite: {:?} (engine {engine:?})",
            p.top_left
        );
        // ★★ assert สัญญาจริง (`>= MIN_SIDE`) ไม่ใช่แค่ `> 0.0`
        //    เวอร์ชันแรกใช้ `> 0.0` แล้ว **จับการถอดด่านสุดท้ายออกไม่ได้เลย**
        //    (157,000 รอบไม่แดง) เพราะขนาด 0.525 ก็ยัง "มากกว่าศูนย์"
        //    — assertion ที่อ่อนกว่าสัญญาคือ target ที่เขียวโดยไม่ได้ตรวจอะไร
        assert!(
            p.size.is_finite() && p.size.x >= MIN_SIDE && p.size.y >= MIN_SIDE,
            "ขนาดต่ำกว่าสัญญา: {:?} (engine {engine:?})",
            p.size
        );
        assert!(
            p.size.x <= MAX_SIDE && p.size.y <= MAX_SIDE,
            "ขนาดเกินเพดาน: {:?} (engine {engine:?})",
            p.size
        );
        assert!(p.centre().is_finite(), "จุดกึ่งกลางไม่ finite");
    }

    // ★ deterministic — เรียกซ้ำด้วย input เดิมต้องได้ผลเดิมเป๊ะ (docs/03 §3)
    assert_eq!(placed, layout(engine, &items, params), "ผลไม่คงที่");
});
