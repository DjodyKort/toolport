//! `setup parts` (what an export would hold) and `setup export` (one zip with a manifest).

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::parts::{self, Body, Build, Entry, Flagged, Part};
use super::repos::{self, Repo};
use super::scratch::{self, Scratch};
use super::{crypto, hex, search_roots, zipio, Ctx, PartId, ALL_PARTS, FORMAT, MANIFEST, VERSION};
use crate::op::OpError;

/// Sub-choices a caller can switch off with `--without`.
pub const SUBS: [&str; 6] = [
    "claude-instructions",
    "claude-settings",
    "claude-plugins",
    "claude-own",
    "memory-auto",
    "memory-switches",
];
pub const SUBS_EXTRA: [&str; 1] = ["memory-instructions"];

#[derive(Clone, Debug)]
pub struct Options {
    pub parts: BTreeSet<PartId>,
    pub roots: Vec<PathBuf>,
    pub worktrees: bool,
    pub skip_no_remote: bool,
    pub library_zip: bool,
    pub without: BTreeSet<String>,
    pub session_repos: Vec<String>,
    pub out: Option<PathBuf>,
    pub name: Option<String>,
    pub peek: Option<PartId>,
    pub dry_run: bool,
    /// The whole file, manifest included, is sealed with the passphrase (the sync form).
    pub seal_file: bool,
    /// The records other machines published, carried into the file unchanged (`serverRepos`).
    pub peer_records: Vec<(String, Value)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            parts: ALL_PARTS.into_iter().filter(|p| p.in_all()).collect(),
            roots: Vec::new(),
            worktrees: true,
            skip_no_remote: false,
            library_zip: false,
            without: BTreeSet::new(),
            session_repos: Vec::new(),
            out: None,
            name: None,
            peek: None,
            dry_run: false,
            seal_file: false,
            peer_records: Vec::new(),
        }
    }
}

impl Options {
    pub fn secrets_on(&self) -> bool {
        self.parts.contains(&PartId::Secrets)
    }

    pub fn needs_passphrase(&self) -> bool {
        self.seal_file || self.parts.iter().any(|p| p.sealed())
    }
}

pub fn valid_sub(name: &str) -> bool {
    SUBS.contains(&name) || SUBS_EXTRA.contains(&name)
}

struct Scan {
    roots: Vec<PathBuf>,
    repos: Vec<Repo>,
}

fn scan_repos(ctx: &Ctx<'_>, opts: &Options, ignored_names: bool) -> Scan {
    let needs_repos = [PartId::Repos, PartId::Memory, PartId::History]
        .iter()
        .any(|p| opts.parts.contains(p));
    if !needs_repos {
        return Scan {
            roots: Vec::new(),
            repos: Vec::new(),
        };
    }
    let roots = search_roots(ctx, &opts.roots);
    let repos = repos::find(&roots)
        .iter()
        .map(|dir| repos::read(dir, &ctx.home, ignored_names))
        .collect();
    Scan { roots, repos }
}

fn build_all(
    ctx: &Ctx<'_>,
    opts: &Options,
    scan: &Scan,
    scratch: &Path,
    content: bool,
    passphrase: Option<&str>,
) -> Vec<Part> {
    let memory = ctx.memory();
    let b = Build {
        ctx,
        opts,
        repos: &scan.repos,
        memory: &memory,
        scratch,
        content,
    };
    let mut flagged: Vec<Flagged> = Vec::new();
    let mut declared = 0;
    let mut parts = Vec::new();
    for id in ALL_PARTS {
        if !opts.parts.contains(&id) {
            continue;
        }
        let part = match id {
            PartId::Repos => parts::repos_part(&b, &scan.roots),
            PartId::Servers => {
                let part = parts::servers_part(&b);
                declared = part.info["secretValues"].as_u64().unwrap_or(0);
                part
            }
            PartId::Access => parts::access_part(&b),
            PartId::Library => parts::library_part(&b),
            PartId::Tasks => parts::tasks_part(&b),
            PartId::Settings => parts::settings_part(&b),
            PartId::ClaudeUser => {
                let (part, held) = parts::claude_user_part(&b);
                flagged.extend(held);
                part
            }
            PartId::Memory => {
                let (part, held) = parts::memory_part(&b);
                flagged.extend(held);
                part
            }
            PartId::History => parts::history_part(&b, passphrase),
            PartId::Secrets => parts::secrets_part(&b, passphrase, &flagged, declared),
            PartId::ServerRepos => parts::server_repos_part(&b),
        };
        parts.push(part);
    }
    parts
}

