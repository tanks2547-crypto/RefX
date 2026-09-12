//! ตัวห่อ `winit::ApplicationHandler` — ที่เดียวในโปรเจกต์ที่รู้จัก winit event loop
//!
//! หน้าที่หลักไม่ใช่แค่ "เปิดหน้าต่าง" แต่คือ **บังคับ I-1 ด้วยโครงสร้าง**:
//!
//!   * ตั้ง `ControlFlow::Wait` ให้ที่เดียว ชั้นบนเปลี่ยนไม่ได้
//!   * `RedrawRequested` ถูกแยกออกจากสายของ input ตั้งแต่ต้นทาง
//!     ชั้นบนจึงไม่มีทาง "เผลอ" เอาผลของการวาดไปขอวาดซ้ำจนเป็นลูป
//!   * ทุกคำขอ redraw ต้องผ่าน [`RedrawTracker`] จึงตอบได้เสมอว่าเฟรมเกิดจากอะไร
//!
//! spec: docs/04-rendering.md §1, ARCHITECTURE.md §2

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::redraw::{RedrawReason, RedrawTracker};

/// เปิดหน้าต่าง/รัน event loop ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum WindowError {
    /// สร้าง event loop ไม่ได้
    #[error("cannot start the window event loop: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),

    /// สร้างหน้าต่างไม่ได้
    #[error("cannot create the window: {0}")]
    Create(#[from] winit::error::OsError),
}

/// เหตุการณ์ที่เธรดอื่นส่งมาปลุก event loop
///
/// ★ นี่คือเงื่อนไขข้อ 2 ของ docs/04 §1 — worker ที่ decode เสร็จต้อง **ปลุก**
/// event loop ที่กำลังหลับอยู่ ไม่งั้นภาพจะไม่ขึ้นจนกว่าผู้ใช้จะขยับเมาส์
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeEvent {
    /// worker ถอดรหัสภาพเสร็จแล้ว มีของใหม่ให้วาด
    TextureReady,
}

/// ตัวปลุก event loop จากเธรดอื่น — clone แล้วส่งไปให้ worker ได้
///
/// `Clone` + `Send` เพราะต้องแจกให้ decode worker หลายตัวถือคนละใบ
#[derive(Debug, Clone)]
pub struct Waker {
    proxy: winit::event_loop::EventLoopProxy<WakeEvent>,
}

impl Waker {
    /// ปลุก event loop ให้วาดเฟรมใหม่
    ///
    /// เรียกจากเธรดไหนก็ได้ ปลอดภัยแม้ event loop ปิดไปแล้ว (จะเงียบ ๆ ไม่ทำอะไร)
    pub fn wake(&self) {
        // ผิดพลาดได้กรณีเดียวคือ event loop ปิดไปแล้ว ซึ่งไม่ใช่ error
        let _ = self.proxy.send_event(WakeEvent::TextureReady);
    }
}

/// ค่าตั้งต้นของหน้าต่าง
#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// ข้อความบนแถบชื่อหน้าต่าง
    pub title: String,
    /// ความกว้างเริ่มต้น (logical pixel)
    pub width: f64,
    /// ความสูงเริ่มต้น (logical pixel)
    pub height: f64,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "RefX".to_owned(),
            width: 1280.0,
            height: 800.0,
        }
    }
}

/// สิ่งที่ชั้นบน (`refx-ui`) ต้อง implement เพื่อรับ event จากหน้าต่าง
///
/// สังเกตว่า **ไม่มีเมธอดสำหรับ `RedrawRequested`ในกลุ่ม input** — การวาดมาทาง
/// [`AppDelegate::redraw`] ทางเดียว นี่คือสิ่งที่กันลูป redraw ไม่รู้จบตั้งแต่ระดับ type
pub trait AppDelegate {
    /// error ที่ delegate คืนได้ตอนเตรียมตัว
    type Error: std::error::Error + Send + Sync + 'static;

    /// หน้าต่างพร้อมแล้ว — สร้าง GPU device/surface ที่นี่
    ///
    /// คืน `Err` เพื่อยกเลิกการเปิดโปรแกรมอย่างเรียบร้อย (ไม่ panic)
    fn window_ready(&mut self, window: Arc<Window>) -> Result<(), Self::Error>;

    /// รับตัวปลุก event loop — เรียกครั้งเดียว **ก่อน** loop เริ่มทำงาน
    ///
    /// delegate ต้องส่งต่อให้ worker เพื่อให้ปลุกได้เมื่อ decode เสร็จ
    fn set_waker(&mut self, _waker: Waker) {}

    /// มีของใหม่จากเธรดอื่น — คืน `true` ถ้าต้องวาดใหม่
    fn on_wake_event(&mut self, _event: WakeEvent) -> bool {
        true
    }

    /// วาดหนึ่งเฟรม — เรียกจาก `RedrawRequested` เท่านั้น
    ///
    /// คืน `Some(reason)` **ก็ต่อเมื่อ** จำเป็นต้องวาดอีกเฟรมจริง ๆ
    /// (egui มี animation ค้าง หรือเพิ่งกู้ surface) คืน `None` = หลับ
    ///
    /// ★ การคืนค่าแทนที่จะให้ delegate เรียก `request_redraw()` เองมีเหตุผล:
    /// ทำให้ทุกคำขอผ่าน [`RedrawTracker`] เสมอ ตรวจสอบ I-1 ได้จากที่เดียว
    #[must_use]
    fn redraw(&mut self) -> Option<RedrawReason>;

