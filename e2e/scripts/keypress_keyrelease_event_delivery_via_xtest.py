import Xlib.display, Xlib.X
import time

d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    event_mask=Xlib.X.KeyPressMask | Xlib.X.KeyReleaseMask)
w.map()
d.sync()

# Set focus to our window
d.set_input_focus(w, Xlib.X.RevertToParent, Xlib.X.CurrentTime)
d.sync()

# Use xdotool to send a key press (inherit env so PATH/LD_* survive)
import subprocess, os
subprocess.run(['xdotool', 'key', '--window', str(w.id), 'a'],
    env={**os.environ, 'DISPLAY': ':99'}, capture_output=True, timeout=5)

time.sleep(0.5)
d.sync()

key_events = 0
while d.pending_events():
    e = d.next_event()
    if e.type in (Xlib.X.KeyPress, Xlib.X.KeyRelease):
        key_events += 1

print(f"key_events={key_events}")
print(f"got_key_events={key_events > 0}")

w.destroy()
d.close()
