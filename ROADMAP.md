# RefX — Roadmap

แผนงานสำหรับ coder agent แต่ละ task ควรจบใน 1 session และมี "เสร็จเมื่อ" ที่ตรวจสอบได้จริง
**ห้ามข้าม phase** โดยเฉพาะ P0 — งานพวกนั้นแก้ทีหลังแพงกว่าหลายเท่า

---

## P0 — Foundations (สัปดาห์ 1–2)

เป้า: หน้าต่างเปิดได้ มีสี่เหลี่ยมสีวาดบน GPU pan/zoom ได้ CPU idle = 0% และ **กู้จาก device lost ได้แล้ว**

| # | Task | เสร็จเมื่อ |
|---|---|---|
| P0-1 | Cargo workspace ตาม ARCHITECTURE §2 + lint + `deny.toml` + CI | `cargo build/clippy/fmt/deny` ผ่านทั้งหมดบน Win + Linux |
| P0-2 | `refx-platform`: window (winit 0.30 `ApplicationHandler`), single-instance, dirs | หน้าต่างเปิด/ปิด/resize ได้ ไม่มี warning |
| P0-3 | `refx-render`: wgpu init, surface config, clear color | เห็นหน้าต่างสีพื้น |
| P0-4 | **Event-driven loop** (`ControlFlow::Wait`) + เทสต์ idle-no-redraw | เทสต์ `idle_produces_no_redraw` ผ่าน; Task Manager แสดง 0.0% |
| P0-5 | **Device lost / surface error recovery** + flag จำลอง **สองแบบ** (นับเฟรม และ นับเวลา) | กู้ได้ทั้งตอนวาดอยู่และตอน idle ไม่ crash ไม่กู้วนซ้ำ — ดูหมายเหตุท้าย phase |
| P0-6 | Instanced quad pipeline + `QuadInstance` + `quad.wgsl` | วาด 10,000 สี่เหลี่ยมสีสุ่มที่ 60 fps |
| P0-7 | Camera + pan/zoom (ซูมเข้าหาเคอร์เซอร์) | ลื่น ไม่มี jitter ที่ zoom สุดทั้งสองทาง |
| P0-8 | egui + egui-wgpu บน device เดียวกัน + shell เปล่า | เห็น panel ทั้ง 5 ฝั่ง ปุ่มกดได้ |
| P0-9 | `tracing` + panic hook + crash log | panic แล้วได้ log ไฟล์ + dialog |

> **P0-5 คืองานที่คนข้ามบ่อยที่สุดและเจ็บที่สุด** ถ้าไม่ทำตอนนี้ พอ P4 มีอะไรให้เสียหายแล้วค่อยมาแก้ จะต้องรื้อ resource management ทั้งหมด

### หมายเหตุ P0-5: flag จำลองต้องมีสองแบบ (แก้ 26 ก.ค. 2026)

flag เดิม `--force-device-lost-after=<เฟรม>` **ใช้ไม่ได้ถ้า I-1 ถูกต้อง** — แอปวาดแค่ ~6 เฟรมตอนเปิด
แล้วหลับ ตัวนับเฟรมไม่มีวันถึงเป้า ข้อกำหนดสองข้อขัดกันเองโดยธรรมชาติ

| flag | จำลองสถานการณ์ | I-1 |
|---|---|---|
| `--force-device-lost-after=<เฟรม>` | device ตายระหว่างผู้ใช้ pan/zoom | ฝืน (build ที่เปิด feature วาดต่อเนื่องจนถึงเฟรมเป้าหมาย) |
| `--force-device-lost-after-ms=<ms>` ★ | **device ตายตอนแอปหลับอยู่** — driver update, sleep/resume | ไม่ฝืน (`ControlFlow::WaitUntil` ตื่นครั้งเดียว) |

