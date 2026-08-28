# 03 — สอง Mode และ UI กลาง

crate: `refx-ui`

---

## 0. ภาษาและข้อความ (ตัดสิน 28 ก.ค. 2026)

**กลุ่มเป้าหมายคือนักวาดหลายประเทศ ไม่ใช่เฉพาะไทย**

| | |
|---|---|
| ภาษาหลักของ UI | **อังกฤษ** |
| ภาษาที่สอง | **ไทย** |
| CJK (จีน/ญี่ปุ่น/เกาหลี) | **ยังไม่รองรับ** — ดูเหตุผลด้านล่าง |

### ฟอนต์ที่ฝัง

ฝัง **Latin (ของ egui เดิม) + Noto Sans Thai (OFL-1.1)** เท่านั้น
`deny.toml` อนุญาต `OFL-1.1` ไว้แล้ว ไม่ต้องขอเพิ่ม

ใส่ฟอนต์ไทยต่อ **ท้าย** รายการของทั้ง `Proportional` และ `Monospace`
ไม่ใช่แทนที่ของเดิม — ให้ละตินยังใช้ฟอนต์ที่ออกแบบมาคู่กับ egui แล้วอักษรไทยค่อยตกมาที่ฟอนต์ใหม่

> **ทำไมยังไม่ฝัง CJK:** ฟอนต์ CJK ที่ครบชุดใหญ่ 16+ MB ซึ่ง**ทะลุเพดาน binary 25 MB**
> ใน `docs/08 §6` ทันที (ตอนนี้ใช้ 12 MB)
> **ห้ามฝัง Noto Sans CJK เข้ามาเฉย ๆ** ถ้าจะรองรับต้องกลับมาคุยเรื่องเพดานก่อน
> ทางที่เป็นไปได้ในอนาคต: ฟอนต์แยกไฟล์ข้าง binary หรือดึงจากระบบเฉพาะช่วง CJK
> (ห้ามดาวน์โหลด — **I-8 ห้ามมี network เด็ดขาด**)

### ★ ข้อผูกพัน license ของฟอนต์ — `cargo deny` จับให้ไม่ได้

`cargo deny` ตรวจเฉพาะ **crate** ฟอนต์เป็นไฟล์ asset จึงอยู่นอกสายตาเครื่องมือทั้งหมด
ความรับผิดชอบตกอยู่กับเราเองล้วน ๆ

**OFL-1.1 บังคับว่าต้องแจกสำเนา license ไปพร้อมกับฟอนต์เสมอ** ไม่ใช่แค่ระบุชื่อ license
→ ต้องมี `assets/fonts/OFL.txt` (ตัวเต็มจากต้นทาง) commit คู่กับไฟล์ `.ttf` **เสมอ**
→ ห้ามฝังฟอนต์โดยไม่มีไฟล์ license อยู่ข้าง ๆ แม้แต่ชั่วคราว

**ห้ามใช้ฟอนต์ของ Microsoft** (`LeelawUI` `leelawad` `tahoma` `segoeui` ฯลฯ)
license ผูกกับ Windows ห้ามแจกจ่ายต่อ — ต่อให้มีอยู่ในเครื่องที่ build ก็ตาม

> **`THIRD-PARTY-LICENSES.md` ที่ราก repo** ต้องมีก่อนแพ็กเกจใน P5-6
> รวมข้อผูกพันการแสดงที่มาของทุกอย่างที่เราแจกจ่าย: ฟอนต์ที่ฝัง (OFL-1.1),
> ฟอนต์เดิมของ egui (`Ubuntu-font-1.0`, OFL), และ crate ที่ต้องแสดง attribution
> ตอนนี้ยังไม่มีไฟล์นี้ — เป็นเงื่อนไขที่ต้องผ่านก่อนปล่อยให้คนอื่นใช้ ไม่ใช่แค่ของแถม

### ★ กฎเรื่องสตริง — ทำตั้งแต่ตอนนี้ ไม่ใช่ตอน P5

ทำ **ช่องต่อ** ไว้ก่อน ยังไม่ต้องทำระบบ i18n เต็มรูปแบบ และ**ยังไม่ต้องเพิ่ม dependency**

