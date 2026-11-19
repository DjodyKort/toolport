//! `setup restore` and `setup undo`: reads a setup file, plans every row against this Mac without
//! writing, and applies the plan only when this Mac still matches its hash. Existing repositories,
//! servers, files and memory are never replaced unless a conflict row is taken by name; the
//! default is always "keep mine".

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use toolport_plus_api::{StaticSecret, UpdatedServer};

use super::inspect::{self, Opened, Standing};
use super::repos;
use super::threeway::{hash_json, Baseline, Verdict};
use super::{crypto, sha256_hex, unhex, Ctx, PartId};
use crate::fsx::{atomic_write_bytes, Mode};
use crate::op::OpError;
use crate::skills::gitexec::{git_command, run_command};

const CLONE_TIMEOUT: Duration = Duration::from_secs(600);

pub const STATE_ADD: &str = "add";
pub const STATE_HERE: &str = "here";
pub const STATE_CONFLICT: &str = "conflict";
pub const STATE_NEEDS: &str = "needs";
pub const STATE_SKIP: &str = "skip";
/// Only the other machine changed it since the last sync (sync plans only).
pub const STATE_INCOMING: &str = "incoming";
/// Only this machine changed it since the last sync: nothing to restore, it goes out on a push.
pub const STATE_MINE: &str = "mine";

#[derive(Clone, Debug, Default)]
pub struct RestoreOptions {
    pub file: PathBuf,
    pub passphrase: Option<String>,
    pub parts: Option<BTreeSet<PartId>>,
    pub take: BTreeSet<String>,
    pub apply: bool,
    pub plan: Option<String>,
    /// Set by sync: rows are then judged against what both machines last agreed on.
    pub baseline: Option<Baseline>,
}

#[derive(Clone, Debug)]
struct Write {
    path: PathBuf,
    bytes: Vec<u8>,
    replace: bool,
}

enum Step {
    Clone {
        url: String,
        branch: Option<String>,
        dest: PathBuf,
    },
    Servers(Vec<Value>),
    UpdateServer(Value),
    Access(Value),
    Settings(Map<String, Value>),
    Files(Vec<Write>),
    Switches {
        repo: PathBuf,
        off: bool,
        excludes: Vec<String>,
    },
    Tasks(Vec<Value>),
    Secrets(Vec<StaticSecret>),
}

struct Row {
    id: String,
    part: PartId,
    label: String,
    state: &'static str,
    note: String,
    step: Option<Step>,
    other: Option<String>,
}

impl Row {
    fn new(
        id: impl Into<String>,
        part: PartId,
        label: impl Into<String>,
        state: &'static str,
    ) -> Self {
        Self {
            id: id.into(),
            part,
            label: label.into(),
            state,
            note: String::new(),
            step: None,
            other: None,
        }
    }

    fn hash(mut self, other: String) -> Self {
        self.other = Some(other);
        self
    }

    fn note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    fn step(mut self, step: Step) -> Self {
        self.step = Some(step);
        self
    }
}

fn failed(why: impl Into<String>) -> OpError {
    OpError::failed("restore", why)
}

fn same_file(path: &Path, bytes: &[u8]) -> Option<bool> {
    std::fs::read(path).ok().map(|existing| existing == bytes)
}

/// The state of one file to write: absent here, the same, or different (kept unless taken).
fn file_state(path: &Path, bytes: &[u8]) -> &'static str {
    match same_file(path, bytes) {
        None => STATE_ADD,
        Some(true) => STATE_HERE,
        Some(false) => STATE_CONFLICT,
    }
}

/// Maps the plain state of a row onto the sync states. Without a baseline nothing changes. The
/// flag says the row's content replaces this machine's.
fn judged(
    opts: &RestoreOptions,
    id: &str,
    state: &'static str,
    other: &str,
    local: Option<&str>,
) -> (&'static str, bool) {
    let taken = opts.take.contains(id);
    let Some(baseline) = &opts.baseline else {
        return (state, taken);
    };
    match baseline.judge(id, other, local) {
        Verdict::Same => (STATE_HERE, false),
        Verdict::Absent => (STATE_ADD, false),
        Verdict::RemovedHere | Verdict::Mine => (STATE_MINE, false),
        Verdict::Incoming => (STATE_INCOMING, true),
        Verdict::Conflict => (STATE_CONFLICT, taken),
    }
}

fn is_git(dir: &Path) -> bool {
    dir.join(".git").exists()
}

