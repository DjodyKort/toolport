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

plan_missing="$("$SCRIPT_DIR/fork-codesign.sh" --dry-run --identity "Test Identity" --app /x/Toolport.app sign)"
check "a dry run without a bundle plans toolportctl" "$plan_missing" "--identifier com.djodykort.toolportplus.toolportctl /x/Toolport.app/Contents/MacOS/toolportctl"
check "a dry run without a bundle plans toolport-selfmcp" "$plan_missing" "--identifier com.djodykort.toolportplus.toolport-selfmcp /x/Toolport.app/Contents/MacOS/toolport-selfmcp"

work="$(mktemp -d "${TMPDIR:-/tmp}/fork-codesign-tests.XXXXXX")"
trap 'rm -rf "$work"' EXIT
app="$work/Toolport.app"
stubs="$work/stubs"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks/Inner" "$app/Contents/Resources" "$stubs" "$work/state"

macho() { printf '\xcf\xfa\xed\xfe\x0c\x00\x00\x01 payload' >"$1"; }
macho "$app/Contents/MacOS/conduit"
macho "$app/Contents/MacOS/toolport-gateway"
macho "$app/Contents/MacOS/toolportctl"
macho "$app/Contents/MacOS/toolport-selfmcp"
macho "$app/Contents/Frameworks/Inner/libinner.dylib"
printf '\xca\xfe\xba\xbe\x00\x00\x00\x02 fat payload' >"$app/Contents/Frameworks/libfat.dylib"
printf '\xca\xfe\xba\xbe\x00\x00\x00\x34 java class' >"$app/Contents/Resources/Sample.class"
printf '\xca\xfe\xba\xbe' >"$app/Contents/Resources/short.bin"
printf 'not a binary\n' >"$app/Contents/MacOS/notes.txt"
ln -s toolportctl "$app/Contents/MacOS/linked"
cat >"$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
	<key>CFBundleExecutable</key>
	<string>conduit</string>
	<key>CFBundleIdentifier</key>
	<string>com.djodykort.toolportplus</string>
</dict></plist>
PLIST

listing="$("$SCRIPT_DIR/fork-codesign.sh" --app "$app" list)"
check "list names the gateway" "$listing" "$app/Contents/MacOS/toolport-gateway"
check "list names toolportctl" "$listing" "$app/Contents/MacOS/toolportctl"
check "list names toolport-selfmcp" "$listing" "$app/Contents/MacOS/toolport-selfmcp"
check "list names a nested dylib" "$listing" "$app/Contents/Frameworks/Inner/libinner.dylib"
check "list names a fat Mach-O" "$listing" "$app/Contents/Frameworks/libfat.dylib"
check "list names the main executable" "$listing" "$app/Contents/MacOS/conduit"
refuse "list skips a text file" "$listing" "notes.txt"
refuse "list skips a Java class file" "$listing" "Sample.class"
refuse "list skips a file shorter than a fat header" "$listing" "short.bin"
refuse "list skips a symlink" "$listing" "linked"
if [ "$(wc -l <<<"$listing" | tr -d ' ')" -eq 6 ]; then echo "ok   list holds exactly the six Mach-O files"; else echo "FAIL list count: $listing"; failures=$((failures + 1)); fi
if [ "$(head -n 1 <<<"$listing")" = "$app/Contents/Frameworks/Inner/libinner.dylib" ]; then echo "ok   list starts with the deepest file"; else echo "FAIL list order (first)"; failures=$((failures + 1)); fi
if [ "$(tail -n 1 <<<"$listing")" = "$app/Contents/MacOS/conduit" ]; then echo "ok   list ends with the main executable"; else echo "FAIL list order (last)"; failures=$((failures + 1)); fi

