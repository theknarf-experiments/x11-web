import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display(":99")
screen = d.screen()
root = screen.root
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.StructureNotifyMask)
w.map()
d.sync()
# Set WM_PROTOCOLS with WM_DELETE_WINDOW
wm_protocols = d.intern_atom("WM_PROTOCOLS")
wm_delete = d.intern_atom("WM_DELETE_WINDOW")
import struct
w.change_property(wm_protocols, Xlib.Xatom.ATOM, 32,
    [wm_delete])
d.sync()
# Verify the property is set
prop = w.get_property(wm_protocols, Xlib.Xatom.ATOM, 0, 100)
assert prop is not None, "WM_PROTOCOLS not set"
atoms = list(prop.value)
assert wm_delete in atoms, "WM_DELETE_WINDOW not in WM_PROTOCOLS"
print("WM_DELETE_OK")
w.destroy()
d.sync()
d.close()
