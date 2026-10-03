use super::{
    map_all, relocate_entry, screen_entry, ClientConfig, Mapping, McpmInput, Warning, IMPORT_SOURCE,
};
use crate::clients;
use crate::plus::args::{flag, flag_or, str_arg};
use crate::plus::registry_ro;
use crate::registry::{self, ManagedEntry, Profile, Registry, ServerEntry};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CLIENT_FILES: &[(&str, &str)] = &[
    ("claude-code", "claude-code"),
    ("claude-desktop", "claude-desktop"),
    ("cursor", "cursor"),
    ("windsurf", "windsurf"),
    ("vscode", "vscode"),
    ("cline", "cline"),
    ("roo-code", "roo-code"),
    ("opencode", "opencode"),
    ("qwen-cli", "qwen-code"),
    ("gemini-cli", "gemini-cli"),
    ("codex-cli", "codex"),
    ("continue", "continue"),
    ("goose-cli", "goose"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Created,
    Updated,
    Unchanged,
    Conflict,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub id: String,
    pub action: Action,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClientChange {
    pub id: String,
    pub action: Action,
    pub path: Option<String>,
    pub profile: String,
    pub removed: Vec<String>,
    pub orphans: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Reject {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub dry_run: bool,
    pub rejects: Vec<Reject>,
    pub scripts: Vec<String>,
    pub servers: Vec<Change>,
    pub profiles: Vec<Change>,
    pub client_scopes: Vec<Change>,
    pub client_discovery: Vec<Change>,
    pub clients: Vec<ClientChange>,
    pub secrets: Vec<Change>,
    pub skipped_clients: Value,
    pub warnings: Value,
    pub counts: BTreeMap<String, usize>,
}

impl Plan {
    pub fn changed(&self) -> bool {
        self.counts.get("created").copied().unwrap_or(0) > 0
            || self.counts.get("updated").copied().unwrap_or(0) > 0
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn summary(&self) -> String {
        let mut lines = vec![format!(
            "{}: {} created, {} updated, {} unchanged, {} conflicts",
            if self.dry_run { "dry run" } else { "import" },
            self.count("created"),
            self.count("updated"),
            self.count("unchanged"),
            self.count("conflict"),
        )];
        for c in self
            .clients
            .iter()
            .filter(|c| c.action != Action::Unchanged)
        {
            let mut line = format!("  client {} {:?}", c.id, c.action).to_lowercase();
            if !c.removed.is_empty() {
                line.push_str(&format!(" (removed {})", c.removed.join(", ")));
            }
            if let Some(error) = &c.error {
                line.push_str(&format!(": {error}"));
            }
            lines.push(line);
        }
        let groups: [(&str, &[Change]); 5] = [
            ("server", &self.servers),
            ("profile", &self.profiles),
            ("scope", &self.client_scopes),
            ("discovery", &self.client_discovery),
            ("secret", &self.secrets),
        ];
        for (label, changes) in groups {
            for c in changes.iter().filter(|c| c.action != Action::Unchanged) {
                lines.push(format!("  {label} {} {:?}", c.id, c.action).to_lowercase());
            }
        }
        for r in &self.rejects {
            lines.push(format!("  rejected {}: {}", r.id, r.reason));
        }
        lines.join("\n")
    }

    fn count(&self, key: &str) -> usize {
        self.counts.get(key).copied().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub root: PathBuf,
    pub short_ids_path: Option<PathBuf>,
    pub home: Option<String>,
    pub dry_run: bool,
    pub write_clients: bool,
    pub prune_orphans: bool,
}

fn read_object(path: &Path, required: bool) -> Result<Map<String, Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => return Ok(Map::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!("{} is not a JSON object", path.display())),
        Err(e) => Err(format!("{} is not valid JSON: {e}", path.display())),
    }
}

pub fn load_input(opts: &RunOptions) -> Result<(McpmInput, Vec<ClientConfig>), String> {
    let servers = read_object(&opts.root.join("servers.json"), true)?;
    let sources = read_object(&opts.root.join("sources.json"), false)?;
    let mut short_ids = BTreeMap::new();
    if let Some(path) = &opts.short_ids_path {
        for (k, v) in read_object(path, true)? {
            let id = v
                .as_str()
                .ok_or_else(|| format!("short id for {k} is not a string"))?;
            short_ids.insert(k, id.to_string());
        }
    }
    let home = opts
        .home
        .clone()
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_default();
    let mut clients = Vec::new();
    for (file, id) in CLIENT_FILES {
        let path = opts.root.join(format!("{file}.json"));
        if !path.exists() {
            continue;
        }
        let root = read_object(&path, true)?;
        let servers = ["mcpServers", "servers"]
            .iter()
            .find_map(|key| root.get(*key).and_then(Value::as_object))
            .cloned()
            .unwrap_or_default();
        clients.push(ClientConfig {
            client_id: (*id).to_string(),
            servers,
        });
    }
    Ok((
        McpmInput {
            servers,
            sources,
            home,
            short_ids,
        },
        clients,
    ))
}

fn upsert_server(reg: &mut Registry, mut entry: ServerEntry) -> Action {
    let Some(pos) = reg.servers.iter().position(|s| s.id == entry.id) else {
        reg.servers.push(entry);
        return Action::Created;
    };
    let existing = &reg.servers[pos];
    if existing.source.as_deref() != Some(IMPORT_SOURCE) {
        return Action::Conflict;
    }
    let mut unknown = existing.unknown_fields.clone();
    unknown.extend(std::mem::take(&mut entry.unknown_fields));
    entry.unknown_fields = unknown;
    entry.disabled_tools = existing.disabled_tools.clone();
    entry.client_credentials = existing.client_credentials.clone();
    entry.request_timeout_ms = existing.request_timeout_ms;
    entry.initialize_timeout_ms = existing.initialize_timeout_ms;
    if *existing == entry {
        return Action::Unchanged;
    }
    reg.servers[pos] = entry;
    Action::Updated
}

fn upsert_profile(reg: &mut Registry, profile: &Profile) -> Action {
    match reg.profiles.iter().position(|p| p.id == profile.id) {
        None => {
            reg.profiles.push(profile.clone());
            Action::Created
        }
        Some(pos) if reg.profiles[pos] == *profile => Action::Unchanged,
        Some(pos) => {
            reg.profiles[pos] = profile.clone();
            Action::Updated
        }
    }
}

fn upsert_map(
    target: &mut std::collections::HashMap<String, String>,
    source: &BTreeMap<String, String>,
) -> Vec<Change> {
    source
        .iter()
        .map(|(k, v)| {
            let action = match target.get(k) {
                None => Action::Created,
                Some(old) if old == v => Action::Unchanged,
                Some(_) => Action::Updated,
            };
            if action != Action::Unchanged {
                target.insert(k.clone(), v.clone());
            }
            Change {
                id: k.clone(),
                action,
            }
        })
        .collect()
}

struct RegistryChanges {
    servers: Vec<Change>,
    profiles: Vec<Change>,
    client_scopes: Vec<Change>,
    client_discovery: Vec<Change>,
}

fn merge_registry(reg: &mut Registry, mapping: &Mapping) -> RegistryChanges {
    let mut servers = Vec::new();
    let mut skipped = std::collections::HashSet::new();
    for s in &mapping.servers {
        let action = upsert_server(reg, s.entry.clone());
        if action == Action::Conflict {
            skipped.insert(s.entry.id.clone());
        }
        servers.push(Change {
            id: s.entry.id.clone(),
            action,
        });
    }
    let profiles = mapping
        .profiles
        .iter()
        .map(|p| {
            let mut p = p.clone();
            p.enabled_server_ids.retain(|id| !skipped.contains(id));
            Change {
                id: p.id.clone(),
                action: upsert_profile(reg, &p),
            }
        })
        .collect();
    RegistryChanges {
        servers,
        profiles,
        client_scopes: upsert_map(&mut reg.client_scopes, &mapping.client_scopes),
        client_discovery: upsert_map(&mut reg.client_discovery, &mapping.client_discovery),
    }
}

fn prepare_launch(
    mapping: &mut Mapping,
    home: &str,
    data_dir: &Path,
) -> (Vec<Reject>, Vec<(PathBuf, PathBuf)>) {
    let mut rejects = Vec::new();
    let mut moves = Vec::new();
    for s in mapping.servers.iter_mut() {
        for (from, to) in relocate_entry(&mut s.entry, home, data_dir) {
            moves.push((PathBuf::from(from), to));
        }
    }
    let mut kept = Vec::new();
    for s in std::mem::take(&mut mapping.servers) {
        match screen_entry(&s.entry) {
            Ok(()) => kept.push(s),
            Err(reason) => rejects.push(Reject {
                id: s.entry.id.clone(),
                reason,
            }),
        }
    }
    mapping.servers = kept;
    let gone: Vec<&str> = rejects.iter().map(|r| r.id.as_str()).collect();
    for p in mapping.profiles.iter_mut() {
        p.enabled_server_ids
            .retain(|id| !gone.contains(&id.as_str()));
    }
    for r in &rejects {
        mapping.warnings.push(Warning {
            server: r.id.clone(),
            kind: "rejected-launch".into(),
            detail: r.reason.clone(),
        });
    }
    (rejects, moves)
}

fn copy_scripts(moves: &[(PathBuf, PathBuf)], warnings: &mut Vec<Warning>) -> Result<(), String> {
    for (from, to) in moves {
        if !from.is_file() {
            warnings.push(Warning {
                server: String::new(),
                kind: "script-missing".into(),
                detail: from.display().to_string(),
            });
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::copy(from, to)
            .map_err(|e| format!("cannot copy {} to {}: {e}", from.display(), to.display()))?;
    }
    Ok(())
}

fn read_registry() -> Result<Registry, String> {
    let path = registry::registry_path().ok_or("Could not resolve registry path")?;
    Ok(registry_ro::read_at(&path)?.unwrap_or_default())
}

fn plan_secrets(
    mapping: &Mapping,
    skipped: &[String],
) -> Result<(Vec<Change>, Vec<usize>), String> {
    let mut changes = Vec::new();
    let mut pending = Vec::new();
    for (index, s) in mapping.secret_writes().into_iter().enumerate() {
        let action = if skipped.contains(&s.server_id) {
            Action::Conflict
        } else {
            match crate::secrets::get_vault_secret_result(&s.server_id, &s.key)? {
                None => Action::Created,
                Some(old) if old == s.value => Action::Unchanged,
                Some(_) => Action::Updated,
            }
        };
        if matches!(action, Action::Created | Action::Updated) {
            pending.push(index);
        }
        changes.push(Change {
            id: s.vault_key(),
            action,
        });
    }
    Ok((changes, pending))
}

fn counts(plan: &Plan) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let all = plan
        .servers
        .iter()
        .chain(&plan.profiles)
        .chain(&plan.client_scopes)
        .chain(&plan.client_discovery)
        .chain(&plan.secrets)
        .map(|c| c.action)
        .chain(plan.clients.iter().map(|c| c.action));
    for action in all {
        let key = serde_json::to_value(action)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        *out.entry(key).or_insert(0) += 1;
    }
    out
}

fn apply_clients(
    mapping: &Mapping,
    opts: &RunOptions,
    conflicts: &[String],
    managed: &std::collections::HashMap<String, ManagedEntry>,
) -> (Vec<ClientChange>, Vec<(String, ManagedEntry)>) {
    let mut changes = Vec::new();
    let mut records = Vec::new();
    for entry in &mapping.client_entries {
        let mut prune: Vec<String> = entry
            .mapped
            .iter()
            .filter(|m| !m.server_ids.iter().any(|id| conflicts.contains(id)))
            .map(|m| m.key.clone())
            .collect();
        if opts.prune_orphans {
            prune.extend(entry.orphans.iter().cloned());
        }
        let kept: Vec<String> = if opts.prune_orphans {
            Vec::new()
        } else {
            entry.orphans.clone()
        };
        let mut change = ClientChange {
            id: entry.client_id.clone(),
            action: Action::Unchanged,
            path: None,
            profile: entry.profile_id.clone(),
            removed: Vec::new(),
            orphans: kept,
            error: None,
        };
        match clients::apply_import(&entry.client_id, &entry.profile_id, &prune, opts.dry_run) {
            Ok(applied) => {
                let record_stale = managed.get(&entry.client_id).map_or(true, |m| {
                    let mut same = applied.managed.clone();
                    same.updated_at = m.updated_at;
                    *m != same
                });
                change.path = Some(applied.path);
                change.removed = applied.removed;
                if applied.wrote_gateway {
                    change.action = if managed.contains_key(&entry.client_id) {
                        Action::Updated
                    } else {
                        Action::Created
                    };
                } else if !change.removed.is_empty() || record_stale {
                    change.action = Action::Updated;
                }
                if record_stale {
                    records.push((entry.client_id.clone(), applied.managed));
                }
            }
            Err(error) => {
                change.action = Action::Conflict;
                change.error = Some(error);
            }
        }
        changes.push(change);
    }
    (changes, records)
}

pub fn run(opts: &RunOptions) -> Result<Plan, String> {
    let (input, clients) = load_input(opts)?;
    let mut mapping = map_all(&input, &clients);
    let data_dir = registry::conduit_dir().ok_or("Could not resolve data directory")?;
    let (rejects, moves) = prepare_launch(&mut mapping, &input.home, &data_dir);
    let mut preview = read_registry()?;
    let preview_changes = merge_registry(&mut preview, &mapping);
    let skipped: Vec<String> = preview_changes
        .servers
        .iter()
        .filter(|c| c.action == Action::Conflict)
        .map(|c| c.id.clone())
        .collect();
    let blocked: Vec<String> = skipped
        .iter()
        .cloned()
        .chain(rejects.iter().map(|r| r.id.clone()))
        .collect();
    let (secret_changes, pending) = plan_secrets(&mapping, &skipped)?;

    let registry_dirty = [
        &preview_changes.servers,
        &preview_changes.profiles,
        &preview_changes.client_scopes,
        &preview_changes.client_discovery,
    ]
    .iter()
    .any(|v| {
        v.iter()
            .any(|c| matches!(c.action, Action::Created | Action::Updated))
    });
    let changes = if opts.dry_run || (!registry_dirty && pending.is_empty()) {
        preview_changes
    } else {
        copy_scripts(&moves, &mut mapping.warnings)?;
        let writes = mapping.secret_writes();
        for index in &pending {
            let s = writes[*index];
            crate::secrets::set_secret(&s.server_id, &s.key, &s.value)
                .map_err(|e| format!("vault write failed for {}: {e}", s.vault_key()))?;
        }
        let bump = !pending.is_empty();
        let (_, changes) = registry::update(|reg| {
            let changes = merge_registry(reg, &mapping);
            if bump {
                reg.secrets_generation += 1;
            }
            Ok(changes)
        })?;
        changes
    };

    if !opts.dry_run {
        crate::plus::selfmcp::register::ensure_self_server()?;
    }

    let mut client_changes = Vec::new();
    if opts.write_clients {
        let managed = read_registry()?.client_managed_entries;
        let (applied, records) = apply_clients(&mapping, opts, &blocked, &managed);
        client_changes = applied;
        if !opts.dry_run && !records.is_empty() {
            registry::update(|reg| {
                for (id, entry) in &records {
                    reg.set_client_managed_entry(id, entry.clone());
                }
                Ok(())
            })?;
        }
    }

    let mut plan = Plan {
        dry_run: opts.dry_run,
        rejects,
        scripts: moves
            .iter()
            .map(|(_, to)| to.to_string_lossy().into_owned())
            .collect(),
        servers: changes.servers,
        profiles: changes.profiles,
        client_scopes: changes.client_scopes,
        client_discovery: changes.client_discovery,
        clients: client_changes,
        secrets: secret_changes,
        skipped_clients: json!(mapping.skipped_clients),
        warnings: json!(mapping.warnings),
        counts: BTreeMap::new(),
    };
    plan.counts = counts(&plan);
    Ok(plan)
}

pub fn run_handler(args: Value) -> Result<Value, String> {
    let root = str_arg(&args, "root").ok_or("root is required")?;
    let opts = RunOptions {
        root: PathBuf::from(root),
        short_ids_path: str_arg(&args, "shortIds").map(PathBuf::from),
        home: str_arg(&args, "home").map(String::from),
        dry_run: flag(&args, "dryRun"),
        write_clients: flag_or(&args, "writeClients", true),
        prune_orphans: flag(&args, "pruneOrphans"),
    };
    run(&opts).map(|p| p.to_value())
}