bundle_plan="$("$SCRIPT_DIR/fork-codesign.sh" --dry-run --identity "Test Identity" --app "$app" sign)"
check "a bundle plan signs the gateway" "$bundle_plan" "--identifier com.djodykort.toolportplus.gateway $app/Contents/MacOS/toolport-gateway"
check "a bundle plan signs toolportctl" "$bundle_plan" "--identifier com.djodykort.toolportplus.toolportctl $app/Contents/MacOS/toolportctl"
check "a bundle plan signs toolport-selfmcp" "$bundle_plan" "--identifier com.djodykort.toolportplus.toolport-selfmcp $app/Contents/MacOS/toolport-selfmcp"
check "a bundle plan signs a nested dylib" "$bundle_plan" "--identifier com.djodykort.toolportplus.libinner.dylib $app/Contents/Frameworks/Inner/libinner.dylib"
refuse "a bundle plan leaves the main executable to the app signature" "$bundle_plan" "--identifier com.djodykort.toolportplus.conduit"
refuse "a bundle plan never uses --deep" "$bundle_plan" "--deep"
first_app="$(grep -n -- "--options runtime" <<<"$bundle_plan" | head -1 | cut -d: -f1)"
last_helper="$(grep -n -- "--identifier com.djodykort.toolportplus\.[a-z.-]* " <<<"$bundle_plan" | tail -1 | cut -d: -f1)"
if [ "$last_helper" -lt "$first_app" ]; then echo "ok   every helper is signed before the app"; else echo "FAIL helper order"; failures=$((failures + 1)); fi

cat >"$stubs/uname" <<'STUB'
#!/usr/bin/env bash
if [ "${1:-}" = "-s" ]; then echo Darwin; exit 0; fi
exec /usr/bin/uname "$@"
STUB
cat >"$stubs/security" <<'STUB'
#!/usr/bin/env bash
if [ "$1" = "find-identity" ] && [ -z "${STUB_NO_IDENTITY:-}" ]; then
  echo '  1) 0123456789ABCDEF0123456789ABCDEF01234567 "Test Identity"'
fi
exit 0
STUB
cat >"$stubs/codesign" <<'STUB'
#!/usr/bin/env bash
echo "$*" >>"$STUB_LOG"
key() { printf '%s' "$1" | cksum | cut -d' ' -f1; }
args=("$@")
last="${args[$((${#args[@]} - 1))]}"
case "$1" in
  --force)
    ident=""; id=""
    for ((i = 0; i < ${#args[@]}; i++)); do
      case "${args[$i]}" in
        --sign) id="${args[$((i + 1))]}" ;;
        --identifier) ident="${args[$((i + 1))]}" ;;
      esac
    done
    printf '%s\n%s\n' "$ident" "$id" >"$STUB_STATE/$(key "$last")"
    ;;
  --verify)
    if [ ! -f "$STUB_STATE/$(key "$last")" ]; then
      echo "$last: code object is not signed at all" >&2
      exit 1
    fi
    ;;
  -dvv)
    if [ -f "$STUB_STATE/$(key "$last")" ]; then
      {
        echo "Executable=$last"
        echo "Identifier=$(sed -n 1p "$STUB_STATE/$(key "$last")")"
        echo "Authority=$(sed -n 2p "$STUB_STATE/$(key "$last")")"
      } >&2
    else
      { echo "Identifier=$(basename "$last")-0123"; echo "Signature=adhoc"; } >&2
    fi
    ;;
  -d)
    echo '<?xml version="1.0"?><plist version="1.0"><dict/></plist>'
    ;;
esac
STUB
chmod +x "$stubs/uname" "$stubs/security" "$stubs/codesign"

export STUB_STATE="$work/state" STUB_LOG="$work/codesign.log"
: >"$STUB_LOG"
keychain="$work/test.keychain-db"
entitlements="$work/Fork.entitlements.plist"
printf '<plist/>\n' >"$entitlements"
sign_cmd=("$SCRIPT_DIR/fork-codesign.sh" --identity "Test Identity" --keychain "$keychain" --entitlements "$entitlements" --app "$app")

if signed="$(PATH="$stubs:$PATH" "${sign_cmd[@]}" sign 2>&1)"; then
  echo "ok   a real sign passes with every file signed"
else
  echo "FAIL a real sign failed: $signed"
  failures=$((failures + 1))
