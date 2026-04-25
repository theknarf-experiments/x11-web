import sys
import struct
import socket

# Manual X11 connection handshake to verify byte-level conformance
# with the X11 connection setup protocol (Section 8 of the X protocol spec).

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect('/tmp/.X11-unix/X99')

# Send connection setup request (little-endian, protocol 11.0)
# Byte order: 0x6c = little-endian
# Protocol major: 11, minor: 0
# Auth proto name length: 0, auth proto data length: 0
setup = struct.pack('<BxHHHHxx', 0x6c, 11, 0, 0, 0)
sock.sendall(setup)

# Read the response header (8 bytes minimum)
header = b''
while len(header) < 8:
    chunk = sock.recv(8 - len(header))
    if not chunk:
        print("FAIL: connection closed before header")
        sys.exit(1)
    header += chunk

status = header[0]
# status: 0=Failed, 1=Success, 2=Authenticate
if status == 1:
    print("PASS: connection setup succeeded (status=1)")
    # Parse the additional length field
    additional_length = struct.unpack_from('<H', header, 6)[0]
    # Read the rest of the setup response
    remaining = additional_length * 4
    body = b''
    while len(body) < remaining:
        chunk = sock.recv(remaining - len(body))
        if not chunk:
            break
        body += chunk

    # Parse server info from the setup response
    # Bytes 0-3: release number (4 bytes)
    # Bytes 4-7: resource-id-base (4 bytes)
    # Bytes 8-11: resource-id-mask (4 bytes)
    # Bytes 12-15: motion-buffer-size (4 bytes)
    # Bytes 16-17: vendor length (2 bytes)
    # Bytes 18-19: max request length (2 bytes)
    # Bytes 20: number of screens (1 byte)
    # Bytes 21: number of pixmap formats (1 byte)
    if len(body) >= 22:
        release = struct.unpack_from('<I', body, 0)[0]
        rid_base = struct.unpack_from('<I', body, 4)[0]
        rid_mask = struct.unpack_from('<I', body, 8)[0]
        vendor_len = struct.unpack_from('<H', body, 16)[0]
        max_req = struct.unpack_from('<H', body, 18)[0]
        num_screens = body[20]
        num_formats = body[21]
        print(f"PASS: release={release}")
        print(f"PASS: resource-id-base=0x{rid_base:08x}")
        print(f"PASS: resource-id-mask=0x{rid_mask:08x}")
        print(f"PASS: max-request-length={max_req}")
        print(f"PASS: screens={num_screens}")
        print(f"PASS: pixmap-formats={num_formats}")
        if rid_mask == 0:
            print("FAIL: resource-id-mask is zero")
            sys.exit(1)
        if num_screens < 1:
            print("FAIL: no screens")
            sys.exit(1)
        if max_req < 256:
            print("FAIL: max-request-length too small")
            sys.exit(1)
    else:
        print(f"FAIL: setup body too short ({len(body)} bytes)")
        sys.exit(1)

    # Now test QueryExtension (opcode 98) for a known extension
    # Request: opcode=98, pad=0, length=2+((n+p)/4), name
    ext_name = b'SHAPE'
    n = len(ext_name)
    pad = (4 - (n % 4)) % 4
    req_len = 2 + (n + pad) // 4
    req = struct.pack('<BxHH', 98, req_len, n)
    req += b'\x00' * 2  # unused padding after name-length
    req += ext_name + b'\x00' * pad

    # Actually, QueryExtension wire format is:
    # opcode(1) + unused(1) + length(2) + name-length(2) + unused(2) + name + pad
    req = struct.pack('<BxHHxx', 98, req_len, n) + ext_name + b'\x00' * pad
    sock.sendall(req)

    # Read reply (32 bytes)
    reply = b''
    while len(reply) < 32:
        chunk = sock.recv(32 - len(reply))
        if not chunk:
            break
        reply += chunk

    if len(reply) == 32:
        reply_type = reply[0]
        present = reply[8]
        major_opcode = reply[9]
        if reply_type == 1:
            print(f"PASS: QueryExtension reply received")
            print(f"PASS: SHAPE present={present} major_opcode={major_opcode}")
        else:
            print(f"FAIL: unexpected reply type {reply_type}")
    else:
        print(f"FAIL: incomplete reply ({len(reply)} bytes)")

elif status == 0:
    reason_len = header[1]
    print(f"FAIL: connection refused, reason_length={reason_len}")
    sys.exit(1)
else:
    print(f"FAIL: unexpected status {status}")
    sys.exit(1)

sock.close()
print("XTS_SETUP_OK")
