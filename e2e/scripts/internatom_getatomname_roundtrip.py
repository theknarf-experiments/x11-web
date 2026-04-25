import Xlib.display
d = Xlib.display.Display(":99")
# Test predefined atoms
name = d.get_atom_name(1)
assert name == "PRIMARY", f"Atom 1 should be PRIMARY, got {name}"
# Test custom atom round-trip
atom_id = d.intern_atom("_TEST_CUSTOM_ATOM", False)
assert atom_id > 0, f"InternAtom failed: {atom_id}"
name2 = d.get_atom_name(atom_id)
assert name2 == "_TEST_CUSTOM_ATOM", f"GetAtomName mismatch: {name2}"
# Test only_if_exists=True for non-existent atom
atom_none = d.intern_atom("_NONEXISTENT_ATOM_12345", True)
assert atom_none == 0, f"Expected 0 for non-existent atom, got {atom_none}"
print("ATOM_ROUNDTRIP_OK")
d.close()
