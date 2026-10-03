//! Sourceable zsh shims: the freshness wrapper (`claude()` / `hrclaude()`) and `claude-<profile>()`.
//!
//! Pure string generation. The wrapper is a superset of cf-dev-tools' own: its org sync (same
//! python call, same throttle cache) followed by the context sync, so union restores land in the
//! same beat as cf's clobber. The doctor's cf-wrapper hash tripwire guards this copied contract.

use super::config::ProfileSpec;
use super::roots::Roots;
use std::collections::BTreeMap;
use std::fs;

const HEADER: &str = "# Managed by `mcpm context` — do not edit by hand.\n# Source from ~/.zshrc AFTER cf's shell-wrapper.sh and compression-shims.zsh:\n#   source ~/.config/mcpm/context-shims.zsh\n";

const PRESYNC: &str = r##"mcpm_context_presync() {
    local now; now=$(date +%s)
    local interval="${CLAUDE_SYNC_INTERVAL:-14400}"
    local cf_dir="${HOME}/.local/share/cf-dev-tools"
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

# Default claude = codeforward mode (org CLAUDE.md + personal/client layers).
claude() { mcpm_context_presync; command claude "$@"; }

# hrclaude launches the claude BINARY via `mcpm compression run` (it never hits
# shell functions), so give daily hrclaude use the same freshness path.
if (( $+functions[hrclaude] )); then
    hrclaude() { mcpm_context_presync; command mcpm compression run -- "$@"; }
fi
"##;

pub fn shim_snippet(
    roots: &Roots,
    profiles: &BTreeMap<String, ProfileSpec>,
    wrap_default: bool,
) -> String {
    let mut lines: Vec<String> = vec![HEADER.to_string()];
    if wrap_default {
        lines.push(PRESYNC.to_string());
    }
    let presync = if wrap_default {
        "mcpm_context_presync; "
    } else {
        ""
    };
    for name in profiles.keys() {
        let dir = roots.profiles_root().join(name);
        lines.push(format!(
            "claude-{name}() {{ {presync}CLAUDE_CONFIG_DIR=\"{}\" command claude \"$@\"; }}",
            dir.display()
        ));
    }
    format!("{}\n", lines.join("\n"))
}

pub fn write_shims(
    roots: &Roots,
    profiles: &BTreeMap<String, ProfileSpec>,
    wrap_default: bool,
    dry_run: bool,
) -> Result<String, String> {
    let path = roots.shims_path();
    if !dry_run {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&path, shim_snippet(roots, profiles, wrap_default))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(path.display().to_string())
}

pub fn remove_shims(roots: &Roots, dry_run: bool) -> Result<bool, String> {
    let path = roots.shims_path();
    if !path.exists() {
        return Ok(false);
    }
    if !dry_run {
        fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(true)
}
