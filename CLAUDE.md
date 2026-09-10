# CLAUDE.md — กฎสำหรับ coder agent

โปรเจกต์: **RefX** — โปรแกรมจัดการภาพ reference สำหรับนักวาด (Rust + wgpu + egui)

---

## อ่านก่อนเขียนโค้ดบรรทัดแรก

1. [`ARCHITECTURE.md`](ARCHITECTURE.md) — โดยเฉพาะ **§1 Invariants** ทั้ง 8 ข้อ
2. [`docs/09-crate-versions.md`](docs/09-crate-versions.md) — **wgpu 30 ใช้กับ egui ไม่ได้ ต้องใช้ 29.0.4**
3. [`ROADMAP.md`](ROADMAP.md) — หา task ถัดไป ทำทีละ task ห้ามข้าม phase
4. spec ของส่วนที่กำลังทำใน `docs/`

---

## ลำดับความสำคัญ (ห้ามสลับ)

```
1. เสถียร   →  2. ปลอดภัย  →  3. กิน CPU/RAM น้อย  →  4. ฟีเจอร์
```

เจอทางแยกเมื่อไหร่ ให้เลือกตามลำดับนี้เสมอ
โค้ดที่เร็วขึ้น 5% แต่เพิ่มโอกาส crash = **ไม่เอา**

---

## Invariants ที่ห้ามละเมิด (สรุปจาก ARCHITECTURE §1)

| # | กฎ | วิธีตรวจ |
|---|---|---|
| I-1 | Idle = 0% CPU. `ControlFlow::Wait` เท่านั้น | เทสต์ `idle_produces_no_redraw` |
| I-2 | UI thread ห้ามบล็อก — ไม่มี fs / decode / DB | code review + profiler |
| I-3 | ข้อมูลผู้ใช้ห้ามหาย — Command + journal + atomic save | property test undo, kill test |
| I-4 | ทุกไฟล์คือ input ที่ไม่น่าไว้ใจ | fuzz + limit |
| I-5 | `#![forbid(unsafe_code)]` ทุก crate ยกเว้น `refx-platform` | คอมไพเลอร์ |
| I-6 | ทุก cache มี hard cap + LRU | สถานะบน status bar |
| I-7 | ภาพเสีย 1 ไฟล์ ห้ามล้มโปรแกรม | `catch_unwind` ต่อ decode job |
| I-8 | ไม่มี network เลย | `cargo tree \| grep -iE 'reqwest\|hyper\|tokio\|curl\|ureq\|openssl'` ต้องว่าง |

---

## กฎการเขียนโค้ด

### ห้ามเด็ดขาด

```rust
❌ .unwrap() / .expect() / panic!()  บนเส้นทางที่ข้อมูลมาจากผู้ใช้หรือไฟล์
❌ unsafe  นอก refx-platform
❌ std::fs::read()  บน UI thread
❌ device.create_texture()  โดยไม่ผ่าน TextureAllocator
❌ แก้ Board โดยไม่ผ่าน Command
❌ สร้าง buffer/texture ใหม่ทุกเฟรม
❌ HashMap iteration order ในการคำนวณ layout   // ทำให้ผลไม่ deterministic
❌ panic = "abort" ใน release profile          // ทำให้ catch_unwind พัง
❌ เพิ่ม dependency ที่มี C codec หรือ network
❌ ControlFlow::Poll
❌ ส่งผลจากเธรดอื่นโดยไม่มีคนปลุก event loop   // docs/08 §3.9 ข้อ 18
```

### ★★★ ห้ามใช้ PowerShell `Get-Content` / `Set-Content` แก้ไฟล์ซอร์ส

Windows PowerShell 5.1 อ่านไฟล์ที่ไม่มี BOM ด้วย **codepage ANSI** และเขียนกลับ
เป็น UTF-8 → ข้อความไทยทั้งไฟล์ถูกเข้ารหัสซ้อนสองชั้น และได้ CRLF + BOM แถมมา
(เกิดจริง 8 ก.ย. 2026 กับ `crates/refx-ui/src/export.rs` ทั้งไฟล์)

```powershell
❌ (Get-Content f.rs) -replace 'a','b' | Set-Content f.rs
❌ Set-Content / Out-File / Add-Content  กับไฟล์ .rs .md .toml .wgsl
✅ ใช้เครื่องมือแก้ไฟล์ของ editor  หรือ  awk/sed ผ่าน Bash
```

