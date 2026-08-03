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
| **แยกชั้น `refx-asset`** (§2.0 เดิม) | ✅ เสร็จ (`41c2693`) — ดู §2.0 ด้านล่าง |
| **P2-1** `Arena`/`Board`/`Item` | ✅ เสร็จ (`43b76d5`) |
| **P2-2** `Command` + `History` + merge/seal | ✅ เสร็จ (`43b76d5`) — **ผ่านรีวิวแล้ว** |
| **P2-3** `SpatialIndex` + hit-test | ✅ เสร็จ (`7a81b4e`) |
| **ย้าย `refx-ui` ไปใช้ `Board`** | ✅ เสร็จ (`0cc1e76`) — เทียบเส้นฐานครบ ไม่มีตัวไหนแย่ลงเกิน 5% |
| **P2-4** select / rubber-band / multi-select | ✅ **เสร็จสมบูรณ์** (`dd51d60` · `439f380` · `a6ac127` · `0f6071c`) — รวม Ctrl+Z/Ctrl+Y ยืนยันด้วยภาพหน้าจอ |
| P2-5 move/scale/rotate + snap | ← **เริ่มที่นี่** |
| **`selection` ย้ายออกจาก `Board`** (docs/02 §2.9) | ✅ เสร็จ (`439f380`) — คลิกดูภาพไม่ทำให้เอกสาร dirty อีกต่อไป |
| P2-5 ขึ้นไป | ยังไม่เริ่ม |

**เกณฑ์คุณภาพล่าสุดที่ผ่าน:** `fmt` / `clippy --all-features -D warnings` (workspace + `fuzz/`) /
`nextest 383/383` / `deny check` ครบ 4 หมวด / **binary 15.46 MB จากเพดาน 25 MB (บังคับใน CI)**

crate ที่มี: `refx-app` `refx-asset` `refx-core` `refx-io` `refx-platform` `refx-render` `refx-ui`

### สถานะ CI (มีชีวิตแล้วตั้งแต่ 1 ส.ค. 2026)

repo: **`github.com/tanks2547-crypto/RefX` (private)** · branch `main`

| | windows-latest | ubuntu-latest |
|---|---|---|
| GPU ที่ใช้ตรวจจริง | **WARP** (Dx12/Cpu) | **lavapipe** (Vulkan/Cpu) |
| เทสต์ | 383/383 | 383/383 |
| binary | 15.52 MB (เครื่องพัฒนา) | — |

**`Fuzz (nightly)` เขียวครั้งแรก 2 ส.ค. 2026** — ทั้งสาม job (`fuzz_decode` ยิงเต็ม 15 นาที ·
`unwired-targets` · `fuzz-lint`) บน `rustc 1.99.0-nightly (73dc9167f)` ดูเหตุใน §5 ข้อ false green

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

### 2.0 ✅ แยกชั้น `refx-asset` ออกจาก `refx-platform` (เสร็จ `41c2693`)

เก็บไว้เพราะ **ผลลัพธ์เป็นข้อผูกมัดต่อไป** — ห้ามใส่ `refx-platform` กลับเข้า `refx-asset`

เดิม `fuzz/` → `refx-asset` → `refx-platform` → `rfd` → `ashpd` → `zbus` ทำให้ fuzz
ต้องคอมไพล์ทั้งกอง GUI ด้วย nightly แล้วแตกที่ `zbus` (ไม่ใช่โค้ดเรา)

**ข้อเสนอเดิมในไฟล์นี้ผิดบางส่วน:** เขียนว่าย้าย `panic_guard` อย่างเดียวแล้ว
"dependency หายทั้งเส้น" — ของจริง `refx-asset` ใช้จาก `refx-platform` **สามอย่าง**

| | เดิม | ตอนนี้ |
|---|---|---|
| `panic_guard` | `refx-platform` | `refx-core::panic_guard` (std ล้วน) |
| `total_ram()` | เรียกเองใน `with_defaults` | ผู้เรียกส่งเข้ามา (`refx-ui`) |
| clipboard | เรียก `arboard` ผ่าน `refx-platform` | DTO + trait `ClipboardReader` อยู่ `refx-core` · ตัวที่คุย `arboard` อยู่ `refx-platform` · `refx-ui` เสียบให้ |