**ตัวที่สำคัญกว่าคือ `-ms`** เพราะเป็นสถานการณ์จริงของผู้ใช้: เปิดโปรแกรมทิ้งไว้ข้าง Photoshop
ทั้งวันแล้ว driver อัปเดต ไม่ใช่ตอนกำลังลากภาพ ต้องพิสูจน์ว่ากู้จาก**สถานะหลับ**ได้ด้วย

```bash
cargo run --features force-device-lost -p refx-app -- --force-device-lost-after=120
cargo run --features force-device-lost -p refx-app -- --force-device-lost-after-ms=8000
```

ทั้งคู่ต้อง: กระพริบครั้งเดียว → วาดต่อได้ → **กลับไป idle สนิท (redraw count = 0)** ไม่กู้วนซ้ำ

---

## P1 — Asset pipeline (สัปดาห์ 3–4)

เป้า: ลากภาพ 1000 ไฟล์เข้าโปรแกรมแล้วเห็น thumbnail ครบภายใน 5 วินาที RAM ไม่เกินเพดาน

| # | Task | เสร็จเมื่อ |
|---|---|---|
| P1-1 | `decode_guarded` ครบเกราะทุกชั้น (06-security §3) | เทสต์ยิงภาพเสีย/bomb 50 แบบ → คืน `Err` ทุกอัน ไม่ crash |
| P1-2 | blake3 hashing + fast path ไฟล์ใหญ่ | เทสต์ hash คงที่, ไฟล์ 100 MB < 200 ms |
| P1-3 | cache.sqlite + schema + IO thread | เปิด/ปิด/เสียหายแล้วสร้างใหม่ ทำงานถูกทุกกรณี |
| P1-4 | Decode worker pool + priority queue + cancellation | pan เร็วผ่าน 500 ภาพ → job ถูก cancel จริง (วัดจาก counter) |
| P1-5 | Thumbnail atlas (array texture) + free-list + BC7 fallback | 1000 thumbnail ใน 1 draw call, VRAM ตรงกับที่คำนวณ |
| P1-6 | `MemoryBudget` + `TextureAllocator` + LRU eviction | ตั้ง limit ต่ำ ๆ แล้วยังทำงานถูก ไม่ crash, สถานะโชว์บน status bar |
| P1-7 | T1 working texture + mipmap + เลือกขนาดตามระยะซูม | ซูมเข้าแล้วภาพชัดขึ้นภายใน ~200 ms |
| P1-8 | Drag & drop + clipboard paste | ลากจาก Explorer / เบราว์เซอร์ / Ctrl+V ได้ครบ |

---

## P2 — Canvas mode (สัปดาห์ 5–7)

| # | Task | เสร็จเมื่อ |
|---|---|---|
| P2-1 | `Arena` + `ItemId` + `Board` + `Item` ตาม 02-data-model **เฉพาะส่วนที่มีคนใช้จริง** | unit test ครบ ไม่ต้องมี GPU |

> **P2-1 หมายเหตุ (2 ส.ค. 2026):** คำว่า "ครบตาม 02-data-model" เดิมกำกวม
> `Workspace` (§2) และ `AssetId`/`Arena<Asset>` (§1) **ยังไม่ต้องทำ** —
> multi-board tabs คือ P4-7 และยังไม่มีใครเรียกใช้
> เขียนไว้ก่อนโดยไม่มีคนใช้ = โครงเปล่าที่ `docs/08 §3.9` ข้อ 2 ห้ามไว้
> `SetCrop` / `ApplyLayout` / `GroupItems` ก็เช่นกัน — เป็นของ P2-7 / P3-2 / P2-9
| P2-2 | `Command` trait + `History` + merge/seal | property test `undo_restores_exactly` ผ่าน |
| P2-3 | `SpatialIndex` (loose grid) + hit-test | คลิกโดนภาพบนสุดเสมอแม้ทับกัน 50 ชั้น |
| P2-4 | Select / rubber-band / multi-select | |
| P2-5 | Move / scale (handle) / rotate + snap | ลากค้าง = 1 undo, ปล่อยแล้ว seal |
| P2-6 | Z-order (`[` `]` + ส่งหน้า/หลังสุด) | |
| P2-7 | Crop แบบ non-destructive | ไฟล์ต้นฉบับไม่ถูกแตะ, ดับเบิลคลิกรีเซ็ต |
| P2-8 | Grayscale / flip / opacity / filter ผ่าน shader flags | สลับ grayscale ทั้ง board ที่ 1000 ภาพ = 0 texture upload |
| P2-9 | Align / distribute | |
| P2-10 | Color picker + measure tool | picker อ่านจาก pixel ต้นฉบับ ไม่ใช่จากจอ |
| P2-11 | Text note | |

