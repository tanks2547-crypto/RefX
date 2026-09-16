//! RefX — reference image manager สำหรับนักวาด
//!
//! ลำดับความสำคัญ: เสถียร > ปลอดภัย > เบา > ฟีเจอร์
//! อ่าน CLAUDE.md ก่อนแก้ไฟล์ใด ๆ ในโปรเจกต์นี้
#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod logging;

use refx_platform::paths::AppPaths;
use refx_platform::single_instance::SingleInstance;
use refx_ui::app::AppArgs;

/// อาร์กิวเมนต์บรรทัดคำสั่งที่ RefX รู้จัก
///
/// เขียน parser เองเพราะมีแค่ไม่กี่ตัว — `clap` ไม่มีใน docs/09-crate-versions.md
/// และการเพิ่ม dependency ต้องขออนุญาตก่อน (CLAUDE.md)
struct Cli {
    args: AppArgs,
    show_help: bool,
    show_version: bool,
}

/// ★★★ บรรทัดเดียวที่ประตูของแพ็กเกจใช้เทียบเวอร์ชัน
///
/// `ROADMAP` ก้อน d บังคับว่าเวอร์ชันต้องตรงกัน **สามที่**: `Cargo.toml` ·
/// `--version` ของไบนารี · ชื่อ/เมทาดาทาของแพ็กเกจ · ถ้าไบนารีบอกเวอร์ชันเองไม่ได้
/// ประตูนั้นเทียบได้แค่สองที่ แล้วไฟล์ที่แจกออกไปอาจเป็นบิลด์คนละตัวกับที่กล่องบอก
///
/// รูปแบบ `refx <semver>` — ★ ห้ามเปลี่ยนรูปโดยไม่แก้ `xtask/src/package.rs` ด้วย
#[must_use]
fn version_line() -> String {
    format!("refx {}", env!("CARGO_PKG_VERSION"))
}

fn parse_cli() -> Result<Cli, String> {
    parse_args(std::env::args().skip(1))
}

/// ★ รับอาร์กิวเมนต์เป็นพารามิเตอร์ **ไม่ใช่อ่าน `std::env` เอง**
///
/// ตราบใดที่มันอ่าน environment เอง ไม่มีเทสต์ตัวไหนเดินเข้ากิ่ง error ของมันได้เลย
/// — ทุกข้อความที่ผู้ใช้เห็นตอนพิมพ์ผิดจึงไม่เคยถูกตรวจสักบรรทัด (`docs/08 §3.9` ข้อ 8)
fn parse_args<I: Iterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut cli = Cli {
        args: AppArgs::default(),
        show_help: false,
        show_version: false,
    };

    for arg in args {
        if arg == "--help" || arg == "-h" {
            cli.show_help = true;
        } else if arg == "--version" || arg == "-V" {
            cli.show_version = true;
        } else if let Some(value) = arg.strip_prefix("--force-device-lost-after-ms=") {
            // ★ ต้องเช็คตัวนี้ก่อน --force-device-lost-after= ไม่งั้น prefix สั้นกว่าจะกินไปก่อน
            let ms: u64 = value.parse().map_err(|_| {
                format!(
                    "--force-device-lost-after-ms needs a whole number of milliseconds, got {value:?}"
                )
            })?;
            cli.args.force_device_lost_after_ms = Some(ms);
        } else if let Some(value) = arg.strip_prefix("--lang=") {
            cli.args.lang = Some(match value {
                "en" => refx_ui::text::Lang::En,
                "th" => refx_ui::text::Lang::Th,
                other => {
                    return Err(format!(
                        "--lang accepts en or th, got {other:?}
                         leave it out to follow the system language"
                    ));
                }
            });
        } else if let Some(value) = arg.strip_prefix("--mode=") {
            cli.args.mode = Some(match value {
                "canvas" => refx_core::view::Mode::Canvas,
                "arrange" => refx_core::view::Mode::Arrange,
                other => {
                    return Err(format!("--mode accepts canvas or arrange, got {other:?}"));
                }
            });
        } else if let Some(value) = arg.strip_prefix("--open=") {
            // ★ เปิดเอกสาร `.refx` ตั้งแต่เริ่มโปรแกรม — เส้นทางเดียวกับ `Ctrl+O`
            //   ทุกประการ ต่างแค่ไม่ต้องผ่าน dialog · นี่คือสิ่งที่ Explorer ทำ
            //   ตอนผู้ใช้ดับเบิลคลิกไฟล์ `.refx` และเป็นทางเดียวที่ relink (P4-6)
            //   ถูกยืนยันบนแอปจริงได้ (native dialog ขับด้วยสคริปต์ไม่ได้ — HANDOFF §2.26)
            cli.args.open_document = Some(std::path::PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--open-dir=") {
            // สแกนโฟลเดอร์ตอนเริ่มโปรแกรม (ไม่ใช่ในลูปเฟรม) — ไม่ขัด I-2
            cli.args.open_files = scan_images(std::path::Path::new(value))?;
        } else if let Some(value) = arg.strip_prefix("--demo-quads=") {
            let n: u32 = value.parse().map_err(|_| {
                format!("--demo-quads needs a whole number of quads, got {value:?}")
            })?;
            cli.args.demo_quads = Some(n);
        } else if let Some(value) = arg.strip_prefix("--bench-seconds=") {
            let n: u64 = value.parse().map_err(|_| {
                format!("--bench-seconds needs a whole number of seconds, got {value:?}")
            })?;
            cli.args.bench_seconds = Some(n);
        } else if let Some(value) = arg.strip_prefix("--force-device-lost-after=") {
            let n: u64 = value.parse().map_err(|_| {
                format!("--force-device-lost-after needs a whole number of frames, got {value:?}")
            })?;
            cli.args.force_device_lost_after = Some(n);
        } else {
            return Err(format!(
                "unknown option {arg:?}\nrun `refx --help` to see what is available"
            ));
        }
    }
    Ok(cli)
}

