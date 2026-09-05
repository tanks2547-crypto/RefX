//! `keymap.toml` — **ชั้นอ่านไฟล์เท่านั้น** (P5-3b ก้อน b)
//!
//! spec: `docs/03-modes-and-ui.md §5` · `ROADMAP` P5-3b
//!
//! ## ★ ทำไมชั้นนี้ไม่รู้จักคำว่า "ปุ่ม" หรือ "action" เลย
//!
//! ชนิดจริงของคีย์ลัด (`Chord` · `Action` · `Mods`) อยู่ที่ `refx-ui::keymap`
//! เพราะมันอ้าง `winit::keyboard::NamedKey` และ `refx_core::interact::Tool` —
//! `refx-io` พึ่ง `winit` ไม่ได้ (ARCHITECTURE §2 · และ `fuzz/` จะลาก GUI ตามไป
//! ทั้งกอง — `HANDOFF §4` ข้อ 18)
//!
//! → ที่นี่จึงอ่านไฟล์ออกมาเป็น **สตริงดิบ** พร้อมเลขบรรทัดที่มันอยู่
//! แล้วให้ชั้นบนแปลเป็นชนิดจริงเอง · รูปแบบเดียวกับ `dto` ที่แยก DTO ออกจาก
//! ชนิดใน `refx-core`
//!
//! ## ★★★ ไฟล์นี้คือ input ที่ไม่น่าไว้ใจ (I-4)
//!
//! ผู้ใช้แก้เองด้วยมือได้ และคนอื่นส่งไฟล์ให้เขาก็ได้ · ทุกเพดานที่นี่มีไว้ให้
//! **จำนวนงานที่เราทำไม่ขึ้นกับตัวเลขในไฟล์**: ขนาดไฟล์ · จำนวนแถว ·
//! ความยาวของสตริงแต่ละตัว
//!
//! ## ★★ พังที่ไหนก็ใช้ค่าปริยาย **ทั้งไฟล์**
//!
//! ห้ามใช้ครึ่งเดียวเงียบ ๆ — ผู้ใช้ที่พิมพ์ผิดบรรทัดเดียวแล้วได้คีย์ลัดหายไป
//! ครึ่งหนึ่งจะหาสาเหตุไม่เจอ และจะสรุปว่าโปรแกรมทำงานหาย (`ROADMAP` P5-3)

use std::path::Path;

/// เพดานขนาดไฟล์ — 64 KB (เท่ากับ `settings.toml`)
///
/// ไฟล์จริงยาวไม่กี่สิบบรรทัด · เพดานนี้คือสิ่งที่ทำให้ `Vec` ข้างล่างโตได้
/// ไม่เกินขนาดที่คาดเดาได้ **โดยไม่ต้องเชื่อตัวเลขใด ๆ ในไฟล์**
pub const MAX_BYTES: u64 = 64 << 10;

/// จำนวนแถวสูงสุดที่ยอมรับ
///
/// ตารางค่าปริยายมี 38 แถว · 256 เผื่อไว้เกินพอสำหรับคนที่ผูกทุกปุ่มบนคีย์บอร์ด
/// · เกินกว่านี้ไม่ใช่ keymap ของคนอีกต่อไป
pub const MAX_BINDS: usize = 256;

/// ความยาวสูงสุดของสตริงหนึ่งตัว (`keys` หรือ `action`)
///
/// `"ctrl+shift+backspace"` ยาว 20 · 64 เผื่อไว้เกินพอ · มีไว้กันไฟล์ที่ยัด
/// สตริงยาวเป็นกิโลไบต์เข้ามาให้เราแบกไว้ในหน่วยความจำและในข้อความ error
pub const MAX_TOKEN: usize = 64;

/// หนึ่งแถวในไฟล์ — **ยังเป็นสตริงดิบ ยังไม่ถูกแปลเป็นปุ่มจริง**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawBind {
    /// ลำดับที่ในไฟล์ เริ่มที่ 1 — ★ ใช้บอกผู้ใช้ว่า **แถวไหน** ผิดหรือชนกัน
    pub row: usize,
    /// ปุ่มตามที่เขาเขียน เช่น `"ctrl+shift+z"`
    pub keys: String,
    /// ชื่อ action ตามที่เขาเขียน เช่น `"redo"`
    pub action: String,
    /// กดค้างแล้วสั่งซ้ำได้ไหม — ไม่ใส่ = `false`
    pub repeat: bool,
}

