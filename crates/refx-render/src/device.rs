//! `RenderContext` — instance / adapter / device / queue / surface ทั้งชุด + การกู้คืน
//!
//! crate นี้ห้าม depend `winit` (ARCHITECTURE §2) จึงรับหน้าต่างเป็น generic
//! ที่ implement `raw-window-handle` แทน — winit จะเป็นคนส่งเข้ามาจากชั้นบน
//!
//! **Device lost คือสาเหตุ crash อันดับหนึ่งของแอปกราฟิกบน Windows**
//! (driver update, sleep/resume, สลับ iGPU↔dGPU, TDR timeout)
//! จึงต้องกู้ได้ตั้งแต่ P0 ไม่ใช่ไปแก้ทีหลัง
//!
//! spec: docs/04-rendering.md §6, §7, §8

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

/// เตรียมกราฟิกไม่สำเร็จ
///
/// ★ ข้อความเป็นอังกฤษสำหรับ log/นักพัฒนา — ข้อความที่ผู้ใช้เห็นอยู่ที่
/// `refx-ui::text` ประกอบจากฟิลด์แล้วแปลตามภาษา (docs/03 §0)
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    /// ผูก surface กับหน้าต่างไม่ได้
    #[error("cannot create a drawing surface from the window: {0}")]
    CreateSurface(#[from] wgpu::CreateSurfaceError),

    /// ไม่พบ GPU ที่ใช้ได้
    #[error("no usable GPU adapter found (needs Vulkan or DirectX 12): {0}")]
    NoAdapter(#[from] wgpu::RequestAdapterError),

    /// ขอ device ไม่สำเร็จ
    #[error("cannot acquire a GPU device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),

    /// surface ไม่รองรับรูปแบบใดที่ใช้ได้
    #[error("surface supports no colour format that RefX can use")]
    NoSupportedFormat,

    /// `WGPU_BACKEND` บอกชื่อที่ใช้ไม่ได้
    #[error(transparent)]
    Backend(#[from] BackendChoiceError),
}

/// ตัวแปรสภาพแวดล้อมที่เลือก backend — **ชื่อเดียวกับที่ wgpu ใช้โดยตั้งใจ**
///
/// ★★★ ทำไมต้องมี: ตอนไล่บั๊ก `gles` ที่ค้างหน้าต่าง (สิบวัน) การเปลี่ยน backend
/// หนึ่งตัวแปร **ต้อง build ใหม่ทั้งทรีทุกครั้ง** เพราะโค้ดเดิมสร้าง instance ด้วย
/// `InstanceDescriptor::new_with_display_handle(...)` ซึ่งไม่อ่าน env เลย ·
/// ราคาของการถามคำถามหนึ่งข้อจึงเป็นหลักสิบนาที แทนที่จะเป็นหลักวินาที
pub const BACKEND_ENV: &str = "WGPU_BACKEND";

/// ชื่อที่รับได้ — **ชุดเดียวกับ `wgpu::Backends::from_comma_list` เป๊ะ**
/// เพื่อให้คนที่เคยใช้ wgpu ไม่ต้องจำสองชุด
const BACKEND_NAMES: &[(&str, wgpu::Backends)] = &[
    ("vulkan", wgpu::Backends::VULKAN),
    ("vk", wgpu::Backends::VULKAN),
    ("dx12", wgpu::Backends::DX12),
    ("d3d12", wgpu::Backends::DX12),
    ("metal", wgpu::Backends::METAL),
    ("mtl", wgpu::Backends::METAL),
    ("gl", wgpu::Backends::GL),
    ("gles", wgpu::Backends::GL),
    ("opengl", wgpu::Backends::GL),
];

/// backend ที่จะใช้ **และที่มาของการเลือก**
///
/// เก็บ "เลือกอะไร" กับ "เพราะอะไร" ไว้ด้วยกันโดยตั้งใจ — log ที่บอกแค่ชื่อ
/// backend ตอบไม่ได้ว่ามันมาจาก env ของคนที่กำลังไล่บั๊ก หรือเป็นค่าปริยาย
/// ซึ่งเป็นคำถามแรกเสมอเวลาอ่าน log ของคนอื่น
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendChoice {
    /// ชุดที่จะส่งให้ `wgpu`
    pub backends: wgpu::Backends,
    /// `true` = มาจาก [`BACKEND_ENV`] · `false` = ทุกตัวที่คอมไพล์เข้ามา
    pub from_env: bool,
}

/// `WGPU_BACKEND` บอกชื่อที่ใช้ไม่ได้ — **ทั้งสองแบบต้องล้ม ไม่ใช่ตกไปค่าปริยาย**
///
/// ★ `wgpu::Backends::from_comma_list` เลือกทาง "เตือนแล้วข้าม" ซึ่งแปลว่าพิมพ์
/// ชื่อผิดหนึ่งตัวอักษรแล้วได้ backend อื่นมาเงียบ ๆ · สำหรับเครื่องมือที่มีไว้
/// **ไล่บั๊ก** นั่นคือพฤติกรรมที่แย่ที่สุดที่เป็นไปได้: มันตอบคำถามที่ไม่ได้ถาม
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendChoiceError {
    /// ไม่ใช่ชื่อ backend เลย
    #[error("{BACKEND_ENV}={asked:?} is not a backend name — use one of: {names}")]
    UnknownName {
        /// ชื่อที่ผู้ใช้พิมพ์มา
        asked: String,
        /// ชื่อทั้งหมดที่รับได้
        names: String,
    },

    /// เป็นชื่อที่ถูก แต่ build นี้ไม่ได้คอมไพล์มันเข้ามา
    #[error(
        "{BACKEND_ENV}={asked:?}: this build does not contain {missing} — it contains {available}"
    )]
    NotInThisBuild {
        /// ค่าที่ผู้ใช้ตั้งไว้ทั้งก้อน
        asked: String,
        /// ตัวที่ขาด
        missing: String,
        /// ตัวที่มีจริงใน build นี้
        available: String,
    },
}

/// ชื่อที่ **พิมพ์ออก** — หนึ่งชื่อต่อหนึ่ง backend
///
/// แยกจาก [`BACKEND_NAMES`] เพราะอันนั้นมีชื่อเล่นหลายชื่อต่อ backend ซึ่งเป็น
/// เรื่องของ *สิ่งที่รับเข้า* · log ที่เขียน "vulkan, vk" คือ log ที่อ่านแล้วงง
const BACKEND_LABELS: &[(wgpu::Backends, &str)] = &[
    (wgpu::Backends::VULKAN, "vulkan"),
    (wgpu::Backends::DX12, "dx12"),
    (wgpu::Backends::METAL, "metal"),
    (wgpu::Backends::GL, "gl"),
];

/// ชื่อที่อ่านออกของชุด backend — ใช้ทั้งใน log และในข้อความ error
#[must_use]
pub fn backend_names(backends: wgpu::Backends) -> String {
    let out: Vec<&str> = BACKEND_LABELS
        .iter()
        .filter(|(bit, _)| backends.contains(*bit))
        .map(|(_, name)| *name)
        .collect();
    if out.is_empty() {
        "(none)".to_owned()
    } else {
        out.join(", ")
    }
}

/// ตัดสินว่าจะใช้ backend ไหน จากค่า env และ **ชุดที่คอมไพล์เข้ามาจริง**
///
/// ★★★ `compiled_in` เป็นพารามิเตอร์ ไม่ใช่การเรียก `enabled_backend_features()`
/// ข้างใน — เพื่อให้เทสต์ถามคำถามนี้ได้โดยไม่ต้องมี GPU และไม่ขึ้นกับว่าเครื่อง
/// ที่รันเทสต์คอมไพล์อะไรมา (เทสต์ที่ผลเปลี่ยนตามเครื่องคือเทสต์ที่อ่านไม่ออก)
///
/// ★★ **ขอบเขตยังเป็นชุดที่คอมไพล์เข้ามาเสมอ** — env เลือกได้แค่ *ในนั้น*
/// ประตู `the_backend_that_hangs_the_window_is_not_compiled_in` จึงไม่ถูกผ่อน
/// แม้แต่นิดเดียว: สิ่งที่ไม่ได้คอมไพล์เข้ามา ไม่มีทางถูกเลือกด้วย env
///
/// # Errors
/// เมื่อชื่อไม่ถูก หรือชื่อถูกแต่ build นี้ไม่มีตัวนั้น
pub fn choose_backends(
    raw: Option<&str>,
    compiled_in: wgpu::Backends,
) -> Result<BackendChoice, BackendChoiceError> {
    let Some(asked) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(BackendChoice {
            backends: compiled_in,
            from_env: false,
        });
    };

    let mut wanted = wgpu::Backends::empty();
    let mut missing: Vec<String> = Vec::new();
    for item in asked.split(',') {
        let item = item.trim().to_lowercase();
        if item.is_empty() {
            continue;
        }
        let Some((_, bit)) = BACKEND_NAMES
            .iter()
            .find(|(name, _)| *name == item.as_str())
        else {
            return Err(BackendChoiceError::UnknownName {
                asked: item,
                names: BACKEND_NAMES
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", "),
            });
        };
        // ★ ตรวจ **ทีละชื่อ** ไม่ใช่ตัดกันทั้งชุด — ขอ `vulkan,gl` แล้วได้ vulkan
        //   อย่างเดียวเงียบ ๆ คือการตอบคำถามที่ไม่ได้ถาม เหมือนกับชื่อผิด
        if !compiled_in.contains(*bit) && !missing.iter().any(|seen| seen == &item) {
            missing.push(item.clone());
        }
        wanted |= *bit;
    }

    if !missing.is_empty() {
        return Err(BackendChoiceError::NotInThisBuild {
            asked: asked.to_owned(),
            missing: missing.join(", "),
            available: backend_names(compiled_in),
        });
    }
    if wanted.is_empty() {
        return Err(BackendChoiceError::UnknownName {
            asked: asked.to_owned(),
            names: BACKEND_NAMES
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", "),
        });
    }

    Ok(BackendChoice {
        backends: wanted,
        from_env: true,
    })
}