/// หาไฟล์ภาพในโฟลเดอร์ (ไม่ลงลึกในโฟลเดอร์ย่อย)
fn scan_images(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|err| format!("cannot open the folder {}: {err}", dir.display()))?;

    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        // ★★★ ไม่ใช่การกรองนามสกุล — ไฟล์อะไรก็เปิดได้และกลายเป็น `Missing`
        //     ถ้าเปิดไม่ออก (I-7) · แต่ `.refx-meta` เป็น **ไฟล์ที่เราเขียนเอง**
        //     ตั้งแต่ P5-5 การไม่กรองมันแปลว่าเปิดโฟลเดอร์เดิมซ้ำแล้วได้ภาพเสีย
        //     แถมมาหนึ่งใบทุกครั้ง (เจอจริงบนแอปจริง 11 ก.ย. 2026: 3 ไฟล์ → 4 items)
        .filter(|p| !refx_io::sidecar::is_sidecar(p))
        .collect();
    // เรียงชื่อให้ผลลัพธ์คงที่ทุกครั้ง (วัดผลเทียบกันได้)
    files.sort();
    Ok(files)
}

/// ★★★ ภาษาอังกฤษ **ไม่ใช่ตัวเลือก** — `docs/03 §0` ตัดสินไว้ว่าภาษาหลักของ UI
/// คืออังกฤษ ไทยเป็นภาษาที่สอง · ข้อความนี้เคยเป็นไทยล้วนตั้งแต่ P0 ซึ่งแปลว่า
/// **ผู้ใช้ที่ไม่ได้อ่านไทยรัน `refx --help` แล้วอ่านไม่ออกสักบรรทัด**
///
/// ★★ และที่นี่ **ไม่แปลตาม `--lang`** โดยตั้งใจ: ข้อความนี้ถูกพิมพ์ระหว่างอ่าน
/// อาร์กิวเมนต์ ซึ่งเกิด**ก่อน**ที่ระบบภาษาจะถูกตั้งขึ้น · การยกระบบแปลขึ้นมา
/// ก่อนเวลาเพื่อข้อความเดียวคือการเพิ่มสิ่งที่พังได้บนเส้นทางเริ่มโปรแกรม
/// เพื่อแลกกับอะไรที่เล็กกว่ามาก — ถ้าวันหนึ่งต้องแปล ให้แปลทั้งชั้น CLI พร้อมกัน
const HELP: &str = "\
RefX - reference image manager for artists

Usage:
  refx [options]

Options:
  -h, --help                           show this message
  -V, --version                        print the version and exit
      --lang=en|th                     force the UI language (default: follow the system)
      --open=FILE.refx                 open a saved board (same as double-clicking it)
      --open-dir=PATH                  open every image in a folder (same as dragging it in)
      --demo-quads=N                   draw N random quads (exercises pipeline / pan-zoom)
      --bench-seconds=S                measure frame time for S seconds, then report
      --mode=canvas|arrange            mode to start in (default: Canvas)
                                       lets Arrange be measured or photographed
                                       without pressing anything first

Test options (needs a build with --features force-device-lost):
      --force-device-lost-after=N      simulate a GPU device loss after N frames
                                       (simulates losing it mid-drag)
      --force-device-lost-after-ms=MS  simulate a GPU device loss after MS milliseconds
                                       (simulates losing it while the app sleeps -
                                        driver update / sleep-resume, which is what
                                        users actually hit)
