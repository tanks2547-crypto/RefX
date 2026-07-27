# 03 — สอง Mode และ UI กลาง

crate: `refx-ui`

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