/// แหล่งที่สร้าง surface ได้ **ซ้ำหลายครั้ง**
///
/// ต้องสร้างซ้ำได้เพราะตอนกู้ device lost เราทิ้ง `Instance` เดิมทั้งก้อน
/// แล้วสร้างใหม่ทั้งชุด — ถ้าเก็บแค่ `Surface` ไว้จะกู้ไม่ได้
///
/// มี blanket impl ให้ทุกอย่างที่เป็น window handle อยู่แล้ว ชั้นบนไม่ต้อง implement เอง
pub trait SurfaceSource: Send + Sync + 'static {
    /// สร้าง `Instance` ใหม่ (พร้อม display handle สำหรับ Wayland)
    ///
    /// `backends` มาจาก [`choose_backends`] — ส่งเข้ามาแทนที่จะอ่าน env เองที่นี่
    /// เพราะการอ่าน env ต้อง **ล้มได้** และเมธอดนี้คืน `Instance` เฉย ๆ ·
    /// อีกเหตุผล: ตอนกู้ device lost เราสร้าง instance ใหม่ทั้งก้อน แล้วมันต้อง
    /// ได้ชุดเดิมเป๊ะ ไม่ใช่ไปอ่าน env ซ้ำซึ่งอาจเปลี่ยนไปแล้วระหว่างโปรแกรมรัน
    fn make_instance(&self, backends: wgpu::Backends) -> wgpu::Instance;
    /// สร้าง `Surface` ใหม่จาก instance ที่ให้มา
    fn make_surface(
        &self,
        instance: &wgpu::Instance,
    ) -> Result<wgpu::Surface<'static>, wgpu::CreateSurfaceError>;
}

impl<W> SurfaceSource for W
where
    W: wgpu::DisplayAndWindowHandle + std::fmt::Debug + Clone + Send + Sync + 'static,
{
    fn make_instance(&self, backends: wgpu::Backends) -> wgpu::Instance {
        // ส่ง display handle เข้าไปด้วย — Linux/Wayland ต้องใช้เลือก backend ให้ถูก
        //
        // ★ ตั้ง `backends` เอง **ไม่ใช้ `with_env()` ของ wgpu** ทั้งที่มันมีให้ —
        //   ตัวนั้นเจอชื่อที่ไม่รู้จักแล้ว "เตือนแล้วข้าม" และเปิดประตูให้ env
        //   ตัวอื่น (`WGPU_DEBUG`, backend options) เปลี่ยนพฤติกรรมไปด้วย
        //   ซึ่งกว้างกว่าที่เราต้องการและเงียบกว่าที่เรายอมรับได้
        let mut descriptor =
            wgpu::InstanceDescriptor::new_with_display_handle(Box::new(self.clone()));
        descriptor.backends = backends;
        wgpu::Instance::new(descriptor)
    }

    fn make_surface(
        &self,
        instance: &wgpu::Instance,
    ) -> Result<wgpu::Surface<'static>, wgpu::CreateSurfaceError> {
        instance.create_surface(self.clone())
    }
}

/// ตัวเลือกตอนสร้าง context
///
/// flag จำลอง device lost มีสองแบบเพราะทดสอบคนละสถานการณ์ (ROADMAP หมายเหตุ P0-5)
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// จำลอง device lost หลังวาดครบ N เฟรม — "device ตายระหว่างผู้ใช้ลากภาพ"
    ///
    /// มีผลเฉพาะเมื่อคอมไพล์ด้วย feature `force-device-lost`
    /// — build ปกติจะเพิกเฉยเสมอ เพื่อไม่ให้ release ทำลายตัวเองได้
    pub force_device_lost_after: Option<u64>,

    /// ★ จำลอง device lost หลังผ่านไป N มิลลิวินาที — **"device ตายตอนแอปหลับอยู่"**
    ///
    /// นี่คือสถานการณ์จริงของผู้ใช้: เปิดโปรแกรมทิ้งไว้ข้าง Photoshop ทั้งวัน
    /// แล้ว driver อัปเดต หรือเครื่อง sleep/resume ตอนไม่มีใครแตะ
    ///
    /// ใช้ `ControlFlow::WaitUntil` จึง **ไม่ฝืน I-1** เลย
    pub force_device_lost_after_ms: Option<u64>,

    /// ★ ปลด vsync เพื่อวัด **ต้นทุนจริงของการวาด** (โหมด benchmark เท่านั้น)
    ///
    /// ถ้าไม่ปลด ตัวเลขที่วัดได้คือคาบสัญญาณของจอ (เช่น 165 Hz = 6.06 ms)
    /// ไม่ใช่เวลาที่ใช้วาดจริง → ตอบไม่ได้ว่าเหลือ headroom เท่าไหร่
    /// และพอ P1 เอา texture มาแปะแล้วเฟรมตก จะไม่มีเส้นฐานให้เทียบ
    ///
    /// **การใช้งานปกติต้องเป็น `AutoVsync` เสมอ** (docs/04 §8 — `Immediate` เผา GPU ฟรี)
    pub uncapped_present: bool,
}

/// ความสามารถของ GPU ที่ตรวจได้ตอนเปิดโปรแกรม
#[derive(Debug, Clone)]
pub struct GpuCapabilities {
    /// ชื่อการ์ดจอ
    pub adapter_name: String,
    /// backend ที่เลือกจริง
    pub backend: wgpu::Backend,
    /// ประเภทการ์ด (แยก iGPU / dGPU)
    pub device_type: wgpu::DeviceType,
    /// รองรับ BC compression ไหม — docs/04 §4 บอกให้มี fallback เสมอ
    pub bc_compression: bool,
    /// ขนาด texture ใหญ่สุดที่รองรับ (จำกัดขนาด atlas)
    pub max_texture_dimension_2d: u32,
}

impl GpuCapabilities {
    /// ขนาด atlas ที่ใช้ได้จริง — อย่างมาก 2048 ตาม docs/04 §4
    #[must_use]
    pub fn atlas_size(&self) -> u32 {
        self.max_texture_dimension_2d.min(2048)
    }
}

/// ผลของการขอเฟรมจาก surface
///
/// map ตรงกับ `wgpu::CurrentSurfaceTexture` ของ wgpu 29 (docs/04 §7)
/// แยกออกมาเพื่อให้ชั้นบนตัดสินใจเรื่อง `request_redraw` ได้เอง
/// — สำคัญมากกับ I-1 เพราะ `Skip` **ห้าม** ขอวาดใหม่
#[derive(Debug)]
pub enum FrameStatus {
    /// ได้เฟรมแล้ว วาดได้เลย
    Ready(wgpu::SurfaceTexture),
    /// ข้ามเฟรมนี้ และ **ห้ามขอวาดใหม่** (หน้าต่างถูกบัง / driver ไม่ว่าง)
    ///
    /// นี่คือของฟรีสำหรับ I-1: minimize แล้ว = 0% CPU จริง ๆ
    Skip,
    /// surface ใช้ไม่ได้แล้ว — ตั้งค่าใหม่ให้แล้ว ชั้นบนควรขอวาดอีกครั้ง
    Recovered,
    /// device หายทั้งก้อน — ชั้นบนต้องเรียก [`RenderContext::recover`] ก่อนวาดต่อ
    DeviceLost,
}

/// ชิ้นส่วน GPU ทั้งชุดที่ถูกสร้าง/ทิ้งไปด้วยกัน
///
/// แยกเป็น struct เพื่อให้ `new()` กับ `recover()` ใช้โค้ดสร้างชุดเดียวกัน
/// ไม่ต้องเขียนซ้ำแล้วลืม sync กัน
struct GpuStack {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    caps: GpuCapabilities,
}

