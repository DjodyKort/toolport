use crate::registry::Registry;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

const PLUS_KEY: &str = "plus";
const RECORD_KEY: &str = "directEntries";

/// What Toolport wrote for one direct entry: the registry's ownership record, kept under
/// `plus.directEntries.<client>.<entry>` next to the managed gateway entry. The launcher's own
/// env is non-secret by construction, so the record holds values and nothing needs stripping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub server: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl Record {
    fn to_value(&self) -> Value {
        json!({
            "server": self.server,
            "command": self.command,
            "args": self.args,
            "env": self.env,
        })
    }

    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        Some(Self {
            server: object.get("server")?.as_str()?.to_string(),
            command: object.get("command")?.as_str()?.to_string(),
            args: object
                .get("args")?
                .as_array()?
                .iter()
                .filter_map(|a| a.as_str().map(String::from))
                .collect(),
            env: object
                .get("env")
                .and_then(Value::as_object)
                .map(|env| {
                    env.iter()
                        .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

fn section(reg: &Registry) -> Option<&Map<String, Value>> {
    reg.unknown_fields
        .get(PLUS_KEY)?
        .get(RECORD_KEY)?
        .as_object()
}

/// Every record as `(client id, entry name, record)`, ordered by client then entry.
pub fn all(reg: &Registry) -> Vec<(String, String, Record)> {
    let mut rows = Vec::new();
    for (client, entries) in section(reg).into_iter().flatten() {
        for (entry, value) in entries.as_object().into_iter().flatten() {
            if let Some(record) = Record::from_value(value) {
                rows.push((client.clone(), entry.clone(), record));
            }
        }
    }
    rows.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    rows
}

pub fn get(reg: &Registry, client: &str, entry: &str) -> Option<Record> {
    Record::from_value(section(reg)?.get(client)?.get(entry)?)
}

pub fn set(reg: &mut Registry, client: &str, entry: &str, record: &Record) {
    let plus = reg
        .unknown_fields
        .entry(PLUS_KEY.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !plus.is_object() {
        *plus = Value::Object(Map::new());
    }
    let clients = plus
        .as_object_mut()
        .expect("just made an object")
        .entry(RECORD_KEY.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !clients.is_object() {
        *clients = Value::Object(Map::new());
    }
    let entries = clients
        .as_object_mut()
        .expect("just made an object")
        .entry(client.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entries.is_object() {
        *entries = Value::Object(Map::new());
    }
    entries
        .as_object_mut()
        .expect("just made an object")
        .insert(entry.to_string(), record.to_value());
}

/// Drops one record and prunes the empty containers so a registry without direct entries
/// serializes exactly as it did before the feature existed.
pub fn clear(reg: &mut Registry, client: &str, entry: &str) -> bool {
    let Some(plus) = reg
        .unknown_fields
        .get_mut(PLUS_KEY)
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let mut removed = false;
    if let Some(clients) = plus.get_mut(RECORD_KEY).and_then(Value::as_object_mut) {
        if let Some(entries) = clients.get_mut(client).and_then(Value::as_object_mut) {
            removed = entries.remove(entry).is_some();
            if entries.is_empty() {
                clients.remove(client);
            }
        }
        if clients.is_empty() {
            plus.remove(RECORD_KEY);
        }
    }
    if plus.is_empty() {
        reg.unknown_fields.remove(PLUS_KEY);
    }
    removed
}
