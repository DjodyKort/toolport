//! What Claude Code keeps per repository: auto memory, instruction files git does not hold and the
//! session transcripts. The setup file reads them through [`MemorySource`].
//!
//! SEAM: the memory commands (`memory ls`, `sessions ls`) are built on another branch. When they
//! land, a `MemorySource` over their functions replaces [`FsMemory`]; nothing else in `setup`
//! reads `~/.claude/projects` directly.

use std::path::{Path, PathBuf};

use super::repos::{untilde, Repo};

/// Largest single file that goes into the setup file.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptFile {
    /// Path inside the memory folder (or, for an instruction file, inside the repository).
    pub name: String,
    pub bytes: Vec<u8>,
}

/// One folder of session transcripts, with the files that belong to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionFolder {
    /// Claude Code's name for the folder (the working directory with `-` for every other character).
    pub slug: String,
    pub dir: PathBuf,
    /// Transcripts (`<id>.jsonl`) and per-session folders, relative to `dir`.
    pub files: Vec<PathBuf>,
}

pub trait MemorySource {
    /// The files of the repository's auto memory folder.
    fn auto_memory(&self, repo: &Repo) -> Vec<KeptFile>;

    /// Instruction files in the repository that git does not hold.
    fn instruction_files(&self, repo: &Repo) -> Vec<KeptFile>;

    /// The transcript folders of the repository and its worktrees that exist.
    fn session_folders(&self, repo: &Repo) -> Vec<SessionFolder>;
}

/// Claude Code's folder name for a working directory.
pub fn slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub struct FsMemory {
    pub home: PathBuf,
    pub claude_home: PathBuf,
}

impl FsMemory {
    pub fn new(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            claude_home: crate::fsx::claude_dir_in(home),
        }
    }

    fn projects(&self) -> PathBuf {
        self.claude_home.join("projects")
    }

    fn candidates(&self, repo: &Repo) -> Vec<PathBuf> {
        let mut paths = vec![repo.abs.clone()];
        if let Ok(real) = std::fs::canonicalize(&repo.abs) {
            if real != repo.abs {
                paths.push(real);
            }
        }
        paths
    }

    /// The memory folder: `autoMemoryDirectory` of the repository's local, project or the user
    /// settings (in that order), else the folder under `projects/`.
    pub fn memory_dir(&self, repo_abs: &Path) -> PathBuf {
        let configured = [
            repo_abs.join(".claude/settings.local.json"),
            repo_abs.join(".claude/settings.json"),
            self.claude_home.join("settings.json"),
        ]
        .iter()
        .find_map(|file| {
            read_json(file)?
                .get("autoMemoryDirectory")?
                .as_str()
                .map(String::from)
        });
        match configured {
            Some(dir) if dir.starts_with("~/") => untilde(&self.home, &dir),
            Some(dir) if Path::new(&dir).is_absolute() => PathBuf::from(dir),
            _ => self.projects().join(slug(repo_abs)).join("memory"),
        }
    }

    pub fn project_dir(&self, path: &Path) -> PathBuf {
        self.projects().join(slug(path))
    }
}

fn collect(dir: &Path, base: &Path, out: &mut Vec<KeptFile>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect(&path, base, out);
        } else if kind.is_file() && entry.metadata().is_ok_and(|m| m.len() <= MAX_FILE_BYTES) {
            if let (Ok(bytes), Ok(rel)) = (std::fs::read(&path), path.strip_prefix(base)) {
                out.push(KeptFile {
                    name: rel.to_string_lossy().replace('\\', "/"),
                    bytes,
                });
            }
        }
    }
}

pub(super) const INSTRUCTION_PATHS: [&str; 4] = [
    "CLAUDE.local.md",
    "CLAUDE.md",
    "AGENTS.md",
    ".claude/CLAUDE.md",
];

fn tracked(repo: &Path) -> Option<std::collections::BTreeSet<String>> {
    let out = crate::skills::gitexec::run_command(
        crate::skills::gitexec::git_command(Some(repo), &["ls-files"]),
        std::time::Duration::from_secs(30),
    )
    .ok()?;
    out.ok()
        .then(|| out.stdout.lines().map(String::from).collect())
}

