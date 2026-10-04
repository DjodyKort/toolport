//! Sourceable zsh shims: the freshness wrapper (`claude()` / `hrclaude()`) and `claude-<profile>()`.
//!
//! Pure string generation. The wrapper is a superset of corp-dev-tools' own: its org sync (same
//! python call, same throttle cache) followed by the context sync, so union restores land in the
//! same beat as the org tool's clobber. The doctor's wrapper hash tripwire guards this copied contract.

use super::config::ProfileSpec;
use super::launch::launch_argv;
use super::roots::Roots;
use super::zshrc::shell_path;
use crate::plus::skills::pyfs::write_text;
use std::collections::BTreeMap;
use std::fs;

fn header(roots: &Roots) -> String {
    format!(
        "# Managed by `toolportctl context` — do not edit by hand.\n# Source from ~/.zshrc AFTER cf's shell-wrapper.sh and compression-shims.zsh:\n#   source {}\n",
        shell_path(&roots.home, &roots.shims_path())
    )
}

const PRESYNC: &str = r##"mcpm_context_presync() {
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
        if command toolportctl context sync >/dev/null 2>&1; then
            mkdir -p "$(dirname "$ctx_cache")" 2>/dev/null
            echo "$now" > "$ctx_cache"
        else
            print -u2 "toolportctl: context sync failed, retrying next launch (run 'toolportctl doctor')"
        fi
    fi
}

# Default claude = examplecorp mode (org CLAUDE.md + personal/client layers).
claude() { mcpm_context_presync; command claude "$@"; }

# hrclaude launches the claude BINARY via `toolportctl compression run` (it never hits
# shell functions), so give daily hrclaude use the same freshness path.
if (( $+functions[hrclaude] )); then
    hrclaude() { mcpm_context_presync; command toolportctl compression run -- "$@"; }
fi
"##;

pub(super) fn shell_quote(arg: &str) -> String {
    if arg.starts_with("--") {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

const DEFAULT_CF_DIR_LINE: &str = r#"local cf_dir="${HOME}/.local/share/corp-dev-tools""#;

fn presync_block(roots: &Roots) -> String {
    if roots.cf_dir == roots.default_cf_dir() {
        return PRESYNC.to_string();
    }
    let line = format!("local cf_dir={}", shell_quote(&roots.cf_dir.to_string_lossy()));
    PRESYNC.replacen(DEFAULT_CF_DIR_LINE, &line, 1)
}

pub fn shim_snippet(
    roots: &Roots,
    profiles: &BTreeMap<String, ProfileSpec>,
    wrap_default: bool,
) -> String {
    let mut lines: Vec<String> = vec![header(roots)];
    if wrap_default {
        lines.push(presync_block(roots));
    }
    let presync = if wrap_default {
        "mcpm_context_presync; "
    } else {
        ""
    };
    for (name, spec) in profiles {
        let args: Vec<String> = launch_argv(roots, name, spec)
            .iter()
            .map(|a| shell_quote(a))
            .collect();
        let args = args.join(" ");
        lines.push(format!(
            "claude-{name}() {{ {presync}command claude {args} \"$@\"; }}"
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
        write_text(&path, &shim_snippet(roots, profiles, wrap_default))?;
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