หลักการเดียวกับ `WakeHandle` ที่ทำให้ `refx-asset` ไม่รู้จัก `winit` อยู่ก่อนแล้ว
ต่างกันแค่ใช้ trait แทน closure เพราะมี error type ที่สองฝั่งต้องแชร์

**ผลข้างเคียงที่ ARCHITECTURE §2 บันทึกไว้แล้ว:** `refx-platform` ไม่ใช่ leaf อีกต่อไป
(พึ่ง `refx-core`) กราฟยังเป็น DAG ทิศทางเดียว

ยืนยันด้วย `cargo tree`: `refx-asset` และ `fuzz/` ไม่เหลือ `rfd`/`winit`/`arboard`/`refx-platform`

### 2.1 ✅ P2-1 + P2-2 (เสร็จ `43b76d5` ผ่านรีวิว 2 ส.ค. 2026)

`Arena` + `ItemId`/`GroupId`/`BoardId` · `Board`/`Item`/`ItemCanvas`/`ItemMeta`/`AssetRef` ·
`Selection` · `ViewState` · `Command` + `History` + merge/seal + `AddItems`/`RemoveItems`/
`TransformItems`/`ReorderZ`/`EditMeta`

**สิ่งที่ต้องรู้ก่อนเขียน `Command` ตัวใหม่** (รายละเอียดอยู่ใน doc comment ของ `command.rs`):

1. `apply` ที่คืน `Err` **ต้องไม่แตะ board เลย** — คำสั่งที่แตะหลาย item ย้อนสิ่งที่ทำไปแล้วคืนก่อน
2. `undo` คืนสภาพ **เป๊ะ**: `ItemId` เดิม · ชั้น z เดิม · ธง `dirty` เดิม (`selection` ไม่อยู่ใน `Board` แล้ว — docs/02 §2.9)
3. **redo ต้องได้ `ItemId` ชุดเดิม** ไม่งั้นคำสั่งถัดไปในสาย redo ชี้ไปที่ว่าง
   (นี่คือเหตุผลที่ `Arena::insert_at` มีอยู่ และเหตุผลที่ไม่ใช้ `slotmap`)
4. `heap_size()` **ไม่มีค่าเริ่มต้น** — ต้องเขียนเองทุกตัว ไม่งั้นเพดาน 64 MB ไม่นับคำสั่งใหม่
5. `affected()` **ไม่มีค่าเริ่มต้น** เช่นกัน — บอกว่าคำสั่งแตะ item ไหน เพื่อให้ชั้น editor
   ตั้ง selection ตามหลัง undo/redo (การเลือกไม่ได้ถูก undo มันตามผลลัพธ์)

### 2.2 ★★ งานถัดไป: ย้าย `refx-ui` มาใช้ `refx_core::Board`

**นี่คือสิ่งที่ขวาง P2-4 ครึ่งหลังอยู่ และไม่มีใครเคยลงมันไว้ใน ROADMAP**

`refx-ui` ยังเก็บสถานะเองเป็น `Vec<QuadInstance>` คู่ขนานกับ `Vec<BoardItem>`
(`app.rs` — มี TODO เขียนไว้ตั้งแต่ P1 ว่า "P2 จะแทนที่ด้วย `Board`/`Item` ตัวจริง")
ตราบใดที่ยังเป็นแบบนี้ ของที่ P2-1…P2-4 สร้างไว้ **ไม่มีทางไปถึงผู้ใช้เลย**

สิ่งที่พร้อมใช้แล้วและรอ caller อยู่:

| ของ | อยู่ที่ | ใครควรเรียก |
|---|---|---|
| `Board` / `Item` / `Command` / `History` | `refx-core` | `RefxApp` แทน `board_items` + `quads` |
| `SpatialIndex` | `refx_core::spatial` | culling ต่อเฟรม + hit-test |
| `SelectTool` (+ `Selection` ที่ editor ถือเอง) | `refx_core::interact` | ตัวแปลง pointer ของ egui |

> ⚠️ **ของที่เทสต์ครบแต่ไม่มีใครเรียก คือสภาพเดียวกับที่ทำให้ `fuzz_decode`
> เป็น stub อยู่ 5 session** ต่างกันแค่ตรงนี้ไม่ได้ *โกหก* ว่าทำงานอยู่
> ถ้ารอบหน้าไม่ได้ต่อให้ครบ ต้องเขียนไว้ตรงนี้ว่าทำไม อย่าปล่อยเงียบ

