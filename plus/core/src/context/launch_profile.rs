//! Launch profiles: a named start of Claude Code that carries its own client id
//! `claude-code-<name>`, so the gateway applies the profile's access set to that start alone.
//! The binding is upstream's `client_scopes` entry, the discovery override is copied from
//! `claude-code`, and `mcp.json` holds the one gateway entry the launch passes with
//! `--strict-mcp-config`. Every change shows its plan first and names its undo.

use super::bundle_apply::folder_key;
use super::bundle_controls::{self as controls, Run};
use super::bundle_ledger as ledger;
use super::bundle_store::op as bundle_op;
use super::bundle_use::shell_word;
use super::config::{self, LaunchProfile};
use super::fsx;
use super::plan::{Diff, Effects, PlanV1, ResultV1, Step};
use super::Roots;
use crate::op::OpError;
use crate::reach::{self, FolderRow, Scope};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use toolport_plus_api::{AccessSetInfo, Host, RegistryView, ScopeKind, ScopeReach};

pub const BASE_CLIENT: &str = "claude-code";
pub const CLIENT_PREFIX: &str = "claude-code-";
const GATEWAY_ENTRY: &str = "toolport";
const CLIENT_ID_ENV: &str = "TOOLPORT_CLIENT_ID";
const PROFILE_ENV: &str = "TOOLPORT_PROFILE";
const MCP_FILE: &str = "mcp.json";
const MAX_NAME: usize = 64;
const SETTINGS_CAP: usize = 1024 * 1024;
const UNVERIFIED: &str = "unverified: whether Claude Code honours deniedMcpServers from a folder's settings.local.json; one real launch on the Mac has to confirm it before you rely on it";

pub fn client_id(name: &str) -> String {
    format!("{CLIENT_PREFIX}{name}")
}

pub fn control_name(name: &str) -> String {
    format!("launch-policy:{name}")
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    !bytes.is_empty()
        && bytes.len() <= MAX_NAME
        && alnum(bytes[0])
        && bytes.iter().all(|&b| alnum(b) || b == b'-')
}

struct Env<'a> {
    host: &'a dyn Host,
    roots: Roots,
    data_dir: PathBuf,
}

impl Env<'_> {
    fn config_path(&self) -> PathBuf {
        self.roots.context_config_path()
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.roots
            .home
            .join(".config")
            .join("toolport")
            .join("profiles")
            .join(name)
    }

    fn mcp_file(&self, name: &str) -> PathBuf {
        self.dir(name).join(MCP_FILE)
    }

    fn view(&self) -> Result<Box<dyn RegistryView>, OpError> {
        self.host
            .read_view()
            .map_err(|e| OpError::failed("registry", e))?
            .ok_or_else(|| OpError::failed("no_registry", "no Toolport registry was found"))
    }
}

fn env(host: &dyn Host) -> Result<Env<'_>, OpError> {
    let home = host
        .home_dir()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data_dir = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    Ok(Env {
        host,
        roots: Roots::from_home(&home),
        data_dir,
    })
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default() + "\n"
}

fn find_set(view: &dyn RegistryView, reference: &str) -> Result<AccessSetInfo, OpError> {
    let sets = view.access_sets();
    let wanted = reference.trim();
    sets.iter()
        .find(|set| set.id == wanted)
        .or_else(|| {
            sets.iter()
                .find(|set| set.name.eq_ignore_ascii_case(wanted))
        })
        .cloned()
        .ok_or_else(|| {
            let known: Vec<&str> = sets.iter().map(|set| set.name.as_str()).collect();
            OpError::not_found(format!(
                "no access set called {wanted}; the access sets are: {}",
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            ))
        })
}

fn bound_reference(view: &dyn RegistryView, id: &str) -> Option<String> {
    view.scope_reach()
        .into_iter()
        .find(|scope| scope.kind == ScopeKind::Client && scope.key == id)
        .map(|scope| scope.reference)
}

