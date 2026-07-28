//! โครง UI กลาง — โค้ดชุดเดียวใช้ทั้งสอง mode
//!
//! วาดกรอบทั้งหมด (tabs / toolbar / library / inspector / status bar)
//! แล้วเจาะช่องกลางให้ mode ปัจจุบันวาดเอง
//!
//! > **หมายเหตุ egui 0.34:** `SidePanel` / `TopBottomPanel` / `CentralPanel::show`
//! > ถูก deprecate หมดแล้ว ตัวอย่างใน docs/03 เขียนด้วย API เก่า
//! > ของจริงต้องใช้ `Panel::top/left/right/bottom` + `show_inside(ui)`
//! > (ถ้าใช้ของเก่า clippy `-D warnings` จะตกทันที)
//!
//! spec: docs/03-modes-and-ui.md §1

use refx_core::view::Mode;

use crate::text::{self, Key, Lang, Template};

/// สถานะที่ shell ต้องอ่าน/เขียน
///
/// P2+ จะขยายเป็น `App` เต็มที่มี board, selection, history
/// ตอนนี้เก็บเฉพาะที่ P0-8 ต้องใช้จริง — ไม่เดาโครงล่วงหน้า
#[derive(Debug)]
pub struct ShellState {
    /// mode ปัจจุบัน
    pub mode: Mode,
    /// ★ ภาษาของ UI — ทุกข้อความที่ผู้ใช้เห็นต้องผ่าน `text::t`/`text::fill` ด้วยค่านี้
    ///
    /// อ่านจาก locale ของ OS ครั้งเดียวตอนเปิดโปรแกรม (docs/03 §0 ข้อ 3)
    pub lang: Lang,
    /// ข้อความสถานะฝั่งซ้ายของ status bar
    pub status: String,
    /// จำนวน item บน board (ตอนนี้คือจำนวนสี่เหลี่ยมทดสอบ)
    pub item_count: usize,
    /// ระดับซูมปัจจุบัน — แสดงบน status bar
    pub zoom: f32,
    /// จำนวนเฟรมที่วาดไปแล้ว — ตัวชี้วัด I-1 ที่เห็นได้ด้วยตา
    ///
    /// ปล่อยโปรแกรมทิ้งไว้แล้วตัวเลขนี้ต้อง **หยุดนิ่ง** ถ้ายังไต่ขึ้นเรื่อย ๆ
    /// แปลว่ามีที่ไหนสักแห่งขอวาดทุกเฟรม
    pub frames_drawn: u64,

    /// ★ I-6: RAM ที่ decode pool ใช้อยู่ / เพดาน (ไบต์)
    ///
    /// spec บังคับให้ตัวเลขนี้ **เห็นได้ด้วยตา** ตลอดเวลา ไม่ใช่ซ่อนใน log
    pub ram_used: usize,
    /// เพดาน RAM รวมทุก worker
    pub ram_limit: usize,
    /// ★ I-6: VRAM ที่ texture ใช้อยู่ / เพดาน (ไบต์)
    pub vram_used: usize,
    /// เพดาน VRAM (คำนวณจากชนิดการ์ดจอ — iGPU ได้น้อยกว่า)
    pub vram_limit: usize,
    /// จำนวน thumbnail ใน cache.sqlite
    pub cache_thumbs: u64,
    /// ขนาด cache.sqlite (ไบต์)
    pub cache_bytes: u64,
    /// งาน decode ที่ยังค้างคิว
    pub decode_queued: usize,
    /// งาน decode ที่ถูกยกเลิกไปแล้ว — หลักฐานว่า cancellation ทำงาน
    pub decode_cancelled: u64,

    /// จำนวน draw call ของภาพในเฟรมล่าสุด
    ///
    /// ชั้น B ทำให้มี draw call ต่อ working texture หนึ่งใบ — ตัวเลขนี้คือสิ่งที่
    /// บอกว่ามันบานออกไปหรือยัง (docs/04 §4 ตั้งเพดานไว้ที่ราว 30)
    pub draw_calls: u32,
    /// VRAM ที่ working texture ใช้ / เพดานของชั้นนั้น (ไบต์)
    pub working_used: usize,
    /// เพดานของ working texture
    pub working_limit: usize,
    /// จำนวน working texture ที่ถูกไล่ออกตาม LRU — หลักฐานว่า LRU ทำงาน
    pub working_evicted: u64,