1. **ข้อความที่ผู้ใช้เห็นทุกตัวต้องผ่านที่เดียว** เช่น `refx-ui::text` — ห้ามเขียนสตริงตรงใน widget
   วันนี้ข้างในเป็นแค่ `match lang { En => "…", Th => "…" }` ก็พอ
   แต่พอมีช่องต่อแล้ว การเปลี่ยนไปใช้ระบบเต็มรูปแบบทีหลังคืองานเชิงกล ไม่ใช่การรื้อ
2. **แปลไม่ครบ → ตกกลับเป็นอังกฤษเสมอ** ห้ามโชว์ key ห้ามโชว์ช่องว่าง
3. **ภาษาเริ่มต้นอ่านจาก locale ของ OS** ตอนเปิดโปรแกรม (ไม่รู้จัก → อังกฤษ)
   อ่านผ่าน `refx-platform` แบบเดียวกับที่ทำ `total_ram()` — ไม่ต้องเพิ่ม dependency
   ให้ผู้ใช้เลือกทับได้ใน Settings ตอน P5-3

### ★ ข้อความ error: แยก "ของนักพัฒนา" ออกจาก "ของผู้ใช้"

`#[error("…")]` ของ `thiserror` เป็น format string ตอนคอมไพล์ → **แปลตอนรันไม่ได้**

→ `#[error(…)]` ให้เป็น **อังกฤษ สำหรับ log และนักพัฒนา** (ค้นหาง่าย ส่งต่อได้)
→ ข้อความที่ผู้ใช้เห็นให้ **ชั้น UI ประกอบขึ้นจากฟิลด์ของ error** แล้วแปลตามภาษา

โชคดีที่ `LoadError` มีฟิลด์ครบอยู่แล้ว (`width` `height` `pixels` `limit` `actual_mb` …)
งานนี้จึงเป็นการ**ย้ายข้อความ** ไม่ใช่ออกแบบใหม่

กฎเดิมใน `CLAUDE.md` ยังอยู่ครบ — ข้อความที่ผู้ใช้เห็นต้องบอก **สิ่งที่เกิดขึ้น + สิ่งที่ทำได้ต่อ**
แค่ย้ายที่อยู่ของมัน

---

## 1. Shell กลาง (โค้ดชุดเดียว ใช้ทั้งสอง mode)

`refx-ui/src/shell.rs` วาดกรอบทั้งหมด แล้วเจาะช่องกลางให้ mode ปัจจุบันวาดเอง

> **แก้ 26 ก.ค. 2026 — ตัวอย่างเดิมในเอกสารนี้ใช้ API ที่ egui 0.34 deprecate หมดแล้ว**
> `SidePanel` / `TopBottomPanel` / `CentralPanel::show(ctx, …)` และ `default_width`
> ทั้งหมดตกทันทีภายใต้ `clippy -D warnings` ของจริงคือ `egui::Panel::top/left/right/bottom`
> + `.show_inside(ui)` + `default_size` และ shell รับ `&mut Ui` ไม่ใช่ `&Context`
> (เรียกจากข้างใน `Context::run_ui`) ด้านล่างคือรูปแบบที่คอมไพล์ผ่านจริง

```rust
// เรียกจากข้างใน egui::Context::run_ui ซึ่งส่ง &mut Ui ของ root มาให้
pub fn draw_shell(ui: &mut egui::Ui, app: &mut App, viewport: impl FnOnce(&mut egui::Ui)) {
    egui::Panel::top("tabs").show_inside(ui, |ui| board_tabs(ui, app));
    egui::Panel::top("toolbar").show_inside(ui, |ui| {
        mode_switch(ui, app);                    // [Canvas | Arrange]
        ui.separator();
        match app.mode() {                        // ← ต่างกันแค่ตรงนี้
            Mode::Canvas  => canvas_tools(ui, app),
            Mode::Arrange => arrange_tools(ui, app),
        }
    });
    egui::Panel::left("library").default_size(220.0).show_inside(ui, |ui| library_panel(ui, app));
    egui::Panel::right("inspector").default_size(260.0).show_inside(ui, |ui| inspector_panel(ui, app));
    egui::Panel::bottom("status").show_inside(ui, |ui| status_bar(ui, app));
    // egui 0.34 ไม่มี Panel::center (มีแค่ top/bottom/left/right)
    // CentralPanel::show_inside ไม่ deprecate ใช้ตัวนี้
    egui::CentralPanel::default().show_inside(ui, viewport);   // ← เนื้อในต่างกันตาม mode
}
```

