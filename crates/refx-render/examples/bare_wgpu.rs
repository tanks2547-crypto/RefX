//! ★★★ หน้าต่าง `winit` + พื้นผิว `wgpu` — **ขั้นที่สองของบันไดหาตัวการ**
//!
//! ## บันไดที่กำลังไต่อยู่
//!
//! `PostMessage(WM_INPUTLANGCHANGEREQUEST)` ทำ RefX ค้างทุกครั้งภายใน 4–5 ข้อความ
//! แต่หน้าต่าง `winit` เปล่า ๆ **ไม่ค้างเลย** (8/8) ทั้งแบบเปิด IME ครั้งเดียว
//! และแบบยิง `set_ime_allowed` ซ้ำทุกรอบ → ตัวการอยู่เหนือ winit ขึ้นมา
//!
//! | ขั้น | มีอะไร | ผล |
//! |---|---|---|
//! | 1 | winit เปล่า | รอด 8/8 |
//! | 1ก | winit + IME ครั้งเดียว | รอด 8/8 |
//! | 1ข | winit + IME ทุกรอบ | รอด 8/8 |
//! | 2 | winit + wgpu surface (ไฟล์นี้) | **รอด 8/8** |
//! | 3–5 | + egui · + user event · release | **รอด 8/8 ทุกขั้น** |
//!
//! ★★★ **คำตอบไม่ได้อยู่ในบันไดนี้** — ตัวการคือ backend `gles` ของ wgpu ซึ่ง
//! ไม่มีขั้นไหนคอมไพล์เข้ามาเลย · ดูหัวไฟล์ของ `refx-ui/examples/bare_egui.rs`
//!
//! ★★ **ห้ามกระโดดข้ามขั้น** — ถ้าขั้นนี้ค้าง เราได้คำตอบโดยไม่ต้องสงสัย egui เลย
//! ถ้าไม่ค้าง egui คือผู้ต้องสงสัยที่เหลืออยู่คนเดียว · การใส่ทั้งสองอย่างพร้อมกัน
//! แล้วเห็นมันค้าง **ตอบไม่ได้ว่าเป็นเพราะอันไหน** (`docs/08 §3.9` ข้อ 9)
//!
//! ## ทำไมพื้นผิวถึงน่าสงสัย
//!
//! การ present เฟรมคุยกับ **DWM ซึ่งเป็นอีกโปรเซส** · เธรดที่ค้างของ RefX อยู่ใน
//! `EventPairLow` = กำลังรอการเรียกข้ามโปรเซสอยู่พอดี · ไฟล์นี้จึง present จริง
//! ทุกเฟรมเหมือน RefX ไม่ใช่แค่เปิดหน้าต่างเฉย ๆ
//!
//! ## ใช้
//!
//! ```text
//! cargo run -p refx-render --example bare_wgpu
//! scripts/lang-switch-bare.ps1 -ProcessName bare_wgpu
//! ```

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const TITLE: &str = "refx-bare-wgpu";

/// ตัวเดียวกับที่ `device.rs` ใช้ — wgpu บน native resolve ตั้งแต่ poll แรก
fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

#[derive(Default)]
struct Bare {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    frames: u64,
}

impl Bare {
    /// สร้าง device + surface แบบเดียวกับ `refx-render` (ย่อเฉพาะที่จำเป็น)
    fn start_gpu(window: &Arc<Window>) -> Option<Gpu> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(Arc::clone(window)).ok()?;
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok()?;
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("bare-wgpu"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .ok()?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| caps.formats.first().copied())?;
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        println!("gpu ready: {:?} / {:?}", adapter.get_info().name, format);
        Some(Gpu {
            surface,
            device,
            queue,
            config,
        })
    }

    /// วาดหนึ่งเฟรมแล้ว **present จริง** — จุดที่คุยกับ DWM
    fn draw(&mut self) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        // ★ รูปเดียวกับ `device.rs` — surface คืน enum ไม่ใช่ `Result`
        let frame = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        gpu.queue.submit(Some(encoder.finish()));
        frame.present();
        self.frames += 1;
    }
}

impl ApplicationHandler for Bare {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(TITLE)
            .with_inner_size(winit::dpi::LogicalSize::new(480.0, 320.0));
        match event_loop.create_window(attrs) {
            Ok(window) => {
                let window = Arc::new(window);
                self.gpu = Self::start_gpu(&window);
                if self.gpu.is_none() {
                    eprintln!("no GPU — the experiment would measure nothing");
                    event_loop.exit();
                }
                self.window = Some(window);
            }
            Err(err) => {
                eprintln!("cannot create the window: {err}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(&gpu.device, &gpu.config);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // ★ สภาพเดียวกับ RefX: หลับสนิทจนกว่าจะมี event (I-1)
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

fn main() -> Result<(), winit::error::EventLoopError> {
    println!("bare wgpu starting");
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = Bare::default();
    event_loop.run_app(&mut app)?;
    println!("bare wgpu closed after {} frames", app.frames);
    Ok(())
}
