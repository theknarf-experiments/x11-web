import Xlib.display

d = Xlib.display.Display()
mod = d.get_modifier_mapping()
print(f"modifier_groups={len(mod)}")
# Should have 8 modifier groups (Shift, Lock, Control, Mod1-5)
if len(mod) == 8:
    print("MODMAP_OK")

d.close()
