//! ประตูของ `scripts/ui-drive.ps1` — รันทุก push พร้อมชุดเทสต์ปกติ
//!
//! # ทำไมเทสต์นี้อยู่ที่นี่
//!
//! 17 ก.ย. 2026 · `ui-drive.ps1` อ่านแค่ `$parts[2]` แล้วทิ้งฟิลด์ที่สี่เงียบ ๆ
//! การทดลอง device lost ทั้งรอบจึงไม่เคยเกิดขึ้น แต่ถูกรายงานว่า "ผ่าน"
//! (`docs/08 §3.9` ข้อ 9) · สคริปต์ตอนนี้มีตาราง `$ARITY` ที่ล้มเมื่อได้ของเกิน
//! **แต่ตารางนั้นกับ `switch` เป็นสองรายการที่ต้องตรงกันเอง** และไม่มีอะไรบังคับ
//! ให้ตรง — ถ้าใครเพิ่ม verb ลงข้างเดียว บานประตูก็เปิดอีกครั้งเงียบ ๆ
//!
//! เทสต์นี้คือสิ่งที่บังคับ · และมันอ่านตัวสคริปต์จริง ไม่ใช่สำเนาของตาราง —
//! เทสต์ที่ถือสำเนาไว้เองจะเขียวต่อไปได้แม้สคริปต์เปลี่ยน (ข้อ 9 เหมือนกัน)
//!
//! ข้อสองคือการตรวจย้อนหลัง: ไม่ให้มี "หลักฐาน" ที่สั่งด้วยฟิลด์เกินหลงเหลือ
//! อยู่ในทรี เพราะรันครั้งนั้นไม่ได้ทำสิ่งที่เขียนไว้

// I-2 ("ห้าม fs บน UI thread") คุมเธรดของแอป · เทสต์ตัวนี้ไม่มี UI thread และ
// งานทั้งหมดของมันคือการอ่านไฟล์ในทรี
#![expect(
    clippy::disallowed_methods,
    reason = "เทสต์ของ xtask — ไม่มี UI thread ให้บล็อก"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask อยู่ใต้ราก workspace")
        .to_path_buf()
}

fn script() -> String {
    let path = root().join("scripts").join("ui-drive.ps1");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("อ่าน {} ไม่ได้: {err}", path.display()))
}

/// ชื่อที่อยู่ในเครื่องหมายคำพูดเดี่ยวตัวแรกของบรรทัด
fn quoted(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('\'')?;
    let end = rest.find('\'')?;
    Some(&rest[..end])
}

/// `verb -> (อย่างน้อย, อย่างมาก)` จากตาราง `$ARITY` ในสคริปต์จริง
fn arity(text: &str) -> BTreeMap<String, (usize, usize)> {
    let mut table = BTreeMap::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("$ARITY = @{") {
            inside = true;
            continue;
        }
        if inside {
            if line.starts_with('}') {
                break;
            }
            let Some(verb) = quoted(line) else { continue };
            let Some(open) = line.find("@(") else {
                continue;
            };
            let Some(close) = line[open..].find(')') else {
                continue;
            };
            let nums: Vec<usize> = line[open + 2..open + close]
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            assert_eq!(nums.len(), 2, "แถว {verb} ของ $ARITY ต้องมีสองตัวเลข");
            table.insert(verb.to_owned(), (nums[0], nums[1]));
        }
    }
    assert!(!table.is_empty(), "หาตาราง $ARITY ในสคริปต์ไม่เจอ");
    table
}

/// verb ทุกตัวที่ `switch` มีแขนรองรับจริง
fn handled(text: &str) -> BTreeMap<String, ()> {
    let mut verbs = BTreeMap::new();
    let mut inside = false;
    for line in text.lines() {
        if line.contains("switch ($parts[0]) {") {
            inside = true;
            continue;
        }
        if inside {
            if line.trim_start().starts_with("default") {
                break;
            }
            // แขนของ switch เขียนเป็น  'verb' { ... }  ขึ้นต้นบรรทัดเสมอ
            if let Some(verb) = quoted(line)
                && line.contains('{')
            {
                verbs.insert(verb.to_owned(), ());
            }
        }
    }
    assert!(!verbs.is_empty(), "หา switch ของ step ในสคริปต์ไม่เจอ");
    verbs
}

/// ★★★ ตารางกับ `switch` ต้องเป็นรายการเดียวกัน
///
/// ถ้าตารางขาด verb ที่ `switch` รองรับ → step นั้นถูกปฏิเสธว่า "ไม่รู้จัก"
/// ถ้า `switch` ขาด verb ที่ตารางมี → step นั้นผ่านด่านแล้วไปตกที่ `default`
#[test]
fn the_arity_table_and_the_switch_list_the_same_steps() {
    let text = script();
    let table = arity(&text);
    let arms = handled(&text);

    let missing_in_table: Vec<_> = arms.keys().filter(|v| !table.contains_key(*v)).collect();
    let missing_in_switch: Vec<_> = table.keys().filter(|v| !arms.contains_key(*v)).collect();
    assert!(
        missing_in_table.is_empty(),
        "switch รองรับ step ที่ไม่มีใน $ARITY: {missing_in_table:?} — step พวกนี้จะถูกปฏิเสธก่อนถึงแขนของมัน"
    );
    assert!(
        missing_in_switch.is_empty(),
        "$ARITY มี step ที่ switch ไม่รองรับ: {missing_in_switch:?} — จะผ่านด่านแล้วไปตกที่ default"
    );
}

