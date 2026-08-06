//! ก๊อปข้อความขึ้น clipboard **บนเธรดชั่วคราว** (P2-10)
//!
//! ★★ **ทำไมไม่ทำบน UI thread และไม่ทำบน decode worker**
//!
//! | ที่ | ทำไมไม่เอา |
//! |---|---|
//! | UI thread | ผิด I-2 — บน Windows clipboard เป็น **global lock ของทั้งระบบ** `OpenClipboard` รอเจ้าของเดิมปล่อยได้นานเป็นวินาที และเจ้าของอาจเป็นโปรแกรมที่กำลังค้าง · การก๊อป hex หนึ่งบรรทัดจะทำให้ทั้งแอปค้าง ทั้งที่ผู้ใช้แค่จิ้มดูสี |
//! | decode worker | ผิดเชิงความหมาย (มันคือ pool สำหรับ *ถอดรหัสภาพ*) และงานก๊อปจะไปต่อคิว **หลัง** decode ที่กินครั้งละ ~150 ms — ผู้ใช้กด Ctrl+V ในโปรแกรมอื่นก่อนที่ค่าจะถูกเขียนจริง |
//! | เธรดชั่วคราว | การก๊อปเป็นงานที่ผู้ใช้สั่งเอง เกิดนาน ๆ ครั้ง และ **ไม่ต้องรอผล** · ต้นทุนคือสร้างเธรดครั้งละ ~50 µs ซึ่งถูกกว่าการมีเธรดค้างไว้ตลอดอายุโปรแกรมเพื่องานที่นาทีละครั้ง |
//!
//! ★ **ล้มเหลว = `warn!` + ข้อความบน status bar ห้ามเด้ง dialog** — ก๊อปไม่ติด
//! คือความรำคาญ ไม่ใช่เหตุขัดข้อง (ต่างจาก save ล้มซึ่งคืองานหาย)
//!
//! spec: docs/03 §2 (`I` = picker + คัดลอก hex)

use std::sync::{Arc, Mutex};

use refx_core::clipboard::{ClipboardError, ClipboardWriter};

/// ช่องฝากงานก๊อป + ธงว่ามีเธรดทำงานอยู่ — **อยู่ใต้ล็อกเดียวกันโดยตั้งใจ**
///
/// ★★ ถ้าแยกธงออกไปเป็น `AtomicBool` จะมีช่องแข่งกันที่ทำให้ **ข้อความหายเงียบ ๆ**:
/// เธรดเห็นช่องว่าง → กำลังจะลงธง → UI ใส่ข้อความใหม่แล้วเห็นธงยังตั้งอยู่ →
/// ไม่ spawn → เธรดลงธงแล้วจบ → **ไม่มีใครเขียนข้อความนั้นเลย** ผู้ใช้จะเห็นสีใหม่
/// บน status bar แต่ได้สีเก่าตอนวาง ซึ่งแยกไม่ออกจากบั๊กของโปรแกรมอื่น
#[derive(Debug, Default)]
struct Slot {
    /// ข้อความล่าสุดที่รอเขียน — **ตัวใหม่ทับตัวเก่าเสมอ**
    ///
    /// จิ้มรัว ๆ แล้วต้องได้สีของ**ครั้งสุดท้าย** ไม่ใช่ครั้งแรกที่ชิงเธรดได้
    pending: Option<String>,
    /// มีเธรดกำลังไล่เก็บช่องนี้อยู่หรือไม่
    running: bool,
}

/// ผลของการก๊อปครั้งล่าสุดที่ **ล้มเหลว** — `None` = ยังไม่มีอะไรผิด
///
/// สำเร็จไม่ต้องรายงาน: status bar บอก hex อยู่แล้ว การขึ้นว่า "ก๊อปแล้ว"
/// ทุกครั้งคือเสียงรบกวนที่ผู้ใช้จะเลิกอ่านภายในสิบครั้งแรก
type LastError = Arc<Mutex<Option<ClipboardError>>>;

/// ตัวก๊อปข้อความขึ้น clipboard
#[derive(Clone)]
pub struct Copier {
    writer: Arc<dyn ClipboardWriter>,
    slot: Arc<Mutex<Slot>>,
    error: LastError,
    /// ปลุก UI ให้วาดใหม่เมื่อมีอะไรต้องรายงาน
    ///
    /// ★ ต้องมี ไม่งั้นข้อความ error จะไปนอนรออยู่เฉย ๆ จนกว่าผู้ใช้จะบังเอิญ
    /// ขยับเมาส์ — แอปหลับสนิทตอน idle (I-1) ไม่มีเฟรมไหนมาอ่านมันเอง
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for Copier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Copier")
            .field("busy", &self.is_busy())
            .finish_non_exhaustive()
    }
}