**ถ้าพลาดไปแล้ว:** อ่านไฟล์เป็น UTF-8 → เข้ารหัสสตริงนั้นกลับด้วย CP1252 →
เขียนเป็นไบต์ดิบ · แล้ว **ยืนยันด้วย `git diff` ว่าเหลือแต่การแก้ที่ตั้งใจ**
และ `git ls-files --eol` ต้องไม่มี `w/crlf`

### ต้องทำเสมอ

```rust
✅ Result<T, E> + thiserror  ทุกอย่างที่ล้มเหลวได้
✅ ตรวจ header ก่อน allocate เสมอตอน decode
✅ validate_path() ก่อนแตะไฟล์ทุกครั้ง
✅ ทุก mutation → Command ที่ undo ได้
✅ ค่า float จากไฟล์ → ตรวจ is_finite() + clamp
✅ งานหนักไป worker + มี cancellation token
✅ คอมเมนต์อธิบาย "ทำไม" ไม่ใช่ "ทำอะไร"
```

### เรื่อง error message

ข้อความ error ที่ผู้ใช้เห็นต้องบอก **สิ่งที่เกิดขึ้น + สิ่งที่ทำได้ต่อ**

```
❌ "Error: InvalidData"
✅ "เปิด reference.refx ไม่ได้: ไฟล์เสียหาย (checksum ไม่ตรง)
    ลองไฟล์สำรอง reference.refx.bak ที่อยู่ในโฟลเดอร์เดียวกัน"
```

---

## ก่อน commit ทุกครั้ง

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo deny check
cargo tree -d | grep '^wgpu'     # ต้องว่าง (^ สำคัญ — ไม่งั้นจับ leaf crate อื่นติดมาด้วย)
cargo tree -d | grep '^png'      # ต้องว่าง — เราตรึง png ตรง ๆ ต้องตรงกับที่ image ใช้
```

ครบทั้ง 13 ข้อใน [`docs/08-testing-and-budgets.md`](docs/08-testing-and-budgets.md) §4 จึงจะ merge ได้

---

## วิธีทำงานที่ต้องการ

- **ทำทีละ task ใน ROADMAP** ให้จบและเทสต์ผ่าน แล้วค่อยขึ้นตัวถัดไป ห้ามเขียนหลายโมดูลค้างไว้พร้อมกัน
- **เขียนเทสต์ไปพร้อมโค้ด** ไม่ใช่ทีหลัง โดยเฉพาะ `refx-core` ที่ต้องเทสต์ได้โดยไม่มี GPU
- **เจอสิ่งที่ spec ไม่ได้ระบุ** → เลือกทางที่ปลอดภัย/เสถียรกว่า แล้ว **บันทึกไว้ใน PR** ว่าตัดสินใจอะไรและทำไม
- **เจอสิ่งที่ spec ขัดกันเอง หรือ spec ผิด** → หยุด ถาม ไม่ต้องเดา
- **ห้าม refactor นอกขอบเขต task** ที่กำลังทำ

## สิ่งที่ต้องหยุดถามก่อนทำ

- เพิ่ม dependency ใหม่ที่ไม่มีใน `docs/09-crate-versions.md`
- เปลี่ยน file format หรือ schema ของ cache DB
- แตะ threading model
- อะไรก็ตามที่ทำให้ Invariant ข้อใดข้อหนึ่งทำไม่ได้ตามเดิม

---

## บริบทของโดเมน

ผู้ใช้คือ **นักวาด/นักออกแบบ** ไม่ใช่โปรแกรมเมอร์ สิ่งที่พวกเขาสนใจ:

- ภาพต้องขึ้น **ทันที** ที่ลากเข้ามา — รอ 2 วินาทีคือช้าเกินไปแล้ว
- **ห้ามทำงานหาย** งาน mood board ที่จัดมา 3 ชั่วโมงหายไปเพราะ crash = เลิกใช้ทันที ไม่มีโอกาสที่สอง
- โปรแกรมเปิดค้างไว้ **ทั้งวัน** ข้าง ๆ Photoshop/Clip Studio → ห้ามแย่ง RAM/CPU กับโปรแกรมหลักของเขา
- Grayscale / flip / opacity ไม่ใช่ของเล่น — เป็นเครื่องมือทำงานจริงที่ใช้ทุกวัน
- ความรู้สึกว่า "เชื่อถือได้" มาจากรายละเอียดเล็ก ๆ: pan ที่ไม่กระตุก, ภาพที่ไม่กระพริบหาย, undo ที่คืนสภาพได้ตรงเป๊ะทุกครั้ง
