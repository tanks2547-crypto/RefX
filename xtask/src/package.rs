//! สร้างแพ็กเกจที่แจกได้ — **portable zip ของ Windows**
//!
//! ## ★★★ "portable" = ไม่ต้องติดตั้ง **ไม่ใช่** ย้ายที่เก็บข้อมูล
//!
//! zip ตัวนี้ยังใช้ config/data/cache ของ OS เหมือนกับที่ติดตั้งปกติทุกประการ ·
//! การเก็บข้อมูลข้าง `.exe` คือ **ฟีเจอร์** ที่แตะทุกจุดที่ตัดสิน path
//! ไม่ใช่รูปแบบการแพ็ก (`ROADMAP` ก้อน d) → `README.txt` ในซิปจึงต้องบอกให้ชัด
//! ว่าข้อมูลอยู่ที่ไหน ไม่ใช่ปล่อยให้ผู้ใช้เดาว่ามันพกไปกับโฟลเดอร์
//!
//! ## ★★ ประตูสองบานที่ห้ามขาด
//!
//! 1. **แตกแพ็กเกจออกมาแล้วต้องเจอไฟล์ใบอนุญาต** — และต้องตรวจ **จากตัวแพ็กเกจ**
//!    ไม่ใช่จากโฟลเดอร์ staging ที่เราเพิ่งเขียนเอง · โฟลเดอร์ staging ตอบได้แค่ว่า
//!    *เราตั้งใจใส่* ส่วนคำถามจริงคือ *มันเข้าไปอยู่ในไฟล์ที่ผู้ใช้ได้รับหรือเปล่า*
//! 2. **เวอร์ชันตรงกันสามที่** — `Cargo.toml` · `--version` ของไบนารีที่แตกออกมา
//!    · ชื่อแพ็กเกจ · สองที่ไม่พอ: ไบนารีที่หลงรุ่นเข้ามาในซิปที่ชื่อถูกต้อง
//!    คือสิ่งที่ผู้ใช้แยกไม่ออกเลยจนกว่าจะเจอบั๊กที่แก้ไปแล้ว
//!
//! ## ใช้
//!
//! ```text
//! cargo xtask package
//! ```

// ★ เหตุผลเดียวกับ `mutation.rs`/`licenses.rs` — เครื่องมือ dev อ่านไฟล์ของ
//   โปรเจกต์เอง ไม่มี UI thread ให้บล็อก
#![expect(
    clippy::disallowed_methods,
    reason = "เครื่องมือ dev ประกอบไฟล์แพ็กเกจ ไม่ใช่ดิสก์ I/O บนลูปเฟรม"
)]

use std::path::{Path, PathBuf};

/// ไฟล์ที่ **ต้อง** อยู่ในทุกแพ็กเกจ — `ROADMAP` ก้อน d
///
/// `(ที่มาในทรี, ชื่อในแพ็กเกจ)` · ★ `THIRD-PARTY-LICENSES.md` กับ `OFL.txt`
/// เป็นเงื่อนไขทางกฎหมายของการแจกจ่าย ไม่ใช่ของแถม
const REQUIRED: &[(&str, &str)] = &[
    ("THIRD-PARTY-LICENSES.md", "THIRD-PARTY-LICENSES.md"),
    ("assets/fonts/OFL.txt", "OFL.txt"),
    ("LICENSE-MIT", "LICENSE-MIT"),
    ("LICENSE-APACHE", "LICENSE-APACHE"),
];

/// โฟลเดอร์ผลลัพธ์ — ของจริงกับของ NC **ห้ามอยู่ที่เดียวกัน**
///
/// `.github/workflows/package.yml` อัปโหลดเฉพาะ `target/package/` ดังนั้นการ
/// แยกที่อยู่คือสิ่งที่ทำให้ของปลอมหลุดออกไปไม่ได้เลย ไม่ใช่แค่ "ไม่น่าหลุด"
fn nc_out_dir(nc: &str) -> &'static str {
    if nc.is_empty() {
        "package"
    } else {
        "package-nc"
    }
}

