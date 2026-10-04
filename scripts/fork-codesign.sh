#!/usr/bin/env bash
# Re-sign a Toolport+ app bundle and every Mach-O file inside it (the gateway,
# toolportctl, toolport-selfmcp, ...) with one stable self-signed code-signing
# identity (D-009), so a keychain "Always Allow" survives updates for each of
# them. Inside-out, no --deep, no provisioning profile, no
# keychain-access-groups entitlement.
#
#   fork-codesign.sh [--dry-run] [--identity NAME] [--app PATH]
#                    [--entitlements PATH] [--keychain PATH] sign
#   fork-codesign.sh [--dry-run] [--identity NAME] [--app PATH] verify
#   fork-codesign.sh [--dry-run] [--app PATH] list
#   fork-codesign.sh [--dry-run] [--identity NAME] [--keychain PATH] create-cert
#
# list prints every Mach-O file of the bundle (found by its magic bytes, symlinks
# skipped). verify fails and names each one that is not signed with the identity.
# --dry-run prints the exact commands and exits 0 on any OS; with no bundle at
# --app it plans for the helpers the bundle is expected to hold.

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
EXPECTED_HELPERS="toolport-gateway toolportctl toolport-selfmcp"
DRY_RUN=0
ACTION=""

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '2,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY_RUN=1 ;;
    --identity) [ $# -ge 2 ] || die "--identity needs a value"; IDENTITY="$2"; shift ;;
    --app) [ $# -ge 2 ] || die "--app needs a value"; APP="$2"; shift ;;
    --entitlements) [ $# -ge 2 ] || die "--entitlements needs a value"; ENTITLEMENTS="$2"; shift ;;
    --keychain) [ $# -ge 2 ] || die "--keychain needs a value"; KEYCHAIN="$2"; shift ;;
    -h|--help) usage; exit 0 ;;
    sign|verify|list|create-cert) ACTION="$1" ;;
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

is_macho() {
  local head
  head="$(od -An -N8 -tx1 "$1" 2>/dev/null | tr -d ' \n')" || return 1
  case "$head" in
    feedface*|feedfacf*|cefaedfe*|cffaedfe*) return 0 ;;
    cafebabe*|cafebabf*)
      # a fat header counts a few architectures; a Java class file has its version here
      [ "${#head}" -ge 16 ] && [ "$((16#${head:8:8}))" -lt 45 ] && return 0
      ;;
  esac
  return 1
}

main_executable() {
  local plist="$APP/Contents/Info.plist" name=""
  [ -f "$plist" ] || return 0
  if command -v plutil >/dev/null 2>&1; then
    name="$(plutil -extract CFBundleExecutable raw "$plist" 2>/dev/null || true)"
  fi
  if [ -z "$name" ]; then
    name="$(tr -d '\n' <"$plist" | sed -n 's|.*<key>CFBundleExecutable</key>[[:space:]]*<string>\([^<]*\)</string>.*|\1|p')"
  fi
  printf '%s' "$name"
}

# Every Mach-O file of the bundle, deepest first and the main executable last, which is the
# order codesign needs. Without a bundle, a dry run lists the helpers it should hold.
bundle_binaries() {
  local name main tab
  if [ ! -d "$APP" ]; then
    [ "$DRY_RUN" -eq 1 ] || die "app bundle not found: $APP"
    for name in $EXPECTED_HELPERS; do
      printf '%s\n' "$APP/Contents/MacOS/$name"
    done
    return 0
  fi
  main="$(main_executable)"
  tab="$(printf '\t')"
  find "$APP" -type f ! -path '*/_CodeSignature/*' | LC_ALL=C sort | while IFS= read -r file; do
    if is_macho "$file"; then printf '%s\n' "$file"; fi
  done | awk -v main="$APP/Contents/MacOS/$main" \
    'NF { n = gsub("/", "/"); print (($0 == main) ? 0 : 1) "\t" n "\t" $0 }' |
    LC_ALL=C sort -t "$tab" -k1,1nr -k2,2nr -k3,3 | cut -f3-
}

