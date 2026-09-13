//! ★★★ ประตูของ `docs/08 §3.9` ข้อ 18 — **ผลจากเธรดอื่นต้องมาพร้อมคนปลุก**
//!
//! ## บั๊กที่ประตูนี้มีไว้กัน
//!
//! แอปหลับด้วย `ControlFlow::Wait` ตาม I-1 · ผลที่เธรดอื่นส่งกลับมาโดยไม่มีใคร
//! ปลุก event loop จะ **นอนอยู่ในช่องจนกว่าผู้ใช้จะบังเอิญขยับเมาส์**
//!
//! เกิดมาแล้วสามครั้งในโปรเจกต์นี้ (`docs/08 §3.9` ข้อ 18):
//!
//! | | อาการ |
//! |---|---|
//! | "Save and close" | บันทึกแล้วแต่ไม่ปิด |
//! | autosave | เส้นปลุกต่อไว้แล้วแต่ `on_wake()` ไม่มีกิ่งรับ |
//! | กล่อง export | เลือกไฟล์เสร็จแล้วกล่องยังเขียนว่า "กำลังรอ" |
//!
//! ## ★★ ทำไมเทสต์ธรรมดาจับไม่ได้ — และประตูนี้ต่างออกไปยังไง
//!
//! **เทสต์ไม่มี event loop** มันเรียกฟังก์ชันตรง ๆ จึงไม่มีสภาพ "หลับ" ให้ผิด
//! ได้เลย · ทุกชิ้นถูกหมด ผิดแค่ว่าไม่มีใครมาเรียก (`§3.9` ข้อ 8)
//!
//! → ประตูนี้จึงไม่ทดสอบ *พฤติกรรม* แต่**ไล่รายการช่องทางผลข้ามเธรดทุกช่อง
//! จากซอร์สจริง** แล้วบังคับว่าทุกช่องต้องขึ้นทะเบียนพร้อมคำตอบว่าใครปลุก
//! · ช่องใหม่ที่ไม่มีใครคิดเรื่องปลุก = **เทสต์แดงทันทีที่เขียน** ไม่ใช่รอให้
//! ผู้ใช้เจอเป็นครั้งที่สี่
//!
//! รูปแบบเดียวกับ job `unwired-targets` ที่จับ fuzz target ที่ยังไม่ต่อสาย
//!
//! ## ★ `WaitUntil` ไม่ใช่คำตอบ
//!
//! มันทำงานได้เหมือนกัน แต่ต้องมีคนคอยดูแลว่าปิดตอนไม่มีงานจริงไหม ซึ่งเป็น
//! คำถามที่ I-1 แพ้ได้เงียบ ๆ · **ตัวปลุกไม่มีคำถามนั้น**

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

