import Xlib.display

success = 0
for i in range(20):
    try:
        d = Xlib.display.Display()
        screen = d.screen()
        # Do a basic operation
        _ = screen.root.get_geometry()
        d.close()
        success += 1
    except Exception as e:
        print(f"Connection {i} failed: {e}")

print(f"success={success}/20")
if success == 20:
    print("STRESS_OK")
