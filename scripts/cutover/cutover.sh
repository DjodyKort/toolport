#!/usr/bin/env bash
# Cutover from mcpm to Toolport+ on one HOME.
# Run: scripts/cutover/cutover.sh [--home DIR] [--dry-run] [--mcpm-root DIR]
#        [--short-ids FILE] [--tools FILE] [--ctl PATH]
# Needs a toolportctl (TOOLPORT_CTL or --ctl) providing:
#   import mcpm <root> [--dry-run] [--short-ids f] [--home d] [--skip-clients]
#   import rename-refs <root> --tools <f> --paths <p>... [--short-ids f] [--home d] [--dry-run]
#   doctor
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
    -h|--help) sed -n '2,8p' "${BASH_SOURCE[0]}"; exit 0 ;;
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

echo "cutover home=$home dry_run=$dry_run"

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
for root in "${SCOPE_ROOTS[@]}"; do
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
d = json.load(open(p))
d["wrap_default_claude"] = False
d["ensure_allow"] = []
with open(p, "w") as f:
    json.dump(d, f, indent=2)
    f.write("\n")
PY
  echo "context.json: wrap_default_claude=false, ensure_allow emptied"
else
  echo "no context.json, nothing to neutralize"
fi

echo "== 5/6 import (apply) and switch clients =="
run_ctl "${import_args[@]}"
if [ ${#rename_args[@]} -gt 0 ]; then
  run_ctl "${rename_args[@]}"
fi

echo "== 6/6 verify =="
fail=0
if ! run_ctl doctor; then echo "verify: doctor failed" >&2; fail=1; fi
for rel in .claude.json .config/Claude/claude_desktop_config.json \
  "Library/Application Support/Claude/claude_desktop_config.json" .cursor/mcp.json .gemini/settings.json; do
  [ -f "$home/$rel" ] || continue
  if python3 - "$home/$rel" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
s = d.get("mcpServers", {})
bad = [k for k, v in s.items() if k != "toolport" and "mcpm" in json.dumps(v.get("command", "")) + json.dumps(v.get("args", []))]
sys.exit(0 if "toolport" in s and not bad else 1)
PY
  then echo "verify ok: $rel has a toolport entry and no mcpm launchers"
  else echo "verify FAIL: $rel" >&2; fail=1; fi
done
if grep -rIl 'mcp__mcpm_' "${rename_paths[@]}" >/dev/null 2>&1 && [ ${#rename_args[@]} -gt 0 ]; then
  echo "verify warn: unmapped mcp__mcpm_ references remain (see orphan report above)"
fi
[ "$fail" = 0 ] || { echo "cutover verify failed; roll back with: scripts/cutover/rollback.sh --home $home --backup $backup" >&2; exit 1; }
echo "cutover done; rollback with: scripts/cutover/rollback.sh --home $home --backup $backup"
echo "manual (outside --home, not done here): stop mcpm processes, replace shell shims, shadow period"
