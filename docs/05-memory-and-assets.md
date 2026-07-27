# 05 — Memory & Asset Pipeline

crate: `refx-asset` — depend: `image`, `zune-*`, `blake3`, `rusqlite`, `memmap2` | ห้าม: `wgpu`, `egui`

นี่คือ crate ที่ตัดสินว่าโปรแกรมจะกิน RAM 200 MB หรือ 2 GB

---

## 1. สามชั้น cache

| ชั้น | ขนาด | มีให้ใคร | Budget | ที่อยู่ |
|---|---|---|---|---|
| **T0 Thumbnail** | 128×128 | ทุกภาพใน board | ~16 MB @ 1000 ภาพ (BC7) | atlas ใน VRAM + sqlite บนดิสก์ |
| **T1 Working** | เท่าขนาดบนจอ (pow2) | เฉพาะที่มองเห็นและใหญ่กว่า 128px | **384 MB VRAM** (ปรับได้) | VRAM, LRU |
| **T2 Full-res** | ขนาดจริง | ≤ 2 ภาพ ตอน zoom > 100% | ตามภาพ, บังคับ ≤ 2 ตัว | VRAM, ทิ้งทันทีที่ซูมออก |

**T0 ต้องอยู่ตลอด (ห้าม evict)** เพราะ:
- 16 MB ถูกมากเทียบกับผลที่ได้
- ผู้ใช้ต้องเห็นภาพเสมอ ไม่ว่าซูมออกไกลแค่ไหน — ถ้า T0 ถูก evict จะเกิดอาการ "ภาพหายเป็นช่อง ๆ ตอนซูมออก" ที่รู้สึกเหมือนโปรแกรมพัง
- ถ้า board ใหญ่จน T0 เกิน 128 MB (≈ 8000 ภาพ) ค่อยเริ่ม evict ตาม LRU

---

## 2. Budget Manager

> **แก้หลัง spike:** ต้องอ่านจาก `adapter.limits()` / `adapter.get_info()` จริง ห้าม hard-code
> `PowerPreference::LowPower` อาจได้ **iGPU ที่แชร์ RAM ระบบ** — ถ้า `device_type == IntegratedGpu`
> ให้ลด `vram_limit` เหลือ **128 MB** เพราะทุกไบต์ที่จองไปเบียด RAM ที่ Photoshop ใช้อยู่ตรง ๆ

```rust
pub struct MemoryBudget {
    vram_limit: usize,      // Discrete: min(384 MB, VRAM/4) | Integrated: 128 MB
    // ★ 27 ก.ค. 2026: atlas layer ต้องจองแบบ lazy ทีละ layer ตามการใช้จริง
    // ห้ามจองเต็มเพดานตั้งแต่เปิดโปรแกรม — โปรแกรมที่เปิดค้างไว้ทั้งวันโดยยังไม่มีภาพ
    // แต่ยึด VRAM 192 MB ไว้เฉย ๆ คือการแย่ง VRAM จาก Photoshop โดยไม่ได้ใช้ประโยชน์
    ram_limit:  usize,      // default: 256 MB สำหรับ decode staging
    vram_used:  AtomicUsize,
    ram_used:   AtomicUsize,
}
```

- **ทุกการจอง texture/buffer ต้องผ่าน budget manager** ห้ามเรียก `device.create_texture` ตรง ๆ จากที่อื่น — บังคับด้วยการทำ `TextureAllocator` เป็นทางเดียวที่เข้าถึง device ได้ใน crate นี้
- เมื่อจะเกินเพดาน → evict LRU จาก T2 ก่อน แล้ว T1 แล้วค่อย T0
- แสดงตัวเลข used/limit บน status bar ตลอด (I-6 ตรวจสอบได้ด้วยตา)
- ปรับ limit ได้ใน settings — ผู้ใช้ที่มี VRAM 16 GB ควรใช้ได้เต็มที่ถ้าต้องการ

---

## 3. Decode pipeline

```
ItemVisible(id)
  ↓
[cache lookup: hash → thumbnail?]  ─เจอ─►  upload atlas  ─►  จบ (ไม่ decode)
  ↓ ไม่เจอ
[Job { hash, path, target_size, priority, cancel: Arc<AtomicBool> }]
  ↓ ส่งเข้า priority queue
[Decode worker]
  1. ~~memmap ไฟล์~~ → **`metadata()` เช็คขนาด แล้ว `read()` เข้า Vec** (ยกเลิก mmap 27 ก.ค. 2026 — SIGBUS จาก Dropbox/OneDrive sync ทำให้ `catch_unwind` ไร้ผล ดู docs/06 §3)
  2. ตรวจ magic bytes → รู้ format จริง (ห้ามเชื่อนามสกุลไฟล์)
  3. อ่าน header → ได้ w × h → ตรวจ limit ก่อน allocate ★
  4. catch_unwind { decode }
  5. แก้ EXIF orientation
  6. resize → target_size (Lanczos3 สำหรับ thumbnail, Triangle สำหรับ working)
  7. (ถ้า thumbnail) BC7 encode
  8. เช็ค cancel flag → ถ้า cancel แล้ว ทิ้งทันที
  ↓ ส่งกลับ main thread
[queue.write_texture]
```

