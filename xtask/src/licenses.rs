//! สร้าง `THIRD-PARTY-LICENSES.md` — **เงื่อนไขทางกฎหมายของการแจกจ่าย**
//!
//! ## ทำไมต้องสร้างจากเครื่องมือ
//!
//! รายการที่พิมพ์ด้วยมือ **ล้าสมัยเงียบ ๆ ทุกครั้งที่ `Cargo.lock` ขยับ** —
//! และความเสียหายไม่ได้โผล่ตอนคอมไพล์ แต่โผล่ตอนมีคนถามว่าเราแจกโค้ดของเขา
//! โดยไม่ใส่ใบอนุญาตมาด้วยหรือเปล่า
//!
//! ## ★★★ ประตู: `--check` **สร้างใหม่แล้วเทียบเนื้อ** ไม่ใช่เทียบเวลาไฟล์
//!
//! `ROADMAP` เขียนไว้ว่า *"ไฟล์นี้ต้องใหม่กว่า `Cargo.lock`"* ซึ่งเจตนาถูก
//! แต่กลไกนั้นใช้จริงไม่ได้: **git ไม่เก็บเวลาแก้ไขไฟล์** — clone ใหม่ทุกครั้ง
//! ไฟล์ทั้งหมดได้เวลาเดียวกันโดยเรียงตามใจ checkout ประตูจะเขียวหรือแดงแบบสุ่ม
//! บนเครื่อง CI แล้วไม่มีใครเชื่อมันอีก
//!
//! → ใช้กลไกที่**แรงกว่าและอยู่รอดจาก git**: สร้างใหม่ทั้งไฟล์แล้วเทียบไบต์
//! (รูปเดียวกับ `cargo fmt --check`) · ในหัวไฟล์มี crc32 ของ `Cargo.lock`
//! ฝังอยู่ด้วย → lock ขยับเมื่อไหร่ เนื้อไฟล์ที่สร้างใหม่ก็ต่างทันที **แดงแน่นอน
//! ไม่ว่าเวลาไฟล์จะเป็นอะไร** ซึ่งครอบคลุมเจตนาเดิมทั้งหมด
//!
//! ## ขอบเขต: อะไรถูกนับ
//!
//! **dependency ปกติของ `refx-app` เท่านั้น** ยูเนียนของสองแพลตฟอร์มที่เราแจกจริง
//! · `dev-dependencies` ไม่ถูกแจก (criterion / proptest ไม่ได้อยู่ในไบนารี)
//! · `build-dependencies` ก็ไม่อยู่ในไบนารีเช่นกัน
//! · crate ของเราเอง (path dependency) ไม่ใช่ "บุคคลที่สาม"
//!
//! ## ใช้
//!
//! ```text
//! cargo xtask licenses           # สร้าง/อัปเดตไฟล์
//! cargo xtask licenses --check   # ประตู: ไฟล์ตรงกับ Cargo.lock ปัจจุบันไหม
//! ```

// ★ เหตุผลเดียวกับ `mutation.rs` — เครื่องมือ dev ที่อ่านไฟล์ของ toolchain เอง
//   ไม่มี UI thread ให้บล็อก และไม่มีไฟล์ของผู้ใช้เข้ามาเกี่ยวเลย
#![expect(
    clippy::disallowed_methods,
    reason = "เครื่องมือ dev อ่านซอร์สของ dependency เอง ไม่ใช่ดิสก์ I/O บนลูปเฟรม"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// ไฟล์ปลายทาง — ★ ต้องถูกใส่ลงในทุกแพ็กเกจที่แจก (`ROADMAP` ก้อน d)
pub const OUTPUT: &str = "THIRD-PARTY-LICENSES.md";

/// แพลตฟอร์มที่เราแจกจริง — ยูเนียนของทั้งสอง
///
/// ★ ไม่ใช้ `--target all` เพราะจะลากใบอนุญาตของ crate สำหรับ macOS/wasm/android
/// เข้ามาด้วย ซึ่งเราไม่ได้แจก · การประกาศเกินไม่ผิดกฎหมาย แต่ทำให้ไฟล์นี้
/// ตอบคำถาม "อะไรอยู่ในโปรแกรมที่ฉันถือ" ไม่ได้ ซึ่งเป็นหน้าที่เดียวของมัน
const TARGETS: &[(&str, &str)] = &[
    ("x86_64-pc-windows-msvc", "Windows"),
    ("x86_64-unknown-linux-gnu", "Linux"),
];

/// ไฟล์ใน `assets/` ที่ถูก **ฝังลงไบนารี** — ไม่ใช่ crate จึงไม่มีใน `Cargo.lock`
///
/// ★ ตรงนี้เป็นรายการเดียวในไฟล์นี้ที่เขียนด้วยมือ **โดยจำเป็น** — และมีประตู
/// คุมอยู่: ทุกไฟล์ที่ `include_bytes!` ใน `crates/` ต้องมีแถวของมันที่นี่
/// (ดู [`check_embedded`]) ไม่งั้นแดง
const EMBEDDED: &[(&str, &str, &str)] = &[(
    "assets/fonts/NotoSansThai-Regular.ttf",
    "SIL Open Font License 1.1 (OFL-1.1)",
    "assets/fonts/OFL.txt",
)];

