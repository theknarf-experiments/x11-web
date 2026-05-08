import Xlib.display
d = Xlib.display.Display()
screen = d.screen()
result = screen.root.query_pointer()
print(f"root_x={result.root_x}")
print(f"root_y={result.root_y}")
print(f"same_screen={result.same_screen}")
# same_screen comes back as int 1/0 — coerce to bool so the test
# assertion ("pointer_ok=True") matches.
print(f"pointer_ok={bool(result.same_screen)}")
d.close()
