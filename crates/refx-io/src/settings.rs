//! `settings.toml` — ค่าที่ผู้ใช้ตั้งเอง (P5-3)
//!
//! spec: `docs/05-memory-and-assets.md §2` (ปรับ limit ได้ใน settings) ·
//! `docs/03-modes-and-ui.md §6` (theme) · `ROADMAP` P5-3
//!
//! ### ★★★ กฎข้อเดียวที่สำคัญกว่าทุกข้อ: **ไฟล์นี้ห้ามทำให้เปิดโปรแกรมไม่ขึ้น**
//!
//! `settings.toml` เป็นไฟล์ที่ผู้ใช้แก้เองด้วยมือได้ ซึ่งแปลว่ามันจะพัง — พิมพ์ผิด
//! วางค่าผิดบรรทัด ก๊อปมาจากอินเทอร์เน็ต หรือถูกตัดกลางคันตอนดิสก์เต็ม
//! ถ้าเราตอบด้วยการปฏิเสธเปิดโปรแกรม ผู้ใช้จะเข้าไม่ถึงงานของตัวเองเพราะ
//! **ไฟล์ที่ไม่ได้เก็บงานของเขาเลยสักไบต์** ซึ่งเป็น I-3 ที่เราเป็นคนก่อเอง
//! (รูปแบบเดียวกับ `.refx` ที่เสียแล้วยังต้องเปิด `.bak` ให้ได้ — `docs/07 §1`)
//!
//! → ทุกทางที่ล้มเหลวจบที่ **ค่าปริยายที่ใช้งานได้จริง** เสมอ
//! → แต่ **ห้ามเงียบ**: ทุกครั้งที่ค่าของผู้ใช้ไม่ถูกใช้ตามที่เขาเขียน จะมี
//!   [`Note`] หนึ่งใบออกไปให้ชั้น UI บอกเขา พร้อมเหตุผล
//!
//! ### ทำไม `Note` เป็น enum ไม่ใช่ `String`
//!
//! ข้อความที่ผู้ใช้เห็นทุกตัวต้องผ่าน `refx-ui::text` เพื่อแปลได้ (`docs/03 §0`)
//! ชั้นนี้จึงรายงาน **สิ่งที่เกิดขึ้นพร้อมตัวเลข** ไม่ใช่ประโยคสำเร็จรูป
//!
//! ### สิ่งที่ชั้นนี้ **ไม่** ทำ
//!
//! ไม่ถามเครื่องว่ามี RAM เท่าไหร่ และไม่รู้จักการ์ดจอ — ทั้งสองอย่างเป็นงานของ
//! `refx-platform`/`refx-render` ผู้เรียกส่งมาให้ทาง [`Caps`] แทน
//! (หลักการเดียวกับ `DecodePool::with_defaults` ที่รับ `total_ram` เข้ามา — `HANDOFF §2.0`)

use std::path::Path;

/// เพดาน RAM ของ decode pool ที่ยังไม่ได้ตั้งอะไร (MB) — `docs/05 §2`
pub const DEFAULT_RAM_LIMIT_MB: u64 = 256;

/// ช่วงที่ยอมให้ตั้ง `memory.ram_limit_mb`
///
/// * **ต่ำสุด 64 MB** — ภาพ 4000×3000 หนึ่งใบใช้ราว 96 MB ตอน decode
///   ([`crate`] ไม่รู้เลขนี้เอง แต่ `refx-asset::budget` ยอมให้งานที่ใหญ่กว่าทั้งถัง
///   รันได้ถ้าอยู่คนเดียว) ต่ำกว่านี้โปรแกรมยังทำงานถูก แค่ decode ทีละใบเสมอ
///   จึงเป็นเพดานของ *ความคุ้ม* ไม่ใช่ของความถูกต้อง
/// * **สูงสุด 4096 MB** — เกินจากนี้ขัดเป้าหมายของโปรแกรมโดยตรง (เปิดค้างทั้งวัน
///   ข้าง Photoshop ห้ามแย่ง RAM) และมันเป็นแค่ **staging ของ decode** ไม่ใช่
///   ที่เก็บภาพ — ให้มากกว่านี้ไม่ได้อะไรเพิ่ม
pub const RAM_LIMIT_MB: std::ops::RangeInclusive<u64> = 64..=4096;

/// ช่วงที่ยอมให้ตั้ง `memory.vram_limit_mb`
///
/// * **ต่ำสุด 64 MB** — atlas ได้ครึ่งหนึ่ง = 32 MB = 2 layer × 256 ช่อง
///   ยังเป็นโปรแกรมที่ใช้งานได้จริง ต่ำกว่านี้ thumbnail จะถูกไล่ออกเร็วจน
///   เลื่อนจอแล้วภาพกระพริบ
/// * **สูงสุด 8192 MB** — การ์ดใหญ่สุดที่พบทั่วไปในตลาด ณ ตอนเขียน
pub const VRAM_LIMIT_MB: std::ops::RangeInclusive<u64> = 64..=8192;

/// จำนวน pixel ต่อภาพที่น้อยที่สุดที่ยอมให้ตั้ง — 1 MP
///
/// ต่ำกว่านี้ภาพถ่ายธรรมดาจะถูกปฏิเสธหมด ซึ่งอ่านได้อย่างเดียวว่าโปรแกรมพัง
pub const MIN_MAX_PIXELS: u64 = 1 << 20;

