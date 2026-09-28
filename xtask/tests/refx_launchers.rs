//! ประตูของ P5-9e — **ไม่มีสคริปต์ไหนเปิด RefX ทับข้อมูลจริงของผู้ใช้ได้อีก**
//!
//! # ทำไมเทสต์นี้อยู่ที่นี่
//!
//! ตลอดสองเดือนแรก ทุกครั้งที่เราขับแอปจริงเพื่อเก็บหลักฐาน มันเขียนลง
//! `%LOCALAPPDATA%\RefX` ของเครื่องที่เจ้าของโปรเจกต์ใช้ทำงานจริง · 27 ก.ย. 2026
//! เกือบจะประทับ `.asked` ลง snapshot ที่เขายังไม่เคยตอบ (ROADMAP P5-9d)
//!
//! ตอนนี้มี `--data-root` และตัวช่วย `scripts/refx-test-root.ps1` แล้ว **แต่ตัวช่วย
//! ที่ไม่มีอะไรบังคับให้ใช้ คือตัวช่วยที่สคริปต์ตัวถัดไปจะลืม** · เทสต์นี้อ่านสคริปต์
//! ตัวจริงทุกตัว (รูปเดียวกับ `ui_drive_step_lists.rs`) ไม่ใช่รายชื่อที่จดไว้ —
//! สคริปต์ใหม่ที่เพิ่มเข้ามาถูกตรวจทันทีโดยไม่ต้องมีใครจำมาเติม
//!
//! ★ ขอบเขตที่มันไม่ครอบ: การเปิดแอปด้วยมือ และสคริปต์นอก `scripts/` ·
//!   ของที่เปิดด้วยมือคือของผู้ใช้เอง ซึ่งควรใช้โฟลเดอร์ปกติอยู่แล้ว

// I-2 ("ห้าม fs บน UI thread") คุมเธรดของแอป · เทสต์ตัวนี้อ่านไฟล์ในทรีอย่างเดียว
#![expect(
    clippy::disallowed_methods,
    reason = "เทสต์ของ xtask — ไม่มี UI thread ให้บล็อก"
)]

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask อยู่ใต้ราก workspace")
        .to_path_buf()
}

const HELPER: &str = "refx-test-root.ps1";

/// ทุกคำสั่ง `Start-Process` ในไฟล์ — **ต่อบรรทัดที่ขึ้นด้วย backtick ให้เป็นคำสั่งเดียว**
///
/// ★ ต้องต่อบรรทัด ไม่งั้น `-ArgumentList` ที่อยู่บรรทัดถัดไป (รูปที่
///   `checklist-app.ps1` เขียนจริง) จะไม่ถูกเห็นเลย แล้วเทสต์จะแดงทั้งที่ถูก
///   หรือแย่กว่า — ถูกเขียนให้หลวมจนเขียวทั้งที่ผิด
fn start_process_statements(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let code = line.trim_start();
        if !code.starts_with('#') && code.contains("Start-Process") {
            let first = i + 1;
            let mut statement = line.to_owned();
            while statement.trim_end().ends_with('`') && i + 1 < lines.len() {
                i += 1;
                statement.push(' ');
                statement.push_str(lines[i].trim());
            }
            out.push((first, statement));
        }
        i += 1;
    }
    out
}

fn scripts() -> Vec<PathBuf> {
    let dir = root().join("scripts");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("เปิดโฟลเดอร์ scripts ไม่ได้")
        .map(|e| e.expect("อ่านรายการในโฟลเดอร์ไม่ได้").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ps1"))
        .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some(HELPER))
        .collect();
    files.sort();
    files
}

fn name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn every_scripted_launch_goes_through_its_own_data_root() {
    let mut launches = 0;
    let mut wrong = Vec::new();

    for path in scripts() {
        let text = std::fs::read_to_string(&path).expect("อ่านสคริปต์ไม่ได้");
        let statements = start_process_statements(&text);
        if statements.is_empty() {
            continue;
        }
        // ตัวช่วยต้องถูกโหลดจริง — ไม่งั้นทุกการเปิดจะพังตอนรันด้วย
        // "Add-RefxDataRoot is not recognized" แทนที่จะพังตอนนี้
        if !text.contains(&format!("'{HELPER}'")) {
            wrong.push(format!(
                "{} เรียก Start-Process แต่ไม่ได้ dot-source {HELPER}",
                name(&path)
            ));
        }
        for (line, statement) in statements {
            launches += 1;
            if !statement.contains("Add-RefxDataRoot") {
                wrong.push(format!(
                    "{}:{line} — Start-Process ไม่ได้ผ่าน Add-RefxDataRoot · \
                     แอปที่เปิดจากตรงนี้จะเขียนลงข้อมูลจริงของผู้ใช้",
                    name(&path)
                ));
            }
        }
    }

    // ★ ถ้าไม่เจอการเปิดเลย เทสต์นี้กำลังตรวจอากาศ — ตอนเขียนมีสี่จุดในสามไฟล์
    assert!(
        launches >= 4,
        "เจอ Start-Process แค่ {launches} จุด — ตัวหาคำสั่งน่าจะพัง"
    );
    assert!(
        wrong.is_empty(),
        "สคริปต์ที่อาจเปิด RefX ทับข้อมูลจริงของผู้ใช้ (ROADMAP P5-9e):\n  {}",
        wrong.join("\n  ")
    );
}

