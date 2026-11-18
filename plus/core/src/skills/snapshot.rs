//! What a sync is about to change, kept before it writes, and `skills undo`, which puts it back.
//!
//! A [`Journal`] records the prior state of every file the sync touches (content and mode, a
//! symlink's target, or that the file did not exist), the prior `hooks` value of every settings
//! file it writes and the prior lock, under `<data dir>/plus/skills-snapshots/<id>/`. Only the
//! newest [`KEEP`] snapshots stay. Undo restores the newest one that is not used yet and skips
//! whatever changed after the sync wrote it, as a conflict.

use super::auto;
use super::clock::Clock;
use super::lock::lockfile_path;
use super::sync::{delete_stale, installed_hash, rel_or_abs};
use crate::fsx::{atomic_write, atomic_write_bytes, Mode};
use crate::hashing::lock_hash;
use crate::op::OpError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use toolport_plus_api::SettingsStore;

pub const DIR_NAME: &str = "skills-snapshots";
pub const KEEP: usize = 5;
const MANIFEST: &str = "manifest.json";
const BLOBS: &str = "blobs";
const LOCK_BLOB: &str = "lock";

pub fn snapshots_dir(base: &Path) -> PathBuf {
    base.join("plus").join(DIR_NAME)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Prior {
    Absent,
    File {
        blob: String,
        mode: u32,
        hash: String,
    },
    Link {
        target: PathBuf,
        hash: String,
    },
}

impl Prior {
    fn hash(&self) -> Option<&str> {
        match self {
            Self::Absent => None,
            Self::File { hash, .. } | Self::Link { hash, .. } => Some(hash),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    path: PathBuf,
    prior: Prior,
    /// What the sync left behind; `None` when it removed the file.
    after_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HooksEntry {
    path: PathBuf,
    prior: Option<Value>,
    after: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockRecord {
    path: PathBuf,
    has_before: bool,
    after_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: u32,
    id: String,
    created_at: String,
    global: bool,
    lock_dir: PathBuf,
    output_root: PathBuf,
    files: Vec<FileEntry>,
    hooks: Vec<HooksEntry>,
    lock: LockRecord,
    #[serde(default)]
    used_at: Option<String>,
}

#[derive(Default)]
struct Recorded {
    files: Vec<FileEntry>,
    hooks: Vec<HooksEntry>,
    blobs: usize,
}

pub struct Journal {
    dir: PathBuf,
    base: PathBuf,
    id: String,
    created_at: String,
    recorded: RefCell<Recorded>,
}

/// Where a finished journal says the sync stands.
pub struct Context<'a> {
    pub global: bool,
    pub lock_dir: &'a Path,
    pub output_root: &'a Path,
    /// The lock file's bytes as they were before the sync, `None` when there was none.
    pub lock_before: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub id: String,
    pub files: usize,
    pub hooks: usize,
}

impl Info {
    pub fn to_json(&self) -> Value {
        json!({"id": self.id, "files": self.files, "hooks": self.hooks})
    }
}

fn io_error(path: &Path, e: impl std::fmt::Display) -> String {
    format!("{}: {e}", path.display())
}

fn mode_of(meta: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o777
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0o644
    }
}

fn hooks_of(root: &Value) -> Option<Value> {
    root.get("hooks").filter(|v| !v.is_null()).cloned()
}

impl Journal {
    pub fn begin(base: &Path, clock: &dyn Clock) -> Self {
        let now = clock.now();
        let stamp = now.backup_stamp().trim_end_matches('Z').replace('T', "-");
        let root = snapshots_dir(base);
        let mut id = stamp.clone();
        let mut n = 1;
        while root.join(&id).exists() {
            n += 1;
            id = format!("{stamp}-{n:02}");
        }
        Self {
            dir: root.join(&id),
            base: base.to_path_buf(),
            id,
            created_at: now.zulu(),
            recorded: RefCell::default(),
        }
    }

    /// Keeps the state of `path` as it is now, the first time it is named. A sync calls this
    /// before every write or removal and stops when it fails: a change that cannot be undone is
    /// not made.
    pub fn before_change(&self, path: &Path) -> Result<(), String> {
        let mut recorded = self.recorded.borrow_mut();
        if recorded.files.iter().any(|f| f.path == path) {
            return Ok(());
        }
        let prior = match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Prior::Absent,
            Err(e) => return Err(io_error(path, e)),
            Ok(meta) if meta.file_type().is_symlink() => Prior::Link {
                target: fs::read_link(path).map_err(|e| io_error(path, e))?,
                hash: installed_hash(path).unwrap_or_default(),
            },
            Ok(meta) if meta.is_file() => {
                let bytes = fs::read(path).map_err(|e| io_error(path, e))?;
                recorded.blobs += 1;
                let blob = format!("{:05}", recorded.blobs);
                atomic_write_bytes(&self.dir.join(BLOBS).join(&blob), &bytes, Mode::Private)
                    .map_err(|e| io_error(&self.dir, e))?;
                Prior::File {
                    blob,
                    mode: mode_of(&meta),
                    hash: lock_hash(&bytes),
                }
            }
            Ok(_) => return Ok(()),
        };
        recorded.files.push(FileEntry {
            path: path.to_path_buf(),
            prior,
            after_hash: None,
        });
        Ok(())
    }

    /// Keeps the `hooks` value a settings file has now, the first time it is named.
    pub fn before_hooks(&self, path: &Path, prior: Option<Value>) {
        let mut recorded = self.recorded.borrow_mut();
        if recorded.hooks.iter().any(|h| h.path == path) {
            return;
        }
        recorded.hooks.push(HooksEntry {
            path: path.to_path_buf(),
            prior,
            after: None,
        });
    }

    /// Writes the manifest of what changed and prunes older snapshots. `None` when the sync
    /// changed nothing, so a run that did nothing never pushes a useful snapshot out.
    pub fn finish(
        self,
        settings: Option<&dyn SettingsStore>,
        scope: Context<'_>,
    ) -> Result<Option<Info>, String> {
        let recorded = self.recorded.into_inner();
        let mut files = Vec::new();
        for mut entry in recorded.files {
            entry.after_hash = installed_hash(&entry.path);
            let unchanged = entry.after_hash.as_deref() == entry.prior.hash()
                && match &entry.prior {
                    Prior::File { mode, .. } => fs::metadata(&entry.path)
                        .map(|m| mode_of(&m) == *mode)
                        .unwrap_or(false),
                    _ => true,
                };
            if !unchanged {
                files.push(entry);
            }
        }
        let mut hooks = Vec::new();
        for mut entry in recorded.hooks {
            entry.after = settings
                .and_then(|s| s.read_settings(&entry.path).ok())
                .and_then(|(root, _)| hooks_of(&root));
            if entry.after != entry.prior {
                hooks.push(entry);
            }
        }
        if files.is_empty() && hooks.is_empty() {
            let _ = fs::remove_dir_all(&self.dir);
            return Ok(None);
        }
        let lock_path = lockfile_path(scope.lock_dir);
        if let Some(bytes) = &scope.lock_before {
            atomic_write_bytes(&self.dir.join(BLOBS).join(LOCK_BLOB), bytes, Mode::Private)
                .map_err(|e| io_error(&self.dir, e))?;
        }
        let keep: Vec<&str> = files
            .iter()
            .filter_map(|f| match &f.prior {
                Prior::File { blob, .. } => Some(blob.as_str()),
                _ => None,
            })
            .chain(scope.lock_before.as_ref().map(|_| LOCK_BLOB))
            .collect();
        prune_blobs(&self.dir.join(BLOBS), &keep);
        let manifest = Manifest {
            version: 1,
            id: self.id.clone(),
            created_at: self.created_at,
            global: scope.global,
            lock_dir: scope.lock_dir.to_path_buf(),
            output_root: scope.output_root.to_path_buf(),
            lock: LockRecord {
                path: lock_path.clone(),
                has_before: scope.lock_before.is_some(),
                after_hash: fs::read(&lock_path).ok().map(lock_hash),
            },
            files,
            hooks,
            used_at: None,
        };
        write_manifest(&self.dir, &manifest)?;
        prune(&self.base);
        Ok(Some(Info {
            id: manifest.id,
            files: manifest.files.len(),
            hooks: manifest.hooks.len(),
        }))
    }
}

/// A settings store that tells the journal what `hooks` held before each write.
pub struct Recording<'a> {
    pub inner: &'a dyn SettingsStore,
    pub journal: &'a Journal,
}

impl SettingsStore for Recording<'_> {
    fn claude_settings_paths(&self) -> Vec<PathBuf> {
        self.inner.claude_settings_paths()
    }

    fn read_settings(&self, path: &Path) -> Result<(Value, Option<String>), String> {
        self.inner.read_settings(path)
    }

    fn write_hooks(&self, path: &Path, original: Option<&str>, root: &Value) -> Result<(), String> {
        let prior = self
            .inner
            .read_settings(path)
            .ok()
            .and_then(|(current, _)| hooks_of(&current));
        self.journal.before_hooks(path, prior);
        self.inner.write_hooks(path, original, root)
    }

    fn write_key(
        &self,
        path: &Path,
        original: Option<&str>,
        root: &Value,
        key: &str,
    ) -> Result<(), String> {
        self.inner.write_key(path, original, root, key)
    }
}

