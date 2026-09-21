//! ประตูของตารางเวลา — **คอมเมนต์ข้าง `cron:` ต้องตรงกับ `cron:` เสมอ**
//!
//! # ทำไมเทสต์นี้อยู่ที่นี่
//!
//! 21 ก.ย. 2026 · หัวไฟล์ `fuzz.yml` เขียนว่า "ตารางคือ จ/พ/ศ" และคอมเมนต์ท้าย
//! บรรทัด cron เขียนว่า "จันทร์ / พุธ / ศุกร์ 03:00 UTC" — ทั้งที่ `cron` จริงคือ
//! `0 3 * * 1` คือ **จันทร์อย่างเดียว** · ผิดมาตั้งแต่วันที่ลดความถี่
//!
//! นี่คือชนชั้นเดียวกับคอมเมนต์ `idle_produces_no_redraw (5 วินาที)` ที่ผิดไป
//! 120 เท่า: **แก้ของจริงแล้วไม่แก้คำบรรยาย** · และมันแย่กว่าคอมเมนต์ที่หายไป
//! เพราะคนอ่านเชื่อมัน แล้วไปสรุปเรื่องอื่นต่อจากตัวเลขที่ผิด
//!
//! # วิธีที่เลือก และสิ่งที่มันไม่ครอบคลุม
//!
//! เทสต์นี้ **สร้างคำบรรยายขึ้นจาก `cron` เอง** แล้วบังคับให้คอมเมนต์ท้ายบรรทัด
//! เท่ากันเป๊ะ · คนเขียนจึงไม่มีทางเขียนคำบรรยายที่ไม่ตรงได้เลย — ไม่ใช่เพราะ
//! มีใครคอยตรวจ แต่เพราะรูปแบบเดียวที่ผ่านคือรูปที่เครื่องสร้างเอง
//!
//! ★ สิ่งที่มัน **ไม่** ครอบคลุม: ร้อยแก้วที่อื่นในไฟล์ · บังคับด้วยเครื่องไม่ได้
//!   โดยไม่ห้ามการเล่าประวัติไปด้วย ("เคยเป็นทุกคืน แล้วลดเป็น จ/พ/ศ")
//!   → กฎที่ใช้แทนคือ **ร้อยแก้วต้องเป็นอดีตและมีวันที่กำกับเสมอ**
//!     ประโยคปัจจุบันกาลที่บอกตารางมีได้ที่เดียวคือคอมเมนต์ท้ายบรรทัด `cron`

// I-2 ("ห้าม fs บน UI thread") คุมเธรดของแอป · เทสต์ตัวนี้อ่านไฟล์ในทรีอย่างเดียว
#![expect(
    clippy::disallowed_methods,
    reason = "เทสต์ของ xtask — ไม่มี UI thread ให้บล็อก"
)]

use std::path::PathBuf;

fn workflows_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask อยู่ใต้ราก workspace")
        .join(".github/workflows")
}

const DAYS: [&str; 7] = ["อาทิตย์", "จันทร์", "อังคาร", "พุธ", "พฤหัส", "ศุกร์", "เสาร์"];

/// แปลง `cron` เป็นคำบรรยายภาษาไทยแบบเดียว — **ตัวสร้างความจริงตัวเดียว**
///
/// รองรับเท่าที่เราใช้จริง: นาที ชั่วโมง เป็นตัวเลข · วันที่/เดือนเป็น `*` ·
/// วันในสัปดาห์เป็น `*` หรือรายการตัวเลขคั่นจุลภาค
///
/// ★ คืน `Err` แทนที่จะเดา — cron รูปแบบที่ยังไม่รองรับต้องทำให้เทสต์ล้ม
///   ไม่ใช่ผ่านไปเงียบ ๆ · ประตูที่ยอมแพ้เมื่อเจอของแปลกคือประตูที่เปิดอยู่
fn describe(cron: &str) -> Result<String, String> {
    let f: Vec<&str> = cron.split_whitespace().collect();
    let [minute, hour, dom, month, dow] = f[..] else {
        return Err(format!("cron ต้องมีห้าช่อง แต่ได้ {} ช่อง: {cron:?}", f.len()));
    };
    if dom != "*" || month != "*" {
        return Err(format!("ยังไม่รองรับ cron ที่ระบุวันที่/เดือน: {cron:?}"));
    }
    let hour: u8 = hour
        .parse()
        .map_err(|_| format!("ชั่วโมงต้องเป็นตัวเลข: {cron:?}"))?;
    let minute: u8 = minute
        .parse()
        .map_err(|_| format!("นาทีต้องเป็นตัวเลข: {cron:?}"))?;

    let when = if dow == "*" {
        "ทุกวัน".to_string()
    } else {
        let mut names = Vec::new();
        for part in dow.split(',') {
            let n: usize = part
                .parse()
                .map_err(|_| format!("วันในสัปดาห์ต้องเป็นตัวเลข: {cron:?}"))?;
            // cron ยอมให้ 7 เป็นวันอาทิตย์เหมือน 0
            let n = if n == 7 { 0 } else { n };
            names.push(
                *DAYS
                    .get(n)
                    .ok_or_else(|| format!("วันในสัปดาห์นอกช่วง 0–7: {cron:?}"))?,
            );
        }
        names.join("/")
    };
    Ok(format!("{when} {hour:02}:{minute:02} UTC"))
}

