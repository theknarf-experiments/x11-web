from Xlib import display
d = display.Display()
d.grab_server()
# Should be able to perform operations while grabbed
screen = d.screen()
root = screen.root
geo = root.get_geometry()
print(f"root_width={geo.width}")
d.ungrab_server()
d.sync()
print("grab_ungrab_ok=True")
d.close()
