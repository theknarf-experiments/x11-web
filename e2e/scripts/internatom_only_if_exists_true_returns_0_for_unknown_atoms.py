import Xlib.display
d = Xlib.display.Display()

# This atom should not exist
atom = d.intern_atom('_NONEXISTENT_ATOM_12345', True)
print(f"atom={atom}")
print(f"returns_zero={atom == 0}")

# Now intern it for real
real_atom = d.intern_atom('_NONEXISTENT_ATOM_12345', False)
print(f"real_atom_nonzero={real_atom != 0}")

# Now only_if_exists should find it
found_atom = d.intern_atom('_NONEXISTENT_ATOM_12345', True)
print(f"found_after_intern={found_atom == real_atom}")

d.close()
