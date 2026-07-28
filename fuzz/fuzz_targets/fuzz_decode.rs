#![no_main]
//! ★ ผิวโจมตีหลักของโปรแกรมทั้งหมด (I-4) — ทุกไบต์ที่นี่คือไฟล์ที่เชื่อไม่ได้
//!
//! เป้าหมายไม่ใช่แค่ "ห้าม panic" แต่คือ **ยืนยันว่าเกราะยังกันได้จริง**
//! ถ้า input มั่วทำให้ `decode_guarded` คืน `Ok` ที่มีภาพเกินเพดาน แปลว่าเกราะ
//! ที่กัน decompression bomb พังแล้ว ซึ่งอันตรายกว่า panic เพราะ panic ถูก
//! `catch_unwind` ดักไว้อยู่แล้ว (I-7) แต่การจอง RAM เกินเพดานไม่มีใครดัก
//!
//! > **บทเรียน 28 ก.ค. 2026:** ไฟล์นี้เคยเป็น `let _ = data;` อยู่ 5 session
//! > โดยที่ `fuzz.yml` รันทุกคืนแล้วเขียวทุกครั้ง — เพราะไม่ได้เรียกอะไรเลย
//! > สาเหตุที่ไม่มีใครจับได้: `fuzz/` อยู่ใน `exclude` ของ workspace
//! > `cargo clippy --all-targets` จึงไม่เคยมองเห็นโค้ดในนี้
//!
//! spec: docs/06-security.md §2.5, §3

use libfuzzer_sys::fuzz_target;
use refx_asset::decode::{Limits, decode_guarded, probe_dimensions};
use refx_asset::thumb::read_orientation;

/// เพดานสำหรับ fuzz — เล็กกว่าของจริงโดยตั้งใจ
///
/// `Limits::default()` ยอมให้ถึง 16384² ซึ่ง decode จริงจะจอง RAM ~1 GiB
/// libFuzzer มี `-rss_limit_mb=2048` เป็นค่าปริยาย → ภาพใหญ่ที่ **ถูกต้อง**
/// จะถูกรายงานเป็น OOM ทั้งที่ไม่ใช่บั๊ก แล้วกลบผลจริงที่เราต้องการหา
///
/// ตรรกะของเกราะเป็นชุดเดียวกันทุกเพดาน การลดตัวเลขจึงไม่ลดความครอบคลุม
fn fuzz_limits() -> Limits {
    Limits {
        max_file_bytes: 1 << 20, // 1 MB — input ของ fuzzer เล็กกว่านี้อยู่แล้ว
        max_pixels: 4 << 20,     // 2048² = 4 Mpx
        max_dimension: 8192,
        max_alloc: 64 << 20,
        ..Limits::default()
    }
}

fuzz_target!(|data: &[u8]| {
    let limits = fuzz_limits();

    // ---- 1. อ่าน header อย่างเดียว (เส้นทางที่ decode pool ใช้ขอโควตา RAM) ----
    let probed = probe_dimensions(data, &limits);
    if let Ok((width, height)) = probed {
        assert!(
            width <= limits.max_dimension && height <= limits.max_dimension,
            "probe ปล่อยขนาดเกิน max_dimension: {width}x{height}"
        );
        assert!(
            u64::from(width) * u64::from(height) <= limits.max_pixels,
            "probe ปล่อยภาพเกิน max_pixels: {width}x{height}"
        );
    }

    // ---- 2. ตัวสแกน EXIF ของ JPEG ที่เขียนเอง (28 ก.ค. 2026) ----
    // เดินไบต์ดิบด้วยมือ จึงเป็นจุดที่ควรถูก fuzz ที่สุดจุดหนึ่งในโปรเจกต์
    let _ = read_orientation(data);

    // ---- 3. decode เต็มรูปแบบ ----
    if let Ok(image) = decode_guarded(data, &limits) {
        let (width, height) = (image.width(), image.height());
        assert!(
            width <= limits.max_dimension && height <= limits.max_dimension,
            "decode คืนภาพเกิน max_dimension: {width}x{height}"
        );
        assert!(
            u64::from(width) * u64::from(height) <= limits.max_pixels,
            "decode คืนภาพเกิน max_pixels: {width}x{height}"
        );
        // buffer ต้องมีขนาดตรงกับที่ประกาศ ไม่งั้นผู้เรียกที่เชื่อ w×h จะอ่านเกินขอบ
        assert_eq!(
            image.as_raw().len(),
            width as usize * height as usize * 4,
            "ขนาด buffer ไม่ตรงกับ {width}x{height}"
        );

        // ★ ขนาดที่ decode ได้ต้องไม่ใหญ่กว่าที่ header บอก
        //   decode pool จองโควตา RAM จากค่า probe **ก่อน** decode (docs/05 §3)
        //   ถ้าภาพจริงใหญ่กว่าที่ probe บอก แปลว่าโควตาที่จองไว้ต่ำกว่าการใช้จริง
        //   ซึ่งทำให้เพดาน RAM รวมทุก worker (I-6) ไม่มีความหมาย
        if let Ok((probe_w, probe_h)) = probed {
            assert!(
                u64::from(width) * u64::from(height) <= u64::from(probe_w) * u64::from(probe_h),
                "decode ได้ {width}x{height} ใหญ่กว่าที่ header บอก {probe_w}x{probe_h} \
                 — โควตา RAM ที่จองไว้จะไม่พอ"
            );
        }
    }
});
