//! ที่อยู่ของ cache / config / log ตามมาตรฐานของแต่ละ OS
//!
//! ใช้ `directories` เพื่อไม่ต้องเดา path เอง — บน Windows คือใต้ `%LOCALAPPDATA%`
//! บน Linux ตาม XDG (`~/.cache`, `~/.config`)
//!
//! spec: ARCHITECTURE.md §5

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

/// หา path ของโปรแกรมไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    /// OS ไม่บอกว่า home directory อยู่ไหน
    #[error(
        "หาโฟลเดอร์ข้อมูลผู้ใช้ไม่เจอ\n\
         RefX ต้องใช้โฟลเดอร์นี้เก็บ cache ของภาพย่อ\n\
         ตรวจว่าตัวแปรระบบ HOME (Linux) หรือ LOCALAPPDATA (Windows) ตั้งไว้ถูกต้อง"
    )]
    NoHomeDir,

    /// สร้างโฟลเดอร์ไม่ได้
    #[error("สร้างโฟลเดอร์ {path} ไม่ได้: {source}\nตรวจสิทธิ์การเขียนหรือพื้นที่ว่างในไดรฟ์")]
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
            cache_dir: dirs.cache_dir().to_path_buf(),
            config_dir: dirs.config_dir().to_path_buf(),
            // directories ไม่มี log_dir บนทุก OS — เก็บไว้ใต้ cache ให้เหมือนกันทุกแพลตฟอร์ม
            // (log คือของที่ลบทิ้งได้ ไม่ใช่ config ของผู้ใช้)
            log_dir: dirs.cache_dir().join("logs"),
        })
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

    /// สร้างโฟลเดอร์ทั้งสามถ้ายังไม่มี
    ///
    /// แตะดิสก์ — เรียกตอนเปิดโปรแกรมเท่านั้น ห้ามเรียกในลูปเฟรม (I-2)
    pub fn ensure_exist(&self) -> Result<(), PathError> {
        for dir in [&self.cache_dir, &self.config_dir, &self.log_dir] {
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
    }
}
