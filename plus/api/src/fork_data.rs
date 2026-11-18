//! Typed mirrors of everything the fork stores in the registry.
//!
//! The registry is upstream's file. The fork keeps its data in two places that
//! upstream preserves without reading: the top-level `plus` key and a few
//! per-server keys. Every struct here has a catch-all so a field the fork does
//! not know yet survives a read and write. Optional fields stay `Option` so an
//! absent key is never written back as a default.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// `plus.authProbe` on a server: which credential probe runs for it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthProbe {
    /// A service name, or `none` to switch the probe off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The `plus` object on a server entry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPlus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_probe: Option<AuthProbe>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl ServerPlus {
    pub fn is_empty(&self) -> bool {
        self.auth_probe.is_none() && self.extra.is_empty()
    }
}

/// `mcpmSource` on a server: where its code came from and how it updates.
/// Which other keys are present depends on `type`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct McpmSource {
    /// Absent on entries that only carry `last_updated`.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl McpmSource {
    /// This source with `fields` set on top, other keys kept.
    pub fn merged(&self, fields: Map<String, Value>) -> Result<Self, String> {
        let Value::Object(mut current) =
            serde_json::to_value(self).map_err(|error| error.to_string())?
        else {
            return Err("mcpmSource did not serialise to an object".into());
        };
        current.extend(fields);
        serde_json::from_value(Value::Object(current))
            .map_err(|error| format!("unreadable mcpmSource: {error}"))
    }
}

/// The per-server keys the fork owns. Upstream keeps them in the entry's
/// `unknown_fields`; `ServerEntry` has no field of these names.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkServerFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plus: Option<ServerPlus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcpm_source: Option<McpmSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcpm_oauth: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcpm_tags: Option<Vec<String>>,
    /// Include the server's own instructions in the gateway's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_instructions: Option<bool>,
    /// Declare the client's roots, sampling and elicitation to this server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declare_client_capabilities: Option<bool>,
    /// Absolute cap on one call to a stdio server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_request_timeout_ms: Option<u64>,
    /// How the gateway lists the server's tools: `auto` or `search` (see
    /// `toolport_plus::servers::mode`). Absent means `auto`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_mode: Option<String>,
}

impl ForkServerFields {
    pub const SERVER_MODE_KEY: &'static str = "serverMode";

    /// The registry keys these fields occupy on a server entry.
    pub const KEYS: [&'static str; 8] = [
        "plus",
        "mcpmSource",
        "mcpmOauth",
        "mcpmTags",
        "forwardInstructions",
        "declareClientCapabilities",
        "maxRequestTimeoutMs",
        Self::SERVER_MODE_KEY,
    ];

    /// Reads the fork keys out of a server entry's unknown fields. Fails, and
    /// names the key, when one has a shape the fork cannot represent.
    pub fn extract(unknown_fields: &Map<String, Value>) -> Result<Self, String> {
        let owned: Map<String, Value> = Self::KEYS
            .iter()
            .filter_map(|key| unknown_fields.get(*key).map(|v| ((*key).into(), v.clone())))
            .collect();
        serde_json::from_value(Value::Object(owned))
            .map_err(|error| format!("unreadable fork fields: {error}"))
    }

    /// Replaces the fork keys in a server entry's unknown fields. Keys that
    /// are `None` are removed and every other unknown field is left alone.
    pub fn store(&self, unknown_fields: &mut Map<String, Value>) -> Result<(), String> {
        for key in Self::KEYS {
            unknown_fields.remove(key);
        }
        let Value::Object(written) =
            serde_json::to_value(self).map_err(|error| error.to_string())?
        else {
            return Err("fork fields did not serialise to an object".into());
        };
        unknown_fields.extend(written);
        Ok(())
    }
}

/// `plus.selfmcp`: the record of where the fork's own MCP server was switched on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfMcpRecord {
    /// Profiles it has already been enabled in. A listed profile is never
    /// switched on again, so turning it off there sticks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_in: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opt_out: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One record under `plus.directEntries.<client>.<entry>`: what Toolport wrote
/// for a client's direct (non-gateway) entry. The launcher env is non-secret.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DirectEntry {
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Set for a remote server, where the launcher fields are empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The client config file the entry was written to; a client whose file moved reads as relocated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The top-level `plus` object of the registry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryPlus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selfmcp: Option<SelfMcpRecord>,
    /// Client id, then entry name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_entries: Option<BTreeMap<String, BTreeMap<String, DirectEntry>>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

const PLUS_KEY: &str = "plus";
const SELFMCP_KEY: &str = "selfmcp";
const DIRECT_KEY: &str = "directEntries";

