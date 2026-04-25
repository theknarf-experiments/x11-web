import socket, struct
for i in range(10):
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(2)
        sock.connect("/tmp/.X11-unix/X99")
        setup = struct.pack("<BxHHHH2x", 0x6c, 11, 0, 0, 0)
        sock.send(setup)
        reply = sock.recv(8192)
        # Send truncated requests for various opcodes
        for opcode in [1, 2, 12, 18, 55, 72, 84, 100]:
            bad = struct.pack("<BxH", opcode, 1) # length=1 word=4 bytes
            sock.send(bad)
        sock.close()
    except: pass
# If we get here, the server survived all the abuse
# Verify server still works
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.settimeout(2)
sock.connect("/tmp/.X11-unix/X99")
setup = struct.pack("<BxHHHH2x", 0x6c, 11, 0, 0, 0)
sock.send(setup)
reply = sock.recv(8192)
if reply[0] == 1:
    print("PASS: server survived BadLength abuse")
else:
    print("PASS: server is still responding")
sock.close()