/// ★★★ crate ที่ **ไม่มีไฟล์ตัวบทใบอนุญาตมากับ tarball เลย**
///
/// ใช้ตาม SPDX ที่มันประกาศไว้ใน `Cargo.toml` แทน · จับคู่ด้วย **ชื่อ ไม่ใช่รุ่น**
/// เพราะรุ่นขยับทุกสัปดาห์แล้วทะเบียนจะกลายเป็นของที่ต้องแก้ตลอดเวลา
///
/// ## ทำไมต้องเขียนไว้ แทนที่จะปล่อยให้เงียบ
///
/// 13 ก.ย. 2026: ไฟล์ที่สร้างบนเครื่องพัฒนา (เจอตัวบท 298 ตัว) **ไม่ตรงกับ**
/// ไฟล์ที่สร้างบน CI (เจอ 295) — ทั้ง Linux และ Windows · ประตูแดงโดยบอกได้แค่
/// "ไม่ตรง" และไม่มีใครรู้ว่าหายไปตัวไหน
///
/// ★ รากของปัญหาเชิงออกแบบ: เนื้อไฟล์ขึ้นกับ **สิ่งที่บังเอิญมีอยู่ในเครื่อง**
/// ซึ่งเป็นสิ่งที่ version control ไม่ได้พาไปด้วย (กฎเดียวกับที่ห้ามใช้ mtime
/// เป็นฐานของประตู — `docs/08 §6`) · ทะเบียนนี้ทำให้ความคาดหวังถูกเขียนไว้
/// **ในคอมมิต**: เจอไม่ครบเมื่อไหร่ ประตูบอกชื่อทันที ไม่ใช่บอกแค่ว่าไม่ตรง
const NO_LICENSE_TEXT: &[&str] = &[
    "accesskit",
    "clipboard-win",
    "ecolor",
    "egui",
    "egui-wgpu",
    "egui-winit",
    "emath",
    "epaint",
    "epaint_default_fonts",
    "gpu-descriptor",
    "gpu-descriptor-types",
    "hexf-parse",
    "profiling",
    "spirv",
    "zune-core",
    "zune-jpeg",
];

/// crate หนึ่งตัวที่ถูกแจกไปกับโปรแกรม
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Dep {
    name: String,
    version: String,
    /// SPDX ตามที่ crate ประกาศไว้เอง
    license: String,
}

impl Dep {
    fn label(&self) -> String {
        format!("{} {}", self.name, self.version)
    }

    /// โฟลเดอร์ที่ cargo แตกซอร์สของ crate นี้ไว้
    fn dir_name(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }
}

/// สร้างไฟล์ หรือตรวจว่าไฟล์ยังตรงกับ `Cargo.lock` ปัจจุบัน
///
/// # Errors
/// เมื่อเรียก cargo ไม่สำเร็จ · หาซอร์สของ crate ไม่เจอ · หรือ (`--check`)
/// ไฟล์ในทรีไม่ตรงกับสิ่งที่สร้างใหม่
pub fn run() -> anyhow::Result<()> {
    let check = std::env::args().any(|a| a == "--check");
    let root = root()?;

    // ★ ต้องมีซอร์สของ **ทุก** แพลตฟอร์มอยู่ในเครื่อง ไม่งั้นฝั่งที่ไม่ได้ build
    //   จะอ่านตัวบทใบอนุญาตไม่ได้ แล้วไฟล์จะต่างกันไปตามเครื่องที่สร้าง
    fetch_every_platform(&root)?;

    let deps = roster(&root)?;
    anyhow::ensure!(
        deps.len() > 50,
        "เจอ dependency แค่ {} ตัว — `cargo tree` อ่านไม่โดน",
        deps.len()
    );
    check_embedded(&root)?;

    let lock = std::fs::read(root.join("Cargo.lock"))?;
    let doc = render(&deps, &collect_texts(&deps)?, crc32fast::hash(&lock));

    let target = root.join(OUTPUT);
    if !check {
        std::fs::write(&target, doc.as_bytes())?;
        println!("เขียน {OUTPUT} — {} crate · {} ไบต์", deps.len(), doc.len());
        return Ok(());
    }

    let found = std::fs::read_to_string(&target).unwrap_or_default();
    // ★ เทียบหลัง normalize ปลายบรรทัด — git บน Windows แปลงให้เองได้
    //   ประตูที่แดงเพราะ `\r` คือประตูที่คนจะปิดทิ้ง ไม่ใช่ประตูที่ทำงาน
    let found = found.replace("\r\n", "\n");
    anyhow::ensure!(
        found == doc,
        "★★★ {OUTPUT} ไม่ตรงกับ `Cargo.lock` ปัจจุบัน\n\
         dependency เปลี่ยนแล้วแต่ไฟล์ใบอนุญาตยังเป็นของเก่า — \
         นั่นแปลว่าเราแจกโค้ดของคนอื่นโดยไม่มีใบอนุญาตของเขาอยู่ในแพ็กเกจ\n\
         → รัน `cargo xtask licenses` แล้ว commit ไฟล์ที่ได้\n\n{}",
        difference(&found, &doc)
    );
    println!("{OUTPUT} ตรงกับ Cargo.lock ปัจจุบัน — {} crate", deps.len());
    Ok(())
}

