# 04 — Rendering

crate: `refx-render` — depend: `wgpu`, `refx-core` (อ่านอย่างเดียว) | ห้าม: `egui`, `winit`

---

## 1. หลักการเดียวที่สำคัญที่สุด: Event-driven redraw

นี่คือสิ่งที่ทำให้ CPU idle = 0% ตามข้อกำหนด **ทำผิดข้อนี้ข้อเดียว แล้วเป้าหมาย "กิน CPU น้อย" พังทั้งหมด**

```rust
event_loop.set_control_flow(ControlFlow::Wait);   // ← ไม่ใช่ Poll
```

`request_redraw()` เรียกเมื่อ **เท่านั้น**:
1. มี input จากผู้ใช้ (เมาส์/คีย์/resize/drop)
2. worker ส่ง texture ที่โหลดเสร็จกลับมา
3. มี animation กำลังเล่น (มี frame budget และจบใน ≤ 200 ms)
4. `egui::Context::request_repaint_after()` ขอมา

**ตัวตรวจ:** เปิดโปรแกรม เปิด board 1000 ภาพ ปล่อยไว้ 60 วินาทีโดยไม่แตะ → Task Manager ต้องแสดง 0.0% ถ้าเห็น 1–3% แปลว่ามีที่ไหนสัก loop เรียก `request_redraw` ทุกเฟรม ให้ตามหาจนเจอ อย่าปล่อยผ่าน

### ★ กับดักที่เจอจริงตอน spike (26 ก.ค. 2026) — อ่านก่อนเขียน event loop

`egui_winit::State::on_window_event()` คืน `EventResponse { repaint: true, .. }` **สำหรับ `WindowEvent::RedrawRequested` ตัวมันเอง**
ถ้าเอาค่านั้นไปป้อน `window.request_redraw()` ตรง ๆ จะได้ลูปเลี้ยงตัวเองทันที

วัดได้จริงในการทดลอง: **3068 เฟรมใน 20 วินาที (~160 fps) ทั้งที่ไม่มี input เลย**
และ `egui` เองบอกว่า `repaint_delay = Duration::MAX` (แปลว่า "ไม่ต้องวาดอีก") — ผู้ร้ายคือ event loop ไม่ใช่ egui

```rust
// ❌ ผิด — ลูปไม่จบ
let resp = egui_state.on_window_event(&window, &event);
if resp.repaint { window.request_redraw(); }

// ✅ ถูก — แยก RedrawRequested ออกมาก่อนถึงจะส่งเข้า egui
match event {
    WindowEvent::RedrawRequested => { self.render(); return; }   // ★ return ก่อน
    _ => {}
}
let resp = egui_state.on_window_event(&window, &event);
if resp.repaint { window.request_redraw(); }
```

หลังแก้: **42 เฟรมใน 20 วินาที** ซึ่งทั้งหมดกระจุกอยู่ใน ~1.2 วินาทีแรก (event ตอนสร้างหน้าต่างของ Windows) แล้ว **0 เฟรมตลอด 19 วินาทีที่เหลือ** — นี่คือรูปแบบที่ถูกต้อง

ดังนั้นเทสต์ `idle_produces_no_redraw` ต้อง**ข้ามช่วง settle 2 วินาทีแรก** แล้วค่อยเริ่มนับ (ชื่อเมธอด `redraw_count_since_settled()` ใน docs/08 ตั้งใจไว้แบบนี้)

---

## 2. Render graph (เรียบที่สุดเท่าที่จะทำได้)

```
1 render pass, 2 draw call:
  [1] instanced quads  — ภาพทั้งหมดที่มองเห็น (draw เดียวต่อ atlas/texture group)
  [2] egui             — UI chrome ทั้งหมด
```

