from Xlib import display, X
d = display.Display()
screen = d.screen()
root = screen.root
# Create GC with various attributes
gc = root.create_gc(
    foreground=screen.white_pixel,
    background=screen.black_pixel,
    line_width=2,
    line_style=X.LineSolid,
    cap_style=X.CapButt,
    join_style=X.JoinMiter,
    fill_style=X.FillSolid,
    function=X.GXcopy)
# Change some attributes
gc.change(foreground=0xFF0000, line_width=3)
d.sync()
# Create a window and draw
w = root.create_window(0, 0, 100, 100, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent, background_pixel=0)
w.map()
d.sync()
w.fill_rectangle(gc, 10, 10, 80, 80)
w.draw_line(gc, 0, 0, 100, 100)
w.draw_rectangle(gc, 5, 5, 90, 90)
d.sync()
gc.free()
w.destroy()
d.sync()
print("PASS: GC create/change/draw/free cycle completed")
d.close()
