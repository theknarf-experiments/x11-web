from Xlib import X, display, Xatom
d = display.Display()
root = d.screen().root

# Check if TARGETS atom exists
targets_atom = d.intern_atom("TARGETS")
utf8_atom = d.intern_atom("UTF8_STRING")
string_atom = Xatom.STRING
print(f"targets_atom={targets_atom}")
print(f"utf8_atom={utf8_atom}")
print(f"string_atom={string_atom}")
# Atoms should be non-zero
assert targets_atom != 0
assert utf8_atom != 0
print("atoms_ok=True")
d.close()