    /// ★★★ ช่วงเงียบเปลี่ยนไป — **ตัวจับ I-1 ที่รั่วเป็นครั้งคราว**
    ///
    /// เรียกทุกครั้งที่มีคนขอ redraw · ค่าที่ได้คือ [`RedrawTracker::quiet`]
    /// หลังบันทึกคำขอนั้นแล้ว (ดู [`crate::redraw::QuietStreak`])
    ///
    /// ★ **ห้าม implement ให้มันขอ redraw** ไม่ว่าทางตรงหรือทางอ้อม — ตัวจับที่
    /// ทำให้เกิดเฟรมเพิ่ม คือตัวจับที่ทำให้สิ่งที่มันวัดผิดไปเอง · ค่าเริ่มต้น
    /// ไม่ทำอะไรเลย ชั้นบนที่อยากแสดงมันบนแถบสถานะค่อย override
    fn on_quiet_streak(&mut self, _quiet: crate::redraw::QuietStreak) {}

    /// event จากผู้ใช้ (ไม่มี `RedrawRequested` ปนมาแน่นอน)
    ///
    /// คืน `true` ถ้าต้องวาดใหม่เพราะ event นี้
    fn on_input(&mut self, event: &WindowEvent) -> bool;

    /// ขนาดหน้าต่างเปลี่ยน (physical pixel)
    fn on_resize(&mut self, width: u32, height: u32);

    /// ผู้ใช้กดปิดหน้าต่าง — คืน `true` เพื่อปิดจริง
    ///
    /// ค่าเริ่มต้นคือปิดเลย P4 จะ override เพื่อถามเรื่องงานที่ยังไม่เซฟ
    fn on_close_requested(&mut self) -> bool {
        true
    }

    /// ★ delegate ขอปิดโปรแกรม **ด้วยตัวเอง** — `true` = ออกจาก event loop
    ///
    /// ต่างจาก [`AppDelegate::on_close_requested`] ตรงที่อันนั้นตอบ *คำถาม*
    /// ของผู้ใช้ ณ ตอนนั้น ส่วนอันนี้คือการ **ขอปิดทีหลัง** ซึ่งจำเป็นเมื่อ
    /// การตัดสินใจต้องรออะไรสักอย่างก่อน
    ///
    /// เคสจริงที่ทำให้ต้องมี (P4-2): ผู้ใช้เลือก "บันทึกแล้วปิด" —
    /// การบันทึกเกิดบนเธรดอื่น (I-2) ผลจึงกลับมาหลายเฟรมถัดไป **หลังจาก**
    /// `on_close_requested` ตอบไปแล้วว่า "ยังไม่ปิด" · ถ้าไม่มีทางนี้
    /// ธงที่ตั้งไว้จะไม่มีใครอ่าน แล้วโปรแกรมจะบันทึกสำเร็จแต่ไม่ปิด
    /// (เจอจริงตอนทดสอบบนของจริง — ไม่มีเทสต์ไหนจับได้เพราะมันคือ
    /// การประกอบระหว่างชั้น ซึ่ง `docs/08 §3.9` ข้อ 5 บอกว่ามีแต่การรันจริงที่เห็น)
    ///
    /// ★ ถูกถามใน `about_to_wait` ซึ่งเกิดก่อนหลับทุกครั้ง **ไม่ใช่การวนเช็ค**
    /// จึงไม่แตะ I-1
    fn wants_exit(&self) -> bool {
        false
    }

    /// ขอให้ปลุก event loop ตอนเวลาที่กำหนด — `None` = หลับจนกว่าจะมี input
    ///
    /// ใช้กับสิ่งที่ต้องเกิดตามเวลาโดยไม่มี input มาก่อน เช่น
    /// เคอร์เซอร์กะพริบ, animation ที่ egui ขอผ่าน `repaint_after`
    ///
    /// ★ **ไม่ขัด I-1** เพราะ `ControlFlow::WaitUntil` ทำให้เธรดหลับสนิทจนถึงเวลานั้น
    /// ไม่ใช่การวนเช็คเวลา — ต่างจาก `ControlFlow::Poll` โดยสิ้นเชิง
    fn wake_deadline(&self) -> Option<Instant> {
        None
    }

    /// ถึงเวลาที่ขอไว้ใน [`AppDelegate::wake_deadline`] แล้ว
    ///
    /// คืน `Some(reason)` ถ้าต้องวาดเฟรม delegate ต้อง**เคลียร์ deadline ของตัวเอง**
    /// ที่นี่ ไม่งั้นจะถูกปลุกซ้ำไม่รู้จบ
    fn on_wake(&mut self) -> Option<RedrawReason> {
        None
    }
}

