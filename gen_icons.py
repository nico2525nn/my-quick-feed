import struct, zlib, io

def create_png(width, height, r, g, b):
    """Create a minimal valid PNG with solid color."""
    raw = b""
    for y in range(height):
        raw += b"\x00"  # filter byte
        for x in range(width):
            raw += bytes([r, g, b, 255])  # RGBA

    def chunk(chunk_type, data):
        c = chunk_type + data
        crc = struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)
        return struct.pack(">I", len(data)) + c + crc

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)  # 8-bit RGBA
    idat = zlib.compress(raw)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", idat)
        + chunk(b"IEND", b"")
    )

def create_ico(sizes):
    """Create a multi-size ICO file from PNG data."""
    pngs = []
    for w, h, color in sizes:
        pngs.append(create_png(w, h, *color))

    count = len(pngs)
    header = struct.pack("<HHH", 0, 1, count)  # reserved, type=ico, count
    offset = 6 + 16 * count
    entries = b""
    for (w, h, _), png_data in zip(sizes, pngs):
        w_byte = 0 if w == 256 else w
        h_byte = 0 if h == 256 else h
        entries += struct.pack("<BBBBHHII", w_byte, h_byte, 0, 0, 1, 32, len(png_data), offset)
        offset += len(png_data)

    return header + entries + b"".join(pngs)

sizes = [
    (32, 32, (22, 27, 34)),    # #161b22 sidebar bg
    (128, 128, (22, 27, 34)),
    (256, 256, (22, 27, 34)),
]

import os
icon_dir = os.path.join(os.path.dirname(__file__), "src-tauri", "icons")
os.makedirs(icon_dir, exist_ok=True)

# Save individual PNGs
for w, h, color in [(32, 32, (22, 27, 34)), (128, 128, (22, 27, 34)), (256, 256, (22, 27, 34))]:
    png = create_png(w, h, *color)
    name = f"{w}x{w}.png" if w != 256 else "128x128@2x.png"
    with open(os.path.join(icon_dir, name), "wb") as f:
        f.write(png)
    print(f"Created {name}")

# Save ICO
ico = create_ico(sizes)
with open(os.path.join(icon_dir, "icon.ico"), "wb") as f:
    f.write(ico)
print("Created icon.ico")

# Create a simple ICNS placeholder (just a copy of 128 PNG for macOS)
with open(os.path.join(icon_dir, "icon.icns"), "wb") as f:
    png_128 = create_png(128, 128, 22, 27, 34)
    # Wrap in icns container
    icon_data = b"ic07" + struct.pack(">I", len(png_128) + 8) + png_128
    f.write(b"icns" + struct.pack(">I", len(icon_data) + 8) + icon_data)
print("Created icon.icns")

print("All icons generated!")
