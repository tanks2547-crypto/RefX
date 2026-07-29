# 06 — Security

หลักคิด: โปรแกรมนี้เปิดไฟล์ที่ผู้ใช้โหลดมาจากอินเทอร์เน็ต วันละหลายร้อยไฟล์
**ทุกไฟล์คือ input ที่ควบคุมโดยผู้โจมตี** (I-4) นั่นคือ threat model ทั้งหมด

---

## 1. Threat model

| # | ภัยคุกคาม | ผลกระทบ | มาตรการ |
|---|---|---|---|
| T1 | ภาพที่ถูกสร้างมาเพื่อโจมตี decoder | RCE | pure-Rust decoder เท่านั้น → กลายเป็น panic ไม่ใช่ RCE + `catch_unwind` |
| T2 | Decompression bomb | หน่วยความจำหมด, เครื่องค้าง | ตรวจ limit จาก header **ก่อน** allocate |
| T3 | ไฟล์ `.refx` ที่ถูกดัดแปลง | path traversal, OOM, panic | deserialize แบบมี bound + ตรวจ path ทุกอัน |
| T4 | Symlink / junction ชี้ออกนอกโฟลเดอร์ | อ่านไฟล์ที่ไม่ควรอ่าน | canonicalize + ตรวจว่ายังอยู่ใน root ที่อนุญาต |
| T5 | Zip-slip ตอนเปิด .refx แบบ packed | เขียนไฟล์ทับที่อื่นในระบบ | ห้าม extract ลงดิสก์ อ่านจาก memory เท่านั้น |
| T6 | Dependency ที่ถูกแทรกโค้ดร้าย | RCE | `cargo-deny` + `cargo-vet` + lockfile + ตรึงเวอร์ชัน |
| T7 | ข้อมูลผู้ใช้รั่วออกเน็ต | ความเป็นส่วนตัว | **ไม่มี network stack ใน binary เลย** — พิสูจน์ได้ด้วย dependency tree |
| T8 | Cache DB ถูกดัดแปลง | โหลด thumbnail ผิด / panic | ถือ cache เป็น untrusted เช่นกัน: ตรวจขนาด blob ก่อนใช้, เสียหาย = ลบทิ้ง |

**สิ่งที่ไม่อยู่ใน threat model v1:** multi-user, sandbox escape, side-channel, การป้องกันผู้ใช้ที่มีสิทธิ์ admin บนเครื่องตัวเอง

---

## 2. กฎเหล็กเรื่อง decoder

> **ห้ามมี C/C++ codec ใน dependency tree เด็ดขาด**

CVE ที่มีผลกระทบสูงสุดในโปรแกรมประเภทนี้ตลอด 10 ปีที่ผ่านมา เกือบทั้งหมดมาจาก libpng / libjpeg-turbo / libwebp / libtiff — ล้วนเป็น heap overflow ใน C
Pure-Rust decoder เปลี่ยนช่องโหว่ระดับ RCE ให้เหลือแค่ panic ที่เรา catch ได้

บังคับใน `deny.toml`:

```toml
[bans]
multiple-versions = "warn"
deny = [
  { name = "libwebp-sys" }, { name = "libwebp-sys2" },
  { name = "mozjpeg-sys" }, { name = "jpeg-sys" },
  { name = "libpng-sys" },  { name = "png-sys" },
  { name = "libtiff-sys" }, { name = "openjpeg-sys" },
  { name = "libavif-sys" }, { name = "dav1d-sys" },
  { name = "openssl-sys" }, { name = "openssl" },   # ไม่มี network = ไม่ควรมีอันนี้
  { name = "reqwest" }, { name = "hyper" }, { name = "curl" }, { name = "ureq" },
  { name = "tokio" },                                # ไม่ใช้ async runtime
]
```

`image` crate: ปิด default features แล้วเปิดเฉพาะที่ต้องการ

```toml
image = { version = "0.25", default-features = false,
          features = ["png", "jpeg", "webp", "gif", "bmp", "tga", "tiff"] }
```

---

## 2.5 ★ หนี้ความปลอดภัยที่ค้างอยู่จริง (ตรวจ 28 ก.ค. 2026)

### ★★ `fuzz_decode` เป็น stub เปล่า — CI รายงานเขียวมาตลอดโดยไม่ได้ fuzz อะไรเลย

`fuzz/fuzz_targets/fuzz_decode.rs` ยังเป็น `let _ = data;` พร้อม `TODO(P1-1)`
ทั้งที่ **P1-1 เสร็จไปตั้งแต่ 5 session ก่อน**
`.github/workflows/fuzz.yml` รันทุกคืนตี 3 เป็นเวลาหลายวัน ผ่านทุกครั้ง — เพราะไม่ได้เรียกอะไร

