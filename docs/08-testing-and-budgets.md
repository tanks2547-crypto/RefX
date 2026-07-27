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
