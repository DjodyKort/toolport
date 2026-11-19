//! The parts of a setup file: what each one reads and the entries it adds to the zip. Every
//! builder reads and returns; none of them writes outside its scratch folder.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use toolport_plus_api::StaticSecret;
use zeroize::{Zeroize, Zeroizing};

use super::mem::{self, KeptFile, MemorySource};
use super::repos::{self, Repo};
use super::{crypto, hex, scan, zipio, Ctx, PartId};
use crate::library::remote as library_remote;
use crate::skills::clock::SystemClock;
use crate::skills::gitexec::{git_command, run_command};

/// Largest file taken from `~/.claude` into the setup file.
const MAX_USER_FILE: u64 = 1024 * 1024;
const HISTORY_INNER: &str = "history.inner.zip";

#[derive(Clone, Debug)]
pub enum Body {
    Bytes(Vec<u8>),
    File(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub body: Body,
}

impl Entry {
    pub fn json(name: &str, value: &Value) -> Self {
        let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
        text.push('\n');
        Self {
            name: name.to_string(),
            body: Body::Bytes(text.into_bytes()),
        }
    }

    pub fn bytes(name: &str, bytes: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            body: Body::Bytes(bytes),
        }
    }

    pub fn size(&self) -> u64 {
        match &self.body {
            Body::Bytes(b) => b.len() as u64,
            Body::File(p) => fs::metadata(p).map_or(0, |m| m.len()),
        }
    }

    pub fn hash(&self) -> Result<String, String> {
        match &self.body {
            Body::Bytes(bytes) => Ok(super::sha256_hex(bytes)),
            Body::File(path) => {
                super::sha256_file(path).map_err(|e| format!("{}: {e}", path.display()))
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub id: PartId,
    pub entries: Vec<Entry>,
    pub count: u64,
    /// One line each, for the screen and the preview.
    pub notes: Vec<String>,
    /// Memory files that look like they hold a token (names only).
    pub flagged: Vec<String>,
    /// What the part reports about itself for the parts list (sizes, sub-counts).
    pub info: Value,
    /// The part could not be read at all.
    pub error: Option<String>,
    /// Sized but not built: sealed parts in a listing.
    pub sized_only: Option<u64>,
}

impl Part {
    pub fn new(id: PartId) -> Self {
        Self {
            id,
            entries: Vec::new(),
            count: 0,
            notes: Vec::new(),
            flagged: Vec::new(),
            info: Value::Object(Map::new()),
            error: None,
            sized_only: None,
        }
    }

    pub fn failed(id: PartId, why: impl Into<String>) -> Self {
        Self {
            error: Some(why.into()),
            ..Self::new(id)
        }
    }

    pub fn bytes(&self) -> u64 {
        self.sized_only
            .unwrap_or_else(|| self.entries.iter().map(Entry::size).sum())
    }

    /// One file: its hash. Several: the hash of the sorted `name NUL hash` lines.
    pub fn hash(&self) -> Result<String, String> {
        if let [only] = self.entries.as_slice() {
            return only.hash();
        }
        let mut lines: BTreeMap<&str, String> = BTreeMap::new();
        for entry in &self.entries {
            lines.insert(&entry.name, entry.hash()?);
        }
        let mut hasher = Sha256::new();
        for (name, hash) in lines {
            hasher.update(name.as_bytes());
            hasher.update([0]);
            hasher.update(hash.as_bytes());
            hasher.update(b"\n");
        }
        Ok(hex(&hasher.finalize()))
    }
}

pub fn server_repos_part(b: &Build<'_>) -> Part {
    super::repo_heads::part(b)
}

/// What the part builders share for one run.
pub struct Build<'a> {
    pub ctx: &'a Ctx<'a>,
    pub opts: &'a super::export::Options,
    pub repos: &'a [Repo],
    pub memory: &'a dyn MemorySource,
    pub scratch: &'a Path,
    /// False for a listing: sealed parts are sized and nothing is read from the vault.
    pub content: bool,
}

impl Build<'_> {
    fn on(&self, sub: &str) -> bool {
        !self.opts.without.contains(sub)
    }
}

/// The key a repository's memory and sessions are stored under: its normalised remote, else a hash
/// of its home-relative path.
pub fn repo_key(repo: &Repo) -> String {
    match repo.origin_url() {
        Some(url) => crate::sources::gitx::normalize_url(url),
        None => format!("path-{}", &super::sha256_hex(repo.path.as_bytes())[..12]),
    }
}

/// A key and a folder name for each repository; two clones of one remote get distinct folders.
pub fn repo_dirs(repos: &[Repo]) -> Vec<(String, String)> {
    let mut used: BTreeSet<String> = BTreeSet::new();
    repos
        .iter()
        .map(|repo| {
            let key = repo_key(repo);
            let base: String = key
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '-') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let mut dir = base.clone();
            let mut n = 1;
            while !used.insert(dir.clone()) {
                n += 1;
                dir = format!("{base}-{n}");
            }
            (key, dir)
        })
        .collect()
}

