#!/bin/bash
# Generates the mcpm compression reference outputs used by MIG-CMP-3's golden replay.
# Runs the Python reference (mcpm-compression) in a scratch HOME with stub `headroom` and `uv`
# binaries and a stub /health server; nothing real is started or contacted.
#
#   gen.sh <out-dir>
#
# Needs: the mcpm venv (MCPM_VENV, default /opt/toolport-plus/scratch/mcpm-venv), python3.
set -u
OUT=${1:?usage: gen.sh <out-dir>}
VENV=${MCPM_VENV:-/opt/toolport-plus/scratch/mcpm-venv}
HERE=$(cd "$(dirname "$0")" && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/mcpm-gen.XXXXXX")
PROXY_PORT=18787
SERVER_PID=""
trap '[ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null; rm -rf "$W"' EXIT
rm -rf "$OUT"; mkdir -p "$OUT"

mkdir -p "$W/bin"
cat > "$W/bin/headroom" <<'EOF'
#!/bin/sh
v=$(cat "$HEADROOM_STATE/version" 2>/dev/null || echo 0.29.0)
case "$1" in
  --version) echo "headroom $v" ;;
  agent-savings)
    profile=$3
    case "$profile" in
      agent-90)
        if [ "$v" = "0.29.0" ]; then
          echo '{"HEADROOM_MODE": "token", "HEADROOM_SAVINGS_PROFILE": "agent-90", "HEADROOM_MAX_ITEMS": "30", "HEADROOM_FORCE_KOMPRESS": "1"}'
        else
          echo '{"HEADROOM_MODE": "token", "HEADROOM_SAVINGS_PROFILE": "agent-90", "HEADROOM_MAX_ITEMS": "40", "HEADROOM_NEW_KNOB": "1"}'
        fi ;;
      balanced) echo '{"HEADROOM_MODE": "token", "HEADROOM_SAVINGS_PROFILE": "balanced", "HEADROOM_MAX_ITEMS": "60"}' ;;
      *) echo "unknown profile $profile" >&2; exit 2 ;;
    esac ;;
  mcp) echo "removed headroom mcp registration" ;;
  unwrap) echo "unwrapped $2" ;;
  *) echo "stub headroom: $*" ;;
esac
EOF
cat > "$W/bin/uv" <<'EOF'
#!/bin/sh
req=$3
echo "${req##*==}" > "$HEADROOM_STATE/version"
echo "Installed 1 executable: headroom"
EOF
chmod +x "$W/bin/headroom" "$W/bin/uv"

cat > "$W/health.py" <<'EOF'
import http.server, json, sys
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = json.dumps({"ready": True, "status": "healthy", "version": "0.29.0", "config": {
            "max_items_after_crush": 30, "accuracy_guard": True, "protect_recent": None,
            "compress_user_messages": False}}).encode()
        self.send_response(200); self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def log_message(self, *a): pass
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
EOF
python3 "$W/health.py" $PROXY_PORT & SERVER_PID=$!
sleep 1

LEGACY='{
  "provider": "rtk-only",
  "runtime": "hook",
  "presets": {"agent": {"mode": "token", "savings_profile": "agent-90",
    "env": {"HEADROOM_MODE": "token"}, "code_aware": false, "port": 8788}},
  "active_preset": "agent"
}'

mcpm() {
  env -i HOME="$H" PATH="$PB" TERM=dumb COLUMNS=1000 NO_COLOR=1 PYTHONDONTWRITEBYTECODE=1 \
    HEADROOM_STATE="$H/.state" "$VENV/bin/python" -c \
    "from mcpm_compression.cli import compression; compression()" "$@"
}

