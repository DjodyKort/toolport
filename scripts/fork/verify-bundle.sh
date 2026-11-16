#!/usr/bin/env bash
# Check a locally built fork Toolport.app without the real signing identity.
#
#   scripts/fork/verify-bundle.sh [APP]
#
# 1. The bundle carries the fork identifier and every sidecar.
# 2. fork-codesign.sh list names the sidecars, and verify fails on the unsigned
#    (or ad-hoc signed) bundle, naming each file.
# 3. The sign plan (--dry-run) covers every sidecar.
# Nothing is signed and no keychain is read: the positive sign-and-verify path needs a
# trusted code-signing identity and is covered by fork-codesign.Tests.bash with stubs.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

APP="${1:-$FORK_ROOT/src-tauri/target/release/bundle/macos/Toolport.app}"
APP="${APP%/}"
SIGN="$FORK_ROOT/scripts/fork/fork-codesign.sh"
IDENTITY="Toolport Plus Local Signing"
SIDECARS="toolport-gateway toolportctl toolport-selfmcp"

fail() { echo "FAIL $*" >&2; exit 1; }
ok() { echo "ok   $*"; }

[ -d "$APP" ] || fail "no bundle at $APP (run scripts/fork/build-app.sh)"

app_id="$(plutil -extract CFBundleIdentifier raw "$APP/Contents/Info.plist")"
[ "$app_id" = "com.djodykort.toolportplus" ] || fail "bundle identifier is $app_id"
ok "bundle identifier is $app_id"

listing="$("$SIGN" --app "$APP" list)"
for name in $SIDECARS; do
  [ -x "$APP/Contents/MacOS/$name" ] || fail "sidecar missing or not executable: $name"
  grep -qxF "$APP/Contents/MacOS/$name" <<<"$listing" || fail "list does not name $name"
  file "$APP/Contents/MacOS/$name" | grep -q "Mach-O" || fail "$name is not a Mach-O file"
  ok "sidecar $name is in the bundle and in the sign list"
done

if refused="$("$SIGN" --identity "$IDENTITY" --app "$APP" verify 2>&1)"; then
  fail "verify passed on a bundle that was never signed with $IDENTITY"
fi
for name in $SIDECARS; do
  grep -F "$APP/Contents/MacOS/$name" <<<"$refused" | grep -q "not signed\|ad-hoc signed\|failed verification" ||
    fail "verify did not name $name"
done
ok "verify names every sidecar on the unsigned bundle"

plan="$("$SIGN" --dry-run --identity "$IDENTITY" --app "$APP" sign)"
for name in toolportctl toolport-selfmcp; do
  grep -qF -- "--identifier com.djodykort.toolportplus.$name $APP/Contents/MacOS/$name" <<<"$plan" ||
    fail "sign plan does not cover $name"
done
grep -qF -- "--identifier com.djodykort.toolportplus.gateway $APP/Contents/MacOS/toolport-gateway" <<<"$plan" ||
  fail "sign plan does not cover the gateway"
grep -qF -- "--deep" <<<"$plan" && fail "sign plan uses --deep"
ok "sign plan covers every sidecar, inside-out, without --deep"
