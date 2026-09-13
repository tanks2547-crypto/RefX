//! ★★★ ขั้นที่สามของบันได — **winit + wgpu + egui ล้วน ๆ ไม่มีโค้ดของ RefX เลย**
//!
//! | ขั้น | มีอะไร | ผล |
//! |---|---|---|
//! | 1 | winit เปล่า | รอด 8/8 |
//! | 1ก/1ข | + IME ครั้งเดียว / ทุกรอบ | รอด 8/8 ทั้งคู่ |
//! | 2 | + wgpu surface + present จริง | รอด 8/8 |
//! | 3 | + egui (ไฟล์นี้) | **รอด 8/8** |
//! | 4 | + user event loop + เธรดปลุกทุก 250 ms (`--waker`) | **รอด 8/8** |
//! | 5 | ข้อ 4 แต่ build **release** | **รอด 8/8** |
//!
//! ## ★★★ คำตอบ: ไม่ใช่ขั้นไหนในบันไดนี้เลย — เป็น **backend `gles` ของ wgpu**
//!
//! ทุกขั้นข้างบนรอดหมด เพราะ **ไม่มีขั้นไหนคอมไพล์ `gles` เข้ามา** ·
//! พิสูจน์บนไบนารีของ RefX เองด้วยตัวแปรเดียว:
//!
//! | RefX build | ผล |
//! |---|---|
//! | ไม่มี `gles` (หลัง `d9e1a9b`) | **รอด 8/8** · layout สลับได้ทุกครั้ง |
//! | ใส่ `gles` กลับเข้าไป | **ค้างที่ข้อความที่ 4** · layout ค้างตั้งแต่ข้อความที่ 2 |
//!
//! บน Windows `gles` ลาก WGL/EGL (`glutin_wgl_sys`) เข้ามาด้วย · ประตูที่กัน
//! ไม่ให้มันกลับมาคือ `refx_render::device::tests::the_backend_that_hangs_the_window_is_not_compiled_in`
//!
//! ★ ไฟล์นี้ยังมีค่าอยู่: มันคือขั้นที่ทำให้ **ตัดทุกอย่างข้างบนออกได้**
//! ถ้าวันหน้ามีอาการคล้ายกัน บันไดนี้พร้อมใช้ทันที
//!
//! ★★ ที่ต้องมีให้ครบคือ **`handle_platform_output`** — นั่นคือที่ที่ `egui-winit`
//! เรียก `set_ime_allowed`/`set_ime_cursor_area` ซึ่งเป็นการคุยกับ TSF ข้ามโปรเซส
//! · ตัดมันออกแล้วการทดลองจะตอบคนละคำถามกับที่ตั้งใจถาม
//!
//! ## ใช้
//!
//! ```text
//! cargo run -p refx-ui --example bare_egui
//! scripts/lang-switch-bare.ps1 -ProcessName bare_egui
//! ```

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const TITLE: &str = "refx-bare-egui";

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

struct Egui {
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
}

/// สิ่งที่เธรดอื่นส่งมาปลุก — รูปเดียวกับ `refx_platform::window::WakeEvent`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wake;

#[derive(Default)]
struct Bare {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    egui: Option<Egui>,
    frames: u64,
    wakes: u64,
}