**ลำดับที่แนะนำ** — ★ **แก้ 2 ส.ค. 2026 หลังไปดูโค้ดจริง:**

> ข้อ 1–3 ที่เคยเขียนไว้ว่าแยกกันได้ **แยกไม่ได้จริง** ต้องลงพร้อมกันเป็นก้อนเดียว
>
> เหตุผล: ตอนนี้ `quads` **เป็นแหล่งความจริงของเรขาคณิตอยู่คนเดียว** — ตำแหน่ง/ขนาด
> ถูกคำนวณสด ๆ ตอน ingest (`app.rs` ราวบรรทัด 767: ตาราง 16 คอลัมน์ + สเกลจาก
> `thumb.source_width`) ไม่ได้เก็บไว้ที่ไหนอีก การให้ `RefxApp` ถือ `Board` ไว้เฉย ๆ
> โดยที่ `quads` ยังคำนวณเองอยู่ = **แหล่งความจริงสองที่** ซึ่งคือปัญหาที่กำลังจะแก้พอดี

1. ✅ **เสร็จแล้ว** (`0cc1e76`) — `Gfx` ถือ `Board`+`History`+`SpatialIndex` ·
   ingest ผ่าน `AddItems` · `quads` สร้างจาก `items_in_z_order()` ที่ `rebuild_quads`
   จุดเดียว · `ItemRender` เป็น side map คีย์ด้วย `ItemId`
   > `Board`/`History` อยู่ใน `Gfx` ซึ่งเป็นบ้านที่ไม่ตรงความหมาย (`Gfx` คือของที่ผูก
   > กับ device) — ไม่ใช่บั๊กเพราะ `recover_device()` แก้ฟิลด์ทีละตัว ไม่ได้สร้าง `Gfx`
   > ใหม่ทั้งก้อน แต่ควรย้ายออกตอน P4-7 multi-board
2. ✅ **เสร็จแล้ว** (`a6ac127`) — `allocate_response` + `SelectTool` + วาดกรอบเลือก/rubber-band
   > `egui_owns_pointer` ที่เดาเอาถูกลบทิ้งแล้ว · pan ย้ายไป **ปุ่มกลาง**
   > (ปุ่มซ้ายเป็นของการเลือก · space จะชนกับ text note ของ P2-11)
3. ✅ **เสร็จแล้ว** (`0f6071c`) — Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z เข้า `History` ·
   id จาก `affected()` ไปตั้ง selection · **กล้องเลื่อนไปหาสิ่งที่ถูกย้อนถ้ามันนอกจอ**
   > ★ §2.2 ปิดครบแล้ว — ของที่ P2-1…P2-4 สร้างไว้มีคนเรียกครบทุกตัว
   > **งานถัดไปคือ P2-5** move / scale (handle) / rotate + snap

### 2.2b ★ audit: enum ที่จะถูก serialize (ทำครั้งเดียว 3 ส.ค. 2026)

docs/02 §2.2.5 บังคับว่า **ทุก enum ที่ลง `.refx` หรือ `cache.sqlite` ต้องมี variant สำรอง**
เจอมาแล้วสองตัวแบบไล่ทีละตัวตอนกำลังเขียนโค้ดทับ — นี่คือการกวาดทั้งหมดพร้อมกัน

**ยังไม่มีตัวไหนถูก serialize จริง** เพราะ `refx-io::dto` ยังเป็นไฟล์ TODO บรรทัดเดียว
ตัวที่ยังไม่มีคนใช้จึง **ไม่ต้องเขียนล่วงหน้า** (โครงเปล่าที่ docs/08 §3.9 ข้อ 2 ห้าม)
— บันทึกไว้ว่ารู้แล้วและจะจัดการยังไงตอนถึงคิว (P4-1 file format)

