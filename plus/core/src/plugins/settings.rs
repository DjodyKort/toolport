//! The Claude Code settings files that decide which plugins are on: user, project and local
//! settings of a folder (and of its git root), and the managed file. Read-only; a missing file is
//! skipped, an unparseable one is kept with its problem.

use crate::context::fsx;
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

#[derive(Clone, Debug)]
pub struct Layers {
    files: Vec<SettingsFile>,
}

/// The git root above `cwd` (or `cwd` itself) found by looking for `.git`; never looks at or
/// above `stop`, which is the home directory.
fn git_root(cwd: &Path, stop: Option<&Path>) -> Option<PathBuf> {
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
fn project_dirs(cwd: &Path, stop: Option<&Path>) -> Vec<PathBuf> {
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
            (
                "settings.local.json",
                claude_home.join("settings.local.json"),
            ),
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
                if files
                    .iter()
                    .any(|f| fsx::canonical(&f.path) == fsx::canonical(&path))
                {
                    continue;
                }
                files.extend(SettingsFile::read(scope, name, path));
            }
        }
        if let Some(path) = managed {
            files.extend(SettingsFile::read(
                Scope::Managed,
                "managed-settings.json",
                path.to_path_buf(),
            ));
        }
        Self { files }
    }

    fn last_bool(&self, scope: Scope, name: &str, id: &str) -> Option<bool> {
        self.files
            .iter()
            .filter(|f| f.scope == scope && (scope == Scope::Managed || f.name == name))
            .filter_map(|f| f.get("enabledPlugins")?.get(id)?.as_bool())
            .next_back()
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
}
