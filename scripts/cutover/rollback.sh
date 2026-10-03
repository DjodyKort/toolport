#!/usr/bin/env bash
# Restore a HOME from a cutover backup, byte-identical.
# Run: scripts/cutover/rollback.sh [--home DIR] [--backup DIR] [--dry-run]
# Refuses when the backup is missing, incomplete or modified.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "$SCRIPT_DIR/lib.sh"

home="${HOME:-}"
backup=""
dry_run=0
while [ $# -gt 0 ]; do
  case "$1" in
    --home) home="${2:?--home needs a directory}"; shift 2 ;;
    --backup) backup="${2:?}"; shift 2 ;;
    --dry-run) dry_run=1; shift ;;
    -h|--help) sed -n '2,4p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

home="$(resolve_home "$home")"
if [ -z "$backup" ]; then
  backup="$(find "$home/$BACKUP_PARENT_REL" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | LC_ALL=C sort | tail -1)"
  [ -n "$backup" ] || die "no backup found under $home/$BACKUP_PARENT_REL"
fi
[ -d "$backup" ] || die "backup directory missing: $backup"
backup="$(cd "$backup" && pwd -P)"
case "$backup" in "$home"/*) ;; *) die "backup must live under --home" ;; esac
for f in manifest.sha256 scope.list pre.files pre.dirs; do
  [ -f "$backup/$f" ] || die "backup incomplete, missing $f"
done
[ -d "$backup/files" ] || die "backup incomplete, missing files/"

echo "verifying backup $backup"
check_hashes "$backup/files" "$backup/manifest.sha256" || die "backup was modified or is incomplete; refusing to restore"

while IFS= read -r root; do
  case "$root" in /*|*..*) die "unsafe scope entry in backup" ;; esac
done <"$backup/scope.list"

if [ "$dry_run" = 1 ]; then
  while IFS= read -r root; do echo "would restore: $root"; done <"$backup/scope.list"
  echo "dry run: nothing written"
  exit 0
fi

for root in "${SCOPE_ROOTS[@]}"; do
  if grep -Fxq -- "$root" "$backup/scope.list"; then
    rm -rf "${home:?}/$root"
    mkdir -p "$(dirname "$home/$root")"
    copy_rel "$backup/files" "$home" "$root"
    echo "restored: $root"
  elif [ -e "$home/$root" ] || [ -L "$home/$root" ]; then
    rm -rf "${home:?}/$root"
    echo "removed (absent before cutover): $root"
  fi
done

cur_files="$(mktemp)"
cur_dirs="$(mktemp)"
for w in "${WATCH_DIRS[@]}"; do
  list_entries "$home" "$w" >>"$cur_files"
  list_dirs "$home" "$w" >>"$cur_dirs"
done
LC_ALL=C sort -o "$cur_files" "$cur_files"
LC_ALL=C sort -o "$cur_dirs" "$cur_dirs"
LC_ALL=C comm -23 "$cur_files" "$backup/pre.files" | while IFS= read -r rel; do
  rm -f "${home:?}/$rel"
  echo "removed (created by cutover): $rel"
done
LC_ALL=C comm -23 "$cur_dirs" "$backup/pre.dirs" | LC_ALL=C sort -r | while IFS= read -r rel; do
  rmdir "$home/$rel" 2>/dev/null && echo "removed dir: $rel" || true
done
rm -f "$cur_files" "$cur_dirs"

tmp="$(mktemp)"
while IFS= read -r root; do
  list_entries "$home" "$root"
done <"$backup/scope.list" | LC_ALL=C sort >"$tmp"
while IFS= read -r rel; do
  printf '%s  %s\n' "$(hash_entry "$home/$rel")" "$rel"
done <"$tmp" | LC_ALL=C sort >"$tmp.now"
LC_ALL=C sort "$backup/manifest.sha256" >"$tmp.want"
if diff "$tmp.want" "$tmp.now" >/dev/null; then echo "rollback verified: restored tree matches the manifest"
else rm -f "$tmp" "$tmp.now" "$tmp.want"; die "restored tree differs from the manifest"; fi
rm -f "$tmp" "$tmp.now" "$tmp.want"
echo "manual (outside --home, not done here): uv tool install --force --editable <mcpm.sh checkout>; mcpm context sync"