fn repo_rows(
    ctx: &Ctx<'_>,
    opened: &mut Opened,
    take: &BTreeSet<String>,
    rows: &mut Vec<Row>,
    cloning: &mut BTreeMap<PathBuf, String>,
) -> Result<(), OpError> {
    let doc = opened.read_json("repos.json").map_err(failed)?;
    for entry in doc["repos"].as_array().into_iter().flatten() {
        let path = entry["path"].as_str().unwrap_or_default();
        let name = entry["name"].as_str().unwrap_or(path);
        let dest = ctx.untilde(path);
        let id = format!("repo:{path}");
        let label = format!("{name} → {path}");
        let url = entry["remotes"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|r| r["name"] == "origin")
            .or_else(|| entry["remotes"].get(0))
            .and_then(|r| r["url"].as_str())
            .map(String::from);
        let branch = entry["branch"].as_str().map(String::from);
        if dest.exists() {
            let here = is_git(&dest).then(|| repos::read(&dest, &ctx.home, false));
            let Some(here) = here else {
                rows.push(
                    Row::new(id, PartId::Repos, label, STATE_SKIP)
                        .note("the path is taken by something else"),
                );
                continue;
            };
            let same_remote = match (&url, here.origin_url()) {
                (Some(a), Some(b)) => {
                    crate::sources::gitx::normalize_url(a) == crate::sources::gitx::normalize_url(b)
                }
                (None, None) => true,
                _ => false,
            };
            if !same_remote {
                rows.push(
                    Row::new(id, PartId::Repos, label, STATE_SKIP)
                        .note("the path is taken by another repository"),
                );
            } else if here.branch == branch {
                rows.push(
                    Row::new(id, PartId::Repos, label, STATE_HERE)
                        .note("already here, same branch"),
                );
            } else {
                rows.push(
                    Row::new(id, PartId::Repos, label, STATE_CONFLICT).note(format!(
                        "here on {}, the file says {}: left as it is",
                        here.branch.as_deref().unwrap_or("a detached head"),
                        branch.as_deref().unwrap_or("none"),
                    )),
                );
            }
            continue;
        }
        let Some(url) = url else {
            rows.push(
                Row::new(id, PartId::Repos, label, STATE_SKIP)
                    .note("no remote: it cannot be restored"),
            );
            continue;
        };
        let never_pushed = branch.as_ref().is_some_and(|b| {
            entry["neverPushed"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|n| n.as_str() == Some(b))
        });
        let (checkout, note) = if never_pushed {
            (
                None,
                "the branch was never pushed: clones the default branch".to_string(),
            )
        } else {
            (
                branch.clone(),
                format!(
                    "clone at {}",
                    branch.as_deref().unwrap_or("the default branch")
                ),
            )
        };
        let _ = take;
        cloning.insert(dest.clone(), path.to_string());
        rows.push(
            Row::new(id, PartId::Repos, label, STATE_ADD)
                .note(note)
                .step(Step::Clone {
                    url,
                    branch: checkout,
                    dest,
                }),
        );
    }
    Ok(())
}

fn without_switches(doc: &Value) -> Value {
    let mut copy = doc.clone();
    if let Some(map) = copy.as_object_mut() {
        map.remove("forkSwitches");
        map.remove("id");
    }
    copy
}

