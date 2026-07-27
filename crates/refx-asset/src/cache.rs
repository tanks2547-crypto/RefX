//! `cache.sqlite` — thumbnail + metadata cache
//!
//! **เข้าถึงจาก IO thread ตัวเดียวเท่านั้น** (serialized) จึงไม่มี lock contention
//! และไม่ต้องใช้ connection pool — ดู [`crate::cache::IoThread`]
//!
//! หลักการที่ห้ามลืม: **cache เสียหาย ≠ ข้อมูลผู้ใช้หาย**
//! ถ้าเปิด DB ไม่ได้ให้ลบทิ้งแล้วสร้างใหม่ ห้ามให้โปรแกรมเปิดไม่ขึ้นเพราะ cache
//! (thumbnail สร้างใหม่ได้เสมอ งานของผู้ใช้สร้างใหม่ไม่ได้)
//!
//! spec: docs/05-memory-and-assets.md §5

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension as _, params};

use crate::hash::ContentHash;

/// รูปแบบของ thumbnail ที่เก็บใน DB
///
/// ★ มีค่าเดียวคือ RGBA8 — **BC7 ถูกตัดออกจาก P1** (docs/04 §4, ตัดสิน 27 ก.ค. 2026)
/// เพราะ encode ใช้เวลาระดับร้อย ms ต่อ tile ซึ่งชนกับสัญญา "ภาพขึ้นทันที"
///
/// คอลัมน์ `thumb_fmt` ยังอยู่ใน schema เผื่ออนาคต แต่ **ไม่มีโค้ดสาขา** ตามที่ spec สั่ง
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbFormat {
    /// RGBA 8 บิตต่อช่อง — รูปแบบเดียวที่ใช้ใน P1
    Rgba8 = 0,
}

impl ThumbFormat {
    fn from_i64(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Rgba8),
            // ค่าอื่น (รวมของที่ P1 รุ่นก่อนเคยเขียนไว้) = cache miss ไม่ใช่ error
            _ => None,
        }
    }
}

/// หนึ่งรายการ thumbnail ใน cache
#[derive(Debug, Clone)]
pub struct ThumbEntry {
    /// ขนาดจริงของภาพต้นฉบับ
    pub width: u32,
    /// ความสูงจริงของภาพต้นฉบับ
    pub height: u32,
    /// รูปแบบไฟล์ต้นฉบับ (เก็บเป็นเลขเพื่อไม่ผูกกับ enum ของ `image`)
    pub format: i64,
    /// รูปแบบของข้อมูล thumbnail
    pub thumb_fmt: ThumbFormat,
    /// ข้อมูล thumbnail 128×128
    pub thumb: Vec<u8>,
    /// สีเด่น (ARGB) ใช้เป็น placeholder ก่อนภาพจริงจะมา
    pub dominant: u32,
}

/// ข้อมูลไฟล์ที่ใช้ข้ามการ hash
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathFingerprint {
    /// mtime เป็นวินาที unix
    pub mtime: i64,
    /// ขนาดไฟล์
    pub size: u64,
}

/// ★ คีย์ของ cache — **hash อย่างเดียวไม่พอ** (docs/05 §4, แก้ 27 ก.ค. 2026)
///
/// fast path ของไฟล์ > 64 MB อ่านแค่หัว 1 MB + ท้าย 1 MB + ขนาด
/// ไฟล์สองไฟล์ที่ต่างกัน **เฉพาะตรงกลาง** จึงได้ hash เดียวกัน
///
/// นี่ไม่ใช่เคสสมมติ: นักวาด save ทับเป็น v1/v2 ของ PSD/TIFF 100 MB
/// โดยแก้เลเยอร์กลางไฟล์ = งานประจำวัน หัว/ท้าย/ขนาดแทบไม่ขยับ
/// ผลคือ **เห็น thumbnail ของเวอร์ชันเก่า** ซึ่งเงียบ ไม่ crash ไม่มี error
/// ผู้ใช้แค่รู้สึกว่าโปรแกรม "มั่วเป็นบางที" แล้วหาสาเหตุไม่เจอ
///
/// `mtime` ปิดจุดบอดนี้ฟรี ๆ เพราะ save ทับย่อมเปลี่ยน mtime เสมอ
/// ผลข้างเคียงที่ยอมรับได้: copy/move ไฟล์ → mtime เปลี่ยน → cache miss → decode ใหม่
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheKey {
    /// hash ของเนื้อไฟล์ (อาจเป็น fast path)
    pub hash: ContentHash,
    /// mtime ของไฟล์ตอนที่คำนวณ
    pub mtime: i64,
    /// ขนาดไฟล์ตอนที่คำนวณ
    pub size: u64,
}

impl CacheKey {
    /// ประกอบคีย์จาก hash + ลายนิ้วมือไฟล์
    #[must_use]
    pub fn new(hash: ContentHash, fingerprint: PathFingerprint) -> Self {
        Self {
            hash,
            mtime: fingerprint.mtime,
            size: fingerprint.size,
        }
    }

    fn size_i64(&self) -> i64 {
        i64::try_from(self.size).unwrap_or(i64::MAX)
    }
}

