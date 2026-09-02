/*
 * wl-input-probe — a deterministic Wayland input probe.
 *
 * The Wayland analogue of the X11 sidecar's `input-probe.html`: a
 * surface whose colour can ONLY change as a result of real input
 * arriving over the wire. That makes an exact-colour pixel count a
 * sound end-to-end assertion for "the browser's click/keystroke
 * reached the client", immune to the animations, blinking carets and
 * repaint timing that make screenshot diffs and pixel hashes lie.
 *
 *   start          → white   (255,255,255)
 *   pointer button → magenta (255,  0,255)
 *   key `g`        → green   (  0,204,  0)
 *
 * The three colours are the same ones the Firefox probe page uses, so
 * the two suites' assertions read alike.
 *
 * Every event is also printed to stdout as one `EV ...` line. The
 * sidecar drains a spawned child's stdout into its own tracing log, so
 * when the pixels don't move the log still says whether the event
 * reached the client — that is the fallback evidence path for a
 * headless CI run.
 *
 * Deliberately minimal: wl_compositor + wl_shm + xdg_shell + wl_seat,
 * one 400x300 XRGB8888 buffer, no toolkit. It binds nothing this
 * compositor doesn't advertise (notably not wl_data_device_manager,
 * which is what makes weston's toytoolkit demos die here).
 *
 * Built into the sidecar image by Dockerfile.sidecar-wayland's
 * `clients` stage; not part of the Rust build.
 */
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>

#include "xdg-shell-client-protocol.h"

#define W 400
#define H 300

/* 0xAARRGGBB — wl_shm's XRGB8888 is a little-endian 32-bit word, so a
 * literal like this is written straight into the pool. */
#define COLOR_IDLE 0xffffffffu
#define COLOR_CLICK 0xffff00ffu
#define COLOR_KEY 0xff00cc00u

static struct wl_compositor *comp;
static struct wl_shm *shm;
static struct xdg_wm_base *wm_base;
static struct wl_surface *surf;
static struct wl_buffer *buf;
static uint32_t *pixels;
static struct xkb_context *xkb_ctx;
static struct xkb_keymap *keymap;
static struct xkb_state *xkb_st;
static int configured;

#define OUT(...)                                                               \
	do {                                                                         \
		printf("PROBE " __VA_ARGS__);                                              \
		fflush(stdout);                                                            \
	} while (0)

/* Repaint the whole surface in one colour. The compositor copies the
 * pool on commit and releases the buffer immediately, so reusing the
 * single buffer without waiting for wl_buffer.release is safe here —
 * and keeps the probe free of a release-tracking state machine that
 * could itself stall and be mistaken for "input never arrived". */
static void fill(uint32_t argb) {
	for (int i = 0; i < W * H; i++)
		pixels[i] = argb;
	if (!configured)
		return;
	wl_surface_attach(surf, buf, 0, 0);
	wl_surface_damage(surf, 0, 0, W, H);
	wl_surface_commit(surf);
}

/* ---- pointer ---- */
static void p_enter(void *d, struct wl_pointer *p, uint32_t s,
                    struct wl_surface *sf, wl_fixed_t x, wl_fixed_t y) {
	OUT("pointer.enter x=%.1f y=%.1f\n", wl_fixed_to_double(x),
	    wl_fixed_to_double(y));
}
static void p_leave(void *d, struct wl_pointer *p, uint32_t s,
                    struct wl_surface *sf) {
	OUT("pointer.leave\n");
}
static void p_motion(void *d, struct wl_pointer *p, uint32_t t, wl_fixed_t x,
                     wl_fixed_t y) {
	OUT("pointer.motion x=%.1f y=%.1f\n", wl_fixed_to_double(x),
	    wl_fixed_to_double(y));
}
static void p_button(void *d, struct wl_pointer *p, uint32_t s, uint32_t t,
                     uint32_t button, uint32_t state) {
	OUT("pointer.button code=0x%x state=%u\n", button, state);
	if (state == WL_POINTER_BUTTON_STATE_PRESSED)
		fill(COLOR_CLICK);
}
static void p_axis(void *d, struct wl_pointer *p, uint32_t t, uint32_t axis,
                   wl_fixed_t v) {
	OUT("pointer.axis axis=%u value=%.1f\n", axis, wl_fixed_to_double(v));
}
static void p_frame(void *d, struct wl_pointer *p) {}
static void p_axis_source(void *d, struct wl_pointer *p, uint32_t src) {}
static void p_axis_stop(void *d, struct wl_pointer *p, uint32_t t,
                        uint32_t axis) {}