/// เข้าลูปเฟรมแล้วหรือยัง — ใช้กับ `debug_assert!` ใน [`block_on`] เท่านั้น
///
/// `block_on` ใช้ได้เฉพาะตอน init/กู้ device ถ้าหลุดไปอยู่บนเส้นทางต่อเฟรม
/// จะกลายเป็นการบล็อก UI thread ซึ่งผิด I-2 (docs/09)
static FRAME_LOOP_STARTED: AtomicBool = AtomicBool::new(false);

/// ทุกอย่างที่เกี่ยวกับ GPU
pub struct RenderContext {
    /// เก็บหน้าต่างไว้เพื่อสร้าง surface ใหม่ตอนกู้ device
    window: Arc<dyn SurfaceSource>,
    /// `None` ชั่วคราวระหว่างกู้ device เท่านั้น
    ///
    /// ต้องเป็น `Option` เพราะ **หน้าต่างหนึ่งบานมี surface ได้ทีละอันเดียว**
    /// (Vulkan/DXGI ตอบ "Native window is in use" ถ้าสร้างซ้อน)
    /// จึงต้องทิ้งของเก่าให้หมดก่อนแล้วค่อยสร้างใหม่ ห้ามสร้างคร่อมกัน
    stack: Option<GpuStack>,
    options: RenderOptions,

    /// callback ของ wgpu ตั้ง flag นี้เมื่อ device หาย (ทำงานคนละเธรดได้)
    device_lost: Arc<AtomicBool>,
    /// ตั้งเมื่อ GPU แจ้ง out of memory
    oom: Arc<AtomicBool>,

    /// เพิ่มขึ้นทุกครั้งที่กู้ device — ชั้นบนใช้รู้ว่าต้องสร้าง resource ใหม่
    generation: u64,
    /// นับ validation error ติดกัน (docs/04 §7)
    consecutive_errors: u32,
    /// นับเฟรมที่วาดสำเร็จ — ใช้กับ `--force-device-lost-after=N`
    frames: u64,
    /// ตัวจำลอง device lost — มีเฉพาะ build ทดสอบ release ไม่ต้องแบกฟิลด์นี้เลย
    #[cfg(feature = "force-device-lost")]
    forced: ForcedLoss,
}

impl RenderContext {
    /// จำนวน validation error ติดกันก่อนจะถือว่า device เสีย
    const MAX_CONSECUTIVE_ERRORS: u32 = 10;

    /// สร้าง context จากหน้าต่าง
    pub fn new<W>(
        window: W,
        width: u32,
        height: u32,
        options: RenderOptions,
    ) -> Result<Self, DeviceError>
    where
        W: wgpu::DisplayAndWindowHandle + std::fmt::Debug + Clone + Send + Sync + 'static,
    {
        let window: Arc<dyn SurfaceSource> = Arc::new(window);
        let device_lost = Arc::new(AtomicBool::new(false));
        let oom = Arc::new(AtomicBool::new(false));

        let forced_requested = options.force_device_lost_after.is_some()
            || options.force_device_lost_after_ms.is_some();
        if forced_requested && !cfg!(feature = "force-device-lost") {
            tracing::warn!(
                "device-lost simulation flag given but the force-device-lost feature is off — it does nothing"
            );
        }

        // ตั้งนาฬิกาปลุกไว้ตั้งแต่ตอนสร้าง เพื่อให้เวลานับจาก "เปิดโปรแกรม"
        // ไม่ใช่จาก "เฟรมแรก" — ตรงกับสถานการณ์จริงที่แอปหลับอยู่
        #[cfg(feature = "force-device-lost")]
        let forced = ForcedLoss::new(&options);

        let stack = build_stack(
            window.as_ref(),
            width,
            height,
            &device_lost,
            &oom,
            /* generation */ 0,
            options.uncapped_present,
        )?;

        Ok(Self {
            window,
            stack: Some(stack),
            options,
            device_lost,
            oom,
            generation: 0,
            consecutive_errors: 0,
            frames: 0,
            #[cfg(feature = "force-device-lost")]
            forced,
        })
    }

    /// สร้าง instance/adapter/device/queue/surface ใหม่ทั้งชุดหลัง device หาย
    ///
    /// **ไม่แตะ document state เลย** — ผู้ใช้เห็นแค่จอกระพริบครั้งเดียว งานไม่หาย
    /// (docs/04 §7 ข้อ 4)
    ///
    /// หลังเรียกสำเร็จ [`RenderContext::generation`] จะเปลี่ยน ชั้นบนต้องสร้าง
    /// pipeline / texture / atlas ใหม่ทั้งหมด เพราะของเดิมผูกกับ device ที่ตายไปแล้ว
    pub fn recover(&mut self) -> Result<(), DeviceError> {
        let (width, height) = self
            .stack
            .as_ref()
            .map_or((1, 1), |s| (s.config.width, s.config.height));
        let next_generation = self.generation + 1;

        tracing::warn!(generation = next_generation, "creating a new GPU device");

        // ★ ต้องทิ้งของเก่าให้หมดก่อน — หน้าต่างหนึ่งบานมี surface ได้ทีละอันเดียว
        //   ถ้าสร้างใหม่คร่อมของเก่า จะได้ "Native window is in use" แล้ว panic
        //   (เจอของจริงตอนทดสอบ P0-5) ในสถานการณ์จริง device ตายไปแล้วอยู่ดี
        //   จึงไม่มีอะไรให้เสียจากการทิ้งก่อน
        drop(self.stack.take());

        // เคลียร์ flag หลังทิ้งของเก่า: การ drop device จะยิง callback ด้วย
        // DeviceLostReason::Destroyed ซึ่งเราไม่นับ แต่เคลียร์ทีหลังปลอดภัยกว่า
        self.device_lost.store(false, Ordering::SeqCst);
        self.oom.store(false, Ordering::SeqCst);

        // กู้ device เป็นข้อยกเว้นที่ block_on ใช้ได้ (device ตายไปแล้ว วาดต่อไม่ได้อยู่ดี)
        // ปลด flag ชั่วคราวเพื่อไม่ให้ debug_assert ใน block_on ตีความผิด
        FRAME_LOOP_STARTED.store(false, Ordering::Relaxed);
        let stack = build_stack(
            self.window.as_ref(),
            width,
            height,
            &self.device_lost,
            &self.oom,
            next_generation,
            self.options.uncapped_present,
        );
        FRAME_LOOP_STARTED.store(true, Ordering::Relaxed);
        let stack = stack?;

        self.stack = Some(stack);
        self.generation = next_generation;
        self.consecutive_errors = 0;
        self.frames = 0;

        tracing::info!(generation = self.generation, "GPU device recovered");
        Ok(())
    }

    /// device หายและยังไม่ได้กู้หรือยัง
    #[must_use]
    pub fn needs_recovery(&self) -> bool {
        self.device_lost.load(Ordering::Relaxed) || self.looks_broken()
    }

    /// GPU แจ้ง out of memory หรือไม่ (เคลียร์ flag ในตัว)
    ///
    /// ชั้นบนควรตอบสนองด้วยการทิ้ง cache ชั้น T2/T1 แล้ว autosave (docs/04 §7)
    #[must_use]
    pub fn take_oom(&mut self) -> bool {
        self.oom.swap(false, Ordering::SeqCst)
    }

