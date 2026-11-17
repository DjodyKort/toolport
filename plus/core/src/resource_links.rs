//! The `resource_link` URIs the gateway has relayed to its clients.
//!
//! A tool result can link to a resource the server never lists and no listed template covers.
//! The router only knows listed and templated URIs, so a client's read of such a link had no
//! owner. The gateway remembers which server returned each link and routes a read of the URI back
//! to it.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use serde_json::Value;

/// Links kept. The oldest goes when a new one arrives past the cap.
pub const MAX_SEEN_LINKS: usize = 256;

/// A link longer than this is not remembered, so one result cannot fill the memory with a few
/// huge strings.
pub const MAX_LINK_URI_BYTES: usize = 2048;

#[derive(Default)]
pub struct SeenLinks {
    next_seq: u64,
    seen: HashMap<String, Seen>,
}

struct Seen {
    server: String,
    seq: u64,
}

impl SeenLinks {
    /// Remembers that `server` returned a link to `uri`. The first server to link a URI keeps it
    /// while it is remembered, as the first server to list a resource keeps that resource.
    pub fn remember(&mut self, server: &str, uri: &str) {
        if uri.is_empty() || uri.len() > MAX_LINK_URI_BYTES {
            return;
        }
        self.next_seq += 1;
        let seq = self.next_seq;
        if let Some(seen) = self.seen.get_mut(uri) {
            if seen.server == server {
                seen.seq = seq;
            }
            return;
        }
        if self.seen.len() >= MAX_SEEN_LINKS {
            let oldest = self
                .seen
                .iter()
                .min_by_key(|(_, seen)| seen.seq)
                .map(|(uri, _)| uri.clone());
            if let Some(oldest) = oldest {
                self.seen.remove(&oldest);
            }
        }
        self.seen.insert(
            uri.to_string(),
            Seen {
                server: server.to_string(),
                seq,
            },
        );
    }

    pub fn owner(&self, uri: &str) -> Option<&str> {
        self.seen.get(uri).map(|seen| seen.server.as_str())
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

/// The URIs of the `resource_link` blocks in a tool result's `content`.
pub fn link_uris(result: &Value) -> impl Iterator<Item = &str> {
    result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("resource_link"))
        .filter_map(|block| block.get("uri").and_then(Value::as_str))
}

static SEEN: Mutex<Option<SeenLinks>> = Mutex::new(None);

/// Remembers the links of a tool result that `server` returned.
pub fn remember_result(server: &str, result: &Value) {
    let mut links = link_uris(result).peekable();
    if links.peek().is_none() {
        return;
    }
    let mut guard = SEEN.lock().unwrap_or_else(PoisonError::into_inner);
    let seen = guard.get_or_insert_with(SeenLinks::default);
    for uri in links {
        seen.remember(server, uri);
    }
}

/// The server that returned a link to `uri`.
pub fn owner_of(uri: &str) -> Option<String> {
    SEEN.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()?
        .owner(uri)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test01_a_remembered_link_names_the_server_that_returned_it() {
        let mut seen = SeenLinks::default();
        seen.remember("odh", "odh://dyn/42");
        assert_eq!(seen.owner("odh://dyn/42"), Some("odh"));
        assert_eq!(seen.owner("odh://dyn/43"), None);
    }

    #[test]
    fn test02_the_first_server_to_link_a_uri_keeps_it() {
        let mut seen = SeenLinks::default();
        seen.remember("first", "shared://x");
        seen.remember("second", "shared://x");
        assert_eq!(seen.owner("shared://x"), Some("first"));
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn test03_memory_is_bounded_and_the_oldest_link_goes_first() {
        let mut seen = SeenLinks::default();
        for i in 0..MAX_SEEN_LINKS + 10 {
            seen.remember("s", &format!("s://item/{i}"));
        }
        assert_eq!(seen.len(), MAX_SEEN_LINKS);
        assert_eq!(seen.owner("s://item/9"), None);
        assert_eq!(seen.owner("s://item/10"), Some("s"));
        assert_eq!(
            seen.owner(&format!("s://item/{}", MAX_SEEN_LINKS + 9)),
            Some("s")
        );
    }

    #[test]
    fn test04_a_link_the_same_server_returns_again_is_kept_longer() {
        let mut seen = SeenLinks::default();
        seen.remember("s", "s://keep");
        for i in 0..MAX_SEEN_LINKS - 1 {
            seen.remember("s", &format!("s://item/{i}"));
        }
        seen.remember("s", "s://keep");
        seen.remember("s", "s://newest");
        assert_eq!(seen.owner("s://keep"), Some("s"));
        assert_eq!(seen.owner("s://item/0"), None);
    }

    #[test]
    fn test05_empty_and_oversized_uris_are_not_remembered() {
        let mut seen = SeenLinks::default();
        seen.remember("s", "");
        seen.remember("s", &"u".repeat(MAX_LINK_URI_BYTES + 1));
        assert!(seen.is_empty());
        seen.remember("s", &"u".repeat(MAX_LINK_URI_BYTES));
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn test06_only_resource_link_blocks_of_a_result_are_read() {
        let result = json!({ "content": [
            { "type": "text", "text": "odh://not-a-link" },
            { "type": "resource_link", "uri": "odh://a", "name": "a" },
            { "type": "resource", "resource": { "uri": "odh://embedded" } },
            { "type": "resource_link", "name": "no uri" },
            { "type": "resource_link", "uri": "odh://b" }
        ]});
        assert_eq!(
            link_uris(&result).collect::<Vec<_>>(),
            ["odh://a", "odh://b"]
        );
        assert_eq!(link_uris(&json!({})).count(), 0);
        assert_eq!(link_uris(&json!({ "content": "text" })).count(), 0);
    }

    #[test]
    fn test07_a_result_is_remembered_for_the_server_that_returned_it() {
        remember_result(
            "s7-server",
            &json!({ "content": [{ "type": "resource_link", "uri": "s7://dyn/1" }] }),
        );
        assert_eq!(owner_of("s7://dyn/1").as_deref(), Some("s7-server"));
        assert_eq!(owner_of("s7://dyn/2"), None);
    }
}
