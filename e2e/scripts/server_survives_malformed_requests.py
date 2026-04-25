import socket, struct, os

sock_path = "/tmp/.X11-unix/X99"
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sock_path)

# Read xauthority
xauth_path = os.environ.get("XAUTHORITY", "/tmp/.x11-web-Xauthority")
try:
    with open(xauth_path, "rb") as f:
        xauth_data = f.read()
    # Extract cookie (last 16 bytes before any padding)
    cookie = xauth_data[-16:]
    auth_name = b"MIT-MAGIC-COOKIE-1"
except:
    cookie = b""
    auth_name = b""

# Send connection setup with auth
setup = struct.pack("<BxHHHH",
    0x6c,  # LSB first
    11,    # major version
    0,     # minor version
    len(auth_name),
    len(cookie),
)
# Pad auth_name and cookie to 4-byte boundaries
name_pad = (4 - len(auth_name) % 4) % 4
cookie_pad = (4 - len(cookie) % 4) % 4
setup += auth_name + b"\x00" * name_pad
setup += cookie + b"\x00" * cookie_pad
s.sendall(setup)

# Read setup response
resp = s.recv(8192)
if len(resp) < 8:
    print("CONNECTION_FAILED")
    exit(1)
if resp[0] == 1:
    print("CONNECTED")
else:
    print(f"REJECTED: {resp[0]}")
    exit(1)

# Send a valid InternAtom request first
name = b"TEST_ATOM"
name_len = len(name)
pad = (4 - name_len % 4) % 4
req_len = (8 + name_len + pad) // 4
req = struct.pack("<BBH", 16, 0, req_len)  # InternAtom, only_if_exists=0
req += struct.pack("<HH", name_len, 0)
req += name + b"\x00" * pad
s.sendall(req)
reply = s.recv(32)
if reply and reply[0] == 1:
    print("INTERN_ATOM_OK")

# Send a zero-length request (invalid)
s.sendall(struct.pack("<BBH", 255, 0, 0))
import time; time.sleep(0.1)

# Try reading — server should send an error, not crash
try:
    err = s.recv(32)
    if err and err[0] == 0:
        print("GOT_ERROR_RESPONSE")
    elif err:
        print(f"GOT_RESPONSE_TYPE_{err[0]}")
    else:
        print("CONNECTION_CLOSED")
except:
    print("CONNECTION_ERROR")

s.close()
print("FUZZ_COMPLETE")
