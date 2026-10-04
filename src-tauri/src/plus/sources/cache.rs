//! `<data dir>/plus/cache/sources.json`: parse results keyed by a stamp (file mtime and length, a
//! git commit or a blob id). A stale stamp is a miss, the file is derived state, and a failed
//! write is ignored. Only the engine flushes it; detectors read and fill the in-memory map.

use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const VERSION: u64 = 1;

pub fn path_in(data_dir: &Path) -> PathBuf {
    data_dir.join("plus").join("cache").join("sources.json")
}

pub struct Cache {
    path: Option<PathBuf>,
    reuse: bool,
    entries: RefCell<BTreeMap<String, (String, Value)>>,
    touched: RefCell<BTreeSet<String>>,
    dirty: Cell<bool>,
    hits: Cell<usize>,
    misses: Cell<usize>,
}

impl Cache {
    pub fn memory() -> Self {
        Self::with(None, BTreeMap::new(), true)
    }

    /// Loads the file; `refresh` ignores what is stored but still writes the new results.
    pub fn open(path: PathBuf, refresh: bool) -> Self {
        let stored = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .filter(|doc| doc["version"] == VERSION)
            .and_then(|doc| doc["entries"].as_object().cloned())
            .map(|map| {
                map.into_iter()
                    .filter_map(|(key, e)| {
                        let stamp = e["stamp"].as_str()?.to_string();
                        Some((key, (stamp, e["value"].clone())))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self::with(Some(path), stored, !refresh)
    }

    fn with(
        path: Option<PathBuf>,
        entries: BTreeMap<String, (String, Value)>,
        reuse: bool,
    ) -> Self {
        Self {
            path,
            reuse,
            entries: RefCell::new(entries),
            touched: RefCell::new(BTreeSet::new()),
            dirty: Cell::new(false),
            hits: Cell::new(0),
            misses: Cell::new(0),
        }
    }

    pub fn get(&self, key: &str, stamp: &str) -> Option<Value> {
        let found = self
            .reuse
            .then(|| {
                self.entries
                    .borrow()
                    .get(key)
                    .filter(|(s, _)| s == stamp)
                    .map(|(_, v)| v.clone())
            })
            .flatten();
        match found {
            Some(value) => {
                self.hits.set(self.hits.get() + 1);
                self.touched.borrow_mut().insert(key.to_string());
                Some(value)
            }
            None => {
                self.misses.set(self.misses.get() + 1);
                None
            }
        }
    }

    pub fn put(&self, key: &str, stamp: &str, value: Value) {
        self.entries
            .borrow_mut()
            .insert(key.to_string(), (stamp.to_string(), value));
        self.touched.borrow_mut().insert(key.to_string());
        self.dirty.set(true);
    }

    pub fn hits(&self) -> usize {
        self.hits.get()
    }

    pub fn misses(&self) -> usize {
        self.misses.get()
    }

    /// A complete scan drops what it did not use; a partial one keeps everything it loaded.
    pub fn flush(&self, complete: bool) {
        let Some(path) = &self.path else { return };
        let touched = self.touched.borrow();
        let mut entries = self.entries.borrow_mut();
        if complete {
            let before = entries.len();
            entries.retain(|key, _| touched.contains(key));
            if entries.len() != before {
                self.dirty.set(true);
            }
        }
        if !self.dirty.get() {
            return;
        }
        let doc = json!({
            "version": VERSION,
            "entries": entries
                .iter()
                .map(|(k, (stamp, value))| (k.clone(), json!({"stamp": stamp, "value": value})))
                .collect::<serde_json::Map<String, Value>>(),
        });
        let _ = crate::registry::atomic_write(path, &doc.to_string());
        self.dirty.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sources-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_matching_stamp_hits_and_a_changed_one_misses() {
        let dir = temp("stamp");
        let path = path_in(&dir);
        let cache = Cache::open(path.clone(), false);
        assert!(cache.get("k", "1").is_none());
        cache.put("k", "1", json!({"a": 1}));
        cache.flush(true);
        let again = Cache::open(path.clone(), false);
        assert_eq!(again.get("k", "1"), Some(json!({"a": 1})));
        assert!(again.get("k", "2").is_none());
        assert_eq!((again.hits(), again.misses()), (1, 1));
        let refreshed = Cache::open(path, true);
        assert!(refreshed.get("k", "1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_complete_flush_drops_unused_entries_and_a_partial_one_keeps_them() {
        let dir = temp("prune");
        let path = path_in(&dir);
        let seed = Cache::open(path.clone(), false);
        seed.put("old", "1", json!(1));
        seed.put("new", "1", json!(2));
        seed.flush(true);
        let partial = Cache::open(path.clone(), false);
        partial.get("new", "1");
        partial.flush(false);
        assert!(Cache::open(path.clone(), false).get("old", "1").is_some());
        let complete = Cache::open(path.clone(), false);
        complete.get("new", "1");
        complete.flush(true);
        let last = Cache::open(path, false);
        assert!(last.get("old", "1").is_none());
        assert!(last.get("new", "1").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_reads_as_empty() {
        let dir = temp("corrupt");
        let path = path_in(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        assert!(Cache::open(path, false).get("k", "1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
