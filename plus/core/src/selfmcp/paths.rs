//! Folder arguments of the tools that create or register a folder: a path is resolved against the
//! real home directory before it is judged, so a symlink or `..` cannot step out of it.

use std::path::{Component, Path, PathBuf};

use super::ToolError;

pub(super) struct Resolved {
    pub path: PathBuf,
    pub home: PathBuf,
}

fn refused(message: impl Into<String>) -> ToolError {
    ToolError::new("refused", message)
}

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::new("invalid_arguments", message)
}

fn real(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut base = path;
    while !base.exists() {
        match (base.file_name(), base.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_owned());
                base = parent;
            }
            _ => break,
        }
    }
    let mut resolved = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    resolved.extend(rest.iter().rev());
    resolved
}

pub(super) fn resolve(raw: &str) -> Result<Resolved, ToolError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid("the path is empty"));
    }
    let home =
        crate::fsx::home().ok_or_else(|| ToolError::new("not_found", "home directory unknown"))?;
    let expanded = match raw {
        "~" => home.clone(),
        _ => match raw.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(raw),
        },
    };
    if !expanded.is_absolute() {
        return Err(invalid("give an absolute path or one that starts with ~/"));
    }
    if expanded
        .components()
        .any(|part| part == Component::ParentDir)
    {
        return Err(invalid("the path must not contain .."));
    }
    Ok(Resolved {
        path: real(&expanded),
        home: real(&home),
    })
}

/// A folder strictly inside the home directory.
pub(super) fn under_home(raw: &str) -> Result<PathBuf, ToolError> {
    let Resolved { path, home } = resolve(raw)?;
    if path == home || !path.starts_with(&home) {
        return Err(refused(format!(
            "{} is not inside the home directory {}",
            path.display(),
            home.display()
        )));
    }
    Ok(path)
}

/// Any folder except the filesystem root and the home directory itself, which would put every
/// repository on the machine in scope.
pub(super) fn not_root_or_home(raw: &str) -> Result<PathBuf, ToolError> {
    let Resolved { path, home } = resolve(raw)?;
    if path.parent().is_none() || path == home {
        return Err(refused(format!(
            "{} is too broad to use as a source root",
            path.display()
        )));
    }
    Ok(path)
}
