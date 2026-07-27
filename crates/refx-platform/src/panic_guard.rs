//! ธงบอก panic hook ว่า "panic นี้ถูกดักไว้แล้ว ไม่ต้องตื่นตูม"
//!
//! ปัญหาที่แก้: `catch_unwind` **ไม่ได้ปิดปาก panic hook** — hook ทำงานก่อน unwind เสมอ
//! โฟลเดอร์ที่มีไฟล์เสีย 500 ไฟล์จึงได้ backtrace 500 ชุดลง log
//! → log หมุนทะลุ 5 MB → **ทับ crash log จริงหายหมด**
//! ซึ่งขัด docs/08 §5 ที่ว่า crash log คือสิ่งเดียวที่ผู้ใช้มีให้ส่งเวลารายงานปัญหา
//!
//! อยู่ใน `refx-platform` เพราะเป็น leaf ที่ทุก crate depend ได้ (ARCHITECTURE §2)
//! — `refx-asset` เป็นคนตั้งธง ส่วน panic hook ใน `refx-app` เป็นคนอ่าน
//!
//! เป็น **thread-local** จึงไม่กระทบ panic ของเธรดอื่นที่เกิดขึ้นพร้อมกัน
//!
//! spec: docs/06-security.md §3

use std::cell::RefCell;

thread_local! {
    /// ป้ายของงานที่กำลังอยู่ในเกราะ (ปกติคือชื่อไฟล์) — `None` = ไม่ได้อยู่ในเกราะ
    static GUARD_LABEL: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// ตัวถือขอบเขตของเกราะ — ล้างธงให้เองตอน drop
///
/// ต้องล้างแบบ RAII เพราะ **บล็อกที่ถือธงอยู่จะ panic** ซึ่งกระโดดข้ามโค้ดปกติไป
/// ถ้าล้างด้วยมือจะไม่มีวันได้ล้างจริง แล้วเธรดนั้นจะเงียบ panic ตลอดกาล
#[must_use = "ต้องผูกไว้กับตัวแปร ไม่งั้นธงจะถูกล้างทันที"]
pub struct PanicGuardScope {
    previous: Option<String>,
}

impl Drop for PanicGuardScope {
    fn drop(&mut self) {
        GUARD_LABEL.with(|slot| {
            *slot.borrow_mut() = self.previous.take();
        });
    }
}

/// เข้าเกราะ — panic ที่เกิดหลังจากนี้จนกว่าจะ drop ถือว่า "คาดไว้แล้ว"
///
/// `label` ควรเป็นชื่อไฟล์ (ไม่ใช่ path เต็ม — docs/08 §5 ห้ามมี path เต็มใน log)
pub fn enter(label: impl Into<String>) -> PanicGuardScope {
    let label = label.into();
    let previous = GUARD_LABEL.with(|slot| slot.borrow_mut().replace(label));
    PanicGuardScope { previous }
}

/// อยู่ในเกราะอยู่หรือไม่ (เรียกจาก panic hook)
#[must_use]
pub fn is_active() -> bool {
    GUARD_LABEL.with(|slot| slot.borrow().is_some())
}

/// ป้ายของเกราะปัจจุบัน — `None` ถ้าไม่ได้อยู่ในเกราะ
#[must_use]
pub fn current_label() -> Option<String> {
    GUARD_LABEL.with(|slot| slot.borrow().clone())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn flag_is_off_by_default() {
        assert!(!is_active());
        assert_eq!(current_label(), None);
    }

    #[test]
    fn enter_sets_and_drop_clears() {
        {
            let _scope = enter("cat.png");
            assert!(is_active());
            assert_eq!(current_label().as_deref(), Some("cat.png"));
        }
        assert!(!is_active(), "ต้องล้างธงหลังออกจากขอบเขต");
    }

    /// ★ สำคัญ: บล็อกที่ถือธงอยู่จะ panic เป็นเรื่องปกติ
    /// ธงต้องถูกล้างระหว่าง unwind ไม่งั้นเธรดนั้นจะเงียบ panic ตลอดกาล
    #[test]
    fn flag_is_cleared_even_when_panicking() {
        let result = std::panic::catch_unwind(|| {
            let _scope = enter("broken.jpg");
            assert!(is_active());
            panic!("จำลอง decoder พัง");
        });
        assert!(result.is_err());
        assert!(!is_active(), "panic แล้วธงต้องถูกล้างด้วย");
    }

    #[test]
    fn nested_scopes_restore_previous() {
        let _outer = enter("outer.png");
        {
            let _inner = enter("inner.png");
            assert_eq!(current_label().as_deref(), Some("inner.png"));
        }
        assert_eq!(current_label().as_deref(), Some("outer.png"));
    }

    /// ★ สิ่งที่ panic hook ของ P0-9 พึ่งพาจริง ๆ:
    /// hook ทำงาน **ก่อน** unwind จึงต้องเห็นธงตอนที่มันยังตั้งอยู่
    ///
    /// ถ้าข้อนี้ไม่จริง โฟลเดอร์ที่มีไฟล์เสีย 500 ไฟล์จะได้ backtrace 500 ชุด
    /// จนหมุนทับ crash log จริงหายหมด (docs/06 §3)
    #[test]
    fn panic_hook_sees_label_while_unwinding() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(None::<Option<String>>));
        let recorder = std::sync::Arc::clone(&seen);

        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |_| {
            if let Ok(mut slot) = recorder.lock() {
                *slot = Some(current_label());
            }
        }));

        let result = std::panic::catch_unwind(|| {
            let _scope = enter("เสียหาย.png");
            panic!("จำลอง decoder พัง");
        });

        std::panic::set_hook(previous); // คืนของเดิมก่อนออกเสมอ
        assert!(result.is_err());

        let observed = seen.lock().unwrap().clone();
        assert_eq!(
            observed,
            Some(Some("เสียหาย.png".to_owned())),
            "hook ต้องเห็นชื่อไฟล์ตอน panic ไม่งั้นมันจะเขียน backtrace เต็ม log"
        );
        assert!(!is_active(), "ออกมาแล้วธงต้องถูกล้าง");
    }

    /// thread-local จริง — เธรดอื่นต้องไม่เห็นธงของเรา
    /// ไม่งั้น panic ของ main thread จะถูกกลืนเพราะ worker บังเอิญถือธงอยู่
    #[test]
    fn flag_does_not_leak_across_threads() {
        let _scope = enter("mine.png");
        let seen = std::thread::spawn(is_active).join().unwrap();
        assert!(!seen, "ธงรั่วไปเธรดอื่น");
    }
}
