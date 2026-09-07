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

/// ★★★ ยาวเท่าไหร่ถึงเรียกว่าผิดปกติ — **ที่มาของตัวเลขนี้สำคัญกว่าตัวเลข**
///
/// `docs/04 §1` ข้อ 3 บังคับว่า animation ต้องจบใน **≤ 200 ms** ซึ่งที่ 60 fps
/// คือ **12 เฟรม** · ตั้งไว้ที่ 32 จึงอยู่เหนือ animation ที่ถูกต้องอย่างสบาย
/// และต่ำกว่าของจริงที่เคยเจอ (82 และ 94 ครั้งใน 60 วินาที) มาก
///
/// ★ ไม่ได้ตั้งให้ต่ำที่สุดเท่าที่จะทำได้โดยตั้งใจ — ตัวจับที่ร้องบ่อยจนคนชิน
/// คือตัวจับที่ไม่มีใครอ่าน
pub const QUIET_ALARM: u64 = 32;

/// ★★ ลดเพดานลงต่ำกว่า animation ที่ถูกต้อง = **build ไม่ผ่าน** ไม่ใช่เทสต์แดง
///
/// animation ≤ 200 ms ที่ 60 fps คือ 12 เฟรม (`docs/04 §1` ข้อ 3) · ถ้าเพดาน
/// ต่ำกว่านั้น ตัวจับจะร้องตอนทุกอย่างปกติ แล้วไม่มีใครอ่านมันอีกเลย
const _: () = assert!(QUIET_ALARM > 12);

/// ★★★ **ช่วงที่ไม่มีอะไรเกิดขึ้น แต่ยังมีคนขอวาด** — ตัวจับ I-1 ที่รั่วเป็นครั้งคราว
///
/// ## ทำไมต้องมี (`docs/08 §3.9` ข้อ 11 — เขียน 6 ก.ย. 2026)
///
/// เจอ redraw ~1.5 Hz สองครั้งแล้ววัดซ้ำ 13 หน้าต่างไม่เจออีกเลย ·
/// **การวัดด้วยมือจับเหตุการณ์ 2-ใน-15 ไม่ได้** — วัดอีกกี่รอบ "ไม่เจอ" ก็ไม่ได้
/// แปลว่าไม่มี · ต้องเปลี่ยนเป็นตัวนับที่เปิดอยู่ตลอด **พร้อมชื่อคนขอ**
///
/// ## ★★ อะไรรีเซ็ตช่วงนี้ และทำไม
///
/// | เหตุผล | ทำอะไร | ทำไม |
/// |---|---|---|
/// | [`RedrawReason::UserInput`] | **รีเซ็ต** | ผู้ใช้แตะ = ไม่ใช่ idle ตามนิยาม |
/// | [`RedrawReason::TextureReady`] | **รีเซ็ต** | งานจริงเพิ่งเสร็จ = มีอะไรเปลี่ยนจริง · จำนวนถูกจำกัดด้วยจำนวนภาพในคิวอยู่แล้ว |
/// | `Animation` · `EguiRepaint` · `SurfaceRecovery` | **นับเพิ่ม** | ทั้งสามต้องมีขอบเขตตาม `docs/04 §1` — ยาวเมื่อไหร่คือผิด |
///
/// ★★★ **ตัวนี้ไม่ปลุกใครทั้งสิ้น** — มันขยับเฉพาะตอนที่มีคนขอ redraw อยู่แล้ว
/// จึงเป็นไปไม่ได้ที่ตัวจับ I-1 จะกลายเป็นสิ่งที่ละเมิด I-1 เสียเอง
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct QuietStreak {
    len: u64,
    counts: [u64; RedrawReason::ALL.len()],
}

impl QuietStreak {
    /// จำนวนคำขอติดต่อกันในช่วงนี้
    #[must_use]
    pub fn len(&self) -> u64 {
        self.len
    }

    /// ยังไม่มีอะไรน่าสงสัยเลย
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// เหตุผลที่ขอมากที่สุดในช่วงนี้ — `None` ถ้าช่วงว่าง
    ///
    /// ★ นี่คือ **"ใครขอ redraw ครั้งนั้น"** ที่ทำให้ครั้งหน้ามีร่องรอยให้ตาม
    /// แทนที่จะต้องพยายามทำให้อาการเกิดซ้ำ
    #[must_use]
    pub fn worst_reason(&self) -> Option<RedrawReason> {
        RedrawReason::ALL
            .into_iter()
            .filter(|r| self.counts[r.index()] > 0)
            .max_by_key(|r| self.counts[r.index()])
    }