pub fn repos_part(b: &Build<'_>, roots: &[PathBuf]) -> Part {
    let mut part = Part::new(PartId::Repos);
    let mut kept = Vec::new();
    let mut worktrees = 0;
    let mut no_remote = 0;
    let mut skipped = 0;
    for repo in b.repos {
        if repo.no_remote {
            no_remote += 1;
            if b.opts.skip_no_remote {
                skipped += 1;
                continue;
            }
        }
        let mut value = serde_json::to_value(repo).unwrap_or(Value::Null);
        if let Some(map) = value.as_object_mut() {
            map.remove("ignoredNames");
            if !b.opts.worktrees {
                map.insert("worktrees".into(), json!([]));
            }
        }
        worktrees += if b.opts.worktrees {
            repo.worktrees.len()
        } else {
            0
        };
        kept.push(value);
    }
    part.count = kept.len() as u64;
    part.info = json!({"worktrees": worktrees, "noRemote": no_remote, "skipped": skipped});
    let doc = json!({
        "version": 1,
        "roots": roots.iter().map(|r| b.ctx.tilde(r)).collect::<Vec<_>>(),
        "repos": kept,
    });
    part.entries.push(Entry::json("repos.json", &doc));
    if no_remote > 0 && !b.opts.skip_no_remote {
        part.notes.push(format!(
            "{no_remote} without a remote cannot be cloned again"
        ));
    }
    part
}

pub fn servers_part(b: &Build<'_>) -> Part {
    let servers = match b.ctx.host.setup_servers() {
        Ok(servers) => servers,
        Err(why) => return Part::failed(PartId::Servers, why),
    };
    let secrets = secret_slots(&servers);
    let mut part = Part::new(PartId::Servers);
    part.count = servers.len() as u64;
    part.info = json!({"secretValues": secrets});
    part.entries.push(Entry::json(
        "servers.json",
        &json!({"kind": "toolport-setup-servers", "version": 1, "servers": servers}),
    ));
    part
}

/// How many secret values the servers declare (environment entries and launch inputs).
fn secret_slots(servers: &[Value]) -> u64 {
    servers
        .iter()
        .map(|server| {
            let env = server["env"].as_array().into_iter().flatten();
            let inputs = server["launch"]["inputs"].as_array().into_iter().flatten();
            env.chain(inputs).filter(|e| e["secret"] == true).count() as u64
        })
        .sum()
}

