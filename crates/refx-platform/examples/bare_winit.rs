//! ★★★ หน้าต่าง `winit` เปล่า ๆ — **ตัวตัดสินว่าบั๊กค้างเป็นของใคร**
//!
//! ## คำถามเดียวที่ตัวอย่างนี้มีไว้ตอบ
//!
//! `PostMessage(WM_INPUTLANGCHANGEREQUEST)` ทำหน้าต่างของ RefX **หยุดตอบทุกครั้ง**
//! (HANDOFF §2.42ก · ยืนยันซ้ำ 13 ก.ย. 2026) · เธรดทั้ง 32 ตัวอยู่ใน `Wait`
//! ไม่มีตัวไหนหมุน และหนึ่งตัวรอการเรียกข้ามโปรเซสอยู่ — แปลว่าไม่ใช่ลูป
//! และไม่ใช่ deadlock บนล็อกของเราเอง
//!
//! เหลือสองความเป็นไปได้ และมันต่างกันสุดขั้ว:
//!
//! | ถ้าหน้าต่างเปล่านี้ | แปลว่า |
//! |---|---|
//! | **ค้างเหมือนกัน** | ไม่ใช่โค้ดของเรา — อยู่ใต้ `winit`/OS ลงไป |
//! | **ไม่ค้าง** | **เป็นของเรา** — ชั้นบน (`egui`/`wgpu`/`refx-ui`) ทำให้เกิด |
//!
//! ## ทำไมต้อง "เปล่า" จริง ๆ
//!
//! ตัวอย่างนี้ **ไม่มี egui ไม่มี wgpu ไม่มี refx-ui** และไม่วาดอะไรเลย ·
//! ทุกอย่างที่ใส่เพิ่มเข้ามาคือสิ่งที่ทำให้คำตอบกำกวม — ถ้ามันค้างเหมือนกัน
//! เราต้องพูดได้ว่า *"ด้วยโค้ดเท่านี้ก็ค้างแล้ว"* โดยไม่มีใครแย้งได้ว่าเป็นเพราะ
//! ชั้นอื่นที่แอบอยู่ในนั้น (`docs/08 §3.9` ข้อ 9: เครื่องมือที่พิสูจน์ว่าไม่ได้โกหก)
//!
//! ★ สิ่งเดียวที่เหมือน RefX โดยตั้งใจคือ **`ControlFlow::Wait`** (I-1) เพราะนั่นคือ
//! สภาพที่แอปอยู่ตอนโดนข้อความนั้นจริง ๆ — หน้าต่างที่วนเช็คตลอดเวลาอาจกลบอาการได้
//!
//! ## ใช้
//!
//! ```text
//! cargo run -p refx-platform --example bare_winit
//! ```
//!
//! แล้วยิงข้อความใส่มันด้วย `scripts/lang-switch-bare.ps1`

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// ชื่อหน้าต่าง — สคริปต์ใช้ชื่อนี้หาตัวมัน
const TITLE: &str = "refx-bare-winit";

#[derive(Default)]
struct Bare {
    window: Option<Arc<Window>>,
    events: u64,
    /// ★★★ เปิด IME ให้หน้าต่าง — **ตัวแปรเดียวที่ต่างจากสภาพเปล่า**
    ///
    /// เราไม่เคยเรียก `set_ime_allowed` เอง แต่ `egui-winit` เรียกให้ทุกเฟรม
    /// (`egui-winit-0.34.3/src/lib.rs:1119`) · หน้าต่างที่เปิด IME จะถูก TSF
    /// (Text Services Framework) เข้ามาเกี่ยวข้อง ซึ่งเป็นการเรียก **ข้ามโปรเซส**
    /// — ตรงกับเธรดที่ค้างอยู่ใน `EventPairLow` พอดี
    ///
    /// ธงนี้จึงเป็นการถามว่า *"แค่เปิด IME อย่างเดียวก็พอทำให้ค้างไหม"*
    ime: bool,
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
                if self.ime {
                    window.set_ime_allowed(true);
                }
                self.window = Some(Arc::new(window));
            }
            Err(err) => {
                eprintln!("cannot create the window: {err}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        self.events += 1;
        if matches!(event, WindowEvent::CloseRequested) {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // ★ สภาพเดียวกับ RefX: หลับสนิทจนกว่าจะมี event (I-1)
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

fn main() -> Result<(), winit::error::EventLoopError> {
    let ime = std::env::args().any(|a| a == "--ime");
    println!("bare winit starting (ime = {ime})");
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = Bare {
        ime,
        ..Bare::default()
    };
    event_loop.run_app(&mut app)?;
    println!("bare winit closed after {} window events", app.events);
    Ok(())
}
