#![no_main]
//! layout ต้องไม่ panic และไม่คืน NaN/inf ไม่ว่า input จะเป็นอะไร
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // TODO(P3-2): แปลง data เป็น Vec<ItemAspect> + LayoutParams แล้วยืนยันว่าผลลัพธ์ finite
    let _ = data;
});
