use crate::registry::{self, Registry};
use std::io::ErrorKind;
use std::path::Path;

/// Reads the registry without taking its lock or running the loader's recovery
/// and migration writes, so inspection never changes the data directory.
/// `Ok(None)` means the file does not exist.
pub(crate) fn read_at(path: &Path) -> Result<Option<Registry>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| registry::unreadable_message(path, e)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read registry: {e}")),
    }
}

/// A missing registry file, or an unresolvable data directory, reads as empty.
pub(crate) fn read() -> Result<Registry, String> {
    match registry::resolved_path() {
        Some(path) => Ok(read_at(&path)?.unwrap_or_default()),
        None => Ok(Registry::default()),
    }
}

pub(crate) fn read_opt() -> Option<Registry> {
    read_at(&registry::resolved_path()?).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plus::testutil::DataDirFx;

    #[test]
    fn missing_file_reads_as_empty_and_bad_json_reports() {
        let _fx = DataDirFx::new("registry-ro", "read");
        let path = registry::registry_path().unwrap();
        assert!(read_at(&path).unwrap().is_none());
        assert!(read().unwrap().servers.is_empty());
        assert!(read_opt().is_none());

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        let err = read().unwrap_err();
        assert!(err.starts_with("registry is not readable:"), "{err}");
        assert!(read_opt().is_none());

        std::fs::write(&path, serde_json::to_string(&Registry::default()).unwrap()).unwrap();
        assert!(read_at(&path).unwrap().is_some());
        assert!(read_opt().is_some());
    }

    #[test]
    fn reads_follow_toolport_registry_like_the_loader() {
        let _env = crate::clients::env_test_lock();
        let fx = DataDirFx::new("registry-ro", "override");
        let elsewhere = fx.dir.join("elsewhere.json");
        let mut other = Registry::default();
        other.deny_destructive = true;
        std::fs::write(&elsewhere, serde_json::to_string(&other).unwrap()).unwrap();

        assert!(!read().unwrap().deny_destructive);
        assert!(read_opt().is_none());
        let _restore = crate::clients::EnvRestore::set("TOOLPORT_REGISTRY", &elsewhere);
        assert!(read().unwrap().deny_destructive);
        assert!(read_opt().unwrap().deny_destructive);
    }
}
