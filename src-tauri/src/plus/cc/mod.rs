use crate::plus::args::flag;
use crate::plus::plugins::installed::{self, NET_TIMEOUT};
use crate::plus::plugins::settings::Layers;
pub use crate::plus::plugins::{ClaudeRunner, PluginStatus, SystemClaude};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[cfg(all(test, unix))]
mod tests;

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
    pub status: PluginStatus,
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
            let mut line = format!(
                "{:<32} {:<10} {installed} -> {available}",
                r.id(),
                r.status.as_str()
            );
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

fn validate(opts: &Options) -> Result<(), String> {
    for (label, v) in [("plugin", &opts.plugin), ("marketplace", &opts.marketplace)] {
        if let Some(v) = v {
            if !installed::valid_ident(v) {
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
    let refresh_error = if opts.refresh {
        installed::refresh_marketplaces(runner, opts.marketplace.as_deref())
    } else {
        None
    };
    let listed = installed::cli_list(runner)?;
    let root = claude_root(opts.claude_root.as_deref());
    let catalog = installed::catalog_versions(&root);
    let layers = Layers::load(&root, None, None, None);
    let blocked = installed::blocked(&root);

    let mut rows = Vec::new();
    for plugin in listed {
        if opts.plugin.as_deref().is_some_and(|p| p != plugin.name) {
            continue;
        }
        if opts.marketplace.is_some() && opts.marketplace != plugin.marketplace {
            continue;
        }
        let available = plugin.marketplace.as_ref().and_then(|m| {
            catalog
                .get(&(m.clone(), plugin.name.clone()))
                .cloned()
                .flatten()
        });
        let status = PluginStatus::of(plugin.version.as_deref(), available.as_deref());
        let enabled = layers
            .enabled(&plugin.id)
            .user
            .or(plugin.flagged_enabled)
            .unwrap_or(true);
        rows.push(PluginRow {
            blocked: blocked.contains(&plugin.id),
            name: plugin.name,
            marketplace: plugin.marketplace,
            installed: plugin.version,
            available,
            status,
            enabled,
            outcome: None,
            error: None,
        });
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
        let wanted =
            row.status == PluginStatus::Update || (explicit && row.status == PluginStatus::Unknown);
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