### จุดที่สำคัญที่สุด: ข้อ 3

**ต้องอ่าน header เพื่อรู้ขนาดก่อนจอง memory เสมอ** ไฟล์ PNG 4 KB ที่ประกาศว่ากว้าง 65535 × สูง 65535 จะทำให้ decoder จอง 17 GB แล้วโปรแกรมตายทันที นี่คือ decompression bomb แบบคลาสสิก

```rust
const MAX_PIXELS_ABS: u64 = 268_435_456;   // 16384² — เพดานสูงสุดที่ยอมให้เป็นไปได้

// ★ แก้ 27 ก.ค. 2026 — เพดานจริงต้องผูกกับ RAM ที่เครื่องมี ไม่ใช่ค่าคงที่
//
// ปัญหา: ภาพ 16384² ต้องใช้ RAM ตอน decode ~2 GiB (RGBA 1 GiB + บัฟเฟอร์กลางของ decoder)
// บนเครื่อง 8 GB ที่เปิด Photoshop อยู่ = swap หนักหรือโดน OOM killer
// ซึ่งแปลว่า "งานหาย" — ผิด I-3 และเป็นสิ่งที่ห้ามเกิดเด็ดขาด
//
// กติกา: ภาพเดียวใช้ได้ไม่เกิน 1/8 ของ RAM ที่ติดตั้งไว้
//   8 GB  → ~11,500²   (พอสำหรับสแกน A3 300 dpi)
//   16 GB → ~16,384²   (ชนเพดานสัมบูรณ์)
// คำนวณครั้งเดียวตอนเปิดโปรแกรม แล้ว log ค่าที่ได้
fn max_pixels(total_ram: u64) -> u64 {
    ((total_ram / 8) / (4 * 2)).min(MAX_PIXELS_ABS)   // 4 ไบต์/pixel × เผื่อบัฟเฟอร์ 2 เท่า
}
const MAX_FILE_BYTES: u64 = 512 << 20; // 512 MB
const MAX_DIMENSION: u32 = 65_535;
const DECODE_TIMEOUT: Duration = Duration::from_secs(20);
```

ใช้ `image::ImageReader::into_dimensions()` เพื่ออ่านแค่ header — ไม่ decode

### Priority

```rust
priority = ระยะจากกึ่งกลาง viewport ถึงกึ่งกลาง item   // น้อย = มาก่อน
// ภาพที่อยู่นอกจอ (prefetch) บวก penalty คงที่ให้ไปอยู่ท้ายคิวเสมอ
```

### Cancellation — สำคัญต่อ "CPU น้อย" มาก

ผู้ใช้ pan อย่างเร็วผ่าน 500 ภาพ → ถ้าไม่มี cancel จะมี 500 job เข้าคิว decode ทั้งที่ผู้ใช้ไม่ได้อยากดูสักภาพ
ทุกครั้งที่ viewport เปลี่ยน: set cancel flag ให้ job ที่หลุด visible set + margin ทั้งหมด worker เช็ค flag ก่อนเริ่มและระหว่างทำ

### จำนวน worker

```rust
let n = (available_parallelism() - 2).clamp(2, 6);
```

ไม่ใช้ทุก core: เผื่อไว้ให้ UI thread และระบบปฏิบัติการ — โปรแกรมที่ยึด CPU 100% ตอนโหลดภาพทำให้ทั้งเครื่องหนืด และผู้ใช้จะรู้สึกว่า "กิน CPU"
เพดาน 6 เพราะเกินกว่านั้นคอขวดอยู่ที่ดิสก์ ไม่ใช่ CPU

---

## 4. Content hash

```rust
blake3::hash(&bytes[..])   // เร็วกว่า SHA-256 หลายเท่า, มี SIMD
// (ไม่ใช่ mmap แล้ว — ดู docs/06 §3)

// ★ แก้ 27 ก.ค. 2026 — cache key ต้องมี mtime + size ด้วย ไม่ใช่ hash อย่างเดียว
//
// fast path ของไฟล์ > 64 MB อ่านแค่หัว 1 MB + ท้าย 1 MB + ขนาด
// จุดบอด: ไฟล์สองไฟล์ที่ต่างกัน **เฉพาะตรงกลาง** จะได้ hash เดียวกัน
// นี่ไม่ใช่เคสสมมติ — นักวาด save ทับเป็น v1/v2 ของไฟล์ PSD/TIFF 100 MB
// โดยแก้เฉพาะเลเยอร์กลางไฟล์ เป็นเรื่องปกติมาก ผลคือ **เห็น thumbnail ของเวอร์ชันเก่า**
// ซึ่งทำลายความรู้สึก "เชื่อถือได้" โดยตรง และหาสาเหตุไม่เจอด้วย
//
// cache key = (fast_hash, mtime, file_size)   ← mtime ปิดจุดบอดนี้ฟรี ๆ
// ผลข้างเคียงที่ยอมรับได้: copy/move ไฟล์แล้ว mtime เปลี่ยน → cache miss → decode ใหม่
```