impl Bare {
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
            label: Some("bare-egui"),
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
        println!("gpu ready: {} / {format:?}", adapter.get_info().name);
        Some(Gpu {
            surface,
            device,
            queue,
            config,
        })
    }

    /// ผูก egui เข้ากับหน้าต่างแบบเดียวกับ `refx-ui` (ย่อเฉพาะที่จำเป็น)
    fn start_egui(window: &Arc<Window>, gpu: &Gpu) -> Egui {
        let ctx = egui::Context::default();
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            Some(gpu.device.limits().max_texture_dimension_2d as usize),
        );
        let renderer = egui_wgpu::Renderer::new(
            &gpu.device,
            gpu.config.format,
            egui_wgpu::RendererOptions::default(),
        );
        Egui {
            ctx,
            state,
            renderer,
        }
    }

    fn draw(&mut self) {
        let (Some(window), Some(gpu), Some(egui)) =
            (self.window.as_ref(), self.gpu.as_mut(), self.egui.as_mut())
        else {
            return;
        };

        let raw_input = egui.state.take_egui_input(window);
        // ★ `run_ui` ไม่ใช่ `run` — ตัวหลัง deprecated แล้วใน egui 0.34
        //   และ `refx-ui` ใช้ `run_ui` อยู่ ตัวอย่างต้องเดินทางเดียวกับของจริง
        let full_output = egui.ctx.run_ui(raw_input, |ui| {
            ui.label("bare egui - posting WM_INPUTLANGCHANGEREQUEST at me");
            // ★ ช่องข้อความมีไว้ให้ egui เปิด IME ได้จริงถ้ามันถูก focus
            //   — สภาพที่ `handle_platform_output` เรียก TSF
            ui.text_edit_singleline(&mut String::new());
        });

        // ★★ ตัวที่คุยกับ TSF — ต้องมี ไม่งั้นถามคนละคำถาม
        egui.state
            .handle_platform_output(window, full_output.platform_output);

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

        let pixels_per_point = egui.ctx.pixels_per_point();
        let tris = egui.ctx.tessellate(full_output.shapes, pixels_per_point);
        for (id, delta) in &full_output.textures_delta.set {
            egui.renderer
                .update_texture(&gpu.device, &gpu.queue, *id, delta);
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [gpu.config.width, gpu.config.height],
            pixels_per_point,
        };
        egui.renderer
            .update_buffers(&gpu.device, &gpu.queue, &mut encoder, &tris, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            egui.renderer
                .render(&mut pass.forget_lifetime(), &tris, &screen);
        }
        gpu.queue.submit(Some(encoder.finish()));
        frame.present();
        for id in &full_output.textures_delta.free {
            egui.renderer.free_texture(id);
        }
        self.frames += 1;
    }
}

impl ApplicationHandler<Wake> for Bare {
    /// ★★★ เธรดอื่นปลุกเข้ามา — สิ่งที่ RefX มีแต่สามขั้นก่อนหน้าไม่มี
    ///
    /// บน Windows การส่ง user event ของ winit คือการ **post ข้อความ** เข้าคิว
    /// ของหน้าต่างที่ซ่อนอยู่ · คิวเดียวกับที่ `WM_INPUTLANGCHANGEREQUEST` เข้ามา
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: Wake) {
        self.wakes += 1;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

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
                match Self::start_gpu(&window) {
                    Some(gpu) => {
                        self.egui = Some(Self::start_egui(&window, &gpu));
                        self.gpu = Some(gpu);
                    }
                    None => {
                        eprintln!("no GPU — the experiment would measure nothing");
                        event_loop.exit();
                    }
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
        // ★ egui เห็น event ก่อนเราเสมอ — รูปเดียวกับ `on_input` ของ RefX
        if let (Some(window), Some(egui)) = (self.window.as_ref(), self.egui.as_mut()) {
            let _ = egui.state.on_window_event(window, &event);
        }
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
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

fn main() -> Result<(), winit::error::EventLoopError> {
    // ★ `--waker` = มีเธรดอื่นปลุก event loop เป็นระยะ เหมือน decode worker ของ RefX
    let waker = std::env::args().any(|a| a == "--waker");
    println!("bare egui starting (waker = {waker})");

    let event_loop = EventLoop::<Wake>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    if waker {
        let proxy = event_loop.create_proxy();
        std::thread::Builder::new()
            .name("bare-waker".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    if proxy.send_event(Wake).is_err() {
                        return; // event loop ปิดแล้ว
                    }
                }
            })
            .ok();
    }

    let mut app = Bare::default();
    event_loop.run_app(&mut app)?;
    println!(
        "bare egui closed after {} frames, {} wakes",
        app.frames, app.wakes
    );
    Ok(())
}
