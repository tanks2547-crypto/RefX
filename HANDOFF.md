# HANDOFF — สถานะโปรเจกต์ RefX ณ 27 ก.ค. 2026

> เอกสารนี้มีไว้ให้ **session ใหม่ของ coder agent** อ่านเพื่อรับช่วงต่อ
> อ่านไฟล์นี้ให้จบก่อน แล้วค่อยอ่าน `CLAUDE.md` → `ARCHITECTURE.md §1` → `docs/09` → `ROADMAP.md`

---

## 1. สรุปสถานะ

| Phase | สถานะ |
|---|---|
| **P0-1 ถึง P0-9** | ✅ เสร็จครบ ผ่านรีวิวแล้ว |
| **P1-1 ถึง P1-6** | ✅ เสร็จ (P1-6 ครบแล้วหลังทำ `TextureAllocator`) |
| **P1-7** working texture | ✅ เสร็จ (`9e1033e`) — mip บน CPU · LRU · 17 draw call ที่ 100 ภาพ |
| **P1-8** drag & drop | ⚠️ ลากไฟล์ได้ · **clipboard paste ยังไม่ทำ** ← เหลือข้อเดียวของ P1 |
| P2 ขึ้นไป | ยังไม่เริ่ม |

**เกณฑ์คุณภาพล่าสุดที่ผ่าน:** `fmt` / `clippy --all-features -D warnings` (workspace + `fuzz/`) /
`nextest 236/236` / `deny check` ครบ 4 หมวด / **binary 15.42 MB จากเพดาน 25 MB (บังคับใน CI)**

crate ที่มี: `refx-app` `refx-asset` `refx-core` `refx-io` `refx-platform` `refx-render` `refx-ui`

### สถานะ git

repo **อยู่ใต้ git แล้ว** (branch `master`) — `git fsck` สะอาด working tree สะอาด

- **identity ตั้งไว้แบบ repo-local** คือ `RefX <00ifrit00@gmail.com>`
  (global ยังว่าง จงใจ — repo อื่นในเครื่องไม่ถูกกระทบ)
  ถ้า `git commit` ฟ้อง `unable to auto-detect email address` แปลว่าอยู่คนละ repo หรือ config หาย
- **`.gitattributes` ตรึง line ending เป็น LF ทั้งโปรเจกต์** และ renormalize ไปแล้ว
  ตรวจได้ด้วย `git ls-files --eol | grep -c 'w/crlf'` → ต้องเป็น **0** เสมอ
  > ★ ระวัง: `core.autocrlf = true` ถูกตั้งไว้ที่ **system level** ของ Git for Windows
  > `eol=lf` ใน `.gitattributes` ชนะอยู่ จึงไม่ต้องแก้ system config
  > แต่ถ้าแก้ไฟล์ด้วยเครื่องมือที่แปลง `\n` → `\r\n` เอง (เช่น Python `open(f,'w')` บน Windows)
  > ไฟล์บนดิสก์จะกลายเป็น CRLF อีก — index ยังเป็น LF เพราะ git normalize ให้ตอน add
  > แต่ควรเช็คตัวเลขข้างบนก่อน commit
- **`.claude/settings.local.json` ไม่ถูก track แล้ว** (อยู่ใน `.gitignore`)
  เป็นตั้งค่าต่อเครื่อง ไม่ใช่ของโปรเจกต์ — ไฟล์ยังอยู่บนดิสก์ตามปกติ

---

## 2. ★ งานถัดไป (เรียงตามลำดับ ห้ามสลับ)

> **P1 เหลือข้อเดียว** — งานที่เสร็จแล้วถูกย้ายออกจากหัวข้อนี้ ดูประวัติใน git log
> ต้นเหตุและทางแก้ของบั๊กสำคัญบันทึกไว้ใน `docs/` แล้ว ไม่ใช่ในไฟล์นี้

### 2.1 P1-8 clipboard paste — ปิด P1

`Ctrl+V` จากเบราว์เซอร์ / โปรแกรมวาด / Explorer · `arboard` อยู่ใน `docs/09` แล้ว

**ระวัง:** clipboard เป็น input ที่ไม่น่าไว้ใจเท่าไฟล์ (I-4) ต้องผ่าน `decode_guarded` เส้นทางเดียวกัน
ห้ามมีทางลัด · clipboard ให้ byte มาตรง ๆ ไม่มี path จึงไม่มี mtime สำหรับ cache key
→ ตัดสินว่าจะ cache ไหม และถ้า cache จะใช้อะไรเป็น key (ดูข้อผูกมัด §4 ข้อ 4)

### 2.2 เทสต์อัตโนมัติสำหรับ `force-device-lost`

เส้นทางกู้ device ยืนยันด้วยมือล้วนมาตลอด ทั้งที่เป็นสาเหตุ crash อันดับหนึ่งของแอปกราฟิก
บน Windows (ARCHITECTURE §7) และตอนนี้**ยังไม่มีเทสต์ตัวไหนอยู่ใต้ `#[cfg(feature)]` เลย**