pub fn access_part(b: &Build<'_>) -> Part {
    let mut doc = match b.ctx.host.setup_access() {
        Ok(doc) => doc,
        Err(why) => return Part::failed(PartId::Access, why),
    };
    if let Some(folders) = doc["folderProfiles"].as_array_mut() {
        for folder in folders {
            if let Some(path) = folder["path"].as_str() {
                folder["path"] = Value::from(b.ctx.tilde(Path::new(path)));
            }
        }
    }
    doc["version"] = json!(1);
    let mut part = Part::new(PartId::Access);
    part.count = doc["accessSets"].as_array().map_or(0, Vec::len) as u64;
    part.info = json!({
        "accessSets": part.count,
        "clients": doc["clients"].as_array().map_or(0, Vec::len),
    });
    part.entries.push(Entry::json("access.json", &doc));
    part
}

fn uncommitted_files(repo: &Path) -> Vec<String> {
    let mut cmd = git_command(
        Some(repo),
        &["status", "--porcelain=v1", "--untracked-files=all"],
    );
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    let Ok(out) = run_command(cmd, std::time::Duration::from_secs(30)) else {
        return Vec::new();
    };
    out.stdout
        .lines()
        .filter_map(|line| {
            let path = line.get(3..)?;
            Some(
                path.rsplit(" -> ")
                    .next()
                    .unwrap_or(path)
                    .trim_matches('"')
                    .to_string(),
            )
        })
        .collect()
}

pub fn library_part(b: &Build<'_>) -> Part {
    let mut part = Part::new(PartId::Library);
    let Ok(repo) = library_remote::locate() else {
        part.entries.push(Entry::json(
            "skills-library/library.json",
            &json!({"version": 1, "present": false}),
        ));
        part.notes
            .push("no skills library found on this Mac".into());
        return part;
    };
    let state = library_remote::local_state(&repo);
    let raw_url = library_remote::git_out(&repo, &["config", "--get", "remote.origin.url"])
        .filter(|u| !u.is_empty());
    let remote = raw_url.as_deref().map(repos::clean_url);
    let uncommitted = uncommitted_files(&repo);
    let skills = crate::skills::parser::discover_skills(&repo).len();
    let doc = json!({
        "version": 1,
        "present": true,
        "path": b.ctx.tilde(&repo),
        "remote": remote,
        "branch": state.branch,
        "head": state.head,
        "skills": skills,
        "uncommitted": uncommitted,
    });
    part.count = skills as u64;
    part.info = json!({"uncommitted": uncommitted.len(), "remote": remote});
    part.entries
        .push(Entry::json("skills-library/library.json", &doc));
    if b.opts.library_zip && !uncommitted.is_empty() {
        let names: BTreeSet<String> = uncommitted
            .iter()
            .filter_map(|path| {
                let mut parts = path.split('/');
                matches!(parts.next(), Some("skills" | "rules"))
                    .then(|| parts.next().map(String::from))?
            })
            .collect();
        if names.is_empty() {
            part.notes
                .push("the uncommitted files are not skills or rules: not zipped".into());
        } else if b.content {
            let target = b.scratch.join("uncommitted.zip");
            let options = crate::skills::bundle::BundleOptions {
                output: Some(target.clone()),
                skill_names: Some(names.iter().cloned().collect()),
                clock: &SystemClock,
            };
            match crate::skills::bundle::create_bundle(&repo, &options) {
                Ok(_) => part.entries.push(Entry {
                    name: "skills-library/uncommitted.zip".into(),
                    body: Body::File(target),
                }),
                Err(why) => part
                    .notes
                    .push(format!("the uncommitted skills could not be zipped: {why}")),
            }
        }
    }
    part
}

pub fn tasks_part(b: &Build<'_>) -> Part {
    let mut part = Part::new(PartId::Tasks);
    let base = match crate::tasks::triggers::base_of(b.ctx.host) {
        Ok(base) => base,
        Err(why) => return Part::failed(PartId::Tasks, why.message),
    };
    let mut tasks = Vec::new();
    let dir = crate::tasks::store::tasks_dir(&base);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".json"))
                .filter(|n| !n.starts_with('.'))
                .map(String::from)
        })
        .collect();
    names.sort();
    for id in names {
        match crate::tasks::store::load_task(&base, &id) {
            Ok(task) => tasks.push(serde_json::to_value(task).unwrap_or(Value::Null)),
            Err(why) => part
                .notes
                .push(format!("task {id} was left out: {}", why.message)),
        }
    }
    part.count = tasks.len() as u64;
    part.entries.push(Entry::json(
        "tasks.json",
        &json!({"version": 1, "tasks": tasks}),
    ));
    part
}

