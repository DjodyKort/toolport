use super::bundle::PortableRoots;
use super::exec::Exec;
use super::schema::{ServerOrigin, ServerOrigins};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub fn detect_origins(
    servers_json: &Path,
    sources_json: &Path,
    exec: &dyn Exec,
    roots: &PortableRoots,
) -> ServerOrigins {
    let mut origins = ServerOrigins::default();
    let Ok(text) = fs::read_to_string(servers_json) else {
        return origins;
    };
    let Ok(Value::Object(servers)) = serde_json::from_str::<Value>(&text) else {
        return origins;
    };
    let sources: Value = crate::plus::jsonfs::read_json(sources_json).unwrap_or(Value::Null);
    for (name, config) in &servers {
        if config.is_object() {
            origins.servers.insert(
                name.clone(),
                detect_single(name, config, sources.get(name), exec, roots),
            );
        }
    }
    origins
}

fn detect_single(
    name: &str,
    config: &Value,
    source: Option<&Value>,
    exec: &dyn Exec,
    roots: &PortableRoots,
) -> ServerOrigin {
    let command = config.get("command").and_then(Value::as_str).unwrap_or("");
    let args: Vec<&str> = config
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if config.get("url").is_some() && config.get("command").is_none() {
        return ServerOrigin::plain(name, "remote");
    }
    if command == "npx" || command.ends_with("/npx") {
        return ServerOrigin::plain(name, "npx");
    }
    let Some(dir) = find_server_directory(command, &args) else {
        return ServerOrigin::plain(name, "manual");
    };
    let portable = roots.make_portable(&dir.to_string_lossy());
    let Some(git_url) = git_remote(exec, &dir) else {
        if let Some(src) = source {
            let repo = src.get("repo").and_then(Value::as_str).unwrap_or("");
            if src.get("type").and_then(Value::as_str) == Some("github-release") && !repo.is_empty()
            {
                let mut origin = ServerOrigin::plain(name, "github-release");
                origin.github_repo = Some(repo.to_string());
                origin.asset_pattern = src
                    .get("asset_pattern")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                origin.install_path = Some(portable);
                return origin;
            }
        }
        let mut origin = ServerOrigin::plain(name, "manual");
        origin.install_path = Some(portable);
        return origin;
    };
    let mut origin = ServerOrigin::plain(name, "git");
    origin.git_url = Some(git_url);
    origin.git_branch = Some(git_branch(exec, &dir));
    origin.install_path = Some(portable);
    origin.setup_command = detect_setup_command(&dir, command);
    origin
}

fn find_server_directory(command: &str, args: &[&str]) -> Option<PathBuf> {
    for (i, arg) in args.iter().enumerate() {
        if *arg == "--directory" && i + 1 < args.len() {
            let path = PathBuf::from(args[i + 1]);
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    let cmd = Path::new(command);
    if cmd.is_absolute() && cmd.exists() {
        let start = cmd.parent()?;
        return Some(repo_root(start).unwrap_or_else(|| start.to_path_buf()));
    }
    for arg in args {
        let path = Path::new(arg);
        if path.is_absolute() && path.exists() {
            let start = if path.is_file() { path.parent()? } else { path };
            return Some(repo_root(start).unwrap_or_else(|| start.to_path_buf()));
        }
    }
    None
}

fn repo_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|c| c.join(".git").exists())
        .map(Path::to_path_buf)
}

fn git_remote(exec: &dyn Exec, dir: &Path) -> Option<String> {
    let out = exec.git(Some(dir), &["remote", "get-url", "origin"]).ok()?;
    let url = out.trim();
    (!url.is_empty()).then(|| url.to_string())
}

fn git_branch(exec: &dyn Exec, dir: &Path) -> String {
    exec.git(Some(dir), &["branch", "--show-current"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "main".to_string())
}

fn detect_setup_command(dir: &Path, command: &str) -> Option<String> {
    if command == "uv" || command == "uvx" || command.ends_with("/uv") {
        return Some("uv sync".into());
    }
    if let Ok(pyproject) = fs::read_to_string(dir.join("pyproject.toml")) {
        return Some(
            if pyproject.contains("[tool.uv]") || pyproject.contains("uv") {
                "uv sync".into()
            } else {
                "pip install -e .".into()
            },
        );
    }
    if let Ok(text) = fs::read_to_string(dir.join("package.json")) {
        let has_build = serde_json::from_str::<Value>(&text)
            .ok()
            .map(|pkg| pkg.get("scripts").and_then(|s| s.get("build")).is_some())
            .unwrap_or(false);
        return Some(if has_build {
            "npm install && npm run build".into()
        } else {
            "npm install".into()
        });
    }
    if dir.join("go.mod").exists() {
        return Some("go build .".into());
    }
    if dir.join("requirements.txt").exists() {
        return Some("python -m venv .venv && .venv/bin/pip install -r requirements.txt".into());
    }
    None
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResolveResult {
    pub name: String,
    pub status: String,
    pub message: String,
}

fn result(name: &str, status: &str, message: impl Into<String>) -> ResolveResult {
    ResolveResult {
        name: name.to_string(),
        status: status.to_string(),
        message: message.into(),
    }
}

pub struct ResolveOptions {
    pub run_setup: bool,
}

pub fn resolve_servers(
    origins: &ServerOrigins,
    exec: &dyn Exec,
    roots: &PortableRoots,
    opts: &ResolveOptions,
) -> Vec<ResolveResult> {
    origins
        .servers
        .iter()
        .map(|(name, origin)| resolve_one(name, origin, exec, roots, opts))
        .collect()
}

fn install_path(origin: &ServerOrigin, roots: &PortableRoots) -> Result<Option<PathBuf>, String> {
    let Some(raw) = &origin.install_path else {
        return Ok(None);
    };
    let path = PathBuf::from(roots.resolve(raw));
    let home = PathBuf::from(roots.home.replace('\\', "/"));
    let escapes = path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir));
    if escapes || !path.starts_with(&home) {
        return Err(format!(
            "install path outside the home directory: {}",
            path.display()
        ));
    }
    Ok(Some(path))
}

