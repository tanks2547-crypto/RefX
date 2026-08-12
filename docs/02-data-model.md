# 02 — Data Model

crate: `refx-core` — **ห้าม** depend wgpu / egui / winit / rusqlite / std::fs
ทุก type ในไฟล์นี้ต้อง unit-test ได้โดยไม่มี GPU และไม่แตะดิสก์

---

## 1. Identity: generational arena

ห้ามใช้ `Vec<Item>` + index ดิบ (index ค้างหลังลบ = บั๊กเงียบ) และห้ามใช้ `Rc<RefCell<Item>>` (cache-hostile + runtime panic)

```rust
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ItemId { index: u32, generation: u32 }

pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    generation: u32,
}
enum Slot<T> { Occupied { gen: u32, value: T }, Vacant { gen: u32 } }
```

`get(id)` คืน `None` ถ้า generation ไม่ตรง → id เก่าใช้ไม่ได้โดยอัตโนมัติ ไม่ใช่ไปอ่านของคนอื่น
ทำแบบเดียวกันสำหรับ `GroupId`, `BoardId`, `AssetId`

> ใช้ crate `slotmap` ได้ (audited, no unsafe ใน API ที่เราใช้) — เขียนเองก็ได้ ~120 บรรทัด เลือกอย่างใดอย่างหนึ่งแล้วอย่าปนกัน

---

## 2. โครงหลัก

```rust
pub struct Workspace {
    pub boards: Arena<Board>,
    pub active: BoardId,
    pub order: Vec<BoardId>,          // ลำดับแท็บ
}

pub struct Board {
    pub id: BoardId,
    pub name: String,
    pub items: Arena<Item>,
    pub z_order: Vec<ItemId>,         // ล่างสุด → บนสุด. source of truth ของ z
    pub groups: Arena<Group>,
    // ❌ pub selection: Selection,   ← ย้ายออกแล้ว ดู §2.9
    pub view: ViewState,              // camera ของแต่ละ mode (persist แต่ไม่ undo ไม่ dirty — §2.9)
    pub arrange: ArrangeState,
    pub settings: BoardSettings,
    pub dirty: bool,
}

pub struct Item {
    pub id: ItemId,
    pub kind: ItemKind,
    pub canvas: ItemCanvas,           // ★ สถานะฝั่ง Canvas mode
    pub meta: ItemMeta,               // ★ สถานะฝั่ง Arrange mode
}

pub enum ItemKind {
    Image(AssetRef),
    Text(TextNote),
    /// ภาพที่โหลดไม่สำเร็จ — ยังอยู่ใน board ไม่หายไปไหน ผู้ใช้ relink ได้
    Missing { original_path: PathBuf, reason: LoadError },
}
```

### 2.1 ItemCanvas — เจ้าของโดย Canvas mode

```rust
pub struct ItemCanvas {
    pub pos: Vec2,          // world space, f32 (ดู §6 เรื่องความแม่นยำ)
    pub size: Vec2,         // ขนาดที่แสดง (world units) — ไม่ใช่ขนาด pixel ต้นฉบับ
    pub rotation: f32,      // เรเดียน
    pub flip: Flip,         // none | horizontal | vertical | both
    pub opacity: f32,       // 0.0..=1.0
    pub crop: CropRect,     // normalized 0..1 ของภาพต้นฉบับ
    pub locked: bool,
    pub visible: bool,
    pub filter: ItemFilter, // grayscale, invert, brightness/contrast (คำนวณใน shader)
}
```

**z-order ไม่เก็บใน `ItemCanvas`** — เก็บเป็น `Vec<ItemId>` ระดับ board เพราะ:
การ "ส่งไปหลังสุด" ด้วยตัวเลข z จะทำให้เลขชนกันแล้วต้อง renumber ทั้งหมด ส่วน `Vec` แค่ย้ายตำแหน่ง O(n) และ **ลำดับ render = ลำดับใน Vec** ตรงไปตรงมา ไม่ต้อง sort ทุกเฟรม