ไฟล์ > 64 MB: hash แค่ 1 MB แรก + 1 MB สุดท้าย + ขนาดไฟล์ (โอกาสชนกันในทางปฏิบัติ ≈ 0 และเร็วกว่ามาก)
เก็บ hash เต็มไว้ใน `AssetRef` เสมอ — ใช้ dedupe, ใช้ relink, ใช้เป็นคีย์ cache

---

## 5. Cache database

`%LOCALAPPDATA%\RefX\cache.sqlite` (Win) / `$XDG_CACHE_HOME/refx/cache.sqlite` (Linux) / `~/Library/Caches/RefX/` (mac)

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;   -- cache เสียหายได้ ไม่ใช่ข้อมูลผู้ใช้ ยอมแลกความเร็ว

CREATE TABLE thumbs (
    hash        BLOB PRIMARY KEY,   -- blake3-256
    width       INTEGER NOT NULL,   -- ขนาดจริงของภาพต้นฉบับ
    height      INTEGER NOT NULL,
    format      INTEGER NOT NULL,
    thumb_fmt   INTEGER NOT NULL,   -- 0 = RGBA8, 1 = BC7
    thumb       BLOB NOT NULL,      -- 128×128
    dominant    INTEGER NOT NULL,   -- สีเด่น (ARGB) ใช้เป็น placeholder ก่อนภาพจะมา
    last_used   INTEGER NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX idx_last_used ON thumbs(last_used);

CREATE TABLE paths (          -- ช่วย relink เวลาไฟล์ถูกย้าย
    path       TEXT PRIMARY KEY,
    hash       BLOB NOT NULL,
    mtime      INTEGER NOT NULL,
    size       INTEGER NOT NULL
);
```

- **เข้าถึงจาก IO thread ตัวเดียวเท่านั้น** (serialized) → ไม่มีปัญหา lock contention, ไม่ต้องใช้ connection pool
- `paths` ทำให้ข้ามการ hash ได้ถ้า mtime+size ตรง (การ hash ไฟล์ 4000px ใช้เวลาพอ ๆ กับอ่านมัน)
- Cache eviction: เมื่อ DB > 2 GB ลบตาม `last_used` เก่าสุด รันตอนปิดโปรแกรม ไม่ใช่ระหว่างใช้งาน
- **cache เสียหาย ≠ ข้อมูลหาย** ถ้าเปิด DB ไม่ได้ → ลบทิ้งแล้วสร้างใหม่ อย่าให้โปรแกรมเปิดไม่ขึ้นเพราะ cache

---

## 6. เปิด board 1000 ภาพให้เร็ว

```
t=0     อ่าน .refx (metadata ล้วน, ไม่มีภาพ) → มี layout ครบ → วาด placeholder ทันที
t+20ms  หน้าต่างพร้อมใช้ ผู้ใช้ pan/zoom ได้แล้ว ★
t+50ms  IO thread: SELECT hash, thumb FROM thumbs WHERE hash IN (...)  [batch 500 ต่อ query]
t+300ms upload atlas เป็นชุด ๆ ละ 64 ภาพ (ไม่ใช่ทีเดียว — กัน frame กระตุก)
t+1.2s  thumbnail ครบ (cache อุ่น)
```

จุดที่ต้องได้: **ผู้ใช้ต้อง interact ได้ที่ t+20ms** ไม่ใช่รอ 1.2 วินาที การโหลดภาพเกิดเบื้องหลังทั้งหมด

### Cache เย็น — ★ ตัวเลขเดิมในเอกสารนี้ผิด แก้ 27 ก.ค. 2026

เอกสารเดิมเขียนว่า "1000 ภาพ × ~15 ms ÷ 4 worker ≈ 4 วินาที" — **ผิดประมาณ 20 เท่า**
ตัวเลข 15 ms มาจากการเดาบนภาพเล็ก ของจริงที่วัดได้บนภาพ 4000×3000 (ขนาดที่ผู้ใช้มีจริง):

| วัดจริง (6 worker, JPEG 70% / PNG 30%) | |
|---|---|
| 1 ไฟล์ 4000×3000 cache เย็น | **148.8 ms** |
| 100 ไฟล์ (616 MB) cache เย็น | **8.09 s** → ~81 ms/ไฟล์ |
| 100 ไฟล์ cache อุ่น | **24.6 ms** |
| คาดการณ์ 1000 ไฟล์ cache เย็น | **~80 s** (ไม่ใช่ 4 s) |

**80 วินาทีรับได้ ก็ต่อเมื่อ** ทั้งสามข้อนี้จริง — ถ้าข้อใดข้อหนึ่งพัง ตัวเลขนี้กลายเป็นรับไม่ได้ทันที:

1. ภาพ **ทยอยขึ้นทีละใบ**ทันทีที่ decode เสร็จ ไม่ใช่ขึ้นพร้อมกันตอนจบ
2. ผู้ใช้ pan/zoom/จัดวางได้ตลอดเวลาที่โหลดอยู่ (ห้ามมี modal ห้ามค้าง)
3. มีตัวบอกความคืบหน้าที่เห็นได้ เช่น `กำลังโหลด 312 / 1000`

เพราะสิ่งที่ผู้ใช้รู้สึกคือ "ภาพแรกขึ้นเร็วแค่ไหน" (149 ms — ดีมาก) ไม่ใช่ "ครบเมื่อไหร่"

### ★ ทางลดที่ต้องลองก่อนยอมรับ 80 วินาที: decode JPEG แบบย่อขนาด

thumbnail ปลายทางคือ **128 px** แต่ตอนนี้เรา decode ภาพ 4000×3000 ออกมาเต็ม 12 ล้าน pixel
แล้วค่อยย่อด้วย Lanczos3 — **ทำงานมากกว่าที่จำเป็นราว 64 เท่า**

JPEG ย่อขนาดได้ตั้งแต่ตอน decode (ข้าม DCT coefficient ความถี่สูง) ที่ 1/8 scale
ได้ 500×375 ซึ่งยังใหญ่กว่า 128 px ที่ต้องการ และเร็วกว่าหลายเท่าโดยไม่เสียคุณภาพที่มองเห็นได้เลย

`zune-jpeg` อยู่ใน dependency list มาตั้งแต่ต้นเพื่อเรื่องนี้โดยเฉพาะ — **แต่ยังไม่มีโค้ดไหนเรียกใช้**

→ ให้ตรวจว่า API ที่มีจริงรองรับแค่ไหน แล้ววัดเทียบก่อน/หลัง **ห้ามเดา**
→ ถ้าลดได้จริงตามคาด 1000 ภาพเย็นจะเหลือระดับ 15–20 วินาที ซึ่งเปลี่ยนประสบการณ์ผู้ใช้คนละเรื่อง
→ ถ้าทำไม่ได้ ให้รายงานว่าติดตรงไหน แล้วเรายอมรับ 80 วินาทีพร้อมเงื่อนไข 3 ข้อข้างบน

---

## 7. Format ที่รองรับ (v1)

| Format | Decoder (pure Rust) | หมายเหตุ |
|---|---|---|
| JPEG | `zune-jpeg` | เร็วกว่า `jpeg-decoder` มาก, มี SIMD |
| PNG | `png` | |
| WebP | `image-webp` | pure Rust — **ห้ามใช้ `libwebp-sys` เด็ดขาด** (CVE-2023-4863) |
| GIF | `gif` | เฟรมแรกอย่างเดียวใน v1 |
| BMP / TGA / TIFF | `image` | |
| AVIF | ❌ ไม่รองรับ v1 | decoder pure-Rust ยังไม่นิ่งพอ |
| PSD | ❌ ไม่รองรับ v1 | ซับซ้อนและเป็นผิวสัมผัสความเสี่ยงสูง |

การเพิ่ม format ใหม่ = ต้องมี fuzz target ก่อนเสมอ (ดู [06](06-security.md))

---

## 8. ที่มาของตัวเลข RAM ≤ 250 MB

| ส่วน | ประมาณการ |
|---|---|
| Rust runtime + wgpu + driver | ~60 MB |
| egui + font atlas | ~15 MB |
| Document (1000 items × ~400 B) | ~0.4 MB |
| Decode staging buffer (6 worker × ~16 MB) | ~96 MB (peak, คืนเมื่อ idle) |
| sqlite page cache | ~8 MB |
| เบ็ดเตล็ด / allocator overhead | ~40 MB |
| **รวม idle** | **~130 MB** |
| **รวม peak ตอนโหลด** | **~230 MB** |

VRAM แยกต่างหาก: atlas 16 MB + working ≤ 384 MB
ถ้าวัดจริงแล้วเกิน ให้หาว่าอะไรผิดจากตารางนี้ — อย่าปรับเพดานหนี
