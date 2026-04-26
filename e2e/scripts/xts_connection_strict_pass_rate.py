import Xlib.display, Xlib.X, sys
d = Xlib.display.Display()
pass_count = 0; fail_count = 0
# python-xlib's connection-setup info is a DictWrapper backed by
# _data, with field names that differ from the Xlib C names
# (protocol_major not protocol_major_version, release_number not
# vendor_release, etc.).
info = d.display.info._data
# XOpenDisplay
try:
    assert d is not None; pass_count += 1
except: fail_count += 1
# XConnectionNumber
try:
    fd = d.fileno(); assert fd >= 0; pass_count += 1
except: fail_count += 1
# XDisplayString
try:
    ds = d.get_display_name(); assert ":99" in ds; pass_count += 1
except: fail_count += 1
# XDefaultScreen
try:
    s = d.get_default_screen(); assert s >= 0; pass_count += 1
except: fail_count += 1
# XScreenCount
try:
    assert d.screen_count() >= 1; pass_count += 1
except: fail_count += 1
# XProtocolVersion
try:
    assert info["protocol_major"] == 11; pass_count += 1
except: fail_count += 1
# XProtocolRevision
try:
    assert info["protocol_minor"] == 0; pass_count += 1
except: fail_count += 1
# XServerVendor
try:
    v = info["vendor"]; assert len(v) > 0; pass_count += 1
except: fail_count += 1
# XVendorRelease
try:
    r = info["release_number"]; assert r >= 0; pass_count += 1
except: fail_count += 1
# XImageByteOrder
try:
    bo = info["image_byte_order"]; assert bo in (0, 1); pass_count += 1
except: fail_count += 1
# XBitmapUnit
try:
    bu = info["bitmap_format_scanline_unit"]; assert bu in (8, 16, 32); pass_count += 1
except: fail_count += 1
# XBitmapBitOrder
try:
    bbo = info["bitmap_format_bit_order"]; assert bbo in (0, 1); pass_count += 1
except: fail_count += 1
# XBitmapPad
try:
    bp = info["bitmap_format_scanline_pad"]; assert bp in (8, 16, 32); pass_count += 1
except: fail_count += 1
# MaxRequestSize
try:
    mrl = info["max_request_length"]; assert mrl >= 4096; pass_count += 1
except: fail_count += 1
# Root depth check
try:
    root = d.screen().root; g = root.get_geometry(); assert g.depth >= 24; pass_count += 1
except: fail_count += 1
# Root visual
try:
    rv = d.screen().root_visual; assert rv > 0; pass_count += 1
except: fail_count += 1
# DefaultColormap — python-xlib wraps it as a Colormap object (not an
# int), so compare via .id.
try:
    cm = d.screen().default_colormap
    cm_id = cm.id if hasattr(cm, "id") else cm
    assert cm_id > 0; pass_count += 1
except: fail_count += 1
# WhitePixel / BlackPixel
try:
    wp = d.screen().white_pixel; bp = d.screen().black_pixel; assert wp != bp; pass_count += 1
except: fail_count += 1
d.close()
print(f"xts-conn-strict: pass={pass_count} fail={fail_count}")
sys.exit(1 if fail_count > 0 else 0)
