//! งาน build/dev — เรียกด้วย `cargo xtask <cmd>`
//!
//!   gen-testdata   สร้าง dataset 1000 ภาพสำหรับ benchmark
//!   gen-fuzz-seeds สร้าง corpus ตั้งต้นของ fuzz_packed         (P4-9)
//!   dump-refx      แปลง .refx (binary) เป็น JSON เพื่อ debug  (P4-8)
//!   mutation       ประตูข้อ 1b — ทำให้ด่านทำงานทุกครั้งแล้วดูว่าเทสต์ไหนแดง
//!   licenses       สร้าง THIRD-PARTY-LICENSES.md  (`--check` = ประตู)
//!   bench          รัน benchmark ทั้งชุดแล้วเทียบกับเพดานใน docs/08
//!   package        สร้าง portable zip ของ Windows + ตรวจจากตัวแพ็กเกจเอง

mod dump;
mod json;
mod licenses;
mod mutation;
mod package;
mod seeds;

/// สร้าง dataset สำหรับ benchmark (P5-1 บางส่วน)
///
/// ใช้ขนาดและ format ที่**ใกล้เคียงของจริง**: 4000×3000 ผสม JPEG/PNG
/// เพราะนักวาดลากไฟล์จาก ArtStation หรือสแกนงานตัวเอง ปกติ 3000–6000 px
/// และมี JPEG เยอะกว่า PNG ซึ่ง decode ช้ากว่ามาก
///
/// `cargo xtask gen-testdata <โฟลเดอร์> <จำนวน> [กว้าง] [สูง]`
fn gen_testdata() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(2);
    let dir: std::path::PathBuf = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("ระบุโฟลเดอร์ปลายทางด้วย"))?
        .into();
    let count: u32 = args.next().unwrap_or_else(|| "100".into()).parse()?;
    let width: u32 = args.next().unwrap_or_else(|| "4000".into()).parse()?;
    let height: u32 = args.next().unwrap_or_else(|| "3000".into()).parse()?;

    std::fs::create_dir_all(&dir)?;
    for i in 0..count {
        // ลายที่บีบอัดไม่ลงง่าย ๆ เพื่อให้เวลา decode ใกล้เคียงภาพถ่ายจริง
        let mut img = image::RgbImage::new(width, height);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let s = i.wrapping_mul(37);
            *px = image::Rgb([
                ((x.wrapping_mul(7).wrapping_add(s)) % 256) as u8,
                ((y.wrapping_mul(11).wrapping_add(s)) % 256) as u8,
                (((x ^ y).wrapping_add(s)) % 256) as u8,
            ]);
        }
        // ผสม JPEG 70% / PNG 30% ตามสัดส่วนที่ผู้ใช้มีจริง
        let path = if i % 10 < 7 {
            dir.join(format!("img{i:04}.jpg"))
        } else {
            dir.join(format!("img{i:04}.png"))
        };
        img.save(&path)?;
        if i % 10 == 0 {
            println!("  {}/{count}", i + 1);
        }
    }
    println!("สร้าง {count} ไฟล์ ({width}x{height}) ที่ {}", dir.display());
    Ok(())
}

/// ★ `cargo xtask dump-refx <ไฟล์> [--verify-assets] [--out <ไฟล์>]`
///
/// ★★ **โค้ดสถานะเป็น 0 แม้ไฟล์จะพัง** — การรายงานว่าไฟล์พังยังไงคือ *ผลลัพธ์*
/// ของเครื่องมือนี้ ไม่ใช่ความล้มเหลวของมัน · โค้ดที่ไม่ใช่ 0 สงวนไว้ให้กรณีที่
/// **เปิดไฟล์ไม่ได้เลย** หรือเขียนผลลัพธ์ไม่ได้ ซึ่งเป็นคนละเรื่องกัน
fn dump_refx() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(2);
    let mut path: Option<std::path::PathBuf> = None;
    let mut out_path: Option<std::path::PathBuf> = None;
    let mut options = dump::Options::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verify-assets" => options.verify_assets = true,
            "--out" => {
                out_path = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("`--out` ต้องตามด้วยชื่อไฟล์ปลายทาง"))?
                        .into(),
                );
            }
            other if other.starts_with('-') => {
                anyhow::bail!(
                    "ไม่รู้จักตัวเลือก {other:?}\n\
                     ใช้: cargo xtask dump-refx <ไฟล์.refx> [--verify-assets] [--out <ไฟล์.json>]"
                );
            }
            other => path = Some(other.into()),
        }
    }

    let path = path.ok_or_else(|| {
        anyhow::anyhow!(
            "ระบุไฟล์ .refx ที่จะ dump ด้วย\n\
             ใช้: cargo xtask dump-refx <ไฟล์.refx> [--verify-assets] [--out <ไฟล์.json>]\n\
             \n\
             --verify-assets  อ่าน asset blob ทุกก้อนแล้วตรวจ crc (ต้องอ่านทั้งไฟล์ ช้ากับไฟล์ packed ใหญ่ ๆ)\n\
             --out            เขียนลงไฟล์แทน stdout — ★ แนะนำบน Windows เพราะคอนโซล\n\
             \x20                มักไม่ได้ตั้ง UTF-8 แล้วข้อความไทยจะอ่านไม่ออก"
        )
    })?;

    match out_path {
        Some(target) => {
            let file = std::fs::File::create(&target)
                .map_err(|err| anyhow::anyhow!("เขียน {} ไม่ได้: {err}", target.display()))?;
            let mut out = std::io::BufWriter::new(file);
            dump::dump_file(&path, &mut out, options)?;
            println!("เขียนผลลัพธ์ลง {}", target.display());
        }
        None => {
            let stdout = std::io::stdout();
            let mut out = std::io::BufWriter::new(stdout.lock());
            dump::dump_file(&path, &mut out, options)?;
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    match cmd.as_str() {
        "gen-testdata" => gen_testdata(),
        "gen-fuzz-seeds" => seeds::gen_fuzz_seeds(),
        "dump-refx" => dump_refx(),
        "mutation" => mutation::run(),
        "licenses" => licenses::run(),
        "bench" => todo!("P5-1"),
        "package" => package::run(),
        other => {
            eprintln!(
                "ไม่รู้จักคำสั่ง: {other:?}\n\
                 คำสั่งที่มี: gen-testdata · gen-fuzz-seeds · dump-refx · mutation · \
                 licenses · bench (P5-1) · package (P5-6)"
            );
            Ok(())
        }
    }
}
