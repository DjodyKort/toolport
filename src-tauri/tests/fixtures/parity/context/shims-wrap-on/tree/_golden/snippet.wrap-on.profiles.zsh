# Managed by `mcpm context` — do not edit by hand.
# Source from ~/.zshrc AFTER cf's shell-wrapper.sh and compression-shims.zsh:
#   source ~/.config/mcpm/context-shims.zsh

mcpm_context_presync() {
    local now; now=$(date +%s)
    local interval="${CLAUDE_SYNC_INTERVAL:-14400}"
    local cf_dir="${HOME}/.local/share/corp-dev-tools"
    local cf_cache="${HOME}/.claude/.last_auto_sync"
    local cf_ran=0
    if [[ -d "${cf_dir}/.git" && -x "${cf_dir}/.venv/bin/python" ]]; then
        local last=0
        [[ -f "$cf_cache" ]] && last=$(cat "$cf_cache" 2>/dev/null || echo 0)
        if (( now - last >= interval )); then
            git -C "$cf_dir" pull --quiet 2>/dev/null
            "${cf_dir}/.venv/bin/python" -c 'import sys; sys.path.insert(0, sys.argv[1]); from cfdevtools.claude import sync_claude_files, auto_update_mcp_repos; sync_claude_files(); auto_update_mcp_repos()' "$cf_dir" 2>/dev/null
            echo "$now" > "$cf_cache"
            cf_ran=1
        fi
    fi
    local ctx_cache="${HOME}/.cache/mcpm/context/.last_auto_sync"
    local ctx_last=0
    [[ -f "$ctx_cache" ]] && ctx_last=$(cat "$ctx_cache" 2>/dev/null || echo 0)
    if (( cf_ran )) || (( now - ctx_last >= interval )); then
        if command mcpm context sync >/dev/null 2>&1; then
            mkdir -p "$(dirname "$ctx_cache")" 2>/dev/null
            echo "$now" > "$ctx_cache"
        else
            print -u2 "mcpm: context sync failed, retrying next launch (run 'mcpm doctor')"
        fi
    fi
}

# Default claude = examplecorp mode (org CLAUDE.md + personal/client layers).
claude() { mcpm_context_presync; command claude "$@"; }

# hrclaude launches the claude BINARY via `mcpm compression run` (it never hits
# shell functions), so give daily hrclaude use the same freshness path.
if (( $+functions[hrclaude] )); then
    hrclaude() { mcpm_context_presync; command mcpm compression run -- "$@"; }
fi

claude-bare() { mcpm_context_presync; CLAUDE_CONFIG_DIR="<HOME>/.config/mcpm/claude-profiles/bare" command claude "$@"; }
claude-work() { mcpm_context_presync; CLAUDE_CONFIG_DIR="<HOME>/.config/mcpm/claude-profiles/work" command claude "$@"; }
