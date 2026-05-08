from Xlib import X, display

d = display.Display()
screen = d.screen()
root = screen.root
created = 0
for i in range(500):
    gc = root.create_gc()
    created += 1
    gc.free()
d.sync()
# Verify we can still create GCs
final = root.create_gc()
print(f"created={created} gc_ok=True")
final.free()
d.close()