/// สร้างแพ็กเกจแล้วตรวจมันจากตัวไฟล์ที่ได้
///
/// # Errors
/// เมื่อ build ไม่ผ่าน · ไฟล์ที่ต้องมีหายไปจากแพ็กเกจ · หรือเวอร์ชันสามที่ไม่ตรงกัน
pub fn run() -> anyhow::Result<()> {
    anyhow::ensure!(
        cfg!(windows),
        "รอบนี้ทำเฉพาะแพ็กเกจของ Windows — AppImage/deb ต้องสร้างบน Linux (ทำใน CI)"
    );

    let root = root()?;
    let declared = declared_version(&root)?;
    // ★★★ negative control (`docs/08 §3.9` ข้อ 1) — ประตูที่ไม่เคยเห็นสีแดง
    //   ไม่ใช่ประตู · สองธงนี้จำลองความผิดพลาดที่เกิดได้จริงตอนปล่อยของ:
    //   ลืมใส่ไฟล์ใบอนุญาต · กับซิปที่ชื่อรุ่นหนึ่งแต่ข้างในเป็นอีกรุ่น
    let nc = std::env::var("REFX_PKG_NC").unwrap_or_default();
    let stem = match nc.as_str() {
        "stale-binary" => "refx-9.9.9-windows-x86_64".to_owned(),
        _ => format!("refx-{declared}-windows-x86_64"),
    };
    if !nc.is_empty() {
        println!("★ NC เปิดอยู่: {nc} — ประตูต้องล้ม");
    }
    println!("เวอร์ชันที่ประกาศใน Cargo.toml: {declared}");

    // ★ default features เท่านั้น — `force-device-lost` เป็นนั่งร้านของเทสต์
    //   ไม่ใช่ของที่ผู้ใช้ควรได้รับ (ต่างจากประตูขนาดไบนารีที่วัด --all-features)
    let built = std::process::Command::new("cargo")
        .args(["build", "--release", "--locked", "-p", "refx-app"])
        .current_dir(&root)
        .status()?;
    anyhow::ensure!(built.success(), "build ไม่ผ่าน — ไม่แพ็กของที่คอมไพล์ไม่ได้");

    // ★★★ NC ต้องเขียนลง **คนละโฟลเดอร์** กับของจริง (`docs/08 §3.9` ข้อ 19)
    //
    //   เดิมทั้งคู่ลง `target/package/` โดยอาศัยว่า NC "ทับชื่อเดิม" — จริงกับ
    //   `no-license` เท่านั้น · `stale-binary` เปลี่ยน **ชื่อไฟล์** เป็น 9.9.9
    //   จึงไม่ทับใคร แล้ว `upload-artifact` ที่เก็บ `target/package/*.zip`
    //   ก็หอบมันไปด้วย · artifact ที่ CI แจกจึงมีซิปสองอัน อันหนึ่งเป็นของปลอม
    //   ที่ตกประตูของตัวเอง (เจอจริงตอนโหลดของจากรอบ 34771844462 มาใช้ 14 ก.ย. 2026)
    //
    //   แยกโฟลเดอร์แล้วลำดับของขั้นใน workflow ไม่สำคัญอีกต่อไป และไม่มีทาง
    //   ที่ของปลอมจะหลุดเข้าไปในสิ่งที่ถูกอัปโหลด ไม่ว่าใครจะสลับขั้นทีหลัง
    let out_dir = root.join("target").join(nc_out_dir(&nc));
    let stage = out_dir.join(&stem);
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage)?;

    let exe = root.join("target").join("release").join("refx.exe");
    anyhow::ensure!(exe.is_file(), "ไม่เจอ {} หลัง build", exe.display());
    std::fs::copy(&exe, stage.join("refx.exe"))?;

    for (from, to) in REQUIRED {
        // NC: "ลืมใส่ใบอนุญาต" — ความผิดพลาดที่เงียบที่สุดในการปล่อยของ
        if nc == "no-license" && *to == "THIRD-PARTY-LICENSES.md" {
            println!("★ NC: จงใจไม่ใส่ {to}");
            continue;
        }
        let source = root.join(from);
        anyhow::ensure!(
            source.is_file(),
            "ไฟล์ที่ต้องแจกไปด้วยหายไปจากทรี: {}",
            source.display()
        );
        std::fs::copy(&source, stage.join(to))?;
    }
    std::fs::write(stage.join("README.txt"), readme(&declared))?;

    // ---- ซิป ----
    let archive = out_dir.join(format!("{stem}.zip"));
    let _ = std::fs::remove_file(&archive);
    zip_dir(&stage, &archive)?;
    let size = std::fs::metadata(&archive)?.len();
    println!("สร้าง {} ({size} ไบต์)", archive.display());

    verify(&out_dir, &archive, &stem, &declared)?;
    println!("\nแพ็กเกจผ่านประตูทั้งสองบาน");
    Ok(())
}

/// ไฟล์ใบอนุญาตอยู่ที่ไหนใน `.deb` (ตามมาตรฐาน Debian)
const DEB_DOC_DIR: &str = "usr/share/doc/refx";

