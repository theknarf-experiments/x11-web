import ctypes, ctypes.util
x11 = ctypes.CDLL(ctypes.util.find_library('X11'))
xext = ctypes.CDLL(ctypes.util.find_library('Xext'))
x11.XOpenDisplay.restype = ctypes.c_void_p
d = x11.XOpenDisplay(b':99')
assert d, 'Failed to open display'
# Check SYNC extension is available
x11.XQueryExtension.restype = ctypes.c_int
x11.XQueryExtension.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int)]
major = ctypes.c_int(0)
fe = ctypes.c_int(0)
ferr = ctypes.c_int(0)
ret = x11.XQueryExtension(d, b'SYNC', ctypes.byref(major), ctypes.byref(fe), ctypes.byref(ferr))
print(f'SYNC present={ret} major_opcode={major.value}')
assert ret != 0, 'SYNC extension not present'
print('PASS: SYNC extension available')
x11.XCloseDisplay(d)