/// ใช้ cache ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// sqlite มีปัญหา
    #[error("ฐานข้อมูล cache ของภาพย่อมีปัญหา: {0}\nRefX จะสร้างใหม่ให้เอง งานของคุณไม่ได้รับผลกระทบ")]
    Sqlite(#[from] rusqlite::Error),

    /// สร้างโฟลเดอร์ให้ DB ไม่ได้
    #[error("สร้างโฟลเดอร์เก็บ cache ที่ {path} ไม่ได้: {source}")]
    CreateDir {
        /// โฟลเดอร์ที่มีปัญหา
        path: PathBuf,
        /// สาเหตุ
        source: std::io::Error,
    },
}

/// schema ของ cache — ตรงตาม docs/05 §5
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS thumbs (
    -- ★ คีย์ประกอบ (hash, mtime, size) ไม่ใช่ hash เดี่ยว ๆ — ดู CacheKey
    hash        BLOB NOT NULL,
    mtime       INTEGER NOT NULL,
    size        INTEGER NOT NULL,
    width       INTEGER NOT NULL,
    height      INTEGER NOT NULL,
    format      INTEGER NOT NULL,
    thumb_fmt   INTEGER NOT NULL,
    thumb       BLOB NOT NULL,
    dominant    INTEGER NOT NULL,
    last_used   INTEGER NOT NULL,
    created_at  INTEGER NOT NULL,
    PRIMARY KEY (hash, mtime, size)
);
CREATE INDEX IF NOT EXISTS idx_last_used ON thumbs(last_used);

CREATE TABLE IF NOT EXISTS paths (
    path       TEXT PRIMARY KEY,
    hash       BLOB NOT NULL,
    mtime      INTEGER NOT NULL,
    size       INTEGER NOT NULL
);
";

/// ฐานข้อมูล cache
///
/// ★ ห้ามแชร์ข้ามเธรด — ตัวนี้เป็นของ IO thread คนเดียว
pub struct CacheDb {
    conn: Connection,
    path: PathBuf,
}