identifier_for() {
  case "$(basename "$1")" in
    toolport-gateway) printf '%s' "$GATEWAY_ID" ;;
    *) printf '%s.%s' "$APP_ID" "$(basename "$1")" ;;
  esac
}

is_main_executable() {
  local main
  main="$(main_executable)"
  [ -n "$main" ] && [ "$1" = "$APP/Contents/MacOS/$main" ]
}

plan_sign() {
  local bins bin name
  echo "# sign plan: identity='$IDENTITY' app='$APP'"
  echo "# order: every nested Mach-O file, deepest first, then the app bundle (inside-out, never deep)"
  if [ "$DRY_RUN" -eq 0 ]; then
    [ -d "$APP" ] || die "app bundle not found: $APP"
    [ -f "$GATEWAY" ] || die "nested gateway not found: $GATEWAY"
    [ -f "$ENTITLEMENTS" ] || die "entitlements not found: $ENTITLEMENTS"
    if ! security find-identity -p codesigning "$KEYCHAIN" | grep -F -- "\"$IDENTITY\"" >/dev/null; then
      die "no code-signing identity named '$IDENTITY' in $KEYCHAIN (run: $0 create-cert)"
    fi
    for name in $EXPECTED_HELPERS; do
      [ -f "$APP/Contents/MacOS/$name" ] || echo "warning: expected helper not in the bundle: $name" >&2
    done
  fi
  bins="$(bundle_binaries)"
  while IFS= read -r bin <&3; do
    [ -n "$bin" ] || continue
    if is_main_executable "$bin"; then continue; fi
    echo "# mach-o: $bin"
    run codesign --force --timestamp=none --keychain "$KEYCHAIN" \
      --sign "$IDENTITY" --identifier "$(identifier_for "$bin")" "$bin"
  done 3<<<"$bins"
  run codesign --force --timestamp=none --keychain "$KEYCHAIN" \
    --sign "$IDENTITY" --identifier "$APP_ID" \
    --options runtime --entitlements "$ENTITLEMENTS" "$APP"
  plan_verify
}

# One signature must carry the identity and the identifier that the keychain ACL keys on.
signed_with_identity() {
  local target="$1" want_id="$2" out
  if ! out="$(codesign -dvv "$target" 2>&1)"; then
    echo "not signed: $target (codesign cannot read a signature)" >&2
    return 1
  fi
  if ! grep -qxF "Authority=$IDENTITY" <<<"$out"; then
    if grep -qxF "Signature=adhoc" <<<"$out"; then
      echo "ad-hoc signed, not '$IDENTITY': $target" >&2
    else
      echo "not signed with '$IDENTITY': $target" >&2
    fi
    return 1
  fi
  if ! grep -qxF "Identifier=$want_id" <<<"$out"; then
    echo "wrong identifier on $target (want $want_id)" >&2
    return 1
  fi
}

verify_one() {
  local target="$1" want_id="$2"
  if ! run codesign --verify --strict --verbose=2 "$target"; then
    echo "failed verification: $target" >&2
    return 1
  fi
  run codesign -dvv "$target" || true
  if [ "$DRY_RUN" -eq 0 ]; then
    signed_with_identity "$target" "$want_id"
  fi
}

plan_verify() {
  local bins bin bad=0
  bins="$(bundle_binaries)"
  while IFS= read -r bin <&3; do
    [ -n "$bin" ] || continue
    if is_main_executable "$bin"; then continue; fi
    verify_one "$bin" "$(identifier_for "$bin")" || bad=$((bad + 1))
  done 3<<<"$bins"
  verify_one "$APP" "$APP_ID" || bad=$((bad + 1))
  run codesign -d --entitlements - "$APP" || true
  [ "$bad" -eq 0 ] || die "$bad file(s) in $APP are not signed with '$IDENTITY'"
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
  list) bundle_binaries ;;
  create-cert) plan_create_cert ;;
esac
