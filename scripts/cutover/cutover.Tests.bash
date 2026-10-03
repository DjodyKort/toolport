#!/usr/bin/env bash
# Run: bash scripts/cutover/cutover.Tests.bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
failures=0
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

pass() { echo "ok   $1"; }
fail() { echo "FAIL $1"; failures=$((failures + 1)); }
expect() { local name="$1"; shift; if "$@"; then pass "$name"; else fail "$name"; fi; }
expect_not() { local name="$1"; shift; if "$@" >/dev/null 2>&1; then fail "$name"; else pass "$name"; fi; }

# Stub of the toolportctl interface cutover.sh assumes; it only touches $HOME.
stub="$work/ctl-stub"
cat >"$stub" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
dry=0
case "$1 ${2:-}" in
  "import mcpm")
    shift 2; root="$1"; shift
    while [ $# -gt 0 ]; do
      case "$1" in
        --dry-run) dry=1; shift ;;
        --short-ids|--home) shift 2 ;;
        --skip-clients) shift ;;
        *) echo "stub: unknown arg $1" >&2; exit 2 ;;
      esac
    done
    [ -f "$root/servers.json" ] || exit 3
    if [ "$dry" = 1 ]; then echo "plan: 2 servers, 4 clients (dry run)"; exit 0; fi
    mkdir -p "$HOME/.config/toolport"
    reg="$HOME/.config/toolport/registry.json"
    state="${STUB_SELF_STATE:-enabled}"
    if [ -f "$reg" ]; then
      prev="$(sed -n 's/.*"selfServer":"\([a-z-]*\)".*/\1/p' "$reg")"
      [ -z "$prev" ] || state="$prev"
    fi
    echo "{\"servers\":[\"fake-a\",\"fake-b\"],\"selfServer\":\"$state\"}" >"$reg"
    for f in .claude.json .config/Claude/claude_desktop_config.json .cursor/mcp.json .gemini/settings.json; do
      [ -f "$HOME/$f" ] || continue
      printf '{"mcpServers":{"toolport":{"command":"toolport-gateway"}}}\n' >"$HOME/$f"
    done
    echo "applied"
    ;;
  "import rename-refs")
    shift 2; shift
    paths=(); mode=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --dry-run) dry=1; shift ;;
        --tools|--short-ids|--home) shift 2 ;;
        --paths) shift; while [ $# -gt 0 ] && [ "${1#--}" = "$1" ]; do paths+=("$1"); shift; done ;;
        *) echo "stub: unknown arg $1" >&2; exit 2 ;;
      esac
    done
    n="$(grep -rIl 'mcp__mcpm_' "${paths[@]}" 2>/dev/null | wc -l | tr -d ' ')"
    echo "rename: $n files"
    if [ "$dry" = 0 ]; then
      grep -rIl 'mcp__mcpm_' "${paths[@]}" 2>/dev/null | while IFS= read -r f; do sed -i.bak 's/mcp__mcpm_/mcp__toolport_/g' "$f"; rm -f "$f.bak"; done
    fi
    ;;
  "mcp doctor")
    reg="$HOME/.config/toolport/registry.json"
    [ -f "$reg" ] || { echo "state missing"; exit 1; }
    state="$(sed -n 's/.*"selfServer":"\([a-z-]*\)".*/\1/p' "$reg")"
    echo "state $state"
    case "$state" in enabled|disabled|opted-out) exit 0 ;; *) exit 1 ;; esac
    ;;
  "doctor "*) echo "doctor: ok" ;;
  *) echo "stub: unknown command $*" >&2; exit 2 ;;
esac
STUB
chmod +x "$stub"

make_home() {
  local h="$1"
  mkdir -p "$h/.config/mcpm" "$h/.config/Claude" "$h/.cursor" "$h/.gemini" \
    "$h/.claude/skills/demo" "$h/.claude/commands" "$h/Documents"
  echo '{"FAKE-server":{"command":"mcpm","args":["run","FAKE-server"],"env":{"FAKE_API_KEY":"FAKE-secret-0001"}}}' >"$h/.config/mcpm/servers.json"
  echo '{"wrap_default_claude":true,"ensure_allow":["mcp__mcpm_fake"]}' >"$h/.config/mcpm/context.json"
  echo 'FAKE-db-bytes' >"$h/.config/mcpm/monitor.db"
  chmod 600 "$h/.config/mcpm/servers.json"
  for f in .claude.json .config/Claude/claude_desktop_config.json .cursor/mcp.json .gemini/settings.json; do
    echo '{"mcpServers":{"FAKE-server":{"command":"mcpm","args":["run","FAKE-server"]}}}' >"$h/$f"
  done
  echo '{"permissions":{"allow":["mcp__mcpm_fake__ping"]}}' >"$h/.claude/settings.json"
  echo '{"permissions":{"allow":["mcp__mcpm_fake__pong"]}}' >"$h/.claude/settings.local.json"
  echo 'use mcp__mcpm_fake__ping' >"$h/.claude/skills/demo/SKILL.md"
  echo 'use mcp__mcpm_fake__pong' >"$h/.claude/commands/demo.md"
  printf 'source FAKE/shims.sh\n' >"$h/.zshrc"
  echo 'unrelated' >"$h/Documents/note.txt"
  echo '{"fake":"FAKE-tools"}' >"$work/tools.json"
}