impl CacheDb {
    /// เปิด DB — **ถ้าเสียหายจะลบทิ้งแล้วสร้างใหม่ให้อัตโนมัติ**
    ///
    /// # Errors
    /// คืน error เฉพาะกรณีที่สร้างใหม่แล้วยังไม่ได้ (เช่น ดิสก์เต็ม/ไม่มีสิทธิ์เขียน)
    pub fn open_or_recreate(path: &Path) -> Result<Self, CacheError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| CacheError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        match Self::try_open(path) {
            Ok(db) => Ok(db),
            Err(err) => {
                // cache พังไม่ใช่เรื่องคอขาดบาดตาย — ลบแล้วเริ่มใหม่
                tracing::warn!(%err, "เปิด cache ไม่ได้ — ลบทิ้งแล้วสร้างใหม่");
                Self::remove_db_files(path);
                Self::try_open(path)
            }
        }
    }

    fn try_open(path: &Path) -> Result<Self, CacheError> {
        let conn = Connection::open(path)?;

        // WAL: อ่านกับเขียนพร้อมกันได้ ไม่บล็อกกัน
        // journal_mode คืนค่ากลับมาเป็นแถว จึงต้องใช้ query_row ไม่ใช่ execute
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        // NORMAL: ยอมเสี่ยงเสีย cache ตอนไฟดับ แลกความเร็ว — cache สร้างใหม่ได้
        conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
        conn.execute_batch(SCHEMA)?;

        // ตรวจสุขภาพ + ตรวจว่า schema เป็นรุ่นปัจจุบัน
        // ★ ต้อง select คอลัมน์ใหม่ด้วย ไม่งั้น cache รุ่นเก่า (PK = hash เดี่ยว)
        //   จะผ่านการตรวจแล้วไปพังตอน query จริง
        //   cache ไม่ต้อง migrate — ลบทิ้งสร้างใหม่ถูกกว่าและปลอดภัยกว่า
        conn.query_row(
            "SELECT count(*) FROM thumbs WHERE mtime = 0 AND size = 0",
            [],
            |row| row.get::<_, i64>(0),
        )?;

        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// ลบไฟล์ DB ทั้งชุด (รวม -wal และ -shm ของ WAL mode)
    fn remove_db_files(path: &Path) {
        let _ = std::fs::remove_file(path);
        for suffix in ["-wal", "-shm"] {
            let mut side = path.as_os_str().to_os_string();
            side.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(side));
        }
    }

    /// ที่อยู่ของไฟล์ DB
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// ดึง thumbnail จาก cache แล้วอัปเดต `last_used`
    ///
    /// # Errors
    /// คืน error เมื่อ query ล้มเหลว
    pub fn get_thumb(&self, key: &CacheKey) -> Result<Option<ThumbEntry>, CacheError> {
        let entry = self
            .conn
            .query_row(
                "SELECT width, height, format, thumb_fmt, thumb, dominant
                 FROM thumbs WHERE hash = ?1 AND mtime = ?2 AND size = ?3",
                params![&key.hash.as_bytes()[..], key.mtime, key.size_i64()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Vec<u8>>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;

        let Some((width, height, format, thumb_fmt, thumb, dominant)) = entry else {
            return Ok(None);
        };

        // ค่าที่อ่านจาก DB คือ input ที่ไม่น่าไว้ใจเหมือนกัน (I-4)
        // ไฟล์ cache อาจถูกแก้จากภายนอกได้ — แถวที่ค่าเพี้ยนถือเป็น cache miss
        let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
            tracing::warn!("แถวใน cache มีขนาดภาพผิดปกติ — ถือว่าไม่มีใน cache");
            return Ok(None);
        };
        let Some(thumb_fmt) = ThumbFormat::from_i64(thumb_fmt) else {
            return Ok(None);
        };

        self.touch(key)?;

        Ok(Some(ThumbEntry {
            width,
            height,
            format,
            thumb_fmt,
            thumb,
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "dominant เก็บเป็น ARGB 32 บิตใน INTEGER ของ sqlite ตัดกลับได้ตรง"
            )]
            dominant: dominant as u32,
        }))
    }

    /// อัปเดตเวลาที่ใช้ล่าสุด (ใช้กับ LRU eviction)
    fn touch(&self, key: &CacheKey) -> Result<(), CacheError> {
        self.conn.execute(
            "UPDATE thumbs SET last_used = ?4 WHERE hash = ?1 AND mtime = ?2 AND size = ?3",
            params![
                &key.hash.as_bytes()[..],
                key.mtime,
                key.size_i64(),
                now_unix()
            ],
        )?;
        Ok(())
    }

    /// เก็บ thumbnail ลง cache (เขียนทับถ้ามีอยู่แล้ว)
    ///
    /// # Errors
    /// คืน error เมื่อเขียนไม่สำเร็จ
    pub fn put_thumb(&self, key: &CacheKey, entry: &ThumbEntry) -> Result<(), CacheError> {
        let now = now_unix();
        self.conn.execute(
            "INSERT INTO thumbs
                (hash, mtime, size, width, height, format, thumb_fmt, thumb, dominant,
                 last_used, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
             ON CONFLICT(hash, mtime, size) DO UPDATE SET
                width = ?4, height = ?5, format = ?6, thumb_fmt = ?7,
                thumb = ?8, dominant = ?9, last_used = ?10",
            params![
                &key.hash.as_bytes()[..],
                key.mtime,
                key.size_i64(),
                i64::from(entry.width),
                i64::from(entry.height),
                entry.format,
                entry.thumb_fmt as i64,
                entry.thumb,
                i64::from(entry.dominant),
                now,
            ],
        )?;
        Ok(())
    }

    /// หา hash ของไฟล์จาก path ถ้า mtime + size ยังตรงกับที่เคยบันทึกไว้
    ///
    /// ช่วยข้ามการ hash ไปได้ทั้งขั้น — การ hash ไฟล์ 4000px ใช้เวลาพอ ๆ กับอ่านมัน
    ///
    /// # Errors
    /// คืน error เมื่อ query ล้มเหลว
    pub fn lookup_path(
        &self,
        path: &Path,
        fingerprint: PathFingerprint,
    ) -> Result<Option<ContentHash>, CacheError> {
        let key = path.to_string_lossy();
        let row = self
            .conn
            .query_row(
                "SELECT hash, mtime, size FROM paths WHERE path = ?1",
                params![key],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;

        let Some((hash_bytes, mtime, size)) = row else {
            return Ok(None);
        };

        // ไฟล์ถูกแก้ตั้งแต่ครั้งก่อน → ต้อง hash ใหม่
        if mtime != fingerprint.mtime || size != i64::try_from(fingerprint.size).unwrap_or(-1) {
            return Ok(None);
        }

        let Ok(bytes) = <[u8; 32]>::try_from(hash_bytes.as_slice()) else {
            return Ok(None); // แถวเพี้ยน = cache miss
        };
        Ok(Some(ContentHash::from_bytes(bytes)))
    }

    /// บันทึกความสัมพันธ์ path → hash
    ///
    /// # Errors
    /// คืน error เมื่อเขียนไม่สำเร็จ
    pub fn record_path(
        &self,
        path: &Path,
        hash: &ContentHash,
        fingerprint: PathFingerprint,
    ) -> Result<(), CacheError> {
        self.conn.execute(
            "INSERT INTO paths (path, hash, mtime, size) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET hash = ?2, mtime = ?3, size = ?4",
            params![
                path.to_string_lossy(),
                &hash.as_bytes()[..],
                fingerprint.mtime,
                i64::try_from(fingerprint.size).unwrap_or(i64::MAX),
            ],
        )?;
        Ok(())
    }

    /// จำนวน thumbnail ที่เก็บอยู่
    ///
    /// # Errors
    /// คืน error เมื่อ query ล้มเหลว
    pub fn thumb_count(&self) -> Result<u64, CacheError> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM thumbs", [], |row| row.get(0))?;
        Ok(n.max(0).unsigned_abs())
    }

    /// ขนาดโดยประมาณของ DB (ไบต์)
    ///
    /// # Errors
    /// คืน error เมื่อ query ล้มเหลว
    pub fn size_bytes(&self) -> Result<u64, CacheError> {
        let pages: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))?;
        Ok((pages.max(0) as u64).saturating_mul(page_size.max(0) as u64))
    }

    /// ★ I-6: ลบรายการเก่าสุดจนกว่าจะเล็กกว่าเพดาน
    ///
    /// docs/05 §5 บอกให้รันตอน **ปิดโปรแกรม** ไม่ใช่ระหว่างใช้งาน
    /// เพราะ `VACUUM` กินเวลาและจะทำให้ผู้ใช้รู้สึกว่าโปรแกรมค้าง
    ///
    /// # Errors
    /// คืน error เมื่อลบไม่สำเร็จ
    pub fn evict_until_under(&self, limit_bytes: u64) -> Result<u64, CacheError> {
        let mut removed = 0u64;

        // ลบทีละก้อนแล้ววัดใหม่ — ขนาดต่อแถวไม่เท่ากัน คำนวณล่วงหน้าไม่ได้
        loop {
            if self.size_bytes()? <= limit_bytes {
                break;
            }
            let deleted = self.conn.execute(
                "DELETE FROM thumbs WHERE rowid IN (
                     SELECT rowid FROM thumbs ORDER BY last_used ASC LIMIT 256
                 )",
                [],
            )?;
            if deleted == 0 {
                break; // ไม่มีอะไรให้ลบแล้ว แต่ยังใหญ่อยู่ — ออกก่อนจะวนไม่จบ
            }
            removed += deleted as u64;
            // คืนพื้นที่จริงให้ไฟล์ ไม่งั้น page_count ไม่ลด แล้วจะวนไม่จบ
            self.conn.execute_batch("VACUUM;")?;
        }

        if removed > 0 {
            tracing::info!(removed, "ล้าง thumbnail เก่าออกจาก cache");
        }
        Ok(removed)
    }
}

