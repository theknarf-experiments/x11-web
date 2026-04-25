import Xlib.display, Xlib.X, sys
from Xlib.protocol import request
passed = 0; failed = 0
d = Xlib.display.Display()
root = d.screen().root
try:
    # QueryBestSize for Cursor (class 0)
    reply = d.query_best_size(0, root, 32, 32)
    if reply.width > 0 and reply.height > 0:
        passed += 1; print(f"PASS: cursor best={reply.width}x{reply.height}")
    else:
        failed += 1; print(f"FAIL: cursor best={reply.width}x{reply.height}")
    # QueryBestSize for Tile (class 1)
    reply = d.query_best_size(1, root, 16, 16)
    if reply.width > 0 and reply.height > 0:
        passed += 1; print(f"PASS: tile best={reply.width}x{reply.height}")
    else:
        failed += 1; print(f"FAIL: tile best={reply.width}x{reply.height}")
    # QueryBestSize for Stipple (class 2)
    reply = d.query_best_size(2, root, 8, 8)
    if reply.width > 0 and reply.height > 0:
        passed += 1; print(f"PASS: stipple best={reply.width}x{reply.height}")
    else:
        failed += 1; print(f"FAIL: stipple best={reply.width}x{reply.height}")
except Exception as e:
    failed += 1; print(f"FAIL: {e}")
d.close()
print(f"xts-bestsize: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