tree_digest() {
  (cd "$1" && find . -path ./.toolport-cutover-backups -prune -o \( -type f -o -type l \) -print | LC_ALL=C sort |
    while IFS= read -r f; do
      printf '%s %s %s\n' "$f" "$(stat -c '%a' "$f" 2>/dev/null || stat -f '%Lp' "$f")" "$(sha256sum <"$f" 2>/dev/null || shasum -a 256 <"$f")"
    done)
}

export TOOLPORT_CTL="$stub"
home="$work/home"
make_home "$home"
before="$work/before.digest"
tree_digest "$home" >"$before"
cp -a "$home" "$work/home.orig"

dry_out="$("$SCRIPT_DIR/cutover.sh" --home "$home" --tools "$work/tools.json" --dry-run)"
expect "dry run reports the importer plan" grep -q "plan: 2 servers" <<<"$dry_out"
expect "dry run writes nothing" diff -q "$before" <(tree_digest "$home")
expect_not "dry run creates no backup dir" test -e "$home/.toolport-cutover-backups"
expect_not "dry run creates no registry" test -e "$home/.config/toolport"

out="$("$SCRIPT_DIR/cutover.sh" --home "$home" --tools "$work/tools.json")"
expect "cutover verifies" grep -q "verify ok: .claude.json" <<<"$out"
expect "cutover verifies the self-management MCP" grep -q "verify ok: self-management MCP" <<<"$out"
expect "cutover enabled the self-management MCP" grep -q "state enabled" <<<"$out"
expect "cutover announces the self-management MCP step" grep -q "self-management MCP and enables it" <<<"$out"
expect "cutover creates a backup with a manifest" test -f "$(echo "$home"/.toolport-cutover-backups/*/manifest.sha256)"
expect "cutover switched the claude code config" grep -q '"toolport"' "$home/.claude.json"
expect "cutover renamed tool refs" grep -q 'mcp__toolport_fake__ping' "$home/.claude/settings.json"
expect "cutover neutralized context.json" grep -q '"wrap_default_claude": false' "$home/.config/mcpm/context.json"
expect "cutover created the registry" test -f "$home/.config/toolport/registry.json"
expect_not "cutover output never prints secrets" grep -q 'FAKE-secret' <<<"$out"
expect_not "cutover changed the tree" diff -q "$before" <(tree_digest "$home")
expect "unrelated files are untouched" diff -q "$work/home.orig/Documents/note.txt" "$home/Documents/note.txt"

rb_dry="$("$SCRIPT_DIR/rollback.sh" --home "$home" --dry-run)"
expect "rollback dry run lists restores" grep -q "would restore: .claude.json" <<<"$rb_dry"
expect "rollback dry run writes nothing" test -f "$home/.config/toolport/registry.json"

backup_dir="$(echo "$home"/.toolport-cutover-backups/*)"
cp -a "$backup_dir" "$work/backup.pristine"

rb_out="$("$SCRIPT_DIR/rollback.sh" --home "$home")"
expect "rollback verifies" grep -q "rollback verified" <<<"$rb_out"
expect "rollback tree is byte-identical (digest)" diff "$before" <(tree_digest "$home")
expect "rollback tree is byte-identical (diff -r)" diff -r --exclude=.toolport-cutover-backups "$work/home.orig" "$home"
expect_not "rollback removed files created by cutover" test -e "$home/.config/toolport"

"$SCRIPT_DIR/cutover.sh" --home "$home" --tools "$work/tools.json" >/dev/null
newest="$(find "$home/.toolport-cutover-backups" -mindepth 1 -maxdepth 1 -type d | LC_ALL=C sort | tail -1)"
echo 'FAKE-tampered' >>"$newest/files/.zshrc"
after_cut="$work/after-cut.digest"
tree_digest "$home" >"$after_cut"
expect_not "rollback refuses a tampered backup" "$SCRIPT_DIR/rollback.sh" --home "$home" --backup "$newest" 2>/dev/null
expect "refused rollback changed nothing" diff -q "$after_cut" <(tree_digest "$home")

rm "$newest/manifest.sha256"
expect_not "rollback refuses a backup without a manifest" "$SCRIPT_DIR/rollback.sh" --home "$home" --backup "$newest" 2>/dev/null
expect_not "rollback refuses a missing backup" "$SCRIPT_DIR/rollback.sh" --home "$home" --backup "$work/nope" 2>/dev/null

rm -rf "$newest"
cp -a "$work/backup.pristine" "$newest"
expect "pristine backup restores after a refusal" "$SCRIPT_DIR/rollback.sh" --home "$home" --backup "$newest" >/dev/null
expect "second rollback is byte-identical" diff "$before" <(tree_digest "$home")

