#!/bin/bash
# Generates the mcpm reference outputs used by MIG-PRF-1's golden replay (profile ls/create/edit/rm,
# client edit/import). Runs the Python reference (mcpm profile/client commands) against a synthetic
# config root in a scratch HOME; nothing real is read or contacted.
#
#   gen.sh <out-dir>
#
# Needs: the mcpm venv (MCPM_VENV, default /opt/toolport-plus/scratch/mcpm-venv), python3.
set -u
OUT=${1:?usage: gen.sh <out-dir>}
VENV=${MCPM_VENV:-/opt/toolport-plus/scratch/mcpm-venv}
W=$(mktemp -d "${TMPDIR:-/tmp}/mcpm-prf-gen.XXXXXX")
trap 'rm -rf "$W"' EXIT
rm -rf "$OUT"; mkdir -p "$OUT"

cat > "$W/import_driver.py" <<'PY'
import sys
from unittest import mock

from mcpm.commands import client as c


class Prompt:
    def __init__(self, value):
        self.value = value

    def execute(self):
        return self.value


client, select, create, profile = sys.argv[1], sys.argv[2], sys.argv[3] == "1", sys.argv[4]
chosen = [s for s in select.split(",") if s]
answers = iter([create, False, False])
with mock.patch.object(c.inquirer, "checkbox", lambda **kw: Prompt(chosen)), \
     mock.patch.object(c.inquirer, "confirm", lambda **kw: Prompt(next(answers))), \
     mock.patch.object(c.inquirer, "text", lambda **kw: Prompt(profile)):
    c.import_client.main(args=[client], standalone_mode=False)
PY

# mcpm <args...>: the mcpm CLI in the scratch HOME.
mcpm() {
  env -i HOME="$H" PATH=/usr/bin:/bin TERM=dumb COLUMNS=1000 NO_COLOR=1 PYTHONDONTWRITEBYTECODE=1 \
    "$VENV/bin/mcpm" "$@"
}

driver() {
  env -i HOME="$H" PATH=/usr/bin:/bin TERM=dumb COLUMNS=1000 NO_COLOR=1 PYTHONDONTWRITEBYTECODE=1 \
    "$VENV/bin/python" "$W/import_driver.py" "$@"
}

seed() {
  mkdir -p "$H/.config/mcpm" "$H/.cursor"
  cat > "$H/.config/mcpm/servers.json" <<'EOF'
{
  "alpha": {"name": "alpha", "command": "npx", "args": ["-y", "alpha-mcp"], "env": {}, "profile_tags": ["work"]},
  "beta": {"name": "beta", "command": "uvx", "args": ["beta-mcp", "--flag"], "env": {}, "profile_tags": ["work", "play"]},
  "gamma": {"name": "gamma", "url": "https://gamma.example.com/mcp", "headers": {}, "profile_tags": []}
}
EOF
  cat > "$H/.config/mcpm/profiles_metadata.json" <<'EOF'
{
  "work": {"name": "work", "api_key": null, "description": null},
  "play": {"name": "play", "api_key": null, "description": null},
  "empty": {"name": "empty", "api_key": null, "description": null}
}
EOF
}

cursor_scoped() {
  cat > "$H/.cursor/mcp.json" <<'EOF'
{"mcpServers": {"mcpm_profile_work": {"command": "mcpm", "args": ["profile", "run", "work"]}, "direct1": {"command": "npx", "args": ["-y", "direct-mcp"]}}}
EOF
}

cursor_bare() {
  cat > "$H/.cursor/mcp.json" <<'EOF'
{"mcpServers": {"direct1": {"command": "npx", "args": ["-y", "direct-mcp"]}}}
EOF
}

cursor_direct() {
  cat > "$H/.cursor/mcp.json" <<'EOF'
{"mcpServers": {
  "direct1": {"command": "npx", "args": ["-y", "direct-mcp"]},
  "direct2": {"command": "uvx", "args": ["tool-mcp", "--option", "value-that-is-long-enough-to-clip"]},
  "alpha": {"command": "npx", "args": ["-y", "alpha-mcp"]}
}}
EOF
}

