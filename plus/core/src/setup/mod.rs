//! Setup and backup (D-144, D-145): one zip with a manifest holds the parts you tick, secrets and
//! sessions only sealed with a passphrase; a restore is always a plan first; a checklist says what a
//! wipe would still cost. `toolportctl setup ...` and the `setup_*` self-management tools are thin
//! renderers over this module.

pub mod check;
pub mod crypto;
pub mod export;
pub mod inspect;
pub mod mem;
pub mod parts;
pub mod repo_heads;
pub mod repos;
pub mod restore;
pub mod safe_path;
pub mod scan;
pub mod scratch;
pub mod threeway;
pub mod zipio;

#[cfg(test)]
mod export_safety_tests;
#[cfg(test)]
mod restore_safety_tests;
#[cfg(test)]
mod restore_tests;
#[cfg(test)]
pub(crate) mod tests;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use toolport_plus_api::Host;

pub const FORMAT: &str = "toolport-setup";
pub const VERSION: u32 = 1;
pub const MANIFEST: &str = "manifest.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PartId {
    Repos,
    Servers,
    Access,
    Library,
    Tasks,
    Settings,
    ClaudeUser,
    Memory,
    History,
    Secrets,
    ServerRepos,
}

pub const ALL_PARTS: [PartId; 11] = [
    PartId::Repos,
    PartId::Servers,
    PartId::Access,
    PartId::Library,
    PartId::Tasks,
    PartId::Settings,
    PartId::ClaudeUser,
    PartId::Memory,
    PartId::History,
    PartId::Secrets,
    PartId::ServerRepos,
];

impl PartId {
    pub const fn id(self) -> &'static str {
        match self {
            PartId::Repos => "repos",
            PartId::Servers => "servers",
            PartId::Access => "access",
            PartId::Library => "library",
            PartId::Tasks => "tasks",
            PartId::Settings => "settings",
            PartId::ClaudeUser => "claudeUser",
            PartId::Memory => "memory",
            PartId::History => "history",
            PartId::Secrets => "secrets",
            PartId::ServerRepos => "serverRepos",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            PartId::Repos => "Repositories",
            PartId::Servers => "Servers",
            PartId::Access => "Clients and access sets",
            PartId::Library => "Skills library",
            PartId::Tasks => "Tasks",
            PartId::Settings => "Toolport settings",
            PartId::ClaudeUser => "Claude Code user files",
            PartId::Memory => "Memory",
            PartId::History => "Sessions",
            PartId::Secrets => "Secrets",
            PartId::ServerRepos => "Server repositories",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = if text == "server-repos" {
            "serverRepos"
        } else {
            text
        };
        ALL_PARTS.into_iter().find(|p| p.id() == text)
    }

    /// Travels only through sync: it describes this machine's checkouts for the others and has
    /// nothing to restore, so no export lists or includes it.
    pub const fn sync_only(self) -> bool {
        matches!(self, PartId::ServerRepos)
    }

    /// Sealed parts: the passphrase decides whether anyone can read them.
    pub const fn sealed(self) -> bool {
        matches!(self, PartId::History | PartId::Secrets)
    }

    /// What `--parts all` includes: everything that is readable. Sessions and secrets are asked for.
    pub const fn in_all(self) -> bool {
        !self.sealed() && !self.sync_only()
    }
}

/// `all`, or a comma separated list of part ids (`claudeUser` may also be written `claude-user`).
pub fn parse_parts(text: &str) -> Result<BTreeSet<PartId>, String> {
    let mut parts = BTreeSet::new();
    for word in text.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        if word == "all" {
            parts.extend(ALL_PARTS.into_iter().filter(|p| p.in_all()));
            continue;
        }
        let word = if word == "claude-user" {
            "claudeUser"
        } else {
            word
        };
        parts.insert(PartId::parse(word).ok_or_else(|| {
            format!(
                "unknown part {word:?}: use all or any of {}",
                ALL_PARTS
                    .into_iter()
                    .filter(|p| !p.sync_only())
                    .map(PartId::id)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?);
    }
    if parts.is_empty() {
        return Err("no part named: use --parts all or a list of part ids".into());
    }
    Ok(parts)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The hash of a file, read in pieces: a setup file can hold gigabytes of sessions.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = match file.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if n == 0 {
            return Ok(hex(&hasher.finalize()));
        }
        hasher.update(&buf[..n]);
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// What a command runs against.
pub struct Ctx<'a> {
    pub host: &'a dyn Host,
    pub home: PathBuf,
    pub data_dir: PathBuf,
    pub claude_home: PathBuf,
}

impl<'a> Ctx<'a> {
    pub fn new(host: &'a dyn Host) -> Result<Self, String> {
        let home = crate::fsx::home().ok_or("home directory could not be resolved")?;
        let data_dir = host
            .data_dir()
            .ok_or("the data directory could not be resolved")?;
        let claude_home = mem::FsMemory::new(&home).claude_home;
        Ok(Self {
            host,
            home,
            data_dir,
            claude_home,
        })
    }

    pub fn memory(&self) -> mem::FsMemory {
        mem::FsMemory {
            home: self.home.clone(),
            claude_home: self.claude_home.clone(),
        }
    }

    /// Where setup keeps its own records: the last export, the runs a restore can undo.
    pub fn state_dir(&self) -> PathBuf {
        self.data_dir.join("plus").join("setup")
    }

    pub fn tilde(&self, path: &Path) -> String {
        repos::tilde(&self.home, path)
    }

    pub fn untilde(&self, text: &str) -> PathBuf {
        repos::untilde(&self.home, text)
    }
}

/// The folders searched for repositories: the ones named, else those of `sources root ls`, else
/// the usual places for checkouts that exist.
pub fn search_roots(ctx: &Ctx<'_>, named: &[PathBuf]) -> Vec<PathBuf> {
    if !named.is_empty() {
        return named.to_vec();
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some((places_roots, places, config)) = crate::sources::host_world() {
        let listed = crate::sources::source_roots::list(&places_roots, &places, &config);
        for row in listed["roots"].as_array().into_iter().flatten() {
            if let (Some(path), true) = (row["path"].as_str(), row["exists"] == true) {
                roots.push(PathBuf::from(path));
            }
        }
    }
    if repos::find(&roots).is_empty() {
        for usual in [
            "Documents/GitHub",
            "code",
            "src",
            "Developer",
            "projects",
            "dev",
        ] {
            let dir = ctx.home.join(usual);
            if dir.is_dir() {
                roots.push(dir);
            }
        }
    }
    roots.dedup();
    roots
}
