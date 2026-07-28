# fuzz — ยิง input ที่เชื่อไม่ได้ใส่เกราะทุกชั้น

> **บทเรียน 28 ก.ค. 2026 — อ่านก่อนแตะอะไรในโฟลเดอร์นี้**
>
> ทั้งสี่ target เคยเป็น `let _ = data;` และ `fuzz.yml` รันทุกคืนตี 3 **ผ่านทุกครั้ง**
> อยู่ 5 session เพราะไม่ได้เรียกอะไรเลย — `decode_guarded` ซึ่งเป็นผิวโจมตีหลัก
> ทั้งหมดตาม I-4 จึงไม่เคยถูก fuzz จริงสักครั้ง
>
> **สัญญาณที่โกหกแย่กว่าไม่มีสัญญาณ** เพราะมันทำให้เราหยุดมองไปเลย
>
> สาเหตุที่ไม่มีใครจับได้: `fuzz/` อยู่ใน `exclude` ของ workspace
> `cargo clippy --all-targets` ของ CI หลักจึงมองไม่เห็นโค้ดในนี้
> → แก้แล้วด้วย job `fuzz-lint` ใน `fuzz.yml`

## สถานะ target

| target | ยิงอะไร | สถานะ |
|---|---|---|
| `fuzz_decode` | `probe_dimensions` · `read_orientation` · `decode_guarded` | ✅ ต่อแล้ว |
| `fuzz_document` | `refx_io::dto` | ⛔ โค้ดยังไม่มี — **ไม่ลงทะเบียนเป็น `[[bin]]`** |
| `fuzz_journal` | `refx_io::journal` | ⛔ โค้ดยังไม่มี |
| `fuzz_layout` | `refx_core::layout` | ⛔ โค้ดยังไม่มี |

target ที่ยังไม่ต่อ **จงใจถอดออกจาก `Cargo.toml` และ matrix** เพื่อให้ CI ข้าม
อย่างชัดเจน แทนที่จะรันแล้วเขียวแบบว่างเปล่า

**เมื่อโค้ดที่มันควรยิงมีจริงแล้ว ต้องทำสามอย่างพร้อมกัน:**

1. ต่อ `fuzz_target!` ให้เรียกโค้ดนั้น
2. ลงทะเบียน `[[bin]]` กลับใน `fuzz/Cargo.toml`
3. ใส่ชื่อ target กลับใน matrix ของ `.github/workflows/fuzz.yml`

job `unwired-targets` จะ **ล้ม** ถ้าโมดูลปลายทางมีโค้ดแล้วแต่ target ยังไม่ถูกต่อ

## corpus

- `fuzz/seeds/<target>/` — **commit ไว้ใน repo** เคสตั้งต้นที่คัดมาจากเทสต์จริง
  (decompression bomb, ไฟล์ถูกตัด, .exe ที่ตั้งชื่อเป็น .png, JPEG ที่มี EXIF จริง)
- `fuzz/corpus/<target>/` — ของที่ fuzzer งอกเอง **อยู่ใน `.gitignore`** (เป็นพันไฟล์)

สร้าง seed ใหม่: `python fuzz/seeds/make_seeds.py` (รันจากรากโปรเจกต์)

## รันในเครื่อง

```bash
rustup toolchain install nightly
cargo install cargo-fuzz --locked

mkdir -p fuzz/corpus/fuzz_decode
cp -n fuzz/seeds/fuzz_decode/* fuzz/corpus/fuzz_decode/
cargo +nightly fuzz run fuzz_decode -- -max_total_time=900
```

### ★ กับดักบน Windows: `STATUS_DLL_NOT_FOUND` (0xc0000135)

`cargo fuzz` บน MSVC ลิงก์กับ AddressSanitizer แบบ dynamic แต่ **ไม่ได้ใส่ path
ของ runtime DLL ให้** ตัว target จึงตายทันทีที่เริ่มรันโดยไม่มีข้อความอธิบาย

ต้องเติม path ของ `clang_rt.asan_dynamic-x86_64.dll` เข้า `PATH` ก่อน:

```bash
export PATH="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/<version>/bin/Hostx64/x64:$PATH"
```

(บน Linux ไม่มีปัญหานี้ ซึ่งเป็นเหตุผลที่ CI ใช้ `ubuntu-latest`)

## เจอ crash แล้วทำยังไง

1. **หยุด อย่าเพิ่งแก้** — บันทึกไฟล์ที่ทำให้ล้มไว้ก่อน (`fuzz/artifacts/<target>/`)
2. รันซ้ำเคสเดียว: `cargo +nightly fuzz run fuzz_decode fuzz/artifacts/fuzz_decode/<ไฟล์>`
3. ย่อ input ให้เล็กที่สุด: `cargo +nightly fuzz tmin fuzz_decode <ไฟล์>`
4. เอา input ที่ย่อแล้วไปเป็น **เทสต์ถาวร** ใน crate ที่เกี่ยวข้อง แล้วค่อยแก้โค้ด
   — ไม่งั้นบั๊กเดิมกลับมาได้โดยไม่มีอะไรเตือน
