//! The three-way comparison behind sync: what both machines last agreed on (the baseline), what
//! this machine holds now and what the other machine's file holds. It only classifies; nothing
//! here merges, and a row both machines changed is left for a person to settle.

use std::collections::BTreeMap;

use serde_json::Value;

use super::sha256_hex;

/// Hash of what both machines held for each row at the last sync, by row id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Baseline {
    pub rows: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Same,
    /// Only the other machine has it, and it was not here at the last sync.
    Absent,
    /// It was there at the last sync and this machine removed it since.
    RemovedHere,
    /// Only the other machine changed it.
    Incoming,
    /// Only this machine changed it.
    Mine,
    /// Both changed it, or there is no baseline to tell.
    Conflict,
}

impl Baseline {
    pub fn judge(&self, id: &str, other: &str, local: Option<&str>) -> Verdict {
        let base = self.rows.get(id).map(String::as_str);
        match local {
            Some(local) if local == other => Verdict::Same,
            Some(local) if base == Some(local) => Verdict::Incoming,
            Some(_) if base == Some(other) => Verdict::Mine,
            Some(_) => Verdict::Conflict,
            None if base == Some(other) => Verdict::RemovedHere,
            None => Verdict::Absent,
        }
    }
}

/// The JSON text with every object's keys sorted, so equal documents hash equal on every machine.
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::from(k.as_str()), canonical(&map[k])))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

pub fn hash_json(value: &Value) -> String {
    sha256_hex(canonical(value).as_bytes())
}
