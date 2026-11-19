//! Working folders of an export. The sessions part is zipped in the clear before it is sealed, so
//! that zip lives in the data folder, owner-only, and never next to the file being written.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::Ctx;
use crate::op::OpError;

const DIR: &str = "scratch";
const PREFIX: &str = "export-";
/// A run that long is a crash, whatever its process id has become since.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

pub struct Scratch {
    path: PathBuf,
    data_dir: PathBuf,
}

impl Scratch {
    pub fn new(ctx: &Ctx<'_>) -> Result<Self, OpError> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = ctx.state_dir().join(DIR).join(format!(
            "{PREFIX}{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        create_private_dir(&path)
            .map_err(|e| OpError::failed("write", format!("{}: {e}", path.display())))?;
        Ok(Self {
            path,
            data_dir: ctx.data_dir.clone(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
        // The folders made on the way go too, as long as nothing else lives in them.
        let mut dir = self.path.parent();
        while let Some(parent) =
            dir.filter(|d| d.starts_with(&self.data_dir) && *d != self.data_dir)
        {
            if std::fs::remove_dir(parent).is_err() {
                break;
            }
            dir = parent.parent();
        }
    }
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

/// Removes what an export that never finished left behind: folders of a process that is gone, or
/// older than a day. Folders of this process belong to runs that are still going.
pub fn sweep(ctx: &Ctx<'_>) {
    let state = ctx.state_dir();
    sweep_in(&state.join(DIR), PREFIX);
    // Where scratch folders lived before they moved under `scratch/`.
    sweep_in(&state, ".toolport-setup-");
}

fn sweep_in(dir: &Path, prefix: &str) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(rest) = name.strip_prefix(prefix) else {
            continue;
        };
        let Some(pid) = rest.split('-').next().and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| SystemTime::now().duration_since(at).ok())
            .is_some_and(|age| age > STALE_AFTER);
        let running = pid == std::process::id() || crate::tasks::proc::alive(pid);
        if running && !old {
            continue;
        }
        let path = entry.path();
        let _ = match entry.file_type() {
            Ok(kind) if kind.is_dir() => std::fs::remove_dir_all(&path),
            _ => std::fs::remove_file(&path),
        };
    }
}
