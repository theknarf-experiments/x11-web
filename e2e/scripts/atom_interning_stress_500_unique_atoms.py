import Xlib.display
d = Xlib.display.Display()

atoms = {}
errors = 0

for i in range(500):
    name = f"_STRESS_ATOM_{i}"
    try:
        atom_id = d.intern_atom(name)
        atoms[name] = atom_id
    except Exception:
        errors += 1

# Verify round-trip: get name back from ID
roundtrip_ok = 0
for name, atom_id in list(atoms.items())[:50]:
    try:
        got_name = d.get_atom_name(atom_id)
        if got_name == name:
            roundtrip_ok += 1
    except Exception:
        pass

print(f"interned={len(atoms)}")
print(f"errors={errors}")
print(f"roundtrip_ok={roundtrip_ok}")
print(f"result={'OK' if len(atoms) == 500 and errors == 0 and roundtrip_ok == 50 else 'FAIL'}")

d.close()