fn manifest_key(id: PartId) -> (&'static str, &'static str) {
    match id {
        PartId::Repos => ("file", "repos.json"),
        PartId::Servers => ("file", "servers.json"),
        PartId::Access => ("file", "access.json"),
        PartId::Library => ("dir", "skills-library/"),
        PartId::Tasks => ("file", "tasks.json"),
        PartId::Settings => ("file", "settings.json"),
        PartId::ClaudeUser => ("dir", "claude-user/"),
        PartId::Memory => ("dir", "memory/"),
        PartId::History => ("file", "history.enc"),
        PartId::Secrets => ("file", "secrets.enc"),
        PartId::ServerRepos => ("dir", "server-repos/"),
    }
}

fn manifest_row(part: &Part) -> Result<Value, OpError> {
    let (key, name) = manifest_key(part.id);
    let mut row = json!({
        "id": part.id.id(),
        key: name,
        "sha256": part.hash().map_err(|e| OpError::failed("read", e))?,
        "bytes": part.bytes(),
        "count": part.count,
    });
    if part.id.sealed() {
        row["encrypted"] = json!(true);
        row["kdf"] = json!("argon2id");
        row["cipher"] = json!("xchacha20poly1305");
    }
    Ok(row)
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_MACHINE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

pub fn machine_name() -> String {
    #[cfg(test)]
    if let Some(name) = TEST_MACHINE.with(|m| m.borrow().clone()) {
        return name;
    }
    let named = ["HOSTNAME", "COMPUTERNAME"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()));
    named
        .or_else(|| {
            let out = crate::skills::gitexec::run_command(
                std::process::Command::new("hostname"),
                std::time::Duration::from_secs(2),
            )
            .ok()?;
            Some(out.stdout.trim().to_string()).filter(|s| !s.is_empty() && out.ok())
        })
        .unwrap_or_else(|| "this machine".into())
}

fn part_json(part: &Part) -> Value {
    json!({
        "id": part.id.id(),
        "label": part.id.label(),
        "sealed": part.id.sealed(),
        "count": part.count,
        "fileSize": part.bytes(),
        "notes": part.notes,
        "flagged": part.flagged,
        "heldBack": part.held,
        "info": part.info,
        "error": part.error,
    })
}

fn repo_summary(repo: &Repo) -> Value {
    json!({
        "name": repo.name,
        "path": repo.path,
        "remote": repo.origin_url(),
        "branch": repo.branch,
        "upstream": repo.upstream,
        "dirty": repo.dirty,
        "untracked": repo.untracked,
        "stashes": repo.stashes,
        "unpushed": repo.unpushed_commits(),
        "neverPushed": repo.never_pushed,
        "noRemote": repo.no_remote,
        "worktrees": repo.worktrees.len(),
        "error": repo.error,
    })
}

pub fn last_export_path(ctx: &Ctx<'_>) -> PathBuf {
    ctx.state_dir().join("last-export.json")
}

pub fn last_export(ctx: &Ctx<'_>) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(last_export_path(ctx)).ok()?).ok()
}

/// `setup parts`: what an export would hold, with counts and sizes. Reads; builds nothing sealed
/// and reads nothing from the vault.
pub fn list(ctx: &Ctx<'_>, roots: &[PathBuf]) -> Result<Value, OpError> {
    let mut opts = Options {
        roots: roots.to_vec(),
        parts: ALL_PARTS.into_iter().filter(|p| !p.sync_only()).collect(),
        library_zip: true,
        ..Options::default()
    };
    opts.parts.insert(PartId::History);
    opts.parts.insert(PartId::Secrets);
    let scan = scan_repos(ctx, &opts, false);
    let parts = build_all(ctx, &opts, &scan, Path::new(""), false, None);
    let total: u64 = parts
        .iter()
        .filter(|p| !p.id.sealed())
        .map(Part::bytes)
        .sum();
    Ok(json!({
        "roots": scan.roots.iter().map(|r| ctx.tilde(r)).collect::<Vec<_>>(),
        "repos": scan.repos.iter().map(repo_summary).collect::<Vec<_>>(),
        "parts": parts
            .iter()
            .map(|p| {
                let mut value = part_json(p);
                value["default"] = json!(p.id.in_all());
                value
            })
            .collect::<Vec<_>>(),
        "totalSize": total,
        "encryption": crypto::available(),
        "minPassphrase": crypto::MIN_PASSPHRASE_CHARS,
        "lastExport": last_export(ctx),
    }))
}

