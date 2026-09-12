//! ★★★ ประตูของ `docs/08 §3.9` ข้อ 1b — **กิ่งที่ input ไปไม่ถึง**
//!
//! ## ปัญหาที่ประตูนี้มีไว้จับ
//!
//! เทสต์ที่เรียกฟังก์ชันซึ่งมี **ด่านคืนค่าเร็ว** อยู่ต้น ๆ แล้ว fixture ของเทสต์
//! ทำให้ด่านนั้นทำงานทุกครั้ง → เทสต์เขียวโดยไม่เคยแตะกิ่งที่ชื่อมันอ้างเลย
//! (เจอมาแล้วสี่ครั้ง · ครั้งล่าสุด `autosave_deadline` ที่คืน `None` ตั้งแต่
//! บรรทัดแรกเพราะเทสต์ไม่มี `gfx` — เขียวมาตลอดโดยไม่ได้ตรวจอะไร)
//!
//! ## ★★ ทำไมอ่านโค้ดแล้วเดาไม่พอ
//!
//! การไล่ด้วยสายตาได้แค่ *"น่าสงสัย"* · คำถามจริงคือ **"ถ้ากิ่งนี้ตาย มีเทสต์
//! ตัวไหนแดงไหม"** ซึ่งตอบได้ทางเดียวคือ**ทำให้มันตายแล้วรัน**
//!
//! วัดจริง 11 ก.ย. 2026 บน `refx-core`: ด่าน **110 ตัว** · รอดจากทุกเทสต์ **5 ตัว**
//! · และในสี่ตัวที่อ่านแล้วน่าสงสัยที่สุด **เทสต์พี่น้องแดงทุกครั้ง** —
//! สิ่งที่โกหกคือ *ชื่อเทสต์* ไม่ใช่ชุดเทสต์ · อันตรายจริงคือตอน**ไม่มีพี่น้องเลย**
//!
//! ## ★★★ ทะเบียนต้องค้นหาสมาชิกเอง (`§3.9` ข้อ 18)
//!
//! **ด่านถูกค้นเจอเอง** จากซอร์ส ไม่ใช่ถูกยื่นรายชื่อ — ด่านใหม่ที่ไม่มีเทสต์
//! ตัวไหนเห็น จึงแดงตั้งแต่วันที่มันถูกเขียน · สิ่งเดียวที่เขียนด้วยมือคือ
//! [`ALLOWED`] ซึ่งเป็น **ข้อยกเว้น** และถูกพิมพ์ออกมาทุกครั้งที่รัน (`ข้อ 9`)
//!
//! ## ใช้
//!
//! ```text
//! cargo xtask mutation                 # refx-core (ค่าปริยาย)
//! cargo xtask mutation refx-io         # crate อื่น
//! cargo xtask mutation refx-core --time-only   # วัดเวลาอย่างเดียว ไม่ตัดสิน
//! REFX_MUT_NC=1 cargo xtask mutation   # negative control — ต้องล้มเสมอ
//! ```
//!
//! ★ ช้าเกินกว่าจะอยู่ใน CI ทุก push (`refx-core` 7.4 นาที · `refx-io` 18 นาที)
//! → **รอบสัปดาห์ละครั้ง** (`.github/workflows/mutation.yml`)

// ★ `fs::read_to_string` ถูกแบนเพราะ **ดิสก์ I/O บน UI thread** (I-2) และไฟล์
//   ของผู้ใช้ที่ขนาดไม่รู้จบ · ที่นี่เป็นเครื่องมือ dev ที่อ่านซอร์สของโปรเจกต์เอง
//   ไม่มี UI thread ให้บล็อก และไม่มีไฟล์ของผู้ใช้เข้ามาเกี่ยวเลย
#![expect(
    clippy::disallowed_methods,
    reason = "เครื่องมือ dev อ่านซอร์สของโปรเจกต์เอง ไม่ใช่ดิสก์ I/O บนลูปเฟรม"
)]

use std::path::{Path, PathBuf};

