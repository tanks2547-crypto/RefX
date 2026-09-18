//! สร้างไอคอนทุกรูปแบบจาก `assets/icon/refx.svg` — **ไฟล์เดียวคือแหล่งความจริง**
//!
//! ## ทำไมต้องเป็นเครื่องมือ ไม่ใช่ไฟล์ที่แปลงด้วยมือแล้ว commit เฉย ๆ
//!
//! `.ico` / `.png` / `.rgba` เป็นไบนารีที่มนุษย์อ่านไม่ออก · ถ้าใครแก้ `.svg`
//! แล้วลืมแปลงใหม่ จะไม่มีอะไรบอกเลย — ไอคอนบน taskbar ต่างจากไอคอนใน
//! installer โดยไม่มีใครรู้ตัว รูปเดียวกับ `THIRD-PARTY-LICENSES.md`
//! ที่ล้าสมัยเงียบ ๆ · `icon --check` จึงสร้างใหม่แล้วเทียบไบต์ทุกครั้งใน CI
//!
//! ## ★★★ ทำไมเขียน rasterizer เองแทนที่จะดึง `resvg`
//!
//! เพิ่ม dependency ใหม่ต้องหยุดถามเจ้าของตาม `CLAUDE.md` · และเราต้องการแค่
//! รูปทรงห้าอย่างที่ไฟล์นี้ใช้จริง ไม่ใช่ SVG ทั้งสเปก
//!
//! ★ ตัว parser จึง **ล้มเสียงดังเมื่อเจอสิ่งที่ไม่รู้จัก** ทั้ง tag, attribute
//! และคำสั่งใน `d=` — นี่คือคุณสมบัติที่สำคัญที่สุดของมัน: ถ้าเจ้าของแก้ `.svg`
//! ด้วยของที่เรายังไม่รองรับ (gradient, opacity, scale) ประตูต้องแดงพร้อมชื่อ
//! สิ่งที่ไม่รองรับ **ไม่ใช่วาดผิดเงียบ ๆ แล้วบอกว่าผ่าน** (`docs/08 §3.9` ข้อ 19)

#![expect(
    clippy::disallowed_methods,
    reason = "เครื่องมือ dev อ่าน/เขียนไฟล์ asset เอง ไม่ใช่ดิสก์ I/O บนลูปเฟรม"
)]

use std::path::{Path, PathBuf};

/// ต้นฉบับที่มนุษย์แก้ — ทุกอย่างข้างล่างสร้างจากไฟล์นี้
pub const SVG: &str = "assets/icon/refx.svg";
/// ไอคอนของ Windows — ใช้โดย MSI (ARP + shortcut) และ `refx.exe` เอง
pub const ICO: &str = "assets/icon/refx.ico";
/// ไอคอนของ Linux — `usr/share/icons/hicolor/256x256/apps/refx.png`
pub const PNG_256: &str = "assets/icon/refx-256.png";
/// RGBA ดิบสำหรับ `winit::window::Icon` — ไม่ต้อง decode ตอนเปิดโปรแกรม
pub const RGBA_64: &str = "assets/icon/refx-64.rgba";

/// ขนาดที่ใส่ใน `.ico` — 16/32/48 คือที่ Windows ใช้จริงบน taskbar, Alt+Tab,
/// และหน้า "Apps & features" · 256 คือมุมมอง icon ใหญ่ใน Explorer
const ICO_SIZES: &[u32] = &[16, 32, 48, 256];
/// ขนาดของไอคอนหน้าต่าง — winit ย่อเองให้ 16/32 ตามที่ Windows ขอ
const WINDOW_ICON: u32 = 64;
/// จำนวนจุดสุ่มต่อด้านต่อพิกเซล (64 ระดับ) — พอให้ขอบมนที่ 16 px ไม่หยัก
const SUBSAMPLES: u32 = 8;

