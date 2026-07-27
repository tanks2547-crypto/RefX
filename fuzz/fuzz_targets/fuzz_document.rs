#![no_main]
//! .refx ที่ถูกดัดแปลงต้องคืน Err — ห้าม panic, ห้ามจอง memory ตามตัวเลขในไฟล์ (T3)
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // TODO(P4-1): let _ = refx_io::dto::load_bytes(data);
    let _ = data;
});