/// คำสั่งที่ส่งให้ IO thread
///
/// ทุกคำสั่งที่ต้องการคำตอบพก `reply` มาเอง — ผู้เรียกเป็นคนเลือกว่าจะรอหรือไม่รอ
#[derive(Debug)]
pub enum IoRequest {
    /// ขอ thumbnail จาก cache
    GetThumb {
        /// คีย์ประกอบ (hash, mtime, size)
        key: CacheKey,
        /// ช่องส่งคำตอบกลับ
        reply: crossbeam_channel::Sender<Option<ThumbEntry>>,
    },
    /// เก็บ thumbnail (ไม่ต้องรอคำตอบ)
    PutThumb {
        /// คีย์ประกอบ (hash, mtime, size)
        key: CacheKey,
        /// ข้อมูล
        entry: Box<ThumbEntry>,
    },
    /// ถามว่าไฟล์นี้เคย hash ไว้แล้วหรือยัง
    LookupPath {
        /// path ของไฟล์
        path: PathBuf,
        /// mtime + size ปัจจุบัน
        fingerprint: PathFingerprint,
        /// ช่องส่งคำตอบกลับ
        reply: crossbeam_channel::Sender<Option<ContentHash>>,
    },
    /// บันทึก path → hash
    RecordPath {
        /// path ของไฟล์
        path: PathBuf,
        /// hash ที่คำนวณได้
        hash: ContentHash,
        /// mtime + size ตอนคำนวณ
        fingerprint: PathFingerprint,
    },
    /// ล้างของเก่าจนเล็กกว่าเพดาน (เรียกตอนปิดโปรแกรม)
    EvictUnder {
        /// เพดานขนาด DB
        limit_bytes: u64,
        /// จำนวนแถวที่ลบไป
        reply: crossbeam_channel::Sender<u64>,
    },
    /// ขอสถิติไปแสดงบน status bar (I-6)
    Stats {
        /// ช่องส่งคำตอบกลับ
        reply: crossbeam_channel::Sender<CacheStats>,
    },
}

/// สถิติ cache สำหรับ status bar
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// จำนวน thumbnail ที่เก็บอยู่
    pub thumb_count: u64,
    /// ขนาดไฟล์ DB โดยประมาณ
    pub size_bytes: u64,
}

/// IO thread ตัวเดียวที่เป็นเจ้าของ `cache.sqlite`
///
/// ★ ทุกการแตะ DB ต้องผ่านที่นี่ (ARCHITECTURE §3) — serialized โดยธรรมชาติ
/// จึงไม่มี lock contention และไม่ต้องใช้ connection pool
///
/// ★ **UI thread ห้ามรอคำตอบจากที่นี่** (I-2) ให้ส่งคำสั่งแล้วรับผลในเฟรมถัดไป
/// worker thread รอได้ตามสบาย
pub struct IoThread {
    tx: Option<crossbeam_channel::Sender<IoRequest>>,
    handle: Option<std::thread::JoinHandle<()>>,
    /// เพดานขนาด DB ที่จะล้างตอนปิดโปรแกรม
    evict_limit: u64,
}

