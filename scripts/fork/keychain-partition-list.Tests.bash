#!/usr/bin/env bash
# Run: bash scripts/fork/keychain-partition-list.Tests.bash
# Runs keychain-partition-list.sh against stubbed `security` and `uname`; the real
# keychain is never touched.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$SCRIPT_DIR/keychain-partition-list.sh"
PASSWORD="synthetic-keychain-password"
failures=0

pass() { echo "ok   $1"; }
fail() { echo "FAIL $1"; failures=$((failures + 1)); }

check() {
  local name="$1" haystack="$2" needle="$3"
  if grep -F -- "$needle" <<<"$haystack" >/dev/null; then pass "$name"; else fail "$name: missing '$needle'"; fi
}

refuse() {
  local name="$1" haystack="$2" needle="$3"
  if grep -F -- "$needle" <<<"$haystack" >/dev/null; then fail "$name: unexpected '$needle'"; else pass "$name"; fi
}

work="$(mktemp -d "${TMPDIR:-/tmp}/keychain-partition-tests.XXXXXX")"
trap 'rm -rf "$work"' EXIT
stubs="$work/stubs"
state="$work/state"
mkdir -p "$stubs" "$state"
keychain="$work/login.keychain-db"
: >"$keychain"

# Answers per run, set through STUB_* variables: the kernel name, whether the
# item exists, and whether the partition-list call succeeds.
cat >"$stubs/uname" <<'STUB'
#!/usr/bin/env bash
echo "${STUB_UNAME:-Darwin}"
STUB
cat >"$stubs/security" <<'STUB'
#!/usr/bin/env bash
echo "$1" >>"$STUB_STATE/commands"
case "$1" in
  find-generic-password)
    [ "${STUB_ITEM:-present}" = present ] || exit 44
    ;;
  set-generic-password-partition-list)
    shift
    while [ $# -gt 0 ]; do
      case "$1" in
        -k) printf '%s' "$2" >"$STUB_STATE/password"; shift ;;
        -S|-s|-a) printf '%s %s\n' "$1" "$2" >>"$STUB_STATE/arguments"; shift ;;
        *) echo "$1" >"$STUB_STATE/keychain" ;;
      esac
      shift
    done
    [ "${STUB_SET:-ok}" = ok ] || exit 1
    ;;
esac
STUB
chmod +x "$stubs/uname" "$stubs/security"

run() {
  rm -f "$state"/*
  PATH="$stubs:$PATH" STUB_STATE="$state" TOOLPORT_LOGIN_KEYCHAIN="${KEYCHAIN:-$keychain}" \
    bash "$SCRIPT" <<<"$PASSWORD" 2>&1
}

status=0
out="$(STUB_UNAME=Linux run)" || status=$?
[ "$status" -ne 0 ] && pass "refuses off macOS" || fail "refuses off macOS: exit 0"
check "off macOS names the kernel" "$out" "found Linux"
[ ! -e "$state/commands" ] && pass "off macOS runs no security command" || fail "off macOS ran security"

status=0
out="$(KEYCHAIN="$work/missing.keychain-db" run)" || status=$?
[ "$status" -ne 0 ] && pass "refuses a missing keychain file" || fail "missing keychain: exit 0"
check "a missing keychain names the override variable" "$out" "TOOLPORT_LOGIN_KEYCHAIN"
[ ! -e "$state/commands" ] && pass "a missing keychain runs no security command" || fail "missing keychain ran security"

status=0
out="$(STUB_ITEM=absent run)" || status=$?
[ "$status" -ne 0 ] && pass "refuses when the master key item does not exist" || fail "absent item: exit 0"
check "an absent item says to launch the app first" "$out" "launch the Toolport+ app at least once"
refuse "an absent item never rewrites the partition list" "$(cat "$state/commands")" "set-generic-password-partition-list"

status=0
out="$(run)" || status=$?
[ "$status" -eq 0 ] && pass "succeeds with the item present" || fail "success run exited $status"
arguments="$(cat "$state/arguments")"
check "sets the partition list" "$arguments" "-S apple-tool:,apple:"
check "targets the service" "$arguments" "-s conduit-mcp"
check "targets the master key account" "$arguments" "-a __conduit_master_key__"
[ "$(cat "$state/keychain")" = "$keychain" ] && pass "uses the login keychain file given" || fail "wrong keychain file"
[ "$(cat "$state/password")" = "$PASSWORD" ] && pass "hands the typed password to security" || fail "password not passed"
refuse "never prints the password" "$out" "$PASSWORD"
check "reports the result" "$out" "Partition list set to: apple-tool:,apple:"

status=0
out="$(STUB_SET=fail run)" || status=$?
[ "$status" -ne 0 ] && pass "a failing security call fails the script" || fail "failed set exited 0"
refuse "a failure does not report success" "$out" "Partition list set to"
refuse "a failure never prints the password" "$out" "$PASSWORD"

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
echo "all passed"