/// จำนวนฟิลด์ที่แขนของ `switch` อ่านจริง (`$parts[n]` ตัวสูงสุด + 1)
fn fields_read(text: &str, verb: &str) -> Option<usize> {
    let mut inside = false;
    let mut depth = 0i32;
    let mut top = 0usize;
    for line in text.lines() {
        if !inside {
            if quoted(line) == Some(verb) && line.contains('{') && line.starts_with("    '") {
                inside = true;
                depth = 0;
            } else {
                continue;
            }
        }
        for ch in line.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        let mut rest = line;
        while let Some(at) = rest.find("$parts[") {
            rest = &rest[at + 7..];
            if let Some(end) = rest.find(']')
                && let Ok(n) = rest[..end].parse::<usize>()
            {
                top = top.max(n + 1);
            }
        }
        if depth <= 0 {
            return Some(top.max(1));
        }
    }
    None
}

/// ★★ ตัวเลขในตารางต้องตรงกับ `$parts[n]` ที่แขนนั้นอ่านจริง
///
/// นี่คือข้อที่ปิดบานเดิมได้จริง: `launch` เขียนว่ารับ 3 เพราะมันอ่านถึง
/// `$parts[2]` · ถ้าวันหนึ่งมันเริ่มอ่าน `$parts[3]` แล้วลืมขยายตาราง
/// ฟิลด์ที่สี่จะถูกปฏิเสธทั้งที่ควรใช้ได้ — และตรงกันข้ามคือบานเดิมเป๊ะ ๆ
#[test]
fn the_table_says_exactly_how_many_fields_each_arm_really_reads() {
    let text = script();
    for (verb, (_, most)) in arity(&text) {
        let Some(read) = fields_read(&text, &verb) else {
            panic!("หาแขนของ '{verb}' ใน switch ไม่เจอ");
        };
        assert_eq!(
            read, most,
            "'{verb}' อ่านถึง {read} ฟิลด์ แต่ $ARITY บอกว่ารับได้มากสุด {most}"
        );
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            // target/ คือของที่สร้างขึ้น ไม่ใช่หลักฐาน · .git ไม่ใช่ข้อความ
            if name == "target" || name.starts_with('.') {
                continue;
            }
            walk(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("ps1" | "md" | "yml" | "yaml")
        ) {
            out.push(path);
        }
    }
}

/// ★★★ ไม่มี step ที่ส่งฟิลด์เกินหลงเหลืออยู่ในทรี
///
/// รันครั้งที่ส่งของเกินคือรันที่ **ไม่ได้ทำสิ่งที่เขียนไว้** · ปล่อยให้อยู่ใน
/// เอกสารเท่ากับปล่อยข้อสรุปปลอมไว้ในที่ที่คนรุ่นหลังจะเชื่อ
#[test]
fn no_recorded_step_carries_a_field_that_would_be_dropped() {
    let text = script();
    let table = arity(&text);
    let script_path = root().join("scripts").join("ui-drive.ps1");

    let mut files = Vec::new();
    walk(&root(), &mut files);
    assert!(files.len() > 20, "เดินทรีแล้วได้แค่ {} ไฟล์", files.len());

    let mut bad = Vec::new();
    for path in files {
        // สคริปต์เองมีตารางกับคู่มือที่พูดถึง verb ทุกตัว — ตรวจแล้วสองเทสต์ข้างบน
        if path == script_path {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (no, line) in body.lines().enumerate() {
            for (verb, (_, most)) in &table {
                let mut rest = line;
                let mut at = 0usize;
                while let Some(found) = rest.find(&format!("{verb}|")) {
                    let start = at + found;
                    at = start + verb.len() + 1;
                    rest = &line[at..];
                    // ต้องเป็นต้นคำจริง ไม่ใช่หางของคำอื่น เช่น `hotkey|`
                    if start > 0
                        && line[..start]
                            .chars()
                            .next_back()
                            .is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
                    {
                        continue;
                    }
                    let token: &str = &line[start..];
                    let end = token
                        .find(|c: char| c.is_whitespace() || "\"'),".contains(c))
                        .unwrap_or(token.len());
                    let n = token[..end].split('|').count();
                    if n > *most {
                        bad.push(format!(
                            "{}:{} — {} ({n} ฟิลด์ · '{verb}' อ่าน {most})",
                            path.display(),
                            no + 1,
                            &token[..end]
                        ));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "step ที่ส่งฟิลด์เกิน:\n  {}", bad.join("\n  "));
}