/// event ตัวนี้ควรถูกจัดการแบบไหน
///
/// แยกออกมาเป็น **ฟังก์ชันบริสุทธิ์** เพราะ `ActiveEventLoop` สร้างเองไม่ได้นอก winit
/// ทำให้เทสต์ headless ได้ว่าการคัดกรอง `RedrawRequested` ยังถูกต้องอยู่ (I-1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventRoute {
    /// วาดหนึ่งเฟรม — มาจาก `RedrawRequested` เท่านั้น
    Draw,
    /// ผู้ใช้ขอปิดหน้าต่าง
    Close,
    /// ขนาดหน้าต่างเปลี่ยน (physical pixel)
    Resize(u32, u32),
    /// input อื่น ๆ ส่งต่อให้ delegate ตัดสิน
    Input,
}

/// จัดประเภท event ก่อนลงมือทำอะไร
///
/// ★ หัวใจของ I-1 อยู่ที่บรรทัดแรก: `RedrawRequested` ต้องออกทาง [`EventRoute::Draw`]
/// เท่านั้น **ห้าม** ตกไปทาง [`EventRoute::Input`] เด็ดขาด เพราะ `egui-winit`
/// ตอบว่า "ต้องวาดใหม่" ให้ event ตัวนี้ ซึ่งจะกลายเป็นลูปเลี้ยงตัวเองไม่รู้จบ
#[must_use]
pub fn route(event: &WindowEvent) -> EventRoute {
    match event {
        WindowEvent::RedrawRequested => EventRoute::Draw,
        WindowEvent::CloseRequested => EventRoute::Close,
        WindowEvent::Resized(size) => EventRoute::Resize(size.width, size.height),
        _ => EventRoute::Input,
    }
}

/// สิ่งเดียวที่ host ต้องการจากหน้าต่างจริง: ขอให้วาดใหม่
///
/// ★★★ แยกเป็น trait เพราะ `winit::Window` **สร้างนอก event loop ไม่ได้** →
/// ตราบใดที่ host ผูกกับตัวหน้าต่างจริง ทุกกิ่งที่ทำงาน *หลัง* หน้าต่างพร้อม
/// จะไม่มีทางถูกเทสต์เลย · ประตู mutation วัดได้ว่านั่นคือด่านสามตัวใน
/// `ApplicationHandler` ที่รอดจากทุกเทสต์ (`docs/08 §3.9` ข้อ 8 ทางที่ 1)
pub trait Surface {
    /// ขอเฟรมใหม่จากระบบหน้าต่าง
    fn request_redraw(&self);
}

impl Surface for Arc<Window> {
    fn request_redraw(&self) {
        Window::request_redraw(self);
    }
}

/// event ตัวนั้นจบลงยังไง — คืนค่าแทนที่จะสั่ง `event_loop.exit()` ตรงนั้น
/// เพื่อให้การตัดสินทั้งหมดเกิดในที่ที่ไม่ต้องมี `ActiveEventLoop`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterEvent {
    /// หน้าต่างอยู่ต่อ
    Stay,
    /// delegate อนุญาตให้ปิดแล้ว
    Close,
}

/// ตัวเชื่อม winit เข้ากับ [`AppDelegate`]
///
/// `S` มีไว้ให้เทสต์ใส่หน้าต่างปลอมเข้ามาได้ — ของจริงคือ `Arc<Window>` เสมอ
pub struct WindowHost<D: AppDelegate, S: Surface = Arc<Window>> {
    delegate: D,
    config: WindowConfig,
    window: Option<S>,
    tracker: RedrawTracker,
    /// error ที่เกิดตอน `window_ready` — เก็บไว้คืนหลัง event loop จบ
    failure: Option<D::Error>,
}

impl<D: AppDelegate, S: Surface> WindowHost<D, S> {
    /// สร้าง host โดยยังไม่เปิดหน้าต่าง (หน้าต่างเกิดใน `resumed()`)
    pub fn new(delegate: D, config: WindowConfig) -> Self {
        Self {
            delegate,
            config,
            window: None,
            tracker: RedrawTracker::new(),
            failure: None,
        }
    }

    /// ตัวนับ redraw — ใช้ตรวจ I-1
    #[must_use]
    pub fn tracker(&self) -> &RedrawTracker {
        &self.tracker
    }

    /// ขอวาดเฟรมใหม่พร้อมบันทึกเหตุผล
    ///
    /// ทางเดียวใน crate นี้ที่เรียก `request_redraw()` ได้
    fn request_redraw(&mut self, reason: RedrawReason) {
        if let Some(window) = self.window.as_ref() {
            self.tracker.record(reason);
            window.request_redraw();
            // ★ บอกชั้นบนหลังบันทึกแล้ว — ค่าที่ส่งไปคือสภาพหลังคำขอนี้
            //   ★★ ไม่ขอเฟรมเพิ่มเอง: เราอยู่ในเส้นทางที่กำลังขอเฟรมอยู่แล้ว
            self.delegate.on_quiet_streak(self.tracker.quiet());
        }
    }