impl Copier {
    /// ตัวก๊อปที่เขียนผ่าน `writer`
    #[must_use]
    pub fn new(writer: Arc<dyn ClipboardWriter>) -> Self {
        Self {
            writer,
            slot: Arc::new(Mutex::new(Slot::default())),
            error: Arc::new(Mutex::new(None)),
            wake: None,
        }
    }

    /// เสียบตัวปลุก event loop — เรียกหลังหน้าต่างพร้อม
    pub fn set_waker(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.wake = Some(wake);
    }

    /// กำลังก๊อปอยู่หรือไม่ (สำหรับเทสต์และ debug)
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.slot.lock().is_ok_and(|slot| slot.running)
    }

    /// ★ ขอให้ก๊อป `text` — คืนทันที ไม่รอผล
    ///
    /// ถ้ามีเธรดทำงานอยู่แล้วจะ **ไม่ spawn ใหม่** แต่ฝากข้อความไว้ให้เธรดนั้นเก็บไป
    /// (ธงกันซ้อน) — ผลคือมีเธรดก๊อปได้ทีละตัวเสมอ ไม่ว่าผู้ใช้จะจิ้มเร็วแค่ไหน
    pub fn copy(&self, text: String) {
        let Ok(mut slot) = self.slot.lock() else {
            // ล็อกพังแปลว่ามีเธรด panic ค้างไว้ — ก๊อปไม่ได้ก็ไม่ใช่เหตุให้ล้มทั้งแอป
            tracing::warn!("the clipboard copy slot is poisoned; skipping this copy");
            return;
        };
        slot.pending = Some(text);
        if slot.running {
            return; // เธรดที่วิ่งอยู่จะเก็บตัวใหม่ไปเอง
        }
        slot.running = true;
        drop(slot);

        let writer = Arc::clone(&self.writer);
        let queue = Arc::clone(&self.slot);
        let error = Arc::clone(&self.error);
        let wake = self.wake.clone();
        // ★ spawn แล้วปล่อย — ไม่เก็บ `JoinHandle` เพราะไม่มีใครรอผล
        //   เธรดจบเองเมื่อช่องว่าง · ตอนปิดโปรแกรมมันอาจยังเขียนค้างอยู่ ซึ่ง
        //   ยอมรับได้: การก๊อปไม่ใช่ข้อมูลของผู้ใช้ที่ I-3 ต้องกัน
        std::thread::spawn(move || {
            loop {
                let next = {
                    let Ok(mut slot) = queue.lock() else { return };
                    match slot.pending.take() {
                        Some(text) => text,
                        None => {
                            // ★ ลงธง**ใต้ล็อกเดียวกับที่เพิ่งเห็นว่าช่องว่าง**
                            //   ถ้าปล่อยล็อกก่อนลงธง ข้อความที่มาแทรกตรงกลางจะหาย
                            slot.running = false;
                            return;
                        }
                    }
                };
                if let Err(err) = writer.write_text(&next) {
                    tracing::warn!(%err, "cannot copy to the clipboard");
                    if let Ok(mut last) = error.lock() {
                        *last = Some(err);
                    }
                    if let Some(wake) = wake.as_ref() {
                        wake();
                    }
                }
            }
        });
    }

    /// เก็บ error ล่าสุดไปแสดง — **เก็บแล้วหาย** ข้อความค้างจากเมื่อกี้ไม่ควรขึ้นซ้ำ
    #[must_use]
    pub fn take_error(&self) -> Option<ClipboardError> {
        self.error.lock().ok()?.take()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// ตัวเขียนปลอมที่จดทุกอย่างที่ถูกเขียน และหน่วงได้เพื่อบังคับให้เกิดการซ้อน
    #[derive(Debug, Default)]
    struct Recorder {
        written: Mutex<Vec<String>>,
        delay: std::time::Duration,
        fail: bool,
    }

    impl ClipboardWriter for Recorder {
        fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
            std::thread::sleep(self.delay);
            if self.fail {
                return Err(ClipboardError::Busy);
            }
            self.written.lock().unwrap().push(text.to_owned());
            Ok(())
        }
    }

    fn settle(copier: &Copier) {
        // ตาข่ายจับ "ค้างจริง" ไม่ใช่การวัดความเร็ว — ตั้งหลวม ๆ ตาม HANDOFF §2.2c
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while copier.is_busy() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!copier.is_busy(), "เธรดก๊อปไม่จบภายใน 30 วินาที");
    }

    #[test]
    fn a_copy_actually_reaches_the_writer() {
        let recorder = Arc::new(Recorder::default());
        let copier = Copier::new(Arc::clone(&recorder) as Arc<dyn ClipboardWriter>);
        copier.copy("#E23A2F".to_owned());
        settle(&copier);
        assert_eq!(recorder.written.lock().unwrap().as_slice(), ["#E23A2F"]);
    }

    /// ★★ จิ้มรัว ๆ ต้องได้สีของ **ครั้งสุดท้าย** เสมอ
    ///
    /// ธงกันซ้อนแบบง่าย ๆ (ถ้ายังก๊อปอยู่ก็ข้ามไปเลย) จะทำให้ clipboard ค้างอยู่ที่
    /// สีเก่า ขณะที่ status bar โชว์สีใหม่ — ผู้ใช้วางแล้วได้สีผิดโดยไม่มีอะไรเตือน
    /// จึงฝากข้อความไว้ให้เธรดที่วิ่งอยู่เก็บไปแทนการทิ้ง
    #[test]
    fn the_last_colour_asked_for_is_the_one_that_lands() {
        let recorder = Arc::new(Recorder {
            delay: std::time::Duration::from_millis(40),
            ..Recorder::default()
        });
        let copier = Copier::new(Arc::clone(&recorder) as Arc<dyn ClipboardWriter>);

        copier.copy("#111111".to_owned());
        for shade in ["#222222", "#333333", "#444444"] {
            copier.copy(shade.to_owned());
        }
        settle(&copier);

        let written = recorder.written.lock().unwrap().clone();
        assert_eq!(
            written.last().map(String::as_str),
            Some("#444444"),
            "ตัวสุดท้ายต้องเป็นตัวที่ผู้ใช้ขอล่าสุด แต่ได้ {written:?}"
        );
    }

    /// ★ ธงกันซ้อนต้องทำงานจริง — ไม่ใช่ spawn เธรดหนึ่งตัวต่อการจิ้มหนึ่งครั้ง
    ///
    /// วัดด้วย **จำนวนครั้งที่เขียนจริง** ไม่ใช่จับเวลา (docs/08 §3.9 ข้อ 5b):
    /// ถ้าไม่มีธง ทุก `copy()` จะได้เธรดของตัวเองแล้วเขียนครบทั้ง 50 ครั้ง
    #[test]
    fn queued_copies_collapse_instead_of_spawning_a_thread_each() {
        let recorder = Arc::new(Recorder {
            delay: std::time::Duration::from_millis(20),
            ..Recorder::default()
        });
        let copier = Copier::new(Arc::clone(&recorder) as Arc<dyn ClipboardWriter>);
        for i in 0..50 {
            copier.copy(format!("#{i:06X}"));
        }
        settle(&copier);

        let count = recorder.written.lock().unwrap().len();
        assert!(count < 50, "เขียนไป {count} ครั้งจาก 50 — ธงกันซ้อนไม่ทำงาน");
        assert!(count >= 1);
    }

    /// ★ ล้มเหลวต้องเก็บไว้ให้ UI เอาไปบอก **และปลุกให้มาอ่าน**
    ///
    /// ถ้าไม่ปลุก ข้อความจะนอนรออยู่จนกว่าผู้ใช้จะบังเอิญขยับเมาส์
    /// เพราะแอปหลับสนิทตอน idle (I-1)
    #[test]
    fn a_failed_copy_is_reported_once_and_wakes_the_ui() {
        let recorder = Arc::new(Recorder {
            fail: true,
            ..Recorder::default()
        });
        let woke = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut copier = Copier::new(Arc::clone(&recorder) as Arc<dyn ClipboardWriter>);
        let counter = Arc::clone(&woke);
        copier.set_waker(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }));

        copier.copy("#ABCDEF".to_owned());
        settle(&copier);

        assert!(copier.take_error().is_some(), "ต้องเก็บ error ไว้ให้ UI");
        assert!(copier.take_error().is_none(), "เก็บแล้วต้องหาย ไม่ใช่ขึ้นซ้ำทุกเฟรม");
        assert!(
            woke.load(std::sync::atomic::Ordering::Relaxed) >= 1,
            "ต้องปลุก UI ให้มาอ่าน"
        );
    }

    /// สำเร็จต้อง **ไม่** รายงานอะไร — ขึ้นว่า "ก๊อปแล้ว" ทุกครั้งคือเสียงรบกวน
    #[test]
    fn a_successful_copy_reports_nothing() {
        let recorder = Arc::new(Recorder::default());
        let copier = Copier::new(Arc::clone(&recorder) as Arc<dyn ClipboardWriter>);
        copier.copy("#000000".to_owned());
        settle(&copier);
        assert!(copier.take_error().is_none());
    }
}
