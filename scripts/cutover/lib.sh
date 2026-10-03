#!/usr/bin/env bash
# Shared helpers for cutover.sh and rollback.sh. Bash 3.2 compatible (macOS).

BACKUP_PARENT_REL=".toolport-cutover-backups"

SCOPE_ROOTS=(
  ".claude.json"
  ".config/Claude/claude_desktop_config.json"
  "Library/Application Support/Claude/claude_desktop_config.json"
  ".cursor/mcp.json"
  ".gemini/settings.json"
  ".config/mcpm"
  ".zshrc"
  ".claude/settings.json"
  ".claude/settings.local.json"
  ".claude/skills"
  ".claude/commands"
)

# Directories scanned to find entries cutover created, so rollback can remove them.
WATCH_DIRS=(".config" ".claude" ".cursor" ".gemini" "Library/Application Support")

die() { echo "error: $*" >&2; exit 1; }

sha_of_stdin() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum | cut -d' ' -f1
  else shasum -a 256 | cut -d' ' -f1; fi
}

hash_entry() {
  if [ -L "$1" ]; then printf 'link:%s' "$(readlink "$1")" | sha_of_stdin
  else sha_of_stdin <"$1"; fi
}

resolve_home() {
  local h="$1"
  [ -n "$h" ] || die "--home must not be empty"
  [ -d "$h" ] || die "--home is not a directory: $h"
  (cd "$h" && pwd -P)
}

list_entries() {
  local base="$1" root="$2"
  [ -e "$base/$root" ] || [ -L "$base/$root" ] || return 0
  (cd "$base" && find "$root" \( -type f -o -type l \) -print)
}

list_dirs() {
  local base="$1" root="$2"
  [ -d "$base/$root" ] || return 0
  (cd "$base" && find "$root" -type d -print)
}

write_hashes() {
  local base="$1" out="$2"
  : >"$out"
  local rel
  (cd "$base" && find . \( -type f -o -type l \) -print | sed 's|^\./||' | LC_ALL=C sort) |
    while IFS= read -r rel; do
      printf '%s  %s\n' "$(hash_entry "$base/$rel")" "$rel" >>"$out"
    done
}

# Recompute hashes of $1 and compare to manifest $2; prints offending paths only.
check_hashes() {
  local base="$1" manifest="$2" tmp rc=0
  tmp="$(mktemp)"
  write_hashes "$base" "$tmp"
  if ! diff <(LC_ALL=C sort "$manifest") <(LC_ALL=C sort "$tmp") >"$tmp.diff"; then
    sed -n 's/^[<>] [0-9a-f]*  /  differs: /p' "$tmp.diff" | sort -u >&2
    rc=1
  fi
  rm -f "$tmp" "$tmp.diff"
  return $rc
}

copy_rel() {
  local src_base="$1" dst_base="$2" rel="$3"
  mkdir -p "$dst_base"
  (cd "$src_base" && tar -cf - "$rel") | (cd "$dst_base" && tar -xpf -)
}
