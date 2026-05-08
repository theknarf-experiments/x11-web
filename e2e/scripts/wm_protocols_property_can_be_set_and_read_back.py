from Xlib import display, X
d = display.Display()
root = d.screen().root
w = root.create_window(10, 10, 200, 200, 0, d.screen().root_depth)

wm_protocols = d.intern_atom('WM_PROTOCOLS')
wm_delete = d.intern_atom('WM_DELETE_WINDOW')
wm_take_focus = d.intern_atom('WM_TAKE_FOCUS')

w.change_property(wm_protocols, 4, 32, [wm_delete, wm_take_focus])
d.sync()

prop = w.get_full_property(wm_protocols, X.AnyPropertyType)
if prop and prop.value:
    import array
    atoms = array.array('I', prop.value)
    has_delete = wm_delete in atoms
    has_focus = wm_take_focus in atoms
    print(f"has_delete={has_delete}")
    print(f"has_focus={has_focus}")
else:
    print("has_delete=False")
    print("has_focus=False")

w.destroy()
d.close()
