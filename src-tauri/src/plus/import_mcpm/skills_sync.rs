//! mcpm's `skills_sync.json`: the git-sync clone that `skills ls` and the self-MCP skills tools
//! fall back to when the working directory is not a skills repository.

use super::run::{Action, Change};
use super::Warning;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const FILE: &str = "skills_sync.json";

/// Set on the copy so a later import may update it; a file Toolport or the user rewrote has no
/// marker and is left alone.
const MARKER: &str = "imported_from";

#[derive(Debug, Default)]
pub struct Carry {
    pub change: Option<Change>,
    pub write: Option<(PathBuf, String)>,
    pub warnings: Vec<Warning>,
}

enum Repo {
    Keep,
    Moved(PathBuf),
    Missing,
}

fn warning(kind: &str, detail: String) -> Warning {
    Warning {
        server: String::new(),
        kind: kind.into(),
        detail,
    }
}

fn expand(local: &str, home: &str) -> PathBuf {
    match local.strip_prefix("~/") {
        Some(rest) => Path::new(home.trim_end_matches('/')).join(rest),
        None => PathBuf::from(local),
    }
}

/// The clone stays where mcpm put it while it is there. Once it is gone, it is looked for in the
/// data dir under the same path relative to the mcpm root, then under its own name.
fn locate(local: &str, root: &Path, data_dir: &Path, home: &str) -> Repo {
    let path = expand(local, home);
    if path.exists() {
        return Repo::Keep;
    }
    let mut candidates = Vec::new();
    if let Ok(rel) = path.strip_prefix(root) {
        candidates.push(data_dir.join(rel));
    }
    if let Some(name) = path.file_name() {
        candidates.push(data_dir.join(name));
    }
    match candidates.into_iter().find(|c| c.is_dir()) {
        Some(found) => Repo::Moved(found),
        None => Repo::Missing,
    }
}

pub fn plan(root: &Path, data_dir: &Path, home: &str) -> Carry {
    let source = root.join(FILE);
    let text = match std::fs::read_to_string(&source) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Carry::default(),
        Err(e) => return unreadable(&source, &e.to_string()),
    };
    let Ok(Value::Object(mut doc)) = serde_json::from_str::<Value>(&text) else {
        return unreadable(&source, "not a JSON object");
    };
    let mut warnings = Vec::new();
    if let Some(local) = doc
        .get("local_path")
        .and_then(Value::as_str)
        .map(String::from)
    {
        match locate(&local, root, data_dir, home) {
            Repo::Keep if local.starts_with("~/") => {
                let path = expand(&local, home);
                doc.insert("local_path".into(), json!(path.to_string_lossy()));
            }
            Repo::Keep => {}
            Repo::Moved(to) => {
                warnings.push(warning(
                    "skills-repo-moved",
                    format!("{local} -> {}", to.display()),
                ));
                doc.insert("local_path".into(), json!(to.to_string_lossy()));
            }
            Repo::Missing => warnings.push(warning("skills-repo-missing", local)),
        }
    }
    doc.insert(MARKER.into(), json!("mcpm"));
    let want = Value::Object(doc);
    let target = data_dir.join(FILE);
    let action = match std::fs::read_to_string(&target) {
        Err(_) => Action::Created,
        Ok(old) => match serde_json::from_str::<Value>(&old) {
            Ok(have) if have == want => Action::Unchanged,
            Ok(have) if have.get(MARKER) == Some(&json!("mcpm")) => Action::Updated,
            _ => Action::Conflict,
        },
    };
    let write = matches!(action, Action::Created | Action::Updated).then(|| {
        (
            target,
            serde_json::to_string_pretty(&want).unwrap_or_default(),
        )
    });
    Carry {
        change: Some(Change {
            id: FILE.into(),
            action,
        }),
        write,
        warnings,
    }
}

fn unreadable(source: &Path, why: &str) -> Carry {
    Carry {
        warnings: vec![warning(
            "skills-sync-unreadable",
            format!("{}: {why}", source.display()),
        )],
        ..Carry::default()
    }
}