| enum | ลงไฟล์ผ่าน | มีทางออกแล้ว | แผน |
|---|---|---|---|
| `ImageFormat` | `AssetRef.format` | ✅ `Unknown` | ทำแล้ว (`0cc1e76`) |
| `MissingReason` | `ItemKind::Missing.reason` | ✅ `Unknown` | ทำแล้ว (`43b76d5`) |
| `Flip` | `ItemCanvas.flip` | ❌ | ค่าที่ไม่รู้จัก → `Flip::None` (เสียการพลิก แต่ภาพยังอยู่) **ต้องเพิ่มตอน P4-1** |
| `ColorLabel` | `ItemMeta.color_label` | ⚠️ `Option` ช่วยได้ครึ่งเดียว | ค่าที่ไม่รู้จักตกเป็น `None` = **ป้ายสีของผู้ใช้หายเงียบ ๆ ตอน round-trip** · ทางที่ถูกคือ DTO เก็บเลขดิบไว้แล้วเขียนกลับตามเดิม ตัดสินตอน P4-1 |
| `ItemKind` | `Item.kind` | ❌ | ★ **ไม่ต้องมี `Unknown` ของตัวเอง** — kind ที่ไม่รู้จักให้ตกเป็น `Missing { reason: Unknown }` ซึ่งมีอยู่แล้วและถูกต้องตามความหมาย (item ยังอยู่บน board ผู้ใช้เห็นว่ามีของ) |
| `SortKey` | `ArrangeState.sort` | ❌ | ค่าที่ไม่รู้จัก → `AddedAt` (ค่าเริ่มต้น) ไม่มีข้อมูลผู้ใช้หาย |
| `Mode` | `ViewState.mode` | ❌ | ค่าที่ไม่รู้จัก → `Canvas` ไม่มีข้อมูลผู้ใช้หาย |
| `ThumbFormat` | `cache.sqlite` | ✅ (คนละแบบ) | `from_i64` คืน `None` → **นับเป็น cache miss ไม่ใช่ error** ซึ่งถูกต้องกว่ามี variant สำรอง: cache หายไม่ใช่งานหาย |

**ไม่เข้าข่าย** (เป็น error type / runtime เท่านั้น ไม่ลงไฟล์): `ArenaError` `BoardError`
`CmdError` `ClipboardError` `ClipboardContent` `CanvasButton` `CanvasEvent` `MetaField`
`Slot<T>` · `ItemFilter` เป็น struct ของ bool/f32 ไม่ใช่ enum จึงไม่มีปัญหานี้

### 2.3 กับดัก pointer ของ egui — ถึงเวลาแก้ที่ต้นเหตุแล้ว

`docs/03 §1` บันทึกทางแก้ชั่วคราวที่ใช้อยู่ (`egui_is_using_pointer()` +
`canvas.contains(cursor)`) พร้อมเงื่อนไขว่า **ทางที่ถูกคือทำ canvas เป็น widget จริง
ด้วย `ui.allocate_response(rect, Sense::click_and_drag())` แล้วขับทุกอย่างจาก response นั้น**

P2-4 คือจุดที่มันคุ้มแล้ว เพราะปุ่มซ้ายต้องเปลี่ยนหน้าที่จาก "pan" ไปเป็น
"เลือก/ลากกรอบ" ซึ่งแปลว่าต้องรื้อการตัดสินใจเรื่อง pointer อยู่ดี
และ `docs/03 §1` เตือนไว้ว่า **ถ้าเอา widget ของ egui ไปวางในช่อง canvas
ก่อนจะแก้ตรงนี้ เราจะแย่ง event นั้นไป**

ปุ่มกลาง = pan (กล้องไม่ใช่สถานะของ board จึงไม่ผ่าน `Command`)

