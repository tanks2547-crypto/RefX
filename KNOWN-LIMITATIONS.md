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

## 4. ยังไม่มีไอคอนของโปรแกรม / No application icon yet

หน้าต่าง แถบงาน และเมนูของระบบจะแสดงไอคอนกลาง ๆ ของ OS ·
และ **AppImage ยังสร้างไม่ได้ด้วยเหตุผลนี้** (`appimagetool` บังคับต้องมีไอคอน)

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
| Linux — AppImage | ❌ ยังไม่มี (ดูข้อ 4) |
| macOS | ❌ ยังไม่รองรับ (`ROADMAP` P6) |

## 7. สิ่งที่ **ตั้งใจ** ไม่มี / Deliberately absent

ไม่ใช่ข้อจำกัด แต่เขียนไว้กันเข้าใจผิดว่าลืม:

* **ไม่มีการเชื่อมต่อเครือข่ายเลย** — ไม่มี auto-update ไม่มี telemetry
  ไม่มี cloud sync · โปรแกรมนี้ไม่เปิด socket ใด ๆ ทั้งสิ้น และมีประตูใน CI
  ที่ทำให้ build ล้มถ้ามีใครเพิ่ม network library เข้ามา
* **ไม่มีเครื่องมือวาด** — นี่เป็นเครื่องมือดู reference ไม่ใช่โปรแกรมวาด
* **`portable` zip ไม่ได้ย้ายที่เก็บข้อมูล** — มันแค่ไม่ต้องติดตั้ง
  ข้อมูลยังอยู่ในโฟลเดอร์มาตรฐานของ OS (ดู `README.txt` ในซิป)
