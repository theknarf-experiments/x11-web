import ctypes, ctypes.util
x11 = ctypes.CDLL(ctypes.util.find_library('X11'))
xcomposite = ctypes.CDLL(ctypes.util.find_library('Xcomposite'))
x11.XOpenDisplay.restype = ctypes.c_void_p
d = x11.XOpenDisplay(b':99')
assert d, 'Failed to open display'
# QueryVersion
major = ctypes.c_int(0)
minor = ctypes.c_int(0)
xcomposite.XCompositeQueryVersion(d, ctypes.byref(major), ctypes.byref(minor))
print(f'Composite version={major.value}.{minor.value}')
assert major.value >= 0, 'Bad version'
# GetOverlayWindow
xcomposite.XCompositeGetOverlayWindow.restype = ctypes.c_ulong
x11.XDefaultRootWindow.restype = ctypes.c_ulong
root = x11.XDefaultRootWindow(d)
overlay = xcomposite.XCompositeGetOverlayWindow(d, root)
print(f'overlay-window={overlay:#x}')
assert overlay != 0, 'GetOverlayWindow returned 0'
# ReleaseOverlayWindow
xcomposite.XCompositeReleaseOverlayWindow(d, root)
print('PASS: Composite overlay get/release succeeded')
x11.XCloseDisplay(d)