/// ข้อจำกัดของเครื่องเครื่องนี้ที่ **ผู้เรียกส่งเข้ามา**
///
/// ชั้นนี้ไม่ถามเครื่องเอง (ARCHITECTURE §2) — `refx-ui` เป็นชั้นเดียวที่รู้จักทั้ง
/// OS และค่าพวกนี้
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// RAM ที่ติดตั้งไว้ทั้งเครื่อง (ไบต์) — ใช้บอกผู้ใช้ว่าทำไมถึงถูก clamp
    pub total_ram: u64,
    /// เพดาน pixel ต่อภาพของเครื่องนี้ = `refx_asset::decode::max_pixels_for_ram`
    ///
    /// ★★ **ผู้ใช้ตั้งเกินค่านี้ไม่ได้** (`HANDOFF §4` ข้อ 3) — ภาพ 16384² ขอ RAM
    /// ~2 GiB ตอน decode บนเครื่อง 8 GB ที่เปิด Photoshop อยู่ = OOM = งานหาย
    /// การยอมให้ตั้งเกินคือการเปิดช่องให้ผู้ใช้ทำ I-3 พังด้วยมือตัวเอง
    pub max_pixels_ceiling: u64,
}

/// ธีมของ UI (`docs/03 §6`)
///
/// ★ นี่คือธีมของ **แผงรอบ ๆ** ไม่ใช่สีพื้นหลังของ canvas — สีพื้นหลัง canvas
/// เป็น `BoardSettings::background` ซึ่งเป็นของ *เอกสาร* และ persist ลง `.refx`
/// อยู่แล้ว (`docs/02 §2`) สองอย่างนี้คนละชั้นกันโดยตั้งใจ: ผู้ใช้เปลี่ยนพื้นหลัง
/// เป็นเทากลาง 50% เพื่อเช็คค่า value ของ *ภาพในบอร์ดใบนั้น* ไม่ใช่ของทั้งโปรแกรม
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// ค่าปริยาย — `docs/03 §6` บอกว่าธีมมืดเป็นค่าเริ่มต้น
    #[default]
    Dark,
    /// สว่าง
    Light,
}

impl Theme {
    /// ชื่อที่เขียนลงไฟล์
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// จังหวะการแสดงเฟรม
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Present {
    /// ตามรอบจอ — ค่าปริยาย (`docs/04 §8`)
    #[default]
    Vsync,
    /// ไม่รอรอบจอ — ภาพฉีกได้ แต่ latency ต่ำกว่า
    Uncapped,
}

impl Present {
    /// ชื่อที่เขียนลงไฟล์
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vsync => "vsync",
            Self::Uncapped => "uncapped",
        }
    }
}

/// ★★★ เขียน `.refx-meta` ลงโฟลเดอร์ภาพของผู้ใช้หรือไม่ (P5-5 · `docs/07 §5`)
///
/// **ค่านี้อยู่ที่นี่ ไม่ใช่ในโฟลเดอร์นั้น** — การจำว่าผู้ใช้ตอบ "ไม่"
/// โดยเขียนไฟล์ลงโฟลเดอร์เดียวกัน คือการทำสิ่งที่เขาเพิ่งห้ามไปหมาด ๆ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidecarPolicy {
    /// ถามครั้งแรกที่ผู้ใช้ใส่ tag/rating ในโฟลเดอร์ที่ยังไม่มี `.refx-meta`
    #[default]
    Ask,
    /// เขียนเสมอ ไม่ต้องถาม
    Always,
    /// ไม่เขียนเลย — tag ยังใช้ได้ในเซสชันนี้ แต่หายเมื่อปิดโปรแกรม
    Never,
}

impl SidecarPolicy {
    /// ชื่อที่เขียนลงไฟล์
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

/// ช่องที่ [`Note`] พูดถึง — ชั้น UI แปลชื่อช่องเป็นภาษาผู้ใช้เอง
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// `memory.ram_limit_mb`
    RamLimitMb,
    /// `memory.vram_limit_mb`
    VramLimitMb,
    /// `memory.max_pixels`
    MaxPixels,
    /// `theme`
    Theme,
    /// `present`
    Present,
    /// `sidecar`
    Sidecar,
}

impl Field {
    /// ชื่อช่องอย่างที่เขียนในไฟล์ — ผู้ใช้ต้องหาบรรทัดนั้นเจอ
    #[must_use]
    pub fn path(self) -> &'static str {
        match self {
            Self::RamLimitMb => "memory.ram_limit_mb",
            Self::VramLimitMb => "memory.vram_limit_mb",
            Self::MaxPixels => "memory.max_pixels",
            Self::Theme => "theme",
            Self::Present => "present",
            Self::Sidecar => "sidecar",
        }
    }
}

