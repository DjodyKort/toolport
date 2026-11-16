//! Mirror types for upstream records.
//!
//! A mirror only names the fields the fork reads. Everything else travels in
//! `extra` (`#[serde(flatten)]`), so a record converted upstream -> mirror ->
//! upstream comes back unchanged, including fields a newer upstream added.
//! Every mirror struct needs that catch-all and a round-trip test against an
//! upstream fixture.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::fork_data::ForkServerFields;

/// Mirror of upstream's `ServerEntry`.
///
/// `fork` carries the keys the fork owns (`ForkServerFields`); everything else,
/// including fields upstream types itself, travels in `extra`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(flatten)]
    pub fork: ForkServerFields,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Convert between two serde types through `serde_json::Value`.
pub fn convert<Src, Dst>(value: &Src) -> Result<Dst, String>
where
    Src: Serialize,
    Dst: DeserializeOwned,
{
    let json = serde_json::to_value(value).map_err(|error| error.to_string())?;
    serde_json::from_value(json).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mirror_keeps_unknown_fields_and_fork_data() {
        let upstream = json!({
            "id": "srv-1",
            "name": "Files",
            "enabled": true,
            "transport": "stdio",
            "hidePrompts": true,
            "plus": {"authProbe": {"kind": "slack"}, "futurePlusField": 7},
            "mcpmSource": {"type": "git", "path": "/tmp/x", "drift": false},
            "mcpmTags": ["team"],
            "forwardInstructions": true,
            "fieldFromNewerUpstream": {"nested": [1, 2]},
        });

        let record: ServerRecord = convert(&upstream).unwrap();
        assert_eq!(record.id, "srv-1");
        let plus = record.fork.plus.as_ref().unwrap();
        assert_eq!(
            plus.auth_probe.as_ref().unwrap().kind.as_deref(),
            Some("slack")
        );
        assert_eq!(
            record.fork.mcpm_source.as_ref().unwrap().kind.as_deref(),
            Some("git")
        );
        assert_eq!(record.fork.forward_instructions, Some(true));
        assert_eq!(record.extra["hidePrompts"], json!(true));
        assert!(!record.extra.contains_key("mcpmTags"));

        let back: Value = convert(&record).unwrap();
        assert_eq!(back, upstream);
    }

    #[test]
    fn mirror_without_fork_data_does_not_invent_keys() {
        let upstream = json!({"id": "srv-2", "name": "Docs", "enabled": false});
        let record: ServerRecord = convert(&upstream).unwrap();
        let back: Value = convert(&record).unwrap();
        assert_eq!(back, upstream);
    }

    #[test]
    fn mirror_keeps_falsy_and_empty_fork_values() {
        let upstream = json!({
            "id": "srv-3", "name": "Edge", "enabled": true,
            "mcpmOauth": false, "mcpmTags": [], "forwardInstructions": false,
            "maxRequestTimeoutMs": 0, "plus": {}, "mcpmSource": {"last_updated": "x"},
        });
        let record: ServerRecord = convert(&upstream).unwrap();
        let back: Value = convert(&record).unwrap();
        assert_eq!(back, upstream);
    }
}
