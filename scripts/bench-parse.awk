# แปลงผลของ `cargo bench -- --output-format bencher` เป็น "NAME NS" บรรทัดละแถว
#
# ★ ทำไมไม่ใช่ regex บรรทัดเดียวอย่างที่เคยเป็น: บน runner ที่ยังไม่มี
#   `target/criterion/` มาก่อน criterion พ่น
#
#       test cull_1000 ... Criterion.rs ERROR: ... base/sample.json ... (os error 2)
#       bench:         833 ns/iter (+/- 24)
#
#   ชื่อกับตัวเลข **คนละบรรทัด** เพราะข้อความ ERROR ออกทาง stderr แล้วมาแทรก
#   กลางบรรทัดที่ criterion กำลังพิมพ์ทาง stdout ค้างไว้
#
#   เครื่องที่เคยรัน bench มาก่อนจะไม่มีวันเห็นรูปนี้ (มี baseline ให้อ่านแล้ว)
#   — CI เห็นทุกครั้ง เพราะ checkout ใหม่เสมอ นี่คือเหตุที่ประตูนี้เขียวบนเครื่อง
#   แต่แดงบน CI สามรอบติด
#
# ★★ ถ้าเจอ "bench: N ns/iter" ที่ไม่มีชื่อ test ค้างอยู่ → exit 3
#    ตัวเลขที่จับคู่ผิดแถวอันตรายกว่าไม่ได้ตัวเลขเลย เพราะมันจะถูกเทียบกับ
#    เพดานของ bench ตัวอื่นแล้วผ่านฟรี — เขียวปลอมแบบที่ docs/08 §3.9 ข้อ 2 ห้าม

/^test / { name = $2; have = 1 }

/bench:/ {
  for (i = 1; i <= NF; i++) {
    if ($i == "ns/iter") {
      if (have) {
        print name, $(i - 1)
        have = 0
      } else {
        orphan = 1
      }
    }
  }
}

END { if (orphan) exit 3 }
