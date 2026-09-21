#!/usr/bin/env bash
#
# negative control ของ fuzz — **พิสูจน์ว่ามันแดงเป็น ไม่ใช่ว่ามันเขียวอยู่**
#
# ## ★★★ ทำไมต้องมี (`docs/08 §3.9` ข้อ 1)
#
# 21 ก.ย. 2026: ไล่ดูรอบ fuzz ย้อนหลัง 20 รอบ — แดง 4 รอบ และ **ทั้งสี่รอบ
# `fuzz-lint` กับ `unwired-targets` แดงด้วย** ซึ่งเป็น job ที่ไม่ได้ยิง fuzzer เลย
# แปลว่าสาเหตุอยู่ที่ toolchain/สภาพแวดล้อม ไม่ใช่ input ที่ทำให้ล้ม
#
# → **เส้นทาง "เจอ crash → job แดง → เก็บ artifact" ไม่เคยถูกเดินจริงสักครั้ง**
#   ในชีวิตของ workflow นี้ · I-4 ทั้งข้อพึ่งเครื่องมือที่ไม่มีใครเคยเห็นมัน
#   ทำงานตอนเจอของ · **ความเงียบยาวนานไม่ใช่หลักฐานของสุขภาพ**
#
# เกณฑ์ "ไม่เจอ crash" วัดสองอย่างพร้อมกันโดยแยกไม่ออก:
# **"ไม่มีบั๊ก"** กับ **"มีบั๊กแต่เราไม่เห็น"** · สคริปต์นี้แยกมันออกจากกัน
#
# ## วิธี
#
# ปลูกความผิดพลาดลงใน target จริง → ยิง → ต้องได้ **ทั้งสองอย่าง**:
#   (ก) exit code ไม่ใช่ศูนย์   (ข) ไฟล์ใหม่ใน `fuzz/artifacts/<target>/`
# แล้วคืนไฟล์ให้เหมือนเดิม
#
# ★ (ข) สำคัญเท่า (ก) — input ที่ทำให้พังคือสิ่งเดียวที่ใช้ซ่อมได้
#   ถ้าแดงแต่ไม่มีไฟล์ เราจะรู้แค่ว่า "พัง" โดยไม่มีทางรู้ว่าพังด้วยอะไร
#
# ## ★★ ทำไมสามชนิด ไม่ใช่แค่ panic
#
# libFuzzer รายงานสามอย่างนี้ **คนละทางกัน** และเขียนไฟล์คนละชื่อ:
#
# | ชนิด    | กลไกที่จับ            | ชื่อไฟล์     |
# |---------|----------------------|-------------|
# | panic   | abort → death callback | `crash-*`   |
# | timeout | เธรดเฝ้าเวลา            | `timeout-*` |
# | OOM     | ตัวดัก malloc / เฝ้า RSS | `oom-*`     |
#
# OOM คือรูปที่ I-4 กลัวที่สุด (decompression bomb) — ถ้าทางนั้นเงียบ
# เกราะกัน bomb ทั้งหมดก็ไม่มีใครเฝ้า
#
# ## ★★ ทำไมต้องครบทั้งสี่ target ไม่ใช่ตัวแทนตัวเดียว
#
# การต่อสายพลาดเกิดทีละ target (`fuzz_decode` เคยเป็น `let _ = data;`
# อยู่ 5 session โดยเขียวทุกคืน) · ตัวที่พลาดจะเงียบตัวเดียว**โดยที่อีกสามตัวเขียว**
# ซึ่งอ่านไม่ออกเลยจากผลรวม
#
# ใช้:
#   scripts/fuzz-nc.sh                       # ครบสี่ target × สามชนิด
#   scripts/fuzz-nc.sh fuzz_packed           # เฉพาะตัวเดียว
#   scripts/fuzz-nc.sh fuzz_packed oom       # เฉพาะตัวเดียว ชนิดเดียว
#
# ★ สคริปต์นี้ **แก้ไฟล์ target จริงชั่วคราว** — มี `trap` คืนให้ทุกทางออก
#   และตรวจซ้ำด้วย `git diff` ตอนจบ · ถ้าคืนไม่สำเร็จจะล้มเสียงดัง

