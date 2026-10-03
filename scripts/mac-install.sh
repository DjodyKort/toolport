#!/usr/bin/env bash
# Install the fork-ci "unsigned macOS app" artifact on a Mac: download, verify,
# re-sign with the self-signed identity (fork-codesign.sh), install, and print
# the G1 checks. See D-009 (login-keychain secrets, one stable signing identity).
#
#   mac-install.sh [--dry-run] [--run-id ID] [--repo OWNER/NAME]
#                  [--identity NAME] [--dest DIR] [--no-login-keychain]
#
# --run-id defaults to the newest successful Fork CI run on djody/main.
# --dry-run prints every step without needing macOS, gh or network access.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CODESIGN="$SCRIPT_DIR/fork-codesign.sh"

REPO="${FORK_REPO:-DjodyKort/toolport}"
BRANCH="djody/main"
WORKFLOW="Fork CI"
ARTIFACT="toolport-app-arm64-unsigned"
ZIP_NAME="Toolport-arm64-unsigned.app.zip"
APP_ID="com.djodykort.toolportplus"
IDENTITY="${FORK_SIGN_IDENTITY:-Toolport Plus Local Signing}"
DEST="/Applications"
RUN_ID=""
LOGIN_KEYCHAIN=1
DRY_RUN=0

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '2,10p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY_RUN=1 ;;
    --run-id) [ $# -ge 2 ] || die "--run-id needs a value"; RUN_ID="$2"; shift ;;
    --repo) [ $# -ge 2 ] || die "--repo needs a value"; REPO="$2"; shift ;;
    --identity) [ $# -ge 2 ] || die "--identity needs a value"; IDENTITY="$2"; shift ;;
    --dest) [ $# -ge 2 ] || die "--dest needs a value"; DEST="$2"; shift ;;
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
  [ "$(uname -m)" = "arm64" ] || die "the artifact is arm64 only"
  for tool in gh security codesign ditto plutil lipo xattr; do
    command -v "$tool" >/dev/null 2>&1 || die "required tool missing: $tool"
  done
  gh auth status >/dev/null 2>&1 || die "gh is not authenticated (run: gh auth login)"
else
  echo "dry run: skipping platform and tool checks"
fi

WORK="${TMPDIR:-/tmp}/toolport-install-$$"
if [ "$DRY_RUN" -eq 1 ]; then
  WORK="/tmp/toolport-install-dry-run"
fi
APP_NAME="Toolport.app"
STAGED="$WORK/extract/$APP_NAME"

step "select the fork-ci run"
if [ -z "$RUN_ID" ]; then
  if [ "$DRY_RUN" -eq 1 ]; then
    echo "+ gh run list --repo $REPO --workflow '$WORKFLOW' --branch $BRANCH --status success --limit 1 --json databaseId --jq '.[0].databaseId'"
    RUN_ID="<newest-successful-run-id>"
  else
    RUN_ID="$(gh run list --repo "$REPO" --workflow "$WORKFLOW" --branch "$BRANCH" \
      --status success --limit 1 --json databaseId --jq '.[0].databaseId')"
    [ -n "$RUN_ID" ] && [ "$RUN_ID" != "null" ] || die "no successful $WORKFLOW run on $BRANCH"
  fi
fi
echo "run: $RUN_ID"

step "download the artifact"
run mkdir -p "$WORK/dl" "$WORK/extract"
run gh run download "$RUN_ID" --repo "$REPO" --name "$ARTIFACT" --dir "$WORK/dl"
if [ "$DRY_RUN" -eq 0 ]; then
  [ -f "$WORK/dl/$ZIP_NAME" ] || die "artifact does not contain $ZIP_NAME"
fi
run ditto -x -k "$WORK/dl/$ZIP_NAME" "$WORK/extract"

step "verify the download"
if [ "$DRY_RUN" -eq 0 ]; then
  [ -d "$STAGED" ] || die "extracted bundle not found: $STAGED"
  bundle_id="$(plutil -extract CFBundleIdentifier raw "$STAGED/Contents/Info.plist")"
  [ "$bundle_id" = "$APP_ID" ] || die "unexpected bundle identifier: $bundle_id (expected $APP_ID)"
  exe="$(plutil -extract CFBundleExecutable raw "$STAGED/Contents/Info.plist")"
  [ -f "$STAGED/Contents/MacOS/$exe" ] || die "main executable missing"
  [ -f "$STAGED/Contents/MacOS/toolport-gateway" ] || die "nested gateway missing"
  for bin in "$exe" toolport-gateway; do
    lipo -archs "$STAGED/Contents/MacOS/$bin" | grep -qw arm64 || die "$bin is not arm64"
  done
  echo "ok: bundle id $bundle_id, executable $exe, gateway present, arm64"
else
  echo "+ plutil -extract CFBundleIdentifier raw $STAGED/Contents/Info.plist  (expect $APP_ID)"
  echo "+ test -f $STAGED/Contents/MacOS/toolport-gateway && lipo -archs (expect arm64)"
fi
run xattr -dr com.apple.quarantine "$STAGED"

step "re-sign with the self-signed identity"
sign_args=(--identity "$IDENTITY" --app "$STAGED")
if [ "$DRY_RUN" -eq 1 ]; then
  sign_args=(--dry-run "${sign_args[@]}")
fi
"$CODESIGN" "${sign_args[@]}" sign

step "install to $DEST"
if [ "$DRY_RUN" -eq 0 ]; then
  osascript -e 'tell application "Toolport" to quit' >/dev/null 2>&1 || true
  pkill -x toolport-gateway >/dev/null 2>&1 || true
fi
run rm -rf "$DEST/$APP_NAME"
run ditto "$STAGED" "$DEST/$APP_NAME"

if [ "$LOGIN_KEYCHAIN" -eq 1 ]; then
  step "select the login-keychain secrets mode"
  DATA_DIR="$HOME/Library/Application Support/Toolport"
  echo "note: the marker must sit in the data dir the app reports; adjust DATA_DIR if it differs"
  run mkdir -p "$DATA_DIR"
  if [ "$DRY_RUN" -eq 0 ]; then
    printf 'login-keychain\n' >"$DATA_DIR/secrets-backend"
  else
    echo "+ printf 'login-keychain\\n' > '$DATA_DIR/secrets-backend'"
  fi
fi
run rm -rf "$WORK"

step "G1 checks (run these, then launch the app)"
cat <<CHECKS
1. codesign -dvv "$DEST/$APP_NAME" 2>&1 | grep -E 'Identifier|Authority|Signature'
   codesign -dvv "$DEST/$APP_NAME/Contents/MacOS/toolport-gateway" 2>&1 | grep -E 'Identifier|Authority|Signature'
   expect: the same Authority ($IDENTITY) on both; Identifier $APP_ID and $APP_ID.gateway
2. codesign -d --entitlements - "$DEST/$APP_NAME" 2>&1 | grep -c keychain-access-groups
   expect: 0
3. open "$DEST/$APP_NAME"
   expect: the app starts and is not killed (no AMFI crash; check: log show --last 2m --predicate 'process == "amfid"')
4. In the app, save one synthetic secret on a test server; a Keychain prompt for "conduit-mcp" may appear once: choose Always Allow.
   expect: no further prompts after quitting and relaunching the app
5. Gateway read: run the gateway from a client (or: "$DEST/$APP_NAME/Contents/MacOS/toolport-gateway" --help) and confirm the server with the secret starts.
   expect: no keychain prompt
6. Update stability: re-run this script (same run id) and relaunch.
   expect: no new keychain prompt (same identity and identifier keep Always Allow)
7. Egress: nettop -m tcp -x -J bytes_in,bytes_out -p Toolport (30 s idle)
   expect: no connections to btsouth or toolport.app hosts
CHECKS
