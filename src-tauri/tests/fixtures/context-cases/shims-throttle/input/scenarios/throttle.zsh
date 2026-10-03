set +e
cf=$HOME/.local/share/cf-dev-tools
bin=$HOME/stubbin
mkdir -p $bin $HOME/.claude $HOME/.cache/mcpm/context
cat > $bin/mcpm <<'S'
#!/bin/sh
echo "mcpm $*" >> $HOME/stub.log
exit ${STUB_MCPM_RC:-0}
S
cat > $bin/claude <<'S'
#!/bin/sh
echo "claude $* CLAUDE_CONFIG_DIR=${CLAUDE_CONFIG_DIR:-<unset>}" >> $HOME/stub.log
S
cat > $bin/git <<'S'
#!/bin/sh
echo "git $*" >> $HOME/stub.log
S
mkdir -p $cf/.git $cf/.venv/bin
cat > $cf/.venv/bin/python <<'S'
#!/bin/sh
echo "cf-python sync_claude_files+auto_update_mcp_repos" >> $HOME/stub.log
S
chmod +x $bin/* $cf/.venv/bin/python
export PATH=$bin:/usr/bin:/bin
source $HOME/.config/mcpm/context-shims.zsh
hrclaude() { :; }
source $HOME/.config/mcpm/context-shims.zsh
cfc=$HOME/.claude/.last_auto_sync
ctc=$HOME/.cache/mcpm/context/.last_auto_sync
state() { echo "caches: cf=$([[ -f $cfc ]] && echo yes || echo no) ctx=$([[ -f $ctc ]] && echo yes || echo no)"; }
launch() {
  : > $HOME/stub.log
  echo "## $1"; shift
  "$@" 2>&1
  echo "rc=$?"; sed 's/^/  /' $HOME/stub.log; state
}
launch "cold launch: cf sync and ctx sync both run" claude one
launch "relaunch inside interval: nothing but claude" claude two
CLAUDE_SYNC_INTERVAL=0 launch "interval 0: both run again" claude three
rm -f $ctc
launch "ctx cache missing, cf fresh: only ctx sync" claude four
echo 1 > $cfc
launch "cf cache stale (epoch 1): cf runs, ctx follows" claude five
rm -f $cfc $ctc
STUB_MCPM_RC=1 launch "mcpm fails: stderr hint, ctx cache not written" claude six
echo 1 > $ctc
STUB_MCPM_RC=0 launch "retry next launch with stale ctx cache" claude seven
CLAUDE_SYNC_INTERVAL=99999999 launch "huge interval: no sync" claude eight
launch "profile shim passes CLAUDE_CONFIG_DIR and presyncs" claude-work nine
rm -f $cfc $ctc
mv $cf/.venv $cf/.venv.off
launch "cf venv missing: cf skipped, ctx sync by cache age" claude ten
mv $cf/.venv.off $cf/.venv
rm -rf $cf/.git $ctc
launch "cf .git missing: cf skipped" claude eleven
rm -f $ctc
launch "hrclaude defined: runs compression via mcpm" hrclaude --flag
launch "claude args with spaces" claude "a b" "c"