นี่คือความล้มเหลวแบบเดียวกับ canvas ว่างเปล่า: **สัญญาณเขียวที่ไม่ได้ตรวจอะไรจริง**
และรอบนี้อันตรายกว่า เพราะ `decode_guarded` คือ **ผิวโจมตีหลักทั้งหมดของโปรแกรม** ตาม I-4

→ ต้องต่อ target ให้เรียก `decode_guarded` จริง **ทันที ไม่ต้องรอ P4-9**
→ target อื่น (`fuzz_document` `fuzz_journal` `fuzz_layout`) ให้ตรวจด้วยว่าเป็น stub เหมือนกันไหม
   ถ้าโค้ดที่มันควรเรียกยังไม่มี ให้ CI **ข้ามอย่างชัดเจน** ไม่ใช่รันแล้วผ่านแบบว่างเปล่า
→ เก็บ corpus เริ่มต้นจากไฟล์ทดสอบที่มีอยู่แล้ว (bomb, truncated, นามสกุลโกหก)

### `cargo deny` ตรวจ advisories เฉพาะตอน push

CVE ใหม่ถูกประกาศ**หลังจาก** commit สุดท้ายเสมอ repo ที่ไม่มีใครแตะสองสัปดาห์
= สองสัปดาห์ที่ไม่มีใครรู้ว่ามีช่องโหว่ใหม่ใน dependency
→ เพิ่ม schedule รายสัปดาห์ให้ job `deny` (มี `fuzz.yml` เป็นตัวอย่างอยู่แล้ว)

### ฟอนต์ที่ดาวน์โหลดมาฝัง = ผิวโจมตีของ supply chain

ไฟล์นี้จะถูกฝังใน binary ของผู้ใช้ทุกคน ถ้าต้นทางถูกแทรกแซงตอนที่เราดึง เราแจกต่อให้ทุกคน

1. **ปักหมุดที่ release tag ที่ระบุได้** ห้ามดึงจาก branch ที่ขยับได้
2. **บันทึก SHA-256 ของไฟล์ลง repo** (`assets/fonts/CHECKSUMS.txt`) พร้อมวันที่และ URL เต็ม
   ใครดึงใหม่ต้องได้ค่าเดิม ไม่ตรง = หยุด
3. **โหลดฟอนต์ต้องไม่ panic** ถึงจะเป็นไฟล์ของเราเองก็ตาม —
   ถ้า parse ไม่ผ่านให้ตกกลับไปใช้ฟอนต์เดิมของ egui แล้ว `warn!`
   โปรแกรมที่เปิดไม่ขึ้นเพราะฟอนต์ แย่กว่าโปรแกรมที่ตัวหนังสือไทยเป็น `□`

---

## 3. เกราะรอบการ decode

```rust
pub fn decode_guarded(mmap: &[u8], limits: &Limits) -> Result<RgbaImage, LoadError> {
    // 1. ขนาดไฟล์
    if mmap.len() as u64 > limits.max_file_bytes { return Err(LoadError::TooLarge); }

    // 2. format จาก magic bytes — ไม่เชื่อนามสกุลไฟล์
    let fmt = image::guess_format(mmap).map_err(|_| LoadError::UnknownFormat)?;
    if !limits.allowed_formats.contains(&fmt) { return Err(LoadError::FormatNotAllowed); }

    // 3. อ่านขนาดจาก header ก่อน allocate ★ กัน T2
    let (w, h) = image::ImageReader::with_format(Cursor::new(mmap), fmt)
        .into_dimensions().map_err(|_| LoadError::BadHeader)?;
    if w > limits.max_dimension || h > limits.max_dimension { return Err(LoadError::TooLarge); }
    if (w as u64) * (h as u64) > limits.max_pixels { return Err(LoadError::TooLarge); }

    // 4. เกราะ panic — บั๊กใน decoder ต้องไม่ล้มโปรแกรม (I-7)
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        let mut r = image::ImageReader::with_format(Cursor::new(mmap), fmt);
        r.limits(image::Limits {           // เกราะชั้นสองของ image crate เอง
            max_image_width:  Some(limits.max_dimension),
            max_image_height: Some(limits.max_dimension),
            max_alloc:        Some(limits.max_alloc),
            ..Default::default()
        });
        r.decode()
    }))
    .map_err(|_| LoadError::DecoderPanic)?      // ← panic กลายเป็น Err ธรรมดา
    .map(|img| img.into_rgba8())
    .map_err(LoadError::from)
}
```

**`catch_unwind` ไม่ได้ปิดปาก panic hook** — hook ของ P0-9 ยังทำงานก่อน unwind ทุกครั้ง
โฟลเดอร์ที่มีไฟล์เสีย 500 ไฟล์ = backtrace 500 ชุดลง log → หมุนทะลุ 5 MB → **ทับ crash log จริงหายหมด**
ซึ่งขัดกับกฎใน docs/08 §5 ที่ว่า log คือสิ่งเดียวที่ผู้ใช้มีให้ส่งเวลารายงานปัญหา
→ ต้องมี thread-local flag ตอนอยู่ในเกราะ decode ให้ hook ข้ามการเขียน backtrace
   แล้วบันทึกเป็น `warn!` บรรทัดเดียวพร้อมชื่อไฟล์แทน