### ★ กับดักสองข้อที่ทำให้ canvas ตายเงียบมาตั้งแต่ P0 (บันทึก 28 ก.ค. 2026)

ทั้งสองข้อนี้ทำให้ **canvas ว่างเปล่าและ pan/zoom ไม่ทำงาน** โดยที่เทสต์ 170 ตัวผ่านหมด
เพราะไม่มีเทสต์ไหนเรนเดอร์แล้ว *ดู*

**1. `CentralPanel` ทาพื้นหลังทึบทับสิ่งที่วาดด้วย wgpu**
เราวาด quad ก่อนแล้วให้ egui วาดทับ (ถูกแล้ว) แต่ `CentralPanel` เติม `panel_fill`
เต็มพื้นที่โดยปริยาย → กลบทุกอย่างใต้มัน
→ ต้องใช้ `.frame(egui::Frame::NONE)` (`Frame::none()` ถูก deprecate ตั้งแต่ 0.34 ใช้แล้ว clippy ตก)

**2. `egui_wants_pointer_input()` เป็น `true` ทั่วทั้ง canvas**
`is_pointer_over_egui()` ของ layer พื้นหลังคำนวณจาก `!root_ui_available_rect.contains(pointer)`
และ `CentralPanel` **กิน root rect ที่เหลือจนหมด** → ค่านี้จริงทุกจุดบน canvas
→ ถ้าเช็ค `response.consumed` เดี่ยว ๆ แล้ว `return` จะทิ้ง event ทุกตัวก่อนถึงกล้อง

ทางแก้ชั่วคราวที่ใช้อยู่ — แยกสองกรณีที่ต่างกันจริง:

```rust
let egui_owns_pointer = egui_ctx.is_using_pointer()          // กำลังลาก widget อยู่
    || (response.consumed && !canvas_rect.contains(cursor));  // ชี้อยู่นอก canvas
```

> **ทางที่ถูกระยะยาว (ทำใน P2):** ให้ canvas เป็น widget จริงของ egui ด้วย
> `ui.allocate_response(rect, Sense::click_and_drag())` แล้วขับกล้องจาก response นั้น
> egui จะจัดลำดับความสำคัญของ pointer ให้เอง ไม่ต้องเดา
> **ข้อจำกัดของวิธีชั่วคราว:** ถ้าเอา widget ของ egui ไปวางในช่อง canvas ก่อนถึง P2 เราจะแย่ง event นั้นไป

### ★ ข้อความชั่วคราว vs ตัวบ่งชี้ถาวร (12 ส.ค. 2026)

**สภาวะที่ยังคงอยู่ ต้องมีตัวบ่งชี้ที่ยังคงอยู่ — ห้ามใช้ข้อความชั่วคราว**

เคสจริง (P4-4): กู้งานคืนสำเร็จแล้วขึ้นข้อความ *"กู้คืนแล้ว — กด Ctrl+S เพื่อเก็บไว้"*
บน status bar แต่ถูกรายงานความคืบหน้าของการโหลดภาพเขียนทับใน **~3 มิลลิวินาที**
ผู้ใช้ที่เพิ่งได้งานคืนจึงไม่มีทางรู้ว่างานนั้น**ยังไม่ถูกบันทึก** และปิดโปรแกรมทิ้งได้อีกรอบ

"งานนี้ยังไม่เคยถูกบันทึก" ไม่ใช่ *เหตุการณ์* ที่เกิดแล้วจบ — เป็น **สภาวะ**
ที่คงอยู่จนกว่าผู้ใช้จะกด Save

| ชนิด | ใช้อะไร |
|---|---|
| เหตุการณ์ (บันทึกสำเร็จ · ก๊อป hex แล้ว · เปิด 200 ไฟล์ใน 108 ms) | ข้อความชั่วคราวบน status bar |
| **สภาวะ** (ยังไม่บันทึก · board เต็ม · กำลังโหลด N/M · เปิดแบบอ่านอย่างเดียว) | **ตัวบ่งชี้ถาวรจนกว่าสภาวะจะหมดไป** |

### ★ กฎการทดสอบ UI ที่ตามมา

egui รันได้แบบ headless โดยไม่ต้องมี GPU — มันแค่ผลิต **รูปทรง** ออกมา
→ งานที่เกี่ยวกับสิ่งที่ผู้ใช้เห็น **ต้องมีเทสต์ที่รัน shell จริงแล้วไล่ตรวจรูปทรงที่ได้**
ไม่ใช่แค่ตรวจว่า "มีโค้ดเรียกอยู่"

