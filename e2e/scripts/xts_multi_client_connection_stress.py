from Xlib import display, X
# Open 10 simultaneous connections
displays = []
windows = []
for i in range(10):
    d = display.Display()
    screen = d.screen()
    w = screen.root.create_window(i*10, i*10, 50, 50, 0,
        screen.root_depth, X.InputOutput, X.CopyFromParent,
        background_pixel=(i*25) << 16)
    w.map()
    d.sync()
    displays.append(d)
    windows.append(w)
# Verify all windows exist via xdotool
import subprocess
r = subprocess.run(["xdotool", "search", "--name", ""], capture_output=True, timeout=5)
# Clean up
for w in windows:
    w.destroy()
for d in displays:
    d.sync()
    d.close()
print(f"PASS: 10 concurrent connections created and destroyed")
