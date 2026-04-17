#!/bin/sh
# Container entrypoint: set up virtual DRM device for Mesa software rendering,
# then start the sidecar.
#
# Mesa's software (swrast/llvmpipe) DRI driver needs a DRM device node to
# initialize, even when doing pure CPU rendering.  In privileged containers the
# vkms (Virtual Kernel Mode Setting) module provides a /dev/dri device without
# requiring real GPU hardware.

# Try to load vkms for a virtual /dev/dri/renderD128 (best-effort).
modprobe vkms 2>/dev/null || true

exec x11-web-sidecar "$@"