/// ★★★ ตรวจแพ็กเกจ `.deb` — **จากตัวไฟล์ `.deb` ไม่ใช่จากโฟลเดอร์ build**
///
/// สร้างบน Linux (CI) ด้วย `cargo deb` · ตัวนี้แตกมันออกมาแล้วถามคำถามเดียวกับ
/// ที่ถาม zip ทุกประการ: ใบอนุญาตอยู่ครบไหม · เวอร์ชันตรงกันสามที่ไหม
///
/// # Errors
/// เมื่อแตกไฟล์ไม่ได้ · ไฟล์ที่ต้องมีหายไป · หรือเวอร์ชันไม่ตรง
pub fn verify_deb() -> anyhow::Result<()> {
    let path: PathBuf = std::env::args()
        .nth(2)
        .ok_or_else(|| anyhow::anyhow!("ใช้: cargo xtask verify-deb <ไฟล์.deb>"))?
        .into();
    anyhow::ensure!(path.is_file(), "ไม่เจอไฟล์ {}", path.display());

    let root = root()?;
    let declared = declared_version(&root)?;
    let nc = std::env::var("REFX_PKG_NC").unwrap_or_default();
    if !nc.is_empty() {
        println!("★ NC เปิดอยู่: {nc} — ประตูต้องล้ม");
    }

    let unpacked = root.join("target").join("package").join("verify-deb");
    let _ = std::fs::remove_dir_all(&unpacked);
    std::fs::create_dir_all(&unpacked)?;
    let out = std::process::Command::new("dpkg-deb")
        .arg("-x")
        .arg(&path)
        .arg(&unpacked)
        .output()?;
    anyhow::ensure!(
        out.status.success(),
        "แตก {} ไม่ได้:\n{}",
        path.display(),
        String::from_utf8_lossy(&out.stderr)
    );

    println!("\n— ประตู 1: ไฟล์ใบอนุญาตอยู่ในแพ็กเกจจริงไหม —");
    for (_, name) in REQUIRED {
        let file = unpacked.join(DEB_DOC_DIR).join(name);
        // NC: ลบไฟล์ออกจากสิ่งที่แตกมา = จำลอง "แพ็กเกจที่ลืมใส่ใบอนุญาต"
        if nc == "no-license" && *name == "THIRD-PARTY-LICENSES.md" {
            let _ = std::fs::remove_file(&file);
            println!("★ NC: ลบ {name} ออกจากสิ่งที่แตกมา");
        }
        let bytes = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        println!("  {name:<26} {bytes} ไบต์");
        anyhow::ensure!(
            bytes > 0,
            "★ {name} ไม่อยู่ใน .deb (หรือว่างเปล่า) — เราแจกโค้ดของคนอื่น\n\
             โดยไม่มีใบอนุญาตของเขาไปด้วย"
        );
    }
    let desktop = unpacked.join("usr/share/applications/refx.desktop");
    anyhow::ensure!(
        desktop.is_file(),
        "★ ไม่มี refx.desktop — ผู้ใช้จะไม่เห็นโปรแกรมในเมนูเลย"
    );
    // ★ `Icon=refx` ใน .desktop ชี้ไปที่ **ชื่อ** ไม่ใช่เส้นทาง · ไฟล์ผิดชื่อ
    //   หรือผิดโฟลเดอร์ = เมนูแสดงกล่องเปล่า ซึ่งไม่มีอะไรในระบบ build บอกเลย
    let png = unpacked.join("usr/share/icons/hicolor/256x256/apps/refx.png");
    anyhow::ensure!(
        png.is_file(),
        "★ ไม่มี usr/share/icons/hicolor/256x256/apps/refx.png — \
         `Icon=refx` ใน .desktop จะหาไม่เจอแล้วเมนูขึ้นเป็นกล่องเปล่า"
    );

    println!("\n— ประตู 2: เวอร์ชันตรงกันสามที่ไหม —");
    let exe = unpacked.join("usr/bin/refx");
    anyhow::ensure!(exe.is_file(), "ไม่มี usr/bin/refx ใน .deb");
    let printed = std::process::Command::new(&exe).arg("--version").output()?;
    let printed = String::from_utf8_lossy(&printed.stdout).trim().to_owned();
    let from_binary = printed
        .strip_prefix("refx ")
        .ok_or_else(|| anyhow::anyhow!("`refx --version` ตอบรูปที่อ่านไม่ออก: {printed:?}"))?
        .to_owned();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let from_name = version_in_deb_name(&name)
        .ok_or_else(|| anyhow::anyhow!("อ่านเวอร์ชันจากชื่อ {name:?} ไม่ได้"))?;

    println!("  Cargo.toml     {declared}");
    println!("  ไบนารีใน .deb  {from_binary}");
    println!("  ชื่อแพ็กเกจ     {from_name}");
    anyhow::ensure!(
        agree(&declared, &from_binary, from_name),
        "★★★ เวอร์ชันไม่ตรงกัน — Cargo.toml={declared} ไบนารี={from_binary} ชื่อ={from_name}"
    );
    println!("\n— ประตู 3: การเสริมความแข็งแรงต้องอยู่ **ในไฟล์ที่แจก** —");
    hardened(&exe)?;

    println!("\n.deb ผ่านประตูทั้งสามบาน");
    Ok(())
}

/// ★★★ ตรวจ PIE + BIND_NOW จาก **ตัว binary ที่แตกออกมาจาก .deb**
///
/// ## ทำไมต้องตรวจไฟล์ ไม่ใช่เชื่อธงใน `.cargo/config.toml`
///
/// เพราะธงหายไปได้โดยไม่มีใครรู้ · และเราเพิ่งเสียเวลาไปทั้งวันกับตรงกันข้าม:
/// ธง `-C relocation-model=pie` ที่ **มีอยู่** แต่ทำให้ build ทั้งทรีพัง แล้ว
/// ต้องถอดออก · ถ้าความมั่นใจของเราผูกกับ "มีธงอยู่ในไฟล์ config ไหม"
/// การถอดมันออกจะเป็นการลดความแข็งแรงแบบเงียบ ๆ ทันที
///
/// PIE เป็นค่าเริ่มต้นของ target `x86_64-unknown-linux-gnu` อยู่แล้ว ประตูนี้
/// จึงไม่ได้ตรวจธง แต่ตรวจ **ผลลัพธ์** ซึ่งเป็นสิ่งเดียวที่ผู้ใช้ได้รับจริง
///
/// `readelf` ที่หายไป = แดง ไม่ใช่ข้าม (`docs/08 §3.9` ข้อ 11 — ประตูที่ข้าม
/// เงียบ ๆ คือประตูที่ไม่มีอยู่)
fn hardened(exe: &Path) -> anyhow::Result<()> {
    let header = std::process::Command::new("readelf")
        .arg("-h")
        .arg(exe)
        .output();
    let header = header.map_err(|err| {
        anyhow::anyhow!("เรียก `readelf` ไม่ได้ ({err}) — ประตูนี้ห้ามข้าม ติดตั้ง binutils ก่อน")
    })?;
    anyhow::ensure!(
        header.status.success(),
        "`readelf -h` ล้มกับ {}",
        exe.display()
    );
    let dynamic = std::process::Command::new("readelf")
        .arg("-d")
        .arg(exe)
        .output()?;
    anyhow::ensure!(
        dynamic.status.success(),
        "`readelf -d` ล้มกับ {}",
        exe.display()
    );

    judge(
        &String::from_utf8_lossy(&header.stdout),
        &String::from_utf8_lossy(&dynamic.stdout),
    )
    .map_err(|err| anyhow::anyhow!("{} — {err}", exe.display()))
}

