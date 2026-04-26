import Xlib.display, Xlib.X, Xlib.Xatom, sys
d = Xlib.display.Display()
root = d.screen().root
pass_count = 0; fail_count = 0
# InternAtom
try:
    a = d.intern_atom("XTS_STRICT_ATOM"); assert a > 0; pass_count += 1
except: fail_count += 1
# InternAtom only_if_exists=True
try:
    a2 = d.intern_atom("XTS_STRICT_ATOM", True); assert a2 == a; pass_count += 1
except: fail_count += 1
# GetAtomName
try:
    name = d.get_atom_name(a); assert name == "XTS_STRICT_ATOM"; pass_count += 1
except: fail_count += 1
# ChangeProperty + GetProperty (STRING)
try:
    root.change_property(a, Xlib.Xatom.STRING, 8, b"hello")
    d.sync()
    p = root.get_full_property(a, Xlib.Xatom.STRING)
    assert p is not None and p.value == b"hello"; pass_count += 1
except Exception as e: fail_count += 1; print(f"FAIL prop: {e}")
# ChangeProperty Replace mode
try:
    root.change_property(a, Xlib.Xatom.STRING, 8, b"world")
    d.sync()
    p = root.get_full_property(a, Xlib.Xatom.STRING)
    assert p is not None and p.value == b"world"; pass_count += 1
except Exception as e: fail_count += 1; print(f"FAIL replace: {e}")
# DeleteProperty
try:
    root.delete_property(a)
    d.sync()
    p = root.get_full_property(a, Xlib.Xatom.STRING)
    assert p is None; pass_count += 1
except Exception as e: fail_count += 1; print(f"FAIL delete: {e}")
# ListProperties
try:
    props = root.list_properties(); assert isinstance(props, (list, tuple)); pass_count += 1
except Exception as e: fail_count += 1; print(f"FAIL list: {e}")
# CARDINAL property (32-bit)
try:
    ca = d.intern_atom("XTS_CARDINAL")
    root.change_property(ca, Xlib.Xatom.CARDINAL, 32, [42, 100])
    d.sync()
    p = root.get_full_property(ca, Xlib.Xatom.CARDINAL)
    assert p is not None and len(p.value) >= 2; pass_count += 1
    root.delete_property(ca)
except Exception as e: fail_count += 1; print(f"FAIL cardinal: {e}")
# Selection owner — get_selection_owner returns a Window object (not
# a raw XID); compare via .id.
try:
    sel = d.intern_atom("XTS_SELECTION")
    w = root.create_window(0, 0, 1, 1, 0, d.screen().root_depth)
    w.set_selection_owner(sel, Xlib.X.CurrentTime)
    d.sync()
    owner = d.get_selection_owner(sel)
    owner_id = owner.id if hasattr(owner, "id") else owner
    assert owner_id == w.id, f"owner_id={owner_id:#x} expected {w.id:#x}"
    pass_count += 1
    w.destroy()
except Exception as e: fail_count += 1; print(f"FAIL selection: {e}")
d.close()
print(f"xts-prop-strict: pass={pass_count} fail={fail_count}")
sys.exit(1 if fail_count > 0 else 0)
