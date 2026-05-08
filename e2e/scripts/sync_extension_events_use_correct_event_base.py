import Xlib.display
d = Xlib.display.Display()
info = d.query_extension('SYNC')
if info:
    print(f"present={info.major_opcode > 0}")
    print(f"major_opcode={info.major_opcode}")
    print(f"first_event={info.first_event}")
    # SYNC first_event must be 83 (AlarmNotify)
    print(f"event_base_correct={info.first_event == 83}")
else:
    print("present=False")
d.close()