> ⚠️ `crates/refx-core/src/layout.rs` ยังเป็นไฟล์ TODO บรรทัดเดียวโดยตั้งใจ —
> job `unwired-targets` ใน `fuzz.yml` จะ **แดงทันที** ที่มันมีโค้ดเกิน 2 บรรทัด
> จนกว่าจะต่อ `fuzz_layout` ให้ครบสามขั้นตามที่ job นั้นพิมพ์บอก (เรื่องเดียวกันกับ
> `refx-io::dto` และ `refx-io::journal`)

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
| 18 | **`refx-asset` ห้าม depend `refx-platform`** (ตัดสิน 2 ส.ค. 2026) | ชั้น asset ที่รู้จัก windowing/dialog/clipboard ลาก `rfd`→`ashpd`→`zbus` เข้า fuzz จนคอมไพล์ nightly ไม่ผ่าน · ของที่ต้องถาม OS ให้ **ผู้เรียกส่งเข้ามา** หรือกลับทิศด้วย trait ใน `refx-core` | `refx-asset/Cargo.toml` (มีคอมเมนต์ห้ามไว้) + `refx-core::clipboard` |
| 19 | **เขียน `Arena` เอง ห้ามกลับไปใช้ `slotmap`** | `slotmap` ไม่มี API ใส่ของกลับที่คีย์เดิม (คีย์ที่ลบแล้วตายถาวร) → undo ของ "ลบภาพ" จะคืน `ItemId` **ใหม่** แล้ว `z_order`/`Selection`/`ItemMeta::group` ที่ถือคีย์เก่าจะห้อยหมด = ภาพกลับมาแต่ลำดับและการเลือกหาย · ทำ I-3 ไม่ได้ตั้งแต่ต้น | `refx-core::arena::Arena::insert_at` |
| 20 | **`Arena`/`Selection` `PartialEq` เทียบสิ่งที่ผู้ใช้สัมผัสได้ ไม่ใช่โครงข้างใน** | `Arena` เทียบ **คีย์+ค่าของสิ่งที่มีชีวิต** ไม่ใช่ `slots` ดิบ — นับช่องว่างท้าย vec ด้วยจะทำให้ undo ของ "เพิ่มภาพ" ไม่มีวันคืนสภาพได้ ทั้งที่นั่นคือการวัด*ตัวจัดสรร* · ส่วน `Selection` เทียบ **ตามลำดับ** ไม่ใช่แบบเซต เพราะ anchor ของ align ขึ้นกับลำดับคลิก | `arena.rs` / `selection.rs` + เทสต์คู่ที่อธิบายทั้งสองทิศ |

---

## 5. เรื่องที่ spec เคยผิดแล้วแก้แล้ว (อย่าเชื่อเอกสารเก่าที่จำมา)

- `docs/05 §6` เคยเขียนว่า 1000 ภาพ cache เย็น ≈ **4 วินาที** → ของจริง **~80 วินาที** (ผิด 20 เท่า เพราะเดาจากภาพเล็ก)
- `docs/03 §1` เคยใช้ `SidePanel` / `TopBottomPanel` / `CentralPanel::show` ซึ่ง egui 0.34 **deprecate หมดแล้ว**
  ของจริง: `egui::Panel::top/left/right/bottom` + `.show_inside(ui)` + `default_size`
  **`egui::Panel::center` ไม่มีจริง** ใช้ `CentralPanel::default().show_inside(ui, …)`
- `docs/09` มีตาราง wgpu 29 API delta: `push_constant_ranges`→`immediate_size`,
  `bind_group_layouts` รับ `&[Option<&_>]`, `multiview`→`multiview_mask`
- `tracing-appender` 0.2 **หมุนไฟล์ตามขนาดไม่ได้** (มีแค่ตามเวลา) ต้องเขียน writer เอง
- `docs/02 §2` เคยให้ `refx-core` ถือ `ImageFormat` กับ `LoadError` ของ crate `image`
  ซึ่ง **ทำไม่ได้และไม่เคยทำได้** (`refx-core` depend `image` ไม่ได้ · `image::ImageError`
  serialize ไม่ได้ แต่ §7 บังคับให้ทุกอย่างใน `Board` ลง DTO ได้) → แก้แล้วที่ **§2.2.5**:
  core นิยาม enum เอง ส่วน `impl From<&LoadError> for MissingReason` อยู่ `refx-asset` จุดเดียว
  **`MissingReason::Unknown` ต้องมีเสมอ** — ไฟล์จากรุ่นใหม่กว่าอาจมีเหตุผลที่รุ่นนี้ไม่รู้จัก
  อ่านเจอต้องตกมาที่นั่น ห้าม error ทิ้งทั้งไฟล์ (I-3) · **กฎนี้ใช้กับทุก enum ที่ลงไฟล์**
- `docs/02 §5` เคยเขียน zoom `0.02..=32.0` ทั้งที่โค้ดใช้ `0.01..=64.0` มาตั้งแต่ P0-7
  → แก้เอกสารให้ตรงโค้ด (เอกสารเป็นฝ่ายผิด)
- `HANDOFF §2.0` เดิมเขียนว่าย้าย `panic_guard` อย่างเดียวแล้ว dependency หายทั้งเส้น
  → ของจริงมีสามอย่าง (ดู §2.0)

