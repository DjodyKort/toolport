use super::exec::GitRunner;
use super::gitops;
use crate::plus::tags;
use crate::registry::ServerEntry;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const META_KEY: &str = "mcpmSource";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub remote: String,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Git {
        path: String,
        remote: String,
        branch: String,
        upstream: Option<Upstream>,
        post_update: Option<String>,
        /// Recomputed only when something re-runs detection against the entry's current
        /// args/cwd (MIG-UPD-4 phase 2); `true` means the stored path no longer matches the
        /// launch directory. Read-only for now: nothing edits the source from this flag yet.
        drift: bool,
    },
    GithubRelease {
        path: String,
        repo: Option<String>,
        current_version: Option<String>,
        asset_pattern: Option<String>,
        verify_command: Option<String>,
    },
    Npx {
        package: String,
    },
    Uvx {
        package: String,
    },
    Remote,
    Unknown {
        reason: String,
    },
}

impl Source {
    pub fn kind(&self) -> &'static str {
        match self {
            Source::Git { .. } => "git",
            Source::GithubRelease { .. } => "github-release",
            Source::Npx { .. } => "npx",
            Source::Uvx { .. } => "uvx",
            Source::Remote => "remote",
            Source::Unknown { .. } => "unknown",
        }
    }

    fn git_fallback(path: &Path) -> Source {
        Source::Git {
            path: path.to_string_lossy().into_owned(),
            remote: "origin".into(),
            branch: String::new(),
            upstream: None,
            post_update: None,
            drift: false,
        }
    }

    pub fn to_meta(&self) -> Map<String, Value> {
        fn put(m: &mut Map<String, Value>, k: &str, v: &Option<String>) {
            if let Some(v) = v {
                m.insert(k.into(), json!(v));
            }
        }
        let mut m = Map::new();
        m.insert("type".into(), json!(self.kind()));
        match self {
            Source::Git {
                path,
                remote,
                branch,
                upstream,
                post_update,
                drift,
            } => {
                m.insert("path".into(), json!(path));
                m.insert("remote".into(), json!(remote));
                m.insert("branch".into(), json!(branch));
                if let Some(u) = upstream {
                    m.insert(
                        "upstream".into(),
                        json!({"remote": u.remote, "branch": u.branch}),
                    );
                }
                put(&mut m, "post_update", post_update);
                m.insert("drift".into(), json!(*drift));
            }
            Source::GithubRelease {
                path,
                repo,
                current_version,
                asset_pattern,
                verify_command,
            } => {
                put(&mut m, "repo", repo);
                put(&mut m, "current_version", current_version);
                put(&mut m, "asset_pattern", asset_pattern);
                put(&mut m, "verify_command", verify_command);
                m.insert("path".into(), json!(path));
            }
            Source::Npx { package } | Source::Uvx { package } => {
                m.insert("package".into(), json!(package));
            }
            Source::Remote => {}
            Source::Unknown { reason } => {
                m.insert("reason".into(), json!(reason));
            }
        }
        m
    }
}

fn text(obj: &Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn upstream_from(obj: &Map<String, Value>) -> Option<Upstream> {
    let u = obj.get("upstream")?.as_object()?;
    Some(Upstream {
        remote: text(u, "remote")?,
        branch: text(u, "branch")?,
    })
}

pub fn from_meta(meta: &Value) -> Option<Source> {
    let obj = meta.as_object()?;
    match obj.get("type")?.as_str()? {
        "git" => Some(Source::Git {
            path: text(obj, "path")?,
            // Pre-MIG-UPD-4 metadata had no `remote`/`upstream`: every stored source used to
            // mean "origin, no upstream tracked".
            remote: text(obj, "remote").unwrap_or_else(|| "origin".into()),
            branch: text(obj, "branch").unwrap_or_default(),
            upstream: upstream_from(obj),
            post_update: text(obj, "post_update"),
            drift: obj.get("drift").and_then(Value::as_bool).unwrap_or(false),
        }),
        "github-release" => Some(Source::GithubRelease {
            path: text(obj, "path")?,
            repo: text(obj, "repo"),
            current_version: text(obj, "current_version"),
            asset_pattern: text(obj, "asset_pattern"),
            verify_command: text(obj, "verify_command"),
        }),
        "npx" => Some(Source::Npx {
            package: text(obj, "package")?,
        }),
        "uvx" => Some(Source::Uvx {
            package: text(obj, "package")?,
        }),
        "remote" => Some(Source::Remote),
        "unknown" => Some(Source::Unknown {
            reason: text(obj, "reason").unwrap_or_default(),
        }),
        _ => None,
    }
}

pub fn stored(entry: &ServerEntry) -> Option<Source> {
    entry.unknown_fields.get(META_KEY).and_then(from_meta)
}

pub fn set_meta_fields(entry: &mut ServerEntry, fields: Map<String, Value>) {
    tags::object_mut(&mut entry.unknown_fields, &[META_KEY]).extend(fields);
}

pub fn expand_path(raw: &str, home: Option<&Path>) -> PathBuf {
    if raw == "~" {
        return home.map(Path::to_path_buf).unwrap_or_else(|| raw.into());
    }
    if let (Some(rest), Some(home)) = (raw.strip_prefix("~/"), home) {
        return home.join(rest.trim_start_matches('/'));
    }
    PathBuf::from(raw)
}

pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// `true` when `repo` is a linked worktree (`.git` is a file pointing at the main checkout's
/// `.git/worktrees/<name>`) rather than the primary checkout (`.git` is a directory). Detection
/// does not need to treat the two differently -- git resolves the file transparently for every
/// command run with `-C repo` -- but callers use this to explain a path in diagnostics.
pub fn is_worktree(repo: &Path) -> bool {
    repo.join(".git").is_file()
}

fn directory_arg(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--directory" {
            return iter.next().cloned();
        }
        if let Some(v) = arg.strip_prefix("--directory=") {
            return Some(v.to_string());
        }
    }
    None
}

