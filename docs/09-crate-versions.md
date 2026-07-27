# 09 — Crate Versions & กับดักความเข้ากันได้

ตรวจสอบจาก docs.rs/crates.io เมื่อ **26 กรกฎาคม 2026**
**ยืนยันด้วย spike จริงบนเครื่องแล้ว (RTX 4060 / Vulkan / Windows) — build ผ่าน หน้าต่างเปิดได้**

---

## ⚠️ กับดักที่สำคัญที่สุด: wgpu 30 ยังใช้กับ egui ไม่ได้

| crate | ที่ใช้ (ยืนยันแล้ว) | หมายเหตุ |
|---|---|---|
| `wgpu` | **29.0.4** | ← ใช้ตัวนี้ (30.0.0 ออกแล้วแต่ egui-wgpu ยัง pin `^29`) |
| `egui` / `egui-wgpu` / `egui-winit` | **0.34.3** | ต้องเป็นเวอร์ชันเดียวกันทั้งสามตัว |
| `winit` | **0.30.13** | |
| **MSRV** | **1.92** | ← egui 0.34.3 บังคับ (ดูหัวข้อถัดไป) |

> เอกสารฉบับก่อนหน้าเขียนว่า egui ล่าสุดคือ 0.35.0 — **ผิด/สับสน**
> `egui-wgpu` ที่ resolve ได้จริงคือ 0.34.3 และ pin `wgpu ^29.0.1` ยึดตารางนี้เท่านั้น

## ⚠️ MSRV = 1.92 ไม่ใช่ 1.87

`egui@0.34.3`, `egui-wgpu@0.34.3`, `egui-winit@0.34.3`, `epaint@0.34.3` ต้องการ **rustc 1.92**
(`vello_common` / `vello_cpu` ต้องการ 1.88)

การ pin ไว้ที่ 1.87 มีผลข้างเคียงที่ยอมรับไม่ได้: มันตรึง `time` ไว้ที่ **0.3.45**
ซึ่งติด **RUSTSEC-2026-0009** (DoS / stack exhaustion) ส่วน 0.3.47 ที่แก้แล้วต้องการ 1.88
→ **MSRV pin กำลังบล็อก security fix อยู่** ขัดกับลำดับความสำคัญข้อ 2 โดยตรง จึงขึ้นเป็น 1.92

**`egui-wgpu` 0.34.3 ประกาศ dependency เป็น `wgpu ^29.0.1`** ถ้าใส่ `wgpu = "30"` ใน Cargo.toml จะได้ wgpu สองเวอร์ชันในโปรเจกต์เดียว แล้วเจอ error ประหลาดแบบ *"expected `wgpu::Device`, found `wgpu::Device`"* ซึ่งเสียเวลาหาสาเหตุมาก

**สิ่งที่ต้องทำก่อนเขียนโค้ดบรรทัดแรก:**

```bash
cargo add wgpu egui egui-wgpu egui-winit winit
cargo tree -d | grep wgpu     # ต้องไม่มีเวอร์ชันซ้ำ ถ้ามี = แก้ก่อน
```

ถ้าตอนที่เริ่มทำจริง `egui-wgpu` ออกเวอร์ชันที่รองรับ wgpu 30 แล้ว ให้อัปทั้งชุดพร้อมกัน แล้วอัปเดตไฟล์นี้

---

## Cargo.toml (workspace dependencies)

```toml
[workspace.dependencies]
# --- GPU / Window / UI (ล็อกทั้งชุด อัปพร้อมกันเท่านั้น) ---
wgpu        = "29.0.4"
winit       = "0.30"
egui        = "0.34.3"
egui-wgpu   = "0.34.3"
egui-winit  = "0.34.3"

# --- Image decode: pure Rust เท่านั้น (ดู 06-security.md) ---
image       = { version = "0.25", default-features = false,
                features = ["png", "jpeg", "webp", "gif", "bmp", "tga", "tiff"] }
zune-jpeg   = "0.4"
fast_image_resize = "5"      # SIMD resize — เร็วกว่า image::imageops หลายเท่า
kamadak-exif = "0.6"         # อ่าน orientation

# --- Data ---
blake3      = "1"
rusqlite    = { version = "0.32", features = ["bundled"] }
postcard    = { version = "1", features = ["use-std"] }
zstd        = "0.13"
serde       = { version = "1", features = ["derive"] }
crc32fast   = "1"

# --- Concurrency (ไม่มี tokio — ADR-004) ---
crossbeam-channel = "0.5"
parking_lot = "0.12"

# --- Utility ---
glam        = { version = "0.29", features = ["bytemuck"] }
bytemuck    = { version = "1", features = ["derive"] }
smallvec    = { version = "1", features = ["union"] }
indexmap    = "2"
slotmap     = "1"
memmap2     = "0.9"
thiserror   = "2"
anyhow      = "1"
tracing     = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
rfd         = "0.15"         # native file dialog
arboard     = "3"            # clipboard รวมรูปภาพ
notify      = "7"            # file watching
directories = "5"            # หา cache/config dir ตามมาตรฐานแต่ละ OS

[workspace.package]
edition      = "2024"
rust-version = "1.92"        # MSRV ของ egui 0.34.3 (สูงกว่าของ wgpu 29 ที่ต้องการแค่ 1.87)
```

---

## กับดักอื่นที่ต้องรู้ล่วงหน้า