### 2.2 ItemMeta — เจ้าของโดย Arrange mode

```rust
pub struct ItemMeta {
    pub tags: SmallVec<[TagId; 4]>,
    pub rating: u8,               // 0..=5
    pub color_label: Option<ColorLabel>,
    pub group: Option<GroupId>,
    pub note: String,
    pub added_at: i64,            // unix millis
    pub pinned: bool,             // arrange จะไม่ย้ายภาพที่ pin
}
```

**สองอันนี้ต้องอยู่คู่กันตลอดชีวิตของ item** สลับ mode ไปมาแล้วข้อมูลอีกฝั่งไม่หาย — นี่คือข้อกำหนดหลักของดีไซน์สองโหมด

### ★ 2.2.5 ชนิดใน `refx-core` ต้องเป็นของตัวเอง ห้ามยืมจาก `image` (แก้ 2 ส.ค. 2026)

เอกสารฉบับแรกเขียนว่า `AssetRef.format` เป็น `ImageFormat` และ `ItemKind::Missing.reason`
เป็น `LoadError` โดยตั้งใจให้หมายถึงชนิดของ crate `image` — **ทำไม่ได้ และไม่เคยทำได้**

1. `refx-core` **ห้าม** depend `image` (ARCHITECTURE §2) → คอมไพล์ไม่ผ่านตั้งแต่ต้น
2. §7 บังคับให้ทุกอย่างใน `Board` serialize ลง DTO ที่มีเวอร์ชัน แต่ **`image::ImageError`
   serialize ไม่ได้เลย** → ต่อให้ข้อ 1 ไม่มีปัญหา ข้อนี้ก็ยังตัน

**ทางที่ถูก — `refx-core` นิยาม enum ของตัวเอง:**

```rust
// refx-core — ข้อมูลล้วน serialize ได้ ไม่พึ่ง crate ภายนอก
pub enum ImageFormat {
    Png, Jpeg, WebP, Gif, Bmp, Tga, Tiff,
    /// ★ ต้องมี (เพิ่ม 2 ส.ค. 2026) — สองเหตุผล
    /// 1. ไฟล์ที่บันทึกด้วยรุ่นใหม่กว่าอาจมี format ที่รุ่นนี้ไม่รู้จัก
    ///    อ่านเจอค่าที่ไม่รู้จักต้องตกมาที่นี่ **ห้าม error ทิ้งทั้งไฟล์** (I-3)
    /// 2. บางเส้นทางยังไม่รู้ format จริงตอนสร้าง `AssetRef`
    ///    (cache hit ไม่ได้แตะไบต์เลย) — เขียน `Unknown` ตรง ๆ **ดีกว่าเดาจากนามสกุล**
    ///    เพราะค่านี้ถูก persist ลงไฟล์ และนามสกุลโกหกได้ (I-4)
    Unknown,
}

pub enum MissingReason {
    FileNotFound, TooLarge, UnsupportedFormat, Damaged, Unreadable,
    /// ★ ต้องมี — ไฟล์ที่บันทึกด้วยรุ่นใหม่กว่าอาจมีเหตุผลที่รุ่นนี้ไม่รู้จัก
    /// อ่านเจอค่าที่ไม่รู้จักต้องตกมาที่นี่ **ห้าม error ทิ้งทั้งไฟล์** (I-3)
    Unknown,
}
```

### ★ กฎทั่วไป: ทุก enum ที่ลงไฟล์ต้องมีทางออก

**enum ใดก็ตามที่ถูก serialize ลง `.refx` หรือ `cache.sqlite` ต้องมี variant สำรอง**
(`Unknown` / `Other`) ที่รับค่าที่อ่านไม่รู้จักได้ **ห้ามให้ค่าเดียวที่ไม่รู้จักล้มทั้งไฟล์**

ไฟล์ของผู้ใช้คืองานที่เขาทำมาหลายชั่วโมง การปฏิเสธทั้งไฟล์เพราะไบต์เดียว = งานหาย = ผิด I-3
และมันจะเกิดกับคนที่เปิดไฟล์ด้วยรุ่นเก่ากว่า ซึ่งเป็นเรื่องปกติเวลาส่งไฟล์ให้กัน

