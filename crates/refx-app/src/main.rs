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
}

fn parse_cli() -> Result<Cli, String> {
    let mut cli = Cli {
        args: AppArgs::default(),
        show_help: false,
    };

    for arg in std::env::args().skip(1) {
        if arg == "--help" || arg == "-h" {
            cli.show_help = true;
        } else if let Some(value) = arg.strip_prefix("--force-device-lost-after-ms=") {
            // ★ ต้องเช็คตัวนี้ก่อน --force-device-lost-after= ไม่งั้น prefix สั้นกว่าจะกินไปก่อน
            let ms: u64 = value.parse().map_err(|_| {
                format!(
                    "--force-device-lost-after-ms ต้องเป็นจำนวนมิลลิวินาที (ตัวเลขเต็มบวก) แต่ได้ {value:?}"
                )
            })?;
            cli.args.force_device_lost_after_ms = Some(ms);
        } else if let Some(value) = arg.strip_prefix("--lang=") {
            cli.args.lang = Some(match value {
                "en" => refx_ui::text::Lang::En,
                "th" => refx_ui::text::Lang::Th,
                other => {
                    return Err(format!(
                        "--lang รู้จักแค่ en กับ th แต่ได้ {other:?}
                         ไม่ใส่เลย = ใช้ภาษาของระบบ"
                    ));
                }
            });
        } else if let Some(value) = arg.strip_prefix("--mode=") {
            cli.args.mode = Some(match value {
                "canvas" => refx_core::view::Mode::Canvas,
                "arrange" => refx_core::view::Mode::Arrange,
                other => {
                    return Err(format!("--mode รู้จักแค่ canvas กับ arrange แต่ได้ {other:?}"));
                }
            });
        } else if let Some(value) = arg.strip_prefix("--open-dir=") {
            // สแกนโฟลเดอร์ตอนเริ่มโปรแกรม (ไม่ใช่ในลูปเฟรม) — ไม่ขัด I-2
            cli.args.open_files = scan_images(std::path::Path::new(value))?;
        } else if let Some(value) = arg.strip_prefix("--demo-quads=") {
            let n: u32 = value.parse().map_err(|_| {
                format!("--demo-quads ต้องเป็นจำนวนสี่เหลี่ยม (ตัวเลขเต็มบวก) แต่ได้ {value:?}")
            })?;
            cli.args.demo_quads = Some(n);
        } else if let Some(value) = arg.strip_prefix("--bench-seconds=") {
            let n: u64 = value.parse().map_err(|_| {
                format!("--bench-seconds ต้องเป็นจำนวนวินาที (ตัวเลขเต็มบวก) แต่ได้ {value:?}")
            })?;
            cli.args.bench_seconds = Some(n);
        } else if let Some(value) = arg.strip_prefix("--force-device-lost-after=") {
            let n: u64 = value.parse().map_err(|_| {
                format!("--force-device-lost-after ต้องเป็นจำนวนเฟรม (ตัวเลขเต็มบวก) แต่ได้ {value:?}")
            })?;
            cli.args.force_device_lost_after = Some(n);
        } else {
            return Err(format!(
                "ไม่รู้จักตัวเลือก {arg:?}\nลองรัน refx --help เพื่อดูรายการที่ใช้ได้"
            ));
        }
    }
    Ok(cli)
}

/// หาไฟล์ภาพในโฟลเดอร์ (ไม่ลงลึกในโฟลเดอร์ย่อย)
fn scan_images(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|err| format!("เปิดโฟลเดอร์ {} ไม่ได้: {err}", dir.display()))?;

    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    // เรียงชื่อให้ผลลัพธ์คงที่ทุกครั้ง (วัดผลเทียบกันได้)
    files.sort();
    Ok(files)
}

const HELP: &str = "\
RefX — โปรแกรมจัดการภาพ reference สำหรับนักวาด

การใช้งาน:
  refx [ตัวเลือก]

ตัวเลือก:
  -h, --help                           แสดงข้อความนี้
      --lang=en|th                     บังคับภาษาของ UI (ไม่ใส่ = ตามภาษาของระบบ)
      --open-dir=PATH                  เปิดไฟล์ภาพทั้งโฟลเดอร์ (เหมือนลากเข้ามา)
      --demo-quads=N                   วาดสี่เหลี่ยมสีสุ่ม N อัน (ทดสอบ pipeline/pan-zoom)
      --bench-seconds=S                วัด frame time ต่อเนื่อง S วินาทีแล้วรายงานผล
      --mode=canvas|arrange            โหมดที่เปิดขึ้นมา (ไม่ใส่ = Canvas)
                                       มีไว้ให้วัด/ถ่ายภาพโหมด Arrange ได้โดยไม่ต้องกดปุ่มก่อน

ตัวเลือกสำหรับทดสอบ (ต้อง build ด้วย --features force-device-lost):
      --force-device-lost-after=N      จำลอง GPU device lost หลังวาดครบ N เฟรม
                                       (จำลองตอนผู้ใช้กำลังลากภาพ)
      --force-device-lost-after-ms=MS  จำลอง GPU device lost หลังผ่านไป MS มิลลิวินาที
                                       (จำลองตอนแอปหลับอยู่ — driver อัปเดต/sleep-resume
                                        ซึ่งเป็นสถานการณ์จริงที่ผู้ใช้เจอบ่อยกว่า)
";

fn main() -> anyhow::Result<()> {
    let cli = match parse_cli() {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

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
    refx_ui::app::run(
        cli.args,
        &paths.cache_dir().join("cache.sqlite"),
        &paths.recovery_dir(),
        // ★ ภาพที่วางจาก clipboard สร้างใหม่ไม่ได้จากอะไรเลย — เหตุผลเดียวกับ
        //   `recovery_dir()` เป๊ะ จึงอยู่ใต้ data_local_dir ไม่ใช่ cache_dir
        &paths.spool_dir(),
    )?;
    Ok(())
}
