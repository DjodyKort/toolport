#!/bin/bash
# Generates the mcpm `agents` reference outputs used by MIG-SKL-8's golden replay.
# Runs the Python reference in a scratch HOME on a copy of a synthetic repository from
# ../repos; nothing outside the scratch directory is touched.
#
#   gen.sh <out-dir>
#
# Per case: args.txt (one argument per line, {repo} {home} {root} placeholders), exit.txt,
# output.txt (stdout and stderr, scratch root shown as <root>), tree.txt (every file left
# behind, relative to the scratch root, logs excluded) and lock-repo.json / lock-global.json
# when a lockfile exists (synced_at replaced). Needs: the mcpm venv (MCPM_VENV, default
# /opt/toolport-plus/scratch/mcpm-venv).
set -u
OUT=${1:?usage: gen.sh <out-dir>}
VENV=${MCPM_VENV:-/opt/toolport-plus/scratch/mcpm-venv}
HERE=$(cd "$(dirname "$0")" && pwd)
REPOS=$HERE/../repos
export LC_ALL=C
rm -rf "$OUT"; mkdir -p "$OUT"

EMPTY_LOCK='{"version": 1, "synced_at": "x", "scope": "", "output_root": "", "skills": {}, "rules": {}, "agents": {}, "styles": {}, "active_styles": {}}'