ไม่มี post-processing, ไม่มี MSAA (ภาพเป็นสี่เหลี่ยม ไม่มีขอบแหลม), ไม่มี depth buffer (เรียงด้วย painter's algorithm ตาม `z_order` ซึ่ง sorted อยู่แล้ว)
ความเรียบ = เสถียร + ประหยัด

---

## 3. Instanced quad

Vertex buffer มีสี่เหลี่ยมหน่วยเดียว (4 vertex) ทุกภาพคือ 1 instance

```rust
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct QuadInstance {
    pub transform: [f32; 6],   // affine 2x3: [a, b, c, d, tx, ty] — รวม pos/scale/rot/flip
    pub uv_rect:   [f32; 4],   // ตำแหน่งใน atlas หรือ crop rect
    pub tint:      [f32; 4],   // rgb multiply + alpha (opacity)
    pub layer:     u32,        // ชั้นใน texture array
    pub flags:     u32,        // bit0 grayscale, bit1 invert, bit2 selected, bit3 placeholder
}
// = 64 bytes ต่อ instance → 1000 ภาพ = 64 KB ต่อเฟรม อัปโหลดทั้งก้อนได้สบาย
```

`transform` เป็น affine 2×3 ไม่ใช่ mat4 — 2D ไม่ต้องใช้ 64 ไบต์ต่อ matrix
`flags` เป็น bitfield ทำให้ grayscale/invert/highlight เกิดใน shader โดยไม่ต้องแตะ texture เลย = ราคาเกือบศูนย์

### Instance buffer

- จอง buffer ขนาดคงที่ (เช่น 8192 instance = 512 KB) ตอนเริ่มโปรแกรม แล้ว `queue.write_buffer` เขียนทับทุกเฟรม
- **ห้ามสร้าง buffer ใหม่ทุกเฟรม** — เป็นสาเหตุอันดับหนึ่งของ VRAM ที่โตเรื่อย ๆ กับ hitching
- เกิน 8192 → แบ่งเป็นหลาย draw call (แทบไม่มีทางเกิดหลัง culling)

---

## 4. Texture strategy

wgpu รับประกัน binding array/bindless แค่บาง backend — **ห้ามพึ่งพา** ใช้สองชั้นนี้แทน:

### ชั้น A — Thumbnail atlas (ตัวหลัก, ครอบคลุม 95% ของสิ่งที่วาด)

```
Texture2DArray  2048 × 2048 × N layers
  format: Rgba8UnormSrgb          ← ★ ตัดสิน 27 ก.ค. 2026: ตัด BC7 ออกจาก P1
  ช่องละ 128×128 → 256 ช่องต่อ layer
```

1000 ภาพ = 4 layers → 2048×2048×4 × 4 = **64 MB** (16.7% ของงบ VRAM 384 MB)

### ★ ทำไมถึงตัด BC7 ทิ้ง (ไม่ใช่แค่เพราะหา crate ไม่ได้)

เหตุผลที่ชัดกว่าเรื่อง dependency คือ **เวลาที่ใช้เข้ารหัส**
BC7 encode คุณภาพดีใช้เวลาระดับ *ร้อยมิลลิวินาทีถึงวินาที* ต่อ tile 2048²
ซึ่งชนกับสัญญาข้อแรกสุดของโปรแกรมโดยตรง — **ภาพต้องขึ้นทันทีที่ลากเข้ามา**
ถ้าจะทำให้ไม่ชน ต้องอัปแบบ RGBA8 ก่อนแล้วค่อย re-encode เป็น BC7 ทีหลังเบื้องหลัง
= atlas สองชุด + สถานะกำลังแปลง + ต้องสลับ bind group กลางคัน
ความซับซ้อนระดับนั้นเพื่อประหยัด VRAM 48 MB จากงบ 384 MB **ไม่คุ้ม**

และ encoder BC7 ที่ใช้ได้จริงล้วนเป็น C wrapper (`intel_tex_2`) ซึ่ง CLAUDE.md ห้ามไว้อยู่แล้ว

→ **P1 ใช้ `Rgba8UnormSrgb` อย่างเดียว ไม่ต้องมีโค้ดสาขา BC7**
→ ถ้า VRAM กลายเป็นคอขวดจริงตอนวัดใน P5 ค่อยกลับมาคุยเรื่องนี้ใหม่พร้อมตัวเลขจริง

ทั้งหมดนี้คือ **1 bind group 1 draw call** สำหรับภาพทุกภาพบน board — นี่คือเหตุผลที่ 1000+ ภาพยังลื่นได้

Atlas จัดสรรด้วย free-list ธรรมดา (ช่องขนาดคงที่ = ไม่มี fragmentation ไม่ต้องใช้ bin-packing)

### ★ หลังกู้ device: atlas ต้องเติมกลับเอง

resource ทั้งหมดหายไปพร้อม device เก่า ถ้าไม่เติมกลับ ผู้ใช้จะเห็น **board ว่างเปล่า**
หลัง driver อัปเดต ทั้งที่ P0-5 "กู้สำเร็จ" แล้ว — ซึ่งจากมุมผู้ใช้แยกไม่ออกจากงานหาย
และทำให้งานกู้ device ทั้งหมดเสียเปล่า

หลังสร้าง device ใหม่เสร็จ ต้อง re-upload thumbnail ของทุก item ที่อยู่บน board
จาก `cache.sqlite` (ไม่ใช่ decode ใหม่จากไฟล์ต้นฉบับ — ช้ากว่ามากและไฟล์อาจหายไปแล้ว)
ระหว่างเติมให้แสดง placeholder ไม่ใช่ช่องว่าง

### ชั้น B — Working texture (เฉพาะภาพที่ซูมเข้าจนเห็นชัด)

เมื่อขนาดบนจอของภาพ > 128 px → ขอ texture แยกที่ขนาด **power-of-two ที่พอดีขนาดบนจอ** (256/512/1024/2048/4096)
- แยก draw call ต่อ texture (เรียงตาม z แล้ว batch ตัวที่ติดกันและใช้ texture เดียวกัน)
- ปกติมีในจอพร้อมกัน < 30 ตัว → < 30 draw call ยอมรับได้สบาย
- อยู่ภายใต้ VRAM budget + LRU (ดู [05](05-memory-and-assets.md))

### ชั้น C — Full resolution

เฉพาะตอน zoom > 100% และสูงสุด **2 ภาพ** พร้อมกัน ทิ้งทันทีที่ซูมออก

### Mipmap

Working texture ต้องมี mip chain (สร้างด้วย compute shader ตอนอัปโหลด) ไม่งั้นภาพจะ aliasing รุนแรงตอนซูมออก
Thumbnail atlas **ไม่ต้องมี mip** (128px เล็กพอแล้ว และ mip ใน array texture ทำให้ atlas ซับซ้อนขึ้นโดยไม่คุ้ม)

---

## 5. Culling

โครงสร้าง: **loose uniform grid** ไม่ใช่ quadtree

```rust
pub struct SpatialIndex {
    cell_size: f32,                        // ~2× ขนาดภาพเฉลี่ย
    cells: HashMap<IVec2, SmallVec<[ItemId; 8]>>,
}
```

เหตุผลที่เลือก grid: insert/remove เป็น O(1) (ผู้ใช้ลากภาพตลอดเวลา = update บ่อยมาก) ส่วน quadtree ต้อง rebalance และซับซ้อนกว่าโดยไม่ได้เร็วกว่าในกรณีใช้งานจริง

- rebuild เฉพาะ cell ที่ได้รับผลกระทบตอน item ขยับ
- query = สแกน cell ที่ทับกับ viewport + margin 1 หน้าจอ (prefetch)
- ที่ 1000 ภาพ ยังไม่ต้องใช้ก็ได้ (สแกน linear 1000 ตัว ~10 µs) แต่ต้องมีตั้งแต่แรกเพราะ **hit-test ก็ใช้โครงสร้างเดียวกัน** และเป้าหมายคือรองรับ "หลาย board พร้อมกัน"

---

## 6. Color management

- Surface format: `Bgra8UnormSrgb` (เลือก sRGB จาก `surface.get_capabilities()` เสมอ)
- **egui จะเตือนว่าอยากได้ framebuffer แบบ linear** — ยืนยันว่า**ใช้ sRGB ต่อไป**
  โปรแกรมนี้มีหน้าที่แสดงสีภาพให้ถูก ความถูกต้องของสีภาพสำคัญกว่า gamma ของ UI chrome
  วิธีแก้ที่ถูก: ส่ง format จริงเข้า `egui_wgpu::Renderer::new(.., output_color_format, ..)`
  ให้ egui รู้ตัวและไม่แก้ gamma ซ้ำซ้อน **ห้ามเปลี่ยน surface เป็น linear** เพราะจะต้องแปลง sRGB
  เองใน quad shader ทุก pixel ทุกเฟรม = ช้ากว่าและพลาดง่ายกว่า
- ภาพที่มี ICC profile ที่ไม่ใช่ sRGB → **แปลงเป็น sRGB ตอน decode** ไม่ใช่ตอน render (ทำครั้งเดียว ไม่ใช่ทุกเฟรม)
- ไม่รองรับ HDR/wide gamut ใน v1 — แต่เก็บ `color_space` ไว้ใน `AssetRef` เผื่ออนาคต
- ผสมสีใน linear space: ให้ GPU จัดการผ่าน sRGB texture format (ฟรี) ไม่ต้องแปลงเองใน shader

---

## 7. Device lost / Surface error — ★ ต้องทำตั้งแต่ P0

นี่คือสาเหตุ crash อันดับหนึ่งของแอปกราฟิกบน Windows (driver update, sleep/resume, สลับ iGPU↔dGPU, TDR timeout)

> **แก้ 26 ก.ค. 2026 หลัง spike:** `wgpu::SurfaceError` **ไม่มีแล้วใน wgpu 29**
> `get_current_texture()` คืน enum `CurrentSurfaceTexture` (ไม่ใช่ `Result`)
> variant: `Success` / `Suboptimal` / `Timeout` / `Occluded` / `Outdated` / `Lost` / `Validation`
> ยืนยัน signature จาก docs.rs ของ wgpu 29.0.4 อีกครั้งก่อนเขียน

```rust
use wgpu::CurrentSurfaceTexture as CST;

match surface.get_current_texture() {
    // ปกติ
    CST::Success(frame) => render(frame),

    // ยังวาดได้ แต่ config ไม่ตรงจอแล้ว — วาดเฟรมนี้ให้จบก่อน แล้วค่อย reconfigure
    // (ถ้า skip เลย ผู้ใช้จะเห็นจอค้างตอนลากขอบหน้าต่าง)
    CST::Suboptimal(frame) => { render(frame); reconfigure = true; }

    // ★ หน้าต่างถูกบัง / minimize — ข้ามเฟรม และ "ห้าม" request_redraw
    // นี่คือของขวัญฟรีสำหรับ I-1: minimize แล้ว = 0% CPU จริง ๆ
    CST::Occluded => { /* ไม่ทำอะไรเลย */ }

    // surface ตายแต่ device ยังอยู่ — configure ใหม่พอ
    CST::Lost | CST::Outdated => { surface.configure(&device, &config); window.request_redraw(); }

    // driver ยังไม่ว่าง — ข้ามเฟรมนี้เฉย ๆ
    CST::Timeout => {}

    // ผิดพลาดระดับ validation — log แล้วข้าม
    // ถ้าเกิดติดกัน 10 เฟรม ให้ถือว่า device เสีย แล้วไปเส้นทางกู้ device ข้างล่าง
    CST::Validation(e) => { tracing::error!(?e); consecutive_errors += 1; }
}
```

**OutOfMemory ไม่ได้มาทาง `get_current_texture()` อีกแล้ว** ต้องดักสองทางนี้แทน:

```rust
device.on_uncaptured_error(Box::new(|e| {
    if matches!(e, wgpu::Error::OutOfMemory { .. }) {
        OOM_FLAG.store(true, Ordering::Relaxed);   // ห้ามทำงานหนักใน callback นี้
    }
}));
// เฟรมถัดไปเห็น flag → cache.emergency_evict() (ทิ้ง T2 แล้ว T1)
// ถ้ายัง OOM อีก → autosave ทันที + แจ้งผู้ใช้ว่าต้องลด memory budget
```

### ข้อบังคับที่ค้นพบตอน implement P0-5 (26 ก.ค. 2026) — ห้ามทำผิดทางนี้

**1. หน้าต่างหนึ่งบานมี surface ได้ทีละอันเดียว**
เอกสารฉบับแรกเขียนว่า "สร้างชุดใหม่ก่อน แล้วค่อยทิ้งของเก่า" (เพื่อให้กู้ไม่สำเร็จก็ยังมีของเดิม)
**ทำไม่ได้จริง** — Vulkan ตอบ `Native window is in use` แล้ว panic
→ ต้อง **drop ชุดเก่าให้หมดก่อนเสมอ** แล้วค่อยสร้างใหม่
→ ผลตามมา: `RenderContext.stack` ต้องเป็น `Option<_>` และต้องมี `is_usable()` ให้ชั้นบนเช็ค
   ทุกจุดที่แตะ device ต้องรับมือกับสถานะ "ไม่มี device ชั่วคราว" ได้
(ในทางปฏิบัติไม่เสียอะไร เพราะตอนนั้น device ตายไปแล้ว)

**2. `DeviceLostReason::Destroyed` ห้ามนับเป็นอุบัติเหตุ**
callback นี้ยิงตอน **เราเอง** drop device ระหว่างกู้ ถ้านับด้วยจะกู้วนไม่รู้จบ

**3. ต้องสร้าง `egui::Context` ใหม่ทั้งก้อน ไม่ใช่แค่ `Renderer`**
font atlas ผูกอยู่กับ `Renderer` ตัวเดิม (`forget_all_images()` ล้างแค่ image cache ไม่ใช่ atlas)
ถ้าเก็บ Context เดิมไว้แต่สร้าง Renderer ใหม่ → UI หายทั้งหมด
**ผลข้างเคียงที่ยอมรับใน P0:** สถานะ UI ชั่วคราวรีเซ็ต (scroll position, panel ที่เปิดค้าง)
*หนี้ที่ต้องจ่ายใน P5:* clone `ctx.memory()` ไว้ก่อนกู้ แล้วคืนกลับหลังกู้เสร็จ

**Device lost จริง ๆ** (ไม่ใช่แค่ surface) ต้องกู้ระดับ:
1. ตั้ง `device.set_device_lost_callback` ให้ตั้ง flag
2. เฟรมถัดไปเห็น flag → สร้าง `Instance`/`Adapter`/`Device`/`Queue` ใหม่ทั้งชุด
3. สร้าง pipeline + atlas ใหม่ แล้ว re-upload thumbnail จาก **cache.sqlite** (ไม่ต้อง decode ใหม่ → กู้ได้ในไม่ถึงวินาที)
4. **document state ไม่แตะเลย** — ผู้ใช้เห็นแค่จอกระพริบครั้งเดียว งานไม่หาย

ต้องมีเทสต์: ใส่ feature flag `--force-device-lost-after=N` เพื่อจำลองสถานการณ์นี้ใน CI

---

## 8. Frame flow

```
RedrawRequested
  → คำนวณ visible set (culling)                    ~50 µs
  → ขอ texture ที่ยังไม่มี → ส่ง job เข้า decode pool  (ไม่บล็อก)
  → build QuadInstance สำหรับ visible set          ~100 µs @ 1000 items
  → queue.write_buffer(instance_buffer)
  → egui: run() → tessellate → update textures
  → encoder: 1 pass { draw quads; draw egui }
  → queue.submit + frame.present
```

ภาพที่ยังไม่มี texture วาดเป็น placeholder (สี่เหลี่ยมเทา + สีเด่นของภาพถ้ารู้แล้วจาก cache) **ห้ามรอ ห้ามข้าม** — ผู้ใช้ต้องเห็น layout ทันทีเสมอ แล้วภาพค่อยชัดขึ้นมา

### Present mode

`PresentMode::AutoVsync` เป็นค่าเริ่มต้น — จำกัด frame rate ตามจอ = ประหยัดไฟ/CPU
ห้ามใช้ `Immediate` (เผา GPU ฟรี) เปิดเป็นตัวเลือกใน settings ได้แต่ไม่ใช่ default

---

## 9. Shader

WGSL ไฟล์เดียว `quad.wgsl` ฝังด้วย `include_str!`

```wgsl
// vertex: unit quad → apply affine transform → clip space
// fragment: sample atlas/texture → apply tint → apply flags (grayscale/invert) → selection outline
```

- Grayscale ใช้ luminance coefficient ของ Rec. 709: `dot(rgb, vec3(0.2126, 0.7152, 0.0722))` (ไม่ใช่ค่าเฉลี่ยธรรมดา — ผลลัพธ์ต่างกันชัดเจนสำหรับงานเช็ค value)
- Selection outline วาดใน fragment shader จาก `flags` (ไม่ต้อง draw call เพิ่ม)
- **ห้ามมี branch ที่แตกต่างกันต่อ instance มากเกินไป** — ใช้ `select()` แทน `if` ที่ทำได้
