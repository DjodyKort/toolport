//! Which plugins are installed: `claude plugin list --json` when the binary answers, the files
//! under the Claude home (`plugins/installed_plugins.json`) otherwise. `cc list|update`, the
//! `plugin` source detector and `plugins ls|show` all read through here.

use super::claude::{ClaudeRunner, NOT_FOUND};
use crate::plus::jsonfs::read_json;
use crate::plus::update::exec::CmdOutput;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const LIST_TIMEOUT: Duration = Duration::from_secs(30);
pub const NET_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginStatus {
    Update,
    Current,
    Unknown,
}

impl PluginStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PluginStatus::Update => "update",
            PluginStatus::Current => "current",
            PluginStatus::Unknown => "unknown",
        }
    }

    pub fn of(installed: Option<&str>, available: Option<&str>) -> Self {
        match (installed, available) {
            (Some(a), Some(b)) if a != b => PluginStatus::Update,
            (Some(_), Some(_)) => PluginStatus::Current,
            _ => PluginStatus::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    ClaudeCli,
    Files,
}

impl Via {
    pub fn as_str(self) -> &'static str {
        match self {
            Via::ClaudeCli => "claude-cli",
            Via::Files => "files",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub marketplace: Option<String>,
    pub version: Option<String>,
    pub install_path: Option<PathBuf>,
    pub scope: Option<String>,
    pub flagged_enabled: Option<bool>,
    pub errors: Vec<String>,
}

fn str_field(entry: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| entry.get(*k).and_then(Value::as_str))
        .map(String::from)
}

fn split_id(raw: String) -> (String, Option<String>) {
    match raw.split_once('@') {
        Some((n, m)) => (n.to_string(), Some(m.to_string())),
        None => (raw, None),
    }
}

fn join_id(name: &str, marketplace: Option<&str>) -> String {
    match marketplace {
        Some(m) => format!("{name}@{m}"),
        None => name.to_string(),
    }
}

impl Installed {
    fn from_cli(entry: &Value) -> Option<Self> {
        let (name, id_marketplace) = split_id(str_field(entry, &["name", "plugin", "id"])?);
        let marketplace =
            str_field(entry, &["marketplace", "marketplaceName", "source"]).or(id_marketplace);
        let version = entry
            .get("version")
            .or_else(|| entry.get("installedVersion"))
            .filter(|v| !v.is_null())
            .map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string()));
        Some(Self {
            id: join_id(&name, marketplace.as_deref()),
            name,
            marketplace,
            version,
            install_path: str_field(entry, &["installPath"]).map(PathBuf::from),
            scope: str_field(entry, &["scope"]),
            flagged_enabled: entry.get("enabled").and_then(Value::as_bool),
            errors: entry
                .get("errors")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|e| e.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

fn keep_one(list: Vec<Installed>) -> Vec<Installed> {
    let mut out: Vec<Installed> = Vec::new();
    for plugin in list {
        match out.iter_mut().find(|p| p.id == plugin.id) {
            Some(kept) if kept.scope.as_deref() != Some("user") && plugin.scope.as_deref() == Some("user") => {
                *kept = plugin
            }
            Some(_) => {}
            None => out.push(plugin),
        }
    }
    out
}

pub fn parse_cli_list(stdout: &str) -> Result<Vec<Installed>, String> {
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
    Ok(keep_one(
        arr.iter()
            .filter(|v| v.is_object())
            .filter_map(Installed::from_cli)
            .collect(),
    ))
}

fn failure(output: &CmdOutput, what: &str) -> String {
    let e = output.first_error_line();
    if e.is_empty() {
        format!("{what} exited {}", output.code)
    } else {
        e
    }
}

/// `claude plugin list --json`; every failure is an error.
pub fn cli_list(runner: &dyn ClaudeRunner) -> Result<Vec<Installed>, String> {
    let listed = runner.run(&["plugin", "list", "--json"], LIST_TIMEOUT)?;
    if !listed.ok() {
        return Err(failure(&listed, "claude plugin list"));
    }
    parse_cli_list(&listed.stdout)
}

/// `installed_plugins.json`: a list of install records per plugin id (version 2) or one record.
pub fn file_list(claude_home: &Path) -> Vec<Installed> {
    let Some(doc) = read_json::<Value>(&claude_home.join("plugins/installed_plugins.json")) else {
        return Vec::new();
    };
    let Some(plugins) = doc.get("plugins").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (raw, entry) in plugins {
        let records: Vec<&Value> = match entry {
            Value::Array(list) => list.iter().collect(),
            other => vec![other],
        };
        for record in records {
            let (name, marketplace) = split_id(raw.clone());
            out.push(Installed {
                id: raw.clone(),
                name,
                marketplace,
                version: str_field(record, &["version"]),
                install_path: str_field(record, &["installPath"]).map(PathBuf::from),
                scope: str_field(record, &["scope"]),
                flagged_enabled: None,
                errors: Vec::new(),
            });
        }
    }
    keep_one(out)
}

#[derive(Clone, Debug)]
pub struct Listing {
    pub plugins: Vec<Installed>,
    pub via: Via,
    /// Why `claude` was asked and did not deliver; absent when it is not installed at all.
    pub cli_error: Option<String>,
}

/// The CLI list when `claude` answers, else the files. A plugin the CLI lists without an install
/// path gets the one the files record.
pub fn list(runner: Option<&dyn ClaudeRunner>, claude_home: &Path) -> Listing {
    let files = file_list(claude_home);
    let Some(runner) = runner else {
        return Listing {
            plugins: files,
            via: Via::Files,
            cli_error: None,
        };
    };
    match cli_list(runner) {
        Ok(mut plugins) => {
            for plugin in &mut plugins {
                if plugin.install_path.is_none() {
                    plugin.install_path = files
                        .iter()
                        .find(|f| f.id == plugin.id)
                        .and_then(|f| f.install_path.clone());
                }
            }
            Listing {
                plugins,
                via: Via::ClaudeCli,
                cli_error: None,
            }
        }
        Err(e) => Listing {
            plugins: files,
            via: Via::Files,
            cli_error: (e != NOT_FOUND).then_some(e),
        },
    }
}

/// `claude plugin marketplace update [name]`; the error text when it did not work.
pub fn refresh_marketplaces(runner: &dyn ClaudeRunner, marketplace: Option<&str>) -> Option<String> {
    let mut args = vec!["plugin", "marketplace", "update"];
    if let Some(m) = marketplace {
        args.push(m);
    }
    match runner.run(&args, NET_TIMEOUT) {
        Ok(o) if o.ok() => None,
        Ok(o) => Some(failure(&o, "marketplace update")),
        Err(e) => Some(e),
    }
}

fn entry_version(entry: &Value) -> Option<String> {
    if let Some(v) = entry.get("version").filter(|v| !v.is_null()) {
        let s = v.as_str().map(String::from).unwrap_or_else(|| v.to_string());
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

/// The versions the marketplace catalogs offer, by (marketplace, plugin).
pub fn catalog_versions(root: &Path) -> BTreeMap<(String, String), Option<String>> {
    let mut map = BTreeMap::new();
    let Some(Value::Object(known)) = read_json(&root.join("plugins/known_marketplaces.json"))
    else {
        return map;
    };
    for (mkt, meta) in known {
        let Some(loc) = meta.get("installLocation").and_then(Value::as_str) else {
            continue;
        };
        let Some(catalog) =
            read_json::<Value>(&Path::new(loc).join(".claude-plugin/marketplace.json"))
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

pub fn blocked(root: &Path) -> BTreeSet<String> {
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

/// Where a marketplace comes from, as one readable string (`github:owner/repo`, a git URL, a path).
pub fn marketplace_source(root: &Path, marketplace: &str) -> Option<String> {
    let known = read_json::<Value>(&root.join("plugins/known_marketplaces.json"))?;
    let source = known.get(marketplace)?.get("source")?;
    if let Some(text) = source.as_str() {
        return Some(text.to_string());
    }
    let kind = source.get("source").and_then(Value::as_str)?;
    let field = |key: &str| source.get(key).and_then(Value::as_str);
    match kind {
        "github" => field("repo").map(|repo| format!("github:{repo}")),
        "git" | "url" => field("url").map(String::from),
        "directory" | "file" => field("path").map(String::from),
        other => Some(other.to_string()),
    }
}

/// `--flag` look-alikes never reach the claude command line.
pub fn valid_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
}