/// ด่านที่ **ยอมให้รอด** ได้ พร้อมเหตุผล — ★ ข้อยกเว้น ไม่ใช่รายชื่อของสมาชิก
///
/// `(ไฟล์, ข้อความของบรรทัดด่าน, เหตุผล)` — ★ จับคู่ด้วย **ข้อความ** ไม่ใช่
/// เลขบรรทัด เพราะเลขบรรทัดขยับทุกครั้งที่มีคนเพิ่มคอมเมนต์ แล้วทะเบียนจะ
/// กลายเป็นของที่ต้องแก้ตลอดเวลาจนไม่มีใครอ่านมัน
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "crates/refx-core/src/command.rs",
        "let Some(next) = next.as_any().downcast_ref::<Self>() else {",
        "ด่านชนิดของ `merge` — สองคำสั่งคนละชนิดไม่มีวัน merge กัน · การทำให้ด่านนี้ \
         ทำงานเสมอแปลว่า 'ไม่ merge เลย' ซึ่งถูกต้องตามสเปกอยู่แล้ว จึงไม่มีเทสต์ไหนแดง",
    ),
    (
        "crates/refx-core/src/layout.rs",
        "if count == 0 {",
        "board ว่างไม่มีทางถึงตัวจัดหน้า — ผู้เรียกกรองไปก่อนแล้ว · ด่านนี้เป็นการ \
         กันหารศูนย์ที่เหลือไว้เผื่อผู้เรียกในอนาคต",
    ),
    (
        "crates/refx-core/src/spatial.rs",
        "let Some(slot) = self.cells.get_mut(&cell) else {",
        "ลบ item ออกจากช่องที่ไม่มีอยู่ — เกิดได้ก็ต่อเมื่อ index กับ board ไม่ตรงกัน \
         ซึ่งเป็นบั๊กคนละตัวที่ `rebuild` จับอยู่แล้ว",
    ),
    (
        "crates/refx-core/src/spatial.rs",
        "if count == 0.0 {",
        "ค่าเฉลี่ยของศูนย์ช่อง — ตัวเรียกถามเฉพาะตอนมีของ",
    ),
    (
        "crates/refx-core/src/spatial.rs",
        "if value.is_nan() {",
        "NaN ถูกกรองที่ `ItemCanvas::sanitized` ตั้งแต่ทางเข้า board แล้ว (I-4) \
         · ด่านนี้เป็นชั้นที่สองที่ไม่มีทางไปถึงด้วย input ที่ผ่าน board มา",
    ),
];

/// ด่านหนึ่งตัวที่ค้นเจอในซอร์ส
#[derive(Debug)]
struct Guard {
    file: String,
    /// บรรทัดของตัวด่าน (0-based)
    at: usize,
    /// ข้อความที่จะแทรกไว้ข้างบนเพื่อให้ด่าน "ทำงานทุกครั้ง"
    inject: String,
    /// ข้อความของบรรทัดด่าน — ใช้จับคู่กับ [`ALLOWED`]
    text: String,
}

