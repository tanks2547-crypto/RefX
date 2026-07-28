//! ★ ประตูเดียวของข้อความที่ผู้ใช้เห็น — ห้ามเขียนสตริงตรงใน widget
//!
//! spec: docs/03-modes-and-ui.md §0 (ตัดสิน 28 ก.ค. 2026)
//!
//! กลุ่มเป้าหมายคือนักวาดหลายประเทศ **UI หลักเป็นอังกฤษ ไทยเป็นภาษาที่สอง**
//! ยังไม่รองรับ CJK (ฟอนต์ครบชุด 16+ MB ทะลุเพดาน binary 25 MB — docs/08 §6)
//!
//! ตอนนี้ข้างในเป็นแค่ `match` ธรรมดา **ยังไม่เพิ่ม dependency** ประเด็นคือ
//! **มีช่องต่อ** — พอถึงเวลาทำระบบเต็มรูปแบบจะเป็นงานเชิงกล ไม่ใช่การรื้อทั้งโปรแกรม
//!
//! ### กฎที่บังคับด้วยชนิดข้อมูล ไม่ใช่ด้วยวินัย
//!
//! [`th`] คืน `Option<&str>` → คำแปลที่ยังไม่มี **ตกกลับเป็นอังกฤษเอง**
//! เป็นไปไม่ได้เลยที่ผู้ใช้จะเห็น key หรือช่องว่าง เพราะไม่มีทางเขียนโค้ดแบบนั้นได้
//!
//! ### ข้อความ error
//!
//! `#[error(…)]` ของ `thiserror` เป็น format string ตอนคอมไพล์ → แปลตอนรันไม่ได้
//! จึงแยกบทบาท: `#[error(…)]` เป็น **อังกฤษสำหรับ log/นักพัฒนา** (ค้นหาง่าย
//! ผู้ใช้ส่ง log มาให้เราอ่านได้) ส่วนข้อความที่ผู้ใช้เห็นประกอบขึ้นที่นี่
//! **จากฟิลด์ของ error** แล้วแปลตามภาษา (ดู [`load_error`], [`job_failure`])

use refx_asset::decode::LoadError;
use refx_asset::pool::JobFailure;
use refx_render::atlas::AtlasError;

/// ภาษาของ UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    /// อังกฤษ — ภาษาหลัก และเป็นตัวสำรองเสมอเมื่อคำแปลไม่ครบ
    #[default]
    En,
    /// ไทย
    Th,
}

impl Lang {
    /// เลือกภาษาจากแท็กของ OS เช่น `"th-TH"` `"en_US.UTF-8"`
    ///
    /// ไม่รู้จัก → [`Lang::En`] (docs/03 §0 ข้อ 3)
    #[must_use]
    pub fn from_tag(tag: &str) -> Self {
        match refx_platform::locale::primary_language(tag).as_str() {
            "th" => Self::Th,
            _ => Self::En,
        }
    }

    /// ภาษาที่ควรใช้บนเครื่องนี้ — อ่านจาก OS ครั้งเดียวตอนเปิดโปรแกรม
    #[must_use]
    pub fn from_system() -> Self {
        let lang = refx_platform::locale::user_language_tag()
            .as_deref()
            .map_or(Self::En, Self::from_tag);
        tracing::info!(?lang, "เลือกภาษาของ UI");
        lang
    }
}

/// รหัสของข้อความคงที่ทุกตัวที่ผู้ใช้เห็น
///
/// เพิ่มรายการใหม่ที่นี่เท่านั้น — คอมไพเลอร์จะบังคับให้ไปเติมข้อความอังกฤษให้ครบเอง
/// (`match` ใน [`en`] ไม่มี `_ =>` โดยตั้งใจ)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// สถานะเริ่มต้นบน status bar
    Ready,
    /// เปิด cache ไม่ได้ แต่โปรแกรมยังใช้งานได้
    RunningWithoutCache,
    /// ชื่อ board ที่ยังไม่ได้ตั้งชื่อ
    UntitledBoard,
    /// tooltip ของปุ่มเปิด board ใหม่
    NewBoardHint,
    /// หัวข้อ panel ซ้าย
    Library,
    /// คำอธิบายในช่อง library ที่ยังว่าง
    LibraryPlaceholder,
    /// คำใบ้ว่าลากไฟล์เข้ามาได้
    LibraryDropHint,
    /// หัวข้อ panel ขวา
    Inspector,
    /// รายการคุณสมบัติของ Canvas mode
    InspectorCanvasGeometry,
    /// รายการคุณสมบัติของ Canvas mode (ต่อ)
    InspectorCanvasTransform,
    /// รายการคุณสมบัติของ Arrange mode
    InspectorArrangeMeta,
    /// รายการคุณสมบัติของ Arrange mode (ต่อ)
    InspectorArrangeGroup,
    /// เครื่องมือ: เลือก
    ToolSelect,
    /// เครื่องมือ: ย้าย
    ToolMove,
    /// เครื่องมือ: ครอป
    ToolCrop,
    /// เครื่องมือ: ขาวดำ
    ToolGrayscale,
    /// เครื่องมือ: เรียง
    ToolSort,
    /// เครื่องมือ: กรอง
    ToolFilter,
    /// เครื่องมือ: ติดแท็ก
    ToolTag,
    /// เครื่องมือ: ส่งภาพเข้า canvas
    ToolSendToCanvas,
}

