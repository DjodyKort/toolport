//! Small file helpers the skills code shares: atomic writes that follow a symlinked destination,
//! a copy of an unreadable file kept aside, and the home and data dirs of this run.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);
const MAX_SYMLINK_HOPS: usize = 16;

/// Writes through a symlink to its target and replaces the file in one rename.
pub fn atomic_write(path: &Path, contents: &str) -> Result<(), String> {
    atomic_write_bytes(path, contents.as_bytes())
}

pub fn atomic_write_bytes(path: &Path, contents: &[u8]) -> Result<(), String> {
    let dest = resolve_dest(path)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = PathBuf::from(format!(
        "{}.{}.{}.plus-tmp",
        dest.display(),
        std::process::id(),
        WRITE_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let written = std::fs::write(&tmp, contents)
        .and_then(|()| restrict(&tmp))
        .and_then(|()| std::fs::rename(&tmp, &dest));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", dest.display()));
    }
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn resolve_dest(path: &Path) -> Result<PathBuf, String> {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_SYMLINK_HOPS {
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = std::fs::read_link(&current).map_err(|e| e.to_string())?;
                current = match current.parent() {
                    Some(parent) if target.is_relative() => parent.join(target),
                    _ => target,
                };
            }
            _ => return Ok(current),
        }
    }
    Err(format!("symlink loop at {}", path.display()))
}

/// Keeps a copy of a file about to be replaced because it could not be read.
pub fn quarantine_existing(path: &Path) -> Option<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let name = path.file_name()?.to_string_lossy().into_owned();
    let dest = path.with_file_name(format!("{name}.corrupt-{secs}"));
    std::fs::copy(path, &dest).ok()?;
    Some(dest)
}

#[derive(Clone, Debug, Default)]
pub struct Paths {
    pub home: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
}

thread_local! {
    static PATHS: RefCell<Option<Paths>> = const { RefCell::new(None) };
}

/// Keeps these dirs for this thread until dropped; without a guard `home` reads `HOME` and there
/// is no data dir.
pub struct PathsGuard(Option<Paths>);

pub fn enter(paths: Paths) -> PathsGuard {
    PathsGuard(PATHS.with(|p| p.borrow_mut().replace(paths)))
}

impl Drop for PathsGuard {
    fn drop(&mut self) {
        PATHS.with(|p| *p.borrow_mut() = self.0.take());
    }
}

pub fn home() -> Option<PathBuf> {
    let set = PATHS.with(|p| p.borrow().as_ref().and_then(|p| p.home.clone()));
    set.or_else(|| {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    })
}

pub fn data_dir() -> Option<PathBuf> {
    PATHS.with(|p| p.borrow().as_ref().and_then(|p| p.data_dir.clone()))
}

/// Civil date of a day count since 1970-01-01 (proleptic Gregorian).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