    /// รุ่นของ device ปัจจุบัน — เปลี่ยนเมื่อไหร่แปลว่า resource เดิมใช้ไม่ได้แล้ว
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// เปลี่ยนขนาด surface ตามหน้าต่าง
    ///
    /// ขนาด 0 (ย่อลง taskbar) จะถูกข้าม — `configure` ด้วย 0 ทำให้ driver ล้ม
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let Some(stack) = self.stack.as_mut() else {
            return; // กำลังกู้ device อยู่ — เดี๋ยว build_stack ใช้ขนาดล่าสุดเอง
        };
        if stack.config.width == width && stack.config.height == height {
            return; // ไม่มีอะไรเปลี่ยน — อย่า configure ซ้ำให้เสียเวลา
        }
        stack.config.width = width;
        stack.config.height = height;
        stack.surface.configure(&stack.device, &stack.config);
    }

    /// ขอเฟรมถัดไปจาก surface พร้อมจัดการทุก variant ของ wgpu 29
    ///
    /// spec: docs/04-rendering.md §7
    pub fn acquire_frame(&mut self) -> FrameStatus {
        use wgpu::CurrentSurfaceTexture as Cst;

        // ตั้งแต่จุดนี้ไป การเรียก block_on ถือว่าผิด (I-2)
        FRAME_LOOP_STARTED.store(true, Ordering::Relaxed);

        // จำลอง device lost ตามที่สั่งไว้ (เฉพาะ build ที่เปิด feature)
        #[cfg(feature = "force-device-lost")]
        if self.forced.due_by_frames(self.frames) {
            tracing::warn!(
                frames = self.frames,
                "simulating device lost (frame counter reached)"
            );
            self.device_lost.store(true, Ordering::SeqCst);
        }

        // device หายแล้ว — ห้ามแตะ surface ต่อ ต้องให้ชั้นบนกู้ก่อน
        if self.needs_recovery() {
            return FrameStatus::DeviceLost;
        }

        // กู้ device ไม่สำเร็จก่อนหน้านี้ — ไม่มีอะไรให้วาดแล้ว
        let Some(stack) = self.stack.as_ref() else {
            return FrameStatus::DeviceLost;
        };

        match stack.surface.get_current_texture() {
            Cst::Success(frame) => {
                self.consecutive_errors = 0;
                self.frames += 1;
                FrameStatus::Ready(frame)
            }

            // ยังวาดได้ แต่ config ไม่ตรงจอแล้ว — วาดเฟรมนี้ให้จบก่อนแล้วค่อยตั้งใหม่
            // ถ้าข้ามเลย ผู้ใช้จะเห็นจอค้างตอนลากขอบหน้าต่าง
            Cst::Suboptimal(frame) => {
                self.consecutive_errors = 0;
                self.frames += 1;
                stack.surface.configure(&stack.device, &stack.config);
                FrameStatus::Ready(frame)
            }

            // ★ หน้าต่างถูกบัง/ย่อลง หรือ driver ไม่ว่าง — ห้ามขอวาดใหม่ (I-1)
            Cst::Occluded | Cst::Timeout => FrameStatus::Skip,

            // surface ตายแต่ device ยังอยู่ — configure ใหม่พอ
            Cst::Lost | Cst::Outdated => {
                tracing::debug!("surface is no longer usable — reconfiguring it");
                stack.surface.configure(&stack.device, &stack.config);
                FrameStatus::Recovered
            }

            Cst::Validation => {
                self.consecutive_errors += 1;
                tracing::error!(
                    count = self.consecutive_errors,
                    "surface validation error — skipping this frame"
                );
                // ผิดพลาดติดกันหลายเฟรม = ไม่ใช่อาการชั่วคราวแล้ว ให้กู้ device
                if self.looks_broken() {
                    FrameStatus::DeviceLost
                } else {
                    FrameStatus::Skip
                }
            }
        }
    }

    /// เกิด validation error ติดกันจนน่าจะเป็น device เสียจริงหรือยัง
    #[must_use]
    pub fn looks_broken(&self) -> bool {
        self.consecutive_errors >= Self::MAX_CONSECUTIVE_ERRORS
    }

    /// context ยังใช้วาดได้อยู่ไหม
    ///
    /// เป็น `false` เฉพาะตอนกู้ device ไม่สำเร็จ — ชั้นบนต้องเลิกวาดแล้วปิดโปรแกรม
    /// อย่างเรียบร้อย (บันทึกงานก่อน) ไม่ใช่วาดต่อ
    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.stack.is_some()
    }

    /// เข้าถึงชิ้นส่วน GPU
    ///
    /// `stack` เป็น `None` ได้เฉพาะตอน [`RenderContext::recover`] ล้มเหลว
    /// ซึ่งจุดนั้นคืน `Err` ออกไปแล้ว และชั้นบนต้องหยุดวาดทันที
    /// การเรียก accessor ต่อหลังจากนั้นคือบั๊กของผู้เรียก ไม่ใช่ข้อมูลจากผู้ใช้
    #[expect(
        clippy::expect_used,
        reason = "invariant ภายใน: ชั้นบนต้องเช็ค is_usable() ก่อน ไม่ใช่ข้อมูลจากไฟล์/ผู้ใช้"
    )]
    #[track_caller]
    fn stack(&self) -> &GpuStack {
        self.stack
            .as_ref()
            .expect("เรียกใช้ GPU หลังกู้ device ไม่สำเร็จ — ต้องเช็ค is_usable() ก่อน")
    }

    /// GPU device
    #[must_use]
    pub fn device(&self) -> &wgpu::Device {
        &self.stack().device
    }

    /// คิวคำสั่งของ GPU
    #[must_use]
    pub fn queue(&self) -> &wgpu::Queue {
        &self.stack().queue
    }

    /// adapter ที่เลือกไว้
    #[must_use]
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.stack().adapter
    }

    /// instance ที่สร้าง surface
    #[must_use]
    pub fn instance(&self) -> &wgpu::Instance {
        &self.stack().instance
    }

    /// การตั้งค่า surface ปัจจุบัน
    #[must_use]
    pub fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.stack().config
    }

    /// รูปแบบสีของ surface (sRGB เสมอ)
    #[must_use]
    pub fn format(&self) -> wgpu::TextureFormat {
        self.stack().config.format
    }

    /// present mode ที่ใช้จริง
    ///
    /// ต้องรายงานค่านี้ทุกครั้งที่ลงตัวเลข benchmark — ถ้าเป็น `AutoVsync`
    /// แปลว่าตัวเลขชนเพดานจอ อ่านเป็นต้นทุนการวาดไม่ได้
    #[must_use]
    pub fn present_mode(&self) -> wgpu::PresentMode {
        self.stack().config.present_mode
    }

    /// ความสามารถของ GPU ที่ตรวจไว้
    #[must_use]
    pub fn capabilities(&self) -> &GpuCapabilities {
        &self.stack().caps
    }

    /// จำนวนเฟรมที่วาดสำเร็จนับจากการสร้าง/กู้ device ครั้งล่าสุด
    #[must_use]
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// ตัวเลือกที่สร้าง context นี้ขึ้นมา
    #[must_use]
    pub fn options(&self) -> &RenderOptions {
        &self.options
    }

    /// เวลาที่ต้องปลุก event loop มาจำลอง device lost — `None` = ไม่ต้องปลุก
    ///
    /// ★ เส้นทางนี้ **ไม่ฝืน I-1** เลย เพราะใช้ `ControlFlow::WaitUntil`
    /// เธรดหลับสนิทจนถึงเวลานั้น แล้วตื่นครั้งเดียว
    /// จำลองสถานการณ์จริง: driver อัปเดตตอนแอปเปิดทิ้งไว้ข้าง Photoshop
    #[must_use]
    pub fn forced_lost_deadline(&self) -> Option<std::time::Instant> {
        #[cfg(feature = "force-device-lost")]
        {
            self.forced.deadline()
        }
        #[cfg(not(feature = "force-device-lost"))]
        {
            None
        }
    }

    /// ถึงเวลาแล้ว — ยิง device lost หนึ่งครั้ง
    ///
    /// คืน `true` ถ้ายิงจริง เคลียร์นาฬิกาปลุกในตัวเพื่อไม่ให้ถูกปลุกซ้ำ
    pub fn fire_forced_device_lost(&mut self) -> bool {
        #[cfg(feature = "force-device-lost")]
        {
            if !self.forced.due_by_time(std::time::Instant::now()) {
                return false;
            }
            tracing::warn!("simulating device lost on the timer (app was idle)");
            self.device_lost.store(true, Ordering::SeqCst);
            true
        }
        #[cfg(not(feature = "force-device-lost"))]
        {
            false
        }
    }

    /// ต้องวาดเฟรมต่อเนื่องเพื่อไปให้ถึงจุดจำลอง device lost หรือยัง
    ///
    /// ★ เป็น **โหมดทดสอบเท่านั้น** — จงใจฝืน I-1 เพื่อให้นับเฟรมถึง N ได้
    /// โดยไม่ต้องให้คนขยับเมาส์ ในทางกลับกันแปลว่า build ปกติ (ไม่มี feature)
    /// คืน `false` เสมอ จึงไม่มีทางหลุดไปอยู่ใน release
    ///
    /// เหตุผลที่ต้องมี: พอ I-1 ทำงานถูกต้อง โปรแกรมจะวาดแค่ ~6 เฟรมตอนเปิด
    /// แล้วหลับ ตัวนับเฟรมจึงไม่มีวันถึง 120 ตามที่ ROADMAP สั่งให้ทดสอบ
    #[must_use]
    pub fn wants_forced_frames(&self) -> bool {
        #[cfg(feature = "force-device-lost")]
        {
            self.forced.wants_frames()
        }
        #[cfg(not(feature = "force-device-lost"))]
        {
            false
        }
    }
}

/// device หายแบบ "อุบัติเหตุ" ที่ต้องกู้ หรือเป็นเราเองที่สั่งปิด
///
/// ★ **ข้อผูกมัด: `Destroyed` ห้ามนับเป็นอุบัติเหตุ** (docs/04 §7)
/// wgpu ยิง `Destroyed` ตอนเรา drop device เอง ซึ่งเกิดขึ้น **ทุกครั้งที่กู้**
/// ถ้านับด้วย การกู้ครั้งหนึ่งจะจุดชนวนการกู้ครั้งถัดไปทันที → วนไม่รู้จบ
/// ผู้ใช้เห็นจอกระพริบไม่หยุดแล้วโปรแกรมกิน CPU 100% (ขัด I-1 ด้วย)
///
/// แยกเป็นฟังก์ชันบริสุทธิ์เพราะตัว callback เรียกจากในไส้ wgpu ทดสอบตรง ๆ ไม่ได้
#[must_use]
fn is_accidental_loss(reason: wgpu::DeviceLostReason) -> bool {
    match reason {
        // เราเป็นคนสั่งเอง (ตอนกู้ หรือตอนปิดโปรแกรม)
        wgpu::DeviceLostReason::Destroyed => false,
        // driver update / sleep-resume / TDR / สลับ GPU — ของจริงที่ต้องกู้
        _ => true,
    }
}