/// รันประตู
///
/// # Errors
/// เมื่อมีด่านที่ไม่มีเทสต์ตัวไหนเห็น หรือทะเบียนไม่ตรงกับซอร์สจริง
pub fn run() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(2);
    let mut krate = String::from("refx-core");
    let mut time_only = false;
    for arg in args.by_ref() {
        if arg == "--time-only" {
            time_only = true;
        } else {
            krate = arg;
        }
    }
    let nc = std::env::var_os("REFX_MUT_NC").is_some();

    let real = root()?;
    // ★★★ **ทำงานบนสำเนา ไม่ใช่ทรีจริง** (เจอจากการรันจริง 12 ก.ย. 2026)
    //
    //   เครื่องมือนี้แก้ไฟล์ซอร์สทีละตัวแล้วคืนสภาพ · ตราบใดที่มันทำงานอยู่
    //   **ทรีจริงมีไฟล์ที่ถูกดัดแปลงอยู่เสมอหนึ่งไฟล์** → `cargo clippy` ที่คนรัน
    //   ขนานกันอ่านโค้ดนั้นแล้วรายงาน error ที่ไม่มีอยู่จริง (เกิดจริง: clippy
    //   บ่น `unreachable statement` ใน `command.rs` ที่ไม่มีใครแก้)
    //   · และถ้าโดน Ctrl+C กลางคัน **ไฟล์ของผู้ใช้ค้างสภาพ mutate** ซึ่งเป็น
    //   เครื่องมือที่ทำให้งานเสียหายเอง — ขัดลำดับความสำคัญข้อ 1 ของโปรเจกต์
    let root = mirror(&real)?;
    let target = real.join("target").join("mutation");
    println!("crate: {krate}  สำเนา: {}", root.display());

    let guards = find_guards(&root, &krate)?;
    anyhow::ensure!(
        guards.len() > 5,
        "ตัวค้นหาเจอด่านแค่ {} ตัว — มันอ่านซอร์สไม่โดน",
        guards.len()
    );
    println!("ด่านที่ค้นเจอเอง: {}", guards.len());

    // ★ ฐานต้องเขียวก่อน ไม่งั้นทุกตัวจะ "แดง" ด้วยเหตุผลที่ไม่เกี่ยวกับ mutation
    let started = std::time::Instant::now();
    anyhow::ensure!(
        run_tests(&root, &krate, &target)?,
        "ฐานยังไม่เขียว — หยุดก่อนที่จะรายงานตัวเลขที่ไม่มีความหมาย"
    );
    let baseline = started.elapsed();
    println!("ฐานเขียวใน {:.1} วินาที", baseline.as_secs_f64());

    if time_only {
        println!(
            "\nประเมินเวลาทั้งรอบ: {} ด่าน × {:.1} วินาที ≈ {:.0} นาที",
            guards.len(),
            baseline.as_secs_f64(),
            guards.len() as f64 * baseline.as_secs_f64() / 60.0
        );
        return Ok(());
    }

    let mut survivors: Vec<&Guard> = Vec::new();
    let mut tested = 0usize;
    for guard in &guards {
        let path = root.join(&guard.file);
        let original = std::fs::read_to_string(&path)?;
        let mut lines: Vec<&str> = original.lines().collect();
        lines.insert(guard.at, &guard.inject);
        std::fs::write(&path, lines.join("\n") + "\n")?;

        let green = run_tests(&root, &krate, &target);
        std::fs::write(&path, &original)?; // ★ คืนสภาพก่อนตัดสินใจอะไรทั้งสิ้น

        match green {
            Ok(true) => {
                survivors.push(guard);
                println!("รอด  {}:{}  {}", guard.file, guard.at + 1, guard.text);
                tested += 1;
            }
            Ok(false) => tested += 1,
            // คอมไพล์ไม่ผ่าน = การแทรกไม่ถูกไวยากรณ์ตรงนั้น ไม่ใช่ผลของเทสต์
            Err(_) => println!("ข้าม  {}:{}  (แทรกแล้วคอมไพล์ไม่ผ่าน)", guard.file, guard.at + 1),
        }
    }

    // ---- ★ ข้อยกเว้นถูกพิมพ์ทุกครั้ง (`§3.9` ข้อ 9) ----
    let mine: Vec<_> = ALLOWED
        .iter()
        .filter(|(f, _, _)| f.contains(&krate))
        .collect();
    println!("\nข้อยกเว้นที่ขึ้นทะเบียนไว้ {} ข้อ:", mine.len());
    for (file, text, why) in &mine {
        println!("  · {file} :: {text}\n      {why}");
    }

    // ---- ทะเบียนต้องไม่มีของที่ไม่มีอยู่จริงแล้ว ----
    let mut stale: Vec<&str> = Vec::new();
    for (file, text, _) in &mine {
        if !guards.iter().any(|g| g.file == *file && g.text == *text) {
            stale.push(text);
        }
    }
    let unexpected = unregistered(&survivors, nc);

    println!(
        "\nด่านที่ทดสอบ: {tested} · รอดจากทุกเทสต์: {} · ไม่ได้ขึ้นทะเบียน: {} · {:.0} วินาที",
        survivors.len(),
        unexpected.len(),
        started.elapsed().as_secs_f64()
    );
    if nc {
        println!("NC วิ่งผ่านจริง: ปลอมตัวรอดเข้าไปหนึ่งตัว — ประตูต้องล้มบรรทัดถัดไป");
    }

    anyhow::ensure!(
        stale.is_empty(),
        "★ ทะเบียนพูดถึงด่านที่ไม่มีอยู่ในซอร์สแล้ว — ลบออกจาก ALLOWED:\n  {}",
        stale.join("\n  ")
    );
    anyhow::ensure!(
        unexpected.is_empty(),
        "★★★ ด่านที่ **ไม่มีเทสต์ตัวไหนเห็น** — input ไปไม่ถึงกิ่งนี้เลย\n\
         (`docs/08 §3.9` ข้อ 1b) · เขียนเทสต์ที่เข้าถึงมัน หรือขึ้นทะเบียนพร้อมเหตุผล:\n  {}",
        unexpected.join("\n  ")
    );
    println!("\nทุกด่านมีเทสต์เห็น หรือขึ้นทะเบียนไว้แล้ว");
    Ok(())
}

