# 08 — Testing, Benchmarks & Definition of Done

หลักคิด: ข้อกำหนด "เสถียร + เบา" ที่ไม่มีตัววัดอัตโนมัติ = ความหวัง ไม่ใช่วิศวกรรม
ทุกข้อในเอกสารนี้ต้องเป็นสิ่งที่ CI ทำให้ fail ได้

---

## 1. ชั้นของการทดสอบ

| ชั้น | ที่ไหน | ทดสอบอะไร | ต้องเร็วแค่ไหน |
|---|---|---|---|
| Unit | `refx-core` | layout algorithm, command/undo, arena, geometry | < 2 s ทั้งหมด |
| Property | `refx-core` (`proptest`) | undo(apply(x)) == x, layout ไม่คืน NaN | < 30 s |
| Integration | `refx-io`, `refx-asset` | save/load round-trip, journal recovery, cache hit/miss | < 20 s |
| Fuzz | `fuzz/` | decoder, document parser, journal, layout | nightly |
| Golden image | `refx-render` | render off-screen เทียบภาพอ้างอิง | < 60 s |
| Benchmark | `benches/` | frame time, memory, startup | ทุก PR |
| Manual | checklist | device lost, drag&drop, ปิดเครื่องกลางคัน | ก่อน release |

### Property test ที่ต้องมี (คุ้มที่สุด)

```rust
// 1. undo คืนสภาพเดิมเป๊ะเสมอ — นี่คือหัวใจของ I-3
proptest! { fn undo_restores_exactly(cmds: Vec<AnyCommand>) {
    let before = board.snapshot();
    for c in &cmds { history.apply(c); }
    for _ in &cmds { history.undo(); }
    assert_eq!(before, board.snapshot());
}}

// 2. layout ไม่เคยคืนค่าที่ไม่ finite ไม่ว่า input จะบ้าแค่ไหน
proptest! { fn layout_always_finite(items: Vec<ItemAspect>, p: LayoutParams) {
    for (_, pos, size) in layout(&items, &p) {
        assert!(pos.is_finite() && size.is_finite() && size.x > 0.0);
    }
}}

// 3. save → load ได้ของเดิม
proptest! { fn roundtrip(board: Board) {
    assert_eq!(board, load(&save(&board)?)?);
}}

// 4. layout เป็น deterministic
proptest! { fn layout_deterministic(items: Vec<ItemAspect>, p: LayoutParams) {
    assert_eq!(layout(&items, &p), layout(&items, &p));
}}
```

---

## 2. Benchmark ที่ CI ต้องบังคับ

`benches/` ใช้ criterion + dataset สังเคราะห์ 1000 ภาพ (สร้างด้วย `xtask gen-testdata`)

| Bench | เพดาน | fail CI ถ้าเกิน |
|---|---|---|
| `frame_pan_1000` | 8.0 ms | ✅ |
| `frame_pan_1000_p99` | 16.0 ms | ✅ |
| `cull_1000` | 200 µs | ✅ |
| `build_instances_1000` | 300 µs | ✅ |
| `layout_justified_1000` | 2 ms | ✅ |
| `open_board_1000_warm_cache` | 1.5 s | ✅ |
| `startup_to_window` | 400 ms | ✅ |
| `rss_idle_1000` | 250 MB | ✅ |
| `vram_idle_1000` | 200 MB | ✅ |
| `binary_size` | 25 MB | ⚠️ warn |

> **ค่าปัจจุบัน (28 ก.ค. 2026): 15.35 MB → เหลือ 9.65 MB**
> ตัวเลข "12 MB" ในบันทึกเก่าผิด — วัดซ้ำจาก commit เดิมด้วย profile/toolchain เดียวกันได้ 15.26 MB
>
> **ต้องบังคับใน CI ไม่ใช่วัดด้วยมือ** — งบที่ไม่มีใครตรวจอัตโนมัติคืองบที่จะถูกละเมิดเงียบ ๆ
> (บทเรียนเดียวกับ §3.9) ให้ job หลัง `cargo build --release --all-features` วัดขนาดไฟล์
> แล้วล้มถ้าเกินเพดาน พร้อมพิมพ์ค่าปัจจุบันทุกครั้งเพื่อให้เห็นแนวโน้ม
>
> เรื่องนี้สำคัญเพราะ headroom ถูกใช้ไปกับ P2–P5 ที่ยังไม่ได้เขียน (working texture,
> file format, export, packaging) และเราปฏิเสธฟอนต์ CJK ไปแล้วโดยอ้างเพดานนี้

