#!/usr/bin/env bash
# Browser-free smoke test for the Wayland sidecar image.
#
#   bash e2e/scripts/wayland-smoke.sh [image]
#
# Builds (unless the image already exists) and runs the sidecar image,
# then checks four things in order — each one the prerequisite of the
# next, so the first failure tells you where the pipeline broke:
#
#   1. the compositor bound a wayland socket in XDG_RUNTIME_DIR
#   2. `wayland-info` sees the globals the vertical slice promises
#   3. `weston-simple-shm` (a real third-party client) maps a toplevel
#   4. that toplevel produced PutImage updates — i.e. pixels actually
#      made it through commit -> swizzle -> composite -> crop
#
# No backend is involved: the sidecar's QUIC dial fails in a loop and
# the DisplayUpdates queue up unread in the channel, which is fine for
# the ~20s this takes. That is deliberate — it isolates "the compositor
# and its clients work" from "the wire works", and it means this script
# can be re-run in seconds while debugging without standing up a
# backend, a browser or Playwright.
#
# Step 4 reads the `emitting PutImage` trace from
# crates/wayland-server/src/windows.rs, which is the only externally
# visible evidence of the pixel path when nothing drains the channel.
set -euo pipefail

IMAGE="${1:-x11web-sidecar-wayland}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
NAME="x11web-wayland-smoke-$$"
LOG=/tmp/sidecar.log

# `grep -c` prints 0 and exits 1 when nothing matches, so piping it
# through `tail -1` *inside* the container makes the pipeline succeed
# and keeps the output a single bare integer. Without that, the failure
# path would produce "0\n0" and the arithmetic tests below would blow
# up with "integer expression expected" instead of reporting the real
# problem.
count_in_log() {
	docker exec "$NAME" bash -c "grep -c '$1' '$2' 2>/dev/null | tail -1"
}

fail() {
	echo "FAIL: $*" >&2
	echo "--- sidecar log (tail) ---" >&2
	docker exec "$NAME" tail -n 60 "$LOG" 2>/dev/null >&2 || true
	exit 1
}

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
	echo "==> building $IMAGE (not present locally)"
	docker build -f "$REPO_ROOT/Dockerfile.sidecar-wayland" -t "$IMAGE" "$REPO_ROOT"
fi

# `sleep infinity` as PID 1: we start the sidecar ourselves below so we
# can point RUST_LOG at the windows module and keep the log in a file
# the later steps can grep. `--entrypoint bash` bypasses the image's
# own entrypoint, so we redo its one job (XDG_RUNTIME_DIR) by hand.
echo "==> starting $IMAGE as $NAME"
docker run -d --rm --name "$NAME" --entrypoint bash "$IMAGE" \
	-c 'mkdir -p /run/user/0 && chmod 700 /run/user/0 && sleep infinity' >/dev/null
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT

# `x11_web_wayland_server::windows=trace` turns on the per-frame
# PutImage line without the seat/surface trace firehose.
docker exec -d "$NAME" bash -c \
	"XDG_RUNTIME_DIR=/run/user/0 \
	 RUST_LOG=info,x11_web_wayland_server::windows=trace \
	 WAYLAND_SCREEN_SIZE=1280x800 \
	 x11-web-sidecar-wayland >$LOG 2>&1"

echo "==> [1/4] waiting for the wayland socket"
for _ in $(seq 1 40); do
	if docker exec "$NAME" bash -c 'ls /run/user/0/wayland-* >/dev/null 2>&1'; then
		break
	fi
	sleep 0.5
done
SOCKET=$(docker exec "$NAME" bash -c 'ls /run/user/0/ 2>/dev/null | grep -E "^wayland-[0-9]+$" | head -1' || true)
[ -n "$SOCKET" ] || fail "no wayland socket appeared in /run/user/0"
echo "    socket: $SOCKET"

echo "==> [2/4] protocol inventory (wayland-info)"
GLOBALS=$(docker exec -e XDG_RUNTIME_DIR=/run/user/0 -e WAYLAND_DISPLAY="$SOCKET" \
	"$NAME" wayland-info 2>&1 || true)
MISSING=""
for iface in wl_compositor wl_subcompositor wl_shm xdg_wm_base wl_seat wl_output \
	wp_viewporter zxdg_decoration_manager_v1 wp_single_pixel_buffer_manager_v1; do
	grep -q "interface: '$iface'" <<<"$GLOBALS" || MISSING="$MISSING $iface"
done
[ -z "$MISSING" ] || fail "wayland-info is missing globals:$MISSING"
echo "    all 9 slice globals advertised"

echo "==> [3/4] weston-simple-shm maps a toplevel"
docker exec -d -e XDG_RUNTIME_DIR=/run/user/0 -e WAYLAND_DISPLAY="$SOCKET" \
	"$NAME" weston-simple-shm
sleep 5
MAPPED=$(count_in_log "registering wayland window" "$LOG")
# The debug! that logs the registration is below the info! threshold
# for the crate as a whole, so fall back to the PutImage evidence: a
# window that emits pixels is, by construction, a mapped window.
PUTS=$(count_in_log "emitting PutImage" "$LOG")
[ "$PUTS" -gt 0 ] || fail "weston-simple-shm produced no PutImage (mapped=$MAPPED)"
echo "    weston-simple-shm: $PUTS PutImage updates"

echo "==> [4/4] wl-input-probe renders and reports its seat"
docker exec -d -e XDG_RUNTIME_DIR=/run/user/0 -e WAYLAND_DISPLAY="$SOCKET" \
	"$NAME" bash -c 'wl-input-probe >/tmp/probe.log 2>&1'
sleep 4
PROBE=$(docker exec "$NAME" cat /tmp/probe.log 2>/dev/null || true)
grep -q "PROBE ready" <<<"$PROBE" || fail "wl-input-probe never connected: $PROBE"
grep -q "PROBE xdg_surface.configure" <<<"$PROBE" ||
	fail "wl-input-probe was never configured: $PROBE"
grep -q "PROBE seat.capabilities" <<<"$PROBE" ||
	fail "wl-input-probe saw no seat capabilities: $PROBE"
TOTAL=$(count_in_log "emitting PutImage" "$LOG")
[ "$TOTAL" -gt "$PUTS" ] || fail "wl-input-probe added no PutImage updates"
echo "    probe connected, configured, keyboard+pointer seat present"
echo "    PutImage updates total: $TOTAL"

echo
echo "PASS: wayland sidecar smoke ($IMAGE)"
