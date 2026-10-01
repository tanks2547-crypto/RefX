//! ★★ เคอร์เซอร์อยู่ตรงไหน **ตอนนี้** — สำหรับจุดปล่อยของการลากไฟล์มาวาง
//!
//! ROADMAP (ตัดสิน 1 ต.ค. 2026): ภาพที่ลากมาวาง **ลงที่จุดที่ปล่อยเมาส์** · แต่
//! winit 0.30 ได้จุดนั้นจาก OS แล้วทิ้งไป — `platform_impl/windows/drop_handler.rs`
//! รับ `_pt: *const POINTL` ใน `IDropTarget::Drop` แล้วส่งต่อแค่ `DroppedFile(path)`
//!
//! ★ ระหว่างลาก OS **ไม่ส่ง mouse-move ให้หน้าต่างเลย** (ลูปลากของ OLE ถือเมาส์ไว้)
//!   ตำแหน่งที่ egui จำไว้จึงเป็นจุดที่เมาส์ออกจากหน้าต่างครั้งล่าสุด ไม่ใช่จุดปล่อย
//!
//! → ถามตำแหน่งเคอร์เซอร์จาก OS **ตอนที่ `DroppedFile` มาถึง** — winit ส่ง event
//!   นั้นจากข้างใน `Drop` แบบ synchronous จังหวะนั้นคือจังหวะที่ผู้ใช้เพิ่งปล่อยปุ่ม
//!
//! OS อื่นตอบ `None` → ผู้เรียกใช้ทางสำรองของตัวเอง (ไม่มีอะไรล้ม)

use winit::window::Window;

/// เคอร์เซอร์ตอนนี้ เทียบกับมุมบนซ้ายของ **พื้นที่ภายใน** หน้าต่าง (physical pixel)
///
/// `None` = OS นี้ถามไม่ได้ · ถามแล้วไม่ตอบ · หรือไม่รู้ตำแหน่งหน้าต่าง
#[must_use]
pub fn cursor_in(window: &Window) -> Option<(f64, f64)> {
    let (x, y) = screen_cursor()?;
    let origin = window.inner_position().ok()?;
    Some((
        f64::from(x) - f64::from(origin.x),
        f64::from(y) - f64::from(origin.y),
    ))
}

/// เคอร์เซอร์ตอนนี้ในพิกัดจอ (physical pixel)
///
/// ★ winit ประกาศ per-monitor DPI awareness ให้โปรเซสแล้ว `GetCursorPos` จึงคืน
///   physical pixel ชุดเดียวกับ `inner_position` — สองค่าลบกันได้ตรง ๆ
#[cfg(target_os = "windows")]
fn screen_cursor() -> Option<(i32, i32)> {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(point: *mut Point) -> i32;
    }

    let mut point = Point { x: 0, y: 0 };
    // SAFETY: `point` อยู่บนสแตกและมีชีวิตตลอดการเรียก · `Point` มีรูปเดียวกับ
    // `POINT` ของ Win32 (`repr(C)` สอง `i32`) · API เขียนแค่สองฟิลด์นั้นและไม่เก็บ
    // ตัวชี้ไว้ใช้ต่อ · คืน 0 = ล้มเหลว (เช่น session ที่ไม่มีเดสก์ท็อป)
    let ok = unsafe { GetCursorPos(&mut point) };
    (ok != 0).then_some((point.x, point.y))
}

#[cfg(not(target_os = "windows"))]
fn screen_cursor() -> Option<(i32, i32)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ถามได้หรือไม่ได้ก็ต้องไม่ล้ม · ถ้าได้ ต้องเป็นตัวเลขที่อยู่บนจอได้จริง
    ///
    /// ★ ไม่ assert ว่า `Some` — runner ของ CI บาง session ไม่มีเดสก์ท็อปให้ถาม
    #[test]
    fn asking_where_the_cursor_is_never_fails_and_answers_something_plausible() {
        if let Some((x, y)) = screen_cursor() {
            assert!(
                (-100_000..100_000).contains(&x) && (-100_000..100_000).contains(&y),
                "ตำแหน่งเคอร์เซอร์ไม่สมเหตุสมผล: {x}, {y}"
            );
        }
    }
}