fn expected_mcp(gateway: &str, id: &str, set_id: &str) -> String {
    pretty(&json!({
        "mcpServers": {
            GATEWAY_ENTRY: {
                "command": gateway,
                "args": [],
                "env": { CLIENT_ID_ENV: id, PROFILE_ENV: set_id },
            }
        }
    }))
}

fn launch_command(file: &Path) -> String {
    format!(
        "claude --strict-mcp-config --mcp-config {}",
        shell_word(&fsx::display(file))
    )
}

/// The folder mappings a launch can fall under. Started in a folder, the gateway applies the one
/// mapping its own matcher picks for the folder as spelled, so only that one counts; without a
/// folder every mapping counts.
fn mapped_folders(view: &dyn RegistryView, cwd: Option<&Path>) -> Vec<ScopeReach> {
    match cwd {
        Some(cwd) => view
            .folder_route_for(&cwd.to_string_lossy())
            .into_iter()
            .collect(),
        None => view
            .scope_reach()
            .into_iter()
            .filter(|scope| scope.kind == ScopeKind::Folder)
            .collect(),
    }
}

fn warnings_for(
    view: &dyn RegistryView,
    name: &str,
    set: &AccessSetInfo,
    cwd: Option<&Path>,
) -> Vec<Value> {
    let Ok(servers) = view.servers() else {
        return Vec::new();
    };
    let enabled: Vec<&str> = servers
        .iter()
        .filter(|server| server.enabled)
        .map(|server| server.id.as_str())
        .collect();
    let own = Scope {
        set_id: Some(set.id.clone()),
        set_name: Some(set.name.clone()),
        dangling: false,
        server_ids: set
            .server_ids
            .iter()
            .filter(|id| enabled.contains(&id.as_str()))
            .cloned()
            .collect(),
    };
    let names: std::collections::BTreeMap<String, String> = view
        .access_sets()
        .into_iter()
        .map(|set| (set.id, set.name))
        .collect();
    mapped_folders(view, cwd)
        .iter()
        .filter(|scope| !scope.dangling)
        .filter(|scope| scope.set_id.as_deref() != Some(set.id.as_str()))
        .filter_map(|scope| {
            let folder = FolderRow {
                path: scope.key.clone(),
                scope: reach::scope_of(scope, &names),
            };
            reach::folder_outranks(
                &servers,
                &client_id(name),
                &format!("launch profile {name}"),
                &own,
                &folder,
            )
        })
        .map(|finding| {
            json!({
                "folder": finding.folder,
                "text": finding.text,
                "servers": finding.servers,
                "rule": reach::FOLDER_RULE,
            })
        })
        .collect()
}

fn texts(warnings: &[Value]) -> Vec<String> {
    warnings
        .iter()
        .filter_map(|w| w["text"].as_str().map(String::from))
        .collect()
}

fn policy_folders(data_dir: &Path, name: &str) -> Vec<String> {
    let control = control_name(name);
    ledger::load(data_dir)
        .controls
        .iter()
        .filter(|(_, rec)| rec.contains_key(&control))
        .map(|(folder, _)| folder.clone())
        .collect()
}

fn state_of(
    env: &Env,
    view: &dyn RegistryView,
    name: &str,
    profile: &LaunchProfile,
) -> (&'static str, Vec<String>, Option<AccessSetInfo>) {
    let id = client_id(name);
    let set = find_set(view, &profile.access_set).ok();
    let mut problems = Vec::new();
    let mut state = "ok";
    let mut flag = |label: &'static str, text: String| {
        if state == "ok" {
            state = label;
        }
        problems.push(text);
    };
    match &set {
        None => flag(
            "set-missing",
            format!(
                "the access set {} no longer exists, so the launch sees no servers",
                profile.access_set
            ),
        ),
        Some(set) => {
            let bound = bound_reference(view, &id);
            let current = bound
                .as_deref()
                .and_then(|reference| find_set(view, reference).ok());
            if current.as_ref().map(|s| &s.id) != Some(&set.id) {
                flag(
                    "unbound",
                    format!("{id} is not bound to {} in the registry", set.name),
                );
            }
            let file = env.mcp_file(name);
            match fsx::read_text(&file, SETTINGS_CAP) {
                None => flag(
                    "file-missing",
                    format!("{} does not exist", fsx::display(&file)),
                ),
                Some(text) => {
                    let wanted = env
                        .host
                        .gateway_command()
                        .map(|gateway| expected_mcp(&gateway, &id, &set.id));
                    if wanted.is_some_and(|wanted| wanted != text) {
                        flag(
                            "stale-file",
                            format!(
                                "{} differs from what `launch-profile add` would write",
                                fsx::display(&file)
                            ),
                        );
                    }
                }
            }
        }
    }
    (state, problems, set)
}

