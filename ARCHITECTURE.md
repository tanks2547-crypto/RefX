# RefX — Architecture

> โปรแกรมจัดการภาพ reference สำหรับนักวาด/นักออกแบบ
> เป้าหมายลำดับความสำคัญ (ห้ามสลับ): **1) เสถียร  2) ปลอดภัย  3) กิน CPU/RAM น้อย  4) ฟีเจอร์**

เอกสารนี้คือ single source of truth ของสถาปัตยกรรม ถ้า spec ย่อยใน `docs/` ขัดกับไฟล์นี้ ให้ยึดไฟล์นี้แล้วแจ้งแก้

---

## 0. สรุปการตัดสินใจ (TL;DR)

| หัวข้อ | เลือก | เหตุผลย่อ |
|---|---|---|
| ภาษา | **Rust (stable, edition 2024)** | memory-safe ตั้งแต่คอมไพล์ = ตัดสาเหตุ crash/RCE อันดับหนึ่งทิ้ง, ไม่มี GC pause, ไม่มี runtime |
| Renderer | **wgpu** (Vulkan / D3D12 / Metal) | native GPU, abstraction ข้าม 3 OS ด้วยโค้ดชุดเดียว, ตัวมันเอง safe Rust |
| Window/Input | **winit 0.30.x** | มาตรฐาน de-facto, event-driven (`ControlFlow::Wait`) |
| UI chrome | **egui + egui-wgpu** | immediate-mode, เบามาก, แชร์ GPU device เดียวกับ canvas, พิสูจน์แล้วใน Rerun (data หนักระดับล้าน point) |
| Storage/cache | **SQLite (rusqlite, bundled)** | thumbnail + metadata cache, ACID, ไฟล์เดียว, ไม่มี server |
| Decoders | **pure-Rust เท่านั้น** (`image`, `zune-jpeg`, `png`) | libwebp/libjpeg/libpng คือแหล่ง CVE อันดับต้นของโปรแกรมประเภทนี้ — ตัดทิ้งทั้งหมด |
| Async runtime | **ไม่ใช้** (ไม่มี tokio) | ใช้ thread pool + `crossbeam-channel` — binary เล็กลง, ไม่มี scheduler overhead, debug ง่ายกว่า |
| Platform | **P0–P5: Windows + Linux → P6: macOS** | ตามที่ตกลง: macOS ทำแยกทีหลังได้ เพราะ wgpu/winit/egui abstract ให้แล้ว งานที่เหลือคือ notarization + menu bar + file dialog |

### ทำไมไม่เลือกอย่างอื่น

- **Electron / Tauri** — WebView บังคับให้ทุกภาพผ่าน DOM/compositor ของเบราว์เซอร์ ที่ 1000+ ภาพจะกิน RAM 1GB+ และ frame time คาดเดาไม่ได้ ขัดข้อกำหนดข้อ 3 โดยตรง
- **C++ / Qt6** — เร็วพอ ๆ กัน แต่ทุกบั๊ก memory คือ crash หรือช่องโหว่ ขัดข้อ 1 และ 2 บวกปัญหา license เชิงพาณิชย์
- **C# / .NET** — GC pause ทำให้ pan/zoom กระตุกแบบสุ่ม และ runtime footprint สูง

---

## 1. หลักการที่ห้ามละเมิด (Invariants)

โค้ดที่ละเมิดข้อใดข้อหนึ่ง = reject ใน review

1. **I-1 Idle = 0% CPU.** ไม่มี render loop แบบวนตลอด เรนเดอร์เฉพาะเมื่อมี input, animation ที่กำลังเล่น, หรือ texture โหลดเสร็จ (`ControlFlow::Wait` + `request_redraw()`)
2. **I-2 UI thread ห้ามบล็อก.** ห้าม file I/O, ห้าม decode, ห้าม query DB บน UI thread เด็ดขาด งานทุกอย่างที่ยาวเกิน 1 ms ต้องไปอยู่ worker
3. **I-3 ข้อมูลผู้ใช้ห้ามหาย.** ทุก mutation ผ่าน `Command` ที่ undo ได้ + journal ลงดิสก์ การ save ต้องเป็น atomic (temp → fsync → rename) ห้ามเขียนทับไฟล์ต้นฉบับตรง ๆ
4. **I-4 ไฟล์ทุกไฟล์คือ input ที่ไม่น่าไว้ใจ.** ภาพ, .refx, sidecar, clipboard — ต้องผ่าน validation + resource limit ทั้งหมด
5. **I-5 `#![forbid(unsafe_code)]`** ในทุก crate ยกเว้น `refx-platform` (crate เดียวที่อนุญาต และต้องมีคอมเมนต์ `// SAFETY:` ทุกบล็อก)
6. **I-6 มี memory budget เสมอ.** ไม่มี cache ไหนโตได้ไม่จำกัด ทุก cache ต้องมี hard cap + LRU eviction
7. **I-7 ภาพเสียหาย 1 ไฟล์ ห้ามล้มทั้งโปรแกรม.** decode ทุกครั้งห่อด้วย `catch_unwind` ใน worker — ผลลัพธ์คือ item ขึ้นสถานะ "โหลดไม่ได้" ไม่ใช่ crash
8. **I-8 ไม่มี network ใน v1.** ไม่มี telemetry, ไม่มี auto-update, ไม่มี plugin/scripting ผิวสัมผัสการโจมตี = ระบบไฟล์ local เท่านั้น

