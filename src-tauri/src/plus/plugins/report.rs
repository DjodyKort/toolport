//! `plugins ls` and `plugins show`: one row per installed plugin, built from the reader in
//! `installed` and the plugin's own files, plus the figures only `claude plugin` knows when it is
//! there. The row is the contract's `PluginRow`; `warnings` is additive.

use super::claude::{ClaudeRunner, NOT_FOUND};
use super::installed::{self, Installed, PluginStatus, Via};
use super::manifest::{self, McpServer, OptionRow};
use super::settings::{Enabled, Layers, Scope};
use super::{cli, hooks, Env};
use crate::plus::sources::fsx;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

const DETAIL_WORKERS: usize = 4;

#[derive(Clone, Copy, Default)]
pub struct Opts<'a> {
    pub cwd: Option<&'a Path>,
    pub refresh: bool,
}

#[derive(Debug)]
pub enum Failure {
    Missing(String),
    Failed(String),
}

struct Shared<'a> {
    env: &'a Env,
    layers: Layers,
    catalog: BTreeMap<(String, String), Option<String>>,
    blocked: BTreeSet<String>,
    cwd: Option<&'a Path>,
    via: Via,
}

struct Built {
    row: Map<String, Value>,
    manifest: Value,
    mcp: Vec<McpServer>,
    components: manifest::Components,
    lsp: Vec<String>,
    hooks: Vec<hooks::Entry>,
}

fn tokens(value: u64, basis: &str) -> Value {
    json!({"value": value, "basis": basis})
}

fn mcp_key(plugin: &str, server: &str) -> String {
    format!("plugin:{plugin}:{server}")
}

fn managed_warning(id: &str, enabled: &Enabled, layers: &Layers) -> Option<String> {
    let on = enabled.managed?;
    let path = layers
        .files()
        .iter()
        .find(|f| f.scope == Scope::Managed)
        .map(|f| f.path.display().to_string())
        .unwrap_or_default();
    Some(format!(
        "managed settings ({path}) set {id} to {} and win over user, project and local settings",
        if on { "on" } else { "off" }
    ))
}

fn build(s: &Shared, p: &Installed, projected: Option<u64>, warnings: &mut Vec<String>) -> Built {
    let enabled = s.layers.enabled(&p.id);
    if let Some(w) = managed_warning(&p.id, &enabled, &s.layers) {
        warnings.push(w);
    }
    let install = p.install_path.clone();
    let present = install.as_deref().filter(|d| fsx::is_dir(d));
    if present.is_none() {
        warnings.push(format!(
            "{}: the install folder is missing, so what it brings is unknown",
            p.id
        ));
    }
    let doc = present.map(manifest::load).unwrap_or(Value::Null);
    let components = present.map(manifest::components).unwrap_or_default();
    let mcp = present
        .map(|d| manifest::mcp_servers(d, &doc))
        .unwrap_or_default();
    let lsp = present
        .map(|d| manifest::lsp_servers(d, &doc))
        .unwrap_or_default();
    let plugin_hooks = present
        .map(|d| hooks::plugin_entries(&s.env.home, &p.id, d, &doc, enabled.effective, warnings))
        .unwrap_or_default();

    let version = p
        .version
        .clone()
        .or_else(|| manifest::version(&doc))
        .unwrap_or_else(|| "unknown".to_string());
    let marketplace = p.marketplace.clone().unwrap_or_default();
    let available = s
        .catalog
        .get(&(marketplace.clone(), p.name.clone()))
        .cloned()
        .flatten();
    let state = if s.blocked.contains(&p.id) {
        "blocked"
    } else {
        PluginStatus::of(Some(&version).filter(|v| *v != "unknown").map(String::as_str), available.as_deref()).as_str()
    };
    let outside: Vec<Value> = mcp
        .iter()
        .map(|m| {
            let key = mcp_key(&p.name, &m.name);
            let denied = s
                .layers
                .denied(&key, m.raw_command.as_deref(), m.raw_url.as_deref())
                .any();
            json!({"key": key, "name": m.name, "denied": denied})
        })
        .collect();
    let measured = s
        .cwd
        .zip(s.env.data_dir.as_deref())
        .and_then(|(cwd, data)| measured_cost(data, cwd, &p.id));

    let mut row = Map::new();
    row.insert("id".into(), json!(p.id));
    row.insert("name".into(), json!(p.name));
    row.insert("marketplace".into(), json!(marketplace));
    row.insert("version".into(), json!(version));
    row.insert(
        "installPath".into(),
        json!(install.as_deref().map(fsx::display).unwrap_or_default()),
    );
    row.insert(
        "source".into(),
        json!(p
            .marketplace
            .as_deref()
            .and_then(|m| installed::marketplace_source(&s.env.claude_home, m))
            .unwrap_or_default()),
    );
    row.insert(
        "enabled".into(),
        json!({
            "user": enabled.user,
            "project": enabled.project,
            "local": enabled.local,
            "effective": enabled.effective,
        }),
    );
    row.insert("update".into(), json!({"state": state, "available": available}));
    row.insert(
        "brings".into(),
        json!({
            "skills": components.skills.len(),
            "agents": components.agents.len(),
            "commands": components.commands.len(),
            "hooks": plugin_hooks.len(),
            "mcpServers": mcp.len(),
            "lspServers": lsp.len(),
        }),
    );
    row.insert(
        "cost".into(),
        json!({
            "projected": projected.map(|v| tokens(v, "projected")),
            "measured": measured.map(|v| tokens(v, "measured")),
        }),
    );
    row.insert("adapter".into(), Value::Null);
    row.insert("mcpOutsideGateway".into(), Value::Array(outside));
    row.insert("from".into(), json!(s.via.as_str()));
    Built {
        row,
        manifest: doc,
        mcp,
        components,
        lsp,
        hooks: plugin_hooks,
    }
}

