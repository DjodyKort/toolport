//! The `resource_link` URIs the gateway has handed to one session.
//!
//! A tool result can link to a resource the server never lists and no listed
//! template covers, and the client then asks the gateway to read it. The router
//! only knows listed and templated URIs, so the read had no owner. The gateway
//! remembers the links it relayed, per session, and sends a read of such a URI
//! back to the server that returned the link.

use serde_json::Value;
use std::collections::HashMap;

/// Links kept per session. The oldest goes when a new one arrives past the cap.
pub const MAX_SEEN_LINKS: usize = 256;

/// A link longer than this is not remembered, so one result cannot fill the
/// memory with a few huge strings.
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
    /// Remember that `server` returned a link to `uri`. The first server to link a
    /// URI keeps it while it is remembered, as the first server to list a resource
    /// keeps that resource: a later link to the same URI from another server must
    /// not move the read.
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

    /// The server that returned a link to `uri`, if this session was given one.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_remembered_link_names_the_server_that_returned_it() {
        let mut seen = SeenLinks::default();
        seen.remember("odh", "odh://dyn/42");
        assert_eq!(seen.owner("odh://dyn/42"), Some("odh"));
        assert_eq!(seen.owner("odh://dyn/43"), None);
    }

    #[test]
    fn the_first_server_to_link_a_uri_keeps_it() {
        let mut seen = SeenLinks::default();
        seen.remember("first", "shared://x");
        seen.remember("second", "shared://x");
        assert_eq!(seen.owner("shared://x"), Some("first"));
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn memory_is_bounded_and_the_oldest_link_goes_first() {
        let mut seen = SeenLinks::default();
        for i in 0..MAX_SEEN_LINKS + 10 {
            seen.remember("s", &format!("s://item/{i}"));
        }
        assert_eq!(seen.len(), MAX_SEEN_LINKS);
        assert_eq!(seen.owner("s://item/0"), None);
        assert_eq!(seen.owner("s://item/9"), None);
        assert_eq!(seen.owner("s://item/10"), Some("s"));
        assert_eq!(
            seen.owner(&format!("s://item/{}", MAX_SEEN_LINKS + 9)),
            Some("s")
        );
    }

    #[test]
    fn seeing_a_link_again_keeps_it_from_being_the_next_one_evicted() {
        let mut seen = SeenLinks::default();
        for i in 0..MAX_SEEN_LINKS {
            seen.remember("s", &format!("s://item/{i}"));
        }
        seen.remember("s", "s://item/0");
        seen.remember("s", "s://item/new");
        assert_eq!(seen.owner("s://item/0"), Some("s"));
        assert_eq!(seen.owner("s://item/1"), None);
    }

    #[test]
    fn an_empty_or_oversized_uri_is_not_remembered() {
        let mut seen = SeenLinks::default();
        seen.remember("s", "");
        seen.remember("s", &format!("s://{}", "x".repeat(MAX_LINK_URI_BYTES)));
        assert!(seen.is_empty());
        let longest = format!("s://{}", "x".repeat(MAX_LINK_URI_BYTES - 4));
        seen.remember("s", &longest);
        assert_eq!(seen.owner(&longest), Some("s"));
    }

    #[test]
    fn only_resource_link_blocks_with_a_uri_are_collected() {
        let result = json!({
            "content": [
                { "type": "text", "text": "odh://not/a/link" },
                { "type": "resource_link", "uri": "odh://a", "name": "a" },
                { "type": "resource", "resource": { "uri": "odh://embedded", "text": "t" } },
                { "type": "resource_link", "name": "no uri" },
                { "type": "resource_link", "uri": 7 },
                { "type": "resource_link", "uri": "odh://b" }
            ],
            "structuredContent": { "uri": "odh://structured" }
        });
        assert_eq!(
            link_uris(&result).collect::<Vec<_>>(),
            vec!["odh://a", "odh://b"]
        );
        assert_eq!(link_uris(&json!({ "isError": false })).count(), 0);
        assert_eq!(link_uris(&json!({ "content": "text" })).count(), 0);
    }
}
