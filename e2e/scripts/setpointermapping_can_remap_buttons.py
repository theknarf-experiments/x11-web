import Xlib.display
d = Xlib.display.Display()

# Get current mapping
original = d.get_pointer_mapping()
n = len(original)

# Swap button 1 and 3 (left-hand mouse)
new_mapping = list(original)
if n >= 3:
    new_mapping[0] = 3
    new_mapping[2] = 1
    d.set_pointer_mapping(new_mapping)
    d.sync()

    # Read it back
    result = d.get_pointer_mapping()
    if result[0] == 3 and result[2] == 1:
        print("REMAP_OK")
    else:
        print(f"REMAP_FAIL: {list(result)}")

    # Restore original mapping
    d.set_pointer_mapping(list(original))
    d.sync()
else:
    print("REMAP_SKIP: not enough buttons")
