# 01 — Architecture Decision Records

บันทึกว่า *ทำไม* ไม่ใช่แค่ *อะไร* — เพื่อไม่ให้ต้องเถียงเรื่องเดิมซ้ำในอีก 6 เดือน
ADR ที่ตัดสินแล้วห้ามเปลี่ยนโดยไม่เขียน ADR ใหม่ที่ superseded ตัวเก่า

---

## ADR-001 — ใช้ Rust

**บริบท:** เป้าหมาย คือ เสถียร > ปลอดภัย > เบา

**ทางเลือกที่พิจารณา**

| | เสถียร | ปลอดภัย | RAM idle | เวลาพัฒนา |
|---|---|---|---|---|
| Rust + wgpu | ✅ ไม่มี UB, ไม่มี GC | ✅ memory-safe | ~130 MB | ช้าที่สุด |
| C++ / Qt6 | ⚠️ UB ได้ตลอด | ⚠️ | ~150 MB | กลาง |
| C# / Avalonia | ✅ | ✅ | ~250 MB + GC pause | เร็ว |
| Tauri | ✅ | ⚠️ WebView surface | ~200 MB | เร็ว |
| Electron | ✅ | ⚠️ | ~500 MB+ | เร็วที่สุด |

**ตัดสิน:** Rust

**เหตุผล:** ผู้ใช้ระบุ "เสถียรที่สุด" เป็นเป้าหมายอันดับหนึ่ง ไม่ใช่ "ทำเสร็จเร็วที่สุด" ในโปรแกรมที่หน้าที่หลักคือ *แกะไฟล์ไบนารีจากอินเทอร์เน็ต* memory safety ไม่ใช่ของฟุ่มเฟือย มันคือฟีเจอร์หลัก

**ต้นทุนที่ยอมรับ:** เวลาพัฒนามากกว่าทางเลือกอื่นราว 1.5–2 เท่า, ecosystem UI ยังใหม่, compile time ช้า

---

## ADR-002 — wgpu แทน OpenGL / DirectX ตรง ๆ

**ตัดสิน:** wgpu

**เหตุผล:** โค้ดชุดเดียวได้ Vulkan + D3D12 + Metal, validation layer จับ error ตอน dev, ตัว wgpu เองเป็น safe Rust, ใช้จริงใน Firefox/Deno/Bevy/Rerun (battle-tested มาก)

**ทางเลือกที่ไม่เอา:**
- OpenGL — deprecated บน macOS, driver bug บน Windows/Intel เยอะมาก, ไม่มีอนาคต
- Vulkan ตรง ๆ — verbose มาก, unsafe ทั้งหมด, ต้องเขียน backend แยกให้ macOS อยู่ดี
- Skia — C++, ผูก dependency ก้อนใหญ่, ขัด ADR-001 ในเชิงเจตนา

**ความเสี่ยง:** wgpu ยัง breaking API ทุก ~3 เดือน → **บรรเทาด้วยการตรึงเวอร์ชันและอัปเกรดเป็น PR แยกที่ตั้งใจทำ** ไม่ใช่ `cargo update` ลอย ๆ

---

## ADR-003 — egui แทน GUI framework แบบ retained

**ตัดสิน:** egui + egui-wgpu

**เหตุผล:**
- แชร์ `wgpu::Device` ตัวเดียวกับ canvas renderer → ไม่มี interop, ไม่มีการคัดลอกข้าม context
- immediate mode = ไม่มี widget tree ค้างใน memory, ไม่มี state ซิงก์ไม่ตรง (แหล่งบั๊ก UI อันดับหนึ่งของ retained-mode)
- เบามาก (~15 MB รวม font atlas)
- Rerun ใช้ egui กับข้อมูลระดับล้านจุด — พิสูจน์แล้วว่าสเกลได้

**ทางเลือกที่ไม่เอา:**
- Iced — retained, ecosystem เล็กกว่า, การผสม custom wgpu renderer ยุ่งกว่า
- Slint — DSL แยก + เรื่อง license
- GTK/Qt binding — ลาก C/C++ กลับเข้ามา ขัด ADR-001

**ข้อเสียที่ยอมรับ:** egui สวยน้อยกว่า native และ text input ภาษาซับซ้อนยังไม่สมบูรณ์เท่า native — สำหรับเครื่องมือของนักวาดที่ UI แทบทั้งหมดเป็นปุ่มกับตัวเลข ยอมรับได้

---

## ADR-004 — ไม่ใช้ async runtime

**ตัดสิน:** thread pool + `crossbeam-channel` ไม่มี tokio ไม่มี async-std

**เหตุผล:**
- งานของเราคือ **CPU-bound** (decode/resize) ไม่ใช่ IO-bound แบบเซิร์ฟเวอร์ — async แก้ปัญหาที่เราไม่มี
- tokio เพิ่ม ~40 dependency, เพิ่ม binary หลาย MB, เพิ่มพื้นที่ให้ผิดพลาด
- stack trace ของ thread ธรรมดาอ่านรู้เรื่อง ต่างจาก async ที่ debug ยากกว่ามาก
- **ไม่มี async = ไม่มีข้ออ้างให้ใครเผลอเพิ่ม network dependency** (สอดคล้อง I-8)

---

## ADR-005 — SQLite สำหรับ thumbnail cache