fi
signed_log="$(cat "$STUB_LOG")"
for file in Contents/MacOS/toolport-gateway Contents/MacOS/toolportctl Contents/MacOS/toolport-selfmcp Contents/Frameworks/Inner/libinner.dylib Contents/Frameworks/libfat.dylib; do
  check "a real sign calls codesign on $file" "$signed_log" "--sign Test Identity --identifier com.djodykort.toolportplus"
  state_file="$work/state/$(printf '%s' "$app/$file" | cksum | cut -d' ' -f1)"
  if [ -f "$state_file" ] && [ "$(sed -n 2p "$state_file")" = "Test Identity" ]; then
    echo "ok   $file carries the identity"
  else
    echo "FAIL $file was not signed with the identity"
    failures=$((failures + 1))
  fi
done
app_state="$work/state/$(printf '%s' "$app" | cksum | cut -d' ' -f1)"
if [ "$(sed -n 1p "$app_state")" = "com.djodykort.toolportplus" ]; then echo "ok   the app carries its own identifier"; else echo "FAIL app identifier"; failures=$((failures + 1)); fi
ctl_state="$work/state/$(printf '%s' "$app/Contents/MacOS/toolportctl" | cksum | cut -d' ' -f1)"
if [ "$(sed -n 1p "$ctl_state")" = "com.djodykort.toolportplus.toolportctl" ]; then echo "ok   toolportctl carries a stable identifier"; else echo "FAIL ctl identifier"; failures=$((failures + 1)); fi

if verified="$(PATH="$stubs:$PATH" "${sign_cmd[@]}" verify 2>&1)"; then
  echo "ok   verify passes when every file carries the identity"
else
  echo "FAIL verify after sign: $verified"
  failures=$((failures + 1))
fi

printf '%s\n%s\n' "com.djodykort.toolportplus.toolportctl" "Other Identity" >"$ctl_state"
rm -f "$work/state/$(printf '%s' "$app/Contents/MacOS/toolport-selfmcp" | cksum | cut -d' ' -f1)"
if broken="$(PATH="$stubs:$PATH" "${sign_cmd[@]}" verify 2>&1)"; then
  echo "FAIL verify must fail when a helper has another identity or none"
  failures=$((failures + 1))
else
  echo "ok   verify fails when a helper has another identity or none"
fi
check "verify names the helper with another identity" "$broken" "not signed with 'Test Identity': $app/Contents/MacOS/toolportctl"
check "verify names the helper that was never signed" "$broken" "$app/Contents/MacOS/toolport-selfmcp"
check "verify counts every offender" "$broken" "2 file(s)"
refuse "verify leaves the signed gateway out of the complaints" "$broken" "not signed with 'Test Identity': $app/Contents/MacOS/toolport-gateway"

if STUB_NO_IDENTITY=1 PATH="$stubs:$PATH" "${sign_cmd[@]}" sign >"$work/noid.out" 2>&1; then
  echo "FAIL sign must refuse when the identity is not in the keychain"
  failures=$((failures + 1))
else
  check "sign refuses without the identity in the keychain" "$(cat "$work/noid.out")" "no code-signing identity named 'Test Identity'"
fi

install_listing="$("$SCRIPT_DIR/mac-install.sh" --dry-run --run-id 42 --dest "$work/dest" --identity "Test Identity")"
check "install lists every Mach-O for the arm64 check" "$install_listing" "fork-codesign.sh --app"
check "install verifies the installed bundle" "$install_listing" "verify the installed bundle"
check "install verifies toolportctl" "$install_listing" "codesign --verify --strict --verbose=2 $work/dest/Toolport.app/Contents/MacOS/toolportctl"
check "install G1 lists toolportctl" "$install_listing" "codesign -dvv \"$work/dest/Toolport.app/Contents/MacOS/toolportctl\""
check "install G1 lists toolport-selfmcp" "$install_listing" "codesign -dvv \"$work/dest/Toolport.app/Contents/MacOS/toolport-selfmcp\""
check "install G1 offers the one-step verify" "$install_listing" "--identity \"Test Identity\" --app \"$work/dest/Toolport.app\" verify"

if "$SCRIPT_DIR/fork-codesign.sh" sign >/dev/null 2>&1 && [ "$(uname -s)" != "Darwin" ]; then
  echo "FAIL sign without --dry-run must refuse off macOS"
  failures=$((failures + 1))
else
  echo "ok   sign without --dry-run refuses off macOS"
fi

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
echo "all passed"