static void p_axis_discrete(void *d, struct wl_pointer *p, uint32_t axis,
                            int32_t n) {}
static void p_axis_value120(void *d, struct wl_pointer *p, uint32_t axis,
                            int32_t v) {
	OUT("pointer.axis_value120 axis=%u v=%d\n", axis, v);
}
static const struct wl_pointer_listener p_listener = {
    .enter = p_enter,
    .leave = p_leave,
    .motion = p_motion,
    .button = p_button,
    .axis = p_axis,
    .frame = p_frame,
    .axis_source = p_axis_source,
    .axis_stop = p_axis_stop,
    .axis_discrete = p_axis_discrete,
    .axis_value120 = p_axis_value120,
};

/* ---- keyboard ---- */
static void k_keymap(void *d, struct wl_keyboard *k, uint32_t fmt, int32_t fd,
                     uint32_t size) {
	char *map = mmap(NULL, size, PROT_READ, MAP_PRIVATE, fd, 0);
	if (map == MAP_FAILED) {
		OUT("kb.keymap MMAP_FAILED\n");
		close(fd);
		return;
	}
	keymap = xkb_keymap_new_from_string(xkb_ctx, map, XKB_KEYMAP_FORMAT_TEXT_V1,
	                                    XKB_KEYMAP_COMPILE_NO_FLAGS);
	munmap(map, size);
	close(fd);
	if (xkb_st)
		xkb_state_unref(xkb_st);
	xkb_st = keymap ? xkb_state_new(keymap) : NULL;
	OUT("kb.keymap format=%u size=%u compiled=%d\n", fmt, size, keymap != NULL);
}
static void k_enter(void *d, struct wl_keyboard *k, uint32_t s,
                    struct wl_surface *sf, struct wl_array *keys) {
	OUT("kb.enter held=%zu\n", keys->size / sizeof(uint32_t));
}
static void k_leave(void *d, struct wl_keyboard *k, uint32_t s,
                    struct wl_surface *sf) {
	OUT("kb.leave\n");
}
static void k_key(void *d, struct wl_keyboard *k, uint32_t s, uint32_t t,
                  uint32_t key, uint32_t state) {
	char symname[64] = "?";
	xkb_keysym_t sym = XKB_KEY_NoSymbol;
	if (xkb_st) {
		/* wl_keyboard.key carries an evdev code; xkb wants evdev + 8. */
		sym = xkb_state_key_get_one_sym(xkb_st, key + 8);
		xkb_keysym_get_name(sym, symname, sizeof(symname));
	}
	OUT("kb.key evdev=%u state=%u sym=%s\n", key, state, symname);
	if (state == WL_KEYBOARD_KEY_STATE_PRESSED && sym == XKB_KEY_g)
		fill(COLOR_KEY);
}
static void k_modifiers(void *d, struct wl_keyboard *k, uint32_t s, uint32_t dep,
                        uint32_t lat, uint32_t lock, uint32_t group) {
	if (xkb_st)
		xkb_state_update_mask(xkb_st, dep, lat, lock, 0, 0, group);
	OUT("kb.modifiers depressed=0x%x latched=0x%x locked=0x%x\n", dep, lat, lock);
}
static void k_repeat(void *d, struct wl_keyboard *k, int32_t rate,
                     int32_t delay) {}
static const struct wl_keyboard_listener k_listener = {
    .keymap = k_keymap,
    .enter = k_enter,
    .leave = k_leave,
    .key = k_key,
    .modifiers = k_modifiers,
    .repeat_info = k_repeat,
};

/* ---- seat ---- */
static void seat_caps(void *d, struct wl_seat *s, uint32_t caps) {
	OUT("seat.capabilities 0x%x\n", caps);
	if (caps & WL_SEAT_CAPABILITY_POINTER)
		wl_pointer_add_listener(wl_seat_get_pointer(s), &p_listener, NULL);
	if (caps & WL_SEAT_CAPABILITY_KEYBOARD)
		wl_keyboard_add_listener(wl_seat_get_keyboard(s), &k_listener, NULL);
}
static void seat_name(void *d, struct wl_seat *s, const char *name) {}
static const struct wl_seat_listener seat_listener = {
    .capabilities = seat_caps,
    .name = seat_name,
};

/* ---- shell ---- */
static void ping(void *d, struct xdg_wm_base *b, uint32_t serial) {
	xdg_wm_base_pong(b, serial);
}
static const struct xdg_wm_base_listener wm_listener = {.ping = ping};