### winit 0.30 เปลี่ยนไปใช้ `ApplicationHandler`

winit 0.30 เลิกใช้ closure ใน `run()` แล้ว ต้อง implement trait แทน — ตัวอย่างเก่าบนอินเทอร์เน็ตส่วนใหญ่ยังเป็นแบบ 0.28/0.29 ใช้ไม่ได้

```rust
impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) { /* สร้าง window + surface ที่นี่ */ }
    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, ev: WindowEvent) { }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) { }
}
```

**สร้าง window ใน `resumed()` เท่านั้น** ไม่ใช่ก่อนเรียก `run()`

### wgpu 29: API ที่ต่างจากที่เอกสารรุ่นแรกเขียนไว้ (ยืนยันจาก spike)

| เรื่อง | ความจริงใน wgpu 29.0.4 |
|---|---|
| `Instance::new` | รับ `InstanceDescriptor` **by value** ไม่ใช่ `&InstanceDescriptor` และ **ไม่มี** `Default` — ใช้ `Instance::new_with_display_handle()` หรือ `new_without_display_handle()` |
| `get_current_texture()` | คืน enum `CurrentSurfaceTexture` **ไม่ใช่ `Result`** — `SurfaceError` ไม่มีแล้ว (ดู docs/04 §7) |
| `RenderPassDescriptor` | มีฟิลด์ใหม่ `multiview_mask` |
| `request_adapter` / `request_device` | คืน **future** — แต่ ADR-004 ห้าม async runtime และ `pollster` ไม่อยู่ในรายการ dependency |

**เรื่อง future ตอน init:** เขียน `block_on` เองสั้น ๆ (~10 บรรทัด ด้วย `Waker::noop()`) ใน `refx-render::device`
ใช้ได้**เฉพาะตอน init เท่านั้น** ห้ามหลุดไปอยู่บนเส้นทางต่อเฟรม — ใส่ `debug_assert!(!STARTED.load(..))` กันไว้

### wgpu 29 เปลี่ยนชื่อฟิลด์จากที่ spec เขียนไว้ (เจอตอน P0-6)

| spec เขียนว่า | ของจริงใน wgpu 29 |
|---|---|
| `PipelineLayoutDescriptor.push_constant_ranges` | `immediate_size` |
| `bind_group_layouts: &[&BindGroupLayout]` | `&[Option<&BindGroupLayout>]` |
| `RenderPipelineDescriptor.multiview` | `multiview_mask` |

### egui 0.34 เปลี่ยนชื่อ API ที่ docs/03 ใช้อยู่

| เดิม (deprecated) | ใหม่ |
|---|---|
| `Context::run` | `Context::run_ui` |
| `CentralPanel::default().show(ctx, ..)` | `CentralPanel::default().show_inside(..)` / ตาม signature ใหม่ |
| `SidePanel::left` / `TopBottomPanel::top` | `Panel::left` / `Panel::top` (และ `right` / `bottom`) |

ตัวอย่างโค้ดใน [`03-modes-and-ui.md`](03-modes-and-ui.md) §1 เขียนด้วยชื่อเก่า — **ให้ยึดชื่อใหม่ตอน implement**
(ถ้าใช้ชื่อเก่า `clippy -D warnings` จะตกเพราะ deprecation)

### สรุปการตั้งค่า adapter ที่ใช้จริง

```rust
PowerPreference::LowPower      // RefX เปิดค้างทั้งวันข้าง Photoshop — ไม่ควรปลุก dGPU
```
แต่ **VRAM budget ต้องคำนวณจาก `adapter.limits()` จริง ไม่ใช่ค่าคงที่** เพราะ LowPower อาจได้ iGPU
ที่แชร์ RAM ระบบ — ในกรณีนั้น budget ต้องเล็กลง (ดู [`05-memory-and-assets.md`](05-memory-and-assets.md) §2)
ต้องมี setting ให้ผู้ใช้บังคับ `HighPerformance` ได้

### BC7 ไม่ได้มีทุกเครื่อง

```rust
let bc7 = adapter.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
```

ต้องมี fallback ไป `Rgba8UnormSrgb` เสมอ และปรับ budget ตาม (16 MB → 64 MB) — เขียน fallback ตั้งแต่แรก อย่าเขียนทีหลัง

### `rfd` เปิด dialog แบบ blocking ห้ามเรียกบน UI thread

ใช้ `AsyncFileDialog` หรือ spawn thread แล้วส่งผลกลับทาง channel (I-2)

### `panic = "abort"` ทำให้ catch_unwind พัง

ถ้าใครใส่ `panic = "abort"` ใน release profile เพื่อลดขนาด binary → เกราะป้องกัน decoder (I-7) หายทั้งหมด **ห้ามเปลี่ยน**

---

## นโยบายการอัปเกรด

1. ตรึงเวอร์ชันชัดเจน commit `Cargo.lock`
2. `cargo update` เป็น PR แยกเสมอ ไม่ปนกับงานฟีเจอร์
3. อัป `wgpu`/`egui`/`winit` **พร้อมกันทั้งชุด** เท่านั้น
4. หลังอัปทุกครั้ง: `cargo tree -d`, รัน benchmark, ทดสอบ device-lost recovery ด้วยมือ
5. `cargo deny check` ต้องผ่านก่อน merge เสมอ
