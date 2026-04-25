import ctypes, os
os.environ["DISPLAY"] = ":99"
try:
    sdl2 = ctypes.cdll.LoadLibrary("libSDL2-2.0.so.0")
    ret = sdl2.SDL_Init(0x20)  # SDL_INIT_VIDEO
    if ret == 0:
        print("SDL2_INIT_OK")
        sdl2.SDL_Quit()
    else:
        err_fn = sdl2.SDL_GetError
        err_fn.restype = ctypes.c_char_p
        err = err_fn()
        print(f"SDL2_INIT_FAILED: {err}")
        sdl2.SDL_Quit()
except Exception as e:
    print(f"SDL2_NOT_AVAILABLE: {e}")
