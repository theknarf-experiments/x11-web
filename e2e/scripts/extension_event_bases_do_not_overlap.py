import Xlib.display
d = Xlib.display.Display()
# Extensions with events and the number of event codes their CLIENT LIBRARY
# reserves. That is the number that matters: libXext's XextAddDisplay installs
# a wire_to_event hook into dpy->event_vec[first_event + i] for i in
# 0..nevents, using the library's own compiled-in constant, not anything the
# server says. GLX reserves 17 (__GLX_NUMBER_EVENTS, GL/glxproto.h) and
# XInputExtension reserves 17 (IEVENTS, X11/extensions/XIproto.h) even though
# we emit far fewer, so their bases need 17 codes of clearance.
ext_events = {
    'SHAPE': 1,
    'MIT-SHM': 1,
    'GLX': 17,
    'SYNC': 1,
    'XKEYBOARD': 1,
    'XFIXES': 2,
    'RANDR': 2,
    'DAMAGE': 1,
    'MIT-SCREEN-SAVER': 1,
    'SECURITY': 1,
    'XVideo': 2,
    'XInputExtension': 17,
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
