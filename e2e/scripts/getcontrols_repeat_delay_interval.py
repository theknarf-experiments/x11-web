import sys, os
passed = 0; failed = 0
# Inherit env so PATH/LD_* etc. survive — adding only DISPLAY would
# strip those and the binaries can't find their libraries.
env = {**os.environ, "DISPLAY": ":99"}
try:
    import subprocess
    out = subprocess.check_output(["xset", "q"], env=env).decode()
    # xset q reports auto repeat delay and rate
    if "auto repeat delay" in out:
        passed += 1; print("PASS: xset reports auto repeat settings")
    else:
        failed += 1; print("FAIL: xset does not report auto repeat")
    # Check xkbcomp can read the keyboard map
    proc = subprocess.run(
        ["xkbcomp", "-xkb", "-w", "10", ":99", "-"],
        env=env,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        failed += 1
        print(f"FAIL: xkbcomp exit={proc.returncode}")
        print(f"--- xkbcomp stderr ---\n{proc.stderr.strip()}\n--- end ---")
        print(f"--- xkbcomp stdout (last 500 chars) ---\n{proc.stdout[-500:]}\n--- end ---")
    elif "repeat" in proc.stdout.lower():
        passed += 1; print("PASS: xkbcomp includes repeat key definitions")
    else:
        failed += 1; print("FAIL: xkbcomp missing repeat definitions")
except Exception as e:
    failed += 1; print(f"FAIL: exception {e}")
print(f"key-repeat: pass={passed} fail={failed}")
sys.exit(1 if failed > 0 else 0)
