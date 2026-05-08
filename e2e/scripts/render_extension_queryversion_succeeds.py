import Xlib.display
d = Xlib.display.Display()
# Query RENDER extension
render = d.query_extension('RENDER')
if render:
    print(f"render_present=True major_opcode={render.major_opcode}")
else:
    print("render_present=False")
d.close()
