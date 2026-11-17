//! What crosses a downstream server's handshake, in both directions.
//!
//! Toward the server, `declareClientCapabilities` makes the gateway declare in `initialize` the
//! capabilities its own clients declared. From the server, `forwardInstructions` puts the server's
//! `instructions` into the gateway's. Both are off by default, which keeps upstream's empty
//! declaration and the gateway's own text.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use serde_json::{json, Map, Value};

pub const DECLARE_KEY: &str = "declareClientCapabilities";
pub const FORWARD_KEY: &str = "forwardInstructions";

/// What the gateway can service for a server: it relays `roots/list`, `sampling/createMessage` and
/// `elicitation/create` to the client that made the call. Anything else a client declares
/// (`experimental`, `extensions`, `tasks`) is not relayed, so it is not declared downstream.
const RELAYABLE_CLIENT_CAPABILITIES: [&str; 3] = ["roots", "sampling", "elicitation"];

/// Characters kept from one server's forwarded instructions, cut at a character boundary.
pub const MAX_FORWARDED_INSTRUCTIONS_CHARS: usize = 4096;

const FORWARDED_LABEL_MAX_CHARS: usize = 64;

/// The part of an `initialize` capabilities object that is declared downstream.
pub fn relayable_client_capabilities(capabilities: Option<&Value>) -> Map<String, Value> {
    let Some(declared) = capabilities.and_then(Value::as_object) else {
        return Map::new();
    };
    RELAYABLE_CLIENT_CAPABILITIES
        .iter()
        .filter_map(|key| {
            declared
                .get(*key)
                .filter(|value| !value.is_null())
                .map(|value| ((*key).to_string(), value.clone()))
        })
        .collect()
}

/// Adds `incoming` to `into`, never removing what is there. Returns whether `into` changed.
pub fn merge_client_capabilities(
    into: &mut Map<String, Value>,
    incoming: Map<String, Value>,
) -> bool {
    let mut changed = false;
    for (key, value) in incoming {
        match into.get_mut(&key) {
            None => {
                into.insert(key, value);
                changed = true;
            }
            Some(existing) => changed |= merge_value(existing, value),
        }
    }
    changed
}

fn merge_value(existing: &mut Value, incoming: Value) -> bool {
    match (existing, incoming) {
        (Value::Object(have), Value::Object(add)) => merge_client_capabilities(have, add),
        (Value::Bool(have), Value::Bool(true)) if !*have => {
            *have = true;
            true
        }
        _ => false,
    }
}

/// The capabilities the clients of this process declared, and what each server was told.
///
/// One gateway process serves many clients through one set of downstream connections, so the
/// declaration only grows: it is the union of every client seen.
#[derive(Default)]
pub struct Declarations {
    clients: Mutex<Map<String, Value>>,
    told: Mutex<HashMap<String, Map<String, Value>>>,
}

impl Declarations {
    /// Records what a client declared. Returns whether the union changed.
    pub fn note_client(&self, capabilities: Option<&Value>) -> bool {
        let incoming = relayable_client_capabilities(capabilities);
        if incoming.is_empty() {
            return false;
        }
        let mut declared = self.clients.lock().unwrap_or_else(PoisonError::into_inner);
        merge_client_capabilities(&mut declared, incoming)
    }