### ★ นโยบายการลดขนาด binary (28 ก.ค. 2026)

**ลำดับความสำคัญใน `CLAUDE.md` ยังบังคับอยู่: เสถียร → ปลอดภัย → เบา → ฟีเจอร์**
ขนาดไฟล์อยู่อันดับ **3** การลดขนาดที่แลกมาด้วยความเสถียรหรือความปลอดภัย = ไม่เอา

#### ห้ามเด็ดขาด

| ห้าม | เหตุผล |
|---|---|
| `panic = "abort"` | ทำให้ `catch_unwind` พัง → ภาพเสีย 1 ไฟล์ล้มทั้งโปรแกรม (**ผิด I-7**) มีคอมเมนต์เตือนไว้ใน `Cargo.toml` แล้ว |
| ตัด backend ของ wgpu จนเหลือตัวเดียว | เครื่องที่ driver มีปัญหาต้องมีทางถอย — Windows เก็บ **DX12 + Vulkan** · Linux เก็บ **Vulkan + GL** |
| ตัด format ภาพที่ผู้ใช้มีจริง | ดู `docs/05 §7` — เป็นการตัดฟีเจอร์ ไม่ใช่การ optimize ต้องถามก่อน |

#### ต้องวัดก่อนแก้เสมอ

`cargo bloat --release --crates` และ `--filter` ต่อ crate **ห้ามเดาว่าอะไรใหญ่**
เราเพิ่งเสียเวลาไปกับตัวเลข "12 MB" ที่ไม่มีใครวัด

#### เป้าที่ตรวจแล้วว่ามีจริง (เรียงตามความเสี่ยงต่ำ→สูง)

1. **dependency ที่ประกาศไว้แต่ไม่มีใครเรียก** — `zune-jpeg` (ยืนยันแล้วว่าไม่มีโค้ดไหนใช้)
   และ `memmap2` (ถอดออกจาก `refx-asset` แล้วตั้งแต่ห้าม mmap แต่ยังค้างใน `[workspace.dependencies]`)
   *ความเสี่ยง: ไม่มี* — ถ้า `image` ดึง `zune-jpeg` เข้ามาเองอยู่แล้ว ขนาดจะไม่ลด แต่ก็ควรลบเพื่อความชัดเจน
2. **backend ของ wgpu ที่ build ไม่ได้ใช้** — เช่น Metal บน Windows/Linux
   ตัดแบบ per-OS ได้โดยไม่เสียทางถอย · น่าจะเป็นก้อนใหญ่ที่สุด
3. **format ภาพที่แทบไม่มีใครใช้** — `tga` (แทบไม่เจอในงานนักวาด) และ `bmp`
   `tiff` **ให้เก็บไว้** (สแกนงานจริงใช้) และมันคือ decoder ที่ควร fuzz ที่สุด
   → ตัด `tga`/`bmp` ต้องแก้ `ALLOWED_FORMATS` + ข้อความ error + `docs/05 §7` พร้อมกัน
4. **`rusqlite` feature `bundled`** = คอมไพล์ SQLite (ภาษา C) เข้ามาทั้งก้อน
   น่าจะเป็นก้อนใหญ่อันดับต้น ๆ **แต่ห้ามเปลี่ยนไปใช้ SQLite ของระบบ** —
   บน Windows ไม่มีให้ใช้แน่นอน และการพึ่ง library ภายนอกทำลายความคาดเดาได้
   → **วัดว่ามันกินเท่าไหร่ แล้วรายงาน ยังไม่ต้องแก้**
5. `opt-level = "s"` หรือ `"z"` — **ทางเลือกสุดท้าย ต้องวัดคู่กับ benchmark เสมอ**
   decode ใช้เวลา ~81 ms/ไฟล์อยู่แล้ว ถ้าช้าลงอีกเพื่อประหยัดไม่กี่ MB ถือว่าขาดทุน