ต้องตั้ง `panic = "unwind"` ใน release profile (ไม่ใช่ `abort`) ไม่งั้น `catch_unwind` ใช้ไม่ได้ — **นี่คือการแลกที่คุ้ม**: binary ใหญ่ขึ้นเล็กน้อย เพื่อให้ภาพเสีย 1 ไฟล์ไม่ทำให้ผู้ใช้เสียงานทั้ง session

### ★ ห้าม memory-map ไฟล์ของผู้ใช้ (ตัดสิน 27 ก.ค. 2026)

เอกสารฉบับแรก (docs/05 §2 ข้อ 1) สั่งให้ `memmap` ไฟล์เพื่อ zero-copy — **ยกเลิกกฎนั้น**

เหตุผล: ถ้าไฟล์ที่ map ไว้ถูก **ตัดสั้นลงหรือหายไประหว่างที่เรายังถืออยู่**
การอ่าน page นั้นจะได้ **SIGBUS** ซึ่งเป็น *signal* ไม่ใช่ panic
→ **`catch_unwind` จับไม่ได้ เกราะชั้น 4 ทั้งชั้นไร้ผล** โปรเซสตายทันทีโดยไม่มีทางกู้

นี่ไม่ใช่เคสทฤษฎี — กลุ่มผู้ใช้ของเราคือนักวาดที่เก็บโฟลเดอร์ reference ไว้บน
**OneDrive / Dropbox / Google Drive / ไดรฟ์นอก / NAS** เป็นเรื่องปกติ
พวกนี้ตัดและเขียนไฟล์ทับระหว่าง sync ตลอดเวลา ผลคือ mood board ที่จัดมา 3 ชั่วโมงหายไป
เพราะ Dropbox sync ไฟล์พอดี — ผู้ใช้ไม่มีทางเข้าใจว่าเกิดอะไรขึ้น และไม่มีโอกาสที่สอง

**ให้ทำแทน:** `metadata()` เช็คขนาดก่อน → ถ้าเกิน `max_file_bytes` ปฏิเสธตั้งแต่ยังไม่เปิด
→ แล้ว `read()` เข้า `Vec<u8>` ธรรมดา

ต้นทุนคือ memcpy ไฟล์หนึ่งครั้ง (ปกติ 2–20 MB ≈ 1 ms) เทียบกับตัว decode ที่จอง
w×h×4 ไบต์อยู่แล้ว แทบไม่ต่าง — **แลกกับการที่โปรแกรมไม่ตายจากไฟล์ที่ขยับใต้เท้าเรา คุ้มเกินคุ้ม**
ตรงตามลำดับความสำคัญใน CLAUDE.md: เสถียร มาก่อน กิน RAM น้อย

> `memmap2` จึงถูกถอดออกจาก dependency ของ `refx-asset`
> ถ้าจะเอากลับมาใช้กับไฟล์ `.refx` ของเราเอง (ที่เราคุมวงจรชีวิตได้) ต้องถามก่อน

**Timeout:** worker เก็บเวลาเริ่ม ถ้า job ไหนเกิน 20 s ให้ mark เป็น `Failed(Timeout)` (ยกเลิก decode กลางคันไม่ได้ในทางปฏิบัติ แต่กันไม่ให้ทั้งคิวค้าง และเตือน user ได้)

---

## 4. การตรวจสอบ path

```rust
pub fn validate_path(candidate: &Path, roots: &[PathBuf]) -> Result<PathBuf, SecError> {
    // 1. ปฏิเสธ component ที่น่าสงสัยก่อนแตะระบบไฟล์
    for c in candidate.components() {
        if matches!(c, Component::ParentDir) { return Err(SecError::Traversal); }
    }
    // 2. canonicalize → คลาย symlink/junction/8.3 name ทั้งหมด
    let real = candidate.canonicalize()?;
    // 3. ต้องอยู่ใต้ root ที่ผู้ใช้อนุญาตจริง ๆ
    if !roots.iter().any(|r| real.starts_with(r)) { return Err(SecError::OutsideRoot); }
    Ok(real)
}
```

