//! Sourceable zsh output, generated as pure strings. Function names never start with an
//! underscore: Claude Code's shell snapshot drops those, which silently disabled a wrapper once.

use super::model::OrderedMap;

pub const SHIM_FUNCTIONS: [&str; 8] = [
    "hrclaude",
    "hrup",
    "hrdown",
    "hrrestart",
    "hrstat",
    "hrupdate",
    "hrperf",
    "hrdash",
];

#[derive(Clone, Copy, Debug, Default)]
pub struct ShimOptions {
    /// Also define `claude()` so a plain `claude` resolves the per-directory policy.
    /// Off by default: mcpm only ever routed through `hrclaude`.
    pub route_claude: bool,
}

const HEADER: &str = "# Managed by `toolportctl compression` \u{2014} do not edit by hand.\n\
# Source from ~/.zshrc:  source <data dir>/compression-shims.zsh\n\
# (replaces the old hand-maintained ~/.config/headroom-aliases.zsh)\n";

/// Thin wrappers over `toolportctl compression`; all launch and lifecycle logic lives there.
/// `claude` itself is exec'd by `run` from PATH, so a `claude` function never recurses.
pub fn shim_snippet(opts: ShimOptions) -> String {
    let mut text = String::from(HEADER);
    text.push_str(concat!(
        "hrclaude() { toolportctl compression run -- \"$@\"; }   # launch claude under the per-dir policy\n",
        "hrup()     { toolportctl compression proxy up; }        # start the active-preset proxy\n",
        "hrdown()   { toolportctl compression proxy down; }      # stop it\n",
        "hrrestart(){ toolportctl compression proxy restart; }   # restart (apply a mode change)\n",
        "hrstat()   { toolportctl compression status; }\n",
        "hrupdate() { toolportctl compression update --latest --accept; }  # pin newest headroom + re-snapshot presets\n",
        "hrperf()   { headroom perf \"$@\"; }              # savings report (headroom passthrough)\n",
        "hrdash()   { { open \"http://127.0.0.1:8787/dashboard\" || xdg-open \"http://127.0.0.1:8787/dashboard\"; } 2>/dev/null || headroom perf; }\n",
    ));
    if opts.route_claude {
        text.push_str(
            "claude()   { toolportctl compression run -- \"$@\"; }   # route plain claude through the policy\n",
        );
    }
    text
}

pub fn escape_double_quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The 0600 env snippet for the active preset; sourced from `~/.zshrc` to route a shell.
pub fn shell_env_snippet(env: &OrderedMap<String>) -> String {
    let mut text = String::from(
        "# Managed by `toolportctl compression` \u{2014} do not edit by hand.\n\
         # Source from ~/.zshrc to route this shell's AI clients through the compressor.\n",
    );
    for (k, v) in env.iter() {
        text.push_str(&format!("export {k}=\"{}\"\n", escape_double_quoted(v)));
    }
    text
}

/// Names of shell functions defined at column 0 (`name()`), for presence checks.
pub fn defined_functions(snippet: &str) -> Vec<String> {
    snippet
        .lines()
        .filter_map(|line| {
            let name: String = line
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            let rest = line[name.len()..].trim_start();
            (!name.is_empty() && rest.starts_with("()")).then_some(name)
        })
        .collect()
}
