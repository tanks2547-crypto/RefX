#![no_main]
//! journal ที่เสียหายต้องกู้ได้เท่าที่กู้ได้ ไม่ทำให้แย่ลง
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // TODO(P4-3): let _ = refx_io::journal::replay_bytes(data);
    let _ = data;
});
