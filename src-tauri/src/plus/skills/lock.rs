//! `mcpm-skills.lock`: byte-compatible with mcpm's `json.dumps(model_dump(), indent=2)`.

use super::json::{self, J};
use std::path::{Path, PathBuf};

pub const LOCKFILE_NAME: &str = "mcpm-skills.lock";

pub type OrderedMap<V> = Vec<(String, V)>;

#[derive(Clone, Debug, PartialEq)]
pub struct LockEntry {
    pub source: String,
    pub version: Option<String>,
    pub hash: String,
    pub clients_synced: Vec<String>,
    pub warnings: Vec<String>,
    pub output_files: OrderedMap<Vec<String>>,
    pub hooks_installed: OrderedMap<Vec<String>>,
}

impl LockEntry {
    pub fn new(version: Option<String>, hash: String) -> Self {
        Self {
            source: "local".into(),
            version,
            hash,
            clients_synced: Vec::new(),
            warnings: Vec::new(),
            output_files: Vec::new(),
            hooks_installed: Vec::new(),
        }
    }

    pub fn push_output(&mut self, client: &str, rel: String) {
        push_to(&mut self.output_files, client, std::iter::once(rel));
    }

    pub fn extend_outputs(&mut self, client: &str, rels: Vec<String>) {
        push_to(&mut self.output_files, client, rels);
    }

    pub fn set_hooks(&mut self, client: &str, ids: Vec<String>) {
        match self.hooks_installed.iter_mut().find(|(k, _)| k == client) {
            Some(slot) => slot.1 = ids,
            None => self.hooks_installed.push((client.to_string(), ids)),
        }
    }

    fn to_json(&self) -> J {
        J::Obj(vec![
            ("source".into(), J::str(&self.source)),
            (
                "version".into(),
                self.version.as_deref().map_or(J::Null, J::str),
            ),
            ("hash".into(), J::str(&self.hash)),
            ("clients_synced".into(), str_arr(&self.clients_synced)),
            ("warnings".into(), str_arr(&self.warnings)),
            ("output_files".into(), list_map(&self.output_files)),
            ("hooks_installed".into(), list_map(&self.hooks_installed)),
        ])
    }

    fn from_json(v: &J) -> Option<Self> {
        let J::Obj(_) = v else { return None };
        Some(Self {
            source: opt_string(v.get("source"))?.unwrap_or_else(|| "local".into()),
            version: opt_string(v.get("version"))?,
            hash: v.get("hash")?.as_str()?.to_string(),
            clients_synced: opt_str_list(v.get("clients_synced"))?,
            warnings: opt_str_list(v.get("warnings"))?,
            output_files: opt_list_map(v.get("output_files"))?,
            hooks_installed: opt_list_map(v.get("hooks_installed"))?,
        })
    }
}

fn push_to(map: &mut OrderedMap<Vec<String>>, key: &str, items: impl IntoIterator<Item = String>) {
    match map.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1.extend(items),
        None => map.push((key.to_string(), items.into_iter().collect())),
    }
}

fn str_arr(items: &[String]) -> J {
    J::Arr(items.iter().map(J::str).collect())
}

fn list_map(map: &OrderedMap<Vec<String>>) -> J {
    J::Obj(map.iter().map(|(k, v)| (k.clone(), str_arr(v))).collect())
}

fn opt_string(v: Option<&J>) -> Option<Option<String>> {
    match v {
        None | Some(J::Null) => Some(None),
        Some(J::Str(s)) => Some(Some(s.clone())),
        Some(_) => None,
    }
}

fn opt_str_list(v: Option<&J>) -> Option<Vec<String>> {
    match v {
        None => Some(Vec::new()),
        Some(J::Arr(items)) => items
            .iter()
            .map(|i| i.as_str().map(str::to_string))
            .collect(),
        Some(_) => None,
    }
}

fn opt_list_map(v: Option<&J>) -> Option<OrderedMap<Vec<String>>> {
    match v {
        None => Some(Vec::new()),
        Some(J::Obj(items)) => items
            .iter()
            .map(|(k, v)| Some((k.clone(), opt_str_list(Some(v))?)))
            .collect(),
        Some(_) => None,
    }
}

fn entries_to_json(map: &OrderedMap<LockEntry>) -> J {
    J::Obj(map.iter().map(|(k, e)| (k.clone(), e.to_json())).collect())
}