/// `cargo xtask icon [--check]`
pub fn run() -> anyhow::Result<()> {
    // ★ ไม่ใช่ `.any(|a| a == "--check")` — แบบนั้น `--chek` จะถูกทิ้งเงียบ ๆ
    //   แล้วคำสั่งกลายเป็น "สร้างไฟล์ใหม่" แทนที่จะเป็นประตู (`docs/08 §3.9` ข้อ 9)
    let mut args = crate::args::Args::new("cargo xtask icon [--check]");
    let check = args.flag("--check");
    args.finish()?;
    let root = repo_root()?;

    let svg_path = root.join(SVG);
    let text = std::fs::read_to_string(&svg_path)
        .map_err(|err| anyhow::anyhow!("อ่าน {} ไม่ได้: {err}", svg_path.display()))?;
    let icon = parse(&text)?;

    let wanted: Vec<(PathBuf, Vec<u8>)> = vec![
        (root.join(ICO), build_ico(&icon)?),
        (root.join(PNG_256), encode_png(&render(&icon, 256), 256)?),
        (root.join(RGBA_64), render(&icon, WINDOW_ICON)),
    ];

    if check {
        let mut stale = Vec::new();
        for (path, bytes) in &wanted {
            match std::fs::read(path) {
                Ok(found) if found == *bytes => {}
                Ok(found) => stale.push(format!(
                    "  {} — ต่างกัน (บนดิสก์ {} ไบต์ · ที่ควรเป็น {} ไบต์)",
                    rel(&root, path),
                    found.len(),
                    bytes.len()
                )),
                Err(err) => stale.push(format!("  {} — อ่านไม่ได้: {err}", rel(&root, path))),
            }
        }
        if !stale.is_empty() {
            anyhow::bail!(
                "ไอคอนที่ commit ไว้ไม่ตรงกับ {SVG}:\n{}\n\n\
                 แก้ด้วย `cargo xtask icon` แล้ว commit ไฟล์ที่ได้ไปด้วย",
                stale.join("\n")
            );
        }
        println!("ไอคอน {} ไฟล์ตรงกับ {SVG}", wanted.len());
        return Ok(());
    }

    for (path, bytes) in &wanted {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, bytes)
            .map_err(|err| anyhow::anyhow!("เขียน {} ไม่ได้: {err}", path.display()))?;
        println!("  {} ({} ไบต์)", rel(&root, path), bytes.len());
    }
    println!("สร้างไอคอนจาก {SVG} เรียบร้อย");
    Ok(())
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// รากของ repo — `CARGO_MANIFEST_DIR` ของ xtask คือ `<root>/xtask`
fn repo_root() -> anyhow::Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("หารากของ repo จาก {} ไม่ได้", manifest.display()))
}

// ───────────────────────── ตัวแทนรูปทรง ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rgb(u8, u8, u8);

#[derive(Debug, Clone)]
enum Shape {
    /// สี่เหลี่ยมมุมมน (`rx = 0` คือมุมฉาก)
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        rx: f64,
    },
    Circle {
        cx: f64,
        cy: f64,
        r: f64,
    },
    /// `d="M .. L .. Z"` เท่านั้น — เก็บเป็นจุดยอด
    Polygon(Vec<(f64, f64)>),
}

#[derive(Debug, Clone)]
struct Drawn {
    shape: Shape,
    fill: Rgb,
    /// `rotate(องศา, cx, cy)` จาก `<g>` ที่ครอบอยู่
    rotate: Option<(f64, f64, f64)>,
}

#[derive(Debug, Clone)]
struct Icon {
    /// ด้านของ `viewBox` (เป็นจัตุรัสเสมอ)
    side: f64,
    /// เรียงจากหลังไปหน้า ตามลำดับในไฟล์
    shapes: Vec<Drawn>,
}

// ───────────────────────── parser ─────────────────────────