/// ★★★ **ตัวตัดสินของประตู** — แยกออกมาเป็นฟังก์ชันบริสุทธิ์โดยตั้งใจ
///
/// ตัวรอดที่ไม่มีใน [`ALLOWED`] = ด่านที่ไม่มีเทสต์ตัวไหนเห็น
///
/// ## ★★ ทำไม NC อยู่ที่นี่ ไม่ใช่ใน CI
///
/// NC ของประตูนี้ต้องพิสูจน์ว่า **มันล้มเพราะตัวรอดที่ไม่ได้ขึ้นทะเบียน** ·
/// การยิงมันใส่ crate จริงพิสูจน์ข้อนั้นไม่ได้เลยถ้า crate นั้น**มีตัวรอดของจริง
/// อยู่แล้ว** — ประตูจะล้มด้วยเหตุผลคนละอย่างแล้วเราจะอ่านว่า "NC ผ่าน"
/// (เกิดจริง 12 ก.ย. 2026: `refx-platform` มีตัวรอดจริงห้าตัว)
///
/// ★ และรอบเต็มของ crate ที่สะอาดใช้เวลา 7–18 นาที — แพงเกินกว่าจะจ่ายเพื่อ NC
/// → ตัดสินใจแยกออกมา แล้ว NC ยิงตรงเข้าตัวตัดสิน **ทุก push** แทนสัปดาห์ละครั้ง
fn unregistered(survivors: &[&Guard], nc: bool) -> Vec<String> {
    survivors
        .iter()
        .filter(|g| {
            !ALLOWED
                .iter()
                .any(|(file, text, _)| *file == g.file && *text == g.text)
        })
        .map(|g| format!("{}:{}  {}", g.file, g.at + 1, g.text))
        // ★ negative control: ปลอมตัวรอดขึ้นมาหนึ่งตัว ประตูต้องล้มพร้อมตำแหน่ง
        .chain(nc.then(|| "crates/fake/src/nc.rs:1  if true { return; }".to_owned()))
        .collect()
}

/// ★★★ สำเนาของ workspace ที่ mutation จะไปแก้ — **ทรีจริงไม่ถูกแตะเลย**
///
/// คัดเฉพาะสิ่งที่ `cargo test -p <crate> --lib` ต้องใช้: `Cargo.toml`/`Cargo.lock`
/// `clippy.toml` และ `crates/` ทั้งก้อน · **ไม่คัด `target/`** ซึ่งใหญ่เป็น GB
/// (target dir ของการรันชี้กลับไปที่ทรีจริงเพื่อใช้ cache ร่วมกัน)
fn mirror(real: &Path) -> anyhow::Result<PathBuf> {
    let to = std::env::temp_dir().join(format!("refx-mutation-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&to);
    std::fs::create_dir_all(&to)?;
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "clippy.toml",
        "rust-toolchain.toml",
    ] {
        let from = real.join(name);
        if from.exists() {
            std::fs::copy(&from, to.join(name))?;
        }
    }
    copy_tree(&real.join("crates"), &to.join("crates"))?;
    copy_tree(&real.join("xtask"), &to.join("xtask"))?;
    // ★ `refx-ui` ฝังฟอนต์ด้วย `include_bytes!("../../../assets/...")` — สำเนา
    //   ที่ไม่มีโฟลเดอร์นี้จะคอมไพล์ไม่ผ่านทุกตัว แล้วประตูจะ "ข้าม" ทั้งชุด
    //   โดยดูเหมือนทำงานปกติ
    copy_tree(&real.join("assets"), &to.join("assets"))?;
    Ok(to)
}

/// คัดโฟลเดอร์ทั้งก้อน — ★ ข้าม `target/` เสมอ
fn copy_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" {
            continue;
        }
        if path.is_dir() {
            copy_tree(&path, &to.join(&name))?;
        } else {
            std::fs::copy(&path, to.join(&name))?;
        }
    }
    Ok(())
}

