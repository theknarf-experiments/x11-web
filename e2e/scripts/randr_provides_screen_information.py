import Xlib.display
d = Xlib.display.Display()
randr = d.query_extension('RANDR')
print(f"randr_present={randr is not None and randr.major_opcode > 0}")
screen = d.screen()
print(f"width={screen.width_in_pixels}")
print(f"height={screen.height_in_pixels}")
d.close()
