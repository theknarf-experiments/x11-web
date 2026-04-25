import Xlib.display, sys
passed = 0; failed = 0
try:
    d = Xlib.display.Display()
    # QueryKeymap returns 32 bytes of key state
    # Check auto_repeats from GetKeyboardControl
    ctrl = d.get_keyboard_control()
    auto_repeats = ctrl.auto_repeats
    # Modifier keys should NOT auto-repeat
    # Keycode 37 = Ctrl_L, byte 4 bit 5
    ctrl_bit = (auto_repeats[37 // 8] >> (37 % 8)) & 1
    if ctrl_bit == 0:
        passed += 1; print("PASS: Ctrl_L (kc=37) does not auto-repeat")
    else:
        failed += 1; print("FAIL: Ctrl_L (kc=37) auto-repeats")
    # Regular key should auto-repeat
    # Keycode 38 = 'a' key
    a_bit = (auto_repeats[38 // 8] >> (38 % 8)) & 1
    if a_bit == 1:
        passed += 1; print("PASS: 'a' (kc=38) auto-repeats")
    else:
        failed += 1; print("FAIL: 'a' (kc=38) does not auto-repeat")
    d.close()
except Exception as e:
    failed += 1; print(f"FAIL: exception {e}")
print(f"per-key-repeat: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