fn entries_from_json(v: Option<&J>) -> Option<OrderedMap<LockEntry>> {
    match v {
        None => Some(Vec::new()),
        Some(J::Obj(items)) => items
            .iter()
            .map(|(k, v)| Some((k.clone(), LockEntry::from_json(v)?)))
            .collect(),
        Some(_) => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LockFile {
    pub version: i64,
    pub synced_at: String,
    pub scope: String,
    pub output_root: String,
    pub skills: OrderedMap<LockEntry>,
    pub rules: OrderedMap<LockEntry>,
    pub agents: OrderedMap<LockEntry>,
    pub styles: OrderedMap<LockEntry>,
    pub active_styles: OrderedMap<String>,
}

pub fn get_entry<'a>(map: &'a OrderedMap<LockEntry>, name: &str) -> Option<&'a LockEntry> {
    map.iter().find(|(k, _)| k == name).map(|(_, e)| e)
}

pub fn get_entry_mut<'a>(
    map: &'a mut OrderedMap<LockEntry>,
    name: &str,
) -> Option<&'a mut LockEntry> {
    map.iter_mut().find(|(k, _)| k == name).map(|(_, e)| e)
}

/// Dict assignment: replaces in place when the key exists, otherwise appends.
pub fn set_entry(map: &mut OrderedMap<LockEntry>, name: &str, entry: LockEntry) {
    match map.iter_mut().find(|(k, _)| k == name) {
        Some(slot) => slot.1 = entry,
        None => map.push((name.to_string(), entry)),
    }
}

impl LockFile {
    pub fn new(synced_at: String) -> Self {
        Self {
            version: 1,
            synced_at,
            scope: String::new(),
            output_root: String::new(),
            skills: Vec::new(),
            rules: Vec::new(),
            agents: Vec::new(),
            styles: Vec::new(),
            active_styles: Vec::new(),
        }
    }

    pub fn to_json(&self) -> J {
        J::Obj(vec![
            ("version".into(), J::int(self.version)),
            ("synced_at".into(), J::str(&self.synced_at)),
            ("scope".into(), J::str(&self.scope)),
            ("output_root".into(), J::str(&self.output_root)),
            ("skills".into(), entries_to_json(&self.skills)),
            ("rules".into(), entries_to_json(&self.rules)),
            ("agents".into(), entries_to_json(&self.agents)),
            ("styles".into(), entries_to_json(&self.styles)),
            (
                "active_styles".into(),
                J::Obj(
                    self.active_styles
                        .iter()
                        .map(|(k, v)| (k.clone(), J::str(v)))
                        .collect(),
                ),
            ),
        ])
    }

    /// No trailing newline, exactly like `Path.write_text(json.dumps(..., indent=2))`.
    pub fn serialize(&self) -> String {
        self.to_json().dumps()
    }

    pub fn parse(text: &str) -> Option<Self> {
        let v = json::parse(text).ok()?;
        let J::Obj(_) = v else { return None };
        let int = |key: &str, default: i64| match v.get(key) {
            None => Some(default),
            Some(J::Num(n)) => n.parse::<i64>().ok(),
            Some(_) => None,
        };
        let string = |key: &str| Some(opt_string(v.get(key))?.unwrap_or_default());
        let active_styles = match v.get("active_styles") {
            None => Vec::new(),
            Some(J::Obj(items)) => items
                .iter()
                .map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect::<Option<Vec<_>>>()?,
            Some(_) => return None,
        };
        Some(Self {
            version: int("version", 1)?,
            synced_at: string("synced_at")?,
            scope: string("scope")?,
            output_root: string("output_root")?,
            skills: entries_from_json(v.get("skills"))?,
            rules: entries_from_json(v.get("rules"))?,
            agents: entries_from_json(v.get("agents"))?,
            styles: entries_from_json(v.get("styles"))?,
            active_styles,
        })
    }
}

pub fn lockfile_path(dir: &Path) -> PathBuf {
    dir.join(LOCKFILE_NAME)
}

/// `None` when the file is missing or does not validate, as mcpm's `load_lockfile`.
pub fn load_lockfile(dir: &Path) -> Option<LockFile> {
    let text = std::fs::read_to_string(lockfile_path(dir)).ok()?;
    LockFile::parse(&text)
}

pub fn save_lockfile(dir: &Path, lock: &LockFile) -> Result<(), String> {
    crate::registry::atomic_write(&lockfile_path(dir), &lock.serialize())
}
