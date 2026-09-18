# ข้อจำกัดที่รู้อยู่ / Known limitations

รายการนี้คือสิ่งที่ **เรารู้ว่ายังทำไม่ได้หรือทำได้ไม่ดี** ในรุ่นนี้ —
เขียนไว้เพื่อให้คุณไม่ต้องเสียเวลาหาว่าตัวเองทำอะไรผิด

This is what we know does not work, or does not work well, in this release.
It is here so you do not spend time wondering what you did wrong.

> ★ ไฟล์นี้เขียนด้วยมือ ไม่ใช่ของที่เครื่องสร้าง — ต่างจาก
> `KEYBOARD-SHORTCUTS.md` และ `THIRD-PARTY-LICENSES.md` ที่มีประตูคุมความตรง
> · ถ้าคุณเจอข้อจำกัดที่ไม่ได้อยู่ในนี้ แปลว่าเรายังไม่รู้ ไม่ใช่ว่าเราปิดบัง

---

## 1. Windows จะเตือนตอนเปิดครั้งแรก / Windows warns you the first time

**เราไม่มีใบรับรองสำหรับเซ็นโค้ด** → SmartScreen ขึ้นว่า *"Windows protected
your PC"* · กด **More info → Run anyway** ได้ · แพ็กเกจ `.deb` ก็ไม่มีลายเซ็น
เช่นกัน

We do not have a code-signing certificate, so SmartScreen shows a warning and
the `.deb` is unsigned. Buying a certificate is a decision for the project
owner, not something the build can work around.

## 2. `refx --help` อาจไม่แสดงอะไรบน Windows / may print nothing on Windows

โปรแกรมถูก build เป็น **GUI application** (ไม่มีหน้าต่างคอนโซลโผล่ตอนเปิด) ซึ่ง
แปลว่าเมื่อสั่งจาก console ที่เปิดอยู่แล้ว Windows จะไม่ผูก console ให้ ·
ข้อความจึงไม่ปรากฏ

**ทางแก้ชั่วคราว / Workaround:** ส่งผ่าน pipe หรือ redirect ซึ่งทำงานปกติ

```powershell
refx --help | Out-String
refx --help > help.txt
```

บน Linux ไม่มีปัญหานี้ · (ทางแก้จริงคือ `AttachConsole` — ยังไม่ได้ทำ)

## 3. ภาพที่ส่งออกยังคมได้เท่าภาพตัวอย่าง / Export is limited by the preview

**ตอนนี้ภาพที่ส่งออกถูกประกอบจาก thumbnail ขนาด 128 px** ไม่ใช่จากไฟล์ต้นฉบับ
ที่ความละเอียดเต็ม · ขนาดภาพที่ขอได้จึงถูกจำกัดตามจำนวนภาพบนกระดาน และภาพที่
ได้จะไม่คมเท่าต้นฉบับ

Export currently composes from the 128 px thumbnails, not from the original
files at full resolution, so the output is softer than the sources and the
size ceiling is computed from what is actually on the board.

→ กำลังทำอยู่ใน **P5-4b** (`ROADMAP`)

## 4. ไอคอนเป็นของชั่วคราว / The icon is a placeholder

ไอคอนปัจจุบันวาดขึ้นเองเพื่อให้มีของใช้ ไม่ใช่งานออกแบบที่ผ่านการคิดเรื่องแบรนด์ ·
**ตั้งใจให้ถูกแทนที่** · ต้นฉบับคือ `assets/icon/refx.svg` ไฟล์เดียว —
`.ico` / `.png` / ไอคอนหน้าต่างสร้างจากมันด้วย `cargo xtask icon` และมีประตูใน CI
คอยยืนยันว่าไม่หลุดจากกัน

The icon is a placeholder we drew to have something usable; it is meant to be
replaced. `assets/icon/refx.svg` is the only source — every raster form is
generated from it and a CI gate fails if they drift apart.

**ยังไม่ได้ฝังไอคอนลงในตัว `refx.exe`** → หน้าต่างและแถบงานมีไอคอนแล้ว แต่ไฟล์
`refx.exe` ใน File Explorer ยังเป็นไอคอนกลาง ๆ ของ Windows · การฝังต้องใช้
Windows resource ซึ่งต้องเพิ่ม build dependency ที่ยังไม่ได้ตัดสินใจ

