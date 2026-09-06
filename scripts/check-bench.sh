#!/usr/bin/env bash
#
# เพดาน benchmark ของ `docs/08 §2` — บังคับอัตโนมัติ ไม่ใช่ความหวัง (P5-1)
#
# ★ ทำไมต้องมี: ตาราง 10 แถวใน `docs/08 §2` เขียนว่า "fail CI ถ้าเกิน" มาตั้งแต่
#   วันแรก แต่ **9 ใน 10 ไม่เคยถูกวัดเลยสักครั้ง** — มีแต่ `binary_size` ที่บังคับจริง
#   เอกสารเองยอมรับข้อนี้ไว้ในกล่อง ⚠️ ที่หัวตาราง
#
# ★★ สคริปต์นี้ครอบ **เฉพาะแถวที่วัดได้โดยไม่ต้องมี GPU และไม่ต้องมี dataset
#    หลาย GB** — ซึ่งเป็นแถวที่เอาเข้า CI ทุก push ได้จริงโดยไม่ชนงบ Actions
#    แถวที่เหลือต้องรันแอปจริงพร้อมภาพ 1000 ใบ (ดู `docs/08 §2` และรายงานของ P5-1)
#
# วิธีวัด: `criterion` ผ่าน `--output-format bencher` แล้วอ่านค่ามัธยฐานที่มันพิมพ์
# **ไม่วัดเอง** — สองตัววัดที่วัดของเดียวกันคนละที่จะ drift (docs/08 §3.9)
#
# ใช้:
#   scripts/check-bench.sh
#   REFX_BENCH_LIMIT_NS=1 scripts/check-bench.sh    # negative control
#
# ★ negative control (docs/08 §3.9 ข้อ 1): บังคับเพดานทุกแถวเป็น 1 ns แล้วสคริปต์นี้
#   **ต้องล้ม** ถ้าไม่ล้มแปลว่าตัวตรวจเองพัง แล้วเพดานจะกลายเป็นสัญญาณเขียวปลอม
#   อีกอันหนึ่ง — รูปแบบเดียวกับที่โปรเจกต์นี้โดนมาแล้วสี่ครั้ง

set -uo pipefail

DIR=$(cd "$(dirname "$0")" && pwd)
PARSER="$DIR/bench-parse.awk"

# ---------------------------------------------------------------------------
# ★ ตัวอ่านผลต้องพิสูจน์ตัวเองก่อนที่เราจะเชื่อมัน (docs/08 §3.9 ข้อ 9)
#
# ประตูนี้เขียวบนเครื่อง dev แต่แดงบน CI สามรอบติด เพราะรูปของ output ต่างกัน:
# runner ที่ยังไม่มี `target/criterion/` โดน "Criterion.rs ERROR" แทรกผ่าบรรทัด
# `test NAME ... bench:` ออกเป็นสองท่อน ตัวอ่านผลเดิมจึงไม่เจอสักแถว
#
# ทางแก้ไม่ใช่ "แก้ regex แล้วหวังว่ารอบหน้าถูก" — เก็บ output จริงจาก CI ไว้เป็น
# ตัวอย่าง แล้วให้สคริปต์ตรวจตัวเองกับมันทุกครั้งที่รัน เครื่อง dev จึงเห็นรูปที่
# ตัวเองไม่มีวันสร้างได้
# ---------------------------------------------------------------------------
PARSE_EXPECT="cull_1000 833
layout_justified_1000 8090
build_instances_1000 20032"

PARSE_GOT=$(awk -f "$PARSER" < "$DIR/testdata/bench-bencher-output.txt")
if [ "$PARSE_GOT" != "$PARSE_EXPECT" ]; then
  echo "ได้:"; echo "$PARSE_GOT"
  echo "ควรได้:"; echo "$PARSE_EXPECT"
  echo "::error::ตัวอ่านผล bench อ่านตัวอย่างจาก CI ไม่ถูก — ยังไม่ต้องรัน bench เพราะผลจะเชื่อไม่ได้"
  exit 1
fi

awk -f "$PARSER" < "$DIR/testdata/bench-bencher-orphan.txt" > /dev/null 2>&1
if [ $? -ne 3 ]; then
  echo "::error::ตัวอ่านผล bench ยอมรับตัวเลขที่ไม่มีชื่อ test นำหน้า — มันจะจับคู่ตัวเลขผิดแถวแล้วผ่านฟรี"
  exit 1
fi

# ---------------------------------------------------------------------------
# เพดานจาก docs/08 §2 (แปลงเป็น ns) — ★ ห้ามขยับเพื่อให้ผ่าน
#
# ตัวเลขพวกนี้เป็น *สัญญา* ที่เขียนไว้ตอนออกแบบ ถ้าวัดแล้วผ่านไม่ได้จริง
# ให้หยุดแล้วรายงาน ไม่ใช่แก้ตัวเลขที่นี่ (บทเรียนของ "1000 ภาพ 4 วินาที"
# ที่ผิด 20 เท่าแล้วไม่มีใครรู้อยู่หลาย session)
# ---------------------------------------------------------------------------
limit_of() {
  case "$1" in
    cull_1000)             echo 200000 ;;    # 200 µs
    build_instances_1000)  echo 300000 ;;    # 300 µs
    layout_justified_1000) echo 2000000 ;;   # 2 ms
    *)                     echo "" ;;
  esac
}