fn server_rows(
    ctx: &Ctx<'_>,
    opened: &mut Opened,
    locked: bool,
    opts: &RestoreOptions,
    rows: &mut Vec<Row>,
) -> Result<(), OpError> {
    let doc = opened.read_json("servers.json").map_err(failed)?;
    let here = ctx.host.setup_servers().map_err(failed)?;
    let mut new = Vec::new();
    for server in doc["servers"].as_array().into_iter().flatten() {
        let name = server["name"].as_str().unwrap_or_default();
        let id = format!("server:{name}");
        let existing = here.iter().find(|s| {
            s["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        });
        let compared = |doc: &Value| {
            let mut doc = without_switches(doc);
            if opts.baseline.is_some() {
                if let Some(map) = doc.as_object_mut() {
                    map.remove("disabled");
                }
            }
            hash_json(&doc)
        };
        let other = compared(server);
        let local = existing.map(compared);
        let (state, replace) = judged(
            opts,
            &id,
            match existing {
                Some(_) if local.as_deref() == Some(other.as_str()) => STATE_HERE,
                Some(_) => STATE_CONFLICT,
                None => STATE_ADD,
            },
            &other,
            local.as_deref(),
        );
        let row = Row::new(id, PartId::Servers, name, state).hash(other);
        match state {
            STATE_HERE => rows.push(row.note("already here, same")),
            STATE_MINE if existing.is_none() => rows.push(row.note(
                "removed here since the last sync: it stays removed, the next push leaves it out",
            )),
            STATE_MINE => rows.push(
                row.note("changed only on this machine: it goes out with your next push"),
            ),
            STATE_INCOMING => rows.push(row.step(Step::UpdateServer(server.clone())).note(
                "changed on the other machine: it is replaced, held if what it runs changed; the old one is saved for Undo",
            )),
            STATE_CONFLICT if replace => rows.push(row.step(Step::UpdateServer(server.clone())).note(
                "changed on both machines: replaced by the other machine's, held if what it runs changed; the old one is saved for Undo",
            )),
            STATE_CONFLICT if opts.baseline.is_some() => rows.push(row.note(
                "changed on both machines: yours is kept until you take the other machine's",
            )),
            STATE_CONFLICT => rows.push(row.note(
                "already here and different: yours is kept until you take the other machine's",
            )),
            _ => {
                let secrets = server["env"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .chain(server["launch"]["inputs"].as_array().into_iter().flatten())
                    .any(|e| e["secret"] == true);
                let note = match (secrets, locked) {
                    (true, true) => {
                        "added held, needs a secret value: unlock the secrets or type it"
                    }
                    (true, false) => "added held, secret values come from the unlocked part",
                    _ => "added held for review",
                };
                new.push(server.clone());
                rows.push(row.note(note));
            }
        }
    }
    if let Some(first) = rows
        .iter_mut()
        .find(|r| r.part == PartId::Servers && r.state == STATE_ADD)
    {
        first.step = Some(Step::Servers(new));
    }
    Ok(())
}

fn access_row(ctx: &Ctx<'_>, opened: &mut Opened, rows: &mut Vec<Row>) -> Result<(), OpError> {
    let mut doc = opened.read_json("access.json").map_err(failed)?;
    let here = ctx.host.setup_access().map_err(failed)?;
    let have: BTreeSet<&str> = here["accessSets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["name"].as_str())
        .collect();
    let fresh = doc["accessSets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["name"].as_str().is_some_and(|n| !have.contains(n)))
        .count();
    if let Some(folders) = doc["folderProfiles"].as_array_mut() {
        for folder in folders {
            if let Some(path) = folder["path"].as_str() {
                folder["path"] = Value::from(ctx.untilde(path).to_string_lossy().into_owned());
            }
        }
    }
    let label = "Clients and access sets";
    rows.push(
        if fresh == 0 && doc["clients"].as_array().is_none_or(Vec::is_empty) {
            Row::new("access", PartId::Access, label, STATE_HERE).note("no access set to add")
        } else {
            Row::new("access", PartId::Access, label, STATE_ADD)
                .note(format!(
                    "{fresh} access sets to add, client scopes merged where there is none"
                ))
                .step(Step::Access(doc))
        },
    );
    Ok(())
}

fn settings_rows(
    ctx: &Ctx<'_>,
    opened: &mut Opened,
    opts: &RestoreOptions,
    rows: &mut Vec<Row>,
) -> Result<(), OpError> {
    let doc = opened.read_json("settings.json").map_err(failed)?;
    let here = ctx.host.setup_settings().map_err(failed)?;
    let differing: Map<String, Value> = doc["registry"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, v)| here.get(*k) != Some(*v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let label = "Toolport settings";
    let other = hash_json(&doc["registry"]);
    let local: Map<String, Value> = doc["registry"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, _)| here.get(k).map(|v| (k.clone(), v.clone())))
        .collect();
    let local = hash_json(&Value::Object(local));
    let (state, replace) = judged(
        opts,
        "settings",
        if differing.is_empty() {
            STATE_HERE
        } else {
            STATE_ADD
        },
        &other,
        Some(&local),
    );
    let row = Row::new("settings", PartId::Settings, label, state).hash(other);
    rows.push(match state {
        STATE_HERE => row.note("same values"),
        STATE_MINE => row.note("changed only on this machine: it goes out with your next push"),
        STATE_CONFLICT if !replace => {
            row.note("changed on both machines: yours is kept until you take the other machine's")
        }
        STATE_CONFLICT => row
            .note(format!(
                "{} values change to the other machine's; the old ones are saved for Undo",
                differing.len()
            ))
            .step(Step::Settings(differing)),
        STATE_INCOMING => row
            .note(format!(
                "changed on the other machine: {} values change, the old ones are saved for Undo",
                differing.len()
            ))
            .step(Step::Settings(differing)),
        _ => row
            .note(format!(
                "{} values change; the old ones are saved for Undo",
                differing.len()
            ))
            .step(Step::Settings(differing)),
    });
    let roots = crate::context::Roots::from_home(&ctx.home);
    let dir = crate::context::bundle_store::dir(&roots);
    for (name, text) in doc["bundles"].as_object().into_iter().flatten() {
        let Some(text) = text.as_str() else { continue };
        let path = dir.join(format!("{name}.yaml"));
        let id = format!("settings:bundle:{name}");
        rows.push(file_row(
            opts,
            id,
            PartId::Settings,
            format!("Bundle {name}"),
            path,
            text.as_bytes().to_vec(),
        ));
    }
    if !doc["compression"].is_null() {
        let path = ctx.data_dir.join("compression.json");
        let bytes = serde_json::to_vec_pretty(&doc["compression"]).unwrap_or_default();
        let other = hash_json(&doc["compression"]);
        let existing = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let local = existing.as_ref().map(hash_json);
        let state = match &existing {
            None => STATE_ADD,
            Some(existing) if *existing == doc["compression"] => STATE_HERE,
            Some(_) => STATE_CONFLICT,
        };
        let id = "settings:compression".to_string();
        let (state, replace) = judged(opts, &id, state, &other, local.as_deref());
        let write = Write {
            path,
            bytes,
            replace,
        };
        rows.push(
            file_row_in(id, PartId::Settings, "Compression settings", state, write).hash(other),
        );
    }
    Ok(())
}

/// A file row judged by the hash of its bytes against the file on this machine.
fn file_row(
    opts: &RestoreOptions,
    id: String,
    part: PartId,
    label: impl Into<String>,
    path: PathBuf,
    bytes: Vec<u8>,
) -> Row {
    let state = file_state(&path, &bytes);
    let other = sha256_hex(&bytes);
    let local = std::fs::read(&path).ok().map(|b| sha256_hex(&b));
    let (state, replace) = judged(opts, &id, state, &other, local.as_deref());
    let write = Write {
        path,
        bytes,
        replace,
    };
    file_row_in(id, part, label, state, write).hash(other)
}

fn file_row_in(
    id: String,
    part: PartId,
    label: impl Into<String>,
    state: &'static str,
    write: Write,
) -> Row {
    let note = match state {
        STATE_ADD => "added".to_string(),
        STATE_HERE => "already here, same".to_string(),
        STATE_INCOMING => {
            "changed on the other machine: replaced, yours is saved for Undo".to_string()
        }
        STATE_MINE if !write.path.exists() => {
            "removed here since the last sync: it stays removed, the next push leaves it out"
                .to_string()
        }
        STATE_MINE => "changed only on this machine: it goes out with your next push".to_string(),
        _ if write.replace => {
            "differs: replaced by the file's copy, yours is saved for Undo".to_string()
        }
        _ => "differs: yours is kept".to_string(),
    };
    let row = Row::new(id, part, label, state).note(note);
    if matches!(state, STATE_HERE | STATE_MINE) {
        row
    } else {
        row.step(Step::Files(vec![write]))
    }
}

fn task_rows(ctx: &Ctx<'_>, opened: &mut Opened, rows: &mut Vec<Row>) -> Result<(), OpError> {
    let doc = opened.read_json("tasks.json").map_err(failed)?;
    let base = crate::tasks::triggers::base_of(ctx.host)?;
    let mut new = Vec::new();
    let mut here = 0;
    for task in doc["tasks"].as_array().into_iter().flatten() {
        let id = task["id"].as_str().unwrap_or_default();
        if crate::tasks::store::load_task(&base, id).is_ok() {
            here += 1;
        } else {
            new.push(task.clone());
        }
    }
    if new.is_empty() {
        rows.push(
            Row::new("tasks", PartId::Tasks, "Tasks", STATE_HERE)
                .note(format!("{here} already here")),
        );
    } else {
        rows.push(
            Row::new("tasks", PartId::Tasks, "Tasks", STATE_ADD)
                .note(format!(
                    "{} added, turned off until you turn them on",
                    new.len()
                ))
                .step(Step::Tasks(new)),
        );
    }
    Ok(())
}

fn library_row(opened: &mut Opened, rows: &mut Vec<Row>) {
    let doc = opened
        .read_json("skills-library/library.json")
        .unwrap_or(Value::Null);
    let row = Row::new("library", PartId::Library, "Skills library", STATE_NEEDS);
    rows.push(match doc["remote"].as_str() {
        Some(remote) => row.note(format!(
            "connect it again from Library (remote {remote}); restore does not clone it"
        )),
        None => row.note("the file holds no library to restore"),
    });
}

fn claude_rows(ctx: &Ctx<'_>, opened: &mut Opened, opts: &RestoreOptions, rows: &mut Vec<Row>) {
    for name in opened.names_under("claude-user/") {
        let rel = name.trim_start_matches("claude-user/").to_string();
        let id = format!("claude:{rel}");
        let label = format!("~/.claude/{rel}");
        match rel.as_str() {
            "removed.json" => continue,
            "plugins.json" => {
                rows.push(
                    Row::new(id, PartId::ClaudeUser, label, STATE_NEEDS)
                        .note("the plugin list: install the plugins in Claude Code"),
                );
                continue;
            }
            "settings.json" => {
                rows.push(
                    Row::new(id, PartId::ClaudeUser, label, STATE_NEEDS)
                        .note("the filtered copy is in the file; Claude Code settings are never written by a restore"),
                );
                continue;
            }
            _ => {}
        }
        let Ok(bytes) = opened.zip.read(&name) else {
            continue;
        };
        let path = ctx.claude_home.join(&rel);
        rows.push(file_row(opts, id, PartId::ClaudeUser, label, path, bytes));
    }
}

fn memory_rows(
    ctx: &Ctx<'_>,
    opened: &mut Opened,
    take: &BTreeSet<String>,
    secrets: Option<&Value>,
    cloning: &BTreeMap<PathBuf, String>,
    rows: &mut Vec<Row>,
) -> Result<(), OpError> {
    let index = opened.read_json("memory/index.json").map_err(failed)?;
    let memory = ctx.memory();
    for repo in index["repos"].as_array().into_iter().flatten() {
        let dir = repo["dir"].as_str().unwrap_or_default();
        let name = repo["name"].as_str().unwrap_or(dir);
        let path = repo["path"].as_str().unwrap_or_default();
        let abs = ctx.untilde(path);
        let id = format!("memory:{dir}");
        let label = format!("Memory of {name}");
        if !abs.is_dir() && !cloning.contains_key(&abs) {
            rows.push(
                Row::new(id, PartId::Memory, label, STATE_SKIP).note("the repository is not here"),
            );
            continue;
        }
        let target = memory.memory_dir(&abs);
        let replace = take.contains(&id);
        let mut writes = Vec::new();
        let (mut add, mut same, mut differ) = (0, 0, 0);
        let mut place = |dest: PathBuf, bytes: Vec<u8>, writes: &mut Vec<Write>| match file_state(
            &dest, &bytes,
        ) {
            STATE_ADD => {
                add += 1;
                writes.push(Write {
                    path: dest,
                    bytes,
                    replace: false,
                });
            }
            STATE_HERE => same += 1,
            _ => {
                differ += 1;
                if replace {
                    writes.push(Write {
                        path: dest,
                        bytes,
                        replace: true,
                    });
                }
            }
        };
        for file in repo["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Ok(bytes) = opened.zip.read(&format!("memory/{dir}/{file}")) {
                place(target.join(file), bytes, &mut writes);
            }
        }
        for file in repo["instructions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Ok(bytes) = opened
                .zip
                .read(&format!("memory/{dir}/instructions/{file}"))
            {
                place(abs.join(file), bytes, &mut writes);
            }
        }
        for flagged in secrets
            .and_then(|s| s["memoryFiles"].as_array())
            .into_iter()
            .flatten()
            .filter(|f| f["dir"].as_str() == Some(dir))
        {
            if let (Some(name), Some(bytes)) = (
                flagged["name"].as_str(),
                flagged["hex"].as_str().and_then(unhex),
            ) {
                place(target.join(name), bytes, &mut writes);
            }
        }
        let state = if differ > 0 {
            STATE_CONFLICT
        } else if add > 0 {
            STATE_ADD
        } else {
            STATE_HERE
        };
        let note =
            format!("{add} to add, {same} already here, {differ} differ (kept unless taken)");
        let mut row = Row::new(id, PartId::Memory, label, state).note(note);
        if !writes.is_empty() {
            row.step = Some(Step::Files(writes));
        }
        rows.push(row);
    }
    for switch in index["switches"].as_array().into_iter().flatten() {
        let path = switch["path"].as_str().unwrap_or_default();
        let abs = ctx.untilde(path);
        let id = format!("switch:{path}");
        let off = switch["autoMemory"] == "off";
        let excludes: Vec<String> = switch["claudeMdExcludes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let local: Value = std::fs::read(abs.join(".claude/settings.local.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        let missing = (off && local.get("autoMemoryEnabled").is_none())
            || (!excludes.is_empty() && local.get("claudeMdExcludes").is_none());
        let label = format!("Memory switches of {path}");
        if !abs.is_dir() && !cloning.contains_key(&abs) {
            rows.push(
                Row::new(id, PartId::Memory, label, STATE_SKIP).note("the repository is not here"),
            );
        } else if missing {
            rows.push(
                Row::new(id, PartId::Memory, label, STATE_ADD)
                    .note("the switches come back")
                    .step(Step::Switches {
                        repo: abs,
                        off,
                        excludes,
                    }),
            );
        } else {
            rows.push(Row::new(id, PartId::Memory, label, STATE_HERE).note("already set here"));
        }
    }
    Ok(())
}

pub struct Plan {
    rows: Vec<Row>,
    locked: bool,
}

fn secrets_of(opened: &mut Opened, passphrase: Option<&str>) -> Result<Option<Value>, OpError> {
    let Some(pass) = passphrase else {
        return Ok(None);
    };
    if !opened.usable(PartId::Secrets) {
        return Ok(None);
    }
    opened
        .unseal_json("secrets.enc", pass)
        .map(Some)
        .map_err(|e| match e {
            crypto::OpenError::Unreadable(why) => failed(why),
            _ => OpError::failed(
                "passphrase",
                "that passphrase does not open the secrets part; nothing was changed",
            ),
        })
}

fn build(ctx: &Ctx<'_>, opened: &mut Opened, opts: &RestoreOptions) -> Result<Plan, OpError> {
    let secrets = secrets_of(opened, opts.passphrase.as_deref())?;
    let sealed_here = opened.part(PartId::Secrets).is_some();
    let locked = sealed_here && secrets.is_none();
    let wanted = |id: PartId| opts.parts.as_ref().is_none_or(|p| p.contains(&id));
    let mut rows = Vec::new();
    let mut cloning = BTreeMap::new();
    for (id, label) in [
        (PartId::Repos, "Repositories"),
        (PartId::Servers, "Servers"),
        (PartId::Access, "Clients and access sets"),
        (PartId::Library, "Skills library"),
        (PartId::Tasks, "Tasks"),
        (PartId::Settings, "Toolport settings"),
        (PartId::ClaudeUser, "Claude Code user files"),
        (PartId::Memory, "Memory"),
    ] {
        if !wanted(id) || opened.part(id).is_none() {
            continue;
        }
        if !opened.usable(id) {
            rows.push(
                Row::new(id.id(), id, label, STATE_SKIP).note("does not match its hash: left out"),
            );
            continue;
        }
        match id {
            PartId::Repos => repo_rows(ctx, opened, &opts.take, &mut rows, &mut cloning)?,
            PartId::Servers => server_rows(ctx, opened, locked, opts, &mut rows)?,
            PartId::Access => access_row(ctx, opened, &mut rows)?,
            PartId::Library => library_row(opened, &mut rows),
            PartId::Tasks => task_rows(ctx, opened, &mut rows)?,
            PartId::Settings => settings_rows(ctx, opened, opts, &mut rows)?,
            PartId::ClaudeUser => claude_rows(ctx, opened, opts, &mut rows),
            PartId::Memory => memory_rows(
                ctx,
                opened,
                &opts.take,
                secrets.as_ref(),
                &cloning,
                &mut rows,
            )?,
            _ => {}
        }
    }
    if wanted(PartId::History) && opened.part(PartId::History).is_some() {
        rows.push(
            Row::new("history", PartId::History, "Sessions", STATE_NEEDS)
                .note("sessions are not restored yet: the sealed part stays in the file"),
        );
    }
    let mut values = Vec::new();
    if wanted(PartId::Secrets) && sealed_here {
        let adding: BTreeSet<&str> = rows
            .iter()
            .filter(|r| r.part == PartId::Servers && r.state == STATE_ADD)
            .map(|r| r.label.as_str())
            .collect();
        for v in secrets
            .iter()
            .flat_map(|s| s["values"].as_array())
            .flatten()
        {
            let server = v["server"].as_str().unwrap_or_default();
            if adding.contains(server) {
                values.push(StaticSecret {
                    server: server.into(),
                    key: v["key"].as_str().unwrap_or_default().into(),
                    kind: v["kind"].as_str().unwrap_or_default().into(),
                    value: v["value"].as_str().unwrap_or_default().into(),
                });
            }
        }
        let row = Row::new("secrets", PartId::Secrets, "Secrets", STATE_ADD);
        rows.push(if locked {
            Row::new("secrets", PartId::Secrets, "Secrets", STATE_NEEDS)
                .note("locked: give the passphrase to include them")
        } else if values.is_empty() {
            Row::new("secrets", PartId::Secrets, "Secrets", STATE_HERE)
                .note("no value belongs to a server this restore adds")
        } else {
            row.note(format!(
                "{} values go to the servers that are added",
                values.len()
            ))
            .step(Step::Secrets(values.clone()))
        });
    }
    Ok(Plan { rows, locked })
}

fn plan_hash(plan: &Plan, opts: &RestoreOptions, file_sha: &str) -> String {
    let mut text = format!("{file_sha}\n{}\n", plan.locked);
    for row in &plan.rows {
        text.push_str(&format!("{}|{}|{}\n", row.id, row.state, row.note));
    }
    for t in &opts.take {
        text.push_str(&format!("take:{t}\n"));
    }
    sha256_hex(text.as_bytes())[..12].to_string()
}

fn plan_json(plan: &Plan, hash: &str, opened: &Opened, judged: bool) -> Value {
    let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
    for row in &plan.rows {
        *counts.entry(row.state).or_default() += 1;
    }
    let count = |s: &str| counts.get(s).copied().unwrap_or(0);
    let mut plan_doc = json!({
        "applied": false,
        "dryRun": true,
        "file": opened.path.to_string_lossy(),
        "plan": hash,
        "locked": plan.locked,
        "summary": {
            "add": count(STATE_ADD),
            "here": count(STATE_HERE),
            "conflict": count(STATE_CONFLICT),
            "needs": count(STATE_NEEDS),
            "skip": count(STATE_SKIP),
        },
        "rows": plan.rows.iter().map(|r| {
            let mut row = json!({
                "id": r.id,
                "part": r.part.id(),
                "label": r.label,
                "state": r.state,
                "note": r.note,
            });
            if let (true, Some(other)) = (judged, &r.other) {
                row["hash"] = json!(other);
            }
            row
        }).collect::<Vec<_>>(),
        "file_info": inspect::summary(opened)["parts"],
    });
    if judged {
        plan_doc["summary"]["incoming"] = json!(count(STATE_INCOMING));
        plan_doc["summary"]["mine"] = json!(count(STATE_MINE));
    }
    plan_doc
}

#[derive(Default, Serialize, Deserialize)]
struct Journal {
    id: String,
    file: String,
    #[serde(default)]
    at: u128,
    #[serde(default)]
    undone: bool,
    settings: Map<String, Value>,
    servers: Vec<String>,
    #[serde(default)]
    updated: Vec<UpdatedServer>,
    access: Option<Value>,
    tasks: Vec<String>,
    clones: Vec<(String, String)>,
    files: Vec<Touched>,
    switches: Vec<(String, Map<String, Value>)>,
    secrets: u64,
}

#[derive(Serialize, Deserialize)]
struct Touched {
    path: String,
    sha: String,
    backup: Option<String>,
}

fn runs_dir(ctx: &Ctx<'_>) -> PathBuf {
    ctx.state_dir().join("runs")
}

fn save_journal(ctx: &Ctx<'_>, journal: &Journal) -> Result<(), OpError> {
    let path = runs_dir(ctx).join(format!("{}.json", journal.id));
    let text = serde_json::to_string_pretty(journal).unwrap_or_default() + "\n";
    atomic_write_bytes(&path, text.as_bytes(), Mode::Private)
        .map_err(|e| failed(format!("{}: {e}", path.display())))
}

fn write_one(ctx: &Ctx<'_>, journal: &mut Journal, write: &Write) -> Result<bool, String> {
    let exists = write.path.exists();
    if exists && !write.replace {
        return Ok(false);
    }
    let backup = if exists {
        let dest = runs_dir(ctx)
            .join(&journal.id)
            .join("backup")
            .join(journal.files.len().to_string());
        let old = std::fs::read(&write.path).map_err(|e| e.to_string())?;
        atomic_write_bytes(&dest, &old, Mode::Private).map_err(|e| e.to_string())?;
        Some(dest.to_string_lossy().into_owned())
    } else {
        None
    };
    atomic_write_bytes(&write.path, &write.bytes, Mode::KeepOrDefault)
        .map_err(|e| format!("{}: {e}", write.path.display()))?;
    journal.files.push(Touched {
        path: write.path.to_string_lossy().into_owned(),
        sha: sha256_hex(&write.bytes),
        backup,
    });
    Ok(true)
}

fn clone_one(url: &str, branch: Option<&str>, dest: &Path) -> Result<String, String> {
    let mut args = vec!["clone", "--quiet"];
    if let Some(branch) = branch {
        args.extend(["--branch", branch]);
    }
    let dest_text = dest.to_string_lossy();
    args.extend(["--", url, &dest_text]);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let out = run_command(git_command(None, &args), CLONE_TIMEOUT)?;
    if !out.ok() {
        return Err(out.stderr.trim().to_string());
    }
    let head = run_command(
        git_command(Some(dest), &["rev-parse", "HEAD"]),
        CLONE_TIMEOUT,
    )?;
    Ok(head.stdout.trim().to_string())
}

fn run_step(
    ctx: &Ctx<'_>,
    journal: &mut Journal,
    step: &Step,
    added: &mut Vec<String>,
) -> Result<String, String> {
    match step {
        Step::Clone { url, branch, dest } => {
            let head = clone_one(url, branch.as_deref(), dest)?;
            journal
                .clones
                .push((dest.to_string_lossy().into_owned(), head));
            Ok("cloned".into())
        }
        Step::Servers(docs) => {
            let made = ctx.host.setup_add_servers(docs)?;
            for server in &made {
                journal.servers.push(server.name.clone());
                added.push(server.name.clone());
            }
            Ok(format!("{} added, held", made.len()))
        }
        Step::UpdateServer(doc) => {
            let updated = ctx.host.setup_update_server(doc)?;
            let note = format!("{} replaced, the old one is saved for Undo", updated.name);
            journal.updated.push(updated);
            Ok(note)
        }
        Step::Access(doc) => {
            journal.access = Some(ctx.host.setup_merge_access(doc)?);
            Ok("merged".into())
        }
        Step::Settings(changes) => {
            let previous = ctx.host.setup_apply_settings(changes)?;
            journal.settings.extend(previous);
            Ok(format!("{} values changed", changes.len()))
        }
        Step::Files(writes) => {
            let mut done = 0;
            for write in writes {
                if write_one(ctx, journal, write)? {
                    done += 1;
                }
            }
            Ok(format!("{done} files written"))
        }
        Step::Switches {
            repo,
            off,
            excludes,
        } => {
            let path = repo.join(".claude/settings.local.json");
            let mut doc: Value = std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_else(|| json!({}));
            let map = doc
                .as_object_mut()
                .ok_or("settings.local.json is not an object")?;
            let mut set = Map::new();
            if *off && !map.contains_key("autoMemoryEnabled") {
                map.insert("autoMemoryEnabled".into(), json!(false));
                set.insert("autoMemoryEnabled".into(), Value::Null);
            }
            if !excludes.is_empty() && !map.contains_key("claudeMdExcludes") {
                map.insert("claudeMdExcludes".into(), json!(excludes));
                set.insert("claudeMdExcludes".into(), Value::Null);
            }
            let text = serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n";
            atomic_write_bytes(&path, text.as_bytes(), Mode::KeepOrDefault)
                .map_err(|e| e.to_string())?;
            journal
                .switches
                .push((repo.to_string_lossy().into_owned(), set));
            Ok("switches set".into())
        }
        Step::Tasks(tasks) => {
            let base = crate::tasks::triggers::base_of(ctx.host).map_err(|e| e.message)?;
            for value in tasks {
                let mut task: crate::tasks::model::Task =
                    serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
                task.enabled = false;
                crate::tasks::store::save_task(&base, &task).map_err(|e| e.message)?;
                journal.tasks.push(task.id);
            }
            Ok(format!("{} added, turned off", tasks.len()))
        }
        Step::Secrets(values) => {
            let mut stored = 0;
            for value in values.iter().filter(|v| added.contains(&v.server)) {
                ctx.host.setup_store_secret(value)?;
                stored += 1;
            }
            journal.secrets += stored;
            Ok(format!("{stored} values stored"))
        }
    }
}

/// Orders the steps: settings, servers, access, tasks, then clones, then files, then secrets.
fn order(step: &Option<Step>) -> u8 {
    match step {
        Some(Step::Settings(_)) => 0,
        Some(Step::Servers(_) | Step::UpdateServer(_)) => 1,
        Some(Step::Access(_)) => 2,
        Some(Step::Tasks(_)) => 3,
        Some(Step::Clone { .. }) => 4,
        Some(Step::Files(_) | Step::Switches { .. }) => 5,
        _ => 6,
    }
}

pub fn run(ctx: &Ctx<'_>, opts: &RestoreOptions) -> Result<Value, OpError> {
    super::scratch::sweep(ctx);
    let mut opened = inspect::open(&opts.file)?;
    if opened.parts.iter().all(|p| p.standing != Standing::Ok) {
        return Err(failed(
            "no part of this file matches its hash: nothing to restore",
        ));
    }
    let file_sha = super::sha256_file(&opts.file).map_err(|e| failed(e.to_string()))?;
    let mut plan = build(ctx, &mut opened, opts)?;
    let hash = plan_hash(&plan, opts, &file_sha);
    if !opts.apply {
        return Ok(plan_json(&plan, &hash, &opened, opts.baseline.is_some()));
    }
    match opts.plan.as_deref() {
        None => {
            return Err(OpError::usage(
                "apply needs the plan hash of a dry run: --plan <hash>",
            ))
        }
        Some(given) if given != hash => {
            return Err(OpError::failed(
                "stale_plan",
                "this Mac changed since the plan was made: run the dry run again",
            ))
        }
        Some(_) => {}
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut journal = Journal {
        id: format!(
            "run-{now:010}-{}",
            &sha256_hex(format!("{hash}{now}").as_bytes())[..8]
        ),
        file: opts.file.to_string_lossy().into_owned(),
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
        ..Journal::default()
    };
    let mut rows = std::mem::take(&mut plan.rows);
    rows.sort_by_key(|r| order(&r.step));
    let mut added = Vec::new();
    let mut results = Vec::new();
    for row in &rows {
        let Some(step) = &row.step else {
            continue;
        };
        let (ok, note) = match run_step(ctx, &mut journal, step, &mut added) {
            Ok(note) => (true, note),
            Err(why) => (false, why),
        };
        results.push(json!({"id": row.id, "part": row.part.id(), "label": row.label, "ok": ok, "note": note}));
    }
    save_journal(ctx, &journal)?;
    let failures = results.iter().filter(|r| r["ok"] == false).count();
    let needs: Vec<Value> = rows
        .iter()
        .filter(|r| r.state == STATE_NEEDS)
        .map(|r| json!({"id": r.id, "label": r.label, "note": r.note}))
        .collect();
    let mut data = json!({
        "applied": true,
        "dryRun": false,
        "run": journal.id,
        "steps": results,
        "failed": failures,
        "needsYou": needs,
        "serversToRemoveOnUndo": journal.servers,
    });
    if opts.baseline.is_some() {
        data["rows"] = rows
            .iter()
            .map(|r| json!({"id": r.id, "part": r.part.id(), "state": r.state, "hash": r.other}))
            .collect();
    }
    Ok(data)
}

fn latest_journal(ctx: &Ctx<'_>, wanted: Option<&str>) -> Result<Journal, OpError> {
    let dir = runs_dir(ctx);
    let mut all: Vec<Journal> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    all.sort_by_key(|j| j.at);
    let pick = match wanted {
        Some(id) => all.into_iter().find(|j| j.id == id).map(|j| j.id),
        None => all.into_iter().rev().find(|j| !j.undone).map(|j| j.id),
    };
    let id = pick.ok_or_else(|| OpError::not_found("no restore run to undo"))?;
    let bytes = std::fs::read(dir.join(format!("{id}.json"))).map_err(|e| failed(e.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|e| failed(format!("{id}: {e}")))
}

/// Undoes a restore run as far as nothing changed since: files still as the restore wrote them,
/// clones without new work. Servers and vault values stay: the result names them.
pub fn undo(ctx: &Ctx<'_>, run_id: Option<&str>, dry_run: bool) -> Result<Value, OpError> {
    let mut journal = latest_journal(ctx, run_id)?;
    if journal.undone {
        return Err(OpError::failed("undone", "that run is already undone"));
    }
    let mut steps = Vec::new();
    let mut step = |what: String, ok: bool, note: &str| {
        steps.push(json!({"what": what, "ok": ok, "note": note}))
    };
    if !journal.settings.is_empty() {
        let result = if dry_run {
            Ok(Map::new())
        } else {
            ctx.host.setup_apply_settings(&journal.settings)
        };
        step(
            format!("{} settings", journal.settings.len()),
            result.is_ok(),
            "old values put back",
        );
    }
    for updated in journal.updated.iter().rev() {
        let result = if dry_run {
            Ok(())
        } else {
            ctx.host.setup_restore_server(updated)
        };
        step(
            format!("server {}", updated.name),
            result.is_ok(),
            match &result {
                Ok(()) => "the old definition is back",
                Err(_) => "kept: changed since the restore",
            },
        );
    }
    if let Some(added) = &journal.access {
        let result = if dry_run {
            Ok(())
        } else {
            ctx.host.setup_unmerge_access(added)
        };
        step(
            "access sets".into(),
            result.is_ok(),
            "what the merge added is removed where unchanged",
        );
    }
    if !journal.tasks.is_empty() {
        let base = crate::tasks::triggers::base_of(ctx.host)?;
        for id in &journal.tasks {
            let ok = dry_run || crate::tasks::store::remove_task(&base, id).is_ok();
            step(format!("task {id}"), ok, "removed");
        }
    }
    for (path, head) in &journal.clones {
        let dir = PathBuf::from(path);
        let here = repos::read(&dir, &ctx.home, false);
        let clean = here.error.is_none()
            && here.dirty == 0
            && here.untracked == 0
            && here.unpushed_commits() == 0
            && here.head.as_deref() == Some(head.as_str());
        if clean && !dry_run {
            let _ = std::fs::remove_dir_all(&dir);
        }
        step(
            ctx.tilde(&dir),
            clean,
            if clean {
                "clone removed"
            } else {
                "kept: it has work of its own"
            },
        );
    }
    for touched in journal.files.iter().rev() {
        let path = PathBuf::from(&touched.path);
        let unchanged = std::fs::read(&path).is_ok_and(|b| sha256_hex(&b) == touched.sha);
        if unchanged && !dry_run {
            match &touched.backup {
                Some(backup) => {
                    if let Ok(old) = std::fs::read(backup) {
                        let _ = atomic_write_bytes(&path, &old, Mode::KeepOrDefault);
                    }
                }
                None => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        step(
            ctx.tilde(&path),
            unchanged,
            if !unchanged {
                "kept: changed since the restore"
            } else if touched.backup.is_some() {
                "your copy put back"
            } else {
                "removed"
            },
        );
    }
    for (repo, set) in &journal.switches {
        if !dry_run {
            let path = PathBuf::from(repo).join(".claude/settings.local.json");
            if let Some(mut doc) = std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            {
                if let Some(map) = doc.as_object_mut() {
                    for key in set.keys() {
                        map.remove(key);
                    }
                    let text = serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n";
                    let _ = atomic_write_bytes(&path, text.as_bytes(), Mode::KeepOrDefault);
                }
            }
        }
        step(
            format!("switches of {}", ctx.tilde(Path::new(repo))),
            true,
            "removed",
        );
    }
    if !dry_run {
        journal.undone = true;
        save_journal(ctx, &journal)?;
    }
    Ok(json!({
        "run": journal.id,
        "dryRun": dry_run,
        "steps": steps,
        "left": {
            "servers": journal.servers,
            "secretValues": journal.secrets,
            "note": "servers and stored values are removed in Servers; a restore does not delete them",
        },
    }))
}