impl IoThread {
    /// เพดานขนาด cache ก่อนเริ่มล้างของเก่า (docs/05 §5)
    pub const DEFAULT_SIZE_LIMIT: u64 = 2 << 30; // 2 GB

    /// เปิด DB แล้วปล่อย IO thread
    ///
    /// # Errors
    /// คืน error เมื่อเปิด/สร้าง DB ไม่ได้จริง ๆ (ดิสก์เต็ม, ไม่มีสิทธิ์เขียน)
    pub fn spawn(db_path: &Path) -> Result<Self, CacheError> {
        // เปิด DB บนเธรดนี้ก่อนเพื่อให้ error เด้งกลับหาผู้เรียกได้ตรง ๆ
        // ถ้าเปิดในเธรดลูกจะได้แค่ log แล้วผู้ใช้ไม่รู้เรื่อง
        let db = CacheDb::open_or_recreate(db_path)?;
        let (tx, rx) = crossbeam_channel::unbounded::<IoRequest>();

        let handle = std::thread::Builder::new()
            .name("refx-io".to_owned())
            .spawn(move || io_loop(&db, &rx))
            .map_err(|source| CacheError::CreateDir {
                path: db_path.to_path_buf(),
                source,
            })?;

        Ok(Self {
            tx: Some(tx),
            handle: Some(handle),
            evict_limit: Self::DEFAULT_SIZE_LIMIT,
        })
    }

    /// ช่องส่งคำสั่ง — clone ไปให้ worker ได้
    ///
    /// # Panics
    /// ไม่ panic — คืน `None` ถ้า thread ปิดไปแล้ว
    #[must_use]
    pub fn sender(&self) -> Option<crossbeam_channel::Sender<IoRequest>> {
        self.tx.clone()
    }

    /// ตั้งเพดานขนาด cache ที่จะล้างตอนปิด
    pub fn set_evict_limit(&mut self, bytes: u64) {
        self.evict_limit = bytes;
    }
}

impl Drop for IoThread {
    fn drop(&mut self) {
        // ★ ล้าง cache ที่โตเกินเพดาน **ตอนปิดโปรแกรม** ไม่ใช่ระหว่างใช้งาน (docs/05 §5)
        //   VACUUM กินเวลา ถ้าทำระหว่างใช้งานผู้ใช้จะรู้สึกว่าโปรแกรมค้าง
        if let Some(tx) = self.tx.as_ref() {
            let (reply, rx) = crossbeam_channel::bounded(1);
            if tx
                .send(IoRequest::EvictUnder {
                    limit_bytes: self.evict_limit,
                    reply,
                })
                .is_ok()
            {
                // รอได้ ตอนนี้กำลังปิดโปรแกรมอยู่แล้ว แต่ต้องมีเพดานเวลากันค้าง
                match rx.recv_timeout(std::time::Duration::from_secs(10)) {
                    Ok(removed) if removed > 0 => {
                        tracing::info!(removed, "ล้าง cache ตอนปิดโปรแกรม");
                    }
                    Ok(_) => {}
                    Err(_) => tracing::warn!("ล้าง cache ไม่ทันเวลา — ข้ามไปก่อน"),
                }
            }
        }

        // ปิดช่องก่อน → io_loop เห็น channel ปิดแล้วออกจากลูป
        drop(self.tx.take());
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            tracing::error!("IO thread จบแบบผิดปกติ — cache อาจไม่ถูกล้าง");
        }
    }
}

/// ลูปหลักของ IO thread
fn io_loop(db: &CacheDb, rx: &crossbeam_channel::Receiver<IoRequest>) {
    // ทุก error ที่นี่ถูก log แล้วข้าม — cache พังต้องไม่ล้มโปรแกรม
    for request in rx {
        match request {
            IoRequest::GetThumb { key, reply } => {
                let result = db.get_thumb(&key).unwrap_or_else(|err| {
                    tracing::warn!(%err, "อ่าน thumbnail จาก cache ไม่ได้");
                    None
                });
                // ผู้ขออาจเลิกสนใจไปแล้ว (ผู้ใช้ pan ผ่านไป) — ไม่ใช่ error
                let _ = reply.send(result);
            }
            IoRequest::PutThumb { key, entry } => {
                if let Err(err) = db.put_thumb(&key, &entry) {
                    tracing::warn!(%err, "เก็บ thumbnail ลง cache ไม่ได้");
                }
            }
            IoRequest::LookupPath {
                path,
                fingerprint,
                reply,
            } => {
                let result = db.lookup_path(&path, fingerprint).unwrap_or_else(|err| {
                    tracing::warn!(%err, "ค้น path ใน cache ไม่ได้");
                    None
                });
                let _ = reply.send(result);
            }
            IoRequest::RecordPath {
                path,
                hash,
                fingerprint,
            } => {
                if let Err(err) = db.record_path(&path, &hash, fingerprint) {
                    tracing::warn!(%err, "บันทึก path ลง cache ไม่ได้");
                }
            }
            IoRequest::EvictUnder { limit_bytes, reply } => {
                let removed = db.evict_until_under(limit_bytes).unwrap_or_else(|err| {
                    tracing::warn!(%err, "ล้าง cache ไม่สำเร็จ");
                    0
                });
                let _ = reply.send(removed);
            }
            IoRequest::Stats { reply } => {
                let stats = CacheStats {
                    thumb_count: db.thumb_count().unwrap_or(0),
                    size_bytes: db.size_bytes().unwrap_or(0),
                };
                let _ = reply.send(stats);
            }
        }
    }
    tracing::debug!("IO thread ปิดตัว");
}