/// อ่าน `keymap.toml` ไม่สำเร็จ — ★ ทุก variant บอก **ที่ที่ผิด** ไม่ใช่แค่ว่าผิด
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    /// อ่านไฟล์จากดิสก์ไม่ได้ (ถูกลบ/ไม่มีสิทธิ์/ไม่ใช่ UTF-8)
    #[error("cannot read keymap.toml: {detail}")]
    Unreadable {
        /// สิ่งที่ระบบไฟล์บ่น (อังกฤษ สำหรับ log)
        detail: String,
    },
    /// ไฟล์ใหญ่เกินเพดาน — ไม่แตะเนื้อในเลย
    #[error("keymap.toml is {size} bytes, limit is {MAX_BYTES}")]
    TooLarge {
        /// ขนาดจริง
        size: u64,
    },
    /// TOML ไม่ถูกไวยากรณ์ หรือรูปร่างไม่ตรง
    #[error("keymap.toml is not valid TOML: {detail}")]
    Malformed {
        /// ข้อความของ parser (มีเลขบรรทัดอยู่ในนั้นแล้ว)
        detail: String,
    },
    /// แถวเยอะเกินเพดาน
    #[error("keymap.toml has {count} bindings, limit is {MAX_BINDS}")]
    TooManyBinds {
        /// จำนวนแถวจริง
        count: usize,
    },
    /// สตริงในแถวหนึ่งยาวเกินเพดาน
    #[error("row {row}: a value is longer than {MAX_TOKEN} characters")]
    TokenTooLong {
        /// แถวที่ผิด (เริ่มที่ 1)
        row: usize,
    },
    /// แถวที่ไม่มี `keys` หรือ `action` หรือปล่อยว่าง
    #[error("row {row}: both `keys` and `action` must be given and non-empty")]
    Incomplete {
        /// แถวที่ผิด (เริ่มที่ 1)
        row: usize,
    },
}

/// รูปร่างของไฟล์ตามที่ serde อ่าน
///
/// ★ `deny_unknown_fields` **ที่นี่เข้มกว่า `settings.toml` โดยตั้งใจ** —
/// คีย์แปลกใน `settings.toml` แปลว่าค่าหนึ่งค่าไม่ถูกใช้ ส่วนคีย์แปลกใน
/// `keymap.toml` แปลว่าผู้ใช้เข้าใจรูปแบบผิด แล้ว**ปุ่มทั้งแถวจะเงียบ**
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    bind: Vec<RawRow>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRow {
    keys: Option<String>,
    action: Option<String>,
    #[serde(default)]
    repeat: bool,
}

/// อ่าน `keymap.toml` จากดิสก์
///
/// `Ok(None)` = **ไม่มีไฟล์** ซึ่งเป็นสภาพปกติ ไม่ใช่ error ·
/// `Ok(Some(rows))` = อ่านได้ครบและผ่านเพดานทุกข้อ (ยังไม่ได้แปลเป็นปุ่มจริง) ·
/// `Err` = พัง — ผู้เรียกต้องใช้ค่าปริยาย **ทั้งชุด** แล้วบอกผู้ใช้
///
/// **ห้ามเรียกจาก UI thread** — แตะดิสก์ (I-2)
///
/// # Errors
/// [`KeymapError`] เมื่ออ่านไม่ได้ · ใหญ่เกินเพดาน · ไวยากรณ์ผิด ·
/// แถวเกินเพดาน · สตริงยาวเกินเพดาน · หรือแถวไม่ครบ
pub fn load(path: &Path) -> Result<Option<Vec<RawBind>>, KeymapError> {
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(KeymapError::Unreadable {
                detail: err.to_string(),
            });
        }
    };
    // ★ ถามขนาดก่อนเปิด แล้วอ่านผ่าน `take` — รูปแบบเดียวกับ `settings::load`
    //   (`clippy.toml` แบน `fs::read_to_string` ไว้ด้วยเหตุผลนี้)
    if size > MAX_BYTES {
        return Err(KeymapError::TooLarge { size });
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|file| {
            use std::io::Read as _;
            std::io::Read::take(file, MAX_BYTES).read_to_string(&mut text)
        })
        .map_err(|err| KeymapError::Unreadable {
            detail: err.to_string(),
        })?;
    parse(&text).map(Some)
}