fn prune_blobs(dir: &Path, keep: &[&str]) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !keep.contains(&name.as_str()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn write_manifest(dir: &Path, manifest: &Manifest) -> Result<(), String> {
    let text = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())? + "\n";
    atomic_write(&dir.join(MANIFEST), &text, Mode::Private).map_err(|e| io_error(dir, e))
}

fn ids(base: &Path) -> Vec<String> {
    let mut ids: Vec<String> = fs::read_dir(snapshots_dir(base))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    ids.sort();
    ids
}

fn read_manifest(base: &Path, id: &str) -> Option<Manifest> {
    let text = fs::read_to_string(snapshots_dir(base).join(id).join(MANIFEST)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Keeps the newest [`KEEP`] snapshots and drops the rest, with any that never got a manifest
/// (a run that died before it finished).
fn prune(base: &Path) {
    let root = snapshots_dir(base);
    let (complete, broken): (Vec<String>, Vec<String>) = ids(base)
        .into_iter()
        .partition(|id| root.join(id).join(MANIFEST).is_file());
    for id in broken {
        let _ = fs::remove_dir_all(root.join(id));
    }
    let drop = complete.len().saturating_sub(KEEP);
    for id in complete.into_iter().take(drop) {
        let _ = fs::remove_dir_all(root.join(id));
    }
}

enum Act {
    Restore,
    Remove,
    Keep,
    Conflict,
}

impl Act {
    fn name(&self) -> &'static str {
        match self {
            Self::Restore => "restore",
            Self::Remove => "remove",
            Self::Keep => "keep",
            Self::Conflict => "conflict",
        }
    }
}

const CHANGED_AFTER: &str = "changed after the sync wrote it";

/// What undo does with one thing: `after` is what the sync left, `prior` what it found.
fn decide<T: PartialEq>(current: &T, after: &T, prior: &T, prior_absent: bool) -> Act {
    if current == after {
        if prior_absent {
            Act::Remove
        } else {
            Act::Restore
        }
    } else if current == prior {
        Act::Keep
    } else {
        Act::Conflict
    }
}

struct FilePlan<'a> {
    entry: &'a FileEntry,
    act: Act,
}

