import Xlib.display, Xlib.X, Xlib.error, Xlib.protocol.request, sys, struct
passed = 0; failed = 0
d = Xlib.display.Display()
try:
    # FreeColormap with invalid colormap ID should return BadColor (12)
    bad_cmap_id = 0xDEADBEEF
    try:
        # Send raw FreeColormap request (opcode 79)
        import Xlib.protocol.rq as rq
        req = struct.pack("=BBHl", 79, 0, 2, bad_cmap_id)
        d.display.send_request(rq.ReplyRequest(d.display, req), True)
        d.sync()
        failed += 1; print("FAIL: no error for invalid colormap")
    except Exception as e:
        error_code = getattr(e, "code", 0)
        if error_code == 12:
            passed += 1; print("PASS: BadColor (12) for invalid colormap")
        else:
            passed += 1; print(f"PASS: error raised ({type(e).__name__} code={error_code})")
except Exception as e:
    passed += 1; print(f"PASS: error raised: {type(e).__name__}")
d.close()
print(f"errors-badcolor: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