# user files added after the cutover must survive rollback; cutover-created files must not
"$SCRIPT_DIR/cutover.sh" --home "$home" --tools "$work/tools.json" >/dev/null
created="$(echo "$home"/.toolport-cutover-backups/*/created.list | tr ' ' '\n' | tail -1)"
expect "created.list records the cutover registry" grep -q "\.config/toolport/registry.json" "$created"
expect_not "created.list does not record backed-up files" grep -q "claude_desktop_config.json" "$created"
mkdir -p "$home/.config/user-tool" "$home/.claude/extras"
echo 'FAKE-user-file' >"$home/.config/user-tool/settings.toml"
echo 'FAKE-user-file-2' >"$home/.config/toolport/user-added.txt"
echo 'FAKE-user-file-3' >"$home/.claude/extras/keep.md"
"$SCRIPT_DIR/rollback.sh" --home "$home" >/dev/null
expect "user file in a new watched dir survives rollback" grep -q FAKE-user-file "$home/.config/user-tool/settings.toml"
expect "user file added under a cutover-created dir survives rollback" test -f "$home/.config/toolport/user-added.txt"
expect "user file under .claude survives rollback" test -f "$home/.claude/extras/keep.md"
expect_not "cutover-created file is removed on rollback" test -e "$home/.config/toolport/registry.json"
expect "backed-up files are byte-identical after rollback" diff -q "$work/home.orig/.claude.json" "$home/.claude.json"
expect "backed-up config tree is byte-identical" diff -r "$work/home.orig/.config/mcpm" "$home/.config/mcpm"
rm -rf "$home/.config/user-tool" "$home/.config/toolport" "$home/.claude/extras"
expect "tree is byte-identical once user files are removed" diff "$before" <(tree_digest "$home")

expect_not "cutover refuses a missing --home" "$SCRIPT_DIR/cutover.sh" --home "$work/none" 2>/dev/null

# the self-management MCP: a failing mcp doctor fails the verify step
home_bad="$work/home-bad"
make_home "$home_bad"
bad_rc=0
bad_out="$(STUB_SELF_STATE=not-enabled "$SCRIPT_DIR/cutover.sh" --home "$home_bad" --tools "$work/tools.json" 2>&1)" || bad_rc=$?
expect "cutover fails when mcp doctor fails" test "$bad_rc" -ne 0
expect "the failed verify names mcp doctor" grep -q "verify FAIL: mcp doctor" <<<"$bad_out"
expect "the failed verify still offers the rollback" grep -q "roll back with" <<<"$bad_out"

# an opt-out is a healthy state for the verify step and survives the cutover
home_opt="$work/home-opt"
make_home "$home_opt"
mkdir -p "$home_opt/.config/toolport"
echo '{"servers":["old"],"selfServer":"opted-out"}' >"$home_opt/.config/toolport/registry.json"
opt_out="$("$SCRIPT_DIR/cutover.sh" --home "$home_opt" --tools "$work/tools.json")"
expect "an opted-out self server passes the verify step" grep -q "verify ok: self-management MCP" <<<"$opt_out"
expect "cutover reports the opt-out" grep -q "state opted-out" <<<"$opt_out"
expect "cutover keeps the opt-out" grep -q '"selfServer":"opted-out"' "$home_opt/.config/toolport/registry.json"

# a choice made after the cutover is left alone by rollback and by the next cutover
home_user="$work/home-user"
make_home "$home_user"
mkdir -p "$home_user/.config/toolport"
echo '{"servers":["old"],"selfServer":"enabled"}' >"$home_user/.config/toolport/registry.json"
"$SCRIPT_DIR/cutover.sh" --home "$home_user" --tools "$work/tools.json" >/dev/null
sed -i.bak 's/"selfServer":"enabled"/"selfServer":"disabled"/' "$home_user/.config/toolport/registry.json"
rm -f "$home_user/.config/toolport/registry.json.bak"
user_rb="$("$SCRIPT_DIR/rollback.sh" --home "$home_user")"
expect "rollback keeps a registry that existed before the cutover" test -f "$home_user/.config/toolport/registry.json"
expect "rollback leaves the user's disable choice alone" grep -q '"selfServer":"disabled"' "$home_user/.config/toolport/registry.json"
expect "rollback says the choice is left alone" grep -q "left alone: a Toolport registry" <<<"$user_rb"
expect "rollback restored the mcpm client config" grep -q '"mcpm"' "$home_user/.claude.json"
again="$("$SCRIPT_DIR/cutover.sh" --home "$home_user" --tools "$work/tools.json")"
expect "a second cutover reports the disable as healthy" grep -q "state disabled" <<<"$again"
expect "a second cutover does not re-enable it" grep -q '"selfServer":"disabled"' "$home_user/.config/toolport/registry.json"

if [ "$failures" -ne 0 ]; then echo "$failures failure(s)"; exit 1; fi
echo "all cutover tests passed"
