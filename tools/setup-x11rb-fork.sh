#!/usr/bin/env bash
# Clone upstream x11rb and apply our patches.
#
# Run this once after `git clone`-ing this repo, and again whenever
# `tools/x11rb-rev` or `crates/x11rb-protocol-msb/patches/*.patch` changes.
# Idempotent: re-running with an already-set-up tree is a no-op.
#
# We keep the working copy in `tools/x11rb-fork/` (not under `target/`)
# so `cargo clean` doesn't wipe it. The workspace-root Cargo.toml's
# `[patch."<git>"]` section points Cargo at the patched generator
# directory inside it.
#
# `patch-crate` doesn't work for us because its `git apply` step runs
# outside a git repo, and `git apply` silently skips new-file hunks in
# that mode. Our patch creates `src/lib.rs`, so we use plain `patch`.

set -euo pipefail

UPSTREAM_URL="https://github.com/psychon/x11rb.git"
PINNED_REV="4cd9f2429a9e83d8963e569b977df79894f70cab"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FORK_DIR="$REPO_ROOT/tools/x11rb-fork"
PATCH_DIR="$REPO_ROOT/crates/x11rb-protocol-msb/patches"
STAMP_FILE="$FORK_DIR/.x11web-applied-stamp"

# Hash of the inputs that determine whether the fork is up to date.
desired_stamp=$(printf '%s\n' "$PINNED_REV" "$(find "$PATCH_DIR" -name '*.patch' -print0 | sort -z | xargs -0 sha256sum 2>/dev/null | sha256sum)" | sha256sum | cut -d' ' -f1)

if [[ -f "$STAMP_FILE" ]] && [[ "$(cat "$STAMP_FILE")" == "$desired_stamp" ]]; then
    echo "x11rb fork already up to date at $FORK_DIR"
    exit 0
fi

echo "Setting up x11rb fork at $FORK_DIR (rev $PINNED_REV)..."
rm -rf "$FORK_DIR"
git clone --quiet "$UPSTREAM_URL" "$FORK_DIR"
git -C "$FORK_DIR" checkout --quiet "$PINNED_REV"

shopt -s nullglob
for patch in "$PATCH_DIR"/*.patch; do
    echo "  applying $(basename "$patch")"
    patch --quiet -p1 -d "$FORK_DIR/generator" < "$patch"
done

echo "$desired_stamp" > "$STAMP_FILE"
echo "x11rb fork ready"