/// The delta `context measure` cached for `without plugin:<id>` in a folder, as a token count.
/// The cache is read as written by `context measure` (`<data dir>/plus/cache/measure/*.json`, a
/// document with `cwd`, `measuredAt` and `deltas`); the newest matching one wins.
pub fn measured_cost(data_dir: &Path, cwd: &Path, id: &str) -> Option<u64> {
    let want = fsx::canonical(cwd);
    let label = format!("without plugin:{id}");
    let mut best: Option<(String, u64)> = None;
    for entry in fsx::list_dir(&data_dir.join("plus/cache/measure")) {
        if !entry.name.ends_with(".json") {
            continue;
        }
        let Some(doc) = manifest::read_doc(&entry.path) else {
            continue;
        };
        let doc = doc.get("data").filter(|d| d.is_object()).unwrap_or(&doc);
        let same = doc
            .get("cwd")
            .and_then(Value::as_str)
            .is_some_and(|c| fsx::canonical(Path::new(c)) == want);
        if !same {
            continue;
        }
        let tokens = doc
            .get("deltas")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|d| d.get("label").and_then(Value::as_str) == Some(&label))
            .and_then(|d| d.get("tokens")?.as_f64())
            .map(|t| t.abs().round() as u64);
        let at = doc
            .get("measuredAt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if let Some(t) = tokens {
            if best.as_ref().is_none_or(|(prev, _)| at >= *prev) {
                best = Some((at, t));
            }
        }
    }
    best.map(|(_, t)| t)
}

fn shared<'a>(
    env: &'a Env,
    listing_via: Via,
    cwd: Option<&'a Path>,
) -> Shared<'a> {
    Shared {
        env,
        layers: env.layers(cwd),
        catalog: installed::catalog_versions(&env.claude_home),
        blocked: installed::blocked(&env.claude_home),
        cwd,
        via: listing_via,
    }
}

/// `plugin details` for several plugins at once; one failure does not stop the others.
fn projected_costs(runner: &dyn ClaudeRunner, ids: &[String]) -> Vec<Result<Option<u64>, String>> {
    let next = AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<Result<Option<u64>, String>>>> =
        ids.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..DETAIL_WORKERS.min(ids.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(id) = ids.get(i) else { break };
                *slots[i].lock().unwrap() = Some(cli::details(runner, id));
            });
        }
    });
    slots
        .into_iter()
        .map(|s| s.into_inner().unwrap().unwrap_or_else(|| Err("not run".into())))
        .collect()
}

fn read_listing(
    env: &Env,
    runner: Option<&dyn ClaudeRunner>,
    warnings: &mut Vec<String>,
) -> installed::Listing {
    let listing = installed::list(runner, &env.claude_home);
    if let Some(e) = &listing.cli_error {
        warnings.push(format!("claude plugin list failed ({e}); read the plugin files instead"));
    }
    listing
}

