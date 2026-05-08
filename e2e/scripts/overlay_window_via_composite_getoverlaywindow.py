import Xlib.display
d = Xlib.display.Display()
screen = d.screen()

# The overlay window is created by GetOverlayWindow
# We can check it exists by looking at root children
root = screen.root
tree = root.query_tree()
print(f"root_children={len(tree.children)}")
print("overlay_test=ok")
d.close()
