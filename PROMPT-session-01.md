# Prompt สำหรับ Claude Code — Session 1 (P0-1 ถึง P0-5)

> วิธีใช้: เปิด Claude Code ในโฟลเดอร์นี้ แล้ว copy ทั้งบล็อกด้านล่างวางไปทีเดียว
> มี **STOP** 2 จุด — เมื่อถึงจุดนั้นมันจะหยุดรายงานให้คุณดูก่อนไปต่อ

---

```
คุณเป็น coder ของโปรเจกต์ RefX สถาปัตยกรรมออกแบบเสร็จแล้ว หน้าที่คุณคือ implement ตาม spec

## อ่านก่อนทำอะไรทั้งสิ้น
1. CLAUDE.md                     — กฎทั้งหมด โดยเฉพาะ invariant I-1 ถึง I-8
2. ARCHITECTURE.md §1, §2, §3    — invariants, โครง crate, threading model
3. docs/09-crate-versions.md     — ★ กับดักเวอร์ชัน อ่านให้จบก่อนแตะ Cargo.toml
4. docs/04-rendering.md §1, §7   — event-driven redraw + device lost recovery
5. ROADMAP.md ส่วน P0

## กฎเหล็กของ session นี้
- ห้ามแก้เวอร์ชัน crate ใน Cargo.toml เอง ถ้าคอมไพล์ไม่ผ่านเพราะเวอร์ชัน → หยุด รายงาน รอคำสั่ง
- ห้ามเพิ่ม dependency ที่ไม่มีใน docs/09-crate-versions.md
- ห้ามแก้ deny.toml, .github/, docs/ — ถ้าคิดว่า spec ผิด ให้หยุดแล้วบอก อย่าแก้เอง
- ห้ามทำ task ข้าม P0 ไปล่วงหน้า
- ทำทีละ task ให้ build ผ่านก่อนขึ้นตัวถัดไป

---

# ขั้นที่ 0 — เตรียมเครื่อง (ทำได้เลย ไม่ต้องขออนุญาต)

ตรวจและติดตั้งให้ครบ รายงานผลเป็นตารางว่าอะไรมีอยู่แล้ว/อะไรเพิ่งติดตั้ง:

    rustc --version          # ต้อง >= 1.87 (edition 2024)
    cargo --version
    rustup component add rustfmt clippy
    cargo install cargo-deny --locked
    cargo install cargo-nextest --locked

ถ้าไม่มี rustup เลย:
- Windows: ดาวน์โหลด https://win.rustup.rs แล้วรันแบบ -y
- Linux:   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

**เช็ค linker (ข้อนี้พลาดบ่อยที่สุดบน Windows):**
รัน `cargo build` เปล่า ๆ ถ้าเจอ error `link.exe not found` หรือ `linker not found`
→ **หยุดทันที** แล้วบอกผู้ใช้ว่าต้องติดตั้ง "Visual Studio Build Tools" พร้อม workload
"Desktop development with C++" จาก https://visualstudio.microsoft.com/visual-cpp-build-tools/
(คุณติดตั้งเองไม่ได้ ต้องใช้ GUI installer และสิทธิ์ admin)
Linux: ถ้าขาด ให้แนะนำ `sudo apt install build-essential libxkbcommon-dev libwayland-dev pkg-config`

---

# ขั้นที่ 1 — SPIKE ยืนยันเวอร์ชัน ★ STOP CHECKPOINT

นี่คืองานที่สำคัญที่สุดของ session ทั้งหมด อย่าเขียนอะไรเกินนี้

1. สร้าง `crates/refx-app/src/main.rs` แบบชั่วคราวที่ทำแค่:
   - winit 0.30 `ApplicationHandler` เปิดหน้าต่าง (สร้าง window ใน `resumed()` เท่านั้น)
   - wgpu instance/adapter/device/surface
   - render pass ที่ clear เป็นสีเดียว
   - egui + egui-wgpu วาดปุ่มเดียว บน device เดียวกันกับข้างบน
   - `event_loop.set_control_flow(ControlFlow::Wait)`

2. รัน แล้วรายงานผลทั้ง 4 ข้อนี้:

       cargo build 2>&1 | tail -30
       cargo tree -d | grep -i wgpu       # ★ ต้องว่าง ถ้าไม่ว่าง = ปัญหาใหญ่
       cargo tree | grep -iE "reqwest|hyper|tokio|curl|ureq|openssl"   # ต้องว่าง (I-8)
       cargo deny check 2>&1 | tail -20

3. **แล้วหยุด** รายงานให้ผู้ใช้เห็น:
   - เวอร์ชันจริงของ wgpu / egui / egui-wgpu / winit ที่ cargo resolve ให้
   - build ผ่านหรือไม่ ถ้าไม่ผ่าน error เต็ม ๆ คืออะไร
   - หน้าต่างเปิดขึ้นจริงหรือไม่ เห็นปุ่ม egui หรือไม่
   - GPU backend ที่ wgpu เลือก (Vulkan / DX12 / GL) และชื่อ adapter
   - `Features::TEXTURE_COMPRESSION_BC` รองรับหรือไม่ (มีผลกับ atlas ใน P1)

   ถ้าเวอร์ชันชนกันหรือ API ไม่ตรงกับที่ spec เขียนไว้ → **อย่าแก้เอง** รายงานอย่างเดียว

รอผู้ใช้สั่งต่อ

---

# ขั้นที่ 2 — P0-1 ถึง P0-4 (ทำต่อเมื่อผู้ใช้อนุมัติแล้ว)

**P0-1 workspace + lint + CI**
scaffold มีอยู่แล้วครบ ตรวจว่าทั้ง 4 คำสั่งนี้ผ่าน แล้วแก้เท่าที่จำเป็น:

    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings
    cargo nextest run
    cargo deny check

หมายเหตุ: `[workspace.lints.clippy]` ตั้ง `unwrap_used`/`expect_used` เป็น warn
ในไฟล์เทสต์ใส่ `#![allow(clippy::unwrap_used, clippy::expect_used)]` ได้ ในโค้ดจริงห้าม