fn read_json_file(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// Keys of `compression.json` that belong to one machine.
fn machine_local_key(key: &str) -> bool {
    let key = key.to_lowercase();
    key.contains("port") || key.contains("path") || key.contains("dir") || key == "provider_version"
}

fn without_machine_keys(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| !machine_local_key(k))
                .map(|(k, v)| (k.clone(), without_machine_keys(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub fn settings_part(b: &Build<'_>) -> Part {
    let mut part = Part::new(PartId::Settings);
    let registry = match b.ctx.host.setup_settings() {
        Ok(map) => map,
        Err(why) => return Part::failed(PartId::Settings, why),
    };
    let roots = crate::context::Roots::from_home(&b.ctx.home);
    let source_roots: Vec<String> = read_json_file(&roots.context_config_path())
        .and_then(|doc| doc.get("sourceRoots").cloned())
        .and_then(|v| v.as_array().cloned())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .map(|raw| b.ctx.tilde(&b.ctx.untilde(&raw)))
        .collect();
    let compression = read_json_file(&b.ctx.data_dir.join("compression.json"))
        .map(|doc| without_machine_keys(&doc));
    let mut bundles = Map::new();
    let bundle_dir = crate::context::bundle_store::dir(&roots);
    let mut files: Vec<PathBuf> = fs::read_dir(&bundle_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    files.sort();
    for file in files {
        if let (Some(name), Ok(text)) = (
            file.file_stem().and_then(|s| s.to_str()),
            fs::read_to_string(&file),
        ) {
            if text.len() as u64 <= MAX_USER_FILE {
                bundles.insert(name.to_string(), Value::String(text));
            }
        }
    }
    part.count = (registry.len() + source_roots.len() + bundles.len()) as u64
        + u64::from(compression.is_some());
    part.info = json!({
        "registry": registry.len(),
        "sourceRoots": source_roots.len(),
        "bundles": bundles.len(),
        "compression": compression.is_some(),
    });
    let doc = json!({
        "version": 1,
        "registry": registry,
        "sourceRoots": source_roots,
        "compression": compression,
        "bundles": bundles,
    });
    part.entries.push(Entry::json("settings.json", &doc));
    part
}

fn user_files(dir: &Path, prefix: &str, out: &mut Vec<Entry>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            user_files(&path, &format!("{prefix}/{name}"), out);
        } else if kind.is_file() && entry.metadata().is_ok_and(|m| m.len() <= MAX_USER_FILE) {
            if let Ok(bytes) = fs::read(&path) {
                out.push(Entry::bytes(&format!("{prefix}/{name}"), bytes));
            }
        }
    }
}

fn plugin_list(claude_home: &Path) -> Value {
    let installed = read_json_file(&claude_home.join("plugins/installed_plugins.json"));
    let plugins: Vec<Value> = installed
        .as_ref()
        .and_then(|doc| doc.get("plugins"))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(id, installs)| {
            let scope = installs
                .as_array()
                .and_then(|a| a.first())
                .and_then(|i| i.get("scope"))
                .cloned()
                .unwrap_or(Value::Null);
            json!({"id": id, "scope": scope})
        })
        .collect();
    let marketplaces: Vec<Value> = read_json_file(&claude_home.join("plugins/known_marketplaces.json"))
        .as_ref()
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(name, entry)| json!({"name": name, "source": entry.get("source").cloned().unwrap_or(Value::Null)}))
        .collect();
    json!({"version": 1, "plugins": plugins, "marketplaces": marketplaces})
}

fn looks_secret(bytes: &[u8]) -> bool {
    scan::looks_secret_text(&String::from_utf8_lossy(bytes))
}

pub fn claude_user_part(b: &Build<'_>) -> Part {
    let mut part = Part::new(PartId::ClaudeUser);
    let home = &b.ctx.claude_home;
    // Files copied as they are; the rest below is built here and holds no foreign text.
    let mut copied: Vec<Entry> = Vec::new();
    let mut files: Vec<Entry> = Vec::new();
    if b.on("claude-instructions") {
        if let Ok(bytes) = fs::read(home.join("CLAUDE.md")) {
            copied.push(Entry::bytes("claude-user/CLAUDE.md", bytes));
        }
        user_files(&home.join("rules"), "claude-user/rules", &mut copied);
    }
    let mut removed: Vec<String> = Vec::new();
    if b.on("claude-settings") {
        if let Some(doc) = read_json_file(&home.join("settings.json")) {
            let (kept, names) = scan::filter_settings(&doc);
            removed = names;
            files.push(Entry::json("claude-user/settings.json", &kept));
        }
    }
    let mut plugins = 0;
    if b.on("claude-plugins") {
        let list = plugin_list(home);
        plugins = list["plugins"].as_array().map_or(0, Vec::len);
        if plugins > 0
            || list["marketplaces"]
                .as_array()
                .is_some_and(|m| !m.is_empty())
        {
            files.push(Entry::json("claude-user/plugins.json", &list));
        }
    }
    if b.on("claude-own") {
        for dir in ["agents", "commands", "output-styles", "workflows"] {
            user_files(&home.join(dir), &format!("claude-user/{dir}"), &mut copied);
        }
        if let Ok(bytes) = fs::read(home.join("keybindings.json")) {
            copied.push(Entry::bytes("claude-user/keybindings.json", bytes));
        }
    }
    let mut held: Vec<String> = Vec::new();
    for entry in copied {
        match &entry.body {
            Body::Bytes(bytes) if looks_secret(bytes) => {
                held.push(format!(
                    "~/.claude/{}",
                    entry.name.trim_start_matches("claude-user/")
                ));
            }
            _ => files.push(entry),
        }
    }
    part.count = files.len() as u64;
    part.info = json!({"plugins": plugins, "filtered": removed.len(), "flagged": held.len()});
    if !removed.is_empty() {
        part.notes.push(format!(
            "{} values of settings.json were left out",
            removed.len()
        ));
        files.push(Entry::json(
            "claude-user/removed.json",
            &json!({"version": 1, "removed": removed}),
        ));
    }
    if !held.is_empty() {
        part.notes.push(format!(
            "{} files in ~/.claude look like they hold a token and stay out of the file",
            held.len()
        ));
        part.flagged = held;
    }
    part.entries = files;
    part
}

/// A memory file held back for the sealed part.
#[derive(Clone, Debug)]
pub struct Flagged {
    pub dir: String,
    pub name: String,
    pub bytes: Zeroizing<Vec<u8>>,
}

fn switches_of(repo: &Repo) -> Option<Value> {
    let local = read_json_file(&repo.abs.join(".claude/settings.local.json"))?;
    let off = local.get("autoMemoryEnabled") == Some(&Value::Bool(false));
    let excludes = local
        .get("claudeMdExcludes")
        .cloned()
        .filter(|v| v.as_array().is_some_and(|a| !a.is_empty()));
    (off || excludes.is_some()).then(|| {
        json!({
            "path": repo.path,
            "autoMemory": if off { json!("off") } else { Value::Null },
            "claudeMdExcludes": excludes.unwrap_or(json!([])),
        })
    })
}

pub fn memory_part(b: &Build<'_>) -> (Part, Vec<Flagged>) {
    let mut part = Part::new(PartId::Memory);
    let dirs = repo_dirs(b.repos);
    let mut index = Vec::new();
    let mut flagged_files: Vec<Flagged> = Vec::new();
    let mut switches = Vec::new();
    let mut memory_files = 0u64;
    let mut instruction_files = 0u64;
    let mut instructions_held_total = 0u64;
    let sealing = b.opts.secrets_on();
    for (repo, (key, dir)) in b.repos.iter().zip(&dirs) {
        if repo.error.is_some() {
            continue;
        }
        if b.on("memory-switches") {
            switches.extend(switches_of(repo));
        }
        let auto = if b.on("memory-auto") {
            b.memory.auto_memory(repo)
        } else {
            Vec::new()
        };
        let instructions = if b.on("memory-instructions") {
            b.memory.instruction_files(repo)
        } else {
            Vec::new()
        };
        if auto.is_empty() && instructions.is_empty() {
            continue;
        }
        let mut kept_names = Vec::new();
        let mut flagged_names = Vec::new();
        for file in auto {
            if looks_secret(&file.bytes) {
                flagged_names.push(file.name.clone());
                part.flagged.push(format!("{}/{}", repo.name, file.name));
                let bytes = Zeroizing::new(file.bytes);
                if sealing {
                    flagged_files.push(Flagged {
                        dir: dir.clone(),
                        name: file.name,
                        bytes,
                    });
                }
                continue;
            }
            kept_names.push(file.name.clone());
            part.entries.push(Entry::bytes(
                &format!("memory/{dir}/{}", file.name),
                file.bytes,
            ));
        }
        let mut instruction_names = Vec::new();
        let mut instructions_held = Vec::new();
        for KeptFile { name, bytes } in instructions {
            if looks_secret(&bytes) {
                let _wiped = Zeroizing::new(bytes);
                part.flagged.push(format!("{}/{name}", repo.name));
                instructions_held.push(name);
                continue;
            }
            instruction_names.push(name.clone());
            part.entries.push(Entry::bytes(
                &format!("memory/{dir}/instructions/{name}"),
                bytes,
            ));
        }
        memory_files += kept_names.len() as u64;
        instruction_files += instruction_names.len() as u64;
        instructions_held_total += instructions_held.len() as u64;
        index.push(json!({
            "dir": dir,
            "key": key,
            "name": repo.name,
            "remote": repo.origin_url(),
            "path": repo.path,
            "files": kept_names,
            "instructions": instruction_names,
            "instructionsHeld": instructions_held,
            "flagged": flagged_names,
        }));
    }
    part.count = index.len() as u64;
    part.info = json!({
        "memoryFiles": memory_files,
        "instructionFiles": instruction_files,
        "switches": switches.len(),
        "flagged": part.flagged.len(),
    });
    let memory_held = part.flagged.len() as u64 - instructions_held_total;
    if memory_held > 0 {
        part.notes.push(format!(
            "{memory_held} memory files look like they hold a token: {}",
            if sealing {
                "they go into the encrypted part"
            } else {
                "they stay out of the file"
            }
        ));
    }
    if instructions_held_total > 0 {
        part.notes.push(format!(
            "{instructions_held_total} instruction files look like they hold a token and stay out of the file"
        ));
    }
    part.entries.insert(
        0,
        Entry::json(
            "memory/index.json",
            &json!({"version": 1, "repos": index, "switches": switches}),
        ),
    );
    (part, flagged_files)
}

pub fn history_part(b: &Build<'_>, passphrase: Option<&str>) -> Part {
    let mut part = Part::new(PartId::History);
    let dirs = repo_dirs(b.repos);
    let mut sessions = 0u64;
    let mut total = 0u64;
    let mut per_repo = Vec::new();
    let mut inner_files: Vec<(String, PathBuf)> = Vec::new();
    let mut index = Vec::new();
    for (repo, (key, dir)) in b.repos.iter().zip(&dirs) {
        if !b.opts.session_repos.is_empty() && !b.opts.session_repos.iter().any(|n| n == &repo.name)
        {
            continue;
        }
        let mut repo_sessions = 0u64;
        let mut repo_bytes = 0u64;
        for folder in b.memory.session_folders(repo) {
            for file in &folder.files {
                let path = folder.dir.join(file);
                if file.to_string_lossy().ends_with(".jsonl") {
                    repo_sessions += 1;
                }
                for rel in mem::files_under(&path) {
                    let full = if path.is_file() {
                        path.clone()
                    } else {
                        path.join(&rel)
                    };
                    repo_bytes += fs::metadata(&full).map_or(0, |m| m.len());
                    let within = if path.is_file() {
                        file.to_string_lossy().into_owned()
                    } else {
                        format!("{}/{}", file.to_string_lossy(), rel.to_string_lossy())
                    };
                    inner_files.push((format!("{dir}/{}/{within}", folder.slug), full));
                }
            }
            index.push(json!({"dir": dir, "key": key, "slug": folder.slug}));
        }
        if repo_sessions > 0 {
            per_repo.push(json!({"name": repo.name, "path": repo.path, "sessions": repo_sessions, "fileSize": repo_bytes}));
        }
        sessions += repo_sessions;
        total += repo_bytes;
    }
    part.count = sessions;
    part.info = json!({"repos": per_repo});
    if !b.content || sessions == 0 {
        part.sized_only = Some(total);
        return part;
    }
    let Some(passphrase) = passphrase else {
        return Part::failed(PartId::History, "sessions need a passphrase");
    };
    let inner = b.scratch.join(HISTORY_INNER);
    let sealed = b.scratch.join("history.enc");
    let build = || -> Result<(), String> {
        let mut zip = zipio::StreamZip::create(&inner)?;
        zip.add(
            "index.json",
            &serde_json::to_vec_pretty(&json!({"version": 1, "folders": index}))
                .unwrap_or_default(),
        )?;
        for (name, path) in &inner_files {
            zip.add_file(name, path)?;
        }
        zip.finish()?;
        let mut source = fs::File::open(&inner).map_err(|e| e.to_string())?;
        let mut target = create_private(&sealed)?;
        crypto::seal(passphrase, &mut source, &mut target)?;
        Ok(())
    };
    let outcome = build();
    let _ = fs::remove_file(&inner);
    match outcome {
        Ok(()) => {
            part.entries.push(Entry {
                name: "history.enc".into(),
                body: Body::File(sealed),
            });
            part
        }
        Err(why) => Part::failed(PartId::History, why),
    }
}

pub fn create_private(path: &Path) -> Result<fs::File, String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The vaulted values as the host hands them over; each is wiped when the list is dropped.
struct Wiped(Vec<StaticSecret>);

impl Zeroize for Wiped {
    fn zeroize(&mut self) {
        for secret in &mut self.0 {
            secret.value.zeroize();
        }
    }
}

impl Drop for Wiped {
    fn drop(&mut self) {
        self.zeroize();
    }
}

#[derive(Serialize)]
struct SecretsDoc<'a> {
    version: u32,
    values: Vec<SecretRow<'a>>,
    #[serde(rename = "memoryFiles")]
    memory_files: Vec<MemoryRow<'a>>,
}

