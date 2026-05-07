#!/usr/bin/env bash
# One-shot installer that points git at this repo's `.hooks/`
# directory (instead of the per-clone default `.git/hooks`).
# Re-running is safe — `git config` just overwrites the same key.

set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
git -C "$ROOT" config core.hooksPath .hooks
echo "git core.hooksPath → .hooks"
