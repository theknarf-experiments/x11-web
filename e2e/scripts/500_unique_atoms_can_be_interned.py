from Xlib import display
d = display.Display()
atoms = []
for i in range(500):
    a = d.intern_atom(f"_TEST_ATOM_{i}")
    atoms.append(a)
d.sync()
# Verify all are unique
unique = set(atoms)
print(f"total={len(atoms)} unique={len(unique)}")
# Verify we can look them back up
name = d.get_atom_name(atoms[0])
print(f"first_name={name}")
d.close()
