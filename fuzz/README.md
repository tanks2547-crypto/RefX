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

| target | ยิงอะไร | ต่อเมื่อ |
|---|---|---|
| `fuzz_decode` | `probe_dimensions` · `read_orientation` · `decode_guarded` | P1-1 |
| `fuzz_layout` | `refx_core::layout` ทั้งห้าตัว | P3-2 |
| `fuzz_document` | `refx_io::dto` — หัวไฟล์ · zstd · postcard | P4-1 |
| `fuzz_packed` | `packed::read_index` · `extract` · ที่อยู่ที่ `spool` จะเขียน | **P4-9** |

**ครบทั้งสี่แล้ว ทุกตัวยิงโค้ดจริง** (P4-9)

### ★ `fuzz_journal` ถูกถอดทิ้ง ไม่ใช่ยังไม่ได้ทำ (P4-9)

มันเล็งไปที่ `refx_io::journal` ซึ่ง **จะไม่มีวันถูกเขียน** — P4-3 เปลี่ยนจาก
command journal เป็น snapshot ไปแล้ว (`docs/07 §4`) · target ที่รอโมดูลซึ่ง
จะไม่มีวันเกิด คือ target ที่ไม่มีวันได้ทำงาน และเป็นช่องว่างที่ *ดูเหมือน*
มีคนวางแผนไว้แล้ว ซึ่งหลอกให้ไม่มีใครกลับมามอง

`fuzz_packed` มาแทนเพราะมันคือ **parser ไบนารีตัวเดียวที่เหลือซึ่งไม่เคยถูก
fuzz แตะเลย** — และช่องนี้ใหญ่กว่าที่คิดเพราะ `fuzz_document::wrap()` เขียน
`flags = 0` เสมอ **ไม่มี input ไหนเคยตั้งบิต packed เลยสักครั้ง**
(รายละเอียดใน `ROADMAP P4-9` และหัวไฟล์ `fuzz_targets/fuzz_packed.rs`)

> ★ มันจับบั๊กจริงได้ใน **60 วินาทีแรกที่มันเคยรัน**: `doc_len` ที่เกือบเต็ม
> `u64` ทำให้ `read_index` **panic ตอนกำลังประกอบข้อความ error** ·
> เทสต์ถาวรที่กันไม่ให้กลับมาคือ
> `packed::tests::a_doc_len_near_the_end_of_u64_does_not_overflow_the_error_message`

**เพิ่ม target ใหม่ ต้องทำสามอย่างพร้อมกัน:**

1. ต่อ `fuzz_target!` ให้เรียกโค้ดนั้น
2. ลงทะเบียน `[[bin]]` ใน `fuzz/Cargo.toml`
3. ใส่ชื่อ target ใน matrix ของ `.github/workflows/fuzz.yml`

job `unwired-targets` จะ **ล้มทั้งสองทิศ** — โมดูลมีโค้ดแต่ target ยังไม่ต่อ
หรือ target ต่อแล้วแต่ไม่มีโค้ดให้ยิง

## corpus

- `fuzz/seeds/<target>/` — **commit ไว้ใน repo** เคสตั้งต้นที่คัดมาจากเทสต์จริง
  (decompression bomb, ไฟล์ถูกตัด, .exe ที่ตั้งชื่อเป็น .png, JPEG ที่มี EXIF จริง)
- `fuzz/corpus/<target>/` — ของที่ fuzzer งอกเอง **อยู่ใน `.gitignore`** (เป็นพันไฟล์)

สร้าง seed ใหม่ (รันจากรากโปรเจกต์):

| target | คำสั่ง | ทำไมคนละทาง |
|---|---|---|
| `fuzz_decode` | `python fuzz/seeds/make_seeds.py` | PNG/JPEG เป็น **รูปแบบของคนอื่น** เขียนขึ้นเองได้ |
| `fuzz_packed` | `cargo xtask gen-fuzz-seeds` | ★ `.refx` เป็น **รูปแบบของเราเอง** — seed ต้องออกจาก `packed::write_packed` ตัวจริง |

> ★★ ห้ามเขียนตัวประกอบไบต์ `.refx` ตัวที่สองขึ้นมาในภาษาอื่น · `docs/08 §3.9`
> ห้ามไว้ตรง ๆ: *สิ่งที่เขียนเลียนแบบจะสะท้อนความเข้าใจของคนเขียน ไม่ใช่
> พฤติกรรมของของจริง* · seed ที่ผิดไปหนึ่งช่องจะถูกปฏิเสธตั้งแต่ด่านแรกทุกใบ
> = corpus ที่ดูเหมือนมี แต่ไม่เคยพา fuzzer ไปถึง parser เลย

seed ของ `fuzz_packed` มีทั้ง **ไฟล์ที่ดี** (linked · packed 0/1/3 asset) และ
**ไฟล์ที่ถูกประกอบมาอย่างตั้งใจ** (`packed_offset_past_eof.refx` — `table_crc`
ถูกต้องแต่ entry ชี้เลยท้ายไฟล์) · ตัวหลังคือตัวที่ fuzzer แทบไม่มีวันสุ่มเจอเอง
เพราะมันต้องผ่าน checksum ก่อนถึงจะไปถึงด่านขอบเขต (`docs/08 §3.9` ข้อ 1b)

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
