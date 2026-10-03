use crate::registry::ServerEntry;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const META_KEY: &str = "mcpmSource";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Git {
        path: String,
        remote_url: Option<String>,
        branch: Option<String>,
        post_update: Option<String>,
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

    pub fn to_meta(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("type".into(), json!(self.kind()));
        let mut put = |k: &str, v: &Option<String>| {
            if let Some(v) = v {
                m.insert(k.into(), json!(v));
            }
        };
        match self {
            Source::Git {
                path,
                remote_url,
                branch,
                post_update,
            } => {
                put("remote_url", remote_url);
                put("branch", branch);
                put("post_update", post_update);
                m.insert("path".into(), json!(path));
            }
            Source::GithubRelease {
                path,
                repo,
                current_version,
                asset_pattern,
                verify_command,
            } => {
                put("repo", repo);
                put("current_version", current_version);
                put("asset_pattern", asset_pattern);
                put("verify_command", verify_command);
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

pub fn from_meta(meta: &Value) -> Option<Source> {
    let obj = meta.as_object()?;
    match obj.get("type")?.as_str()? {
        "git" => Some(Source::Git {
            path: text(obj, "path")?,
            remote_url: text(obj, "remote_url"),
            branch: text(obj, "branch"),
            post_update: text(obj, "post_update"),
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
    let slot = entry
        .unknown_fields
        .entry(META_KEY.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !slot.is_object() {
        *slot = Value::Object(Map::new());
    }
    if let Some(obj) = slot.as_object_mut() {
        for (k, v) in fields {
            obj.insert(k, v);
        }
    }
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

fn git_source_for(path: &Path) -> Option<Source> {
    find_git_root(path).map(|root| Source::Git {
        path: root.to_string_lossy().into_owned(),
        remote_url: None,
        branch: None,
        post_update: None,
    })
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

pub fn detect(entry: &ServerEntry, home: Option<&Path>) -> Source {
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
        _ => {}
    }
    if command == "uv" && args.first().map(String::as_str) == Some("run") {
        if let Some(dir) = directory_arg(args) {
            let dir = expand_path(&dir, home);
            return git_source_for(&dir).unwrap_or(Source::Git {
                path: dir.to_string_lossy().into_owned(),
                remote_url: None,
                branch: None,
                post_update: None,
            });
        }
    }
    if let Some(cwd) = entry.cwd.as_deref() {
        let cwd = expand_path(cwd, home);
        if cwd.is_absolute() {
            if let Some(src) = git_source_for(&cwd) {
                return src;
            }
        }
    }
    let cmd_path = expand_path(command, home);
    if cmd_path.is_absolute() && cmd_path.exists() {
        if let Some(src) = git_source_for(&cmd_path) {
            return src;
        }
        if let Some(first) = args.first() {
            let p = expand_path(first, home);
            if p.is_absolute() && p.exists() {
                if let Some(src) = git_source_for(&p) {
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
        if let Some(first) = args.first() {
            let p = expand_path(first, home);
            if p.is_absolute() {
                if let Some(src) = git_source_for(&p) {
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
pub fn effective(entry: &ServerEntry, home: Option<&Path>) -> (Source, bool) {
    match stored(entry) {
        Some(s) => (s, true),
        None => (detect(entry, home), false),
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
