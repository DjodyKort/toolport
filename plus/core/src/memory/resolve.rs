//! Which repository a folder belongs to, where its auto memory lives and who decides whether it
//! is on. Claude Code keys auto memory by the git repository (every worktree and subfolder share
//! one directory) and sessions by working folder, so everything here starts from the repository.

use crate::context::{fsx, gitlayout};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const MANAGED_SETTINGS: &str = "/Library/Application Support/ClaudeCode/managed-settings.json";
pub const ENV_DISABLE: &str = "CLAUDE_CODE_DISABLE_AUTO_MEMORY";
pub const SETTINGS_LOCAL: &str = ".claude/settings.local.json";
pub const SETTINGS_SHARED: &str = ".claude/settings.json";
const NAME_CAP: usize = 200;

/// Claude Code's folder name for a path: every character that is not a letter or digit becomes `-`.
pub fn project_slug(path: &Path) -> String {
    fsx::display(path)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The folders under `projects/` that carry `path`'s name. A name longer than 200 characters is
/// cut and given a hash by Claude Code; the hash is not reproduced here, so those are matched by
/// their first 200 characters.
pub fn project_dirs(projects: &Path, path: &Path) -> Vec<PathBuf> {
    let slug = project_slug(path);
    if slug.len() <= NAME_CAP {
        let dir = projects.join(&slug);
        return if dir.is_dir() { vec![dir] } else { Vec::new() };
    }
    let prefix = &slug[..NAME_CAP];
    fsx::list_dir(projects)
        .into_iter()
        .filter(|e| e.kind == fsx::Kind::Dir && e.name.starts_with(prefix))
        .map(|e| e.path)
        .collect()
}

#[derive(Clone, Debug)]
pub struct Repo {
    pub cwd: PathBuf,
    /// The main checkout, or the folder itself outside a git repository.
    pub root: PathBuf,
    pub git: bool,
    pub linked: bool,
    pub worktrees: Vec<PathBuf>,
}

impl Repo {
    pub fn of(cwd: &Path) -> Self {
        let cwd = fsx::canonical(cwd);
        match gitlayout::locate(&cwd) {
            Some(layout) => Self {
                root: layout.main_root(),
                git: true,
                linked: layout.linked,
                worktrees: layout.worktrees(),
                cwd,
            },
            None => Self {
                root: cwd.clone(),
                git: false,
                linked: false,
                worktrees: vec![cwd.clone()],
                cwd,
            },
        }
    }

    pub fn name(&self) -> String {
        self.root.file_name().map_or_else(
            || fsx::display(&self.root),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    pub fn slug(&self) -> String {
        project_slug(&self.root)
    }

    pub fn local_settings(&self) -> PathBuf {
        self.root.join(SETTINGS_LOCAL)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Keys {
    pub enabled: Option<bool>,
    pub dir: Option<String>,
    pub present: bool,
    pub unreadable: bool,
}

pub fn read_keys(path: &Path) -> Keys {
    let Some(text) = fsx::read_text(path, fsx::TEXT_CAP) else {
        return Keys::default();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Keys {
            enabled: map.get("autoMemoryEnabled").and_then(Value::as_bool),
            dir: map
                .get("autoMemoryDirectory")
                .and_then(Value::as_str)
                .map(String::from),
            present: true,
            unreadable: false,
        },
        _ => Keys {
            unreadable: true,
            ..Keys::default()
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetBy {
    Managed,
    Environment,
    Repository,
    Project,
    Global,
    Default,
}

impl SetBy {
    pub fn as_str(self) -> &'static str {
        match self {
            SetBy::Managed => "managed",
            SetBy::Environment => "environment",
            SetBy::Repository => "repository",
            SetBy::Project => "project",
            SetBy::Global => "global",
            SetBy::Default => "default",
        }
    }
}

/// The settings files that decide auto memory for a repository, read once.
#[derive(Clone, Debug, Default)]
pub struct Layers {
    pub managed: Keys,
    pub local_root: Keys,
    /// An older file left in the folder the session started in; the root's value wins over it.
    pub local_cwd: Keys,
    pub project_root: Keys,
    pub project_cwd: Keys,
    pub user: Keys,
    pub env: Option<String>,
    pub user_file: PathBuf,
}

impl Layers {
    pub fn load(home: &Path, repo: &Repo, managed: Option<&Path>, env: Option<String>) -> Self {
        let user_file = home.join(".claude/settings.json");
        let other = repo.cwd != repo.root;
        let beside = |rel: &str| {
            if other {
                read_keys(&repo.cwd.join(rel))
            } else {
                Keys::default()
            }
        };
        Self {
            managed: managed.map(read_keys).unwrap_or_default(),
            local_root: read_keys(&repo.root.join(SETTINGS_LOCAL)),
            local_cwd: beside(SETTINGS_LOCAL),
            project_root: read_keys(&repo.root.join(SETTINGS_SHARED)),
            project_cwd: beside(SETTINGS_SHARED),
            user: read_keys(&user_file),
            env,
            user_file,
        }
    }

    /// What the repository's local files say: the root file first, then an older subfolder file.
    pub fn switch(&self) -> Option<bool> {
        self.local_root.enabled.or(self.local_cwd.enabled)
    }

    pub fn effective(&self) -> (bool, SetBy) {
        if let Some(on) = self.managed.enabled {
            return (on, SetBy::Managed);
        }
        match self.env.as_deref().map(str::trim) {
            Some("1" | "true") => return (false, SetBy::Environment),
            Some("0" | "false") => return (true, SetBy::Environment),
            _ => {}
        }
        if let Some(on) = self.switch() {
            return (on, SetBy::Repository);
        }
        if let Some(on) = self.project_root.enabled.or(self.project_cwd.enabled) {
            return (on, SetBy::Project);
        }
        match self.user.enabled {
            Some(on) => (on, SetBy::Global),
            None => (true, SetBy::Default),
        }
    }

    /// The memory folder: `autoMemoryDirectory` from managed, local or user settings (a shared
    /// project file cannot point Toolport at another folder), else the default under `projects/`.
    pub fn directory(&self, home: &Path, repo: &Repo) -> (PathBuf, bool) {
        let custom = [&self.managed, &self.local_root, &self.local_cwd, &self.user]
            .iter()
            .find_map(|keys| keys.dir.as_deref())
            .and_then(|raw| {
                let path = match raw.strip_prefix("~/") {
                    Some(rest) => home.join(rest),
                    None => PathBuf::from(raw),
                };
                path.is_absolute().then_some(path)
            });
        match custom {
            Some(path) => (path, true),
            None => {
                let projects = home.join(".claude/projects");
                let dir = project_dirs(&projects, &repo.root)
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| projects.join(repo.slug()));
                (dir.join("memory"), false)
            }
        }
    }
}