/// ข้อความภาษาอังกฤษ — **ต้องมีครบทุก key เสมอ** (เป็นตัวสำรองสุดท้าย)
fn en(key: Key) -> &'static str {
    match key {
        Key::Ready => "Ready",
        Key::RunningWithoutCache => "Running without a thumbnail cache",
        Key::UntitledBoard => "Untitled board",
        Key::NewBoardHint => "New board (P4-7)",
        Key::Library => "Library",
        Key::LibraryPlaceholder => "Image folders appear here",
        Key::LibraryDropHint => "P1-8: drag images in",
        Key::Inspector => "Inspector",
        Key::InspectorCanvasGeometry => "X / Y / W / H",
        Key::InspectorCanvasTransform => "Rotation, opacity, crop",
        Key::InspectorArrangeMeta => "Tags, rating, colour label",
        Key::InspectorArrangeGroup => "Group, notes",
        Key::ToolSelect => "Select",
        Key::ToolMove => "Move",
        Key::ToolCrop => "Crop",
        Key::ToolGrayscale => "Grayscale",
        Key::ToolSort => "Sort",
        Key::ToolFilter => "Filter",
        Key::ToolTag => "Tag",
        Key::ToolSendToCanvas => "Send to Canvas",
    }
}

/// ข้อความภาษาไทย — **ไม่ครบก็ได้** ตัวที่ยังไม่มีคืน `None` แล้วตกกลับเป็นอังกฤษ
fn th(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::Ready => "พร้อมใช้งาน",
        Key::RunningWithoutCache => "ใช้งานได้ แต่ไม่มี cache ภาพย่อ",
        Key::UntitledBoard => "board ที่ยังไม่ได้ตั้งชื่อ",
        Key::NewBoardHint => "เปิด board ใหม่ (P4-7)",
        Key::Library => "คลังภาพ",
        Key::LibraryPlaceholder => "โฟลเดอร์ภาพจะมาอยู่ตรงนี้",
        Key::LibraryDropHint => "P1-8: ลากไฟล์เข้ามาได้",
        Key::Inspector => "รายละเอียด",
        Key::InspectorCanvasGeometry => "X / Y / กว้าง / สูง",
        Key::InspectorCanvasTransform => "หมุน, ความทึบ, ครอป",
        Key::InspectorArrangeMeta => "แท็ก, เรตติ้ง, ป้ายสี",
        Key::InspectorArrangeGroup => "กลุ่ม, โน้ต",
        Key::ToolSelect => "เลือก",
        Key::ToolMove => "ย้าย",
        Key::ToolCrop => "ครอป",
        Key::ToolGrayscale => "ขาวดำ",
        Key::ToolSort => "เรียง",
        Key::ToolFilter => "กรอง",
        Key::ToolTag => "ติดแท็ก",
        Key::ToolSendToCanvas => "ส่งเข้า Canvas",
    })
}

/// ★ กติกาตกกลับ — จุดเดียวที่ตัดสินว่าจะแสดงอะไรเมื่อคำแปลยังไม่มี
///
/// แยกเป็นฟังก์ชันเพื่อให้ **ทดสอบเส้นทางตกกลับได้จริง** ไม่ใช่แค่เชื่อว่ามันทำงาน
#[must_use]
fn pick(translated: Option<&'static str>, fallback: &'static str) -> &'static str {
    translated.unwrap_or(fallback)
}

/// ข้อความคงที่ตามภาษาที่เลือก
///
/// คำแปลที่ยังไม่มีจะได้อังกฤษกลับไป — **ไม่มีทางได้ key หรือช่องว่าง**
#[must_use]
pub fn t(lang: Lang, key: Key) -> &'static str {
    match lang {
        Lang::En => en(key),
        Lang::Th => pick(th(key), en(key)),
    }
}

