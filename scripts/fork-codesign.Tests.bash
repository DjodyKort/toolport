#!/usr/bin/env bash
# Run: bash scripts/fork-codesign.Tests.bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
failures=0

check() {
  local name="$1" haystack="$2" needle="$3"
  if grep -F -- "$needle" <<<"$haystack" >/dev/null; then
    echo "ok   $name"
  else
    echo "FAIL $name: missing '$needle'"
    failures=$((failures + 1))
  fi
}

refuse() {
  local name="$1" haystack="$2" needle="$3"
  if grep -F -- "$needle" <<<"$haystack" >/dev/null; then
    echo "FAIL $name: unexpected '$needle'"
    failures=$((failures + 1))
  else
    echo "ok   $name"
  fi
}

plan="$("$SCRIPT_DIR/fork-codesign.sh" --dry-run --identity "Test Identity" --app /x/Toolport.app sign)"
check "sign signs the gateway" "$plan" "--identifier com.djodykort.toolportplus.gateway /x/Toolport.app/Contents/MacOS/toolport-gateway"
check "sign signs the app" "$plan" "--identifier com.djodykort.toolportplus --options runtime"
check "sign uses the identity" "$plan" "--sign Test\\ Identity"
refuse "sign never uses --deep" "$plan" "--deep"
refuse "sign names no keychain access group" "$plan" "keychain-access-groups"
gateway_line="$(grep -n 'toolport-gateway$' <<<"$plan" | head -1 | cut -d: -f1)"
app_line="$(grep -n 'options runtime' <<<"$plan" | head -1 | cut -d: -f1)"
if [ "$gateway_line" -lt "$app_line" ]; then echo "ok   gateway is signed before the app"; else echo "FAIL order"; failures=$((failures + 1)); fi

verify="$("$SCRIPT_DIR/fork-codesign.sh" --dry-run --app /x/Toolport.app verify)"
check "verify prints codesign -dvv" "$verify" "codesign -dvv /x/Toolport.app"

cert="$("$SCRIPT_DIR/fork-codesign.sh" --dry-run create-cert)"
check "create-cert imports into the keychain" "$cert" "security import"
check "create-cert marks codeSigning" "$cert" "extendedKeyUsage = critical,codeSigning"

install_plan="$("$SCRIPT_DIR/mac-install.sh" --dry-run --run-id 42 --dest /tmp/dest)"
check "install downloads the stable artifact" "$install_plan" "gh run download 42 --repo DjodyKort/toolport --name toolport-app-arm64-unsigned"
check "install re-signs" "$install_plan" "codesign --force"
check "install copies to dest" "$install_plan" "/tmp/dest/Toolport.app"
check "install prints G1 checks" "$install_plan" "G1 checks"

if "$SCRIPT_DIR/fork-codesign.sh" sign >/dev/null 2>&1 && [ "$(uname -s)" != "Darwin" ]; then
  echo "FAIL sign without --dry-run must refuse off macOS"
  failures=$((failures + 1))
else
  echo "ok   sign without --dry-run refuses off macOS"
fi

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
echo "all passed"
