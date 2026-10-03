use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[cfg(unix)]
#[path = "../../tests/common/exec.rs"]
pub(crate) mod exec;

struct SecretKeyRestore(Option<std::ffi::OsString>);

impl Drop for SecretKeyRestore {
    fn drop(&mut self) {
        match &self.0 {
            Some(value) => std::env::set_var("TOOLPORT_SECRET_KEY", value),
            None => std::env::remove_var("TOOLPORT_SECRET_KEY"),
        }
    }
}

pub(crate) struct DataDirFx {
    pub dir: PathBuf,
    // fields drop in order: key and override must go before the lock is released
    _secret_key: Option<SecretKeyRestore>,
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
            _secret_key: None,
            _override: guard,
            _lock: lock,
        }
    }

    /// Keeps secrets in the encrypted-file backend so a test never reaches the platform store.
    pub fn with_secret_key(mut self, key: &str) -> Self {
        if self._secret_key.is_none() {
            self._secret_key = Some(SecretKeyRestore(std::env::var_os("TOOLPORT_SECRET_KEY")));
        }
        std::env::set_var("TOOLPORT_SECRET_KEY", key);
        self
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

/// Every file and directory under `root`, directories as `None` under a trailing-slash key.
pub(crate) fn tree_snapshot(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                out.insert(format!("{rel}/"), None);
                stack.push(path);
            } else {
                out.insert(rel, Some(std::fs::read(&path).unwrap_or_default()));
            }
        }
    }
    out
}
