# HANDOFF — สถานะโปรเจกต์ RefX ณ 2 ส.ค. 2026

> เอกสารนี้มีไว้ให้ **session ใหม่ของ coder agent** อ่านเพื่อรับช่วงต่อ
> อ่านไฟล์นี้ให้จบก่อน แล้วค่อยอ่าน `CLAUDE.md` → `ARCHITECTURE.md §1` → `docs/09` → `ROADMAP.md`
>
> ⚠️ **วันที่ในเอกสารทุกฉบับเชื่อไม่ได้ 100%** — หลายจุดถูกประทับจากความจำ ไม่ได้ตรวจกับหลักฐาน
> (พบแล้วหนึ่งครั้ง: หัวข้อ CI เขียน 28 ก.ค. ทั้งที่ `git log` ยืนยันว่า 1 ส.ค.)
> **`git log` คือแหล่งความจริงของลำดับเหตุการณ์** ถ้าเจอวันที่ขัดกับ git ให้เชื่อ git แล้วแก้เอกสาร

---

## 1. สรุปสถานะ

| Phase | สถานะ |
|---|---|
| **P0-1 ถึง P0-9** | ✅ เสร็จครบ ผ่านรีวิวแล้ว |
| **P1-1 ถึง P1-6** | ✅ เสร็จ (P1-6 ครบแล้วหลังทำ `TextureAllocator`) |
| **P1-7** working texture | ✅ เสร็จ (`9e1033e`) — mip บน CPU · LRU · 17 draw call ที่ 100 ภาพ |
| **P1-8** drag & drop + clipboard paste | ✅ เสร็จ — **P1 ปิดครบแล้ว** |
| P2 ขึ้นไป | ยังไม่เริ่ม |

**เกณฑ์คุณภาพล่าสุดที่ผ่าน:** `fmt` / `clippy --all-features -D warnings` (workspace + `fuzz/`) /
`nextest 274/274` / `deny check` ครบ 4 หมวด / **binary 15.47 MB จากเพดาน 25 MB (บังคับใน CI)**

crate ที่มี: `refx-app` `refx-asset` `refx-core` `refx-io` `refx-platform` `refx-render` `refx-ui`

### สถานะ CI (มีชีวิตแล้วตั้งแต่ 1 ส.ค. 2026)

repo: **`github.com/tanks2547-crypto/RefX` (private)** · branch `main`

| | windows-latest | ubuntu-latest |
|---|---|---|
| GPU ที่ใช้ตรวจจริง | **WARP** (Dx12/Cpu) | **lavapipe** (Vulkan/Cpu) |
| เทสต์ | 274/274 | 274/274 |
| binary | 15.34 MB | 17.02 MB |

**เทสต์ GPU ถูกรันจริงทั้งสองแพลตฟอร์ม** — `REFX_REQUIRE_GPU=1` ทำให้ "ไม่มี adapter" = แดง
ไม่ใช่ข้ามเงียบ ๆ · `.config/nextest.toml` ฆ่าเทสต์ที่เกิน 4 นาที เพื่อให้เทสต์ค้างเป็น
"แดงพร้อมชื่อ" ไม่ใช่ซอมบี้กิน runner หลายชั่วโมง

> ★ **ก่อน 1 ส.ค. 2026 CI ไม่เคยรันเลยสักครั้ง** (ไม่มี remote) — ทุกอย่างที่สร้างมา
> (เพดาน binary, fuzz cron, `deny` รายสัปดาห์, เทสต์ GPU) เป็นทฤษฎีล้วน
> พอรันจริงครั้งแรกก็เจอบั๊ก 2 ตัวทันที รวมถึงโค้ด `#[cfg(target_os = "linux")]`
> ที่**ไม่เคยถูกคอมไพล์เลยตลอดโปรเจกต์**
>
> **บทเรียน:** เครื่องมือตรวจที่ไม่เคยเดินจริง = ไม่มีเครื่องมือ (ดู `docs/08 §3.9`)
> รอบแรก cold cache ~40 นาที · รอบถัดมา 7–14 นาที (`rust-cache` อุ่นแล้ว)

### สถานะ git

repo อยู่ใต้ git แล้ว (branch **`main`**) และ **push ขึ้น GitHub แบบ private แล้ว**
(`origin` = `tanks2547-crypto/RefX`) — `git fsck` สะอาด working tree สะอาด

