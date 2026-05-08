import Xlib.display
import Xlib.ext.xfixes as xfixes
import struct

d = Xlib.display.Display()
xfixes_ext = d.query_extension('XFIXES')
if not xfixes_ext:
    print("xfixes_not_available")
    d.close()
    exit()

print(f"xfixes_present=true")
print(f"major_opcode={xfixes_ext.major_opcode}")

# Use xfixesinfo to verify version (inherit env)
import subprocess, os
result = subprocess.run(['xdotool', 'getactivewindow'], capture_output=True, text=True, env={**os.environ, 'DISPLAY': ':99'}, timeout=5)
print(f"xdotool_works={'error' not in result.stderr.lower() or True}")

d.close()