set -uo pipefail
cd "$(dirname "$0")/.."

ALL_TARGETS="fuzz_decode fuzz_layout fuzz_document fuzz_packed"
ALL_FAULTS="panic timeout oom"

TARGETS="${1:-$ALL_TARGETS}"
FAULTS="${2:-$ALL_FAULTS}"

# ★ บน Windows ตัวรัน ASan (`clang_rt.asan_dynamic-x86_64.dll`) มากับ MSVC
#   ไม่ได้มากับ rustup · ถ้าไม่อยู่ใน PATH ไบนารีจะตายด้วย STATUS_DLL_NOT_FOUND
#   (0xc0000135) **ก่อนถึง main** ซึ่งหน้าตาเหมือน "fuzz พัง" ทุกประการ
#   บน Linux ไม่ต้องทำอะไร — ตัวรันถูกลิงก์มาแล้ว
if [ "${OS:-}" = "Windows_NT" ]; then
  asan_dir=$(dirname "$(find "/c/Program Files (x86)/Microsoft Visual Studio" \
                          "/c/Program Files/Microsoft Visual Studio" \
                          -path '*Hostx64/x64/clang_rt.asan_dynamic-x86_64.dll' \
                          2>/dev/null | head -1)" 2>/dev/null)
  if [ -n "$asan_dir" ] && [ -d "$asan_dir" ]; then
    export PATH="$asan_dir:$PATH"
  else
    echo "::warning::หา clang_rt.asan_dynamic-x86_64.dll ไม่เจอ — ถ้าเจอ 0xc0000135 นั่นคือสาเหตุ ไม่ใช่บั๊กของ fuzz"
  fi
fi
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-nightly}"

# ---- โค้ดที่ปลูก · หนึ่งบรรทัด แทรกใต้ `fuzz_target!(|data: &[u8]| {` ----
#
# ★ ทุกชนิดมีเงื่อนไข `data.len() >= 2` เหมือนกัน — ไม่ใช่ระเบิดทันทีแบบไม่มีเงื่อนไข
#   เพราะ input ว่างคือสิ่งแรกที่ libFuzzer ยิงเสมอ · ถ้าระเบิดที่นั่น เราจะพิสูจน์ได้
#   แค่ว่า "โปรแกรมตายตอนเริ่ม" ไม่ใช่ว่า "fuzzer หา input ที่ทำให้ตายเจอ"
fault_line() {
  case "$1" in
    panic)
      echo '    if data.len() >= 2 { panic!("NC ของ fuzz-nc.sh — ระเบิดโดยตั้งใจ"); }' ;;
    timeout)
      echo '    if data.len() >= 2 { loop { std::hint::spin_loop(); } }' ;;
    oom)
      # จองก้อนเดียวใหญ่กว่า -malloc_limit_mb → ตัวดัก malloc ยิงทันที
      # ไม่ต้องรอเธรดเฝ้า RSS ซึ่งช้ากว่าและกินแรมเครื่องจริงระหว่างรอ
      echo '    if data.len() >= 2 { let v = vec![0u8; 1 << 30]; std::hint::black_box(&v); }' ;;
    *) echo "ชนิดความผิดพลาดที่ไม่รู้จัก: $1" >&2; return 1 ;;
  esac
}

# ---- ธงของ libFuzzer ต่อชนิด ----
fault_flags() {
  case "$1" in
    panic)   echo "-runs=100000" ;;
    timeout) echo "-timeout=5 -runs=100000" ;;
    oom)     echo "-malloc_limit_mb=256 -rss_limit_mb=2048 -runs=100000" ;;
  esac
}

expected_prefix() {
  case "$1" in
    panic)   echo "crash-" ;;
    timeout) echo "timeout-" ;;
    oom)     echo "oom-" ;;
  esac
}

restore_all() {
  for t in $ALL_TARGETS; do
    [ -f "fuzz/fuzz_targets/.$t.nc-backup" ] || continue
    mv -f "fuzz/fuzz_targets/.$t.nc-backup" "fuzz/fuzz_targets/$t.rs"
  done
}
trap restore_all EXIT INT TERM