---

## P3 — Arrange mode (สัปดาห์ 8–9)

| # | Task | เสร็จเมื่อ |
|---|---|---|
| P3-1 | `ItemMeta` + tag/rating/color label + inspector ฝั่ง arrange | |
| P3-2 | Layout engines ทั้ง 5 ตัว (pure function) | property test finite + deterministic ผ่าน |
| P3-3 | Virtual scrolling | **3,072 item** scroll ลื่น, วาดจริง < 60 ตัว |

> ### ★ แก้เพดานจาก 10,000 → 3,072 (9 ส.ค. 2026)
>
> ตัวเลข 10,000 เป็นค่าที่**เดาไว้ตอนเขียน ROADMAP โดยไม่ได้คิดจากข้อจำกัดจริง**
> (ชุดเดียวกับ "1000 ภาพ cache เย็น ≈ 4 วินาที" และ "binary 12 MB" ที่ผิดทั้งคู่)
>
> เพดานจริงคือ **3,072 = 12 layer × 256 ช่อง** ของ thumbnail atlas
> การยกให้ถึง 10,000 ต้องแก้สองชั้นพร้อมกัน — atlas เป็น LRU ตามสิ่งที่อยู่ในจอ
> **และ** thumbnail (64 KB × 10,000 = 640 MB) ต้องอ่านกลับจาก `cache.sqlite` ได้
> → แตะทั้ง I-6 และ threading model
>
> **ไม่ทำตอนนี้** เพราะยังไม่มีหลักฐานว่ามีผู้ใช้จริงที่มีภาพ 3,000+ ใบบน board เดียว
> mood board ที่มีภาพหมื่นใบไม่ใช่ mood board แล้ว
> ถ้าเจอผู้ใช้จริงที่ชนเพดาน ค่อยกลับมาทำใน P5 พร้อมตัวเลขจากของจริง
>
> **★ แต่สิ่งที่ต้องแก้ทันทีคืออาการ ไม่ใช่เพดาน** — ดูด้านล่าง

### ★★ ไฟล์ที่เปิดไม่ได้ต้องกลายเป็น `Missing` ไม่ใช่หายไป (ตัดสิน 28 ส.ค. 2026)

วัดจริง: ลากไฟล์เสีย 20 ใบ → ขึ้นบน board **4 ใบ** · ลากเสีย 5 + ดี 1 → ขึ้น **1 ใบ**
ไม่มีข้อความบอกสักคำ · และแอปพิมพ์ *"ลากไฟล์ 20 ไฟล์ → ขึ้นจอครบใน 62 ms"* ซึ่งไม่จริง

กฎจริงคือ **item ถูกสร้างก็ต่อเมื่อ `probe_dimensions` สำเร็จ** — ไฟล์ที่ตกตั้งแต่ด่านหัวไฟล์
ไม่กลายเป็น item เลย

**ทางที่ถูก: ใช้กลไกที่มีอยู่แล้ว ไม่ต้องออกแบบ UI ใหม่**

`ItemKind::Missing { reason: MissingReason::Damaged }` มีอยู่แล้วใน `docs/02 §2`
และ P4-6 มี placeholder + ข้อความสรุป *"N images could not be found"* ทำงานอยู่แล้ว