/// แปลงเนื้อไฟล์เป็นแถวดิบ — ★ **ฟังก์ชันบริสุทธิ์** เทสต์ได้โดยไม่แตะดิสก์
///
/// # Errors
/// [`KeymapError`] — ดู [`load`]
pub fn parse(text: &str) -> Result<Vec<RawBind>, KeymapError> {
    let file: RawFile = basic_toml::from_str(text).map_err(|err| KeymapError::Malformed {
        detail: err.to_string(),
    })?;

    // ★ ตรวจจำนวนแถว **หลัง** parse ได้เพราะ `MAX_BYTES` คุมขนาดที่ `Vec` โตได้
    //   อยู่แล้ว — เราไม่เคยจองตามตัวเลขที่ไฟล์บอก มีแต่โตตามของที่มีจริง
    if file.bind.len() > MAX_BINDS {
        return Err(KeymapError::TooManyBinds {
            count: file.bind.len(),
        });
    }

    let mut rows = Vec::with_capacity(file.bind.len());
    for (index, row) in file.bind.into_iter().enumerate() {
        let at = index + 1;
        let (Some(keys), Some(action)) = (row.keys, row.action) else {
            return Err(KeymapError::Incomplete { row: at });
        };
        if keys.chars().count() > MAX_TOKEN || action.chars().count() > MAX_TOKEN {
            return Err(KeymapError::TokenTooLong { row: at });
        }
        let keys = keys.trim().to_owned();
        let action = action.trim().to_owned();
        if keys.is_empty() || action.is_empty() {
            return Err(KeymapError::Incomplete { row: at });
        }
        rows.push(RawBind {
            row: at,
            keys,
            action,
            repeat: row.repeat,
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const GOOD: &str = "\
[[bind]]
keys   = \"ctrl+z\"
action = \"undo\"
repeat = true

[[bind]]
keys   = \"ctrl+shift+z\"
action = \"redo\"
";

    #[test]
    fn a_well_formed_file_keeps_its_row_numbers() {
        let rows = parse(GOOD).unwrap();
        assert_eq!(
            rows,
            vec![
                RawBind {
                    row: 1,
                    keys: "ctrl+z".to_owned(),
                    action: "undo".to_owned(),
                    repeat: true,
                },
                RawBind {
                    row: 2,
                    keys: "ctrl+shift+z".to_owned(),
                    action: "redo".to_owned(),
                    repeat: false,
                },
            ]
        );
    }

    /// ★★★ ทุกแบบที่ไฟล์พังได้ ต้อง **บอกที่ที่ผิด** ไม่ใช่แค่ว่าผิด
    ///
    /// ผู้ใช้ที่ได้ข้อความว่า "keymap.toml ผิด" เฉย ๆ ต้องไล่อ่านทั้งไฟล์เอง ·
    /// เลขแถวคือสิ่งเดียวที่ทำให้เขาแก้ได้ในสามสิบวินาที
    #[test]
    fn every_broken_shape_names_where_it_went_wrong() {
        // ไวยากรณ์พัง — parser ใส่เลขบรรทัดมาให้ในข้อความอยู่แล้ว
        assert!(matches!(
            parse("[[bind]\nkeys = \"z\""),
            Err(KeymapError::Malformed { .. })
        ));
        // ขาด action
        assert_eq!(
            parse("[[bind]]\nkeys = \"z\""),
            Err(KeymapError::Incomplete { row: 1 })
        );
        // แถวที่สองขาด keys — ต้องชี้แถว 2 ไม่ใช่แถว 1
        assert_eq!(
            parse("[[bind]]\nkeys=\"z\"\naction=\"undo\"\n[[bind]]\naction=\"redo\""),
            Err(KeymapError::Incomplete { row: 2 })
        );
        // ว่างเปล่าหลัง trim ก็คือไม่ครบ
        assert_eq!(
            parse("[[bind]]\nkeys = \"   \"\naction = \"undo\""),
            Err(KeymapError::Incomplete { row: 1 })
        );
        // คีย์ที่เราไม่รู้จักในรูปแบบไฟล์ — แถวจะเงียบทั้งแถวถ้าปล่อยผ่าน
        assert!(matches!(
            parse("[[bind]]\nkeys=\"z\"\naction=\"undo\"\nwhen=\"canvas\""),
            Err(KeymapError::Malformed { .. })
        ));
        assert!(matches!(
            parse("keys = \"z\""),
            Err(KeymapError::Malformed { .. })
        ));
    }

    /// ★★ ไฟล์ว่าง = ไม่ผูกอะไรเลย **ไม่ใช่ error**
    ///
    /// แยกจาก "ไม่มีไฟล์" ที่ [`load`] คืน `Ok(None)` — ไฟล์ว่างคือผู้ใช้บอกว่า
    /// "ฉันตั้งใจไม่ผูกอะไร" ส่วนไม่มีไฟล์คือ "ยังไม่เคยแตะ" · ชั้นบนเป็นคน
    /// ตัดสินว่าสองอย่างนี้ต่างกันยังไง ชั้นนี้แค่ต้องไม่กลืนความต่างทิ้ง
    #[test]
    fn an_empty_file_is_not_an_error() {
        assert_eq!(parse("").unwrap(), Vec::new());
        assert_eq!(parse("# ยังไม่ได้ตั้งอะไร\n").unwrap(), Vec::new());
    }

    /// ★★★ I-4: จำนวนงานที่เราทำต้องไม่ขึ้นกับตัวเลขในไฟล์
    #[test]
    fn a_file_can_never_make_us_hold_more_than_the_caps_allow() {
        // แถวเกินเพดาน
        let many = (0..=MAX_BINDS)
            .map(|i| format!("[[bind]]\nkeys=\"{i}\"\naction=\"undo\"\n"))
            .collect::<String>();
        assert_eq!(
            parse(&many),
            Err(KeymapError::TooManyBinds {
                count: MAX_BINDS + 1
            })
        );
        // เท่าเพดานพอดีต้องผ่าน — เพดานที่แคบไปหนึ่งจะทำให้ไฟล์ที่ถูกต้องถูกปฏิเสธ
        let exactly = (0..MAX_BINDS)
            .map(|i| format!("[[bind]]\nkeys=\"{i}\"\naction=\"undo\"\n"))
            .collect::<String>();
        assert_eq!(parse(&exactly).unwrap().len(), MAX_BINDS);

        // สตริงยาวเกินเพดาน — ทั้งฝั่ง keys และ action
        let long = "z".repeat(MAX_TOKEN + 1);
        assert_eq!(
            parse(&format!("[[bind]]\nkeys=\"{long}\"\naction=\"undo\"")),
            Err(KeymapError::TokenTooLong { row: 1 })
        );
        assert_eq!(
            parse(&format!("[[bind]]\nkeys=\"z\"\naction=\"{long}\"")),
            Err(KeymapError::TokenTooLong { row: 1 })
        );
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-keymap-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// ★★ สภาพที่ `parse(&str)` เห็นไม่ได้เลย — ต้องมีเทสต์ที่แตะดิสก์จริง
    #[test]
    fn the_shapes_only_a_disk_can_produce() {
        let dir = temp_dir("disk");

        // ไม่มีไฟล์ = สภาพปกติ ไม่ใช่ error
        assert_eq!(load(&dir.join("nope.toml")).unwrap(), None);

        // ใหญ่เกินเพดาน — ต้องปฏิเสธ **โดยไม่แตะเนื้อใน**
        let huge = dir.join("huge.toml");
        std::fs::write(&huge, "# ".repeat((MAX_BYTES as usize) + 1)).unwrap();
        assert!(matches!(load(&huge), Err(KeymapError::TooLarge { .. })));

        // ไม่ใช่ UTF-8
        let binary = dir.join("binary.toml");
        std::fs::write(&binary, [0xFF_u8, 0xFE, 0x00, 0x80]).unwrap();
        assert!(matches!(load(&binary), Err(KeymapError::Unreadable { .. })));

        // ไฟล์ที่ถูกต้องต้องเดินผ่านเส้นทางเดียวกันแล้วได้ของครบ
        let good = dir.join("good.toml");
        std::fs::write(&good, GOOD).unwrap();
        assert_eq!(load(&good).unwrap().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