# case <name> <setup,setup,...|-> -- <args...>
case_run() {
  local name=$1 setups=$2; shift 3
  H="$W/$name/home"; PB="$W/bin:/usr/bin:/bin"
  mkdir -p "$H/.config/mcpm" "$H/.state"
  echo 0.29.0 > "$H/.state/version"
  local s
  for s in ${setups//,/ }; do
    case $s in
      -) ;;
      headroom) mcpm enable >/dev/null 2>&1 ;;
      headroom-p) mcpm enable --port $PROXY_PORT >/dev/null 2>&1 ;;
      agent) mcpm use agent >/dev/null 2>&1 ;;
      rtk) mcpm enable --provider rtk-only >/dev/null 2>&1 ;;
      drift) echo 0.30.1 > "$H/.state/version" ;;
      nohr) PB="/usr/bin:/bin" ;;
      nouv) PB="$W/bin-nouv:/usr/bin:/bin"; mkdir -p "$W/bin-nouv"; cp "$W/bin/headroom" "$W/bin-nouv/" ;;
      legacy) printf '%s\n' "$LEGACY" > "$H/.config/mcpm/compression.json" ;;
      plist) mkdir -p "$H/Library/LaunchAgents"; echo '<plist/>' > "$H/Library/LaunchAgents/sh.mcpm.compression.proxy.plist" ;;
      sealed) mcpm seal --apply >/dev/null 2>&1 ;;
      rule) python3 - "$H/.config/mcpm/compression.json" <<'PY'
import json, sys
p = sys.argv[1]; c = json.load(open(p))
c["contexts"] = [{"pattern": "*/plain/*", "provider": "none"}, {"pattern": "*/agent/*", "preset": "agent"}]
open(p, "w").write(json.dumps(c, indent=2) + "\n")
PY
      ;;
      *) echo "unknown setup $s" >&2; exit 2 ;;
    esac
  done
  local o="$OUT/$name"
  mkdir -p "$o"
  mcpm "$@" > "$o/output.txt" 2>&1
  echo "[exit $?]" >> "$o/output.txt"
  printf '%s\n' "$*" > "$o/args.txt"
  local f
  local files="compression.json"
  case $name in enable-headroom|enable-headroom-opts|use-agent) files="$files compression-shims.zsh compression-env.sh" ;; esac
  for f in $files; do
    [ -f "$H/.config/mcpm/$f" ] && cp "$H/.config/mcpm/$f" "$o/$f"
  done
  [ -f "$H/Library/LaunchAgents/sh.mcpm.compression.proxy.plist" ] && : > "$o/plist.present"
  sed -i -e "s#$H/.config/mcpm#<DATA>#g" -e "s#$H#<HOME>#g" -e "s#$W#<TMP>#g" \
    -e '/^Trae is not supported on Linux yet\.$/d' -e 's/[[:space:]]*$//' "$o"/*.txt
  return 0
}

c() { case_run "$@"; }

c enable-headroom         -               -- enable
c enable-headroom-opts    -               -- enable --preset agent --mode cache --port 18788 --telemetry on
c enable-rtk-only         -               -- enable --provider rtk-only
c enable-none             -               -- enable --provider none
c enable-no-engine        nohr            -- enable
c enable-unknown-preset   -               -- enable --preset ghost
c disable                 headroom        -- disable
c disable-teardown        headroom        -- disable --teardown
c set-provider-rtk        headroom        -- set-provider rtk-only
c set-provider-headroom   -               -- set-provider headroom
c use-agent               headroom        -- use agent
c use-unknown             headroom        -- use ghost
c sync                    headroom        -- sync
c sync-fresh              -               -- sync
c sync-legacy             legacy          -- sync
c sync-plist              headroom,plist  -- sync
c env-headroom            headroom        -- env --cwd /work/app
c env-agent               headroom,agent  -- env --cwd /work/app
c env-routed-plain        headroom,rule   -- env --cwd /work/plain/app
c env-rtk                 rtk             -- env --cwd /work/app
c env-fresh               -               -- env --cwd /work/app
c pin                     headroom        -- pin
c pin-set                 headroom        -- pin 0.30.1
c pin-drift               headroom,drift  -- pin
c pin-install             headroom,drift  -- pin --install
c pin-install-refresh     headroom,drift  -- pin --install --refresh
c pin-set-install-refresh headroom        -- pin 0.30.1 --install --refresh
c pin-install-no-uv       headroom,nouv   -- pin --install
c seal-no-proxy           headroom        -- seal
c seal-preview            headroom-p      -- seal
c seal-apply              headroom-p      -- seal --apply
c seal-again              headroom-p,sealed -- seal --apply
c seal-unknown            headroom-p      -- seal ghost
c presets-refresh         headroom,drift  -- presets --refresh
c presets-refresh-same    headroom        -- presets --refresh
c presets-refresh-no-engine headroom,nohr -- presets --refresh

echo "generated $(ls "$OUT" | wc -l) cases into $OUT"
