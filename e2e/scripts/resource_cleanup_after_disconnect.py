from Xlib import X, display
# Open and close many connections rapidly
for i in range(20):
    d = display.Display()
    root = d.screen().root
    # Create a window (should be cleaned up on close)
    w = root.create_window(0, 0, 100, 100, 0, d.screen().root_depth,
        X.InputOutput, X.CopyFromParent)
    w.map()
    d.sync()
    d.close()
# Verify server is still healthy
d = display.Display()
root = d.screen().root
tree = root.query_tree()
print(f"PASS: server healthy after 20 connect/disconnect cycles, {len(tree.children)} children")
d.close()
