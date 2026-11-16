#!/usr/bin/env bash
# Build the unsigned fork Toolport.app with the fork overlay, on this Mac.
#
#   scripts/fork/build-app.sh            frontend, sidecars, release app bundle
#
# The bundle lands in src-tauri/target/release/bundle/macos/. Nothing is
# installed, signed with a real identity or uploaded. src-tauri/target is the
# default because upstream's prepare-sidecar.mjs looks for the gateway there.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
cd "$FORK_ROOT"

unset CARGO_TARGET_DIR
export RUSTUP_TOOLCHAIN="$FORK_TOOLCHAIN"

if [ ! -d node_modules/@tauri-apps/cli ]; then
  npm ci --ignore-scripts --no-audit --no-fund
fi

started=$SECONDS
nice -n 10 npx tauri build \
  --config src-tauri/tauri.bundle.conf.json \
  --config src-tauri/tauri.fork.conf.json \
  --bundles app
echo "built src-tauri/target/release/bundle/macos/Toolport.app in $((SECONDS - started))s"
