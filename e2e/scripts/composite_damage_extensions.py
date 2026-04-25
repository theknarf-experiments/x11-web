import Xlib.display
d = Xlib.display.Display(":99")
comp = d.query_extension("Composite")
assert comp is not None and comp.major_opcode > 0, "Composite not found"
damage = d.query_extension("DAMAGE")
assert damage is not None and damage.major_opcode > 0, "DAMAGE not found"
print(f"COMPOSITE_OPCODE={comp.major_opcode}")
print(f"DAMAGE_OPCODE={damage.major_opcode}")
print("COMP_DAMAGE_OK")
d.close()
