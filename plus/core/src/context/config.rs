//! The one `context.json` setting this slice owns: whether `context sync` applies bound bundles.
//! The file is edited as plain JSON so every other key stays as it was.

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
    bundle_io::write_atomic(path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}
