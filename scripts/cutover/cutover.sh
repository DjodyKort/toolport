#!/usr/bin/env bash
# Cutover from mcpm to Toolport+ on one HOME.
# Run: scripts/cutover/cutover.sh [--home DIR] [--dry-run] [--mcpm-root DIR]
#        [--short-ids FILE] [--tools FILE] [--ctl PATH]
# Needs a toolportctl (TOOLPORT_CTL or --ctl) providing:
#   import mcpm <root> [--dry-run] [--short-ids f] [--home d] [--skip-clients]
#   import rename-refs <root> --tools <f> --paths <p>... [--short-ids f] [--home d] [--dry-run]
#   --json client ls
#   doctor
#   mcp doctor
# The importer reads each client's own config when the mcpm root has no <client>.json snapshot
# (a real mcpm root has none). Every detected client config is backed up, and the run fails
# unless no detected client still launches mcpm.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "$SCRIPT_DIR/lib.sh"

home="${HOME:-}"
dry_run=0
mcpm_root=""
short_ids=""
tools=""
ctl="${TOOLPORT_CTL:-toolportctl}"

while [ $# -gt 0 ]; do
  case "$1" in
    --home) home="${2:?--home needs a directory}"; shift 2 ;;
    --dry-run) dry_run=1; shift ;;
    --mcpm-root) mcpm_root="${2:?}"; shift 2 ;;
    --short-ids) short_ids="${2:?}"; shift 2 ;;
    --tools) tools="${2:?}"; shift 2 ;;
    --ctl) ctl="${2:?}"; shift 2 ;;
    -h|--help) sed -n '2,13p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

home="$(resolve_home "$home")"
mcpm_root="${mcpm_root:-$home/.config/mcpm}"
[ -f "$mcpm_root/servers.json" ] || die "no servers.json under $mcpm_root"
command -v "$ctl" >/dev/null 2>&1 || [ -x "$ctl" ] || die "toolportctl not found ($ctl); set TOOLPORT_CTL"
command -v python3 >/dev/null 2>&1 || die "python3 is required"

# The ctl resolves its data dir from HOME/XDG_CONFIG_HOME, so pinning both keeps it inside --home.
run_ctl() { HOME="$home" XDG_CONFIG_HOME="$home/.config" "$ctl" "$@"; }

import_args=(import mcpm "$mcpm_root" --home "$home")
[ -z "$short_ids" ] || import_args+=(--short-ids "$short_ids")

rename_paths=()
for rel in .claude/settings.json .claude/settings.local.json .claude/skills .claude/commands; do
  [ -e "$home/$rel" ] && rename_paths+=("$home/$rel")
