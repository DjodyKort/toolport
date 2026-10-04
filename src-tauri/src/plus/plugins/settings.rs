//! The Claude Code settings files that decide which plugins are on and which hooks and MCP denials
//! apply: user, project and local settings of a folder (and of its git root), and the managed
//! file. Read-only; a missing file is skipped, an unparseable one is kept with its problem.

use crate::plus::sources::fsx;
use serde_json::Value;
use std::path::{Path, PathBuf};

const CAP: usize = 4 * 1024 * 1024;
const WALK_LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    User,
    Project,
    Local,
    Managed,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Project => "project",
            Scope::Local => "local",
            Scope::Managed => "managed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SettingsFile {
    pub scope: Scope,
    pub name: &'static str,
    pub path: PathBuf,
    pub doc: Option<Value>,
    pub problem: Option<String>,
}

impl SettingsFile {
    fn read(scope: Scope, name: &'static str, path: PathBuf) -> Option<Self> {
        if !fsx::is_file(&path) {
            return None;
        }
        let text = fsx::read_text(&path, CAP).unwrap_or_default();
        let (doc, problem) = match serde_json::from_str::<Value>(&text) {
            Ok(doc) if doc.is_object() => (Some(doc), None),
            Ok(_) => (None, Some("is not a JSON object".to_string())),
            Err(e) => (None, Some(format!("is not valid JSON ({e})"))),
        };
        Some(Self {
            scope,
            name,
            path,
            doc,
            problem,
        })
    }

    fn get(&self, key: &str) -> Option<&Value> {
        self.doc.as_ref()?.get(key)
    }
}

/// Where a plugin is switched on or off, per layer; `effective` is what Claude Code ends up doing
/// (managed over local over project over user; absent everywhere means off).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Enabled {
    pub user: Option<bool>,
    pub project: Option<bool>,
    pub local: Option<bool>,
    pub managed: Option<bool>,
    pub effective: bool,
}

impl Enabled {
    /// The layer that decided, for human text.
    pub fn layer(&self) -> &'static str {
        if self.managed.is_some() {
            "managed"
        } else if self.local.is_some() {
            "project-local"
        } else if self.project.is_some() {
            "project"
        } else if self.user.is_some() {
            "user"
        } else {
            "no settings file"
        }
    }
}

/// What `disableAllHooks` does to a folder: a managed setting turns every hook off, any other
/// layer turns off everything but the managed hooks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HooksOff {
    pub managed_too: bool,
    pub set_in: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Denied {
    pub user: bool,
    pub project: bool,
    pub local: bool,
    pub managed: bool,
}

impl Denied {
    pub fn any(&self) -> bool {
        self.user || self.project || self.local || self.managed
    }
}

#[derive(Clone, Debug)]
pub struct Layers {
    files: Vec<SettingsFile>,
}

