from Xlib import display

# Open and close connections sequentially
count = 0
for i in range(10):
    d = display.Display()
    d.close()
    count += 1

# Verify server is still accepting connections
final = display.Display()
info = final.get_display_name()
print(f"count={count} final_ok=True display={info}")
final.close()