/// ใครเป็นคนปลุกหลังผลถูกส่ง
#[derive(Debug, Clone, Copy)]
enum Wake {
    /// เธรดที่ฟังก์ชันนี้สร้าง ปลุกเอง — ต้องเจอการเรียกปลุก**ในตัวฟังก์ชันเดียวกัน**
    InThread,
    /// ปลุกที่อื่น เพราะผู้ส่งผลจริงอยู่คนละฟังก์ชัน (`file`, `func`)
    ///
    /// ★ ต้องระบุให้เจาะจงถึงระดับฟังก์ชัน ไม่ใช่ "อยู่ในไฟล์นั้นแหละ" —
    /// ไม่งั้นมันกลายเป็นการยืนยันว่า "ไฟล์นี้มีคำว่า wake อยู่" ซึ่งจริงเสมอ
    Elsewhere {
        /// ไฟล์ที่ผู้ปลุกอยู่ (relative จากรากโปรเจกต์)
        file: &'static str,
        /// ฟังก์ชันที่เรียกปลุก
        func: &'static str,
    },
    /// ★★ ปลุกด้วย **นาฬิกาของตัวเอง** (`wake_deadline` → `on_wake`) — ไม่ขอเฟรม
    ///
    /// ใช้เมื่อผลที่กลับมา **ไม่เปลี่ยนอะไรบนจอ** · ทางของ `Waker` จบที่
    /// `request_redraw` เสมอ ซึ่งทำให้ตัวนับเฟรมไต่ทั้งที่จอเหมือนเดิม
    /// แล้วประตู `ui-idle-diff` จะแดงให้กับพฤติกรรมที่ถูกต้อง
    ///
    /// ★★★ **ต้องชี้ว่านาฬิกาอยู่ที่ไหนและพิสูจน์ยังไง** (แก้ 11 ก.ย. 2026)
    ///
    /// รุ่นแรกตรวจแค่ว่า *ไฟล์เดียวกัน* มีคำว่า `_deadline(` อยู่ — ซึ่ง (ก)
    /// ผ่านได้ด้วยฟังก์ชันนาฬิกาตัวไหนก็ได้ที่ไม่เกี่ยวกัน และ (ข) **ตกกับ
    /// นาฬิกาที่อยู่คนละไฟล์** ทั้งที่นั่นเป็นรูปที่ถูกต้องพอ ๆ กัน
    /// → รูปเดียวกับ `Elsewhere`: ระบุให้เจาะจงแล้วให้ประตูไปดูของจริง
    OwnTimer {
        /// ไฟล์ที่นาฬิกาอยู่
        clock_in: &'static str,
        /// ฟังก์ชันนาฬิกาในไฟล์นั้น
        clock_fn: &'static str,
        /// ★ สิ่งที่ต้องเจอ **ในตัวฟังก์ชันนาฬิกา** — หลักฐานว่ามันดูช่องนี้อยู่จริง
        proof: &'static str,
        /// ทำไมไม่ใช้ตัวปลุก
        why: &'static str,
    },
    /// ไม่ต้องปลุก — **ต้องมีเหตุผล** และเหตุผลถูกพิมพ์ออกมาทุกครั้งที่รัน
    NotNeeded(&'static str),
}

/// หนึ่งช่องทางที่ผล/งานข้ามเธรด
#[derive(Debug, Clone, Copy)]
struct Crossing {
    /// ไฟล์ที่ช่องถูกสร้าง (relative จากรากโปรเจกต์)
    file: &'static str,
    /// ฟังก์ชันที่สร้างช่องหรือสร้างเธรด
    func: &'static str,
    /// ใครปลุก
    wake: Wake,
}

/// ★★★ **ทะเบียนช่องทางผลข้ามเธรดทั้งหมดที่จบลงที่ UI thread**
///
/// เพิ่มช่องใหม่โดยไม่มาต่อแถวนี้ = เทสต์แดงพร้อมบอกชื่อฟังก์ชัน
const CROSSINGS: &[Crossing] = &[
    // ---------- เครื่องมือวินิจฉัย (examples) ----------
    Crossing {
        file: "crates/refx-ui/examples/bare_egui.rs",
        func: "main",
        wake: Wake::NotNeeded(
            "★ ตัวส่งเองคือตัวปลุก — `EventLoopProxy::send_event` ปลุก event loop \
             ตามนิยามของมัน ไม่มีช่องผลแยกที่ต้องมีคนมาอ่านทีหลัง · และไฟล์นี้เป็น \
             เครื่องมือวินิจฉัยบั๊กหน้าต่างค้าง (§2.53) ไม่ได้อยู่ในโปรแกรมที่แจก",
        ),
    },
    // ---------- refx-ui ----------
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "paths_known_for",
        wake: Wake::NotNeeded(
            "รอผลแบบ blocking บน worker ของตัวเอง (recv_timeout) — ไม่มีอะไรเดินทางกลับไปหา UI thread",
        ),
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "start_assets",
        wake: Wake::Elsewhere {
            file: "crates/refx-asset/src/cache.rs",
            func: "io_loop",
        },
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "drain_decode_results",
        wake: Wake::Elsewhere {
            file: "crates/refx-asset/src/cache.rs",
            func: "io_loop",
        },
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "tick_autosave_one",
        wake: Wake::OwnTimer {
            clock_in: "crates/refx-ui/src/app.rs",
            clock_fn: "next_autosave_across_tabs",
            proof: "autosave_job",
            why: "นาฬิกาของ autosave เอง ตั้งเวลาไว้ข้างหน้าระหว่างที่งานยังค้าง — วัดแล้วว่าทางของ `Waker` ทำให้ Frames 516→517 ทั้งที่จอไม่เปลี่ยน",
        },
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "start_recovery_scan",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "sweep_recovery_folder",
        wake: Wake::NotNeeded("ไม่มีช่องผลเลย — ลบ snapshot ที่หมดอายุอย่างเดียว ไม่มีอะไรต้องขึ้นจอ"),
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "sweep_spool_folder",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "start_load",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "start_relink_scan",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "start_folder_match",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "start_save",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-ui/src/app.rs",
        func: "redraw",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-ui/src/copy.rs",
        func: "copy",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-ui/src/export.rs",
        func: "spawn",
        wake: Wake::InThread,
    },
    // ---------- refx-platform: กล่องของ OS ----------
    Crossing {
        file: "crates/refx-platform/src/dialog.rs",
        func: "pick_save_location",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-platform/src/dialog.rs",
        func: "pick_document_to_open",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-platform/src/dialog.rs",
        func: "pick_export_location",
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-platform/src/dialog.rs",
        func: "pick_missing_image",
        wake: Wake::InThread,
    },
    // ---------- refx-asset ----------
    Crossing {
        file: "crates/refx-asset/src/pool.rs",
        func: "new",
        wake: Wake::Elsewhere {
            file: "crates/refx-asset/src/pool.rs",
            func: "worker_loop",
        },
    },
    Crossing {
        file: "crates/refx-asset/src/pool.rs",
        func: "cache_lookup",
        wake: Wake::NotNeeded(
            "รอผลแบบ blocking บน decode worker เอง — คำตอบถูกใช้ที่นั่นแล้วจบ ไม่ข้ามไป UI",
        ),
    },
    Crossing {
        file: "crates/refx-asset/src/cache.rs",
        func: "spawn",
        wake: Wake::NotNeeded(
            "ช่อง **คำสั่ง** (UI → IO) ไม่ใช่ช่องผล · คำตอบเดินทางกลับทาง `reply` ของแต่ละคำสั่ง ซึ่งขึ้นทะเบียนที่จุดสร้างของมันเอง",
        ),
    },
    Crossing {
        file: "crates/refx-asset/src/cache.rs",
        func: "drop",
        wake: Wake::NotNeeded(
            "ล้าง cache ตอนปิดโปรแกรม — รอแบบ blocking แล้วโปรแกรมก็จบ ไม่มีเฟรมถัดไปให้ปลุก",
        ),
    },
    // ---------- .refx-meta (P5-5) — ★ สองช่อง สองวิธีปลุก ----------
    Crossing {
        file: "crates/refx-ui/src/sidecar.rs",
        func: "start_load",
        // ผลคือ **แท็กโผล่บนจอ** — ผู้ใช้ต้องเห็นทันที ไม่ใช่ตอนขยับเมาส์
        wake: Wake::InThread,
    },
    Crossing {
        file: "crates/refx-ui/src/sidecar.rs",
        func: "start_write",
        wake: Wake::OwnTimer {
            // ★ นาฬิกาอยู่คนละไฟล์กับช่อง — และนั่นถูกต้อง: `Folders` ไม่รู้จัก
            //   event loop และไม่ควรรู้ · `app.rs` เป็นที่เดียวที่รู้ทั้งสองฝั่ง
            clock_in: "crates/refx-ui/src/app.rs",
            clock_fn: "next_autosave_across_tabs",
            proof: "has_work_outstanding",
            why: "ผลคือไฟล์ลงดิสก์ — ไม่มีอะไรให้วาด · เก็บผลบนนาฬิกาเดียวกับ autosave · ปลุกให้วาดจะทำให้ `ui-idle-diff` แดงกับพฤติกรรมที่ถูก",
        },
    },
];