**P0-2 refx-platform**
- `window.rs` — winit `ApplicationHandler` wrapper (ย้ายโค้ดจาก spike มา)
- `paths.rs` — cache/config/log dir ผ่าน `directories`
- `single_instance.rs` — กันเปิดซ้ำ (lock file ใน cache dir พอ)
- `dialog.rs`, `clipboard.rs` — วางโครง `todo!()` ไว้ ยังไม่ต้อง implement

**P0-3 refx-render::device**
- `RenderContext { instance, adapter, device, queue, surface, config }`
- เลือก surface format ที่เป็น sRGB จาก `surface.get_capabilities()` เสมอ
- `PresentMode::AutoVsync`
- ตรวจ + เก็บ flag ว่ารองรับ BC compression ไหม

**P0-4 event-driven loop + เทสต์ ★ อันนี้ห้ามลัด**
- `ControlFlow::Wait` เท่านั้น ห้ามมี `Poll` ที่ไหนเลย
- `request_redraw()` เรียกได้เฉพาะ 4 กรณีใน docs/04-rendering.md §1
- เขียน `RedrawTracker` ที่นับจำนวนครั้งที่ขอ redraw + เหตุผล
- เทสต์ headless:

      #[test] fn idle_produces_no_redraw()

  รันแอปแบบไม่มี window 5 วินาที ไม่ป้อน input เลย → `redraw_count` ต้องเป็น 0
  ถ้าเทสต์นี้ไม่ผ่าน ห้ามไปต่อ ให้หาสาเหตุจนเจอ

---

# ขั้นที่ 3 — P0-5 device lost recovery ★ STOP CHECKPOINT

spec เต็มอยู่ใน docs/04-rendering.md §7 ทำให้ครบทั้ง 4 ระดับ

1. จัดการ `SurfaceError` ครบทุก variant (Lost / Outdated / OutOfMemory / Timeout / Other)
2. `device.set_device_lost_callback` → ตั้ง flag → เฟรมถัดไปสร้าง instance/adapter/device/queue ใหม่ทั้งชุด
3. สร้าง pipeline + resource ใหม่หลังกู้ device
4. feature `force-device-lost` + flag จำลองสองแบบ (นับเฟรม / นับเวลา) — ดูหมายเหตุ P0-5 ใน ROADMAP.md