/// เทมเพลตที่มีตัวแปร — ค่าที่ใส่ได้มีอะไรบ้างเขียนไว้ในคอมเมนต์ของแต่ละตัว
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// `{n}` — จำนวน item บน board
    ItemCount,
    /// `{pct}` — ระดับซูมเป็นเปอร์เซ็นต์
    Zoom,
    /// `{n}` — จำนวนเฟรมที่วาดไปแล้ว (ตัวชี้วัด I-1 ที่เห็นด้วยตา)
    FramesDrawn,
    /// `{used}` `{limit}` — RAM ของ decode pool
    Ram,
    /// `{used}` `{limit}` — VRAM ของ texture
    Vram,
    /// `{n}` `{size}` — จำนวนภาพใน cache และขนาดไฟล์ DB
    CacheSummary,
    /// `{n}` — งาน decode ที่ยังค้างคิว
    DecodeQueued,
    /// `{n}` — งาน decode ที่ถูกยกเลิกไปแล้ว
    DecodeCancelled,
    /// `{used}` `{limit}` `{calls}` `{evicted}` — working texture ชั้น B (docs/04 §4)
    WorkingTextures,
    /// `{done}` `{total}` — ความคืบหน้าการโหลด (docs/05 §6 เงื่อนไขข้อ 3)
    Loading,
    /// `{n}` — กำลังเปิดไฟล์กี่ไฟล์
    OpeningFiles,
    /// `{n}` `{ms}` — สรุปเวลาหลังเปิดไฟล์ครบ
    OpenedFiles,
    /// `{mode}` — สลับโหมดแล้ว
    SwitchedMode,
    /// `{what}` `{when}` — ฟีเจอร์ที่ยังไม่ได้ทำ
    NotImplemented,
    /// `{mb}` `{limit}` — ไฟล์ใหญ่เกินเพดาน
    ErrFileTooLarge,
    /// `{w}` `{h}` `{pixels}` `{limit}` — ภาพมี pixel เกินเพดาน
    ErrImageTooLarge,
    /// ไม่รู้จักชนิดไฟล์
    ErrUnknownFormat,
    /// `{format}` — รู้จัก format แต่ยังเปิดไม่ได้
    ErrFormatNotAllowed,
    /// header อ่านไม่ได้
    ErrBadHeader,
    /// decoder พัง แต่ถูกดักไว้แล้ว (I-7)
    ErrDecoderPanic,
    /// decode ล้มด้วยสาเหตุปกติ
    ErrDecode,
    /// `{file}` — อ่านไฟล์จากดิสก์ไม่ได้
    ErrReadFile,
    /// `{file}` — ที่อยู่นี้ไม่ใช่ไฟล์
    ErrNotAFile,
    /// `{file}` `{seconds}` — ใช้เวลานานเกินเพดาน
    ErrTimeout,
    /// `{layers}` — atlas เต็ม
    ErrAtlasFull,
    /// `{requested}` `{used}` `{limit}` — VRAM ไม่พอ
    ErrOutOfVram,
}

/// เทมเพลตภาษาอังกฤษ — ต้องมีครบทุกตัว
fn template_en(template: Template) -> &'static str {
    match template {
        Template::ItemCount => "{n} items",
        Template::Zoom => "Zoom {pct}%",
        Template::FramesDrawn => "Frames {n}",
        Template::Ram => "RAM {used} / {limit}",
        Template::Vram => "VRAM {used} / {limit}",
        Template::CacheSummary => "Cache {n} images ({size})",
        Template::DecodeQueued => "Decode queue {n}",
        Template::DecodeCancelled => "Cancelled {n}",
        Template::WorkingTextures => "Sharp {used} / {limit} · {calls} draws · {evicted} evicted",
        Template::Loading => "Loading {done} / {total}",
        Template::OpeningFiles => "Opening {n} files…",
        Template::OpenedFiles => "Opened {n} files in {ms} ms",
        Template::SwitchedMode => "Switched to {mode} mode",
        Template::NotImplemented => "{what} is not available yet — planned for {when}",
        Template::ErrFileTooLarge => {
            "This file is too large ({mb} MB, the limit is {limit} MB)\n\
             Try shrinking the image before adding it."
        }
        Template::ErrImageTooLarge => {
            "This image is too large ({w}×{h} = {pixels} pixels, the limit is {limit})\n\
             If you expected a smaller image, the file may be damaged or altered."
        }
        Template::ErrUnknownFormat => {
            "RefX does not recognise this file type\n\
             It can open PNG, JPEG, WebP, GIF, BMP, TGA and TIFF."
        }
        Template::ErrFormatNotAllowed => {
            "RefX cannot open {format} files yet\nTry converting the image to PNG or JPEG."
        }
        Template::ErrBadHeader => {
            "The file header could not be read — the file is damaged or incomplete
             Try opening it in the program that made it and saving a fresh copy."
        }
        Template::ErrDecoderPanic => {
            "This file broke the image decoder — it was skipped.\nEvery other image still works."
        }
        Template::ErrDecode => {
            "This image could not be opened — the file may be damaged or incomplete\n\
             Try opening it in the program that made it and saving a fresh copy."
        }
        Template::ErrReadFile => {
            "Could not read {file}\n\
             If it lives on OneDrive or Dropbox, wait for that folder to finish syncing."
        }
        Template::ErrNotAFile => {
            "{file} is not an image file
Drag in a PNG, JPEG, WebP, GIF, BMP, TGA or TIFF instead."
        }
        Template::ErrTimeout => {
            "Opening {file} took longer than {seconds} seconds — it was skipped for now\n\
             If the file is on a network or cloud drive, copy it to this computer first."
        }
        Template::ErrAtlasFull => {
            "The thumbnail storage is full (all {layers} layers are in use)\n\
             Close a board you are not using, or raise the memory limit in Settings."
        }
        Template::ErrOutOfVram => {
            "The graphics card is out of memory ({requested} MB requested, {used} MB of {limit} MB in use)\n\
             Close a board you are not using, or raise the memory limit in Settings."
        }
    }
}

