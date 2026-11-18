//! The `context.json` settings this slice owns: whether `context sync` applies bound bundles and
//! the launch profiles. The file is edited as plain JSON so every other key stays as it was.

use super::bundle_io;
use serde_json::{Map, Value};
use std::path::Path;

const KEY: &str = "bundleAutoApply";

fn read(path: &Path) -> Map<String, Value> {
    match bundle_io::read_exact(path) {
        Ok(Some(text)) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        },
        _ => Map::new(),
    }
}

pub fn bundle_auto_apply(path: &Path) -> bool {
    read(path)
        .get(KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn set_bundle_auto_apply(path: &Path, on: bool) -> Result<(), String> {
    let mut map = read(path);
    if on {
        map.insert(KEY.into(), Value::Bool(true));
    } else {
        map.remove(KEY);
    }
    let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())? + "\n";
    crate::fsx::atomic_write(path, &text, crate::fsx::Mode::KeepOrDefault)
        .map_err(|e| format!("{}: {e}", path.display()))
}

const PROFILES_KEY: &str = "launchProfiles";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchProfile {
    pub access_set: String,
    pub deny_direct: bool,
}

pub fn launch_profiles(path: &Path) -> std::collections::BTreeMap<String, LaunchProfile> {
    let map = read(path);
    let Some(Value::Object(all)) = map.get(PROFILES_KEY) else {
        return Default::default();
    };
    all.iter()
        .filter_map(|(name, raw)| {
            Some((
                name.clone(),
                LaunchProfile {
                    access_set: raw.get("accessSet")?.as_str()?.to_string(),
                    deny_direct: raw
                        .get("denyDirect")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                },
            ))
        })
        .collect()
}

/// The profile as stored, with any key it carries that this slice does not own.
pub fn set_launch_profile(
    path: &Path,
    name: &str,
    profile: Option<&LaunchProfile>,
) -> Result<(), String> {
    let mut map = read(path);
    let mut all = match map.remove(PROFILES_KEY) {
        Some(Value::Object(all)) => all,
        _ => Map::new(),
    };
    match profile {
        Some(profile) => {
            let mut entry = match all.remove(name) {
                Some(Value::Object(entry)) => entry,
                _ => Map::new(),
            };
            entry.insert(
                "accessSet".into(),
                Value::String(profile.access_set.clone()),
            );
            if profile.deny_direct {
                entry.insert("denyDirect".into(), Value::Bool(true));
            } else {
                entry.remove("denyDirect");
            }
            all.insert(name.to_string(), Value::Object(entry));
        }
        None => {
            all.remove(name);
        }
    }
    if !all.is_empty() {
        map.insert(PROFILES_KEY.into(), Value::Object(all));
    }
    let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())? + "\n";
    crate::fsx::atomic_write(path, &text, crate::fsx::Mode::KeepOrDefault)
        .map_err(|e| format!("{}: {e}", path.display()))
}