### ★★ ยกระดับ: "ทนได้" ยังไม่พอ ต้อง **ส่งคืนค่าเดิมได้** (2 ส.ค. 2026)

กฎข้างบนกันไฟล์พังได้ แต่**ยังทำงานหายอยู่** — เจอตอน audit `ColorLabel`:

> ผู้ใช้ติดป้ายสีด้วย RefX รุ่นใหม่ → เปิดด้วยรุ่นเก่า → รุ่นเก่าอ่านค่าไม่รู้จักเป็น `None`
> → ผู้ใช้ขยับภาพหนึ่งใบแล้วบันทึก → **ป้ายสีหายถาวร** โดยไม่มีอะไรเตือน

กลไกที่สร้างมาเพื่อกัน I-3 กลับกลายเป็นตัวทำให้ข้อมูลหายเสียเอง

**กฎที่ถูก แยกตามชนิดของสิ่งที่ไม่รู้จัก:**

| ไม่รู้จักอะไร | ต้องทำ |
|---|---|
| **ค่าเดี่ยว ๆ** (enum เช่น `ColorLabel`, `Flip`, `ImageFormat`) | **เก็บค่าดิบไว้ใน DTO แล้วเขียนกลับตามเดิม** ตอนบันทึก · UI แสดงเป็น "ไม่รู้จัก" ได้ แต่ห้ามแปลงค่าทิ้ง |
| **โครงสร้างทั้งก้อน** (variant ใหม่ของ `ItemKind` เช่น `Video`) | round-trip ไม่ได้จริง ๆ → **ต้องกันที่ระดับไฟล์** ดูด้านล่าง |

**สำหรับโครงสร้างที่ไม่รู้จัก — ตรวจเวอร์ชันไฟล์ (`docs/07`):**

ถ้า major version ของ `.refx` สูงกว่าที่รุ่นนี้เข้าใจ →
**เปิดแบบอ่านอย่างเดียว** แล้วบอกผู้ใช้ตรง ๆ ว่าต้องอัปเดตโปรแกรมก่อนจึงจะแก้ไฟล์นี้ได้

ยอมให้ผู้ใช้แก้ไม่ได้ชั่วคราว **ดีกว่า**ปล่อยให้เขาบันทึกทับแล้วงานหายโดยไม่รู้ตัว —
อันแรกน่ารำคาญ อันหลังคือสิ่งที่ `CLAUDE.md` บอกว่า "เลิกใช้ทันที ไม่มีโอกาสที่สอง"

> เจอมาแล้วสองตัวที่ขาดทางออก (`MissingReason`, `ImageFormat`) ทั้งคู่เจอตอนกำลังจะเขียนโค้ดทับ
> และหนึ่งตัวที่มีทางออกแล้วแต่ยัง**ทำข้อมูลหาย** (`ColorLabel`)
> **ให้ audit พร้อมกันทีเดียว** อย่าไล่เจอทีละตัว

`refx-asset` เป็นฝั่งที่รู้จักทั้งสองโลก จึงเป็นที่เดียวที่มี `impl From<&LoadError> for MissingReason`
→ เพิ่ม variant ใน `LoadError` เมื่อไหร่ **คอมไพล์ไม่ผ่านจนกว่าจะจัดการที่จุดเดียวนั้น**

**ห้ามเก็บเป็น `String`** ถึงจะง่ายกว่า — ข้อความจะถูกบันทึกลงไฟล์ `.refx`
ด้วยภาษาที่ผู้ใช้ตั้งไว้ *ตอนบันทึก* แล้วสลับภาษาทีหลังก็แปลกลับไม่ได้อีก
ขัดกับกฎใน `docs/03 §0` ที่ว่าข้อความผู้ใช้ต้องประกอบขึ้นจากข้อมูลที่มีโครงสร้าง
`refx-ui::text` แปลจาก variant ได้ตรง ๆ อยู่แล้ว

