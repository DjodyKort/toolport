#!/usr/bin/env bash
# Re-sign a Toolport+ app bundle and its nested gateway with one stable
# self-signed code-signing identity (D-009). Inside-out, no --deep, no
# provisioning profile, no keychain-access-groups entitlement.
#
#   fork-codesign.sh [--dry-run] [--identity NAME] [--app PATH]
#                    [--entitlements PATH] [--keychain PATH] sign
#   fork-codesign.sh [--dry-run] [--identity NAME] [--app PATH] verify
#   fork-codesign.sh [--dry-run] [--identity NAME] [--keychain PATH] create-cert
#
# --dry-run prints the exact commands and exits 0 on any OS.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

IDENTITY="${FORK_SIGN_IDENTITY:-Toolport Plus Local Signing}"
APP="${APP:-Toolport.app}"
ENTITLEMENTS="${ENTITLEMENTS:-$REPO_ROOT/src-tauri/Fork.entitlements.plist}"
KEYCHAIN="${FORK_SIGN_KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}"
APP_ID="com.djodykort.toolportplus"
GATEWAY_ID="com.djodykort.toolportplus.gateway"
GATEWAY_REL="Contents/MacOS/toolport-gateway"
DRY_RUN=0
ACTION=""

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY_RUN=1 ;;
    --identity) [ $# -ge 2 ] || die "--identity needs a value"; IDENTITY="$2"; shift ;;
    --app) [ $# -ge 2 ] || die "--app needs a value"; APP="$2"; shift ;;
    --entitlements) [ $# -ge 2 ] || die "--entitlements needs a value"; ENTITLEMENTS="$2"; shift ;;
    --keychain) [ $# -ge 2 ] || die "--keychain needs a value"; KEYCHAIN="$2"; shift ;;
    -h|--help) usage; exit 0 ;;
    sign|verify|create-cert) ACTION="$1" ;;
    *) die "unknown argument: $1" ;;
  esac
  shift
done

[ -n "$ACTION" ] || { usage >&2; exit 2; }
[ -n "$IDENTITY" ] || die "identity must not be empty"

APP="${APP%/}"
GATEWAY="$APP/$GATEWAY_REL"

run() {
  printf '+'
  printf ' %q' "$@"
  printf '\n'
  if [ "$DRY_RUN" -eq 0 ]; then
    "$@"
  fi
}

require_real_macos() {
  [ "$DRY_RUN" -eq 1 ] && return 0
  [ "$(uname -s)" = "Darwin" ] || die "this action needs macOS; use --dry-run to print the plan"
  command -v codesign >/dev/null 2>&1 || die "codesign not found"
}

plan_sign() {
  echo "# sign plan: identity='$IDENTITY' app='$APP'"
  echo "# order: nested gateway first, then the app bundle (inside-out, never deep)"
  if [ "$DRY_RUN" -eq 0 ]; then
    [ -d "$APP" ] || die "app bundle not found: $APP"
    [ -f "$GATEWAY" ] || die "nested gateway not found: $GATEWAY"
    [ -f "$ENTITLEMENTS" ] || die "entitlements not found: $ENTITLEMENTS"
    if ! security find-identity -p codesigning "$KEYCHAIN" | grep -F -- "\"$IDENTITY\"" >/dev/null; then
      die "no code-signing identity named '$IDENTITY' in $KEYCHAIN (run: $0 create-cert)"
    fi
  fi
  run codesign --force --timestamp=none --keychain "$KEYCHAIN" \
    --sign "$IDENTITY" --identifier "$GATEWAY_ID" "$GATEWAY"
  run codesign --force --timestamp=none --keychain "$KEYCHAIN" \
    --sign "$IDENTITY" --identifier "$APP_ID" \
    --options runtime --entitlements "$ENTITLEMENTS" "$APP"
  plan_verify
}

plan_verify() {
  run codesign --verify --strict --verbose=2 "$GATEWAY"
  run codesign --verify --strict --verbose=2 "$APP"
  run codesign -dvv "$GATEWAY"
  run codesign -dvv "$APP"
  run codesign -d --entitlements - "$APP"
}

plan_create_cert() {
  local work
  echo "# create-cert plan: self-signed code-signing certificate '$IDENTITY'"
  echo "# the private key stays on this Mac; nothing is uploaded anywhere"
  if [ "$DRY_RUN" -eq 1 ]; then
    work="/tmp/toolport-cert-dry-run"
  else
    [ "$(uname -s)" = "Darwin" ] || die "create-cert needs macOS"
    work="$(mktemp -d)"
  fi
  local conf="$work/cert.cnf" key="$work/key.pem" crt="$work/cert.pem" p12="$work/cert.p12"
  echo "# $conf (written only outside --dry-run):"
  local conf_body
  conf_body="[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $IDENTITY
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning"
  printf '%s\n' "$conf_body" | sed 's/^/#   /'
  if [ "$DRY_RUN" -eq 0 ]; then
    printf '%s\n' "$conf_body" >"$conf"
  fi
  local pass="toolport-local-$$"
  run openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
    -config "$conf" -keyout "$key" -out "$crt"
  run openssl pkcs12 -export -inkey "$key" -in "$crt" -name "$IDENTITY" \
    -out "$p12" -passout "pass:$pass"
  run security import "$p12" -k "$KEYCHAIN" -P "$pass" -T /usr/bin/codesign
  run security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$crt"
  run security find-identity -p codesigning "$KEYCHAIN"
  if [ "$DRY_RUN" -eq 0 ]; then
    rm -rf "$work"
  fi
}

case "$ACTION" in
  sign) require_real_macos; plan_sign ;;
  verify) require_real_macos; plan_verify ;;
  create-cert) plan_create_cert ;;
esac
