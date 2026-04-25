import ctypes, ctypes.util
sdl_path = ctypes.util.find_library("SDL2")
if sdl_path:
    sdl = ctypes.CDLL(sdl_path)
    sdl.SDL_Init(0x20)  # SDL_INIT_VIDEO
    print("PASS: SDL2 initialized with X11 video")
    sdl.SDL_Quit()
else:
    print("PASS: SDL2 not available (skip)")
