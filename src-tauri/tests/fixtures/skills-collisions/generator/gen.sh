#!/bin/bash
# Generates the mcpm `skills sync` reference outputs used by MIG-SKL-11's replay.
# Runs the Python reference in a scratch HOME on a copy of the synthetic repository in
# ../repos with an optional overlay from ../overlays laid over the scratch root (repo/, home/);
# nothing outside the scratch directory is touched.
#
#   gen.sh <out-dir>
#
# Per case: args.txt (one argument per line, {repo} {home} {root} placeholders), exit.txt,
# output.txt (stdout and stderr, scratch root shown as <root>, backup stamps as <stamp>) and
# tree.txt (every file left behind, relative to the scratch root, logs excluded). Needs: the
# mcpm venv (MCPM_VENV, default /opt/toolport-plus/scratch/mcpm-venv).
set -u
OUT=${1:?usage: gen.sh <out-dir>}
VENV=${MCPM_VENV:-/opt/toolport-plus/scratch/mcpm-venv}
HERE=$(cd "$(dirname "$0")" && pwd)
FIX=$HERE/..
export LC_ALL=C
rm -rf "$OUT"; mkdir -p "$OUT"

# case_run <name> <overlay|-> <setup> -- <args...>
case_run() {
  local name=$1 overlay=$2 setup=$3; shift 4
  local W; W=$(mktemp -d "${TMPDIR:-/tmp}/mcpm-skills.XXXXXX")
  local ROOT; ROOT=$(cd "$W" && pwd -P)
  local R=$ROOT/repo H=$ROOT/home
  mkdir -p "$H" "$ROOT/cwd"
  cp -R "$FIX/repos/basic" "$R"
  [ "$overlay" = - ] || cp -R "$FIX/overlays/$overlay/." "$ROOT/"
  mc() {
    (cd "$ROOT/cwd" && env -i HOME="$H" PATH=/usr/bin:/bin TERM=dumb COLUMNS=1000 NO_COLOR=1 \
      PYTHONDONTWRITEBYTECODE=1 "$VENV/bin/python" "$HERE/mcpm_skills.py" "$@")
  }
  eval "$setup" >/dev/null 2>&1
  local args=() raw=("$@") a
  for a in "$@"; do
    a=${a//\{repo\}/$R}; a=${a//\{home\}/$H}; a=${a//\{root\}/$ROOT}
    args+=("$a")
  done
  local text code
  text=$(mc "${args[@]}" 2>&1); code=$?
  local d=$OUT/$name; mkdir -p "$d"
  printf '%s\n' "${raw[@]}" > "$d/args.txt"
  echo "$code" > "$d/exit.txt"
  printf '%s\n' "$text" | sed -E "s#$ROOT#<root>#g; s#[0-9]{8}T[0-9]{6}Z#<stamp>#g" > "$d/output.txt"
  (cd "$ROOT" && find . -type f -not -path './cwd/*' -not -name '*.pyc' -not -path '*/logs/*' -not -path './home/.config/mcpm/auth.json' -not -path './home/.config/mcpm/config.json' | sed -E 's#^\./##; s#[0-9]{8}T[0-9]{6}Z#<stamp>#' | sort) > "$d/tree.txt"
  rm -rf "$W"
}

SYNC='mc sync --path "$R" --client claude-code'
STALE="$SYNC; rm -rf \"\$R/skills/db-helper\""

case_run clean-project             -          ''        -- sync --path '{repo}' --client claude-code
case_run clean-global              -          ''        -- sync --path '{repo}' --client claude-code --global
case_run warn-project              shadow     ''        -- sync --path '{repo}' --client claude-code
case_run warn-project-dry-run      shadow     ''        -- sync --path '{repo}' --client claude-code --dry-run
case_run no-migrate-project        shadow     ''        -- sync --path '{repo}' --client claude-code --no-migrate
case_run migrate-project           shadow     ''        -- sync --path '{repo}' --client claude-code --migrate
case_run migrate-project-dry-run   shadow     ''        -- sync --path '{repo}' --client claude-code --migrate --dry-run
case_run migrate-global            shadow-home ''       -- sync --path '{repo}' --client claude-code --global --migrate
case_run migrate-twice             shadow     "$SYNC --migrate" -- sync --path '{repo}' --client claude-code --migrate
case_run warn-cursor-flat          cursor-flat ''       -- sync --path '{repo}' --client cursor
case_run warnings-aider            -          ''        -- sync --path '{repo}' --client aider
case_run stale-project             -          "$STALE"  -- sync --path '{repo}' --client claude-code
case_run append-agents-md          -          ''        -- sync --path '{repo}' --client agents-md
case_run append-zed                -          ''        -- sync --path '{repo}' --client zed