`ContentHash` เป็นข้อมูลล้วนอยู่แล้ว → ย้ายลง `refx-core` ได้ตามเดิม
แล้ว `refx-asset` re-export กลับที่ `refx_asset::hash` เพื่อไม่ต้องแก้ call site เดิม

---

### 2.3 AssetRef — แยกภาพออกจาก item

```rust
pub struct AssetRef {
    pub hash: ContentHash,        // blake3-256 ของไฟล์ต้นฉบับ = คีย์หลัก
    pub path: PathBuf,            // เส้นทางล่าสุดที่เจอ (เป็นแค่ hint)
    pub px_size: UVec2,           // ขนาดจริงหลังแก้ EXIF orientation แล้ว
    pub format: ImageFormat,
    pub embedded: bool,           // true = ตัวไฟล์ฝังอยู่ใน .refx (packed mode)

    // ★ เพิ่ม 10 ส.ค. 2026 — สองค่านี้ **ถูกคำนวณอยู่แล้ว** ตอน ingest
    // เพราะเป็นส่วนหนึ่งของ cache key `(hash, mtime, size)` (§2.9 ข้อ 4)
    // แต่เดิมถูกทิ้งหลังใช้เสร็จ ทำให้ sort by "วันที่แก้ไข" กับ "ขนาดไฟล์"
    // ใน docs/03 §3 ทำไม่ได้ทั้งที่ข้อมูลอยู่ในมือแล้ว
    pub mtime: i64,               // unix millis ของไฟล์ต้นฉบับ ตอนที่ ingest
    pub file_size: u64,           // ไบต์
}

> ค่าทั้งสองเป็นสภาพ ณ ตอน ingest — ถ้าไฟล์ถูกแก้ทีหลังจะไม่ตรง
> ยอมรับได้สำหรับการเรียงบน mood board และ cache key ตรวจซ้ำตอนโหลดอยู่แล้ว
```

ภาพเดียวกันวางซ้ำ 10 ครั้ง = 10 `Item` แต่ **1 `AssetRef` เดียว 1 texture เดียว** ประหยัด RAM/VRAM ทันที
`hash` เป็นคีย์หลัก ไม่ใช่ `path` → ย้ายไฟล์แล้ว thumbnail ไม่หาย, relink หาไฟล์เจอด้วย hash, ไฟล์ซ้ำถูกยุบอัตโนมัติ

---

## 3. Command / Undo

**mutation ทุกอย่างต้องผ่าน `Command`** ไม่มีข้อยกเว้น ถ้ามีโค้ดแก้ `Board` ตรง ๆ คือบั๊ก

```rust
pub trait Command: Send + std::fmt::Debug {
    fn apply(&mut self, board: &mut Board) -> Result<(), CmdError>;
    fn undo(&mut self, board: &mut Board);
    /// รวมกับ command ก่อนหน้าได้ไหม (เช่น ลากเมาส์ต่อเนื่อง = 1 undo)
    fn merge(&mut self, next: &dyn Command) -> bool { false }
    fn label(&self) -> &'static str;
}

pub struct History {
    undo: VecDeque<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
    max_entries: usize,   // default 200
    max_bytes: usize,     // default 64 MB — I-6: cache ทุกตัวมีเพดาน
}
```

Command ที่ต้องมี:

| Command | merge ได้ | หมายเหตุ |
|---|---|---|
| `AddItems` / `RemoveItems` | ✗ | เก็บ item ที่ลบไว้ใน command เพื่อ undo |
| `TransformItems` | ✓ | ลาก/สเกล/หมุน — merge ระหว่างลากค้าง, ปิด merge เมื่อปล่อยเมาส์ |
| `ReorderZ` | ✗ | เก็บ `Vec<ItemId>` เดิมทั้งชุด (ถูกกว่า diff และไม่มีบั๊ก) |
| `SetCrop` | ✓ | |
| `EditMeta` | ✓ ต่อ field | tag/rating/label |
| `ApplyLayout` | ✗ | **สะพาน Arrange→Canvas** เก็บ `Vec<(ItemId, ItemCanvas)>` เดิมทั้งชุด → undo ครั้งเดียวคืนหมด |
| `GroupItems` / `Ungroup` | ✗ | |