/// สิ่งที่ต้องบอกผู้ใช้เพราะค่าที่เขาเขียนไม่ได้ถูกใช้ตามนั้น
///
/// ★ ว่างเปล่า = ทุกอย่างที่เขาตั้งถูกใช้จริงครบ · **ไม่มีทางที่ค่าถูกทิ้งเงียบ ๆ**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// อ่านไฟล์ทั้งไฟล์ไม่ได้ (ไม่ใช่ TOML / ชนิดผิด) → **ค่าปริยายทั้งชุด**
    ///
    /// `detail` เป็นข้อความของ parser สำหรับ log — ไม่ใช่ข้อความที่ผู้ใช้เห็น
    Unparsable {
        /// สิ่งที่ parser บ่น (อังกฤษ สำหรับ log)
        detail: String,
    },
    /// มีคีย์ที่ RefX ไม่รู้จัก — ถูกข้ามไป ที่เหลือยังใช้ได้ตามปกติ
    ///
    /// มีอยู่เพราะการพิมพ์ผิดหนึ่งตัวอักษรทำให้ค่าที่ตั้งไม่มีผล **โดยไม่มีอะไรบอก**
    /// แล้วผู้ใช้จะสรุปว่าฟีเจอร์นี้เสีย
    UnknownKey {
        /// ชื่อคีย์เท่าที่ parser บอกได้
        name: String,
    },
    /// ค่าอยู่นอกช่วง → ถูกดึงกลับเข้าช่วง
    Clamped {
        /// ช่องไหน
        field: Field,
        /// ผู้ใช้เขียนอะไร
        asked: u64,
        /// ใช้จริงเท่าไหร่
        used: u64,
    },
    /// ★★ `max_pixels` ถูกจำกัดด้วย **RAM ของเครื่องนี้** ไม่ใช่ด้วยช่วงตายตัว
    ///
    /// แยกจาก [`Note::Clamped`] เพราะทางออกของผู้ใช้ต่างกันสิ้นเชิง: ตัวนั้นแก้ได้
    /// ด้วยการพิมพ์เลขใหม่ ส่วนตัวนี้แก้ได้ด้วยการ**เพิ่ม RAM เท่านั้น** —
    /// ถ้าบอกเหมือนกันเขาจะพิมพ์เลขใหม่ซ้ำแล้วซ้ำอีกโดยไม่มีวันสำเร็จ
    MaxPixelsCappedByRam {
        /// ผู้ใช้เขียนอะไร
        asked: u64,
        /// ใช้จริงเท่าไหร่ (เพดานของเครื่อง)
        used: u64,
        /// RAM ของเครื่อง (GB) — ตัวเลขที่อธิบายว่าทำไม
        ram_gb: u64,
    },
    /// ค่าเป็นคำที่ไม่รู้จัก → กลับไปใช้ค่าปริยายของช่องนั้น
    UnknownValue {
        /// ช่องไหน
        field: Field,
        /// ผู้ใช้เขียนอะไร
        given: String,
    },
}

/// ค่าที่ใช้จริง — **ทุกช่องมีค่าที่ใช้ได้เสมอ** ไม่ว่าไฟล์จะพังแค่ไหน
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// เพดาน RAM ของ decode pool รวมทุก worker (ไบต์)
    pub ram_limit: usize,
    /// เพดาน VRAM (ไบต์) — `None` = ให้โปรแกรมเลือกตามชนิดการ์ดจอ (`docs/05 §2`)
    pub vram_limit: Option<usize>,
    /// เพดาน pixel ต่อภาพ — **ไม่เกิน [`Caps::max_pixels_ceiling`] เสมอ**
    pub max_pixels: u64,
    /// ธีมของ UI
    pub theme: Theme,
    /// จังหวะการแสดงเฟรม
    pub present: Present,
    /// ★ เขียน `.refx-meta` ลงโฟลเดอร์ภาพหรือไม่ (P5-5)
    pub sidecar: SidecarPolicy,
}

impl Settings {
    /// ค่าปริยายทั้งชุดสำหรับเครื่องเครื่องนี้
    #[must_use]
    pub fn defaults(caps: Caps) -> Self {
        Self {
            ram_limit: usize::try_from(DEFAULT_RAM_LIMIT_MB << 20).unwrap_or(usize::MAX),
            vram_limit: None,
            max_pixels: caps.max_pixels_ceiling,
            theme: Theme::default(),
            present: Present::default(),
            sidecar: SidecarPolicy::default(),
        }
    }

    /// เนื้อไฟล์ `settings.toml` ที่แทนค่าชุดนี้
    ///
    /// เขียนทุกช่องเสมอ (ไม่ละช่องที่เป็นค่าปริยาย) เพราะไฟล์นี้เป็นที่ที่ผู้ใช้
    /// เปิดมาอ่านเพื่อ **รู้ว่าตั้งอะไรได้บ้าง** — ไฟล์ที่มีสองบรรทัดไม่บอกอะไรเขาเลย
    ///
    /// ★★ **คอมเมนต์ในไฟล์เป็นภาษาอังกฤษเสมอ ไม่ตามภาษาของ UI**
    ///
    /// รุ่นแรกเขียนเป็นไทย ซึ่งแปลว่าผู้ใช้ที่ใช้ UI อังกฤษเปิดไฟล์ของตัวเองมาแล้ว
    /// อ่านไม่ออก · ชั้นนี้เป็น `refx-io` ซึ่งพึ่ง `refx-ui::text` ไม่ได้อยู่แล้ว
    /// (ประตูเดียวของข้อความที่แปลได้ — `docs/03 §0`) และไฟล์ config อยู่ฝั่ง
    /// เดียวกับ `#[error(…)]` และ log: **อังกฤษสำหรับคนที่เปิดไฟล์ดิบ**
    /// ส่วนข้อความที่แปลแล้วอยู่บนแผง Settings ซึ่งเป็นทางหลักที่ผู้ใช้ใช้จริง
    #[must_use]
    pub fn to_toml(&self) -> String {
        let vram = match self.vram_limit {
            // ★ ละบรรทัดทิ้งไม่ได้ — ต้องเห็นว่าช่องนี้มีอยู่และตอนนี้เป็นอัตโนมัติ
            None => "# vram_limit_mb = 384   # leave out = pick from the GPU".to_owned(),
            Some(bytes) => format!("vram_limit_mb = {}", bytes >> 20),
        };
        format!(
            "# settings.toml - RefX\n\
             # Deleting this file is always safe: RefX goes back to its defaults.\n\
             \n\
             theme   = \"{theme}\"     # dark | light\n\
             present = \"{present}\"   # vsync | uncapped\n\
             \n\
             # Remember tags and ratings for folders you only browse, by writing a\n\
             # .refx-meta file next to the images.  ask | always | never\n\
             sidecar = \"{sidecar}\"\n\
             \n\
             [memory]\n\
             ram_limit_mb  = {ram}     # {ram_lo}-{ram_hi}\n\
             {vram}\n\
             max_pixels    = {pixels}  # capped by this machine's RAM\n",
            theme = self.theme.as_str(),
            present = self.present.as_str(),
            sidecar = self.sidecar.as_str(),
            ram = self.ram_limit >> 20,
            ram_lo = RAM_LIMIT_MB.start(),
            ram_hi = RAM_LIMIT_MB.end(),
            pixels = self.max_pixels,
        )
    }
}