/// ตัดสินจากข้อความของ `readelf` — แยกออกมาเพื่อให้ **เทสต์ได้โดยไม่ต้องมี ELF จริง**
fn judge(header: &str, dynamic: &str) -> anyhow::Result<()> {
    let kind = header
        .lines()
        .find_map(|line| line.trim().strip_prefix("Type:"))
        .unwrap_or("")
        .trim()
        .to_owned();
    println!("  ELF Type   {kind}");
    anyhow::ensure!(
        kind.starts_with("DYN"),
        "ไม่ใช่ PIE (Type = {kind}) — ASLR ใช้กับตัวโปรแกรมไม่ได้"
    );

    let now = dynamic
        .lines()
        .any(|line| line.contains("BIND_NOW") || line.contains("FLAGS") && line.contains("NOW"));
    println!("  BIND_NOW   {}", if now { "มี" } else { "ไม่มี" });
    anyhow::ensure!(
        now,
        "ไม่มี BIND_NOW — GOT เขียนได้ตลอดอายุโปรเซส · ธง \
         `-C link-arg=-Wl,-z,relro,-z,now` ใน .cargo/config.toml หายไปหรือเปล่า"
    );
    Ok(())
}

/// ★★★ ตรวจ MSI — **ติดตั้งจริง แล้วถอนจริง**
///
/// ## ทำไมไม่แค่แตกไฟล์ออกมาดู
///
/// MSI มีสิ่งที่ zip/deb ไม่มี: **ตัวถอนการติดตั้ง** · และกับดักที่ `ROADMAP`
/// ก้อน d ชี้ไว้คือ **ถอนแล้วต้องไม่ลบงานของผู้ใช้** — `I-3` ใช้กับตัวถอนติดตั้ง
/// ด้วย · การแตกไฟล์ดูเฉย ๆ ตอบคำถามนั้นไม่ได้เลย
///
/// ลำดับ: ติดตั้ง → ตรวจไฟล์ + เวอร์ชันจากไบนารีที่ติดตั้งแล้ว → **สร้างไฟล์ของ
/// ผู้ใช้ปลอม** → ถอน → ยืนยันว่าไฟล์ผู้ใช้ยังอยู่ และไฟล์โปรแกรมหายไปแล้ว
///
/// # Errors
/// เมื่อติดตั้ง/ถอนไม่สำเร็จ · ไฟล์หาย · เวอร์ชันไม่ตรง · หรือ **งานผู้ใช้ถูกลบ**
pub fn verify_msi() -> anyhow::Result<()> {
    anyhow::ensure!(cfg!(windows), "MSI ตรวจได้บน Windows เท่านั้น");
    let msi: PathBuf = std::env::args()
        .nth(2)
        .ok_or_else(|| anyhow::anyhow!("ใช้: cargo xtask verify-msi <ไฟล์.msi>"))?
        .into();
    anyhow::ensure!(msi.is_file(), "ไม่เจอไฟล์ {}", msi.display());
    // ★★★ `msiexec` รับ **เส้นทางเต็มเท่านั้น** — เส้นทางสัมพัทธ์ให้ error 1619
    //   ("This installation package could not be opened") ซึ่งอ่านเหมือนไฟล์เสีย
    //   ทั้งที่ไฟล์ดีทุกอย่าง · เสีย CI ไปหนึ่งรอบกับข้อความที่ชี้ผิดทาง
    let msi = std::fs::canonicalize(&msi)?;
    // `canonicalize` บน Windows ให้ `\\?\C:\...` ซึ่ง msiexec ไม่ชอบ — ตัดคำนำหน้าออก
    let msi = PathBuf::from(
        msi.to_string_lossy()
            .strip_prefix(r"\\?\")
            .map_or_else(|| msi.to_string_lossy().into_owned(), str::to_owned),
    );
    println!("MSI: {}", msi.display());

    let root = root()?;
    let declared = declared_version(&root)?;
    let nc = std::env::var("REFX_PKG_NC").unwrap_or_default();
    if !nc.is_empty() {
        println!("★ NC เปิดอยู่: {nc} — ประตูต้องล้ม");
    }

    msiexec(&["/i", &msi.to_string_lossy(), "/qn"], "ติดตั้ง")?;

    let installed =
        PathBuf::from(std::env::var("ProgramFiles").unwrap_or_default()).join("refx-app");
    let result = check_installed(&installed, &declared);

    // ---- ★★★ กับดักของ MSI: ถอนแล้วงานผู้ใช้ต้องอยู่ครบ (I-3) ----
    let marks = user_data_marks();
    for mark in &marks {
        if let Some(parent) = mark.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(mark, b"work the user made before uninstalling\n")?;
    }
    println!("\nสร้างไฟล์ของผู้ใช้ไว้ {} จุด แล้วถอนการติดตั้ง", marks.len());

    msiexec(&["/x", &msi.to_string_lossy(), "/qn"], "ถอน")?;
    result?; // ★ รายงานผลการตรวจไฟล์หลังถอนเสร็จ — ไม่ทิ้งเครื่องไว้ในสภาพติดตั้งค้าง

    println!("\n— ประตู 3: ถอนแล้วงานของผู้ใช้ยังอยู่ไหม (I-3) —");
    for mark in &marks {
        // NC: ลบเอง = จำลองตัวถอนติดตั้งที่กินงานผู้ใช้
        if nc == "uninstall-eats-data" {
            let _ = std::fs::remove_file(mark);
            println!("★ NC: ลบ {} เอง", mark.display());
        }
        let alive = mark.is_file();
        println!(
            "  {:<64} {}",
            mark.display().to_string(),
            if alive {
                "อยู่"
            } else {
                "หายไป"
            }
        );
        anyhow::ensure!(
            alive,
            "★★★ ถอนการติดตั้งแล้ว **งานของผู้ใช้หายไป**: {}\n\
             ผู้ใช้ที่ถอนโปรแกรมไม่ได้ขอให้ลบกระดานของเขา — นี่คือ I-3 ข้อเดียวกับ\n\
             ที่ใช้กับ crash และ autosave ทุกประการ",
            mark.display()
        );
        let _ = std::fs::remove_file(mark);
    }

    anyhow::ensure!(
        !installed.join("bin").join("refx.exe").is_file(),
        "★ ถอนแล้วแต่ {} ยังอยู่ — ตัวถอนติดตั้งทำงานไม่ครบ",
        installed.display()
    );
    println!("\nMSI ผ่านประตูทั้งสามบาน");
    Ok(())
}

/// ไฟล์ตัวแทน "งานของผู้ใช้" ที่การถอนติดตั้งห้ามแตะ
///
/// ★ สามที่นี้คือที่ที่ `AppPaths` วางของจริง — settings ที่เขาแก้เอง ·
/// งานที่ยังไม่เคยบันทึก (recovery) · และภาพที่วางจาก clipboard
fn user_data_marks() -> Vec<PathBuf> {
    let roaming = PathBuf::from(std::env::var("APPDATA").unwrap_or_default());
    let local = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default());
    vec![
        roaming
            .join("RefX")
            .join("config")
            .join("uninstall-probe.toml"),
        local.join("RefX").join("data").join("uninstall-probe.refx"),
        local
            .join("RefX")
            .join("data")
            .join("recovery")
            .join("uninstall-probe.refx"),
    ]
}

