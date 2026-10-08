//! What Claude Code itself says about the entries it holds: whether it has switched one off for a
//! project (read from its own config) and whether it can connect to it (asked of `claude mcp list`).

use super::{How, Report};
use crate::clients::DetectedClient;
use crate::plus::plugins::claude::{ClaudeRunner, SystemClaude};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

const CLIENT: &str = "claude-code";
const LIST_TIMEOUT: Duration = Duration::from_secs(60);

pub type Live = Result<BTreeMap<String, String>, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub disabled_in: Vec<String>,
    pub live: Option<String>,
}

impl Held {
    pub fn enabled(&self) -> bool {
        self.disabled_in.is_empty()
    }
}

/// The projects Claude Code switched the entry off for, from the `disabledMcpServers` it keeps
/// per project.
pub fn disabled_in(doc: &Value, name: &str) -> Vec<String> {
    let Some(projects) = doc.get("projects").and_then(Value::as_object) else {
        return Vec::new();
    };
    projects
        .iter()
        .filter(|(_, project)| {
            project
                .get("disabledMcpServers")
                .and_then(Value::as_array)
                .is_some_and(|names| names.iter().any(|n| n.as_str() == Some(name)))
        })
        .map(|(path, _)| path.clone())
        .collect()
}

/// `name: target - ✔ Connected` lines of `claude mcp list` as name to state.
pub fn parse_list(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (head, status) = line.trim().rsplit_once(" - ")?;
            let (name, _) = head.split_once(": ")?;
            let word = status
                .trim_start_matches(|c: char| !c.is_alphabetic())
                .to_lowercase();
            let state = if word.starts_with("connected") {
                "connected"
            } else if word.starts_with("needs auth") {
                "needs-auth"
            } else if word.starts_with("failed") {
                "failed"
            } else {
                return None;
            };
            Some((name.to_string(), state.to_string()))
        })
        .collect()
}

pub fn runner() -> SystemClaude {
    let found = SystemClaude::from_env();
    if found.present() {
        return found;
    }
    match dirs::home_dir().map(|home| home.join(".local/bin/claude")) {
        Some(bin) if bin.is_file() => SystemClaude::with_bin(bin.to_string_lossy()),
        _ => found,
    }
}

pub fn probe(runner: &dyn ClaudeRunner) -> Live {
    let out = runner.run(&["mcp", "list"], LIST_TIMEOUT)?;
    let states = parse_list(&out.stdout);
    if states.is_empty() && !out.ok() {
        return Err(out.first_error_line());
    }
    Ok(states)
}

pub fn annotate(report: &mut Report, detected: &[DetectedClient], live: Option<&Live>) {
    let doc = detected
        .iter()
        .find(|c| c.id == CLIENT)
        .and_then(|c| std::fs::read_to_string(&c.config_path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    for route in report
        .servers
        .iter_mut()
        .flat_map(|server| server.routes.iter_mut())
        .filter(|route| route.client == CLIENT && route.how == How::OwnEntry)
    {
        let Some(name) = route.entry.clone() else {
            continue;
        };
        route.held = Some(Held {
            disabled_in: doc.as_ref().map_or_else(Vec::new, |d| disabled_in(d, &name)),
            live: live
                .and_then(|states| states.as_ref().ok())
                .and_then(|states| states.get(&name))
                .cloned(),
        });
    }
    if let Some(Err(why)) = live {
        report.findings.push(super::Finding {
            level: "fyi",
            kind: "live-unavailable",
            server: None,
            client: Some(CLIENT.to_string()),
            text: format!("Could not ask Claude Code which servers it connects to: {why}."),
        });
    }
}