fn profile_value(
    env: &Env,
    view: &dyn RegistryView,
    name: &str,
    profile: &LaunchProfile,
    cwd: Option<&Path>,
) -> Value {
    let (state, problems, set) = state_of(env, view, name, profile);
    let id = client_id(name);
    let warnings = set
        .as_ref()
        .map(|set| warnings_for(view, name, set, cwd))
        .unwrap_or_default();
    json!({
        "name": name,
        "clientId": id,
        "accessSet": set.as_ref().map(|s| json!({"id": s.id, "name": s.name})),
        "accessSetRef": profile.access_set,
        "denyDirect": profile.deny_direct,
        "discovery": view.client_discovery(&id),
        "state": state,
        "problems": problems,
        "mcpFile": fsx::display(&env.mcp_file(name)),
        "command": launch_command(&env.mcp_file(name)),
        "policyFolders": policy_folders(&env.data_dir, name),
        "folderWarnings": warnings,
    })
}

pub fn ls(host: &dyn Host, name: Option<&str>, cwd: Option<&str>) -> Result<Value, OpError> {
    let env = env(host)?;
    let cwd = checked_cwd(cwd)?;
    let profiles = config::launch_profiles(&env.config_path());
    if let Some(name) = name {
        if !profiles.contains_key(name) {
            return Err(OpError::not_found(format!(
                "no launch profile called {name}"
            )));
        }
    }
    let view = env.view()?;
    let rows: Vec<Value> = profiles
        .iter()
        .filter(|(profile_name, _)| name.is_none_or(|n| n == profile_name.as_str()))
        .map(|(profile_name, profile)| {
            profile_value(&env, view.as_ref(), profile_name, profile, cwd.as_deref())
        })
        .collect();
    Ok(json!({ "profiles": rows, "base": BASE_CLIENT }))
}

fn checked_cwd(cwd: Option<&str>) -> Result<Option<PathBuf>, OpError> {
    match cwd {
        None => Ok(None),
        Some(raw) => {
            let path = Path::new(raw);
            match fsx::folder_problem(path) {
                Some(problem) => Err(OpError::usage(format!("cwd {problem}"))),
                None => Ok(Some(path.to_path_buf())),
            }
        }
    }
}

fn note(detail: impl Into<String>) -> Step {
    Step {
        op: "note",
        path: None,
        detail: detail.into(),
        keys: None,
        diff: None,
    }
}

fn file_step(file: &Path, before: Option<&str>, after: &str, detail: String) -> Option<Step> {
    if before == Some(after) {
        return None;
    }
    let diff = Diff {
        before: before.unwrap_or_default().to_string(),
        after: after.to_string(),
    };
    let path = fsx::display(file);
    let mut step = match before {
        None => Step::create(&path, detail, &[], diff),
        Some(_) => Step::merge(&path, detail, &[], diff),
    };
    step.keys = None;
    Some(step)
}

fn quote_arg(text: &str) -> String {
    shell_word(text)
}

fn fold(run: Run, steps: &mut Vec<Step>, warnings: &mut Vec<String>, changed: &mut Vec<String>) {
    steps.extend(run.steps.into_iter().filter(|s| s.op != "note"));
    warnings.extend(run.warnings);
    changed.extend(run.changed);
}

pub struct AddArgs<'a> {
    pub name: &'a str,
    pub set: Option<&'a str>,
    pub deny_direct: Option<bool>,
    pub dry_run: bool,
}

