//! ระบบ log + panic hook + crash log
//!
//! ข้อกำหนดจาก docs/08 §5:
//!   * log ไปที่ `<cache_dir>/logs/` **หมุนไฟล์ที่ 5 MB เก็บ 3 ไฟล์**
//!   * ห้ามมี path เต็มใน log ระดับ info (มีชื่อผู้ใช้อยู่ในนั้น)
//!   * crash log คือสิ่งเดียวที่ผู้ใช้มีให้ส่งเวลารายงานปัญหา — ห้ามหาย
//!
//! > **ข้อจำกัดที่เจอตอน implement:** `tracing-appender` 0.2 หมุนไฟล์ตาม **เวลา**
//! > เท่านั้น (minutely/hourly/daily) ไม่มีโหมดหมุนตามขนาด
//! > จึงเขียน [`SizeRotatingWriter`] เองเพื่อให้ตรงกับ spec
//! > (ถ้าใช้ daily แทน ไฟล์เดียวอาจโตเป็น GB ได้ในวันเดียวเมื่อมี error วนซ้ำ)

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// ตั้งค่า log ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    /// เปิดไฟล์ log ไม่ได้
    #[error("เปิดไฟล์ log ที่ {path} ไม่ได้: {source}\nโปรแกรมยังทำงานต่อได้ แต่จะไม่มี log ไว้ตรวจปัญหา")]
    Open {
        /// ไฟล์ที่เปิดไม่ได้
        path: PathBuf,
        /// สาเหตุ
        source: io::Error,
    },
}

/// ชื่อไฟล์ log หลัก
const LOG_FILE: &str = "refx.log";
/// ขนาดสูงสุดก่อนหมุนไฟล์ (docs/08 §5)
const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// เก็บไฟล์เก่ากี่ไฟล์ (docs/08 §5)
const KEEP_FILES: usize = 3;

/// ตัวเขียน log ที่หมุนไฟล์เมื่อโตเกิน [`MAX_BYTES`]
///
/// หมุนแบบ `refx.log` → `refx.log.1` → `refx.log.2` แล้วทิ้งตัวที่เกิน [`KEEP_FILES`]
///
/// ★ ตัวนี้ถูกห่อด้วย `tracing_appender::non_blocking` อีกชั้น จึงเขียนดิสก์
/// อยู่บน worker thread ไม่ใช่ UI thread (I-2)
struct SizeRotatingWriter {
    dir: PathBuf,
    file: Option<File>,
    written: u64,
}

impl SizeRotatingWriter {
    fn new(dir: &Path) -> Result<Self, LogError> {
        let mut writer = Self {
            dir: dir.to_path_buf(),
            file: None,
            written: 0,
        };
        writer.open_current()?;
        Ok(writer)
    }

    fn current_path(&self) -> PathBuf {
        self.dir.join(LOG_FILE)
    }

    fn open_current(&mut self) -> Result<(), LogError> {
        let path = self.current_path();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| LogError::Open {
                path: path.clone(),
                source,
            })?;
        // นับต่อจากของเดิม ไม่ใช่เริ่มศูนย์ ไม่งั้นไฟล์จะโตเกินเพดานหลังเปิดโปรแกรมซ้ำ ๆ
        self.written = file.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = Some(file);
        Ok(())
    }

    /// เลื่อนไฟล์เก่าลงหนึ่งขั้นแล้วเปิดไฟล์ใหม่
    ///
    /// ทุก error ตรงนี้ถูกกลืน — log หมุนไม่ได้ไม่ควรทำให้โปรแกรมล้ม
    fn rotate(&mut self) {
        self.file = None; // ปิดก่อน ไม่งั้น Windows ไม่ให้ rename

        // ทิ้งตัวที่เก่าที่สุด แล้วเลื่อนที่เหลือลง
        let oldest = self.dir.join(format!("{LOG_FILE}.{KEEP_FILES}"));
        let _ = std::fs::remove_file(&oldest);

        for index in (1..KEEP_FILES).rev() {
            let from = self.dir.join(format!("{LOG_FILE}.{index}"));
            let to = self.dir.join(format!("{LOG_FILE}.{}", index + 1));
            let _ = std::fs::rename(from, to);
        }
        let _ = std::fs::rename(self.current_path(), self.dir.join(format!("{LOG_FILE}.1")));

        if let Err(err) = self.open_current() {
            eprintln!("หมุนไฟล์ log ไม่สำเร็จ: {err}");
        }
    }
}

impl Write for SizeRotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written + buf.len() as u64 > MAX_BYTES {
            self.rotate();
        }
        let Some(file) = self.file.as_mut() else {
            // เปิดไฟล์ไม่ได้ — บอกว่าเขียนแล้วเพื่อไม่ให้ tracing วนลองใหม่ไม่จบ
            return Ok(buf.len());
        };
        let n = file.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

