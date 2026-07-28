//! กันเปิดโปรแกรมซ้ำสองหน้าต่างพร้อมกัน
//!
//! เหตุผลที่ต้องมีตั้งแต่ P0: `cache.sqlite` เขียนจากโปรเซสเดียวเท่านั้น
//! ถ้าเปิดซ้ำสองตัวแล้วเขียน DB พร้อมกัน = เสี่ยงข้อมูลเสีย (ขัด I-3)
//!
//! วิธี: จับ **advisory lock ของ OS** บนไฟล์ใน cache dir
//! ข้อดีเทียบกับการเช็ค "ไฟล์มีอยู่ไหม" แบบเดิม ๆ คือ OS ปลดล็อกให้เองเมื่อโปรเซสตาย
//! ต่อให้โปรแกรม crash หรือโดน End Task ก็ไม่มี lock ค้างให้ผู้ใช้ไปลบเอง

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// จับ lock ของอินสแตนซ์เดียวไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum SingleInstanceError {
    /// มี RefX เปิดอยู่แล้ว
    #[error("another RefX instance already holds the lock")]
    AlreadyRunning,

    /// เปิด/สร้างไฟล์ lock ไม่ได้
    #[error("cannot create lock file {path}: {source}")]
    Io {
        /// ไฟล์ lock ที่มีปัญหา
        path: PathBuf,
        /// สาเหตุจากระบบไฟล์
        source: std::io::Error,
    },
}

/// ตัวยึด lock — **ต้องถือไว้ตลอดอายุโปรแกรม**
///
/// ปล่อย (drop) เมื่อไหร่ = ปลดล็อกทันที จึงต้องเก็บไว้ใน `main()`
/// ห้ามเขียน `let _ = SingleInstance::acquire(..)` เพราะจะ drop ทิ้งทันที
#[derive(Debug)]
#[must_use = "ต้องถือ SingleInstance ไว้ตลอดอายุโปรแกรม ถ้า drop จะปลดล็อกทันที"]
pub struct SingleInstance {
    // ถือ File ไว้เฉย ๆ ให้ lock อยู่ต่อ — drop แล้ว OS ปลดล็อกให้เอง
    _file: File,
    path: PathBuf,
}

impl SingleInstance {
    /// ชื่อไฟล์ lock ภายใน cache dir
    pub const LOCK_FILE_NAME: &'static str = "refx.lock";

    /// พยายามจับ lock ใน `cache_dir`
    ///
    /// คืน [`SingleInstanceError::AlreadyRunning`] ถ้ามีโปรเซสอื่นถืออยู่
    pub fn acquire(cache_dir: &Path) -> Result<Self, SingleInstanceError> {
        let path = cache_dir.join(Self::LOCK_FILE_NAME);

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false) // ห้าม truncate ก่อนได้ lock ไม่งั้นไปลบ PID ของตัวที่ถืออยู่
            .open(&path)
            .map_err(|source| SingleInstanceError::Io {
                path: path.clone(),
                source,
            })?;

        match file.try_lock() {
            Ok(()) => {}
            // มีคนถืออยู่ = อีกอินสแตนซ์กำลังรัน
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(SingleInstanceError::AlreadyRunning);
            }
            Err(std::fs::TryLockError::Error(source)) => {
                return Err(SingleInstanceError::Io { path, source });
            }
        }

        // ได้ lock แล้วค่อยเขียน PID — ไว้ให้คนดีบักรู้ว่าใครถืออยู่
        // เขียนไม่สำเร็จไม่ถือว่าล้มเหลว เพราะ lock (ซึ่งเป็นของจริง) ได้มาแล้ว
        let mut writer = &file;
        if let Err(err) = writer
            .set_len(0)
            .and_then(|()| write!(writer, "{}", std::process::id()))
        {
            tracing::warn!(?err, "เขียน PID ลงไฟล์ lock ไม่ได้ (ไม่กระทบการทำงาน)");
        }

        tracing::debug!(path = %path.display(), "จับ lock อินสแตนซ์เดียวสำเร็จ");
        Ok(Self { _file: file, path })
    }

    /// ตำแหน่งไฟล์ lock ที่ถืออยู่
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// สร้างโฟลเดอร์ชั่วคราวเฉพาะเทสต์ (ไม่พึ่ง crate ภายนอก)
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("refx-test-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn second_acquire_is_rejected() {
        let dir = temp_dir("single");
        let _first = SingleInstance::acquire(&dir).expect("ตัวแรกต้องจับ lock ได้");

        let second = SingleInstance::acquire(&dir);
        assert!(
            matches!(second, Err(SingleInstanceError::AlreadyRunning)),
            "ตัวที่สองต้องโดนปฏิเสธ แต่ได้ {second:?}"
        );
    }

    #[test]
    fn lock_is_released_after_drop() {
        let dir = temp_dir("release");
        {
            let _guard = SingleInstance::acquire(&dir).expect("ตัวแรกต้องจับ lock ได้");
        } // drop ที่นี่

        // ปล่อยแล้วต้องจับใหม่ได้ ไม่งั้นผู้ใช้เปิดโปรแกรมซ้ำไม่ได้ตลอดกาล
        let _second = SingleInstance::acquire(&dir).expect("หลัง drop ต้องจับ lock ใหม่ได้");
    }
}
