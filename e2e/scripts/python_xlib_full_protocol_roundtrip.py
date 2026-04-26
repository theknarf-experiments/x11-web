from Xlib import X, display, Xutil
d = display.Display()
screen = d.screen()
# 1. Create window
w = screen.root.create_window(10, 10, 200, 150, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent,
    event_mask=X.StructureNotifyMask | X.ExposureMask)
# 2. Set WM_HINTS with NormalState (kwargs form — no Xutil.Hints class)
w.set_wm_hints(flags=Xutil.InputHint | Xutil.StateHint, input=1, initial_state=1)
# 3. Map window
w.map()
d.sync()
# 4. Query window attributes
attrs = w.get_attributes()
print(f'map_state={attrs.map_state}')
# 5. Test colormap
cmap = screen.default_colormap
color = cmap.alloc_color(0, 0, 65535)
print(f'blue_pixel={color.pixel}')
# 6. Query extension
sync_ext = d.query_extension('SYNC')
dbe_ext = d.query_extension('DOUBLE-BUFFER')
print(f'SYNC={sync_ext is not None} DBE={dbe_ext is not None}')
# 7. Cleanup
w.destroy()
d.close()
print('ALL_OK')