    /// ★ ความคืบหน้าการโหลด — `None` เมื่อไม่มีงานค้าง
    ///
    /// docs/05 §6: การรอ 80 วินาทีบน cache เย็นยอมรับได้ **ก็ต่อเมื่อ** ผู้ใช้
    /// เห็นว่ามันคืบหน้าอยู่ ไม่ใช่ค้าง — ถ้าไม่มีตัวนี้ เขาจะคิดว่าโปรแกรมแฮงก์
    /// แล้วปิดทิ้งกลางคัน ซึ่งแย่กว่ารอนาน
    pub loading: Option<LoadProgress>,
}

/// ความคืบหน้าของงาน decode งวดปัจจุบัน
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadProgress {
    /// จำนวนที่จบแล้ว (รวมที่ล้มเหลวและถูกยกเลิก — ไม่ค้างคิวแล้วทั้งคู่)
    pub done: u64,
    /// จำนวนทั้งหมดในงวดนี้
    pub total: u64,
}

impl LoadProgress {
    /// ข้อความที่ผู้ใช้เห็น — รูปแบบตาม docs/05 §6
    #[must_use]
    pub fn label(self, lang: Lang) -> String {
        text::fill(
            lang,
            Template::Loading,
            &[
                ("done", &self.done.to_string()),
                ("total", &self.total.to_string()),
            ],
        )
    }

    /// สัดส่วนที่เสร็จแล้ว `0.0..=1.0`
    #[must_use]
    pub fn fraction(self) -> f32 {
        if self.total == 0 {
            return 1.0;
        }
        // ตัดที่ 1.0 เสมอ — ตัวเลขที่เกิน 100% ทำให้ผู้ใช้ไม่เชื่อถือทั้งแถบ
        (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
    }
}

/// แปลงไบต์เป็นข้อความสั้น ๆ ที่คนอ่านรู้เรื่อง
fn human_bytes(bytes: u64) -> String {
    const MB: u64 = 1 << 20;
    const KB: u64 = 1 << 10;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            lang: Lang::default(),
            status: text::t(Lang::default(), Key::Ready).to_owned(),
            item_count: 0,
            zoom: 1.0,
            frames_drawn: 0,
            ram_used: 0,
            ram_limit: 0,
            vram_used: 0,
            vram_limit: 0,
            cache_thumbs: 0,
            cache_bytes: 0,
            decode_queued: 0,
            decode_cancelled: 0,
            draw_calls: 0,
            working_used: 0,
            working_limit: 0,
            working_evicted: 0,
            loading: None,
        }
    }
}

