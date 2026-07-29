//! `RedrawTracker` — ประตูเดียวที่อนุญาตให้ขอวาดเฟรมใหม่
//!
//! ทำไมต้องมี: I-1 บอกว่า idle ต้องกิน CPU 0% ซึ่งพังได้ง่ายมากถ้ามีโค้ดสักที่
//! เรียก `request_redraw()` ทุกเฟรมโดยไม่ตั้งใจ (spike ขั้นที่ 1 เจอของจริงมาแล้ว
//! — `egui-winit` คืน `repaint = true` ให้ `RedrawRequested` ของตัวเอง กลายเป็นลูป 160 fps)
//!
//! การบังคับให้ทุกคำขอผ่านที่นี่ทำให้ตอบได้เสมอว่า "เฟรมนี้เกิดเพราะอะไร"
//! และเขียนเทสต์ headless ได้โดยไม่ต้องมีหน้าต่างหรือ GPU
//!
//! spec: docs/04-rendering.md §1

/// เหตุผลที่อนุญาตให้ขอวาดเฟรมใหม่
///
/// สี่ตัวแรกคือรายการใน docs/04 §1 ห้ามเพิ่มเหตุผลใหม่โดยไม่แก้ spec ก่อน
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedrawReason {
    /// §1 ข้อ 1 — เมาส์ / คีย์บอร์ด / resize / drag & drop
    UserInput,
    /// §1 ข้อ 2 — worker ส่ง texture ที่โหลดเสร็จกลับมา
    ///
    /// มาจากเธรดอื่นผ่าน `EventLoopProxy` ซึ่ง **ปลุก event loop ที่หลับอยู่**
    /// ถ้าไม่มีตัวปลุก ภาพที่ decode เสร็จจะไม่ขึ้นจนกว่าผู้ใช้จะขยับเมาส์
    /// = "ลากภาพเข้ามาแล้วไม่มีอะไรเกิดขึ้น" ซึ่งเป็นสิ่งแรกที่ CLAUDE.md ห้าม
    TextureReady,
    /// §1 ข้อ 3 — animation กำลังเล่น (ต้องจบใน ≤ 200 ms)
    Animation,
    /// §1 ข้อ 4 — `egui::Context::request_repaint_after()` ขอมา
    EguiRepaint,
    /// docs/04 §7 — กู้ surface หลัง `Lost` / `Outdated` แล้วต้องวาดใหม่หนึ่งครั้ง
    SurfaceRecovery,
}

impl RedrawReason {
    /// เหตุผลทั้งหมด เรียงคงที่ — ใช้ทำดัชนีและวนรายงาน
    ///
    /// ใช้ array ไม่ใช่ `HashMap` เพราะลำดับต้อง deterministic (กฎใน CLAUDE.md)
    pub const ALL: [Self; 5] = [
        Self::UserInput,
        Self::TextureReady,
        Self::Animation,
        Self::EguiRepaint,
        Self::SurfaceRecovery,
    ];

    const fn index(self) -> usize {
        match self {
            Self::UserInput => 0,
            Self::TextureReady => 1,
            Self::Animation => 2,
            Self::EguiRepaint => 3,
            Self::SurfaceRecovery => 4,
        }
    }
}

/// นับจำนวนครั้งที่ขอ redraw แยกตามเหตุผล
///
/// `Default` = ยังไม่เคยขอเลย (นับเป็นศูนย์ทุกช่อง)
#[derive(Debug, Default, Clone)]
pub struct RedrawTracker {
    counts: [u64; RedrawReason::ALL.len()],
    total: u64,
    last: Option<RedrawReason>,
}

impl RedrawTracker {
    /// สร้างตัวนับเปล่า
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// บันทึกว่ามีการขอวาดเฟรมใหม่เพราะเหตุผลนี้
    ///
    /// คืน `true` เสมอเพื่อให้เขียนแบบ `if tracker.record(..) { window.request_redraw() }` ได้
    /// — จุดประสงค์คือทำให้ "ขอ redraw" กับ "บันทึกเหตุผล" แยกจากกันไม่ได้
    pub fn record(&mut self, reason: RedrawReason) -> bool {
        self.counts[reason.index()] += 1;
        self.total += 1;
        self.last = Some(reason);
        tracing::trace!(?reason, total = self.total, "redraw requested");
        true
    }

    /// จำนวนคำขอทั้งหมดตั้งแต่เริ่มนับ
    #[must_use]
    pub fn total(&self) -> u64 {
        self.total
    }

    /// จำนวนคำขอของเหตุผลหนึ่ง
    #[must_use]
    pub fn count_of(&self, reason: RedrawReason) -> u64 {
        self.counts[reason.index()]
    }

    /// เหตุผลของคำขอล่าสุด — `None` ถ้ายังไม่เคยขอเลย
    #[must_use]
    pub fn last_reason(&self) -> Option<RedrawReason> {
        self.last
    }

    /// ล้างตัวนับ (ใช้ตอนเริ่มช่วงวัดผลใหม่ในเทสต์/benchmark)
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// สรุปเป็นข้อความสำหรับ status bar / log
    #[must_use]
    pub fn summary(&self) -> String {
        let parts: Vec<String> = RedrawReason::ALL
            .iter()
            .filter(|r| self.count_of(**r) > 0)
            .map(|r| format!("{r:?}={}", self.count_of(*r)))
            .collect();
        if parts.is_empty() {
            "ไม่มีการขอวาดเฟรมเลย".to_owned()
        } else {
            parts.join(" ")
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn new_tracker_counts_zero() {
        let tracker = RedrawTracker::new();
        assert_eq!(tracker.total(), 0);
        assert_eq!(tracker.last_reason(), None);
        for reason in RedrawReason::ALL {
            assert_eq!(tracker.count_of(reason), 0);
        }
    }

    #[test]
    fn record_counts_per_reason() {
        let mut tracker = RedrawTracker::new();
        assert!(tracker.record(RedrawReason::UserInput));
        assert!(tracker.record(RedrawReason::UserInput));
        tracker.record(RedrawReason::EguiRepaint);

        assert_eq!(tracker.count_of(RedrawReason::UserInput), 2);
        assert_eq!(tracker.count_of(RedrawReason::EguiRepaint), 1);
        assert_eq!(tracker.count_of(RedrawReason::Animation), 0);
        assert_eq!(tracker.total(), 3);
        assert_eq!(tracker.last_reason(), Some(RedrawReason::EguiRepaint));
    }

    #[test]
    fn reset_clears_everything() {
        let mut tracker = RedrawTracker::new();
        tracker.record(RedrawReason::TextureReady);
        tracker.reset();
        assert_eq!(tracker.total(), 0);
        assert_eq!(tracker.last_reason(), None);
    }

    #[test]
    fn summary_is_deterministic() {
        // ลำดับใน summary ต้องคงที่ทุกครั้ง ไม่งั้น log/status bar จะสลับไปมา
        let mut tracker = RedrawTracker::new();
        tracker.record(RedrawReason::SurfaceRecovery);
        tracker.record(RedrawReason::UserInput);
        let first = tracker.summary();
        for _ in 0..50 {
            assert_eq!(tracker.summary(), first);
        }
        assert_eq!(first, "UserInput=1 SurfaceRecovery=1");
    }

    #[test]
    fn empty_summary_says_so() {
        assert_eq!(RedrawTracker::new().summary(), "ไม่มีการขอวาดเฟรมเลย");
    }
}
