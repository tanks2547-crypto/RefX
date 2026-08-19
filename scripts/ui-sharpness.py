"""Answer "is the image on screen actually sharp?" with a number.

Why this exists
---------------
docs/08 3.9 item 9: pixel evidence must be diffed AS NUMBERS, because the eye
reads repeating patterns wrong.  "Zoomed in and it looks sharper" is exactly
the claim that eye cannot settle: a 128 px thumbnail stretched to 800 px and a
real 1024 px working texture both show the same picture, just one of them
smeared.  P4-5 exists to turn the first into the second, so it needs a number.

How it answers
--------------
Feed it a screenshot of RefX with a FINE CHECKERBOARD image on the canvas
(1 px black / 1 px white).  That pattern is chosen on purpose:

  * at thumbnail resolution the checker averages away to flat mid grey, so a
    stretched thumbnail has almost no local contrast whatever the zoom is
  * a working texture keeps the checker, so neighbouring pixels stay far apart

The reported number is the mean absolute difference between horizontally
neighbouring pixels inside the region.  Flat grey scores near 0.  A visible
checker scores in the tens.  The gap is not subtle, which is the point.

No third-party decoder - PNG is unpacked with the standard library, same rule
as ui-measure.py.  Manual verification tool, not part of CI.

Usage:  python scripts/ui-sharpness.py shot.png [x0 y0 x1 y1]
"""

import struct
import sys
import zlib

# same canvas area as ui-measure.py, for the 1280x800 client area that
# ui-drive.ps1 sets up (shell.rs panel layout)
CANVAS = (200, 46, 1040, 775)


def read_png(path):
    """(width, height, rows) with rows as lists of RGB tuples."""
    with open(path, "rb") as handle:
        data = handle.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path}: not a PNG")

    pos = 8
    width = height = depth = colour = None
    idat = bytearray()
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width, height, depth, colour = struct.unpack(">IIBB", body[:10])
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break

    if depth != 8 or colour not in (2, 6):
        raise SystemExit(f"{path}: only 8-bit RGB/RGBA is handled, got depth={depth} colour={colour}")
    channels = 3 if colour == 2 else 4

    raw = zlib.decompress(bytes(idat))
    stride = width * channels
    out = []
    previous = bytearray(stride)
    at = 0
    for _ in range(height):
        filt = raw[at]
        line = bytearray(raw[at + 1 : at + 1 + stride])
        at += 1 + stride
        for i in range(stride):
            left = line[i - channels] if i >= channels else 0
            up = previous[i]
            upleft = previous[i - channels] if i >= channels else 0
            if filt == 1:
                line[i] = (line[i] + left) & 0xFF
            elif filt == 2:
                line[i] = (line[i] + up) & 0xFF
            elif filt == 3:
                line[i] = (line[i] + (left + up) // 2) & 0xFF
            elif filt == 4:
                p = left + up - upleft
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - upleft)
                pred = left if (pa <= pb and pa <= pc) else (up if pb <= pc else upleft)
                line[i] = (line[i] + pred) & 0xFF
        out.append([tuple(line[i : i + 3]) for i in range(0, stride, channels)])
        previous = line
    return width, height, out


def sharpness(rows, box):
    """Mean absolute luminance difference between neighbouring pixels."""
    x0, y0, x1, y1 = box
    total = 0
    count = 0
    for y in range(y0, min(y1, len(rows))):
        row = rows[y]
        for x in range(x0 + 1, min(x1, len(row))):
            a = sum(row[x - 1]) // 3
            b = sum(row[x]) // 3
            total += abs(a - b)
            count += 1
    return (total / count) if count else 0.0


def main(argv):
    if len(argv) < 2:
        raise SystemExit(__doc__)
    box = CANVAS
    if len(argv) >= 6:
        box = tuple(int(v) for v in argv[2:6])
    for path in [argv[1]]:
        _, _, rows = read_png(path)
        print(f"{path}  region={box}  neighbour-contrast={sharpness(rows, box):.2f}")


if __name__ == "__main__":
    main(sys.argv)
