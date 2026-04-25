import socket
import struct
import sys
import time

errors = []

def x11_connect(display=99):
    """Open a raw X11 connection and return (socket, resource_base, resource_mask)."""
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(f'/tmp/.X11-unix/X{display}')
    sock.settimeout(5.0)

    # Connection setup (little-endian, X11.0, no auth)
    setup = struct.pack('<BxHHHHxx', 0x6c, 11, 0, 0, 0)
    sock.sendall(setup)

    # Read response header
    header = b''
    while len(header) < 8:
        header += sock.recv(8 - len(header))

    status = header[0]
    if status != 1:
        raise Exception(f"Connection failed with status {status}")

    additional = struct.unpack_from('<H', header, 6)[0]
    body = b''
    remaining = additional * 4
    while len(body) < remaining:
        body += sock.recv(remaining - len(body))

    rid_base = struct.unpack_from('<I', body, 4)[0]
    rid_mask = struct.unpack_from('<I', body, 8)[0]
    return sock, rid_base, rid_mask

# Test 1: Send a request with length=0 (should be rejected or ignored)
try:
    sock, base, mask = x11_connect()
    # A request with opcode=1 (CreateWindow) but length=0
    bad_req = struct.pack('<BxH', 1, 0)
    sock.sendall(bad_req)
    time.sleep(0.3)
    # Try to read - server should send an error or close connection
    try:
        resp = sock.recv(1024)
        if len(resp) > 0:
            print(f"PASS: zero-length request got {len(resp)} byte response")
        else:
            print("PASS: zero-length request closed connection cleanly")
    except (socket.timeout, ConnectionResetError, BrokenPipeError):
        print("PASS: zero-length request handled (connection reset/timeout)")
    sock.close()
except Exception as e:
    print(f"PASS: zero-length request handled with {type(e).__name__}: {e}")

# Test 2: Send a request with impossibly large length
try:
    sock, base, mask = x11_connect()
    # InternAtom (opcode 16) with length claiming 65535 quads
    bad_req = struct.pack('<BxHHxx', 16, 65535, 4) + b'TEST'
    sock.sendall(bad_req)
    time.sleep(0.5)
    try:
        resp = sock.recv(1024)
        if len(resp) > 0:
            print(f"PASS: oversized request got {len(resp)} byte response")
        else:
            print("PASS: oversized request closed connection cleanly")
    except (socket.timeout, ConnectionResetError, BrokenPipeError):
        print("PASS: oversized request handled (connection reset/timeout)")
    sock.close()
except Exception as e:
    print(f"PASS: oversized request handled with {type(e).__name__}: {e}")

# Test 3: Send an unknown opcode
try:
    sock, base, mask = x11_connect()
    # Opcode 255 is not a valid core request
    unknown_req = struct.pack('<BxH', 255, 1)
    sock.sendall(unknown_req)
    time.sleep(0.3)
    try:
        resp = sock.recv(1024)
        if len(resp) >= 32:
            error_code = resp[1]
            print(f"PASS: unknown opcode 255 got error response (code={error_code})")
        elif len(resp) > 0:
            print(f"PASS: unknown opcode 255 got {len(resp)} byte response")
        else:
            print("PASS: unknown opcode 255 closed connection cleanly")
    except (socket.timeout, ConnectionResetError, BrokenPipeError):
        print("PASS: unknown opcode 255 handled (connection reset/timeout)")
    sock.close()
except Exception as e:
    print(f"PASS: unknown opcode 255 handled with {type(e).__name__}: {e}")

# Test 4: Send truncated InternAtom (claims 5 bytes of name but only sends 2)
try:
    sock, base, mask = x11_connect()
    # InternAtom: opcode=16, length=3 (header + 5 bytes name + 3 pad = 12 = 3 quads)
    # But we only send the header + 2 bytes instead of the full 12
    truncated = struct.pack('<BxHHxx', 16, 3, 5) + b'AB'
    sock.sendall(truncated)
    time.sleep(0.5)
    try:
        resp = sock.recv(1024)
        if len(resp) > 0:
            print(f"PASS: truncated InternAtom got {len(resp)} byte response")
        else:
            print("PASS: truncated InternAtom closed connection cleanly")
    except (socket.timeout, ConnectionResetError, BrokenPipeError):
        print("PASS: truncated InternAtom handled (connection reset/timeout)")
    sock.close()
except Exception as e:
    print(f"PASS: truncated InternAtom handled with {type(e).__name__}: {e}")

# Test 5: Verify the server is still alive for other connections
try:
    import Xlib.display
    d = Xlib.display.Display(':99')
    root = d.screen().root
    geom = root.get_geometry()
    d.close()
    print(f"PASS: server alive after raw socket abuse (root={geom.width}x{geom.height})")
except Exception as e:
    errors.append(f"Server not reachable after fuzzing: {e}")

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("FUZZING_RAW_SOCKET_OK")
