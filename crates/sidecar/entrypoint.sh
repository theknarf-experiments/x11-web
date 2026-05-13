#!/bin/sh
# Container entrypoint: set up virtual DRM device for Mesa software rendering,
# start PulseAudio (if installed), then start the sidecar.
#
# Mesa's software (swrast/llvmpipe) DRI driver needs a DRM device node to
# initialize, even when doing pure CPU rendering.  In privileged containers the
# vkms (Virtual Kernel Mode Setting) module provides a /dev/dri device without
# requiring real GPU hardware.

# Try to load vkms for a virtual /dev/dri/renderD128 (best-effort).
modprobe vkms 2>/dev/null || true

# Start PulseAudio in system mode so audio.spec.ts can see pactl + virtual
# sinks. The Dockerfile installs the daemon and writes /etc/pulse/system.pa;
# we only need to launch it. Best-effort — if pulseaudio isn't installed
# the audio tests skip themselves.
if command -v pulseaudio >/dev/null 2>&1; then
    pulseaudio --system --daemonize --disallow-exit \
        --exit-idle-time=-1 --log-target=stderr 2>/dev/null || true
fi

exec x11-web-sidecar "$@"
