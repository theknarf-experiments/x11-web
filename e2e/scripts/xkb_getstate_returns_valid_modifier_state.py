import Xlib.display
d = Xlib.display.Display()
xkb = d.query_extension('XKEYBOARD')
if xkb:
    # XKB UseExtension (minor 0)
    import struct
    # Send UseExtension request
    buf = struct.pack('=BBHBB', xkb.major_opcode, 0, 2, 1, 0)
    d.display.send_request(d.display.request_queue, buf, None)
    d.sync()

    # XKB GetState (minor 4)
    buf = struct.pack('=BBHHxx', xkb.major_opcode, 4, 2, 0x100)
    d.display.send_request(d.display.request_queue, buf, None)
    d.sync()

    print("xkb_state_query=ok")
else:
    print("xkb_state_query=no_extension")
d.close()