---

## 2. โครงสร้าง Crate (Cargo workspace)

แยก crate เพื่อบังคับทิศทาง dependency — ชั้นล่างห้ามรู้จักชั้นบน คอมไพเลอร์จะเป็นคนบังคับให้เอง

```
refx/
├── crates/
│   ├── refx-core/        # โดเมนล้วน: Board, Item, Command, undo, layout algorithms
│   │                     # ห้าม depend: wgpu, winit, egui, sqlite, fs
│   ├── refx-asset/       # decode, thumbnail, content hash, cache DB, memory budget
│   │                     # depend ได้: image, zune, blake3, rusqlite  |  ห้าม: wgpu, egui
│   ├── refx-render/      # wgpu device, texture atlas, instanced quad renderer, culling
│   │                     # depend ได้: wgpu, refx-core (read-only)  |  ห้าม: egui, winit
│   ├── refx-ui/          # egui shell, สอง mode, tools, keymap, inspector
│   │                     # depend: ทุกอันข้างบน + egui
│   ├── refx-io/          # .refx format, autosave/journal, recovery, file watching
│   ├── refx-platform/    # ★ crate เดียวที่อนุญาต unsafe — OS dialog, clipboard image, single-instance
│   └── refx-app/         # binary: ต่อสายทุกอย่าง, panic handler, logging, CLI args
├── fuzz/                 # cargo-fuzz targets: document parser, decode wrapper
├── xtask/                # งาน build/package/bench
└── docs/
```

**กฎ dependency (บังคับด้วย `cargo-deny` + CI):**

```
refx-app → refx-ui → refx-render → refx-core
                  ↘ refx-asset  ↗
                  ↘ refx-io    ↗
ทุกตัว → refx-platform (leaf)
```

`refx-core` ต้อง unit-test ได้โดยไม่ต้องมี GPU และไม่ต้องแตะดิสก์ นี่คือตัวชี้วัดว่าแยกชั้นถูก

---

## 3. Threading Model

```
┌─ Main thread ────────────────────────────────────────────┐
│  winit event loop (ControlFlow::Wait)                    │
│  → egui ui pass → build render instances → queue.submit  │
│  แตะ: GPU device, document state (owner เดียว)            │
└──────────────────────────────────────────────────────────┘
        │ Job (priority)                  ▲ DecodedImage / Thumb
        ▼                                 │
┌─ Decode pool (N = clamp(cores-2, 2, 6)) ─────────────────┐
│  decode → resize → (BC7 encode) → ส่งกลับเป็น staging     │
│  ทุก job ห่อ catch_unwind + มี cancellation token         │
└──────────────────────────────────────────────────────────┘
        │                                 ▲
        ▼                                 │
┌─ IO thread (1 ตัว) ──────────────────────────────────────┐
│  scan directory, sqlite (serialized), file watch, journal│
└──────────────────────────────────────────────────────────┘
```

- สื่อสารด้วย `crossbeam-channel` เท่านั้น ห้ามแชร์ `Mutex<Document>` ข้ามเธรด
- Document state เป็นของ main thread คนเดียว worker ส่งกลับเป็น message ที่ main thread เอาไป apply
- **Cancellation:** ทุก decode job ถือ `Arc<AtomicBool>` เมื่อ viewport ขยับ งานที่หลุดจอถูกยกเลิกทันที ไม่เผา CPU กับสิ่งที่มองไม่เห็น
- **Priority queue:** งานเรียงตามระยะห่างจากจุดกึ่งกลาง viewport → ภาพที่ผู้ใช้กำลังมองมาก่อนเสมอ

---

## 4. สอง Mode กับ UI กลาง

หัวใจของดีไซน์: **ทั้งสอง mode คือ view สองแบบบน document ก้อนเดียวกัน ไม่ใช่สองโปรแกรม**