/// ตัวจำลอง device lost สำหรับทดสอบ (feature `force-device-lost` เท่านั้น)
///
/// ★ **ต้องยิงครั้งเดียวตลอดการรัน** ไม่ว่าจะสั่งด้วยตัวนับเฟรมหรือตัวนับเวลา
/// เพราะหลังกู้เสร็จ `frames` ถูกรีเซ็ตเป็น 0 ถ้าเงื่อนไขยังเป็นจริงอยู่
/// มันจะยิงซ้ำทันทีแล้วกลายเป็น **ลูปกู้ device ไม่รู้จบ** ซึ่งเป็นอาการเดียวกับ
/// ที่ข้อผูกมัดเรื่อง `DeviceLostReason::Destroyed` กันไว้ (docs/04 §7)
///
/// แยกออกมาเป็น struct เพื่อให้ทดสอบได้ **โดยไม่ต้องมี GPU หรือหน้าต่าง** —
/// `RenderContext` ทั้งก้อนต้องมี surface จริงจึงสร้างในเทสต์ไม่ได้
#[cfg(feature = "force-device-lost")]
#[derive(Debug)]
struct ForcedLoss {
    /// ยิงเมื่อวาดครบ N เฟรม — "device ตายระหว่างผู้ใช้ลากภาพ"
    after_frames: Option<u64>,
    /// ยิงเมื่อถึงเวลานี้ — "device ตายตอนแอปหลับ" (เคสจริงที่เจอบ่อยกว่า)
    deadline: Option<std::time::Instant>,
    /// ยิงไปแล้วหรือยัง — ★ ธงนี้คือสิ่งเดียวที่กันลูป
    fired: bool,
}

#[cfg(feature = "force-device-lost")]
impl ForcedLoss {
    fn new(options: &RenderOptions) -> Self {
        let deadline = options.force_device_lost_after_ms.map(|ms| {
            tracing::info!(ms, "device-lost simulation armed for the idle case");
            std::time::Instant::now() + std::time::Duration::from_millis(ms)
        });
        Self {
            after_frames: options.force_device_lost_after,
            deadline,
            fired: false,
        }
    }

    /// ถึงเวลายิงตามจำนวนเฟรมหรือยัง (เรียกทุกเฟรม)
    fn due_by_frames(&mut self, frames: u64) -> bool {
        let Some(after) = self.after_frames else {
            return false;
        };
        if self.fired || frames < after {
            return false;
        }
        self.fired = true;
        true
    }

    /// ถึงเวลายิงตามนาฬิกาหรือยัง (เรียกตอนถูกปลุก)
    fn due_by_time(&mut self, now: std::time::Instant) -> bool {
        let Some(deadline) = self.deadline else {
            return false;
        };
        if now < deadline {
            return false;
        }
        // เคลียร์ทั้งนาฬิกาและตั้งธง เพื่อกันยิงซ้ำทุกทาง
        self.deadline = None;
        self.fired = true;
        true
    }

    /// เวลาที่ต้องปลุก event loop มา — `None` = ไม่ต้องปลุก (หลับยาวได้ ตาม I-1)
    fn deadline(&self) -> Option<std::time::Instant> {
        self.deadline
    }

    /// ต้องวาดต่อเนื่องเพื่อให้ตัวนับเฟรมถึงเป้าหรือยัง
    ///
    /// ★ เงื่อนไขคือ "ยังไม่ยิง" ไม่ใช่ "frames < after" — ตัวยิงอยู่ต้นทาง
    /// `acquire_frame()` ของเฟรม*ถัดไป* ถ้าหยุดขอเฟรมตอน `frames == after`
    /// เฟรมนั้นจะไม่มีวันมา แล้วการทดสอบจะค้างรอตลอดกาล
    fn wants_frames(&self) -> bool {
        !self.fired && self.after_frames.is_some()
    }
}

/// สร้างชิ้นส่วน GPU ทั้งชุด + ติดตั้ง callback ของ device lost / OOM
fn build_stack(
    window: &dyn SurfaceSource,
    width: u32,
    height: u32,
    device_lost: &Arc<AtomicBool>,
    oom: &Arc<AtomicBool>,
    generation: u64,
    uncapped_present: bool,
) -> Result<GpuStack, DeviceError> {
    // ★★★ เลือก backend ก่อนแตะ GPU — และ **บอกใน log ทุกครั้งว่ามาจากไหน**
    //
    //   log ที่เขียนแค่ชื่อ backend ตอบไม่ได้ว่ามันมาจาก env ของคนที่กำลังไล่บั๊ก
    //   หรือเป็นค่าปริยาย ซึ่งเป็นคำถามแรกเสมอเวลาอ่าน log ที่คนอื่นส่งมาให้
    let compiled_in = wgpu::Instance::enabled_backend_features();
    let choice = choose_backends(std::env::var(BACKEND_ENV).ok().as_deref(), compiled_in)?;
    tracing::info!(
        backends = %backend_names(choice.backends),
        source = if choice.from_env { BACKEND_ENV } else { "built-in default" },
        compiled_in = %backend_names(compiled_in),
        "graphics backend chosen"
    );

    let instance = window.make_instance(choice.backends);
    let surface = window.make_surface(&instance)?;

    // LowPower: RefX เปิดค้างทั้งวันข้าง Photoshop ไม่ควรปลุก dGPU โดยไม่จำเป็น
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))?;

    let info = adapter.get_info();
    let bc_compression = adapter
        .features()
        .contains(wgpu::Features::TEXTURE_COMPRESSION_BC);

    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("refx-device"),
        required_features: wgpu::Features::empty(),
        // ขอเท่าที่ adapter ให้ได้จริง = ไม่มีทางขอเกินแล้วล้มตอนเปิดโปรแกรม
        required_limits: adapter.limits(),
        ..Default::default()
    }))?;

    // ---- callback: device หาย ----
    {
        let flag = Arc::clone(device_lost);
        device.set_device_lost_callback(move |reason, message| {
            if !is_accidental_loss(reason) {
                tracing::debug!(generation, "previous device destroyed on purpose");
                return;
            }
            tracing::error!(
                ?reason,
                message,
                generation,
                "GPU device lost — recovering on the next frame"
            );
            flag.store(true, Ordering::SeqCst);
        });
    }

    // ---- callback: out of memory ----
    {
        let flag = Arc::clone(oom);
        let lost = Arc::clone(device_lost);
        device.on_uncaptured_error(Arc::new(move |err: wgpu::Error| match err {
            wgpu::Error::OutOfMemory { .. } => {
                // ห้ามทำงานหนักใน callback นี้ — แค่ตั้ง flag แล้วให้เฟรมถัดไปจัดการ
                tracing::error!("GPU out of memory — will drop caches and autosave");
                flag.store(true, Ordering::SeqCst);
            }
            wgpu::Error::Validation { description, .. } => {
                tracing::error!(description, "wgpu validation error");
            }
            wgpu::Error::Internal { description, .. } => {
                tracing::error!(
                    description,
                    "wgpu internal error — treating the device as broken"
                );
                lost.store(true, Ordering::SeqCst);
            }
        }));
    }

    let caps = GpuCapabilities {
        adapter_name: info.name.clone(),
        backend: info.backend,
        device_type: info.device_type,
        bc_compression,
        max_texture_dimension_2d: device.limits().max_texture_dimension_2d,
    };

    let config = build_config(&surface, &adapter, width, height, uncapped_present)?;
    surface.configure(&device, &config);

    tracing::info!(
        adapter = %caps.adapter_name,
        backend = ?caps.backend,
        device_type = ?caps.device_type,
        bc = caps.bc_compression,
        format = ?config.format,
        generation,
        "graphics ready"
    );

    Ok(GpuStack {
        instance,
        adapter,
        device,
        queue,
        surface,
        config,
        caps,
    })
}