/// แยก `<tag attr="v" ...>` ออกมาเป็นรูปทรง — **ไม่รู้จักอะไรคือ error**
fn parse(text: &str) -> anyhow::Result<Icon> {
    let mut side: Option<f64> = None;
    let mut shapes = Vec::new();
    let mut group: Option<(f64, f64, f64)> = None;
    let mut rest = text;

    while let Some(open) = rest.find('<') {
        rest = &rest[open..];
        if let Some(after) = rest.strip_prefix("<!--") {
            let end = after
                .find("-->")
                .ok_or_else(|| anyhow::anyhow!("คอมเมนต์ใน SVG ไม่ถูกปิดด้วย `-->`"))?;
            rest = &after[end + 3..];
            continue;
        }
        let close = rest
            .find('>')
            .ok_or_else(|| anyhow::anyhow!("tag ใน SVG ไม่ถูกปิดด้วย `>`"))?;
        let body = &rest[1..close];
        rest = &rest[close + 1..];

        if let Some(name) = body.strip_prefix('/') {
            match name.trim() {
                "g" => group = None,
                "svg" => {}
                other => anyhow::bail!("ปิด tag ที่ไม่รองรับ: </{other}>"),
            }
            continue;
        }

        let body = body.trim_end_matches('/');
        let (name, attr_text) = match body.find(char::is_whitespace) {
            Some(at) => (&body[..at], &body[at..]),
            None => (body, ""),
        };
        let attrs = attributes(attr_text)?;

        match name {
            "svg" => {
                allow(
                    name,
                    &attrs,
                    &["xmlns", "viewBox", "width", "height", "role", "aria-label"],
                )?;
                side = Some(view_box(get(name, &attrs, "viewBox")?)?);
            }
            "g" => {
                allow(name, &attrs, &["transform"])?;
                if group.is_some() {
                    anyhow::bail!("<g> ซ้อนใน <g> ยังไม่รองรับ — ต้องคูณเมทริกซ์ ซึ่งยังไม่มีใครต้องใช้");
                }
                group = Some(rotate(get(name, &attrs, "transform")?)?);
            }
            "rect" => {
                allow(name, &attrs, &["x", "y", "width", "height", "rx", "fill"])?;
                shapes.push(Drawn {
                    shape: Shape::Rect {
                        x: number(name, &attrs, "x")?,
                        y: number(name, &attrs, "y")?,
                        w: number(name, &attrs, "width")?,
                        h: number(name, &attrs, "height")?,
                        rx: optional_number(&attrs, "rx")?.unwrap_or(0.0),
                    },
                    fill: color(get(name, &attrs, "fill")?)?,
                    rotate: group,
                });
            }
            "circle" => {
                allow(name, &attrs, &["cx", "cy", "r", "fill"])?;
                shapes.push(Drawn {
                    shape: Shape::Circle {
                        cx: number(name, &attrs, "cx")?,
                        cy: number(name, &attrs, "cy")?,
                        r: number(name, &attrs, "r")?,
                    },
                    fill: color(get(name, &attrs, "fill")?)?,
                    rotate: group,
                });
            }
            "path" => {
                allow(name, &attrs, &["d", "fill"])?;
                shapes.push(Drawn {
                    shape: Shape::Polygon(polygon(get(name, &attrs, "d")?)?),
                    fill: color(get(name, &attrs, "fill")?)?,
                    rotate: group,
                });
            }
            other => anyhow::bail!(
                "tag ที่ยังไม่รองรับ: <{other}> — rasterizer ใน xtask รู้จักแค่ \
                 svg/g/rect/circle/path · เพิ่มการรองรับก่อนใช้มันใน {SVG}"
            ),
        }
    }

    let side = side.ok_or_else(|| anyhow::anyhow!("ไม่พบ <svg viewBox=...>"))?;
    if shapes.is_empty() {
        anyhow::bail!("ไม่มีรูปทรงใน {SVG} เลย");
    }
    Ok(Icon { side, shapes })
}

fn attributes(text: &str) -> anyhow::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let eq = rest
            .find('=')
            .ok_or_else(|| anyhow::anyhow!("attribute ไม่มีค่า: {rest:?}"))?;
        let key = rest[..eq].trim().to_owned();
        let after = rest[eq + 1..].trim_start();
        let quoted = after
            .strip_prefix('"')
            .ok_or_else(|| anyhow::anyhow!("ค่าของ {key:?} ต้องอยู่ในเครื่องหมาย \" "))?;
        let end = quoted
            .find('"')
            .ok_or_else(|| anyhow::anyhow!("ค่าของ {key:?} ไม่ถูกปิดด้วย \" "))?;
        out.push((key, quoted[..end].to_owned()));
        rest = quoted[end + 1..].trim_start();
    }
    Ok(out)
}

