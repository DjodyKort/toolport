use super::backend::GitBackend;
use super::bundle::{
    destination_for, import_bundle, read_bundle, read_manifest, safe_relative,
    write_bundle_with_origins, write_salt, write_synced_file, Credential, ImportReport,
    ImportTargets, Manifest, PortableRoots, SourceFile, SyncError, SERVER_ORIGINS_KEY,
};
use super::exec::Exec;
use crate::plus::hashing::lock_hash;
use super::fernet::{self, FernetError, FernetKey};
use super::kdf;
use super::origins::{detect_origins, resolve_servers, ResolveOptions, ResolveResult};
use super::schema::{
    Changes, ConflictInfo, ServerOrigins, SyncConfig, SyncProjectConfig, SyncState, SyncStateEntry,
};
use crate::plus::skills::clock::Clock;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MIN_PASSPHRASE: usize = 8;
const SKILLS_SUBDIRS: [&str; 6] = ["skills", "agents", "styles", "rules", "profiles", "lib"];
const GLOBAL_FILES: [&str; 5] = [
    "servers.json",
    "profiles_metadata.json",
    "sources.json",
    "compression.json",
    "context.json",
];

pub struct SyncContext<'a> {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub roots: PortableRoots,
    pub clock: &'a dyn Clock,
    pub exec: &'a dyn Exec,
}