/// ★★★ ไฟล์ที่ถูกไล่ตรวจ — **ค้นเอง ไม่ใช่รายชื่อที่เขียนด้วยมือ** (แก้ 11 ก.ย. 2026)
///
/// รุ่นแรกของประตูนี้ถือรายชื่อหกไฟล์ · วันที่ `crates/refx-ui/src/sidecar.rs`
/// เกิดขึ้นพร้อมช่องข้ามเธรดสองช่อง **ประตูเขียวผ่านไปเงียบ ๆ** เพราะไฟล์ใหม่
/// ไม่ได้อยู่ในรายชื่อ — ประตูที่รู้จักแต่สิ่งที่มีอยู่แล้ว ไม่ใช่ประตู
///
/// → เดินหาเองทุกไฟล์ใต้ `crates/*/src/` · ไฟล์ที่ไม่มีช่องข้ามเธรดไม่เสียอะไร
///   และไฟล์ใหม่จะ **มองไม่เห็นไม่ได้อีก** (`docs/08 §3.9` ข้อ 18)
fn scanned() -> Vec<String> {
    let mut out = Vec::new();
    let crates = root().join("crates");
    let mut stack = vec![crates.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // ★ `tests/` และ `benches/` ไม่มี UI thread ให้ปลุก
                if path
                    .file_name()
                    .is_some_and(|n| n == "tests" || n == "benches")
                {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && let Ok(rel) = path.strip_prefix(root())
            {
                out.push(
                    rel.to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/"),
                );
            }
        }
    }
    out.sort();
    out
}

