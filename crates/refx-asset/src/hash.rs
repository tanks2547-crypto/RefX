//! Content hash ด้วย blake3 — คีย์ของ cache ทั้งระบบ
//!
//! ใช้ **เนื้อไฟล์** เป็นคีย์ ไม่ใช่ path → ย้ายไฟล์/เปลี่ยนชื่อแล้ว thumbnail ไม่หาย
//! และไฟล์ซ้ำใช้ thumbnail ร่วมกันได้ (ARCHITECTURE §5)
//!
//! ไฟล์ใหญ่เกิน 64 MB ใช้ **fast path**: hash แค่ 1 MB แรก + 1 MB สุดท้าย + ขนาดไฟล์
//! เพราะการ hash ไฟล์ 4000px ใช้เวลาพอ ๆ กับอ่านมันทั้งไฟล์
//!
//! spec: docs/05-memory-and-assets.md §4

use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;

/// ไฟล์ที่ใหญ่กว่านี้ใช้ fast path
pub const FULL_HASH_LIMIT: u64 = 64 << 20; // 64 MB
/// ขนาดตัวอย่างที่อ่านจากหัวและท้ายไฟล์ตอนใช้ fast path
pub const SAMPLE_BYTES: u64 = 1 << 20; // 1 MB

/// ป้ายกำกับวิธี hash — ผสมเข้าไปใน hash ด้วยเพื่อไม่ให้สองวิธีชนกันได้เลย
///
/// ถ้าไม่มีป้ายนี้ ไฟล์เล็กที่มีเนื้อเท่ากับ "ตัวอย่าง" ของไฟล์ใหญ่จะได้ hash เดียวกัน
/// แล้ว cache จะคืน thumbnail ผิดภาพ
const TAG_FULL: u8 = 0x00;
const TAG_SAMPLED: u8 = 0x01;
const TAG_PASTED: u8 = 0x02;

/// hash ของเนื้อไฟล์ (blake3-256)
///
/// ★ ตัวชนิดย้ายลง `refx-core` แล้ว (docs/02 §2.2.5) เพราะ `AssetRef` ถือมันไว้
/// และ `refx-core` depend `blake3` ไม่ได้ — re-export กลับที่นี่เพื่อให้ call site
/// เดิมทั้งหมดยังเขียน `refx_asset::hash::ContentHash` ได้เหมือนเดิม
pub use refx_core::hash::ContentHash;

/// hash ไฟล์ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum HashError {
    /// อ่านไฟล์ไม่ได้
    #[error("cannot read {file} for hashing: {source}")]
    Io {
        /// ชื่อไฟล์ (ไม่ใช่ path เต็ม — docs/08 §5)
        file: String,
        /// สาเหตุ
        source: std::io::Error,
    },
}

/// hash เนื้อในทั้งก้อน (ใช้กับ clipboard หรือไฟล์เล็ก)
#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> ContentHash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[TAG_FULL]);
    hasher.update(bytes);
    ContentHash::from_bytes(*hasher.finalize().as_bytes())
}

/// ★★★ hash ของ **ภาพที่วางจาก clipboard** — คีย์ของ `AssetRef` และชื่อไฟล์ใน spool
///
/// ภาพที่วางไม่มีไฟล์ต้นทางให้ hash · คีย์ของมันจึงต้องมาจาก **พิกเซล** และ
/// ต้องคำนวณได้ **ก่อน** encode เป็น PNG เพราะ `docs/07 §2` ห้ามให้การ encode
/// (ระดับวินาที) มาขวางการที่ภาพขึ้นจอ ส่วนคีย์ต้องพร้อมตั้งแต่ตอนสร้าง item
///
/// ★★ **hash ทั้งก้อน ไม่ใช้ fast path แบบ [`hash_file`]** ถึงแม้ RGBA ของภาพ
/// 6000×4000 จะเป็น 91 MB ก็ตาม: fast path ปลอดภัยได้เพราะ cache key มี `mtime`
/// คร่อมจุดบอดไว้ (`HANDOFF §4` ข้อ 4) แต่ภาพที่วาง **ไม่มี mtime** — คีย์ที่ชนกัน
/// จึงแปลว่า *ภาพใบที่สองถูกกลืนหายไปเงียบ ๆ* ซึ่งคือ I-3 ตรง ๆ
///
/// ★ ผสมขนาดเข้าไปด้วย — ภาพ 2×1 กับ 1×2 ที่พิกเซลชุดเดียวกันคือคนละภาพ
#[must_use]
pub fn hash_pasted(width: u32, height: u32, rgba: &[u8]) -> ContentHash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[TAG_PASTED]);
    hasher.update(&width.to_le_bytes());
    hasher.update(&height.to_le_bytes());
    hasher.update(rgba);
    ContentHash::from_bytes(*hasher.finalize().as_bytes())
}

