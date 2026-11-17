#!/usr/bin/env bash
# Build the unsigned fork Toolport.app with the fork overlay, on this Mac.
#
#   scripts/fork/build-app.sh            frontend, sidecars, release app bundle
#
# The bundle lands in src-tauri/target/release/bundle/macos/. Nothing is
# installed, signed with a real identity or uploaded. src-tauri/target is the
# default because upstream's prepare-sidecar.mjs looks for the gateway there; its release/
# links to the shared $FORK_TAURI_TARGET_DIR so worktrees do not each keep a release build; the plus
# sidecars build into the plus target dir under ~/.cache.
#
# Upstream's prepare-sidecar.mjs builds the gateway with --no-default-features, so the staged
# gateway has no fork seam (no egress guard). After the bundle is assembled the gateway inside it is
# replaced by one built with the `plus` feature; toolportctl doctor (plusBuilt) and
# verify-bundle.sh check the result.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
cd "$FORK_ROOT"

unset CARGO_TARGET_DIR
if [ ! -e src-tauri/target/release ]; then
  mkdir -p src-tauri/target "$FORK_TAURI_TARGET_DIR/release"
  ln -s "$FORK_TAURI_TARGET_DIR/release" src-tauri/target/release
fi
export PLUS_SIDECAR_TARGET_DIR="$FORK_PLUS_TARGET_DIR"
export RUSTUP_TOOLCHAIN="$FORK_TOOLCHAIN"

if [ ! -d node_modules/@tauri-apps/cli ]; then
  npm ci --ignore-scripts --no-audit --no-fund
fi

started=$SECONDS
nice -n 10 npx tauri build \
  --config src-tauri/tauri.bundle.conf.json \
  --config src-tauri/tauri.fork.conf.json \
  --bundles app
APP=src-tauri/target/release/bundle/macos/Toolport.app
CARGO_TARGET_DIR="$FORK_APP_TARGET_DIR" nice -n 10 cargo build --locked --release \
  --manifest-path src-tauri/Cargo.toml --bin toolport-gateway \
  --no-default-features --features search-static,plus
cp "$FORK_APP_TARGET_DIR/release/toolport-gateway" "$APP/Contents/MacOS/toolport-gateway"
chmod 755 "$APP/Contents/MacOS/toolport-gateway"
echo "built $APP in $((SECONDS - started))s"