#### เกณฑ์ตัดสิน

รายงานเป็นตาราง **ก่อน/หลัง ทั้งขนาดและ benchmark** ทุกครั้ง
ถ้าเวลา decode หรือ frame time แย่ลงเกิน 5% ให้ถอยการเปลี่ยนนั้นออก ไม่ว่าจะประหยัดได้เท่าไหร่

**ตัวเลข regression:** ถ้า benchmark ช้าลง > 10% จากค่าบน main → บล็อก merge จนกว่าจะอธิบายได้ในคำอธิบาย PR

### ทดสอบ idle CPU = 0% (I-1)

ทดสอบด้วยเครื่องมืออัตโนมัติได้:

```rust
#[test]
fn idle_produces_no_redraw() {
    let mut app = headless_app_with(1000);
    app.pump_events(Duration::from_secs(5));   // ไม่มี input เลย
    assert_eq!(app.redraw_count_since_settled(), 0, "มีบางอย่างขอ redraw ตอน idle");
}
```

เทสต์นี้สำคัญมาก — เป็นตัวเดียวที่จับ "มีใครแอบเรียก `request_repaint()` ทุกเฟรม" ได้ก่อนที่จะสายเกินแก้

---

## 3. Manual test checklist (ก่อนทุก release)

**ความเสถียร**
- [ ] เปิด board 1000 ภาพ pan/zoom ต่อเนื่อง 3 นาที → ไม่กระตุก RAM ไม่ไต่ขึ้นเรื่อย ๆ
- [ ] อัปเดต GPU driver ระหว่างที่โปรแกรมเปิดอยู่ → กู้คืนได้ ไม่ crash
- [ ] Sleep แล้ว resume → กู้คืนได้
- [ ] สลับ iGPU ↔ dGPU (โน้ตบุ๊ก) → กู้คืนได้
- [ ] กด End Task ระหว่างแก้งาน → เปิดใหม่แล้วกู้งานคืนได้
- [ ] ถอด USB ที่เก็บภาพระหว่างใช้งาน → item เป็น Missing ไม่ crash
- [ ] เปิดไฟล์ภาพเสีย 20 ไฟล์รวด → ขึ้นเป็น Failed ทั้งหมด โปรแกรมยังใช้ได้
- [ ] ดิสก์เต็มตอน save → error ชัดเจน ไฟล์เดิมไม่เสียหาย

**ความปลอดภัย**
- [ ] `cargo tree | grep -iE "reqwest|hyper|tokio|curl|ureq|openssl|rustls|native-tls"` → ไม่มีผลลัพธ์
- [ ] `cargo tree -d | grep '^wgpu'` → ว่าง (ต้องมี `^` ไม่งั้นได้ false positive)
- [ ] `cargo deny check` ผ่าน
- [ ] เปิด decompression bomb (PNG 4 KB ประกาศ 60000×60000) → ปฏิเสธ ไม่ OOM
- [ ] เปิด `.refx` ที่แก้ไบต์มั่ว → error ชัดเจน ไม่ panic
- [ ] `.refx` ที่มี path `../../../etc/passwd` → ปฏิเสธ

**การใช้งาน**
- [ ] ลากภาพจาก Explorer/Finder/เบราว์เซอร์เข้าโปรแกรม
- [ ] `Ctrl+V` ภาพจาก clipboard
- [ ] สลับ Canvas ⇄ Arrange 20 ครั้ง → ข้อมูลทั้งสองฝั่งไม่เปลี่ยนเลย, `dirty` ไม่ถูกตั้ง
- [ ] Apply layout → `Ctrl+Z` ครั้งเดียวคืนสภาพเดิมทั้งหมด
- [ ] Undo/redo 100 ครั้งรวด → ไม่มี state เพี้ยน

---

