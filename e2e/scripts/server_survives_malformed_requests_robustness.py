import socket, struct, time
# Connect to X server
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect("/tmp/.X11-unix/X99")
# Send valid setup request (LSB first, no auth)
setup = bytearray(12)
setup[0] = 0x6c  # LSB first
struct.pack_into("<HH", setup, 2, 11, 0)  # proto 11.0
sock.sendall(setup)
# Read setup reply
reply = sock.recv(8192)
if reply[0] != 1:
    print("FAIL: setup failed")
    exit(1)
# Send various malformed requests
tests_passed = 0
# Test 1: Zero-length request (should be handled gracefully)
try:
    bad = struct.pack("<BBH", 98, 0, 0)  # QueryExtension with len=0
    sock.sendall(bad)
    time.sleep(0.1)
    tests_passed += 1
except: pass
# Test 2: Truncated request
try:
    bad = struct.pack("<BBH", 16, 0, 2) + b"\x00" * 4  # InternAtom truncated
    sock.sendall(bad)
    resp = sock.recv(4096)
    tests_passed += 1
except: pass
# Test 3: Bad window ID in GetWindowAttributes
try:
    bad = struct.pack("<BBH", 3, 0, 2) + struct.pack("<I", 0xDEADBEEF)
    sock.sendall(bad)
    resp = sock.recv(4096)
    if resp and resp[0] == 0:  # Error response
        tests_passed += 1
except: pass
# Test 4: Bad atom in GetAtomName
try:
    bad = struct.pack("<BBH", 17, 0, 2) + struct.pack("<I", 0xFFFFFFFF)
    sock.sendall(bad)
    resp = sock.recv(4096)
    if resp and resp[0] == 0:  # Error response
        tests_passed += 1
except: pass
sock.close()
# Verify server still works after malformed requests
import Xlib.display
try:
    d = Xlib.display.Display()
    info = d.display_name()
    d.close()
    tests_passed += 1
except Exception as e:
    print(f"FAIL: server crashed after fuzzing: {e}")
    exit(1)
print(f"PASS: {tests_passed} robustness tests passed, server still responsive")