→ ไฟล์ที่ decode ไม่ผ่าน = `Missing` เหมือนไฟล์ที่หาไม่เจอทุกประการ
   ได้ placeholder · นับในข้อความสรุป · รอดตอน save · relink ทีหลังได้ถ้าผู้ใช้ซ่อมไฟล์

**ผู้ใช้ลาก 20 ต้องเห็น 20** — จะเป็นภาพหรือ placeholder ก็ได้ แต่ห้ามหาย
และ `DropBatch` (ทำไว้แล้วตอน board เต็ม) ต้องนับใบพวกนี้ด้วย

### ★ ลากไฟล์เกินเพดานแล้วเงียบ = ผิด I-3

ตอนนี้ลาก 10,000 ไฟล์เข้าไปได้ item **3,072 ใบพอดี** แล้ว `drain_decode_results`
**ไม่สร้าง item ที่เหลือเลย โดยไม่บอกอะไรผู้ใช้**

ผู้ใช้เห็นแค่ว่า "ลากเข้าไปแล้วภาพขึ้นไม่ครบ" ซึ่งอ่านได้อย่างเดียวว่าโปรแกรมทำงานหาย —
และเขาไม่มีทางรู้ว่าอีก 6,928 ใบหายไปไหนหรือควรทำอะไรต่อ

**ต้องมี:** ข้อความที่บอก **สิ่งที่เกิดขึ้น + สิ่งที่ทำได้ต่อ** ตาม `CLAUDE.md`
เช่น *"board นี้เต็มแล้ว (3,072 ภาพ) — เพิ่มได้อีก 0 ใบ จาก 10,000 ที่ลากเข้ามา
ลองแยกเป็นหลาย board"* · และตัวเลขต้องนับให้ครบว่ากี่ใบที่ไม่ได้ถูกเพิ่ม
| P3-4 | Sort + filter + cache ผลตาม `board.revision` | ไม่คำนวณซ้ำเมื่อไม่มีอะไรเปลี่ยน (วัดด้วย counter) |
| P3-5 | **`ApplyLayoutCommand`** (Arrange → Canvas) | undo ครั้งเดียวคืนสภาพเดิมครบ |
| P3-6 | **Sort by canvas order** (Canvas → Arrange) | |
| P3-7 | Group / ungroup | |
| P3-8 | เทสต์: สลับ mode 100 ครั้ง แล้วข้อมูลไม่เปลี่ยน | `dirty` ยัง false, snapshot เท่าเดิมเป๊ะ |

---

## P4 — Persistence & Recovery (สัปดาห์ 10–11)

| # | Task | เสร็จเมื่อ |
|---|---|---|
| P4-1 | `.refx` format v1 (linked) + DTO แยกจาก core | round-trip property test ผ่าน |
| P4-2 | Atomic save (tmp → fsync → rename → fsync dir) | ฆ่าโปรเซสกลาง save 100 ครั้ง → ไฟล์เดิมไม่เสียสักครั้ง |
| P4-3 | **Autosave snapshot** + fsync policy (เดิมเขียนว่า command journal) | ฆ่าโปรเซสระหว่างแก้งาน → **วัดว่าเสียไปกี่วินาทีจริง** ไม่ใช่แค่มีไฟล์ |

> **P4-3 เปลี่ยนดีไซน์ 12 ส.ค. 2026** — `Command` เป็น trait ที่ serialize ไม่ได้
> การทำ journal ต้องสร้าง `CommandDto` 13 variant ซึ่งเป็นผิวรูปแบบไฟล์ใหม่ทั้งชุด
> + ภาระถาวรต่อ command ใหม่ทุกตัว และถ้าพลาดหนึ่งจุด ผู้ใช้ได้ **board ที่ผิดแบบเงียบ ๆ**
> → v1 ใช้ **snapshot ทั้ง board** ด้วย DTO เดียวกับ `.refx` · **เหตุผลเต็มใน `docs/07 §4`**
| P4-4 | Crash recovery + dialog 3 ตัวเลือก | End Task ระหว่างแก้งาน → กู้ได้ครบ |
| P4-5 | Packed mode + asset table | |
| P4-6 | Relink flow 5 ขั้น | ย้ายโฟลเดอร์ภาพแล้วยังหาเจอ |
| P4-7 | Multi-board tabs | |
| P4-8 | `xtask dump-refx` (binary → JSON) | debug ไฟล์ผู้ใช้ได้จริง |
| P4-9 | Fuzz targets ทั้ง 4 ตัว + รันใน CI | รัน 15 นาที/target ไม่เจอ crash |

