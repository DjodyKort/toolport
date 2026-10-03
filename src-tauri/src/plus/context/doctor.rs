//! Health checks: drift, staleness and the cf-dev-tools coexistence tripwires. Returns
//! `(level, message)` pairs, level being `ok`, `warn` or `fail`.

use super::config::ContextConfig;
use super::dedupe::plan_dedupe;
use super::launch;
use super::layers::PERSONAL_RULE_NAME;
use super::roots::Roots;
use crate::plus::skills::json::{parse, J};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub type Check = (String, String);

pub const PROFILE_STATE_FILE: &str = ".mcpm-context-state.json";

/// The only `~/.claude` assets cf-dev-tools' sync writes today. New files in its `claude/`
/// source dir mean the org sync grew scope.
const KNOWN_CF_ASSETS: [&str; 5] = [
    "CLAUDE.md",
    "CLAUDE.uncompressed.md",
    "settings.json",
    "shell-wrapper.sh",
    "commands",
];

fn check(level: &str, msg: impl Into<String>) -> Check {
    (level.into(), msg.into())
}

pub fn run_checks(roots: &Roots, config: &ContextConfig) -> Vec<Check> {
    let mut checks = Vec::new();
    checks.extend(check_dupes(roots, config));
    checks.extend(check_layers(roots));
    checks.extend(check_settings_policy(roots, config));
    checks.extend(check_profiles(roots, config));
    checks.extend(check_shims(roots, config));
    checks.extend(check_env(roots));
    checks.extend(check_cf_drift(roots, config));
    checks
}

fn check_dupes(roots: &Roots, config: &ContextConfig) -> Vec<Check> {
    let dupes = plan_dedupe(&roots.claude_json, &config.dedupe);
    if dupes.is_empty() {
        return vec![check("ok", "no legacy MCP duplicates")];
    }
    vec![check(
        "warn",
        format!(
            "legacy MCP duplicates present: {} — run `mcpm context sync`",
            dupes.join(", ")
        ),
    )]
}

fn check_layers(roots: &Roots) -> Vec<Check> {
    let canonical = roots.rules_dir().join(PERSONAL_RULE_NAME).join("SKILL.md");
    let transpiled = roots
        .claude_home
        .join("rules")
        .join(format!("{PERSONAL_RULE_NAME}.md"));
    if !canonical.exists() {
        vec![check(
            "warn",
            "no personal layer scaffolded — run `mcpm context init`",
        )]
    } else if !transpiled.exists() {
        vec![check(
            "warn",
            format!(
                "personal layer not transpiled to {} — run `mcpm skills sync --global`",
                transpiled.display()
            ),
        )]
    } else {
        vec![check(
            "ok",
            "personal layer in place (canonical + transpiled)",
        )]
    }
}

fn check_settings_policy(roots: &Roots, config: &ContextConfig) -> Vec<Check> {
    let wanted = [
        ("allow", &config.settings.ensure_allow),
        ("ask", &config.settings.ensure_ask),
    ];
    if wanted.iter().all(|(_, e)| e.is_empty()) {
        return Vec::new();
    }
    let path = roots.claude_home.join("settings.json");
    let data = if path.exists() {
        match fs::read_to_string(&path).ok().and_then(|t| parse(&t).ok()) {
            Some(v) => Some(v),
            None => {
                return vec![check(
                    "fail",
                    format!("{} is not valid JSON", path.display()),
                )]
            }
        }
    } else {
        None
    };
    let mut missing = 0;
    for (key, entries) in wanted {
        let present = data
            .as_ref()
            .and_then(|d| d.get("permissions"))
            .and_then(|p| p.get(key));
        for entry in entries {
            let found =
                matches!(present, Some(J::Arr(items)) if items.contains(&J::Str(entry.clone())));
            if !found {
                missing += 1;
            }
        }
    }
    if missing > 0 {
        vec![check(
            "warn",
            format!("{missing} policy permission entr(y/ies) missing (cf clobber?) — run `mcpm context sync`"),
        )]
    } else {
        vec![check(
            "ok",
            "policy permission entries present in settings.json",
        )]
    }
}

