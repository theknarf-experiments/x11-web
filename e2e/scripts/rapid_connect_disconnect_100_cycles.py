import Xlib.display
passed = 0
for i in range(100):
    try:
        d = Xlib.display.Display()
        d.close()
        passed += 1
    except: pass
print(f'rapid-connect: passed={passed}')
