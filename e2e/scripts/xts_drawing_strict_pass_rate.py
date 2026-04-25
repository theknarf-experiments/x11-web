import Xlib.display, Xlib.X, sys
d = Xlib.display.Display()
root = d.screen().root
pass_count = 0; fail_count = 0
w = root.create_window(0, 0, 200, 200, 0, d.screen().root_depth,
    event_mask=Xlib.X.ExposureMask)
w.map()
d.sync()
gc = w.create_gc(foreground=d.screen().white_pixel, background=d.screen().black_pixel,
    line_width=1, line_style=Xlib.X.LineSolid)
# PolyPoint
try:
    w.poly_point(gc, Xlib.X.CoordModeOrigin, [(10, 10), (20, 20)]); d.sync(); pass_count += 1
except: fail_count += 1
# PolyLine
try:
    w.poly_line(gc, Xlib.X.CoordModeOrigin, [(0, 0), (50, 50), (100, 0)]); d.sync(); pass_count += 1
except: fail_count += 1
# PolySegment
try:
    w.poly_segment(gc, [(0, 0, 100, 100), (100, 0, 0, 100)]); d.sync(); pass_count += 1
except: fail_count += 1
# PolyRectangle
try:
    w.poly_rectangle(gc, [(10, 10, 80, 80)]); d.sync(); pass_count += 1
except: fail_count += 1
# PolyArc
try:
    w.poly_arc(gc, [(10, 10, 80, 80, 0, 360*64)]); d.sync(); pass_count += 1
except: fail_count += 1
# FillPoly (convex)
try:
    w.fill_poly(gc, Xlib.X.Convex, Xlib.X.CoordModeOrigin,
        [(50, 10), (90, 90), (10, 90)]); d.sync(); pass_count += 1
except: fail_count += 1
# PolyFillRectangle
try:
    w.poly_fill_rectangle(gc, [(120, 10, 40, 40)]); d.sync(); pass_count += 1
except: fail_count += 1
# PolyFillArc
try:
    w.poly_fill_arc(gc, [(120, 60, 40, 40, 0, 360*64)]); d.sync(); pass_count += 1
except: fail_count += 1
# ClearArea
try:
    w.clear_area(0, 0, 50, 50); d.sync(); pass_count += 1
except: fail_count += 1
# CopyArea
try:
    w.copy_area(gc, w, 0, 0, 50, 50, 100, 100); d.sync(); pass_count += 1
except: fail_count += 1
# ImageText8
try:
    w.image_text(gc, 10, 150, "test"); d.sync(); pass_count += 1
except: fail_count += 1
# CreatePixmap + FreePixmap
try:
    pm = w.create_pixmap(100, 100, d.screen().root_depth); pm.free(); d.sync(); pass_count += 1
except: fail_count += 1
# GC operations (ChangeGC, CopyGC)
try:
    gc2 = w.create_gc(foreground=0xFF0000)
    gc2.change(line_width=3)
    d.sync(); pass_count += 1
    gc2.free()
except: fail_count += 1
# SetClipRectangles
try:
    gc3 = w.create_gc()
    gc3.set_clip_rectangles(0, 0, [(0, 0, 100, 100)], Xlib.X.Unsorted)
    d.sync(); pass_count += 1
    gc3.free()
except: fail_count += 1
gc.free()
w.destroy()
d.close()
print(f"xts-draw-strict: pass={pass_count} fail={fail_count}")
sys.exit(1 if fail_count > 0 else 0)