    /// ยังต้องสร้างหน้าต่างอยู่ไหม
    ///
    /// `resumed()` ถูกเรียกซ้ำได้ (มือถือ / สลับ session) — หน้าต่างต้องเกิด
    /// **ครั้งเดียว** ไม่งั้น `window_ready` จะสร้าง GPU device ใบที่สอง
    /// แล้วใบแรกลอยค้างอยู่โดยไม่มีใครถือ
    ///
    /// ★ แยกออกมาเพราะสลักที่อยู่ใน `resumed()` เทสต์ได้ทางเดียว: ทางที่
    /// "ยังไม่มีหน้าต่าง" เท่านั้น — ทางที่มีแล้วต้องมี `Window` จริงซึ่งสร้าง
    /// นอก event loop ไม่ได้ (`docs/08 §3.9` ข้อ 8)
    fn needs_a_window(&self) -> bool {
        if self.window.is_some() {
            return false;
        }
        true
    }

    /// ★★★ ตัดสินว่า event หนึ่งตัวต้องทำอะไร — **ไม่แตะ `ActiveEventLoop`**
    ///
    /// event มาถึงก่อนหน้าต่างพร้อมได้จริง (winit ส่ง event ของ session เก่ามา
    /// ระหว่างกำลัง resume) · ตอนนั้น delegate ยังไม่มี GPU surface การส่งต่อ
    /// ให้มันคือการวาดลงบนของที่ยังไม่มี
    fn handle_event(&mut self, event: &WindowEvent) -> AfterEvent {
        if self.window.is_none() {
            return AfterEvent::Stay;
        }

        // ★ I-1: การคัดกรองอยู่ใน route() ซึ่งเป็นฟังก์ชันบริสุทธิ์ที่มีเทสต์คุม
        //   RedrawRequested คือ "ผลของการขอวาด" ไม่ใช่ input จึงห้ามตกไปทาง on_input
        //   — เจอของจริงตอน spike: 3068 เฟรม/20 วินาที ทั้งที่ไม่แตะอะไรเลย
        match route(event) {
            EventRoute::Draw => {
                // วาดแล้วถามว่า "ต้องวาดอีกไหม" — ถ้าไม่ ก็หลับยาว
                if let Some(reason) = self.delegate.redraw() {
                    self.request_redraw(reason);
                }
            }
            EventRoute::Close => {
                if self.delegate.on_close_requested() {
                    return AfterEvent::Close;
                }
            }
            EventRoute::Resize(width, height) => {
                self.delegate.on_resize(width, height);
                // resize คือ input ตามข้อ 1 ของ docs/04 §1
                self.request_redraw(RedrawReason::UserInput);
            }
            EventRoute::Input => {
                if self.delegate.on_input(event) {
                    self.request_redraw(RedrawReason::UserInput);
                }
            }
        }
        AfterEvent::Stay
    }

    /// เธรดอื่นปลุกมา (decode เสร็จ) — เงื่อนไขข้อ 2 ของ docs/04 §1
    ///
    /// การมาถึงของ event นี้เองคือสิ่งที่ทำให้ event loop ตื่นจาก `ControlFlow::Wait`
    /// **ไม่ขัด I-1** เพราะเกิดเฉพาะตอนมีของใหม่จริง ๆ ไม่ใช่ปลุกเป็นระยะ
    fn handle_wake(&mut self, event: WakeEvent) {
        if self.window.is_none() {
            return;
        }
        if self.delegate.on_wake_event(event) {
            self.request_redraw(RedrawReason::TextureReady);
        }
    }
}

impl<D: AppDelegate> WindowHost<D, Arc<Window>> {
    /// สร้างหน้าต่างจริงแล้วส่งให้ delegate เตรียม GPU
    ///
    /// ★ ตั้ง `self.window` **หลัง** `window_ready` สำเร็จเท่านั้น — ถ้าล้มแล้ว
    /// resume รอบหน้ายังลองใหม่ได้ และ event ที่มาระหว่างนั้นยังถูกทิ้งอยู่
    fn open_window(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.width,
                self.config.height,
            ));

        let window = match event_loop.create_window(attrs) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                tracing::error!(%err, "cannot create the window");
                event_loop.exit();
                return;
            }
        };

        if let Err(err) = self.delegate.window_ready(Arc::clone(&window)) {
            tracing::error!(%err, "graphics setup failed");
            self.failure = Some(err);
            event_loop.exit();
            return;
        }

        self.window = Some(window);
    }
}

impl<D: AppDelegate> ApplicationHandler<WakeEvent> for WindowHost<D, Arc<Window>> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // ★ ทุกการตัดสินอยู่ใน needs_a_window()/handle_*() ซึ่งเทสต์ยิงได้ทั้งสองทาง
        //   เมธอดของ `ApplicationHandler` เหลือไว้แค่ส่งต่อ เพราะ `ActiveEventLoop`
        //   สร้างเองไม่ได้ อะไรที่อยู่ในนี้จึงไม่มีทางถูกเทสต์ (`docs/08 §3.9` ข้อ 8)
        if self.needs_a_window() {
            self.open_window(event_loop);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.handle_event(&event) == AfterEvent::Close {
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: WakeEvent) {
        self.handle_wake(event);
    }

    /// จุดเดียวที่ตัดสินว่า event loop จะ "หลับยาว" หรือ "หลับจนถึงเวลาหนึ่ง"
    ///
    /// ★ I-1: มีแค่สองทางเลือกคือ [`ControlFlow::Wait`] กับ [`ControlFlow::WaitUntil`]
    /// **ไม่มี `Poll` เด็ดขาด** ทั้งสองแบบเธรดหลับจริง ไม่กิน CPU
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // ★ delegate ขอปิดเอง (เช่น "บันทึกแล้วปิด" ที่เพิ่งบันทึกเสร็จ)
        if self.delegate.wants_exit() {
            event_loop.exit();
            return;
        }
        let Some(deadline) = self.delegate.wake_deadline() else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };

        if Instant::now() < deadline {
            // ยังไม่ถึงเวลา — หลับต่อจนถึงตอนนั้นพอดี
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            return;
        }

        // ถึงเวลาแล้ว — delegate ต้องเคลียร์ deadline ของตัวเองใน on_wake()
        if let Some(reason) = self.delegate.on_wake() {
            self.request_redraw(reason);
        }
        // ตั้ง Wait ไว้ก่อน ถ้า delegate ยังมี deadline ใหม่ รอบหน้าจะตั้ง WaitUntil เอง
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

