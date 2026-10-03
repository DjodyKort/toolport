//! Timestamped file snapshots under `<cache>/backups/<YYYYMMDD-HHMMSS>/`.

use super::roots::Roots;
use crate::usage_report::civil_from_days;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn stamp_for(unix_secs: i64) -> String {
    let (y, m, d) = civil_from_days(unix_secs.div_euclid(86_400));
    let rem = unix_secs.rem_euclid(86_400);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn now_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    stamp_for(secs)
}

/// Copies `path` into a timestamped backup dir; `None` when it does not exist.
pub fn snapshot(roots: &Roots, path: &Path) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let dest_dir = roots.backups_dir().join(now_stamp());
    fs::create_dir_all(&dest_dir).map_err(|e| format!("{}: {e}", dest_dir.display()))?;
    let name = path.file_name().ok_or("backup source has no file name")?;
    let mut dest = dest_dir.join(name);
    if dest.exists() {
        let count = fs::read_dir(&dest_dir).map(|r| r.count()).unwrap_or(0);
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        dest = dest_dir.join(format!("{stem}-{count}{ext}"));
    }
    fs::copy(path, &dest).map_err(|e| format!("{} -> {}: {e}", path.display(), dest.display()))?;
    Ok(Some(dest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_formats_utc() {
        assert_eq!(stamp_for(0), "19700101-000000");
        assert_eq!(stamp_for(1_767_225_600), "20260101-000000");
        assert_eq!(stamp_for(1_709_210_096), "20240229-123456");
    }
}
