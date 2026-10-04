#!/bin/bash
# Generates the mcpm `styles` reference outputs used by MIG-SKL-9's golden replay.
# Runs the Python reference in a scratch HOME on a copy of a synthetic repository from
# ../repos; nothing outside the scratch directory is touched.
#
#   gen.sh <out-dir>
#
# Per case: args.txt (one argument per line, {repo} {home} {root} placeholders), exit.txt,
# output.txt (stdout and stderr, scratch root shown as <root>), tree.txt (every file left
# behind, relative to the scratch root, logs excluded), files.txt (the content of every file
# that is new or changed compared with the repository fixture) and lock-repo.json when a
# lockfile exists (synced_at replaced). Needs: the mcpm venv (MCPM_VENV, default
# /opt/toolport-plus/scratch/mcpm-venv).
# The committed lock-repo.json files are reformatted with prettier; the replay compares parsed JSON.
set -u
OUT=${1:?usage: gen.sh <out-dir>}
VENV=${MCPM_VENV:-/opt/toolport-plus/scratch/mcpm-venv}
HERE=$(cd "$(dirname "$0")" && pwd)
REPOS=$HERE/../repos
export LC_ALL=C
rm -rf "$OUT"; mkdir -p "$OUT"

EMPTY_LOCK='{"version": 1, "synced_at": "x", "scope": "", "output_root": "", "skills": {}, "rules": {}, "agents": {}, "styles": {}, "active_styles": {}}'
FOREIGN_MODES='{"customModes": [{"slug": "reviewer", "name": "Reviewer", "roleDefinition": "Reviews", "groups": ["read"]}, {"slug": "style-old", "name": "Old", "roleDefinition": "Old style", "customInstructions": "x", "groups": ["read"]}]}'

