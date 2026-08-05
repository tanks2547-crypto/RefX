"""Turn a RefX screenshot into numbers.

Why this exists
---------------
The session that shipped P2-8 recorded the lesson the hard way: looking at a
screenshot lies when the thing on screen has a repeating pattern.  The `flip`
bug read as "well, it looks flipped" to the eye and only a numeric pixel diff
showed that it had not moved at all (HANDOFF 2.7).

So the rule is: when the thing being proven is a change in pixels, diff it as
numbers, and keep the screenshot as the supporting picture.  For align and for
the drag guides the quantity is a position, which this reports directly.

What it reports, for the canvas area only
-----------------------------------------
  * one line per drawn item: left / right / top / bottom / centre
  * pixels painted in SELECT_STROKE (120,190,255)  -> selection frame present
  * pixels painted in CROP_STROKE   (255,196,92)   -> crop tool handles
  * pixels painted in GUIDE_STROKE  (255,96,160)   -> alignment guide, and the
    exact column/row it sits on

Items are separated with connected components, not column ranges: align puts
items in one column on purpose, and a column-range reading would fuse them.
Take the measurement shot with NOTHING selected -- the group frame is one
unbroken rectangle around every selected item and bridges them all into a
single component.

No third-party decoder: PNG is unpacked with the standard library so this adds
no dependency to anything.  Manual verification tool, not part of CI.

Usage:  python scripts/ui-measure.py shot.png [more.png ...]
"""

import struct
import sys
import zlib

# canvas area in client coordinates, for the 1280x800 client area that
# ui-drive.ps1 sets up (shell.rs panel layout)
CANVAS = (200, 46, 1040, 775)

SELECT = (120, 190, 255)
CROP = (255, 196, 92)
GUIDE = (255, 96, 160)

# tight on purpose: the pastel test images contain colours a loose tolerance
# reports as "selection frame" on a board where nothing is selected
STROKE_TOL = 6


def read_png(path):
    """Decode a non-interlaced 8-bit RGB/RGBA PNG to (w, h, channels, bytes)."""
    with open(path, "rb") as fh:
        data = fh.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG")
    pos = 8
    idat = bytearray()
    w = h = color = None
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or interlace != 0 or color not in (2, 6):
                raise ValueError(f"{path}: unsupported PNG {depth=} {color=} {interlace=}")
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    raw = zlib.decompress(bytes(idat))
    channels = 3 if color == 2 else 4
    stride = w * channels
    out = bytearray(stride * h)
    prev = bytearray(stride)
    p = 0
    for y in range(h):
        f = raw[p]
        p += 1
        line = bytearray(raw[p : p + stride])
        p += stride
        if f == 1:  # Sub
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif f == 2:  # Up
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif f == 3:  # Average
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif f == 4:  # Paeth
            for i in range(stride):
                a = line[i - channels] if i >= channels else 0
                b = prev[i]
                c = prev[i - channels] if i >= channels else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        out[y * stride : (y + 1) * stride] = line
        prev = line
    return w, h, channels, bytes(out)


def near(buf, i, want, tol=STROKE_TOL):
    return (
        abs(buf[i] - want[0]) <= tol
        and abs(buf[i + 1] - want[1]) <= tol
        and abs(buf[i + 2] - want[2]) <= tol
    )


def components(mask, ww, hh, x0, y0):
    """Two-pass connected components; returns [left, top, right, bottom, area]."""
    parent = {}

    def find(a):
        while parent[a] != a:
            parent[a] = parent[parent[a]]
            a = parent[a]
        return a

    def union(a, b):
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[rb] = ra

    label = [0] * (ww * hh)
    nxt = 1
    for yy in range(hh):
        base = yy * ww
        for xx in range(ww):
            if not mask[base + xx]:
                continue
            up = label[base - ww + xx] if yy > 0 else 0
            left = label[base + xx - 1] if xx > 0 else 0
            if up and left:
                label[base + xx] = up
                union(up, left)
            elif up:
                label[base + xx] = up
            elif left:
                label[base + xx] = left
            else:
                parent[nxt] = nxt
                label[base + xx] = nxt
                nxt += 1

    boxes = {}
    for yy in range(hh):
        base = yy * ww
        for xx in range(ww):
            lab = label[base + xx]
            if not lab:
                continue
            root = find(lab)
            px, py = xx + x0, yy + y0
            box = boxes.get(root)
            if box is None:
                boxes[root] = [px, py, px, py, 1]
            else:
                box[0] = min(box[0], px)
                box[1] = min(box[1], py)
                box[2] = max(box[2], px)
                box[3] = max(box[3], py)
                box[4] += 1
    return boxes.values()


def analyse(path):
    w, h, ch, buf = read_png(path)
    x0, y0, x1, y1 = CANVAS
    x1, y1 = min(x1, w), min(y1, h)

    # background = the most common colour inside the canvas, sampled coarsely
    counts = {}
    for y in range(y0, y1, 4):
        row = y * w * ch
        for x in range(x0, x1, 4):
            i = row + x * ch
            key = buf[i : i + 3]
            counts[key] = counts.get(key, 0) + 1
    bg = max(counts, key=counts.get)

    ww, hh = x1 - x0, y1 - y0
    mask = bytearray(ww * hh)
    select, crop, guide = [], [], []
    for y in range(y0, y1):
        row = y * w * ch
        mrow = (y - y0) * ww
        for x in range(x0, x1):
            i = row + x * ch
            if near(buf, i, SELECT):
                select.append((x, y))
            if near(buf, i, CROP):
                crop.append((x, y))
            if near(buf, i, GUIDE):
                guide.append((x, y))
            if (
                abs(buf[i] - bg[0]) + abs(buf[i + 1] - bg[1]) + abs(buf[i + 2] - bg[2])
                > 24
            ):
                mask[mrow + x - x0] = 1

    items = sorted(
        (
            b
            for b in components(mask, ww, hh, x0, y0)
            if b[4] > 400 and b[2] - b[0] > 8 and b[3] - b[1] > 8
        ),
        key=lambda b: (b[0], b[1]),
    )

    print(f"== {path}  bg={tuple(bg)}")
    for b in items:
        print(
            f"   item L={b[0]} R={b[2]} T={b[1]} B={b[3]}  "
            f"w={b[2] - b[0] + 1} h={b[3] - b[1] + 1}  "
            f"cx={(b[0] + b[2]) / 2:.1f} cy={(b[1] + b[3]) / 2:.1f}"
        )
    for name, hits in (("SELECT", select), ("CROP", crop), ("GUIDE", guide)):
        if not hits:
            print(f"   {name} stroke: none")
            continue
        xs = [p[0] for p in hits]
        ys = [p[1] for p in hits]
        # a guide is one long column (vertical) or one long row (horizontal)
        cols = sorted({x for x in xs if xs.count(x) > 30})
        rows = sorted({y for y in ys if ys.count(y) > 30})
        print(
            f"   {name} stroke: {len(hits)} px, x {min(xs)}..{max(xs)}, "
            f"y {min(ys)}..{max(ys)} | full columns {cols} | full rows {rows}"
        )
    return items


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    for p in sys.argv[1:]:
        analyse(p)
