import Xlib.display, Xlib.X
d = Xlib.display.Display()
screen = d.screen()

# QueryBestSize for Tile (class 1)
tile = d.query_best_size(1, screen.root, 100, 100)
print(f"tile_width={tile.width} tile_height={tile.height}")

# QueryBestSize for Stipple (class 2)
stip = d.query_best_size(2, screen.root, 100, 100)
print(f"stipple_width={stip.width} stipple_height={stip.height}")

d.close()