**เรื่อง `merge` ที่พลาดกันบ่อย:** ระหว่างลากเมาส์ต้อง merge เพื่อไม่ให้ undo stack ท่วม แต่ต้อง **ปิด merge ทันทีที่ปล่อยปุ่ม** (`history.seal()`) ไม่งั้นการลากสองครั้งติดกันจะกลายเป็น undo เดียว ผู้ใช้จะงง

---

## 2.9 ★ อะไรอยู่ใน `Board` และอะไรไม่อยู่ (แก้ 2 ส.ค. 2026)

`Board` = **เอกสาร** สิ่งที่บันทึกลง `.refx` และสิ่งที่ undo ได้
ทุกฟิลด์ใน `Board` ต้องผ่าน `Command` ตาม `docs/08 §4` ข้อ 10

เอกสารฉบับแรกใส่ `selection` ไว้ใน `Board` — **ผิด** และสร้างปัญหาสามชั้น:

1. **undo กลายเป็นเรื่องสับสน** คลิกเลือก 10 ครั้งแล้วย้ายภาพหนึ่งครั้ง
   กด Ctrl+Z ย้อนการย้ายได้ถูก แต่กดต่อจะไล่ผ่านการเลือก 10 ครั้ง
   ผู้ใช้เห็น "undo แล้วไม่มีอะไรเกิดขึ้น" ซึ่งอ่านได้ว่าโปรแกรมพัง
   เครื่องมือกระแสหลัก (Figma, Illustrator) ไม่เอาการเลือกขึ้นสแตก
2. **★ คลิกเฉย ๆ ทำให้เอกสาร dirty** — ผู้ใช้เปิดไฟล์ คลิกดูภาพสองสามใบ ปิด
   แล้วโดนถาม "บันทึกการเปลี่ยนแปลงไหม" ทั้งที่ไม่ได้แก้อะไรเลย
   นี่คือรายละเอียดเล็ก ๆ ที่ทำลายความรู้สึก "เชื่อถือได้" ตรงตามที่ `CLAUDE.md` เตือน
3. **ถูกบันทึกลงไฟล์** เปิดไฟล์มาแล้วมีของถูกเลือกค้างอยู่จากเมื่อวาน

### กติกา

| | อยู่ใน `Board` | บันทึกลง `.refx` | ผ่าน `Command` | ทำให้ `dirty` |
|---|---|---|---|---|
| `items` `z_order` `groups` `settings` | ✅ | ✅ | ✅ | ✅ |
| `view` (camera) | ✅ | ✅ | ❌ **ยกเว้น** | ❌ **ยกเว้น** |
| `selection` | ❌ **ย้ายออก** | ❌ | ❌ | ❌ |

**`selection` ย้ายไปอยู่ในสถานะชั่วคราวของ editor** (นอก `Board`) — ไม่ persist ไม่ undo ไม่ dirty

**`view` เป็นข้อยกเว้นที่ตั้งใจ:** อยากให้เปิดไฟล์แล้วกลับมาที่มุมมองเดิม จึงบันทึก
แต่การ pan/zoom ต้องไม่ทำให้เอกสาร dirty และต้องไม่กิน undo
(ลาก pan 200 เฟรมแล้วกด Ctrl+Z ต้องย้อน *การแก้ครั้งล่าสุด* ไม่ใช่ย้อนกล้อง)

### แล้ว undo การลบจะคืนการเลือกยังไง

ต้องคืน — ผู้ใช้ลบ 5 ภาพแล้ว undo ควรได้ 5 ภาพนั้นกลับมาพร้อมถูกเลือกอยู่
ทำโดยให้ `Command` มี **`affected() -> &[ItemId]` เป็น method แยก** แล้วชั้น editor
เอาไปตั้ง selection เอง — การเลือกยังไม่ใช่สิ่งที่ถูก undo มันแค่ตามผลลัพธ์