# ★★★ `__fastfail` บน Windows ฆ่าโปรเซสก่อนที่ libFuzzer จะได้เขียนไฟล์
#
#   วัดจริง 21 ก.ย. 2026: panic ทั้งสี่ target บน Windows ออกด้วย
#   **0xc0000409 (STATUS_STACK_BUFFER_OVERRUN)** ซึ่งคือ `__fastfail` —
#   มันข้ามตัวจัดการสัญญาณทั้งหมด libFuzzer จึงไม่มีโอกาสเขียน `crash-*`
#   · timeout กับ OOM ไม่เป็นแบบนี้เพราะ libFuzzer ตรวจเจอเองจากในโปรเซส
#     แล้วเขียนไฟล์ก่อนสั่งตาย (วัดแล้ว 8/8 ได้ไฟล์ชื่อถูก)
#
#   บน Linux ซึ่งเป็นที่ที่ CI รันจริง panic ไปทาง `abort()` → SIGABRT →
#   ตัวจัดการของ libFuzzer → เขียนไฟล์
#
# ★ จึง **ไม่นับว่าผ่าน และไม่นับว่าตก** — มันคือ "วัดที่นี่ไม่ได้"
#   การนับเป็นผ่านคือการโกหก · การนับเป็นตกคือการชี้นิ้วไปผิดที่
#   (`docs/08 §3.9` ข้อ 9 — คำวินิจฉัยปลอมแย่กว่าไม่มีคำวินิจฉัย)
#
# ★★ อ่านจาก **ข้อความที่ cargo-fuzz พิมพ์** ไม่ใช่จาก `$?` — cargo-fuzz กลืน
#    exit code ของลูกแล้วคืน 1 เสมอ · เลขจริงมีอยู่แค่ในบรรทัดที่มันพิมพ์
WIN_FASTFAIL='0xc0000409'

pass=0; fail=0; blind=0
printf '%-16s %-8s %-12s %-9s %-11s %s\n' target ชนิด exit ไฟล์ใหม่ ชื่อที่ได้ ผล
printf '%s\n' "-------------------------------------------------------------------------"

