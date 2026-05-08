from Xlib import X, display

d = display.Display()
screen = d.screen()
created = 0
for i in range(500):
    pm = screen.root.create_pixmap(64, 64, screen.root_depth)
    created += 1
    pm.free()
d.sync()
# Verify we can still create pixmaps
final = screen.root.create_pixmap(64, 64, screen.root_depth)
print(f"created={created} final_pid={final.id}")
final.free()
d.close()
