import Xlib.display, Xlib.X, sys
passed = 0; failed = 0
d = Xlib.display.Display()
screen = d.screen()
cmap = screen.default_colormap
# Test 1: AllocColor (TrueColor should return exact values)
r = cmap.alloc_color(0xFFFF, 0x0000, 0x0000)
if r.red == 0xFFFF and r.green == 0 and r.blue == 0:
    passed += 1
else:
    print(f"FAIL: alloc_color red got r={r.red} g={r.green} b={r.blue}")
    failed += 1
# Test 2: AllocNamedColor
try:
    n = cmap.alloc_named_color("blue")
    if n.exact_blue > 0:
        passed += 1
    else:
        print(f"FAIL: alloc_named_color blue={n.exact_blue}")
        failed += 1
except Exception as e:
    print(f"FAIL: alloc_named_color exception: {e}")
    failed += 1
# Test 3: QueryColors
try:
    colors = cmap.query_colors([0xFF0000, 0x00FF00, 0x0000FF])
    if len(colors) == 3:
        passed += 1
    else:
        print(f"FAIL: query_colors returned {len(colors)} entries")
        failed += 1
except Exception as e:
    print(f"FAIL: query_colors exception: {e}")
    failed += 1
# Test 4: LookupColor
try:
    lc = cmap.lookup_color("red")
    if lc.exact_red > 0:
        passed += 1
    else:
        print(f"FAIL: lookup_color red={lc.exact_red}")
        failed += 1
except Exception as e:
    print(f"FAIL: lookup_color exception: {e}")
    failed += 1
d.close()
print(f"colormap: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