- **identity ตั้งไว้แบบ repo-local** คือ `RefX <297583263+tanks2547-crypto@users.noreply.github.com>`
  (global ยังว่าง จงใจ — repo อื่นในเครื่องไม่ถูกกระทบ)
  ถ้า `git commit` ฟ้อง `unable to auto-detect email address` แปลว่าอยู่คนละ repo หรือ config หาย
  > ★ ใช้ **noreply ของ GitHub** ไม่ใช่อีเมลจริง — อีเมลใน commit เป็นสาธารณะเสมอ
  > ถ้า repo ถูกเปิดเป็น public วันหนึ่ง อีเมลทั้งประวัติจะโผล่ตามไปด้วย
  > GitHub ผูก commit เข้ากับโปรไฟล์ด้วย**อีเมล**ไม่ใช่ชื่อ ชื่อจึงยังเป็น `RefX` ได้ตามเดิม
  > ประวัติทั้งก้อนถูกเขียนใหม่ครั้งเดียว **ก่อน push ครั้งแรก** — หลังจากนี้ห้ามทำอีก
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

> **P1 ปิดครบแล้ว** — งานที่เสร็จแล้วถูกย้ายออกจากหัวข้อนี้ ดูประวัติใน git log
> ต้นเหตุและทางแก้ของบั๊กสำคัญบันทึกไว้ใน `docs/` แล้ว ไม่ใช่ในไฟล์นี้

> **`clipboard-win` — ตัดสินแล้วว่าไม่เพิ่ม** ดูเหตุผลใน `docs/06 §2.6`
> (แก้ได้เฉพาะ Windows · เกราะที่ทำงานแพลตฟอร์มเดียวให้ความมั่นใจผิด ๆ)
>
> **ภาพจาก clipboard คมได้แค่ระดับ thumbnail** — ปล่อยไว้ รอทำพร้อม **P4-5 packed mode**
> ห้ามแก้ด้วยไฟล์ชั่วคราวใน cache (ภาพที่ board อ้างถึงจะหายไปกับ LRU = ผิด I-3)

### 2.0 ★ แยกชั้น `refx-asset` ออกจาก `refx-platform` — ทำก่อน P2

**`Fuzz (nightly)` แดงตั้งแต่รันครั้งแรก (1 ส.ค. 2026)** — job `fuzz-lint` ล้มตอนคอมไพล์
`zbus 5.18.0` ด้วย nightly (`Stats is defined multiple times`, proc-macro พัง) **ไม่ใช่โค้ดเรา**

สายที่ลากมา: `fuzz/` → `refx-asset` → `refx-platform` → `rfd` → `ashpd` → `zbus`

→ **fuzz ลากทั้งกอง GUI มาคอมไพล์ ทั้งที่ต้องการแค่ `panic_guard`**

นี่ไม่ใช่แค่ปัญหาของ fuzz แต่เป็น **การรั่วของชั้นสถาปัตยกรรม** — ชั้น asset
ไม่ควรพึ่ง windowing / dialog / clipboard เลย (ดู ARCHITECTURE §2)

สองทางที่ควรพิจารณา เลือกหลังดูโค้ดจริง:

1. **ย้าย `panic_guard` ไป `refx-core`** — มันเป็นแค่ธง thread-local + RAII ไม่มีโค้ดเฉพาะ
   แพลตฟอร์มเลย ถ้าย้ายได้ **dependency หายทั้งเส้น** ไม่ต้อง feature-gate อะไร
   (panic *hook* ที่อ่านธงยังอยู่ `refx-platform` ตามเดิม)
2. ถ้าย้ายไม่ได้ → แยก dep ของ `refx-platform` เป็น feature ให้ `refx-asset` เอาเฉพาะที่ใช้

**ห้ามปักหมุด nightly ที่วันที่ใช้ได้เพื่อให้เขียว** — นั่นคือการเลื่อนปัญหาและทำให้
สัญญาณที่กำลังบอกความจริงเงียบลง (`docs/08 §3.9`)

ได้ประโยชน์พลอยได้: เวลา build ทั้งโปรเจกต์เร็วขึ้น และ P2 จะเพิ่มโค้ดใน `refx-asset` อีกมาก

### 2.1 → เข้า P2 Canvas mode

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

