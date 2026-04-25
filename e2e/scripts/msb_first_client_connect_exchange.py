import socket, struct, os

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect('/tmp/.X11-unix/X99')

# Send MSB-first (big-endian) connection setup
auth_name = b'MIT-MAGIC-COOKIE-1'
auth_cookie = b''
try:
    with open(os.environ.get('XAUTHORITY', '/tmp/.x11-web-Xauthority'), 'rb') as f:
        data = f.read()
        if len(data) > 20:
            auth_cookie = data[-16:]
except:
    pass

setup = struct.pack('>BxHHHH2x',
    0x42,  # MSB first (big-endian)
    11, 0,
    len(auth_name),
    len(auth_cookie))
setup += auth_name
while len(setup) % 4: setup += b'\x00'
setup += auth_cookie
while len(setup) % 4: setup += b'\x00'
sock.sendall(setup)

# Read setup reply (should be in big-endian)
reply = sock.recv(8)
status = reply[0]
if status != 1:
    print(f'setup failed: status={status}')
    sock.close()
    exit(1)

# Parse big-endian setup reply
proto_major = struct.unpack_from('>H', reply, 2)[0]
proto_minor = struct.unpack_from('>H', reply, 4)[0]
extra_len = struct.unpack_from('>H', reply, 6)[0] * 4

rest = b''
while len(rest) < extra_len:
    rest += sock.recv(extra_len - len(rest))

# Parse key fields (big-endian)
release = struct.unpack_from('>I', rest, 0)[0]
resource_base = struct.unpack_from('>I', rest, 4)[0]
resource_mask = struct.unpack_from('>I', rest, 8)[0]

print(f'MSB setup: proto={proto_major}.{proto_minor} base={resource_base:#x} mask={resource_mask:#x}')

# Send InternAtom (big-endian): opcode 16, length 3
atom_name = b'TEST_ATOM'
name_len = len(atom_name)
padded = (name_len + 3) & ~3
req_len = (8 + padded) // 4
req = struct.pack('>BBH', 16, 0, req_len)
req += struct.pack('>H2x', name_len)
req += atom_name
while len(req) % 4: req += b'\x00'
sock.sendall(req)

# Read reply (should be big-endian)
resp = sock.recv(32)
if resp[0] == 1:  # Reply
    atom_id = struct.unpack_from('>I', resp, 8)[0]
    print(f'InternAtom reply: atom={atom_id}')
else:
    print(f'unexpected response type: {resp[0]}')

# Send GetAtomName for that atom (big-endian)
req2 = struct.pack('>BBH', 17, 0, 2) + struct.pack('>I', atom_id)
sock.sendall(req2)
resp2 = sock.recv(64)
if resp2[0] == 1:
    name_len2 = struct.unpack_from('>H', resp2, 8)[0]
    name_bytes = resp2[32:32+name_len2]
    print(f'GetAtomName reply: name={name_bytes.decode()}')

sock.close()
print('msb-test-complete')