### ★ false green ตัวที่สี่: `Fuzz (nightly)` ไม่เคยรันบน nightly เลย (2 ส.ค. 2026)

หลังแยกชั้น `refx-asset` แล้ว `fuzz-lint` เขียว แต่ job `fuzz` ยังแดง — **คนละสาเหตุกับ `zbus`**
log บอกตรง ๆ ว่า rustc ที่ถูกเรียกคือ `1.92-x86_64-unknown-linux-gnu`

`rust-toolchain.toml` ที่รากตรึง `channel = "1.92"` ไว้ และ **ไฟล์นั้นชนะ**
`rustup default nightly` ที่ `dtolnay/rust-toolchain` ตั้งให้ ผลคือ:

- job `fuzz` ล้มเสียงดัง เพราะ `cargo fuzz` ต้องใช้ `-Zsanitizer` (ดีแล้ว)
- job `fuzz-lint` **ผ่านแบบเงียบ ๆ บน stable** ทั้งที่หน้าที่เดียวของมันคือตรวจว่า
  โค้ดคอมไพล์บน nightly ได้ → **ชื่อของ job บอกตรงข้ามกับสิ่งที่มันทำ**

แก้: `RUSTUP_TOOLCHAIN: nightly` ระดับ workflow (ชนะ toolchain file ตามลำดับของ rustup)
**ไม่ใช่การปักหมุดวันที่** — ยังเป็น nightly ล่าสุดเสมอ

