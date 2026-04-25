import ctypes, sys
try:
    sdl = ctypes.CDLL("libSDL2-2.0.so.0")
    print("sdl2-loaded-ok")
except OSError:
    print("sdl2-not-available")
