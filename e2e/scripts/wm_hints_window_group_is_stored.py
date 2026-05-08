from Xlib import display, X, Xutil
d = display.Display()
root = d.screen().root
leader = root.create_window(10, 10, 100, 100, 0, d.screen().root_depth)
child = root.create_window(20, 20, 100, 100, 0, d.screen().root_depth)

# Set WM_HINTS with window_group on child
hints = {}
hints['flags'] = Xutil.WindowGroupHint
hints['window_group'] = leader.id
child.set_wm_hints(hints)
d.sync()

read_hints = child.get_wm_hints()
group = getattr(read_hints, 'window_group', 0) if read_hints else 0
group_id = group.id if hasattr(group, 'id') else group
print(f"group_matches={group_id == leader.id}")
leader.destroy()
child.destroy()
d.close()
