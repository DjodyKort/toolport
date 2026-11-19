//! The shell file Toolport owns, `context-shims.zsh` in the data directory: a `claude()` that
//! refreshes the layers before it starts Claude, one `claude-<profile>()` per launch profile and
//! the presync wrapper around `hrclaude`. Generated as a pure string; function names never start
//! with an underscore because Claude Code's shell snapshot drops those.

use super::bundle_use::shell_word;

pub const SHIMS_FILE: &str = "context-shims.zsh";

/// Name of the function that every wrapper calls first.
pub const PRESYNC_FUNCTION: &str = "context_presync";

/// A launch profile as the shell file needs it: the name and the `mcp.json` its launch passes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileShim {
    pub name: String,
    pub mcp_file: String,
}

/// A function the file defines and what it does, for the Advanced list of the tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShimFunction {
    pub name: String,
    pub summary: String,
}

const HEADER: &str = "# Managed by `toolportctl context` \u{2014} do not edit by hand.\n\
# Sourced from your shell file by the line marked `toolport:context-shims`.\n";

const PRESYNC: &str = r#"context_presync() {
    command -v toolportctl >/dev/null 2>&1 || return 0
    command toolportctl context presync --quiet 2>/dev/null || print -u2 "toolportctl: the presync failed; Claude starts anyway (toolportctl context wiring status)"
    return 0
}
"#;

const HRCLAUDE: &str = r#"if (( $+functions[hrclaude] )); then
    hrclaude() { context_presync; command toolportctl compression run -- "$@"; }
fi
"#;

pub fn snippet(profiles: &[ProfileShim]) -> String {
    let mut text = String::from(HEADER);
    text.push('\n');
    text.push_str(PRESYNC);
    text.push('\n');
    text.push_str("claude() { context_presync; command claude \"$@\"; }\n");
    for profile in profiles {
        text.push_str(&format!(
            "claude-{}() {{ context_presync; command claude --strict-mcp-config --mcp-config {} \"$@\"; }}\n",
            profile.name,
            shell_word(&profile.mcp_file)
        ));
    }
    text.push('\n');
    text.push_str(HRCLAUDE);
    text
}

pub fn functions(profiles: &[ProfileShim]) -> Vec<ShimFunction> {
    let mut out = vec![
        ShimFunction {
            name: PRESYNC_FUNCTION.into(),
            summary: "Before a launch: pulls the org tool when it is due, then runs context sync. A failure only warns.".into(),
        },
        ShimFunction {
            name: "claude".into(),
            summary: "Presync, then starts Claude with the default settings folder.".into(),
        },
    ];
    out.extend(profiles.iter().map(|profile| ShimFunction {
        name: format!("claude-{}", profile.name),
        summary: format!(
            "Presync, then starts Claude in launch profile {}.",
            profile.name
        ),
    }));
    out.push(ShimFunction {
        name: "hrclaude".into(),
        summary:
            "Only while a compression proxy is set up: presync, then Claude under that policy."
                .into(),
    });
    out
}
