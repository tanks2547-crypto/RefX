# RefX

โปรแกรมจัดการภาพ reference สำหรับนักวาด — สองโหมด (Canvas / Arrange) บน UI กลางเดียวกัน

**เป้าหมายเรียงตามลำดับ: เสถียร → ปลอดภัย → กิน CPU/RAM น้อย → ฟีเจอร์**

Rust · wgpu · egui · ไม่มี network · ไม่มี C codec · idle 0% CPU

## เริ่มต้น

```bash
cargo xtask gen-testdata     # สร้าง dataset ทดสอบ
cargo run --release
```

## เอกสาร

| ไฟล์ | อ่านเมื่อ |
|---|---|
| [CLAUDE.md](CLAUDE.md) | ★ ก่อนเขียนโค้ดบรรทัดแรก |
| [ARCHITECTURE.md](ARCHITECTURE.md) | ภาพรวม + invariant 8 ข้อ |
| [ROADMAP.md](ROADMAP.md) | หา task ถัดไป |
| [docs/](docs/) | spec รายส่วน |
| [docs/09-crate-versions.md](docs/09-crate-versions.md) | ★ ก่อนแตะ Cargo.toml |

## เพดานที่ต้องผ่าน (board 1000 ภาพ)

| | เพดาน |
|---|---|
| RAM idle | 250 MB |
| VRAM idle | 200 MB |
| CPU idle | **0 %** |
| frame time (pan/zoom) | 8 ms |
| เปิดโปรแกรม | 400 ms |

## สัญญาอนุญาต

RefX แจกจ่ายแบบ **เลือกได้อย่างใดอย่างหนึ่ง** ระหว่าง

- [MIT](LICENSE-MIT) — [`LICENSE-MIT`](LICENSE-MIT)
- [Apache-2.0](LICENSE-APACHE) — [`LICENSE-APACHE`](LICENSE-APACHE)

(ตรงกับที่ `Cargo.toml` ประกาศไว้ว่า `license = "MIT OR Apache-2.0"` ซึ่งเป็น
รูปแบบมาตรฐานของโปรเจกต์ Rust)

**ของคนอื่นที่ฝังมาด้วย:** ฟอนต์ `assets/fonts/NotoSansThai-Regular.ttf` อยู่ใต้
SIL Open Font License 1.1 ซึ่ง **บังคับให้แจกไฟล์สัญญาอนุญาตไปคู่กันเสมอ** —
ดู [`assets/fonts/OFL.txt`](assets/fonts/OFL.txt) และรายการเต็มของ dependency
ทั้งหมดใน [`THIRD-PARTY-LICENSES.md`](THIRD-PARTY-LICENSES.md)
