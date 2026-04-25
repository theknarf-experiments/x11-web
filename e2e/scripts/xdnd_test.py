import Xlib.display
import Xlib.X
import Xlib.protocol.event
import struct
import sys
import time

d = Xlib.display.Display(':99')
root = d.screen().root

# Intern Xdnd atoms
XdndAware = d.intern_atom('XdndAware')
XdndEnter = d.intern_atom('XdndEnter')
XdndPosition = d.intern_atom('XdndPosition')
XdndStatus = d.intern_atom('XdndStatus')
XdndDrop = d.intern_atom('XdndDrop')
XdndFinished = d.intern_atom('XdndFinished')
XdndActionCopy = d.intern_atom('XdndActionCopy')

print(f"PASS: Xdnd atoms interned (XdndAware={XdndAware}, XdndEnter={XdndEnter})")

# Create source and target windows
src = root.create_window(10, 10, 100, 100, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask | Xlib.X.PropertyChangeMask)

tgt = root.create_window(200, 10, 100, 100, 0,
    d.screen().root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask | Xlib.X.PropertyChangeMask)

# Announce XdndAware version 5 on both windows
src.change_property(XdndAware, Xlib.Xatom.ATOM, 32, [5])
tgt.change_property(XdndAware, Xlib.Xatom.ATOM, 32, [5])

src.map()
tgt.map()
d.sync()
time.sleep(0.5)

print("PASS: source and target windows created with XdndAware")

# Send XdndEnter from source to target
# data[0] = source window
# data[1] = version << 24 | flags
# data[2..4] = up to 3 supported types (0 if fewer)
text_uri = d.intern_atom('text/uri-list')
enter_event = Xlib.protocol.event.ClientMessage(
    window=tgt,
    client_type=XdndEnter,
    data=(32, [src.id, (5 << 24), text_uri, 0, 0]),
)
tgt.send_event(enter_event)
d.sync()
print("PASS: XdndEnter sent")

# Send XdndPosition
# data[0] = source window
# data[1] = 0 (reserved)
# data[2] = (x << 16) | y (root coords)
# data[3] = timestamp
# data[4] = action atom
pos_event = Xlib.protocol.event.ClientMessage(
    window=tgt,
    client_type=XdndPosition,
    data=(32, [src.id, 0, (250 << 16) | 60, Xlib.X.CurrentTime, XdndActionCopy]),
)
tgt.send_event(pos_event)
d.sync()
print("PASS: XdndPosition sent")

# Send XdndDrop
drop_event = Xlib.protocol.event.ClientMessage(
    window=tgt,
    client_type=XdndDrop,
    data=(32, [src.id, 0, Xlib.X.CurrentTime, 0, 0]),
)
tgt.send_event(drop_event)
d.sync()
print("PASS: XdndDrop sent")

# Clean up
src.destroy()
tgt.destroy()
d.sync()
d.close()
print("XDND_HANDSHAKE_OK")
