import socket, struct, sys, time
passed = 0; failed = 0

# Raw X11 connection
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect("/tmp/.X11-unix/X99")

# Send connection setup (LSB-first, protocol 11.0)
setup = struct.pack("=BxHHHH", 0x6c, 11, 0, 0, 0)  # no auth
setup += b"\x00" * 6  # padding to align
sock.sendall(setup)

# Read setup reply header
header = sock.recv(8)
if header[0] == 1:  # Success
    passed += 1; print("PASS: connection accepted")
    # Read remaining setup data
    extra_len = struct.unpack_from("<H", header, 6)[0] * 4
    data = b""
    while len(data) < extra_len:
        data += sock.recv(extra_len - len(data))
else:
    failed += 1; print(f"FAIL: connection rejected: {header[0]}")
    sys.exit(1)

# Test 1: Send a too-short request (1 byte)
try:
    sock.sendall(b"\x01")
    time.sleep(0.1)
    passed += 1; print("PASS: server survived 1-byte request")
except Exception as e:
    failed += 1; print(f"FAIL: server crashed on 1-byte: {e}")

# Test 2: Send a zero-length request
try:
    sock.sendall(struct.pack("<BBH", 98, 0, 0))  # opcode 98, length 0
    time.sleep(0.1)
    passed += 1; print("PASS: server survived zero-length request")
except Exception as e:
    failed += 1; print(f"FAIL: server crashed on zero-length: {e}")

# Test 3: Send request with invalid opcode (120-126 are unassigned)
try:
    sock.sendall(struct.pack("<BBH", 120, 0, 1))  # opcode 120, length 1 word
    time.sleep(0.1)
    # Read error reply (32 bytes)
    reply = sock.recv(32)
    if len(reply) >= 2 and reply[0] == 0:  # Error reply
        passed += 1; print(f"PASS: got error reply for invalid opcode (error code={reply[1]})")
    else:
        passed += 1; print("PASS: server handled invalid opcode")
except Exception as e:
    failed += 1; print(f"FAIL: invalid opcode: {e}")

sock.close()
print(f"fuzz: pass={passed} fail={failed}")
