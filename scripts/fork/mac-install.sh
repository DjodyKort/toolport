#!/usr/bin/env bash
# DRAFT, not run yet: install the fork build on this Mac. Replaces the 1.x
# mac-install.sh, which downloaded a Fork CI artifact; there is no CI artifact
# now, so the bundle comes from scripts/fork/build-app.sh on this Mac.
#
# Steps: build the bundle (or take --app), re-sign every Mach-O in it with the
# local identity (fork-codesign.sh, D-009), verify, quit a running Toolport,
# replace the installed app, verify the installed bundle, select the
# login-keychain secrets mode, print the checks to run by hand.
#
#   mac-install.sh [--dry-run] [--build] [--app PATH] [--identity NAME]
#                  [--dest DIR] [--data-dir DIR] [--no-login-keychain]
#
# --build     run scripts/fork/build-app.sh first (about 3 to 7 minutes)
# --app       a bundle to install (default: the one build-app.sh writes)
# --data-dir  where the secrets-backend marker goes (default: the app's data dir)
# --dry-run   print every step without needing macOS or changing anything
#
# It never touches the login keychain itself. The master key item is created or
# read by the app. Nothing here prompts for a keychain password.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CODESIGN="$SCRIPT_DIR/fork-codesign.sh"

APP_ID="com.djodykort.toolportplus"
IDENTITY="${FORK_SIGN_IDENTITY:-Toolport Plus Local Signing}"
BUILT_APP="$REPO_ROOT/src-tauri/target/release/bundle/macos/Toolport.app"
SOURCE_APP="$BUILT_APP"
DEST="/Applications"
DATA_DIR="$HOME/Library/Application Support/Toolport"
APP_NAME="Toolport.app"
BUILD=0
LOGIN_KEYCHAIN=1
DRY_RUN=0

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY_RUN=1 ;;
    --build) BUILD=1 ;;
    --app) [ $# -ge 2 ] || die "--app needs a value"; SOURCE_APP="${2%/}"; shift ;;
    --identity) [ $# -ge 2 ] || die "--identity needs a value"; IDENTITY="$2"; shift ;;
    --dest) [ $# -ge 2 ] || die "--dest needs a value"; DEST="$2"; shift ;;
    --data-dir) [ $# -ge 2 ] || die "--data-dir needs a value"; DATA_DIR="$2"; shift ;;
    --no-login-keychain) LOGIN_KEYCHAIN=0 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
  shift
done

step() { printf '\n== %s\n' "$*"; }

run() {
  printf '+'
  printf ' %q' "$@"
  printf '\n'
  if [ "$DRY_RUN" -eq 0 ]; then
    "$@"
  fi
}

step "preflight"
if [ "$DRY_RUN" -eq 0 ]; then
  [ "$(uname -s)" = "Darwin" ] || die "run this on the Mac (use --dry-run elsewhere)"
  [ "$(uname -m)" = "arm64" ] || die "the build is arm64 only"
  for tool in security codesign ditto plutil lipo xattr osascript; do
    command -v "$tool" >/dev/null 2>&1 || die "required tool missing: $tool"
  done
  security find-identity -p codesigning 2>/dev/null | grep -F "\"$IDENTITY\"" >/dev/null \
    || die "no code-signing identity named '$IDENTITY' (create it once: $CODESIGN create-cert)"
else
  echo "dry run: skipping platform, tool and identity checks"
fi

if [ "$BUILD" -eq 1 ]; then
  step "build the bundle"
  run "$SCRIPT_DIR/build-app.sh"
  SOURCE_APP="$BUILT_APP"
fi

WORK="${TMPDIR:-/tmp}/toolport-install-$$"
if [ "$DRY_RUN" -eq 1 ]; then
  WORK="/tmp/toolport-install-dry-run"
fi
STAGED="$WORK/$APP_NAME"

step "stage a copy of $SOURCE_APP"
if [ "$DRY_RUN" -eq 0 ]; then
  [ -d "$SOURCE_APP" ] || die "no bundle at $SOURCE_APP (run with --build)"
  bundle_id="$(plutil -extract CFBundleIdentifier raw "$SOURCE_APP/Contents/Info.plist")"
  [ "$bundle_id" = "$APP_ID" ] || die "unexpected bundle identifier: $bundle_id (expected $APP_ID)"
fi
run rm -rf "$WORK"
run mkdir -p "$WORK"
run ditto "$SOURCE_APP" "$STAGED"
run xattr -dr com.apple.quarantine "$STAGED"

step "re-sign with the local identity"
sign_args=(--identity "$IDENTITY" --app "$STAGED")
if [ "$DRY_RUN" -eq 1 ]; then
  sign_args=(--dry-run "${sign_args[@]}")
fi
"$CODESIGN" "${sign_args[@]}" sign

step "verify the staged bundle"
verify_args=(--identity "$IDENTITY" --app "$STAGED")
if [ "$DRY_RUN" -eq 1 ]; then
  verify_args=(--dry-run "${verify_args[@]}")
fi
"$CODESIGN" "${verify_args[@]}" verify

step "install to $DEST"
if [ "$DRY_RUN" -eq 0 ]; then
  osascript -e 'tell application "Toolport" to quit' >/dev/null 2>&1 || true
  pkill -x toolport-gateway >/dev/null 2>&1 || true
fi
run rm -rf "$DEST/$APP_NAME"
run ditto "$STAGED" "$DEST/$APP_NAME"

step "verify the installed bundle (every Mach-O carries the identity)"
verify_args=(--identity "$IDENTITY" --app "$DEST/$APP_NAME")
if [ "$DRY_RUN" -eq 1 ]; then
  verify_args=(--dry-run "${verify_args[@]}")
fi
"$CODESIGN" "${verify_args[@]}" verify

if [ "$LOGIN_KEYCHAIN" -eq 1 ]; then
  step "select the login-keychain secrets mode"
  echo "note: the marker must sit in the data dir the app uses; pass --data-dir if it differs"
  run mkdir -p "$DATA_DIR"
  if [ "$DRY_RUN" -eq 0 ]; then
    printf 'login-keychain\n' >"$DATA_DIR/secrets-backend"
  else
    echo "+ printf 'login-keychain\\n' > '$DATA_DIR/secrets-backend'"
  fi
fi
run rm -rf "$WORK"

step "checks to run by hand, then launch the app"
cat <<CHECKS
1. "$CODESIGN" --identity "$IDENTITY" --app "$DEST/$APP_NAME" verify
   expect: exit 0; otherwise it names every Mach-O that is not signed with the identity
2. codesign -d --entitlements - "$DEST/$APP_NAME" 2>&1 | grep -c keychain-access-groups
   expect: 0
3. open "$DEST/$APP_NAME"
   expect: the app starts and is not killed (log show --last 2m --predicate 'process == "amfid"')
4. The Servers view lists every server with its secret names marked as saved.
   expect: no keychain prompt; the first run after an install may ask once for conduit-mcp:
   choose Always Allow, quit, relaunch, and expect no further prompt
5. toolportctl and toolport-selfmcp read the same vault. If either keeps prompting after
   Always Allow, run: $SCRIPT_DIR/keychain-partition-list.sh
6. Egress: nettop -m tcp -x -J bytes_in,bytes_out -p Toolport (30 s idle)
   expect: no connections to btsouth or toolport.app hosts
CHECKS