pub fn sha256_file(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(
        Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

fn profile_state_base(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join(PROFILE_STATE_FILE)).ok()?;
    parse(&text)
        .ok()?
        .get("settings_base_sha256")?
        .as_str()
        .map(String::from)
}

fn check_profiles(roots: &Roots, config: &ContextConfig) -> Vec<Check> {
    let mut checks = Vec::new();
    let base_hash = sha256_file(&roots.claude_home.join("settings.json"));
    for name in config.profiles.keys() {
        let dir = roots.profiles_root().join(name);
        if !dir.is_dir() {
            checks.push(check(
                "warn",
                format!("profile '{name}' not generated — run `mcpm context sync`"),
            ));
            continue;
        }
        let before = checks.len();
        for file in [launch::MCP_FILE, launch::SETTINGS_FILE] {
            if !dir.join(file).is_file() {
                checks.push(check(
                    "warn",
                    format!("profile '{name}': {file} missing — run `mcpm context sync`"),
                ));
            }
        }
        let mut broken: Vec<String> = fs::read_dir(&dir)
            .map(|r| {
                r.flatten()
                    .filter(|e| e.path().is_symlink() && !e.path().exists())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        broken.sort();
        if !broken.is_empty() {
            checks.push(check(
                "warn",
                format!("profile '{name}': broken symlink(s): {}", broken.join(", ")),
            ));
        }
        if let (Some(base), Some(state)) = (&base_hash, profile_state_base(&dir)) {
            if &state != base {
                checks.push(check("warn", format!("profile '{name}': settings.json stale vs ~/.claude — run `mcpm context sync`")));
            }
        }
        if checks.len() == before {
            checks.push(check("ok", format!("profile '{name}' healthy")));
        }
    }
    checks
}

fn check_shims(roots: &Roots, config: &ContextConfig) -> Vec<Check> {
    if config.profiles.is_empty() && !config.wrap_default_claude {
        return Vec::new();
    }
    let path = roots.shims_path();
    if !path.exists() {
        return vec![check(
            "warn",
            "shims file missing — run `mcpm context sync`",
        )];
    }
    let shown = path.display().to_string();
    let zshrc = fs::read_to_string(roots.home.join(".zshrc")).ok();
    let Some(content) = zshrc.filter(|c| c.contains(&shown)) else {
        return vec![check(
            "warn",
            format!("shims not sourced — add to ~/.zshrc:  source {shown}"),
        )];
    };
    if config.wrap_default_claude {
        let ours = content.find(&shown).unwrap_or(0);
        for other in ["shell-wrapper.sh", "compression-shims.zsh"] {
            if content.find(other).is_some_and(|pos| pos > ours) {
                return vec![check(
                    "warn",
                    format!("context-shims sourced BEFORE {other} in ~/.zshrc — move our source line below it"),
                )];
            }
        }
    }
    vec![check("ok", "shims written and sourced (last)")]
}

fn check_env(roots: &Roots) -> Vec<Check> {
    match &roots.env_claude_config_dir {
        Some(v) if !v.is_empty() => vec![check(
            "warn",
            "CLAUDE_CONFIG_DIR is exported globally — default `claude` now targets a profile, not ~/.claude",
        )],
        _ => Vec::new(),
    }
}

fn check_cf_drift(roots: &Roots, config: &ContextConfig) -> Vec<Check> {
    let cf = &roots.cf_dir;
    if !cf.exists() {
        return vec![check(
            "ok",
            "cf-dev-tools not installed — no coexistence constraints",
        )];
    }
    let mut checks = Vec::new();
    let wrapper = cf.join("claude").join("shell-wrapper.sh");
    if let (true, Some(baseline)) = (
        wrapper.exists(),
        config.cf_wrapper_hash.as_deref().filter(|h| !h.is_empty()),
    ) {
        if sha256_file(&wrapper).as_deref() != Some(baseline) {
            checks.push(check(
                "warn",
                "cf-dev-tools shell-wrapper.sh CHANGED since baseline — re-verify its sync still leaves rules/ and mcpServers alone, then re-baseline via `mcpm context sync`",
            ));
        } else {
            checks.push(check(
                "ok",
                "cf-dev-tools shell-wrapper unchanged since baseline",
            ));
        }
    }
    let claude_src = cf.join("claude");
    if claude_src.is_dir() {
        let found: BTreeSet<String> = fs::read_dir(&claude_src)
            .map(|r| {
                r.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        let unexpected: Vec<String> = found
            .into_iter()
            .filter(|n| !KNOWN_CF_ASSETS.contains(&n.as_str()))
            .collect();
        if !unexpected.is_empty() {
            checks.push(check(
                "warn",
                format!(
                    "cf-dev-tools claude/ grew new assets: {} — its sync scope may have expanded",
                    unexpected.join(", ")
                ),
            ));
        }
    }
    checks
}
