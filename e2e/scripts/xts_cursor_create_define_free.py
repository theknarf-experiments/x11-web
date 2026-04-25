from Xlib import display, X, Xcursorfont
d = display.Display()
screen = d.screen()
root = screen.root
# Create cursor from font glyph
font = d.open_font("cursor")
cursor = font.create_glyph_cursor(
    font, Xcursorfont.left_ptr, Xcursorfont.left_ptr + 1,
    (0, 0, 0), (0xFFFF, 0xFFFF, 0xFFFF))
# Set cursor on a window
w = root.create_window(0, 0, 50, 50, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent, cursor=cursor)
w.map()
d.sync()
# Clean up
w.destroy()
cursor.free()
font.close()
d.sync()
print("PASS: cursor create/define/free cycle completed")
d.close()
