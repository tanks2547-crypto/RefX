//! ที่อยู่ของ data / cache / config / log ตามมาตรฐานของแต่ละ OS
//!
//! ใช้ `directories` เพื่อไม่ต้องเดา path เอง — บน Windows คือใต้ `%LOCALAPPDATA%`
//! บน Linux ตาม XDG (`~/.cache`, `~/.config`, `~/.local/share`)
//!
//! ★★★ **`data_dir` กับ `cache_dir` ไม่ใช่ของอย่างเดียวกัน และเส้นแบ่งนี้สำคัญ**
//!
//! `cache_dir` คือที่ของสิ่งที่ **สร้างใหม่ได้** (thumbnail, cache.sqlite) — ลบทิ้ง
//! แล้วโปรแกรมแค่ช้าลงชั่วคราว · ทั้ง OS และเครื่องมือทำความสะอาดของผู้ใช้ถือว่า
//! ที่นั่นลบได้ตามใจ · `data_dir` คือที่ของสิ่งที่ **สร้างใหม่ไม่ได้**
//!
//! งานที่ผู้ใช้จัดมาสามชั่วโมงแล้วยังไม่เคยกด `Ctrl+S` คือสิ่งที่สร้างใหม่ไม่ได้
//! ที่สุดในโปรแกรมนี้ → `docs/07 §4` จึงห้ามวาง `recovery/` ใน `cache_dir`
//! **เด็ดขาด** (เหตุผลเดียวกับที่ `docs/08 §5` ห้าม cache eviction แตะ `logs/`)
//!
//! spec: ARCHITECTURE.md §5, docs/07-file-format.md §4

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

/// หา path ของโปรแกรมไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    /// OS ไม่บอกว่า home directory อยู่ไหน
    #[error(
        "cannot locate the per-user data directory \
         (check HOME on Linux, LOCALAPPDATA on Windows)"
    )]
    NoHomeDir,

    /// สร้างโฟลเดอร์ไม่ได้
    #[error("cannot create directory {path}: {source}")]
    CreateDir {
        /// โฟลเดอร์ที่สร้างไม่สำเร็จ
        path: PathBuf,
        /// สาเหตุจากระบบไฟล์
        source: std::io::Error,
    },
}

/// โฟลเดอร์ทั้งหมดที่ RefX ใช้
///
/// สร้างครั้งเดียวตอนเปิดโปรแกรมแล้วส่งต่อ — ห้ามเรียก `discover()` ซ้ำในลูปเฟรม
/// เพราะมันแตะระบบไฟล์ (I-2)
#[derive(Debug, Clone)]
pub struct AppPaths {
    data_dir: PathBuf,
    cache_dir: PathBuf,
    config_dir: PathBuf,
    log_dir: PathBuf,
}

impl AppPaths {
    /// หา path ตามมาตรฐาน OS โดย **ยังไม่สร้างโฟลเดอร์**
    ///
    /// แยกการ "หา" ออกจากการ "สร้าง" เพราะบางเส้นทาง (เช่น `--help`)
    /// ไม่ควรไปสร้างโฟลเดอร์ทิ้งไว้บนเครื่องผู้ใช้
    pub fn discover() -> Result<Self, PathError> {
        // qualifier/organization ว่าง → Windows ได้ %LOCALAPPDATA%\RefX\...
        // Linux ได้ ~/.cache/refx, ~/.config/refx ตาม XDG
        let dirs = ProjectDirs::from("", "", "RefX").ok_or(PathError::NoHomeDir)?;

        Ok(Self {
            // ★★★ `data_local_dir()` ไม่ใช่ `data_dir()` — **ต่างกันบน Windows**
            //
            //   `directories` แม็พ `data_dir()` ไปที่ **Roaming** (`%APPDATA%`)
            //   ส่วน `data_local_dir()` ไปที่ `%LOCALAPPDATA%` ซึ่งเป็นที่ที่
            //   `docs/07 §4` ระบุไว้ตรง ๆ · บน Linux ทั้งคู่คือ `$XDG_DATA_HOME`
            //   เหมือนกัน จึงไม่มีอะไรเปลี่ยน
            //
            //   ★ เหตุผลที่ Roaming ผิดไม่ใช่แค่ "ไม่ตรง spec": โปรไฟล์ roaming
            //     ถูกซิงค์ขึ้นเซิร์ฟเวอร์ตอน login/logout บนเครื่องในโดเมน —
            //     งานที่กำลังแก้อยู่ (และจะใหญ่ระดับ GB ตอน P4-5 packed) ไม่ควร
            //     ถูกลากข้ามเน็ตเวิร์กทุกครั้งที่ผู้ใช้ล็อกอิน
            //
            //   ★★ เจอเพราะ **รันของจริงแล้วไปดูโฟลเดอร์** ไม่ใช่เพราะเทสต์ —
            //      เทสต์ทุกตัวเขียวอยู่ก่อนหน้านั้น (docs/08 §3.9 ข้อ 5)
            data_dir: dirs.data_local_dir().to_path_buf(),
            cache_dir: dirs.cache_dir().to_path_buf(),
            config_dir: dirs.config_dir().to_path_buf(),
            // directories ไม่มี log_dir บนทุก OS — เก็บไว้ใต้ cache ให้เหมือนกันทุกแพลตฟอร์ม
            // (log คือของที่ลบทิ้งได้ ไม่ใช่ config ของผู้ใช้)
            log_dir: dirs.cache_dir().join("logs"),
        })
    }

