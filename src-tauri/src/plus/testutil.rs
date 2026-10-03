use std::path::PathBuf;

#[cfg(unix)]
#[path = "../../tests/common/exec.rs"]
pub(crate) mod exec;

pub(crate) struct DataDirFx {
    pub dir: PathBuf,
    // fields drop in order: the override must go before the lock is released
    _override: crate::registry::DataDirOverride,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl DataDirFx {
    pub fn new(prefix: &str, tag: &str) -> Self {
        Self::build(prefix, tag, None)
    }

    pub fn with_data_subdir(prefix: &str, tag: &str, data: &str) -> Self {
        Self::build(prefix, tag, Some(data))
    }

    fn build(prefix: &str, tag: &str, data: Option<&str>) -> Self {
        let lock = crate::registry::data_dir_test_lock();
        let dir = std::env::temp_dir().join(format!("{prefix}-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data_dir = match data {
            Some(sub) => dir.join(sub),
            None => dir.clone(),
        };
        std::fs::create_dir_all(&data_dir).unwrap();
        let guard = crate::registry::DataDirOverride::set(&data_dir);
        Self {
            dir,
            _override: guard,
            _lock: lock,
        }
    }

    pub fn write_registry(&self, registry: &serde_json::Value) {
        std::fs::write(
            self.dir.join("registry.json"),
            serde_json::to_string_pretty(registry).unwrap(),
        )
        .unwrap();
    }

}

impl Drop for DataDirFx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
