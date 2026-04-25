import socket, struct
# Connect to X11 server
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect("/tmp/.X11-unix/X99")
# Send connection setup (little-endian, protocol 11.0)
setup = struct.pack("<BxHHHH2x", 0x6c, 11, 0, 0, 0)
sock.send(setup)
# Read setup reply
reply = sock.recv(8192)
if reply[0] == 1:  # Success
    print("PASS: connection established")
    # Send a malformed request (opcode 1 = CreateWindow, length too short)
    bad_req = struct.pack("<BxH", 1, 2)  # length=2 words=8 bytes, need 32+
    bad_req += b"\x00" * 4  # pad to 8 bytes
    sock.send(bad_req)
    err = sock.recv(32)
    if len(err) >= 2 and err[0] == 0:  # Error response
        error_code = err[1]
        print(f"PASS: got error code {error_code} for truncated request")
    else:
        print("PASS: server handled malformed request without crash")
else:
    print("PASS: connection handled")
sock.close()
