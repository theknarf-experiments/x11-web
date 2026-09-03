# Linux cargo toolchain for the Wayland sidecar.
#
# `crates/wayland-server` depends on smithay, which only builds on
# Linux — but this repo is developed on macOS. Rather than have every
# developer (or agent) `apt-get` on each invocation, we bake the build
# dependencies into a one-off image that `tools/wayland-build.sh`
# builds once and then reuses. The repo is bind-mounted at /app at run
# time, so nothing about the source is baked in and the image only
# needs rebuilding when this file changes.
#
# Build with:
#   docker build -t x11web-wayland-builder -f tools/wayland-builder.Dockerfile tools/
FROM rust:1-bookworm

# The dependency set is deliberately a superset of what
# `Dockerfile.sidecar-wayland` installs: this image is also used to
# run `cargo check --workspace` on Linux, which pulls in the X11
# server (osmesa + bundled freetype => cmake) and the backend.
#
#   pkg-config       — every -sys crate's discovery mechanism
#   capnproto        — x11-web-wire's build.rs codegens from wire.capnp
#   libxkbcommon-dev — keymap compilation (x11-server today, the
#                      Wayland seat tomorrow)
#   libwayland-dev   — not needed by smithay's pure-Rust wayland
#                      backend, but present so a future
#                      `server_system` feature flip doesn't turn into
#                      a mystery link error
#   libosmesa6-dev   — x11-web-x11-server's default `osmesa` feature
#   cmake            — freetype-sys's `bundled` feature
#   libudev-dev      — smithay's backend_* features (not enabled today;
#                      cheap insurance against a feature-unification
#                      surprise from another workspace member)
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    cmake \
    git \
    pkg-config \
    capnproto \
    libxkbcommon-dev \
    libwayland-dev \
    libosmesa6-dev \
    libudev-dev \
    && rm -rf /var/lib/apt/lists/*

# clippy is not in the `rust:1-bookworm` base (it ships rustc + cargo +
# rustfmt only), and the Linux-only crates are `cfg`'d out on the macOS
# host — so a host `cargo clippy` lints literally none of the Wayland
# code. Baking the component in is the only way to lint it at all.
RUN rustup component add clippy

WORKDIR /app
