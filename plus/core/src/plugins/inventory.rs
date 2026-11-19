//! `plugins ls|show`: what is installed, where each plugin is on (user, project, local and
//! managed settings), what it brings, what it costs, whether its marketplace offers a newer
//! version, and for one plugin its hooks, MCP servers and options. Reads files, plus the
//! `claude` binary when it is on PATH (list, details, configure); never starts a hook or a server.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::claude::{ClaudeRunner, NOT_FOUND};
use super::detail;
use super::hook_list;
use super::installed::{self, Installed};
use super::settings::{Layers, Scope};
use crate::context::fsx;
use crate::context::measure;
use crate::context::plugin_adapters::Registry;
use crate::op::OpError;
use crate::untrusted;

const REFRESH_TIMEOUT: Duration = Duration::from_secs(180);
const PARALLEL: usize = 8;

pub use super::hook_list::{hooks, HookFilter};

/// The folders the readers look at; a test builds one over a temp tree.
pub struct World {
    pub home: PathBuf,
    pub claude_home: PathBuf,
    /// The administrator's `managed-settings.json`; `None` when it is not looked at.
    pub managed: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
}

impl World {
    pub fn host(host: &dyn Host) -> Option<Self> {
        let home = host.home_dir()?;
        Some(Self {
            claude_home: crate::fsx::claude_dir_in(&home),
            managed: crate::sources::managed_settings_path(),
            data_dir: host.data_dir(),
            home,
        })
    }
}

pub(super) fn shorten(world: &World, text: &str) -> String {
    let home = world.home.to_string_lossy();
    if home.is_empty() {
        return text.to_string();
    }
    text.replace(home.as_ref(), "~")
}

/// The names in a folder of a plugin (somebody else's files): links are not followed.
fn names_in(dir: &Path, want_dirs: bool) -> Vec<String> {
    let mut names: Vec<String> = untrusted::list_dir(dir)
        .into_iter()
        .filter_map(|entry| {
            if want_dirs {
                (entry.kind == untrusted::Kind::Dir
                    && untrusted::is_file(&entry.path.join("SKILL.md")))
                .then_some(entry.name)
            } else {
                entry.name.strip_suffix(".md").map(String::from)
            }
        })
        .collect();
    names.sort();
    names
}

fn lsp_servers(root: &Path) -> Vec<String> {
    let Some(doc) = untrusted::below(root, ".lsp.json")
        .ok()
        .and_then(|path| untrusted::read_json(&path, 4 * 1024 * 1024))
    else {
        return Vec::new();
    };
    let servers = doc.get("lspServers").unwrap_or(&doc);
    let mut keys: Vec<String> = servers
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

pub(super) fn list(world: &World, runner: Option<&dyn ClaudeRunner>) -> installed::Listing {
    installed::list(runner.filter(|r| r.present()), &world.claude_home)
}

pub fn layers(world: &World, cwd: Option<&Path>) -> Layers {
    Layers::load(
        &world.claude_home,
        cwd,
        world.managed.as_deref(),
        Some(&world.home),
    )
}

pub(super) fn managed_info(world: &World, layers: &Layers) -> Value {
    let file = layers.files().iter().find(|f| f.scope == Scope::Managed);
    json!({
        "path": world.managed.as_deref().map(|p| shorten(world, &fsx::display(p))),
        "present": file.is_some(),
        "problem": file.and_then(|f| f.problem.clone()),
    })
}

fn tokens(value: u64, basis: &str) -> Value {
    json!({"value": value, "basis": basis})
}

/// What each plugin costs: Claude Code's projected figure for every plugin (from
/// `plugin details`, a few at a time) and, for a folder, the difference the last
/// `context measure --without plugin:<id>` run measured.
fn projected(runner: Option<&dyn ClaudeRunner>, plugins: &[Installed]) -> BTreeMap<String, u64> {
    let Some(runner) = runner.filter(|r| r.present()) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for chunk in plugins.chunks(PARALLEL) {
        std::thread::scope(|scope| {
            let jobs: Vec<_> = chunk
                .iter()
                .filter(|p| crate::context::plugin_adapters::valid_ident(&p.id))
                .map(|p| {
                    scope.spawn(move || (p.id.clone(), detail::projected_tokens(runner, &p.id)))
                })
                .collect();
            for job in jobs {
                if let Ok((id, Some(value))) = job.join() {
                    out.insert(id, value);
                }
            }
        });
    }
    out
}

struct Read<'a> {
    world: &'a World,
    layers: Layers,
    registry: Registry,
    skills: BTreeMap<String, String>,
    cwd: Option<&'a Path>,
    via: installed::Via,
    projected: BTreeMap<String, u64>,
}

