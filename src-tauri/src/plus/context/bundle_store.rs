//! Where bundles live: `profiles/<name>.yaml` in the skills repository. Toolport only writes the
//! file; committing it is the user's.

use super::bundle::{self, Bundle, Edit};
use super::roots::Roots;
use crate::plus::sources::fsx;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleError {
    pub code: &'static str,
    pub message: String,
}

impl BundleError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub fn dir(roots: &Roots) -> PathBuf {
    roots.skills_repo_path().join("profiles")
}

pub fn valid_name(name: &str) -> Result<(), BundleError> {
    let plain = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if plain {
        Ok(())
    } else {
        Err(BundleError::new(
            "usage",
            format!("not a bundle name: {name} (letters, digits, - _ . and at most 64 characters)"),
        ))
    }
}

pub fn file(roots: &Roots, name: &str) -> Result<PathBuf, BundleError> {
    valid_name(name)?;
    Ok(dir(roots).join(format!("{name}.yaml")))
}

#[derive(Debug)]
pub struct Loaded {
    pub bundle: Bundle,
    pub text: String,
    pub path: PathBuf,
}

pub fn load(roots: &Roots, name: &str) -> Result<Loaded, BundleError> {
    let path = file(roots, name)?;
    if !fsx::is_file(&path) {
        return Err(BundleError::new(
            "not_found",
            format!("unknown bundle: {name} (no {})", path.display()),
        ));
    }
    let text = fs::read_to_string(&path)
        .map_err(|e| BundleError::new("unreadable", format!("{}: {e}", path.display())))?;
    let bundle = bundle::parse(name, &text)
        .map_err(|e| BundleError::new("invalid", format!("bundle {name}: {e}")))?;
    Ok(Loaded { bundle, text, path })
}

pub struct Listed {
    pub name: String,
    pub path: PathBuf,
    pub text: String,
    pub parsed: Result<Bundle, String>,
}

/// Every `*.yaml` in the profiles directory, by name; one that does not parse stays in the list.
pub fn list(roots: &Roots) -> Vec<Listed> {
    let mut out = Vec::new();
    for entry in fsx::list_dir(&dir(roots)) {
        let Some(name) = entry.name.strip_suffix(".yaml") else {
            continue;
        };
        if entry.kind != fsx::Kind::File || valid_name(name).is_err() {
            continue;
        }
        let text = fs::read_to_string(&entry.path).unwrap_or_default();
        out.push(Listed {
            name: name.to_string(),
            parsed: bundle::parse(name, &text),
            path: entry.path,
            text,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn write(path: &Path, text: &str) -> Result<(), BundleError> {
    super::bundle_io::write_atomic(path, text.as_bytes())
        .map_err(|e| BundleError::new("write_failed", format!("{}: {e}", path.display())))
}

#[derive(Debug)]
pub struct Written {
    pub path: PathBuf,
    pub before: Option<String>,
    pub after: String,
}

/// `add` creates and refuses an existing name; `edit` changes an existing file and keeps its
/// unknown keys. With `dry_run` nothing is written.
pub fn save(
    roots: &Roots,
    name: &str,
    edit: &Edit,
    create: bool,
    dry_run: bool,
) -> Result<Written, BundleError> {
    let path = file(roots, name)?;
    let existing = fsx::is_file(&path);
    let (before, after) = match (create, existing) {
        (true, true) => {
            return Err(BundleError::new(
                "exists",
                format!("bundle {name} exists already ({}); use `context bundle edit`", path.display()),
            ))
        }
        (true, false) => (None, bundle::create(name, edit)),
        (false, false) => {
            return Err(BundleError::new("not_found", format!("unknown bundle: {name}")))
        }
        (false, true) => {
            let text = fs::read_to_string(&path)
                .map_err(|e| BundleError::new("unreadable", format!("{}: {e}", path.display())))?;
            let after = bundle::edited(name, &text, edit)
                .map_err(|e| BundleError::new("invalid", format!("bundle {name}: {e}")))?;
            (Some(text), after)
        }
    };
    if !dry_run {
        write(&path, &after)?;
    }
    Ok(Written { path, before, after })
}

pub fn remove(roots: &Roots, name: &str, dry_run: bool) -> Result<PathBuf, BundleError> {
    let path = file(roots, name)?;
    if !fsx::is_file(&path) {
        return Err(BundleError::new("not_found", format!("unknown bundle: {name}")));
    }
    if !dry_run {
        fs::remove_file(&path)
            .map_err(|e| BundleError::new("write_failed", format!("{}: {e}", path.display())))?;
    }
    Ok(path)
}

fn names_of(map: Option<&Value>, want: &str) -> Vec<String> {
    let mut out: Vec<String> = map
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.as_str() == Some(want))
        .map(|(k, _)| k.clone())
        .collect();
    out.sort();
    out
}

/// The edit that recreates what a folder's `.claude/settings.local.json` hides today
/// (`bundle add --from-folder`). Only the keys a bundle owns are read.
pub fn edit_from_folder(folder: &Path) -> Result<Edit, BundleError> {
    let path = folder.join(".claude").join("settings.local.json");
    let text = fs::read_to_string(&path)
        .map_err(|e| BundleError::new("not_found", format!("{}: {e}", path.display())))?;
    let settings: Value = serde_json::from_str(&text)
        .map_err(|e| BundleError::new("invalid", format!("{}: {e}", path.display())))?;
    let list = |v: Vec<String>| (!v.is_empty()).then_some(v);
    let plugins_off: Vec<String> = settings
        .get("enabledPlugins")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, v)| **v == Value::Bool(false))
        .map(|(k, _)| k.clone())
        .collect();
    let strings = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(String::from))
            .collect()
    };
    let agents_off: Vec<String> = strings(settings.pointer("/permissions/deny"))
        .iter()
        .filter_map(|e| e.strip_prefix("Agent(")?.strip_suffix(')').map(String::from))
        .collect();
    Ok(Edit {
        skills_off: list(names_of(settings.get("skillOverrides"), "off")),
        skills_name_only: list(names_of(settings.get("skillOverrides"), "name-only")),
        plugins_off: list(plugins_off),
        layers_exclude: list(strings(settings.get("claudeMdExcludes"))),
        agents_off: list(agents_off),
        ..Edit::default()
    })
}
