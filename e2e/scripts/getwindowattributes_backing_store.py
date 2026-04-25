from Xlib import X, display, Xatom
d = display.Display(":99")
root = d.screen().root
# Create window with backing_store=WhenMapped
w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
    backing_store=X.WhenMapped, save_under=True)
w.map()
d.sync()
attrs = w.get_attributes()
assert attrs.backing_store == X.WhenMapped, f"Expected WhenMapped, got {attrs.backing_store}"
assert attrs.save_under == True, f"Expected save_under=True, got {attrs.save_under}"
w.destroy()
d.close()
print("backing-store-test-pass")
