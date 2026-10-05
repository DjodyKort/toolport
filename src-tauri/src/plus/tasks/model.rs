//! The task definition (contract section 8, D-067) and its validation. One JSON file per task.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub requires: Requires,
    #[serde(default)]
    pub writes_secrets: Vec<SecretRef>,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub triggers: Triggers,
    #[serde(default)]
    pub created_from: Option<CreatedFrom>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Requires {
    #[serde(default)]
    pub servers: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecretRef {
    pub server: String,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WaitKind {
    SecretUnset,
    Url,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitFor {
    pub kind: WaitKind,
    pub timeout_sec: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum Step {
    NeedsYou {
        id: String,
        title: String,
        instructions: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wait_for: Option<WaitFor>,
    },
    Mcp {
        id: String,
        title: String,
        server: String,
        tool: String,
        #[serde(default = "empty_object")]
        args: Value,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        capture: Vec<String>,
    },
    Routine {
        id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routine_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        script: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        capture: Vec<String>,
    },
    Exec {
        id: String,
        title: String,
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Prompt {
        id: String,
        title: String,
        prompt: String,
        #[serde(default)]
        allowed_tools: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    SecretSet {
        id: String,
        title: String,
        server: String,
        key: String,
        from: String,
    },
    RestartServer {
        id: String,
        title: String,
        server: String,
    },
}

fn empty_object() -> Value {
    Value::Object(Default::default())
}

impl Step {
    pub fn id(&self) -> &str {
        match self {
            Step::NeedsYou { id, .. } | Step::Mcp { id, .. } | Step::Routine { id, .. } | Step::Exec { id, .. } | Step::Prompt { id, .. } | Step::SecretSet { id, .. } | Step::RestartServer { id, .. } => id,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Step::NeedsYou { title, .. } | Step::Mcp { title, .. } | Step::Routine { title, .. } | Step::Exec { title, .. } | Step::Prompt { title, .. } | Step::SecretSet { title, .. } | Step::RestartServer { title, .. } => title,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Step::NeedsYou { .. } => "needs-you",
            Step::Mcp { .. } => "mcp",
            Step::Routine { .. } => "routine",
            Step::Exec { .. } => "exec",
            Step::Prompt { .. } => "prompt",
            Step::SecretSet { .. } => "secret-set",
            Step::RestartServer { .. } => "restart-server",
        }
    }

    pub fn captures(&self) -> &[String] {
        match self {
            Step::Mcp { capture, .. } | Step::Routine { capture, .. } => capture,
            _ => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Approval {
    EveryRun,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelfMcpTrigger {
    pub enabled: bool,
    pub approval: Approval,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Schedule {
    pub cron: String,
    #[serde(default)]
    pub auto_run: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Triggers {
    #[serde(default = "yes")]
    pub manual: bool,
    #[serde(default)]
    pub cli: bool,
    #[serde(default = "self_mcp_off")]
    pub self_mcp: SelfMcpTrigger,
    #[serde(default)]
    pub schedule: Option<Schedule>,
    #[serde(default)]
    pub on_auth_failure: Vec<String>,
}

fn yes() -> bool {
    true
}

fn self_mcp_off() -> SelfMcpTrigger {
    SelfMcpTrigger { enabled: false, approval: Approval::EveryRun }
}

impl Default for Triggers {
    fn default() -> Self {
        Triggers { manual: true, cli: false, self_mcp: self_mcp_off(), schedule: None, on_auth_failure: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CreatedKind {
    Command,
    Script,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatedFrom {
    pub kind: CreatedKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Task {
    pub fn has_needs_you(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::NeedsYou { .. }))
    }

    pub fn declares(&self, server: &str, key: &str) -> bool {
        self.writes_secrets.iter().any(|s| s.server == server && s.key == key)
    }

    pub fn secret_sources(&self) -> BTreeSet<&str> {
        self.steps.iter().filter_map(|s| if let Step::SecretSet { from, .. } = s { Some(from.as_str()) } else { None }).collect()
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !id.starts_with('-')
}

pub fn parse(text: &str) -> Result<Task, String> {
    serde_json::from_str::<Task>(text).map_err(|e| format!("not a valid task definition: {e}"))
}

pub fn validate(task: &Task) -> Result<(), String> {
    let mut problems: Vec<String> = Vec::new();
    if !valid_id(&task.id) {
        problems.push(format!("id {:?} must be 1-64 characters of a-z, 0-9 and '-', not starting with '-'", task.id));
    }
    if task.title.trim().is_empty() {
        problems.push("title is empty".into());
    }
    if task.steps.is_empty() {
        problems.push("steps is empty: a task needs at least one step".into());
    }
    if !task.triggers.manual {
        problems.push("triggers.manual must be true".into());
    }
    let mut seen = BTreeSet::new();
    let mut captured: BTreeSet<&str> = BTreeSet::new();
    for (i, step) in task.steps.iter().enumerate() {
        let at = format!("steps[{i}] ({} {:?})", step.kind(), step.id());
        if !valid_id(step.id()) {
            problems.push(format!("{at}: id must be 1-64 characters of a-z, 0-9 and '-'"));
        }
        if !seen.insert(step.id()) {
            problems.push(format!("{at}: duplicate step id"));
        }
        if step.title().trim().is_empty() {
            problems.push(format!("{at}: title is empty"));
        }
        match step {
            Step::NeedsYou { instructions, wait_for, .. } => {
                if instructions.trim().is_empty() {
                    problems.push(format!("{at}: instructions are empty; they are what the user is shown"));
                }
                if wait_for.as_ref().is_some_and(|w| w.timeout_sec == 0) {
                    problems.push(format!("{at}: waitFor.timeoutSec must be at least 1"));
                }
            }
            Step::Mcp { server, tool, args, .. } => {
                if tool.trim().is_empty() {
                    problems.push(format!("{at}: tool is empty"));
                }
                if !task.requires.servers.contains(server) {
                    problems.push(format!("{at}: server {server:?} is not listed in requires.servers"));
                }
                if !args.is_object() {
                    problems.push(format!("{at}: args must be an object"));
                }
            }
            Step::Routine { routine_id, script, args, .. } => {
                if routine_id.is_some() == script.is_some() {
                    problems.push(format!("{at}: give exactly one of routineId and script"));
                }
                if args.as_ref().is_some_and(|a| !a.is_object()) {
                    problems.push(format!("{at}: args must be an object"));
                }
            }
            Step::Exec { program, .. } => {
                if !task.requires.commands.contains(program) {
                    problems.push(format!("{at}: program {program:?} is not listed in requires.commands"));
                }
            }
            Step::Prompt { prompt, .. } => {
                if prompt.trim().is_empty() {
                    problems.push(format!("{at}: prompt is empty"));
                }
            }
            Step::SecretSet { server, key, from, .. } => {
                if !task.declares(server, key) {
                    problems.push(format!("{at}: {server}/{key} is not listed in writesSecrets"));
                }
                if !captured.contains(from.as_str()) {
                    problems.push(format!("{at}: from {from:?} is not captured by an earlier step"));
                }
            }
            Step::RestartServer { server, .. } => {
                if !task.requires.servers.contains(server) {
                    problems.push(format!("{at}: server {server:?} is not listed in requires.servers"));
                }
            }
        }
        captured.extend(step.captures().iter().map(String::as_str));
    }
    for secret in &task.writes_secrets {
        if secret.server.trim().is_empty() || secret.key.trim().is_empty() {
            problems.push("writesSecrets: server and key must not be empty".into());
        }
    }
    if let Some(schedule) = &task.triggers.schedule {
        if let Err(e) = super::cron::Cron::parse(&schedule.cron) {
            problems.push(format!("triggers.schedule.cron: {e}"));
        }
        if schedule.auto_run && task.has_needs_you() {
            problems.push("triggers.schedule.autoRun is not allowed for a task with a needs-you step: a schedule never starts it".into());
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("task {:?} is not valid:\n  - {}", task.id, problems.join("\n  - ")))
    }
}

const MARKERS: [(&str, &str, &str); 2] = [("sign in", "sign-in", "Sign in"), ("log in", "log-in", "Log in")];

fn frontmatter(text: &str) -> (Vec<(String, String)>, &str) {
    let Some(rest) = text.strip_prefix("---\n") else { return (Vec::new(), text) };
    let Some(end) = rest.find("\n---") else { return (Vec::new(), text) };
    let pairs = rest[..end].lines().filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string())).collect();
    let body = rest[end + 4..].trim_start_matches('\n');
    (pairs, body)
}

pub fn from_command(id: &str, path: &str, text: &str) -> Task {
    let (meta, body) = frontmatter(text);
    let get = |k: &str| meta.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    let title = get("title").or_else(|| body.lines().find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))).unwrap_or_else(|| id.to_string());
    let mut steps = Vec::new();
    for (needle, slug, label) in MARKERS {
        if let Some(line) = body.lines().find(|l| l.to_lowercase().contains(needle)) {
            steps.push(Step::NeedsYou { id: slug.into(), title: label.into(), instructions: line.trim().trim_start_matches(['-', '*', ' ']).to_string(), wait_for: None });
        }
    }
    let allowed: Vec<String> = get("allowed-tools").map(|v| v.split([',', ' ']).filter(|t| !t.is_empty()).map(String::from).collect()).unwrap_or_default();
    steps.push(Step::Prompt { id: "run-command".into(), title: format!("Run {title}"), prompt: body.trim().to_string(), allowed_tools: allowed, model: None });
    let mut servers = BTreeSet::new();
    for part in body.split("mcp__toolport__").skip(1) {
        if let Some(name) = part.split("__").next().filter(|n| valid_id(&n.replace('_', "-").to_lowercase())) {
            servers.insert(name.to_string());
        }
    }
    Task {
        id: id.into(),
        title,
        description: get("description").unwrap_or_default(),
        enabled: false,
        requires: Requires { servers: servers.into_iter().collect(), commands: Vec::new() },
        writes_secrets: Vec::new(),
        steps,
        triggers: Triggers::default(),
        created_from: Some(CreatedFrom { kind: CreatedKind::Command, path: Some(path.into()) }),
    }
}