/// ★★★ บอกให้ได้ว่า **ต่างกันตรงไหน** ไม่ใช่แค่ว่า "ไม่ตรง"
///
/// ประตูที่บอกได้แค่ว่าไม่ผ่าน บังคับให้คนไปหาเองว่าอะไรผิด — บนเครื่อง CI
/// ที่ไม่มีใครเข้าไปดูได้ นั่นแปลว่าไม่มีใครรู้เลย (`docs/08 §3.9` ข้อ 9)
///
/// ★ ไฟล์นี้ใหญ่ 700 KB จึงพิมพ์แค่ **บรรทัดแรกที่ต่าง** กับสรุปจำนวน —
/// พอที่จะแยก "lock ขยับเฉย ๆ" (ต่างบรรทัดเดียวตรงหัว) ออกจาก
/// "รายชื่อ crate เปลี่ยน" (ต่างหลายบรรทัดกลางไฟล์) ได้ทันที
fn difference(found: &str, want: &str) -> String {
    let mut out = String::new();
    let found_lines: Vec<&str> = found.lines().collect();
    let want_lines: Vec<&str> = want.lines().collect();
    out.push_str(&format!(
        "ไฟล์ในทรี {} บรรทัด · ที่ควรเป็น {} บรรทัด\n",
        found_lines.len(),
        want_lines.len()
    ));

    let differing = found_lines
        .iter()
        .zip(&want_lines)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .count();
    out.push_str(&format!("บรรทัดที่ต่างกัน (เท่าที่เทียบคู่ได้): {differing}\n"));

    if let Some((at, (a, b))) = found_lines
        .iter()
        .zip(&want_lines)
        .enumerate()
        .find(|(_, (a, b))| a != b)
    {
        out.push_str(&format!(
            "\nบรรทัดแรกที่ต่าง — #{}\n  ในทรี : {}\n  ควรเป็น: {}\n",
            at + 1,
            trim_for_log(a),
            trim_for_log(b)
        ));
    } else if found_lines.len() != want_lines.len() {
        out.push_str("\nต่างกันที่ **ความยาว** เท่านั้น — ไฟล์ถูกตัดหรือมีของต่อท้าย\n");
    }
    out
}

/// ตัดบรรทัดยาวก่อนพิมพ์ลง log — ตัวบทใบอนุญาตบางบรรทัดยาวเป็นพันตัวอักษร
fn trim_for_log(line: &str) -> String {
    const MAX: usize = 160;
    if line.chars().count() <= MAX {
        return line.to_owned();
    }
    let short: String = line.chars().take(MAX).collect();
    format!("{short}… (ตัดจาก {} ตัวอักษร)", line.chars().count())
}

/// รากของ workspace
fn root() -> anyhow::Result<PathBuf> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    Ok(here
        .parent()
        .ok_or_else(|| anyhow::anyhow!("หารากของ workspace ไม่เจอ"))?
        .to_path_buf())
}

