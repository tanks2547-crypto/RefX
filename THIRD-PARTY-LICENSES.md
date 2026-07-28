# License ของงานบุคคลที่สามที่แจกจ่ายไปกับ RefX

เอกสารนี้ครอบคลุมสิ่งที่ **ถูกฝังลงใน binary แล้วส่งต่อถึงผู้ใช้** ไม่ใช่แค่สิ่งที่ใช้ตอน build

license ของ crate ทุกตัวถูกบังคับด้วย `cargo deny check licenses` (ดู `deny.toml`)
ส่วนไฟล์ asset ที่ไม่ใช่ crate — เช่นฟอนต์ — `cargo deny` **มองไม่เห็น** จึงต้องบันทึกที่นี่ด้วยมือ

---

## ฟอนต์

### Noto Sans Thai

| | |
|---|---|
| ไฟล์ | `assets/fonts/NotoSansThai-Regular.ttf` |
| เวอร์ชัน | release tag `NotoSansThai-v2.002` |
| ที่มา | <https://github.com/notofonts/thai> |
| license | **SIL Open Font License 1.1** — ตัวเต็มอยู่ที่ `assets/fonts/OFL.txt` |
| ลิขสิทธิ์ | Copyright 2012 Google Inc. All Rights Reserved. |

SHA-256 และ URL เต็มของไฟล์ที่ดึงมาอยู่ใน `assets/fonts/CHECKSUMS.txt`

**เงื่อนไขที่ต้องทำตามเวลาแจกจ่าย** (สรุปจาก OFL 1.1 — ตัวบทจริงอยู่ใน `OFL.txt`):

- ต้องแจกจ่าย `OFL.txt` ไปพร้อมกับฟอนต์เสมอ ทั้งในรูปแบบ source และแบบที่ฝังไปแล้ว
- ห้ามขายฟอนต์แยกต่างหาก (แจกไปกับซอฟต์แวร์ได้)
- ถ้าดัดแปลงฟอนต์ **ห้ามใช้ชื่อ "Noto"** ในชื่อฟอนต์ที่ดัดแปลง — RefX ไม่ได้ดัดแปลง
- ต้องไม่ถอด copyright notice ออก

### Ubuntu / Hack / Noto Emoji (มากับ egui)

ฝังมากับ crate `epaint_default_fonts` ซึ่งเป็น dependency ปกติ →
`cargo deny` ตรวจให้แล้ว (`Ubuntu-font-1.0`, `OFL-1.1`)

---

## ยังไม่รองรับ CJK — และทำไม

ฟอนต์ CJK ที่ครบชุดมีขนาด 16+ MB ซึ่ง**ทะลุเพดาน binary 25 MB** ใน `docs/08 §6` ทันที
(ดู `docs/03 §0`) ถ้าจะรองรับต้องกลับมาคุยเรื่องเพดานก่อน

**ห้ามแก้ด้วยการดาวน์โหลดฟอนต์ตอนรัน** — I-8 ห้ามมี network เด็ดขาด

---

## กฎสำหรับ asset ชิ้นถัดไป

ทุกไฟล์ที่จะถูกฝังลง binary ต้องมีครบสี่อย่างก่อน merge:

1. ปักหมุดที่ **release tag ที่ระบุได้** ห้ามดึงจาก branch ที่ขยับได้
2. SHA-256 + วันที่ + URL เต็ม ลงใน `CHECKSUMS.txt` ของโฟลเดอร์นั้น
3. ไฟล์ license ตัวเต็มวางคู่กับ asset
4. หนึ่งหัวข้อในไฟล์นี้

รายละเอียดเหตุผล: `docs/06-security.md §2.5`