/// ต้องถือไว้ตลอดอายุโปรแกรม — drop แล้ว log ที่ค้างใน buffer จะหาย
#[must_use = "ต้องถือ LogGuard ไว้ถึงจบ main ไม่งั้น log ที่ค้างอยู่จะหาย"]
pub struct LogGuard {
    _worker: WorkerGuard,
}

/// เริ่มระบบ log — เขียน **ลงไฟล์อย่างเดียว**
///
/// ไม่เขียน stderr ด้วยเพราะ (ก) release เป็น `windows_subsystem = "windows"`
/// ไม่มี console ให้เขียนอยู่แล้ว และ (ข) การเขียน stderr เกิดบน UI thread
/// ซึ่งขัด I-2 — ดู log ระหว่างพัฒนาได้จาก `<cache_dir>/logs/refx.log`
///
/// คืน [`LogGuard`] ที่ต้องถือไว้ถึงจบ `main`
pub fn init(log_dir: &Path) -> Result<LogGuard, LogError> {
    let writer = SizeRotatingWriter::new(log_dir)?;
    // non_blocking = เขียนดิสก์บน worker thread ไม่บล็อก UI (I-2)
    let (non_blocking, worker) = tracing_appender::non_blocking(writer);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // ไฟล์ไม่เอาสี ไม่งั้นได้ escape code เต็มไฟล์จนอ่านไม่ออก
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    Ok(LogGuard { _worker: worker })
}

/// panic บนเธรดนี้ควรเด้ง dialog ให้ผู้ใช้เห็นไหม
///
/// ★ I-7: panic บน worker (เช่น decode ภาพเสีย) **ห้าม** เด้ง dialog
/// เพราะภาพเสีย 1 ไฟล์ต้องไม่รบกวนผู้ใช้ — งานนั้นถูก `catch_unwind` ดักไว้แล้ว
/// ขึ้นสถานะ "โหลดไม่ได้" แทน
///
/// dialog แสดงเฉพาะ panic บน main thread ซึ่งแปลว่าโปรแกรมไปต่อไม่ได้จริง ๆ
fn should_show_dialog(thread_name: Option<&str>) -> bool {
    thread_name == Some("main")
}

/// ดึงข้อความจาก payload ของ panic (เป็นได้ทั้ง `&str` และ `String`)
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "ไม่มีข้อความ".to_owned())
}