- Root = โฟลเดอร์ที่ผู้ใช้เปิดเอง หรือโฟลเดอร์ของไฟล์ `.refx` เท่านั้น
- Windows: ระวัง `\\?\`, UNC path (`\\server\share`), ชื่อสงวน (`CON`, `PRN`, `NUL`, `COM1`…) — `canonicalize` จัดการส่วนใหญ่ แต่ต้องปฏิเสธ UNC ที่ผู้ใช้ไม่ได้เปิดเองอย่างชัดเจน
- ไฟล์ `.refx` ที่มาจากคนอื่น: path ภายในต้องเป็น **relative เท่านั้น** absolute path ในไฟล์ที่ได้รับมา = ปฏิเสธและถามผู้ใช้ว่าจะ relink ที่ไหน

---

## 5. Deserialize `.refx` อย่างปลอดภัย

```rust
// ห้าม: bincode::deserialize(&bytes)   ← Vec ที่ประกาศ len 4 พันล้าน = OOM ทันที
// ใช้:  postcard พร้อม bound ทุกคอลเลกชัน
const MAX_ITEMS: usize    = 100_000;
const MAX_BOARDS: usize   = 256;
const MAX_STRING: usize   = 4096;
const MAX_TAGS: usize     = 1024;
const MAX_DOC_BYTES: u64  = 256 << 20;
```

- ตรวจ magic + version ก่อนอ่านอย่างอื่น เวอร์ชันที่ใหม่กว่าที่รู้จัก = ปฏิเสธพร้อมข้อความชัดเจน (ไม่ใช่พยายามอ่านมั่ว)
- ค่า float ทุกตัวต้องตรวจ `is_finite()` — `NaN` ในตำแหน่งภาพจะทำให้ layout พังทั้ง board แบบหาสาเหตุยาก
- clamp ทุกค่าตัวเลขให้อยู่ในช่วงที่ถูกต้อง (`opacity` 0..1, `zoom` 0.02..32, `pos` ±1e6)
- ID ที่อ้างถึง item ที่ไม่มีอยู่ = ข้ามอย่างเงียบ ๆ + log ไม่ใช่ panic

---

## 6. Supply chain

```
cargo deny check          # advisories + licenses + bans + sources
cargo audit               # RustSec
cargo vet                 # ตรวจสอบ dependency ใหม่ด้วยมือ
```

- `Cargo.lock` commit เสมอ (นี่เป็น binary ไม่ใช่ library)
- ตรึง dependency ด้วยเวอร์ชันชัดเจน ไม่ใช้ `*` หรือ `>=`
- `cargo update` ทำเป็น PR แยก พร้อม changelog ที่อ่านแล้วจริง ๆ
- License allowlist: MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0 — เจอ GPL/AGPL = fail CI

---

## 7. Fuzzing (`fuzz/`)

| Target | input | ตรวจอะไร |
|---|---|---|
| `fuzz_decode` | bytes มั่ว | decoder ต้องคืน `Err` ไม่ใช่ hang/OOM/crash |
| `fuzz_document` | `.refx` มั่ว | parser ต้องคืน `Err` ไม่ใช่ panic/OOM |
| `fuzz_journal` | journal มั่ว | recovery ต้องไม่ทำให้เสียหายไปมากกว่าเดิม |
| `fuzz_layout` | ค่า item สุ่ม (รวม NaN, inf, 0, ค่าติดลบ) | layout ต้องไม่ panic และไม่คืน NaN |

รันใน CI แบบ nightly อย่างน้อย 15 นาทีต่อ target เก็บ corpus ไว้ใน repo แยก
**ทุก crash ที่เจอต้องกลายเป็น unit test ถาวร**

---

## 8. Binary hardening

```toml
# .cargo/config.toml
[target.'cfg(target_os = "windows")']
rustflags = ["-C", "control-flow-guard=yes"]

[target.'cfg(target_os = "linux")']
rustflags = ["-C", "relocation-model=pie", "-C", "link-arg=-Wl,-z,relro,-z,now"]
```

```toml
# Cargo.toml
[profile.release]
opt-level     = 3
lto           = "fat"
codegen-units = 1
panic         = "unwind"   # ★ จำเป็นสำหรับ catch_unwind — ห้ามเปลี่ยนเป็น abort
strip         = "symbols"
debug         = false

[profile.release-debug]    # สำหรับ profiling
inherits = "release"
debug    = 1
strip    = "none"
```

---

## 9. ความเป็นส่วนตัว

- **ไม่มี telemetry ไม่มี analytics ไม่มี crash reporter อัตโนมัติ** crash log เขียนลงเครื่องผู้ใช้เท่านั้น พร้อมปุ่ม "เปิดโฟลเดอร์ log" ให้ส่งเองถ้าอยากส่ง
- log ห้ามมีเนื้อหาไฟล์ ห้ามมี path เต็มใน log ระดับ info (ใช้ชื่อไฟล์อย่างเดียว) — path เต็มมีชื่อผู้ใช้อยู่ในนั้น
- ตรวจสอบได้: `cargo tree | grep -iE "reqwest|hyper|tokio|curl|ureq|socket2"` ต้องไม่คืนอะไรเลย ทำเป็น step ใน CI