/// ดึงซอร์สของ dependency ให้ครบทุกแพลตฟอร์มก่อนอ่านตัวบท
fn fetch_every_platform(root: &Path) -> anyhow::Result<()> {
    for (triple, _) in TARGETS {
        let out = std::process::Command::new("cargo")
            .args(["fetch", "--locked", "--target", triple])
            .current_dir(root)
            .output()?;
        anyhow::ensure!(
            out.status.success(),
            "`cargo fetch --target {triple}` ไม่สำเร็จ:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// รายชื่อ crate ที่ถูกแจก — ยูเนียนของทุกแพลตฟอร์มใน [`TARGETS`]
fn roster(root: &Path) -> anyhow::Result<Vec<Dep>> {
    let mut all: BTreeSet<Dep> = BTreeSet::new();
    // ★★★ ยุบตาม **(ชื่อ, รุ่น)** ไม่ใช่ตามทุกฟิลด์ — ดู [`dedup_by_identity`]
    for (triple, _) in TARGETS {
        let out = std::process::Command::new("cargo")
            .args([
                "tree", "-p", "refx-app", "--edges",
                "normal", // ★ ไม่เอา dev/build — สองอย่างนั้นไม่ได้อยู่ในไบนารี
                "--target", triple, "--prefix", "none", "--format", "{p}|{l}",
            ])
            .current_dir(root)
            .output()?;
        anyhow::ensure!(
            out.status.success(),
            "`cargo tree --target {triple}` ไม่สำเร็จ:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some(dep) = parse_line(line) {
                all.insert(dep);
            }
        }
    }
    Ok(dedup_by_identity(all))
}

/// ★★★ crate เดียวกันต้องมีแถวเดียว — ยุบตาม **(ชื่อ, รุ่น)**
///
/// ## อาการที่ทำให้ต้องมีฟังก์ชันนี้ (14 ก.ย. 2026)
///
/// `BTreeSet<Dep>` ยุบตาม **ทุกฟิลด์** รวม `license` · ถ้าการ resolve สอง
/// แพลตฟอร์มรายงานสตริงใบอนุญาตของ crate เดียวกันต่างกันแม้แต่นิดเดียว
/// **crate นั้นจะกลายเป็นสองแถว** แล้วไฟล์ที่ได้ก็ต่างจากเครื่องอื่นทันที
///
/// เกิดจริงบน CI: `egui` · `emath` · `epaint` โผล่อย่างละสองครั้ง (19 แทนที่จะ
/// เป็น 16) ทั้งบน Linux และ Windows ขณะที่เครื่องพัฒนาได้ 16 — และประตูบอกได้
/// แค่ "ไม่ตรง" จนกระทั่งเปลี่ยนมาพิมพ์ **ชื่อ** ออกมา
///
/// ★ ความไม่ตรงกันของสตริงใบอนุญาตเป็น **ข้อมูล ไม่ใช่ของที่ควรกลบ** —
/// ถ้ามันเกิดขึ้น ให้พิมพ์ออกมาให้เห็น แล้วเลือกตัวแรกตามลำดับตัวอักษร
/// เพื่อให้ผลลัพธ์คงที่ไม่ว่าจะรันบนเครื่องไหน
fn dedup_by_identity(all: BTreeSet<Dep>) -> Vec<Dep> {
    let mut by_identity: BTreeMap<(String, String), Vec<Dep>> = BTreeMap::new();
    for dep in all {
        by_identity
            .entry((dep.name.clone(), dep.version.clone()))
            .or_default()
            .push(dep);
    }

    let mut out = Vec::with_capacity(by_identity.len());
    for ((name, version), mut rows) in by_identity {
        if rows.len() > 1 {
            let seen: Vec<&str> = rows.iter().map(|d| d.license.as_str()).collect();
            println!(
                "★ {name} {version} ถูกรายงานด้วยใบอนุญาตต่างกันระหว่างแพลตฟอร์ม: {seen:?} \
                 — ใช้ตัวแรกตามลำดับตัวอักษรเพื่อให้ผลคงที่"
            );
        }
        rows.sort();
        out.push(rows.remove(0));
    }
    out
}

/// อ่านบรรทัดของ `cargo tree --format "{p}|{l}"`
///
/// รูปที่เจอจริง:
/// * `ahash v0.8.12|MIT OR Apache-2.0`
/// * `ahash v0.8.12|MIT OR Apache-2.0 (*)` — กิ่งที่ cargo ยุบเพราะซ้ำ
/// * `refx-core v0.1.0 (E:\ref 10.0\crates\refx-core)|MIT OR Apache-2.0` — **ของเราเอง**
fn parse_line(line: &str) -> Option<Dep> {
    let line = line.trim().strip_suffix(" (*)").unwrap_or(line.trim());
    let (package, license) = line.rsplit_once('|')?;
    let package = package.trim();
    // ★ path dependency = crate ของเราเอง ไม่ใช่บุคคลที่สาม
    if package.ends_with(')') {
        return None;
    }
    let (name, version) = package.rsplit_once(" v")?;
    Some(Dep {
        name: name.trim().to_owned(),
        version: version.trim().to_owned(),
        license: license.trim().to_owned(),
    })
}

/// ทุกไฟล์ที่ถูก `include_bytes!` ต้องมีแถวใน [`EMBEDDED`]
///
/// ★★ ประตูนี้มีเพราะ [`EMBEDDED`] เป็นรายการที่เขียนด้วยมือ — และรายการที่
/// เขียนด้วยมือจะล้าสมัยเงียบ ๆ เสมอ · ฟอนต์ตัวที่สองที่ถูกฝังในวันหนึ่ง
/// จะทำให้แดงทันที แทนที่จะถูกแจกออกไปโดยไม่มีใบอนุญาตของมัน
fn check_embedded(root: &Path) -> anyhow::Result<()> {
    let mut missing = Vec::new();
    let mut seen = 0usize;
    for file in rust_sources(&root.join("crates"))? {
        let source = std::fs::read_to_string(&file)?;
        for (at, _) in source.match_indices("include_bytes!(\"") {
            let rest = &source[at + "include_bytes!(\"".len()..];
            let Some(end) = rest.find('"') else { continue };
            let raw = &rest[..end];
            seen += 1;
            // เส้นทางในโค้ดเป็นแบบสัมพัทธ์กับไฟล์นั้น — เทียบด้วยชื่อไฟล์ก็พอ
            let name = raw.rsplit('/').next().unwrap_or(raw);
            if !EMBEDDED.iter().any(|(path, _, _)| path.ends_with(name)) {
                missing.push(format!("{} → {raw}", file.display()));
            }
        }
    }
    anyhow::ensure!(
        missing.is_empty(),
        "★ มีไฟล์ที่ถูกฝังลงไบนารีแต่ไม่มีใบอนุญาตกำกับใน `EMBEDDED`:\n  {}",
        missing.join("\n  ")
    );
    println!("ไฟล์ที่ฝังลงไบนารี: {seen} — มีใบอนุญาตกำกับครบ");
    Ok(())
}

/// ไฟล์ `.rs` ทั้งหมดใต้โฟลเดอร์หนึ่ง
fn rust_sources(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != "target" {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// โฟลเดอร์ที่ cargo แตกซอร์สของ crate จาก registry ไว้
fn registry_roots() -> anyhow::Result<Vec<PathBuf>> {
    let home = match std::env::var_os("CARGO_HOME") {
        Some(home) => PathBuf::from(home),
        None => {
            let profile = std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .ok_or_else(|| anyhow::anyhow!("หา CARGO_HOME / HOME ไม่เจอ"))?;
            PathBuf::from(profile).join(".cargo")
        }
    };
    let src = home.join("registry").join("src");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&src)
        .map_err(|err| anyhow::anyhow!("อ่าน {} ไม่ได้: {err}", src.display()))?
        .flatten()
    {
        if entry.path().is_dir() {
            out.push(entry.path());
        }
    }
    anyhow::ensure!(!out.is_empty(), "ไม่มีซอร์สของ registry ที่ {}", src.display());
    Ok(out)
}

/// ตัวบทใบอนุญาตทั้งหมด → crate ที่ใช้ตัวบทนั้น
///
/// ★ จัดกลุ่มตาม **ตัวบท** ไม่ใช่ตามชื่อใบอนุญาต · MIT ของแต่ละ crate มีบรรทัด
/// copyright ของเจ้าของคนละคนอยู่ในตัวบท — การยุบตามชื่อจะทำให้ประกาศ
/// copyright ผิดคน ซึ่งเป็นสิ่งเดียวที่ MIT บังคับให้ทำให้ถูก
fn collect_texts(deps: &[Dep]) -> anyhow::Result<BTreeMap<String, Vec<String>>> {
    let roots = registry_roots()?;
    let mut texts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut no_file = Vec::new();

    for dep in deps {
        let Some(dir) = roots
            .iter()
            .map(|root| root.join(dep.dir_name()))
            .find(|path| path.is_dir())
        else {
            anyhow::bail!(
                "หาซอร์สของ {} ไม่เจอใน registry — รัน `cargo fetch` ก่อน",
                dep.label()
            );
        };

        let mut found = false;
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && is_license_file(p))
            .collect();
        files.sort();
        for file in files {
            let Ok(raw) = std::fs::read(&file) else {
                continue;
            };
            let text = String::from_utf8_lossy(&raw).replace("\r\n", "\n");
            let text = text.trim().to_owned();
            if text.is_empty() {
                continue;
            }
            found = true;
            texts.entry(text).or_default().push(dep.label());
        }
        if !found {
            no_file.push(dep.label());
        }
    }

    // ★★★ พิมพ์ **ชื่อ** ไม่ใช่แค่จำนวน · จำนวนบอกได้แค่ว่าไม่ตรง ชื่อบอกว่าตัวไหน
    //   (กฎเดียวกับที่ใช้ปิดคดี "+1 เทสต์ที่กระทบไม่ลง" — `docs/08 §3.9` ข้อ 9)
    println!(
        "crate ที่ไม่มีไฟล์ใบอนุญาตมาด้วย {} ตัว (ใช้ตาม SPDX ที่ประกาศไว้):",
        no_file.len()
    );
    for label in &no_file {
        println!("  · {label}");
    }

    let unexpected = unregistered_missing(&no_file);
    anyhow::ensure!(
        unexpected.is_empty(),
        "★★★ crate ข้างล่างนี้ **ควรมีตัวบทใบอนุญาตมาด้วย แต่หาไม่เจอ**:\n  {}\n\n\
         เนื้อของ {OUTPUT} จะขาดตัวบทเหล่านี้ไป และไฟล์ที่ได้จะไม่ตรงกับของเครื่องอื่น\n\
         สาเหตุที่เป็นไปได้: `cargo fetch` ยังไม่ได้แตกซอร์สครบ · หรือ crate นั้น\n\
         เลิกแถมไฟล์ใบอนุญาตมาจริง ๆ (ถ้าใช่ ให้เติมชื่อลง `NO_LICENSE_TEXT` \
         พร้อมยืนยันว่า SPDX ของมันยังอ่านได้จาก `Cargo.toml`)",
        unexpected.join("\n  ")
    );
    Ok(texts)
}

/// ★★★ **ตัวตัดสิน** — crate ที่หาตัวบทไม่เจอ ทั้งที่ไม่ได้ขึ้นทะเบียนไว้
///
/// แยกเป็นฟังก์ชันบริสุทธิ์เพื่อให้ NC ยิงเข้ามาได้โดยไม่ต้องมี registry จริง
/// (รูปเดียวกับ `unregistered()` ของประตู mutation)
fn unregistered_missing(no_file: &[String]) -> Vec<String> {
    no_file
        .iter()
        .filter(|label| {
            // `label` คือ "ชื่อ รุ่น" — ทะเบียนจับคู่ด้วยชื่อเท่านั้น
            let name = label.split(' ').next().unwrap_or(label);
            !NO_LICENSE_TEXT.contains(&name)
        })
        .cloned()
        .collect()
}

/// ชื่อไฟล์นี้ใช่ตัวบทใบอนุญาตไหม
fn is_license_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_uppercase())
        .unwrap_or_default();
    // ★ `LICENSE.spdx` เป็นเมทาดาทา ไม่ใช่ตัวบท · `.rs`/`.toml` ก็ไม่ใช่
    if name.ends_with(".SPDX") || name.ends_with(".RS") || name.ends_with(".TOML") {
        return false;
    }
    [
        "LICENSE",
        "LICENCE",
        "COPYING",
        "COPYRIGHT",
        "NOTICE",
        "UNLICENSE",
    ]
    .iter()
    .any(|stem| name.starts_with(stem))
}