> ### ★ target ที่สี่คือ `fuzz_packed` ไม่ใช่ `fuzz_journal` (แก้ 27 ส.ค. 2026)
>
> `fuzz_journal` เล็งไปที่ `refx-io::journal` ซึ่ง **จะไม่มีวันถูกเขียน** —
> P4-3 เปลี่ยนจาก command journal เป็น snapshot ไปแล้ว (`docs/07 §4`)
>
> **`fuzz_packed` → `packed::read_index` + `extract`**
>
> > ★ **ไม่เรียก `spool::unpack` จริงในลูป** (แก้ 27 ส.ค. 2026 หลังวัด)
> > มันทำ `create_dir_all` + `File::create` + `sync_all` + `rename` ต่อหนึ่งรอบ
> > → throughput ตกจาก ~35,000 เหลือหลักร้อยรอบ/วินาที = coverage บน parser
> > น้อยลงหลายร้อยเท่า **แลกกับข้อมูลใหม่แทบเป็นศูนย์** เพราะทุกไบต์ที่ `unpack` อ่าน
> > เดินผ่าน `extract` ซึ่งถูกยิงอยู่แล้ว และกฎ "ห้ามเขียนทับ" มีเทสต์ของตัวเองใน `spool.rs`
> >
> > สิ่งที่ข้อกำหนดต้องการจริง ๆ — **ที่อยู่ปลายทางต้องไม่ถูกไบต์ในไฟล์กำหนด** —
> > ยัง assert ทุกรอบ (ตัดนามสกุลแล้วที่เหลือต้องเป็น hex 64 ตัว)
>
> เป็น parser ไบนารีตัวเดียวที่เหลือซึ่งยังไม่มี fuzz แตะเลย และเป็นตัวที่อ่าน
> `count`/`offset`/`len` **จากไฟล์** แล้ว seek/จองตามนั้น — คือสิ่งที่ I-4 มีไว้กันพอดี
>
> ★ ที่ทำให้ช่องนี้ใหญ่กว่าที่คิด: `fuzz_document` เขียน `flags = 0` เสมอ
> **ไม่มี input ไหนเคยตั้ง packed bit เลย** → `dto::decode` ไม่เคยแตะ asset table
> และตอนนี้ `xtask dump-refx` (P4-8) ก็ขับเส้นทางนี้ด้วย
>
> **ข้อกำหนดเพิ่ม:**
> - corpus เริ่มต้นต้องมี **ไฟล์ packed ที่ถูกต้องจริง** ไม่งั้น fuzzer จะเสียเวลา
>   เด้งอยู่ที่ด่าน magic/version แล้วไม่มีวันถึง parser (กฎข้อ 1b — checksum บังหน้าด่าน)
> - ★ ต้อง assert ว่า **ไบต์ในไฟล์มีอิทธิพลต่อ *ที่อยู่* ที่ `unpack` เขียนไม่ได้เลย**
>   ชื่อไฟล์ใน spool มาจาก hash ที่เราคำนวณเอง — ถ้าวันใดมีใครเปลี่ยนให้อ่านชื่อจากไฟล์
>   จะกลายเป็น path traversal ทันที · fuzz ต้องเป็นตัวที่จับข้อนั้น

---

## P5 — Hardening & Polish (สัปดาห์ 12–13)

