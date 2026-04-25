import sys
passed = 0; failed = 0
try:
    import subprocess
    out = subprocess.check_output(["xset", "q"], env={"DISPLAY": ":99"}).decode()
    # xset q reports auto repeat delay and rate
    if "auto repeat delay" in out:
        passed += 1; print("PASS: xset reports auto repeat settings")
    else:
        failed += 1; print("FAIL: xset does not report auto repeat")
    # Check xkbcomp can read the keyboard map
    xkb_out = subprocess.check_output(
        ["xkbcomp", ":99", "-"],
        env={"DISPLAY": ":99"},
        stderr=subprocess.DEVNULL
    ).decode()
    if "repeat" in xkb_out.lower():
        passed += 1; print("PASS: xkbcomp includes repeat key definitions")
    else:
        failed += 1; print("FAIL: xkbcomp missing repeat definitions")
except Exception as e:
    failed += 1; print(f"FAIL: exception {e}")
print(f"key-repeat: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
