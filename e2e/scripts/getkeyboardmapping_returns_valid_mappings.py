import Xlib.display

d = Xlib.display.Display()
# Get keyboard mapping for a range of keycodes
mapping = d.get_keyboard_mapping(8, 248)
print(f"mapping_entries={len(mapping)}")
if len(mapping) > 0:
    print("KEYMAP_OK")

d.close()
