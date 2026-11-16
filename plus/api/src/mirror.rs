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

/// Fork-owned data stored under the `plus` key of an upstream server entry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlusData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Mirror of upstream's `ServerEntry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerRecord {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plus: Option<PlusData>,
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
    fn mirror_keeps_unknown_fields_and_plus_data() {
        let upstream = json!({
            "id": "srv-1",
            "name": "Files",
            "enabled": true,
            "transport": "stdio",
            "hidePrompts": true,
            "plus": {"origin": "import", "futurePlusField": 7},
            "fieldFromNewerUpstream": {"nested": [1, 2]},
        });

        let record: ServerRecord = convert(&upstream).unwrap();
        assert_eq!(record.id, "srv-1");
        assert_eq!(
            record.plus.as_ref().unwrap().origin.as_deref(),
            Some("import")
        );
        assert_eq!(record.extra["hidePrompts"], json!(true));

        let back: Value = convert(&record).unwrap();
        assert_eq!(back, upstream);
    }

    #[test]
    fn mirror_without_plus_data_does_not_invent_the_key() {
        let upstream = json!({"id": "srv-2", "name": "Docs", "enabled": false});
        let record: ServerRecord = convert(&upstream).unwrap();
        let back: Value = convert(&record).unwrap();
        assert_eq!(back, upstream);
    }
}
