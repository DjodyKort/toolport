#!/bin/bash
# Generates the mcpm tap, search and install reference outputs used by MIG-SKL-10's golden replay.
# Runs the Python reference (`mcpm sync tap|search|install`) in a scratch HOME against local bare
# repositories built from the synthetic trees in ../repos; a `url.<local>.insteadOf` rule in the
# scratch ~/.gitconfig points the GitHub URLs a `user/repo` tap expands to at them, so there is no
# network. Nothing outside the scratch directory is touched.
#
#   gen.sh <out-dir>
#
# Per case: args.txt (one argument per line, {root} {home} {target} placeholders), exit.txt,
# output.txt (stdout and stderr, scratch root shown as <root>), tree.txt (every file left behind,
# relative to the scratch root, without .git directories, the git config and the remotes) and
# index.json (the tap index, scratch root replaced) when it exists. Needs: the mcpm venv
# (MCPM_VENV, default /opt/toolport-plus/scratch/mcpm-venv) and git.
set -u
OUT=${1:?usage: gen.sh <out-dir>}
VENV=${MCPM_VENV:-/opt/toolport-plus/scratch/mcpm-venv}
HERE=$(cd "$(dirname "$0")" && pwd)
REPOS=$HERE/../repos
export LC_ALL=C
rm -rf "$OUT"; mkdir -p "$OUT"

GIT=(git -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false -c init.defaultBranch=main)

# publish <owner/repo>: a bare remote at $ROOT/_infra/remotes/<owner>/<repo>.git
publish() {
  local work=$ROOT/_infra/work/$1 remote=$ROOT/_infra/remotes/$1.git
  mkdir -p "$(dirname "$work")" "$(dirname "$remote")"
  cp -R "$REPOS/$1" "$work"
  GIT_CONFIG_GLOBAL=/dev/null "${GIT[@]}" -C "$work" init -q
  GIT_CONFIG_GLOBAL=/dev/null "${GIT[@]}" -C "$work" add -A
  GIT_CONFIG_GLOBAL=/dev/null "${GIT[@]}" -C "$work" commit -q -m init
  GIT_CONFIG_GLOBAL=/dev/null "${GIT[@]}" -C "$work" clone -q --bare . "$remote"
}