/// วาด shell ทั้งหมด แล้วเรียก `viewport` ให้วาดเนื้อในช่องกลาง
///
/// เรียกจากข้างใน `egui::Context::run_ui` ซึ่งส่ง `&mut Ui` ของ root มาให้
/// (egui 0.34 ไม่มี `Panel::show(ctx)` แล้ว มีแต่ `show_inside(ui)`)
///
/// คืน **rect ของช่องกลาง (หน่วย point)** — ผู้เรียกต้องใช้ค่านี้ตั้ง viewport
/// ของ render pass และเป็นกรอบอ้างอิงของกล้อง ไม่ใช่ขนาดหน้าต่างทั้งบาน
#[must_use = "ต้องเอา rect ไปตั้ง viewport ของ canvas ไม่งั้นภาพจะเยื้อง"]
pub fn draw_in_ui(
    ui: &mut egui::Ui,
    state: &mut ShellState,
    viewport: impl FnOnce(&mut egui::Ui),
) -> egui::Rect {
    // ★ อ่านครั้งเดียวต้นเฟรม — ทุก widget ข้างล่างใช้ค่าเดียวกัน
    let lang = state.lang;

    // ---- แถวบน: board tabs ----
    egui::Panel::top("refx-tabs").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("RefX").strong());
            ui.separator();
            // P4-7: หลาย board พร้อมกัน
            let _ = ui.selectable_label(true, text::t(lang, Key::UntitledBoard));
            if ui
                .button("+")
                .on_hover_text(text::t(lang, Key::NewBoardHint))
                .clicked()
            {
                state.status = text::fill(
                    lang,
                    Template::NotImplemented,
                    &[("what", text::t(lang, Key::NewBoardHint)), ("when", "P4-7")],
                );
            }
        });
    });

    // ---- แถวสอง: mode switch + tools ----
    egui::Panel::top("refx-toolbar").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            // ★ ส่วน shared: ปุ่มสลับ mode เขียนครั้งเดียว
            for mode in [Mode::Canvas, Mode::Arrange] {
                if ui
                    .selectable_label(state.mode == mode, mode.label())
                    .clicked()
                {
                    state.mode = mode;
                    state.status =
                        text::fill(lang, Template::SwitchedMode, &[("mode", mode.label())]);
                }
            }
            ui.separator();

            // ★ ส่วนที่ต่างกันตาม mode — จุดเดียวที่แยกสองทาง
            match state.mode {
                Mode::Canvas => canvas_tools(ui, state),
                Mode::Arrange => arrange_tools(ui, state),
            }
        });
    });

    // ---- ล่างสุด: status bar (ต้องประกาศก่อน panel ซ้าย/ขวาเพื่อให้กินเต็มความกว้าง) ----
    egui::Panel::bottom("refx-status").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            // ★ ความคืบหน้ามาก่อนทุกอย่าง — เป็นสิ่งเดียวที่ผู้ใช้อยากรู้ตอนกำลังโหลด
            //   (เงื่อนไขข้อ 3 ของ docs/05 §6 ที่ทำให้ cache เย็นยอมรับได้)
            if let Some(progress) = state.loading {
                ui.add(
                    egui::ProgressBar::new(progress.fraction())
                        .desired_width(120.0)
                        .text(progress.label(lang)),
                );
            } else {
                ui.label(&state.status);
            }
            ui.separator();
            ui.label(text::fill(
                lang,
                Template::ItemCount,
                &[("n", &state.item_count.to_string())],
            ));
            ui.separator();
            ui.label(text::fill(
                lang,
                Template::Zoom,
                &[("pct", &format!("{:.0}", state.zoom * 100.0))],
            ));
            ui.separator();
            // I-1 ให้เห็นกับตา: ตัวเลขนี้ต้องหยุดนิ่งเมื่อไม่แตะอะไร
            ui.label(text::fill(
                lang,
                Template::FramesDrawn,
                &[("n", &state.frames_drawn.to_string())],
            ));
            ui.separator();

            // ★ I-6 ให้เห็นกับตา: RAM ที่ decode pool ใช้ เทียบกับเพดานรวมทุก worker
            let ram = text::fill(
                lang,
                Template::Ram,
                &[
                    ("used", &human_bytes(state.ram_used as u64)),
                    ("limit", &human_bytes(state.ram_limit as u64)),
                ],
            );
            if state.ram_limit > 0 && state.ram_used * 10 > state.ram_limit * 9 {
                // ใกล้เต็ม — ให้เห็นชัดว่ากำลังตึง
                ui.colored_label(egui::Color32::from_rgb(230, 160, 60), ram);
            } else {
                ui.label(ram);
            }

            ui.separator();
            // ★ I-6: VRAM ต้องเห็นด้วยตาเหมือน RAM
            let vram = text::fill(
                lang,
                Template::Vram,
                &[
                    ("used", &human_bytes(state.vram_used as u64)),
                    ("limit", &human_bytes(state.vram_limit as u64)),
                ],
            );
            if state.vram_limit > 0 && state.vram_used * 10 > state.vram_limit * 9 {
                ui.colored_label(egui::Color32::from_rgb(230, 160, 60), vram);
            } else {
                ui.label(vram);
            }

            ui.separator();
            ui.label(text::fill(
                lang,
                Template::CacheSummary,
                &[
                    ("n", &state.cache_thumbs.to_string()),
                    ("size", &human_bytes(state.cache_bytes)),
                ],
            ));

            // ★ I-6: ชั้น B มีงบของตัวเอง ต้องเห็นด้วยตาเหมือน RAM/VRAM
            if state.working_limit > 0 && state.working_used > 0 {
                ui.separator();
                ui.label(text::fill(
                    lang,
                    Template::WorkingTextures,
                    &[
                        ("used", &human_bytes(state.working_used as u64)),
                        ("limit", &human_bytes(state.working_limit as u64)),
                        ("calls", &state.draw_calls.to_string()),
                        ("evicted", &state.working_evicted.to_string()),
                    ],
                ));
            }

            if state.decode_queued > 0 {
                ui.separator();
                ui.label(text::fill(
                    lang,
                    Template::DecodeQueued,
                    &[("n", &state.decode_queued.to_string())],
                ));
            }
            if state.decode_cancelled > 0 {
                ui.separator();
                ui.label(text::fill(
                    lang,
                    Template::DecodeCancelled,
                    &[("n", &state.decode_cancelled.to_string())],
                ));
            }
        });
    });

    // ---- ซ้าย: library ----
    egui::Panel::left("refx-library")
        .default_size(200.0)
        .show_inside(ui, |ui| {
            ui.heading(text::t(lang, Key::Library));
            ui.separator();
            ui.label(text::t(lang, Key::LibraryPlaceholder));
            ui.small(text::t(lang, Key::LibraryDropHint));
        });

    // ---- ขวา: inspector ----
    egui::Panel::right("refx-inspector")
        .default_size(240.0)
        .show_inside(ui, |ui| {
            ui.heading(text::t(lang, Key::Inspector));
            ui.separator();
            // docs/03 §1: inspector ปรับตัวตาม mode
            match state.mode {
                Mode::Canvas => {
                    ui.label(text::t(lang, Key::InspectorCanvasGeometry));
                    ui.label(text::t(lang, Key::InspectorCanvasTransform));
                    ui.small("P2-5 … P2-8");
                }
                Mode::Arrange => {
                    ui.label(text::t(lang, Key::InspectorArrangeMeta));
                    ui.label(text::t(lang, Key::InspectorArrangeGroup));
                    ui.small("P3-1");
                }
            }
        });

    // ---- กลาง: viewport ของ mode ปัจจุบัน ----
    //
    // ★ `Frame::NONE` สำคัญมาก ห้ามเอาออก
    //   ภาพของผู้ใช้ถูกวาดด้วย wgpu **ใต้** egui อีกที (docs/04 §2 — pass เดียว)
    //   ถ้า CentralPanel ทาพื้นหลังทึบตาม theme (`panel_fill` ซึ่งเป็นค่าปริยาย)
    //   มันจะกลบ quad ทุกอันจนหมด แล้ว canvas จะว่างเปล่าทั้งที่ทุกอย่างทำงานถูก
    //   — อาการนี้เกิดจริงตั้งแต่ P0-6 และไม่มี log ไหนจับได้เลยเพราะการวาดสำเร็จหมด
    //
    //   (egui 0.34: `Frame::none()` ถูก deprecate แล้ว ต้องใช้ `Frame::NONE`)
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| viewport(ui))
        .response
        .rect
}