/// The git root above `cwd` (or `cwd` itself) found by looking for `.git`; never looks at or
/// above `stop`, which is the home directory.
pub fn git_root(cwd: &Path, stop: Option<&Path>) -> Option<PathBuf> {
    for dir in cwd.ancestors().take(WALK_LIMIT) {
        if stop.is_some_and(|stop| dir == stop) {
            return None;
        }
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

/// The folders whose `.claude` settings apply when Claude Code starts in `cwd`: its git root when
/// that is above it, then `cwd` itself. Folders above the git root are never merged.
pub fn project_dirs(cwd: &Path, stop: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = git_root(cwd, stop).filter(|root| root != cwd) {
        dirs.push(root);
    }
    dirs.push(cwd.to_path_buf());
    dirs
}

impl Layers {
    pub fn load(
        claude_home: &Path,
        cwd: Option<&Path>,
        managed: Option<&Path>,
        stop: Option<&Path>,
    ) -> Self {
        let mut files: Vec<SettingsFile> = Vec::new();
        let user = [
            ("settings.json", claude_home.join("settings.json")),
            ("settings.local.json", claude_home.join("settings.local.json")),
        ];
        for (name, path) in &user {
            files.extend(SettingsFile::read(Scope::User, name, path.clone()));
        }
        for dir in cwd.map(|cwd| project_dirs(cwd, stop)).unwrap_or_default() {
            for (scope, name) in [
                (Scope::Project, "settings.json"),
                (Scope::Local, "settings.local.json"),
            ] {
                let path = dir.join(".claude").join(name);
                if files.iter().any(|f| fsx::canonical(&f.path) == fsx::canonical(&path)) {
                    continue;
                }
                files.extend(SettingsFile::read(scope, name, path));
            }
        }
        if let Some(path) = managed {
            files.extend(SettingsFile::read(Scope::Managed, "managed-settings.json", path.to_path_buf()));
        }
        Self { files }
    }

    pub fn files(&self) -> &[SettingsFile] {
        &self.files
    }

    fn last_bool(&self, scope: Scope, name: &str, id: &str) -> Option<bool> {
        self.files
            .iter()
            .filter(|f| f.scope == scope && (scope == Scope::Managed || f.name == name))
            .filter_map(|f| f.get("enabledPlugins")?.get(id)?.as_bool())
            .last()
    }

    pub fn enabled(&self, id: &str) -> Enabled {
        let user = self.last_bool(Scope::User, "settings.json", id);
        let project = self.last_bool(Scope::Project, "settings.json", id);
        let local = self.last_bool(Scope::Local, "settings.local.json", id);
        let managed = self.last_bool(Scope::Managed, "", id);
        Enabled {
            user,
            project,
            local,
            managed,
            effective: managed.or(local).or(project).or(user).unwrap_or(false),
        }
    }

    pub fn disable_all_hooks(&self) -> Option<HooksOff> {
        let flag = |f: &SettingsFile| f.get("disableAllHooks").and_then(Value::as_bool);
        if let Some(file) = self
            .files
            .iter()
            .filter(|f| f.scope == Scope::Managed)
            .find(|f| flag(f) == Some(true))
        {
            return Some(HooksOff {
                managed_too: true,
                set_in: file.path.clone(),
            });
        }
        let decided = [Scope::Local, Scope::Project, Scope::User]
            .iter()
            .find_map(|scope| {
                self.files
                    .iter()
                    .rev()
                    .filter(|f| f.scope == *scope)
                    .find_map(|f| flag(f).map(|on| (on, f)))
            })?;
        decided.0.then(|| HooksOff {
            managed_too: false,
            set_in: decided.1.path.clone(),
        })
    }

    /// Which layers deny a plugin MCP server through `deniedMcpServers`: by `serverName` (the
    /// `plugin:<plugin>:<server>` key), by `serverCommand` or by `serverUrl`.
    pub fn denied(&self, key: &str, command: Option<&[String]>, url: Option<&str>) -> Denied {
        let mut out = Denied::default();
        for file in &self.files {
            let Some(rules) = file.get("deniedMcpServers").and_then(Value::as_array) else {
                continue;
            };
            let hit = rules.iter().any(|rule| {
                rule.get("serverName").and_then(Value::as_str) == Some(key)
                    || match (rule.get("serverCommand").and_then(Value::as_array), command) {
                        (Some(rule), Some(command)) => {
                            rule.len() == command.len()
                                && rule.iter().zip(command).all(|(a, b)| a.as_str() == Some(b))
                        }
                        _ => false,
                    }
                    || match (rule.get("serverUrl").and_then(Value::as_str), url) {
                        (Some(rule), Some(url)) => rule == url,
                        _ => false,
                    }
            });
            if hit {
                match file.scope {
                    Scope::User => out.user = true,
                    Scope::Project => out.project = true,
                    Scope::Local => out.local = true,
                    Scope::Managed => out.managed = true,
                }
            }
        }
        out
    }

    pub fn problems(&self) -> Vec<String> {
        self.files
            .iter()
            .filter_map(|f| {
                f.problem
                    .as_ref()
                    .map(|p| format!("{} {p}; Claude Code ignores it", f.path.display()))
            })
            .collect()
    }
}