/// ตรวจไฟล์ที่ติดตั้งแล้ว + เวอร์ชันของไบนารีที่ติดตั้งจริง
fn check_installed(installed: &Path, declared: &str) -> anyhow::Result<()> {
    println!("\n— ประตู 1: ไฟล์ใบอนุญาตถูกติดตั้งไปด้วยไหม —");
    for (_, name) in REQUIRED {
        let file = installed.join(name);
        let bytes = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        println!("  {name:<26} {bytes} ไบต์");
        anyhow::ensure!(
            bytes > 0,
            "★ {name} ไม่ได้ถูกติดตั้ง — เราแจกโค้ดของคนอื่นโดยไม่มีใบอนุญาตของเขา"
        );
    }

    println!("\n— ประตู 2: เวอร์ชันตรงกันไหม —");
    let exe = installed.join("bin").join("refx.exe");
    anyhow::ensure!(exe.is_file(), "ไม่มี {} หลังติดตั้ง", exe.display());
    let printed = std::process::Command::new(&exe).arg("--version").output()?;
    let printed = String::from_utf8_lossy(&printed.stdout).trim().to_owned();
    let from_binary = printed
        .strip_prefix("refx ")
        .ok_or_else(|| anyhow::anyhow!("`refx --version` ตอบรูปที่อ่านไม่ออก: {printed:?}"))?;
    println!("  Cargo.toml       {declared}");
    println!("  ไบนารีที่ติดตั้ง  {from_binary}");
    anyhow::ensure!(
        declared == from_binary,
        "★★★ MSI ติดตั้งไบนารีคนละรุ่นกับที่ประกาศ — {declared} vs {from_binary}"
    );
    Ok(())
}

/// เรียก `msiexec` แล้วบอกให้ชัดว่าขั้นไหนล้ม
fn msiexec(args: &[&str], step: &str) -> anyhow::Result<()> {
    let status = std::process::Command::new("msiexec").args(args).status()?;
    anyhow::ensure!(
        status.success(),
        "msiexec ขั้น '{step}' ล้ม (code {:?}) — args: {args:?}",
        status.code()
    );
    println!("msiexec {step}: สำเร็จ");
    Ok(())
}

/// เวอร์ชันจากชื่อไฟล์ `.deb` — `refx_0.1.0-1_amd64.deb` → `0.1.0`
///
/// ★ Debian เติม `-<revision>` ต่อท้ายเสมอ · ตัดทิ้งก่อนเทียบ ไม่งั้นประตู
/// จะแดงทุกครั้งด้วยเหตุผลที่ไม่ใช่ความผิดของใคร
fn version_in_deb_name(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("refx_")?;
    let (version, _) = rest.split_once('_')?;
    Some(version.split_once('-').map_or(version, |(v, _)| v))
}

