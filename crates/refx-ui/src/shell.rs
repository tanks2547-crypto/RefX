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

use refx_core::align::{Align as A, Distribute as D};
use refx_core::view::Mode;

use crate::text::{self, Key, Lang, Template};

/// ค่าการแสดงผลที่ inspector ปรับได้ — สำเนาของช่องใน `ItemCanvas` ที่เกี่ยวข้อง
///
/// ★ เป็น **สำเนา** ไม่ใช่ `&mut ItemCanvas` โดยตั้งใจ: ทุกการแก้ `Board`
/// ต้องผ่าน `Command` (docs/08 §4 ข้อ 10) ถ้าปล่อย `&mut` เข้ามาถึง widget
/// egui จะเขียนทับ board ตรง ๆ แล้วกฎนั้นก็หายไปโดยไม่มีอะไรบังคับ
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    /// ความทึบ 0..=1
    pub opacity: f32,
    /// ขาวดำเฉพาะภาพนี้
    pub grayscale: bool,
    /// กลับสี
    pub invert: bool,
    /// ความสว่าง -1..=1
    pub brightness: f32,
    /// คอนทราสต์ -1..=1
    pub contrast: f32,
    /// การพลิก
    pub flip: refx_core::board::Flip,
}

/// ข้อมูลฝั่ง Arrange ของสิ่งที่เลือกอยู่ — **ค่าสำหรับแสดงเท่านั้น** (P3-1)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MetaView {
    /// ดาว 0..=5
    pub rating: u8,
    /// ป้ายสี — `Some(Unknown(_))` = ป้ายจากรุ่นใหม่กว่าที่รุ่นนี้ไม่รู้จัก
    pub color_label: Option<refx_core::board::ColorLabel>,
    /// ปักหมุด (arrange จะไม่ย้าย)
    pub pinned: bool,
    /// โน้ตของผู้ใช้ — ★ **คนละตัวกับ `ItemKind::Text`** ตัวนั้นเป็น item บน canvas
    /// ส่วนตัวนี้เป็น metadata ที่ติดกับ item ใบไหนก็ได้ รวมทั้งภาพ
    pub note: String,
    /// ชื่อแท็กของ item นี้ เรียงตาม `TagId` (deterministic)
    pub tags: Vec<String>,
}

/// สิ่งที่ผู้ใช้ขอแก้ในเฟรมนี้ — **`None` = ไม่ได้แตะอะไรเลย** (P3-1)
///
/// ★ แยกจาก [`MetaView`] ด้วยเหตุผลเดียวกับ `appearance_edit` / `note_edit`:
/// ค่าที่ค้างอยู่ซึ่งถูกเขียนกลับทุกเฟรมจะทับสิ่งที่ undo เพิ่งคืนมา
/// (`docs/08 §3.9` ข้อ 8.1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaRequest {
    /// ตั้งดาว
    Rating(u8),
    /// ตั้ง/ล้างป้ายสี
    ColorLabel(Option<refx_core::board::ColorLabel>),
    /// ปักหมุด
    Pinned(bool),
    /// แก้โน้ต
    Note(String),
    /// ติดแท็กชื่อนี้
    AddTag(String),
    /// ถอดแท็กชื่อนี้
    RemoveTag(String),
}

/// สิ่งที่ปุ่มจัดเรียงขอให้ทำ (P2-9)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrangeRequest {
    /// ชิดขอบ/กึ่งกลางตามที่ระบุ
    Align(refx_core::align::Align),
    /// กระจายระยะให้เท่ากัน
    Distribute(refx_core::align::Distribute),
}