/// รากของ workspace
fn root() -> anyhow::Result<PathBuf> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    Ok(here
        .parent()
        .ok_or_else(|| anyhow::anyhow!("หารากของ workspace ไม่เจอ"))?
        .to_path_buf())
}

/// รันเทสต์ของ crate นี้ใน target dir แยก — คืน `true` ถ้าเขียว
///
/// ★ target dir แยกเพื่อไม่ให้ชนล็อกกับ build ที่คนกำลังทำอยู่
fn run_tests(root: &Path, krate: &str, target: &Path) -> anyhow::Result<bool> {
    let out = std::process::Command::new("cargo")
        .args(["test", "-p", krate, "--lib", "--", "--quiet"])
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target)
        .output()?;
    let text = String::from_utf8_lossy(&out.stderr);
    // ★ แยก "เทสต์แดง" ออกจาก "คอมไพล์ไม่ผ่าน" — สองอย่างนี้แปลคนละความหมาย
    if text.contains("error[") || text.contains("could not compile") {
        anyhow::bail!("คอมไพล์ไม่ผ่าน");
    }
    Ok(out.status.success())
}

/// ★★★ ค้นหาด่านเองจากซอร์ส — **ไม่มีรายชื่อไฟล์ที่เขียนด้วยมือ**
fn find_guards(root: &Path, krate: &str) -> anyhow::Result<Vec<Guard>> {
    let mut out = Vec::new();
    let src = root.join("crates").join(krate).join("src");
    let mut stack = vec![src];
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();

    for path in files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(&path)?;
        let lines: Vec<&str> = source.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            // ★ เทสต์สร้างด่านของตัวเองได้ตามสบาย — ไม่นับ
            if lines[..at]
                .iter()
                .any(|l| l.trim_start().starts_with("#[cfg(test)]"))
            {
                break;
            }
            let trimmed = line.trim();
            let indent = &line[..line.len() - line.trim_start().len()];
            let Some(next) = lines.get(at + 1) else {
                continue;
            };
            let body = next.trim();
            // รูปที่แปลงอัตโนมัติได้อย่างปลอดภัย: ด่านหนึ่งบรรทัดที่ออกจากฟังก์ชัน
            let is_guard = (trimmed.starts_with("let Some(") || trimmed.starts_with("let Ok("))
                && trimmed.ends_with("else {")
                || (trimmed.starts_with("if ")
                    && trimmed.ends_with('{')
                    && lines.get(at + 2).is_some_and(|l| l.trim() == "}"));
            if !is_guard || !leaves_the_function(body) {
                continue;
            }
            out.push(Guard {
                file: rel.clone(),
                at,
                inject: format!("{indent}{body} // MUT"),
                text: trimmed.to_owned(),
            });
        }
    }
    Ok(out)
}

