import ctypes, ctypes.util, struct
x11 = ctypes.CDLL(ctypes.util.find_library('X11'))
x11.XOpenDisplay.restype = ctypes.c_void_p
d = x11.XOpenDisplay(b':99')
assert d, 'XOpenDisplay failed'
# Query the DRI3 extension
x11.XQueryExtension.restype = ctypes.c_int
x11.XQueryExtension.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int)]
major = ctypes.c_int(0)
first_event = ctypes.c_int(0)
first_error = ctypes.c_int(0)
ret = x11.XQueryExtension(d, b'DRI3', ctypes.byref(major), ctypes.byref(first_event), ctypes.byref(first_error))
print(f'DRI3 present={ret} major_opcode={major.value}')
assert ret != 0, 'DRI3 extension not present'
print('PASS: DRI3 extension available')
x11.XCloseDisplay(d)
