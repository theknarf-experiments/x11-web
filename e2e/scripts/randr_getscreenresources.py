import subprocess, re
out = subprocess.check_output(["xrandr", "--query"], env={"DISPLAY": ":99"}).decode()
print(out)
# Should contain a connected output
assert "connected" in out, "No connected output"
# Should report resolution
m = re.search(r"(\d+)x(\d+)", out)
assert m, "No resolution found"
w, h = int(m.group(1)), int(m.group(2))
assert w >= 640 and h >= 480, f"Resolution too small: {w}x{h}"
print(f"RESOLUTION={w}x{h}")
print("RANDR_OK")