EXPECTED="cull_1000 layout_justified_1000 build_instances_1000"

OUT=""
for target in "refx-core --bench core_benches" "refx-ui --bench instances"; do
  # shellcheck disable=SC2086
  PART=$(cargo bench -p $target -- --output-format bencher 2>&1)
  STATUS=$?
  if [ "$STATUS" -ne 0 ]; then
    echo "$PART"
    echo "::error::รัน benchmark ของ $target ไม่สำเร็จ (exit $STATUS)"
    exit 1
  fi
  OUT="$OUT
$PART"
done

# บรรทัดตรวจสภาพที่ bench พิมพ์เอง (เช่น "เห็น 27 จาก 1000 ใบ") ต้องอยู่ใน log
# เสมอ — มันคือสิ่งที่บอกว่าเราวัดของจริง ไม่ใช่วัดกรอบว่าง/กิ่งที่ถูกที่สุด
echo "$OUT" | grep -E '^(cull_1000|layout|build_instances)' || true

# `test NAME ... bench:  N ns/iter (+/- M)` → "NAME N" (ดูหัวไฟล์ bench-parse.awk
# ว่าทำไมมันไม่ใช่ regex บรรทัดเดียว)
MEASURED=$(echo "$OUT" | awk -f "$PARSER")
PARSE_STATUS=$?

if [ "$PARSE_STATUS" -ne 0 ]; then
  echo "$OUT"
  echo "::error::ตัวอ่านผลเจอตัวเลข bench ที่ไม่มีชื่อ test นำหน้า — ไม่รู้ว่ามันเป็นของแถวไหน"
  exit 1
fi

if [ -z "$MEASURED" ]; then
  echo "$OUT"
  echo "::error::ไม่ได้ตัวเลขจาก criterion เลยสักแถว — รูปแบบ --output-format bencher เปลี่ยนไปหรือ bench ไม่ได้รัน"
  exit 1
fi

FAILED=0
echo
printf '%-26s %14s %14s %10s\n' "bench" "วัดได้ (ns)" "เพดาน (ns)" "headroom"
printf '%-26s %14s %14s %10s\n' "--------------------------" "--------------" "--------------" "----------"

while read -r NAME NS; do
  [ -z "$NAME" ] && continue
  LIMIT=$(limit_of "$NAME")
  if [ -z "$LIMIT" ]; then
    echo "::error::bench '$NAME' ไม่มีเพดานใน $0 — เพิ่ม bench แล้วต้องเพิ่มเพดานด้วย"
    FAILED=1
    continue
  fi
  # ★ override ทั้งตารางพร้อมกัน มีไว้ทำ negative control เท่านั้น
  LIMIT="${REFX_BENCH_LIMIT_NS:-$LIMIT}"

  RATIO=$(awk -v n="$NS" -v l="$LIMIT" 'BEGIN { printf "%.1f", (n > 0 ? l / n : 0) }')
  printf '%-26s %14s %14s %9sx\n' "$NAME" "$NS" "$LIMIT" "$RATIO"
  echo "::notice title=bench $NAME::$NS ns จากเพดาน $LIMIT ns (headroom ${RATIO}x)"

  if [ "$NS" -gt "$LIMIT" ]; then
    echo "::error title=$NAME เกินเพดาน::$NS ns เกินเพดาน $LIMIT ns ที่ docs/08 §2 กำหนด"
    echo "อย่าขยับเพดานเพื่อให้ผ่าน — หาว่าอะไรทำให้ช้าลงก่อน (เทียบกับ commit ก่อนหน้าด้วย git worktree)"
    FAILED=1
  fi
done <<< "$MEASURED"

# ★★ แถวที่ **หายไป** ต้องล้มด้วย — ไม่ใช่ผ่านเงียบ ๆ
#
# ถ้ามีคนเปลี่ยนชื่อ bench หรือลบมันทิ้ง สคริปต์นี้จะไม่เจอตัวเลขของมัน
# แล้ว "ไม่มีอะไรเกินเพดาน" จะกลายเป็นความจริงโดยที่ไม่มีอะไรถูกวัดเลย
# ซึ่งคือรูปแบบเขียวปลอมที่ `docs/08 §3.9` ข้อ 2 ห้ามไว้ตรง ๆ
for NAME in $EXPECTED; do
  if ! echo "$MEASURED" | grep -q "^$NAME "; then
    echo "::error::ไม่เจอผลของ bench '$NAME' — ถูกเปลี่ยนชื่อหรือถูกลบ? เพดานที่ไม่มีของให้วัดคือเพดานที่ผ่านฟรี"
    FAILED=1
  fi
done

echo
if [ "$FAILED" -ne 0 ]; then
  exit 1
fi
echo "ทุก bench อยู่ใต้เพดานของ docs/08 §2"
