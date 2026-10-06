//! Finding the org's tools clone when nothing names it. The default folder carries a neutral
//! name, so a clone installed under its own name (`~/.local/share/<org>-dev-tools`) stays
//! invisible until setup records it as `corp_tools_dir`.

use super::config::ContextConfig;
use super::doctor::sha256_file;
use super::roots::Roots;
use super::Report;
use std::fs;
use std::path::{Path, PathBuf};

const ORG_FILES: [&str; 2] = ["CLAUDE.md", "CLAUDE.uncompressed.md"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detection {
    pub chosen: Option<PathBuf>,
    pub candidates: Vec<PathBuf>,
}

/// Git checkouts under `~/.local/share` that ship an org CLAUDE.md in `claude/`.
fn candidates(roots: &Roots) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(roots.home.join(".local/share")) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|dir| dir.join(".git").exists())
        .filter(|dir| ORG_FILES.iter().any(|f| dir.join("claude").join(f).is_file()))
        .collect();
    found.sort();
    found
}

/// A clone whose org file is the one in `~/.claude`, or whose shell wrapper is the recorded
/// baseline, is the clone; with no such proof only a sole candidate is taken.
pub fn detect(roots: &Roots, config: &ContextConfig) -> Detection {
    let candidates = candidates(roots);
    let user_md = sha256_file(&roots.claude_home.join("CLAUDE.md"));
    let baseline = config.cf_wrapper_hash.as_deref().filter(|h| !h.is_empty());
    let proven = |dir: &Path| {
        let claude = dir.join("claude");
        let same_org_file = user_md.is_some()
            && ORG_FILES
                .iter()
                .any(|f| sha256_file(&claude.join(f)) == user_md);
        let same_wrapper = baseline.is_some()
            && sha256_file(&claude.join("shell-wrapper.sh")).as_deref() == baseline;
        same_org_file || same_wrapper
    };
    let matched: Vec<&PathBuf> = candidates.iter().filter(|d| proven(d)).collect();
    let chosen = match (matched.as_slice(), candidates.as_slice()) {
        ([one], _) => Some((*one).clone()),
        ([], [only]) => Some(only.clone()),
        _ => None,
    };
    Detection { chosen, candidates }
}

/// True when neither the environment nor the config names a clone and the default folder has none.
pub fn unconfigured(roots: &Roots, config: &ContextConfig) -> bool {
    let named = |v: Option<&str>| v.filter(|s| !s.is_empty()).is_some();
    !named(roots.env_corp_tools_dir.as_deref())
        && !named(config.corp_tools_dir.as_deref())
        && !roots.resolve_corp_tools_dir(config).exists()
}

pub fn tilde(roots: &Roots, path: &Path) -> String {
    match path.strip_prefix(&roots.home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Records a detected clone as `corp_tools_dir`; a config or environment that already names
/// one is left alone, even when that folder is missing.
pub fn adopt(
    roots: &Roots,
    config: &mut ContextConfig,
    report: &mut Report,
    dry: bool,
) -> Option<String> {
    if !unconfigured(roots, config) {
        return None;
    }
    let detection = detect(roots, config);
    let Some(dir) = detection.chosen else {
        if detection.candidates.len() > 1 {
            let names: Vec<String> = detection
                .candidates
                .iter()
                .map(|d| tilde(roots, d))
                .collect();
            report.warn(format!(
                "several org clones found ({}); set corp_tools_dir in context.json to the right one",
                names.join(", ")
            ));
        }
        return None;
    };
    let shown = tilde(roots, &dir);
    config.corp_tools_dir = Some(shown.clone());
    let verb = if dry { "would record" } else { "recorded" };
    report.add(format!(
        "found the org clone at {shown}; {verb} it as corp_tools_dir"
    ));
    Some(shown)
}