";

fn main() -> anyhow::Result<()> {
    let cli = match parse_cli() {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    // ★ ตอบก่อนแตะอย่างอื่นทั้งหมด — `--version` ต้องใช้ได้แม้เครื่องจะไม่มี
    //   โฟลเดอร์ config/cache ที่เขียนได้ (ประตูของแพ็กเกจเรียกมันบนเครื่องเปล่า)
    if cli.show_version {
        println!("{}", version_line());
        return Ok(());
    }

    if cli.show_help {
        println!("{HELP}");
        return Ok(());
    }

    let paths = AppPaths::discover()?;
    paths.ensure_exist()?;

    // log ต้องพร้อมก่อนอย่างอื่น ไม่งั้นปัญหาตอนเปิดโปรแกรมจะไม่ถูกบันทึก
    // ถือ guard ไว้ถึงจบ main — drop แล้ว log ที่ค้างใน buffer จะหาย
    let _log_guard = logging::init(paths.log_dir())?;
    logging::install_panic_hook(paths.log_dir());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "RefX starting");

    // ต้องถือ guard ไว้ถึงจบ main — drop เมื่อไหร่ = ปลดล็อกทันที
    // เปิดซ้ำสองตัวแล้วเขียน cache.sqlite พร้อมกันเสี่ยงข้อมูลเสีย (I-3)
    let _instance_guard = match SingleInstance::acquire(paths.cache_dir()) {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    // ControlFlow::Wait ตั้งอยู่ใน refx-platform::window::run() ที่เดียว (I-1)
    //
    // ★ `recovery_dir()` อยู่ใต้ **data_dir ไม่ใช่ cache_dir** — งานที่ยังไม่เคย
    //   บันทึกคือสิ่งที่สร้างใหม่ไม่ได้ (docs/07 §4 · เทสต์คุมไว้ที่ AppPaths)
    // ★★★ `?` เฉย ๆ ไม่พอ — RefX เป็น **GUI application** ซึ่งบน Windows
    //   ไม่มี stderr ให้พิมพ์ · ความล้มเหลวตอนเปิด (ไม่มี GPU ที่ใช้ได้ ·
    //   ไดรเวอร์เพิ่งอัปเดตแล้วพัง · `WGPU_BACKEND` ตั้งผิด) จึงกลายเป็น
    //   **"ดับเบิลคลิกแล้วไม่เกิดอะไรขึ้นเลย"** ในสายตาผู้ใช้
    //
    //   เจอ 19 ก.ย. 2026 ตอนทดสอบ `WGPU_BACKEND` ที่ตั้งผิด: ข้อความที่เขียนไว้
    //   อย่างดีลง log ไม่มีค่าอะไรเลยถ้าผู้ใช้ไม่รู้ว่ามี log ให้เปิด
    //
    //   ★ ยังคืน `Err` ต่อไปเหมือนเดิม — exit code ที่ไม่ใช่ศูนย์คือสิ่งที่
    //     สคริปต์และประตูของแพ็กเกจอ่าน · dialog เป็นของผู้ใช้ ไม่ใช่ของเครื่องมือ
    let started = refx_ui::app::run(
        cli.args,
        &paths.cache_dir().join("cache.sqlite"),
        &paths.recovery_dir(),
        // ★ ภาพที่วางจาก clipboard สร้างใหม่ไม่ได้จากอะไรเลย — เหตุผลเดียวกับ
        //   `recovery_dir()` เป๊ะ จึงอยู่ใต้ data_local_dir ไม่ใช่ cache_dir
        &paths.spool_dir(),
        // ★ `settings.toml` อยู่ใต้ config_dir — เป็นของที่ผู้ใช้แก้เองด้วยมือ
        //   จึงต้องอยู่ที่ที่ OS บอกว่าเป็น config ไม่ใช่ cache ที่มีคนตั้งใจลบ
        paths.config_dir(),
    );
    if let Err(err) = started {
        refx_platform::dialog::show_startup_failure(&err.to_string(), paths.log_dir());
        return Err(err.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// ★★★ ทุกบรรทัดที่ **CLI** พิมพ์ออกมาต้องเป็นภาษาอังกฤษ (`docs/03 §0`)
    ///
    /// ข้อความช่วยเหลือเป็นไทยล้วนมาตั้งแต่ P0 — ผู้ใช้ที่อ่านไทยไม่ออกจึงรัน
    /// `refx --help` แล้วไม่ได้อะไรเลย · และมันมองไม่เห็นจากประตู tofu ด้วย
    /// เพราะประตูนั้นดูสิ่งที่ **egui วาด** ส่วนนี่ออกทาง stdout ของเทอร์มินัล
    ///
    /// ★ ตรวจด้วย ASCII เพราะภาษาหลักคืออังกฤษ — ตัวอักษรนอก ASCII โผล่เมื่อไหร่
    /// แปลว่ามีคนเขียนภาษาที่สองกลับเข้ามาในชั้นที่ไม่มีระบบแปล
    #[test]
    fn everything_the_command_line_prints_is_english() {
        assert!(
            HELP.is_ascii(),
            "ข้อความ --help มีอักขระนอก ASCII — ชั้น CLI ไม่มีระบบแปล ภาษาหลักคืออังกฤษ"
        );

        // ทุกกิ่ง error ที่ผู้ใช้ไปถึงได้ด้วยการพิมพ์ผิด
        let broken = [
            vec!["--lang=xx"],
            vec!["--mode=sideways"],
            vec!["--demo-quads=lots"],
            vec!["--bench-seconds=soon"],
            vec!["--force-device-lost-after=x"],
            vec!["--force-device-lost-after-ms=x"],
            vec!["--what-is-this"],
            vec!["--open-dir=E:/no/such/folder/here"],
        ];
        for case in broken {
            let Err(err) = parse_args(args(&case).into_iter()) else {
                panic!("{case:?} ควรถูกปฏิเสธ แต่ผ่านไปได้");
            };
            assert!(
                err.is_ascii(),
                "ข้อความ error ของ {case:?} ไม่ใช่ ASCII: {err}"
            );
            assert!(!err.trim().is_empty(), "{case:?} ถูกปฏิเสธแบบเงียบ ๆ");
        }
    }

    /// ★★★ `--version` ต้องพูดเวอร์ชันเดียวกับ `Cargo.toml` เป๊ะ
    ///
    /// ประตูของแพ็กเกจเทียบสามที่ (`Cargo.toml` · ไบนารี · ชื่อแพ็กเกจ) โดยอ่าน
    /// จากบรรทัดนี้ · ถ้ารูปแบบเปลี่ยนโดยไม่มีใครรู้ ประตูจะอ่านไม่ออกแล้วกลายเป็น
    /// ประตูที่ผ่านตลอด — จึงตรึงทั้ง **รูปแบบ** และ **ค่า** ไว้ที่นี่
    #[test]
    fn the_version_the_binary_prints_is_the_one_in_cargo_toml() {
        let line = version_line();
        assert_eq!(line, format!("refx {}", env!("CARGO_PKG_VERSION")));
        assert!(line.starts_with("refx "), "ประตูของแพ็กเกจอ่านรูปนี้อยู่: {line}");

        // ต้องเป็น semver ที่มีตัวเลขจริง ไม่ใช่สตริงว่างหรือ placeholder
        let version = line.trim_start_matches("refx ").trim();
        assert!(
            version.split('.').count() >= 2
                && version.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "เวอร์ชันไม่ใช่ semver: {version:?}"
        );

        let cli = parse_args(args(&["--version"]).into_iter()).unwrap();
        assert!(cli.show_version, "--version ไม่ถูกอ่าน");
        let short = parse_args(args(&["-V"]).into_iter()).unwrap();
        assert!(short.show_version, "-V ไม่ถูกอ่าน");
        // ★ และต้องไม่ไปทับ --help
        assert!(!cli.show_help);
    }

    /// ★ ประตูของประตู: อาร์กิวเมนต์ที่ถูกต้องต้อง **ผ่าน** และมีผลจริง
    ///
    /// ไม่มีข้อนี้ เทสต์ข้างบนจะยังเขียวแม้ parser ปฏิเสธทุกอย่างบนโลก
    #[test]
    fn the_options_that_should_work_still_do() {
        let cli = parse_args(args(&["--help"]).into_iter()).unwrap();
        assert!(cli.show_help);

        let cli = parse_args(args(&["--lang=th", "--mode=arrange", "--demo-quads=8"]).into_iter())
            .unwrap();
        assert_eq!(cli.args.lang, Some(refx_ui::text::Lang::Th));
        assert_eq!(cli.args.mode, Some(refx_core::view::Mode::Arrange));
        assert_eq!(cli.args.demo_quads, Some(8));
        assert!(!cli.show_help);

        // ★ prefix ที่ยาวกว่าต้องชนะ ไม่งั้น `--force-device-lost-after-ms=` จะถูก
        //   `--force-device-lost-after=` กินไปก่อนแล้วได้ error ที่งงมาก
        let cli = parse_args(args(&["--force-device-lost-after-ms=250"]).into_iter()).unwrap();
        assert_eq!(cli.args.force_device_lost_after_ms, Some(250));
        assert_eq!(cli.args.force_device_lost_after, None);
    }
}
