import socket, struct, os, time

xauth_path = os.environ.get("XAUTHORITY", "/tmp/.x11-web-Xauthority")
try:
    with open(xauth_path, "rb") as f:
        xauth_data = f.read()
    cookie = xauth_data[-16:]
    auth_name = b"MIT-MAGIC-COOKIE-1"
except:
    cookie = b""
    auth_name = b""

success = 0
for i in range(20):
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(2)
        s.connect("/tmp/.X11-unix/X99")
        # Trailing "xx" — X11 connection setup is 12 bytes; without it
        # the server keeps waiting for the rest and we time out.
        setup = struct.pack("<BxHHHHxx", 0x6c, 11, 0, len(auth_name), len(cookie))
        name_pad = (4 - len(auth_name) % 4) % 4
        cookie_pad = (4 - len(cookie) % 4) % 4
        setup += auth_name + b"\x00" * name_pad
        setup += cookie + b"\x00" * cookie_pad
        s.sendall(setup)
        resp = s.recv(4096)
        if resp and resp[0] == 1:
            success += 1
        s.close()
    except:
        pass
print(f"RAPID_CYCLES: {success}/20")