> ★ **ข้อยกเว้นของกฎ "ทุก mutation ต้องผ่าน Command"** (2 ส.ค. 2026)
> `Board.view` (camera) persist ลงไฟล์แต่ **ไม่ผ่าน `Command` และไม่ทำให้ `dirty`**
> ส่วน `selection` **ถูกย้ายออกจาก `Board` ทั้งหมด** — เหตุผลครบใน `docs/02 §2.9`
> ข้อยกเว้นมีสองข้อนี้เท่านั้น เจอข้อที่สามให้หยุดถาม

## 3.9 ★ กฎเหล็ก: การตรวจทุกอย่างต้องพิสูจน์ได้ว่า "ล้มเป็น"

โปรเจกต์นี้โดนสัญญาณเขียวปลอมมาแล้ว **สองครั้ง** และทั้งสองครั้งกินเวลาหลาย session:

| เหตุ | อาการ | ทำไมไม่มีใครเห็น |
|---|---|---|
| canvas ว่างเปล่าตั้งแต่ P0-6 | เทสต์ 170 ตัวผ่าน · log บอกวาดสำเร็จ | ไม่มีเทสต์ไหนถามว่า "แล้วตาเห็นอะไร" |
| `fuzz_decode` เป็น stub 5 session | CI รัน cron ทุกคืน ผ่านทุกครั้ง | `fuzz/` อยู่ใน `exclude` ของ workspace → `clippy --all-targets` มองไม่เห็น |

**กฎที่ตามมา — ใช้กับทุก test / CI job / harness ที่เพิ่มใหม่:**

1. **ต้องพิสูจน์ว่ามันล้มเป็น** ก่อนเชื่อว่าที่มันผ่านคือของจริง
   วิธีที่ถูกคือใส่ **negative control** — จงใจทำให้พังชั่วคราว ดูว่ามันจับได้ แล้วถอดออก
   (ตัวอย่างที่ทำถูกในโปรเจกต์: `assert!(data.first() != Some(&0x89))` ใน `fuzz_decode`
   ยิง seed PNG แล้วยืนยันว่า harness ล้มพร้อมชี้บรรทัด)
2. **โครงเปล่าห้ามเงียบ** — target/เทสต์/job ที่ยังไม่ได้ต่อกับโค้ดจริง
   ต้องทำให้ CI **แดง หรือ ข้ามอย่างชัดเจน** ห้ามรันแล้วผ่านแบบว่างเปล่า
   *สัญญาณที่โกหกแย่กว่าไม่มีสัญญาณ* เพราะมันปิดโอกาสที่จะมีใครกลับมาดู
3. **ทุกไดเรกทอรีที่มี `Cargo.toml` ต้องถูก lint** ถึงจะอยู่นอก workspace ก็ตาม
   (`fuzz/` เคยหลุดเพราะ `exclude` — ปิดด้วย job `fuzz-lint` แล้ว)
4. **CI ต้องรันชุดเดียวกับเกณฑ์ในเครื่องเป๊ะ** ถ้า CI อ่อนกว่า คำว่า "CI เขียว"
   จะแปลว่าน้อยกว่าที่ทุกคนเข้าใจ โดยไม่มีใครรู้ตัว
5. **งานที่ผู้ใช้มองเห็น ต้องมีภาพหน้าจอจริงก่อนบอกว่าเสร็จ** (ดู `docs/03 §1`)
6. **ตัวเลขที่วัดครั้งเดียวแล้วจดไว้ จะเสื่อมสภาพเงียบ ๆ**
   ตัวอย่างจริง: "atlas เติมกลับ **100/100** ใน 0.99 ms" ถูกจดไว้ตอนวัดและเป็นความจริงตอนนั้น
   แต่พอเพิ่ม lazy allocation ทีหลัง atlas ที่เพิ่งสร้างมี 0 layer → ของจริงกลายเป็น **0/8**
   โดยไม่มีอะไรส่งเสียง ตัวเลขในเอกสารยังบอก 100/100 อยู่หลาย session
   → **คุณสมบัติที่สำคัญพอจะจดตัวเลขไว้ ต้องสำคัญพอที่จะมีเทสต์คุม**
   ตัวเลขในเอกสารคือ *ภาพถ่าย ณ เวลานั้น* ไม่ใช่คำรับประกัน
