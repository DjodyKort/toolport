#!/usr/bin/env bash
# Re-lock and re-seed after an upstream merge (or any Cargo.toml change in src-tauri).
#
# 1. src-tauri/Cargo.lock keeps upstream's versions; cargo only adds back the
#    plus path packages if a merge dropped them.
# 2. plus/Cargo.lock is re-seeded from it, so shared dependencies stay on the
#    versions upstream tested. Fork-only dependencies resolve fresh.
# 3. Both lock files must satisfy `--locked`, and no shared package may differ.
#
# Usage: scripts/fork/relock.sh [cargo flags, e.g. --offline]
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
cd "$FORK_ROOT"

echo "1/4 src-tauri/Cargo.lock: keep upstream's versions, add the plus path packages"
app_cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 "$@" >/dev/null

echo "2/4 plus/Cargo.lock: seed from src-tauri/Cargo.lock"
cp src-tauri/Cargo.lock plus/Cargo.lock
plus_cargo metadata --manifest-path plus/Cargo.toml --format-version 1 "$@" >/dev/null

echo "3/4 both lock files satisfy --locked"
app_cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --locked "$@" >/dev/null
plus_cargo metadata --manifest-path plus/Cargo.toml --format-version 1 --locked "$@" >/dev/null

echo "4/4 shared packages match upstream's versions"
python3 -I scripts/fork/lock-drift.py src-tauri/Cargo.lock plus/Cargo.lock

git --no-pager diff --stat -- src-tauri/Cargo.lock plus/Cargo.lock