fn export_name(ctx: &Ctx<'_>, opts: &Options) -> Result<String, OpError> {
    if let Some(name) = &opts.name {
        if name.contains('/') || name.contains('\\') || !name.ends_with(".zip") {
            return Err(OpError::usage("--name is a file name ending in .zip"));
        }
        return Ok(name.clone());
    }
    let (y, m, d) = crate::fsx::civil_from_days(ctx.host.now_unix().div_euclid(86_400));
    Ok(format!("toolport-setup-{y:04}-{m:02}-{d:02}.zip"))
}

fn target_path(ctx: &Ctx<'_>, opts: &Options) -> Result<Option<PathBuf>, OpError> {
    let Some(out) = &opts.out else {
        return Ok(None);
    };
    let name = export_name(ctx, opts)?;
    let is_dir = out.is_dir() || out.to_string_lossy().ends_with('/');
    let path = if is_dir {
        out.join(&name)
    } else if opts.name.is_some() {
        return Err(OpError::usage(
            "--name goes with a folder in --out, not a file",
        ));
    } else {
        out.clone()
    };
    Ok(Some(path))
}

struct HashingSink(Sha256);

impl Write for HashingSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Opens the written file again: every part hashes as the manifest says, and the sealed parts open
/// with the passphrase.
fn verify(path: &Path, rows: &[Value], passphrase: Option<&str>) -> Result<(), OpError> {
    let bad = |why: String| OpError::failed("verify", why);
    let mut zip = zipio::ZipFile::open(path).map_err(bad)?;
    for row in rows {
        let id = row["id"].as_str().unwrap_or_default();
        let names: Vec<String> = match (row["file"].as_str(), row["dir"].as_str()) {
            (Some(file), _) => vec![file.to_string()],
            (_, Some(dir)) => zip
                .names()
                .filter(|n| n.starts_with(dir))
                .map(String::from)
                .collect(),
            _ => Vec::new(),
        };
        let mut lines = std::collections::BTreeMap::new();
        for name in &names {
            let mut sink = HashingSink(Sha256::new());
            zip.copy(name, &mut sink).map_err(bad)?;
            lines.insert(name.clone(), hex(&sink.0.finalize()));
        }
        let hash = if let [(_, only)] = lines.iter().collect::<Vec<_>>().as_slice() {
            (*only).clone()
        } else {
            let mut hasher = Sha256::new();
            for (name, hash) in &lines {
                hasher.update(name.as_bytes());
                hasher.update([0]);
                hasher.update(hash.as_bytes());
                hasher.update(b"\n");
            }
            hex(&hasher.finalize())
        };
        if Some(hash.as_str()) != row["sha256"].as_str() {
            return Err(bad(format!("{id} was written differently than planned")));
        }
        if row["encrypted"] == true {
            let pass = passphrase
                .ok_or_else(|| bad(format!("{id} needs the passphrase to be checked")))?;
            let name = row["file"].as_str().unwrap_or_default();
            let mut reader = zip.stream(name).map_err(bad)?;
            crypto::open(pass, &mut reader, &mut std::io::sink())
                .map_err(|e| bad(format!("{id} does not open with the passphrase: {e}")))?;
        }
    }
    Ok(())
}

/// What the file does not hold, in words, for the result.
pub fn not_included(parts: &BTreeSet<PartId>) -> Vec<&'static str> {
    let mut out = vec![
        "your code and anything uncommitted, stashed or unpushed",
        "server sign-in sessions (you sign in again)",
        "Claude Code login, SSH keys and git credentials",
        "files in .gitignore such as .env",
    ];
    if !parts.contains(&PartId::History) {
        out.push("past conversations (sessions), unless you tick them");
    }
    if !parts.contains(&PartId::Secrets) {
        out.push("secret values, unless you tick Secrets");
    }
    out
}