/// `plugins ls`.
pub fn ls(env: &Env, runner: Option<&dyn ClaudeRunner>, opts: &Opts) -> Value {
    let mut warnings: Vec<String> = Vec::new();
    let mut refresh_error = None;
    if opts.refresh {
        refresh_error = Some(match runner {
            Some(r) => installed::refresh_marketplaces(r, None),
            None => Some(NOT_FOUND.to_string()),
        })
        .flatten();
    }
    let mut listing = read_listing(env, runner, &mut warnings);
    listing.plugins.sort_by(|a, b| a.id.cmp(&b.id));
    let mut partial = listing.cli_error.is_some();
    let s = shared(env, listing.via, opts.cwd);
    warnings.extend(s.layers.problems());

    let projected: Vec<Option<u64>> = match (runner, listing.via) {
        (Some(r), Via::ClaudeCli) => {
            let ids: Vec<String> = listing.plugins.iter().map(|p| p.id.clone()).collect();
            projected_costs(r, &ids)
                .into_iter()
                .zip(&ids)
                .map(|(res, id)| match res {
                    Ok(v) => v,
                    Err(e) => {
                        partial = true;
                        warnings.push(format!("claude plugin details {id} failed ({e})"));
                        None
                    }
                })
                .collect()
        }
        _ => vec![None; listing.plugins.len()],
    };
    let rows: Vec<Value> = listing
        .plugins
        .iter()
        .zip(projected)
        .map(|(p, cost)| Value::Object(build(&s, p, cost, &mut warnings).row))
        .collect();
    json!({
        "plugins": rows,
        "refreshError": refresh_error,
        "restartRequired": false,
        "partial": partial,
        "warnings": warnings,
    })
}

