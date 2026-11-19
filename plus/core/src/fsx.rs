//! Small file helpers the skills code shares: atomic writes that follow a symlinked destination,
//! a copy of an unreadable file kept aside, and the home and data dirs of this run.

use std::cell::RefCell;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);
const MAX_SYMLINK_HOPS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The file ends up owner-only, whatever it was before.
    Private,
    /// A file that exists keeps its mode; a new one gets the umask default.
    KeepOrDefault,
    /// The file ends up with exactly these permission bits, the umask notwithstanding.
    Exact(u32),
}

/// Writes through a symlink to its target and replaces the file in one rename, so a reader never
/// sees half a file and two writers never share a temp file.
pub fn atomic_write(path: &Path, contents: &str, mode: Mode) -> io::Result<()> {
    atomic_write_bytes(path, contents.as_bytes(), mode)
}

/// [`atomic_write`] for content that is not text.
pub fn atomic_write_bytes(path: &Path, contents: &[u8], mode: Mode) -> io::Result<()> {
    let dest = resolve_dest(path)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = std::fs::metadata(&dest).ok();
    let tmp = PathBuf::from(format!(
        "{}.{}.{}.plus-tmp",
        dest.display(),
        std::process::id(),
        WRITE_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let open_mode = match (mode, &existing) {
        (Mode::KeepOrDefault, None) => DEFAULT_MODE,
        _ => PRIVATE_MODE,
    };
    let written = create_new(&tmp, open_mode)
        .and_then(|mut file| {
            file.write_all(contents)?;
            file.sync_all()
        })
        .and_then(|()| match (mode, &existing) {
            (Mode::KeepOrDefault, Some(meta)) => std::fs::set_permissions(&tmp, meta.permissions()),
            (Mode::Exact(bits), _) => set_mode(&tmp, bits),
            _ => Ok(()),
        })
        .and_then(|()| std::fs::rename(&tmp, &dest));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// [`atomic_write`] as [`Mode::Private`] with the destination named in the error.
pub fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    atomic_write(path, contents, Mode::Private).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(unix)]
fn set_mode(path: &Path, bits: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _bits: u32) -> io::Result<()> {
    Ok(())
}

const PRIVATE_MODE: u32 = 0o600;
// The kernel applies the umask to this on creation.
const DEFAULT_MODE: u32 = 0o666;

fn create_new(path: &Path, mode: u32) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, mode);
    #[cfg(not(unix))]
    let _ = mode;
    options.open(path)
}

fn resolve_dest(path: &Path) -> io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_SYMLINK_HOPS {
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = std::fs::read_link(&current)?;
                current = match current.parent() {
                    Some(parent) if target.is_relative() => parent.join(target),
                    _ => target,
                };
            }
            _ => return Ok(current),
        }
    }
    Err(io::Error::other(format!(
        "symlink loop at {}",
        path.display()
    )))
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

/// Keeps these dirs for this thread until dropped; a dir left `None` keeps the one of the guard
/// around it. Without any guard `home` reads `HOME` and there is no data dir.
pub struct PathsGuard(Option<Paths>);

pub fn enter(paths: Paths) -> PathsGuard {
    PathsGuard(PATHS.with(|p| {
        let outer = p.borrow().clone().unwrap_or_default();
        p.borrow_mut().replace(Paths {
            home: paths.home.or(outer.home),
            data_dir: paths.data_dir.or(outer.data_dir),
        })
    }))
}

impl Drop for PathsGuard {
    fn drop(&mut self) {
        PATHS.with(|p| *p.borrow_mut() = self.0.take());
    }
}

pub fn home() -> Option<PathBuf> {
    let set = PATHS.with(|p| p.borrow().as_ref().and_then(|p| p.home.clone()));
    set.or_else(env_home)
}

#[cfg(not(test))]
fn env_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Unit tests never see the real home: a `HOME` outside the temp dir becomes an empty home of
/// this test process, so code that forgets its guard cannot read or fetch the user's own files.
#[cfg(test)]
fn env_home() -> Option<PathBuf> {
    let temp = std::env::temp_dir();
    let set = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|home| home.starts_with(&temp));
    set.or_else(|| {
        let sandbox = temp.join(format!("plus-core-test-home-{}", std::process::id()));
        std::fs::create_dir_all(&sandbox).ok()?;
        Some(sandbox)
    })
}

const CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";

#[cfg(not(test))]
fn env_claude_dir() -> Option<OsString> {
    std::env::var_os(CLAUDE_CONFIG_DIR)
}

/// Like the test `HOME`: a `CLAUDE_CONFIG_DIR` outside the temp dir is the developer's own and
/// is not seen, so a test never reads another Claude Code profile.
#[cfg(test)]
fn env_claude_dir() -> Option<OsString> {
    let temp = std::env::temp_dir();
    std::env::var_os(CLAUDE_CONFIG_DIR).filter(|dir| Path::new(dir).starts_with(&temp))
}

/// The folder Claude Code keeps its settings, memory and sessions in. `CLAUDE_CONFIG_DIR` counts
/// only as an absolute path, with no `~` expanded: the rule of upstream's
/// `claude_config_dir_override` in `clients.rs`, so client setup and everything here agree.
pub fn claude_dir_from(env: Option<OsString>, home: &Path) -> PathBuf {
    env.map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| home.join(".claude"))
}

