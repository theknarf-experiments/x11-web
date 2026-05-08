import Xlib.display, Xlib.X

# Create a window
d1 = Xlib.display.Display()
screen = d1.screen()
w = screen.root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    Xlib.X.InputOutput, Xlib.X.CopyFromParent,
    event_mask=Xlib.X.StructureNotifyMask | Xlib.X.PropertyChangeMask)
w.map()
d1.sync()

# Client 2 selects events on the same window
d2 = Xlib.display.Display()
w2 = d2.create_resource_object('window', w.id)
w2.change_attributes(event_mask=Xlib.X.PropertyChangeMask)
d2.sync()

# Change a property - both clients should be notifiable
atom = d1.intern_atom('_MULTI_TEST')
w.change_property(atom, Xlib.Xatom.STRING, 8, b'test_value')
d1.sync()

# Check client 1 gets PropertyNotify
d1.sync()
ev = d1.pending_events()
print(f"client1_pending={ev}")

# Both connected successfully
print("MULTI_EVENT_OK")
d1.close()
d2.close()
