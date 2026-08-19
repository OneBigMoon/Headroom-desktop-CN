"""Rebuild src-tauri/icons/icon.ico from the 1024px master.

BMP DIB entries below 256 (what the Windows shell wants) plus PNG at 256.
The master's transparent pixels are (0,0,0,0) and the artwork's outline is
near-black, so a straight RGBA resize needs no premultiply dance.
"""
import io
import os
import struct
from PIL import Image

SRC = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons", "headroom.iconset", "icon_512x512@2x.png")
OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons", "icon.ico")
SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]


def dib(img):
    """32bpp bottom-up BGRA DIB + 1bpp AND mask, as an ICO entry expects."""
    w, h = img.size
    px = img.load()
    hdr = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, w * h * 4, 0, 0, 0, 0)
    xor = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = px[x, y]
            xor += bytes((b, g, r, a))
    stride = ((w + 31) // 32) * 4  # AND mask rows pad to 4 bytes
    mask = bytearray()
    for y in range(h - 1, -1, -1):
        row = bytearray(stride)
        for x in range(w):
            if px[x, y][3] == 0:
                row[x // 8] |= 0x80 >> (x % 8)
        mask += row
    return bytes(hdr + xor + mask)


src = Image.open(SRC).convert("RGBA")
blobs = []
for s in SIZES:
    img = src.resize((s, s), Image.LANCZOS)
    if s >= 256:
        buf = io.BytesIO()
        img.save(buf, format="PNG", optimize=True)
        blobs.append(buf.getvalue())
    else:
        blobs.append(dib(img))

offset = 6 + 16 * len(SIZES)
dir_, data = bytearray(struct.pack("<HHH", 0, 1, len(SIZES))), bytearray()
for s, blob in zip(SIZES, blobs):
    dir_ += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(blob), offset)
    data += blob
    offset += len(blob)
open(OUT, "wb").write(bytes(dir_ + data))
print(f"wrote {OUT}  {len(dir_) + len(data)} bytes  sizes={SIZES}")

# Self-check: every declared size must decode back, with alpha intact.
ico = Image.open(OUT)
assert ico.size == (256, 256), ico.size
for s in SIZES:
    frame = ico.ico.getimage((s, s))
    assert frame.size == (s, s), (s, frame.size)
    assert frame.mode == "RGBA", (s, frame.mode)
    assert frame.getpixel((0, 0))[3] == 0, f"{s}px corner is not transparent"
print(f"verified {len(SIZES)} entries decode with transparent corners")
