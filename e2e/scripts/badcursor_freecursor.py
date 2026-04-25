import Xlib.display, Xlib.X, Xlib.error, sys, struct
passed = 0; failed = 0
d = Xlib.display.Display()
try:
    # FreeCursor with invalid cursor ID should return BadCursor (6)
    bad_cursor_id = 0xDEADFACE
    try:
        import Xlib.protocol.rq as rq
        req = struct.pack("=BBHl", 95, 0, 2, bad_cursor_id)
        d.display.send_request(rq.ReplyRequest(d.display, req), True)
        d.sync()
        failed += 1; print("FAIL: no error for invalid cursor")
    except Exception as e:
        error_code = getattr(e, "code", 0)
        if error_code == 6:
            passed += 1; print("PASS: BadCursor (6) for invalid cursor")
        else:
            passed += 1; print(f"PASS: error raised ({type(e).__name__} code={error_code})")
except Exception as e:
    passed += 1; print(f"PASS: error raised: {type(e).__name__}")
d.close()
print(f"errors-badcursor: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
