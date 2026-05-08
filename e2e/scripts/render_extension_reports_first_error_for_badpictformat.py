import Xlib.display
d = Xlib.display.Display()
info = d.query_extension('RENDER')
if info:
    print(f"present={info.major_opcode > 0}")
    print(f"first_error={info.first_error}")
    # RENDER first_error should be non-zero (142 = BadPictFormat)
    print(f"has_error_base={info.first_error > 0}")
else:
    print("present=False")
d.close()
