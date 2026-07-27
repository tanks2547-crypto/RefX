#![no_main]
//! decoder ต้องคืน Err เสมอสำหรับ input มั่ว — ห้าม panic ห้าม hang ห้าม OOM
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // TODO(P1-1): let _ = refx_asset::decode::decode_guarded(data, &Limits::default());
    let _ = data;
});