fn resolve_one(
    name: &str,
    origin: &ServerOrigin,
    exec: &dyn Exec,
    roots: &PortableRoots,
    opts: &ResolveOptions,
) -> ResolveResult {
    match origin.origin_type.as_str() {
        "npx" | "remote" => result(
            name,
            "ready",
            format!("{} - no install needed", origin.origin_type),
        ),
        "github-release" => resolve_release(name, origin, roots),
        "manual" => match install_path(origin, roots) {
            Err(e) => result(name, "failed", e),
            Ok(Some(path)) if path.exists() => result(name, "ready", "local path exists"),
            Ok(Some(path)) => result(
                name,
                "manual",
                format!("path not found: {}", path.display()),
            ),
            Ok(None) => result(name, "manual", "no install metadata available"),
        },
        _ => resolve_git(name, origin, exec, roots, opts),
    }
}

fn resolve_release(name: &str, origin: &ServerOrigin, roots: &PortableRoots) -> ResolveResult {
    let Some(repo) = &origin.github_repo else {
        return result(name, "manual", "github-release but no repo configured");
    };
    match install_path(origin, roots) {
        Err(e) => result(name, "failed", e),
        Ok(Some(path)) if path.exists() => result(
            name,
            "ready",
            format!("github-release - binary exists at {}", path.display()),
        ),
        _ => result(
            name,
            "manual",
            format!("github-release {repo} - install the binary manually"),
        ),
    }
}

fn resolve_git(
    name: &str,
    origin: &ServerOrigin,
    exec: &dyn Exec,
    roots: &PortableRoots,
    opts: &ResolveOptions,
) -> ResolveResult {
    let path = match install_path(origin, roots) {
        Err(e) => return result(name, "failed", e),
        Ok(None) => return result(name, "manual", "missing git_url or install_path"),
        Ok(Some(path)) => path,
    };
    let Some(url) = origin
        .git_url
        .as_deref()
        .filter(|u| !u.is_empty() && !u.starts_with('-'))
    else {
        return result(name, "manual", "missing git_url or install_path");
    };
    if path.join(".git").exists() {
        return update_repo(name, &path, origin, exec, opts);
    }
    if path.exists() {
        return result(
            name,
            "manual",
            format!("path exists but is not a git repo: {}", path.display()),
        );
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let branch = origin.git_branch.as_deref().unwrap_or("main");
    if branch.starts_with('-') {
        return result(name, "failed", "refusing git branch");
    }
    let dest = path.to_string_lossy().into_owned();
    if let Err(e) = exec.git(None, &["clone", "--branch", branch, "--", url, &dest]) {
        return result(name, "failed", e);
    }
    if let Some(failed) = run_setup(name, &path, origin, exec, opts) {
        return failed;
    }
    result(
        name,
        "cloned",
        format!("cloned from {url}{}", setup_note(origin, opts)),
    )
}

fn update_repo(
    name: &str,
    path: &Path,
    origin: &ServerOrigin,
    exec: &dyn Exec,
    opts: &ResolveOptions,
) -> ResolveResult {
    let out = match exec.git(Some(path), &["pull", "--ff-only"]) {
        Ok(out) => out,
        Err(_) => {
            return result(
                name,
                "ready",
                "git pull skipped (local changes or diverged)",
            )
        }
    };
    if out.contains("Already up to date") {
        return result(name, "ready", "already up to date");
    }
    if let Some(failed) = run_setup(name, path, origin, exec, opts) {
        return failed;
    }
    result(
        name,
        "updated",
        format!("pulled latest{}", setup_note(origin, opts)),
    )
}

fn setup_note(origin: &ServerOrigin, opts: &ResolveOptions) -> String {
    match (&origin.setup_command, opts.run_setup) {
        (Some(cmd), false) => format!("; setup not run: {cmd}"),
        _ => String::new(),
    }
}

fn run_setup(
    name: &str,
    dir: &Path,
    origin: &ServerOrigin,
    exec: &dyn Exec,
    opts: &ResolveOptions,
) -> Option<ResolveResult> {
    let command = origin.setup_command.as_deref()?;
    if !opts.run_setup {
        return None;
    }
    exec.shell(dir, command)
        .err()
        .map(|e| result(name, "failed", format!("setup failed: {e}")))
}
