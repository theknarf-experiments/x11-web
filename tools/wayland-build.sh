#!/usr/bin/env bash
# Run cargo for the Linux-only crates (crates/wayland-server,
# crates/sidecar-wayland) inside a Linux container.
#
#   bash tools/wayland-build.sh check -p x11-web-wayland-server
#   bash tools/wayland-build.sh build --release --bin x11-web-sidecar-wayland
#   bash tools/wayland-build.sh test -p x11-web-wayland-server
#
# Everything after the script name is passed straight through to
# cargo. NEVER cargo-build smithay on the macOS host: those crates are
# gated behind `cfg(target_os = "linux")` precisely so the host build
# stays green without a Linux toolchain.
#
# Design notes:
#
#  * The repo is BIND-MOUNTED at /app rather than COPYed, so an edit
#    on the host is visible to the next invocation with no image
#    rebuild. That means the container writes into the host tree —
#    which is why the cargo target dir is redirected (below).
#
#  * CARGO_TARGET_DIR is /app/target-linux, backed by a NAMED VOLUME.
#    Sharing the host's ./target would thrash: same directory, two
#    target triples, mutually-invalidating fingerprints. A named
#    volume (rather than a plain host dir) also keeps Linux build
#    artefacts off the macOS filesystem, which is far slower over the
#    Docker Desktop bind-mount path. `/target-linux/` is gitignored
#    because the mount point still materialises as an empty host dir.
#
#  * The cargo registry and git checkout caches are named volumes too,
#    so a cold smithay git fetch happens exactly once.
#
#  * The workspace's [patch.crates-io] redirects x11rb-protocol at
#    ./tools/x11rb-fork/x11rb-protocol. Cargo resolves patches for the
#    WHOLE workspace even when the crate under -p never touches x11rb,
#    so the fork has to exist or resolution fails outright. Same
#    reason Dockerfile.sidecar runs the setup script.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="x11web-wayland-builder"
DOCKERFILE="$REPO_ROOT/tools/wayland-builder.Dockerfile"

# Build the toolchain image on first use (or when the Dockerfile
# changed — docker's own layer cache makes the no-op case ~instant).
if ! docker image inspect "$IMAGE" >/dev/null 2>&1 || [[ "${WAYLAND_BUILDER_REBUILD:-0}" == "1" ]]; then
    echo "==> building $IMAGE (one-off; subsequent runs reuse it)"
    docker build -t "$IMAGE" -f "$DOCKERFILE" "$REPO_ROOT/tools"
fi

# The x11rb fork is a host-side artefact (gitignored). Generate it
# here rather than inside the container: the generator is a plain
# cargo run that works fine on macOS, and doing it on the host means
# the container never has to carry a second Rust build of it.
if [[ ! -f "$REPO_ROOT/tools/x11rb-fork/x11rb-protocol/Cargo.toml" ]]; then
    echo "==> tools/x11rb-fork missing; running tools/setup-x11rb-fork.sh"
    bash "$REPO_ROOT/tools/setup-x11rb-fork.sh"
fi

if [[ $# -eq 0 ]]; then
    echo "usage: tools/wayland-build.sh <cargo args...>" >&2
    echo "   e.g. tools/wayland-build.sh check -p x11-web-wayland-server" >&2
    exit 2
fi

exec docker run --rm -i \
    -v "$REPO_ROOT":/app \
    -v x11web-wayland-cargo-registry:/usr/local/cargo/registry \
    -v x11web-wayland-cargo-git:/usr/local/cargo/git \
    -v x11web-wayland-target:/app/target-linux \
    -e CARGO_TARGET_DIR=/app/target-linux \
    -e CARGO_TERM_COLOR=never \
    -w /app \
    "$IMAGE" \
    cargo "$@"
