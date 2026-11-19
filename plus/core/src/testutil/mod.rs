#[cfg(feature = "cipher")]
pub mod alloc;
pub mod exec;

use std::collections::BTreeMap;
use std::path::Path;

/// Every file and directory under `root`, directories as `None` under a trailing-slash key.
pub fn tree_snapshot(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
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

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Held by a test that changes process-wide environment variables, so two of them never overlap.
pub fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