/// เวลาปัจจุบันเป็นวินาที unix
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn temp_db(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("refx-cache-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("cache.sqlite")
    }

    fn key_for(seed: &[u8], mtime: i64, size: u64) -> CacheKey {
        CacheKey::new(
            crate::hash::hash_bytes(seed),
            PathFingerprint { mtime, size },
        )
    }

    fn sample_entry() -> ThumbEntry {
        ThumbEntry {
            width: 4000,
            height: 3000,
            format: 1,
            thumb_fmt: ThumbFormat::Rgba8,
            thumb: vec![0xAB; 128],
            dominant: 0xFF00_8040,
        }
    }

    #[test]
    fn creates_and_reopens() {
        let path = temp_db("reopen");
        let key = key_for(b"one", 111, 222);
        {
            let db = CacheDb::open_or_recreate(&path).unwrap();
            db.put_thumb(&key, &sample_entry()).unwrap();
        }
        // เปิดใหม่ต้องเจอของเดิม
        let db = CacheDb::open_or_recreate(&path).unwrap();
        let entry = db.get_thumb(&key).unwrap().expect("ต้องเจอของที่เก็บไว้");
        assert_eq!(entry.width, 4000);
        assert_eq!(entry.thumb, vec![0xAB; 128]);
        assert_eq!(entry.dominant, 0xFF00_8040);
    }

    #[test]
    fn missing_hash_is_none_not_error() {
        let db = CacheDb::open_or_recreate(&temp_db("miss")).unwrap();
        let key = key_for("ไม่เคยเก็บ".as_bytes(), 1, 2);
        assert!(db.get_thumb(&key).unwrap().is_none());
    }

    #[test]
    fn put_twice_overwrites() {
        let db = CacheDb::open_or_recreate(&temp_db("overwrite")).unwrap();
        let key = key_for(b"same", 5, 6);

        db.put_thumb(&key, &sample_entry()).unwrap();
        let mut second = sample_entry();
        second.width = 111;
        db.put_thumb(&key, &second).unwrap();

        assert_eq!(db.thumb_count().unwrap(), 1, "ต้องทับของเดิม ไม่ใช่เพิ่มแถว");
        assert_eq!(db.get_thumb(&key).unwrap().unwrap().width, 111);
    }

    /// ★ cache เสียหาย ≠ ข้อมูลผู้ใช้หาย — ต้องสร้างใหม่ให้เอง ไม่ใช่เปิดโปรแกรมไม่ขึ้น
    #[test]
    fn corrupt_database_is_recreated() {
        let path = temp_db("corrupt");
        {
            let db = CacheDb::open_or_recreate(&path).unwrap();
            db.put_thumb(&key_for(b"x", 1, 1), &sample_entry()).unwrap();
        }
        // เขียนขยะทับ (จำลองไฟล์เสียจากไฟดับ / ดิสก์พัง)
        let mut junk = vec![0x00, 0x01, 0x02, 0x20];
        junk.extend_from_slice("ไฟล์นี้ไม่ใช่ sqlite แล้ว".as_bytes());
        std::fs::write(&path, &junk).unwrap();

        let db = CacheDb::open_or_recreate(&path).expect("ต้องสร้างใหม่ได้ ไม่ใช่ล้ม");
        assert_eq!(db.thumb_count().unwrap(), 0, "DB ใหม่ต้องว่าง");

        // ใช้งานต่อได้ปกติ
        let key = key_for("หลังสร้างใหม่".as_bytes(), 9, 9);
        db.put_thumb(&key, &sample_entry()).unwrap();
        assert!(db.get_thumb(&key).unwrap().is_some());
    }

    #[test]
    fn empty_file_is_recreated() {
        let path = temp_db("emptyfile");
        std::fs::write(&path, b"").unwrap();
        let db = CacheDb::open_or_recreate(&path).expect("ไฟล์ว่างต้องเปิดได้ (sqlite สร้างใหม่ให้)");
        assert_eq!(db.thumb_count().unwrap(), 0);
    }

    // ---------- ★ คีย์ประกอบปิดจุดบอดของ fast hash ----------

    /// เคสจริง: นักวาด save ทับ PSD/TIFF 100 MB โดยแก้เลเยอร์กลางไฟล์
    /// → fast hash ชนกัน แต่ mtime เปลี่ยน → **ต้องไม่คืน thumbnail ของเวอร์ชันเก่า**
    #[test]
    fn same_hash_different_mtime_is_a_different_entry() {
        let db = CacheDb::open_or_recreate(&temp_db("blindspot")).unwrap();
        let hash = crate::hash::hash_bytes("หัวและท้ายเหมือนกันเป๊ะ".as_bytes());
        let size = 100 << 20;

        let v1 = CacheKey::new(hash, PathFingerprint { mtime: 1000, size });
        let v2 = CacheKey::new(hash, PathFingerprint { mtime: 2000, size });

        let mut old = sample_entry();
        old.width = 111; // thumbnail ของเวอร์ชันเก่า
        db.put_thumb(&v1, &old).unwrap();

        // ★ เวอร์ชันใหม่ต้องเป็น cache miss ไม่ใช่คืนของเก่ามา
        assert!(
            db.get_thumb(&v2).unwrap().is_none(),
            "hash ชนกันแต่ mtime ต่าง → ต้อง miss ไม่งั้นผู้ใช้เห็น thumbnail ของเวอร์ชันเก่า"
        );

        let mut new = sample_entry();
        new.width = 222;
        db.put_thumb(&v2, &new).unwrap();

        // ทั้งสองเวอร์ชันอยู่ร่วมกันได้ ไม่ทับกัน
        assert_eq!(db.get_thumb(&v1).unwrap().unwrap().width, 111);
        assert_eq!(db.get_thumb(&v2).unwrap().unwrap().width, 222);
        assert_eq!(db.thumb_count().unwrap(), 2);
    }

    /// ขนาดต่างก็ต้องแยกกันด้วย (แก้ไฟล์แล้วขนาดเปลี่ยนเล็กน้อย)
    #[test]
    fn same_hash_different_size_is_a_different_entry() {
        let db = CacheDb::open_or_recreate(&temp_db("blindsize")).unwrap();
        let hash = crate::hash::hash_bytes("เหมือนกัน".as_bytes());
        let a = CacheKey::new(
            hash,
            PathFingerprint {
                mtime: 5,
                size: 1000,
            },
        );
        let b = CacheKey::new(
            hash,
            PathFingerprint {
                mtime: 5,
                size: 1001,
            },
        );

        db.put_thumb(&a, &sample_entry()).unwrap();
        assert!(db.get_thumb(&b).unwrap().is_none());
    }

    /// cache รุ่นเก่า (PK = hash เดี่ยว) ต้องถูกตรวจเจอแล้วสร้างใหม่
    /// ไม่ใช่ผ่านการตรวจแล้วไปพังตอน query จริง
    #[test]
    fn old_schema_is_detected_and_recreated() {
        let path = temp_db("oldschema");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE thumbs (
                     hash BLOB PRIMARY KEY, width INTEGER, height INTEGER,
                     format INTEGER, thumb_fmt INTEGER, thumb BLOB,
                     dominant INTEGER, last_used INTEGER, created_at INTEGER);",
            )
            .unwrap();
        }

        let db = CacheDb::open_or_recreate(&path).expect("ต้องสร้างใหม่ได้");
        let key = key_for("หลัง migrate".as_bytes(), 1, 1);
        db.put_thumb(&key, &sample_entry()).unwrap();
        assert!(db.get_thumb(&key).unwrap().is_some(), "ใช้งานต่อได้จริง");
    }

    // ---------- paths table ----------

    #[test]
    fn path_fingerprint_roundtrip() {
        let db = CacheDb::open_or_recreate(&temp_db("paths")).unwrap();
        let path = Path::new("C:/ภาพ/cat.png");
        let hash = crate::hash::hash_bytes(b"cat");
        let fp = PathFingerprint {
            mtime: 1_700_000_000,
            size: 12_345,
        };

        assert!(db.lookup_path(path, fp).unwrap().is_none());
        db.record_path(path, &hash, fp).unwrap();
        assert_eq!(db.lookup_path(path, fp).unwrap(), Some(hash));
    }

    /// ★ ไฟล์ถูกแก้ (mtime หรือ size เปลี่ยน) → ต้องไม่ใช้ hash เดิม
    /// ไม่งั้นผู้ใช้แก้ภาพแล้ว thumbnail ไม่อัปเดตตาม
    #[test]
    fn changed_file_invalidates_path_entry() {
        let db = CacheDb::open_or_recreate(&temp_db("changed")).unwrap();
        let path = Path::new("cat.png");
        let hash = crate::hash::hash_bytes(b"cat");
        let fp = PathFingerprint {
            mtime: 100,
            size: 500,
        };
        db.record_path(path, &hash, fp).unwrap();

        let newer_mtime = PathFingerprint { mtime: 101, ..fp };
        assert!(db.lookup_path(path, newer_mtime).unwrap().is_none());

        let different_size = PathFingerprint { size: 501, ..fp };
        assert!(db.lookup_path(path, different_size).unwrap().is_none());
    }

    // ---------- eviction (I-6) ----------

    #[test]
    fn eviction_removes_least_recently_used_first() {
        let db = CacheDb::open_or_recreate(&temp_db("evict")).unwrap();

        // ใส่ 40 รายการ ก้อนละ 32 KB
        let mut keys = Vec::new();
        for i in 0..40u32 {
            let key = key_for(&i.to_le_bytes(), i64::from(i), u64::from(i));
            let entry = ThumbEntry {
                thumb: vec![(i % 251) as u8; 32 * 1024],
                ..sample_entry()
            };
            db.put_thumb(&key, &entry).unwrap();
            keys.push(key);
        }
        let before = db.size_bytes().unwrap();
        assert!(before > 512 * 1024, "DB ทดสอบต้องใหญ่พอ ({before} ไบต์)");

        // แตะรายการสุดท้ายให้เป็นตัวที่ใช้ล่าสุด
        db.touch(&keys[39]).unwrap();

        let removed = db.evict_until_under(256 * 1024).unwrap();
        assert!(removed > 0, "ต้องลบอะไรออกบ้าง");
        assert!(
            db.size_bytes().unwrap() <= 512 * 1024,
            "ต้องเล็กลงจริง ไม่ใช่แค่ลบแถว"
        );
    }

    #[test]
    fn eviction_under_limit_does_nothing() {
        let db = CacheDb::open_or_recreate(&temp_db("noevict")).unwrap();
        db.put_thumb(&key_for(b"a", 1, 1), &sample_entry()).unwrap();
        assert_eq!(db.evict_until_under(64 << 20).unwrap(), 0);
        assert_eq!(db.thumb_count().unwrap(), 1);
    }

    // ---------- IO thread ----------

    #[test]
    fn io_thread_roundtrip() {
        let io = IoThread::spawn(&temp_db("io")).unwrap();
        let tx = io.sender().unwrap();
        let key = key_for(b"io test", 3, 4);

        tx.send(IoRequest::PutThumb {
            key,
            entry: Box::new(sample_entry()),
        })
        .unwrap();

        let (reply, rx) = crossbeam_channel::bounded(1);
        tx.send(IoRequest::GetThumb { key, reply }).unwrap();
        let entry = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .expect("ต้องเจอของที่เพิ่งเก็บ");
        assert_eq!(entry.width, 4000);
    }

    /// ผู้ขอเลิกสนใจ (ทิ้ง receiver) ต้องไม่ทำให้ IO thread ตาย
    /// เกิดจริงตลอดเวลาเวลาผู้ใช้ pan ผ่านภาพเร็ว ๆ
    #[test]
    fn io_thread_survives_dropped_receiver() {
        let io = IoThread::spawn(&temp_db("io-drop")).unwrap();
        let tx = io.sender().unwrap();
        let key = key_for(b"abandoned", 7, 8);

        {
            let (reply, rx) = crossbeam_channel::bounded(1);
            tx.send(IoRequest::GetThumb { key, reply }).unwrap();
            drop(rx); // เลิกสนใจทันที
        }

        // ต้องยังตอบคำสั่งถัดไปได้ตามปกติ
        let (reply, rx) = crossbeam_channel::bounded(1);
        tx.send(IoRequest::Stats { reply }).unwrap();
        let stats = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(stats.thumb_count, 0);
    }

    #[test]
    fn io_thread_reports_stats() {
        let io = IoThread::spawn(&temp_db("io-stats")).unwrap();
        let tx = io.sender().unwrap();
        for i in 0..3u32 {
            tx.send(IoRequest::PutThumb {
                key: key_for(&i.to_le_bytes(), i64::from(i), u64::from(i)),
                entry: Box::new(sample_entry()),
            })
            .unwrap();
        }
        let (reply, rx) = crossbeam_channel::bounded(1);
        tx.send(IoRequest::Stats { reply }).unwrap();
        let stats = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(stats.thumb_count, 3);
        assert!(stats.size_bytes > 0, "ต้องรายงานขนาดจริงให้ status bar");
    }

    /// ปิด IoThread ต้องจบเรียบร้อย ไม่ค้าง (drop → join)
    #[test]
    fn io_thread_shuts_down_cleanly() {
        let path = temp_db("io-shutdown");
        {
            let io = IoThread::spawn(&path).unwrap();
            let tx = io.sender().unwrap();
            tx.send(IoRequest::PutThumb {
                key: key_for(b"bye", 2, 3),
                entry: Box::new(sample_entry()),
            })
            .unwrap();
        } // drop ที่นี่ — ต้อง join สำเร็จ ไม่ค้าง

        // ข้อมูลต้องถูกเขียนลงจริงก่อนปิด
        let db = CacheDb::open_or_recreate(&path).unwrap();
        assert_eq!(db.thumb_count().unwrap(), 1);
    }

    /// เพดานเล็กจนลบหมดแล้วยังไม่พอ — ต้องหยุด ไม่ใช่วนไม่จบ
    #[test]
    fn eviction_with_impossible_limit_terminates() {
        let db = CacheDb::open_or_recreate(&temp_db("impossible")).unwrap();
        for i in 0..5u32 {
            db.put_thumb(
                &key_for(&i.to_le_bytes(), i64::from(i), u64::from(i)),
                &sample_entry(),
            )
            .unwrap();
        }
        // เพดาน 0 ไบต์ เป็นไปไม่ได้ (ไฟล์ sqlite เปล่ายังมี header)
        let removed = db.evict_until_under(0).unwrap();
        assert_eq!(removed, 5, "ต้องลบหมดแล้วหยุด");
        assert_eq!(db.thumb_count().unwrap(), 0);
    }
}