/// สถานะที่ shell ต้องอ่าน/เขียน
///
/// P2+ จะขยายเป็น `App` เต็มที่มี board, selection, history
/// ตอนนี้เก็บเฉพาะที่ P0-8 ต้องใช้จริง — ไม่เดาโครงล่วงหน้า
#[derive(Debug)]
pub struct ShellState {
    /// mode ปัจจุบัน
    pub mode: Mode,
    /// ★ ค่าการแสดงผลของสิ่งที่เลือกอยู่ — inspector อ่านจากที่นี่และเขียนกลับที่นี่
    ///
    /// `None` = ไม่ได้เลือกอะไร · ชั้น `app` เติมค่าก่อนวาดแล้วอ่านกลับหลังวาด
    /// ถ้าต่างจากเดิมแปลว่าผู้ใช้ปรับ แล้วมันจะถูกห่อเป็น `SetFilter` เข้า `History`
    pub appearance: Option<Appearance>,
    /// ★★ **สิ่งที่ผู้ใช้ขอในเฟรมนี้** — `None` = ไม่ได้แตะอะไรเลย
    ///
    /// แยกจาก `appearance` (ที่เป็นแค่ค่าสำหรับ *แสดง*) โดยตั้งใจ เพราะเคยเป็นบั๊กจริง:
    /// เดิมชั้น `app` อ่าน `appearance` กลับไปเขียนลง board ทุกเฟรม ซึ่งแปลว่า
    /// **ค่าที่ค้างอยู่จากเฟรมก่อนจะทับสิ่งที่คีย์ลัดเพิ่งเปลี่ยน** — กด `H` แล้วภาพพลิก
    /// เสี้ยววินาทีแล้วเด้งกลับ โดยไม่มี error ที่ไหนเลย
    ///
    /// ตอนนี้มีเจ้าของเดียว: เขียนตรงนี้ **เฉพาะตอน widget รายงานว่าถูกแตะ**
    /// (`docs/08 §3.9` ข้อ 8.1 — ทำให้ API ถูกได้ทางเดียว ไม่ใช่ต้องใช้ให้ถูก)
    pub appearance_edit: Option<Appearance>,
    /// ผู้ใช้ปล่อยตัวควบคุมในเฟรมนี้ → ปิดหน้าต่าง merge (undo ขั้นใหม่)
    pub appearance_sealed: bool,
    /// ★ ผู้ใช้กดปุ่ม align/distribute ในเฟรมนี้ — ชั้น `app` เป็นคนลงมือ
    ///
    /// เจ้าของเดียวเหมือน `appearance_edit`: widget แค่ **ขอ** ไม่ได้แก้ board เอง
    pub arrange_request: Option<ArrangeRequest>,
    /// ★ grayscale ทั้ง board — **สวิตช์ของการมองเห็น ไม่ใช่ของเอกสาร**
    ///
    /// docs/03 §2 เรียกมันว่า "uniform ตัวเดียว" ซึ่งเป็นสิ่งที่มันเป็นจริงในเส้นทางวาด
    /// ไม่อยู่ใน `Board` จึงไม่กิน undo และไม่ทำให้เอกสาร dirty
    /// (เหตุผลเดียวกับที่ `selection` ถูกย้ายออก — docs/02 §2.9)
    pub board_grayscale: bool,
    /// ★ จำนวน texture upload สะสม — หลักฐานของเกณฑ์ ROADMAP P2-8 ที่ **เห็นได้ด้วยตา**
    ///
    /// สลับ `G` แล้วเลขนี้ต้องไม่ขยับแม้แต่หนึ่ง ไม่ว่าบน board จะมีกี่ภาพ
    pub atlas_uploads: u64,
    /// เครื่องมือที่เลือกอยู่ — **อ่านอย่างเดียว** ชั้นแอปเขียนค่านี้ทุกเฟรม
    ///
    /// ★ เจ้าของจริงคือ `Gfx::tool` ที่เดียว ที่นี่เป็นแค่สำเนาไว้วาดปุ่มให้ถูก
    pub tool: refx_core::interact::Tool,
    /// ★ ผู้ใช้กดปุ่มเครื่องมือบน toolbar — ชั้นแอปมาเก็บไปแล้วล้างทิ้ง
    ///
    /// แยกจาก `tool` โดยตั้งใจ: ถ้าให้ปุ่มเขียน `tool` ตรง ๆ จะมีสองแหล่งความจริง
    /// แล้วต้องพึ่ง**ลำดับ**ว่าใครเขียนก่อน — เคยพลาดมาแล้วตอนทำ P2-7 คือคีย์ลัด
    /// เขียน `Gfx::tool` แล้วโดนค่าเก่าจาก `ShellState` เขียนทับกลับทุกเฟรม
    /// อาการคือ **กด `C` แล้วไม่มีอะไรเกิดขึ้น ส่วนกดปุ่มได้ปกติ** (docs/08 §3.9 ข้อ 8.1)
    pub tool_request: Option<refx_core::interact::Tool>,
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

