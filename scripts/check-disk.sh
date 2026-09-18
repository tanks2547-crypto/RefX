#!/usr/bin/env bash
#
# เนื้อที่ดิสก์ของ runner — คืนที่ว่างก่อนเริ่ม แล้ว **บังคับให้อ่านว่าเหลือเท่าไหร่**
#
# ★★★ ทำไมต้องมี (18 ก.ย. 2026 · run 35308516735)
#
#   `check (ubuntu-latest)` แดงกลางการคอมไพล์ด้วยข้อความ
#
#     error: failed to write to `.../full.rmeta`: No space left on device (os error 28)
#
#   ซึ่งอ่านเหมือนคอมไพเลอร์พังหรือโค้ดพัง · ของจริงคือ **ดิสก์เต็ม**
#
#   แล้วเมื่อไปดูรอบก่อนหน้าที่ **เขียว** พบว่ามันจบด้วยที่ว่าง **96 MB**
#   — job นี้ไม่ได้ผ่านเพราะมันสุขภาพดี มันผ่านเพราะเหลือที่พอดี 96 MB
#   ตัวเลขนั้นอยู่ใน log มาตลอดในฐานะ warning ที่ไม่มีอะไรทำอะไรกับมัน
#
#   → รูปเดียวกับทุกบานในโปรเจกต์นี้: **ผลที่ไม่มีใครถูกบังคับให้อ่าน
#     มีค่าเท่ากับไม่มีผล** · สคริปต์นี้จึงพิมพ์ทุกครั้ง และ *ล้ม* เมื่อ
#     ขอบเหลือน้อยกว่าพื้น แทนที่จะรอให้มันล้มกลางการคอมไพล์รอบถัดไป
#
# ใช้:
#   scripts/check-disk.sh free            # คืนที่ว่างจากของที่ติดมากับ image
#   scripts/check-disk.sh report          # พิมพ์ที่ว่าง + ล้มถ้าต่ำกว่าพื้น
#   REFX_DISK_FLOOR_MB=999999 scripts/check-disk.sh report   # negative control
#
# ★ negative control (docs/08 §3.9 ข้อ 1): ตั้งพื้นให้สูงเกินจริงแล้วสคริปต์นี้
#   **ต้องล้ม** · ถ้าไม่ล้มแปลว่าตัวตรวจเองพัง แล้วเราจะได้สัญญาณเขียวปลอม
#   เพิ่มมาอีกอันแทนที่จะได้ประตู

set -euo pipefail

# ขอบที่ต้องเหลือหลังงานทั้ง job · job นี้ใช้ทั้ง debug + release + bench
# ในโฟลเดอร์ target เดียวกัน จึงกินหลาย GB โดยธรรมชาติ
FLOOR_MB="${REFX_DISK_FLOOR_MB:-2048}"

free_mb() { df -Pm . | awk 'NR == 2 { print $4 }'; }

case "${1:-report}" in
  free)
    BEFORE=$(free_mb)
    echo "ที่ว่างก่อนเก็บกวาด: $BEFORE MB"
    # ของที่ติดมากับ image ของ GitHub และ job นี้ไม่ได้ใช้เลยสักอย่าง
    # ★ `|| true` ทุกบรรทัด — ถ้าวันหนึ่ง image ไม่มีโฟลเดอร์ไหนแล้ว การเก็บกวาด
    #   ต้องไม่กลายเป็นสาเหตุที่ job แดง (นั่นจะเป็นคนละเรื่องกับที่เรากำลังแก้)
    for junk in /usr/local/lib/android /usr/share/dotnet /opt/ghc \
                /usr/local/share/boost /usr/local/share/powershell \
                /usr/share/swift "${AGENT_TOOLSDIRECTORY:-}"; do
      [ -n "$junk" ] && [ -d "$junk" ] && sudo rm -rf "$junk" || true
    done
    AFTER=$(free_mb)
    echo "ที่ว่างหลังเก็บกวาด: $AFTER MB (คืนมา $((AFTER - BEFORE)) MB)"
    echo "::notice title=ดิสก์ของ runner::คืนที่ว่าง $((AFTER - BEFORE)) MB · เหลือ $AFTER MB"
    ;;

  report)
    LEFT=$(free_mb)
    # ★ พิมพ์เสมอ ทั้งตอนผ่านและตอนไม่ผ่าน — ขอบที่มองเห็นทุกรอบคือสิ่งเดียว
    #   ที่ทำให้ "เหลือ 96 MB" ไม่กลายเป็นข่าวดี
    echo "ที่ว่างที่เหลือ: $LEFT MB (พื้น $FLOOR_MB MB)"
    du -sh target 2>/dev/null | awk '{ print "target/ ใช้ไป: " $1 }' || true
    echo "::notice title=ดิสก์ที่เหลือ::$LEFT MB (พื้น $FLOOR_MB MB)"
    if [ "$LEFT" -lt "$FLOOR_MB" ]; then
      echo "::error title=ดิสก์ของ runner ใกล้เต็ม::เหลือ $LEFT MB ต่ำกว่าพื้น $FLOOR_MB MB"
      echo "รอบถัดไปจะแดงกลางการคอมไพล์ด้วย 'No space left on device' ซึ่งอ่านเหมือนโค้ดพัง"
      echo "แก้ที่ต้นเหตุ: ลดของที่ build ใน job นี้ หรือเพิ่มรายการใน check-disk.sh free"
      exit 1
    fi
    ;;

  *)
    echo "::error::ไม่รู้จักโหมด '${1}' — ใช้ได้: free · report"
    exit 1
    ;;
esac