static void xs_configure(void *d, struct xdg_surface *xs, uint32_t serial) {
	xdg_surface_ack_configure(xs, serial);
	configured = 1;
	wl_surface_attach(surf, buf, 0, 0);
	wl_surface_damage(surf, 0, 0, W, H);
	wl_surface_commit(surf);
	OUT("xdg_surface.configure\n");
}
static const struct xdg_surface_listener xs_listener = {.configure =
                                                            xs_configure};

static void tl_configure(void *d, struct xdg_toplevel *t, int32_t w, int32_t h,
                         struct wl_array *states) {
	int activated = 0;
	uint32_t *st;
	for (st = states->data;
	     (const char *)st < ((const char *)states->data + states->size); st++)
		if (*st == XDG_TOPLEVEL_STATE_ACTIVATED)
			activated = 1;
	OUT("xdg_toplevel.configure %dx%d activated=%d\n", w, h, activated);
}
static void tl_close(void *d, struct xdg_toplevel *t) {
	OUT("xdg_toplevel.close\n");
	exit(0);
}
static const struct xdg_toplevel_listener tl_listener = {
    .configure = tl_configure,
    .close = tl_close,
};

/* ---- registry ---- */
static void reg_global(void *d, struct wl_registry *r, uint32_t name,
                       const char *iface, uint32_t ver) {
	if (!strcmp(iface, "wl_compositor"))
		comp = wl_registry_bind(r, name, &wl_compositor_interface, 4);
	else if (!strcmp(iface, "wl_shm"))
		shm = wl_registry_bind(r, name, &wl_shm_interface, 1);
	else if (!strcmp(iface, "xdg_wm_base")) {
		wm_base = wl_registry_bind(r, name, &xdg_wm_base_interface, 1);
		xdg_wm_base_add_listener(wm_base, &wm_listener, NULL);
	} else if (!strcmp(iface, "wl_seat")) {
		struct wl_seat *seat = wl_registry_bind(r, name, &wl_seat_interface, 5);
		wl_seat_add_listener(seat, &seat_listener, NULL);
	}
}
static void reg_remove(void *d, struct wl_registry *r, uint32_t name) {}
static const struct wl_registry_listener reg_listener = {reg_global, reg_remove};

int main(void) {
	struct wl_display *dpy = wl_display_connect(NULL);
	if (!dpy) {
		fprintf(stderr, "wl-input-probe: no wayland display\n");
		return 1;
	}
	xkb_ctx = xkb_context_new(XKB_CONTEXT_NO_FLAGS);

	struct wl_registry *reg = wl_display_get_registry(dpy);
	wl_registry_add_listener(reg, &reg_listener, NULL);
	wl_display_roundtrip(dpy);
	/* Second roundtrip: seat capabilities arrive as an event on the
	 * object bound during the first one. */
	wl_display_roundtrip(dpy);

	if (!comp || !shm || !wm_base) {
		fprintf(stderr, "wl-input-probe: missing globals "
		                "(compositor=%p shm=%p xdg_wm_base=%p)\n",
		        (void *)comp, (void *)shm, (void *)wm_base);
		return 1;
	}

	int stride = W * 4, size = stride * H;
	char tmpl[] = "/tmp/wl-input-probe-XXXXXX";
	int fd = mkstemp(tmpl);
	if (fd < 0)
		return 1;
	unlink(tmpl);
	if (ftruncate(fd, size) < 0)
		return 1;
	pixels = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	if (pixels == MAP_FAILED)
		return 1;
	for (int i = 0; i < W * H; i++)
		pixels[i] = COLOR_IDLE;
	struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, size);
	buf = wl_shm_pool_create_buffer(pool, 0, W, H, stride, WL_SHM_FORMAT_XRGB8888);
	wl_shm_pool_destroy(pool);

	surf = wl_compositor_create_surface(comp);
	struct xdg_surface *xs = xdg_wm_base_get_xdg_surface(wm_base, surf);
	xdg_surface_add_listener(xs, &xs_listener, NULL);
	struct xdg_toplevel *tl = xdg_surface_get_toplevel(xs);
	xdg_toplevel_add_listener(tl, &tl_listener, NULL);
	xdg_toplevel_set_title(tl, "wl-input-probe");
	xdg_toplevel_set_app_id(tl, "web.x11.wl-input-probe");
	wl_surface_commit(surf);

	OUT("ready\n");
	while (wl_display_dispatch(dpy) != -1) {
	}
	return 0;
}