fn find<'a>(plugins: &'a [Installed], id: &str) -> Result<&'a Installed, Failure> {
    if let Some(p) = plugins.iter().find(|p| p.id == id) {
        return Ok(p);
    }
    let named: Vec<&Installed> = plugins.iter().filter(|p| p.name == id).collect();
    match named.as_slice() {
        [one] => Ok(one),
        [] => Err(Failure::Missing(format!("plugin '{id}' is not installed"))),
        many => Err(Failure::Failed(format!(
            "'{id}' is ambiguous: {}",
            many.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

fn file_options(layers: &Layers, id: &str, doc: &Value) -> Vec<OptionRow> {
    let mut rows = manifest::user_config(doc);
    for row in &mut rows {
        let value = layers
            .files()
            .iter()
            .filter(|f| f.scope == Scope::User)
            .filter_map(|f| f.doc.as_ref()?.get("pluginConfigs")?.get(id)?.get("options")?.get(&row.key))
            .last()
            .filter(|v| !v.is_null());
        row.configured = value.is_some();
        if !row.sensitive {
            row.current = value.map(manifest::value_text);
        }
    }
    rows
}

/// `plugins show <id>`.
pub fn show(
    env: &Env,
    runner: Option<&dyn ClaudeRunner>,
    id: &str,
    opts: &Opts,
) -> Result<Value, Failure> {
    let mut warnings: Vec<String> = Vec::new();
    let listing = read_listing(env, runner, &mut warnings);
    let found = find(&listing.plugins, id)?.clone();
    let s = shared(env, listing.via, opts.cwd);
    warnings.extend(s.layers.problems());

    let cli = match (runner, listing.via) {
        (Some(r), Via::ClaudeCli) => Some(r),
        _ => None,
    };
    let projected = cli.and_then(|r| match cli::details(r, &found.id) {
        Ok(v) => v,
        Err(e) => {
            warnings.push(format!("claude plugin details {} failed ({e})", found.id));
            None
        }
    });
    let built = build(&s, &found, projected, &mut warnings);

    let from_cli = cli.and_then(|r| match cli::configure(r, &found.id) {
        Ok(rows) => Some(rows),
        Err(e) => {
            warnings.push(format!("claude plugin configure {} failed ({e})", found.id));
            None
        }
    });
    let options = from_cli.unwrap_or_else(|| file_options(&s.layers, &found.id, &built.manifest));

    let servers: Vec<Value> = built
        .mcp
        .iter()
        .map(|m| {
            let key = mcp_key(&found.name, &m.name);
            let d = s.layers.denied(&key, m.raw_command.as_deref(), m.raw_url.as_deref());
            json!({
                "key": key,
                "name": m.name,
                "command": m.command,
                "url": m.url,
                "toolPrefix": manifest::tool_prefix(&found.name, &m.name),
                "denied": {"user": d.user, "project": d.project, "local": d.local, "managed": d.managed},
            })
        })
        .collect();

    let mut data = built.row;
    data.insert("description".into(), json!(manifest::description(&built.manifest)));
    data.insert(
        "components".into(),
        json!({
            "skills": built.components.skills,
            "agents": built.components.agents,
            "commands": built.components.commands,
            "lspServers": built.lsp,
        }),
    );
    data.insert(
        "hooks".into(),
        Value::Array(built.hooks.iter().map(hooks::Entry::to_json).collect()),
    );
    data.insert("mcpServers".into(), Value::Array(servers));
    data.insert(
        "options".into(),
        Value::Array(options.iter().map(OptionRow::to_json).collect()),
    );
    data.insert("knobs".into(), json!([]));
    data.insert("warnings".into(), json!(warnings));
    Ok(Value::Object(data))
}

fn tok(v: &Value) -> String {
    v["value"]
        .as_u64()
        .map(|n| format!("~{n} tok ({})", v["basis"].as_str().unwrap_or("")))
        .unwrap_or_else(|| "-".to_string())
}

pub fn ls_text(data: &Value) -> String {
    let mut out = String::new();
    let rows = data["plugins"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        out.push_str("no plugins installed\n");
    }
    for r in &rows {
        let b = &r["brings"];
        out.push_str(&format!(
            "{:<32} {:<10} {:<8} {} skills, {} agents, {} commands, {} hooks, {} mcp  always-on {}\n",
            r["id"].as_str().unwrap_or(""),
            r["version"].as_str().unwrap_or(""),
            if r["enabled"]["effective"] == Value::Bool(true) { "on" } else { "off" },
            b["skills"],
            b["agents"],
            b["commands"],
            b["hooks"],
            b["mcpServers"],
            tok(&r["cost"]["projected"]),
        ));
    }
    if let Some(e) = data["refreshError"].as_str() {
        out.push_str(&format!("marketplace refresh warning: {e}\n"));
    }
    for w in data["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        out.push_str(&format!("warning: {w}\n"));
    }
    out
}

pub fn show_text(data: &Value) -> String {
    let mut out = format!(
        "{} {} ({})\n",
        data["id"].as_str().unwrap_or(""),
        data["version"].as_str().unwrap_or(""),
        if data["enabled"]["effective"] == Value::Bool(true) { "on" } else { "off" }
    );
    if let Some(d) = data["description"].as_str().filter(|d| !d.is_empty()) {
        out.push_str(&format!("{d}\n"));
    }
    let b = &data["brings"];
    out.push_str(&format!(
        "brings: {} skills, {} agents, {} commands, {} hooks, {} mcp servers, {} lsp servers\n",
        b["skills"], b["agents"], b["commands"], b["hooks"], b["mcpServers"], b["lspServers"]
    ));
    out.push_str(&format!(
        "always-on: {}  measured: {}\n",
        tok(&data["cost"]["projected"]),
        tok(&data["cost"]["measured"])
    ));
    for m in data["mcpServers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "mcp {}  {}\n",
            m["key"].as_str().unwrap_or(""),
            if m["denied"]["user"] == Value::Bool(true)
                || m["denied"]["project"] == Value::Bool(true)
                || m["denied"]["local"] == Value::Bool(true)
                || m["denied"]["managed"] == Value::Bool(true)
            {
                "denied"
            } else {
                "allowed"
            }
        ));
    }
    for o in data["options"].as_array().into_iter().flatten() {
        let shown = if o["sensitive"] == Value::Bool(true) {
            if o["configured"] == Value::Bool(true) { "set".to_string() } else { "unset".to_string() }
        } else {
            o["current"].as_str().unwrap_or("unset").to_string()
        };
        out.push_str(&format!("option {} = {shown}\n", o["key"].as_str().unwrap_or("")));
    }
    for w in data["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        out.push_str(&format!("warning: {w}\n"));
    }
    out
}
