# HANDOFF — สถานะโปรเจกต์ RefX ณ 27 ก.ค. 2026

> เอกสารนี้มีไว้ให้ **session ใหม่ของ coder agent** อ่านเพื่อรับช่วงต่อ
> อ่านไฟล์นี้ให้จบก่อน แล้วค่อยอ่าน `CLAUDE.md` → `ARCHITECTURE.md §1` → `docs/09` → `ROADMAP.md`

---

## 1. สรุปสถานะ

| Phase | สถานะ |
|---|---|
| **P0-1 ถึง P0-9** | ✅ เสร็จครบ ผ่านรีวิวแล้ว |
| **P1-1 ถึง P1-6** | ✅ เสร็จ (P1-6 ครบแล้วหลังทำ `TextureAllocator`) |
| **P1-7** working texture | ❌ **ยังไม่เริ่ม** |
| **P1-8** drag & drop | ⚠️ ลากไฟล์ได้ · **clipboard paste ยังไม่ทำ** |
| P2 ขึ้นไป | ยังไม่เริ่ม |

**เกณฑ์คุณภาพล่าสุดที่ผ่าน:** `fmt` / `clippy --all-features -D warnings` / `nextest 164/164` / `deny check` ครบ 4 หมวด

crate ที่มี: `refx-app` `refx-asset` `refx-core` `refx-io` `refx-platform` `refx-render` `refx-ui`

---

## 2. ★ งานถัดไป (เรียงตามลำดับ ห้ามสลับ)

### 2.1 JPEG scaled decode — สำคัญที่สุด

thumbnail ปลายทางคือ **128 px** แต่ตอนนี้ decode ภาพ 4000×3000 เต็ม 12 ล้าน pixel
แล้วค่อยย่อด้วย Lanczos3 = **ทำงานเกินจำเป็นราว 64 เท่า**

JPEG ย่อได้ตั้งแต่ตอน decode (ข้าม DCT coefficient ความถี่สูง) ที่ 1/8 scale ได้ 500×375
ซึ่งยังใหญ่กว่า 128 px ที่ต้องการ

- `zune-jpeg` **อยู่ใน dependency list มาตั้งแต่วันแรกเพื่อเรื่องนี้ แต่ยังไม่มีโค้ดไหนเรียกใช้**
  (ไม่ต้องขอเพิ่ม dependency)
- ให้ตรวจ API ที่มีจริงก่อน **ห้ามเดาว่าได้ 8 เท่า — ต้องวัด**
- ใช้ได้กับ JPEG เท่านั้น PNG ไม่มี DCT (ไม่เป็นไร ไฟล์ผู้ใช้ 70% เป็น JPEG)
- รายละเอียดเต็มใน `docs/05-memory-and-assets.md §6`

**ถ้าสำเร็จ:** 1000 ภาพ cache เย็นจาก ~80 s → ~15–20 s

### 2.2 atlas lazy allocation

ตอนนี้จอง 12 layer = **192 MB ตั้งแต่เปิดโปรแกรมแม้ยังไม่มีภาพสักใบ**

ผิดหลักโดเมนตรง ๆ: โปรแกรมถูกเปิดค้างทั้งวันข้าง Photoshop การยึด VRAM ไว้เฉย ๆ
คือการแย่ง VRAM จากโปรแกรมหลักของผู้ใช้ บน iGPU ยิ่งหนักเพราะเพดาน 128 MB และเป็น RAM ระบบ

→ จองทีละ layer ตามการใช้จริง เป้าหมาย: เปิดโปรแกรมเปล่าแล้ว VRAM บน status bar ใกล้ 0

### 2.3 P1-7 working texture

ซูมเข้าใกล้ยังเห็น thumbnail 128 px ที่ถูกยืด เบลอชัดเจน
งบ VRAM ครึ่งหนึ่ง (192 MB) จองรอไว้แล้วแต่ยังไม่มีโค้ดใช้ · spec: `docs/04 §4 ชั้น B`

> ข้อ 2.1 กับ 2.2 สำคัญกว่า P1-7 เพราะแก้สิ่งที่ผู้ใช้เจอทุกวัน
> ถ้าเวลาไม่พอ ให้จบสองข้อแรกให้ดีแล้วหยุด **ดีกว่าเริ่ม P1-7 ค้างไว้**

---

## 3. ตัวเลขจริงที่วัดได้ (ใช้เป็นเส้นฐานเทียบ)

### เวลาโหลดภาพ — dataset 4000×3000, JPEG 70% / PNG 30%

| | cache เย็น | cache อุ่น |
|---|---|---|
| 1 ไฟล์ | **148.8 ms** | **4.5 ms** |
| 100 ไฟล์ (616 MB) | **8.09 s** (~81 ms/ไฟล์) | **24.6 ms** |
| คาด 1000 ไฟล์ | ~80 s | ~250 ms |