fn allow(tag: &str, attrs: &[(String, String)], known: &[&str]) -> anyhow::Result<()> {
    for (key, _) in attrs {
        if !known.contains(&key.as_str()) {
            anyhow::bail!(
                "<{tag}> มี attribute ที่ยังไม่รองรับ: {key:?} — \
                 rasterizer จะวาดผิดโดยไม่บอก จึงหยุดตรงนี้แทน"
            );
        }
    }
    Ok(())
}

fn get<'a>(tag: &str, attrs: &'a [(String, String)], key: &str) -> anyhow::Result<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("<{tag}> ขาด attribute {key:?}"))
}

fn number(tag: &str, attrs: &[(String, String)], key: &str) -> anyhow::Result<f64> {
    let raw = get(tag, attrs, key)?;
    raw.trim()
        .parse::<f64>()
        .map_err(|err| anyhow::anyhow!("<{tag}> {key}={raw:?} ไม่ใช่ตัวเลข: {err}"))
}

fn optional_number(attrs: &[(String, String)], key: &str) -> anyhow::Result<Option<f64>> {
    match attrs.iter().find(|(k, _)| k == key) {
        None => Ok(None),
        Some((_, raw)) => raw
            .trim()
            .parse::<f64>()
            .map(Some)
            .map_err(|err| anyhow::anyhow!("{key}={raw:?} ไม่ใช่ตัวเลข: {err}")),
    }
}

fn view_box(raw: &str) -> anyhow::Result<f64> {
    let parts: Vec<f64> = raw
        .split_whitespace()
        .map(|p| p.parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|err| anyhow::anyhow!("viewBox={raw:?} มีค่าที่ไม่ใช่ตัวเลข: {err}"))?;
    match parts.as_slice() {
        [x, y, w, h] if *x == 0.0 && *y == 0.0 && w == h && *w > 0.0 => Ok(*w),
        _ => anyhow::bail!(
            "viewBox={raw:?} — รองรับเฉพาะจัตุรัสที่เริ่มที่ 0 0 \
             (ไอคอนต้องเป็นจัตุรัสอยู่แล้วทุกแพลตฟอร์ม)"
        ),
    }
}

/// `rotate(-11 128 136)` → (องศา, cx, cy)
fn rotate(raw: &str) -> anyhow::Result<(f64, f64, f64)> {
    let inner = raw
        .trim()
        .strip_prefix("rotate(")
        .and_then(|r| r.strip_suffix(')'))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "transform={raw:?} — รองรับเฉพาะ `rotate(องศา cx cy)` \
                 (translate/scale/matrix ยังไม่รองรับ)"
            )
        })?;
    let parts: Vec<f64> = inner
        .split([' ', ','])
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|err| anyhow::anyhow!("transform={raw:?} มีค่าที่ไม่ใช่ตัวเลข: {err}"))?;
    match parts.as_slice() {
        [deg, cx, cy] => Ok((*deg, *cx, *cy)),
        _ => anyhow::bail!("transform={raw:?} — `rotate` ต้องมีสามค่า (องศา cx cy)"),
    }
}