/// บรรทัดนี้ออกจากฟังก์ชันไปเลยไหม
///
/// ★ `continue`/`break` ไม่นับ — มันออกจาก *ลูป* ไม่ใช่จากฟังก์ชัน การแทรก
/// มันไว้ข้างบนจะได้ความหมายคนละอย่างกับ "ด่านทำงานทุกครั้ง"
fn leaves_the_function(body: &str) -> bool {
    body == "None" || (body.starts_with("return") && body.ends_with(';'))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// ★★★ NC: ตัวค้นหาต้องเจอด่านที่เพิ่งถูกเขียน — ไม่งั้นประตูเงียบตลอดกาล
    #[test]
    fn the_finder_sees_a_guard_that_was_just_written() {
        let dir = std::env::temp_dir().join(format!("refx-mut-nc-{}", std::process::id()));
        let src = dir.join("crates").join("fakecrate").join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("brand_new.rs"),
            "fn already_here() {\n\
             \x20   let Some(x) = thing() else {\n\
             \x20       return;\n\
             \x20   };\n\
             }\n\
             \n\
             fn added_today(&self) -> bool {\n\
             \x20   if self.flag {\n\
             \x20       return false;\n\
             \x20   }\n\
             \x20   true\n\
             }\n\
             \n\
             #[cfg(test)]\n\
             mod tests {\n\
             \x20   fn helper() {\n\
             \x20       if x {\n\
             \x20           return;\n\
             \x20       }\n\
             \x20   }\n\
             }\n",
        )
        .unwrap();

        let found = find_guards(&dir, "fakecrate").unwrap();
        let texts: Vec<&str> = found.iter().map(|g| g.text.as_str()).collect();
        assert!(
            texts.iter().any(|t| t.starts_with("let Some(x)")),
            "ไม่เห็นด่าน let-else: {texts:?}"
        );
        assert!(
            texts.contains(&"if self.flag {"),
            "ไม่เห็นด่าน if ที่เพิ่งเพิ่ม — ไฟล์ใหม่จะหลุดประตูไปเงียบ ๆ: {texts:?}"
        );
        assert_eq!(found.len(), 2, "นับด่านในเทสต์ติดมาด้วย: {texts:?}");

        // ★ ประตูของประตู: `continue` ออกจากลูป ไม่ใช่จากฟังก์ชัน — ต้องไม่นับ
        assert!(!leaves_the_function("continue;"));
        assert!(!leaves_the_function("break;"));
        assert!(leaves_the_function("None"));
        assert!(leaves_the_function("return false;"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★★★ NC ของประตู: ตัวรอดที่ไม่ได้ขึ้นทะเบียน **ต้องถูกรายงานพร้อมตำแหน่ง**
    #[test]
    fn the_gate_really_does_fail_on_a_guard_nobody_tests() {
        let known = Guard {
            file: "crates/refx-core/src/spatial.rs".to_owned(),
            at: 403,
            inject: String::new(),
            text: "if count == 0.0 {".to_owned(),
        };
        let brand_new = Guard {
            file: "crates/refx-core/src/brand_new.rs".to_owned(),
            at: 41,
            inject: String::new(),
            text: "if nobody_tests_this {".to_owned(),
        };

        // ตัวที่ขึ้นทะเบียนแล้วต้องเงียบ — ★ ตัวจับที่ร้องตอนปกติคือตัวจับที่ถูกปิดเสียง
        assert!(unregistered(&[&known], false).is_empty());

        // ด่านใหม่ที่ไม่มีเทสต์เห็น = แดง **พร้อมบอกไฟล์และบรรทัด**
        let caught = unregistered(&[&known, &brand_new], false);
        assert_eq!(caught.len(), 1, "จับไม่ได้ หรือจับเกิน: {caught:?}");
        assert!(
            caught[0].contains("brand_new.rs:42") && caught[0].contains("nobody_tests_this"),
            "รายงานไม่ได้บอกว่าด่านไหน: {caught:?}"
        );

        // และธง NC ต้องทำให้ล้มได้แม้ทุกอย่างสะอาด
        assert_eq!(unregistered(&[&known], true).len(), 1, "REFX_MUT_NC ไม่มีผล");
        println!("NC วิ่งผ่านจริง: ประตูชี้ตำแหน่งของด่านที่ไม่มีใครตรวจได้");
    }

    /// ★ ทะเบียนต้องไม่มีบรรทัดว่างเป็นเหตุผล — ข้อยกเว้นที่ไม่มีเหตุผลคือรูรั่ว
    #[test]
    fn every_registered_exception_says_why() {
        for (file, text, why) in ALLOWED {
            assert!(!file.trim().is_empty(), "ข้อยกเว้นไม่มีไฟล์");
            assert!(!text.trim().is_empty(), "ข้อยกเว้นไม่มีตัวด่าน");
            assert!(
                why.trim().len() > 20,
                "{file} :: {text} ขึ้นทะเบียนโดยไม่มีเหตุผลที่อ่านได้"
            );
        }
    }
}
