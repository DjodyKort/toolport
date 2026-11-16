#!/usr/bin/env bash
# Run: bash scripts/fork/mac-install.Tests.bash
# Reads the plan mac-install.sh --dry-run prints. Nothing is built, signed or installed.
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

plan="$("$SCRIPT_DIR/mac-install.sh" --dry-run --identity "Test Identity" --dest /x/Apps --data-dir /x/data)"
check "signs the staged copy with the identity" "$plan" "--sign Test\\ Identity"
check "installs into the destination" "$plan" "ditto /tmp/toolport-install-dry-run/Toolport.app /x/Apps/Toolport.app"
check "verifies the installed bundle" "$plan" "codesign -dvv /x/Apps/Toolport.app"
check "writes the marker into the data dir" "$plan" "secrets-backend"
check "marker holds login-keychain" "$plan" "login-keychain"
check "points at the partition-list script" "$plan" "keychain-partition-list.sh"
refuse "never downloads a CI artifact" "$plan" "gh run"
refuse "never asks for a keychain password" "$plan" "security set-generic-password-partition-list"
refuse "the plan never uses --deep" "$plan" "--deep"

without="$("$SCRIPT_DIR/mac-install.sh" --dry-run --no-login-keychain --data-dir /x/data)"
refuse "--no-login-keychain writes no marker" "$without" "secrets-backend'"

build="$("$SCRIPT_DIR/mac-install.sh" --dry-run --build)"
check "--build runs build-app.sh first" "$build" "build-app.sh"

if "$SCRIPT_DIR/mac-install.sh" --bogus >/dev/null 2>&1; then
  echo "FAIL an unknown argument must fail"
  failures=$((failures + 1))
else
  echo "ok   an unknown argument fails"
fi

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
echo "all passed"
