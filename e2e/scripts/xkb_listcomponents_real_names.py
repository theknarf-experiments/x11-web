from Xlib import display
d = display.Display()
# Use xkbcomp to dump current keymap and verify it parses
import subprocess
r = subprocess.run(["xkbcomp", ":99", "-"], capture_output=True, timeout=10)
out = r.stdout.decode(errors="replace")
if "xkb_keymap" in out or "xkb_keycodes" in out:
    print(f"PASS: xkbcomp returned valid keymap ({len(out)} bytes)")
else:
    print(f"FAIL: xkbcomp output unexpected: {out[:200]}")
d.close()
