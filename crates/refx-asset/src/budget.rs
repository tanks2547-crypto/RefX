//! เพดานหน่วยความจำ — ★ คุม **รวมทุก worker** ไม่ใช่ต่อ job (I-6)
//!
//! ทำไมสำคัญ: `Limits::max_alloc` = 1 GiB ถูกต้องสำหรับภาพ **เดียว**
//! (16384² × 4 ไบต์ = 1 GiB พอดี) แต่ถ้าเปิด 6 worker แล้วต่างคนต่างใช้เพดานของตัวเอง
//! เพดานรวมจะกลายเป็น 6 GB ซึ่งแย่ง RAM กับ Photoshop ที่ผู้ใช้เปิดคู่กันอยู่ตรง ๆ
//!
//! ตัวนี้จึงเป็น **ถังกลางถังเดียว** ที่ทุก worker ต้องขอโควตาก่อน decode
//!
//! spec: docs/05-memory-and-assets.md §2, §8

use std::sync::Arc;

use parking_lot::{Condvar, Mutex};

/// เพดาน RAM สำหรับ decode staging รวมทุก worker
///
/// ที่มาของ 256 MB: docs/05 §8 ตั้งงบ "decode staging 6 worker × ~16 MB ≈ 96 MB peak"
/// และ §2 ตั้ง `ram_limit` ไว้ที่ 256 MB — เผื่อไว้ ~2.6 เท่าของ peak ที่คาดไว้
/// เทียบกับเป้าหมาย RAM รวม ≤ 250 MB idle / ~230 MB peak
pub const DEFAULT_RAM_LIMIT: usize = 256 << 20; // 256 MB

/// ถังโควตา RAM ที่ทุก worker แชร์กัน
///
/// ใช้ `Condvar` เพราะ worker ที่ขอโควตาไม่ได้ควร **นอนรอ** ไม่ใช่วนเช็ค
/// (วนเช็คจะกิน CPU ซึ่งขัดเป้าหมายข้อ 3 ของโปรเจกต์)
#[derive(Debug)]
pub struct RamBudget {
    limit: usize,
    state: Mutex<usize>,
    space_freed: Condvar,
}

impl RamBudget {
    /// สร้างถังขนาดที่กำหนด
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            state: Mutex::new(0),
            space_freed: Condvar::new(),
        }
    }

    /// เพดานของถัง
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// ใช้ไปแล้วเท่าไหร่ (สำหรับ status bar — I-6 ต้องเห็นด้วยตา)
    #[must_use]
    pub fn used(&self) -> usize {
        *self.state.lock()
    }

    /// ขอโควตา — **นอนรอจนกว่าจะได้**
    ///
    /// ★ กติกาการอนุมัติ:
    ///   * ปกติ: อนุมัติเมื่อ `used + bytes <= limit`
    ///   * ข้อยกเว้น: งานที่ใหญ่กว่าทั้งถัง (เช่นภาพ 16384² = 1 GiB) จะได้รับอนุมัติ
    ///     **ก็ต่อเมื่อถังว่างสนิท** คือรันอยู่คนเดียว
    ///
    /// ข้อยกเว้นนี้จำเป็น ไม่งั้นภาพใหญ่จะรอตลอดกาล (deadlock) เพราะไม่มีวันมีที่พอ
    /// ผลคือ peak ชั่วคราวอาจเกินเพดาน แต่เกินได้แค่ "หนึ่งภาพ" เท่านั้น
    /// และถูกคุมด้วย `Limits::max_pixels` อีกชั้น
    ///
    /// **ห้ามเรียกจาก UI thread** — บล็อกได้ (I-2)
    pub fn reserve(self: &Arc<Self>, bytes: usize) -> RamReservation {
        let mut used = self.state.lock();
        loop {
            let fits = used.saturating_add(bytes) <= self.limit;
            let alone = *used == 0;
            if fits || alone {
                *used += bytes;
                break;
            }
            // ไม่พอ — นอนรอให้คนอื่นคืนโควตา
            self.space_freed.wait(&mut used);
        }

        if bytes > self.limit {
            tracing::warn!(
                bytes,
                limit = self.limit,
                "ภาพเดียวใหญ่กว่าเพดาน RAM ทั้งถัง — ให้รันคนเดียวชั่วคราว"
            );
        }

        RamReservation {
            budget: Arc::clone(self),
            bytes,
        }
    }

    /// คืนโควตา (เรียกจาก [`RamReservation::drop`] เท่านั้น)
    fn release(&self, bytes: usize) {
        let mut used = self.state.lock();
        *used = used.saturating_sub(bytes);
        // ปลุกทุกคนที่รออยู่ — งานที่รอมีขนาดต่างกัน คนที่ตื่นคนแรกอาจยังไม่พอ
        self.space_freed.notify_all();
    }
}

/// ใบจองโควตา — คืนให้เองตอน drop
///
/// ต้องเป็น RAII เพราะ decode **panic ได้** (I-7) ถ้าคืนด้วยมือจะรั่วทุกครั้งที่ panic
/// แล้วถังจะเต็มถาวรจนโปรแกรมค้างไปเลย
#[derive(Debug)]
#[must_use = "ต้องถือใบจองไว้ตลอดการ decode ไม่งั้นโควตาจะถูกคืนทันที"]
pub struct RamReservation {
    budget: Arc<RamBudget>,
    bytes: usize,
}