/// hash ไฟล์บนดิสก์ — เลือก full หรือ fast path ตามขนาดอัตโนมัติ
///
/// **ต้องเรียกบน worker thread เท่านั้น** — แตะดิสก์ (I-2)
///
/// # Errors
/// คืน [`HashError::Io`] เมื่ออ่านไฟล์ไม่ได้
pub fn hash_file(path: &Path) -> Result<ContentHash, HashError> {
    let label = || {
        path.file_name().map_or_else(
            || "(unknown file)".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    let io_err = |source: std::io::Error| HashError::Io {
        file: label(),
        source,
    };

    let mut file = std::fs::File::open(path).map_err(io_err)?;
    let size = file.metadata().map_err(io_err)?.len();

    if size <= FULL_HASH_LIMIT {
        // ไฟล์เล็ก — hash ทั้งไฟล์ blake3 มี SIMD เร็วพอ
        let mut hasher = blake3::Hasher::new();
        hasher.update(&[TAG_FULL]);
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buffer).map_err(io_err)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        return Ok(ContentHash::from_bytes(*hasher.finalize().as_bytes()));
    }

    // ---- fast path: หัว 1 MB + ท้าย 1 MB + ขนาด ----
    // โอกาสชนกันในทางปฏิบัติ ≈ 0 เพราะไฟล์ภาพสองไฟล์ที่มีหัวและท้ายเหมือนกันเป๊ะ
    // และขนาดเท่ากันเป๊ะ แทบจะแปลว่าเป็นไฟล์เดียวกันอยู่แล้ว
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[TAG_SAMPLED]);
    // ใส่ขนาดก่อน — ไฟล์ต่างขนาดต้องได้ hash ต่างกันเสมอ
    hasher.update(&size.to_le_bytes());

    let sample = usize::try_from(SAMPLE_BYTES).unwrap_or(usize::MAX);
    let mut buffer = vec![0u8; sample];

    file.seek(SeekFrom::Start(0)).map_err(io_err)?;
    file.read_exact(&mut buffer).map_err(io_err)?;
    hasher.update(&buffer);

    file.seek(SeekFrom::End(-(SAMPLE_BYTES as i64)))
        .map_err(io_err)?;
    file.read_exact(&mut buffer).map_err(io_err)?;
    hasher.update(&buffer);

    Ok(ContentHash::from_bytes(*hasher.finalize().as_bytes()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("refx-hash-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_temp(tag: &str, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = temp_dir(tag).join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// ข้อมูลแบบ deterministic ขนาดตามต้องการ
    fn pattern(len: usize, seed: u64) -> Vec<u8> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
            })
            .collect()
    }

    #[test]
    fn hash_is_stable_across_calls() {
        let data = pattern(4096, 7);
        let a = hash_bytes(&data);
        let b = hash_bytes(&data);
        assert_eq!(a, b, "hash เดียวกันต้องได้ผลเดิมเสมอ");
    }

    #[test]
    fn different_content_gives_different_hash() {
        assert_ne!(hash_bytes(b"cat"), hash_bytes(b"dog"));
    }

    #[test]
    fn file_and_bytes_agree_for_small_files() {
        let data = pattern(100_000, 3);
        let path = write_temp("agree", "a.bin", &data);
        assert_eq!(hash_file(&path).unwrap(), hash_bytes(&data));
    }

    #[test]
    fn empty_file_hashes_without_error() {
        let path = write_temp("empty", "e.bin", &[]);
        assert_eq!(hash_file(&path).unwrap(), hash_bytes(&[]));
    }

    #[test]
    fn hash_survives_rename() {
        // คีย์คือเนื้อไฟล์ ไม่ใช่ชื่อ — ย้าย/เปลี่ยนชื่อแล้ว thumbnail ต้องไม่หาย
        let data = pattern(2048, 11);
        let a = write_temp("rename", "ก่อนเปลี่ยนชื่อ.png", &data);
        let b = write_temp("rename", "หลังเปลี่ยนชื่อ.png", &data);
        assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
    }

    #[test]
    fn short_is_eight_hex_chars() {
        let h = hash_bytes(b"x");
        assert_eq!(h.short().len(), 8);
        assert_eq!(h.to_string().len(), 64);
    }

    #[test]
    fn roundtrip_through_bytes() {
        let h = hash_bytes(b"round trip");
        assert_eq!(ContentHash::from_bytes(*h.as_bytes()), h);
    }

    #[test]
    fn missing_file_is_error_not_panic() {
        let path = temp_dir("missing").join("ไม่มีจริง.bin");
        assert!(hash_file(&path).is_err());
    }

    // ---------- ★ ภาพที่วางจาก clipboard ----------

    /// ★★★ วางภาพเดิมซ้ำต้องได้ **คีย์เดียวกัน** — ไม่งั้น spool จะได้ไฟล์ละใบ
    #[test]
    fn pasting_the_same_pixels_twice_gives_the_same_key() {
        let rgba = pattern(64 * 48 * 4, 71);
        assert_eq!(hash_pasted(64, 48, &rgba), hash_pasted(64, 48, &rgba));
    }

    /// ★★ พิกเซลชุดเดียวกันแต่คนละขนาด = คนละภาพ
    ///
    /// ถ้าไม่ผสมขนาดเข้าไป ภาพ 2×1 กับ 1×2 จะกลายเป็นไฟล์เดียวกันใน spool
    /// แล้วใบที่สองจะถูกกลืนหายไปเงียบ ๆ (I-3)
    #[test]
    fn the_same_pixels_at_a_different_size_are_a_different_image() {
        let rgba = pattern(2 * 4, 73);
        assert_ne!(hash_pasted(2, 1, &rgba), hash_pasted(1, 2, &rgba));
    }

    /// ★★★ **hash ทั้งก้อน ไม่ใช่ fast path** — ภาพที่วางไม่มี mtime มาคร่อมจุดบอด
    ///
    /// เทสต์นี้เป็นภาพสะท้อนกลับด้านของ `fast_path_known_blind_spot_is_documented`
    /// ตรงนั้นยอมรับจุดบอดได้เพราะคีย์ของ cache มี `mtime` อยู่ด้วย ที่นี่ไม่มี —
    /// คีย์ที่ชนกันแปลว่า **ภาพที่ผู้ใช้วางใบที่สองหายไป** โดยไม่มีอะไรเตือน
    #[test]
    fn a_pasted_image_is_hashed_in_full_however_big_it_is() {
        let side = 2048u32; // 16 MB ของ RGBA — เกินช่วงหัว/ท้ายของ fast path
        let len = (side as usize) * (side as usize) * 4;
        assert!(
            len as u64 > FULL_HASH_LIMIT.min(2 * SAMPLE_BYTES),
            "เทสต์นี้จะไร้ความหมายถ้าภาพเล็กกว่าช่วงที่ fast path อ่าน"
        );

        let mut a = pattern(len, 79);
        let mut b = a.clone();
        let middle = len / 2;
        a[middle] = 0x00;
        b[middle] = 0xff;

        assert_ne!(
            hash_pasted(side, side, &a),
            hash_pasted(side, side, &b),
            "ต่างกันตรงกลางแล้วยังได้คีย์เดียวกัน = เดิน fast path อยู่"
        );
    }

    /// ★ คนละวิธี hash ต้องคนละค่า — ป้ายกำกับ (`TAG_*`) มีไว้เพื่อข้อนี้
    #[test]
    fn a_pasted_key_never_collides_with_a_file_key() {
        let bytes = pattern(16, 83);
        assert_ne!(hash_pasted(2, 2, &bytes), hash_bytes(&bytes));
    }

    // ---------- fast path ----------

    #[test]
    fn large_file_uses_fast_path_and_is_stable() {
        let size = (FULL_HASH_LIMIT + (4 << 20)) as usize; // 68 MB
        let data = pattern(size, 23);
        let path = write_temp("large", "big.bin", &data);

        let a = hash_file(&path).unwrap();
        let b = hash_file(&path).unwrap();
        assert_eq!(a, b, "fast path ต้อง deterministic");

        // ต้องต่างจาก full hash ของเนื้อเดียวกัน (คนละวิธี ต้องคนละค่า)
        assert_ne!(a, hash_bytes(&data));
    }

    /// ★ ไฟล์ใหญ่ที่ต่างกันแค่ **ตรงกลาง** — fast path มองไม่เห็น
    ///
    /// ยอมรับข้อจำกัดนี้ตาม spec แต่ต้องบันทึกไว้ให้ชัดว่ารู้ตัว
    /// ในทางปฏิบัติไฟล์ภาพสองไฟล์ที่หัว/ท้าย/ขนาดเท่ากันเป๊ะ = ไฟล์เดียวกัน
    #[test]
    fn fast_path_known_blind_spot_is_documented() {
        let size = (FULL_HASH_LIMIT + (4 << 20)) as usize;
        let mut a = pattern(size, 31);
        let mut b = a.clone();
        // แก้ไบต์ตรงกลาง (นอกช่วงที่ fast path อ่าน)
        let middle = size / 2;
        a[middle] = 0x00;
        b[middle] = 0xFF;

        let pa = write_temp("blind", "a.bin", &a);
        let pb = write_temp("blind", "b.bin", &b);
        assert_eq!(
            hash_file(&pa).unwrap(),
            hash_file(&pb).unwrap(),
            "จุดบอดที่รู้ตัวของ fast path — ถ้าวันไหนพฤติกรรมนี้เปลี่ยน ต้องรู้"
        );
    }

    #[test]
    fn large_files_differing_in_size_hash_differently() {
        let base = pattern((FULL_HASH_LIMIT + (4 << 20)) as usize, 41);
        let mut longer = base.clone();
        longer.extend_from_slice(&[0u8; 1024]);

        let pa = write_temp("size", "a.bin", &base);
        let pb = write_temp("size", "b.bin", &longer);
        assert_ne!(hash_file(&pa).unwrap(), hash_file(&pb).unwrap());
    }

    #[test]
    fn large_files_differing_at_tail_hash_differently() {
        let size = (FULL_HASH_LIMIT + (4 << 20)) as usize;
        let mut a = pattern(size, 53);
        let mut b = a.clone();
        let last = size - 1;
        a[last] = 0x01;
        b[last] = 0x02;

        let pa = write_temp("tail", "a.bin", &a);
        let pb = write_temp("tail", "b.bin", &b);
        assert_ne!(hash_file(&pa).unwrap(), hash_file(&pb).unwrap());
    }

    /// ★ ไฟล์ใหญ่ต้อง **ไม่ถูก hash ทั้งไฟล์** — วัดด้วยคุณสมบัติ ไม่ใช่ด้วยนาฬิกา
    ///
    /// เดิมเทสต์นี้ยืนยันว่า "ไฟล์ 100 MB ต้องเสร็จใน 200 ms" ซึ่งขึ้นกับความเร็ว
    /// ของเครื่องที่รัน — **แดงสุ่มบน CI** (เกิดจริง 3 ส.ค. 2026 บน ubuntu runner
    /// แล้วรอบถัดมาเขียวทั้งที่โค้ดเหมือนเดิมเป๊ะ) นี่คือกับดักเดียวกับที่เคย
    /// ต้องย้ายเทสต์คิวออกจากการวัดเวลามาแล้วครั้งหนึ่ง
    ///
    /// สิ่งที่ต้องคุมจริง ๆ คือ **ไฟล์ใหญ่เดิน fast path** ซึ่งเป็นคุณสมบัติเชิง
    /// อัลกอริทึม: ถ้าอ่านแค่หัว/ท้าย/ขนาด การแก้เนื้อ *ตรงกลาง* จะไม่ทำให้ hash เปลี่ยน
    /// ข้อนี้เท่ากันทุกเครื่องและจะแดงทันทีถ้าใครดัน `FULL_HASH_LIMIT` ขึ้นไป
    /// จนไฟล์ขนาดนี้กลายเป็น full hash (ซึ่งคือสิ่งที่ทำให้ช้าจริง)
    #[test]
    fn a_file_over_the_limit_never_gets_hashed_in_full() {
        let size = 100 * (1 << 20);
        assert!(
            size as u64 > FULL_HASH_LIMIT,
            "เทสต์นี้จะไร้ความหมายถ้าไฟล์ไม่เกินเพดาน fast path"
        );

        let data = pattern(size, 67);
        let path = write_temp("speed", "100mb.bin", &data);

        let start = std::time::Instant::now();
        let hash = hash_file(&path).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(hash, hash_file(&path).unwrap(), "ต้อง deterministic");

        // แก้ไบต์ตรงกลาง (นอกช่วงหัว/ท้ายที่ fast path อ่าน) แล้ว hash ต้องไม่เปลี่ยน
        // — เป็นไปได้ก็ต่อเมื่อมันไม่ได้อ่านทั้ง 100 MB
        let mut middled = data;
        let middle = size / 2;
        middled[middle] ^= 0xff;
        let other = write_temp("speed", "100mb-middle.bin", &middled);
        assert_eq!(
            hash,
            hash_file(&other).unwrap(),
            "แก้กลางไฟล์แล้ว hash เปลี่ยน = อ่านทั้งไฟล์ ไม่ได้เดิน fast path"
        );

        // ตัวเลขไว้เทียบรุ่นต่อไป — **ไม่ assert** ดูเหตุผลข้างบน
        println!("hash ไฟล์ 100 MB (fast path): {elapsed:?}");
    }
}
