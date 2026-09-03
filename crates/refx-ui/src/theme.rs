//! ธีมของแผงรอบ ๆ (P5-3) — spec: `docs/03-modes-and-ui.md §6`
//!
//! ★★★ **ที่นี่คือธีมของ UI ไม่ใช่สีพื้นหลังของ canvas**
//!
//! สองอย่างนี้แยกกันโดยตั้งใจและอยู่คนละชั้น:
//!
//! | อะไร | อยู่ไหน | ขอบเขต |
//! |---|---|---|
//! | สีของแผง/ปุ่ม/ตัวหนังสือ | ที่นี่ · `settings.toml` | ทั้งโปรแกรม |
//! | สีพื้นหลังหลังภาพ | `BoardSettings::background` · `.refx` | **ต่อ board** |
//!
//! `docs/03 §6` ต้องการทั้งคู่ แต่ตัวหลังเป็นเครื่องมือทำงาน (สลับเป็นเทากลาง 50%
//! เพื่อประเมินค่า value ของภาพในบอร์ดใบนั้น) จึงต้องเป็นของ *เอกสาร* ไม่ใช่ของ
//! โปรแกรม — ผู้ใช้ที่ตั้งเทากลางไว้กับงาน line art ใบหนึ่ง ไม่ได้อยากได้มันกับ
//! งานสีน้ำอีกใบ · ตัวนั้นยังไม่มี UI (`HANDOFF §6` — "อ่านแล้ว รอ UI")
//!
//! ★ ธีมมืดเป็นค่าเริ่มต้นตาม `docs/03 §6` และ **พื้นหลังไม่ใช่ดำสนิท** —
//! ดำสนิททำให้ประเมินค่า value ของภาพผิด ซึ่งเป็นงานหลักของกลุ่มผู้ใช้นี้

use refx_io::settings::Theme;

/// พื้นของแผงในธีมมืด — `#2A2A2E` ตรงตาม `docs/03 §6`
///
/// ★ ค่าเดียวกันนี้เป็นค่าปริยายของพื้นหลัง canvas ด้วย (`BoardSettings`) เพื่อให้
/// ขอบระหว่างแผงกับผืนงานไม่กระโดด — แต่มันเป็น *ค่าปริยายที่ตรงกัน* ไม่ใช่
/// ค่าเดียวกัน: ผู้ใช้เปลี่ยนพื้นหลัง canvas ได้โดยแผงไม่ตาม
const DARK_PANEL: egui::Color32 = egui::Color32::from_rgb(0x2A, 0x2A, 0x2E);

/// ตั้งธีมให้ `Context` ที่เพิ่งสร้าง
///
/// ★★ ต้องเรียก **ทุกครั้งที่สร้าง `egui::Context` ใหม่** ซึ่งรวมถึงตอนกู้ device
/// (`HANDOFF §4` ข้อ 8) · ผู้เรียกจริงคือ `app::RefxApp::build_egui` ที่เดียว
/// ซึ่งเป็นจุดเดียวที่สร้าง Context — เหตุผลเดียวกับ `fonts::install`
pub fn install(ctx: &egui::Context, theme: Theme) {
    ctx.set_visuals(visuals(theme));
    // ★★★ ปิดการกะพริบของเคอร์เซอร์ข้อความ — **เรื่องของ I-1 ไม่ใช่เรื่องรสนิยม**
    //
    //   egui ขอวาดใหม่ทุกครั้งที่เคอร์เซอร์กะพริบ วัดจากของจริงได้ **13 เฟรม/วินาที
    //   ตลอดเวลาที่ช่องข้อความมี focus** (ดู `docs/08 §3.9` ข้อ 11)
    //
    //   ★ อยู่ที่นี่เพราะ `set_visuals` ข้างบน **เขียนทับ `style` ทั้งก้อน** —
    //     ถ้าตั้งก่อนหน้า มันจะถูกล้างทิ้งเงียบ ๆ แล้ว I-1 จะพังกลับมาโดยที่
    //     ไม่มีเทสต์ไหนบ่น (เคอร์เซอร์กะพริบไม่ทำให้อะไรล้ม แค่กิน CPU ทั้งวัน)
    ctx.global_style_mut(|style| style.visuals.text_cursor.blink = false);
}

/// สีของธีมหนึ่ง ๆ — ★ **ฟังก์ชันบริสุทธิ์** เทสต์ได้โดยไม่ต้องมีหน้าต่าง
#[must_use]
pub fn visuals(theme: Theme) -> egui::Visuals {
    match theme {
        Theme::Dark => {
            let mut v = egui::Visuals::dark();
            // ค่าปริยายของ egui เข้มกว่าที่ spec ขอ — ดันขึ้นมาที่ #2A2A2E
            v.panel_fill = DARK_PANEL;
            v
        }
        Theme::Light => egui::Visuals::light(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// `docs/03 §6`: ธีมมืดต้องไม่ใช่ **ดำสนิท** — ดำสนิททำให้ประเมิน value ผิด
    ///
    /// ★ assert สิ่งที่ spec สัญญาไว้จริง ๆ ("กลาง ๆ ไม่ใช่ดำ") ไม่ใช่แค่
    /// "เท่ากับค่าคงที่ที่เราเพิ่งพิมพ์" ซึ่งจะผ่านเสมอไม่ว่าค่าจะผิดแค่ไหน
    #[test]
    fn the_dark_theme_is_never_pure_black() {
        let dark = visuals(Theme::Dark).panel_fill;
        assert!(
            dark.r() > 0x10 && dark.g() > 0x10 && dark.b() > 0x10,
            "พื้นหลังเข้มเกินไป: {dark:?}"
        );
        assert!(
            dark.r() < 0x60 && dark.g() < 0x60 && dark.b() < 0x60,
            "พื้นหลังสว่างเกินจะเรียกว่าธีมมืด: {dark:?}"
        );
        assert_eq!(dark, DARK_PANEL, "spec ระบุ #2A2A2E ตรง ๆ");
    }

    /// สองธีมต้องต่างกันจริง — ไม่ใช่ปุ่มที่กดแล้วไม่มีอะไรเกิดขึ้น
    #[test]
    fn the_two_themes_are_actually_different() {
        let dark = visuals(Theme::Dark);
        let light = visuals(Theme::Light);
        assert_ne!(dark.panel_fill, light.panel_fill);
        assert!(dark.dark_mode);
        assert!(!light.dark_mode);
    }
}