/// เทมเพลตภาษาไทย — ไม่ครบก็ได้ ตัวที่ยังไม่มีตกกลับเป็นอังกฤษ
fn template_th(template: Template) -> Option<&'static str> {
    Some(match template {
        Template::ItemCount => "{n} รายการ",
        Template::Zoom => "ซูม {pct}%",
        Template::FramesDrawn => "เฟรมที่วาด {n}",
        Template::Ram => "RAM {used} / {limit}",
        Template::Vram => "VRAM {used} / {limit}",
        Template::CacheSummary => "cache {n} ภาพ ({size})",
        Template::DecodeQueued => "คิวถอดรหัส {n}",
        Template::DecodeCancelled => "ยกเลิกไป {n}",
        Template::WorkingTextures => "ภาพคม {used} / {limit} · วาด {calls} ครั้ง · ไล่ออก {evicted}",
        Template::Loading => "กำลังโหลด {done} / {total}",
        Template::OpeningFiles => "กำลังเปิด {n} ไฟล์…",
        Template::OpenedFiles => "เปิด {n} ไฟล์ใน {ms} ms",
        Template::SwitchedMode => "สลับไปโหมด {mode}",
        Template::NotImplemented => "{what} ยังทำไม่ได้ — รอ {when}",
        Template::ErrFileTooLarge => {
            "ไฟล์ใหญ่เกินไป ({mb} MB, รับได้ไม่เกิน {limit} MB)\n\
             ลองย่อภาพก่อนแล้วค่อยลากเข้ามาใหม่"
        }
        Template::ErrImageTooLarge => {
            "ภาพใหญ่เกินไป ({w}×{h} = {pixels} จุด, รับได้ไม่เกิน {limit} จุด)\n\
             ถ้าไฟล์นี้ควรจะเล็กกว่านี้ แปลว่าไฟล์อาจเสียหายหรือถูกดัดแปลงมา"
        }
        Template::ErrUnknownFormat => {
            "ไม่รู้จักชนิดของไฟล์นี้\nRefX เปิดได้เฉพาะ PNG, JPEG, WebP, GIF, BMP, TGA และ TIFF"
        }
        Template::ErrFormatNotAllowed => {
            "RefX ยังเปิดไฟล์ชนิด {format} ไม่ได้\nลองแปลงเป็น PNG หรือ JPEG ก่อน"
        }
        Template::ErrBadHeader => {
            "อ่านข้อมูลหัวไฟล์ไม่ได้ — ไฟล์น่าจะเสียหายหรือถูกตัดไม่ครบ
             ลองเปิดด้วยโปรแกรมที่สร้างไฟล์นี้แล้วบันทึกใหม่อีกครั้ง"
        }
        Template::ErrDecoderPanic => {
            "ไฟล์นี้ทำให้ตัวถอดรหัสภาพทำงานผิดพลาด — ข้ามไฟล์นี้ไป\nไฟล์อื่นยังใช้ได้ตามปกติ"
        }
        Template::ErrDecode => {
            "เปิดภาพไม่ได้ — ไฟล์อาจเสียหายหรือถูกตัดไม่ครบ
             ลองเปิดด้วยโปรแกรมที่สร้างไฟล์นี้แล้วบันทึกใหม่อีกครั้ง"
        }
        Template::ErrReadFile => {
            "อ่านไฟล์ {file} ไม่ได้\n\
             ถ้าไฟล์อยู่บน OneDrive หรือ Dropbox ลองเปิดโฟลเดอร์นั้นให้ sync เสร็จก่อน"
        }
        Template::ErrNotAFile => {
            "{file} ไม่ใช่ไฟล์ภาพ
ลากไฟล์ PNG, JPEG, WebP, GIF, BMP, TGA หรือ TIFF เข้ามาแทน"
        }
        Template::ErrTimeout => {
            "ใช้เวลาเปิดภาพ {file} นานเกิน {seconds} วินาที — ข้ามไฟล์นี้ไปก่อน\n\
             ถ้าไฟล์อยู่บนไดรฟ์เครือข่ายหรือ cloud ลองคัดลอกมาไว้ในเครื่องก่อน"
        }
        Template::ErrAtlasFull => {
            "พื้นที่เก็บภาพย่อเต็ม (ใช้ครบ {layers} ชั้นแล้ว)\n\
             ลองปิด board ที่ไม่ได้ใช้ หรือเพิ่มเพดานหน่วยความจำในการตั้งค่า"
        }
        Template::ErrOutOfVram => {
            "หน่วยความจำการ์ดจอไม่พอ (ขอ {requested} MB, ใช้อยู่ {used} MB จากเพดาน {limit} MB)\n\
             ลองปิด board ที่ไม่ได้ใช้ หรือเพิ่มเพดานหน่วยความจำในการตั้งค่า"
        }
    })
}