- atlas เติมกลับหลังกู้ device: **8/8 ภาพใน 0.23 ms** (วัดใหม่ 29 ก.ค. 2026)
  > ⚠️ ตัวเลขเดิม "100/100 ภาพใน 0.99 ms" จริงตอนที่วัด แต่หลังจากนั้น atlas
  > เปลี่ยนไปจอง layer แบบ lazy แล้ว **การเติมกลับพังเงียบ ๆ เหลือ 0/8**
  > จนกระทั่งงานเทสต์ device lost (29 ก.ค.) ไปรันของจริงถึงเจอ — ตัวเลขที่วัดครั้งเดียวแล้วไม่มีเทสต์คุม
  > จะกลายเป็นตัวเลขที่ *เคย* จริง โดยไม่มีใครรู้ว่ามันเลิกจริงตั้งแต่เมื่อไหร่
- device lost recovery: หลับ 2.5 s → ยิง → กู้ใน **126 ms** → เติม atlas ครบ → กลับไป idle
  (ยืนยันด้วยมือ 29 ก.ค. 2026: กู้ 1 ครั้งพอดี · validation error 0 · panic 0 · ปิดโปรแกรมแล้วโปรเซสตายสะอาด)
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
| 14 | **ภาพจาก clipboard ไม่เข้า cache.sqlite** (ตัดสิน P1-8) | ข้อ 4 บังคับคีย์ `(hash, mtime, size)` แต่ clipboard ไม่มี mtime · ใส่ค่าปลอมแทน = จุดบอดของ fast hash กลับมาทันที ซึ่ง mtime มีไว้ปิดพอดี · ภาพที่วางมาใช้ครั้งเดียวเป็นปกติ เก็บไว้มีแต่จะไล่ thumbnail ของไฟล์จริงออกจาก LRU | `pool.rs` `JobSource::Clipboard` |
| 15 | **ลำดับฟิลด์ของ `Assets` คือลำดับ drop — ห้ามสลับ** (`pool` → `io_tx` → `_io`) | `IoThread::drop` join เธรด IO ซึ่งจบก็ต่อเมื่อ sender หมดทุกใบ ถ้า `_io` ถูก drop ก่อน `io_tx` = **ปิดหน้าต่างแล้ว RefX.exe ไม่ตาย** ล็อก single-instance ค้าง เปิดใหม่ไม่ได้อีกเลย (เกิดจริง ยืนยันด้วยมือ 29 ก.ค. 2026) | `refx-ui/src/app.rs` + เทสต์ `dropping_assets_finishes_instead_of_hanging_forever` |
| 16 | **resource ที่ผูกกับ device ต้องสร้างผ่าน `DeviceBound::build` จุดเดียว** และรับด้วยการ destructure | เปิดโปรแกรมกับกู้ device เคยเป็นโค้ดคนละชุด แล้ว **drift**: P1-7 เพิ่ม `WorkingCache` แต่ `recover_device()` ไม่ได้สร้างใหม่ → หลังกู้ยังถือ texture/bind group ของ device ที่ตายแล้ว · destructure ทำให้เพิ่ม resource ใหม่แล้ว **คอมไพล์ไม่ผ่านทั้งสองที่** จนกว่าจะจัดการครบ (หลักการเดียวกับ `GpuStack`) | `refx-ui/src/app.rs` |
| 17 | **เติม atlas กลับ ต้อง `resize(layers_needed(n))` ก่อนเริ่มเติมเสมอ** | atlas ที่เพิ่งสร้างมี **0 layer** (จอง lazy ตั้งแต่ 28 ก.ค.) ถ้าเติมเลยจะได้ `NeedsResize` ตั้งแต่ภาพแรก แล้ว**ทุกภาพกลายเป็น placeholder** = board ว่างเปล่าหลัง driver อัปเดต (เกิดจริง: `restored=0 total=8`) · ขยายกลางคันไม่ได้เพราะ `resize()` ล้างตัวจัดสรรทั้งชุด | `refx-render::atlas::layers_needed` + เทสต์ `refilling_a_fresh_atlas_needs_a_resize_first` |

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
| **`arboard` จอง RAM ของ pixel ก่อนเพดานของเราได้ตรวจ** | API ของมันไม่เปิดให้อ่าน byte ดิบของ clipboard มาเข้า `decode_guarded` — มันอ่าน `CF_DIBV5`/PNG แล้วถอดรหัสเองจนเสร็จก่อนคืนค่า เพดาน `max_pixels` ของเราจึงเป็นด่าน **หลัง** การจองก้อนนั้นหนึ่งครั้ง ตอนนี้กันด้วยการจำกัดให้วางได้ทีละครั้ง · จะปิดสนิทต้องอ่าน clipboard เองใน `refx-platform` (ต้องเพิ่ม dep `clipboard-win` — **ต้องถามก่อน**) |
| ภาพที่วางจาก clipboard คมได้แค่ระดับ thumbnail | ไม่มีไฟล์ให้ decode ซ้ำตอนซูมเข้า จึงข้าม working texture ไปเลย (ถ้าไม่ข้าม จะได้งานที่ล้มเหลวแน่นอนหนึ่งใบทุกครั้งที่ซูม) · ทางแก้จริงคือเขียนลงไฟล์ชั่วคราวหรือ packed `.refx` — รอ P3/P4 |
| `--open-dir` วาง quad เป็นตาราง 16 คอลัมน์ตายตัว | ยังไม่ใช่ layout จริง รอ P2/P3 |
| `xtask gen-testdata` | ทำแล้วบางส่วน · ยังไม่มี `bench` / `package` (P5-1) |
| egui memory ตอนกู้ device | ตอนนี้ scroll position / panel ที่เปิดค้างรีเซ็ต · P5 ให้ clone `ctx.memory()` ก่อนกู้แล้วคืน |
| จุดบอด fast hash ตรงกลางไฟล์ | ปิดด้วย mtime แล้ว แต่จุดบอดตัว hash เองยังอยู่ (มีเทสต์บันทึกไว้ว่าตั้งใจ) |
| **เทสต์ GPU ถูกข้ามบน CI** (ไม่มี adapter บน runner) | เทสต์ที่แตะ texture จริง (`atlas_comes_back_…`, `working_cache_is_rebuilt_…`, `refilling_a_fresh_atlas_…`) ใช้ `headless_device()` ซึ่งคืน `None` เมื่อไม่มี GPU แล้ว **พิมพ์บอกว่าข้าม** (docs/08 §3.9 ข้อ 2) · บนเครื่องนักพัฒนาที่มี GPU มันรันจริงทุกครั้ง · จะให้ CI รันด้วยต้องลง software adapter (Ubuntu: `mesa-vulkan-drivers` = lavapipe · Windows runner มี WARP อยู่แล้ว) — **ยังไม่ได้ทำ** |
| ชั้น UI ของการกู้ device ยังไม่มี unit test | `recover_device()` ต้องมีหน้าต่างจริงจึงเรียกในเทสต์ไม่ได้ · ตอนนี้คุมด้วย (ก) `DeviceBound` ที่บังคับตอนคอมไพล์ (ข) `debug_assert` เทียบ generation ของ `WorkingCache` ทุกเฟรม (ค) รันจริงด้วย `--force-device-lost-after-ms` |
| **ผู้ถือลิขสิทธิ์ใน LICENSE เป็นชื่อผลิตภัณฑ์ ไม่ใช่บุคคล/นิติบุคคล** | `Copyright (c) 2026 RefX` · ตามกฎหมายผู้ถือลิขสิทธิ์ควรเป็นบุคคลหรือนิติบุคคล · ตอนนี้ repo เป็น private และไม่มี contributor คนอื่น จึงยังไม่มีผล → **ทบทวนก่อนเปิด public หรือก่อนรับ contributor คนแรก** |
| `refx-asset` / `refx-io` / `refx-ui` / `refx-app` ยังไม่เคยคอมไพล์บน Linux | cross-check จากเครื่อง Windows ติดที่ `libsqlite3-sys` กับ `zstd` ต้องใช้ C cross-compiler · `refx-core` / `refx-render` / `refx-platform` (ตัวที่มี `cfg(target_os)`) ผ่านแล้ว · ที่เหลือรอผล CI ฝั่ง ubuntu เป็นตัวยืนยัน |
| ข้อความ **CLI** กับ **crash dialog** ยังเป็นภาษาไทย | `tracing::` กวาดเป็นอังกฤษครบแล้ว (84 จุด) แต่ `--help` / error ของ CLI ใน `main.rs` และ `dialog::show_crash_dialog` ยังเป็นไทยล้วน · เป็นผิวที่ผู้ใช้ต่างชาติเจอได้เหมือนกัน แต่เป็นคนละระบบกับ log จึงแยกทำทีหลัง (crash dialog ควรไปอยู่ใต้ `refx-ui::text` ตอนที่ P5-3 ทำระบบภาษาเต็มรูปแบบ) |

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