/// ปุ่มเครื่องมือของ Canvas mode
fn canvas_tools(ui: &mut egui::Ui, state: &mut ShellState) {
    let lang = state.lang;
    for (key, when) in [
        (Key::ToolSelect, "P2-4"),
        (Key::ToolMove, "P2-5"),
        (Key::ToolCrop, "P2-7"),
        (Key::ToolGrayscale, "P2-8"),
    ] {
        let label = text::t(lang, key);
        if ui.button(label).on_hover_text(when).clicked() {
            state.status = text::fill(
                lang,
                Template::NotImplemented,
                &[("what", label), ("when", when)],
            );
        }
    }
}

/// ปุ่มเครื่องมือของ Arrange mode
fn arrange_tools(ui: &mut egui::Ui, state: &mut ShellState) {
    let lang = state.lang;
    for (key, when) in [
        (Key::ToolSort, "P3-2"),
        (Key::ToolFilter, "P3-4"),
        (Key::ToolTag, "P3-1"),
        (Key::ToolSendToCanvas, "P3-5"),
    ] {
        let label = text::t(lang, key);
        if ui.button(label).on_hover_text(when).clicked() {
            state.status = text::fill(
                lang,
                Template::NotImplemented,
                &[("what", label), ("when", when)],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn default_state_starts_in_canvas_mode() {
        let state = ShellState::default();
        assert_eq!(state.mode, Mode::Canvas);
    }

    #[test]
    fn mode_labels_are_distinct() {
        assert_ne!(Mode::Canvas.label(), Mode::Arrange.label());
    }

    // ---------- ★ canvas ต้องโปร่ง ----------
    //
    // egui รันได้โดยไม่มี GPU (มันแค่ผลิตรูปทรงออกมา) จึงทดสอบเรื่องนี้ได้จริง
    // ไม่ใช่แค่ตรวจว่ามีโค้ด `.frame(...)` อยู่

    const SCREEN: egui::Vec2 = egui::Vec2::new(1280.0, 800.0);

    /// รัน shell แบบไม่มีหน้าต่างจริง คืน (rect ของ canvas, รูปทรงที่วาด)
    ///
    /// ต้องรันสองรอบ: egui เป็น immediate mode ที่ใช้ layout ของรอบก่อนหน้า
    /// รอบแรกจึงยังได้ขนาด panel ที่ยังไม่นิ่ง
    fn run_shell() -> (egui::Rect, Vec<egui::epaint::ClippedShape>) {
        let ctx = egui::Context::default();
        let mut state = ShellState::default();
        let mut canvas = egui::Rect::NOTHING;
        let mut shapes = Vec::new();

        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                canvas = draw_in_ui(ui, &mut state, |ui| {
                    ui.allocate_space(ui.available_size());
                });
            });
            shapes = output.shapes;
        }
        (canvas, shapes)
    }

    /// ★ บั๊กที่ทำให้ canvas ว่างเปล่ามาตั้งแต่ P0-6
    ///
    /// ภาพของผู้ใช้ถูกวาดด้วย wgpu **ใต้** egui ถ้า `CentralPanel` ทาพื้นหลังทึบ
    /// (ค่าปริยายของ egui) มันจะกลบภาพทุกใบโดยที่ไม่มี log ไหนจับได้เลย
    /// เพราะทุกขั้นตอน "สำเร็จ" หมด
    #[test]
    fn nothing_opaque_is_painted_over_the_canvas() {
        let (canvas, shapes) = run_shell();
        let center = canvas.center();

        for clipped in &shapes {
            let egui::Shape::Rect(rect) = &clipped.shape else {
                continue;
            };
            assert!(
                !(rect.rect.contains(center) && rect.fill.a() > 0),
                "มีสี่เหลี่ยมทึบ (alpha {}) ทับกลาง canvas ที่ {center:?} — \
                 ภาพของผู้ใช้จะถูกกลบทั้งหมด (ต้องใช้ Frame::NONE)",
                rect.fill.a()
            );
        }
    }

    /// rect ที่คืนออกไปต้องเป็นช่องกลางจริง ๆ ไม่ใช่ทั้งหน้าต่าง
    ///
    /// ผู้เรียกเอาไปตั้ง `set_viewport` และเป็นกรอบอ้างอิงของกล้อง
    /// ถ้าคืนขนาดหน้าต่างทั้งบาน ภาพจะเยื้องแล้วขอบไปอยู่ใต้ panel
    #[test]
    fn canvas_rect_excludes_the_side_panels() {
        let (canvas, _) = run_shell();

        assert!(
            canvas.width() > 100.0 && canvas.height() > 100.0,
            "{canvas:?}"
        );
        assert!(canvas.min.x > 0.0, "ต้องเว้นที่ให้ Library ทางซ้าย: {canvas:?}");
        assert!(
            canvas.max.x < SCREEN.x,
            "ต้องเว้นที่ให้ Inspector ทางขวา: {canvas:?}"
        );
        assert!(
            canvas.min.y > 0.0,
            "ต้องเว้นที่ให้ tabs/toolbar ด้านบน: {canvas:?}"
        );
        assert!(
            canvas.max.y < SCREEN.y,
            "ต้องเว้นที่ให้ status bar ด้านล่าง: {canvas:?}"
        );
    }
}