ตัวอย่างที่ถูกในโปรเจกต์: `nothing_opaque_is_painted_over_the_canvas`
(ล้มทันทีถ้าใครเอา `Frame::NONE` ออก) และ `canvas_rect_excludes_the_side_panels`

**และก่อนบอกว่าเสร็จ ต้องมีภาพหน้าจอจริง** — log ที่บอกว่า draw call สำเร็จ
ไม่ได้แปลว่าตาเห็นอะไร บทเรียนนี้แลกมาด้วยงานหลาย session ที่สร้างบนฐานที่มองไม่เห็น

**สิ่งที่ shared จริง ๆ (ห้ามเขียนสองรอบ):** board tabs, mode switch, library panel, inspector, status bar, keymap dispatcher, selection model, undo/redo, ปุ่ม save/open, ระบบ theme

**สิ่งที่ต่าง:** เนื้อใน `CentralPanel` และปุ่มในกลุ่ม tools

```rust
pub trait ViewportBehavior {
    fn draw(&mut self, ui: &mut egui::Ui, rect: Rect);
    fn handle_input(&mut self, input: &InputState, board: &mut Board) -> Vec<Box<dyn Command>>;
    fn hit_test(&self, world: Vec2, board: &Board) -> Option<ItemId>;
    fn camera_mut<'a>(&self, view: &'a mut ViewState) -> &'a mut Camera;
}
```

> **ข้อควรระวังเรื่องความเสถียร:** `handle_input` **คืน** `Vec<Box<dyn Command>>` ไม่ใช่แก้ board เอง
> ทำแบบนี้เพื่อให้ทุกการเปลี่ยนแปลงไหลผ่านทางเดียว (history → journal) และเทสต์ interaction ได้โดยไม่ต้องเปิดหน้าต่าง

### Inspector ปรับตัวตาม mode

| | Canvas | Arrange |
|---|---|---|
| แสดง | X/Y, W/H, rotation, opacity, crop, lock, filter | tags, rating, color label, group, note, ข้อมูลไฟล์ |
| ส่วนที่เหมือนกัน | ชื่อไฟล์, ขนาด px, format, path, ปุ่ม reveal in folder |

---

## 2. Canvas mode

### Tools

| Tool | คีย์ | พฤติกรรม |
|---|---|---|
| Select/Move | `V` | คลิกเลือก, ลากย้าย, Shift+คลิก = เพิ่ม, ลากพื้นที่ว่าง = rubber-band |
| Pan | `Space` (ค้าง) / กลางเมาส์ | ค้างไว้ระหว่างใช้ tool อื่นได้ |
| Zoom | scroll / `Ctrl+scroll` | **ซูมเข้าหาตำแหน่งเคอร์เซอร์เสมอ** ไม่ใช่กลางจอ |
| Scale | ลาก handle มุม | `Shift` = คงสัดส่วน, `Alt` = จากจุดกึ่งกลาง |
| Rotate | ลากนอก handle มุม | `Shift` = สแนป 15° |
| Crop | `C` | non-destructive, ดับเบิลคลิกเพื่อรีเซ็ต |
| Color picker | `I` | อ่านสีจากภาพต้นฉบับ (ไม่ใช่จากหน้าจอ) + คัดลอก hex |
| Measure | `M` | ลากวัดระยะ/มุม — สำคัญมากสำหรับงานเทียบสัดส่วน |
| Text note | `T` | |

### สิ่งที่ต้องมีเพราะเป็นเครื่องมือของนักวาด

- **Grayscale toggle (`G`)** — สลับทั้ง board เป็นขาวดำเพื่อเช็ค value ทำใน shader (uniform ตัวเดียว) ไม่ต้อง decode ใหม่ ราคา ~0
- **Flip horizontal (`H`)** — เช็คสัดส่วนที่เพี้ยน ทำที่ UV ใน shader
- **Opacity + always-on-top** — วางภาพทับงานตัวเองเพื่อเทียบ
- **Snap/align** — align ซ้าย/กลาง/ขวา/บน/กลาง/ล่าง, distribute ระยะเท่ากัน (ทำงานกับ selection)

### กติกา interaction ที่ห้ามพลาด