/// The first positional (non-flag) argument, skipping both `--flag=value` and `--flag value`
/// forms -- e.g. `node --env-file=/x/.env /path/dist/index.js` yields the script path, not the
/// env file. A bare `--` stops option parsing (everything after is positional) but is itself
/// skipped.
fn first_path_like_arg(args: &[String]) -> Option<&str> {
    let mut rest = args.iter();
    let mut positional_only = false;
    while let Some(arg) = rest.next() {
        if positional_only {
            return Some(arg);
        }
        if arg == "--" {
            positional_only = true;
            continue;
        }
        if arg.starts_with("--") {
            continue; // `--flag=value` or a bare long flag; never eats a separate token here
        }
        if arg.starts_with('-') && arg.len() > 1 {
            // short flag that takes a separate value, e.g. `-r ./setup.js`
            if rest.clone().next().is_some_and(|n| !n.starts_with('-')) {
                rest.next();
            }
            continue;
        }
        return Some(arg);
    }
    None
}

fn git_source_for(path: &Path, git: &dyn GitRunner) -> Option<Source> {
    let root = find_git_root(path)?;
    Some(enrich_git(&root, git))
}

fn default_remote(remotes: &[String]) -> String {
    if remotes.iter().any(|r| r == "origin") {
        "origin".into()
    } else {
        remotes.first().cloned().unwrap_or_else(|| "origin".into())
    }
}

/// Which remote stands in for "upstream": a remote literally named `upstream`, else the first
/// remote the branch is not tracking. Only reported when its branch actually resolves to a
/// commit different from HEAD -- an identical ref is a mirror, not a fork relationship.
fn detect_upstream(
    git: &dyn GitRunner,
    root: &Path,
    remote: &str,
    branch: &str,
    remotes: &[String],
) -> Option<Upstream> {
    let candidates: Vec<&String> = remotes.iter().filter(|r| r.as_str() != remote).collect();
    let chosen = candidates
        .iter()
        .find(|r| r.as_str() == "upstream")
        .or_else(|| candidates.first())?;
    let chosen = (*chosen).clone();
    let cand_branch = if gitops::remote_branch_sha(git, root, &chosen, branch).is_some() {
        branch.to_string()
    } else {
        gitops::remote_default_branch(git, root, &chosen)?
    };
    let cand_sha = gitops::remote_branch_sha(git, root, &chosen, &cand_branch)?;
    let head_sha = gitops::head_sha(git, root)?;
    if cand_sha == head_sha {
        return None;
    }
    Some(Upstream {
        remote: chosen,
        branch: cand_branch,
    })
}

/// Fills a `Source::Git` from the live checkout: which remote and branch the server actually
/// runs from (not origin's default), and an upstream guess when a second remote exists.
fn enrich_git(root: &Path, git: &dyn GitRunner) -> Source {
    if !gitops::is_repo(git, root) {
        return Source::git_fallback(root);
    }
    let remotes = gitops::list_remotes(git, root);
    let (remote, branch) = gitops::tracking_upstream(git, root)
        .filter(|(r, _)| remotes.iter().any(|x| x == r))
        .or_else(|| {
            gitops::current_branch(git, root).map(|b| (default_remote(&remotes), b))
        })
        .unwrap_or_else(|| (default_remote(&remotes), String::new()));
    let upstream = if branch.is_empty() {
        None
    } else {
        detect_upstream(git, root, &remote, &branch, &remotes)
    };
    Source::Git {
        path: root.to_string_lossy().into_owned(),
        remote,
        branch,
        upstream,
        post_update: None,
        drift: false,
    }
}

fn looks_like_release_binary(path: &Path, home: Option<&Path>) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "python" | "python3" | "node" | "ruby" | "perl" | "bash" | "sh"
    ) {
        return false;
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if matches!(ext, "py" | "js" | "ts" | "rb" | "pl" | "sh" | "bat" | "ps1") {
        return false;
    }
    let Some(home) = home else { return false };
    [
        ".local/bin",
        "bin",
        ".config/mcpm/bin",
        ".claude/mcp-servers",
    ]
    .iter()
    .any(|dir| path.starts_with(home.join(dir)))
}