impl RamReservation {
    /// ขนาดที่จองไว้
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

impl Drop for RamReservation {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}

/// ประเมิน RAM ที่ decode ภาพขนาดนี้จะใช้
///
/// `w × h × 4` คือบัฟเฟอร์ RGBA ปลายทาง บวกเผื่ออีกเท่าตัวสำหรับบัฟเฟอร์กลาง
/// ของ decoder เอง (หลาย format ถอดเป็น scanline ก่อนแล้วค่อยแปลง)
#[must_use]
pub fn estimate_decode_bytes(width: u32, height: u32) -> usize {
    let pixels = u64::from(width) * u64::from(height);
    let rgba = pixels.saturating_mul(4);
    // ×2 เผื่อบัฟเฟอร์กลางของ decoder
    let total = rgba.saturating_mul(2);
    usize::try_from(total).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn reserve_and_release() {
        let budget = Arc::new(RamBudget::new(1000));
        {
            let r = budget.reserve(400);
            assert_eq!(budget.used(), 400);
            assert_eq!(r.bytes(), 400);
        }
        assert_eq!(budget.used(), 0, "drop แล้วต้องคืนโควตา");
    }

    #[test]
    fn multiple_reservations_accumulate() {
        let budget = Arc::new(RamBudget::new(1000));
        let _a = budget.reserve(300);
        let _b = budget.reserve(300);
        assert_eq!(budget.used(), 600);
    }

    /// ★ หัวใจของเรื่องนี้: เพดานคุม **รวมทุก worker**
    #[test]
    fn total_never_exceeds_limit_across_threads() {
        let budget = Arc::new(RamBudget::new(1000));
        let peak = Arc::new(Mutex::new(0usize));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let budget = Arc::clone(&budget);
            let peak = Arc::clone(&peak);
            handles.push(std::thread::spawn(move || {
                for _ in 0..40 {
                    let _r = budget.reserve(300);
                    let now = budget.used();
                    let mut p = peak.lock();
                    *p = (*p).max(now);
                    drop(p);
                    std::thread::yield_now();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(budget.used(), 0, "คืนครบทุกใบ");
        assert!(
            *peak.lock() <= 1000,
            "ใช้รวมทะลุเพดาน: {} > 1000",
            peak.lock()
        );
    }

    /// งานที่ใหญ่กว่าทั้งถังต้องได้รันคนเดียว ไม่ใช่รอตลอดกาล
    #[test]
    fn oversized_job_runs_alone_instead_of_deadlocking() {
        let budget = Arc::new(RamBudget::new(1000));
        let huge = budget.reserve(5000); // ใหญ่กว่าถัง 5 เท่า
        assert_eq!(budget.used(), 5000);
        drop(huge);
        assert_eq!(budget.used(), 0);
    }

    /// งานใหญ่ต้องรอจนถังว่างก่อน แล้วค่อยได้รัน
    #[test]
    fn oversized_job_waits_for_empty_budget() {
        let budget = Arc::new(RamBudget::new(1000));
        let small = budget.reserve(100);

        let waiter = {
            let budget = Arc::clone(&budget);
            std::thread::spawn(move || {
                let _big = budget.reserve(9000);
                budget.used()
            })
        };

        // ให้เวลาเธรดนั้นไปติดรอจริง ๆ
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(budget.used(), 100, "งานใหญ่ต้องยังไม่ได้เข้า");

        drop(small); // ถังว่าง → ปลุก
        let used_when_running = waiter.join().unwrap();
        assert_eq!(used_when_running, 9000, "ต้องได้รันคนเดียวหลังถังว่าง");
        assert_eq!(budget.used(), 0);
    }

    /// panic ระหว่างถือใบจองต้องคืนโควตา (I-7 — decode panic ได้เป็นเรื่องปกติ)
    #[test]
    fn reservation_is_returned_on_panic() {
        let budget = Arc::new(RamBudget::new(1000));
        let b = Arc::clone(&budget);
        // AssertUnwindSafe: RamBudget ตั้งใจให้ปลอดภัยข้าม unwind อยู่แล้ว
        // (นั่นคือเหตุผลที่ใบจองเป็น RAII) — ตัวนี้คือเทสต์ที่พิสูจน์ข้อนั้น
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _r = b.reserve(500);
            #[expect(clippy::panic, reason = "จำลอง decoder พังตาม I-7")]
            {
                panic!("decoder พัง");
            }
        }));
        assert!(result.is_err());
        assert_eq!(budget.used(), 0, "panic แล้วโควตาต้องไม่รั่ว");
    }

    #[test]
    fn estimate_is_double_rgba() {
        assert_eq!(estimate_decode_bytes(100, 100), 100 * 100 * 4 * 2);
    }

    #[test]
    fn estimate_saturates_instead_of_overflowing() {
        // ค่าที่ล้นต้องกลายเป็น usize::MAX ไม่ใช่วนกลับเป็นเลขเล็ก
        let huge = estimate_decode_bytes(u32::MAX, u32::MAX);
        assert!(huge >= (1usize << 40), "ได้ {huge}");
    }

    #[test]
    fn default_limit_matches_spec() {
        // docs/05 §2: ram_limit default 256 MB
        assert_eq!(DEFAULT_RAM_LIMIT, 268_435_456);
    }
}
