import Xlib.display, Xlib.X

d = Xlib.display.Display()
screen = d.screen()

# Query DAMAGE extension
dmg = d.query_extension('DAMAGE')
print(f"damage_present={dmg is not None and dmg.major_opcode > 0}")

# Query XFIXES for region support
xfixes = d.query_extension('XFIXES')
print(f"xfixes_present={xfixes is not None and xfixes.major_opcode > 0}")

d.close()
