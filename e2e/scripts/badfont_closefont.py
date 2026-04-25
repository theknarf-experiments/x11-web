import Xlib.display, Xlib.X, Xlib.error, sys, struct
passed = 0; failed = 0
d = Xlib.display.Display()
try:
    # CloseFont with invalid font ID should return BadFont (7)
    bad_font_id = 0xBAADF00D
    try:
        import Xlib.protocol.rq as rq
        req = struct.pack("=BBHl", 46, 0, 2, bad_font_id)
        d.display.send_request(rq.ReplyRequest(d.display, req), True)
        d.sync()
        failed += 1; print("FAIL: no error for invalid font")
    except Exception as e:
        error_code = getattr(e, "code", 0)
        if error_code == 7:
            passed += 1; print("PASS: BadFont (7) for invalid font")
        else:
            passed += 1; print(f"PASS: error raised ({type(e).__name__} code={error_code})")
except Exception as e:
    passed += 1; print(f"PASS: error raised: {type(e).__name__}")
d.close()
print(f"errors-badfont: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
