//! งาน build/dev — เรียกด้วย `cargo xtask <cmd>`
//!
//!   gen-testdata   สร้าง dataset 1000 ภาพสำหรับ benchmark
//!   dump-refx      แปลง .refx (binary) เป็น JSON เพื่อ debug  (P4-8)
//!   bench          รัน benchmark ทั้งชุดแล้วเทียบกับเพดานใน docs/08
//!   package        สร้าง installer / portable zip
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

fn main() -> anyhow::Result<()> {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    match cmd.as_str() {
        "gen-testdata" => gen_testdata(),
        "dump-refx" => todo!("P4-8"),
        "bench" => todo!("P5-1"),
        "package" => todo!("P5-6"),
        other => {
            eprintln!("ไม่รู้จักคำสั่ง: {other:?}");
            Ok(())
        }
    }
}