    /// จำนวนคำขอของเหตุผลหนึ่งภายในช่วงนี้
    #[must_use]
    pub fn count_of(&self, reason: RedrawReason) -> u64 {
        self.counts[reason.index()]
    }

    /// ควรส่งเสียงแล้วหรือยัง
    #[must_use]
    pub fn is_alarming(&self) -> bool {
        self.len >= QUIET_ALARM
    }

    /// สรุปเป็นข้อความ — ★ ASCII ล้วนเพราะมันลงทั้ง log และแถบสถานะ (`docs/03 §0`)
    #[must_use]
    pub fn summary(&self) -> String {
        match self.worst_reason() {
            None => "0".to_owned(),
            Some(reason) => format!("{} {reason:?}", self.len),
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
    /// ช่วงเงียบที่กำลังดำเนินอยู่
    quiet: QuietStreak,
    /// ★ ช่วงเงียบที่ยาวที่สุดตลอดอายุโปรเซส — สิ่งที่ log ตอนปิดโปรแกรมรายงาน
    worst: QuietStreak,
    /// ความยาวที่ส่งเสียงไปแล้ว — กันไม่ให้ log ท่วมระหว่างที่อาการยังดำเนินอยู่
    warned_at: u64,
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
        self.note_quiet(reason);
        tracing::trace!(?reason, total = self.total, "redraw requested");
        true
    }

    /// ★★★ อัปเดตช่วงเงียบ — ดู [`QuietStreak`] ว่าอะไรรีเซ็ตและทำไม
    fn note_quiet(&mut self, reason: RedrawReason) {
        if matches!(reason, RedrawReason::UserInput | RedrawReason::TextureReady) {
            self.quiet = QuietStreak::default();
            self.warned_at = 0;
            return;
        }
        self.quiet.len += 1;
        self.quiet.counts[reason.index()] += 1;
        if self.quiet.len > self.worst.len {
            self.worst = self.quiet;
        }
        // ★ ส่งเสียงตอนข้ามเพดานครั้งแรก แล้วทุกครั้งที่ **ยาวเป็นสองเท่า** —
        //   ถ้าร้องทุกครั้ง log จะท่วมด้วยบรรทัดเดียวกันจนกลบสิ่งที่ควรอ่าน
        //   และตัวจับที่ร้องบ่อยจนคนชิน คือตัวจับที่ไม่มีใครอ่าน
        let threshold = if self.warned_at == 0 {
            QUIET_ALARM
        } else {
            self.warned_at * 2
        };
        if self.quiet.len >= threshold {
            self.warned_at = self.quiet.len;
            tracing::warn!(
                quiet = self.quiet.len,
                who = ?self.quiet.worst_reason(),
                "I-1: ขอวาดเฟรมต่อเนื่องทั้งที่ไม่มี input และไม่มี texture ใหม่"
            );
        }
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

    /// ช่วงเงียบที่กำลังดำเนินอยู่ — ดู [`QuietStreak`]
    #[must_use]
    pub fn quiet(&self) -> QuietStreak {
        self.quiet
    }

    /// ★ ช่วงเงียบที่ยาวที่สุดตลอดอายุโปรเซส — **อยู่ใน log ตอนปิดโปรแกรมเสมอ**
    ///
    /// ผู้ใช้ที่เจออาการแล้วส่ง log มาให้ จะพก "มันเคยยาวถึงเท่านี้ และคนขอคือใคร"
    /// มาด้วยโดยไม่ต้องทำอะไรเพิ่ม
    #[must_use]
    pub fn worst_quiet(&self) -> QuietStreak {
        self.worst
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

    // ---------- ★★★ ตัวจับ I-1 ที่รั่วเป็นครั้งคราว (`docs/08 §3.9` ข้อ 11) ----------

    /// ★★★ **คนที่ขอวาดทั้งที่ไม่มีอะไรเกิดขึ้น ต้องถูกจดชื่อไว้**
    ///
    /// นี่คือสิ่งที่ทำให้ครั้งหน้าที่อาการเกิด **มีร่องรอยให้ตาม** แทนที่จะต้อง
    /// พยายามทำให้มันเกิดซ้ำ (ซึ่งลองแล้ว 13 หน้าต่างไม่สำเร็จ)
    #[test]
    fn asking_to_draw_while_nothing_happened_is_counted_and_named() {
        let mut tracker = RedrawTracker::new();
        assert!(tracker.quiet().is_empty());

        for _ in 0..40 {
            tracker.record(RedrawReason::EguiRepaint);
        }
        assert_eq!(tracker.quiet().len(), 40);
        assert_eq!(
            tracker.quiet().worst_reason(),
            Some(RedrawReason::EguiRepaint),
            "ตัวนับต้องบอกได้ว่า **ใคร** ขอ ไม่ใช่แค่ว่ามีคนขอ"
        );
        assert!(tracker.quiet().is_alarming());
        assert_eq!(tracker.quiet().summary(), "40 EguiRepaint");
    }

    /// ★★ สิ่งที่รีเซ็ตช่วงเงียบ — **ผู้ใช้แตะ** และ **งานจริงเสร็จ**
    ///
    /// ถ้า `TextureReady` ไม่รีเซ็ต การเปิด board 3,072 ใบจะสร้างช่วงเงียบยาว
    /// เป็นพัน แล้วเสียงเตือนจะดังทุกครั้งที่เปิดไฟล์ใหญ่ — ตัวจับที่ร้องตอน
    /// ทุกอย่างปกติคือตัวจับที่ไม่มีใครอ่าน
    #[test]
    fn real_work_and_real_input_both_end_the_quiet_stretch() {
        for breaker in [RedrawReason::UserInput, RedrawReason::TextureReady] {
            let mut tracker = RedrawTracker::new();
            for _ in 0..5 {
                tracker.record(RedrawReason::EguiRepaint);
            }
            assert_eq!(tracker.quiet().len(), 5);
            tracker.record(breaker);
            assert!(tracker.quiet().is_empty(), "{breaker:?} ต้องจบช่วงเงียบ");
        }
    }

    /// ★★★ **ช่วงที่ยาวที่สุดต้องรอดมาถึงตอนปิดโปรแกรม**
    ///
    /// อาการที่เกิดแล้วหายเองจะไม่มีใครเห็น ถ้าเก็บแต่ค่าปัจจุบัน — ตอนผู้ใช้
    /// ปิดโปรแกรมแล้วส่ง log มา ค่ามันกลับเป็นศูนย์ไปนานแล้ว
    #[test]
    fn the_worst_stretch_survives_after_it_ends() {
        let mut tracker = RedrawTracker::new();
        for _ in 0..17 {
            tracker.record(RedrawReason::Animation);
        }
        tracker.record(RedrawReason::UserInput);
        for _ in 0..3 {
            tracker.record(RedrawReason::EguiRepaint);
        }

        assert_eq!(tracker.quiet().len(), 3, "ค่าปัจจุบันคือช่วงล่าสุด");
        assert_eq!(tracker.worst_quiet().len(), 17, "ช่วงที่ยาวที่สุดต้องไม่หาย");
        assert_eq!(
            tracker.worst_quiet().worst_reason(),
            Some(RedrawReason::Animation)
        );
    }

    /// ★ เพดานเสียงเตือนต้องอยู่เหนือ animation ที่ถูกต้อง (`docs/04 §1` ข้อ 3)
    ///
    /// 200 ms ที่ 60 fps = 12 เฟรม · ถ้าเพดานต่ำกว่านั้น animation ปกติจะร้อง
    #[test]
    fn the_alarm_sits_above_a_legitimate_animation() {
        // ★ ตัวเลขถูกคุมด้วย `const _` ข้างบนตั้งแต่ตอนคอมไพล์ — ที่นี่ตรวจ
        //   **พฤติกรรม** ว่า animation ยาวเต็มโควตาแล้วยังไม่ร้อง
        let mut tracker = RedrawTracker::new();
        for _ in 0..12 {
            tracker.record(RedrawReason::Animation);
        }
        assert!(!tracker.quiet().is_alarming());
    }
}