## 5. ยังไม่รองรับ จีน/ญี่ปุ่น/เกาหลี / No CJK yet

UI มีภาษาอังกฤษกับไทย · ฟอนต์ CJK ที่ครบชุดใหญ่กว่า 16 MB ซึ่งทะลุเพดานขนาด
ของโปรแกรมทั้งตัว — ต้องตัดสินใจเรื่องเพดานก่อน ไม่ใช่แค่เพิ่มไฟล์
(`docs/03 §0`)

**ชื่อไฟล์ภาษา CJK ยังเปิดได้ตามปกติ** — ข้อจำกัดนี้เป็นเรื่องของ *ตัวอักษรที่
วาดบนหน้าจอ* เท่านั้น

## 6. แพ็กเกจที่มีให้ / Packages available

| รูปแบบ | สถานะ |
|---|---|
| Windows — portable zip | ✅ |
| Windows — MSI | ✅ |
| Linux — `.deb` | ✅ |
| Linux — AppImage | ❌ ยังไม่มีในรุ่นนี้ — เลื่อนไป v1.1 |
| macOS | ❌ ยังไม่รองรับ (`ROADMAP` P6) |

## 7. ตัวเลขใน Inspector ยัง **แก้ไม่ได้** / The Inspector numbers are read-only

ตอนนี้ **เห็นค่า** ได้ครบ: X / Y / กว้าง / สูง · การหมุน · ชื่อไฟล์ · ขนาดพิกเซล ·
รูปแบบไฟล์ · ขนาดไฟล์ · ที่อยู่เต็ม — แต่ **พิมพ์แก้ไม่ได้** ในรุ่นนี้
ย้ายและหมุนด้วยการลากบน canvas ได้ตามปกติ

**ยังไม่มีเลย:** ครอป · ล็อกไม่ให้ขยับ · ปุ่มเปิดโฟลเดอร์ของไฟล์

★ เราเลือกที่จะ **ไม่ใส่ช่องที่กดแล้วไม่เกิดอะไร** ไว้ล่วงหน้า — ช่องแบบนั้น
อ่านว่าโปรแกรมพัง ไม่ใช่ว่ายังไม่รองรับ

The Inspector shows every number but does not let you type new ones yet; drag
on the canvas to move and rotate. Crop, lock and "reveal in folder" are not in
this release at all — we left the controls out rather than ship ones that do
nothing.

## 8. Library ไม่ขึ้นชื่อโฟลเดอร์ / The Library panel stays empty

เปิดภาพทั้งโฟลเดอร์แล้วภาพขึ้นบนกระดานครบ แต่แผง **Library** ทางซ้ายยังเขียนว่า
*"Image folders appear here"* อยู่เหมือนเดิม · ใช้งานได้ปกติทุกอย่าง
แค่แผงนั้นยังไม่ผูกกับโฟลเดอร์ที่เปิด

## 9. ตัวเลข "RAM" บนแถบสถานะไม่ใช่หน่วยความจำของโปรแกรม

มันคือ **งบที่ตัวถอดรหัสภาพกำลังใช้อยู่ ณ ขณะนั้น** จึงเป็น `0 B` เมื่อถอดรหัส
เสร็จหมดแล้ว ซึ่งเป็นค่าที่ถูกต้อง ไม่ใช่มาตรวัดที่เสีย · หน่วยความจำรวมของ
โปรแกรมดูได้จาก Task Manager ตามปกติ

## 10. cache บนดิสก์ถูกล้างตอน **ปิดโปรแกรมอย่างเรียบร้อย** เท่านั้น

เพดานคือ 2 GB และการล้างเกิดตอนปิดโปรแกรม · ถ้าโปรแกรมถูกฆ่าหรือดับไปกลางคัน
หลายครั้งติดกัน โฟลเดอร์ cache อาจโตเกินเพดานไปจนกว่าจะได้ปิดอย่างเรียบร้อยสักครั้ง
· ลบทิ้งเองได้ตลอดเวลาอย่างปลอดภัย (`%LOCALAPPDATA%\RefX\cache`)

