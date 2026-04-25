from Xlib import display
d = display.Display()
# Create a custom atom
atom = d.intern_atom("_X11WEB_TEST_ATOM")
assert atom > 0, f"InternAtom failed: {atom}"
# Get its name back
name = d.get_atom_name(atom)
assert name == "_X11WEB_TEST_ATOM", f"GetAtomName mismatch: {name}"
# Verify idempotent
atom2 = d.intern_atom("_X11WEB_TEST_ATOM")
assert atom == atom2, f"InternAtom not idempotent: {atom} != {atom2}"
# Verify only_if_exists=True for non-existent atom
missing = d.intern_atom("_X11WEB_NONEXISTENT_ATOM_12345", only_if_exists=True)
assert missing == 0, f"only_if_exists should return None/0: {missing}"
print(f"PASS: atom round-trip verified (atom={atom})")
d.close()