สร้าง dataset ด้วย `cargo xtask gen-testdata`

### Render — 10,000 quad

| N | p50 | ต้นทุน/quad |
|---|---|---|
| 0 (เส้นฐาน) | 0.297 ms | — |
| 1,000 | 0.303 ms | 6.3 ns |
| 10,000 | 0.360 ms | 6.3 ns |
| 100,000 | 1.088 ms | 8.1 ns |

ที่ 1,000 ภาพ ต้นทุน **98% คือ egui shell + present** ไม่ใช่ quad pipeline

> ⚠️ วัด frame time ต้องใช้ `--bench-seconds` ซึ่งบังคับ `PresentMode::Immediate`
> ถ้าวัดใต้ `AutoVsync` จะได้ตัวเลขของ **รอบ vsync จอ** ไม่ใช่ต้นทุนการวาด (เคยพลาดมาแล้ว)

### อื่น ๆ

- atlas เติมกลับหลังกู้ device: **100/100 ภาพใน 0.99 ms**
- device lost recovery: หลับ 6.7 s → ยิง → กู้ใน 128 ms → กลับไป idle
- binary release: 12 MB (เพดาน 25 MB)

---

## 4. ★ การตัดสินใจที่ผูกมัดแล้ว (ห้ามย้อน ห้ามเดาใหม่)

| # | กฎ | เหตุผล | อยู่ที่ |
|---|---|---|---|
| 1 | **ห้าม memmap ไฟล์ผู้ใช้** ใช้ `metadata()` → `File::take()` | ไฟล์ถูกตัดระหว่าง map = **SIGBUS** ซึ่งเป็น signal ไม่ใช่ panic → `catch_unwind` จับไม่ได้ โปรเซสตายทันที เกิดจริงกับ Dropbox/OneDrive sync | `docs/06 §3` |
| 2 | **ไม่ใช้ BC7** ใช้ `Rgba8UnormSrgb` อย่างเดียว | encode ช้าระดับวินาที ชนกับ "ภาพต้องขึ้นทันที" · ประหยัดแค่ 48 MB จากงบ 384 MB ไม่คุ้ม · encoder ที่มีเป็น C wrapper | `docs/04 §4` |
| 3 | **`max_pixels` ผูกกับ RAM เครื่อง** = `min(total_ram/8/8, 16384²)` | ภาพ 16384² ขอ 2 GiB บนเครื่อง 8 GB ที่เปิด Photoshop = OOM = งานหาย | `docs/05 §2` |
| 4 | **cache key = `(hash, mtime, size)`** ไม่ใช่ hash เดี่ยว | fast hash ของไฟล์ >64 MB อ่านแค่หัว/ท้าย → PSD/TIFF ที่แก้เลเยอร์กลางไฟล์ได้ hash ชนกัน → เห็น thumbnail เวอร์ชันเก่า **แบบเงียบ ๆ** | `docs/05 §3` |
| 5 | **atlas ต้องเติมกลับหลังกู้ device** | ไม่งั้น board ว่างเปล่าหลัง driver อัปเดต ผู้ใช้แยกไม่ออกจาก "งานหาย" ทำให้ P0-5 เสียเปล่า | `docs/04 §4` |
| 6 | **หน้าต่างมี surface ได้ทีละอันเดียว** ต้อง drop ชุดเก่าก่อนสร้างใหม่ | Vulkan ตอบ `Native window is in use` แล้ว panic → `stack` เป็น `Option<_>` + `is_usable()` | `docs/04 §7` |
| 7 | **`DeviceLostReason::Destroyed` ห้ามนับเป็นอุบัติเหตุ** | ยิงตอนเรา drop device เอง ถ้านับจะกู้วนไม่รู้จบ | `docs/04 §7` |
| 8 | **ต้องสร้าง `egui::Context` ใหม่ทั้งก้อนตอนกู้** | font atlas ผูกกับ `Renderer` เดิม ถ้าเก็บ Context ไว้ UI หายหมด | `docs/04 §7` |
| 9 | **panic hook ต้องเงียบตอน decode** (thread-local flag) | ไฟล์เสีย 500 ไฟล์ = backtrace 500 ชุด → log หมุนทะลุ 5 MB → ทับ crash log จริง | `docs/06 §3` |
| 10 | **cache eviction ห้ามแตะโฟลเดอร์ `logs/`** | crash log คือสิ่งเดียวที่ผู้ใช้มีให้ส่งเวลารายงานปัญหา | `docs/08 §5` |
| 11 | **flag จำลอง device lost ต้องมีสองแบบ** นับเฟรม + **นับเวลา (`-ms`)** | ตัวนับเฟรมใช้ไม่ได้ถ้า I-1 ถูกต้อง (แอปวาด ~6 เฟรมแล้วหลับ) · ตัว `-ms` ทดสอบเคสจริงคือ device ตายตอนแอปหลับ | `ROADMAP P0-5` |
| 12 | **เพดาน RAM ต้องคุมรวมทุก worker** ไม่ใช่ต่อ job | 6 worker × 1 GiB = 6 GB แย่ง RAM กับ Photoshop · ปัจจุบัน 256 MB รวม + `Condvar` รอ + ใบจองเป็น RAII | `docs/05 §2` |

