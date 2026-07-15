import struct, os

icon_dir = os.path.join(os.path.dirname(__file__), "src-tauri", "icons")
src_path = os.path.join(icon_dir, "icon.png")
with open(src_path, "rb") as f:
    src_data = f.read()

# Save as required sizes
for name in ["32x32.png", "128x128.png", "128x128@2x.png"]:
    path = os.path.join(icon_dir, name)
    with open(path, "wb") as f:
        f.write(src_data)
    print(f"Saved {name}")

# Create ICO: header + dir entries + embedded PNGs
count = 2
header = struct.pack("<HHH", 0, 1, count)
offset = 6 + 16 * count
entries = b""
for w, h in [(32, 32), (256, 256)]:
    wb = 0 if w == 256 else w
    hb = 0 if h == 256 else h
    entries += struct.pack("<BBBBHHII", wb, hb, 0, 0, 1, 32, len(src_data), offset)
    offset += len(src_data)
ico_data = header + entries + src_data * count
with open(os.path.join(icon_dir, "icon.ico"), "wb") as f:
    f.write(ico_data)
print(f"Saved icon.ico ({len(ico_data)} bytes)")

# Create ICNS
icon_entry = b"ic07" + struct.pack(">I", len(src_data) + 8) + src_data
icns_data = b"icns" + struct.pack(">I", len(icon_entry) + 8) + icon_entry
with open(os.path.join(icon_dir, "icon.icns"), "wb") as f:
    f.write(icns_data)
print(f"Saved icon.icns ({len(icns_data)} bytes)")

print("Done")
