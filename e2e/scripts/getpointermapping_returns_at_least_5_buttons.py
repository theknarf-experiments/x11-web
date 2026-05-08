import Xlib.display
d = Xlib.display.Display()
mapping = d.get_pointer_mapping()
n = len(mapping)
if n >= 5:
    print(f"MAPPING_COUNT_OK:{n}")
else:
    print(f"MAPPING_COUNT_FAIL:{n}")

# Verify identity mapping
all_identity = all(mapping[i] == i + 1 for i in range(min(n, 7)))
if all_identity:
    print("MAPPING_IDENTITY_OK")
else:
    print(f"MAPPING_IDENTITY_FAIL:{list(mapping)}")