/// ★★★ ตรวจ **จากตัวแพ็กเกจ** — แตกออกมาใหม่ในที่ว่าง แล้วถามมันเอง
fn verify(out_dir: &Path, archive: &Path, stem: &str, declared: &str) -> anyhow::Result<()> {
    let unpacked = out_dir.join("verify");
    let _ = std::fs::remove_dir_all(&unpacked);
    std::fs::create_dir_all(&unpacked)?;
    unzip(archive, &unpacked)?;

    println!("\n— ประตู 1: ไฟล์ใบอนุญาตอยู่ในแพ็กเกจจริงไหม —");
    for (_, name) in REQUIRED {
        let path = unpacked.join(name);
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        println!("  {name:<26} {bytes} ไบต์");
        anyhow::ensure!(
            bytes > 0,
            "★ {name} ไม่อยู่ในแพ็กเกจ (หรือว่างเปล่า) — เราแจกโค้ดของคนอื่น\n\
             โดยไม่มีใบอนุญาตของเขาไปด้วย"
        );
    }
    let readme = unpacked.join("README.txt");
    anyhow::ensure!(readme.is_file(), "★ README.txt ไม่อยู่ในแพ็กเกจ");

    println!("\n— ประตู 2: เวอร์ชันตรงกันสามที่ไหม —");
    let exe = unpacked.join("refx.exe");
    anyhow::ensure!(exe.is_file(), "ไม่มี refx.exe ในแพ็กเกจ");
    let printed = std::process::Command::new(&exe).arg("--version").output()?;
    let printed = String::from_utf8_lossy(&printed.stdout).trim().to_owned();
    let from_binary = printed
        .strip_prefix("refx ")
        .ok_or_else(|| anyhow::anyhow!("`refx --version` ตอบรูปที่อ่านไม่ออก: {printed:?}"))?
        .to_owned();
    let from_name =
        version_in_name(stem).ok_or_else(|| anyhow::anyhow!("อ่านเวอร์ชันจากชื่อแพ็กเกจไม่ได้: {stem}"))?;

    println!("  Cargo.toml   {declared}");
    println!("  ไบนารีในซิป  {from_binary}");
    println!("  ชื่อแพ็กเกจ   {from_name}");
    anyhow::ensure!(
        agree(declared, &from_binary, from_name),
        "★★★ เวอร์ชันไม่ตรงกัน — Cargo.toml={declared} ไบนารี={from_binary} ชื่อ={from_name}\n\
         ไบนารีที่หลงรุ่นเข้ามาในซิปที่ชื่อถูกต้อง คือสิ่งที่ผู้ใช้แยกไม่ออกเลย"
    );
    Ok(())
}

/// เวอร์ชันที่ประกาศไว้ใน `[workspace.package]`
fn declared_version(root: &Path) -> anyhow::Result<String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))?;
    version_from_manifest(&manifest)
        .ok_or_else(|| anyhow::anyhow!("หา version ใน [workspace.package] ไม่เจอ"))
}

/// อ่าน `version = "x.y.z"` ตัวแรกที่อยู่ **หลัง** `[workspace.package]`
///
/// ★ ห้ามหยิบ `version` ตัวแรกของไฟล์เฉย ๆ — ตารางของ dependency ก็มีคำนั้น
/// เต็มไปหมด แล้วเราจะได้รุ่นของ crate คนอื่นมาเป็นเวอร์ชันของโปรแกรม
fn version_from_manifest(text: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[workspace.package]";
            continue;
        }
        if inside && let Some(rest) = line.strip_prefix("version") {
            let value = rest.trim_start().strip_prefix('=')?.trim();
            return Some(value.trim_matches('"').to_owned());
        }
    }
    None
}

/// เวอร์ชันที่ฝังอยู่ในชื่อแพ็กเกจ `refx-<version>-windows-x86_64`
fn version_in_name(stem: &str) -> Option<&str> {
    stem.strip_prefix("refx-")?.strip_suffix("-windows-x86_64")
}

/// สามที่พูดตรงกันไหม
fn agree(declared: &str, from_binary: &str, from_name: &str) -> bool {
    declared == from_binary && declared == from_name
}

/// ข้อความที่ผู้ใช้จะอ่านตอนแตกซิป — **อังกฤษ** (`docs/03 §0`)
fn readme(version: &str) -> String {
    format!(
        "RefX {version} - portable build for Windows (x86_64)\n\
         =====================================================\n\
         \n\
         Nothing to install: unzip anywhere and run refx.exe.\n\
         \n\
         WHERE YOUR THINGS ARE KEPT\n\
         --------------------------\n\
         Portable here means \"no installer\". It does NOT mean the program\n\
         keeps its data next to the .exe - it uses the normal Windows\n\
         locations, exactly like an installed copy would:\n\
         \n\
           Settings          %APPDATA%\\RefX\\config\n\
           Unsaved work      %LOCALAPPDATA%\\RefX\\data      (recovery, pasted images)\n\
           Cache             %LOCALAPPDATA%\\RefX\\cache     (thumbnails - safe to delete)\n\
           Logs              %LOCALAPPDATA%\\RefX\\cache\\logs\n\
         \n\
         So moving this folder to another machine does not carry your boards\n\
         with it, and deleting the folder does not delete your settings.\n\
         \n\
         WINDOWS WILL WARN YOU THE FIRST TIME\n\
         ------------------------------------\n\
         This build is not signed with a code-signing certificate, so\n\
         SmartScreen shows \"Windows protected your PC\". That is expected.\n\
         \n\
         LICENSES\n\
         --------\n\
           LICENSE-MIT / LICENSE-APACHE   RefX itself (your choice of either)\n\
           THIRD-PARTY-LICENSES.md        every library RefX is built with\n\
           OFL.txt                        the embedded Noto Sans Thai font\n"
    )
}