P2 จะเพิ่ม GPU resource อีกมาก ถ้าไม่มีเทสต์คุม การกู้ device จะพังเงียบ ๆ ตอนไหนก็ได้
ต้องคุมอย่างน้อย: กู้แล้ว atlas เติมกลับครบ · working texture ถูกสร้างใหม่ · ไม่กู้วนซ้ำ

### 2.3 กวาด `tracing::` เป็นภาษาอังกฤษ (~100 จุด)

log มีไว้ให้นักพัฒนาอ่าน · ผู้ใช้ต่างชาติส่ง log มาต้องอ่านออก · เป็นงานเชิงกล
ตรรกะเดียวกับที่ `#[error(…)]` ถูกเปลี่ยนเป็นอังกฤษไปแล้ว

### 2.4 → เข้า P2 Canvas mode

อ่าน `ROADMAP.md` หัวข้อ P2 และ `docs/03 §1` (โดยเฉพาะกับดัก pointer ของ egui
และทางแก้ระยะยาว: ทำ canvas เป็น widget จริงด้วย `allocate_response`)

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
- **binary release: 15.35 MB** (เพดาน 25 MB → headroom เหลือ **9.65 MB**)
  > ตัวเลข "12 MB" ที่เคยเขียนไว้ **ไม่เคยเป็นความจริง** — build commit `b100661` ใหม่
  > ด้วย profile และ toolchain เดียวกันได้ 15.26 MB
  > ตั้งแต่ P1-6 ถึงตอนนี้ binary โตแค่ **92 KB** (33 KB โค้ด + 59 KB ฟอนต์ไทย)

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
| 13 | **เรียก `fast_image_resize` ได้จาก `refx-asset::resize` ที่เดียว** (`#[inline(never)]`) | เรียกจากสองจุดทำให้ binary โต **2.6 MB** จาก monomorphization · ต้นทุนอยู่ที่*จำนวนจุดที่เรียก* ไม่ใช่ตัวเลือกที่ส่งเข้าไป · เจอตอน P1-7 เพราะเพดานใน CI จับได้ | `docs/08 §6` |
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
| **ไม่มีเทสต์อัตโนมัติสำหรับ `force-device-lost` เลย** | เส้นทางกู้ device ยืนยันด้วยมือล้วน ๆ ผ่าน `--force-device-lost-after-ms` ทั้งที่เป็นสาเหตุ crash อันดับหนึ่งของแอปกราฟิกบน Windows (ARCHITECTURE §7) · ตอนนี้ยังไม่มีเทสต์ตัวไหนอยู่ใต้ `#[cfg(feature)]` เลย |
| `tracing::` ยังเป็นภาษาไทย ~100 จุด | log มีไว้ให้นักพัฒนาอ่าน · ผู้ใช้ต่างชาติส่ง log มาต้องอ่านออก · งานเชิงกลข้ามทุก crate |

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

### ★ commit ทุกครั้งที่จบ task — ไม่ใช่รอจบ session

**จบ task ไหนแล้วเทสต์ผ่าน ให้ commit ทันที** อย่าสะสมงานหลาย task ไว้ commit ทีเดียวตอนจบ

เหตุผล:

- commit ก้อนใหญ่ที่มีหลาย task ปนกัน **รีวิวไม่ได้จริง** — แยกไม่ออกว่าการเปลี่ยนแปลงไหน
  เป็นของงานไหน และถ้าต้องย้อนก็ย้อนเฉพาะส่วนที่ผิดไม่ได้
- session อาจจบกลางคัน (context เต็ม, เครื่องดับ) งานที่ยังไม่ commit **หายทั้งหมด**
  ซึ่งเป็นความเสี่ยงแบบเดียวกับที่ I-3 ห้ามไว้กับงานของผู้ใช้
- ตัวเลข benchmark ที่วัดได้จะผูกกับ commit ที่เจาะจงได้ ทำให้ย้อนหาสาเหตุ regression ได้

หนึ่ง commit = หนึ่ง task ที่จบและเทสต์ผ่านแล้ว
ข้อความ commit บอก **"ทำไม"** ไม่ใช่แค่ "ทำอะไร" (diff บอกอยู่แล้วว่าทำอะไร)

**ก่อน commit ทุกครั้ง**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo deny check
cargo tree -d | grep '^wgpu'     # ต้องว่าง
cargo tree | grep -iE 'reqwest|hyper|tokio|curl|ureq|openssl'   # ต้องว่าง (I-8)
git ls-files --eol | grep -c 'w/crlf'                          # ต้องเป็น 0
```

**จบทุก session ด้วยการรายงาน:** อะไรเสร็จ/ไม่เสร็จ · ผล 4 คำสั่ง · ตัวเลขที่วัดได้จริง ·
สิ่งที่ spec ไม่ระบุแล้วตัดสินใจเอง · **อะไรที่ spec เขียนไว้แล้วทำจริงไม่ได้** ← ข้อนี้มีค่าที่สุด