# case_run <name> <fixture> <setup> <patch> <last-line-only> -- <args...>
case_run() {
  local name=$1 fixture=$2 setup=$3 patch=$4 last=$5; shift 6
  local W; W=$(mktemp -d "${TMPDIR:-/tmp}/mcpm-agents.XXXXXX")
  local ROOT; ROOT=$(cd "$W" && pwd -P)
  local R=$ROOT/repo H=$ROOT/home
  mkdir -p "$H" "$ROOT/cwd"
  cp -R "$REPOS/$fixture" "$R"
  mc() {
    (cd "$ROOT/cwd" && env -i HOME="$H" PATH=/usr/bin:/bin TERM=dumb COLUMNS=1000 NO_COLOR=1 \
      PYTHONDONTWRITEBYTECODE=1 MCPM_PATCH="${MCPM_PATCH:-}" "$VENV/bin/python" "$HERE/mcpm_agents.py" "$@")
  }
  eval "$setup" >/dev/null 2>&1
  local args=() raw=("$@") a
  for a in "$@"; do
    a=${a//\{repo\}/$R}; a=${a//\{home\}/$H}; a=${a//\{root\}/$ROOT}
    args+=("$a")
  done
  local text code
  text=$(MCPM_PATCH=$patch mc "${args[@]}" 2>&1); code=$?
  [ "$last" = last ] && text=$(printf '%s\n' "$text" | tail -n 1)
  local d=$OUT/$name; mkdir -p "$d"
  printf '%s\n' "${raw[@]}" > "$d/args.txt"
  echo "$code" > "$d/exit.txt"
  printf '%s\n' "$text" | sed "s#$ROOT#<root>#g" > "$d/output.txt"
  (cd "$ROOT" && find . -type f -not -path './cwd/*' -not -name '*.pyc' -not -path '*/logs/*' -not -path './home/.config/mcpm/auth.json' -not -path './home/.config/mcpm/config.json' | sed 's#^\./##' | sort) > "$d/tree.txt"
  local lock
  for lock in "repo:$R/mcpm-skills.lock:lock-repo.json" "global:$H/.config/mcpm/mcpm-skills.lock:lock-global.json"; do
    IFS=: read -r _ path file <<<"$lock"
    [ -f "$path" ] && sed -E 's#"synced_at": "[^"]*"#"synced_at": "<ts>"#; s#'"$ROOT"'#<root>#g' "$path" > "$d/$file"
  done
  rm -rf "$W"
}

plain() { case_run "$1" "$2" "$3" "" all -- "${@:4}"; }

SYNC='mc sync --path $R'

plain ls                 basic ''                          ls --path '{repo}'
plain ls-wrap            wrap  ''                          ls --path '{repo}'
plain ls-empty           empty ''                          ls --path '{repo}'
plain ls-norepo          empty ''                          ls --path '{root}/nowhere'

plain lint-warn          basic ''                          lint --path '{repo}'
plain lint-clean         clean ''                          lint --path '{repo}'
plain lint-errors        lint  ''                          lint --path '{repo}'
plain lint-none          empty ''                          lint --path '{repo}'
plain lint-norepo        empty ''                          lint --path '{root}/nowhere'

case_run audit-crash     basic '' ""    last -- audit --path '{repo}'
case_run audit-clean     basic '' audit all  -- audit --path '{repo}'
case_run audit-findings  audit '' audit all  -- audit --path '{repo}'
case_run audit-medium    audit-medium '' audit all -- audit --path '{repo}'
case_run audit-none      empty '' audit all  -- audit --path '{repo}'
case_run audit-norepo    empty '' audit all  -- audit --path '{root}/nowhere'

plain diff-nolock        basic ''                          diff --path '{repo}'
plain diff-clean         basic "$SYNC"                     diff --path '{repo}'
plain diff-changes       basic "$SYNC; echo changed >> \$R/agents/helper/AGENT.md; mkdir \$R/agents/newone; printf -- '---\nname: newone\ndescription: A new agent for testing the diff output\n---\nbody\n' > \$R/agents/newone/AGENT.md; rm -r \$R/agents/reviewer" diff --path '{repo}'
plain diff-norepo        empty ''                          diff --path '{root}/nowhere'

plain status-nolock      basic ''                          status --path '{repo}'
plain status-strict-nolock basic ''                        status --strict --path '{repo}'
plain status-ok          basic "$SYNC"                     status --path '{repo}'
plain status-drift       basic "$SYNC; rm \$R/.claude/agents/planner.md" status --path '{repo}'
plain status-strict-drift basic "$SYNC; rm \$R/.claude/agents/planner.md" status --strict --path '{repo}'
plain status-strict-ok   basic "$SYNC"                     status --strict --path '{repo}'
plain status-noagents    basic "printf '%s' '$EMPTY_LOCK' > \$R/mcpm-skills.lock" status --path '{repo}'

plain sync-project       basic ''                          sync --path '{repo}'
plain sync-dry-run       basic ''                          sync --dry-run --path '{repo}'
plain sync-client        basic ''                          sync --client claude-code --path '{repo}'
plain sync-global        basic ''                          sync --global --path '{repo}'
plain sync-global-dry-run basic ''                         sync --global --dry-run --path '{repo}'
plain sync-none          empty ''                          sync --path '{repo}'
plain sync-norepo        empty ''                          sync --path '{root}/nowhere'
plain sync-stale         basic "$SYNC; rm -r \$R/agents/reviewer" sync --path '{repo}'
plain sync-clean-repo    clean ''                          sync --path '{repo}'

plain clean-nolock       basic ''                          clean --path '{repo}'
plain clean-project      basic "$SYNC"                     clean --path '{repo}'
plain clean-client       basic "$SYNC"                     clean --client claude-code --path '{repo}'
plain clean-unknown-client basic "$SYNC"                   clean --client nosuch --path '{repo}'
plain clean-global       basic 'mc sync --global --path $R' clean --global
plain clean-noagents     basic "printf '%s' '$EMPTY_LOCK' > \$R/mcpm-skills.lock" clean --path '{repo}'
plain clean-twice        basic "$SYNC; mc clean --path \$R" clean --path '{repo}'

case_run uninstall       basic "$SYNC" uninstall all -- uninstall helper --path '{repo}'
case_run uninstall-nolock basic '' uninstall all -- uninstall helper --path '{repo}'
case_run uninstall-missing basic "$SYNC" uninstall all -- uninstall nope --path '{repo}'
case_run uninstall-crash basic "$SYNC" "" last -- uninstall helper --path '{repo}'

plain add                empty ''                          add code-reviewer --path '{repo}'
plain add-exists         basic ''                          add helper --path '{repo}'
plain add-invalid        basic ''                          add Bad_Name --path '{repo}'
plain add-double-dash    basic ''                          add a--b --path '{repo}'
plain add-fresh-path     empty ''                          add newagent --path '{root}/fresh'
echo "generated $(ls "$OUT" | wc -l) cases in $OUT"
