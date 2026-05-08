import Xlib.display, Xlib.X, Xlib.Xatom
d = Xlib.display.Display()
screen = d.screen()

w = screen.root.create_window(0, 0, 10, 10, 0, screen.root_depth)
d.sync()

test_atom = d.intern_atom('_STRESS_TEST_PROP')
errors = 0

for i in range(1000):
    try:
        value = f"value_{i}".encode()
        w.change_property(test_atom, Xlib.Xatom.STRING, 8, value)
    except Exception:
        errors += 1

d.sync()

# Read final value
prop = w.get_full_property(test_atom, Xlib.Xatom.STRING)
final_value = prop.value.decode() if prop else "NONE"

print(f"errors={errors}")
print(f"final_value={final_value}")
print(f"result={'OK' if final_value == 'value_999' and errors == 0 else 'FAIL'}")

w.destroy()
d.close()
