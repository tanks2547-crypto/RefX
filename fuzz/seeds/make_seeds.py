"""สร้าง corpus ตั้งต้นให้ fuzz_decode

เอาเคสที่เทสต์ใน decode.rs / thumb.rs พิสูจน์แล้วว่าเป็นเคสที่สำคัญมาเป็นจุดตั้งต้น
libFuzzer จะกลายพันธุ์ต่อจากตรงนี้ ซึ่งเร็วกว่าเริ่มจากไบต์สุ่มล้วนมาก
"""
import io
import os
import struct
import zlib

OUT = 'fuzz/seeds/fuzz_decode'
os.makedirs(OUT, exist_ok=True)


def write(name, data):
    with open(os.path.join(OUT, name), 'wb') as f:
        f.write(data)
    print(f'  {name:<28} {len(data):>7} ไบต์')


def png(width, height, *, declared=None):
    """PNG จริงที่ decode ได้ · declared=(w,h) เพื่อโกหกใน IHDR (decompression bomb)"""
    rows = []
    for y in range(height):
        row = bytearray(b'\x00')
        for x in range(width):
            row += bytes([(x * 7 + y * 11) % 256, (x * 3) % 256, (y * 5) % 256])
        rows.append(bytes(row))
    raw = b''.join(rows)

    def chunk(tag, payload):
        return (struct.pack('>I', len(payload)) + tag + payload
                + struct.pack('>I', zlib.crc32(tag + payload) & 0xFFFFFFFF))

    dw, dh = declared if declared else (width, height)
    ihdr = struct.pack('>IIBBBBB', dw, dh, 8, 2, 0, 0, 0)
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', ihdr)
            + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))


# JPEG จริงขนาดเล็กที่สุดเท่าที่ยังถูกต้อง (8x8 เทาล้วน, baseline)
JPEG_8x8 = bytes.fromhex(
    'ffd8ffdb004300ffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'
    'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'
    'ffffffffffffffc2000b080008000801011100ffc40014000100000000000000000000000'
    '0000000000009ffda0008010100000110ff002affd9'
)


def jpeg_with_exif(orientation=6):
    """แทรก APP1/EXIF จริงหลัง SOI — ยิงตัวสแกน EXIF ที่เขียนเอง"""
    tiff = (b'II' + struct.pack('<HI', 42, 8) + struct.pack('<H', 1)
            + struct.pack('<HHI', 0x0112, 3, 1)
            + struct.pack('<H', orientation) + b'\x00\x00'
            + struct.pack('<I', 0))
    payload = b'Exif\x00\x00' + tiff
    return (JPEG_8x8[:2] + b'\xff\xe1' + struct.pack('>H', len(payload) + 2)
            + payload + JPEG_8x8[2:])


def prng(n, seed=0x123456789ABCDEF0):
    out = bytearray()
    state = seed
    for _ in range(n):
        state ^= (state >> 12) & 0xFFFFFFFFFFFFFFFF
        state ^= (state << 25) & 0xFFFFFFFFFFFFFFFF
        state ^= (state >> 27) & 0xFFFFFFFFFFFFFFFF
        out.append(((state * 0x2545F4914F6CDD1D) >> 56) & 0xFF)
    return bytes(out)


print('สร้าง corpus ที่', OUT)

# --- ไฟล์ที่ถูกต้อง: ให้ fuzzer มีจุดตั้งต้นที่เดินลึกเข้าไปใน decoder ได้ ---
write('png_valid_8x8.png', png(8, 8))
write('png_valid_64x48.png', png(64, 48))
write('jpeg_valid_8x8.jpg', JPEG_8x8)
write('jpeg_with_exif.jpg', jpeg_with_exif(6))

# --- ★ decompression bomb: ไฟล์เล็กที่โกหกใน header ว่าใหญ่มหาศาล ---
write('png_bomb_65535.png', png(1, 1, declared=(65535, 65535)))
write('png_bomb_16k.png', png(1, 1, declared=(16384, 16384)))
write('png_lies_zero.png', png(1, 1, declared=(0, 0)))

# --- ไฟล์ที่ถูกตัด (Dropbox/OneDrive กำลัง sync) ---
full = png(64, 48)
write('png_truncated_half.png', full[:len(full) // 2])
write('png_truncated_header.png', full[:20])
write('jpeg_truncated.jpg', JPEG_8x8[:len(JPEG_8x8) // 2])

# --- นามสกุลโกหก: .exe ที่ถูกตั้งชื่อเป็น .png ---
write('exe_named_png.png',
      b'MZ\x90\x00\x03\x00\x00\x00\x04\x00\x00\x00\xff\xff\x00\x00'
      + bytes(512) + b'This program cannot be run in DOS mode.')

# --- ขยะล้วนและเคสขอบ ---
write('empty.bin', b'')
write('one_byte.bin', b'\x89')
write('random_64.bin', prng(64))
write('random_4k.bin', prng(4096))
write('png_magic_only.png', b'\x89PNG\r\n\x1a\n')
write('jpeg_soi_only.jpg', b'\xff\xd8')
write('jpeg_app1_no_len.jpg', b'\xff\xd8\xff\xe1')
write('jpeg_app1_huge_len.jpg', b'\xff\xd8\xff\xe1\xff\xff')

print('เสร็จ:', len(os.listdir(OUT)), 'ไฟล์')