---

## 5. เรื่องที่ spec เคยผิดแล้วแก้แล้ว (อย่าเชื่อเอกสารเก่าที่จำมา)

- `docs/05 §6` เคยเขียนว่า 1000 ภาพ cache เย็น ≈ **4 วินาที** → ของจริง **~80 วินาที** (ผิด 20 เท่า เพราะเดาจากภาพเล็ก)
- `docs/03 §1` เคยใช้ `SidePanel` / `TopBottomPanel` / `CentralPanel::show` ซึ่ง egui 0.34 **deprecate หมดแล้ว**
  ของจริง: `egui::Panel::top/left/right/bottom` + `.show_inside(ui)` + `default_size`
  **`egui::Panel::center` ไม่มีจริง** ใช้ `CentralPanel::default().show_inside(ui, …)`
- `docs/09` มีตาราง wgpu 29 API delta: `push_constant_ranges`→`immediate_size`,
  `bind_group_layouts` รับ `&[Option<&_>]`, `multiview`→`multiview_mask`
- `tracing-appender` 0.2 **หมุนไฟล์ตามขนาดไม่ได้** (มีแค่ตามเวลา) ต้องเขียน writer เอง

---

## 6. หนี้ที่ค้างอยู่

| เรื่อง | หมายเหตุ |
|---|---|
| clipboard paste (P1-8) | ยังไม่ทำ |
| `--open-dir` วาง quad เป็นตาราง 16 คอลัมน์ตายตัว | ยังไม่ใช่ layout จริง รอ P2/P3 |
| `xtask gen-testdata` | ทำแล้วบางส่วน · ยังไม่มี `bench` / `package` (P5-1) |
| egui memory ตอนกู้ device | ตอนนี้ scroll position / panel ที่เปิดค้างรีเซ็ต · P5 ให้ clone `ctx.memory()` ก่อนกู้แล้วคืน |
| จุดบอด fast hash ตรงกลางไฟล์ | ปิดด้วย mtime แล้ว แต่จุดบอดตัว hash เองยังอยู่ (มีเทสต์บันทึกไว้ว่าตั้งใจ) |

---

## 7. วิธีทำงานที่คาดหวัง

- ทำทีละ task ให้จบและเทสต์ผ่านก่อนขึ้นตัวถัดไป **ห้ามเขียนหลายโมดูลค้างพร้อมกัน**
- เขียนเทสต์ไปพร้อมโค้ด โดยเฉพาะ `refx-core` ที่ต้องเทสต์ได้โดยไม่มี GPU
- **เทสต์ต้องพิสูจน์ว่ากลไกทำงานจริงตอนนั้น ไม่ใช่แค่ว่ามีโค้ดอยู่**
  (ตัวอย่างที่ดีในโปรเจกต์: `panic_hook_sees_label_while_unwinding`,
  `bulk_cancellation_actually_skips_work`, bomb test ที่บังคับว่าต้องได้ `ImageTooLarge` พร้อม w/h เป๊ะ)
- **เทสต์ล้มเพราะสมมติฐานผิด → แก้สมมติฐาน ไม่ใช่แก้ตัวเลขให้ผ่าน**
- เจอสิ่งที่ spec ไม่ระบุ → เลือกทางที่ปลอดภัยกว่า แล้วบันทึกว่าตัดสินใจอะไรและทำไม
- **เจอ spec ที่ขัดกันเองหรือผิด → หยุด ถาม ห้ามแก้ไฟล์ใน `docs/` เอง** (รายงานให้ผู้ใช้แก้)
- ห้ามเพิ่ม dependency ที่ไม่มีใน `docs/09` · ห้าม refactor นอกขอบเขต task

**ก่อน commit ทุกครั้ง**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo tree -d | grep '^wgpu'     # ต้องว่าง
cargo tree | grep -iE 'reqwest|hyper|tokio|curl|ureq|openssl'   # ต้องว่าง (I-8)
```

**จบทุก session ด้วยการรายงาน:** อะไรเสร็จ/ไม่เสร็จ · ผล 4 คำสั่ง · ตัวเลขที่วัดได้จริง ·
สิ่งที่ spec ไม่ระบุแล้วตัดสินใจเอง · **อะไรที่ spec เขียนไว้แล้วทำจริงไม่ได้** ← ข้อนี้มีค่าที่สุด