    /// ★ เนื้อความของโน้ตที่เลือกอยู่ (P2-11) — `None` = ไม่ได้เลือกโน้ต
    ///
    /// **ค่าสำหรับ *แสดง* เท่านั้น** ชั้น `app` เติมให้ทุกเฟรม
    pub note: Option<String>,
    /// ★★ **สิ่งที่ผู้ใช้พิมพ์ในเฟรมนี้** — `None` = ไม่ได้แตะช่องข้อความเลย
    ///
    /// แยกจาก `note` ด้วยเหตุผลเดียวกับ `appearance_edit` เป๊ะ ๆ: ถ้าอ่าน `note`
    /// กลับไปเขียนลง board ทุกเฟรม ค่าที่ค้างจากเฟรมก่อนจะทับสิ่งที่ undo
    /// เพิ่งคืนมา — กด Ctrl+Z แล้วข้อความเด้งกลับทันที (`docs/08 §3.9` ข้อ 8.1)
    pub note_edit: Option<String>,
    /// ผู้ใช้ออกจากช่องข้อความแล้ว → ปิดหน้าต่าง merge (undo ขั้นใหม่)
    pub note_sealed: bool,

    /// ★ ข้อมูลฝั่ง Arrange ของสิ่งที่เลือกอยู่ (P3-1) — `None` = ไม่ได้เลือกอะไร
    pub meta: Option<MetaView>,
    /// ★★ สิ่งที่ผู้ใช้ขอแก้ในเฟรมนี้ — `None` = ไม่ได้แตะ
    pub meta_request: Option<MetaRequest>,
    /// ผู้ใช้ออกจากช่องโน้ตแล้ว → ปิดหน้าต่าง merge
    pub meta_sealed: bool,
    /// ช่องพิมพ์ชื่อแท็กใหม่ — **สถานะของ widget ล้วน ๆ** ไม่ใช่ของเอกสาร
    pub tag_input: String,