- ซูมด้วยการ **scroll เปล่า** (ไม่ต้องกด modifier) — PureRef ทำแบบนี้และผู้ใช้เคยชิน; scroll+Shift = pan แนวนอน
- ลากภาพจาก Explorer/Finder เข้าหน้าต่างได้ (drag & drop) และ **paste จาก clipboard (`Ctrl+V`)** — สองทางนี้คือ 90% ของการเพิ่มภาพจริง
- ลากแล้วปล่อย = 1 undo (ดู `Command::merge` ใน 02-data-model)

---

## 3. Arrange mode

### Layout engines (`refx-core/src/layout/`)

ทุกตัวรับ `&[(ItemId, Vec2 /*aspect*/)]` + `LayoutParams` คืน `Vec<(ItemId, Vec2 pos, Vec2 size)>`
**เป็น pure function** ไม่แตะ board → เทสต์ง่าย, deterministic, รันซ้ำได้ผลเดิมเสมอ

| Layout | ใช้ตอน | อัลกอริทึม |
|---|---|---|
| `Grid` | คัดภาพ, ดูรวม | ช่องเท่ากัน, fit ภาพในช่อง |
| `Masonry` | ภาพสัดส่วนต่างกันมาก | คอลัมน์คงที่ วางลงคอลัมน์ที่เตี้ยสุด |
| `JustifiedRows` | ดูสบายตาที่สุด | เติมแถวจนเกินความกว้าง แล้วสเกลทั้งแถวให้พอดี (แบบ Flickr/Google Photos) |
| `ShelfPack` | อัดให้แน่นที่สุด | first-fit-decreasing-height |
| `Radial` | เทียบภาพรอบภาพหลัก | วางเป็นวงรอบ item ที่ pin ไว้ |

`LayoutParams { gap: f32, target_row_height: f32, columns: Option<u32>, respect_pinned: bool }`

**ต้อง deterministic** — input เดิมให้ output เดิมเป๊ะ ห้ามใช้ `HashMap` iteration order ในการคำนวณ layout (นี่คือบั๊ก "ภาพเรียงไม่เหมือนเดิมทุกครั้งที่กด" ที่หาสาเหตุยากมาก)

### Sort / Filter

Sort: `name | date_added | date_modified | file_size | aspect_ratio | rating | color_label | dominant_hue | canvas_order`
Filter: tag (AND/OR/NOT), rating ≥ N, color label, format, ช่วงขนาด, คำค้นในชื่อ/note

Filter ต้องประเมินแบบ lazy และ cache ผลไว้ ตราบใดที่ `board.revision` ไม่เปลี่ยน — ที่ 1000+ ภาพ การกรองใหม่ทุกเฟรมคือการเผา CPU ฟรี

### Virtual scrolling

Arrange mode วาดเฉพาะแถวที่อยู่ในจอ + 1 หน้าจอเป็น buffer บนล่าง
1000 ภาพ = วาดจริง ~40 ภาพ ที่เหลือแค่คำนวณตำแหน่ง (ถูกมาก)

---

## 4. สะพานเชื่อมสองโหมด

### 4.1 Arrange → Canvas

```
ปุ่ม "Apply layout to canvas"
  → รัน layout engine ปัจจุบัน (เฉพาะ item ที่ผ่าน filter, หรือเฉพาะที่เลือก)
  → สร้าง ApplyLayoutCommand { old: Vec<(ItemId, ItemCanvas)>, new: Vec<...> }
  → history.push → undo ครั้งเดียวคืนสภาพเดิมทั้งหมด
```

ตัวเลือกก่อนกด:
- ขอบเขต: ทุกภาพ / เฉพาะที่กรองไว้ / เฉพาะที่เลือก
- จุดวาง: ตำแหน่งเดิมของกลุ่ม / กลางจอ canvas / ต่อท้ายด้านล่าง
- **ไม่ขยับภาพที่ `pinned = true`** (layout จะจัดรอบมันแทน)

### 4.2 Canvas → Arrange

```
ปุ่ม "Sort by canvas order"
  → อ่านตำแหน่งบน canvas → เรียงแบบอ่านหนังสือ (บน→ล่าง, ซ้าย→ขวา, tolerance แถว = 50% ของความสูงเฉลี่ย)
  → เขียนลง ArrangeState.sort = CanvasOrder
```

### 4.3 หลักที่ห้ามละเมิด

