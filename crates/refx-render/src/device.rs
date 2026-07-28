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
}

/// แหล่งที่สร้าง surface ได้ **ซ้ำหลายครั้ง**
///
/// ต้องสร้างซ้ำได้เพราะตอนกู้ device lost เราทิ้ง `Instance` เดิมทั้งก้อน
/// แล้วสร้างใหม่ทั้งชุด — ถ้าเก็บแค่ `Surface` ไว้จะกู้ไม่ได้
///
/// มี blanket impl ให้ทุกอย่างที่เป็น window handle อยู่แล้ว ชั้นบนไม่ต้อง implement เอง
pub trait SurfaceSource: Send + Sync + 'static {
    /// สร้าง `Instance` ใหม่ (พร้อม display handle สำหรับ Wayland)
    fn make_instance(&self) -> wgpu::Instance;
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
    fn make_instance(&self) -> wgpu::Instance {
        // ส่ง display handle เข้าไปด้วย — Linux/Wayland ต้องใช้เลือก backend ให้ถูก
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(Box::new(
            self.clone(),
        )))
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
    /// จำลอง device lost ไปแล้วหรือยัง — ต้องยิงครั้งเดียว
    /// ไม่งั้นหลังกู้เสร็จ `frames` รีเซ็ตเป็น 0 แล้วจะยิงซ้ำเป็นลูปไม่รู้จบ
    ///
    /// มีเฉพาะ build ทดสอบ — release ไม่ต้องแบกฟิลด์นี้เลย
    #[cfg(feature = "force-device-lost")]
    forced_once: bool,

    /// เวลาที่ต้องปลุกมาจำลอง device lost — `None` = ไม่ได้ตั้งไว้ หรือยิงไปแล้ว
    #[cfg(feature = "force-device-lost")]
    forced_deadline: Option<std::time::Instant>,
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
                "ระบุ flag จำลอง device lost ไว้ แต่ไม่ได้เปิด feature force-device-lost — จะไม่มีผล"
            );
        }

        // ตั้งนาฬิกาปลุกไว้ตั้งแต่ตอนสร้าง เพื่อให้เวลานับจาก "เปิดโปรแกรม"
        // ไม่ใช่จาก "เฟรมแรก" — ตรงกับสถานการณ์จริงที่แอปหลับอยู่
        #[cfg(feature = "force-device-lost")]
        let forced_deadline = options.force_device_lost_after_ms.map(|ms| {
            tracing::info!(ms, "ตั้งเวลาจำลอง device lost ตอนแอปหลับ");
            std::time::Instant::now() + std::time::Duration::from_millis(ms)
        });

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
            forced_once: false,
            #[cfg(feature = "force-device-lost")]
            forced_deadline,
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

        tracing::warn!(generation = next_generation, "กำลังสร้าง GPU device ใหม่");

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

        tracing::info!(generation = self.generation, "กู้ GPU device สำเร็จ");
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
        if let Some(after) = self.options.force_device_lost_after
            && !self.forced_once
            && self.frames >= after
        {
            tracing::warn!(frames = self.frames, "จำลอง device lost ตามคำสั่ง");
            self.forced_once = true;
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
                tracing::debug!("surface ใช้ไม่ได้แล้ว — ตั้งค่าใหม่");
                stack.surface.configure(&stack.device, &stack.config);
                FrameStatus::Recovered
            }

            Cst::Validation => {
                self.consecutive_errors += 1;
                tracing::error!(
                    count = self.consecutive_errors,
                    "surface validation error — ข้ามเฟรมนี้"
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
            self.forced_deadline
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
            let Some(deadline) = self.forced_deadline else {
                return false;
            };
            if std::time::Instant::now() < deadline {
                return false;
            }
            // เคลียร์ทั้งนาฬิกาและตั้ง forced_once เพื่อกันยิงซ้ำทุกทาง
            self.forced_deadline = None;
            self.forced_once = true;
            tracing::warn!("จำลอง device lost ตามเวลา (แอปหลับอยู่)");
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
            // ★ เงื่อนไขคือ "ยังไม่ยิง" ไม่ใช่ "frames < after"
            //   ถ้าใช้ frames < after ลูปจะหยุดพอดีตอน frames == after
            //   แต่ตัวยิงอยู่ต้นทาง acquire_frame() ของเฟรม*ถัดไป* ซึ่งไม่มีวันมา
            //   → ต้องขอเฟรมต่อไปอีกหนึ่งครั้งเสมอจนกว่าจะยิงจริง
            !self.forced_once && self.options.force_device_lost_after.is_some()
        }
        #[cfg(not(feature = "force-device-lost"))]
        {
            false
        }
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
    let instance = window.make_instance();
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
            // ★ Destroyed = เราเป็นคนทิ้ง device เอง (ตอนกู้ หรือตอนปิดโปรแกรม)
            //   ห้ามถือเป็นอุบัติเหตุ ไม่งั้นการกู้ครั้งหนึ่งจะจุดชนวนการกู้ครั้งถัดไปไม่รู้จบ
            if matches!(reason, wgpu::DeviceLostReason::Destroyed) {
                tracing::debug!(generation, "ปิด device เดิมตามปกติ");
                return;
            }
            tracing::error!(?reason, message, generation, "GPU device หาย — จะกู้เฟรมถัดไป");
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
                tracing::error!("GPU หน่วยความจำเต็ม — จะทิ้ง cache แล้วบันทึกงานอัตโนมัติ");
                flag.store(true, Ordering::SeqCst);
            }
            wgpu::Error::Validation { description, .. } => {
                tracing::error!(description, "wgpu validation error");
            }
            wgpu::Error::Internal { description, .. } => {
                tracing::error!(description, "wgpu internal error — ถือว่า device เสีย");
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
        "เตรียมกราฟิกเสร็จ"
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
            tracing::info!(?candidate, "โหมด benchmark: ปลด vsync แล้ว");
            return candidate;
        }
    }

    tracing::warn!(
        ?supported,
        "การ์ดจอนี้ไม่รองรับ Immediate/Mailbox — ตัวเลขที่วัดได้จะชนเพดาน vsync ของจอ อ่านเป็นต้นทุนการวาดไม่ได้"
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

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