/// The file Claude Code keeps its folder list and approvals in: `<CLAUDE_CONFIG_DIR>/.claude.json`
/// when the variable is set, else `~/.claude.json`, which sits beside the folder, not in it.
pub fn claude_json_from(env: Option<OsString>, home: &Path) -> PathBuf {
    match env.map(PathBuf::from).filter(|dir| dir.is_absolute()) {
        Some(dir) => dir.join(".claude.json"),
        None => home.join(".claude.json"),
    }
}

pub fn claude_dir_in(home: &Path) -> PathBuf {
    claude_dir_from(env_claude_dir(), home)
}

pub fn claude_json_in(home: &Path) -> PathBuf {
    claude_json_from(env_claude_dir(), home)
}

/// [`claude_dir_in`] for the home of this run.
pub fn claude_dir() -> Option<PathBuf> {
    home().map(|home| claude_dir_in(&home))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".plus-tmp"))
            .collect()
    }

    #[test]
    fn the_claude_folder_follows_an_absolute_config_dir_and_else_the_home() {
        let home = Path::new("/h");
        let set = |v: &str| Some(OsString::from(v));
        assert_eq!(
            claude_dir_from(set("/profiles/work"), home),
            Path::new("/profiles/work")
        );
        for ignored in [None, set(""), set("work"), set("~/work")] {
            assert_eq!(claude_dir_from(ignored, home), Path::new("/h/.claude"));
        }
    }

    #[test]
    fn the_claude_json_moves_into_a_config_dir_and_stays_beside_the_default_folder() {
        let home = Path::new("/h");
        assert_eq!(
            claude_json_from(Some(OsString::from("/profiles/work")), home),
            Path::new("/profiles/work/.claude.json")
        );
        assert_eq!(claude_json_from(None, home), Path::new("/h/.claude.json"));
        assert_eq!(
            claude_json_from(Some(OsString::from("relative")), home),
            Path::new("/h/.claude.json")
        );
    }

    #[test]
    fn an_unguarded_home_in_tests_is_never_the_users_own() {
        let home = home().unwrap();
        assert!(home.starts_with(std::env::temp_dir()), "{}", home.display());
    }

    #[test]
    fn a_nested_guard_keeps_the_dirs_it_leaves_unset() {
        let _outer = enter(Paths {
            home: Some(PathBuf::from("/outer/home")),
            data_dir: Some(PathBuf::from("/outer/data")),
        });
        {
            let _inner = enter(Paths {
                home: None,
                data_dir: Some(PathBuf::from("/inner/data")),
            });
            assert_eq!(home(), Some(PathBuf::from("/outer/home")));
            assert_eq!(data_dir(), Some(PathBuf::from("/inner/data")));
        }
        assert_eq!(data_dir(), Some(PathBuf::from("/outer/data")));
    }

    #[test]
    fn missing_parent_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/c.json");
        atomic_write(&path, "{}", Mode::Private).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_kept_and_its_target_written() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        let link = dir.path().join("link.txt");
        std::fs::write(&target, "old").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        atomic_write(&link, "new", Mode::KeepOrDefault).unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn keep_or_default_leaves_an_existing_mode_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        atomic_write(&path, "new", Mode::KeepOrDefault).unwrap();
        assert_eq!(mode_of(&path), 0o644);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[cfg(unix)]
    #[test]
    fn private_narrows_an_existing_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.json");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        atomic_write(&path, "new", Mode::Private).unwrap();
        assert_eq!(mode_of(&path), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn exact_sets_the_mode_whatever_the_file_had_and_bytes_survive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool.bin");
        std::fs::write(&path, "old").unwrap();
        atomic_write_bytes(&path, &[0, 159, 146, 150], Mode::Exact(0o755)).unwrap();
        assert_eq!(mode_of(&path), 0o755);
        assert_eq!(std::fs::read(&path).unwrap(), [0, 159, 146, 150]);
        let fresh = dir.path().join("fresh.txt");
        atomic_write_bytes(&fresh, b"x", Mode::Exact(0o640)).unwrap();
        assert_eq!(mode_of(&fresh), 0o640);
    }

    #[test]
    fn symlink_loop_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            let a = dir.path().join("a");
            let b = dir.path().join("b");
            std::os::unix::fs::symlink(&b, &a).unwrap();
            std::os::unix::fs::symlink(&a, &b).unwrap();
            let err = atomic_write(&a, "x", Mode::Private).unwrap_err();
            assert!(err.to_string().contains("symlink loop"), "{err}");
        }
    }

    #[test]
    fn concurrent_writers_leave_one_complete_file_and_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.json");
        let bodies: Vec<String> = (0..8).map(|i| format!("{i}").repeat(50_000)).collect();
        std::thread::scope(|scope| {
            for body in &bodies {
                let path = &path;
                scope.spawn(move || {
                    for _ in 0..20 {
                        atomic_write(path, body, Mode::KeepOrDefault).unwrap();
                    }
                });
            }
        });
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(bodies.contains(&text));
        assert!(leftovers(dir.path()).is_empty());
    }
}