# case <name> <setup,...|-> -- <command> <args...>   (command: mcpm | import)
case_run() {
  local name=$1 setups=$2; shift 3
  H="$W/$name/home"
  mkdir -p "$H"
  local s
  for s in ${setups//,/ }; do
    case $s in
      -) ;;
      seed) seed ;;
      empty) mkdir -p "$H/.config/mcpm" ;;
      scoped) cursor_scoped ;;
      bare) cursor_bare ;;
      direct) cursor_direct ;;
      noconfig) rm -f "$H/.cursor/mcp.json" ;;
      *) echo "unknown setup $s" >&2; exit 2 ;;
    esac
  done
  local o="$OUT/$name" kind=$1
  shift
  mkdir -p "$o"
  case $kind in
    mcpm) mcpm "$@" > "$o/output.txt" 2>&1 ;;
    import) driver "$@" > "$o/output.txt" 2>&1 ;;
  esac
  echo "[exit $?]" >> "$o/output.txt"
  if [ "$kind" = mcpm ]; then printf '%s\n' "$*" > "$o/args.txt"; else printf 'client import %s\n' "$*" > "$o/args.txt"; fi
  sed -i -e "s#$H#<HOME>#g" -e "s#$W#<TMP>#g" \
    -e '/Trae is not supported on Linux yet/d' -e 's/[[:space:]]*$//' "$o/output.txt"
  return 0
}

c() { case_run "$@"; }

c profile-ls                 seed,scoped    -- mcpm profile ls
c profile-ls-verbose         seed,scoped    -- mcpm profile ls --verbose
c profile-ls-empty           empty          -- mcpm profile ls
c profile-create             seed,scoped    -- mcpm profile create demo
c profile-create-exists      seed,scoped    -- mcpm profile create work
c profile-create-force       seed,scoped    -- mcpm profile create work --force
c profile-edit-name          seed,scoped    -- mcpm profile edit work --name job
c profile-edit-servers       seed,scoped    -- mcpm profile edit work --servers beta,gamma
c profile-edit-set-servers   seed,scoped    -- mcpm profile edit work --set-servers gamma
c profile-edit-add           seed,scoped    -- mcpm profile edit work --add-server gamma
c profile-edit-remove        seed,scoped    -- mcpm profile edit work --remove-server alpha
c profile-edit-name-servers  seed,scoped    -- mcpm profile edit play --name fun --add-server alpha
c profile-edit-remove-absent seed,scoped    -- mcpm profile edit work --remove-server gamma
c profile-edit-unknown-server seed,scoped   -- mcpm profile edit work --add-server ghost
c profile-edit-multiple      seed,scoped    -- mcpm profile edit work --servers alpha --add-server beta
c profile-edit-not-found     seed,scoped    -- mcpm profile edit ghost --name x
c profile-edit-no-change     seed,scoped    -- mcpm profile edit work
c profile-edit-same-set      seed,scoped    -- mcpm profile edit work --servers alpha,beta
c profile-edit-name-taken    seed,scoped    -- mcpm profile edit work --name play
c profile-rm                 seed,scoped    -- mcpm profile rm work --force
c profile-rm-no-entry        seed,scoped    -- mcpm profile rm play --force
c profile-rm-empty           seed,scoped    -- mcpm profile rm empty --force
c profile-rm-no-clients      seed,scoped    -- mcpm profile rm work --force --no-clients
c profile-rm-not-found       seed,scoped    -- mcpm profile rm ghost --force
c client-edit-set            seed,scoped    -- mcpm client edit cursor --set-profiles play
c client-edit-remove         seed,scoped    -- mcpm client edit cursor --remove-profile work
c client-edit-clear          seed,scoped    -- mcpm client edit cursor --set-profiles ''
c client-edit-add-bare       seed,bare      -- mcpm client edit cursor --add-profile play
c client-edit-set-bare       seed,bare      -- mcpm client edit cursor --set-profiles work
c client-edit-add-same       seed,scoped    -- mcpm client edit cursor --add-profile work
c client-edit-none           seed,scoped    -- mcpm client edit cursor
c client-edit-not-in-client  seed,scoped    -- mcpm client edit cursor --remove-profile play
c client-edit-unknown-profile seed,scoped   -- mcpm client edit cursor --add-profile ghost
c client-edit-unknown-client seed,scoped    -- mcpm client edit ghostclient --add-profile play
c client-edit-multiple       seed,scoped    -- mcpm client edit cursor --add-profile play --remove-profile work
c client-add-two             seed,scoped    -- mcpm client edit cursor --add-profile play
c client-import-preview      seed,direct    -- import cursor "" 0 cursor
c client-import-select        seed,direct   -- import cursor direct1,direct2 0 cursor
c client-import-profile       seed,direct   -- import cursor direct1,direct2 1 cursor
c client-import-no-config    seed,noconfig  -- mcpm client import cursor

echo "generated $(ls "$OUT" | wc -l) cases into $OUT"