done
rename_args=()
if [ -n "$tools" ] && [ ${#rename_paths[@]} -gt 0 ]; then
  rename_args=(import rename-refs "$mcpm_root" --tools "$tools" --home "$home")
  [ -z "$short_ids" ] || rename_args+=(--short-ids "$short_ids")
  rename_args+=(--paths "${rename_paths[@]}")
fi

# "<id><TAB><path>" for every client whose config file exists, as the ctl sees it.
detected_clients() {
  local out
  out="$(run_ctl --json client ls)" || return 1
  python3 -c '
import json, os, sys
for c in json.loads(sys.stdin.readline())["data"]["clients"]:
    path = c.get("path") or ""
    if os.path.isfile(path):
        print("%s\t%s" % (c["id"], path))
' <<<"$out"
}

# check_client_config <config> <its copy in the backup> [strict]: fails when the config still
# launches mcpm, or when it had an mcpm entry before the cutover and has no toolport entry now.
# Without a copy, strict (a client known to be mcpm's) demands the toolport entry. Prints why.
check_client_config() {
  python3 - "$1" "$2" "${3:-0}" <<'PY'
import json, os, re, sys

path, before_path, strict = sys.argv[1], sys.argv[2], sys.argv[3] == "1"
KEYS = ("mcpServers", "servers", "mcp", "context_servers", "amp.mcpServers")


def scan(p):
    text = open(p, encoding="utf-8", errors="replace").read()
    try:
        doc = json.loads(text)
    except ValueError:
        doc = None
    if not isinstance(doc, dict):
        lines = re.findall(r'^\s*"?command"?\s*[:=].*\bmcpm\b', text, re.M)
        return (["a command line naming mcpm"] if lines else []), "toolport" in text
    found, has = [], False
    for key in KEYS:
        servers = doc.get(key)
        if not isinstance(servers, dict):
            continue
        for name, entry in servers.items():
            if name == "toolport":
                has = True
            elif isinstance(entry, dict) and "mcpm" in json.dumps(entry.get("command", "")) + json.dumps(entry.get("args", [])):
                found.append(name)
    return found, has


after, has = scan(path)
before = scan(before_path)[0] if before_path and os.path.exists(before_path) else None
if after:
    sys.exit("still launches mcpm: " + ", ".join(after))
if (before or (before is None and strict)) and not has:
    sys.exit("has no toolport entry")
print("had mcpm entries" if before else "no mcpm entries to switch")
PY
}

echo "cutover home=$home dry_run=$dry_run"

detected="$(detected_clients)" || die "toolportctl client ls failed; cannot list the detected clients"
echo "detected clients:"
if [ -n "$detected" ]; then
  while IFS=$'\t' read -r id path; do echo "  $id $path"; done <<<"$detected"
else
  echo "  none"
fi

echo "== 1/6 importer dry run =="
run_ctl "${import_args[@]}" --dry-run

if [ ${#rename_args[@]} -gt 0 ]; then
  echo "== 2/6 rename preview =="
  run_ctl "${rename_args[@]}" --dry-run
else
  echo "== 2/6 rename preview skipped (no --tools or nothing to scan) =="
fi

if [ "$dry_run" = 1 ]; then
  echo "dry run: nothing written (backup, context neutralize, import, rename and verify skipped)"
  exit 0
fi

echo "== 3/6 backup =="
umask 077
stamp="$(date -u +%Y%m%dT%H%M%SZ)-$$"
backup="$home/$BACKUP_PARENT_REL/$stamp"
mkdir -p "$backup/files"
: >"$backup/scope.list"
: >"$backup/pre.files"
: >"$backup/pre.dirs"
# a detected client config outside the fixed scope is backed up too, so rollback restores it
extra_roots=()
while IFS=$'\t' read -r id path; do
  [ -n "$id" ] || continue
  case "$path" in
    "$home"/*) rel="${path#"$home"/}" ;;
    *) echo "warn: $id config is outside --home and not in the backup: $path" >&2; continue ;;
  esac
  in_scope=0
  for root in "${SCOPE_ROOTS[@]}"; do
    case "$rel" in "$root"|"$root"/*) in_scope=1 ;; esac
  done
  [ "$in_scope" = 1 ] || extra_roots+=("$rel")
done <<<"$detected"
for root in "${SCOPE_ROOTS[@]}" ${extra_roots[@]+"${extra_roots[@]}"}; do
  if [ -e "$home/$root" ] || [ -L "$home/$root" ]; then
    printf '%s\n' "$root" >>"$backup/scope.list"
    copy_rel "$home" "$backup/files" "$root"
  fi
done
for w in "${WATCH_DIRS[@]}"; do
  list_entries "$home" "$w" >>"$backup/pre.files"
  list_dirs "$home" "$w" >>"$backup/pre.dirs"
done
LC_ALL=C sort -o "$backup/pre.files" "$backup/pre.files"
LC_ALL=C sort -o "$backup/pre.dirs" "$backup/pre.dirs"
write_hashes "$backup/files" "$backup/manifest.sha256"
: >"$backup/created.list"
record_created() {
  record_created_list "$home" "$backup" >"$backup/created.list.tmp" && mv "$backup/created.list.tmp" "$backup/created.list"
}
trap record_created EXIT
echo "backup: $backup ($(wc -l <"$backup/manifest.sha256" | tr -d ' ') files, manifest.sha256)"

echo "== 4/6 neutralize mcpm context re-sync =="
ctxfile="$home/.config/mcpm/context.json"
if [ -f "$ctxfile" ]; then
  python3 - "$ctxfile" <<'PY'
import json, sys
p = sys.argv[1]
try:
    d = json.load(open(p))
except ValueError as e:
    sys.exit("error: %s is not valid JSON (%s); fix or move it aside, then rerun" % (p, e))
d["wrap_default_claude"] = False
if "ensure_allow" in d:
    d["ensure_allow"] = []
if isinstance(d.get("settings"), dict):
    d["settings"]["ensure_allow"] = []
with open(p, "w") as f:
    json.dump(d, f, indent=2)
    f.write("\n")
PY
  echo "context.json: wrap_default_claude=false, ensure_allow emptied"
else
  echo "no context.json, nothing to neutralize"
fi

echo "== 5/6 import (apply) and switch clients =="
echo "(import also registers the self-management MCP and enables it in the default and every client profile, unless you opted out)"
run_ctl "${import_args[@]}"
if [ ${#rename_args[@]} -gt 0 ]; then
  run_ctl "${rename_args[@]}"
fi

echo "== 6/6 verify =="
fail=0
if ! run_ctl doctor; then echo "verify: doctor failed" >&2; fail=1; fi
if run_ctl mcp doctor; then echo "verify ok: self-management MCP (mcp doctor)"
else echo "verify FAIL: mcp doctor; see the failing check above, 'toolportctl mcp install' repairs it and never overrides an opt-out" >&2; fail=1; fi
verified=""
for rel in .claude.json .config/Claude/claude_desktop_config.json \
  "Library/Application Support/Claude/claude_desktop_config.json" .cursor/mcp.json .gemini/settings.json; do
  [ -f "$home/$rel" ] || continue
  verified="$verified$home/$rel"$'\n'
  if why="$(check_client_config "$home/$rel" "$backup/files/$rel" 1 2>&1)"; then
    echo "verify ok: $rel has a toolport entry and no mcpm launchers ($why)"
  else echo "verify FAIL: $rel $why" >&2; fail=1; fi
done
if after="$(detected_clients)"; then
  while IFS=$'\t' read -r id path; do
    [ -n "$id" ] || continue
    case "$verified" in *"$path"$'\n'*) continue ;; esac
    rel="${path#"$home"/}"
    if why="$(check_client_config "$path" "$backup/files/$rel" 2>&1)"; then
      echo "verify ok: client $id ($rel) no longer launches mcpm ($why)"
    else echo "verify FAIL: client $id is not switched, $path $why" >&2; fail=1; fi
  done <<<"$after"
else
  echo "verify FAIL: toolportctl client ls failed, the detected clients cannot be checked" >&2; fail=1
fi
if grep -rIl 'mcp__mcpm_' "${rename_paths[@]}" >/dev/null 2>&1 && [ ${#rename_args[@]} -gt 0 ]; then
  echo "verify warn: unmapped mcp__mcpm_ references remain (see orphan report above)"
fi
[ "$fail" = 0 ] || { echo "cutover verify failed; roll back with: scripts/cutover/rollback.sh --home $home --backup $backup" >&2; exit 1; }
echo "cutover done; rollback with: scripts/cutover/rollback.sh --home $home --backup $backup"
echo "manual (outside --home, not done here): stop mcpm processes, replace shell shims, shadow period"