/// ติดตั้ง panic hook ที่เขียน log + แสดง dialog
///
/// **ต้องเรียกหลัง [`init`]** ไม่งั้น panic ที่เกิดก่อนหน้าจะไม่ถูกบันทึก
pub fn install_panic_hook(log_dir: &Path) {
    let log_path = log_dir.join(LOG_FILE);
    let previous = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map_or_else(|| "ไม่ทราบตำแหน่ง".to_owned(), ToString::to_string);

        let message = panic_message(info.payload());

        // ★ panic ที่ถูกดักไว้แล้ว (decoder ภาพเสีย) — บันทึกบรรทัดเดียวพอ
        //   ถ้าเขียน backtrace ทุกครั้ง โฟลเดอร์ที่มีไฟล์เสีย 500 ไฟล์จะหมุน log
        //   ทะลุ 5 MB จน crash log จริงหายหมด (docs/06 §3)
        if let Some(label) = refx_platform::panic_guard::current_label() {
            tracing::warn!(
                file = label,
                message,
                "ถอดรหัสภาพไม่สำเร็จ (ตัวถอดรหัส panic แต่ถูกดักไว้แล้ว) — ข้ามไฟล์นี้"
            );
            return; // ไม่เรียก hook เดิม ไม่เด้ง dialog ไม่เขียน backtrace
        }

        let backtrace = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current();
        let thread_name = thread.name().map(ToOwned::to_owned);

        tracing::error!(
            location,
            message,
            thread = thread_name.as_deref().unwrap_or("ไม่มีชื่อ"),
            "โปรแกรมพัง (panic)\n{backtrace}"
        );

        if should_show_dialog(thread_name.as_deref()) {
            // TODO(P4-3): เขียน journal ก่อนตาย เพื่อให้กู้งานได้ตอนเปิดใหม่
            refx_platform::dialog::show_crash_dialog(&log_path);
        }

        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    // เทสต์อ่านไฟล์ตรง ๆ ได้ (I-2 คุมเฉพาะ UI thread ของโปรแกรมจริง)
    // และต้อง panic! จริงเพื่อจำลองภาพเสียตาม I-7
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::disallowed_methods
    )]

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("refx-log-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_to_log_file() {
        let dir = temp_dir("write");
        let mut writer = SizeRotatingWriter::new(&dir).unwrap();
        // ข้อความไทยต้องรอดผ่าน writer โดยไม่เพี้ยน (log ของเราเป็นภาษาไทยทั้งหมด)
        writer.write_all("สวัสดี\n".as_bytes()).unwrap();
        writer.flush().unwrap();

        let content = std::fs::read_to_string(dir.join(LOG_FILE)).unwrap();
        assert!(content.contains("สวัสดี"));
    }

    #[test]
    fn continues_counting_from_existing_file() {
        let dir = temp_dir("resume");
        {
            let mut writer = SizeRotatingWriter::new(&dir).unwrap();
            writer.write_all(&vec![b'x'; 1000]).unwrap();
            writer.flush().unwrap();
        }
        // เปิดใหม่ต้องนับต่อจากของเดิม ไม่ใช่เริ่มศูนย์
        let writer = SizeRotatingWriter::new(&dir).unwrap();
        assert_eq!(writer.written, 1000);
    }

    #[test]
    fn rotates_when_over_limit() {
        let dir = temp_dir("rotate");
        let mut writer = SizeRotatingWriter::new(&dir).unwrap();

        // เขียนให้ทะลุ 5 MB
        let chunk = vec![b'a'; 64 * 1024];
        let mut total = 0u64;
        while total <= MAX_BYTES {
            writer.write_all(&chunk).unwrap();
            total += chunk.len() as u64;
        }
        writer.flush().unwrap();

        assert!(
            dir.join(format!("{LOG_FILE}.1")).exists(),
            "ต้องมีไฟล์ที่หมุนไปแล้ว"
        );
        // ไฟล์ปัจจุบันต้องเล็กลง ไม่ใช่โตต่อไปเรื่อย ๆ
        let current = std::fs::metadata(dir.join(LOG_FILE)).unwrap().len();
        assert!(current < MAX_BYTES, "ไฟล์ปัจจุบันยังใหญ่เกินเพดาน: {current}");
    }

    #[test]
    fn keeps_at_most_three_old_files() {
        let dir = temp_dir("keep");
        let mut writer = SizeRotatingWriter::new(&dir).unwrap();
        for _ in 0..5 {
            writer.rotate();
        }
        let extra = dir.join(format!("{LOG_FILE}.{}", KEEP_FILES + 1));
        assert!(!extra.exists(), "เก็บไฟล์เก่าเกิน {KEEP_FILES} ไฟล์");
    }

    /// ★ I-7: panic ของ decode worker ห้ามเด้ง dialog ใส่หน้าผู้ใช้
    #[test]
    fn dialog_only_for_main_thread() {
        assert!(should_show_dialog(Some("main")));
        assert!(!should_show_dialog(Some("refx-decode-0")));
        assert!(!should_show_dialog(Some("refx-io")));
        // เธรดไม่มีชื่อ = ไม่ใช่ main แน่นอน
        assert!(!should_show_dialog(None));
    }

    #[test]
    fn panic_message_handles_both_payload_types() {
        // panic!("...") ให้ &str, panic!("{}", x) ให้ String
        let as_str: Box<dyn std::any::Any + Send> = Box::new("ข้อความแบบ str");
        assert_eq!(panic_message(as_str.as_ref()), "ข้อความแบบ str");

        let as_string: Box<dyn std::any::Any + Send> = Box::new(String::from("ข้อความแบบ String"));
        assert_eq!(panic_message(as_string.as_ref()), "ข้อความแบบ String");

        // payload แปลก ๆ ต้องไม่ทำให้ hook พังซ้ำ
        let odd: Box<dyn std::any::Any + Send> = Box::new(42u32);
        assert_eq!(panic_message(odd.as_ref()), "ไม่มีข้อความ");
    }

    /// panic บน worker ต้องถูกดักได้ ไม่ล้มทั้งโปรเซส (I-7)
    #[test]
    fn worker_panic_is_catchable() {
        let handle = std::thread::Builder::new()
            .name("refx-decode-test".to_owned())
            .spawn(|| {
                std::panic::catch_unwind(|| {
                    panic!("จำลองภาพเสีย");
                })
            })
            .unwrap();

        let result = handle.join().unwrap();
        assert!(result.is_err(), "catch_unwind ต้องดัก panic ของ worker ได้");
    }

    /// เขียน log ไม่ได้ต้องไม่ทำให้โปรแกรมล้ม (เสถียรมาก่อน)
    #[test]
    fn write_survives_missing_file_handle() {
        let dir = temp_dir("nofile");
        let mut writer = SizeRotatingWriter::new(&dir).unwrap();
        writer.file = None;
        assert_eq!(writer.write(b"hello").unwrap(), 5);
        writer.flush().unwrap();
    }
}