```
┌───────────────────────────────────────────────────────────┐
│  Title bar / Board tabs                          [shared] │
├───────────────────────────────────────────────────────────┤
│  [ Canvas | Arrange ]   Toolbar (เปลี่ยนตาม mode) [shared] │
├─────────┬───────────────────────────────────┬─────────────┤
│ Library │                                   │  Inspector  │
│ [shared]│      VIEWPORT ◄── สลับที่นี่เท่านั้น  │  [shared]   │
│         │                                   │             │
├─────────┴───────────────────────────────────┴─────────────┤
│  Status bar: zoom · item count · RAM/VRAM used   [shared] │
└───────────────────────────────────────────────────────────┘
```

ส่วนที่ทำเครื่องหมาย `[shared]` เป็นโค้ดชุดเดียวใน `refx-ui/src/shell.rs` — mode เปลี่ยนแค่ `ViewportBehavior` (trait) กับ tool palette

| | **Canvas mode** | **Arrange mode** |
|---|---|---|
| แนวคิด | ระนาบอิสระไม่จำกัด วางทับกันได้ | ตารางจัดอัตโนมัติ เรียง/กรอง/ติดแท็ก |
| ผู้ใช้คุม | ตำแหน่ง/สเกล/หมุน/crop/z-order เอง | เลือก layout + sort key แล้วโปรแกรมจัดให้ |
| เก็บใน | `ItemCanvas` (pos, scale, rot, z, crop, opacity) | `ItemMeta` (tags, rating, color label, group) |
| ใช้ตอน | จัดหน้า mood board, เทียบสัดส่วน, วาดตาม | คัดภาพ, ติดแท็ก, หาไฟล์, จัดระเบียบก่อนขึ้น canvas |

**สะพานเชื่อมสองโหมด (จุดที่ทำให้ดีไซน์นี้คุ้ม):**

- `Arrange → Canvas`: ปุ่ม **Apply layout to canvas** คำนวณตำแหน่งจาก layout engine แล้วเขียนลง `ItemCanvas` ทั้งชุด เป็น `Command` ก้อนเดียว → **undo ครั้งเดียวกลับได้หมด**
- `Canvas → Arrange`: ปุ่ม **Sort by canvas order** อ่านตำแหน่งบน canvas (บนลงล่าง ซ้ายไปขวา) มาเป็น sort key ของ arrange
- สลับ mode ไปมา **ไม่ทำลายข้อมูลอีกฝั่ง** — `ItemCanvas` และ `ItemMeta` อยู่คู่กันเสมอ นี่คือเหตุผลที่ต้องแยกสอง struct

รายละเอียดเต็ม: [`docs/03-modes-and-ui.md`](docs/03-modes-and-ui.md)

---

## 5. Pipeline ของภาพ (สรุป)

```
ไฟล์บนดิสก์
  │ memmap2 (zero-copy) + probe header
  ▼
[ตรวจ limit: ขนาดไฟล์, pixel count, ประเภท] ──ไม่ผ่าน──► Item = Failed(reason)
  │
  ▼ blake3 content hash
[ถาม cache.sqlite: มี thumbnail ไหม?] ──มี──► ยัดเข้า atlas ทันที (ไม่ decode ใหม่)
  │ ไม่มี
  ▼ decode ใน worker (catch_unwind)
[thumbnail 128px → BC7] → เขียนลง sqlite + atlas
  │
  ▼ เมื่อผู้ใช้ซูมเข้า
[working texture ขนาดเท่าที่เห็นบนจอ] ← LRU, VRAM budget
  │
  ▼ เมื่อซูม > 100%
[full-res texture] ← สูงสุด 2 ภาพพร้อมกัน
```

Cache DB อยู่ที่ `%LOCALAPPDATA%\RefX\cache.sqlite` (Win) / `~/.cache/refx/` (Linux)
คีย์เป็น content hash ไม่ใช่ path → ย้ายไฟล์/เปลี่ยนชื่อแล้ว thumbnail ไม่หาย และไฟล์ซ้ำใช้ thumbnail ร่วมกัน

รายละเอียด: [`docs/05-memory-and-assets.md`](docs/05-memory-and-assets.md)

---

## 6. Performance Budget (ตัวเลขที่ต้องผ่าน ไม่ใช่ความหวัง)

วัดบนเครื่องอ้างอิง: 4 core / 16 GB RAM / iGPU, board 1000 ภาพ (ผสม JPEG/PNG ~4000px)

| ตัวชี้วัด | เพดาน |
|---|---|
| RAM ตอน idle เปิด board 1000 ภาพ | ≤ 250 MB |
| VRAM ตอน idle | ≤ 200 MB |
| CPU ตอน idle (ไม่แตะเมาส์) | **0 %** |
| เวลาเปิด board 1000 ภาพจน thumbnail ขึ้นครบ (cache อุ่น) | ≤ 1.5 s |
| Frame time ขณะ pan/zoom | ≤ 8 ms (p99 ≤ 16 ms) |
| เวลาเปิดโปรแกรมจนหน้าต่างพร้อมใช้ | ≤ 400 ms |
| ขนาด binary | ≤ 25 MB |