> ★ **แก้ 2 ส.ค. 2026:** ฉบับแรกเขียนว่าให้ `undo()` เป็นตัวคืนรายการ — **ใช้ไม่ได้**
> เพราะ redo เดินผ่าน `apply()` ไม่ใช่ `undo()` ถ้าผูกไว้กับ `undo` อย่างเดียว
> **redo จะตั้ง selection ไม่ได้เลย** · แยกเป็น method เดียวจึงรับใช้ทั้งสองทิศทาง
> และเหมือน `heap_size()` คือ **ไม่มี default** — command ใหม่ถูกบังคับให้ประกาศว่าตัวเองแตะอะไร
>
> `ReorderZ::affected()` คืนว่างโดยตั้งใจ — การจัดลำดับ z ไม่ได้เจาะจงภาพไหน
> สิ่งที่ผู้ใช้เลือกไว้ควรอยู่เหมือนเดิม

---

## 4. Selection

```rust
pub struct Selection {
    items: IndexSet<ItemId>,   // รักษาลำดับที่คลิก — "ตัวสุดท้ายที่เลือก" คือ anchor ของ align
    anchor: Option<ItemId>,
}
```

ใช้ `IndexSet` (indexmap) ไม่ใช่ `HashSet` เพราะคำสั่งจัดเรียง/align ต้องรู้ลำดับและตัวอ้างอิง

---

## 5. ViewState — camera แยกต่อ mode

```rust
pub struct ViewState {
    pub canvas: Camera,     // pan/zoom ของ canvas mode
    pub arrange: Camera,    // scroll/zoom ของ arrange mode
    pub mode: Mode,
}

pub struct Camera { pub center: Vec2, pub zoom: f32 }  // zoom: 0.01..=64.0
// ★ แก้ 2 ส.ค. 2026 ให้ตรงกับโค้ดจริงที่ใช้มาตั้งแต่ P0-7 และมีเทสต์ 11 ตัวคุมอยู่
// เอกสารเดิมเขียน 0.02..=32.0 ซึ่งไม่เคยตรงกับ implementation
// ช่วงที่กว้างกว่ามีประโยชน์จริง: 1% เห็น board ทั้งกระดานตอนจัด mood board
// 64× ดูรายละเอียดระดับเส้น — และ P1-7 working texture ยืนยันแล้วว่าคมจริงที่ 436%
```

แยก camera ต่อ mode เพราะสลับ mode แล้วกลับมา ผู้ใช้คาดหวังว่ายังอยู่ตำแหน่งเดิม — ถ้าใช้ camera ร่วมกันจะเด้งไปมาและน่ารำคาญมาก

---

## 6. ความแม่นยำของพิกัด

ใช้ `f32` สำหรับ world space **ได้** ถ้าจำกัดขนาด canvas ที่ ±1e6 world units (f32 ให้ความละเอียด ~0.06 unit ที่ 1e6 — ตากับหน้าจอไม่มีทางเห็น)

**ต้องบังคับ clamp ตำแหน่งไว้ที่ ±1,000,000 เสมอ** ถ้าปล่อยให้ผู้ใช้ลากภาพไปไกลไม่จำกัด จะเกิดอาการ "ภาพสั่น/กระตุกตอนซูม" ที่ตามแก้ยากมาก — clamp ตั้งแต่แรก ถูกกว่าเปลี่ยนไป f64 ทีหลังหลายเท่า

---

## 7. Serialization

- โครงสร้างใน memory (`Arena`, `IndexSet`) **ห้าม** serialize ตรง ๆ
- ต้องแปลงเป็น DTO ที่แบนและมีเวอร์ชัน (`refx-io::v1::BoardDto`) ก่อนเขียนไฟล์
- เหตุผล: refactor ภายในต้องไม่ทำให้ไฟล์ของผู้ใช้เปิดไม่ได้ นี่คือ I-3
- รายละเอียด: [`07-file-format.md`](07-file-format.md)