pub fn run(ctx: &Ctx<'_>, opts: &Options, passphrase: Option<&str>) -> Result<Value, OpError> {
    scratch::sweep(ctx);
    if opts.needs_passphrase() && (!opts.dry_run || passphrase.is_some() || opts.peek.is_some()) {
        let pass = passphrase.ok_or_else(|| {
            OpError::usage("secrets and sessions need a passphrase: use --passphrase-stdin")
        })?;
        crypto::check_passphrase(pass).map_err(OpError::usage)?;
        if !crypto::available() {
            return Err(OpError::failed(
                "no_encryption",
                "this build has no encryption",
            ));
        }
    }
    let target = target_path(ctx, opts)?;
    if let Some(target) = &target {
        if target.exists() {
            return Err(OpError::conflict(format!(
                "{} exists: an export never replaces a file",
                target.display()
            )));
        }
    }
    let scan = scan_repos(ctx, opts, false);
    let content = !opts.dry_run || opts.peek.is_some();
    // A run that writes nothing needs no working folder.
    let scratch = content.then(|| Scratch::new(ctx)).transpose()?;
    let scratch_path = scratch.as_ref().map_or(Path::new(""), Scratch::path);
    let parts = build_all(ctx, opts, &scan, scratch_path, content, passphrase);

    let failed: Vec<String> = parts
        .iter()
        .filter_map(|p| p.error.as_ref().map(|e| format!("{}: {e}", p.id.id())))
        .collect();
    if !failed.is_empty() {
        return Err(OpError::failed("part", failed.join("; ")));
    }
    if let Some(id) = opts.peek {
        let part = parts
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| OpError::usage(format!("{} is not in --parts", id.id())))?;
        return Ok(peek(part));
    }

    // A sessions part with nothing in it has no file to seal: it is left out and said so.
    let (empty, parts): (Vec<Part>, Vec<Part>) = parts
        .into_iter()
        .partition(|p| p.id == PartId::History && p.count == 0);
    let skipped: Vec<&str> = empty
        .iter()
        .map(|_| "sessions: none found for the chosen repositories")
        .collect();
    let tree = tree_of(&parts);
    let total: u64 = parts.iter().map(Part::bytes).sum();
    let summary: Vec<Value> = parts.iter().map(part_json).collect();
    let name = export_name(ctx, opts)?;
    if opts.dry_run {
        return Ok(json!({
            "dryRun": true,
            "file": target.as_ref().map(|t| t.to_string_lossy().into_owned()),
            "name": name,
            "parts": summary,
            "tree": tree,
            "totalSize": total,
            "notIncluded": not_included(&opts.parts),
            "heldBackSealed": opts.secrets_on(),
            "skipped": skipped,
            "plan": {"summary": format!("{} parts, about {} bytes", parts.len(), total), "steps": []},
        }));
    }
    let target = target.ok_or_else(|| OpError::usage("--out is needed to write the file"))?;
    let rows: Vec<Value> = parts.iter().map(manifest_row).collect::<Result<_, _>>()?;
    let now = ctx.host.now_unix();
    let manifest = json!({
        "format": FORMAT,
        "version": VERSION,
        "madeAt": crate::tasks::cron::rfc3339(now),
        "machine": machine_name(),
        "toolport": ctx.host.app_version(),
        "parts": rows,
    });
    let partial = PathBuf::from(format!("{}.part", target.display()));
    let plain = if opts.seal_file {
        scratch_path.join("setup.zip")
    } else {
        partial.clone()
    };
    let written = write_zip(&plain, &manifest, &parts)
        .and_then(|()| verify(&plain, &rows, passphrase))
        .and_then(|()| match (opts.seal_file, passphrase) {
            (true, Some(pass)) => seal_file(&plain, &partial, pass),
            _ => Ok(()),
        })
        .and_then(|()| {
            std::fs::rename(&partial, &target)
                .map_err(|e| OpError::failed("write", format!("{}: {e}", target.display())))
        });
    if let Err(error) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    let bytes = std::fs::metadata(&target).map_or(0, |m| m.len());
    if !opts.seal_file {
        record_last(ctx, &target, &manifest, bytes);
    }
    let mut data = json!({
        "dryRun": false,
        "file": target.to_string_lossy(),
        "name": target.file_name().map(|n| n.to_string_lossy().into_owned()),
        "fileSize": bytes,
        "madeAt": manifest["madeAt"],
        "parts": summary,
        "tree": tree,
        "verified": true,
        "notIncluded": not_included(&opts.parts),
        "heldBackSealed": opts.secrets_on(),
        "skipped": skipped,
        "result": {"undo": "delete the file"},
    });
    if opts.seal_file {
        data["sealedFile"] = json!(true);
    }
    Ok(data)
}

