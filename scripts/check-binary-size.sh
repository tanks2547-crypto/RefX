#!/usr/bin/env bash
#
# เพดานขนาด binary (docs/08 §6) — บังคับอัตโนมัติ ไม่ใช่ความหวัง
#
# ★ ทำไมต้องมี: งบที่ไม่มีใครตรวจอัตโนมัติคืองบที่จะถูกละเมิดเงียบ ๆ
#   และเรามีหลักฐานแล้วว่ามันเกิดขึ้นจริง — เอกสารเขียน 12 MB ขณะที่ของจริง 15.35 MB
#   ต่างกัน 3.3 MB โดยไม่มีใครรู้ตัว จนกระทั่งมีคนไปวัดด้วยมือ (docs/08 §3.9)
#
# พิมพ์ขนาดปัจจุบัน **ทุกครั้ง** ไม่ว่าผ่านหรือไม่ผ่าน เพื่อให้เห็นแนวโน้มใน log ของ CI
# ย้อนหลังได้ว่า commit ไหนทำให้โตขึ้น
#
# ใช้:
#   scripts/check-binary-size.sh
#   REFX_MAX_BINARY_BYTES=1 scripts/check-binary-size.sh   # negative control
#
# ★ negative control (docs/08 §3.9 ข้อ 1): ตั้งเพดานเป็น 1 ไบต์แล้วสคริปต์นี้
#   **ต้องล้ม** ถ้าไม่ล้มแปลว่าตัวตรวจเองพัง แล้วเพดานจะกลายเป็นสัญญาณเขียวปลอม
#   อีกอันหนึ่ง ซึ่งเป็นสิ่งที่โปรเจกต์นี้โดนมาแล้วสองครั้ง

set -euo pipefail

# เพดานจาก docs/08 §6 · override ได้เพื่อทำ negative control เท่านั้น
LIMIT="${REFX_MAX_BINARY_BYTES:-$((25 * 1024 * 1024))}"

# ตรวจ .exe ก่อนโดยตั้งใจ — Git Bash บน Windows ทำ `test -f target/release/refx`
# ให้เป็นจริงโดยไปเจอ `refx.exe` ให้เอง ซึ่งได้ผลถูกแต่กำกวมเวลาอ่าน log
BIN=""
for candidate in target/release/refx.exe target/release/refx; do
  if [ -f "$candidate" ]; then
    BIN="$candidate"
    break
  fi
done

if [ -z "$BIN" ]; then
  echo "::error::ไม่พบ binary ที่ target/release/refx[.exe] — ต้อง build ก่อนเรียกสคริปต์นี้"
  exit 1
fi

SIZE=$(wc -c < "$BIN" | tr -d '[:space:]')

# awk เพราะ bash ทำเลขทศนิยมเองไม่ได้
MB=$(awk -v s="$SIZE" 'BEGIN { printf "%.2f", s / 1048576 }')
LIMIT_MB=$(awk -v s="$LIMIT" 'BEGIN { printf "%.2f", s / 1048576 }')
PCT=$(awk -v s="$SIZE" -v l="$LIMIT" 'BEGIN { printf "%.1f", (l > 0 ? s * 100 / l : 0) }')

# ★ พิมพ์เสมอ ทั้งตอนผ่านและตอนไม่ผ่าน
echo "binary: $BIN"
echo "ขนาด  : $SIZE ไบต์ ($MB MB)"
echo "เพดาน : $LIMIT ไบต์ ($LIMIT_MB MB) — ใช้ไป $PCT %"
echo "::notice title=ขนาด binary::$MB MB จากเพดาน $LIMIT_MB MB ($PCT %)"

if [ "$SIZE" -gt "$LIMIT" ]; then
  echo "::error title=binary เกินเพดาน::$MB MB เกินเพดาน $LIMIT_MB MB ที่ docs/08 §6 กำหนด"
  echo "อย่าขยับเพดานเพื่อให้ผ่าน — หาว่าอะไรทำให้โตก่อน (เทียบกับ commit ก่อนหน้าด้วย git worktree)"
  exit 1
fi