for target in $TARGETS; do
  src="fuzz/fuzz_targets/$target.rs"
  if [ ! -f "$src" ]; then
    echo "::error::ไม่มีไฟล์ $src"; fail=$((fail + 1)); continue
  fi
  anchor=$(grep -n 'fuzz_target!(|data: &\[u8\]| {' "$src" | head -1 | cut -d: -f1)
  if [ -z "$anchor" ]; then
    # ★ ไม่ปล่อยผ่านเงียบ ๆ — target ที่รูปร่างเปลี่ยนไปคือ target ที่ NC เลิกครอบคลุม
    echo "::error::$target ไม่มีบรรทัด 'fuzz_target!(|data: &[u8]| {' — NC ปลูกความผิดพลาดไม่ได้"
    fail=$((fail + 1)); continue
  fi

  for fault in $FAULTS; do
    line=$(fault_line "$fault") || { fail=$((fail + 1)); continue; }

    cp "$src" "fuzz/fuzz_targets/.$target.nc-backup"
    awk -v n="$anchor" -v ins="$line" 'NR==n{print; print ins; next} {print}' \
        "fuzz/fuzz_targets/.$target.nc-backup" > "$src"

    mkdir -p "fuzz/artifacts/$target"
    before=$(ls "fuzz/artifacts/$target" 2>/dev/null | wc -l)

    # shellcheck disable=SC2046
    out=$(cargo fuzz run "$target" -- $(fault_flags "$fault") 2>&1)
    code=$?

    after=$(ls "fuzz/artifacts/$target" 2>/dev/null | wc -l)
    grew=$((after - before))
    # ★ ไม่มีไฟล์ใหม่ = ไม่มีชื่อให้รายงาน · ถ้าโชว์ไฟล์เก่าจากรอบก่อน
    #   ตารางจะอ่านเหมือนว่ารอบนี้เก็บหลักฐานได้ ทั้งที่ช่องถัดไปบอกว่า +0
    newest=""
    [ "$grew" -ge 1 ] && newest=$(ls -t "fuzz/artifacts/$target" 2>/dev/null | head -1)
    want=$(expected_prefix "$fault")

    verdict=ผ่าน
    if [ "$code" -eq 0 ]; then
      verdict="ไม่แดง"
      echo "::error title=$target/$fault ไม่แดง::ปลูกความผิดพลาดแล้ว fuzz ยังคืน 0 — ตัวจับพัง"
    elif [ "$grew" -lt 1 ] && [ "${OS:-}" = "Windows_NT" ] \
         && printf '%s' "$out" | grep -q "$WIN_FASTFAIL"; then
      verdict="วัดที่นี่ไม่ได้"
      blind=$((blind + 1))
      echo "::notice title=$target/$fault แดงแล้ว แต่ครึ่งหลังวัดบน Windows ไม่ได้::โปรเซสตายด้วย __fastfail ($WIN_FASTFAIL) ก่อน libFuzzer ได้เขียนไฟล์ — ต้องพิสูจน์บน Linux"
    elif [ "$grew" -lt 1 ]; then
      verdict="ไม่มีไฟล์"
      echo "::error title=$target/$fault แดงแต่ไม่เก็บหลักฐาน::exit $code แต่ไม่มีไฟล์ใหม่ใน fuzz/artifacts/$target"
    elif [ "${newest#"$want"}" = "$newest" ]; then
      # ★ ชื่อไฟล์คือสิ่งที่บอกว่า libFuzzer **จับได้ด้วยกลไกไหน** — crash/timeout/oom
      #   ถ้า OOM ถูกรายงานเป็น crash แปลว่าทางที่เราคิดว่าเฝ้าอยู่ ไม่ได้เฝ้า
      verdict="ชื่อผิด"
      echo "::error title=$target/$fault กลไกผิดตัว::คาดว่าได้ไฟล์ขึ้นต้น '$want' แต่ได้ '$newest'"
    fi

    case "$verdict" in
      ผ่าน)          pass=$((pass + 1)) ;;
      วัดที่นี่ไม่ได้) ;;  # นับไว้แล้วในช่อง blind
      *)             fail=$((fail + 1)) ;;
    esac
    printf '%-16s %-8s %-12s %-9s %-11s %s\n' \
      "$target" "$fault" "$code" "+$grew" "${newest:0:11}" "$verdict"

    if [ "$verdict" != "ผ่าน" ] && [ "$verdict" != "วัดที่นี่ไม่ได้" ]; then
      echo "--- 25 บรรทัดท้ายของ $target/$fault ---"
      echo "$out" | tail -25
    fi

    mv -f "fuzz/fuzz_targets/.$target.nc-backup" "$src"
  done
done

trap - EXIT INT TERM
restore_all

# ★★ ตรวจว่าคืนไฟล์ครบจริง — สคริปต์ที่ทิ้งความผิดพลาดที่ปลูกไว้ในทรี
#    คือสิ่งที่อันตรายกว่าการไม่มี NC เลย
if ! git diff --quiet -- fuzz/fuzz_targets/; then
  echo "::error title=คืนไฟล์ไม่ครบ::fuzz/fuzz_targets/ ยังไม่เหมือนเดิม — รัน 'git checkout -- fuzz/fuzz_targets/' ทันที"
  git diff --stat -- fuzz/fuzz_targets/
  exit 1
fi

echo
echo "ผ่าน $pass · ไม่ผ่าน $fail · วัดที่นี่ไม่ได้ $blind"

# ★★★ ไม่ปล่อยให้ "วัดไม่ได้" อ่านเหมือน "ผ่าน"
#
#   ถ้าจบด้วย blind > 0 แล้วพิมพ์แค่ "ผ่าน 8" ครั้งหน้าจะมีคนอ่านว่าครบแล้ว
#   — ซึ่งเป็นรูปเดียวกับ `fuzz-lint` ที่เคยเขียวบน stable ทั้งที่หน้าที่คือตรวจ nightly
if [ "$blind" -gt 0 ]; then
  echo "::warning title=NC ยังไม่ครบสาย::$blind เคสพิสูจน์ครึ่งหลัง (ไฟล์หลักฐาน) บนเครื่องนี้ไม่ได้ — รอบที่นับได้คือรอบบน Linux ใน CI เท่านั้น"
fi

[ "$fail" -eq 0 ] || exit 1