impl SyncContext<'_> {
    pub fn config_path(&self) -> PathBuf {
        self.state_dir.join("sync.json")
    }

    pub fn state_path(&self) -> PathBuf {
        self.state_dir.join("sync_state.json")
    }

    pub fn keyfile_path(&self) -> PathBuf {
        self.state_dir.join("sync_keyfile")
    }

    pub fn repo_path(&self) -> PathBuf {
        self.state_dir.join("sync_repo")
    }

    pub fn skills_repo_path(&self) -> PathBuf {
        self.config_dir.join("skills_repo")
    }

    fn now(&self) -> String {
        self.clock.now().isoformat()
    }

    fn targets(&self, cfg: Option<&SyncConfig>) -> ImportTargets {
        ImportTargets {
            config_dir: self.config_dir.clone(),
            skills_repo_dir: self.skills_repo_path(),
            projects: cfg
                .map(|c| {
                    c.projects
                        .iter()
                        .map(|(n, p)| (n.clone(), PathBuf::from(&p.local_path)))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.config_path().exists() && self.keyfile_path().exists()
    }
}

fn io_err(path: &Path, e: std::io::Error) -> SyncError {
    super::bundle::io(path, e)
}

fn cfg_err(message: &str) -> SyncError {
    SyncError::Config(message.to_string())
}

pub fn load_config(ctx: &SyncContext<'_>) -> Option<SyncConfig> {
    crate::plus::jsonfs::read_json(&ctx.config_path())
}

pub fn save_config(ctx: &SyncContext<'_>, cfg: &SyncConfig) -> Result<(), SyncError> {
    write_json(&ctx.config_path(), cfg)
}

pub fn load_state(ctx: &SyncContext<'_>) -> SyncState {
    crate::plus::jsonfs::read_json(&ctx.state_path()).unwrap_or_default()
}

fn save_state(ctx: &SyncContext<'_>, state: &SyncState) -> Result<(), SyncError> {
    write_json(&ctx.state_path(), state)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), SyncError> {
    let text = serde_json::to_string_pretty(value).map_err(|e| SyncError::Format(e.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    }
    fs::write(path, text).map_err(|e| io_err(path, e))
}

fn save_keyfile(ctx: &SyncContext<'_>, key: &str) -> Result<(), SyncError> {
    let path = ctx.keyfile_path();
    crate::registry::atomic_write(&path, key)
        .map_err(|e| SyncError::Io(format!("{}: {e}", path.display())))
}

fn load_keyfile(ctx: &SyncContext<'_>) -> Result<String, SyncError> {
    let path = ctx.keyfile_path();
    fs::read_to_string(&path)
        .map(|k| k.trim().to_string())
        .map_err(|_| cfg_err("sync key file missing; run init first"))
}

fn require_config(ctx: &SyncContext<'_>) -> Result<(SyncConfig, String), SyncError> {
    let cfg = load_config(ctx).ok_or_else(|| cfg_err("sync is not configured; run init first"))?;
    if cfg.backend != "git" {
        return Err(SyncError::Config(format!(
            "unsupported sync backend {:?}; only git is supported",
            cfg.backend
        )));
    }
    Ok((cfg, load_keyfile(ctx)?))
}

fn backend<'a>(ctx: &'a SyncContext<'_>, cfg: &SyncConfig) -> Result<GitBackend<'a>, SyncError> {
    let url = cfg
        .repo_url
        .as_deref()
        .ok_or_else(|| cfg_err("sync config has no repo_url"))?;
    Ok(GitBackend::new(
        ctx.exec,
        url,
        &ctx.repo_path(),
        &cfg.branch,
    ))
}

fn hostname() -> String {
    for var in ["HOSTNAME", "COMPUTERNAME"] {
        if let Ok(v) = std::env::var(var) {
            if !v.trim().is_empty() {
                return v.trim().to_string();
            }
        }
    }
    fs::read_to_string("/etc/hostname")
        .map(|v| v.trim().to_string())
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn temp_dir(base: &Path, label: &str) -> Result<PathBuf, SyncError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = base.join(format!(
        "{label}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    Ok(dir)
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), SyncError> {
    fs::create_dir_all(to).map_err(|e| io_err(to, e))?;
    for entry in fs::read_dir(from).map_err(|e| io_err(from, e))? {
        let entry = entry.map_err(|e| io_err(from, e))?;
        let target = to.join(entry.file_name());
        fs::copy(entry.path(), &target).map_err(|e| io_err(&target, e))?;
    }
    Ok(())
}

fn derive(passphrase: &str, salt: &[u8]) -> String {
    kdf::derive_key(passphrase, salt)
}

fn check_passphrase(passphrase: &str) -> Result<(), SyncError> {
    if passphrase.chars().count() < MIN_PASSPHRASE {
        return Err(SyncError::Config(format!(
            "passphrase must be at least {MIN_PASSPHRASE} characters"
        )));
    }
    Ok(())
}

fn random_salt() -> Result<[u8; kdf::SALT_LEN], SyncError> {
    kdf::random_salt().map_err(|_| SyncError::Crypto(FernetError::Entropy))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            walk(&path, out);
        } else if meta.is_file() && !entry.file_name().to_string_lossy().starts_with('.') {
            out.push(path);
        }
    }
}

fn rel_key(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

fn is_plain_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

pub fn gather_files(
    ctx: &SyncContext<'_>,
    cfg: Option<&SyncConfig>,
    include_projects: bool,
) -> Result<Vec<SourceFile>, SyncError> {
    let mut files = Vec::new();
    let mut add = |key: String, path: &Path, category: &str, project: Option<&str>| {
        let bytes = fs::read(path).map_err(|e| io_err(path, e))?;
        files.push(SourceFile {
            key,
            category: category.to_string(),
            project_name: project.map(str::to_string),
            bytes,
        });
        Ok::<(), SyncError>(())
    };
    for name in GLOBAL_FILES {
        let path = ctx.config_dir.join(name);
        if is_plain_file(&path) {
            add(format!("global/{name}"), &path, "global", None)?;
        }
    }
    let repo = ctx.skills_repo_path();
    let mut found = Vec::new();
    for sub in SKILLS_SUBDIRS {
        walk(&repo.join(sub), &mut found);
    }
    let manifest = repo.join("mcpm-skills.yaml");
    if is_plain_file(&manifest) {
        found.push(manifest);
    }
    found.sort();
    for path in found {
        if let Some(rel) = rel_key(&repo, &path) {
            add(format!("skills_repo/{rel}"), &path, "global", None)?;
        }
    }
    let bin = ctx.config_dir.join("bin");
    let mut bins = Vec::new();
    walk(&bin, &mut bins);
    bins.sort();
    for path in bins {
        if let Some(rel) = rel_key(&bin, &path) {
            add(format!("bin/{rel}"), &path, "global", None)?;
        }
    }
    if include_projects {
        if let Some(cfg) = cfg {
            for (name, project) in &cfg.projects {
                for file in &project.files {
                    safe_relative(file)?;
                    let path = Path::new(&project.local_path).join(file);
                    if is_plain_file(&path) {
                        add(
                            format!("projects/{name}/{file}"),
                            &path,
                            "project",
                            Some(name),
                        )?;
                    }
                }
            }
        }
    }
    Ok(files)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InitReport {
    pub machine_id: String,
    pub branch: String,
    pub fresh_remote: bool,
}

pub struct InitOptions<'a> {
    pub repo: &'a str,
    pub branch: &'a str,
    pub machine_id: Option<&'a str>,
    pub passphrase: &'a str,
    pub reconfigure: bool,
}

fn sample_entry_blob(repo: &Path, manifest: &Manifest) -> Option<PathBuf> {
    manifest
        .entries
        .iter()
        .filter(|(key, _)| key.as_str() != SERVER_ORIGINS_KEY)
        .filter_map(|(_, e)| safe_relative(&e.encrypted_file).ok())
        .map(|rel| repo.join(rel))
        .find(|p| p.exists())
}

fn key_decrypts(key: &str, blob: &Path) -> Result<bool, SyncError> {
    let parsed = FernetKey::parse(key)?;
    let token = fs::read(blob).map_err(|e| io_err(blob, e))?;
    match fernet::decrypt(&parsed, &token) {
        Ok(_) => Ok(true),
        Err(FernetError::InvalidToken) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub fn init(ctx: &SyncContext<'_>, opts: &InitOptions<'_>) -> Result<InitReport, SyncError> {
    check_passphrase(opts.passphrase)?;
    if ctx.is_configured() && !opts.reconfigure {
        return Err(cfg_err(
            "sync is already configured; pass reconfigure to replace it",
        ));
    }
    let machine_id = opts
        .machine_id
        .map(str::to_string)
        .filter(|m| !m.is_empty())
        .unwrap_or_else(hostname);
    let cfg = SyncConfig {
        backend: "git".into(),
        machine_id: machine_id.clone(),
        repo_url: Some(opts.repo.to_string()),
        branch: opts.branch.to_string(),
        cloud_url: None,
        auth_token: None,
        projects: load_config(ctx).map(|c| c.projects).unwrap_or_default(),
    };
    let repo_dir = ctx.repo_path();
    if repo_dir.join(".git").exists() {
        let same = ctx
            .exec
            .git(Some(&repo_dir), &["remote", "get-url", "origin"])
            .map(|u| u.trim() == opts.repo)
            .unwrap_or(false);
        if !same {
            fs::remove_dir_all(&repo_dir).map_err(|e| io_err(&repo_dir, e))?;
        }
    }
    let git = backend(ctx, &cfg)?;
    git.init()?;
    let (key, salt, fresh) = match git.load_salt() {
        Some(salt) => {
            let key = derive(opts.passphrase, &salt);
            if let Some(manifest) = git.manifest() {
                if let Some(blob) = sample_entry_blob(&repo_dir, &manifest) {
                    if !key_decrypts(&key, &blob)? {
                        return Err(cfg_err(
                            "passphrase does not match the bundle on the remote",
                        ));
                    }
                }
            }
            (key, salt, false)
        }
        None => {
            let salt = random_salt()?.to_vec();
            (derive(opts.passphrase, &salt), salt, true)
        }
    };
    if fresh {
        write_salt(&repo_dir, &salt)?;
    }
    save_config(ctx, &cfg)?;
    save_keyfile(ctx, &key)?;
    Ok(InitReport {
        machine_id,
        branch: cfg.branch,
        fresh_remote: fresh,
    })
}

#[derive(Debug, Clone, Default)]
pub struct PushOptions {
    pub include_projects: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushReport {
    pub entries: Vec<String>,
    pub dry_run: bool,
    pub pushed: bool,
    pub committed: bool,
    pub machine_id: String,
}

fn origins_json(ctx: &SyncContext<'_>) -> Option<String> {
    let servers = ctx.config_dir.join("servers.json");
    if !servers.exists() {
        return None;
    }
    let origins = detect_origins(
        &servers,
        &ctx.config_dir.join("sources.json"),
        ctx.exec,
        &ctx.roots,
    );
    serde_json::to_string_pretty(&origins).ok()
}

pub fn push(ctx: &SyncContext<'_>, opts: &PushOptions) -> Result<PushReport, SyncError> {
    let (cfg, key) = require_config(ctx)?;
    let files = gather_files(ctx, Some(&cfg), opts.include_projects)?;
    let origins = origins_json(ctx);
    let mut entries: Vec<String> = files.iter().map(|f| f.key.clone()).collect();
    if origins.is_some() {
        entries.push(SERVER_ORIGINS_KEY.to_string());
    }
    let mut report = PushReport {
        entries,
        dry_run: opts.dry_run,
        pushed: false,
        committed: false,
        machine_id: cfg.machine_id.clone(),
    };
    if files.is_empty() || opts.dry_run {
        return Ok(report);
    }
    let git = backend(ctx, &cfg)?;
    if !git.is_cloned() {
        return Err(cfg_err("sync repo is not initialized; run init first"));
    }
    git.pull();
    if git.load_salt().is_none() {
        return Err(cfg_err(
            "salt.txt is missing from the sync repo; run init again",
        ));
    }
    let tmp = temp_dir(&ctx.state_dir, "push")?;
    let staged = write_bundle_with_origins(
        &tmp,
        Credential::Key(&key),
        None,
        &cfg.machine_id,
        &ctx.now(),
        &files,
        &ctx.roots,
        origins.as_deref(),
    )
    .and_then(|manifest| {
        let repo = ctx.repo_path();
        if read_manifest(&repo)
            .map(|old| same_content(&old, &manifest))
            .unwrap_or(false)
        {
            return Ok(manifest);
        }
        let blobs = repo.join("blobs");
        if blobs.exists() {
            fs::remove_dir_all(&blobs).map_err(|e| io_err(&blobs, e))?;
        }
        copy_dir(&tmp.join("blobs"), &blobs)?;
        let target = repo.join(super::bundle::MANIFEST_FILE);
        fs::copy(tmp.join(super::bundle::MANIFEST_FILE), &target)
            .map_err(|e| io_err(&target, e))?;
        Ok(manifest)
    });
    let _ = fs::remove_dir_all(&tmp);
    let manifest = staged?;
    report.committed = git.commit_and_push(&format!(
        "sync push from {} at {}",
        manifest.machine_id, manifest.pushed_at
    ))?;
    report.pushed = true;
    update_state_after_push(ctx, &cfg, &manifest)?;
    Ok(report)
}

fn same_content(old: &Manifest, new: &Manifest) -> bool {
    old.entries.len() == new.entries.len()
        && new.entries.iter().all(|(key, entry)| {
            old.entries.get(key).is_some_and(|o| {
                o.hash == entry.hash
                    && o.encoding == entry.encoding
                    && o.category == entry.category
                    && o.project_name == entry.project_name
            })
        })
}

fn update_state_after_push(
    ctx: &SyncContext<'_>,
    cfg: &SyncConfig,
    manifest: &Manifest,
) -> Result<(), SyncError> {
    let mut state = load_state(ctx);
    let targets = ctx.targets(Some(cfg));
    for (key, entry) in &manifest.entries {
        if key == SERVER_ORIGINS_KEY {
            state.entries.insert(key.clone(), pinned(&entry.hash));
            continue;
        }
        if let Some(path) = destination_for(key, &entry.category, &targets)? {
            if let Ok(bytes) = fs::read(&path) {
                state.entries.insert(
                    key.clone(),
                    SyncStateEntry {
                        local_hash_at_sync: lock_hash(&bytes),
                        remote_hash_at_sync: entry.hash.clone(),
                    },
                );
            }
        }
    }
    state.last_sync_at = ctx.now();
    state.last_direction = "push".into();
    save_state(ctx, &state)
}

fn pinned(hash: &str) -> SyncStateEntry {
    SyncStateEntry {
        local_hash_at_sync: hash.to_string(),
        remote_hash_at_sync: hash.to_string(),
    }
}

pub fn detect_changes(
    ctx: &SyncContext<'_>,
    cfg: &SyncConfig,
    remote: &Manifest,
) -> Result<Changes, SyncError> {
    let state = load_state(ctx);
    let targets = ctx.targets(Some(cfg));
    let mut changes = Changes::default();
    for key in remote.entries.keys() {
        if !state.entries.contains_key(key) {
            changes.new.push(key.clone());
        }
    }
    for (key, state_entry) in &state.entries {
        let Some(remote_entry) = remote.entries.get(key) else {
            changes.removed.push(key.clone());
            continue;
        };
        if remote_entry.hash == state_entry.remote_hash_at_sync {
            changes.unchanged.push(key.clone());
            continue;
        }
        let local_changed = destination_for(key, &remote_entry.category, &targets)?
            .and_then(|path| fs::read(path).ok())
            .map(|bytes| lock_hash(&bytes) != state_entry.local_hash_at_sync)
            .unwrap_or(false);
        if local_changed {
            changes.conflicts.push(key.clone());
        } else {
            changes.modified.push(key.clone());
        }
    }
    Ok(changes)
}

#[derive(Debug, Clone, Default)]
pub struct PullOptions {
    pub include_projects: bool,
    pub force: bool,
    pub dry_run: bool,
    pub resolve: bool,
    pub run_setup: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PullReport {
    pub no_remote: bool,
    pub dry_run: bool,
    pub machine_id: String,
    pub pushed_at: String,
    pub applied: Vec<String>,
    pub skipped: Vec<String>,
    pub kept_local: Vec<String>,
    pub conflicts: Vec<ConflictInfo>,
    pub changes: Option<Changes>,
    pub resolved: Vec<ResolveResult>,
    pub skills_repo_changed: usize,
}

fn remote_sibling(path: &Path) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(".remote");
    PathBuf::from(name)
}

pub fn pull(ctx: &SyncContext<'_>, opts: &PullOptions) -> Result<PullReport, SyncError> {
    let (cfg, key) = require_config(ctx)?;
    let git = backend(ctx, &cfg)?;
    if !git.is_cloned() {
        return Err(cfg_err("sync repo is not initialized; run init first"));
    }
    git.pull();
    let repo = ctx.repo_path();
    let Some(manifest) = git.manifest() else {
        return Ok(PullReport {
            no_remote: true,
            dry_run: opts.dry_run,
            ..PullReport::default()
        });
    };
    let mut report = PullReport {
        dry_run: opts.dry_run,
        machine_id: manifest.machine_id.clone(),
        pushed_at: manifest.pushed_at.clone(),
        ..PullReport::default()
    };
    if let Some(blob) = sample_entry_blob(&repo, &manifest) {
        if !key_decrypts(&key, &blob)? {
            return Err(cfg_err(
                "cannot decrypt the remote bundle; the local key does not match its passphrase",
            ));
        }
    }
    if opts.dry_run {
        report.changes = Some(detect_changes(ctx, &cfg, &manifest)?);
        return Ok(report);
    }
    let files = read_bundle(&repo, Credential::Key(&key), &ctx.roots)?;
    let targets = ctx.targets(Some(&cfg));
    let mut plan = Vec::new();
    let mut origins: Option<ServerOrigins> = None;
    for file in &files {
        if file.key == SERVER_ORIGINS_KEY {
            origins = serde_json::from_slice(&file.bytes).ok();
            continue;
        }
        if file.category == "project" && !opts.include_projects {
            continue;
        }
        match destination_for(&file.key, &file.category, &targets)? {
            Some(path) => plan.push((file, path)),
            None => report.skipped.push(file.key.clone()),
        }
    }
    let mut state = load_state(ctx);
    if let Some(entry) = manifest.entries.get(SERVER_ORIGINS_KEY) {
        state
            .entries
            .insert(SERVER_ORIGINS_KEY.to_string(), pinned(&entry.hash));
    }
    for (file, path) in plan {
        let Some(entry) = manifest.entries.get(&file.key) else {
            continue;
        };
        if !opts.force {
            if let (Ok(local), Some(prev)) = (fs::read(&path), state.entries.get(&file.key)) {
                let local_hash = lock_hash(&local);
                if local_hash != prev.local_hash_at_sync && entry.hash == prev.remote_hash_at_sync {
                    report.kept_local.push(file.key.clone());
                    continue;
                }
                if local_hash != prev.local_hash_at_sync && entry.hash != prev.remote_hash_at_sync {
                    let saved = remote_sibling(&path);
                    fs::write(&saved, &file.bytes).map_err(|e| io_err(&saved, e))?;
                    report.conflicts.push(ConflictInfo {
                        entry_key: file.key.clone(),
                        local_hash,
                        remote_hash: entry.hash.clone(),
                        remote_file_saved_as: saved.to_string_lossy().into_owned(),
                    });
                    continue;
                }
            }
        }
        write_synced_file(&path, &file.key, &file.bytes)?;
        state.entries.insert(
            file.key.clone(),
            SyncStateEntry {
                local_hash_at_sync: fs::read(&path)
                    .map(|b| lock_hash(&b))
                    .unwrap_or_else(|_| lock_hash(&file.bytes)),
                remote_hash_at_sync: entry.hash.clone(),
            },
        );
        report.applied.push(file.key.clone());
    }
    state.last_sync_at = ctx.now();
    state.last_direction = "pull".into();
    save_state(ctx, &state)?;
    report.skills_repo_changed = report
        .applied
        .iter()
        .filter(|k| k.starts_with("skills_repo/"))
        .count();
    if opts.resolve {
        if let Some(origins) = origins.filter(|o| !o.servers.is_empty()) {
            report.resolved = resolve_servers(
                &origins,
                ctx.exec,
                &ctx.roots,
                &ResolveOptions {
                    run_setup: opts.run_setup,
                },
            );
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffReport {
    pub no_remote: bool,
    pub machine_id: String,
    pub changes: Changes,
}

pub fn diff(ctx: &SyncContext<'_>) -> Result<DiffReport, SyncError> {
    let (cfg, _) = require_config(ctx)?;
    let git = backend(ctx, &cfg)?;
    match git.fetch_manifest() {
        None => Ok(DiffReport {
            no_remote: true,
            machine_id: String::new(),
            changes: Changes::default(),
        }),
        Some(manifest) => Ok(DiffReport {
            no_remote: false,
            machine_id: manifest.machine_id.clone(),
            changes: detect_changes(ctx, &cfg, &manifest)?,
        }),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    pub configured: bool,
    pub backend: Option<String>,
    pub machine_id: Option<String>,
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    pub keyfile_present: bool,
    pub last_sync_at: String,
    pub last_direction: String,
    pub tracked: usize,
    pub projects: BTreeMap<String, SyncProjectConfig>,
}

pub fn status(ctx: &SyncContext<'_>) -> StatusReport {
    let cfg = load_config(ctx);
    let state = load_state(ctx);
    StatusReport {
        configured: ctx.is_configured(),
        backend: cfg.as_ref().map(|c| c.backend.clone()),
        machine_id: cfg.as_ref().map(|c| c.machine_id.clone()),
        repo_url: cfg.as_ref().and_then(|c| c.repo_url.clone()),
        branch: cfg.as_ref().map(|c| c.branch.clone()),
        keyfile_present: ctx.keyfile_path().exists(),
        last_sync_at: state.last_sync_at,
        last_direction: state.last_direction,
        tracked: state.entries.len(),
        projects: cfg.map(|c| c.projects).unwrap_or_default(),
    }
}

pub fn reset(ctx: &SyncContext<'_>) -> Result<Vec<String>, SyncError> {
    if !ctx.is_configured() {
        return Ok(Vec::new());
    }
    let mut removed = Vec::new();
    for path in [ctx.config_path(), ctx.state_path(), ctx.keyfile_path()] {
        if path.exists() {
            fs::remove_file(&path).map_err(|e| io_err(&path, e))?;
            removed.push(path.to_string_lossy().into_owned());
        }
    }
    let repo = ctx.repo_path();
    if repo.exists() {
        fs::remove_dir_all(&repo).map_err(|e| io_err(&repo, e))?;
        removed.push(repo.to_string_lossy().into_owned());
    }
    Ok(removed)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RotateReport {
    pub rotated: usize,
    pub skipped: Vec<String>,
}

pub fn rotate_passphrase(
    ctx: &SyncContext<'_>,
    new_passphrase: &str,
) -> Result<RotateReport, SyncError> {
    check_passphrase(new_passphrase)?;
    let (cfg, old_key) = require_config(ctx)?;
    let git = backend(ctx, &cfg)?;
    git.init()?;
    let repo = ctx.repo_path();
    let manifest = git
        .manifest()
        .filter(|m| !m.entries.is_empty())
        .ok_or_else(|| cfg_err("no manifest on the remote; nothing to rotate"))?;
    let sample = sample_entry_blob(&repo, &manifest)
        .ok_or_else(|| cfg_err("no encrypted entries to rotate"))?;
    if !key_decrypts(&old_key, &sample)? {
        return Err(cfg_err(
            "this machine's key cannot decrypt the remote; rotate from a machine where pull works",
        ));
    }
    let old = FernetKey::parse(&old_key)?;
    let salt = random_salt()?;
    let new_key = derive(new_passphrase, &salt);
    let new = FernetKey::parse(&new_key)?;
    let mut staged = Vec::new();
    let mut skipped = Vec::new();
    for (key, entry) in &manifest.entries {
        if key == SERVER_ORIGINS_KEY {
            continue;
        }
        let path = repo.join(safe_relative(&entry.encrypted_file)?);
        if !path.exists() {
            skipped.push(key.clone());
            continue;
        }
        let token = fs::read(&path).map_err(|e| io_err(&path, e))?;
        let plain = fernet::decrypt(&old, &token)?;
        staged.push((path, fernet::encrypt(&new, &plain)?));
    }
    let head = git.head();
    let write_all = || -> Result<(), SyncError> {
        for (path, token) in &staged {
            fs::write(path, token).map_err(|e| io_err(path, e))?;
        }
        write_salt(&repo, salt.as_slice())
    };
    let pushed = write_all()
        .and_then(|_| git.commit_and_push(&format!("rotate passphrase from {}", cfg.machine_id)));
    if let Err(e) = pushed {
        if let Some(head) = head {
            git.discard_to(&head);
        }
        return Err(e);
    }
    save_keyfile(ctx, &new_key)?;
    Ok(RotateReport {
        rotated: staged.len(),
        skipped,
    })
}

fn check_project_name(name: &str) -> Result<(), SyncError> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(SyncError::UnsafePath(name.to_string()));
    }
    Ok(())
}

pub fn add_project(
    ctx: &SyncContext<'_>,
    name: &str,
    path: &Path,
    files: &[String],
) -> Result<bool, SyncError> {
    check_project_name(name)?;
    let mut cfg =
        load_config(ctx).ok_or_else(|| cfg_err("sync is not configured; run init first"))?;
    let resolved = fs::canonicalize(path).map_err(|e| io_err(path, e))?;
    let files: Vec<String> = if files.is_empty() {
        vec!["CLAUDE.md".into()]
    } else {
        files.to_vec()
    };
    for file in &files {
        safe_relative(file)?;
    }
    let replaced = cfg
        .projects
        .insert(
            name.to_string(),
            SyncProjectConfig {
                local_path: resolved.to_string_lossy().into_owned(),
                files,
            },
        )
        .is_some();
    save_config(ctx, &cfg)?;
    Ok(replaced)
}

pub fn remove_project(ctx: &SyncContext<'_>, name: &str) -> Result<bool, SyncError> {
    let mut cfg = load_config(ctx).ok_or_else(|| cfg_err("sync is not configured"))?;
    let removed = cfg.projects.remove(name).is_some();
    if removed {
        save_config(ctx, &cfg)?;
    }
    Ok(removed)
}

pub fn migrate_bundle(
    ctx: &SyncContext<'_>,
    bundle_dir: &Path,
    cred: Credential<'_>,
    include_projects: bool,
) -> Result<ImportReport, SyncError> {
    read_manifest(bundle_dir)?;
    let cfg = load_config(ctx);
    import_bundle(
        bundle_dir,
        cred,
        &ctx.roots,
        &ctx.targets(cfg.as_ref()),
        include_projects,
    )
}