fn color(raw: &str) -> anyhow::Result<Rgb> {
    let hex = raw.trim().strip_prefix('#').ok_or_else(|| {
        anyhow::anyhow!("fill={raw:?} — รองรับเฉพาะ `#RRGGBB` (ชื่อสี/rgb()/gradient ยังไม่รองรับ)")
    })?;
    if hex.len() != 6 {
        anyhow::bail!("fill={raw:?} — ต้องเป็น `#RRGGBB` หกหลักพอดี");
    }
    let byte = |at: usize| -> anyhow::Result<u8> {
        u8::from_str_radix(&hex[at..at + 2], 16)
            .map_err(|err| anyhow::anyhow!("fill={raw:?} ไม่ใช่เลขฐานสิบหก: {err}"))
    };
    Ok(Rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// `d="M70 178 L108 130 ... Z"` — รองรับแค่ `M`, `L`, `Z`
fn polygon(raw: &str) -> anyhow::Result<Vec<(f64, f64)>> {
    let mut points = Vec::new();
    let mut tokens = raw.split([' ', ',']).filter(|t| !t.is_empty()).peekable();
    let mut closed = false;
    while let Some(token) = tokens.next() {
        let (cmd, first) = match token.chars().next() {
            Some(c @ ('M' | 'L')) => (c, token[1..].to_owned()),
            Some('Z' | 'z') => {
                closed = true;
                continue;
            }
            _ => anyhow::bail!(
                "d={raw:?} — คำสั่ง {token:?} ยังไม่รองรับ \
                 (รู้จักแค่ M, L, Z · เส้นโค้ง C/Q/A ต้องเพิ่มโค้ดก่อน)"
            ),
        };
        let x = if first.is_empty() {
            tokens
                .next()
                .ok_or_else(|| anyhow::anyhow!("d={raw:?} — {cmd} ขาดพิกัด x"))?
                .to_owned()
        } else {
            first
        };
        let y = tokens
            .next()
            .ok_or_else(|| anyhow::anyhow!("d={raw:?} — {cmd} ขาดพิกัด y"))?;
        let x = x
            .parse::<f64>()
            .map_err(|err| anyhow::anyhow!("d={raw:?} — x {x:?} ไม่ใช่ตัวเลข: {err}"))?;
        let y = y
            .parse::<f64>()
            .map_err(|err| anyhow::anyhow!("d={raw:?} — y {y:?} ไม่ใช่ตัวเลข: {err}"))?;
        points.push((x, y));
    }
    if !closed {
        anyhow::bail!("d={raw:?} — ต้องปิดรูปด้วย `Z` (เราเติมสีเท่านั้น ไม่วาดเส้น)");
    }
    if points.len() < 3 {
        anyhow::bail!("d={raw:?} — ต้องมีอย่างน้อยสามจุด");
    }
    Ok(points)
}

// ───────────────────────── rasterizer ─────────────────────────

/// วาดเป็น RGBA8 ขนาด `size × size` · สุ่ม `SUBSAMPLES²` จุดต่อพิกเซล
///
/// ★ ผสมแบบ premultiplied จากหลังไปหน้า — ทำให้ขอบมนของฐานยังโปร่งใสจริง
/// ไม่ใช่ขอบดำ ซึ่งเป็นสิ่งที่เห็นชัดมากบน taskbar สีอ่อน
fn render(icon: &Icon, size: u32) -> Vec<u8> {
    let scale = icon.side / f64::from(size);
    let step = scale / f64::from(SUBSAMPLES);
    let total = f64::from(SUBSAMPLES * SUBSAMPLES);
    let mut out = vec![0u8; (size as usize) * (size as usize) * 4];

    for py in 0..size {
        for px in 0..size {
            // premultiplied [0,1]
            let (mut r, mut g, mut b, mut a) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for drawn in &icon.shapes {
                let mut hits = 0u32;
                for sy in 0..SUBSAMPLES {
                    for sx in 0..SUBSAMPLES {
                        let x = f64::from(px) * scale + (f64::from(sx) + 0.5) * step;
                        let y = f64::from(py) * scale + (f64::from(sy) + 0.5) * step;
                        let (x, y) = match drawn.rotate {
                            // วาดรูปที่ถูกหมุน = ทดสอบจุดที่ถูกหมุน **กลับ**
                            Some((deg, cx, cy)) => unrotate(x, y, deg, cx, cy),
                            None => (x, y),
                        };
                        if inside(&drawn.shape, x, y) {
                            hits += 1;
                        }
                    }
                }
                if hits == 0 {
                    continue;
                }
                let cov = f64::from(hits) / total;
                let Rgb(sr, sg, sb) = drawn.fill;
                let keep = 1.0 - cov;
                r = f64::from(sr) / 255.0 * cov + r * keep;
                g = f64::from(sg) / 255.0 * cov + g * keep;
                b = f64::from(sb) / 255.0 * cov + b * keep;
                a = cov + a * keep;
            }

            let at = ((py as usize) * (size as usize) + px as usize) * 4;
            let (r8, g8, b8, a8) = unpremultiply(r, g, b, a);
            out[at] = r8;
            out[at + 1] = g8;
            out[at + 2] = b8;
            out[at + 3] = a8;
        }
    }
    out
}

fn unrotate(x: f64, y: f64, deg: f64, cx: f64, cy: f64) -> (f64, f64) {
    let rad = -deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    let (dx, dy) = (x - cx, y - cy);
    (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos)
}

fn inside(shape: &Shape, x: f64, y: f64) -> bool {
    match shape {
        Shape::Circle { cx, cy, r } => {
            let (dx, dy) = (x - cx, y - cy);
            dx * dx + dy * dy <= r * r
        }
        Shape::Rect {
            x: rx0,
            y: ry0,
            w,
            h,
            rx,
        } => {
            // SDF ของสี่เหลี่ยมมุมมน — `rx = 0` ได้สี่เหลี่ยมธรรมดาโดยอัตโนมัติ
            let (cx, cy) = (rx0 + w / 2.0, ry0 + h / 2.0);
            let rx = rx.min(w / 2.0).min(h / 2.0);
            let qx = (x - cx).abs() - (w / 2.0 - rx);
            let qy = (y - cy).abs() - (h / 2.0 - rx);
            let outside = qx.max(0.0).hypot(qy.max(0.0));
            outside + qx.max(qy).min(0.0) - rx <= 0.0
        }
        Shape::Polygon(points) => {
            // ray casting แนวนอน — รูปของเราเป็น simple polygon จึงพอ
            let mut inside = false;
            let mut j = points.len() - 1;
            for i in 0..points.len() {
                let (xi, yi) = points[i];
                let (xj, yj) = points[j];
                if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                    inside = !inside;
                }
                j = i;
            }
            inside
        }
    }
}

fn unpremultiply(r: f64, g: f64, b: f64, a: f64) -> (u8, u8, u8, u8) {
    if a <= 0.0 {
        return (0, 0, 0, 0);
    }
    let to8 = |v: f64| (v / a * 255.0).round().clamp(0.0, 255.0) as u8;
    (
        to8(r),
        to8(g),
        to8(b),
        (a * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

// ───────────────────────── เขียนไฟล์ ─────────────────────────

fn encode_png(rgba: &[u8], size: u32) -> anyhow::Result<Vec<u8>> {
    let buffer = image::RgbaImage::from_raw(size, size, rgba.to_vec())
        .ok_or_else(|| anyhow::anyhow!("บัฟเฟอร์ {} ไบต์ ไม่พอดีกับ {size}×{size}", rgba.len()))?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|err| anyhow::anyhow!("encode PNG ไม่สำเร็จ: {err}"))?;
    Ok(out.into_inner())
}

/// ประกอบ `.ico` เอง — `image` ในทรีนี้ไม่ได้เปิด feature `ico` และการเปิดมัน
/// จะลากตัวถอดรหัส ico เข้าไปใน **binary ที่แจก** ด้วย (feature unification)
/// ทั้งที่แอปไม่เคยเปิดไฟล์ ico · รูปแบบนี้เล็กพอที่จะเขียนเองได้ทั้งหมด
fn build_ico(icon: &Icon) -> anyhow::Result<Vec<u8>> {
    let mut images: Vec<Vec<u8>> = Vec::new();
    for &size in ICO_SIZES {
        let rgba = render(icon, size);
        // 256 ใช้ PNG (ที่ .ico รองรับตั้งแต่ Vista) เพราะ DIB 256×256 ใหญ่กว่ามาก
        // ขนาดเล็กใช้ DIB ซึ่งเป็นรูปแบบที่ทุกรุ่นของ Windows อ่านได้แน่นอน
        images.push(if size == 256 {
            encode_png(&rgba, size)?
        } else {
            dib(&rgba, size)
        });
    }

    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type = icon
    let count = u16::try_from(ICO_SIZES.len())?;
    out.extend_from_slice(&count.to_le_bytes());

    let mut offset = 6 + 16 * ICO_SIZES.len();
    for (&size, data) in ICO_SIZES.iter().zip(&images) {
        // 256 เขียนเป็น 0 ตามสเปก (ฟิลด์กว้าง 1 ไบต์)
        let dim = u8::try_from(size % 256).unwrap_or(0);
        out.push(dim);
        out.push(dim);
        out.push(0); // จำนวนสีใน palette — 0 = ไม่ใช้ palette
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&u32::try_from(data.len())?.to_le_bytes());
        out.extend_from_slice(&u32::try_from(offset)?.to_le_bytes());
        offset += data.len();
    }
    for data in &images {
        out.extend_from_slice(data);
    }
    Ok(out)
}

/// BITMAPINFOHEADER + BGRA (ล่างขึ้นบน) + AND mask ว่าง
fn dib(rgba: &[u8], size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mask_row = size.div_ceil(32) * 4; // 1 บิตต่อพิกเซล ปัดขึ้นทีละ 4 ไบต์
    let xor_len = size * size * 4;
    let and_len = mask_row * size;

    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&size.to_le_bytes()); // biWidth
    out.extend_from_slice(&(size * 2).to_le_bytes()); // biHeight = XOR + AND
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    out.extend_from_slice(&(xor_len + and_len).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // biXPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes()); // biYPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    for row in (0..size).rev() {
        for col in 0..size {
            let at = ((row * size + col) * 4) as usize;
            out.push(rgba[at + 2]); // B
            out.push(rgba[at + 1]); // G
            out.push(rgba[at]); // R
            out.push(rgba[at + 3]); // A
        }
    }
    // ช่อง alpha 32 บิตเป็นตัวตัดสินความโปร่งใสอยู่แล้ว AND mask จึงเป็นศูนย์ทั้งผืน
    out.extend(std::iter::repeat_n(0u8, and_len as usize));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_icon() -> Icon {
        let path = repo_root().expect("repo root").join(SVG);
        let text = std::fs::read_to_string(path).expect("อ่าน svg");
        parse(&text).expect("parse svg")
    }

    #[test]
    fn the_shipped_svg_parses_into_every_shape_it_draws() {
        let icon = real_icon();
        assert_eq!(icon.side, 256.0);
        // ฐาน + การ์ดสามใบ + ดวงอาทิตย์ + ภูเขา + swatch
        assert_eq!(icon.shapes.len(), 7, "{:?}", icon.shapes);
        assert!(
            icon.shapes.iter().filter(|s| s.rotate.is_some()).count() == 2,
            "การ์ดสองใบหลังต้องถูกหมุน"
        );
    }

    #[test]
    fn an_unknown_tag_stops_the_tool_instead_of_drawing_something_else() {
        let err = parse(r##"<svg viewBox="0 0 8 8"><ellipse cx="1" cy="1" rx="1" ry="2"/></svg>"##)
            .expect_err("ต้องล้ม");
        assert!(format!("{err}").contains("ellipse"), "{err}");
    }

    #[test]
    fn an_unknown_attribute_stops_the_tool_too() {
        let err =
            parse(r##"<svg viewBox="0 0 8 8"><rect x="0" y="0" width="8" height="8" fill="#000000" opacity="0.5"/></svg>"##)
                .expect_err("ต้องล้ม");
        assert!(format!("{err}").contains("opacity"), "{err}");
    }

    #[test]
    fn a_curve_in_the_path_stops_the_tool() {
        let err = parse(
            r##"<svg viewBox="0 0 8 8"><path d="M0 0 C1 1 2 2 3 3 Z" fill="#000000"/></svg>"##,
        )
        .expect_err("ต้องล้ม");
        assert!(format!("{err}").contains("C1"), "{err}");
    }

    #[test]
    fn the_rounded_corner_stays_transparent_and_the_middle_is_the_front_card() {
        let icon = real_icon();
        let size = 256u32;
        let px = render(&icon, size);
        let at = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            (px[i], px[i + 1], px[i + 2], px[i + 3])
        };
        assert_eq!(at(0, 0).3, 0, "มุมบนซ้ายต้องโปร่งใส — ฐานมี rx=56");
        assert_eq!(at(128, 128), (0xF2, 0xF4, 0xF8, 255), "กลางภาพคือการ์ดใบหน้า");
        assert_eq!(at(128, 8).3, 255, "ขอบบนกลางภาพต้องทึบ");
    }

    /// ★★★ สารบัญที่ถูกต้องไม่ได้แปลว่า **เนื้อ** ถูกต้อง
    ///
    /// เทสต์ข้างล่างตรวจว่าสารบัญของ `.ico` ชี้ไปในไฟล์และประกาศขนาดตรง ·
    /// แต่ถ้า DIB ข้างในเป็นคนละขนาดกับที่สารบัญประกาศ Windows จะขึ้นไอคอน
    /// เพี้ยนหรือว่างเปล่าโดยที่สารบัญยัง "ถูก" ทุกประการ — จึงต้องแกะ DIB
    /// กลับมาอ่านหัวของมันเอง (ยืนยันภายนอกแล้วด้วย `System.Drawing.Icon`
    /// ของ Windows ที่อ่าน entry 16×16 ออกมาได้จริง 14 ก.ย. 2026)
    #[test]
    fn the_bytes_inside_each_small_entry_really_are_a_dib_of_that_size() {
        let ico = build_ico(&real_icon()).expect("สร้าง ico");
        for (n, &size) in ICO_SIZES.iter().enumerate() {
            if size == 256 {
                continue; // 256 เก็บเป็น PNG ตามสเปก ไม่ใช่ DIB
            }
            let entry = 6 + 16 * n;
            let at = |off: usize| {
                u32::from_le_bytes([
                    ico[entry + off],
                    ico[entry + off + 1],
                    ico[entry + off + 2],
                    ico[entry + off + 3],
                ])
            };
            let start = at(12) as usize;
            let dib = &ico[start..start + at(8) as usize];
            let word = |off: usize| {
                u32::from_le_bytes([dib[off], dib[off + 1], dib[off + 2], dib[off + 3]])
            };
            assert_eq!(word(0), 40, "BITMAPINFOHEADER ของรายการที่ {n} ต้องยาว 40");
            assert_eq!(word(4), size, "biWidth ของรายการที่ {n} ไม่ตรงกับที่สารบัญประกาศ");
            assert_eq!(
                word(8),
                size * 2,
                "biHeight ต้องเป็นสองเท่า (XOR + AND) — Windows อ่านครึ่งล่างเป็น mask"
            );
            assert_eq!(
                u32::from(u16::from_le_bytes([dib[14], dib[15]])),
                32,
                "ต้องเป็น 32 bpp"
            );
            assert_eq!(word(16), 0, "biCompression ต้องเป็น BI_RGB");
        }
    }

    #[test]
    fn every_size_in_the_ico_is_declared_where_windows_looks_for_it() {
        let ico = build_ico(&real_icon()).expect("สร้าง ico");
        assert_eq!(&ico[0..2], &[0, 0], "reserved");
        assert_eq!(&ico[2..4], &[1, 0], "type = icon");
        assert_eq!(ico[4] as usize, ICO_SIZES.len());
        for (n, &size) in ICO_SIZES.iter().enumerate() {
            let entry = 6 + 16 * n;
            let declared = u32::from(ico[entry]);
            assert_eq!(declared, size % 256, "รายการที่ {n} ประกาศขนาดผิด");
            let len = u32::from_le_bytes([
                ico[entry + 8],
                ico[entry + 9],
                ico[entry + 10],
                ico[entry + 11],
            ]);
            let offset = u32::from_le_bytes([
                ico[entry + 12],
                ico[entry + 13],
                ico[entry + 14],
                ico[entry + 15],
            ]) as usize;
            assert!(offset + len as usize <= ico.len(), "รายการที่ {n} ชี้ออกนอกไฟล์");
        }
    }

    #[test]
    fn the_window_icon_blob_is_exactly_what_winit_expects() {
        let icon = real_icon();
        let rgba = render(&icon, WINDOW_ICON);
        assert_eq!(rgba.len(), (WINDOW_ICON * WINDOW_ICON * 4) as usize);
    }
}