/// สิ่งที่นับว่าเป็น "ช่องทางข้ามเธรด" ในซอร์ส
///
/// ★ นับ **เธรด** ด้วย ไม่ใช่แค่ช่อง — `refx-ui::copy` ส่งผลกลับทาง
/// `Arc<Mutex<..>>` ไม่ใช่ channel · ประตูที่ดูแต่ channel จะมองมันไม่เห็นเลย
const CROSSING_TOKENS: &[&str] = &[
    "crossbeam_channel::bounded",
    "crossbeam_channel::unbounded",
    "thread::spawn(",
    "thread::Builder::new()",
];

/// ชื่อฟังก์ชันที่นับว่าเป็น "การปลุก" — ตรวจแบบ **เรียกใช้** ไม่ใช่แค่มีสตริง
///
/// ★ ต้องรับทั้ง `.wake()` และ `wake()` เฉย ๆ — `refx-ui::copy` ถือตัวปลุกเป็น
/// **closure** แล้วเรียกตรง ๆ · ประตูที่บังคับให้มีจุดนำหน้าจะรายงานว่า
/// "ไม่มีการปลุก" ทั้งที่มี (เจอทันทีที่ประตูนี้รันครั้งแรก)
const WAKE_CALLS: &[&str] = &["wake", "wake_after_send"];

/// `line` มีการ **เรียก** `name(...)` ไหม
///
/// ★★ ต้องดูขอบคำ ไม่ใช่ `contains` เฉย ๆ — `fn claims_to_wake()` มีสตริง
/// `wake()` อยู่ในชื่อตัวเอง · ตัวตรวจที่ใช้ `contains` จะตอบว่า "ปลุกแล้ว"
/// ให้กับฟังก์ชันที่แค่**ชื่อลงท้ายด้วย wake** ซึ่งเป็นการโกหกที่เนียนที่สุด
/// (NC ของประตูนี้จับได้ตั้งแต่รอบแรก)
fn calls(line: &str, name: &str) -> bool {
    // ★ บรรทัดที่ **ประกาศ** ฟังก์ชันไม่ใช่การเรียกมัน — `pub fn wake(&self)`
    //   ในนิยามของ `WakeHandle` เองจะถูกนับเป็น "มีการปลุก" ถ้าไม่กันตรงนี้
    //   (rustfmt แยกบรรทัดให้เสมอ จึงไม่มีเคส `fn x() { wake(); }` บรรทัดเดียว)
    let head = line.trim_start();
    for prefix in [
        "fn ",
        "pub fn ",
        "pub(crate) fn ",
        "async fn ",
        "pub async fn ",
    ] {
        if head.starts_with(prefix) {
            return false;
        }
    }
    let needle = format!("{name}(");
    let bytes = line.as_bytes();
    let mut from = 0usize;
    while let Some(found) = line[from..].find(&needle) {
        let at = from + found;
        let boundary = at == 0 || {
            let before = bytes[at - 1];
            !(before.is_ascii_alphanumeric() || before == b'_')
        };
        if boundary {
            return true;
        }
        from = at + 1;
    }
    false
}