/// ★★★ **สคริปต์ฆ่าหรือขับได้เฉพาะ RefX ที่สคริปต์เปิดเอง** (P5-9e)
///
/// ก่อน 27 ก.ย. 2026 `kill` ของ `ui-drive.ps1` กับ `Stop-Refx` ของ checklist คือ
/// `Get-Process refx | Stop-Process -Force` · และ `attach` คือ `Get-Process refx`
/// — ทั้งสองไปถึง **RefX ตัวไหนก็ได้ที่เปิดอยู่** รวมถึงตัวที่ผู้ใช้เปิดทำงาน
/// ค้างไว้ · ฆ่า = งานที่ยังไม่ได้บันทึกของเขาต้องพึ่งการกู้คืน · ขับ = คลิกของเรา
/// ลงไปในงานจริงของเขา
///
/// ตอนนี้มี `Get-RefxTestProcesses` ที่คืนแต่ตัวที่เปิดด้วย `--data-root` ·
/// เทสต์นี้ห้าม `Get-Process refx` ถูกใช้ทำอย่างอื่นนอกจาก **อ่านว่ามีตัวที่เปิดอยู่ไหม**
#[test]
fn no_script_kills_or_drives_a_refx_it_did_not_start() {
    let mut wrong = Vec::new();
    let mut paths = scripts();
    paths.push(root().join("scripts").join(HELPER));
    for path in paths {
        let text = std::fs::read_to_string(&path).expect("อ่านสคริปต์ไม่ได้");
        for (i, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with('#') || !code.contains("Get-Process refx") {
                continue;
            }
            let kills = code.contains("Stop-Process") || code.contains(".Kill(");
            let drives = code.contains("$proc =") || code.contains("$proc=");
            if kills || drives {
                wrong.push(format!(
                    "{}:{} — {} RefX ตัวไหนก็ได้ที่เปิดอยู่ (รวมตัวของผู้ใช้): {}",
                    name(&path),
                    i + 1,
                    if kills { "ฆ่า" } else { "ขับ" },
                    code.trim()
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "ใช้ Get-RefxTestProcesses แทน — มันคืนแต่ตัวที่สคริปต์เปิดด้วย --data-root:\n  {}",
        wrong.join("\n  ")
    );
}

/// ★★ ฝั่ง Rust: `xtask` เปิดไบนารีได้ **แค่ `--version`** — ทางเดียวที่คืนค่า
/// ก่อน `AppPaths::discover()` จึงไม่แตะโฟลเดอร์ไหนเลย (ดู `refx-app/src/main.rs`)
///
/// วันที่มีคนเพิ่ม `Command::new(&exe).arg("--open-dir=…")` ในตัวตรวจแพ็กเกจ
/// มันจะเปิดแอปเต็มตัวบนเครื่องที่รันอยู่ — เทสต์นี้แดงก่อนถึงวันนั้น
#[test]
fn xtask_only_ever_asks_the_binary_for_its_version() {
    let src = root().join("xtask").join("src");
    let mut seen = 0;
    let mut wrong = Vec::new();
    for entry in std::fs::read_dir(&src).expect("เปิด xtask/src ไม่ได้") {
        let path = entry.expect("อ่านรายการไม่ได้").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("อ่านไฟล์ไม่ได้");
        for (i, line) in text.lines().enumerate() {
            if !line.contains("Command::new(&exe)") {
                continue;
            }
            seen += 1;
            if !line.contains(".arg(\"--version\")") {
                wrong.push(format!("{}:{} — {}", name(&path), i + 1, line.trim()));
            }
        }
    }
    assert!(seen >= 1, "ไม่เจอจุดที่ xtask เปิดไบนารีเลย — ตัวหาน่าจะพัง");
    assert!(
        wrong.is_empty(),
        "xtask เปิดไบนารีด้วยอย่างอื่นนอกจาก --version (จะแตะโฟลเดอร์ข้อมูล):\n  {}",
        wrong.join("\n  ")
    );
}
