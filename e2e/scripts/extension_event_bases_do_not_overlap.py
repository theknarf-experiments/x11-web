import Xlib.display
d = Xlib.display.Display()
# Extensions with events and their expected event counts
ext_events = {
    'SHAPE': 1,
    'MIT-SHM': 1,
    'SYNC': 1,
    'XKEYBOARD': 1,
    'XFIXES': 2,
    'RANDR': 2,
    'DAMAGE': 1,
    'SECURITY': 1,
    'XVideo': 2,
}
ranges = []
for name, count in ext_events.items():
    info = d.query_extension(name)
    if info and info.first_event > 0:
        base = info.first_event
        ranges.append((base, base + count, name))

# Check for overlaps
overlaps = []
for i in range(len(ranges)):
    for j in range(i + 1, len(ranges)):
        a_start, a_end, a_name = ranges[i]
        b_start, b_end, b_name = ranges[j]
        if a_start < b_end and b_start < a_end:
            overlaps.append(f"{a_name}({a_start}-{a_end-1}) overlaps {b_name}({b_start}-{b_end-1})")

if overlaps:
    print(f"OVERLAP: {'; '.join(overlaps)}")
else:
    print(f"OK: {len(ranges)} event ranges are non-overlapping")
d.close()