#[derive(Serialize)]
struct SecretRow<'a> {
    server: &'a str,
    key: &'a str,
    kind: &'a str,
    value: &'a str,
}

#[derive(Serialize)]
struct MemoryRow<'a> {
    dir: &'a str,
    name: &'a str,
    hex: HexBytes<'a>,
}

/// Written as lowercase hex straight into the output, without a plaintext string in between.
struct HexBytes<'a>(&'a [u8]);

impl std::fmt::Display for HexBytes<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl Serialize for HexBytes<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

struct CountingSink(usize);

impl std::io::Write for CountingSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The document that gets sealed. It is serialised twice, the first time only to size the buffer,
/// because a buffer that grows leaves its earlier, smaller copies in freed memory.
fn secrets_plain(values: &[StaticSecret], flagged: &[Flagged]) -> Zeroizing<Vec<u8>> {
    let doc = SecretsDoc {
        version: 1,
        values: values
            .iter()
            .map(|s| SecretRow {
                server: &s.server,
                key: &s.key,
                kind: &s.kind,
                value: &s.value,
            })
            .collect(),
        memory_files: flagged
            .iter()
            .map(|f| MemoryRow {
                dir: &f.dir,
                name: &f.name,
                hex: HexBytes(&f.bytes),
            })
            .collect(),
    };
    let mut size = CountingSink(0);
    let _ = serde_json::to_writer(&mut size, &doc);
    let mut plain = Zeroizing::new(Vec::with_capacity(size.0));
    let _ = serde_json::to_writer(&mut *plain, &doc);
    plain
}