/// รัน event loop ล้มเหลว — แยกความผิดของหน้าต่างออกจากความผิดของ delegate
#[derive(Debug, thiserror::Error)]
pub enum RunError<E: std::error::Error + Send + Sync + 'static> {
    /// ระบบหน้าต่างเอง
    #[error(transparent)]
    Window(#[from] WindowError),

    /// delegate เตรียมตัวไม่สำเร็จ (เช่น สร้าง GPU device ไม่ได้)
    #[error(transparent)]
    Delegate(E),
}

/// เปิดหน้าต่างแล้วรัน event loop จนผู้ใช้ปิดโปรแกรม
///
/// ตั้ง `ControlFlow::Wait` ให้เอง — **ที่เดียวในโปรเจกต์ที่ตั้งค่านี้** (I-1)
pub fn run<D: AppDelegate>(
    mut delegate: D,
    config: WindowConfig,
) -> Result<(), RunError<D::Error>> {
    let event_loop = EventLoop::<WakeEvent>::with_user_event()
        .build()
        .map_err(WindowError::from)?;
    // ★ Wait เท่านั้น ห้าม Poll เด็ดขาด — ดู CLAUDE.md
    event_loop.set_control_flow(ControlFlow::Wait);

    // ส่งตัวปลุกให้ delegate ก่อนเข้าลูป — worker ต้องมีตั้งแต่ก่อนเริ่มรับงาน
    delegate.set_waker(Waker {
        proxy: event_loop.create_proxy(),
    });

    let mut host = WindowHost::new(delegate, config);
    event_loop.run_app(&mut host).map_err(WindowError::from)?;

    // ★★ ช่วงเงียบที่ยาวที่สุดต้องอยู่ใน log เสมอ — อาการที่เกิดแล้วหายเอง
    //    จะไม่มีใครเห็นถ้ารายงานแต่ค่าปัจจุบัน (`docs/08 §3.9` ข้อ 11)
    tracing::info!(
        redraws = %host.tracker().summary(),
        worst_quiet = %host.tracker().worst_quiet().summary(),
        "shutting down"
    );

    // error ตอน window_ready ต้องไม่เงียบหาย — ผู้ใช้ต้องได้เห็นสาเหตุ
    match host.failure {
        Some(err) => Err(RunError::Delegate(err)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::time::{Duration, Instant};

    use winit::dpi::PhysicalSize;

    use super::*;

    /// delegate จำลองที่ "ไม่มีอะไรต้องวาดต่อ" — เลียนแบบ egui ตอน idle
    /// (ของจริง egui คืน `repaint_delay = Duration::MAX`)
    #[derive(Default)]
    struct IdleDelegate {
        draws: u64,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("unreachable in tests")]
    struct NeverError;

    impl AppDelegate for IdleDelegate {
        type Error = NeverError;

        fn window_ready(&mut self, _window: Arc<Window>) -> Result<(), Self::Error> {
            Ok(())
        }

        fn redraw(&mut self) -> Option<RedrawReason> {
            self.draws += 1;
            None // idle = ไม่ขอเฟรมต่อ
        }

        fn on_input(&mut self, _event: &WindowEvent) -> bool {
            false
        }

        fn on_resize(&mut self, _width: u32, _height: u32) {}
    }

    /// ★ เทสต์หลักของ P0-4 (ROADMAP)
    ///
    /// จำลอง event loop แบบ headless — ไม่มีหน้าต่าง ไม่มี GPU ไม่ป้อน input เลย
    /// ตลอด 5 วินาที ตัวนับ redraw ต้องเป็น 0 พอดี
    ///
    /// ของจริง `ControlFlow::Wait` ทำให้เธรดหลับสนิทตรงนี้ ไม่มี event เข้ามา
    /// จึงไม่มีอะไรไปเรียก `request_redraw()` ได้เลย
    #[test]
    fn idle_produces_no_redraw() {
        let mut tracker = RedrawTracker::new();
        let mut delegate = IdleDelegate::default();

        let start = Instant::now();
        let idle_for = Duration::from_secs(5);
        while start.elapsed() < idle_for {
            // ไม่มี event = ไม่มีการเรียก route()/redraw()/request_redraw() ใด ๆ
            std::thread::sleep(Duration::from_millis(100));
        }

        assert_eq!(
            tracker.total(),
            0,
            "idle 5 วินาทีแล้วยังมีคำขอวาด {} ครั้ง ({}) — ละเมิด I-1",
            tracker.total(),
            tracker.summary()
        );
        assert_eq!(delegate.draws, 0, "idle แล้วไม่ควรมีการวาดเฟรมเลย");

        // กันเทสต์กลายเป็นของว่างเปล่า: ตัวนับต้องนับได้จริงถ้ามีคนขอ
        tracker.record(RedrawReason::UserInput);
        assert_eq!(tracker.total(), 1, "ตัวนับต้องทำงานจริง ไม่ใช่ค้างที่ 0");
        let _ = &mut delegate;
    }

    /// delegate ที่ขอให้ปลุกตามเวลา — จำลอง egui animation / ตัวทดสอบ device lost
    #[derive(Default)]
    struct TimedDelegate {
        deadline: Option<Instant>,
        wakes: u32,
    }

    impl AppDelegate for TimedDelegate {
        type Error = NeverError;

        fn window_ready(&mut self, _window: Arc<Window>) -> Result<(), Self::Error> {
            Ok(())
        }
        fn redraw(&mut self) -> Option<RedrawReason> {
            None
        }
        fn on_input(&mut self, _event: &WindowEvent) -> bool {
            false
        }
        fn on_resize(&mut self, _width: u32, _height: u32) {}

        fn wake_deadline(&self) -> Option<Instant> {
            self.deadline
        }

        fn on_wake(&mut self) -> Option<RedrawReason> {
            self.wakes += 1;
            self.deadline = None; // ต้องเคลียร์ ไม่งั้นถูกปลุกซ้ำไม่รู้จบ
            Some(RedrawReason::Animation)
        }
    }

    /// ตื่นตามเวลาแล้วต้อง **เคลียร์ deadline** ไม่งั้นกลายเป็นลูปปลุกไม่รู้จบ
    /// ซึ่งจะกิน CPU เท่ากับ `ControlFlow::Poll` ทั้งที่เขียนว่า `WaitUntil`
    #[test]
    fn timed_wake_fires_once_then_sleeps() {
        let mut tracker = RedrawTracker::new();
        let mut delegate = TimedDelegate {
            deadline: Some(Instant::now() - Duration::from_millis(1)), // ถึงเวลาแล้ว
            wakes: 0,
        };

        // จำลอง about_to_wait หลาย ๆ รอบ
        for _ in 0..100 {
            let Some(deadline) = delegate.wake_deadline() else {
                continue; // ไม่มี deadline = หลับยาว (ControlFlow::Wait)
            };
            if Instant::now() >= deadline
                && let Some(reason) = delegate.on_wake()
            {
                tracker.record(reason);
            }
        }

        assert_eq!(delegate.wakes, 1, "ต้องตื่นครั้งเดียว ไม่ใช่ทุกรอบ");
        assert_eq!(tracker.total(), 1);
        assert_eq!(delegate.wake_deadline(), None, "ต้องเคลียร์ deadline หลังตื่น");
    }

    /// ยังไม่ถึงเวลา = ห้ามตื่น ห้ามขอวาด
    #[test]
    fn future_deadline_does_not_wake() {
        let mut delegate = TimedDelegate {
            deadline: Some(Instant::now() + Duration::from_secs(3600)),
            wakes: 0,
        };
        for _ in 0..100 {
            if let Some(deadline) = delegate.wake_deadline()
                && Instant::now() >= deadline
            {
                delegate.on_wake();
            }
        }
        assert_eq!(delegate.wakes, 0, "ยังไม่ถึงเวลาต้องไม่ตื่นเลย");
    }

    /// ★ กันการถอยหลังของบั๊กที่ spike เจอ
    ///
    /// ถ้าวันไหนมีคนแก้ให้ `RedrawRequested` ตกไปทาง `Input`
    /// egui-winit จะตอบว่า repaint=true แล้วเกิดลูป 160 fps ทันที เทสต์นี้จับได้ก่อน
    #[test]
    fn redraw_requested_is_never_input() {
        assert_eq!(route(&WindowEvent::RedrawRequested), EventRoute::Draw);
        assert_ne!(route(&WindowEvent::RedrawRequested), EventRoute::Input);
    }

    #[test]
    fn other_events_route_correctly() {
        assert_eq!(route(&WindowEvent::CloseRequested), EventRoute::Close);
        assert_eq!(
            route(&WindowEvent::Resized(PhysicalSize::new(800, 600))),
            EventRoute::Resize(800, 600)
        );
        assert_eq!(route(&WindowEvent::Focused(true)), EventRoute::Input);
        assert_eq!(route(&WindowEvent::Occluded(false)), EventRoute::Input);
    }

    /// หน้าต่างปลอม — สิ่งเดียวที่ host ขอจากหน้าต่างจริงคือ "ขอเฟรมใหม่"
    ///
    /// ★ มีไว้เพื่อให้เทสต์เข้าถึงกิ่ง **หลัง** หน้าต่างพร้อมได้ · `winit::Window`
    /// สร้างนอก event loop ไม่ได้ กิ่งเหล่านั้นจึงไม่เคยถูกเทสต์มาก่อน
    #[derive(Clone, Default)]
    struct FakeSurface(std::rc::Rc<std::cell::Cell<u32>>);

    impl Surface for FakeSurface {
        fn request_redraw(&self) {
            self.0.set(self.0.get() + 1);
        }
    }

    /// delegate ที่จดว่าถูกเรียกอะไรบ้าง — เทสต์ถามมันว่า event ไปถึงจริงไหม
    #[derive(Default)]
    struct SpyDelegate {
        draws: u32,
        inputs: u32,
        wakes: u32,
        resizes: Vec<(u32, u32)>,
        close_asks: u32,
        /// ตอบว่าอะไรเมื่อถูกถามว่าปิดได้ไหม
        may_close: bool,
        /// `on_input` / `on_wake_event` ตอบว่าต้องวาดใหม่ไหม
        wants_frame: bool,
    }

    impl AppDelegate for SpyDelegate {
        type Error = NeverError;

        fn window_ready(&mut self, _window: Arc<Window>) -> Result<(), Self::Error> {
            Ok(())
        }

        fn redraw(&mut self) -> Option<RedrawReason> {
            self.draws += 1;
            None
        }

        fn on_input(&mut self, _event: &WindowEvent) -> bool {
            self.inputs += 1;
            self.wants_frame
        }

        fn on_resize(&mut self, width: u32, height: u32) {
            self.resizes.push((width, height));
        }

        fn on_close_requested(&mut self) -> bool {
            self.close_asks += 1;
            self.may_close
        }

        fn on_wake_event(&mut self, _event: WakeEvent) -> bool {
            self.wakes += 1;
            self.wants_frame
        }
    }

    /// host ที่ "หน้าต่างพร้อมแล้ว" โดยไม่ต้องมีระบบหน้าต่างจริง
    fn host_with_window(delegate: SpyDelegate) -> WindowHost<SpyDelegate, FakeSurface> {
        let mut host = WindowHost::new(delegate, WindowConfig::default());
        host.window = Some(FakeSurface::default());
        host
    }

    /// ★★★ `resumed()` มาซ้ำได้ — หน้าต่างต้องเกิดครั้งเดียว
    ///
    /// ถ้าสลักนี้ค้างที่ "ไม่ต้องสร้าง" โปรแกรมจะเปิดมาเป็นจอว่างตลอดไป
    /// ถ้าค้างที่ "สร้าง" จะได้ GPU device ใบที่สองทับใบแรกตอนสลับ session
    /// — ทั้งสองทางไม่มีเทสต์ตัวไหนเห็นเลยจนกระทั่ง 12 ก.ย. 2026
    #[test]
    fn the_window_is_created_once_no_matter_how_often_resume_comes_back() {
        let fresh: WindowHost<SpyDelegate, FakeSurface> =
            WindowHost::new(SpyDelegate::default(), WindowConfig::default());
        assert!(
            fresh.needs_a_window(),
            "เปิดโปรแกรมมาแล้วไม่ยอมสร้างหน้าต่าง = จอว่าง"
        );

        let already = host_with_window(SpyDelegate::default());
        assert!(
            !already.needs_a_window(),
            "resume รอบสองสร้างหน้าต่างใบใหม่ทับใบเดิม — GPU device ใบแรกจะลอยค้าง"
        );
    }

    /// ★★★ event ที่มาก่อนหน้าต่างพร้อมต้องถูกทิ้ง — **และตัวที่มาทีหลังต้องไม่ถูกทิ้ง**
    ///
    /// ครึ่งหลังคือครึ่งที่หายไป: ด่าน `self.window.is_none()` ถ้าทำงานทุกครั้ง
    /// = โปรแกรมที่ไม่ตอบสนองอะไรเลยทั้งโปรแกรม และไม่มีเทสต์ไหนแดง
    #[test]
    fn events_before_the_window_are_dropped_and_the_rest_get_through() {
        // ---- ก่อนหน้าต่างพร้อม: delegate ต้องไม่ถูกแตะเลย ----
        let mut early: WindowHost<SpyDelegate, FakeSurface> =
            WindowHost::new(SpyDelegate::default(), WindowConfig::default());
        for event in [
            WindowEvent::RedrawRequested,
            WindowEvent::CloseRequested,
            WindowEvent::Focused(true),
            WindowEvent::Resized(PhysicalSize::new(800, 600)),
        ] {
            assert_eq!(early.handle_event(&event), AfterEvent::Stay);
        }
        assert_eq!(early.delegate.draws, 0, "วาดทั้งที่ยังไม่มี surface");
        assert_eq!(early.delegate.inputs, 0);
        assert_eq!(early.delegate.close_asks, 0, "ถามเรื่องปิดก่อนหน้าต่างจะมีอยู่");
        assert!(early.delegate.resizes.is_empty());

        // ---- หลังหน้าต่างพร้อม: ต้องไปถึง delegate ครบทุกทาง ----
        let mut host = host_with_window(SpyDelegate {
            wants_frame: true,
            ..SpyDelegate::default()
        });
        assert_eq!(
            host.handle_event(&WindowEvent::RedrawRequested),
            AfterEvent::Stay
        );
        assert_eq!(host.delegate.draws, 1, "RedrawRequested ไปไม่ถึง redraw()");

        assert_eq!(
            host.handle_event(&WindowEvent::Focused(true)),
            AfterEvent::Stay
        );
        assert_eq!(host.delegate.inputs, 1, "input ไปไม่ถึง delegate");

        assert_eq!(
            host.handle_event(&WindowEvent::Resized(PhysicalSize::new(640, 480))),
            AfterEvent::Stay
        );
        assert_eq!(host.delegate.resizes, vec![(640, 480)]);

        // input ที่ตอบว่า "ต้องวาดใหม่" กับ resize ต้องขอเฟรมจริง ๆ ผ่านหน้าต่าง
        assert_eq!(
            host.tracker().total(),
            2,
            "คำขอวาดไม่ได้ถูกบันทึก: {}",
            host.tracker().summary()
        );
        assert_eq!(
            host.window.as_ref().map(|w| w.0.get()),
            Some(2),
            "บันทึกว่าขอเฟรมแล้วแต่ไม่ได้บอกระบบหน้าต่าง — ภาพจะไม่ขึ้นจนกว่าจะขยับเมาส์"
        );
    }

    /// ปุ่มปิดต้องปิดได้จริง และ **ต้องไม่ปิดเมื่อ delegate ยังไม่ยอม** (งานที่ยังไม่เซฟ — I-3)
    #[test]
    fn closing_waits_for_the_delegate_to_say_yes() {
        let mut refuses = host_with_window(SpyDelegate::default()); // may_close = false
        assert_eq!(
            refuses.handle_event(&WindowEvent::CloseRequested),
            AfterEvent::Stay,
            "ปิดทั้งที่ delegate ยังไม่ตอบ — งานที่ยังไม่เซฟหายทันที (I-3)"
        );
        assert_eq!(refuses.delegate.close_asks, 1, "ไม่ได้ถาม delegate เลยด้วยซ้ำ");

        let mut agrees = host_with_window(SpyDelegate {
            may_close: true,
            ..SpyDelegate::default()
        });
        assert_eq!(
            agrees.handle_event(&WindowEvent::CloseRequested),
            AfterEvent::Close,
            "delegate ยอมปิดแล้วแต่หน้าต่างไม่ปิด — กดกากบาทแล้วโปรแกรมไม่ตอบ"
        );
    }

    /// ★★ ผลจากเธรดอื่น (decode เสร็จ) ต้องกลายเป็นเฟรม — ข้อ 2 ของ docs/04 §1
    #[test]
    fn a_wake_from_another_thread_turns_into_a_frame_once_the_window_exists() {
        let mut early: WindowHost<SpyDelegate, FakeSurface> = WindowHost::new(
            SpyDelegate {
                wants_frame: true,
                ..SpyDelegate::default()
            },
            WindowConfig::default(),
        );
        early.handle_wake(WakeEvent::TextureReady);
        assert_eq!(
            early.delegate.wakes, 0,
            "ปลุกก่อนหน้าต่างพร้อม = วาดลงบนของที่ยังไม่มี"
        );
        assert_eq!(early.tracker().total(), 0);

        let mut host = host_with_window(SpyDelegate {
            wants_frame: true,
            ..SpyDelegate::default()
        });
        host.handle_wake(WakeEvent::TextureReady);
        assert_eq!(host.delegate.wakes, 1, "ของใหม่จากเธรดอื่นไปไม่ถึง delegate");
        assert_eq!(
            host.tracker().total(),
            1,
            "decode เสร็จแล้วไม่มีใครขอเฟรม — ภาพจะไม่ขึ้นจนกว่าผู้ใช้จะขยับเมาส์"
        );

        // delegate บอกว่า "ไม่ต้องวาด" → ต้องไม่มีเฟรมเพิ่ม (I-1)
        let mut quiet = host_with_window(SpyDelegate::default()); // wants_frame = false
        quiet.handle_wake(WakeEvent::TextureReady);
        assert_eq!(quiet.delegate.wakes, 1);
        assert_eq!(
            quiet.tracker().total(),
            0,
            "ปลุกแล้ววาดทั้งที่ไม่มีอะไรเปลี่ยน — ละเมิด I-1"
        );
    }

    /// วาดเฟรมแล้ว delegate บอกว่าไม่ต้องวาดต่อ → ต้องไม่มีคำขอใหม่แม้แต่ครั้งเดียว
    ///
    /// นี่คือรูปแบบเดียวกับลูปที่ spike เจอ แค่ตัดหน้าต่างออก
    #[test]
    fn idle_delegate_never_chains_frames() {
        let mut tracker = RedrawTracker::new();
        let mut delegate = IdleDelegate::default();

        for _ in 0..1000 {
            assert_eq!(route(&WindowEvent::RedrawRequested), EventRoute::Draw);
            if let Some(reason) = delegate.redraw() {
                tracker.record(reason);
            }
        }

        assert_eq!(delegate.draws, 1000);
        assert_eq!(
            tracker.total(),
            0,
            "วาด 1000 เฟรมแล้วต้องไม่เกิดคำขอวาดต่อเลย แต่ได้ {}",
            tracker.summary()
        );
    }
}
