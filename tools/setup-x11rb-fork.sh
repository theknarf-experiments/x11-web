#!/usr/bin/env bash
# Clone upstream x11rb, apply our patches, and (re)run the generator so
# the fork has a self-consistent copy of x11rb-protocol with its
# `src/protocol/*.rs` rendered from the bundled xcb-proto XMLs.
#
# Run this once after `git clone`-ing this repo, and again whenever
# `PINNED_REV` or `crates/x11rb-protocol-msb/patches/*.patch` changes.
# Idempotent: re-running with an already-set-up tree is a no-op via the
# stamp file. To force a regeneration, delete `tools/x11rb-fork/.x11web-applied-stamp`.
#
# We keep the working copy in `tools/x11rb-fork/` (not under `target/`)
# so `cargo clean` doesn't wipe it. The workspace-root Cargo.toml's
# `[patch."<git>"]` / `[patch.crates-io]` sections redirect Cargo at
# the regenerated `x11rb-protocol` inside this fork.
#
# We don't use `patch-crate` because its `git apply` step runs outside
# a git repo, and `git apply` silently skips new-file hunks in that
# mode. Our patch creates files, so we use plain `patch`.

set -euo pipefail

UPSTREAM_URL="https://github.com/psychon/x11rb.git"
PINNED_REV="4cd9f2429a9e83d8963e569b977df79894f70cab"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FORK_DIR="$REPO_ROOT/tools/x11rb-fork"
PATCH_DIR="$REPO_ROOT/crates/x11rb-protocol-msb/patches"
STAMP_FILE="$FORK_DIR/.x11web-applied-stamp"

desired_stamp=$(printf '%s\n' "$PINNED_REV" "$(find "$PATCH_DIR" -name '*.patch' -print0 | sort -z | xargs -0 sha256sum 2>/dev/null | sha256sum)" | sha256sum | cut -d' ' -f1)

if [[ -f "$STAMP_FILE" ]] && [[ "$(cat "$STAMP_FILE")" == "$desired_stamp" ]]; then
    echo "x11rb fork already up to date at $FORK_DIR"
    exit 0
fi

echo "Setting up x11rb fork at $FORK_DIR (rev $PINNED_REV)..."
rm -rf "$FORK_DIR"
git clone --quiet "$UPSTREAM_URL" "$FORK_DIR"
git -C "$FORK_DIR" checkout --quiet "$PINNED_REV"

# Patch files are named `<workspace-member>+<version>.patch` and are
# applied to `$FORK_DIR/<workspace-member>/`. `x11rb-generator` lives
# under `generator/` (not `x11rb-generator/`) in upstream's workspace.
shopt -s nullglob
for patch in "$PATCH_DIR"/*.patch; do
    member_name="${patch##*/}"
    member_name="${member_name%%+*}"
    target_subdir="$member_name"
    if [[ "$member_name" == "x11rb-generator" ]]; then
        target_subdir="generator"
    fi
    echo "  applying $(basename "$patch") to $target_subdir/"
    patch --quiet -p1 -d "$FORK_DIR/$target_subdir" < "$patch"
done

# Regenerate the protocol bindings in place so the fork's
# `x11rb-protocol` is a complete, self-consistent crate. Cargo's
# `[patch]` redirect points at this directory; without regeneration the
# fork's pre-committed `src/protocol/*.rs` is what gets used (which is
# fine for the spike but won't pick up generator patches later on).
echo "Running x11rb-generator against bundled xcb-proto-1.17.0/src..."
cd "$FORK_DIR"
cargo run --quiet --release -p x11rb-generator -- \
    xcb-proto-1.17.0/src \
    x11rb-protocol/src/protocol \
    x11rb/src/protocol \
    x11rb-async/src/protocol

echo "$desired_stamp" > "$STAMP_FILE"
echo "x11rb fork ready"