/// ผลของการอ่าน — ค่าที่ใช้ได้ **บวก** สิ่งที่ต้องบอกผู้ใช้
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// ค่าที่ใช้จริง
    pub settings: Settings,
    /// เรื่องที่ต้องบอก — ว่าง = ไม่มีอะไรผิดปกติ
    pub notes: Vec<Note>,
}

/// ★ เพดานขนาดของไฟล์ config — 64 KB
///
/// I-4 บอกว่า **ทุกไฟล์คือ input ที่ไม่น่าไว้ใจ** และ `settings.toml` ไม่ได้ยกเว้น:
/// ไฟล์ที่โตผิดปกติเกิดได้จากดิสก์เต็มกลางการเขียน หรือจากคนที่ตั้งใจให้เราสูบ
/// ทั้งไฟล์เข้า RAM · ไฟล์จริงยาวไม่กี่สิบบรรทัด 64 KB จึงเผื่อไว้เกินพอแล้ว
pub const MAX_BYTES: u64 = 64 << 10;

/// อ่าน `settings.toml` จากดิสก์
///
/// **ไม่มีทางล้มเหลว** — ไฟล์หายก็คือค่าปริยาย (เป็นสภาพปกติของการเปิดครั้งแรก
/// จึงไม่มี [`Note`]) · อ่านไม่ได้/พัง/ใหญ่เกินเพดานก็คือค่าปริยายพร้อม [`Note`]
///
/// **ห้ามเรียกจาก UI thread** — แตะดิสก์ (I-2) · ของจริงเรียกตอนเปิดโปรแกรม
/// ก่อนหน้าต่างจะมี
#[must_use]
pub fn load(path: &Path, caps: Caps) -> Loaded {
    let failed = |detail: String| Loaded {
        settings: Settings::defaults(caps),
        notes: vec![Note::Unparsable { detail }],
    };

    // ★ ถามขนาดก่อนเปิด แล้วอ่านผ่าน `take` — รูปแบบเดียวกับ `read_file_guarded`
    //   (`clippy.toml` แบน `fs::read`/`read_to_string` ไว้ด้วยเหตุผลนี้)
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Loaded {
                settings: Settings::defaults(caps),
                notes: Vec::new(),
            };
        }
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "cannot read settings - using defaults");
            return failed(err.to_string());
        }
    };
    if size > MAX_BYTES {
        tracing::warn!(
            path = %path.display(), size, limit = MAX_BYTES,
            "settings file is far too large - using defaults"
        );
        return failed(format!("file is {size} bytes, limit is {MAX_BYTES}"));
    }

    let mut text = String::new();
    let read = std::fs::File::open(path).and_then(|file| {
        use std::io::Read as _;
        std::io::Read::take(file, MAX_BYTES).read_to_string(&mut text)
    });
    match read {
        Ok(_) => parse(&text, caps),
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "cannot read settings - using defaults");
            failed(err.to_string())
        }
    }
}