pub fn add(host: &dyn Host, args: &AddArgs) -> Result<Value, OpError> {
    let name = args.name;
    if !valid_name(name) {
        return Err(OpError::usage(format!(
            "launch profile name {name:?} must match [a-z0-9][a-z0-9-]* and be at most {MAX_NAME} characters"
        )));
    }
    let env = env(host)?;
    let config_path = env.config_path();
    let existing = config::launch_profiles(&config_path).remove(name);
    let view = env.view()?;
    if view.version() < 3 {
        return Err(OpError::failed(
            "access_sets",
            "access sets need a version 3 registry; open Toolport once to migrate it",
        ));
    }
    let reference = match (args.set, &existing) {
        (Some(set), _) => set.to_string(),
        (None, Some(existing)) => existing.access_set.clone(),
        (None, None) => {
            return Err(OpError::usage(
                "--set <access set> is required for a new launch profile",
            ))
        }
    };
    let set = find_set(view.as_ref(), &reference)?;
    let deny = args
        .deny_direct
        .unwrap_or(existing.as_ref().is_some_and(|e| e.deny_direct));
    let profile = LaunchProfile {
        access_set: set.id.clone(),
        deny_direct: deny,
    };
    let gateway = host.gateway_command().ok_or_else(|| {
        OpError::failed(
            "no_gateway",
            "the toolport-gateway program could not be found",
        )
    })?;
    let id = client_id(name);
    let file = env.mcp_file(name);
    let wanted_file = expected_mcp(&gateway, &id, &set.id);
    let before_file = fsx::read_text(&file, SETTINGS_CAP);

    let bound_before = bound_reference(view.as_ref(), &id);
    let bound_id_before = bound_before
        .as_deref()
        .and_then(|reference| find_set(view.as_ref(), reference).ok())
        .map(|s| s.id);
    let base_mode = view.client_discovery(BASE_CLIENT);
    let mode_before = view.client_discovery(&id);

    let mut steps = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let registry_changes = bound_id_before.as_deref() != Some(set.id.as_str())
        || bound_before.as_deref() != Some(set.id.as_str())
        || mode_before != base_mode;
    if registry_changes {
        let registry_path = host
            .registry_path()
            .map(|p| fsx::display(&p))
            .unwrap_or_default();
        let shown = |scope: Option<&str>, mode: &Option<String>| {
            format!(
                "clientScopes.{id}: {}\nclientDiscovery.{id}: {}\n",
                scope.unwrap_or("(none)"),
                mode.as_deref().unwrap_or("(inherits the global mode)")
            )
        };
        steps.push(Step {
            op: "update",
            path: Some(registry_path),
            detail: format!(
                "bind {id} to the access set {} and give it Claude Code's discovery mode",
                set.name
            ),
            keys: Some(vec![
                format!("clientScopes.{id}"),
                format!("clientDiscovery.{id}"),
            ]),
            diff: Some(Diff {
                before: shown(bound_before.as_deref(), &mode_before),
                after: shown(Some(&set.id), &base_mode),
            }),
        });
        if existing.is_none() && bound_before.is_some() {
            warnings.push(format!(
                "{id} already had a binding in the registry; it is taken over by this profile"
            ));
        }
    }
    steps.extend(file_step(
        &file,
        before_file.as_deref(),
        &wanted_file,
        format!("one gateway entry carrying {CLIENT_ID_ENV}={id}"),
    ));
    let entry_before = existing.as_ref().map(|e| {
        format!(
            "accessSet: {}\ndenyDirect: {}\n",
            e.access_set, e.deny_direct
        )
    });
    let entry_after = format!("accessSet: {}\ndenyDirect: {deny}\n", set.id);
    if entry_before.as_deref() != Some(entry_after.as_str()) {
        let diff = Diff {
            before: entry_before.clone().unwrap_or_default(),
            after: entry_after,
        };
        let path = fsx::display(&config_path);
        let key = format!("launchProfiles.{name}");
        steps.push(if entry_before.is_some() {
            Step::merge(&path, format!("change the record of {name}"), &[&key], diff)
        } else {
            Step::create(
                &path,
                format!("record launch profile {name}"),
                &[&key],
                diff,
            )
        });
    }

    let mut changed = Vec::new();
    let mut policy_runs = Vec::new();
    if !deny {
        for folder in policy_folders(&env.data_dir, name) {
            match controls::undo(&env.data_dir, Path::new(&folder), &control_name(name), true) {
                Ok(run) => policy_runs.push((folder, run)),
                Err(e) => warnings.push(format!("{folder}: {}", e.message)),
            }
        }
        for (_, run) in &policy_runs {
            steps.extend(run.steps.iter().filter(|s| s.op != "note").map(clone_step));
        }
    } else {
        steps.push(note(format!(
            "deniedMcpServers is written per folder: `toolportctl context launch-profile policy {name} --cwd <folder>` shows that plan"
        )));
    }

    let warnings_json = warnings_for(view.as_ref(), name, &set, None);
    warnings.extend(texts(&warnings_json));
    if base_mode.is_none() {
        warnings.push(format!(
            "{BASE_CLIENT} has no discovery override, so {id} follows the global mode; the small-catalog Full that {BASE_CLIENT} gets under Auto is built into that client id and cannot be copied"
        ));
    }

    let undo = match &existing {
        Some(e) => format!(
            "toolportctl context launch-profile add {} --set {}{}",
            quote_arg(name),
            quote_arg(&e.access_set),
            if e.deny_direct {
                " --deny-direct on"
            } else {
                " --deny-direct off"
            }
        ),
        None => format!("toolportctl context launch-profile rm {}", quote_arg(name)),
    };
    let verb = if existing.is_some() { "Update" } else { "Add" };
    let summary = if steps.iter().all(|s| s.op == "note") {
        format!("Launch profile {name} is up to date")
    } else {
        format!(
            "{verb} launch profile {name}: Claude Code started with {id} sees the access set {}",
            set.name
        )
    };
    let plan = PlanV1 {
        summary,
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };

    if !args.dry_run {
        if registry_changes {
            let (id, set_id, mode) = (id.clone(), set.id.clone(), base_mode.clone());
            host.transact(&mut |tx| {
                tx.set_client_scope(&id, Some(&set_id))?;
                tx.set_client_discovery(&id, mode.as_deref())
            })
            .map_err(|e| OpError::failed("registry", e))?;
            changed.push("registry".to_string());
        }
        if before_file.as_deref() != Some(wanted_file.as_str()) {
            crate::fsx::atomic_write(&file, &wanted_file, crate::fsx::Mode::KeepOrDefault)
                .map_err(|e| OpError::failed("write", format!("{}: {e}", file.display())))?;
            changed.push(fsx::display(&file));
        }
        if existing.as_ref() != Some(&profile) {
            config::set_launch_profile(&config_path, name, Some(&profile))
                .map_err(|e| OpError::failed("write", e))?;
            changed.push(fsx::display(&config_path));
        }
        for (folder, _) in &policy_runs {
            let run = controls::undo(&env.data_dir, Path::new(folder), &control_name(name), false)
                .map_err(bundle_op)?;
            changed.extend(run.changed);
        }
    }
    let result = (!args.dry_run).then(|| ResultV1 {
        applied: true,
        changed,
        undo: undo.clone(),
        backups: Vec::new(),
    });
    let row = if args.dry_run {
        Value::Null
    } else {
        profile_value(&env, env.view()?.as_ref(), name, &profile, None)
    };
    Ok(json!({
        "dryRun": args.dry_run,
        "name": name,
        "clientId": id,
        "created": existing.is_none(),
        "plan": plan,
        "result": result,
        "profile": row,
        "folderWarnings": warnings_json,
    }))
}