> ★ **นี่คือครั้งที่สี่ของรูปแบบเดิม** (canvas ว่าง · `fuzz_decode` stub · เพดาน binary
> ที่ไม่มีใครตรวจ · toolchain ที่ไม่ตรงชื่อ) และ **ทั้งสี่ครั้งเจอเพราะมีอย่างอื่นแตก
> แล้วไปตามต่อ ไม่ใช่เพราะมีใครไปตรวจว่าตัวตรวจทำงานจริงไหม**
>
> ทางแก้ที่ใช้ได้คือทำให้สภาพผิดเป็น **ความล้มเหลวที่ส่งเสียง** ไม่ใช่ความเงียบ —
> ทั้งสอง job จึงมีขั้น `rustc --version | grep nightly` ที่แดงถ้า toolchain หลุด
> **เพิ่ม job/harness ใหม่เมื่อไหร่ ให้ถามก่อนว่า "ถ้ามันไม่ได้ทำงาน จะรู้ได้ยังไง"**

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
| ~~เทสต์ GPU ถูกข้ามบน CI~~ | ✅ **ปิดแล้ว 1 ส.ค. 2026** — ทั้งสอง runner รันจริง (WARP / lavapipe) และ `REFX_REQUIRE_GPU=1` ทำให้ "ไม่มี adapter" = แดง ดู §1 |
| ชั้น UI ของการกู้ device ยังไม่มี unit test | `recover_device()` ต้องมีหน้าต่างจริงจึงเรียกในเทสต์ไม่ได้ · ตอนนี้คุมด้วย (ก) `DeviceBound` ที่บังคับตอนคอมไพล์ (ข) `debug_assert` เทียบ generation ของ `WorkingCache` ทุกเฟรม (ค) รันจริงด้วย `--force-device-lost-after-ms` |
| **ผู้ถือลิขสิทธิ์ใน LICENSE เป็นชื่อผลิตภัณฑ์ ไม่ใช่บุคคล/นิติบุคคล** | `Copyright (c) 2026 RefX` · ตามกฎหมายผู้ถือลิขสิทธิ์ควรเป็นบุคคลหรือนิติบุคคล · ตอนนี้ repo เป็น private และไม่มี contributor คนอื่น จึงยังไม่มีผล → **ทบทวนก่อนเปิด public หรือก่อนรับ contributor คนแรก** |
| ~~`refx-asset` / `refx-io` / `refx-ui` / `refx-app` ยังไม่เคยคอมไพล์บน Linux~~ | ✅ **ปิดแล้ว 1 ส.ค. 2026** — job `check (ubuntu-latest)` build ครบทุก crate + รันเทสต์ + วัดขนาด binary (17.01 MB) |
| **★ enum ที่ลงไฟล์ต้อง round-trip ได้ ไม่ใช่แค่ทนได้** (ยกระดับกฎ 3 ส.ค. 2026) | `ColorLabel` เป็นตัวอย่างที่ชัดที่สุด: ผู้ใช้ติดป้ายสีด้วยรุ่นใหม่ → เปิดด้วยรุ่นเก่า → อ่านเป็น `None` → ขยับภาพใบเดียวแล้วบันทึก → **ป้ายสีหายถาวรโดยไม่มีอะไรเตือน** กลไกที่สร้างมากัน I-3 กลายเป็นตัวทำให้ข้อมูลหายเสียเอง · ทางแก้: ค่าเดี่ยว ๆ (`ColorLabel`/`Flip`/`ImageFormat`) เก็บ**ค่าดิบ**ใน DTO แล้วเขียนกลับตามเดิม UI แสดงว่า "ไม่รู้จัก" ได้แต่ห้ามแปลงค่าทิ้ง · variant ใหม่ของ `ItemKind` round-trip ไม่ได้จริง ต้องกันที่ระดับไฟล์: **major version สูงกว่าที่เข้าใจ → เปิดอ่านอย่างเดียว** แล้วบอกให้ผู้ใช้อัปเดตก่อน (ยอมให้แก้ไม่ได้ชั่วคราว ดีกว่าปล่อยให้บันทึกทับแล้วงานหาย) · **งานจริงตอน P4-1 (dto)** |
| **คอลัมน์ `format` ใน `cache.sqlite` ไม่เคยมีความหมาย** | `pool.rs` เขียน `0` ตายตัวมาตลอด และ `AssetRef.format` ตอนนี้เป็น `ImageFormat::Unknown` เพราะ cache hit ไม่ได้แตะไบต์ของไฟล์ · งานที่ต้องทำ: ร้อย format จริงจาก `image::guess_format` ผ่าน `decode` → `Thumbnail` → `ThumbEntry` **แล้วตัดสินพร้อมกันว่าจะให้ความหมายคอลัมน์นี้หรือลบทิ้ง** (แตะ schema ของ cache = ต้องถามก่อนตาม CLAUDE.md) |
| **`render_state` ไม่เคยถูกล้าง** | undo ของการเพิ่มภาพเอา item ออกจาก `Board` แต่ `ItemRender` (thumbnail 64 KB) ยังค้างอยู่ **โดยตั้งใจ** — redo ต้องใช้มันคืนภาพ · แต่เมื่อ `History` ตัดคำสั่งเก่าทิ้งตามเพดาน item นั้นกลับมาไม่ได้อีกแล้ว thumbnail จึงค้างเปล่า ๆ · ตอนนี้ขอบเขตคือ "จำนวนภาพที่เคยเพิ่มในเซสชัน" ซึ่งอยู่ในงบ 64 MB ที่ documented ไว้อยู่แล้ว → **แก้ตอนทำ P2-6 (ลบภาพจริง)** พร้อมกับตัดสินว่าใครเป็นเจ้าของอายุของ thumbnail |
| **ชื่อขั้น undo ยังไม่ถูกแปล** | `Command::label()` คืน `&'static str` อังกฤษ (`"Add items"` ฯลฯ) แต่ `refx-ui::text` ยังไม่มีตารางแปลให้ · ต้องทำตอนที่ UI มีเมนู/ปุ่ม undo จริง (P2-4 ขึ้นไป) — อย่าเอา `label()` ไปแสดงตรง ๆ กับผู้ใช้ |
| **การเลือกเข้า undo stack — ต้องรีวิว UX** | `SelectItems` เป็น `Command` เพราะ `selection` อยู่ใน `Board` และ docs/08 §4 ข้อ 10 บังคับ · ยุบเป็นขั้นเดียวด้วย `merge` แล้ว แต่ผลคือ **กด Ctrl+Z แล้วการเลือกกลับมาด้วย** ซึ่งโปรแกรมส่วนใหญ่ไม่ทำ · ทางเลือกอื่นคือเจาะรูให้แก้ `Board` ได้โดยไม่ผ่าน `Command` ซึ่งทำให้กฎที่คอมไพเลอร์บังคับอยู่กลายเป็นกฎที่ต้องจำเอง — **ยังไม่ตัดสิน รอเห็นของจริงบนจอก่อน** |
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
