#!/usr/bin/env bash
# Shared settings for the fork scripts. Source it; do not run it.
#
# The app (src-tauri) and the plus crates get separate target directories: after
# several conduit builds with different feature sets, a shared one produced
# bogus "multiple versions of crate serde_core" errors in plus/.

FORK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# The upstream ref this fork line is merged up to; change it in the merge commit.
FORK_UPSTREAM_REF="${FORK_UPSTREAM_REF:-$(head -n 1 "$FORK_ROOT/scripts/fork/upstream-ref")}"

# Same pin as upstream's clippy and rustfmt jobs; plus/rust-toolchain.toml is the source.
FORK_TOOLCHAIN="${FORK_TOOLCHAIN:-$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$FORK_ROOT/plus/rust-toolchain.toml")}"

FORK_APP_TARGET_DIR="${FORK_APP_TARGET_DIR:-$HOME/.cache/toolport-cargo-target-v2}"
FORK_PLUS_TARGET_DIR="${FORK_PLUS_TARGET_DIR:-$HOME/.cache/toolport-cargo-target-v2-plus}"

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

app_cargo() {
  CARGO_TARGET_DIR="$FORK_APP_TARGET_DIR" nice -n 10 cargo "+$FORK_TOOLCHAIN" "$@"
}

plus_cargo() {
  CARGO_TARGET_DIR="$FORK_PLUS_TARGET_DIR" nice -n 10 cargo "+$FORK_TOOLCHAIN" "$@"
}