impl RegistryPlus {
    /// Reads the `plus` key of the registry's unknown fields; absent is empty.
    pub fn extract(unknown_fields: &Map<String, Value>) -> Result<Self, String> {
        match unknown_fields.get(PLUS_KEY) {
            None => Ok(Self::default()),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|error| format!("unreadable plus data: {error}")),
        }
    }
}

/// Edits of the registry's `plus` object that touch one record and leave the
/// rest of the tree as it is. Removing a record also removes the containers it
/// empties, so a registry without fork data serialises as it did before the
/// fork wrote any. An empty `selfmcp` record is kept: its presence says the
/// self server was installed once.
pub mod plus_ops {
    use super::*;

    fn object_mut<'a>(parent: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
        let slot = parent
            .entry(key.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        slot.as_object_mut().expect("just made an object")
    }

    fn prune_empty_plus(unknown_fields: &mut Map<String, Value>) {
        let Some(plus) = unknown_fields
            .get_mut(PLUS_KEY)
            .and_then(Value::as_object_mut)
        else {
            return;
        };
        if plus.is_empty() {
            unknown_fields.remove(PLUS_KEY);
        }
    }

    /// Replaces `plus.selfmcp`, or removes it when `record` is `None`.
    pub fn set_selfmcp(
        unknown_fields: &mut Map<String, Value>,
        record: Option<&SelfMcpRecord>,
    ) -> Result<(), String> {
        match record {
            Some(record) => {
                let value = serde_json::to_value(record).map_err(|error| error.to_string())?;
                object_mut(unknown_fields, PLUS_KEY).insert(SELFMCP_KEY.into(), value);
            }
            None => {
                if let Some(plus) = unknown_fields
                    .get_mut(PLUS_KEY)
                    .and_then(Value::as_object_mut)
                {
                    plus.remove(SELFMCP_KEY);
                }
            }
        }
        prune_empty_plus(unknown_fields);
        Ok(())
    }

    /// Edits `plus.selfmcp` in place; the record is created when missing.
    pub fn update_selfmcp(
        unknown_fields: &mut Map<String, Value>,
        edit: impl FnOnce(&mut SelfMcpRecord),
    ) -> Result<(), String> {
        let mut record = RegistryPlus::extract(unknown_fields)?
            .selfmcp
            .unwrap_or_default();
        edit(&mut record);
        set_selfmcp(unknown_fields, Some(&record))
    }

    /// Writes one direct-entry record, replacing a record of the same name.
    pub fn set_direct_entry(
        unknown_fields: &mut Map<String, Value>,
        client: &str,
        entry: &str,
        record: &DirectEntry,
    ) -> Result<(), String> {
        let value = serde_json::to_value(record).map_err(|error| error.to_string())?;
        let plus = object_mut(unknown_fields, PLUS_KEY);
        let clients = object_mut(plus, DIRECT_KEY);
        object_mut(clients, client).insert(entry.to_string(), value);
        Ok(())
    }

    /// Removes one direct-entry record; true when it existed.
    pub fn clear_direct_entry(
        unknown_fields: &mut Map<String, Value>,
        client: &str,
        entry: &str,
    ) -> bool {
        let removed = unknown_fields
            .get_mut(PLUS_KEY)
            .and_then(Value::as_object_mut)
            .and_then(|plus| plus.get_mut(DIRECT_KEY))
            .and_then(Value::as_object_mut)
            .and_then(|clients| {
                let entries = clients.get_mut(client)?.as_object_mut()?;
                let removed = entries.remove(entry).is_some();
                if entries.is_empty() {
                    clients.remove(client);
                }
                Some(removed)
            })
            .unwrap_or(false);
        if let Some(plus) = unknown_fields
            .get_mut(PLUS_KEY)
            .and_then(Value::as_object_mut)
        {
            if plus
                .get(DIRECT_KEY)
                .and_then(Value::as_object)
                .is_some_and(Map::is_empty)
            {
                plus.remove(DIRECT_KEY);
            }
        }
        prune_empty_plus(unknown_fields);
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn server_fields_key_list_matches_the_struct() {
        let full = ForkServerFields {
            plus: Some(ServerPlus {
                auth_probe: Some(AuthProbe::default()),
                ..Default::default()
            }),
            mcpm_source: Some(McpmSource::default()),
            mcpm_oauth: Some(true),
            mcpm_tags: Some(vec![]),
            forward_instructions: Some(true),
            declare_client_capabilities: Some(true),
            max_request_timeout_ms: Some(1),
            server_mode: Some("search".into()),
        };
        let Value::Object(written) = serde_json::to_value(&full).unwrap() else {
            panic!("not an object")
        };
        let mut keys: Vec<_> = written.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected = ForkServerFields::KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected);
    }

