"""negative control ของ fuzz target ทั้งสี่ (P4-9)

`docs/08 §3.9` ข้อ 1: **target ที่ยังไม่เคยเห็นมันแดง = ยังไม่รู้ว่ามันตรวจอะไรอยู่**
โปรเจกต์นี้เคยมี target ที่เขียวทุกคืนอยู่ 5 session ทั้งที่เป็น `let _ = data;`

สคริปต์นี้ **ถอดด่านจริงในโค้ด production ออกทีละตัว** แล้วดูว่า target ที่ควร
จับได้ แดงจริงไหมและเร็วแค่ไหน — แล้วคืนไฟล์กลับทุกครั้ง (รวมตอนล้มกลางคัน)

★ ข้อ 9 ของหัวข้อเดียวกันบังคับว่าเครื่องมือแบบนี้ **ต้องอยู่ใน repo ไม่ใช่ใน
session** ไม่งั้นบทเรียนหายไปพร้อมกับหน้าต่างที่ปิดไป

    python scripts/fuzz-negative-control.py [วินาทีต่อ target]

ต้องมี nightly + `cargo install cargo-fuzz --locked`
บน Windows ต้องมี ASan runtime DLL ใน PATH — สคริปต์หาให้เองจาก MSVC
(ดู `fuzz/README.md` หัวข้อ `STATUS_DLL_NOT_FOUND`)
"""

import glob
import io
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# ★★ คอนโซล Windows ปริยายเป็น cp1252 ซึ่ง **encode ภาษาไทยไม่ได้เลย**
#    รุ่นแรกของสคริปต์นี้จึงตายตอนพิมพ์ผลบรรทัดแรก — หลังจากรัน fuzz ไป 45
#    วินาทีเรียบร้อยแล้ว · และเพราะมันถูกเรียกผ่าน `| tail` โค้ดที่ออกมาจึงเป็น 0
#    อ่านได้ว่า "สำเร็จแต่ไม่พิมพ์อะไร" ซึ่ง `docs/08 §3.9` บอกว่าต้องแยกให้ออก
#    จาก "ไม่ได้ทำอะไรเลย" · เจอเพราะรันของจริง ไม่ใช่เพราะอ่านโค้ด
for stream in (sys.stdout, sys.stderr):
    try:
        stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

# (target, ไฟล์, ข้อความเดิม, ข้อความแทน, ด่านนั้นคืออะไร)
#
# ★ ข้อความที่ถอดต้องเป็น **ด่านจริง** ไม่ใช่บั๊กที่แต่งขึ้น — เราถามว่า
#   "ถ้าเกราะนี้หายไป target จะรู้ไหม" ไม่ใช่ "target จับ panic ที่เราใส่ได้ไหม"
CASES = [
    (
        "fuzz_packed",
        "crates/refx-io/src/packed.rs",
        """        if offset < blobs_at || end > file_len {
            return Err(OpenError::Malformed);
        }
""",
        "",
        "ด่านขอบเขตของ entry ใน read_index",
    ),
    (
        "fuzz_document",
        "crates/refx-io/src/dto.rs",
        """    if body.len() < declared_usize {
        return Err(OpenError::Truncated {
            declared,
            actual: body.len() as u64,
        });
    }
""",
        "",
        "ด่านเทียบความยาวจริงกับที่หัวไฟล์ประกาศ",
    ),
    (
        "fuzz_layout",
        "crates/refx-core/src/layout.rs",
        """    let side = |v: f32| {
        if v.is_finite() {
            v.clamp(MIN_SIDE, MAX_SIDE)
        } else {
            MIN_SIDE
        }
    };
    Vec2::new(side(size.x), side(size.y))""",
        "    size",
        "sane_size — ด่าน MIN_SIDE/MAX_SIDE ตัวสุดท้ายของทุก engine",
    ),
    (
        "fuzz_decode",
        "crates/refx-asset/src/decode.rs",
        """    if pixels > limits.max_pixels {
        return Err(LoadError::ImageTooLarge {
            width,
            height,
            pixels,
            limit: limits.max_pixels,
        });
    }
""",
        "",
        "เพดาน max_pixels (กัน decompression bomb)",
    ),
]


def asan_dir():
    """หาโฟลเดอร์ของ clang_rt.asan_dynamic-x86_64.dll บน Windows

    ไม่เจอ = คืน None แล้วปล่อยให้ cargo-fuzz ล้มพร้อมข้อความของมันเอง —
    ดีกว่าเดา path แล้วรายงานผลที่ไม่ได้มาจากการรันจริง
    """
    if os.name != "nt":
        return None
    for root in (r"C:\Program Files (x86)\Microsoft Visual Studio",
                 r"C:\Program Files\Microsoft Visual Studio",
                 r"C:\Program Files\LLVM"):
        hits = glob.glob(os.path.join(root, "**", "Hostx64", "x64",
                                      "clang_rt.asan_dynamic-x86_64.dll"),
                         recursive=True)
        if hits:
            return os.path.dirname(hits[0])
    return None


def main():
    secs = int(sys.argv[1]) if len(sys.argv) > 1 else 90
    env = dict(os.environ)
    found = asan_dir()
    if found:
        env["PATH"] = env["PATH"] + os.pathsep + found

    ok = True
    for target, rel, old, new, what in CASES:
        path = os.path.join(ROOT, rel)
        src = io.open(path, encoding="utf-8").read()
        if old not in src:
            print(f"[{target}] ★ หาข้อความที่จะถอดไม่เจอใน {rel} — "
                  f"สคริปต์ล้าสมัยกว่าโค้ด แก้ CASES ก่อน")
            ok = False
            continue

        io.open(path, "w", encoding="utf-8",
                newline="\n").write(src.replace(old, new, 1))
        started = time.time()
        try:
            run = subprocess.run(
                ["cargo", "+nightly", "fuzz", "run", target, "--",
                 f"-max_total_time={secs}"],
                cwd=ROOT, env=env, capture_output=True, text=True,
                encoding="utf-8", errors="replace", timeout=secs + 900,
            )
            out = (run.stdout or "") + (run.stderr or "")
            red = run.returncode != 0
        except subprocess.TimeoutExpired:
            out, red = "TIMEOUT", False
        finally:
            # ★ คืนไฟล์เสมอ แม้ล้มกลางคัน — สคริปต์ที่ทิ้งโค้ดพังไว้ในต้นไม้
            #   อันตรายกว่าไม่มีสคริปต์
            io.open(path, "w", encoding="utf-8", newline="\n").write(src)

        why = next((line.strip()[:150] for line in out.splitlines()
                    if "panicked at" in line or "ERROR:" in line), "")
        verdict = "แดง" if red else "★★ ยังเขียว"
        print(f"[{target}] ถอด: {what}")
        print(f"           ผล: {verdict} · {time.time() - started:.0f}s "
              f"(รวมเวลา rebuild)")
        if why:
            print(f"           เหตุ: {why}")
        if not red:
            print("           → assertion อ่อนกว่าสัญญา หรือมีด่านอื่นบังอยู่ "
                  "(HANDOFF §4 ข้อ 21) — ต้องแก้ ไม่ใช่ปล่อยผ่าน")
            ok = False

    print()
    print("ครบทั้งสี่ target แดงตามที่ควร" if ok
          else "★ มี target ที่ไม่แดง — อ่านหมายเหตุข้างบน")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
