#!/bin/sh
# Container entrypoint for the Wayland sidecar.
#
# Deliberately much smaller than the X11 sidecar's: no vkms modprobe (no
# GLX to software-render — the slice is wl_shm only) and no PulseAudio
# (no audio tests against this sidecar).
#
# All it has to guarantee is XDG_RUNTIME_DIR. debian:bookworm-slim ships
# no /run/user/0, and `ListeningSocket::bind` fails outright without it,
# which would kill the compositor before any client could connect. The
# 0700 mode is not cosmetic: libwayland's client side warns loudly about
# a group/world-accessible runtime dir and some toolkits refuse it.
#
# The library does this too (see `server::ensure_xdg_runtime_dir`);
# doing it here as well costs one mkdir and covers the case where the
# binary is replaced or run with a different entrypoint.

: "${XDG_RUNTIME_DIR:=/run/user/0}"
export XDG_RUNTIME_DIR
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

exec x11-web-sidecar-wayland "$@"