    /// ★ สีที่ picker อ่านได้ล่าสุด (P2-10) — `None` = ยังไม่ได้จิ้มอะไร
    ///
    /// **เป็นสีของ pixel ต้นฉบับ** ไม่ได้ผ่าน grayscale/brightness/opacity ใด ๆ
    /// จึงต่างจากสีที่เห็นบนจอได้ และนั่นคือสิ่งที่ถูกต้อง (ROADMAP P2-10)
    pub picked: Option<refx_core::pick::Picked>,
    /// ★ ไม้บรรทัดที่วางอยู่ (P2-10) — ตัวเลขเป็น **world unit** ไม่ใช่พิกเซลบนจอ
    pub measured: Option<refx_core::pick::Measurement>,

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
            appearance: None,
            appearance_edit: None,
            arrange_request: None,
            appearance_sealed: false,
            board_grayscale: false,
            atlas_uploads: 0,
            tool: refx_core::interact::Tool::default(),
            tool_request: None,
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
            note: None,
            note_edit: None,
            note_sealed: false,
            meta: None,
            meta_request: None,
            meta_sealed: false,
            tag_input: String::new(),
            picked: None,
            measured: None,
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
            // ★ สีที่จิ้มได้ (P2-10) — ตัวอย่างสีคู่กับ hex เสมอ
            //   ตัวเลขอย่างเดียวอ่านไม่ออกด้วยตา ส่วนสีอย่างเดียวก๊อปไปใช้ไม่ได้
            if let Some(picked) = state.picked {
                ui.separator();
                let (rect, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
                let [r, g, b, _] = picked.rgba;
                ui.painter()
                    .rect_filled(rect, 2.0, egui::Color32::from_rgb(r, g, b));
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(1.0, egui::Color32::from_gray(140)),
                    egui::StrokeKind::Middle,
                );
                ui.label(picked.hex()).on_hover_text(format!(
                    "source pixel {}, {}",
                    picked.source_px.0, picked.source_px.1
                ));
            }
            // ★ ไม้บรรทัด (P2-10) — หน่วยเป็น world ตัวเลขจึงไม่ขยับตอนซูม
            if let Some(m) = state.measured {
                ui.separator();
                let extent = m.extent();
                ui.label(format!(
                    "{:.1} u  ({:.1} x {:.1})  {:.1}°",
                    m.length(),
                    extent.x,
                    extent.y,
                    m.angle_deg()
                ));
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

            // ★ หลักฐานของเกณฑ์ P2-8 ที่เห็นได้ด้วยตา — สลับ `G` แล้วเลขนี้ต้องนิ่ง
            ui.separator();
            ui.label(text::fill(
                lang,
                Template::AtlasUploads,
                &[("uploads", &state.atlas_uploads.to_string())],
            ));

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
                Mode::Canvas => canvas_inspector(ui, state),
                Mode::Arrange => arrange_inspector(ui, state),
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
    use refx_core::interact::Tool;

    let lang = state.lang;
    // ★ เครื่องมือจริงเป็นปุ่มสลับที่ชี้ `state.tool` ตัวเดียวกับคีย์ลัด
    //   (`ToolMove` ไม่ได้อยู่ที่นี่แล้ว — docs/03 §2 ระบุว่าการย้ายเป็นส่วนหนึ่งของ
    //   Select/Move ปุ่ม `V` ตัวเดียว การมีปุ่มแยกทำให้เข้าใจผิดว่าเป็นคนละโหมด)
    for (tool, key, hint) in [
        (Tool::Select, Key::ToolSelect, "V"),
        (Tool::Crop, Key::ToolCrop, "C"),
        (Tool::Picker, Key::ToolPicker, "I"),
        (Tool::Measure, Key::ToolMeasure, "M"),
        (Tool::Text, Key::ToolText, "T"),
    ] {
        let label = text::t(lang, key);
        if ui
            .selectable_label(state.tool == tool, label)
            .on_hover_text(hint)
            .clicked()
        {
            state.tool_request = Some(tool);
        }
    }
    // ★ grayscale ทั้ง board — เป็นปุ่มสลับจริงตั้งแต่ P2-8 (ชี้ค่าเดียวกับปุ่ม `G`)
    if ui
        .selectable_label(state.board_grayscale, text::t(lang, Key::ToolGrayscale))
        .on_hover_text("G")
        .clicked()
    {
        state.board_grayscale = !state.board_grayscale;
    }

    ui.separator();
    // ★ จัดเรียง (P2-9) — ป้ายบนปุ่มอยู่ใน `ARRANGE_BUTTONS` คำอธิบายอยู่ใน tooltip
    for (request, glyph, key) in ARRANGE_BUTTONS {
        if ui.button(glyph).on_hover_text(text::t(lang, key)).clicked() {
            state.arrange_request = Some(request);
        }
    }
}

/// ★★ สัญลักษณ์ทุกตัวที่ Arrange inspector วาด (P3-1)
///
/// อยู่รวมกันที่นี่เพื่อให้เทสต์ `arrange_inspector_glyphs_all_exist` ไล่ตรวจได้
/// **ทุกครั้งที่ build** — เพิ่ม/เปลี่ยนสัญลักษณ์เมื่อไหร่ เทสต์จะแดงทันที
/// ถ้าฟอนต์ที่เราฝังไม่มี glyph นั้น (บทเรียนจาก P2-9)
///
/// ★ `#[cfg(test)]` เพราะโค้ดจริงใช้ค่าคงที่แต่ละตัวโดยตรง รายการนี้มีไว้ให้เทสต์
/// ไล่เท่านั้น · **ข้อจำกัดที่ต้องรู้:** มันกันการ *เปลี่ยนค่า* ของสัญลักษณ์ที่มีอยู่
/// ได้แน่นอน แต่กัน "เพิ่มสัญลักษณ์ใหม่แล้วลืมมาต่อท้ายที่นี่" ไม่ได้
/// (ต่างจาก `ARRANGE_BUTTONS` ที่ตัวมันเองคือข้อมูลที่ UI วาด จึงลืมไม่ได้เชิงโครงสร้าง)
#[cfg(test)]
pub(crate) const META_GLYPHS: [&str; 3] = [STAR_FULL, STAR_EMPTY, LABEL_NONE];

/// ★ ดาวที่ให้แล้ว (U+2605) — ★ ตรวจแล้วว่าฟอนต์ที่ฝังมีจริง
const STAR_FULL: &str = "\u{2605}";
/// ดาวที่ยังไม่ให้ (U+2606)
const STAR_EMPTY: &str = "\u{2606}";
/// ล้างป้ายสี (U+00D7)
///
/// ★ ตรวจแล้วว่า U+2713 (check) กับ U+25CF (วงกลมทึบ) **ไม่มี** ในฟอนต์ที่ฝัง
/// จึงใช้ไม่ได้ — เดาไม่ได้ ต้องตรวจ (บทเรียนจาก P2-9)
const LABEL_NONE: &str = "\u{00D7}";

// ★ ปักหมุดใช้ `checkbox` ของ egui — egui วาดเครื่องหมายเอง
// จึงไม่ต้องพึ่ง glyph ในฟอนต์เลย (U+2713 ไม่มีในฟอนต์ที่เราฝัง)

/// ★★ ปุ่มจัดเรียงทั้งแปด — **ป้ายต้องเป็นตัวอักษรที่ฟอนต์ที่เราฝังมี glyph จริง**
///
/// รอบแรกใช้สัญลักษณ์เส้นตาราง ซึ่งดูเหมาะที่สุด แต่ทั้งฟอนต์ละตินของ egui และ
/// Noto Sans Thai **ไม่มี glyph พวกนั้น** ผลคือปุ่มขึ้นเป็นกล่องสี่เหลี่ยมว่างบนจอจริง
/// โดยไม่มี error ที่ไหนเลย (docs/03 §0: ไม่ฝัง CJK เพราะเพดาน binary)
///
/// `↔`/`↕` ใช้ได้เพราะ Noto Sans Thai มีให้ ส่วน `← → ↑ ↓` **ไม่มี** — เดาไม่ได้
/// ต้องตรวจ · เทสต์ `arrange_button_labels_all_have_glyphs` ยืนยันให้ทุกครั้งที่ build
pub(crate) const ARRANGE_BUTTONS: [(ArrangeRequest, &str, Key); 8] = [
    (ArrangeRequest::Align(A::Left), "L", Key::AlignLeft),
    (ArrangeRequest::Align(A::CentreX), "C", Key::AlignCentreX),
    (ArrangeRequest::Align(A::Right), "R", Key::AlignRight),
    (ArrangeRequest::Align(A::Top), "T", Key::AlignTop),
    (ArrangeRequest::Align(A::CentreY), "M", Key::AlignCentreY),
    (ArrangeRequest::Align(A::Bottom), "B", Key::AlignBottom),
    (
        ArrangeRequest::Distribute(D::Horizontal),
        "↔",
        Key::DistributeX,
    ),
    (
        ArrangeRequest::Distribute(D::Vertical),
        "↕",
        Key::DistributeY,
    ),
];

/// ★ inspector ของ Canvas mode — ช่องที่ docs/03 §1 กำหนดไว้ (opacity, filter)
///
/// ตัวควบคุมเขียนลง `state.appearance` เท่านั้น **ไม่แตะ `Board` เลย** ชั้น `app`
/// เป็นคนเทียบกับค่าเดิมแล้วห่อเป็น `SetFilter` เข้า `History` — ทางเดียวที่กฎ
/// "ทุก mutation ผ่าน Command" ยังบังคับได้จริงเมื่อ widget เป็นคนแก้ค่า
fn canvas_inspector(ui: &mut egui::Ui, state: &mut ShellState) {
    use refx_core::board::Flip;

    let lang = state.lang;
    ui.label(text::t(lang, Key::InspectorCanvasGeometry));
    ui.separator();

    // ★ โน้ตข้อความ (P2-11) — ช่องนี้มาก่อนเพราะเป็นทั้งหมดที่โน้ตมีให้แก้
    //   ★★ egui กิน keyboard ให้เองเมื่อช่องนี้มี focus และชั้น `app` หยุด
    //   คีย์ลัดทุกตัวตาม `egui_wants_keyboard_input()` — space จึงเป็นอักขระจริง
    if let Some(note) = state.note.as_mut() {
        ui.label(text::t(lang, Key::Note));
        let response = ui.add(
            egui::TextEdit::multiline(note)
                .desired_width(f32::INFINITY)
                .desired_rows(6)
                .hint_text(text::t(lang, Key::NoteHint)),
        );
        let wanted = note.clone();
        // เขียน "สิ่งที่ผู้ใช้ขอ" เฉพาะตอนมีคนพิมพ์จริง — เจ้าของเดียว ไม่มีการทับกัน
        state.note_edit = response.changed().then_some(wanted);
        // ออกจากช่องแล้ว = จบหนึ่งขั้น undo · ระหว่างพิมพ์อยู่ยังยุบเป็นขั้นเดียว
        state.note_sealed = response.lost_focus();
        ui.separator();
    } else {
        state.note_edit = None;
        state.note_sealed = false;
    }

    let Some(appearance) = state.appearance.as_mut() else {
        if state.note.is_none() {
            ui.label(text::t(lang, Key::InspectorNoSelection));
        }
        return;
    };

    // ★ `touched` = ผู้ใช้แตะตัวควบคุมในเฟรมนี้จริง ๆ · `released` = ปล่อยแล้ว (seal)
    //   ถ้าไม่แยกสองอย่างนี้ ค่าที่ค้างอยู่จะถูกเขียนกลับลง board ทุกเฟรม
    //   แล้วมันจะทับสิ่งที่คีย์ลัดเพิ่งเปลี่ยน (เคยเป็นบั๊กจริงตอนทำ P2-8)
    let mut touched = false;
    let mut released = false;
    let mut note = |response: &egui::Response| {
        touched |= response.changed();
        released |= response.drag_stopped() || response.lost_focus();
    };

    ui.label(text::t(lang, Key::Opacity));
    note(&ui.add(egui::Slider::new(&mut appearance.opacity, 0.0..=1.0).show_value(true)));

    ui.separator();
    note(&ui.checkbox(&mut appearance.grayscale, text::t(lang, Key::ToolGrayscale)));
    note(&ui.checkbox(&mut appearance.invert, text::t(lang, Key::Invert)));

    ui.label(text::t(lang, Key::Brightness));
    note(&ui.add(egui::Slider::new(&mut appearance.brightness, -1.0..=1.0)));

    ui.label(text::t(lang, Key::Contrast));
    note(&ui.add(egui::Slider::new(&mut appearance.contrast, -1.0..=1.0)));

    ui.separator();
    ui.label(text::t(lang, Key::Flip));
    ui.horizontal(|ui| {
        for (flip, label) in [
            (Flip::None, "—"),
            (Flip::Horizontal, "↔"),
            (Flip::Vertical, "↕"),
            (Flip::Both, "⇄⇅"),
        ] {
            if ui
                .selectable_label(appearance.flip == flip, label)
                .clicked()
            {
                appearance.flip = flip;
                touched = true;
                // ปุ่มเป็นการกดครั้งเดียว ต้อง seal ทันที ไม่งั้นการกดถัดไปถูกกลืน
                released = true;
            }
        }
    });

    let wanted = *appearance;
    // ★★ เขียน "สิ่งที่ผู้ใช้ขอ" **เฉพาะตอนมีคนแตะจริง** — เจ้าของเดียว ไม่มีการทับกัน
    state.appearance_edit = touched.then_some(wanted);
    state.appearance_sealed = released;
}

/// ★ inspector ของ Arrange mode — tag / rating / color label / pinned / note (P3-1)
///
/// ★★ **ทุกตัวควบคุมเขียนลง `state.meta_request` เท่านั้น ไม่แตะ `Board` เลย**
/// ชั้น `app` เป็นคนห่อเป็น `EditMeta` / `TagItems` เข้า `History` — ทางเดียวที่กฎ
/// "ทุก mutation ผ่าน Command" ยังบังคับได้จริงเมื่อ widget เป็นคนแก้ค่า
///
/// ★ เขียนคำขอ **ทีละหนึ่ง** ต่อเฟรม (enum ไม่ใช่ struct) เพราะ `EditMeta`
/// merge **ต่อ field**: ให้ดาวแล้วใส่แท็กต้องเป็นคนละขั้น undo ถ้าส่งทั้งก้อน
/// ทุกเฟรม เราจะแยกไม่ออกว่าผู้ใช้เพิ่งแตะอะไร (docs/02 §3)
fn arrange_inspector(ui: &mut egui::Ui, state: &mut ShellState) {
    use refx_core::board::ColorLabel;

    let lang = state.lang;
    ui.label(text::t(lang, Key::InspectorArrangeMeta));
    ui.separator();

    let Some(meta) = state.meta.clone() else {
        ui.label(text::t(lang, Key::InspectorNoSelection));
        state.meta_request = None;
        state.meta_sealed = false;
        return;
    };
    let mut request = None;
    let mut sealed = false;

    // ---- ดาว ----
    ui.label(text::t(lang, Key::Rating));
    ui.horizontal(|ui| {
        for star in 1..=refx_core::board::ItemMeta::MAX_RATING {
            let filled = meta.rating >= star;
            let glyph = if filled { STAR_FULL } else { STAR_EMPTY };
            if ui.selectable_label(filled, glyph).clicked() {
                // ★ กดดาวที่ให้อยู่แล้ว = ล้างเป็น 0 — ไม่งั้นลดดาวไม่ได้เลย
                request = Some(MetaRequest::Rating(if meta.rating == star {
                    0
                } else {
                    star
                }));
                sealed = true;
            }
        }
        ui.label(format!("{}", meta.rating));
    });

    // ---- ป้ายสี ----
    ui.separator();
    ui.label(text::t(lang, Key::ColorLabelTitle));
    ui.horizontal(|ui| {
        // ล้างป้าย
        if ui
            .selectable_label(meta.color_label.is_none(), LABEL_NONE)
            .on_hover_text(text::t(lang, Key::ColorLabelNone))
            .clicked()
        {
            request = Some(MetaRequest::ColorLabel(None));
            sealed = true;
        }
        for label in ColorLabel::CHOICES {
            let picked = meta.color_label == Some(label);
            let (rect, response) =
                ui.allocate_exact_size(egui::Vec2::splat(18.0), egui::Sense::click());
            if let Some([r, g, b]) = label.rgb() {
                ui.painter()
                    .rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
            }
            if picked {
                ui.painter().rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                    egui::StrokeKind::Middle,
                );
            }
            if response.clicked() {
                request = Some(MetaRequest::ColorLabel(Some(label)));
                sealed = true;
            }
        }
    });
    // ★★ ป้ายที่รุ่นนี้ไม่รู้จัก — บอกตรง ๆ **ห้ามวาดเป็นสีใดสีหนึ่ง**
    //    ถ้าเดาสีให้ ผู้ใช้จะคิดว่ามันเป็นสีจริงแล้วเผลอกดทับ ซึ่งคือการทำลาย
    //    ค่าที่เราอุตส่าห์ถือไว้เพื่อเขียนกลับ (docs/02 §2.9)
    if meta.color_label.is_some_and(|label| !label.is_known()) {
        ui.small(text::t(lang, Key::ColorLabelUnknown));
    }

