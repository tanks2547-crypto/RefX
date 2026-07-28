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

/// ตัวเชื่อม winit เข้ากับ [`AppDelegate`]
pub struct WindowHost<D: AppDelegate> {
    delegate: D,
    config: WindowConfig,
    window: Option<Arc<Window>>,
    tracker: RedrawTracker,
    /// error ที่เกิดตอน `window_ready` — เก็บไว้คืนหลัง event loop จบ
    failure: Option<D::Error>,
}

impl<D: AppDelegate> WindowHost<D> {
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
        }
    }
}

impl<D: AppDelegate> ApplicationHandler<WakeEvent> for WindowHost<D> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // resumed() ถูกเรียกซ้ำได้ (มือถือ/สลับ session) — สร้างหน้าต่างครั้งเดียวพอ
        if self.window.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.width,
                self.config.height,
            ));

        let window = match event_loop.create_window(attrs) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                tracing::error!(%err, "สร้างหน้าต่างไม่ได้");
                event_loop.exit();
                return;
            }
        };

        if let Err(err) = self.delegate.window_ready(Arc::clone(&window)) {
            tracing::error!(%err, "เตรียมกราฟิกไม่สำเร็จ");
            self.failure = Some(err);
            event_loop.exit();
            return;
        }

        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.is_none() {
            return;
        }

        // ★ I-1: การคัดกรองอยู่ใน route() ซึ่งเป็นฟังก์ชันบริสุทธิ์ที่มีเทสต์คุม
        //   RedrawRequested คือ "ผลของการขอวาด" ไม่ใช่ input จึงห้ามตกไปทาง on_input
        //   — เจอของจริงตอน spike: 3068 เฟรม/20 วินาที ทั้งที่ไม่แตะอะไรเลย
        match route(&event) {
            EventRoute::Draw => {
                // วาดแล้วถามว่า "ต้องวาดอีกไหม" — ถ้าไม่ ก็หลับยาว
                if let Some(reason) = self.delegate.redraw() {
                    self.request_redraw(reason);
                }
            }
            EventRoute::Close => {
                if self.delegate.on_close_requested() {
                    event_loop.exit();
                }
            }
            EventRoute::Resize(width, height) => {
                self.delegate.on_resize(width, height);
                // resize คือ input ตามข้อ 1 ของ docs/04 §1
                self.request_redraw(RedrawReason::UserInput);
            }
            EventRoute::Input => {
                if self.delegate.on_input(&event) {
                    self.request_redraw(RedrawReason::UserInput);
                }
            }
        }
    }

    /// ★ เธรดอื่นปลุกมา (decode เสร็จ) — เงื่อนไขข้อ 2 ของ docs/04 §1
    ///
    /// การมาถึงของ event นี้เองคือสิ่งที่ทำให้ event loop ตื่นจาก `ControlFlow::Wait`
    /// **ไม่ขัด I-1** เพราะเกิดเฉพาะตอนมีของใหม่จริง ๆ ไม่ใช่ปลุกเป็นระยะ
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: WakeEvent) {
        if self.window.is_none() {
            return;
        }
        if self.delegate.on_wake_event(event) {
            self.request_redraw(RedrawReason::TextureReady);
        }
    }

    /// จุดเดียวที่ตัดสินว่า event loop จะ "หลับยาว" หรือ "หลับจนถึงเวลาหนึ่ง"
    ///
    /// ★ I-1: มีแค่สองทางเลือกคือ [`ControlFlow::Wait`] กับ [`ControlFlow::WaitUntil`]
    /// **ไม่มี `Poll` เด็ดขาด** ทั้งสองแบบเธรดหลับจริง ไม่กิน CPU
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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

    tracing::info!(redraws = %host.tracker().summary(), "ปิดโปรแกรม");

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