pub fn detect(entry: &ServerEntry, home: Option<&Path>, git: &dyn GitRunner) -> Source {
    if entry.transport != "stdio" || entry.url.is_some() && entry.command.is_none() {
        return Source::Remote;
    }
    let Some(command) = entry.command.as_deref().map(str::trim) else {
        return Source::Unknown {
            reason: "stdio server without a command".into(),
        };
    };
    let args = &entry.args;
    match command {
        "npx" | "npx.cmd" => {
            return match crate::plus::update::pins::parse_spec(args, false) {
                Some(spec) => Source::Npx { package: spec.name },
                None => Source::Unknown {
                    reason: "npx server but could not determine package name".into(),
                },
            }
        }
        "uvx" => {
            return match crate::plus::update::pins::parse_spec(args, true) {
                Some(spec) => Source::Uvx { package: spec.name },
                None => Source::Unknown {
                    reason: "uvx server but could not determine package name".into(),
                },
            }
        }
        "docker" | "docker-compose" | "docker.exe" => {
            return Source::Unknown {
                reason: "docker container; source tracking not implemented yet (MIG-UPD-8)".into(),
            }
        }
        _ => {}
    }
    if command == "uv" && args.first().map(String::as_str) == Some("run") {
        if let Some(dir) = directory_arg(args) {
            let dir = expand_path(&dir, home);
            return git_source_for(&dir, git).unwrap_or_else(|| Source::git_fallback(&dir));
        }
    }
    if let Some(cwd) = entry.cwd.as_deref() {
        let cwd = expand_path(cwd, home);
        if cwd.is_absolute() {
            if let Some(src) = git_source_for(&cwd, git) {
                return src;
            }
        }
    }
    let cmd_path = expand_path(command, home);
    if cmd_path.is_absolute() && cmd_path.exists() {
        if let Some(src) = git_source_for(&cmd_path, git) {
            return src;
        }
        if let Some(first) = first_path_like_arg(args) {
            let p = expand_path(first, home);
            if p.is_absolute() && p.exists() {
                if let Some(src) = git_source_for(&p, git) {
                    return src;
                }
            }
        }
        if looks_like_release_binary(&cmd_path, home) {
            return Source::GithubRelease {
                path: cmd_path.to_string_lossy().into_owned(),
                repo: None,
                current_version: None,
                asset_pattern: None,
                verify_command: None,
            };
        }
        return Source::Unknown {
            reason: "absolute path but no git repository found".into(),
        };
    }
    if command == "node" {
        if let Some(first) = first_path_like_arg(args) {
            let p = expand_path(first, home);
            if p.is_absolute() {
                if let Some(src) = git_source_for(&p, git) {
                    return src;
                }
                return Source::Unknown {
                    reason: "node server but no git repository found".into(),
                };
            }
        }
    }
    Source::Unknown {
        reason: "could not determine the source type".into(),
    }
}

/// Stored metadata wins; otherwise detect from the launch command. The flag
/// says whether the result came from stored metadata.
pub fn effective(entry: &ServerEntry, home: Option<&Path>, git: &dyn GitRunner) -> (Source, bool) {
    match stored(entry) {
        Some(s) => (s, true),
        None => (detect(entry, home, git), false),
    }
}

/// Re-runs detection for a server whose `args`/`cwd` may have changed and reports whether the
/// stored path still matches the launch directory. Only `drift` changes: a user's own `remote`,
/// `branch`, `upstream` or `post_update` edits are never overwritten by this (phase 2 wires this
/// into the server-edit path; for now it is a plain function nothing calls automatically).
pub fn recheck_drift(stored: &Source, entry: &ServerEntry, home: Option<&Path>, git: &dyn GitRunner) -> Source {
    let Source::Git {
        path,
        remote,
        branch,
        upstream,
        post_update,
        ..
    } = stored
    else {
        return stored.clone();
    };
    let detected_path = match detect(entry, home, git) {
        Source::Git { path, .. } => Some(path),
        _ => None,
    };
    let drift = match detected_path {
        Some(p) => expand_path(&p, home) != expand_path(path, home),
        None => false,
    };
    Source::Git {
        path: path.clone(),
        remote: remote.clone(),
        branch: branch.clone(),
        upstream: upstream.clone(),
        post_update: post_update.clone(),
        drift,
    }
}

pub fn suggest_post_update(repo: &Path) -> Option<String> {
    if repo.join("pyproject.toml").exists() {
        return Some("uv sync".into());
    }
    if let Ok(text) = std::fs::read_to_string(repo.join("package.json")) {
        let has_build = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get("scripts")?.get("build").cloned())
            .is_some();
        return Some(if has_build {
            "npm install && npm run build".into()
        } else {
            "npm install".into()
        });
    }
    if repo.join("package.json").exists() {
        return Some("npm install".into());
    }
    if repo.join("go.mod").exists() {
        return Some("go build ./...".into());
    }
    None
}
