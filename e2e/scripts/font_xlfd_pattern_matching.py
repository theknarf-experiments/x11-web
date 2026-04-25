import Xlib.display
d = Xlib.display.Display()
passed = 0
# Wildcard pattern should return results
fonts = d.list_fonts('*', 100)
if len(fonts) > 0: passed += 1
# Fixed font should be available
fonts2 = d.list_fonts('fixed', 10)
if len(fonts2) > 0: passed += 1
# Full XLFD wildcard pattern
fonts3 = d.list_fonts('-*-*-*-*-*-*-*-*-*-*-*-*-*-*', 100)
if len(fonts3) > 0: passed += 1
# Specific XLFD pattern
fonts4 = d.list_fonts('-misc-fixed-*-*-*-*-13-*-*-*-*-*-*-*', 10)
if len(fonts4) > 0: passed += 1
d.close()
print(f'xlfd-match: passed={passed}')