7. **ถ้าทั้ง matrix ของ CI ข้ามเทสต์กลุ่มหนึ่งพร้อมกัน = ไม่มีใครตรวจกลุ่มนั้นเลย**
   การข้ามพร้อมพิมพ์เหตุผล (ข้อ 2) ถูกต้องในระดับ job เดียว
   แต่ต้องมีตัวยืนยันว่า **อย่างน้อยหนึ่ง job รันจริง** ไม่งั้นได้เขียวโดยไม่มีความครอบคลุม

---

## 4. Definition of Done (ทุก PR)

PR จะ merge ได้ต่อเมื่อ **ครบทุกข้อ**:

1. `cargo build --release` ผ่านบน Windows + Linux
2. `cargo clippy --all-targets -- -D warnings` ไม่มี warning เลย
3. `cargo fmt --check` ผ่าน
4. `cargo test --all` ผ่าน
5. `cargo deny check` ผ่าน
6. Benchmark ที่เกี่ยวข้องไม่ถดถอยเกิน 10%
7. โค้ดใหม่ไม่มี `unwrap()` / `expect()` / `panic!()` บนเส้นทางที่ข้อมูลมาจากผู้ใช้หรือจากไฟล์
   *(ในเทสต์ใช้ได้ ในโค้ดที่เป็น invariant ภายในใช้ `expect("เหตุผลชัดเจน")` ได้)*
8. ไม่มี `unsafe` นอก `refx-platform`
9. ทุก path ที่แตะดิสก์ผ่าน `validate_path`
10. ทุก mutation ของ `Board` ผ่าน `Command` (ไม่มี `&mut board` หลุดออกไปที่อื่น)
11. ถ้าเพิ่ม image format → ต้องมี fuzz target มาด้วยใน PR เดียวกัน
12. ถ้าแก้ file format → ต้องมี migration + fixture ไฟล์เก่ามาด้วย

---

## 5. Logging

```rust
tracing::info!  // เหตุการณ์ระดับผู้ใช้: เปิด/บันทึกไฟล์, สลับ mode
tracing::warn!  // กู้คืนได้: decode ล้ม, ไฟล์หาย, cache miss ผิดปกติ
tracing::error! // เสียหาย: save ล้ม, device lost, DB เปิดไม่ได้
tracing::debug! // ปิดใน release
```

- Log ไปที่ `<cache_dir>/logs/` หมุนไฟล์ที่ 5 MB เก็บ 3 ไฟล์
  **`tracing-appender` 0.2 หมุนตามขนาดไม่ได้** (รองรับแค่ minutely/hourly/daily)
  ต้องเขียน writer เองแล้วห่อด้วย `non_blocking` — ห้ามใช้ daily แทน เพราะ error ที่วนซ้ำ
  ทำให้ไฟล์เดียวโตระดับ GB ได้ภายในวันเดียว
- **ห้ามเขียน log ลง stderr ใน release** — `windows_subsystem="windows"` ไม่มี console
  และการเขียน stderr เกิดบน UI thread (ผิด I-2)
- **cache eviction ห้ามแตะโฟลเดอร์ `logs/` เด็ดขาด** — crash log คือสิ่งเดียวที่ผู้ใช้มีให้ส่งเวลารายงานปัญหา
  ถ้าโดนล้างพร้อม thumbnail cache เราจะ debug ปัญหาของผู้ใช้ไม่ได้เลย
- **ห้ามมี path เต็มใน log ระดับ info** (มีชื่อผู้ใช้อยู่ในนั้น) ใช้ชื่อไฟล์อย่างเดียว
- ต้องมีเมนู "เปิดโฟลเดอร์ log" ให้ผู้ใช้ส่ง log เองเวลารายงานปัญหา

---

## 6. Profiling

- `puffin` หรือ `tracy` ผ่าน feature flag `profiling` (ปิดใน release)
- ต้อง profile ก่อน optimize เสมอ — **ห้าม optimize ตามความรู้สึก**
- แต่ละ frame แบ่ง scope: `cull`, `build_instances`, `upload`, `egui`, `submit`
- `heaptrack` (Linux) / `dhat` สำหรับหา memory leak — รันเดือนละครั้งเป็นอย่างน้อย