/// หนึ่งฟังก์ชันในซอร์ส พร้อมช่วงบรรทัดของมัน
struct Func {
    name: String,
    start: usize,
    /// บรรทัดแรกของฟังก์ชันถัดไป (หรือท้ายไฟล์)
    end: usize,
}

/// ★★ แบ่งไฟล์เป็นช่วงของแต่ละฟังก์ชันโดย **ไม่นับวงเล็บปีกกา**
///
/// การนับปีกกาพังกับ `format!("{a}")` และคอมเมนต์ที่มีปีกกาข้างเดียว ·
/// ช่วง `[fn นี้, fn ถัดไป)` ตอบคำถามที่เราถามได้ครบโดยไม่ต้องเข้าใจไวยากรณ์
fn functions_in(source: &str) -> Vec<Func> {
    let mut found: Vec<Func> = Vec::new();
    for (index, line) in source.lines().enumerate() {
        // ★ ตัดเทสต์ออก — เทสต์สร้างเธรดของตัวเองได้ตามสบาย ไม่มี event loop ให้ปลุก
        if line.trim_start().starts_with("#[cfg(test)]") {
            if let Some(last) = found.last_mut() {
                last.end = index;
            }
            break;
        }
        let trimmed = line.trim_start();
        let is_fn = trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub(crate) fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub async fn ");
        if !is_fn {
            continue;
        }
        let Some(rest) = trimmed.split("fn ").nth(1) else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        if let Some(last) = found.last_mut() {
            last.end = index;
        }
        found.push(Func {
            name,
            start: index,
            end: source.lines().count(),
        });
    }
    found
}

/// ฟังก์ชันไหนบ้างในไฟล์นี้ที่ข้ามเธรด (เรียงตามที่เจอ ไม่ซ้ำ)
fn crossings_in(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out: Vec<String> = Vec::new();
    for func in functions_in(source) {
        let body = &lines[func.start..func.end.min(lines.len())];
        let hit = body
            .iter()
            .any(|line| CROSSING_TOKENS.iter().any(|token| line.contains(token)));
        if hit && !out.contains(&func.name) {
            out.push(func.name);
        }
    }
    out
}

/// ฟังก์ชันนี้มีการปลุกอยู่ข้างในไหม — `None` = ไม่เจอฟังก์ชันชื่อนี้เลย
/// มี `token` อยู่ **ในตัวฟังก์ชันนี้** ไหม — `None` = ไม่เจอฟังก์ชันเลย
///
/// ★ ต่างจาก `source.contains(token)` ตรงที่ตอบคำถามว่า *ฟังก์ชันนั้น*
/// ทำสิ่งนั้นอยู่ ไม่ใช่ว่า *ไฟล์นั้น* มีคำนั้นอยู่ที่ไหนสักแห่ง
fn contains_inside(source: &str, func_name: &str, token: &str) -> Option<bool> {
    let lines: Vec<&str> = source.lines().collect();
    let func = functions_in(source)
        .into_iter()
        .find(|func| func.name == func_name)?;
    let body = &lines[func.start..func.end.min(lines.len())];
    Some(body.iter().any(|line| line.contains(token)))
}

fn wakes_inside(source: &str, func_name: &str) -> Option<bool> {
    let lines: Vec<&str> = source.lines().collect();
    let func = functions_in(source)
        .into_iter()
        .find(|func| func.name == func_name)?;
    let body = &lines[func.start..func.end.min(lines.len())];
    Some(
        body.iter()
            .any(|line| WAKE_CALLS.iter().any(|name| calls(line, name))),
    )
}

