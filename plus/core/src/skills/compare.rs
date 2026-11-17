//! The content behind `skills diff <skill>`: the library copy of a file next to the installed
//! one, as a unified diff, for a file the next sync holds back (it differs or was edited by hand).

use super::api::{home, resolve_repo, Args};
use super::files::{skill_files, DIFFERS, HAND_EDITED};
use super::keep;
use super::parser::discover_skills;
use super::sync::installed_hash;
use super::textdiff::unified;
use super::transpilers::registry;
use crate::op::OpError;
use crate::skills::assets::rel_string;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const TEXT_LIMIT: u64 = 512 * 1024;
const BINARY_SNIFF: usize = 8000;

struct Side {
    present: bool,
    size: u64,
    bytes: Vec<u8>,
    truncated: bool,
}

impl Side {
    fn read(path: &Path) -> Self {
        let Ok(meta) = fs::metadata(path) else {
            return Self::missing();
        };
        let truncated = meta.len() > TEXT_LIMIT;
        let bytes = fs::File::open(path)
            .map(|file| {
                use std::io::Read;
                let mut bytes = Vec::new();
                let _ = file.take(TEXT_LIMIT).read_to_end(&mut bytes);
                bytes
            })
            .unwrap_or_default();
        Self {
            present: true,
            size: meta.len(),
            bytes,
            truncated,
        }
    }

    fn of_text(text: &str) -> Self {
        Self {
            present: true,
            size: text.len() as u64,
            bytes: text.as_bytes().to_vec(),
            truncated: false,
        }
    }

    fn missing() -> Self {
        Self {
            present: false,
            size: 0,
            bytes: Vec::new(),
            truncated: false,
        }
    }

    fn binary(&self) -> bool {
        let head = &self.bytes[..self.bytes.len().min(BINARY_SNIFF)];
        head.contains(&0) || (std::str::from_utf8(&self.bytes).is_err() && !self.truncated)
    }

    fn text(&self) -> Option<String> {
        (self.present && !self.binary()).then(|| String::from_utf8_lossy(&self.bytes).into_owned())
    }

    fn json(&self) -> Value {
        json!({"present": self.present, "size": self.size, "text": self.text()})
    }
}

struct Pick {
    client: String,
    path: String,
    status: String,
}

fn picks(files: &Value, file: Option<&str>) -> Vec<Pick> {
    files["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| match file {
            Some(wanted) => row["path"] == wanted,
            None => row["status"] == DIFFERS || row["status"] == HAND_EDITED,
        })
        .filter_map(|row| {
            Some(Pick {
                client: row["client"].as_str()?.to_string(),
                path: row["path"].as_str()?.to_string(),
                status: row["status"].as_str()?.to_string(),
            })
        })
        .collect()
}

/// `args.name` is the skill; `file` narrows the answer to one path inside the skill folder, of
/// any status, and is an error when the skill has no such file. Without it the answer lists
/// every file the next sync holds back because it differs or was edited after the last sync.
pub fn compare(args: &Args, file: Option<&str>) -> Result<Value, OpError> {
    let listing = skill_files(args)?;
    let repo = resolve_repo(args.repo.as_deref())?;
    let skills = discover_skills(&repo);
    let skill = skills
        .iter()
        .find(|skill| skill.name() == args.name)
        .ok_or_else(|| OpError::not_found(format!("skill not found: {}", args.name)))?;
    let root = if args.global { home()? } else { repo.clone() };
    let registry = registry();
    let chosen = picks(&listing, file);
    if let Some(wanted) = file {
        if chosen.is_empty() {
            return Err(OpError::not_found(format!(
                "{} has no file {wanted} to compare",
                args.name
            )));
        }
    }
    let mut rows = Vec::new();
    for pick in chosen {
        let dir = listing["installed"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row["client"] == pick.client.as_str())
            .and_then(|row| row["dir"].as_str())
            .map(PathBuf::from)
            .ok_or_else(|| OpError::not_found(format!("no install folder for {}", pick.client)))?;
        let installed_path = dir.join(&pick.path);
        let transpiled = registry
            .get(&pick.client)
            .and_then(|t| t.transpile(skill, &root).ok());
        let library = match &transpiled {
            Some(out)
                if out
                    .output_path
                    .strip_prefix(&dir)
                    .is_ok_and(|rel| rel_string(rel) == pick.path) =>
            {
                Side::of_text(&out.content)
            }
            _ => Side::read(&skill.source_dir().join(&pick.path)),
        };
        let installed = Side::read(&installed_path);
        let (diff, identical) = match (library.text(), installed.text()) {
            (Some(old), Some(new)) => {
                let diff = unified(&old, &new, "library", "installed");
                (Some(diff), old == new)
            }
            _ => (None, library.bytes == installed.bytes && library.present),
        };
        rows.push(json!({
            "path": pick.path,
            "client": pick.client,
            "status": pick.status,
            "installedPath": installed_path.to_string_lossy(),
            "binary": library.binary() || installed.binary(),
            "truncated": library.truncated || installed.truncated,
            "identical": identical,
            "library": library.json(),
            "installed": installed.json(),
            "diff": diff.as_ref().map(|d| d.text.clone()),
            "added": diff.as_ref().map_or(0, |d| d.added),
            "removed": diff.as_ref().map_or(0, |d| d.removed),
        }));
    }
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "skill": skill.name(),
        "type": skill.skill_type.as_str(),
        "lockfilePresent": listing["lockfilePresent"],
        "differing": rows.len(),
        "files": rows,
    }))
}

/// Records that the held files `files` of the skill stay as they are: the next sync stops
/// holding them while their content does not change. `dry_run` reports and writes nothing.
pub fn keep_files(args: &Args, files: &[String], dry_run: bool) -> Result<Value, OpError> {
    if files.is_empty() {
        return Err(OpError::usage("name the files to keep"));
    }
    let listing = skill_files(args)?;
    let held = picks(&listing, None);
    let mut chosen = Vec::new();
    for file in files {
        let rows: Vec<&Pick> = held.iter().filter(|pick| &pick.path == file).collect();
        if rows.is_empty() {
            return Err(OpError::failed(
                "refused",
                format!(
                    "{file} is not held back for {}: only a file that differs or was edited after the last sync can be kept",
                    args.name
                ),
            ));
        }
        chosen.extend(rows);
    }
    let base =
        crate::fsx::data_dir().ok_or_else(|| OpError::not_found("data directory unknown"))?;
    let mut kept = keep::load(&base);
    let mut rows = Vec::new();
    for pick in chosen {
        let dir = listing["installed"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row["client"] == pick.client.as_str())
            .and_then(|row| row["dir"].as_str())
            .map(PathBuf::from)
            .ok_or_else(|| OpError::not_found(format!("no install folder for {}", pick.client)))?;
        let hash = installed_hash(&dir.join(&pick.path))
            .ok_or_else(|| OpError::not_found(format!("{} is not installed", pick.path)))?;
        kept.insert(keep::key(&pick.client, &args.name, &pick.path), hash);
        rows.push(json!({"client": pick.client, "path": pick.path, "status": pick.status}));
    }
    if !dry_run {
        keep::save(&base, &kept)?;
    }
    Ok(json!({
        "skill": args.name,
        "dryRun": dry_run,
        "count": rows.len(),
        "kept": rows,
    }))
}
