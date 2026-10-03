use crate::plus::args::flag;
use crate::plus::jsonfs::read_json;
use crate::plus::update::exec::{is_not_found, run_command, CmdOutput};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[cfg(all(test, unix))]
mod tests;

const LIST_TIMEOUT: Duration = Duration::from_secs(30);
const NET_TIMEOUT: Duration = Duration::from_secs(120);

pub trait ClaudeRunner {
    fn run(&self, args: &[&str], timeout: Duration) -> Result<CmdOutput, String>;
}

pub struct SystemClaude {
    bin: String,
}

impl SystemClaude {
    pub fn from_env() -> Self {
        let bin = std::env::var("TOOLPORT_CLAUDE_BIN")
            .ok()
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "claude".to_string());
        Self { bin }
    }

    pub fn with_bin(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }
}

impl ClaudeRunner for SystemClaude {
    fn run(&self, args: &[&str], timeout: Duration) -> Result<CmdOutput, String> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(args);
        run_command(cmd, timeout).map_err(|e| {
            if is_not_found(&e) {
                "claude not found on PATH".to_string()
            } else {
                e
            }
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub plugin: Option<String>,
    pub marketplace: Option<String>,
    pub dry_run: bool,
    pub refresh: bool,
    pub claude_root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginRow {
    pub name: String,
    pub marketplace: Option<String>,
    pub installed: Option<String>,
    pub available: Option<String>,
    pub status: &'static str,
    pub enabled: bool,
    pub blocked: bool,
    pub outcome: Option<String>,
    pub error: Option<String>,
}

impl PluginRow {
    pub fn id(&self) -> String {
        match &self.marketplace {
            Some(m) => format!("{}@{m}", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub mode: &'static str,
    pub rows: Vec<PluginRow>,
    pub refresh_error: Option<String>,
    pub restart_required: bool,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        self.rows.iter().any(|r| r.error.is_some())
    }

    pub fn to_value(&self) -> Value {
        let rows: Vec<Value> = self
            .rows
            .iter()
            .map(|r| {
                json!({
                    "id": r.id(),
                    "name": r.name,
                    "marketplace": r.marketplace,
                    "installed": r.installed,
                    "available": r.available,
                    "status": r.status,
                    "enabled": r.enabled,
                    "blocked": r.blocked,
                    "outcome": r.outcome,
                    "error": r.error,
                })
            })
            .collect();
        json!({
            "mode": self.mode,
            "plugins": rows,
            "refreshError": self.refresh_error,
            "restartRequired": self.restart_required,
        })
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        if let Some(e) = &self.refresh_error {
            out.push_str(&format!("marketplace refresh warning: {e}\n"));
        }
        if self.rows.is_empty() {
            out.push_str("no plugins installed\n");
        }
        for r in &self.rows {
            let installed = r.installed.as_deref().unwrap_or("?");
            let available = r.available.as_deref().unwrap_or("?");
            let mut line = format!("{:<32} {:<10} {installed} -> {available}", r.id(), r.status);
            if !r.enabled {
                line.push_str("  disabled");
            }
            if r.blocked {
                line.push_str("  blocked");
            }
            if let Some(o) = &r.outcome {
                line.push_str(&format!("  [{o}]"));
            }
            if let Some(e) = &r.error {
                line.push_str(&format!("  error: {e}"));
            }
            out.push_str(&line);
            out.push('\n');
        }
        if self.restart_required {
            out.push_str("restart Claude Code to apply the updates\n");
        }
        out
    }
}

pub fn claude_root(over: Option<&Path>) -> PathBuf {
    if let Some(p) = over {
        return p.to_path_buf();
    }
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::home_dir().unwrap_or_default().join(".claude")
}

fn entry_version(entry: &Value) -> Option<String> {
    if let Some(v) = entry.get("version").filter(|v| !v.is_null()) {
        let s = v
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| v.to_string());
        if !s.is_empty() {
            return Some(s);
        }
    }
    let source = entry.get("source")?.as_object()?;
    if let Some(sha) = source.get("sha").and_then(Value::as_str) {
        return Some(sha.chars().take(12).collect());
    }
    source.get("ref").and_then(Value::as_str).map(String::from)
}

fn catalog_versions(root: &Path) -> BTreeMap<(String, String), Option<String>> {
    let mut map = BTreeMap::new();
    let Some(Value::Object(known)) = read_json(&root.join("plugins/known_marketplaces.json"))
    else {
        return map;
    };
    for (mkt, meta) in known {
        let Some(loc) = meta.get("installLocation").and_then(Value::as_str) else {
            continue;
        };
        let Some(catalog) = read_json::<Value>(&Path::new(loc).join(".claude-plugin/marketplace.json"))
        else {
            continue;
        };
        for entry in catalog
            .get("plugins")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(name) = entry.get("name").and_then(Value::as_str) {
                map.insert((mkt.clone(), name.to_string()), entry_version(entry));
            }
        }
    }
    map
}

fn enabled_plugins(root: &Path) -> BTreeMap<String, bool> {
    read_json::<Value>(&root.join("settings.json"))
        .and_then(|v| v.get("enabledPlugins").cloned())
        .and_then(|v| v.as_object().cloned())
        .map(|o| {
            o.into_iter()
                .filter_map(|(k, v)| v.as_bool().map(|b| (k, b)))
                .collect()
        })
        .unwrap_or_default()
}

fn blocked_plugins(root: &Path) -> BTreeSet<String> {
    read_json::<Value>(&root.join("plugins/blocklist.json"))
        .and_then(|v| v.get("plugins").cloned())
        .and_then(|v| v.as_array().cloned())
        .map(|a| {
            a.iter()
                .filter_map(|e| e.get("plugin").and_then(Value::as_str).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn str_field(entry: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| entry.get(*k).and_then(Value::as_str))
        .map(String::from)
}

fn parse_installed(stdout: &str) -> Result<Vec<Value>, String> {
    let data: Value =
        serde_json::from_str(stdout).map_err(|e| format!("unparseable plugin list: {e}"))?;
    let arr = match data {
        Value::Array(a) => a,
        Value::Object(mut o) => match o.remove("plugins") {
            Some(Value::Array(a)) => a,
            _ => return Err("unexpected plugin list shape".into()),
        },
        _ => return Err("unexpected plugin list shape".into()),
    };
    Ok(arr.into_iter().filter(Value::is_object).collect())
}

fn valid_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
}

fn validate(opts: &Options) -> Result<(), String> {
    for (label, v) in [("plugin", &opts.plugin), ("marketplace", &opts.marketplace)] {
        if let Some(v) = v {
            if !valid_ident(v) {
                return Err(format!("invalid {label} name: {v}"));
            }
        }
    }
    Ok(())
}

fn survey(
    runner: &dyn ClaudeRunner,
    opts: &Options,
) -> Result<(Vec<PluginRow>, Option<String>), String> {
    validate(opts)?;
    let mut refresh_error = None;
    if opts.refresh {
        let mut args = vec!["plugin", "marketplace", "update"];
        if let Some(m) = &opts.marketplace {
            args.push(m);
        }
        match runner.run(&args, NET_TIMEOUT) {
            Ok(o) if o.ok() => {}
            Ok(o) => {
                let e = o.first_error_line();
                refresh_error = Some(if e.is_empty() {
                    format!("marketplace update exited {}", o.code)
                } else {
                    e
                });
            }
            Err(e) => refresh_error = Some(e),
        }
    }
    let listed = runner.run(&["plugin", "list", "--json"], LIST_TIMEOUT)?;
    if !listed.ok() {
        let e = listed.first_error_line();
        return Err(if e.is_empty() {
            format!("claude plugin list exited {}", listed.code)
        } else {
            e
        });
    }
    let installed = parse_installed(&listed.stdout)?;
    let root = claude_root(opts.claude_root.as_deref());
    let catalog = catalog_versions(&root);
    let enabled = enabled_plugins(&root);
    let blocked = blocked_plugins(&root);

    let mut rows = Vec::new();
    for entry in installed {
        let Some(name) = str_field(&entry, &["name", "plugin", "id"]) else {
            continue;
        };
        let (name, id_mkt) = match name.split_once('@') {
            Some((n, m)) => (n.to_string(), Some(m.to_string())),
            None => (name, None),
        };
        let marketplace =
            str_field(&entry, &["marketplace", "marketplaceName", "source"]).or(id_mkt);
        if opts.plugin.as_deref().is_some_and(|p| p != name) {
            continue;
        }
        if opts.marketplace.is_some() && opts.marketplace != marketplace {
            continue;
        }
        let installed_v = entry
            .get("version")
            .or_else(|| entry.get("installedVersion"))
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string())
            });
        let available = marketplace
            .as_ref()
            .and_then(|m| catalog.get(&(m.clone(), name.clone())).cloned().flatten());
        let status = match (&installed_v, &available) {
            (Some(a), Some(b)) if a != b => "update",
            (Some(_), Some(_)) => "current",
            _ => "unknown",
        };
        let mut row = PluginRow {
            name,
            marketplace,
            installed: installed_v,
            available,
            status,
            enabled: true,
            blocked: false,
            outcome: None,
            error: None,
        };
        let id = row.id();
        row.enabled = enabled
            .get(&id)
            .copied()
            .or_else(|| entry.get("enabled").and_then(Value::as_bool))
            .unwrap_or(true);
        row.blocked = blocked.contains(&id);
        rows.push(row);
    }
    Ok((rows, refresh_error))
}

pub fn list(runner: &dyn ClaudeRunner, opts: &Options) -> Result<Report, String> {
    let mut o = opts.clone();
    o.refresh = false;
    let (rows, refresh_error) = survey(runner, &o)?;
    Ok(Report {
        mode: "list",
        rows,
        refresh_error,
        restart_required: false,
    })
}

pub fn update(runner: &dyn ClaudeRunner, opts: &Options) -> Result<Report, String> {
    let mut o = opts.clone();
    o.refresh = true;
    let (mut rows, refresh_error) = survey(runner, &o)?;
    if opts.plugin.is_some() && rows.is_empty() {
        return Err(format!(
            "plugin '{}' is not installed",
            opts.plugin.as_deref().unwrap_or_default()
        ));
    }
    let explicit = opts.plugin.is_some();
    let mut restart_required = false;
    for row in rows.iter_mut() {
        let wanted = row.status == "update" || (explicit && row.status == "unknown");
        if row.blocked {
            row.outcome = Some("skipped: blocked".into());
            continue;
        }
        if !wanted {
            continue;
        }
        if opts.dry_run {
            row.outcome = Some("would update".into());
            continue;
        }
        let id = row.id();
        match runner.run(&["plugin", "update", &id], NET_TIMEOUT) {
            Ok(out) if out.ok() => {
                let text = format!("{}\n{}", out.stdout, out.stderr).to_lowercase();
                let noop = ["up to date", "already", "no update"]
                    .iter()
                    .any(|p| text.contains(p));
                if noop {
                    row.outcome = Some("already current".into());
                } else {
                    row.outcome = Some("updated".into());
                    restart_required = true;
                }
            }
            Ok(out) => {
                let e = out.first_error_line();
                row.error = Some(if e.is_empty() {
                    format!("exited {}", out.code)
                } else {
                    e
                });
            }
            Err(e) => row.error = Some(e),
        }
    }
    Ok(Report {
        mode: if opts.dry_run { "dry-run" } else { "update" },
        rows,
        refresh_error,
        restart_required,
    })
}

fn options_from(args: &Value) -> Options {
    let s = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    Options {
        plugin: s("plugin"),
        marketplace: s("marketplace"),
        dry_run: flag(args, "dryRun"),
        refresh: false,
        claude_root: s("claudeRoot").map(PathBuf::from),
    }
}

pub fn list_handler(args: Value) -> Result<Value, String> {
    list(&SystemClaude::from_env(), &options_from(&args)).map(|r| r.to_value())
}

pub fn update_handler(args: Value) -> Result<Value, String> {
    update(&SystemClaude::from_env(), &options_from(&args)).map(|r| r.to_value())
}