    /// Forgets every declaration. For tests of the callers of the process-wide instance.
    pub fn reset(&self) {
        self.clients
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.told
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    pub fn clients(&self) -> Map<String, Value> {
        self.clients
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Remembers that `server` was connected declaring `declared`.
    pub fn told(&self, server: &str, declared: &Map<String, Value>) {
        self.told
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(server.to_string(), declared.clone());
    }

    /// Whether `server` was connected with a declaration other than the current one. A server
    /// that was told nothing counts as told an empty object.
    pub fn is_stale(&self, server: &str) -> bool {
        let wanted = self.clients();
        let told = self.told.lock().unwrap_or_else(PoisonError::into_inner);
        told.get(server).unwrap_or(&Map::new()) != &wanted
    }
}

pub static DECLARATIONS: std::sync::LazyLock<Declarations> =
    std::sync::LazyLock::new(Declarations::default);

/// The declaration `initialize` carries for a server that opted in: what its clients declared so
/// far, `{}` until one has.
pub fn declaration_for_connect(server: &str) -> Value {
    let declared = DECLARATIONS.clients();
    DECLARATIONS.told(server, &declared);
    Value::Object(declared)
}

static INSTRUCTIONS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Keeps the `instructions` of a server's `initialize` or `server/discover` result, if any, as it
/// sent them. A blank text clears what an earlier connection stored.
pub fn store_instructions(server: &str, handshake: &Value) {
    let text = handshake
        .get("instructions")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty());
    let mut stored = INSTRUCTIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let stored = stored.get_or_insert_with(HashMap::new);
    match text {
        Some(text) => stored.insert(server.to_string(), text.to_string()),
        None => stored.remove(server),
    };
}

pub fn stored_instructions(server: &str) -> Option<String> {
    INSTRUCTIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()?
        .get(server)
        .cloned()
}

/// Text from a server, without control characters other than newline and tab, line ends as `\n`.
pub fn clean(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect()
}

/// One server's instructions as a section of the gateway's text, or `None` when `body` is blank.
/// The caller has cleaned, neutralized and screened `body`; this keeps it to the cap and names
/// the server in a heading it cannot break out of.
pub fn section(server_name: &str, body: &str) -> Option<String> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    let mut kept: String = body
        .chars()
        .take(MAX_FORWARDED_INSTRUCTIONS_CHARS)
        .collect();
    if body.chars().nth(MAX_FORWARDED_INSTRUCTIONS_CHARS).is_some() {
        kept.push_str(&format!(
            "\n[Toolport: cut at {MAX_FORWARDED_INSTRUCTIONS_CHARS} characters]"
        ));
    }
    Some(format!(
        "## Instructions from server \"{}\"\n{kept}",
        label(server_name)
    ))
}

fn label(server_name: &str) -> String {
    let label: String = server_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if c == '"' { '\'' } else { c })
        .take(FORWARDED_LABEL_MAX_CHARS)
        .collect();
    if label.is_empty() {
        "unnamed".to_string()
    } else {
        label
    }
}

/// The gateway's own text with the forwarded sections after it. The configured text keeps the
/// first position; with none, the sections stand alone.
pub fn with_sections(own: Option<String>, sections: &[String]) -> Option<String> {
    if sections.is_empty() {
        return own;
    }
    let mut text = own.unwrap_or_default();
    for section in sections {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(section);
    }
    Some(text)
}

