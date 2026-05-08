import Xlib.display
import Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# Create test window
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    window_class=Xlib.X.InputOutput,
    visual=Xlib.X.CopyFromParent)
w.map()
d.sync()

# Test that the Composite extension is queryable
comp = d.query_extension('Composite')
if comp:
    print(f"composite_present=true")
    print(f"composite_opcode={comp.major_opcode}")
else:
    print("composite_present=false")

w.destroy()
d.sync()
print("composite_test=ok")
d.close()