/// เลือก surface format ที่เป็น sRGB เสมอ + `AutoVsync`
///
/// docs/04 §6 บังคับ sRGB (ให้ GPU แปลง gamma ให้ฟรี ไม่ต้องทำใน shader)
fn build_config(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
    uncapped_present: bool,
) -> Result<wgpu::SurfaceConfiguration, DeviceError> {
    let caps = surface.get_capabilities(adapter);

    // ★ ห้าม hard-code format — ต้องเลือกจากที่ surface บอกว่ารองรับ
    let format = caps
        .formats
        .iter()
        .copied()
        .find(wgpu::TextureFormat::is_srgb)
        .ok_or(DeviceError::NoSupportedFormat)?;

    let alpha_mode = caps
        .alpha_modes
        .first()
        .copied()
        .unwrap_or(wgpu::CompositeAlphaMode::Auto);

    let present_mode = select_present_mode(&caps.present_modes, uncapped_present);

    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: width.max(1),
        height: height.max(1),
        present_mode,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    })
}

/// เลือก present mode
///
/// ปกติคือ `AutoVsync` เสมอ (docs/04 §8) — `uncapped` เปิดเฉพาะโหมด benchmark
/// เพื่อให้ตัวเลขที่วัดเป็นต้นทุนการวาดจริง ไม่ใช่คาบสัญญาณของจอ
fn select_present_mode(supported: &[wgpu::PresentMode], uncapped: bool) -> wgpu::PresentMode {
    if !uncapped {
        return wgpu::PresentMode::AutoVsync;
    }

    // Immediate ก่อน (ไม่รอ vsync เลย) แล้วค่อย Mailbox (ไม่บล็อกแต่ยัง sync ตอน present)
    for candidate in [wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox] {
        if supported.contains(&candidate) {
            tracing::info!(?candidate, "benchmark mode: vsync disabled");
            return candidate;
        }
    }

    tracing::warn!(
        ?supported,
        "this GPU supports neither Immediate nor Mailbox — the numbers will hit the display's vsync ceiling and cannot be read as draw cost"
    );
    wgpu::PresentMode::AutoVsync
}