/// ประกอบไฟล์ทั้งฉบับ
fn render(deps: &[Dep], texts: &BTreeMap<String, Vec<String>>, lock_crc: u32) -> String {
    let mut out = String::new();
    out.push_str("<!-- ★ สร้างโดย `cargo xtask licenses` — ห้ามแก้ด้วยมือ -->\n");
    out.push_str(&format!(
        "<!-- Cargo.lock crc32 = {lock_crc:#010x} · crate {} ตัว -->\n\n",
        deps.len()
    ));
    out.push_str("# ใบอนุญาตของซอฟต์แวร์บุคคลที่สาม / Third-party licenses\n\n");
    out.push_str(
        "RefX ถูกแจกจ่ายพร้อมกับซอฟต์แวร์ของบุคคลที่สามข้างล่างนี้ \
         เอกสารฉบับนี้คือประกาศและตัวบทใบอนุญาตของพวกเขา\n\n\
         RefX is distributed with the third-party software listed below. \
         This document reproduces their notices and license texts.\n\n",
    );
    out.push_str(&format!(
        "ขอบเขต: **dependency ปกติของโปรแกรมที่แจกจริง** บน {} \
         · ไม่รวมเครื่องมือที่ใช้ตอนพัฒนา (`dev-dependencies`) \
         และตัวช่วยตอน build (`build-dependencies`) ซึ่งไม่ได้อยู่ในไบนารี\n\n",
        TARGETS
            .iter()
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(" และ ")
    ));

    out.push_str("## 1. ฟอนต์ที่ฝังอยู่ในโปรแกรม\n\n");
    out.push_str("| ไฟล์ | ใบอนุญาต | ตัวบท |\n|---|---|---|\n");
    for (path, license, text) in EMBEDDED {
        out.push_str(&format!("| `{path}` | {license} | `{text}` |\n"));
    }
    out.push('\n');
    out.push_str(
        "> ★ `epaint` ฝังฟอนต์ของตัวเองมาด้วย — ใบอนุญาตของมัน \
         (`OFL-1.1` และ `Ubuntu-font-1.0`) อยู่ในรายการ crate ข้างล่าง\n\n",
    );

    // ---- สรุปตามชนิดใบอนุญาต ----
    let mut by_license: BTreeMap<&str, usize> = BTreeMap::new();
    for dep in deps {
        *by_license.entry(dep.license.as_str()).or_default() += 1;
    }
    out.push_str("## 2. สรุปตามชนิดใบอนุญาต\n\n");
    out.push_str("| ใบอนุญาตที่ crate ประกาศไว้ | จำนวน |\n|---|---:|\n");
    let mut rows: Vec<_> = by_license.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    for (license, count) in rows {
        out.push_str(&format!("| {license} | {count} |\n"));
    }
    out.push('\n');
    out.push_str(
        "> ★ `OR` แปลว่า **ผู้ใช้เลือกได้** — RefX เลือกทางที่ปล่อยเสรีที่สุดเสมอ \
         (MIT หรือ Apache-2.0) · การเลือกต้องประกาศ ไม่ใช่ปล่อยให้เดา\n\n",
    );

    // ---- ★★ ใบอนุญาตที่ต้องอ่านก่อน ไม่ใช่แค่ประกาศ ----
    //
    //   copyleft มีเงื่อนไขที่ผูกกับ *วิธีใช้* ไม่ใช่แค่การประกาศชื่อ · ถ้า crate
    //   ประเภทนี้เพิ่มเข้ามาวันหนึ่งโดยไม่มีใครเห็น เราจะรู้ตัวตอนมีคนมาถาม
    let copyleft: Vec<&Dep> = deps
        .iter()
        .filter(|dep| {
            ["GPL", "MPL", "CDDL", "EPL", "CC-BY-SA"]
                .iter()
                .any(|tag| dep.license.contains(tag))
        })
        .collect();
    out.push_str(&format!(
        "### 2ก. ใบอนุญาตที่มีเงื่อนไขผูกกับวิธีใช้ ({} ตัว)\n\n",
        copyleft.len()
    ));
    if copyleft.is_empty() {
        out.push_str("ไม่มี\n\n");
    } else {
        out.push_str("| crate | ใบอนุญาต | เราทำยังไง |\n|---|---|---|\n");
        for dep in copyleft {
            let stance = if dep.license.contains(" OR ") {
                "ใช้ทางเลือกที่ไม่ใช่ copyleft ตามที่ใบอนุญาตให้สิทธิ์เลือกไว้"
            } else {
                "ใช้เป็นไลบรารีโดย **ไม่แก้ซอร์สของมัน** — เงื่อนไขผูกกับไฟล์ที่ถูกแก้เท่านั้น \
                 ซอร์สต้นฉบับอยู่ที่ crates.io ตามรุ่นที่ระบุ"
            };
            out.push_str(&format!("| {} | {} | {stance} |\n", dep.name, dep.license));
        }
        out.push('\n');
    }

    // ---- รายชื่อทั้งหมด ----
    out.push_str(&format!("## 3. crate ทั้งหมด ({} ตัว)\n\n", deps.len()));
    out.push_str("| crate | รุ่น | ใบอนุญาต |\n|---|---|---|\n");
    for dep in deps {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            dep.name, dep.version, dep.license
        ));
    }
    out.push('\n');

    // ---- ตัวบท ----
    out.push_str(&format!("## 4. ตัวบทใบอนุญาต ({} ฉบับ)\n\n", texts.len()));
    for (index, (text, users)) in texts.iter().enumerate() {
        out.push_str(&format!("### 4.{}\n\n", index + 1));
        out.push_str("<details><summary>");
        out.push_str(&format!("ใช้โดย {} crate: ", users.len()));
        out.push_str(&users.join(" · "));
        out.push_str("</summary>\n\n```text\n");
        out.push_str(text);
        out.push_str("\n```\n\n</details>\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// ★ รูปที่ `cargo tree` พ่นออกมาจริงทั้งสามแบบ — รวมกิ่งที่ถูกยุบ
    #[test]
    fn every_shape_cargo_tree_prints_is_read_correctly() {
        let plain = parse_line("ahash v0.8.12|MIT OR Apache-2.0").unwrap();
        assert_eq!(plain.name, "ahash");
        assert_eq!(plain.version, "0.8.12");
        assert_eq!(plain.license, "MIT OR Apache-2.0");

        // กิ่งที่ cargo ยุบเพราะซ้ำ ต้องเป็น crate ตัวเดียวกันเป๊ะ ไม่งั้นจะนับสองครั้ง
        assert_eq!(
            parse_line("ahash v0.8.12|MIT OR Apache-2.0 (*)"),
            Some(plain)
        );

        // ★★ path dependency = ของเราเอง — หลุดเข้าไปเมื่อไหร่ ไฟล์จะประกาศว่า
        //    เราเป็นบุคคลที่สามของตัวเอง
        assert!(parse_line("refx-core v0.1.0 (E:\\ref 10.0\\crates\\refx-core)|MIT").is_none());

        // ชื่อที่มี `-` และรุ่นที่มี pre-release ต้องไม่ทำให้แยกผิด
        let dashed = parse_line("smithay-client-toolkit v0.19.2|MIT").unwrap();
        assert_eq!(dashed.name, "smithay-client-toolkit");
        assert_eq!(dashed.version, "0.19.2");
        assert_eq!(dashed.dir_name(), "smithay-client-toolkit-0.19.2");
    }

    /// ตัวบทคือไฟล์ไหน — ★ เอาผิดตัวแปลว่าประกาศ copyright ผิดคน
    #[test]
    fn license_files_are_told_apart_from_everything_else_in_the_crate() {
        for yes in [
            "LICENSE",
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "licence.md",
            "COPYING",
            "NOTICE",
            "UNLICENSE",
            "COPYRIGHT",
        ] {
            assert!(is_license_file(Path::new(yes)), "{yes} ควรถูกนับเป็นตัวบท");
        }
        for no in [
            "src",
            "Cargo.toml",
            "README.md",
            "license.rs",
            "LICENSE.spdx",
        ] {
            assert!(!is_license_file(Path::new(no)), "{no} ไม่ใช่ตัวบท");
        }
    }

    /// ★★★ crate เดียวกันที่ถูกรายงานคนละใบอนุญาต **ต้องเหลือแถวเดียว**
    ///
    /// นี่คืออาการที่ทำให้ไฟล์บน CI ต่างจากไฟล์บนเครื่องพัฒนา 14 ก.ย. 2026 —
    /// `egui`/`emath`/`epaint` โผล่อย่างละสองครั้ง เพราะยุบตาม *ทุกฟิลด์*
    #[test]
    fn the_same_crate_reported_twice_collapses_to_one_row() {
        let dep = |license: &str| Dep {
            name: "egui".to_owned(),
            version: "0.34.3".to_owned(),
            license: license.to_owned(),
        };
        let set = BTreeSet::from([dep("MIT OR Apache-2.0"), dep("Apache-2.0 OR MIT")]);
        let out = dedup_by_identity(set);
        assert_eq!(out.len(), 1, "crate เดียวกันยังเป็นสองแถว: {out:?}");
        // ★ ต้องเลือกแบบคงที่ ไม่ใช่แล้วแต่ลำดับที่เข้ามา
        assert_eq!(out[0].license, "Apache-2.0 OR MIT");

        // crate คนละตัว/คนละรุ่น ต้องไม่ถูกยุบรวมกัน
        let mut many = BTreeSet::new();
        many.insert(dep("MIT"));
        many.insert(Dep {
            name: "egui".to_owned(),
            version: "0.35.0".to_owned(),
            license: "MIT".to_owned(),
        });
        many.insert(Dep {
            name: "emath".to_owned(),
            version: "0.34.3".to_owned(),
            license: "MIT".to_owned(),
        });
        assert_eq!(dedup_by_identity(many).len(), 3);
    }

    /// ★★★ NC: crate ที่หาตัวบทไม่เจอ **และไม่ได้ขึ้นทะเบียน** ต้องถูกชี้ชื่อ
    ///
    /// นี่คือประตูที่ทำให้ "ไฟล์ต่างกันระหว่างเครื่อง" กลายเป็นข้อความที่บอกว่า
    /// **ตัวไหนหาย** แทนที่จะเป็นแค่ "ไม่ตรง" ซึ่งไม่มีใครตามต่อได้
    #[test]
    fn a_crate_whose_licence_text_went_missing_is_named_not_just_counted() {
        // ตัวที่ขึ้นทะเบียนแล้วต้องเงียบ — ประตูที่ร้องตอนปกติคือประตูที่ถูกปิดเสียง
        let known = vec!["egui 0.34.3".to_owned(), "zune-jpeg 0.4.21".to_owned()];
        assert!(unregistered_missing(&known).is_empty());

        // ตัวที่ไม่ได้ขึ้นทะเบียน = แดง พร้อมชื่อและรุ่น
        let mut mixed = known.clone();
        mixed.push("serde 1.0.0".to_owned());
        let caught = unregistered_missing(&mixed);
        assert_eq!(caught, vec!["serde 1.0.0".to_owned()], "จับไม่ได้หรือจับเกิน");

        // ★ ทะเบียนจับคู่ด้วย **ชื่อ** — รุ่นใหม่ของตัวเดิมต้องไม่ทำให้แดง
        let bumped = vec!["egui 0.35.0".to_owned()];
        assert!(
            unregistered_missing(&bumped).is_empty(),
            "รุ่นขยับแล้วทะเบียนใช้ไม่ได้ = ทะเบียนที่ต้องแก้ทุกสัปดาห์"
        );
    }

    /// ★★ ประตูต้องบอกได้ว่า **ต่างกันตรงไหน** ไม่ใช่แค่ "ไม่ตรง"
    ///
    /// บน CI ไม่มีใครเข้าไปเปิดไฟล์ดูเองได้ · ข้อความที่บอกแค่ว่าไม่ผ่าน
    /// แปลว่าไม่มีใครรู้ว่าเกิดอะไรขึ้น
    #[test]
    fn the_gate_says_which_line_disagrees_not_just_that_it_does() {
        let found = "หนึ่ง\nสอง\nสาม\n";
        let want = "หนึ่ง\nสองครึ่ง\nสาม\n";
        let report = difference(found, want);
        assert!(report.contains("#2"), "ไม่ได้บอกเลขบรรทัด: {report}");
        assert!(report.contains("สองครึ่ง"), "ไม่ได้บอกว่าควรเป็นอะไร: {report}");
        assert!(report.contains("1"), "ไม่ได้นับจำนวนบรรทัดที่ต่าง: {report}");

        // ความยาวต่างกันล้วน ๆ ต้องบอกได้เหมือนกัน
        let cut = difference("หนึ่ง\n", "หนึ่ง\nสอง\n");
        assert!(cut.contains("ความยาว"), "ไม่ได้บอกว่าต่างที่ความยาว: {cut}");

        // ★ บรรทัดยาวมากต้องไม่ท่วม log — ตัวบทใบอนุญาตยาวเป็นพันตัวอักษร
        let long = "ก".repeat(500);
        assert!(
            trim_for_log(&long).chars().count() < 220,
            "บรรทัดยาวไม่ถูกตัดก่อนพิมพ์"
        );
        assert_eq!(trim_for_log("สั้น"), "สั้น");
    }

    /// ★★★ ไฟล์ต้องเปลี่ยนเมื่อ `Cargo.lock` เปลี่ยน — นั่นคือทั้งหมดของประตูนี้
    ///
    /// ถ้าลายนิ้วมือไม่ได้อยู่ในเนื้อไฟล์ `--check` จะเขียวตลอดกาลหลัง lock ขยับ
    #[test]
    fn the_document_changes_when_the_lockfile_does() {
        let deps = vec![Dep {
            name: "serde".to_owned(),
            version: "1.0.0".to_owned(),
            license: "MIT OR Apache-2.0".to_owned(),
        }];
        let texts = BTreeMap::from([("ตัวบทสมมติ".to_owned(), vec!["serde 1.0.0".to_owned()])]);

        let before = render(&deps, &texts, 0x1111_1111);
        let after = render(&deps, &texts, 0x2222_2222);
        assert_ne!(before, after, "lock เปลี่ยนแล้วไฟล์ไม่เปลี่ยน — ประตูตายสนิท");
        assert!(before.contains("0x11111111"), "ไม่มีลายนิ้วมือของ lock ในไฟล์");
        assert!(before.contains("serde"), "ไม่มีชื่อ crate ในรายการ");
        assert!(before.contains("ตัวบทสมมติ"), "ไม่มีตัวบทในไฟล์");

        // และของที่ฝังลงไบนารีต้องอยู่ในไฟล์ทุกฉบับ ไม่ว่า crate จะเป็นอะไร
        assert!(
            before.contains("NotoSansThai-Regular.ttf"),
            "ฟอนต์ที่ฝังหายไปจากไฟล์ใบอนุญาต"
        );
    }

    /// ★★★ NC ของประตูที่สอง: ฟอนต์ตัวที่สองที่ถูกฝังโดยไม่มีใบอนุญาต **ต้องแดง**
    ///
    /// `EMBEDDED` เป็นรายการที่เขียนด้วยมือ — และรายการที่เขียนด้วยมือล้าสมัย
    /// เงียบ ๆ เสมอ · ประตูนี้คือเหตุผลเดียวที่ยอมให้มันเขียนด้วยมือได้
    #[test]
    fn a_newly_embedded_file_without_a_license_turns_the_gate_red() {
        let dir = std::env::temp_dir().join(format!("refx-lic-nc-{}", std::process::id()));
        let src = dir.join("crates").join("refx-ui").join("src");
        std::fs::create_dir_all(&src).unwrap();

        // ของที่มีใบอนุญาตกำกับอยู่แล้ว — ต้องเงียบ
        std::fs::write(
            src.join("fonts.rs"),
            "const THAI: &[u8] = include_bytes!(\"../../../assets/fonts/NotoSansThai-Regular.ttf\");\n",
        )
        .unwrap();
        assert!(
            check_embedded(&dir).is_ok(),
            "ประตูร้องตอนที่ทุกอย่างถูกต้อง = ประตูที่จะถูกปิดเสียง"
        );

        // ฟอนต์ตัวที่สองที่ไม่มีใครใส่ใบอนุญาตให้
        std::fs::write(
            src.join("emoji.rs"),
            "const EMOJI: &[u8] = include_bytes!(\"../../../assets/fonts/NotoEmoji.ttf\");\n",
        )
        .unwrap();
        let err = check_embedded(&dir).unwrap_err().to_string();
        assert!(
            err.contains("NotoEmoji.ttf") && err.contains("emoji.rs"),
            "แดงแล้วแต่ไม่บอกว่าไฟล์ไหน: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        println!("NC วิ่งผ่านจริง: ประตูชี้ชื่อไฟล์ที่ถูกฝังโดยไม่มีใบอนุญาต");
    }

    /// ★★★ copyleft ที่โผล่เข้ามาต้อง **ขึ้นมาอยู่ในสายตา** ไม่ใช่จมอยู่ในตาราง 300 แถว
    ///
    /// เงื่อนไขของ MPL/GPL ผูกกับ *วิธีใช้* ไม่ใช่แค่การประกาศชื่อ — การรู้ตัว
    /// ตอนมีคนมาถามคือสายไปแล้ว
    #[test]
    fn a_copyleft_dependency_cannot_hide_in_the_long_table() {
        let texts = BTreeMap::new();
        let permissive = Dep {
            name: "serde".to_owned(),
            version: "1.0.0".to_owned(),
            license: "MIT OR Apache-2.0".to_owned(),
        };
        // ★ ประตูของประตู: ของที่ปล่อยเสรีล้วน ต้องไม่ทำให้หัวข้อนี้มีของ
        let clean = render(std::slice::from_ref(&permissive), &texts, 1);
        assert!(clean.contains("### 2ก. ใบอนุญาตที่มีเงื่อนไขผูกกับวิธีใช้ (0 ตัว)"));

        let deps = vec![
            permissive,
            Dep {
                name: "option-ext".to_owned(),
                version: "0.2.0".to_owned(),
                license: "MPL-2.0".to_owned(),
            },
            Dep {
                name: "self_cell".to_owned(),
                version: "1.3.0".to_owned(),
                license: "Apache-2.0 OR GPL-2.0-only".to_owned(),
            },
        ];
        let doc = render(&deps, &texts, 1);
        assert!(doc.contains("(2 ตัว)"), "ไม่ได้ยกขึ้นมาทั้งสองตัว");
        assert!(doc.contains("option-ext"));
        // ตัวที่ให้เลือกได้ ต้องบอกว่าเราเลือกอะไร ไม่ใช่แค่พิมพ์ชื่อใบอนุญาต
        assert!(
            doc.contains("ทางเลือกที่ไม่ใช่ copyleft"),
            "ไม่ได้ประกาศว่าเลือกทางไหนสำหรับใบอนุญาตแบบ OR"
        );
        assert!(
            doc.contains("ไม่แก้ซอร์สของมัน"),
            "MPL ถูกพิมพ์ชื่อแต่ไม่ได้บอกว่าเราปฏิบัติตามเงื่อนไขยังไง"
        );
    }
}