    /// โฟลเดอร์ data — ของที่ **สร้างใหม่ไม่ได้** (ดูหัวโมดูล)
    ///
    /// บน Windows คือ `%LOCALAPPDATA%\RefX\data`
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// ★★★ โฟลเดอร์ snapshot ของ **งานที่ยังไม่เคยบันทึกลง path จริง** (P4-4)
    ///
    /// `<data_dir>/recovery/<session-id>.refx` ตาม `docs/07 §4` — **ห้ามย้ายไป
    /// `cache_dir`** ไม่ว่าด้วยเหตุผลอะไร (ดูหัวโมดูล + เทสต์
    /// `recovery_never_lives_under_the_cache_dir`)
    #[must_use]
    pub fn recovery_dir(&self) -> PathBuf {
        self.data_dir.join("recovery")
    }

    /// โฟลเดอร์ cache — `cache.sqlite`, thumbnail, lock file
    #[must_use]
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// โฟลเดอร์ config — `settings.toml`, `keymap.toml`
    #[must_use]
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// โฟลเดอร์ log — crash log, tracing output
    #[must_use]
    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    /// สร้างโฟลเดอร์ทั้งหมดถ้ายังไม่มี
    ///
    /// แตะดิสก์ — เรียกตอนเปิดโปรแกรมเท่านั้น ห้ามเรียกในลูปเฟรม (I-2)
    ///
    /// ★ `recovery/` ถูกสร้างตั้งแต่เปิดโปรแกรม **ก่อน**ที่จะมีอะไรให้กู้ —
    /// การรอสร้างตอนจะเขียน snapshot ครั้งแรก แปลว่าความล้มเหลวของการสร้าง
    /// โฟลเดอร์จะไปโผล่ตอนที่ผู้ใช้ต้องการมันที่สุดพอดี
    pub fn ensure_exist(&self) -> Result<(), PathError> {
        let recovery = self.recovery_dir();
        for dir in [
            &self.cache_dir,
            &self.config_dir,
            &self.log_dir,
            &self.data_dir,
            &recovery,
        ] {
            std::fs::create_dir_all(dir).map_err(|source| PathError::CreateDir {
                path: dir.clone(),
                source,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn discover_gives_three_distinct_dirs() {
        let paths = AppPaths::discover().expect("เครื่องทดสอบต้องมี home dir");
        // log ต้องไม่ใช่ตัวเดียวกับ cache ไม่งั้นลบ log แล้ว cache หายไปด้วย
        assert_ne!(paths.cache_dir(), paths.log_dir());
        assert!(paths.log_dir().starts_with(paths.cache_dir()));
    }

    #[test]
    fn discover_is_deterministic() {
        // path ต้องคงที่ระหว่างการเรียกสองครั้ง ไม่งั้น cache จะกระจัดกระจาย
        let a = AppPaths::discover().unwrap();
        let b = AppPaths::discover().unwrap();
        assert_eq!(a.cache_dir(), b.cache_dir());
        assert_eq!(a.config_dir(), b.config_dir());
        assert_eq!(a.data_dir(), b.data_dir());
        assert_eq!(a.recovery_dir(), b.recovery_dir());
    }

    /// ★★★ **`recovery/` ต้องไม่อยู่ใต้ `cache_dir` ไม่ว่ากรณีใด** (`docs/07 §4`)
    ///
    /// cache คือที่ของสิ่งที่สร้างใหม่ได้ — ทั้ง OS, ตัวล้างดิสก์ของผู้ใช้ และ
    /// eviction ของเราเองถือว่าลบได้ตามใจ · งานที่ยังไม่เคยบันทึกคือสิ่งตรงข้าม
    /// สนิท การวางมันในนั้นแปลว่ากลไกที่สร้างมากันงานหาย **ยืนอยู่บนที่ที่มีคน
    /// ตั้งใจจะลบเป็นระยะ**
    ///
    /// ★ เทสต์นี้ล้มเป็น: เปลี่ยน `recovery_dir()` ให้คืน `cache_dir/recovery`
    /// แล้วมันแดงทันที (ยืนยันแล้วตอนเขียน)
    #[test]
    fn recovery_never_lives_under_the_cache_dir() {
        let paths = AppPaths::discover().unwrap();
        let recovery = paths.recovery_dir();
        assert!(
            !recovery.starts_with(paths.cache_dir()),
            "งานที่ยังไม่เคยบันทึกถูกวางไว้ใน cache: {}",
            recovery.display()
        );
        assert!(
            recovery.starts_with(paths.data_dir()),
            "recovery ต้องอยู่ใต้ data_dir: {}",
            recovery.display()
        );
        // และ data_dir เองก็ต้องไม่ใช่ cache_dir — ไม่งั้นข้อบนจริงโดยบังเอิญ
        assert_ne!(paths.data_dir(), paths.cache_dir());
    }

    /// ★★ **บน Windows ต้องอยู่ใต้ `%LOCALAPPDATA%` ไม่ใช่ Roaming** (`docs/07 §4`)
    ///
    /// `directories::ProjectDirs::data_dir()` ให้ **Roaming** มา ซึ่งดูถูกจากชื่อ
    /// ทุกประการ แต่ผิดสองข้อ: ไม่ตรงที่ที่ spec ระบุ และทำให้งานที่กำลังแก้อยู่
    /// ถูกซิงค์ขึ้นเซิร์ฟเวอร์ทุกครั้งที่ผู้ใช้ในโดเมนล็อกอิน
    ///
    /// ★ เจอตอนรันของจริงแล้วไปเปิดโฟลเดอร์ดู (`%APPDATA%\RefX\data\recovery`
    /// มี snapshot อยู่จริง) — เทสต์ทุกตัวเขียวอยู่ก่อนหน้านั้น เพราะไม่มีตัวไหน
    /// ถามว่า "แล้วมันไปลงที่ไหนกันแน่"
    #[cfg(windows)]
    #[test]
    fn on_windows_data_lives_under_local_appdata() {
        let paths = AppPaths::discover().unwrap();
        let local = std::env::var_os("LOCALAPPDATA").expect("Windows ต้องมี LOCALAPPDATA");
        assert!(
            paths.data_dir().starts_with(&local),
            "data อยู่นอก LOCALAPPDATA: {} (Roaming จะซิงค์งานข้ามเน็ตเวิร์ก)",
            paths.data_dir().display()
        );
        if let Some(roaming) = std::env::var_os("APPDATA") {
            assert!(
                !paths.recovery_dir().starts_with(&roaming),
                "งานที่ยังไม่เคยบันทึกถูกวางไว้ในโปรไฟล์ roaming"
            );
        }
    }

    /// โฟลเดอร์ recovery ต้องมีอยู่ตั้งแต่เปิดโปรแกรม ไม่ใช่ตอนจะเขียนครั้งแรก
    #[test]
    fn ensure_exist_creates_the_recovery_dir_too() {
        let root = std::env::temp_dir().join(format!("refx-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = AppPaths {
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            config_dir: root.join("config"),
            log_dir: root.join("cache").join("logs"),
        };
        paths.ensure_exist().unwrap();
        assert!(paths.recovery_dir().is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }
}
