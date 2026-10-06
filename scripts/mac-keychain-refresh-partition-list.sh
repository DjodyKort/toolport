#!/usr/bin/env bash
# Rewrite the macOS Keychain partition list on the Toolport+ master-key item
# (service "conduit-mcp", account "__conduit_master_key__", legacy login
# keychain) so toolportctl and toolport-selfmcp stop prompting "Always Allow"
# after every rebuild.
#
# Why this exists: refresh_master_key_acl() (src-tauri/src/secrets.rs) already
# rewrites the item's trusted-application list on every app launch, so the app
# and gateway survive a rebuild without a prompt. ctl and selfmcp are separate
# binaries the app does not launch, so that refresh never runs for them; and
# with no Apple Developer Team ID, the Keychain also pins access by partition
# ID (derived from each binary's cdhash on an ad-hoc/local signing identity),
# which changes on every rebuild regardless of the trusted-app list. Q-001 /
# D-109 (input #37, Djody, 06-10) chose to widen the partition list by hand
# after each rebuild rather than live with the repeat prompt (option B).
#
# This script only touches the partition list (-S), never the trusted-app
# list the Rust code owns, so it cannot clobber what the app itself wrote.
#
# Run by hand on the Mac after rebuilding toolportctl / toolport-selfmcp.
# Never run this on the migration server: there is no real Keychain there,
# and it asks for your login-keychain password interactively.
set -euo pipefail

SERVICE="conduit-mcp"
ACCOUNT="__conduit_master_key__"
KEYCHAIN="${TOOLPORT_LOGIN_KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}"
PARTITION_LIST="apple-tool:,apple:"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "error: this only makes sense on macOS (found $(uname -s))" >&2
    exit 1
fi

if [[ ! -f "$KEYCHAIN" ]]; then
    echo "error: keychain not found at $KEYCHAIN (pass TOOLPORT_LOGIN_KEYCHAIN=<path> to override)" >&2
    exit 1
fi

echo "This rewrites the partition list on the Toolport+ master-key Keychain item"
echo "(service=$SERVICE, account=$ACCOUNT, keychain=$KEYCHAIN)."
echo "It needs your login-keychain password; it is never stored or logged."
echo

read -r -s -p "Login keychain password: " KC_PASSWORD
echo
trap 'unset KC_PASSWORD' EXIT

if ! security find-generic-password -s "$SERVICE" -a "$ACCOUNT" "$KEYCHAIN" >/dev/null 2>&1; then
    echo "error: no item found for service=$SERVICE account=$ACCOUNT in $KEYCHAIN" >&2
    echo "       launch the Toolport+ app at least once first so it creates the master key." >&2
    exit 1
fi

security set-generic-password-partition-list \
    -S "$PARTITION_LIST" \
    -s "$SERVICE" \
    -a "$ACCOUNT" \
    -k "$KC_PASSWORD" \
    "$KEYCHAIN"

unset KC_PASSWORD
trap - EXIT

echo
echo "Partition list set to: $PARTITION_LIST"
echo "Re-launch toolportctl and toolport-selfmcp now and confirm neither prompts."
echo "If one still prompts, choose Always Allow once more and re-run this script;"
echo "if the prompt keeps coming back, stop and tell the coordinator rather than"
echo "repeating this blind (it may mean a different code path is reading the item)."