fn clone_step(step: &Step) -> Step {
    Step {
        op: step.op,
        path: step.path.clone(),
        detail: step.detail.clone(),
        keys: step.keys.clone(),
        diff: step.diff.as_ref().map(|d| Diff {
            before: d.before.clone(),
            after: d.after.clone(),
        }),
    }
}

pub fn rm(host: &dyn Host, name: &str, dry_run: bool) -> Result<Value, OpError> {
    let env = env(host)?;
    let config_path = env.config_path();
    let existing = config::launch_profiles(&config_path).remove(name);
    let view = env.view()?;
    let id = client_id(name);
    let bound = bound_reference(view.as_ref(), &id);
    let mode = view.client_discovery(&id);
    let file = env.mcp_file(name);
    let file_text = fsx::read_text(&file, SETTINGS_CAP);
    let folders = policy_folders(&env.data_dir, name);
    if existing.is_none() && bound.is_none() && mode.is_none() && file_text.is_none() {
        return Err(OpError::not_found(format!(
            "no launch profile called {name}"
        )));
    }

    let mut steps = Vec::new();
    let mut warnings = Vec::new();
    if bound.is_some() || mode.is_some() {
        steps.push(Step {
            op: "update",
            path: host.registry_path().map(|p| fsx::display(&p)),
            detail: format!("remove the binding and the discovery override of {id}"),
            keys: Some(vec![
                format!("clientScopes.{id}"),
                format!("clientDiscovery.{id}"),
            ]),
            diff: Some(Diff {
                before: format!(
                    "clientScopes.{id}: {}\nclientDiscovery.{id}: {}\n",
                    bound.as_deref().unwrap_or("(none)"),
                    mode.as_deref().unwrap_or("(none)")
                ),
                after: String::new(),
            }),
        });
    }
    if let Some(text) = &file_text {
        steps.push(Step {
            op: "delete",
            path: Some(fsx::display(&file)),
            detail: "remove the generated mcp.json".into(),
            keys: None,
            diff: Some(Diff {
                before: text.clone(),
                after: String::new(),
            }),
        });
    }
    if let Some(e) = &existing {
        steps.push(Step {
            op: "update",
            path: Some(fsx::display(&config_path)),
            detail: format!("remove the record of {name}"),
            keys: Some(vec![format!("launchProfiles.{name}")]),
            diff: Some(Diff {
                before: format!(
                    "accessSet: {}\ndenyDirect: {}\n",
                    e.access_set, e.deny_direct
                ),
                after: String::new(),
            }),
        });
    }
    let mut runs = Vec::new();
    for folder in &folders {
        match controls::undo(&env.data_dir, Path::new(folder), &control_name(name), true) {
            Ok(run) => {
                steps.extend(run.steps.iter().filter(|s| s.op != "note").map(clone_step));
                runs.push(folder.clone());
            }
            Err(e) => warnings.push(format!("{folder}: {}", e.message)),
        }
    }
    let undo = match &existing {
        Some(e) => format!(
            "toolportctl context launch-profile add {} --set {}{}",
            quote_arg(name),
            quote_arg(&e.access_set),
            if e.deny_direct {
                " --deny-direct on"
            } else {
                ""
            }
        ),
        None => format!(
            "nothing recorded {name}; the registry binding and file were left behind and are removed"
        ),
    };
    let plan = PlanV1 {
        summary: format!("Remove launch profile {name}"),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };

    let mut changed = Vec::new();
    if !dry_run {
        if bound.is_some() || mode.is_some() {
            let id = id.clone();
            host.transact(&mut |tx| {
                tx.set_client_scope(&id, None)?;
                tx.set_client_discovery(&id, None)
            })
            .map_err(|e| OpError::failed("registry", e))?;
            changed.push("registry".to_string());
        }
        if file_text.is_some() {
            std::fs::remove_file(&file)
                .map_err(|e| OpError::failed("write", format!("{}: {e}", file.display())))?;
            let _ = std::fs::remove_dir(env.dir(name));
            changed.push(fsx::display(&file));
        }
        if existing.is_some() {
            config::set_launch_profile(&config_path, name, None)
                .map_err(|e| OpError::failed("write", e))?;
            changed.push(fsx::display(&config_path));
        }
        for folder in &runs {
            let run = controls::undo(&env.data_dir, Path::new(folder), &control_name(name), false)
                .map_err(bundle_op)?;
            changed.extend(run.changed);
        }
    }
    let result = (!dry_run).then(|| ResultV1 {
        applied: true,
        changed,
        undo: undo.clone(),
        backups: Vec::new(),
    });
    Ok(json!({
        "dryRun": dry_run,
        "name": name,
        "clientId": id,
        "plan": plan,
        "result": result,
    }))
}