/// รากของ workspace
fn root() -> anyhow::Result<PathBuf> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    Ok(here
        .parent()
        .ok_or_else(|| anyhow::anyhow!("หารากของ workspace ไม่เจอ"))?
        .to_path_buf())
}

/// ★ `tar.exe` ของ Windows คือ **bsdtar** ซึ่งเขียน zip จริงได้
///
/// ห้ามใช้ `tar` ของ Git Bash (GNU tar) — มันเขียน **tar** ให้แล้วตั้งชื่อ `.zip`
/// ตามที่สั่ง ผลคือไฟล์ที่นามสกุลโกหก ผู้ใช้ดับเบิลคลิกแล้วเปิดไม่ออก
fn bsdtar() -> PathBuf {
    let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(system).join("System32").join("tar.exe")
}

fn zip_dir(stage: &Path, archive: &Path) -> anyhow::Result<()> {
    let out = std::process::Command::new(bsdtar())
        .arg("-a")
        .arg("-c")
        .arg("-f")
        .arg(archive)
        .arg("-C")
        .arg(stage)
        .arg(".")
        .output()?;
    anyhow::ensure!(
        out.status.success(),
        "สร้าง zip ไม่สำเร็จ:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // ★ ยืนยันว่าได้ **zip จริง** ไม่ใช่ tar ที่ถูกตั้งชื่อ .zip
    let head = std::fs::read(archive)?;
    anyhow::ensure!(
        head.starts_with(b"PK\x03\x04"),
        "ไฟล์ที่ได้ไม่ใช่ zip (ไม่มีลายเซ็น PK) — นามสกุลกำลังโกหก"
    );
    Ok(())
}

fn unzip(archive: &Path, into: &Path) -> anyhow::Result<()> {
    let out = std::process::Command::new(bsdtar())
        .arg("-x")
        .arg("-f")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .output()?;
    anyhow::ensure!(
        out.status.success(),
        "แตก zip ไม่สำเร็จ:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// ★★★ ของปลอมจาก negative control ต้องออกไปกับ artifact ที่แจกไม่ได้
    ///
    /// เกิดจริง: `.github/workflows/package.yml` อัปโหลด `target/package/*.zip`
    /// และ NC `stale-binary` สร้าง `refx-9.9.9-...zip` ไว้ที่เดียวกัน —
    /// artifact ของรอบ 34771844462 จึงมีซิปสองอัน อันหนึ่งเป็นของที่ตกประตู
    /// ของตัวเอง · จับได้ตอนโหลดของจริงมาใช้ ไม่มีเทสต์ไหนเห็นมาก่อน
    #[test]
    fn a_negative_control_package_never_lands_where_the_real_one_is_uploaded_from() {
        assert_eq!(nc_out_dir(""), "package", "ของจริงต้องอยู่ที่ target/package");
        for nc in ["no-license", "stale-binary", "อะไรก็ตามที่ยังไม่มี"]
        {
            assert_ne!(
                nc_out_dir(nc),
                "package",
                "NC {nc:?} เขียนลงโฟลเดอร์ที่ CI อัปโหลด — ของปลอมจะหลุดไปกับของจริง"
            );
        }
    }

    /// ข้อความจริงของ `readelf -h` บน binary ที่ build ตามปกติ (PIE)
    const PIE_HEADER: &str = "ELF Header:\n  Class:  ELF64\n  Type:  DYN (Position-Independent Executable file)\n  Machine: Advanced Micro Devices X86-64\n";
    /// ข้อความจริงของ `readelf -h` บน binary ที่ลิงก์ด้วย `-no-pie`
    const NO_PIE_HEADER: &str = "ELF Header:\n  Class:  ELF64\n  Type:  EXEC (Executable file)\n  Machine: Advanced Micro Devices X86-64\n";
    const RELRO_NOW: &str =
        " 0x000000000000001e (FLAGS)   BIND_NOW\n 0x000000006ffffffb (FLAGS_1) Flags: NOW PIE\n";
    const LAZY: &str = " 0x0000000000000015 (DEBUG)   0x0\n 0x0000000000000003 (PLTGOT)  0x4000\n";

    /// ★ ประตูที่ตัดสินจาก **ไฟล์ที่แจก** ต้องแดงได้จริงทั้งสองแบบที่มันอ้างว่าจับ
    /// — ไม่งั้นมันคือบรรทัด "ผ่าน" ที่ไม่ได้แปลว่าอะไรเลย
    #[test]
    fn the_hardening_gate_goes_red_for_a_plain_executable_and_for_lazy_binding() {
        judge(PIE_HEADER, RELRO_NOW).expect("binary ที่ถูกต้องต้องผ่าน");

        let err = judge(NO_PIE_HEADER, RELRO_NOW).expect_err("EXEC ต้องไม่ผ่าน");
        assert!(format!("{err}").contains("PIE"), "{err}");

        let err = judge(PIE_HEADER, LAZY).expect_err("ไม่มี BIND_NOW ต้องไม่ผ่าน");
        assert!(format!("{err}").contains("BIND_NOW"), "{err}");
    }

    /// ★★★ ต้องอ่านเวอร์ชันของ **โปรแกรม** ไม่ใช่ของ dependency ตัวแรกที่เจอ
    #[test]
    fn the_version_comes_from_workspace_package_not_from_some_dependency() {
        let manifest = "\
[workspace]\n\
members = [\"crates/*\"]\n\
\n\
[workspace.dependencies]\n\
wgpu = \"29.0.4\"\n\
version = \"9.9.9\"\n\
\n\
[workspace.package]\n\
version = \"0.1.0\"\n\
edition = \"2024\"\n";
        assert_eq!(version_from_manifest(manifest).as_deref(), Some("0.1.0"));

        // ไม่มีหัวข้อนั้นเลย = ต้องตอบว่าไม่รู้ ไม่ใช่เดา
        assert_eq!(
            version_from_manifest("[package]\nversion = \"1.2.3\"\n"),
            None
        );
    }

    /// ★★ ประตูเวอร์ชันต้อง **แดงจริง** เมื่อสามที่ไม่ตรงกัน — ทีละทาง
    #[test]
    fn the_version_gate_fails_on_every_way_they_can_disagree() {
        assert!(agree("0.1.0", "0.1.0", "0.1.0"));

        // ไบนารีหลงรุ่นเข้ามาในซิปที่ชื่อถูก — เคสที่ผู้ใช้แยกไม่ออกเลย
        assert!(!agree("0.1.0", "0.0.9", "0.1.0"), "ไบนารีคนละรุ่นแต่ประตูเงียบ");
        // ชื่อแพ็กเกจหลงรุ่น
        assert!(
            !agree("0.1.0", "0.1.0", "0.2.0"),
            "ชื่อแพ็กเกจคนละรุ่นแต่ประตูเงียบ"
        );
        // ทั้งคู่หลง แต่หลงตรงกัน — ยังผิดเพราะไม่ตรงกับ Cargo.toml
        assert!(!agree("0.1.0", "0.2.0", "0.2.0"));
    }

    #[test]
    fn the_version_is_read_back_out_of_the_package_name() {
        assert_eq!(version_in_name("refx-0.1.0-windows-x86_64"), Some("0.1.0"));
        assert_eq!(
            version_in_name("refx-1.10.3-windows-x86_64"),
            Some("1.10.3")
        );
        // รูปอื่นต้องอ่านไม่ออก ไม่ใช่เดาเอา
        assert_eq!(version_in_name("refx-0.1.0-linux-x86_64"), None);
        assert_eq!(version_in_name("something-else"), None);
    }

    /// ★★ ชื่อ `.deb` มี `-<revision>` ต่อท้ายเวอร์ชันเสมอ
    ///
    /// ไม่ตัดมันทิ้ง ประตูเวอร์ชันจะแดงทุกครั้งด้วยเหตุผลที่ไม่ใช่ความผิดของใคร
    /// แล้วคนจะปิดประตูทิ้ง — ซึ่งแย่กว่าไม่มีประตู
    #[test]
    fn the_debian_revision_suffix_does_not_break_the_version_gate() {
        assert_eq!(version_in_deb_name("refx_0.1.0-1_amd64.deb"), Some("0.1.0"));
        assert_eq!(
            version_in_deb_name("refx_1.10.3-2_amd64.deb"),
            Some("1.10.3")
        );
        // ไม่มี revision ก็ต้องอ่านได้
        assert_eq!(version_in_deb_name("refx_0.1.0_amd64.deb"), Some("0.1.0"));
        // ของคนอื่นต้องอ่านไม่ออก
        assert_eq!(version_in_deb_name("othertool_1.0-1_amd64.deb"), None);
        assert_eq!(version_in_deb_name("refx-0.1.0-windows-x86_64.zip"), None);
    }

    /// ★ README ต้องบอกว่าข้อมูลอยู่ไหน — ไม่งั้น "portable" จะถูกอ่านว่า
    /// "ข้อมูลพกไปกับโฟลเดอร์" ซึ่งไม่จริง และผู้ใช้จะรู้ตัวตอนที่งานไม่ตามไปด้วย
    #[test]
    fn the_readme_says_where_the_data_actually_lives() {
        let text = readme("0.1.0");
        assert!(text.is_ascii(), "README ไม่ใช่ ASCII — ภาษาหลักคืออังกฤษ");
        assert!(text.contains("RefX 0.1.0"), "ไม่มีเวอร์ชันใน README");
        for needle in [
            "%APPDATA%",
            "%LOCALAPPDATA%",
            "SmartScreen",
            "THIRD-PARTY-LICENSES.md",
        ] {
            assert!(text.contains(needle), "README ไม่ได้พูดถึง {needle}");
        }
        assert!(
            text.contains("does NOT mean"),
            "README ไม่ได้แก้ความเข้าใจผิดเรื่อง portable"
        );
    }
}
