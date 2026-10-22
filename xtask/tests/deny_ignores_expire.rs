//! ประตูของข้อยกเว้นใน `deny.toml` — **ignore ทุกตัวต้องมีวันหมดอายุ และแดงเมื่อเลยวันนั้น**
//!
//! # ทำไมเทสต์นี้อยู่ที่นี่
//!
//! 22 ต.ค. 2026 · `cargo deny` ยิงของจริงครั้งแรก: RUSTSEC-2026-0374 (`basic-toml`
//! unmaintained) · เจ้าของโปรเจกต์ตัดสินให้ ignore ไปก่อนแล้วย้ายใน v1.1 (`docs/09`)
//! โดยมีเงื่อนไขว่า **ignore ที่ไม่มีวันหมดอายุ = ประตูที่ถูกปิดถาวร** (`docs/08 §3.9
//! ข้อ 12`)
//!
//! cargo-deny 0.20 รับแค่ `id` กับ `reason` (ลองใส่ `expires` แล้วได้ `unexpected-keys`)
//! → วันหมดอายุจึงอยู่ใน `reason` เป็น `review by YYYY-MM-DD` และเทสต์นี้คือตัวที่บังคับมัน
//!
//! ★ เทสต์นี้ **ขึ้นกับวันที่โดยตั้งใจ** — นั่นคือหน้าที่ทั้งหมดของมัน · ส่วนตรรกะ
//!   (อ่านวัน · เทียบวัน · แปลงวัน) ถูกทดสอบแยกด้วยวันที่ตายตัวข้างล่าง

#![expect(
    clippy::disallowed_methods,
    reason = "เทสต์ของ xtask — ไม่มี UI thread ให้บล็อก"
)]

use std::path::PathBuf;

const MARK: &str = "review by ";

fn deny_toml() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask อยู่ใต้ราก workspace")
        .join("deny.toml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("อ่าน {} ไม่ได้: {err}", path.display()))
}

/// วันที่แบบเทียบได้ (ปี, เดือน, วัน)
type Date = (i64, u32, u32);

/// `YYYY-MM-DD` → [`Date`] · ★ คืน `Err` แทนการเดา — วันที่ที่อ่านไม่ออกต้องทำให้ประตูแดง
fn parse_date(text: &str) -> Result<Date, String> {
    let parts: Vec<&str> = text.split('-').collect();
    let [y, m, d] = parts[..] else {
        return Err(format!("วันที่ต้องเป็น YYYY-MM-DD: {text:?}"));
    };
    let y: i64 = y.parse().map_err(|_| format!("ปีอ่านไม่ออก: {text:?}"))?;
    let m: u32 = m.parse().map_err(|_| format!("เดือนอ่านไม่ออก: {text:?}"))?;
    let d: u32 = d.parse().map_err(|_| format!("วันอ่านไม่ออก: {text:?}"))?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y.to_string().len() != 4 {
        return Err(format!("วันที่ไม่มีจริง: {text:?}"));
    }
    Ok((y, m, d))
}

/// ทุกบรรทัดที่มี `id = "RUSTSEC-` → (id, วันทบทวน) · ไม่มีวัน = `Err`
fn ignores(toml: &str) -> Vec<(String, Result<Date, String>)> {
    toml.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| {
            let start = line.find("id = \"")? + "id = \"".len();
            let id = line[start..].split('"').next()?.to_owned();
            let date = match line.find(MARK) {
                Some(at) => parse_date(
                    line[at + MARK.len()..]
                        .split(|c: char| !(c.is_ascii_digit() || c == '-'))
                        .next()
                        .unwrap_or_default(),
                ),
                None => Err(format!("{id}: reason ไม่มี \"{MARK}YYYY-MM-DD\"")),
            };
            Some((id, date))
        })
        .collect()
}

/// วันนับจาก 1970-01-01 → วันที่ตามปฏิทิน (อัลกอริทึม civil_from_days ของ Howard Hinnant)
fn civil_from_days(days: i64) -> Date {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        u32::try_from(m).unwrap_or(0),
        u32::try_from(d).unwrap_or(0),
    )
}

fn today_utc() -> Date {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("นาฬิกาของเครื่องอยู่ก่อนปี 1970")
        .as_secs();
    civil_from_days(i64::try_from(secs / 86_400).expect("วันพอดี i64"))
}

/// ★★★ **ประตู:** ทุก ignore มีวันทบทวน และวันนี้ยังไม่เลยวันนั้น
#[test]
fn every_ignored_advisory_has_a_review_date_that_has_not_passed() {
    let found = ignores(&deny_toml());
    let today = today_utc();
    let mut problems = Vec::new();
    for (id, date) in &found {
        match date {
            Err(why) => problems.push(why.clone()),
            Ok(date) if *date < today => problems.push(format!(
                "{id}: เลยวันทบทวน {:04}-{:02}-{:02} แล้ว — ย้ายออกจาก crate นั้น หรือให้เจ้าของโปรเจกต์ตัดสินวันใหม่",
                date.0, date.1, date.2
            )),
            Ok(_) => {}
        }
    }
    assert!(
        problems.is_empty(),
        "ข้อยกเว้นใน deny.toml:\n  {}",
        problems.join("\n  ")
    );
}

/// ★ NC ของตัวอ่าน: ไม่มีวัน · วันพัง · วันที่ผ่านไปแล้ว ต้องถูกจับทั้งหมด
#[test]
fn the_reader_catches_a_missing_a_broken_and_a_past_date() {
    let toml = r#"
# { id = "RUSTSEC-0000-0000", reason = "คอมเมนต์ไม่นับ" },
ignore = [
    { id = "RUSTSEC-2026-0001", reason = "no date at all" },
    { id = "RUSTSEC-2026-0002", reason = "x - review by 2027-13-01" },
    { id = "RUSTSEC-2026-0003", reason = "x - review by 2020-01-31" },
    { id = "RUSTSEC-2026-0004", reason = "x - review by 2027-01-31" },
]
"#;
    let found = ignores(toml);
    let ids: Vec<&str> = found.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "RUSTSEC-2026-0001",
            "RUSTSEC-2026-0002",
            "RUSTSEC-2026-0003",
            "RUSTSEC-2026-0004"
        ]
    );
    assert!(found[0].1.is_err());
    assert!(found[1].1.is_err());
    let today: Date = (2026, 10, 22);
    assert!(
        found[2].1.clone().unwrap() < today,
        "วันที่ผ่านไปแล้วต้องน้อยกว่าวันนี้"
    );
    assert!(found[3].1.clone().unwrap() >= today);
}

/// ตัวแปลงวันต้องถูกที่จุดที่พลาดง่าย — ต้นยุค · ปีอธิกสุรทิน · วันนี้
#[test]
fn the_calendar_conversion_is_right_where_it_is_easy_to_get_wrong() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(59), (1970, 3, 1));
    assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    assert_eq!(civil_from_days(20_748), (2026, 10, 22));
}
