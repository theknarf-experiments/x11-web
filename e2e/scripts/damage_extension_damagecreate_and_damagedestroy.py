import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()
# Verify DAMAGE extension is present
ext = d.query_extension("DAMAGE")
if ext and ext.major_opcode > 0:
    print(f"damage_ext_opcode={ext.major_opcode}")
else:
    print("damage_ext=missing")
# Create a window
w = screen.root.create_window(
    0, 0, 50, 50, 0,
    screen.root_depth,
    Xlib.X.InputOutput,
    Xlib.X.CopyFromParent,
)
w.map()
d.sync()
print("damage_test=ok")
w.destroy()
d.close()