/// The direct entries of Claude Code that the profile's access set leaves out, by entry name.
fn left_out_entries(view: &dyn RegistryView, set: &AccessSetInfo) -> Result<Vec<String>, OpError> {
    let all = view
        .direct_entries()
        .map_err(|e| OpError::failed("registry", e))?;
    Ok(all
        .get(BASE_CLIENT)
        .into_iter()
        .flatten()
        .filter(|(_, record)| !record.server.is_empty() && !set.server_ids.contains(&record.server))
        .map(|(entry, _)| entry.clone())
        .collect())
}

pub fn policy(host: &dyn Host, name: &str, cwd: &str, dry_run: bool) -> Result<Value, OpError> {
    let env = env(host)?;
    let folder = checked_cwd(Some(cwd))?.unwrap_or_default();
    let profile = config::launch_profiles(&env.config_path())
        .remove(name)
        .ok_or_else(|| OpError::not_found(format!("no launch profile called {name}")))?;
    let view = env.view()?;
    let control = control_name(name);
    let record = controls::record(&env.data_dir, &folder, &control);
    let mut warnings = Vec::new();
    let (run, denied) = if !profile.deny_direct {
        match record {
            Some(_) => {
                warnings.push(format!(
                    "policy rendering is off for {name}, so the keys Toolport wrote in this folder are put back"
                ));
                (
                    Some(
                        controls::undo(&env.data_dir, &folder, &control, dry_run)
                            .map_err(bundle_op)?,
                    ),
                    Vec::new(),
                )
            }
            None => {
                warnings.push(format!(
                    "policy rendering is off for {name}; turn it on with `toolportctl context launch-profile add {name} --deny-direct on`"
                ));
                (None, Vec::new())
            }
        }
    } else {
        warnings.push(format!(
            "the keys go into {}/.claude/settings.local.json and apply to every Claude Code session started in that folder, not only launches of {name}",
            fsx::display(&folder)
        ));
        warnings.push(UNVERIFIED.to_string());
        warnings.push(format!("`context launch-profile add {name} --deny-direct off` removes the keys again in every folder they were written to"));
        let set = find_set(view.as_ref(), &profile.access_set)?;
        let denied = left_out_entries(view.as_ref(), &set)?;
        let want = super::bundle_apply::Desired {
            deny_servers: denied.clone(),
            ..Default::default()
        };
        (
            Some(
                controls::apply(&env.data_dir, &folder, &control, &want, dry_run)
                    .map_err(bundle_op)?,
            ),
            denied,
        )
    };
    let mut steps = Vec::new();
    let mut changed = Vec::new();
    let mut conflicts = Vec::new();
    if let Some(run) = run {
        conflicts = run.conflicts.clone();
        fold(run, &mut steps, &mut warnings, &mut changed);
    }
    let undo = if profile.deny_direct {
        format!(
            "toolportctl context launch-profile add {} --deny-direct off",
            quote_arg(name)
        )
    } else {
        format!(
            "toolportctl context launch-profile add {} --deny-direct on",
            quote_arg(name)
        )
    };
    let plan = PlanV1 {
        summary: if profile.deny_direct {
            format!(
                "Keep {} direct entr{} out of {}",
                denied.len(),
                if denied.len() == 1 { "y" } else { "ies" },
                folder_key(&folder)
            )
        } else {
            format!("Policy rendering for {name} in {}", folder_key(&folder))
        },
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let result = (!dry_run).then(|| ResultV1 {
        applied: true,
        changed,
        undo,
        backups: Vec::new(),
    });
    Ok(json!({
        "dryRun": dry_run,
        "name": name,
        "cwd": fsx::display(&folder),
        "denyDirect": profile.deny_direct,
        "keys": denied.iter().map(|entry| json!({"serverName": entry})).collect::<Vec<_>>(),
        "plan": plan,
        "result": result,
        "conflicts": conflicts,
    }))
}
