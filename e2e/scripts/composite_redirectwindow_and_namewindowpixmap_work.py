import Xlib.display, Xlib.X
import struct

d = Xlib.display.Display()
screen = d.screen()

# Query Composite extension
comp = d.query_extension('Composite')
if comp is None or comp.major_opcode == 0:
    print("composite_not_found")
    d.close()
    exit()

opcode = comp.major_opcode

# Create a window
w = screen.root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()

# CompositeQueryVersion (minor=0): check version support
req = struct.pack('<BBHII', opcode, 0, 4, 0, 4)
d.send_request(Xlib.protocol.rq.ReplyRequest(
    _data = req + b'\\x00' * (16 - len(req)),
), True)
d.sync()
print("composite_query_ok=True")

# RedirectWindow (minor=1): redirect the window for compositing
# data: major_opcode, minor=1, length=3, window(4), update(1), pad(3)
redirect_data = struct.pack('<BBHI', opcode, 1, 3, w.id) + struct.pack('B', 0) + b'\\x00' * 3
d.send_request(Xlib.protocol.rq.Request(
    _data = redirect_data,
), True)
d.sync()
print("redirect_ok=True")

# UnredirectWindow (minor=3): un-redirect
unredir_data = struct.pack('<BBHI', opcode, 3, 2, w.id)
d.send_request(Xlib.protocol.rq.Request(
    _data = unredir_data,
), True)
d.sync()
print("unredirect_ok=True")

w.destroy()
d.close()