pub fn secrets_part(
    b: &Build<'_>,
    passphrase: Option<&str>,
    flagged: &[Flagged],
    declared: u64,
) -> Part {
    let mut part = Part::new(PartId::Secrets);
    if !b.content {
        part.count = declared;
        part.sized_only = Some(0);
        return part;
    }
    let Some(passphrase) = passphrase else {
        return Part::failed(PartId::Secrets, "secrets need a passphrase");
    };
    let values = match b.ctx.host.setup_secrets() {
        Ok(values) => Wiped(values),
        Err(why) => return Part::failed(PartId::Secrets, why),
    };
    let plain = secrets_plain(&values.0, flagged);
    match crypto::seal_bytes(passphrase, &plain) {
        Ok(sealed) => {
            part.count = values.0.len() as u64;
            part.entries.push(Entry::bytes("secrets.enc", sealed));
            part
        }
        Err(why) => Part::failed(PartId::Secrets, why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(value: &str) -> StaticSecret {
        StaticSecret {
            server: "alpha".into(),
            key: "API_KEY".into(),
            kind: "env".into(),
            value: value.into(),
        }
    }

    fn flagged(bytes: &[u8]) -> Flagged {
        Flagged {
            dir: "web".into(),
            name: "leak.md".into(),
            bytes: Zeroizing::new(bytes.to_vec()),
        }
    }

    #[test]
    fn the_sealed_document_is_built_once_in_a_buffer_that_never_grew() {
        let values = [secret("v\"alue"), secret(&"x".repeat(10_000))];
        let files = [flagged(&[0, 15, 255]), flagged(&vec![7u8; 5_000])];
        let plain: Zeroizing<Vec<u8>> = secrets_plain(&values, &files);
        assert_eq!(plain.capacity(), plain.len());
        let doc: Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(doc["version"], 1);
        assert_eq!(doc["values"][0]["value"], "v\"alue");
        assert_eq!(doc["values"][0]["server"], "alpha");
        assert_eq!(doc["values"][0]["kind"], "env");
        assert_eq!(doc["values"][1]["value"].as_str().unwrap().len(), 10_000);
        assert_eq!(doc["memoryFiles"][0]["dir"], "web");
        assert_eq!(doc["memoryFiles"][0]["name"], "leak.md");
        assert_eq!(doc["memoryFiles"][0]["hex"], "000fff");
        assert_eq!(doc["memoryFiles"][1]["hex"].as_str().unwrap().len(), 10_000);
    }

    #[test]
    fn the_values_the_host_hands_over_are_blanked_when_they_are_wiped() {
        let mut values = Wiped(vec![secret("first"), secret("second")]);
        values.zeroize();
        assert!(values.0.iter().all(|s| s.value.is_empty()));
        assert_eq!(values.0[0].server, "alpha");
    }

    #[test]
    fn the_bytes_of_a_held_back_file_are_wiped_on_drop() {
        fn wipes_on_drop<T: zeroize::ZeroizeOnDrop>(_: &T) {}
        wipes_on_drop(&flagged(b"x").bytes);
    }
}