fn row(read: &Read, plugin: &Installed) -> Value {
    let world = read.world;
    let enabled = read.layers.enabled(&plugin.id);
    let root = plugin.install_path.clone().unwrap_or_default();
    let hooks = hook_list::plugin_entries(world, &read.registry, &read.skills, plugin).len();
    let servers = detail::mcp_servers(&root);
    let outside: Vec<Value> = servers
        .iter()
        .map(|(name, _)| {
            let key = detail::mcp_key(&plugin.name, name);
            json!({
                "key": key,
                "name": name,
                "denied": detail::is_denied(&detail::denied(&read.layers, &key)),
            })
        })
        .collect();
    let measured = read
        .cwd
        .zip(world.data_dir.as_deref())
        .and_then(|(cwd, data)| measure::plugin_delta(data, cwd, &plugin.id));
    json!({
        "id": plugin.id,
        "name": plugin.name,
        "marketplace": plugin.marketplace,
        "version": plugin.version,
        "scope": plugin.scope,
        "installPath": plugin.install_path.as_deref().map(|p| shorten(world, &fsx::display(p))),
        "source": plugin.marketplace.as_deref()
            .and_then(|m| installed::marketplace_source(&world.claude_home, m)),
        "enabled": {
            "user": enabled.user,
            "project": enabled.project,
            "local": enabled.local,
            "managed": enabled.managed,
            "effective": enabled.effective,
        },
        "update": detail::update(&world.claude_home, plugin),
        "brings": {
            "skills": names_in(&root.join("skills"), true).len(),
            "agents": names_in(&root.join("agents"), false).len(),
            "commands": names_in(&root.join("commands"), false).len(),
            "hooks": hooks,
            "mcpServers": servers.len(),
            "lspServers": lsp_servers(&root).len(),
        },
        "cost": {
            "projected": read.projected.get(&plugin.id).map(|v| tokens(*v, "projected")),
            "measured": measured.as_ref().map(|m| tokens(m.tokens, "measured")),
            "measuredAt": measured.as_ref().map(|m| m.measured_at.clone()),
        },
        "adapter": read.registry.for_plugin(&plugin.id).map(|a| a.plugin.clone()),
        "selectSource": detail::select_source(world.data_dir.as_deref(), plugin),
        "mcpOutsideGateway": outside,
        "errors": plugin.errors,
        "from": read.via.as_str(),
    })
}

fn open<'a>(
    world: &'a World,
    runner: Option<&dyn ClaudeRunner>,
    cwd: Option<&'a Path>,
    listing: &installed::Listing,
    projected_for: &[Installed],
) -> Read<'a> {
    Read {
        world,
        layers: layers(world, cwd),
        registry: Registry::load(world.data_dir.as_deref()),
        skills: hook_list::skill_hooks(world.data_dir.as_deref()),
        cwd,
        via: listing.via,
        projected: projected(runner, projected_for),
    }
}

/// `claude plugin marketplace update`: fetch what every marketplace offers now. The error, when
/// there is one; the install list is read afterwards either way.
fn refresh(runner: Option<&dyn ClaudeRunner>) -> Option<String> {
    let Some(runner) = runner.filter(|r| r.present()) else {
        return Some(NOT_FOUND.to_string());
    };
    match runner.run(&["plugin", "marketplace", "update"], REFRESH_TIMEOUT) {
        Err(e) => Some(e),
        Ok(out) if !out.ok() => Some(installed::failure(&out, "claude plugin marketplace update")),
        Ok(_) => None,
    }
}

pub fn ls(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    cwd: Option<&Path>,
    refresh_first: bool,
) -> Value {
    let refresh_error = refresh_first.then(|| refresh(runner)).flatten();
    let listing = list(world, runner);
    let read = open(world, runner, cwd, &listing, &listing.plugins);
    let rows: Vec<Value> = listing.plugins.iter().map(|p| row(&read, p)).collect();
    let waiting = rows
        .iter()
        .filter(|r| r["update"]["state"] == "update")
        .count();
    json!({
        "cwd": cwd.map(fsx::display),
        "plugins": rows,
        "marketplaces": detail::marketplaces(&world.claude_home, &listing.plugins),
        "updatesAvailable": waiting,
        "managedSettings": managed_info(world, &read.layers),
        "refreshed": refresh_first && refresh_error.is_none(),
        "refreshError": refresh_error.or_else(|| listing.cli_error.clone()),
        "partial": listing.via == installed::Via::Files && !listing.plugins.is_empty(),
    })
}

pub fn show(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    id: &str,
    cwd: Option<&Path>,
) -> Result<Value, OpError> {
    let listing = list(world, runner);
    let plugin = listing
        .plugins
        .iter()
        .find(|p| p.id == id || p.name == id)
        .ok_or_else(|| OpError::not_found(format!("plugin not found: {id}")))?;
    let read = open(world, runner, cwd, &listing, std::slice::from_ref(plugin));
    let mut data = row(&read, plugin);
    let root: PathBuf = plugin.install_path.clone().unwrap_or_default();
    let manifest = detail::manifest(&root).unwrap_or(Value::Null);
    let mcp: Vec<Value> = detail::mcp_servers(&root)
        .iter()
        .map(|(name, config)| detail::mcp_row(plugin, name, config, &read.layers))
        .collect();
    let (options, options_from) = detail::options(
        runner.filter(|r| r.present() && crate::context::plugin_adapters::valid_ident(&plugin.id)),
        plugin,
        &read.layers,
    );
    let adapter = read.registry.for_plugin(&plugin.id);
    let extra = json!({
        "description": manifest.get("description").and_then(Value::as_str),
        "components": {
            "skills": names_in(&root.join("skills"), true),
            "agents": names_in(&root.join("agents"), false),
            "commands": names_in(&root.join("commands"), false),
            "lspServers": lsp_servers(&root),
        },
        "hooks": hook_list::plugin_entries(world, &read.registry, &read.skills, plugin),
        "mcpServers": mcp,
        "options": options,
        "optionsFrom": options_from,
        "knobs": detail::knobs(adapter, &read.layers, &plugin.id),
        "adapterProblems": read.registry.problems_json(),
    });
    if let (Some(target), Some(source)) = (data.as_object_mut(), extra.as_object()) {
        target.extend(source.clone());
    }
    Ok(data)
}