    // ---- ปักหมุด ----
    ui.separator();
    let mut pinned = meta.pinned;
    if ui
        .checkbox(&mut pinned, text::t(lang, Key::Pinned))
        .changed()
    {
        request = Some(MetaRequest::Pinned(pinned));
        sealed = true;
    }

    // ---- แท็ก ----
    ui.separator();
    ui.label(text::t(lang, Key::Tags));
    ui.horizontal_wrapped(|ui| {
        for name in &meta.tags {
            // กดที่แท็ก = ถอดออก · tooltip บอกไว้เพราะเดาเองไม่ได้
            if ui
                .button(name)
                .on_hover_text(text::t(lang, Key::RemoveTagHint))
                .clicked()
            {
                request = Some(MetaRequest::RemoveTag(name.clone()));
                sealed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut state.tag_input)
                .desired_width(120.0)
                .hint_text(text::t(lang, Key::AddTagHint)),
        );
        // Enter หรือกดปุ่ม — ทั้งสองทางต้องได้ผลเดียวกัน
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (entered || ui.button("+").clicked()) && !state.tag_input.trim().is_empty() {
            request = Some(MetaRequest::AddTag(std::mem::take(&mut state.tag_input)));
            sealed = true;
        }
    });

    // ---- โน้ต ----
    ui.separator();
    ui.label(text::t(lang, Key::MetaNote));
    let mut note = meta.note.clone();
    let response = ui.add(
        egui::TextEdit::multiline(&mut note)
            .desired_width(f32::INFINITY)
            .desired_rows(4)
            .hint_text(text::t(lang, Key::NoteHint)),
    );
    if response.changed() {
        request = Some(MetaRequest::Note(note));
    }
    sealed |= response.lost_focus();

