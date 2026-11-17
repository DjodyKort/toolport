//! The keys of `context.json` the scan reads, and the one it writes (`sourceRoots`). The file is
//! plain JSON, so every other key stays as the file has it; a file that does not parse reads as
//! the defaults and is never overwritten.

use crate::context::bundle_io;
use serde_json::{Map, Value};
use std::path::Path;

pub const DEFAULT_CLIENTS_ROOT: &str = "~/Documents/GitHub/ExampleWorkspace/clients";
const SOURCE_ROOTS: &str = "sourceRoots";
const SOURCE_ROOTS_ALIAS: &str = "source_roots";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub clients_root: String,
    pub corp_tools_dir: Option<String>,
    pub cf_wrapper_hash: Option<String>,
    pub source_roots: Vec<String>,
    pub inert_patterns: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            clients_root: DEFAULT_CLIENTS_ROOT.into(),
            corp_tools_dir: None,
            cf_wrapper_hash: None,
            source_roots: Vec::new(),
            inert_patterns: Vec::new(),
        }
    }
}

fn read_map(path: &Path) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(&bundle_io::read_exact(path).ok()??) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

fn strings(map: &Map<String, Value>, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| map.get(*key)?.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

pub fn load(path: &Path) -> Config {
    let Some(map) = read_map(path) else {
        return Config::default();
    };
    let text = |key: &str| map.get(key).and_then(Value::as_str).map(String::from);
    Config {
        clients_root: text("clients_root").unwrap_or_else(|| DEFAULT_CLIENTS_ROOT.into()),
        corp_tools_dir: text("corp_tools_dir"),
        cf_wrapper_hash: text("cf_wrapper_hash"),
        source_roots: strings(&map, &[SOURCE_ROOTS, SOURCE_ROOTS_ALIAS]),
        inert_patterns: strings(&map, &["inertPatterns", "inert_patterns"]),
    }
}

/// Whether [`set_source_roots`] can write `path`: it is missing or a JSON object.
pub fn check_writable(path: &Path) -> Result<(), String> {
    match bundle_io::read_exact(path)? {
        None => Ok(()),
        Some(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(_)) => Ok(()),
            _ => Err(format!(
                "{} is not a JSON object; fix or move it, then try again",
                path.display()
            )),
        },
    }
}

/// Replaces `sourceRoots` and leaves the other keys alone; a file that is not a JSON object is an
/// error and stays as it is.
pub fn set_source_roots(path: &Path, roots: &[String]) -> Result<(), String> {
    bundle_io::update(path, 3, |before| {
        let mut map = match before.map(serde_json::from_str::<Value>) {
            None => Map::new(),
            Some(Ok(Value::Object(map))) => map,
            Some(_) => {
                return Err(format!(
                    "{} is not a JSON object; fix or move it, then try again",
                    path.display()
                ))
            }
        };
        map.remove(SOURCE_ROOTS_ALIAS);
        if roots.is_empty() {
            map.remove(SOURCE_ROOTS);
        } else {
            map.insert(SOURCE_ROOTS.into(), roots.iter().cloned().collect());
        }
        let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())?;
        Ok((bundle_io::Action::Write(text + "\n"), ()))
    })
}
