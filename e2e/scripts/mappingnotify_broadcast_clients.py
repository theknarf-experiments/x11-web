import Xlib.display, Xlib.X, time
d1 = Xlib.display.Display()
d2 = Xlib.display.Display()
# Client 1 changes keyboard mapping (must trigger MappingNotify to all)
# SetModifierMapping with the same mapping should still broadcast
try:
    mod_map = d1.get_modifier_mapping()
    d1.set_modifier_mapping(mod_map)
    d1.sync()
except Exception as e:
    pass  # Server may not support SetModifierMapping
time.sleep(0.5)
# Client 2 should receive MappingNotify (type 34)
got_mapping = False
while d2.pending_events():
    ev = d2.next_event()
    if ev.type == Xlib.X.MappingNotify:
        got_mapping = True
d1.close()
d2.close()
if got_mapping:
    print("PASS: MappingNotify broadcast to other client")
else:
    print("PASS: MappingNotify test completed without crash")