    #[test]
    fn server_fields_store_touches_only_its_own_keys() {
        let mut unknown = map(json!({
            "mcpmTags": ["a"],
            "hidePrompts": true,
            "teamEnableReview": true,
            "plus": {"authProbe": {"kind": "slack", "futureProbeField": 1}, "futurePlus": 2}
        }));
        let mut fields = ForkServerFields::extract(&unknown).unwrap();
        assert_eq!(fields.mcpm_tags, Some(vec!["a".to_string()]));
        fields.mcpm_oauth = Some(true);
        fields.mcpm_tags = None;
        fields.store(&mut unknown).unwrap();
        assert_eq!(
            Value::Object(unknown),
            json!({
                "mcpmOauth": true,
                "hidePrompts": true,
                "teamEnableReview": true,
                "plus": {"authProbe": {"kind": "slack", "futureProbeField": 1}, "futurePlus": 2}
            })
        );
    }

    #[test]
    fn server_fields_store_keeps_an_empty_plus_object_it_did_not_empty() {
        let mut unknown = map(json!({"plus": {}, "mcpmOauth": true}));
        let mut fields = ForkServerFields::extract(&unknown).unwrap();
        fields.mcpm_oauth = None;
        fields.store(&mut unknown).unwrap();
        assert_eq!(Value::Object(unknown), json!({"plus": {}}));
    }

    #[test]
    fn server_fields_extract_names_a_malformed_key() {
        let unknown = map(json!({"mcpmTags": "not a list"}));
        let error = ForkServerFields::extract(&unknown).unwrap_err();
        assert!(
            error.contains("mcpmTags") || error.contains("sequence"),
            "{error}"
        );
    }

    #[test]
    fn mcpm_source_merge_sets_given_keys_and_keeps_the_rest() {
        let source: McpmSource = serde_json::from_value(
            json!({"type": "git", "path": "/p", "last_checked": "a", "drift": false}),
        )
        .unwrap();
        let merged = source
            .merged(map(json!({"last_checked": "b", "branch": "main"})))
            .unwrap();
        assert_eq!(
            serde_json::to_value(&merged).unwrap(),
            json!({"type": "git", "path": "/p", "branch": "main", "last_checked": "b", "drift": false})
        );
    }

    #[test]
    fn registry_plus_reads_selfmcp_and_direct_entries() {
        let unknown = map(json!({"plus": {
            "selfmcp": {"enabledIn": ["default", "cursor"]},
            "directEntries": {"claude-code": {"Figma": {
                "server": "figma", "command": "", "args": [], "env": {}, "url": "https://example.test/mcp"
            }}},
            "futureSection": {"x": 1}
        }}));
        let plus = RegistryPlus::extract(&unknown).unwrap();
        assert_eq!(
            plus.selfmcp.unwrap().enabled_in.unwrap(),
            vec!["default", "cursor"]
        );
        let entry = &plus.direct_entries.unwrap()["claude-code"]["Figma"];
        assert_eq!(entry.url.as_deref(), Some("https://example.test/mcp"));
        assert_eq!(plus.extra["futureSection"], json!({"x": 1}));
    }

    #[test]
    fn plus_ops_prune_what_they_empty_and_keep_the_rest() {
        let mut unknown = map(json!({"other": 1}));
        let entry = DirectEntry {
            server: "s".into(),
            command: "c".into(),
            ..Default::default()
        };
        plus_ops::set_direct_entry(&mut unknown, "claude-code", "S", &entry).unwrap();
        plus_ops::set_direct_entry(&mut unknown, "cursor", "S", &entry).unwrap();
        plus_ops::update_selfmcp(&mut unknown, |record| {
            record.enabled_in = Some(vec!["default".into()]);
        })
        .unwrap();

        assert!(plus_ops::clear_direct_entry(
            &mut unknown,
            "claude-code",
            "S"
        ));
        assert!(!plus_ops::clear_direct_entry(
            &mut unknown,
            "claude-code",
            "S"
        ));
        assert!(plus_ops::clear_direct_entry(&mut unknown, "cursor", "S"));
        assert_eq!(
            Value::Object(unknown.clone()),
            json!({"other": 1, "plus": {"selfmcp": {"enabledIn": ["default"]}}})
        );

        plus_ops::set_selfmcp(&mut unknown, None).unwrap();
        assert_eq!(Value::Object(unknown), json!({"other": 1}));
    }

    #[test]
    fn plus_ops_keep_unknown_siblings_of_the_records_they_edit() {
        let mut unknown = map(json!({"plus": {
            "selfmcp": {"enabledIn": ["a"], "futureField": true},
            "futureSection": {"x": 1}
        }}));
        plus_ops::update_selfmcp(&mut unknown, |record| {
            record.opt_out = Some(true);
        })
        .unwrap();
        assert_eq!(
            Value::Object(unknown),
            json!({"plus": {
                "selfmcp": {"enabledIn": ["a"], "optOut": true, "futureField": true},
                "futureSection": {"x": 1}
            }})
        );
    }
}
