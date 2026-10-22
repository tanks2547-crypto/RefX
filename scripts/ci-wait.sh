#!/usr/bin/env bash
#
# รอ CI ของคอมมิตหนึ่งตัวจนจบจริง แล้วรายงาน **ความจริงของตัวเอง**
#
# ## ★★★ ทำไมต้องมี (เจอสองครั้ง 12–14 ต.ค. 2026)
#
# 1. `gh run list --commit <SHA สั้น>` คืนรายการว่าง → สคริปต์รอบนั้นอ่านว่า
#    **"ไม่มี run"** ทั้งที่มี · gh เทียบ SHA เต็มเท่านั้น
# 2. `gh run watch` **จบเองด้วย exit 1** ระหว่างที่ job ยังรันอยู่ → คำสั่งที่ต่อท้าย
#    พิมพ์ผลออกมาแล้วจบด้วย 0 · แจ้งเตือนเขียนว่า "exit code 0" ทั้งที่ run ยัง
#    `in_progress` — รูปเดียวกับ `cmd | tail && echo ok` ใน CLAUDE.md
#
# → สคริปต์นี้:
#   * แปลงเป็น SHA เต็มเองเสมอ (`git rev-parse`)
#   * **หาไม่เจอ ≠ ไม่มี** — ลองซ้ำจนหมดเวลา แล้วพูดว่า "หาไม่เจอ" พร้อม exit ของตัวเอง
#   * ไม่ใช้ `gh run watch` · ถามสถานะเองจนเป็น `completed` จริง
#   * exit 0 **เฉพาะ** เมื่อทุก run ของคอมมิตนั้นจบด้วย `success`
#   * ★ job ที่ `skipped` ถูกพิมพ์แยกให้เห็น — "ข้าม" ไม่ใช่ "ผ่าน" (`docs/08 §3.9`)
#
# ใช้:   scripts/ci-wait.sh [คอมมิต=HEAD] [เพดานนาที=60]
#
# exit:  0 = ทุก run สำเร็จ
#        1 = มี run ที่จบแต่ไม่สำเร็จ (failure · cancelled · …)
#        2 = หา run ของคอมมิตนี้ไม่เจอภายใน 3 นาที (ไม่ได้แปลว่าไม่มี)
#        3 = เกินเพดานเวลาแล้ว run ยังไม่จบ
#        4 = ถาม gh ไม่สำเร็จ / อาร์กิวเมนต์ผิด
#
# ★ negative control: `scripts/ci-wait.sh 0000000000000000000000000000000000000000`
#   ต้องได้ exit 4 (SHA ที่ไม่มีใน repo) · คอมมิตจริงที่ไม่ได้ push ต้องได้ exit 2

set -u

ref="${1:-HEAD}"
limit_min="${2:-60}"

if ! sha="$(git rev-parse --verify --quiet "${ref}^{commit}")"; then
  echo "ci-wait: '${ref}' ไม่ใช่คอมมิตใน repo นี้"
  exit 4
fi

deadline=$(( $(date +%s) + limit_min * 60 ))
find_deadline=$(( $(date +%s) + 180 ))

# ---- 1. หา run ของคอมมิตนี้ (SHA เต็ม) ----
ids=""
while [ -z "$ids" ]; do
  if ! out="$(gh run list --commit "$sha" --json databaseId --jq '.[].databaseId' 2>&1)"; then
    echo "ci-wait: ถาม gh ไม่สำเร็จ: $out"
    exit 4
  fi
  ids="$out"
  if [ -z "$ids" ]; then
    if [ "$(date +%s)" -ge "$find_deadline" ]; then
      echo "ci-wait: หา run ของ ${sha} ไม่เจอภายใน 3 นาที — ไม่ได้แปลว่าไม่มี (push แล้วหรือยัง · workflow ถูกกรองด้วย paths ไหม)"
      exit 2
    fi
    sleep 10
  fi
done

# ---- 2. รอทุก run จนจบจริง ----
status=0
for id in $ids; do
  while :; do
    if ! st="$(gh run view "$id" --json status --jq .status 2>&1)"; then
      echo "ci-wait: ถามสถานะของ run ${id} ไม่สำเร็จ: $st"
      exit 4
    fi
    [ "$st" = "completed" ] && break
    if [ "$(date +%s)" -ge "$deadline" ]; then
      echo "ci-wait: run ${id} ยัง '${st}' หลัง ${limit_min} นาที — ยังไม่จบ ไม่ใช่ผ่าน"
      exit 3
    fi
    sleep 30
  done
  if ! summary="$(gh run view "$id" --json workflowName,conclusion,jobs \
      --jq '"\(.workflowName) → \(.conclusion)", (.jobs[] | "  \(.name): \(.conclusion)")' 2>&1)"; then
    echo "ci-wait: อ่านผลของ run ${id} ไม่สำเร็จ: $summary"
    exit 4
  fi
  echo "run ${id} · ${summary}"
  conclusion="$(printf '%s\n' "$summary" | head -n 1)"
  case "$conclusion" in
    *"→ success") ;;
    *) status=1 ;;
  esac
done

if [ "$status" -eq 0 ]; then
  echo "ci-wait: ${sha} — ทุก run สำเร็จ (job ที่ขึ้น 'skipped' ข้างบนคือไม่ได้ตรวจ ไม่ใช่ผ่าน)"
else
  echo "ci-wait: ${sha} — มี run ที่ไม่สำเร็จ"
fi
exit "$status"