/// เทมเพลตตามภาษาที่เลือก (ตกกลับเป็นอังกฤษเหมือน [`t`])
#[must_use]
fn template(lang: Lang, template: Template) -> &'static str {
    match lang {
        Lang::En => template_en(template),
        Lang::Th => pick(template_th(template), template_en(template)),
    }
}

/// เติมค่าลงตัวยึดตำแหน่งของเทมเพลต
///
/// ใช้แทน `format!` เพราะ `format!` ต้องการ literal ตอนคอมไพล์ ซึ่งแปลตอนรันไม่ได้
/// วิธีนี้ทำให้ **ผู้แปลแตะแค่สตริง** และสลับลำดับคำได้ตามไวยากรณ์ของแต่ละภาษา
///
/// ตัวยึดที่ไม่มีใครเติมจะถูกทิ้งไว้ตามเดิม — เห็นได้ทันทีตอนทดสอบ ดีกว่าหายเงียบ
#[must_use]
pub fn fill(lang: Lang, which: Template, args: &[(&str, &str)]) -> String {
    let mut out = template(lang, which).to_owned();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// ข้อความสำหรับผู้ใช้เมื่อเปิดภาพไม่สำเร็จ
///
/// ประกอบจาก **ฟิลด์** ของ error ไม่ใช่จาก `Display` ของมัน — `Display`
/// เป็นภาษาอังกฤษสำหรับ log เสมอ (docs/03 §0)
#[must_use]
pub fn load_error(lang: Lang, err: &LoadError) -> String {
    match err {
        LoadError::FileTooLarge {
            actual_mb,
            limit_mb,
        } => fill(
            lang,
            Template::ErrFileTooLarge,
            &[
                ("mb", &actual_mb.to_string()),
                ("limit", &limit_mb.to_string()),
            ],
        ),
        LoadError::ImageTooLarge {
            width,
            height,
            pixels,
            limit,
        } => fill(
            lang,
            Template::ErrImageTooLarge,
            &[
                ("w", &width.to_string()),
                ("h", &height.to_string()),
                ("pixels", &pixels.to_string()),
                ("limit", &limit.to_string()),
            ],
        ),
        LoadError::UnknownFormat => fill(lang, Template::ErrUnknownFormat, &[]),
        LoadError::FormatNotAllowed { format } => fill(
            lang,
            Template::ErrFormatNotAllowed,
            &[("format", &format!("{format:?}"))],
        ),
        LoadError::BadHeader => fill(lang, Template::ErrBadHeader, &[]),
        LoadError::DecoderPanic => fill(lang, Template::ErrDecoderPanic, &[]),
        LoadError::Decode(_) => fill(lang, Template::ErrDecode, &[]),
        LoadError::Io { file, .. } => fill(lang, Template::ErrReadFile, &[("file", file)]),
        LoadError::NotAFile { file } => fill(lang, Template::ErrNotAFile, &[("file", file)]),
    }
}

/// ข้อความสำหรับผู้ใช้เมื่องาน decode ล้มเหลว
#[must_use]
pub fn job_failure(lang: Lang, err: &JobFailure) -> String {
    match err {
        JobFailure::Load(inner) => load_error(lang, inner),
        JobFailure::Timeout { file, seconds } => fill(
            lang,
            Template::ErrTimeout,
            &[("file", file), ("seconds", &seconds.to_string())],
        ),
    }
}

/// ข้อความสำหรับผู้ใช้เมื่อเก็บภาพย่อลง atlas ไม่ได้
#[must_use]
pub fn atlas_error(lang: Lang, err: &AtlasError) -> String {
    match err {
        // ★ `NeedsResize` เป็นสัญญาณควบคุมภายใน — ชั้น UI จัดการเองหมดแล้ว
        //   (`RefxApp::upload_thumb` ขยาย atlas แล้วเติมของเดิมกลับให้)
        //   ถ้ามาถึงตรงนี้ได้แปลว่ามีเส้นทางที่ลืมจัดการ จึงบอกผู้ใช้แบบเดียวกับ
        //   "เต็ม" เพราะสิ่งที่เขาเห็นเหมือนกัน คือภาพนี้ขึ้นเป็นสี่เหลี่ยมสีเด่นแทน
        AtlasError::Full { layers } | AtlasError::NeedsResize { layers } => fill(
            lang,
            Template::ErrAtlasFull,
            &[("layers", &layers.to_string())],
        ),
        AtlasError::OutOfVram(refx_render::texture::VramError::OverBudget {
            requested_mb,
            used_mb,
            limit_mb,
        }) => fill(
            lang,
            Template::ErrOutOfVram,
            &[
                ("requested", &requested_mb.to_string()),
                ("used", &used_mb.to_string()),
                ("limit", &limit_mb.to_string()),
            ],
        ),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// รายการ key ทั้งหมด — ต้องเติมเมื่อเพิ่ม key ใหม่
    const ALL_KEYS: &[Key] = &[
        Key::Ready,
        Key::RunningWithoutCache,
        Key::UntitledBoard,
        Key::NewBoardHint,
        Key::Library,
        Key::LibraryPlaceholder,
        Key::LibraryDropHint,
        Key::Inspector,
        Key::InspectorCanvasGeometry,
        Key::InspectorCanvasTransform,
        Key::InspectorArrangeMeta,
        Key::InspectorArrangeGroup,
        Key::ToolSelect,
        Key::ToolMove,
        Key::ToolCrop,
        Key::ToolGrayscale,
        Key::ToolSort,
        Key::ToolFilter,
        Key::ToolTag,
        Key::ToolSendToCanvas,
    ];

    const ALL_TEMPLATES: &[Template] = &[
        Template::ItemCount,
        Template::Zoom,
        Template::FramesDrawn,
        Template::Ram,
        Template::Vram,
        Template::CacheSummary,
        Template::DecodeQueued,
        Template::DecodeCancelled,
        Template::WorkingTextures,
        Template::Loading,
        Template::OpeningFiles,
        Template::OpenedFiles,
        Template::SwitchedMode,
        Template::NotImplemented,
        Template::ErrFileTooLarge,
        Template::ErrImageTooLarge,
        Template::ErrUnknownFormat,
        Template::ErrFormatNotAllowed,
        Template::ErrBadHeader,
        Template::ErrDecoderPanic,
        Template::ErrDecode,
        Template::ErrReadFile,
        Template::ErrNotAFile,
        Template::ErrTimeout,
        Template::ErrAtlasFull,
        Template::ErrOutOfVram,
    ];

    /// ★ กฎข้อ 2 ของ docs/03 §0: ห้ามมีทางที่ผู้ใช้จะเห็นช่องว่างหรือชื่อ key
    #[test]
    fn no_string_is_ever_empty_in_either_language() {
        for &lang in &[Lang::En, Lang::Th] {
            for &key in ALL_KEYS {
                assert!(!t(lang, key).trim().is_empty(), "{lang:?} {key:?} ว่างเปล่า");
            }
            for &tpl in ALL_TEMPLATES {
                assert!(
                    !template(lang, tpl).trim().is_empty(),
                    "{lang:?} {tpl:?} ว่างเปล่า"
                );
            }
        }
    }

    /// ★ สลับภาษาแล้วต้องได้คนละข้อความจริง ไม่ใช่คืนอังกฤษทั้งคู่
    #[test]
    fn switching_language_actually_changes_the_text() {
        // ถ้าคำแปลไทยหายไปทั้งชุด เทสต์นี้จะล้มทันที
        let changed = ALL_KEYS
            .iter()
            .filter(|&&key| t(Lang::En, key) != t(Lang::Th, key))
            .count();
        assert!(
            changed >= ALL_KEYS.len() / 2,
            "แปลไทยแค่ {changed} จาก {} key",
            ALL_KEYS.len()
        );

        assert_eq!(t(Lang::En, Key::Ready), "Ready");
        assert_eq!(t(Lang::Th, Key::Ready), "พร้อมใช้งาน");
    }

    /// ★ คำแปลที่ยังไม่มีต้องตกกลับเป็นอังกฤษ — ห้ามได้ key ห้ามได้ช่องว่าง
    ///
    /// จำลองด้วยการเรียกเส้นทาง fallback ตรง ๆ เพราะตอนนี้แปลครบทุกตัวแล้ว
    #[test]
    fn missing_translation_falls_back_to_english() {
        // `pick` คือกติกาเดียวกับที่ `t` และ `template` ใช้จริง ไม่ใช่โค้ดจำลอง
        assert_eq!(pick(None, en(Key::Ready)), "Ready");
        assert_eq!(pick(Some("พร้อมใช้งาน"), en(Key::Ready)), "พร้อมใช้งาน");
        assert_eq!(
            pick(None, template_en(Template::Loading)),
            "Loading {done} / {total}"
        );

        // ★ สิ่งที่ห้ามเกิดเด็ดขาด: ช่องว่างหรือชื่อ key โผล่ใส่หน้าผู้ใช้
        assert!(!pick(None, en(Key::Ready)).is_empty());
        assert!(!pick(None, en(Key::Ready)).contains("Key::"));
    }

    #[test]
    fn fill_replaces_every_placeholder() {
        let text = fill(
            Lang::En,
            Template::Loading,
            &[("done", "312"), ("total", "1000")],
        );
        assert_eq!(text, "Loading 312 / 1000");
        assert!(!text.contains('{'), "ยังมีตัวยึดตำแหน่งเหลืออยู่: {text}");

        let thai = fill(
            Lang::Th,
            Template::Loading,
            &[("done", "312"), ("total", "1000")],
        );
        assert_eq!(thai, "กำลังโหลด 312 / 1000");
    }

    /// เทมเพลตทุกตัวต้องถูกเติมจนไม่เหลือตัวยึดตำแหน่ง เมื่อผู้เรียกส่งค่าครบ
    ///
    /// จับกรณีที่แปลไทยแล้วพิมพ์ชื่อตัวแปรผิด เช่น `{total}` เป็น `{totol}`
    /// ซึ่งจะโผล่เป็นข้อความประหลาดใส่หน้าผู้ใช้
    #[test]
    fn thai_templates_use_the_same_placeholders_as_english() {
        for &tpl in ALL_TEMPLATES {
            let names = |text: &str| {
                let mut found: Vec<String> = text
                    .split('{')
                    .skip(1)
                    .filter_map(|rest| rest.split('}').next().map(ToOwned::to_owned))
                    .collect();
                found.sort();
                found
            };
            assert_eq!(
                names(template_en(tpl)),
                names(template(Lang::Th, tpl)),
                "{tpl:?} ใช้ตัวยึดตำแหน่งไม่ตรงกันระหว่างสองภาษา"
            );
        }
    }

    // ---------- ข้อความ error ที่ประกอบจากฟิลด์ ----------

    /// ★ ข้อความที่ผู้ใช้เห็นต้องมี **ตัวเลขจริงจากฟิลด์** ไม่ใช่ข้อความลอย ๆ
    /// และต้องบอก "ทำอะไรต่อได้" ตามที่ CLAUDE.md บังคับ
    #[test]
    fn image_too_large_shows_real_numbers_and_a_way_out() {
        let err = LoadError::ImageTooLarge {
            width: 65_535,
            height: 65_535,
            pixels: 4_294_836_225,
            limit: 268_435_456,
        };
        for lang in [Lang::En, Lang::Th] {
            let message = load_error(lang, &err);
            assert!(message.contains("65535"), "{lang:?}: {message}");
            assert!(message.contains("268435456"), "{lang:?}: {message}");
            assert!(!message.contains('{'), "{lang:?}: ยังมีตัวยึดเหลือ {message}");
            // บรรทัดที่สองคือ "ทำอะไรต่อได้"
            assert!(message.lines().count() >= 2, "{lang:?}: {message}");
        }
    }

    /// ★ CLAUDE.md: ข้อความที่ผู้ใช้เห็นต้องบอก **สิ่งที่เกิดขึ้น + สิ่งที่ทำได้ต่อ**
    ///
    /// บังคับด้วยเทสต์แทนวินัย — ข้อความ error ทุกตัวต้องมีอย่างน้อยสองบรรทัด
    /// ทั้งสองภาษา ไม่งั้นผู้ใช้รู้แค่ว่า "พัง" แต่ไม่รู้ว่าจะทำอะไรต่อ
    #[test]
    fn every_error_message_tells_the_user_what_to_do_next() {
        let error_templates = ALL_TEMPLATES
            .iter()
            .filter(|tpl| format!("{tpl:?}").starts_with("Err"));
        for &tpl in error_templates {
            for lang in [Lang::En, Lang::Th] {
                let text = template(lang, tpl);
                assert!(
                    text.lines().count() >= 2,
                    "{tpl:?} ({lang:?}) บอกแค่ว่าพัง ไม่ได้บอกว่าทำอะไรต่อได้: {text}"
                );
            }
        }
    }

    #[test]
    fn every_load_error_variant_has_a_user_message() {
        let cases = [
            LoadError::FileTooLarge {
                actual_mb: 600,
                limit_mb: 512,
            },
            LoadError::ImageTooLarge {
                width: 9,
                height: 9,
                pixels: 81,
                limit: 64,
            },
            LoadError::UnknownFormat,
            LoadError::FormatNotAllowed {
                // หยิบจากรายการที่ refx-asset ประกาศไว้ เพื่อไม่ต้องดึง `image`
                // เข้ามาเป็น dependency ของ crate นี้เพียงเพื่อเขียนเทสต์
                format: refx_asset::decode::ALLOWED_FORMATS[0],
            },
            LoadError::BadHeader,
            LoadError::DecoderPanic,
            LoadError::Io {
                file: "cat.png".to_owned(),
                source: std::io::Error::other("x"),
            },
            LoadError::NotAFile {
                file: "folder".to_owned(),
            },
        ];
        for err in &cases {
            for lang in [Lang::En, Lang::Th] {
                let message = load_error(lang, err);
                assert!(!message.trim().is_empty(), "{err:?} ({lang:?}) ว่างเปล่า");
                assert!(!message.contains('{'), "{err:?} ({lang:?}): {message}");
            }
        }
    }

    /// ★ ชื่อไฟล์ต้องไปถึงผู้ใช้ — "เปิดไฟล์ไหนไม่ได้" คือสิ่งแรกที่เขาอยากรู้
    #[test]
    fn io_error_names_the_file() {
        let err = LoadError::Io {
            file: "ภาพอ้างอิง.png".to_owned(),
            source: std::io::Error::other("boom"),
        };
        for lang in [Lang::En, Lang::Th] {
            assert!(load_error(lang, &err).contains("ภาพอ้างอิง.png"));
        }
    }

    #[test]
    fn timeout_message_names_the_file_and_the_limit() {
        let err = JobFailure::Timeout {
            file: "big.tif".to_owned(),
            seconds: 20,
        };
        for lang in [Lang::En, Lang::Th] {
            let message = job_failure(lang, &err);
            assert!(message.contains("big.tif"), "{message}");
            assert!(message.contains("20"), "{message}");
        }
    }

    #[test]
    fn atlas_errors_report_their_numbers() {
        let full = AtlasError::Full { layers: 12 };
        assert!(atlas_error(Lang::En, &full).contains("12"));

        let vram = AtlasError::OutOfVram(refx_render::texture::VramError::OverBudget {
            requested_mb: 192,
            used_mb: 176,
            limit_mb: 384,
        });
        for lang in [Lang::En, Lang::Th] {
            let message = atlas_error(lang, &vram);
            assert!(
                message.contains("192") && message.contains("384"),
                "{message}"
            );
        }
    }

    // ---------- การเลือกภาษาจาก OS ----------

    #[test]
    fn language_is_picked_from_the_os_tag() {
        assert_eq!(Lang::from_tag("th-TH"), Lang::Th);
        assert_eq!(Lang::from_tag("th_TH.UTF-8"), Lang::Th);
        assert_eq!(Lang::from_tag("en-US"), Lang::En);
        // ภาษาที่ยังไม่รองรับต้องได้อังกฤษ ไม่ใช่ค้างหรือ panic
        assert_eq!(Lang::from_tag("ja-JP"), Lang::En);
        assert_eq!(Lang::from_tag("zh-Hans-CN"), Lang::En);
        assert_eq!(Lang::from_tag(""), Lang::En);
        assert_eq!(Lang::from_tag("!!!"), Lang::En);
        assert_eq!(Lang::default(), Lang::En);
    }
}