    state.meta_request = request;
    state.meta_sealed = sealed;
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

    /// ★★ inspector ต้อง **ไม่ขออะไร** ถ้าผู้ใช้ไม่ได้แตะมันในเฟรมนั้น
    ///
    /// เคยเป็นบั๊กจริงตอน P2-8: `appearance` ถูกอ่านกลับไปเขียนลง board ทุกเฟรม
    /// ค่าที่ค้างจากเฟรมก่อนจึง**ทับสิ่งที่คีย์ลัด `H` เพิ่งเปลี่ยน** — ภาพพลิกแล้วเด้งกลับ
    /// ทันทีโดยไม่มี error ที่ไหนเลย และ unit test ของแต่ละชิ้นก็ผ่านหมด
    /// เพราะแต่ละชิ้นถูกจริง ๆ สิ่งที่ผิดอยู่ระหว่างชิ้น (`docs/08 §3.9` ข้อ 8)
    #[test]
    fn the_inspector_asks_for_nothing_when_nobody_touches_it() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            appearance: Some(Appearance {
                opacity: 0.5,
                grayscale: true,
                invert: false,
                brightness: 0.25,
                contrast: -0.25,
                flip: refx_core::board::Flip::Horizontal,
            }),
            ..ShellState::default()
        };

        // วาดหลายเฟรมโดยไม่มี pointer/keyboard เลย
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let _ = draw_in_ui(ui, &mut state, |_| {});
            });
            assert!(
                state.appearance_edit.is_none(),
                "ไม่มีใครแตะ แต่ inspector กลับขอให้เขียนค่าลง board"
            );
        }
    }

    /// ★★ ช่องโน้ตต้อง **ไม่ขออะไร** ถ้าผู้ใช้ไม่ได้พิมพ์ในเฟรมนั้น (P2-11)
    ///
    /// เหตุผลเดียวกับ `the_inspector_asks_for_nothing_when_nobody_touches_it`:
    /// ถ้าค่าที่ค้างอยู่ถูกเขียนกลับลง board ทุกเฟรม มันจะทับสิ่งที่ undo เพิ่งคืนมา
    /// อาการคือ **กด Ctrl+Z แล้วข้อความเด้งกลับ** โดยไม่มี error ที่ไหนเลย
    #[test]
    fn the_note_field_asks_for_nothing_when_nobody_types() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            note: Some("เขียนไว้แล้ว".to_owned()),
            ..ShellState::default()
        };

        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let _ = draw_in_ui(ui, &mut state, |_| {});
            });
            assert!(
                state.note_edit.is_none(),
                "ไม่มีใครพิมพ์ แต่ช่องโน้ตกลับขอให้เขียนลง board"
            );
            assert!(!state.note_sealed);
        }
    }

    /// ไม่ได้เลือกโน้ต = ต้องไม่มีคำขอค้างจากรอบก่อน
    #[test]
    fn deselecting_a_note_clears_any_pending_edit() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            note: None,
            note_edit: Some("ค้างจากรอบก่อน".to_owned()),
            note_sealed: true,
            ..ShellState::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let _ = draw_in_ui(ui, &mut state, |_| {});
        });
        assert!(state.note_edit.is_none());
        assert!(!state.note_sealed);
    }

    /// ★★ แผง Arrange ต้อง **ไม่ขออะไร** ถ้าผู้ใช้ไม่ได้แตะมันในเฟรมนั้น (P3-1)
    ///
    /// เหตุผลเดียวกับแผง Canvas: ค่าที่ค้างแล้วถูกเขียนกลับทุกเฟรมจะทับสิ่งที่
    /// undo เพิ่งคืนมา — กด Ctrl+Z แล้วดาว/แท็กเด้งกลับทันที
    #[test]
    fn the_arrange_panel_asks_for_nothing_when_nobody_touches_it() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            mode: Mode::Arrange,
            meta: Some(MetaView {
                rating: 3,
                color_label: Some(refx_core::board::ColorLabel::Blue),
                pinned: true,
                note: "จดไว้".to_owned(),
                tags: vec!["portrait".to_owned()],
            }),
            ..ShellState::default()
        };

        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let _ = draw_in_ui(ui, &mut state, |_| {});
            });
            assert!(
                state.meta_request.is_none(),
                "ไม่มีใครแตะ แต่แผง Arrange กลับขอให้เขียนลง board"
            );
        }
    }

    /// ไม่ได้เลือกอะไร = ต้องไม่มีคำขอค้างจากรอบก่อน
    #[test]
    fn deselecting_clears_a_pending_meta_request() {
        let ctx = egui::Context::default();
        let mut state = ShellState {
            mode: Mode::Arrange,
            meta: None,
            meta_request: Some(MetaRequest::Rating(5)),
            meta_sealed: true,
            ..ShellState::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let _ = draw_in_ui(ui, &mut state, |_| {});
        });
        assert!(state.meta_request.is_none());
        assert!(!state.meta_sealed);
    }

    /// ★★ ป้ายสีที่รุ่นนี้ไม่รู้จักต้อง **ไม่ถูกวาดเป็นสีใดสีหนึ่ง**
    ///
    /// ถ้าเดาสีให้ ผู้ใช้จะคิดว่ามันเป็นสีจริงแล้วเผลอกดทับ ซึ่งทำลายค่าที่
    /// `ColorLabel::Unknown` อุตส่าห์ถือไว้เพื่อเขียนกลับ (docs/02 §2.9)
    #[test]
    fn an_unknown_colour_label_is_never_drawn_as_a_real_colour() {
        let unknown = refx_core::board::ColorLabel::from_wire(200).unwrap();
        assert!(unknown.rgb().is_none());
        // และไม่มีปุ่มไหนบน UI สร้างค่านี้ได้
        assert!(
            !refx_core::board::ColorLabel::CHOICES
                .iter()
                .any(|choice| !choice.is_known())
        );
    }

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