/// Seals the finished zip as a whole and opens the result once more before it is kept.
fn seal_file(plain: &Path, out: &Path, passphrase: &str) -> Result<(), OpError> {
    let write = |e: String| OpError::failed("write", e);
    let mut input = std::fs::File::open(plain).map_err(|e| write(e.to_string()))?;
    let mut sealed = parts::create_private(out).map_err(write)?;
    crypto::seal(passphrase, &mut input, &mut sealed).map_err(write)?;
    sealed.sync_all().map_err(|e| write(e.to_string()))?;
    drop(sealed);
    let mut again = std::fs::File::open(out).map_err(|e| write(e.to_string()))?;
    crypto::open(passphrase, &mut again, &mut std::io::sink())
        .map_err(|e| OpError::failed("verify", format!("the sealed file does not open: {e}")))
}

fn write_zip(path: &Path, manifest: &Value, parts: &[Part]) -> Result<(), OpError> {
    let write = |e: String| OpError::failed("write", e);
    let mut zip = zipio::StreamZip::create(path).map_err(write)?;
    let mut text = serde_json::to_string_pretty(manifest).unwrap_or_default();
    text.push('\n');
    zip.add(MANIFEST, text.as_bytes()).map_err(write)?;
    for part in parts {
        let mut entries: Vec<&Entry> = part.entries.iter().collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries {
            match &entry.body {
                Body::Bytes(bytes) => zip.add(&entry.name, bytes).map_err(write)?,
                Body::File(file) => {
                    zip.add_file(&entry.name, file).map_err(write)?;
                }
            }
        }
    }
    zip.finish().map_err(write)
}

fn tree_of(parts: &[Part]) -> Vec<Value> {
    let mut tree = vec![json!({"name": MANIFEST, "fileSize": 0, "part": null})];
    for part in parts {
        let (key, name) = manifest_key(part.id);
        tree.push(json!({
            "name": name,
            "fileSize": part.bytes(),
            "part": part.id.id(),
            "count": part.count,
            "kind": key,
            "sealed": part.id.sealed(),
        }));
    }
    tree
}

fn peek(part: &Part) -> Value {
    let files: Vec<Value> = part
        .entries
        .iter()
        .map(|entry| match &entry.body {
            Body::Bytes(bytes) if entry.name.ends_with(".json") => json!({
                "name": entry.name,
                "json": serde_json::from_slice::<Value>(bytes).unwrap_or(Value::Null),
            }),
            Body::Bytes(bytes) if entry.name.ends_with(".md") || !part.id.sealed() => json!({
                "name": entry.name,
                "fileSize": bytes.len(),
            }),
            _ => json!({"name": entry.name, "fileSize": entry.size()}),
        })
        .collect();
    json!({"dryRun": true, "peek": {"part": part.id.id(), "label": part.id.label(), "files": files}})
}

fn record_last(ctx: &Ctx<'_>, target: &Path, manifest: &Value, bytes: u64) {
    let doc = json!({
        "version": 1,
        "file": target.to_string_lossy(),
        "madeAt": manifest["madeAt"],
        "bytes": bytes,
        "parts": manifest["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| (p["id"].as_str().unwrap_or("").to_string(), json!({"count": p["count"], "sha256": p["sha256"]})))
            .collect::<serde_json::Map<_, _>>(),
    });
    let mut text = serde_json::to_string_pretty(&doc).unwrap_or_default();
    text.push('\n');
    let _ = crate::fsx::write_private(&last_export_path(ctx), &text);
}