impl MemorySource for FsMemory {
    fn auto_memory(&self, repo: &Repo) -> Vec<KeptFile> {
        let mut files = Vec::new();
        let dir = self.memory_dir(&repo.abs);
        collect(&dir, &dir, &mut files);
        if files.is_empty() {
            for path in self.candidates(repo).iter().skip(1) {
                let dir = self.memory_dir(path);
                collect(&dir, &dir, &mut files);
            }
        }
        files
    }

    fn instruction_files(&self, repo: &Repo) -> Vec<KeptFile> {
        let Some(tracked) = tracked(&repo.abs) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let rules = repo.abs.join(".claude/rules");
        let mut rule_files = Vec::new();
        collect(&rules, &repo.abs, &mut rule_files);
        let mut candidates: Vec<KeptFile> = INSTRUCTION_PATHS
            .iter()
            .filter_map(|rel| {
                let bytes = std::fs::read(repo.abs.join(rel)).ok()?;
                Some(KeptFile {
                    name: (*rel).to_string(),
                    bytes,
                })
            })
            .collect();
        candidates.extend(rule_files.into_iter().filter(|f| f.name.ends_with(".md")));
        for file in candidates {
            if !tracked.contains(&file.name) && file.bytes.len() as u64 <= MAX_FILE_BYTES {
                found.push(file);
            }
        }
        found
    }

    fn session_folders(&self, repo: &Repo) -> Vec<SessionFolder> {
        let mut paths: Vec<PathBuf> = self.candidates(repo);
        for tree in &repo.worktrees {
            paths.push(untilde(&self.home, &tree.path));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut folders = Vec::new();
        for path in paths {
            let name = slug(&path);
            if !seen.insert(name.clone()) {
                continue;
            }
            let dir = self.projects().join(&name);
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut files: Vec<PathBuf> = read
                .flatten()
                .filter(|entry| {
                    let file = entry.file_name().to_string_lossy().into_owned();
                    file != "memory"
                        && (file.ends_with(".jsonl") || entry.file_type().is_ok_and(|t| t.is_dir()))
                })
                .map(|entry| PathBuf::from(entry.file_name()))
                .collect();
            files.sort();
            if !files.is_empty() {
                folders.push(SessionFolder {
                    slug: name,
                    dir,
                    files,
                });
            }
        }
        folders
    }
}

/// The size of everything under `path`.
pub fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| size_of(&e.path()))
        .sum()
}

/// Every file under `path`, relative to it.
pub fn files_under(path: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = read.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let p = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => walk(&p, base, out),
                Ok(t) if t.is_file() => {
                    if let Ok(rel) = p.strip_prefix(base) {
                        out.push(rel.to_path_buf());
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    if path.is_file() {
        if let Some(name) = path.file_name() {
            out.push(PathBuf::from(name));
        }
    } else {
        walk(path, path, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_names_a_folder_by_replacing_every_other_character() {
        assert_eq!(
            slug(Path::new("/Users/me/code/web.app")),
            "-Users-me-code-web-app"
        );
    }

    #[test]
    fn a_configured_memory_folder_wins_over_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let repo = home.join("code/app");
        std::fs::create_dir_all(repo.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let memory = FsMemory {
            home: home.clone(),
            claude_home: home.join(".claude"),
        };
        assert_eq!(
            memory.memory_dir(&repo),
            home.join(".claude/projects")
                .join(slug(&repo))
                .join("memory")
        );
        std::fs::write(
            home.join(".claude/settings.json"),
            r#"{"autoMemoryDirectory": "~/notes/user"}"#,
        )
        .unwrap();
        assert_eq!(memory.memory_dir(&repo), home.join("notes/user"));
        std::fs::write(
            repo.join(".claude/settings.local.json"),
            r#"{"autoMemoryDirectory": "/srv/mem"}"#,
        )
        .unwrap();
        assert_eq!(memory.memory_dir(&repo), PathBuf::from("/srv/mem"));
    }
}