| # | Task |
|---|---|
| P5-1 | Benchmark suite ครบ + บังคับใน CI |
| P5-2 | Manual checklist ครบทุกข้อ (08-testing §3) |
| P5-3 | Settings (memory budget, theme, present mode, keymap.toml) |
| P5-4 | Export PNG/JPEG แบบ tile |
| P5-5 | Sidecar `.refx-meta` (opt-in) |
| P5-6 | Binary hardening + packaging (MSI/portable zip, AppImage/deb) · **+ ลดขนาด binary (ดูด้านล่าง)** |
| P5-7 | เอกสารผู้ใช้ + คู่มือคีย์ลัด |

### P5-6: รายการลดขนาด binary (สำรวจไว้แล้ว 28 ก.ค. 2026)

ค่าปัจจุบัน **15.37 MB / เพดาน 25 MB** — เลื่อนมาทำตรงนี้ได้เพราะ
**CI บังคับเพดานแล้ว** (`scripts/check-binary-size.sh` + negative control ถาวร)
ถ้ามันโตผิดปกติระหว่าง P2–P5 เราจะรู้ทันที ไม่ใช่มารู้ตอนแพ็กเกจ

> **อ่าน `docs/08 §6` หัวข้อ "นโยบายการลดขนาด binary" ก่อนเริ่ม**
> โดยเฉพาะรายการ **ห้ามเด็ดขาด** — `panic = "abort"` ทำลาย I-7 ทั้งข้อ
> และการตัด wgpu backend จนเหลือตัวเดียวทำลายทางถอยเมื่อ driver มีปัญหา

1. **วัดก่อนด้วย `cargo bloat --release --crates`** — ห้ามเดาว่าอะไรใหญ่
2. ตัด backend ของ wgpu ที่ build ไม่ได้ใช้ แบบ per-OS
   (Win: DX12+Vulkan · Linux: Vulkan+GL) — น่าจะเป็นก้อนใหญ่สุด
3. วัดว่า `rusqlite` feature `bundled` (SQLite ภาษา C) กินเท่าไหร่
   **ห้ามเปลี่ยนไปใช้ SQLite ของระบบ** — Windows ไม่มีให้ใช้
4. `tga` / `bmp` ใน feature ของ `image` — ต้องถามก่อน เป็นการตัดฟีเจอร์
5. `opt-level = "s"/"z"` — ทางเลือกสุดท้าย ต้องวัดคู่ benchmark เสมอ

**เกณฑ์:** รายงานตารางก่อน/หลังทั้งขนาดและ benchmark ·
decode time หรือ frame time แย่ลงเกิน 5% ให้ถอยการเปลี่ยนนั้นออก

---

## P6 — macOS (หลัง v1.0 นิ่งแล้ว)

| # | Task |
|---|---|
| P6-1 | Build + ทดสอบบน Metal |
| P6-2 | Native menu bar |
| P6-3 | keymap Cmd แทน Ctrl (แค่เปลี่ยน data ถ้า ADR-007 ถูกทำตาม) |
| P6-4 | Code signing + notarization |
| P6-5 | `.app` bundle + dmg |

---

## สิ่งที่ **ไม่ทำ** ใน v1 (จงใจ)

ตัดออกเพื่อรักษาข้อกำหนดข้อ 1–3 — ถ้าจะเพิ่มต้องเขียน ADR ใหม่ก่อน

- ❌ Plugin / scripting — ผิวสัมผัสความเสี่ยงมหาศาล
- ❌ Cloud sync / collaboration — ต้องมี network stack (ขัด I-8)
- ❌ Auto-update — ช่องทางส่งโค้ดเข้าเครื่องผู้ใช้
- ❌ วาด/ระบายบน canvas — นี่เป็นเครื่องมือ reference ไม่ใช่โปรแกรมวาด
- ❌ วิดีโอ / GIF animation — memory model คนละแบบทั้งหมด
- ❌ AI tagging — ลาก ML runtime หลายร้อย MB เข้ามา
- ❌ PSD / AVIF — ความเสี่ยง decoder สูง, ทำใน v2 พร้อม fuzz หนัก ๆ
