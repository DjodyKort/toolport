//! Read, compare and write files that Claude Code also writes.

use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Debug)]
pub enum Action {
    Keep,
    Write(String),
    Delete,
}

/// Read, compute, and act only if the bytes are still the ones the answer was computed from;
/// otherwise compute again (up to `tries` times).
pub fn update<T>(
    path: &Path,
    tries: usize,
    mut compute: impl FnMut(Option<&str>) -> Result<(Action, T), String>,
) -> Result<T, String> {
    for _ in 0..tries {
        let before = read_exact(path)?;
        let (action, out) = compute(before.as_deref())?;
        if read_exact(path)? != before {
            continue;
        }
        match action {
            Action::Write(text) => {
                crate::fsx::atomic_write(path, &text, crate::fsx::Mode::KeepOrDefault)
                    .map_err(|e| format!("{}: {e}", path.display()))?
            }
            Action::Delete => match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{}: {e}", path.display())),
            },
            Action::Keep => {}
        }
        return Ok(out);
    }
    Err(format!(
        "{} kept changing while it was being written",
        path.display()
    ))
}

pub fn read_exact(path: &Path) -> Result<Option<String>, String> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| format!("{} is not valid UTF-8", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}