/// เก็บ `(ไฟล์, บรรทัดที่, cron, คอมเมนต์ท้ายบรรทัด)` ของทุก `cron:` ในทรี
fn cron_lines() -> Vec<(String, usize, String, Option<String>)> {
    let mut out = Vec::new();
    let dir = workflows_dir();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("เปิดโฟลเดอร์ .github/workflows ไม่ได้")
        .map(|e| e.expect("อ่านรายการในโฟลเดอร์ไม่ได้").path())
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("yml" | "yaml")))
        .collect();
    files.sort();

    for path in files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let text = std::fs::read_to_string(&path).expect("อ่านไฟล์ workflow ไม่ได้");
        for (i, line) in text.lines().enumerate() {
            // ★ มองเฉพาะบรรทัดที่ **ตั้งค่า** cron จริง ไม่ใช่บรรทัดที่พูดถึงมัน
            //   (หัวไฟล์ fuzz.yml มีคำว่า cron อยู่ในร้อยแก้วด้วย)
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') || !line.contains("cron:") {
                continue;
            }
            let after = line.split("cron:").nth(1).unwrap_or_default();
            let Some(open) = after.find('"') else {
                continue;
            };
            let rest = &after[open + 1..];
            let Some(close) = rest.find('"') else {
                continue;
            };
            let cron = rest[..close].to_string();
            let tail = &rest[close + 1..];
            let comment = tail.find('#').map(|h| tail[h + 1..].trim().to_string());
            out.push((name.clone(), i + 1, cron, comment));
        }
    }
    out
}

#[test]
fn every_cron_has_a_comment_that_says_exactly_what_it_does() {
    let lines = cron_lines();
    assert!(
        !lines.is_empty(),
        "ไม่เจอบรรทัด cron สักบรรทัด — เทสต์นี้กำลังตรวจอากาศ"
    );

    let mut wrong = Vec::new();
    for (file, lineno, cron, comment) in &lines {
        let want = match describe(cron) {
            Ok(w) => w,
            Err(why) => {
                wrong.push(format!("{file}:{lineno} — {why}"));
                continue;
            }
        };
        match comment {
            None => wrong.push(format!(
                "{file}:{lineno} — `cron: \"{cron}\"` ไม่มีคอมเมนต์ · ต้องเป็น `# {want}`"
            )),
            Some(got) if got != &want => wrong.push(format!(
                "{file}:{lineno} — `cron: \"{cron}\"` คือ **{want}** แต่คอมเมนต์เขียนว่า **{got}**"
            )),
            Some(_) => {}
        }
    }

    assert!(
        wrong.is_empty(),
        "คอมเมนต์ของตารางเวลาไม่ตรงกับ cron:\n  {}\n\n\
         คอมเมนต์ท้ายบรรทัด cron ถูกสร้างจาก cron เอง — แก้คอมเมนต์ให้ตรง \
         หรือแก้ cron ถ้าคอมเมนต์คือสิ่งที่ตั้งใจ",
        wrong.join("\n  ")
    );
}

#[test]
fn the_description_maker_says_what_we_expect() {
    // ★ เทสต์ของตัวสร้างเอง — ถ้ามันเพี้ยน ประตูข้างบนจะบังคับความเพี้ยนนั้น
    //   ลงไปในทุกไฟล์แทนที่จะจับผิด
    assert_eq!(describe("0 3 * * 1").unwrap(), "จันทร์ 03:00 UTC");
    assert_eq!(describe("0 4 * * 1").unwrap(), "จันทร์ 04:00 UTC");
    assert_eq!(describe("0 2 * * 4").unwrap(), "พฤหัส 02:00 UTC");
    assert_eq!(describe("30 3 * * 1,3,5").unwrap(), "จันทร์/พุธ/ศุกร์ 03:30 UTC");
    assert_eq!(describe("0 0 * * *").unwrap(), "ทุกวัน 00:00 UTC");
    // 7 กับ 0 คือวันอาทิตย์เหมือนกันใน cron
    assert_eq!(
        describe("0 1 * * 7").unwrap(),
        describe("0 1 * * 0").unwrap()
    );
}

#[test]
fn the_description_maker_refuses_what_it_cannot_describe() {
    // ★ negative control: รูปแบบที่ยังไม่รองรับต้อง **ล้ม** ไม่ใช่เดาคำบรรยาย
    //   ประตูที่เงียบเมื่อเจอของที่อ่านไม่ออก คือประตูที่เปิดอยู่ตลอดเวลา
    for bad in [
        "0 3 * * 1-5",  // ช่วงวัน
        "*/15 * * * *", // ทุก 15 นาที
        "0 3 1 * *",    // ระบุวันที่ของเดือน
        "0 3 * 6 1",    // ระบุเดือน
        "0 3 * *",      // สี่ช่อง
        "0 3 * * 9",    // วันนอกช่วง
    ] {
        assert!(
            describe(bad).is_err(),
            "{bad:?} ต้องถูกปฏิเสธ ไม่ใช่ถูกบรรยายมั่ว ๆ"
        );
    }
}