CI ต้องมี benchmark ที่ fail เมื่อทะลุเพดาน ไม่ใช่แค่รายงาน

---

## 7. ความเสถียร: กันข้อมูลหาย

1. **Command journal** — ทุก `Command` ที่ apply แล้วเขียนต่อท้าย `<doc>.refx.journal` (append-only, fsync ทุก 2 วินาที หรือทุก 20 commands แล้วแต่อะไรถึงก่อน)
2. **Atomic save** — เขียน `.refx.tmp` → `fsync` → `rename` ทับตัวจริง → ลบ journal ระบบไฟล์รับประกัน rename เป็น atomic
3. **Panic hook** — `std::panic::set_hook` เขียน journal + log แล้วค่อยตาย ครั้งถัดไปที่เปิดโปรแกรมเจอ journal ค้าง → เสนอกู้คืน
4. **ไม่แก้ไฟล์ต้นฉบับ** — RefX อ่านภาพต้นทางอย่างเดียวตลอดชีวิตโปรแกรม การ crop/rotate เก็บเป็น transform ใน document เท่านั้น
5. **GPU device lost** — wgpu แจ้ง `SurfaceError::Lost` / device lost (เกิดจริงเวลา driver update, sleep/resume, GPU switch) ต้อง recreate device + re-upload atlas โดยไม่ปิดโปรแกรม **ข้อนี้คือสาเหตุ crash อันดับหนึ่งของแอปกราฟิกบน Windows — ต้องทำตั้งแต่ P0**

---

## 8. ความปลอดภัย (สรุป)

| ผิวสัมผัส | มาตรการ |
|---|---|
| Image decoder | pure-Rust เท่านั้น, ban ทุก `*-sys` codec ด้วย `cargo-deny`, `catch_unwind` ต่อ job, limit pixel/alloc/เวลา |
| Decompression bomb | ปฏิเสธก่อน decode: > 268 M pixels (16384²) หรือ ratio ที่ประกาศไว้กับขนาดไฟล์ผิดปกติ |
| .refx (ไฟล์ที่คนอื่นส่งมา) | deserialize แบบมี bound ทุก field, ปฏิเสธ path ที่มี `..` / absolute / symlink ออกนอก root |
| Path traversal | canonicalize + ตรวจว่ายังอยู่ใน root ที่ผู้ใช้อนุญาต |
| Supply chain | `cargo-deny` (license + advisory + banned), `cargo-vet`, lockfile commit, ตรึงเวอร์ชัน |
| Binary hardening | `control-flow-guard` (Win), `relro`+`pie` (Linux), ไม่มี debug symbol ใน release |
| ผิวสัมผัสที่ตัดทิ้งเลย | ไม่มี network, ไม่มี plugin, ไม่มี scripting, ไม่มี auto-update ใน v1 |

รายละเอียด + threat model: [`docs/06-security.md`](docs/06-security.md)

---

## 9. เอกสารที่เหลือ

| ไฟล์ | เนื้อหา |
|---|---|
| [`docs/01-decisions.md`](docs/01-decisions.md) | ADR — ทางเลือกที่พิจารณาแล้วไม่เอา พร้อมเหตุผล |
| [`docs/02-data-model.md`](docs/02-data-model.md) | type ทุกตัว, arena + generational ID, Command/undo |
| [`docs/03-modes-and-ui.md`](docs/03-modes-and-ui.md) | สอง mode, shared shell, tool, keymap, layout algorithms |
| [`docs/04-rendering.md`](docs/04-rendering.md) | instanced quad, atlas, culling, event-driven redraw, device lost |
| [`docs/05-memory-and-assets.md`](docs/05-memory-and-assets.md) | 3 tier cache, budget manager, decode scheduler, sqlite schema |
| [`docs/06-security.md`](docs/06-security.md) | threat model + มาตรการ + fuzzing |
| [`docs/07-file-format.md`](docs/07-file-format.md) | .refx container, versioning, linked vs packed, journal |
| [`docs/08-testing-and-budgets.md`](docs/08-testing-and-budgets.md) | test strategy, benchmark, definition of done |
| [`docs/09-crate-versions.md`](docs/09-crate-versions.md) | เวอร์ชันที่ตรวจสอบแล้ว ณ 26 ก.ค. 2026 + กับดักความเข้ากันได้ |
| [`ROADMAP.md`](ROADMAP.md) | แผนงาน P0–P6 แตกเป็น task ให้ coder |
| [`CLAUDE.md`](CLAUDE.md) | กฎบังคับสำหรับ coder agent |
