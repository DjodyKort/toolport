//! The one way bundle code writes a file: next to the target, then renamed over it, so a reader
//! (Claude Code writes the same settings file) never sees half a file.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(
        ".{name}.toolport-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, bytes)?;
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(&tmp, meta.permissions());
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

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
                write_atomic(path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?
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
    Err(format!("{} kept changing while it was being written", path.display()))
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
