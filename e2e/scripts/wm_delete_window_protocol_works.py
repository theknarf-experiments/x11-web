from Xlib import display, X, Xatom
d = display.Display()
screen = d.screen()
root = screen.root
wm_delete = d.intern_atom('WM_DELETE_WINDOW')
wm_protocols = d.intern_atom('WM_PROTOCOLS')
# Create window and set WM_PROTOCOLS
w = root.create_window(
    10, 10, 200, 200, 0,
    screen.root_depth,
    X.InputOutput,
    X.CopyFromParent,
    event_mask=X.StructureNotifyMask,
)
w.set_wm_protocols([wm_delete])
w.map()
d.sync()
# Verify the property was set
prop = w.get_full_property(wm_protocols, Xatom.ATOM)
if prop and prop.value:
    protocols = list(prop.value)
    print(f"delete_protocol_set={wm_delete in protocols}")
else:
    print("delete_protocol_set=False")
w.destroy()
d.close()
