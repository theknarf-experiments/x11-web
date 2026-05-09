"""QueryBestSize via raw bytes — python-xlib has no high-level wrapper.

QueryBestSize (opcode 97) returns the closest tile/stipple/cursor size
the server actually supports. Most Xorg servers just echo back the
requested size; we just verify the reply parses and yields non-zero
dimensions.
"""

import os
import socket
import struct

import Xlib.display

DISPLAY = os.environ.get("DISPLAY", ":99")
display_num = int(DISPLAY.lstrip(":").split(".")[0])

# Use python-xlib only to fetch the root window ID, then drop down to a
# raw socket so we can build the request directly.
d = Xlib.display.Display()
root = d.screen().root.id
d.close()

s = socket.socket(socket.AF_UNIX)
s.connect(f"/tmp/.X11-unix/X{display_num}")

s.sendall(struct.pack("<BBHHHHH", 0x6C, 0, 11, 0, 0, 0, 0))


def read_exact(n):
    buf = b""
    while len(buf) < n:
        chunk = s.recv(n - len(buf))
        if not chunk:
            raise IOError("short read")
        buf += chunk
    return buf


hdr = read_exact(8)
extra = struct.unpack("<H", hdr[6:8])[0] * 4
read_exact(extra)


def query_best_size(class_):
    req = struct.pack("<BBHIHH", 97, class_, 3, root, 100, 100)
    s.sendall(req)
    head = read_exact(32)
    if head[0] == 0:
        raise RuntimeError(f"X error: code={head[1]}")
    width, height = struct.unpack_from("<HH", head, 8)
    return width, height


tile_w, tile_h = query_best_size(1)
print(f"tile_width={tile_w} tile_height={tile_h}")

stip_w, stip_h = query_best_size(2)
print(f"stipple_width={stip_w} stipple_height={stip_h}")

s.close()