# case_run <name> <setup> -- <args...>
case_run() {
  local name=$1 setup=$2; shift 3
  local W; W=$(mktemp -d "${TMPDIR:-/tmp}/mcpm-taps.XXXXXX")
  local ROOT; ROOT=$(cd "$W" && pwd -P)
  local H=$ROOT/home T=$ROOT/target
  mkdir -p "$H" "$ROOT/cwd"
  local r
  for r in acme/skills acme/audit acme/extras acme/empty acme/mild; do publish "$r"; done
  printf '[url "file://%s/_infra/remotes/"]\n\tinsteadOf = https://github.com/\n' "$ROOT" > "$H/.gitconfig"
  mc() {
    (cd "$ROOT/cwd" && env -i HOME="$H" PATH=/usr/bin:/bin TERM=dumb COLUMNS=1000 NO_COLOR=1 \
      PYTHONDONTWRITEBYTECODE=1 "$VENV/bin/python" "$HERE/mcpm_taps.py" "$@")
  }
  eval "$setup" >/dev/null 2>&1
  local args=() raw=("$@") a
  for a in "$@"; do
    a=${a//\{root\}/$ROOT}; a=${a//\{home\}/$H}; a=${a//\{target\}/$T}
    args+=("$a")
  done
  local text code
  text=$(mc "${args[@]}" 2>&1); code=$?
  local d=$OUT/$name; mkdir -p "$d"
  printf '%s\n' "${raw[@]}" > "$d/args.txt"
  echo "$code" > "$d/exit.txt"
  printf '%s\n' "$text" | sed "s#$ROOT#<root>#g" > "$d/output.txt"
  (cd "$ROOT" && find . -type f -not -path './cwd/*' -not -path './_infra/*' -not -path '*/.git/*' \
    -not -name '.gitconfig' -not -name '*.pyc' -not -path '*/logs/*' -not -path './home/.config/mcpm/auth.json' \
    -not -path './home/.config/mcpm/config.json' | sed 's#^\./##' | sort) > "$d/tree.txt"
  local index=$H/.config/mcpm/taps_index.json
  [ -f "$index" ] && sed "s#$ROOT#<root>#g" "$index" > "$d/index.json"
  rm -rf "$W"
}

ADD='mc tap add acme/skills'
BOTH='mc tap add acme/skills; mc tap add acme/extras --name extra'
INSTALLED='mc tap add acme/skills; mc install @acme/skills --path $T'
upstream() {
  local work=$ROOT/_infra/work/acme/skills
  mkdir -p "$work/skills/new-one"
  printf -- '---\nname: new-one\ndescription: Newly added\n---\nbody\n' > "$work/skills/new-one/SKILL.md"
  GIT_CONFIG_GLOBAL=/dev/null "${GIT[@]}" -C "$work" add -A
  GIT_CONFIG_GLOBAL=/dev/null "${GIT[@]}" -C "$work" commit -q -m two
  GIT_CONFIG_GLOBAL=/dev/null "${GIT[@]}" -C "$work" push -q "$ROOT/_infra/remotes/acme/skills.git" main
}

case_run tap-ls-empty            ''                          -- tap ls
case_run tap-add                 ''                          -- tap add acme/skills
case_run tap-add-name            ''                          -- tap add acme/skills --name team
case_run tap-add-duplicate       "$ADD"                      -- tap add acme/skills
case_run tap-add-missing-remote  ''                          -- tap add acme/nonexistent
case_run tap-ls                  "$BOTH"                     -- tap ls
case_run tap-remove              "$ADD"                      -- tap remove acme-skills
case_run tap-remove-missing      ''                          -- tap remove nosuch
case_run tap-update-none         ''                          -- tap update
case_run tap-update              "$BOTH"                     -- tap update
case_run tap-update-one          "$BOTH"                     -- tap update extra
case_run tap-update-pulls        "$ADD; upstream"            -- tap update
case_run tap-update-unknown      "$BOTH"                     -- tap update nosuch
case_run tap-update-failed       "$ADD; rm -rf \$ROOT/_infra/remotes/acme/skills.git" -- tap update

case_run search                  "$BOTH"                     -- search review
case_run search-tag              "$ADD"                      -- search quality
case_run search-case             "$ADD"                      -- search TERRAFORM
case_run search-none             "$ADD"                      -- search nomatch
case_run search-no-taps          ''                          -- search review
case_run search-after-update     "$ADD; upstream; mc tap update" -- search newly

case_run install-all             "$ADD"                      -- install @acme/skills --path '{target}'
case_run install-one             "$ADD"                      -- install @acme/skills/code-review --path '{target}'
case_run install-rule            "$ADD"                      -- install @acme/skills/style-guide --path '{target}'
case_run install-autotap         ''                          -- install @acme/skills --path '{target}'
case_run install-twice           "$INSTALLED"                -- install @acme/skills --path '{target}'
case_run install-missing-skill   "$ADD"                      -- install @acme/skills/nope --path '{target}'
case_run install-bad-spec        ''                          -- install acme --path '{target}'
case_run install-version         "$ADD"                      -- install @acme/skills/code-review@1.2.0 --path '{target}'
case_run install-audit-blocked   ''                          -- install @acme/audit --path '{target}'
case_run install-no-audit        ''                          -- install @acme/audit --no-audit --path '{target}'
case_run install-audit-medium    ''                          -- install @acme/mild --path '{target}'
case_run install-empty-tap       ''                          -- install @acme/empty --path '{target}'
case_run install-after-update    "$ADD; upstream; mc tap update" -- install @acme/skills/new-one --path '{target}'
echo "generated $(ls "$OUT" | wc -l) cases in $OUT"