fn file_plan(entry: &FileEntry) -> Act {
    let prior = entry.prior.hash().map(str::to_string);
    let absent = prior.is_none();
    decide(
        &installed_hash(&entry.path),
        &entry.after_hash,
        &prior,
        absent,
    )
}

fn restore_file(manifest_dir: &Path, entry: &FileEntry) -> Result<(), String> {
    match &entry.prior {
        Prior::Absent => Ok(()),
        Prior::File { blob, mode, .. } => {
            let bytes = fs::read(manifest_dir.join(BLOBS).join(blob))
                .map_err(|e| io_error(&manifest_dir.join(BLOBS).join(blob), e))?;
            if fs::symlink_metadata(&entry.path).is_ok_and(|m| m.file_type().is_symlink()) {
                fs::remove_file(&entry.path).map_err(|e| io_error(&entry.path, e))?;
            }
            atomic_write_bytes(&entry.path, &bytes, Mode::Exact(*mode))
                .map_err(|e| io_error(&entry.path, e))
        }
        Prior::Link { target, .. } => {
            if let Some(parent) = entry.path.parent() {
                fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
            }
            let _ = fs::remove_file(&entry.path);
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(target, &entry.path)
                    .map_err(|e| io_error(&entry.path, e))
            }
            #[cfg(not(unix))]
            {
                let _ = target;
                Err(io_error(
                    &entry.path,
                    "symbolic links are not restored here",
                ))
            }
        }
    }
}

fn restore_hooks(store: &dyn SettingsStore, entry: &HooksEntry) -> Result<(), String> {
    let (mut root, text) = store.read_settings(&entry.path)?;
    let map = root
        .as_object_mut()
        .ok_or_else(|| format!("{}: top level is not an object", entry.path.display()))?;
    match &entry.prior {
        Some(value) => {
            map.insert("hooks".into(), value.clone());
        }
        None => {
            map.remove("hooks");
        }
    }
    store.write_hooks(&entry.path, text.as_deref(), &root)
}

fn current_hooks(store: &dyn SettingsStore, path: &Path) -> Result<Option<Value>, String> {
    store.read_settings(path).map(|(root, _)| hooks_of(&root))
}

