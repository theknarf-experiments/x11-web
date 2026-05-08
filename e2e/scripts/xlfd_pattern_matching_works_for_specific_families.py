import Xlib.display
d = Xlib.display.Display()
# Query with XLFD wildcard pattern
fonts = d.list_fonts('-*-fixed-*-*-*-*-*-*-*-*-*-*-*-*', 100)
print(f"xlfd_match_count={len(fonts)}")
has_xlfd = any(f.startswith('-') and 'fixed' in f for f in fonts)
print(f"has_xlfd_fixed={has_xlfd}")
d.close()