เสร็จแล้วรันทั้งสอง:

    cargo run --features force-device-lost -p refx-app -- --force-device-lost-after=120
    cargo run --features force-device-lost -p refx-app -- --force-device-lost-after-ms=8000

ต้องเห็นจอกระพริบครั้งเดียวแล้ววาดต่อได้ **ห้าม crash ห้ามค้าง**

**แล้วหยุด** รายงานสรุป:
- P0-1..P0-5 ผ่านกี่ข้อ
- ผล 4 คำสั่ง (fmt / clippy / nextest / deny)
- ผลเทสต์ `idle_produces_no_redraw`
- ผลทดสอบ device lost
- สิ่งที่ spec ไม่ได้ระบุแล้วคุณตัดสินใจเอง — ระบุว่าตัดสินใจอะไรและทำไม
- อะไรที่ spec เขียนไว้แล้วทำจริงไม่ได้ / ทำแล้วไม่เข้าท่า
```

---

## Prompt ต่อเนื่อง (ใช้หลังจาก session 1 ผ่าน)

**Session 2 — P0-6 ถึง P0-9**

```
อ่าน CLAUDE.md + docs/04-rendering.md ทั้งไฟล์ + docs/03-modes-and-ui.md §1
ทำ P0-6 ถึง P0-9 ใน ROADMAP.md

P0-6 ต้องผ่านเกณฑ์: วาด 10,000 quad ที่ 60 fps, instance buffer จองครั้งเดียวตอน init
     ห้ามสร้าง buffer/texture ใหม่ทุกเฟรม — ถ้าเห็นตัวเองกำลังเขียน create_buffer
     ในฟังก์ชันที่รันทุกเฟรม แปลว่าผิด

P0-7 ซูมต้องเข้าหาตำแหน่งเคอร์เซอร์ ไม่ใช่กลางจอ และต้องไม่มี jitter ที่ zoom
     สุดทั้งสองทาง (0.02 และ 32.0)

หลังทำเสร็จรัน cargo fmt / clippy -D warnings / nextest / deny ให้ผ่านครบ
แล้วรายงานผลพร้อม frame time ที่วัดได้จริง
```

**Session 3 — P1 asset pipeline**

```
อ่าน CLAUDE.md + docs/05-memory-and-assets.md + docs/06-security.md ทั้งสองไฟล์
ทำ P1-1 (decode_guarded) ก่อนอย่างเดียว ยังไม่ต้องทำ P1-2

P1-1 ต้องมี unit test ที่ยิง input พวกนี้แล้วได้ Err ทุกอัน ไม่มี panic ไม่มี OOM:
  - ไฟล์ว่าง / ไฟล์ 1 ไบต์ / bytes สุ่มล้วน
  - PNG header ที่ประกาศ 65535x65535 แต่ไฟล์ 4 KB  (decompression bomb)
  - JPEG ที่ตัดครึ่ง
  - ไฟล์ .png ที่จริง ๆ เป็น .exe (นามสกุลโกหก)
  - ภาพที่ใหญ่เกิน MAX_PIXELS พอดี ๆ และเกินไป 1 pixel

แล้วหยุด ให้ผมรีวิวเกราะก่อนไปทำ pipeline ที่เหลือ
```

---

## หมายเหตุ

- **VS Build Tools บน Windows ติดตั้งเองไม่ได้** ต้องใช้ GUI installer + สิทธิ์ admin — เตรียมไว้ก่อนเริ่มจะประหยัดเวลา
- ประโยค "ห้ามแก้เวอร์ชันเอง ให้หยุดรายงาน" สำคัญมาก ไม่งั้นมันจะ `cargo update` แล้วหลุดจาก spec โดยคุณไม่รู้ตัว
- ทุก session ให้จบด้วยคำถาม *"อะไรที่ spec เขียนไว้แล้วทำจริงไม่ได้"* — คนเขียน spec (ผม) ไม่ได้รันโค้ด ข้อมูลย้อนกลับตรงนี้มีค่าที่สุด
