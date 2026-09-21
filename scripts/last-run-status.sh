#!/usr/bin/env bash
#
# ประตู "รอบล่าสุดของ workflow นั้นเป็นอย่างไร" — **ตัวเดียว ใช้ได้ทุก workflow**
#
# ## ★★★ ทำไมต้องมี (`docs/08 §3.9` ข้อ 12)
#
# งานที่รันตามตารางมีจุดอ่อนเดียวกันหมด: **ผลของมันไม่มีใครถูกบังคับให้อ่าน**
# · Windows แดงเงียบมาสองสัปดาห์เพราะไม่มีอะไรจุดให้เห็น จนกระทั่ง `cold-build`
# กลายเป็น job แรกที่สะดุดให้ดู
#
# วิธีแก้คือประตูที่รัน **ทุก push** แล้วอ่านผลของรอบตามตารางล่าสุด → ถ้ามันแดง
# หรือ **หายไปนานเกินกำหนด** push ถัดไปจะแดงทันทีพร้อมลิงก์ไปที่รอบนั้น
#
# ★★ "ไม่ได้รัน" ต้องอ่านค่า **เท่ากับแดง** ไม่ใช่เท่ากับเขียว — ตารางที่เงียบ
# เพราะ cron พัง หน้าตาเหมือนตารางที่เงียบเพราะทุกอย่างปกติทุกประการ
#
# ## ★★ ทำไมเป็นสคริปต์ ไม่ใช่ก๊อป job ซ้ำสามรอบ
#
# 20 ก.ย. 2026 เราลดความถี่ของ fuzz · mutation · cold-build พร้อมกัน แต่ละตัว
# ต้องมีประตูของตัวเอง · ถ้าก๊อป YAML สามชุด วันที่มีคนแก้ข้อความ error
# ให้ดีขึ้นจะแก้แค่ชุดเดียว แล้วอีกสองชุดจะ drift ออกไปเงียบ ๆ
# (รูปเดียวกับที่ `ci.yml` เลี่ยงการก๊อป job ไปทำ `check-windows` แยก)
#
# ใช้:
#   scripts/last-run-status.sh <workflow.yml> <เพดานอายุเป็นวัน> [ชื่อที่จะแสดง] [ชื่อ job]
#
# ★★★ อาร์กิวเมนต์ที่สี่ — **ถามถึง job ไม่ใช่ run**
#
# ขา Windows ไม่ได้อยู่ใน workflow ของตัวเอง มันเป็นแขนหนึ่งของ matrix ใน
# `ci.yml` · การแยกมันออกไปเป็นไฟล์ใหม่เพื่อให้ประตูอ่านง่ายจะได้ **นิยาม job
# สองชุดที่ต้องตรงกันเอง** ซึ่ง `ci.yml` เขียนเตือนไว้เองว่าเป็นจุดที่มัน drift
# ตั้งแต่วันแรกที่มีใครแก้ข้างเดียว
#
# → ให้ประตูทำงานหนักขึ้นแทน: ไล่ดู run ล่าสุดของ workflow นั้นจนเจอรอบที่
#   **มี job ชื่อนั้นรันจริง** แล้วอ่านผลของ job ตัวนั้น · รอบที่ข้ามมันไป
#   (เช่น push ที่ไม่ได้รัน Windows) ถูกมองข้าม ไม่ใช่ถูกนับว่าผ่าน
#
# ต้องมี `GH_TOKEN` กับ `GITHUB_REPOSITORY` ในสภาพแวดล้อม
#
# ★ negative control: ตั้งเพดานเป็น -1 แล้วประตูนี้ **ต้องแดง** ไม่ว่ารอบล่าสุด
#   จะเพิ่งจบไปเมื่อวินาทีที่แล้วก็ตาม

set -uo pipefail

WORKFLOW="${1:?ระบุชื่อไฟล์ workflow เช่น cold-build.yml}"
MAX_AGE_DAYS="${2:?ระบุเพดานอายุเป็นวัน}"
LABEL="${3:-$WORKFLOW}"
JOB="${4:-}"

branch="${GITHUB_DEFAULT_BRANCH:-main}"

