from Xlib import X, display

d = display.Display()
screen = d.screen()
root = screen.root
created = 0
for i in range(500):
    w = root.create_window(0, 0, 10, 10, 0, screen.root_depth,
        X.InputOutput, X.CopyFromParent)
    created += 1
    w.destroy()
d.sync()
# Verify we can still create windows after mass create/destroy
final = root.create_window(0, 0, 10, 10, 0, screen.root_depth,
    X.InputOutput, X.CopyFromParent)
print(f"created={created} final_wid={final.id}")
final.destroy()
d.close()
