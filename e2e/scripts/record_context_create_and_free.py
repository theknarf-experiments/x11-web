import Xlib.display
d = Xlib.display.Display()
record_ext = d.query_extension('RECORD')
if record_ext:
    print(f"record_present=true")
    print(f"record_opcode={record_ext.major_opcode}")
else:
    print("record_present=false")
d.close()
