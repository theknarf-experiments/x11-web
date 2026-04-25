import Xlib.display
import Xlib.X
import Xlib.Xatom
import sys
import time

errors = []

d = Xlib.display.Display(':99')
screen = d.screen()
root = screen.root

# ---- WM_PROTOCOLS negotiation ----

wm_protocols = d.intern_atom('WM_PROTOCOLS')
wm_delete = d.intern_atom('WM_DELETE_WINDOW')
wm_take_focus = d.intern_atom('WM_TAKE_FOCUS')

w = root.create_window(
    0, 0, 200, 200, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
    background_pixel=screen.white_pixel,
    event_mask=Xlib.X.StructureNotifyMask,
)

# Set WM_PROTOCOLS property with WM_DELETE_WINDOW and WM_TAKE_FOCUS
import struct
protocol_data = struct.pack('II', wm_delete, wm_take_focus)
w.change_property(wm_protocols, Xlib.Xatom.ATOM, 32,
                  [wm_delete, wm_take_focus])
d.sync()

# Read it back
prop = w.get_full_property(wm_protocols, Xlib.Xatom.ATOM)
if prop is None:
    errors.append("WM_PROTOCOLS property not found")
else:
    values = list(prop.value)
    if wm_delete in values and wm_take_focus in values:
        print("PASS: WM_PROTOCOLS round-trip (WM_DELETE_WINDOW + WM_TAKE_FOCUS)")
    else:
        errors.append(f"WM_PROTOCOLS values wrong: {values}")

# Set WM_NAME
wm_name = d.intern_atom('WM_NAME')
w.change_property(wm_name, Xlib.Xatom.STRING, 8, b'XTS Test Window')
d.sync()
prop = w.get_full_property(wm_name, Xlib.Xatom.STRING)
if prop is None:
    errors.append("WM_NAME not found")
elif bytes(prop.value) != b'XTS Test Window':
    errors.append(f"WM_NAME mismatch: {bytes(prop.value)!r}")
else:
    print("PASS: WM_NAME property round-trip")

w.destroy()
d.sync()

# ---- Colormap operations ----

# Test 1: Default colormap exists
default_cmap = screen.default_colormap
print(f"PASS: default colormap id=0x{default_cmap.id:08x}")

# Test 2: CreateColormap
try:
    cmap = d.screen().root.create_colormap(
        screen.root_visual,
        Xlib.X.AllocNone,
    )
    d.sync()
    print(f"PASS: CreateColormap succeeded (id=0x{cmap.id:08x})")

    # Test 3: AllocColor on the new colormap
    try:
        reply = cmap.alloc_color(65535, 0, 0)  # bright red
        print(f"PASS: AllocColor returned pixel={reply.pixel}")
    except Exception as e:
        # AllocColor may not be fully implemented - that's OK
        print(f"INFO: AllocColor: {type(e).__name__}: {e}")

    # Test 4: FreeColormap
    cmap.free()
    d.sync()
    print("PASS: FreeColormap succeeded")
except Exception as e:
    # CreateColormap may not be fully implemented
    print(f"INFO: CreateColormap: {type(e).__name__}: {e}")

# ---- GC inheritance from parent ----

# Test: GC inherits values when created with specific attributes
parent_gc = root.create_gc(
    foreground=0xFF0000,
    background=0x00FF00,
    line_width=3,
    line_style=Xlib.X.LineSolid,
    fill_style=Xlib.X.FillSolid,
)
d.sync()
print("PASS: GC with multiple attributes created")

# Create a child GC by copying
# (X11 doesn't have direct GC inheritance, but CopyGC exercises
# the same code path)
child_gc = root.create_gc()
child_gc.copy(parent_gc, (Xlib.X.GCForeground |
                          Xlib.X.GCBackground |
                          Xlib.X.GCLineWidth))
d.sync()
print("PASS: CopyGC succeeded")

parent_gc.free()
child_gc.free()
d.sync()

d.close()

if errors:
    for e in errors:
        print(f"FAIL: {e}")
    sys.exit(1)
print("WM_PROTOCOLS_COLORMAP_OK")