/// Whether a server's unknown fields switch `key` on.
pub fn switched_on(unknown_fields: &Map<String, Value>, key: &str) -> bool {
    unknown_fields.get(key) == Some(&json!(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn test01_only_the_capabilities_the_gateway_relays_are_declared() {
        let declared = json!({
            "roots": { "listChanged": true },
            "sampling": {},
            "elicitation": { "form": {}, "url": {} },
            "experimental": { "x": {} },
            "extensions": { "io.modelcontextprotocol/ui": {} },
            "tasks": {}
        });
        assert_eq!(
            Value::Object(relayable_client_capabilities(Some(&declared))),
            json!({
                "roots": { "listChanged": true },
                "sampling": {},
                "elicitation": { "form": {}, "url": {} }
            })
        );
        for nothing in [json!({}), json!({ "roots": null }), json!("elicitation")] {
            assert!(relayable_client_capabilities(Some(&nothing)).is_empty());
        }
        assert!(relayable_client_capabilities(None).is_empty());
    }

    #[test]
    fn test02_the_declaration_only_grows_and_says_when_it_did() {
        let mut declared = Map::new();
        assert!(merge_client_capabilities(
            &mut declared,
            map(json!({ "roots": {} }))
        ));
        assert!(!merge_client_capabilities(
            &mut declared,
            map(json!({ "roots": {} }))
        ));
        assert!(
            !merge_client_capabilities(&mut declared, Map::new()),
            "a client that declares less takes nothing away"
        );
        assert!(merge_client_capabilities(
            &mut declared,
            map(json!({ "roots": { "listChanged": true }, "elicitation": {} }))
        ));
        assert!(merge_client_capabilities(
            &mut declared,
            map(json!({ "elicitation": { "url": {} } }))
        ));
        assert_eq!(
            Value::Object(declared),
            json!({ "roots": { "listChanged": true }, "elicitation": { "url": {} } })
        );
    }

    #[test]
    fn test03_a_flag_one_client_set_is_not_taken_back_by_one_that_did_not() {
        let mut declared = map(json!({ "roots": { "listChanged": true } }));
        assert!(!merge_client_capabilities(
            &mut declared,
            map(json!({ "roots": { "listChanged": false } }))
        ));
        assert_eq!(declared["roots"]["listChanged"], true);
    }

    #[test]
    fn test04_a_server_is_stale_when_it_was_told_less_than_its_clients_declared() {
        let declarations = Declarations::default();
        assert!(
            !declarations.is_stale("odh"),
            "nothing declared, nothing to tell"
        );

        assert!(declarations.note_client(Some(&json!({ "elicitation": {}, "tasks": {} }))));
        assert!(!declarations.note_client(Some(&json!({ "elicitation": {} }))));
        assert!(declarations.is_stale("odh"));

        declarations.told("odh", &declarations.clients());
        assert!(!declarations.is_stale("odh"));
        assert!(declarations.is_stale("other"));

        assert!(declarations.note_client(Some(&json!({ "roots": {} }))));
        assert!(declarations.is_stale("odh"));
    }

    #[test]
    fn test05_a_section_names_the_server_and_keeps_the_text() {
        assert_eq!(
            section("odh", &clean("FAKE-odh-instructions\r\nsecond line")).as_deref(),
            Some("## Instructions from server \"odh\"\nFAKE-odh-instructions\nsecond line")
        );
    }

    #[test]
    fn test06_blank_text_and_control_characters_forward_nothing() {
        assert_eq!(section("odh", &clean(" \n\t ")), None);
        assert_eq!(section("odh", &clean("\u{7}\u{0}")), None);
    }

    #[test]
    fn test07_a_long_text_is_cut_at_the_cap_on_a_character_boundary() {
        let exact = "é".repeat(MAX_FORWARDED_INSTRUCTIONS_CHARS);
        let kept = section("odh", &exact).unwrap();
        assert!(kept.ends_with(&exact), "text at the cap is kept whole");
        assert!(!kept.contains("cut at"));

        let over = format!("{exact}é");
        let kept = section("odh", &over).unwrap();
        let body = kept.split_once('\n').unwrap().1;
        let (text, note) = body.split_once("\n[").unwrap();
        assert_eq!(text.chars().count(), MAX_FORWARDED_INSTRUCTIONS_CHARS);
        assert_eq!(
            note,
            format!("Toolport: cut at {MAX_FORWARDED_INSTRUCTIONS_CHARS} characters]")
        );
    }

    #[test]
    fn test08_a_server_name_cannot_break_the_heading() {
        let kept = section("od\nh \"x\"\r\n## fake", "FAKE-text").unwrap();
        assert_eq!(
            kept.lines().next().unwrap(),
            "## Instructions from server \"od h 'x' ## fake\""
        );
        assert_eq!(
            section("\n", "FAKE-text").unwrap().lines().next().unwrap(),
            "## Instructions from server \"unnamed\""
        );
        let long = section(&"n".repeat(200), "FAKE-text").unwrap();
        assert_eq!(
            long.lines().next().unwrap().chars().count(),
            "## Instructions from server \"\"".chars().count() + FORWARDED_LABEL_MAX_CHARS
        );
    }

    #[test]
    fn test09_sections_follow_the_configured_text() {
        let sections = vec!["## A\none".to_string(), "## B\ntwo".to_string()];
        assert_eq!(
            with_sections(Some("Profile text.".into()), &sections).as_deref(),
            Some("Profile text.\n\n## A\none\n\n## B\ntwo")
        );
        assert_eq!(
            with_sections(None, &sections).as_deref(),
            Some("## A\none\n\n## B\ntwo")
        );
        assert_eq!(
            with_sections(Some("Profile text.".into()), &[]).as_deref(),
            Some("Profile text.")
        );
        assert_eq!(with_sections(None, &[]), None);
    }

    #[test]
    fn test10_a_stored_text_is_kept_per_server_and_a_blank_one_clears_it() {
        store_instructions("s10-a", &json!({ "instructions": "FAKE-first" }));
        store_instructions("s10-b", &json!({}));
        assert_eq!(stored_instructions("s10-a").as_deref(), Some("FAKE-first"));
        assert_eq!(stored_instructions("s10-b"), None);
        store_instructions("s10-a", &json!({ "instructions": "  \n" }));
        assert_eq!(stored_instructions("s10-a"), None);
    }

    #[test]
    fn test11_a_switch_is_on_only_for_a_true_boolean() {
        let fields = map(json!({ DECLARE_KEY: true, FORWARD_KEY: "yes" }));
        assert!(switched_on(&fields, DECLARE_KEY));
        assert!(!switched_on(&fields, FORWARD_KEY));
        assert!(!switched_on(&fields, "absent"));
    }
}