**ตัดสิน:** rusqlite (bundled)

**ทางเลือกที่ไม่เอา:**
- ไฟล์ .thumb แยกไฟล์ต่อภาพ — 1000 ภาพ = 1000 ไฟล์เล็ก ๆ ช้ามากบน Windows (NTFS overhead ต่อไฟล์สูง) และเปลืองพื้นที่ตาม cluster size
- sled / redb — pure Rust น่าสนใจ แต่ SQLite ผ่านการใช้งานมา 25 ปีและมีเครื่องมือ debug ครบ
- ไม่ cache เลย — decode 1000 ภาพทุกครั้งที่เปิด = ผู้ใช้รอ 4 วินาทีทุกครั้ง ยอมรับไม่ได้

**หมายเหตุความปลอดภัย:** SQLite เป็น C — เป็นข้อยกเว้นเดียวของกฎ "ไม่มี C" ยอมรับได้เพราะเป็นซอฟต์แวร์ที่ผ่านการ audit หนักที่สุดตัวหนึ่งในโลก, input ที่มันรับคือ query ที่เราเขียนเอง (ไม่ใช่ข้อมูลจากผู้โจมตี) และเราถือ cache DB เป็น untrusted อยู่แล้ว (T8)

---

## ADR-006 — สอง mode บน document เดียว ไม่ใช่สอง document

**ตัดสิน:** `Item` ถือทั้ง `ItemCanvas` และ `ItemMeta` ตลอดชีวิต mode คือ view

**เหตุผล:** ถ้าแยก document ต่อ mode จะต้องทำ sync สองทาง ซึ่งเป็นแหล่งบั๊กชนิดที่ผู้ใช้เสียข้อมูลจริง ๆ (แก้ฝั่งหนึ่งแล้วอีกฝั่งทับ)
การมี state สองก้อนใน item เดียวกินเพิ่มแค่ ~40 ไบต์ต่อ item = 40 KB ที่ 1000 ภาพ ถูกมากเทียบกับความถูกต้องที่ได้

**ผลตามมา:** สลับ mode ห้ามสร้าง Command ห้ามตั้ง dirty การเปลี่ยนข้อมูลข้าม mode เกิดได้ทางเดียวคือปุ่ม "Apply layout" / "Sort by canvas order" ที่ผู้ใช้กดเอง

---

## ADR-007 — Windows + Linux ก่อน macOS ทีหลัง

**ตัดสิน:** P0–P5 ทำ Windows + Linux, macOS เป็น P6

**เหตุผล:** wgpu/winit/egui รองรับ macOS อยู่แล้ว (Metal) งานที่เหลือจริง ๆ คือ:
- native menu bar (macOS คาดหวังเมนูบนสุดของจอ ไม่ใช่ในหน้าต่าง)
- code signing + notarization (ต้องมี Apple Developer account $99/ปี)
- คีย์ Cmd แทน Ctrl ทั่วทั้ง keymap
- file dialog + sandbox entitlements

**สิ่งที่ต้องทำตั้งแต่วันนี้เพื่อให้ P6 ไม่เจ็บ:** ทุกโค้ดที่แตะ OS ต้องอยู่ใน `refx-platform` เท่านั้น และ keymap ต้องเป็น data ไม่ใช่ hard-code

---

## ADR-008 — Thumbnail 128px ไม่ใช่ 256px

**ตัดสิน:** 128×128

**คำนวณ @ 1000 ภาพ:**

| ขนาด | RGBA8 | BC7 | เห็นความต่างไหม |
|---|---|---|---|
| 128 | 64 MB | **16 MB** | พอสำหรับ zoom-out (ภาพแสดง < 128px อยู่แล้ว) |
| 256 | 256 MB | 64 MB | ต่างเฉพาะช่วงซูมแคบ ๆ ก่อนชั้น T1 จะเข้ามา |

128px ที่ BC7 ใช้ 16 MB — เก็บทั้งหมดใน VRAM ตลอดเวลาได้โดยไม่ต้อง evict ซึ่งกำจัดอาการ "ภาพหายเป็นช่อง ๆ ตอนซูมออก" ที่ทำลายความรู้สึกว่าโปรแกรมเสถียร
ช่วงที่ 128px ไม่พอ ชั้น T1 (working texture) รับช่วงต่อพอดี

---

## ADR-009 — postcard + zstd ไม่ใช่ JSON

**ตัดสิน:** `postcard` (binary) บีบด้วย `zstd -3`

**เหตุผล:** document ที่มี 1000 item เป็น JSON ≈ 2 MB, postcard+zstd ≈ 180 KB → parse เร็วกว่า ~10 เท่า และ postcard มี bound ที่ควบคุมได้ชัด (สำคัญต่อ T3)

**ทางเลือกที่ไม่เอา:** `bincode` — deserialize `Vec` แบบไม่มี bound ทำให้ไฟล์ที่ประกาศ length 4 พันล้านทำให้ OOM ทันที เป็นช่องโหว่ที่ทราบกันดี

**ต้นทุน:** อ่านด้วยตาเปล่าไม่ได้ → **บรรเทาด้วย `xtask dump-refx <file>` ที่แปลงเป็น JSON ให้อ่าน/debug** (ต้องทำใน P4 ไม่ใช่ทีหลัง)