# case_run <name> <fixture> <setup> -- <args...>
case_run() {
  local name=$1 fixture=$2 setup=$3; shift 4
  local W; W=$(mktemp -d "${TMPDIR:-/tmp}/mcpm-styles.XXXXXX")
  local ROOT; ROOT=$(cd "$W" && pwd -P)
  local R=$ROOT/repo H=$ROOT/home
  mkdir -p "$H" "$ROOT/cwd"
  cp -R "$REPOS/$fixture" "$R"
  mc() {
    (cd "$ROOT/cwd" && env -i HOME="$H" PATH=/usr/bin:/bin TERM=dumb COLUMNS=1000 NO_COLOR=1 \
      PYTHONDONTWRITEBYTECODE=1 "$VENV/bin/python" "$HERE/mcpm_styles.py" "$@")
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
  printf '%s\n' "$text" | sed "s#$ROOT#<root>#g" > "$d/output.txt"
  local tree
  tree=$(cd "$ROOT" && find . -type f -not -path './cwd/*' -not -name '*.pyc' -not -path '*/logs/*' -not -path './home/.config/mcpm/auth.json' -not -path './home/.config/mcpm/config.json' | sed 's#^\./##' | sort)
  printf '%s\n' "$tree" > "$d/tree.txt"
  local rel
  : > "$d/files.txt"
  while IFS= read -r rel; do
    case $rel in *mcpm-skills.lock) continue ;; esac
    if [[ $rel == repo/* ]] && cmp -s "$ROOT/$rel" "$REPOS/$fixture/${rel#repo/}"; then continue; fi
    printf '=== %s ===\n' "$rel" >> "$d/files.txt"
    cat "$ROOT/$rel" >> "$d/files.txt"
    printf '\n' >> "$d/files.txt"
  done <<<"$tree"
  [ -s "$d/files.txt" ] || rm "$d/files.txt"
  [ -f "$R/mcpm-skills.lock" ] && sed -E 's#"synced_at": "[^"]*"#"synced_at": "<ts>"#; s#'"$ROOT"'#<root>#g' "$R/mcpm-skills.lock" > "$d/lock-repo.json"
  rm -rf "$W"
}

plain() { case_run "$1" "$2" "$3" -- "${@:4}"; }

SYNC='mc sync --path $R'
APPLY='mc apply concise --path $R'
ROOM='printf "%s" "$FOREIGN_MODES" > $R/.roomodes'

plain ls                  basic   ''                                      ls --path '{repo}'
plain ls-empty            empty   ''                                      ls --path '{repo}'
plain ls-norepo           empty   ''                                      ls --path '{root}/nowhere'
plain ls-synced           basic   "$SYNC"                                 ls --path '{repo}'
plain ls-applied          basic   "$SYNC; mc apply teacher --path \$R"    ls --path '{repo}'
plain ls-partial          partial ''                                      ls --path '{repo}'
plain ls-subdir           basic   ''                                      ls --path '{repo}/styles/concise'

plain lint-warn           lint    ''                                      lint --path '{repo}'
plain lint-clean          basic   ''                                      lint --path '{repo}'
plain lint-none           empty   ''                                      lint --path '{repo}'
plain lint-norepo         empty   ''                                      lint --path '{root}/nowhere'
plain lint-partial        partial ''                                      lint --path '{repo}'

plain diff-nolock         basic   ''                                      diff --path '{repo}'
plain diff-clean          basic   "$SYNC"                                 diff --path '{repo}'
plain diff-changes        basic   "$SYNC; echo changed >> \$R/styles/concise/STYLE.md; mkdir \$R/styles/newone; printf -- '---\nname: newone\ndescription: A new style for testing the diff output\n---\nbody\n' > \$R/styles/newone/STYLE.md; rm -r \$R/styles/terse" diff --path '{repo}'
plain diff-applied        basic   "$APPLY"                                diff --path '{repo}'
plain diff-nolock-none    empty   ''                                      diff --path '{repo}'
plain diff-norepo         empty   ''                                      diff --path '{root}/nowhere'

plain status-nolock       basic   ''                                      status --path '{repo}'
plain status-synced       basic   "$SYNC"                                 status --path '{repo}'
plain status-applied      basic   "$APPLY"                                status --path '{repo}'
plain status-both         basic   "$SYNC; mc apply teacher --client cursor --path \$R" status --path '{repo}'
plain status-norepo       empty   ''                                      status --path '{root}/nowhere'

plain sync-project        basic   ''                                      sync --path '{repo}'
plain sync-dry-run        basic   ''                                      sync --dry-run --path '{repo}'
plain sync-client         basic   ''                                      sync --client claude-code --path '{repo}'
plain sync-client-roo     basic   ''                                      sync --client roomodes-style --path '{repo}'
plain sync-client-tier2   basic   ''                                      sync --client cursor --path '{repo}'
plain sync-none           empty   ''                                      sync --path '{repo}'
plain sync-norepo         empty   ''                                      sync --path '{root}/nowhere'
plain sync-twice          basic   "$SYNC"                                 sync --path '{repo}'
plain sync-roomodes-merge basic   "$ROOM"                                 sync --path '{repo}'
plain sync-after-apply    basic   "$APPLY"                                sync --path '{repo}'
plain sync-stale          basic   "$SYNC; rm -r \$R/styles/terse"         sync --path '{repo}'
plain sync-partial        partial ''                                      sync --path '{repo}'
plain sync-dry-run-roomodes basic "$ROOM"                                 sync --dry-run --path '{repo}'

plain apply               basic   ''                                      apply concise --path '{repo}'
plain apply-dry-run       basic   ''                                      apply concise --dry-run --path '{repo}'
plain apply-client        basic   ''                                      apply concise --client cursor --path '{repo}'
plain apply-tier1-client  basic   ''                                      apply concise --client claude-code --path '{repo}'
plain apply-unknown-client basic  ''                                      apply concise --client nosuch --path '{repo}'
plain apply-unknown-style basic   ''                                      apply nope --path '{repo}'
plain apply-nostyles      empty   ''                                      apply concise --path '{repo}'
plain apply-replace       basic   "$APPLY"                                apply teacher --path '{repo}'
plain apply-replace-client basic  "$APPLY"                                apply teacher --client cursor --path '{repo}'
plain apply-same-twice    basic   "$APPLY"                                apply concise --path '{repo}'
plain apply-dry-run-replace basic "$APPLY"                                apply teacher --dry-run --path '{repo}'
plain apply-zed-existing  basic   "printf 'Be kind.\n' > \$R/.rules"      apply concise --client zed --path '{repo}'
plain apply-zed-twice     basic   "mc apply concise --client zed --path \$R" apply teacher --client zed --path '{repo}'
plain apply-huge          lint    ''                                      apply huge --client windsurf --path '{repo}'
plain apply-after-sync    basic   "$SYNC"                                 apply concise --path '{repo}'
plain apply-norepo        empty   ''                                      apply concise --path '{root}/nowhere'

plain remove              basic   "$APPLY"                                remove --path '{repo}'
plain remove-dry-run      basic   "$APPLY"                                remove --dry-run --path '{repo}'
plain remove-client       basic   "$APPLY"                                remove --client cursor --path '{repo}'
plain remove-noactive     basic   "$SYNC"                                 remove --path '{repo}'
plain remove-nolock       basic   ''                                      remove --path '{repo}'
plain remove-client-none  basic   "mc apply concise --client aider --path \$R" remove --client cursor --path '{repo}'
plain remove-unknown-client basic "$APPLY"                                remove --client nosuch --path '{repo}'
plain remove-zed-kept     basic   "printf 'Be kind.\n' > \$R/.rules; mc apply concise --client zed --path \$R" remove --client zed --path '{repo}'
plain remove-zed-only     basic   "mc apply concise --client zed --path \$R" remove --client zed --path '{repo}'
plain remove-twice        basic   "$APPLY; mc remove --path \$R"         remove --path '{repo}'
plain remove-norepo       empty   ''                                      remove --path '{root}/nowhere'

plain clean-both          basic   "$SYNC; $APPLY"                         clean --path '{repo}'
plain clean-synced        basic   "$SYNC"                                 clean --path '{repo}'
plain clean-applied       basic   "$APPLY"                                clean --path '{repo}'
plain clean-nolock        basic   ''                                      clean --path '{repo}'
plain clean-empty-lock    basic   "printf '%s' '$EMPTY_LOCK' > \$R/mcpm-skills.lock" clean --path '{repo}'
plain clean-twice         basic   "$SYNC; mc clean --path \$R"           clean --path '{repo}'
plain clean-zed-nolock    basic   "mc apply concise --client zed --path \$R; rm \$R/mcpm-skills.lock" clean --path '{repo}'
plain clean-roomodes-foreign basic "$ROOM; $SYNC"                         clean --path '{repo}'
plain clean-roomodes-only basic   "$SYNC; printf '%s' '{\"customModes\": []}' > \$R/.roomodes" clean --path '{repo}'
plain clean-norepo        empty   ''                                      clean --path '{root}/nowhere'

plain add                 empty   ''                                      add concise-engineer --path '{repo}'
plain add-exists          basic   ''                                      add concise --path '{repo}'
plain add-invalid         basic   ''                                      add Bad_Name --path '{repo}'
plain add-double-dash     basic   ''                                      add a--b --path '{repo}'
plain add-subdir          basic   ''                                      add fresh --path '{repo}/styles/concise'
plain add-norepo          empty   ''                                      add fresh --path '{root}/nowhere'
plain add-then-ls         basic   'mc add fresh --path $R'                ls --path '{repo}'
echo "generated $(ls "$OUT" | wc -l) cases in $OUT"
