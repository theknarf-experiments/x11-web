"""BIG-REQUESTS extension presence and BigReqEnable round-trip.

python-xlib does not auto-negotiate BIG-REQUESTS, and `Display.info` is
populated from the initial Setup reply only — so reading
`d.info.max_request_length` after `query_extension` won't see the
extended value. We instead drive the protocol by hand via ctypes/libxcb-
flavoured raw bytes: query the extension to get its major opcode, send
BigReqEnable (the only request the extension defines), and verify the
reply contains a max_request_length larger than 65535.
"""

import os
import socket
import struct

DISPLAY = os.environ.get("DISPLAY", ":99")
display_num = int(DISPLAY.lstrip(":").split(".")[0])

s = socket.socket(socket.AF_UNIX)
s.connect(f"/tmp/.X11-unix/X{display_num}")

# X11 connection setup (little-endian, no auth)
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
if hdr[0] != 1:
    print(f"setup_failed status={hdr[0]}")
    raise SystemExit
extra = struct.unpack("<H", hdr[6:8])[0] * 4
body = read_exact(extra)
# Setup reply:  bytes [4..6] = max_request_length (CARD16, in 4-byte units)
initial_max = struct.unpack_from("<H", hdr, 2)
# release/resource fields live in `body`; we don't need them.

seq = 0

def send(req_bytes):
    global seq
    seq = (seq + 1) & 0xFFFF
    s.sendall(req_bytes)

def read_reply():
    head = read_exact(32)
    if head[0] == 0:
        raise RuntimeError(f"X error: code={head[1]}")
    extra_words = struct.unpack_from("<I", head, 4)[0]
    rest = read_exact(extra_words * 4)
    return head, rest

# QueryExtension("BIG-REQUESTS") — opcode 98
name = b"BIG-REQUESTS"
nlen = len(name)
pad = (-nlen) & 3
req = struct.pack("<BBHHH", 98, 0, 2 + (nlen + pad) // 4, nlen, 0) + name + b"\0" * pad
send(req)

head, _ = read_reply()
present = head[8]
major_opcode = head[9]
print(f"bigreq_present={present == 1}")

if present != 1 or major_opcode == 0:
    s.close()
    raise SystemExit

# BigReqEnable — minor opcode 0, length 1, no payload
req = struct.pack("<BBH", major_opcode, 0, 1)
send(req)

head, _ = read_reply()
new_max = struct.unpack_from("<I", head, 8)[0]
print(f"new_max_request_length={new_max}")
print(f"big_requests_work={new_max > 65535}")

s.close()