# ★★★ ใช้ `--jq` **ในตัว `gh`** ไม่ใช่ `jq` ของระบบ
#
#   รอบแรกเขียนด้วย `jq` ภายนอก แล้วบนเครื่องที่ไม่มีมันติดตั้ง สคริปต์พิมพ์ว่า
#   **"รอบล่าสุดไม่ผ่าน"** ทั้งที่สาเหตุจริงคือหาเครื่องมือไม่เจอ —
#   คำวินิจฉัยปลอมที่ชี้ไปผิดที่ทั้งหมด (`docs/08 §3.9` ข้อ 9)
#   · `gh` มี jq อยู่ข้างในอยู่แล้ว จึงไม่ต้องพึ่งของนอกเลย
#
# รูปแบบที่ส่งกลับมา: `<conclusion> <runId> <sha> <updatedAt>` บรรทัดเดียว
one_line='.[0] | "\(.conclusion) \(.databaseId) \(.headSha) \(.updatedAt)"'

if [ -z "$JOB" ]; then
  line=$(gh run list --repo "$GITHUB_REPOSITORY" \
           --workflow "$WORKFLOW" --branch "$branch" \
           --status completed --limit 1 \
           --json conclusion,databaseId,updatedAt,headSha \
           --jq "$one_line" 2>/dev/null)
else
  # ★ ไล่จากใหม่ไปเก่า หยุดที่รอบแรกที่ job นั้น **ไม่ถูกข้าม**
  #   จำกัด 30 รอบเพื่อไม่ให้ประตูนี้กลายเป็นงานหนักของมันเอง
  line=""
  while read -r id sha when; do
    [ -n "$id" ] || continue
    got=$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$id/jobs?per_page=100" \
            --jq "[.jobs[] | select(.name | contains(\"$JOB\")) | select(.conclusion != \"skipped\" and .conclusion != null)] | .[0].conclusion // empty" 2>/dev/null)
    if [ -n "$got" ]; then
      line="$got $id $sha $when"
      break
    fi
  done <<EOF
$(gh run list --repo "$GITHUB_REPOSITORY" \
    --workflow "$WORKFLOW" --branch "$branch" \
    --status completed --limit 30 \
    --json databaseId,updatedAt,headSha \
    --jq '.[] | "\(.databaseId) \(.headSha) \(.updatedAt)"' 2>/dev/null)
EOF
fi

if [ -z "$line" ]; then
  if [ -n "$JOB" ]; then
    echo "::error title=ไม่เจอรอบที่ job '$JOB' รันจริง::ดู 30 รอบล่าสุดของ $WORKFLOW บน $branch แล้วไม่มีรอบไหนที่ job นี้ไม่ถูกข้าม — กด Run workflow ก่อน"
  else
    echo "::error title=ยังไม่เคยมีรอบ $LABEL ที่จบเลย::กด Run workflow ของ $WORKFLOW ก่อน — ประตูนี้อ่านผลของมัน"
  fi
  exit 1
fi

read -r conclusion run_id full_sha when <<EOF
$line
EOF
sha=$(echo "$full_sha" | cut -c1-7)
url="$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$run_id"
age=$(( ( $(date -u +%s) - $(date -u -d "$when" +%s) ) / 86400 ))

# ★ พิมพ์เสมอ ทั้งตอนผ่านและไม่ผ่าน — ขอบที่มองเห็นทุกรอบคือสิ่งที่ทำให้
#   "เหลืออีกวันเดียวจะเกินเพดาน" ไม่กลายเป็นเซอร์ไพรส์
echo "$LABEL รอบล่าสุด: $run_id ($sha) · $conclusion · $when · เก่า $age วัน (เพดาน $MAX_AGE_DAYS)"

if [ "$conclusion" != "success" ]; then
  echo "::error title=$LABEL ล่าสุดไม่ผ่าน::$conclusion ที่ $sha — $url"
  exit 1
fi
if [ "$age" -gt "$MAX_AGE_DAYS" ]; then
  echo "::error title=$LABEL เก่าเกินไป::ผ่านล่าสุดเมื่อ $age วันก่อน (เพดาน $MAX_AGE_DAYS) — $url"
  exit 1
fi
echo "::notice title=$LABEL::ผ่านที่ $sha เมื่อ $age วันก่อน — $url"