fn root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` = crates/refx-ui
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("หา root ของโปรเจกต์ไม่เจอ")
        .to_path_buf()
}

/// ★ ข้อยกเว้นของกฎห้าม `fs::read_to_string` (clippy.toml) — กฎนั้นมีไว้กัน
/// **ดิสก์ I/O บน UI thread** (I-2) · ที่นี่คือประตูที่อ่านซอร์สของตัวเอง
/// ไม่มี UI thread ให้บล็อก และไม่มีไฟล์ของผู้ใช้เข้ามาเกี่ยวเลย
#[expect(
    clippy::disallowed_methods,
    reason = "ประตูอ่านซอร์สของโปรเจกต์เอง ไม่ใช่ดิสก์ I/O บนลูปเฟรม"
)]
fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("อ่าน {} ไม่ได้: {err}", path.display()))
}

/// ★★★ **ทุกช่องทางข้ามเธรดต้องขึ้นทะเบียน และทะเบียนต้องไม่โกหก**
#[test]
fn every_thread_that_returns_a_result_has_someone_to_wake_the_ui() {
    let mut unregistered: Vec<String> = Vec::new();
    let mut checked = 0usize;

    let scanned = scanned();
    assert!(
        scanned.len() > 20,
        "ตัวเดินหาไฟล์เจอแค่ {} ไฟล์ — มันเดินไม่ถึงซอร์สจริง",
        scanned.len()
    );
    for file in &scanned {
        let source = read(file);
        for func in crossings_in(&source) {
            checked += 1;
            let known = CROSSINGS.iter().any(|c| c.file == file && c.func == func);
            if !known {
                unregistered.push(format!("{file} :: {func}()"));
            }
        }
    }

    assert!(checked > 10, "ตัวไล่หาเจอแค่ {checked} ช่อง — น่าจะสแกนไม่โดน");
    assert!(
        unregistered.is_empty(),
        "★ ช่องทางข้ามเธรดที่ยังไม่ขึ้นทะเบียน — ต้องตอบให้ได้ว่า **ใครปลุก UI**\n\
         หลังผลถูกส่ง ไม่งั้นผลจะนอนรอจนกว่าผู้ใช้จะขยับเมาส์ (docs/08 §3.9 ข้อ 18):\n  {}",
        unregistered.join("\n  ")
    );

    // ---- ทะเบียนต้องตรงกับซอร์สจริง ----
    let mut exceptions: Vec<&str> = Vec::new();
    for crossing in CROSSINGS {
        match crossing.wake {
            Wake::InThread => {
                let source = read(crossing.file);
                assert_eq!(
                    wakes_inside(&source, crossing.func),
                    Some(true),
                    "{} :: {}() ขึ้นทะเบียนว่าปลุกเอง แต่หาการปลุกในตัวมันไม่เจอ",
                    crossing.file,
                    crossing.func
                );
            }
            Wake::Elsewhere { file, func } => {
                let source = read(file);
                assert_eq!(
                    wakes_inside(&source, func),
                    Some(true),
                    "{} :: {}() บอกว่าให้ {file} :: {func}() ปลุกให้ แต่ที่นั่นไม่มีการปลุก",
                    crossing.file,
                    crossing.func
                );
            }
            // ★★ นาฬิกาของตัวเอง — ต้องมีเหตุผล และต้องมี **นาฬิกาอยู่จริง**
            //    ในไฟล์นั้น ไม่ใช่แค่อ้างว่ามี
            Wake::OwnTimer {
                clock_in,
                clock_fn,
                proof,
                why,
            } => {
                assert!(
                    !why.trim().is_empty(),
                    "{} :: {}() บอกว่าใช้นาฬิกาของตัวเองโดยไม่มีเหตุผล",
                    crossing.file,
                    crossing.func
                );
                // ★★★ ไปดูของจริง: นาฬิกาที่อ้างต้องมีอยู่ **และต้องดูช่องนี้อยู่**
                let source = read(clock_in);
                assert_eq!(
                    contains_inside(&source, clock_fn, proof),
                    Some(true),
                    "{} :: {}() บอกว่า {clock_in} :: {clock_fn}() เป็นคนตั้งนาฬิกาให้
                     แต่ในตัวฟังก์ชันนั้นไม่มี `{proof}` เลย — คำอ้างที่พิสูจน์ไม่ได้",
                    crossing.file,
                    crossing.func
                );
                exceptions.push(why);
            }
            Wake::NotNeeded(why) => {
                assert!(
                    !why.trim().is_empty(),
                    "{} :: {}() ยกเว้นโดยไม่มีเหตุผล",
                    crossing.file,
                    crossing.func
                );
                exceptions.push(why);
            }
        }
    }

    // ★ ข้อยกเว้นต้อง **ดังทุกครั้งที่รัน** ไม่ใช่ซ่อนอยู่ในโค้ด (`§3.9` ข้อ 2)
    println!(
        "ช่องทางข้ามเธรดที่ตรวจแล้ว: {checked} · ขึ้นทะเบียนไว้ {}",
        CROSSINGS.len()
    );
    println!("ข้อยกเว้น {} ข้อ:", exceptions.len());
    for why in &exceptions {
        println!("  · {why}");
    }
}

/// ★★★ **negative control — ประตูนี้จับได้จริงไหม** (`docs/08 §3.9` ข้อ 1 · ข้อ 9)
///
/// ตัวไล่หาเป็นฟังก์ชันบริสุทธิ์ที่กินสตริง จึงยิงซอร์สปลอมใส่ได้โดย
/// **ไม่ต้องแก้โค้ด production** — และไม่มีทางที่ NC จะ "ถูกใส่ไว้ในที่ที่ไม่ทำงาน"
/// ซึ่งเป็นความผิดพลาดที่เกิดมาแล้วรอบก่อน
#[test]
fn the_gate_really_does_catch_a_channel_nobody_wakes() {
    // ---- 1. ช่องใหม่ที่ไม่มีในทะเบียน ต้องถูกชี้ชื่อ ----
    let fake = "\
fn already_registered() {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let _ = tx;
}

fn brand_new_worker() {
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send(42);
    });
}
";
    let found = crossings_in(fake);
    assert!(
        found.iter().any(|name| name == "brand_new_worker"),
        "ตัวไล่หามองไม่เห็นช่องใหม่ — ประตูจะเงียบวันที่มีคนเพิ่มของจริง: {found:?}"
    );
    println!("NC วิ่งผ่านจริง (1/3): ตัวไล่หาชี้ชื่อ {found:?}");

    // ---- 2. เธรดที่ส่งผลกลับทาง Mutex (ไม่ใช่ channel) ต้องถูกจับด้วย ----
    let by_mutex = "\
fn writes_into_a_mutex() {
    let shared = state.clone();
    std::thread::spawn(move || {
        *shared.lock().unwrap() = Some(work());
    });
}
";
    assert_eq!(
        crossings_in(by_mutex),
        vec!["writes_into_a_mutex".to_owned()],
        "ประตูที่ดูแต่ channel จะมองข้ามทางที่ `copy.rs` ใช้จริง"
    );
    println!("NC วิ่งผ่านจริง (2/3): เธรดที่ส่งผลทาง Mutex ก็ถูกจับ");

    // ---- 3. ฟังก์ชันที่อ้างว่าปลุกเอง แต่ไม่มีการปลุก ต้องตรวจเจอ ----
    let lying = "\
fn claims_to_wake() {
    std::thread::spawn(move || {
        let _ = tx.send(result);
    });
}
";
    assert_eq!(
        wakes_inside(lying, "claims_to_wake"),
        Some(false),
        "ตัวตรวจการปลุกตอบว่ามีทั้งที่ไม่มี — ทะเบียนจะโกหกได้โดยไม่มีใครรู้"
    );
    let honest = "\
fn really_wakes() {
    std::thread::spawn(move || {
        let _ = tx.send(result);
        waker.wake();
    });
}
";
    assert_eq!(
        wakes_inside(honest, "really_wakes"),
        Some(true),
        "ตัวตรวจการปลุกตอบว่าไม่มีทั้งที่มี — ประตูจะแดงตลอดกาลแล้วไม่มีใครเชื่อมัน"
    );
    println!("NC วิ่งผ่านจริง (3/3): แยก 'อ้างว่าปลุก' ออกจาก 'ปลุกจริง' ได้");

    // ---- 3ก. ★ ชื่อฟังก์ชันที่ลงท้ายด้วย wake ต้องไม่นับเป็นการปลุก ----
    //
    //   `fn claims_to_wake()` มีสตริง `wake()` อยู่ในชื่อตัวเอง · ตัวตรวจรุ่นแรก
    //   ใช้ `contains` แล้วตอบว่า "ปลุกแล้ว" ให้กับฟังก์ชันที่ไม่ได้ปลุกอะไรเลย
    assert!(
        !calls("fn claims_to_wake() {", "wake"),
        "ชื่อฟังก์ชันถูกนับเป็นการเรียก"
    );
    assert!(
        calls("        ctx.wake.wake();", "wake"),
        "การเรียกแบบมีจุดนำหน้าไม่ถูกนับ"
    );
    assert!(
        calls("            wake();", "wake"),
        "closure ที่เรียกตรง ๆ ไม่ถูกนับ"
    );
    assert!(
        !calls("    pub fn wake(&self) {", "wake"),
        "นิยามของฟังก์ชันถูกนับเป็นการเรียก"
    );
    println!("NC วิ่งผ่านจริง (3ก): แยก 'ชื่อมีคำว่า wake' ออกจาก 'เรียก wake จริง' ได้");

    // ---- 4. เทสต์ในไฟล์เดียวกันต้องไม่ถูกนับ ----
    let with_tests = "\
fn production() {
    std::thread::spawn(move || {});
}

#[cfg(test)]
mod tests {
    fn a_test_helper() {
        std::thread::spawn(move || {});
    }
}
";
    assert_eq!(
        crossings_in(with_tests),
        vec!["production".to_owned()],
        "เธรดในเทสต์ถูกนับด้วย — ประตูจะบังคับให้เทสต์มีตัวปลุกทั้งที่ไม่มี event loop"
    );
    println!("NC วิ่งผ่านจริง (4): เธรดใน `mod tests` ไม่ถูกนับ");

    // ---- 5. ★★★ ตัวเดินหาไฟล์ต้องไปถึงไฟล์ที่ **ไม่เคยอยู่ในรายชื่อเดิม** ----
    //
    //   นี่คือบั๊กจริงของวันนี้: `sidecar.rs` เกิดขึ้นพร้อมช่องข้ามเธรดสองช่อง
    //   แล้วประตู **เขียวผ่านไปเงียบ ๆ** เพราะมันถือรายชื่อหกไฟล์ที่เขียนด้วยมือ
    let files = scanned();
    for must in [
        "crates/refx-ui/src/sidecar.rs", // ไฟล์ที่รายชื่อเดิมมองไม่เห็น
        "crates/refx-io/src/save.rs",    // ทั้ง crate ที่รายชื่อเดิมไม่เคยแตะเลย
        "crates/refx-render/src/device.rs",
    ] {
        assert!(
            files.iter().any(|f| f == must),
            "ตัวเดินหาไปไม่ถึง {must} — ไฟล์ใหม่ในนั้นจะสร้างช่องข้ามเธรดได้โดยไม่มีใครเห็น"
        );
    }
    assert!(
        !files.iter().any(|f| f.contains("/tests/")),
        "เดินเข้าไปใน tests/ ด้วย — ประตูจะบังคับให้เทสต์มีตัวปลุก"
    );
    println!(
        "NC วิ่งผ่านจริง (5): ตัวเดินหาเจอ {} ไฟล์ รวมของที่รายชื่อเดิมมองไม่เห็น",
        files.len()
    );

    // ---- 6. ★★ คำอ้างเรื่องนาฬิกาต้องพิสูจน์ได้ ไม่ใช่แค่มีคำนั้นอยู่ในไฟล์ ----
    let clock = "fn unrelated_deadline() {
    let _ = something_else;
}

fn real_clock() {
    if doc.sidecar.has_work_outstanding() {
        return Some(now + WAIT);
    }
}
";
    assert_eq!(
        contains_inside(clock, "unrelated_deadline", "has_work_outstanding"),
        Some(false),
        "นาฬิกาตัวอื่นในไฟล์เดียวกันถูกนับเป็นหลักฐาน — ซึ่งคือประตูรุ่นแรกเป๊ะ"
    );
    assert_eq!(
        contains_inside(clock, "real_clock", "has_work_outstanding"),
        Some(true),
        "นาฬิกาที่ดูช่องนี้อยู่จริงกลับไม่ถูกนับ — ประตูจะแดงตลอดกาล"
    );
    assert_eq!(
        contains_inside(clock, "no_such_function", "anything"),
        None,
        "ฟังก์ชันที่ไม่มีอยู่ต้องตอบว่าไม่มี ไม่ใช่ตอบว่าไม่เจอหลักฐาน"
    );
    println!("NC วิ่งผ่านจริง (6): แยก 'ไฟล์นั้นมีคำนี้' ออกจาก 'ฟังก์ชันนั้นทำสิ่งนี้' ได้");
}