## 11. การกู้ device หลังเครื่องหลับ — **ยังไม่เคยเจอของจริง**

18 ก.ย. 2026 เจ้าของโปรเจกต์สั่งเครื่องหลับจริงแล้วปลุก (RTX 4060 · 9 ภาพที่ยังไม่เซฟ)
· โปรแกรมรอด ภาพครบ ไม่มี error — **แต่ `generation` ยังเป็น 0 แปลว่า device
ไม่เคยหาย** ไดรเวอร์พามันรอดข้ามการหลับมาได้ทั้งดุ้น

เส้นทาง "device หายแล้วกู้กลับ" จึงถูกตรวจด้วย **การจำลอง** เท่านั้น
(`--force-device-lost-after-ms` → `generation` 0→1 · กู้ใน 118 ms · ภาพครบ 20 ใบ)

→ เครื่องที่ไดรเวอร์ทำ device หายตอนหลับจริง ๆ — iGPU บางรุ่น · หลัง driver update ·
เครื่องที่สลับ iGPU↔dGPU ตอนเสียบ/ถอดสายไฟ — **ยังไม่มีใครลอง** ถ้าเจออาการ
"ตื่นมาแล้วจอดำ/ภาพหาย" กรุณาส่ง `%LOCALAPPDATA%\RefX\cache\logs\refx.log` มาด้วย

## 12. ตัวจับ I-1 ร้องตอน **ลากไฟล์ค้างเหนือหน้าต่าง**

log อาจมีบรรทัด `I-1: ขอวาดเฟรมต่อเนื่องทั้งที่ไม่มี input ... who=EguiRepaint`
ช่วงที่กำลังลากไฟล์เข้ามาแต่ยังไม่ปล่อย · **ไม่ใช่อาการเสีย และไม่กินเครื่องตอน idle**

วัดแล้ววันเดียวกัน: ปล่อยโปรแกรมนิ่ง 10 วินาที ใช้ CPU **15.6 ms = 0.156 % ของ
หนึ่งคอร์** และตัวจับไม่แตะเพดานเลย · บรรทัดที่ร้องทั้ง 4 ครั้งของวันนั้น**เริ่ม
ไม่กี่ร้อย ms ก่อนการปล่อยไฟล์ และจบพอดีตอนปล่อย** ทุกครั้ง

สมมติฐาน (**ยังไม่ได้พิสูจน์ด้วยการวัด** — เขียนไว้ตรง ๆ): Windows ส่ง `HoveredFile`
ครั้งเดียวตอนลากเข้ามา ระหว่างค้างเมาส์ไว้ winit ไม่ส่งอะไรอีก ขณะที่ egui วาด
ไฮไลต์เป้าหมายต่อเนื่อง → ตัวนับมองว่า "ไม่มี input" ทั้งที่ผู้ใช้กำลังโต้ตอบอยู่
· ถ้าจริง นี่คือ**ความแม่นของตัวจับ ไม่ใช่การละเมิด I-1** · ทางพิสูจน์: แยกเหตุผล
`DragHover` ออกจาก `EguiRepaint` แล้วดูว่าชื่อผู้ขอเปลี่ยนไหม

## 13. สิ่งที่ **ตั้งใจ** ไม่มี / Deliberately absent

ไม่ใช่ข้อจำกัด แต่เขียนไว้กันเข้าใจผิดว่าลืม:

* **ไม่มีการเชื่อมต่อเครือข่ายเลย** — ไม่มี auto-update ไม่มี telemetry
  ไม่มี cloud sync · โปรแกรมนี้ไม่เปิด socket ใด ๆ ทั้งสิ้น และมีประตูใน CI
  ที่ทำให้ build ล้มถ้ามีใครเพิ่ม network library เข้ามา
* **ไม่มีเครื่องมือวาด** — นี่เป็นเครื่องมือดู reference ไม่ใช่โปรแกรมวาด
* **`portable` zip ไม่ได้ย้ายที่เก็บข้อมูล** — มันแค่ไม่ต้องติดตั้ง
  ข้อมูลยังอยู่ในโฟลเดอร์มาตรฐานของ OS (ดู `README.txt` ในซิป)