/// Restores the newest snapshot of `lock_dir` that is not used yet. `dry_run` reports the same
/// plan and writes nothing. A `hooks` value that changed since the sync refuses the whole undo
/// before anything is touched; a file or the lock that changed is skipped and listed.
pub fn undo(
    base: &Path,
    lock_dir: &Path,
    dry_run: bool,
    settings: Option<&dyn SettingsStore>,
    clock: &dyn Clock,
) -> Result<Value, OpError> {
    let manifest = ids(base)
        .into_iter()
        .rev()
        .filter_map(|id| read_manifest(base, &id))
        .find(|m| m.used_at.is_none() && m.lock_dir == lock_dir)
        .ok_or_else(|| OpError::not_found("no snapshot to undo: run skills sync first"))?;
    let dir = snapshots_dir(base).join(&manifest.id);
    let plans: Vec<FilePlan<'_>> = manifest
        .files
        .iter()
        .map(|entry| FilePlan {
            entry,
            act: file_plan(entry),
        })
        .collect();

    let mut hook_rows = Vec::new();
    let mut hook_conflicts = Vec::new();
    for entry in &manifest.hooks {
        let store = settings.ok_or_else(|| {
            OpError::failed(
                "undo",
                "the snapshot holds hooks and no settings store is set",
            )
        })?;
        let current = current_hooks(store, &entry.path).map_err(|e| OpError::failed("undo", e))?;
        let act = decide(&current, &entry.after, &entry.prior, false);
        if matches!(act, Act::Conflict) {
            hook_conflicts.push(entry.path.display().to_string());
        }
        hook_rows.push((entry, act));
    }

    let lock_current = fs::read(&manifest.lock.path).ok().map(lock_hash);
    let lock_act = match (&lock_current, &manifest.lock.after_hash) {
        (a, b) if a == b => {
            if manifest.lock.has_before {
                Act::Restore
            } else {
                Act::Remove
            }
        }
        _ => Act::Conflict,
    };

    let rows: Vec<Value> = plans
        .iter()
        .map(|p| {
            json!({
                "path": rel_or_abs(&p.entry.path, &manifest.output_root),
                "action": p.act.name(),
                "reason": matches!(p.act, Act::Conflict).then_some(CHANGED_AFTER),
            })
        })
        .collect();
    let count = |names: &[&str]| {
        plans
            .iter()
            .filter(|p| names.contains(&p.act.name()))
            .count()
    };
    let mut data = json!({
        "dryRun": dry_run,
        "snapshot": {
            "id": manifest.id,
            "createdAt": manifest.created_at,
            "files": manifest.files.len(),
            "hooks": manifest.hooks.len(),
        },
        "files": rows,
        "restored": count(&["restore"]),
        "removed": count(&["remove"]),
        "conflicts": count(&["conflict"]) + hook_conflicts.len() + usize::from(matches!(lock_act, Act::Conflict)),
        "hooks": hook_rows
            .iter()
            .map(|(e, act)| json!({"path": e.path.to_string_lossy(), "action": act.name()}))
            .collect::<Vec<_>>(),
        "lock": {
            "path": manifest.lock.path.to_string_lossy(),
            "action": lock_act.name(),
        },
    });

    if !hook_conflicts.is_empty() {
        let message = format!(
            "hooks in {} changed since the sync; undo would overwrite them. Nothing was restored",
            hook_conflicts.join(", ")
        );
        if dry_run {
            data["refused"] = json!(message);
            return Ok(data);
        }
        return Err(OpError::conflict(message).with_details(data));
    }
    if dry_run {
        data["autoSync"] = json!({"willPause": true});
        return Ok(data);
    }

    for (entry, act) in &hook_rows {
        if matches!(act, Act::Restore) {
            let store = settings.expect("checked above");
            restore_hooks(store, entry).map_err(|e| OpError::failed("undo", e))?;
        }
    }
    let mut doomed = Vec::new();
    for plan in &plans {
        match plan.act {
            Act::Restore => {
                restore_file(&dir, plan.entry).map_err(|e| OpError::failed("undo", e))?
            }
            Act::Remove => doomed.push(plan.entry.path.clone()),
            Act::Keep | Act::Conflict => {}
        }
    }
    delete_stale(&doomed, &manifest.output_root);
    match lock_act {
        Act::Restore => {
            let bytes = fs::read(dir.join(BLOBS).join(LOCK_BLOB))
                .map_err(|e| OpError::failed("undo", io_error(&dir, e)))?;
            atomic_write_bytes(&manifest.lock.path, &bytes, Mode::Private)
                .map_err(|e| OpError::failed("undo", io_error(&manifest.lock.path, e)))?;
        }
        Act::Remove => {
            let _ = fs::remove_file(&manifest.lock.path);
        }
        Act::Keep | Act::Conflict => {}
    }
    let mut used = manifest.clone();
    used.used_at = Some(clock.now().zulu());
    write_manifest(&dir, &used).map_err(|e| OpError::failed("undo", e))?;
    let state = auto::pause(base, auto::REASON_UNDO, clock)?;
    data["autoSync"] = state.0.to_json();
    Ok(data)
}

#[cfg(test)]
pub(crate) fn newest(base: &Path) -> Option<String> {
    ids(base).pop()
}

#[cfg(test)]
pub(crate) fn count(base: &Path) -> usize {
    ids(base).len()
}