/// รัน future ที่ resolve ทันทีบน native
///
/// ทำไมเขียนเอง: `pollster` ไม่มีใน docs/09-crate-versions.md และ ADR-004 ห้าม
/// async runtime — ใช้เฉพาะตอน init/กู้ device ไม่เคยอยู่ในเส้นทางเฟรมปกติ
///
/// `Waker::noop()` เป็น safe API จึงไม่ต้องใช้ `unsafe` (I-5)
fn block_on<F: Future>(fut: F) -> F::Output {
    // docs/09: กันไม่ให้ block_on หลุดไปอยู่บนเส้นทางต่อเฟรม
    // ถ้าวันไหนมีคนเรียกจากใน redraw() เทสต์ debug จะจับได้ทันที
    debug_assert!(
        !FRAME_LOOP_STARTED.load(Ordering::Relaxed),
        "block_on ถูกเรียกหลังเข้าลูปเฟรมแล้ว — ห้ามบล็อก UI thread (I-2)"
    );
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            // บน native wgpu resolve ตั้งแต่ poll แรก กิ่งนี้แทบไม่ถูกใช้
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

/// ชื่อ env ที่ CI ใช้บอกว่า **job นี้ต้องมี GPU จริง** (docs/08 §3.9 ข้อ 7)
#[cfg(test)]
pub(crate) const REQUIRE_GPU_ENV: &str = "REFX_REQUIRE_GPU";

/// ค่าใน env นี้แปลว่า "ต้องมี GPU" หรือไม่
///
/// ★ ว่างเปล่าต้องแปลว่า **ไม่บังคับ** — GitHub Actions ตั้ง env เป็นสตริงว่าง
/// เมื่อ expression ไม่เข้าเงื่อนไข ถ้าตีความว่า "มีค่า = บังคับ" job ที่ไม่ควร
/// บังคับจะแดงทันทีโดยไม่มีใครเข้าใจว่าทำไม
#[cfg(test)]
pub(crate) fn gpu_required_from(value: Option<&str>) -> bool {
    matches!(value, Some(v) if !v.is_empty() && v != "0")
}

/// ไม่มี adapter ให้ใช้ — จะข้ามหรือจะล้ม
///
/// ★ docs/08 §3.9 ข้อ 7: การข้ามพร้อมพิมพ์เหตุผลถูกต้องในระดับ job เดียว
/// แต่ต้องมีอย่างน้อยหนึ่ง job ที่**บังคับ**ว่าต้องรันจริง ไม่งั้นทั้ง matrix
/// ข้ามพร้อมกันแล้ว CI ยังเขียว = ไม่มีใครตรวจเลย
///
/// แยกเป็นฟังก์ชันเพื่อให้ **ทดสอบสาขา "บังคับแล้วไม่มี" ได้โดยไม่ต้องถอดการ์ดจอ**
///
/// # Panics
/// panic เมื่อ `required` เป็นจริง — นั่นคือพฤติกรรมที่ต้องการบน CI
#[cfg(test)]
pub(crate) fn no_adapter_available(required: bool) {
    assert!(
        !required,
        "ตั้ง {REQUIRE_GPU_ENV}=1 ไว้แต่หา GPU adapter ไม่เจอ — job นี้ถูกกำหนดให้เป็น \
         job ที่รันเทสต์ GPU จริง (docs/08 §3.9 ข้อ 7) ถ้า runner ไม่มี software adapter \
         ให้แก้ที่ CI ไม่ใช่ปลดการบังคับทิ้ง ไม่งั้นจะไม่เหลือใครตรวจกลุ่มนี้เลย"
    );
    println!("ข้าม: เครื่องนี้ไม่มี GPU ที่ใช้ได้ (ไม่ได้ตั้ง {REQUIRE_GPU_ENV})");
}

/// GPU สำหรับเทสต์ หรือ `None` ถ้าเครื่องนี้ไม่มี (และไม่ได้บังคับไว้)
#[cfg(test)]
pub(crate) fn gpu_for_test() -> Option<(wgpu::Device, wgpu::Queue, GpuCapabilities)> {
    if let Some(gpu) = headless_device() {
        return Some(gpu);
    }
    no_adapter_available(gpu_required_from(
        std::env::var(REQUIRE_GPU_ENV).ok().as_deref(),
    ));
    None
}

/// GPU จริงแบบไม่มีหน้าต่าง — สำหรับเทสต์ที่ต้องแตะ texture จริง (P0-5)
///
/// ★ **`None` = เครื่องนี้ไม่มี GPU ที่ใช้ได้** ผู้เรียกต้องรายงานว่า "ข้าม"
/// อย่างชัดเจน ห้ามผ่านเงียบ ๆ (docs/08 §3.9 ข้อ 2) — ใช้ [`gpu_for_test`] แทน
/// การเรียกตัวนี้ตรง ๆ เพื่อให้ได้กติกาการข้าม/ล้มชุดเดียวกันทั้งโปรเจกต์
///
/// ไม่ต้องมี surface เพราะเทสต์พวกนี้ตรวจ **resource ที่ผูกกับ device**
/// (atlas, working texture) ไม่ได้ตรวจการ present ลงหน้าต่าง
#[cfg(test)]
pub(crate) fn headless_device() -> Option<(wgpu::Device, wgpu::Queue, GpuCapabilities)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let ask = |force_fallback_adapter| {
        block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter,
        }))
    };
    // ★ ไม่มีการ์ดจริงก็ยังเทสต์ได้ — ขอ software adapter แทน
    //   (WARP ที่ติดมากับ Windows · lavapipe จาก mesa-vulkan-drivers บน Linux)
    //   CI ไม่มี GPU จริง ถ้าไม่ลองขั้นนี้ เทสต์กลุ่ม GPU จะไม่มีวันได้รันบน CI เลย
    let adapter = match ask(false) {
        Ok(adapter) => adapter,
        Err(err) => {
            println!("ไม่มีการ์ดจอจริง ({err}) — ลอง software adapter");
            ask(true).ok()?
        }
    };

    let info = adapter.get_info();
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("refx-test-device"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .ok()?;

    let caps = GpuCapabilities {
        adapter_name: info.name.clone(),
        backend: info.backend,
        device_type: info.device_type,
        bc_compression: adapter
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
        max_texture_dimension_2d: device.limits().max_texture_dimension_2d,
    };
    Some((device, queue, caps))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// ไม่ตั้ง env = ได้ทุกตัวที่คอมไพล์เข้ามา และ log ต้องบอกว่าเป็นค่าปริยาย
    #[test]
    fn with_no_environment_variable_the_choice_is_everything_this_build_contains() {
        let built = wgpu::Backends::VULKAN | wgpu::Backends::DX12;
        for raw in [None, Some(""), Some("   ")] {
            let choice = choose_backends(raw, built).expect("ค่าว่างต้องไม่ใช่ error");
            assert_eq!(choice.backends, built, "{raw:?}");
            assert!(!choice.from_env, "{raw:?} ไม่ได้มาจาก env");
        }
    }

    /// ★★★ ชื่อผิด **ต้องล้ม ไม่ใช่ตกไปค่าปริยาย** — และต้องบอกว่ามีชื่อไหนให้เลือก
    ///
    /// `wgpu::Backends::from_comma_list` เลือกทาง "เตือนแล้วข้าม" ซึ่งแปลว่าพิมพ์
    /// ผิดหนึ่งตัวอักษรแล้วได้ backend อื่นมาเงียบ ๆ · สำหรับเครื่องมือที่มีไว้
    /// **ไล่บั๊ก** นั่นคือการตอบคำถามที่ไม่ได้ถาม
    #[test]
    fn a_name_that_is_not_a_backend_stops_the_program_and_lists_the_real_ones() {
        let err = choose_backends(Some("vulcan"), wgpu::Backends::all()).expect_err("ต้องล้ม");
        let shown = err.to_string();
        assert!(shown.contains("vulcan"), "ไม่ได้บอกว่าพิมพ์อะไรมา: {shown}");
        for name in ["vulkan", "dx12", "gl"] {
            assert!(shown.contains(name), "ไม่ได้บอกชื่อ {name}: {shown}");
        }
    }

    /// ★★★ ชื่อถูกแต่ **build นี้ไม่มี** ก็ต้องล้ม พร้อมบอกว่ามีตัวไหนอยู่จริง
    ///
    /// นี่คือข้อที่ทำให้ env **ไม่ผ่อนประตู** `the_backend_that_hangs_the_window…`
    /// ข้างล่าง: สิ่งที่ไม่ได้คอมไพล์เข้ามา ไม่มีทางถูกเลือกด้วยตัวแปรสภาพแวดล้อม
    #[test]
    fn asking_for_a_backend_this_build_does_not_contain_fails_loudly() {
        let built = wgpu::Backends::VULKAN | wgpu::Backends::DX12;
        let err = choose_backends(Some("gl"), built).expect_err("gl ไม่ได้คอมไพล์มา ต้องล้ม");
        let shown = err.to_string();
        assert!(shown.contains("gl"), "ไม่ได้บอกว่าตัวไหนขาด: {shown}");
        assert!(shown.contains("vulkan"), "ไม่ได้บอกว่ามีตัวไหนอยู่: {shown}");
        assert!(shown.contains("dx12"), "ไม่ได้บอกว่ามีตัวไหนอยู่: {shown}");
    }

    /// ★★ ขอหลายตัวแล้วมีตัวหนึ่งขาด = ล้ม **ไม่ใช่เงียบ ๆ ให้เท่าที่มี**
    #[test]
    fn asking_for_two_when_only_one_exists_never_quietly_gives_the_one() {
        let built = wgpu::Backends::VULKAN;
        let err = choose_backends(Some("vulkan,gl"), built).expect_err("ขาดไปหนึ่งตัวก็ต้องล้ม");
        assert!(err.to_string().contains("gl"), "{err}");
    }

    /// ชื่อที่ถูกและมีอยู่จริง — รับทั้งชื่อเต็มและชื่อเล่น และไม่สนตัวพิมพ์
    #[test]
    fn the_names_wgpu_accepts_are_the_names_we_accept() {
        let built = wgpu::Backends::all();
        for (raw, want) in [
            ("vulkan", wgpu::Backends::VULKAN),
            ("VK", wgpu::Backends::VULKAN),
            ("  dx12 ", wgpu::Backends::DX12),
            ("d3d12", wgpu::Backends::DX12),
            ("gles", wgpu::Backends::GL),
            ("opengl", wgpu::Backends::GL),
            ("mtl", wgpu::Backends::METAL),
        ] {
            let result = choose_backends(Some(raw), built);
            assert!(result.is_ok(), "{raw:?} ควรใช้ได้: {result:?}");
            let choice = result.expect("ตรวจด้วย is_ok ไปแล้วบรรทัดบน");
            assert_eq!(choice.backends, want, "{raw:?}");
            assert!(choice.from_env, "{raw:?} ต้องนับว่ามาจาก env");
        }
        let both = choose_backends(Some("vulkan,dx12"), built).expect("สองตัวพร้อมกัน");
        assert_eq!(both.backends, wgpu::Backends::VULKAN | wgpu::Backends::DX12);
    }

    /// ★ ชื่อที่พิมพ์ออกต้องมีหนึ่งชื่อต่อหนึ่ง backend — ไม่ใช่ "vulkan, vk"
    #[test]
    fn the_printed_names_have_no_duplicates_for_the_same_backend() {
        assert_eq!(backend_names(wgpu::Backends::VULKAN), "vulkan");
        assert_eq!(
            backend_names(wgpu::Backends::VULKAN | wgpu::Backends::DX12),
            "vulkan, dx12"
        );
        assert_eq!(backend_names(wgpu::Backends::empty()), "(none)");
    }

    /// ★★★ **GL ต้องไม่ถูกคอมไพล์เข้ามาบน Windows — นี่คือบั๊กเสถียรภาพ ไม่ใช่เรื่องขนาด**
    ///
    /// 14 ก.ย. 2026: `PostMessage(WM_INPUTLANGCHANGEREQUEST)` ทำหน้าต่างของ RefX
    /// **หยุดตอบภายใน 4–5 ข้อความ** มาตั้งแต่ 6 ก.ย. · ตัวการคือ backend `gles`
    /// ของ wgpu ซึ่งบน Windows ลาก WGL/EGL (`glutin_wgl_sys`) เข้ามาด้วย
    ///
    /// พิสูจน์ด้วยตัวแปรเดียว บนไบนารีเดียวกันทุกอย่างยกเว้น feature นี้:
    ///
    /// | build | ผล |
    /// |---|---|
    /// | ไม่มี `gles` | **รอด 8/8** · layout สลับได้ทุกครั้ง |
    /// | มี `gles` | **ค้างที่ข้อความที่ 4** · layout ค้างตั้งแต่ข้อความที่ 2 |
    ///
    /// ★ และบันไดที่ไล่มาก่อนหน้า (winit เปล่า · +IME · +wgpu surface · +egui
    /// · +user event · release) **รอดทุกขั้น** — ทั้งหมดนั้นไม่มี `gles` อยู่เลย
    ///
    /// เทสต์นี้ถามคำถามที่ **ไม่ต้องมี GPU** เพราะ `enabled_backend_features()`
    /// ตอบจากสิ่งที่ **คอมไพล์เข้ามา** ไม่ใช่จากเครื่องที่รัน
    #[test]
    fn the_backend_that_hangs_the_window_is_not_compiled_in() {
        let enabled = wgpu::Instance::enabled_backend_features();

        #[cfg(windows)]
        {
            assert!(
                !enabled.contains(wgpu::Backends::GL),
                "backend `gles` กลับเข้ามาใน build ของ Windows — \
                 มันทำให้หน้าต่างหยุดตอบเมื่อโปรแกรมสลับภาษาของคนอื่นส่ง \
                 WM_INPUTLANGCHANGEREQUEST มา (HANDOFF §2.53)"
            );
            // ★ และทางถอยต้องยังครบสองทางตาม `docs/08 §6`
            assert!(enabled.contains(wgpu::Backends::DX12), "DX12 หายไป");
            assert!(enabled.contains(wgpu::Backends::VULKAN), "Vulkan หายไป");
        }

        #[cfg(target_os = "linux")]
        {
            // บน Linux ไม่มีอาการนี้ (ไม่มี WGL) และ GL คือทางถอยที่สเปกบังคับ
            assert!(enabled.contains(wgpu::Backends::VULKAN), "Vulkan หายไป");
            assert!(enabled.contains(wgpu::Backends::GL), "ทางถอย GL หายไป");
        }

        // ทุกแพลตฟอร์ม: ต้องมีอย่างน้อยสองทาง ไม่งั้น driver มีปัญหาแล้วจบเลย
        assert!(
            enabled.iter().count() >= 2,
            "เหลือ backend เดียว = ไม่มีทางถอย: {enabled:?}"
        );
    }

    fn fake_caps(max_dim: u32) -> GpuCapabilities {
        GpuCapabilities {
            adapter_name: "test".to_owned(),
            backend: wgpu::Backend::Noop,
            device_type: wgpu::DeviceType::Cpu,
            bc_compression: false,
            max_texture_dimension_2d: max_dim,
        }
    }

    #[test]
    fn block_on_returns_ready_value() {
        assert_eq!(block_on(std::future::ready(42)), 42);
    }

    #[test]
    fn atlas_size_is_capped_at_2048() {
        assert_eq!(fake_caps(16384).atlas_size(), 2048);
    }

    #[test]
    fn atlas_size_respects_small_gpu() {
        // การ์ดเล็กต้องได้ atlas เล็กตาม ไม่ใช่ 2048 แล้วล้มตอน allocate
        assert_eq!(fake_caps(1024).atlas_size(), 1024);
    }

    /// ★ ประตูบานเดียวที่บอกว่า "job นี้ตรวจ GPU จริงไหม" (docs/08 §3.9 ข้อ 7)
    ///
    /// รันได้ → พิมพ์ชื่อ adapter ลง log ของ CI เป็นหลักฐานว่ามีคนตรวจจริง
    /// รันไม่ได้ → ข้ามพร้อมเหตุผล **เว้นแต่** job นั้นตั้ง `REFX_REQUIRE_GPU`
    /// ไว้ ซึ่งแปลว่ามันคือ job ที่รับหน้าที่ตรวจกลุ่มนี้ให้ทั้ง matrix → ต้องแดง
    #[test]
    fn gpu_tests_run_where_they_are_required() {
        match gpu_for_test() {
            Some((_, _, caps)) => println!(
                "GPU tests: ทำงานจริงบน {} ({:?} / {:?})",
                caps.adapter_name, caps.backend, caps.device_type
            ),
            None => println!("GPU tests: ข้าม — job นี้ไม่ได้ถูกกำหนดให้ตรวจ GPU"),
        }
    }

    /// ค่าที่ CI ส่งมาต้องถูกตีความให้ถูก — โดยเฉพาะ **สตริงว่าง**
    ///
    /// GitHub Actions ตั้ง env เป็นสตริงว่างเมื่อ expression ไม่เข้าเงื่อนไข
    /// ถ้าตีความว่า "มีค่า = บังคับ" job ฝั่ง Linux จะแดงทันทีโดยไม่มีใครเข้าใจว่าทำไม
    #[test]
    fn empty_env_never_means_required() {
        assert!(gpu_required_from(Some("1")));
        assert!(gpu_required_from(Some("true")));
        assert!(!gpu_required_from(Some("")), "สตริงว่าง = ไม่บังคับ");
        assert!(!gpu_required_from(Some("0")));
        assert!(!gpu_required_from(None));
    }

    /// ★ negative control ของกลไกข้อ 7 เอง (docs/08 §3.9 ข้อ 1)
    ///
    /// พิสูจน์ว่าสาขา "บังคับไว้แต่ไม่มี GPU" **ล้มจริง** โดยไม่ต้องถอดการ์ดจอ
    /// ถ้าสาขานี้ไม่ล้ม การตั้ง `REFX_REQUIRE_GPU` บน CI ก็ไม่มีความหมายอะไรเลย
    #[test]
    #[should_panic(expected = "REFX_REQUIRE_GPU")]
    fn requiring_a_gpu_that_is_missing_turns_ci_red() {
        no_adapter_available(true);
    }

    /// ไม่ได้บังคับไว้ = ข้ามเงียบ ๆ ได้ (แต่พิมพ์เหตุผลไว้ใน log)
    #[test]
    fn missing_gpu_without_the_flag_is_only_a_skip() {
        no_adapter_available(false);
    }

    // ---------- ★ กฎที่กันลูปกู้ device ไม่รู้จบ (P0-5 / docs/04 §7) ----------

    /// ★ ข้อผูกมัด: `Destroyed` = เราสั่งปิดเอง **ห้ามนับเป็นอุบัติเหตุ**
    ///
    /// ทุกครั้งที่กู้ device เราทิ้งของเก่า → wgpu ยิง `Destroyed` เสมอ
    /// ถ้านับด้วย การกู้ครั้งหนึ่งจะจุดชนวนครั้งถัดไปทันที = จอกระพริบไม่หยุด
    #[test]
    fn destroying_our_own_device_is_never_an_accident() {
        assert!(
            !is_accidental_loss(wgpu::DeviceLostReason::Destroyed),
            "นับ Destroyed เป็นอุบัติเหตุ = กู้วนไม่รู้จบ"
        );
        // ของจริงที่ต้องกู้: driver update / sleep-resume / TDR / สลับ GPU
        assert!(is_accidental_loss(wgpu::DeviceLostReason::Unknown));
    }

    #[cfg(feature = "force-device-lost")]
    mod forced {
        use std::time::{Duration, Instant};

        use super::*;

        fn by_frames(after: u64) -> ForcedLoss {
            ForcedLoss::new(&RenderOptions {
                force_device_lost_after: Some(after),
                ..RenderOptions::default()
            })
        }

        fn by_time(ms: u64) -> ForcedLoss {
            ForcedLoss::new(&RenderOptions {
                force_device_lost_after_ms: Some(ms),
                ..RenderOptions::default()
            })
        }

        /// ★ ยิงครั้งเดียวเท่านั้น — หลังกู้เสร็จตัวนับเฟรมถูกรีเซ็ตเป็น 0
        /// ถ้ายิงซ้ำได้ จะกลายเป็นลูปกู้ device ไม่รู้จบทันที
        #[test]
        fn frame_trigger_fires_exactly_once_even_after_the_counter_resets() {
            let mut forced = by_frames(3);
            assert!(!forced.due_by_frames(0));
            assert!(!forced.due_by_frames(2), "ยังไม่ถึงเป้าต้องไม่ยิง");
            assert!(forced.due_by_frames(3), "ถึงเป้าแล้วต้องยิง");

            // จำลองสิ่งที่เกิดหลังกู้: frames กลับไปนับหนึ่งใหม่แล้ววิ่งผ่านเป้าอีกรอบ
            for frames in [0, 1, 2, 3, 4, 100] {
                assert!(
                    !forced.due_by_frames(frames),
                    "ยิงซ้ำที่เฟรม {frames} = ลูปกู้ device ไม่รู้จบ"
                );
            }
        }

        /// ★ ตัวนับเวลาก็ต้องยิงครั้งเดียว และต้อง **เคลียร์นาฬิกาปลุก** ด้วย
        /// ไม่งั้น event loop จะถูกปลุกซ้ำทุกครั้งที่หลับ = เผา CPU ทั้งที่เขียนว่า Wait (I-1)
        #[test]
        fn time_trigger_fires_once_then_stops_waking_the_loop() {
            let mut forced = by_time(0); // ถึงเวลาทันที
            assert!(forced.deadline().is_some(), "ต้องตั้งนาฬิกาปลุกไว้");

            let now = Instant::now() + Duration::from_millis(1);
            assert!(forced.due_by_time(now), "ถึงเวลาแล้วต้องยิง");
            assert_eq!(
                forced.deadline(),
                None,
                "ยิงแล้วต้องเลิกขอให้ปลุก ไม่งั้นตื่นซ้ำไม่รู้จบ (I-1)"
            );
            for _ in 0..10 {
                assert!(!forced.due_by_time(Instant::now()), "ยิงซ้ำ = กู้วนไม่จบ");
            }
        }

        /// ยังไม่ถึงเวลา = ห้ามยิง และนาฬิกาต้องยังอยู่
        #[test]
        fn time_trigger_waits_for_its_deadline() {
            let mut forced = by_time(60_000);
            assert!(!forced.due_by_time(Instant::now()));
            assert!(forced.deadline().is_some(), "ยังไม่ถึงเวลา ต้องยังปลุกอยู่");
        }

        /// ไม่ได้สั่งจำลองไว้ = ต้องไม่ยิงเลย และไม่ขอให้ปลุก (เส้นทางปกติของผู้ใช้)
        #[test]
        fn without_a_flag_nothing_ever_fires() {
            let mut forced = ForcedLoss::new(&RenderOptions::default());
            assert!(!forced.due_by_frames(u64::MAX));
            assert!(!forced.due_by_time(Instant::now()));
            assert_eq!(forced.deadline(), None);
            assert!(!forced.wants_frames(), "ไม่ได้สั่งไว้ ต้องไม่ฝืนวาดต่อเนื่อง (I-1)");
        }

        /// ★ ต้องขอเฟรมต่อไปเรื่อย ๆ **จนกว่าจะยิงจริง** ไม่ใช่หยุดตอน frames == after
        ///
        /// ตัวยิงอยู่ต้นทาง `acquire_frame()` ของเฟรมถัดไป ถ้าหยุดขอก่อน
        /// เฟรมนั้นไม่มีวันมา แล้วการทดสอบจะค้างรอตลอดกาล (เจอจริงตอน P0-5)
        #[test]
        fn keeps_asking_for_frames_until_it_actually_fires() {
            let mut forced = by_frames(2);
            assert!(forced.wants_frames());
            assert!(!forced.due_by_frames(1));
            assert!(forced.wants_frames(), "ยังไม่ยิง ต้องขอเฟรมต่อ");
            assert!(forced.due_by_frames(2));
            assert!(!forced.wants_frames(), "ยิงแล้วต้องปล่อยให้กลับไปหลับ (I-1)");
        }
    }

    /// flag ของ device lost ต้องถูกตั้ง/เคลียร์ได้ข้ามเธรด (callback ของ wgpu
    /// ไม่รับประกันว่าจะถูกเรียกบนเธรดไหน)
    #[test]
    fn device_lost_flag_is_shareable_across_threads() {
        let flag = Arc::new(AtomicBool::new(false));
        let worker = Arc::clone(&flag);
        std::thread::spawn(move || worker.store(true, Ordering::SeqCst))
            .join()
            .unwrap();
        assert!(flag.load(Ordering::SeqCst));
    }
}