/// รูปแบบของไฟล์ **ก่อน** ตรวจค่า — ทุกช่องเป็น `Option` เพราะไม่ใส่ = ใช้ค่าปริยาย
#[derive(Debug, Default, serde::Deserialize)]
struct Raw {
    theme: Option<String>,
    present: Option<String>,
    sidecar: Option<String>,
    memory: Option<RawMemory>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct RawMemory {
    ram_limit_mb: Option<u64>,
    vram_limit_mb: Option<u64>,
    max_pixels: Option<u64>,
}

/// สำเนาที่ **ปฏิเสธคีย์แปลกปลอม** — ใช้ตรวจอย่างเดียว ไม่ได้เอาค่าไปใช้
///
/// ★ ทำไมต้องอ่านสองรอบ: การ deserialize ปกติ **ข้ามคีย์ที่ไม่รู้จักเงียบ ๆ**
/// ซึ่งแปลว่าผู้ใช้ที่พิมพ์ `ram_limit_md` ผิดหนึ่งตัวอักษรจะไม่มีวันรู้ว่าทำไม
/// ค่าที่ตั้งไม่มีผล · แต่ถ้าใช้ `deny_unknown_fields` เป็นตัวหลัก คีย์แปลกหนึ่งตัว
/// จะทำให้ **ทั้งไฟล์ถูกทิ้ง** ซึ่งแรงเกินไป — สองรอบจึงได้ทั้งสองอย่าง:
/// รอบผ่อนปรนเอาค่า รอบเข้มบอกว่ามีอะไรแปลกอยู่
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Strict {
    #[allow(dead_code)]
    theme: Option<String>,
    #[allow(dead_code)]
    present: Option<String>,
    #[allow(dead_code)]
    sidecar: Option<String>,
    #[allow(dead_code)]
    memory: Option<StrictMemory>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictMemory {
    #[allow(dead_code)]
    ram_limit_mb: Option<u64>,
    #[allow(dead_code)]
    vram_limit_mb: Option<u64>,
    #[allow(dead_code)]
    max_pixels: Option<u64>,
}

/// อ่านเนื้อไฟล์ — ★ **ฟังก์ชันบริสุทธิ์** เทสต์ได้โดยไม่ต้องแตะดิสก์
#[must_use]
pub fn parse(text: &str, caps: Caps) -> Loaded {
    let mut notes = Vec::new();

    let raw: Raw = match basic_toml::from_str(text) {
        Ok(raw) => raw,
        Err(err) => {
            // ★ ทั้งไฟล์ใช้ไม่ได้ → ค่าปริยายทั้งชุด **แต่ยังเปิดโปรแกรมได้**
            tracing::warn!(%err, "settings.toml could not be parsed - using defaults");
            return Loaded {
                settings: Settings::defaults(caps),
                notes: vec![Note::Unparsable {
                    detail: err.to_string(),
                }],
            };
        }
    };

    if let Err(err) = basic_toml::from_str::<Strict>(text) {
        notes.push(Note::UnknownKey {
            name: unknown_field_name(&err.to_string()),
        });
    }

    let memory = raw.memory.unwrap_or_default();
    let defaults = Settings::defaults(caps);

    let ram_limit_mb = match memory.ram_limit_mb {
        None => DEFAULT_RAM_LIMIT_MB,
        Some(asked) => clamp_into(asked, &RAM_LIMIT_MB, Field::RamLimitMb, &mut notes),
    };

    let vram_limit = memory.vram_limit_mb.map(|asked| {
        let used = clamp_into(asked, &VRAM_LIMIT_MB, Field::VramLimitMb, &mut notes);
        usize::try_from(used << 20).unwrap_or(usize::MAX)
    });

    let max_pixels = match memory.max_pixels {
        None => defaults.max_pixels,
        Some(asked) => cap_max_pixels(asked, caps, &mut notes),
    };

    let theme = match normalised(raw.theme.as_deref()).as_deref() {
        None => Theme::default(),
        Some("dark") => Theme::Dark,
        Some("light") => Theme::Light,
        Some(other) => {
            notes.push(Note::UnknownValue {
                field: Field::Theme,
                given: other.to_owned(),
            });
            Theme::default()
        }
    };

    let present = match normalised(raw.present.as_deref()).as_deref() {
        None => Present::default(),
        Some("vsync") => Present::Vsync,
        Some("uncapped") => Present::Uncapped,
        Some(other) => {
            notes.push(Note::UnknownValue {
                field: Field::Present,
                given: other.to_owned(),
            });
            Present::default()
        }
    };

    let sidecar = match normalised(raw.sidecar.as_deref()).as_deref() {
        None => SidecarPolicy::default(),
        Some("ask") => SidecarPolicy::Ask,
        Some("always") => SidecarPolicy::Always,
        Some("never") => SidecarPolicy::Never,
        Some(other) => {
            notes.push(Note::UnknownValue {
                field: Field::Sidecar,
                given: other.to_owned(),
            });
            SidecarPolicy::default()
        }
    };

    Loaded {
        settings: Settings {
            ram_limit: usize::try_from(ram_limit_mb << 20).unwrap_or(usize::MAX),
            vram_limit,
            max_pixels,
            theme,
            present,
            sidecar,
        },
        notes,
    }
}

/// ตัดช่องว่างและทำเป็นตัวเล็ก — `"Dark"` กับ `" dark "` ควรใช้ได้ทั้งคู่
///
/// ผู้ใช้ที่พิมพ์ `theme = "Dark"` แล้วโดนปฏิเสธจะไม่เข้าใจว่าผิดตรงไหน
fn normalised(given: Option<&str>) -> Option<String> {
    given.map(|word| word.trim().to_lowercase())
}

/// ดึงค่าเข้าช่วง แล้วบันทึกไว้ถ้าต้องดึงจริง
fn clamp_into(
    asked: u64,
    range: &std::ops::RangeInclusive<u64>,
    field: Field,
    notes: &mut Vec<Note>,
) -> u64 {
    let used = asked.clamp(*range.start(), *range.end());
    if used != asked {
        notes.push(Note::Clamped { field, asked, used });
    }
    used
}

/// เพดาน pixel — ★ ผูกกับ RAM ของเครื่อง ไม่ใช่กับตัวเลขตายตัว (`HANDOFF §4` ข้อ 3)
///
/// ต่ำเกินไปถูกดึงขึ้นด้วย [`Note::Clamped`] ตามปกติ · **สูงเกินเพดานของเครื่อง**
/// ได้ [`Note::MaxPixelsCappedByRam`] แทน เพราะทางออกของผู้ใช้คนละอย่างกัน
fn cap_max_pixels(asked: u64, caps: Caps, notes: &mut Vec<Note>) -> u64 {
    if asked > caps.max_pixels_ceiling {
        notes.push(Note::MaxPixelsCappedByRam {
            asked,
            used: caps.max_pixels_ceiling,
            ram_gb: caps.total_ram / (1 << 30),
        });
        return caps.max_pixels_ceiling;
    }
    // ★ เพดานของเครื่องอาจต่ำกว่า MIN_MAX_PIXELS บนเครื่องเล็กมาก — ห้ามดันขึ้น
    //   เกินเพดานนั้นเด็ดขาด ไม่งั้นกฎข้อ 3 ถูกละเมิดโดยด่านที่ควรจะช่วย
    let floor = MIN_MAX_PIXELS.min(caps.max_pixels_ceiling);
    if asked < floor {
        notes.push(Note::Clamped {
            field: Field::MaxPixels,
            asked,
            used: floor,
        });
        return floor;
    }
    asked
}

/// ชื่อคีย์ที่ serde บ่นถึง — `"unknown field `foo`, expected …"` → `"foo"`
///
/// ★ ถ้าวันหนึ่งข้อความของ serde/basic-toml เปลี่ยนรูป จะได้ทั้งประโยคกลับไปแทน
/// ซึ่งยังใช้บอกผู้ใช้ได้ (แค่ยาวขึ้น) ไม่ใช่ panic — เทสต์
/// `a_typo_is_reported_by_name` จะแดงถ้ารูปแบบเปลี่ยน
fn unknown_field_name(message: &str) -> String {
    let mut parts = message.split('`');
    match (parts.next(), parts.next()) {
        (Some(head), Some(name)) if head.contains("unknown field") && !name.is_empty() => {
            name.to_owned()
        }
        _ => message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// เครื่อง 16 GB — เพดาน pixel ชนค่าสัมบูรณ์ 16384² พอดี
    fn big_machine() -> Caps {
        Caps {
            total_ram: 16 << 30,
            max_pixels_ceiling: 268_435_456,
        }
    }

    /// เครื่อง 8 GB — เพดาน pixel ต่ำกว่าค่าสัมบูรณ์ (ราว 11,500²)
    fn small_machine() -> Caps {
        Caps {
            total_ram: 8 << 30,
            max_pixels_ceiling: (8u64 << 30) / 8 / 8,
        }
    }

    /// ★★★ เกณฑ์หลักของ P5-3: **ไฟล์ที่พังทุกแบบต้องเปิดโปรแกรมได้**
    ///
    /// ไม่ใช่ "ไม่ panic" อย่างเดียว — ค่าที่ได้ต้อง **ใช้งานได้จริง** ทุกช่อง
    /// (เพดาน RAM ที่เป็น 0 จะทำให้ decode pool รอตลอดกาล ซึ่งผู้ใช้แยกไม่ออก
    /// จากโปรแกรมค้าง)
    #[test]
    fn every_broken_settings_file_still_gives_a_usable_program() {
        let caps = big_machine();
        let broken = [
            ("ว่างเปล่า", ""),
            ("ช่องว่างล้วน", "   \n\n\t\n"),
            ("ไม่ใช่ TOML เลย", "\u{0}\u{1}\u{2}not toml at all ]]]["),
            ("เป็น JSON", r#"{"theme": "dark"}"#),
            ("วงเล็บไม่ปิด", "[memory\nram_limit_mb = 512"),
            ("ค่าเป็นชนิดผิด", "[memory]\nram_limit_mb = \"เยอะ ๆ\""),
            ("ติดลบ", "[memory]\nram_limit_mb = -5"),
            ("ล้น u64", "[memory]\nmax_pixels = 99999999999999999999999"),
            (
                "ศูนย์ทุกช่อง",
                "[memory]\nram_limit_mb = 0\nvram_limit_mb = 0\nmax_pixels = 0",
            ),
            (
                "ใหญ่เกินจริง",
                "[memory]\nram_limit_mb = 9999999\nvram_limit_mb = 9999999\nmax_pixels = 9999999999",
            ),
            ("คีย์ไม่รู้จัก", "nonsense = 1\n[memory]\nram_limit_md = 512"),
            ("ค่า enum ไม่รู้จัก", "theme = \"neon\"\npresent = \"turbo\""),
            ("ตารางซ้อนที่ไม่มีจริง", "[memory.deep.deeper]\nx = 1"),
        ];

        for (what, text) in broken {
            let loaded = parse(text, caps);
            let s = loaded.settings;
            assert!(
                RAM_LIMIT_MB.contains(&((s.ram_limit >> 20) as u64)),
                "{what}: เพดาน RAM ใช้ไม่ได้ ({} MB)",
                s.ram_limit >> 20
            );
            if let Some(vram) = s.vram_limit {
                assert!(
                    VRAM_LIMIT_MB.contains(&((vram >> 20) as u64)),
                    "{what}: เพดาน VRAM ใช้ไม่ได้"
                );
            }
            assert!(
                s.max_pixels > 0 && s.max_pixels <= caps.max_pixels_ceiling,
                "{what}: เพดาน pixel ใช้ไม่ได้ ({})",
                s.max_pixels
            );
        }
    }

    /// ★★ ไฟล์ที่พังต้อง **ไม่เงียบ** — ทุกเคสข้างบนที่ไม่ใช่ "ว่างเปล่า" ต้องมีคำอธิบาย
    ///
    /// negative control ของเทสต์ข้างบน: ถ้า `parse` ตอบค่าปริยายทุกครั้งโดยไม่บ่นเลย
    /// เทสต์ข้างบนจะยังเขียวสนิท ทั้งที่ผู้ใช้ไม่มีทางรู้ว่าค่าที่ตั้งไม่มีผล
    #[test]
    fn a_setting_that_did_not_take_effect_is_never_silent() {
        let caps = big_machine();
        let noisy = [
            "ไม่ใช่ TOML เลย ]]][",
            "[memory]\nram_limit_mb = \"เยอะ ๆ\"",
            "[memory]\nram_limit_mb = 0",
            "[memory]\nram_limit_mb = 9999999",
            "[memory]\nmax_pixels = 9999999999",
            "nonsense = 1",
            "theme = \"neon\"",
            "present = \"turbo\"",
        ];
        for text in noisy {
            assert!(
                !parse(text, caps).notes.is_empty(),
                "ค่าที่ใช้ไม่ได้ถูกทิ้งเงียบ ๆ: {text:?}"
            );
        }

        // ไฟล์ที่ไม่มีอยู่และไฟล์ที่ถูกต้องต้อง **ไม่** บ่น ไม่งั้นคำเตือนจะกลายเป็นเสียงรบกวน
        assert!(parse("", caps).notes.is_empty(), "ไฟล์ว่างเป็นสภาพปกติ");
        assert!(
            parse(
                "theme = \"light\"\npresent = \"uncapped\"\n[memory]\nram_limit_mb = 512",
                caps
            )
            .notes
            .is_empty()
        );
    }

    /// ★★★ `HANDOFF §4` ข้อ 3: ตั้ง `max_pixels` เกิน RAM ของเครื่องไม่ได้
    ///
    /// และต้องบอก **ว่าทำไม** ด้วยตัวเลข RAM — ไม่ใช่แค่ "ค่าเกินช่วง"
    /// เพราะทางออกคือเพิ่ม RAM ไม่ใช่พิมพ์เลขใหม่
    #[test]
    fn max_pixels_can_never_exceed_what_this_machine_can_decode() {
        let caps = small_machine();
        let loaded = parse("[memory]\nmax_pixels = 268435456", caps);
        assert_eq!(loaded.settings.max_pixels, caps.max_pixels_ceiling);
        assert_eq!(
            loaded.notes,
            vec![Note::MaxPixelsCappedByRam {
                asked: 268_435_456,
                used: caps.max_pixels_ceiling,
                ram_gb: 8,
            }],
            "ต้องบอกด้วยว่าเพดานมาจาก RAM ของเครื่อง ไม่ใช่จากช่วงตายตัว"
        );

        // ★ เครื่องใหญ่ตั้งเท่าเพดานพอดีได้ ไม่ถูกบ่น
        let big = big_machine();
        let ok = parse("[memory]\nmax_pixels = 268435456", big);
        assert_eq!(ok.settings.max_pixels, big.max_pixels_ceiling);
        assert!(ok.notes.is_empty());
    }

    /// เครื่องเล็กมากที่เพดานของมันต่ำกว่า `MIN_MAX_PIXELS` — พื้นห้ามดันทะลุเพดาน
    ///
    /// ★ ถ้าพื้นชนะ เครื่อง 1 GB จะยอมรับภาพที่ตัวมันเอง decode ไม่ไหว
    /// ซึ่งคือสิ่งที่กฎข้อ 3 มีไว้กันพอดี — ด่านที่ควรช่วยกลายเป็นตัวเจาะรูเสียเอง
    #[test]
    fn the_floor_never_pushes_max_pixels_above_a_tiny_machines_ceiling() {
        let tiny = Caps {
            total_ram: 1 << 30,
            max_pixels_ceiling: (1u64 << 30) / 8 / 8, // 16,777,216
        };
        for asked in [1u64, 1000, MIN_MAX_PIXELS] {
            let used = parse(&format!("[memory]\nmax_pixels = {asked}"), tiny)
                .settings
                .max_pixels;
            assert!(
                used <= tiny.max_pixels_ceiling,
                "asked {asked} → {used} ทะลุเพดานของเครื่อง"
            );
        }

        let tinier = Caps {
            total_ram: 64 << 20,
            max_pixels_ceiling: 1024, // ต่ำกว่า MIN_MAX_PIXELS มาก
        };
        let used = parse("[memory]\nmax_pixels = 1", tinier)
            .settings
            .max_pixels;
        assert_eq!(used, 1024, "พื้นต้องยอมเพดาน ไม่ใช่ทะลุมัน");
    }

    /// ★ พิมพ์คีย์ผิดหนึ่งตัวอักษรต้องได้ชื่อคีย์นั้นกลับมา ไม่ใช่ความเงียบ
    ///
    /// ถ้าข้อความของ parser เปลี่ยนรูปวันหนึ่ง เทสต์นี้จะแดง แล้วเราจะได้
    /// [`unknown_field_name`] ที่ตรงกับของจริง ไม่ใช่ที่ตรงกับความจำ
    #[test]
    fn a_typo_is_reported_by_name() {
        let loaded = parse("[memory]\nram_limit_md = 512", big_machine());
        assert_eq!(
            loaded.notes,
            vec![Note::UnknownKey {
                name: "ram_limit_md".to_owned()
            }]
        );
        // ★ ค่าที่เหลือยังต้องทำงาน — คีย์แปลกหนึ่งตัวห้ามทิ้งทั้งไฟล์
        let mixed = parse(
            "theme = \"light\"\n[memory]\nram_limit_md = 512\nram_limit_mb = 512",
            big_machine(),
        );
        assert_eq!(mixed.settings.theme, Theme::Light);
        assert_eq!(mixed.settings.ram_limit, 512 << 20);
    }

    /// ★ ตัวพิมพ์ใหญ่และช่องว่างไม่ควรทำให้ค่าที่ถูกต้องกลายเป็นค่าที่ไม่รู้จัก
    #[test]
    fn a_value_the_user_clearly_meant_is_accepted() {
        let caps = big_machine();
        for text in ["theme = \"Dark\"", "theme = \" dark \"", "theme = \"DARK\""] {
            let loaded = parse(text, caps);
            assert_eq!(loaded.settings.theme, Theme::Dark, "{text}");
            assert!(loaded.notes.is_empty(), "{text}");
        }
    }

    /// ★★ เขียนออกแล้วอ่านกลับต้องได้ค่าเดิมเป๊ะ — แผง Settings บันทึกด้วยทางนี้
    ///
    /// ถ้า round-trip ไม่ตรง ผู้ใช้จะตั้งค่า ปิดโปรแกรม เปิดใหม่ แล้วเจอค่าอื่น
    /// ซึ่งอ่านได้อย่างเดียวว่าโปรแกรมไม่จำสิ่งที่เขาสั่ง
    #[test]
    fn what_the_panel_writes_is_what_the_next_start_reads() {
        let caps = big_machine();
        for settings in [
            Settings::defaults(caps),
            Settings {
                ram_limit: 1024 << 20,
                vram_limit: Some(2048 << 20),
                max_pixels: 100_000_000,
                theme: Theme::Light,
                present: Present::Uncapped,
                sidecar: SidecarPolicy::Always,
            },
            Settings {
                ram_limit: 64 << 20,
                vram_limit: None,
                max_pixels: caps.max_pixels_ceiling,
                theme: Theme::Dark,
                present: Present::Vsync,
                sidecar: SidecarPolicy::Never,
            },
        ] {
            let text = settings.to_toml();
            let back = parse(&text, caps);
            assert_eq!(back.settings, settings, "round-trip เพี้ยน:\n{text}");
            assert!(back.notes.is_empty(), "ไฟล์ที่เราเขียนเองต้องไม่ถูกบ่น:\n{text}");
        }
    }

    /// ไฟล์ที่ไม่มีอยู่คือสภาพปกติของการเปิดครั้งแรก — ต้องเงียบสนิท
    #[test]
    fn a_missing_file_is_not_a_problem() {
        let loaded = load(Path::new("no-such-dir-xyz/settings.toml"), big_machine());
        assert_eq!(loaded.settings, Settings::defaults(big_machine()));
        assert!(loaded.notes.is_empty());
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-settings-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// ★★ ไฟล์บนดิสก์ที่พังในแบบที่ `parse` เห็นไม่ได้ — ต้องจบที่ค่าปริยายเหมือนกัน
    ///
    /// `parse` รับ `&str` จึงทดสอบสองอย่างนี้ไม่ได้เลย: ไฟล์ที่ **ใหญ่เกินเพดาน**
    /// (I-4) และไฟล์ที่ **ไม่ใช่ UTF-8** · ทั้งคู่เป็นสภาพที่เกิดจริงได้จากดิสก์เต็ม
    /// กลางการเขียน — และทั้งคู่ต้องไม่หยุดการเปิดโปรแกรม
    #[test]
    fn a_file_that_only_the_disk_can_produce_still_opens_the_program() {
        let caps = big_machine();
        let dir = temp_dir("disk");

        let huge = dir.join("huge.toml");
        std::fs::write(&huge, "# ".repeat((MAX_BYTES as usize) + 1)).unwrap();
        let loaded = load(&huge, caps);
        assert_eq!(loaded.settings, Settings::defaults(caps));
        assert!(
            matches!(loaded.notes.as_slice(), [Note::Unparsable { .. }]),
            "ไฟล์ใหญ่เกินเพดานต้องถูกบอก ไม่ใช่ถูกตัดแล้วอ่านต่อเงียบ ๆ: {:?}",
            loaded.notes
        );

        let binary = dir.join("binary.toml");
        std::fs::write(&binary, [0xFF_u8, 0xFE, 0x00, 0x01, 0x80]).unwrap();
        let loaded = load(&binary, caps);
        assert_eq!(loaded.settings, Settings::defaults(caps));
        assert!(!loaded.notes.is_empty(), "ไฟล์ที่ไม่ใช่ UTF-8 ต้องไม่เงียบ");

        // ★ ไฟล์ที่ **ถูกต้อง** บนดิสก์ต้องผ่านเส้นทางเดียวกันแล้วได้ค่าครบ
        let good = dir.join("good.toml");
        std::fs::write(&good, "theme = \"light\"\n[memory]\nram_limit_mb = 512").unwrap();
        let loaded = load(&good, caps);
        assert_eq!(loaded.settings.theme, Theme::Light);
        assert_eq!(loaded.settings.ram_limit, 512 << 20);
        assert!(loaded.notes.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