**สลับ mode ห้ามแก้ข้อมูล** การกด `Canvas ⇄ Arrange` เป็นการเปลี่ยน view ล้วน ๆ ไม่สร้าง Command ไม่ทำให้ `dirty = true`
ข้อมูลจะเปลี่ยนก็ต่อเมื่อผู้ใช้กดปุ่มสะพานข้างบนอย่างจงใจเท่านั้น

---

## 5. Keymap

### ★★ โหมด Canvas/Arrange เป็นของ **แท็บ** ไม่ใช่ของหน้าต่าง (ตัดสิน 26 ส.ค. 2026)

`ViewState` เก็บโหมด **ต่อเอกสาร** และ persist ลงไฟล์อยู่แล้ว (`docs/02 §2`)
ถ้าปล่อยให้ `ShellState::mode` เป็นของหน้าต่าง จะได้ **แหล่งความจริงที่สอง**
ที่จะขัดกับไฟล์ทันทีที่มีคนต่อ `Board::view` กลับเข้ามาใช้จริง

→ โหมดอ่านจาก `Board::view` ของแท็บที่ active · สลับแท็บแล้วโหมดตามไปด้วย
→ กด `Tab` สลับโหมด = แก้ `view` ของแท็บนั้น (ไม่ผ่าน `Command` ไม่ dirty — `docs/02 §2.9`)

เหมือนแท็บเบราว์เซอร์ที่จำ scroll/zoom ของตัวเอง — ผู้ใช้ที่บันทึก board ไว้ในโหมด Arrange
ควรได้ Arrange กลับมาตอนเปิด ไม่ใช่โหมดที่แท็บอื่นบังเอิญค้างไว้

> ข้อนี้ปิดครึ่งที่เหลือของ P4-1: `set_view()` เขียน `Board::view` ทุกเฟรมแล้ว
> แต่ยังไม่มีใคร**อ่าน**มันกลับมาใช้ — ตอนนี้มีแล้ว

### ★ คีย์ของแท็บ (เพิ่ม 21 ส.ค. 2026 — P4-7)

| คีย์ | ทำอะไร |
|---|---|
| `Ctrl+O` | เปิดไฟล์ **เป็นแท็บใหม่** (เดิมแทนที่ board ปัจจุบันทั้งก้อน) |
| `Ctrl+T` | board เปล่าใหม่ |
| `Ctrl+W` | ปิดแท็บ — **ถามก่อนถ้ายังไม่บันทึก** |
| `Ctrl+Tab` | แท็บถัดไป |

> `Ctrl+O` เปลี่ยนพฤติกรรมโดยตั้งใจ — การแทนที่ board ที่ผู้ใช้กำลังทำอยู่
> โดยไม่ถามคือการทำงานหาย ซึ่งเป็นสิ่งที่โครงแท็บมีไว้แก้พอดี

เก็บเป็นตาราง `HashMap<(Modifiers, Key), Action>` โหลดจาก TOML ได้ (`keymap.toml`) — **ห้าม hard-code `if key == ...` กระจายทั่วโค้ด**

| คีย์ | ผล |
|---|---|
| `Tab` | สลับ Canvas ⇄ Arrange |
| `Ctrl+Z` / `Ctrl+Shift+Z` | undo / redo |
| `Ctrl+S` / `Ctrl+Shift+S` | save / save as |
| `Ctrl+V` | วางภาพจาก clipboard |
| `Delete` | ลบ item ที่เลือก |
| `Ctrl+A` / `Esc` | เลือกทั้งหมด / ยกเลิกเลือก |
| `F` | zoom ให้พอดีกับ selection (ถ้าไม่เลือก = พอดีทั้ง board) |
| `1` / `0` | zoom 100% / zoom fit |
| `G` | grayscale toggle |
| `[` `]` | ส่งไปหลัง / นำมาหน้า |
| `Ctrl+G` / `Ctrl+Shift+G` | group / ungroup |

---

## 6. Theme และความสบายตา

- **ธีมมืดเป็นค่าเริ่มต้น** พื้นหลังกลาง ๆ (`#2A2A2E`) — ไม่ใช่ดำสนิท เพราะดำสนิททำให้ประเมินค่า value ของภาพผิด
- มีปุ่มสลับพื้นหลังเป็นเทากลาง 50% (`#808080`) — มาตรฐานสำหรับเช็คสีงานศิลป์
- ห้ามมี animation ที่วนตลอด (ขัด I-1) transition สั้น ๆ ตอน user action เท่านั้น และต้องปิดได้
