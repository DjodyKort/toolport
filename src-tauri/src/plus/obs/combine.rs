//! Joins OTel `api_request` events with the transcript index so one API request counts once.

use super::store::{day_from_ms, iso_from_ms, iso_to_ms, Event, MsgRecord};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

const MATCH_WINDOW_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub request_id: String,
    pub session: String,
    pub model: String,
    pub ts_ms: i64,
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

fn attr_text<'a>(e: &'a Event, key: &str) -> &'a str {
    e.attrs.get(key).and_then(Value::as_str).unwrap_or("")
}

fn attr_tokens(e: &Event, key: &str) -> Option<u64> {
    let v = e.attrs.get(key)?;
    v.as_u64()
        .or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
}

/// An `api_request` that carries no token attribute at all says nothing about usage.
pub fn request_of(e: &Event) -> Option<Request> {
    if e.kind != "api_request" {
        return None;
    }
    let tokens = [
        "input_tokens",
        "output_tokens",
        "cache_creation_tokens",
        "cache_read_tokens",
    ]
    .map(|key| attr_tokens(e, key));
    if tokens.iter().all(Option::is_none) {
        return None;
    }
    let [input, output, cache_creation, cache_read] = tokens.map(Option::unwrap_or_default);
    Some(Request {
        request_id: attr_text(e, "request_id").to_string(),
        session: attr_text(e, "session.id").to_string(),
        model: attr_text(e, "model").to_string(),
        ts_ms: e.ts_ms,
        input,
        output,
        cache_creation,
        cache_read,
    })
}

type Shape<'a> = (&'a str, &'a str, u64, u64, u64, u64);

struct Slot {
    ts_ms: i64,
    has_id: bool,
    used: bool,
}

fn shapes(messages: &BTreeMap<String, MsgRecord>) -> HashMap<Shape<'_>, Vec<Slot>> {
    let mut out: HashMap<Shape<'_>, Vec<Slot>> = HashMap::new();
    for r in messages.values() {
        let Some(ts_ms) = iso_to_ms(&r.ts) else {
            continue;
        };
        out.entry((
            &r.session,
            &r.model,
            r.input,
            r.output,
            r.cache_creation,
            r.cache_read,
        ))
        .or_default()
        .push(Slot {
            ts_ms,
            has_id: !r.request_id.is_empty(),
            used: false,
        });
    }
    out
}

/// OTel requests the transcripts do not already hold.
///
/// A request matches a transcript message by request id. When either side lacks the id (older
/// Claude Code builds), it matches one still-unclaimed message of the same session, model and
/// token counts within [`MATCH_WINDOW_MS`]; each message is claimed once.
pub fn only_in_otel(messages: &BTreeMap<String, MsgRecord>, requests: &[Request]) -> Vec<MsgRecord> {
    let known: HashSet<&str> = messages
        .values()
        .map(|r| r.request_id.as_str())
        .filter(|id| !id.is_empty())
        .collect();
    let mut by_shape: Option<HashMap<Shape<'_>, Vec<Slot>>> = None;
    let mut only = Vec::new();
    for r in requests {
        if !r.request_id.is_empty() && known.contains(r.request_id.as_str()) {
            continue;
        }
        let slots = by_shape.get_or_insert_with(|| shapes(messages));
        let key = (
            r.session.as_str(),
            r.model.as_str(),
            r.input,
            r.output,
            r.cache_creation,
            r.cache_read,
        );
        let claimed = slots.get_mut(&key).and_then(|list| {
            list.iter_mut().find(|s| {
                !s.used
                    && (r.request_id.is_empty() || !s.has_id)
                    && (s.ts_ms - r.ts_ms).abs() <= MATCH_WINDOW_MS
            })
        });
        if let Some(slot) = claimed {
            slot.used = true;
            continue;
        }
        only.push(MsgRecord {
            session: r.session.clone(),
            model: r.model.clone(),
            ts: iso_from_ms(r.ts_ms),
            day: day_from_ms(r.ts_ms),
            input: r.input,
            output: r.output,
            cache_creation: r.cache_creation,
            cache_read: r.cache_read,
            request_id: r.request_id.clone(),
            ..MsgRecord::default()
        });
    }
    only
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(session: &str, request_id: &str, ts: &str, tokens: (u64, u64, u64, u64)) -> MsgRecord {
        MsgRecord {
            session: session.into(),
            model: "m".into(),
            ts: ts.into(),
            input: tokens.0,
            output: tokens.1,
            cache_creation: tokens.2,
            cache_read: tokens.3,
            request_id: request_id.into(),
            ..MsgRecord::default()
        }
    }

    fn req(session: &str, request_id: &str, ts: &str, tokens: (u64, u64, u64, u64)) -> Request {
        Request {
            request_id: request_id.into(),
            session: session.into(),
            model: "m".into(),
            ts_ms: iso_to_ms(ts).unwrap(),
            input: tokens.0,
            output: tokens.1,
            cache_creation: tokens.2,
            cache_read: tokens.3,
        }
    }

    fn index(list: Vec<(&str, MsgRecord)>) -> BTreeMap<String, MsgRecord> {
        list.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    #[test]
    fn a_request_id_match_is_a_duplicate() {
        let messages = index(vec![("msg_1", msg("s", "req_1", "2026-10-01T10:00:00Z", (1, 2, 3, 4)))]);
        let requests = [
            req("s", "req_1", "2026-10-01T10:00:05Z", (1, 2, 3, 4)),
            req("s", "req_2", "2026-10-01T10:01:00Z", (5, 6, 7, 8)),
        ];
        let only = only_in_otel(&messages, &requests);
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].request_id, "req_2");
        assert_eq!((only[0].input, only[0].cache_read), (5, 8));
        assert_eq!(only[0].day, "2026-10-01");
    }

    #[test]
    fn two_requests_with_other_ids_are_never_merged_by_token_shape() {
        let messages = index(vec![("msg_1", msg("s", "req_1", "2026-10-01T10:00:00Z", (1, 2, 3, 4)))]);
        let requests = [req("s", "req_9", "2026-10-01T10:00:01Z", (1, 2, 3, 4))];
        assert_eq!(only_in_otel(&messages, &requests).len(), 1);
    }

    #[test]
    fn without_ids_one_message_is_claimed_once_inside_the_window() {
        let messages = index(vec![
            ("a", msg("s", "req_1", "2026-10-01T10:00:00Z", (1, 2, 3, 4))),
            ("b", msg("s", "", "2026-10-01T10:30:00Z", (1, 2, 3, 4))),
        ]);
        let requests = [
            req("s", "", "2026-10-01T10:00:09Z", (1, 2, 3, 4)),
            req("s", "", "2026-10-01T10:00:10Z", (1, 2, 3, 4)),
            req("s", "", "2026-10-01T10:30:02Z", (1, 2, 3, 4)),
            req("other", "", "2026-10-01T10:00:09Z", (1, 2, 3, 4)),
        ];
        let only = only_in_otel(&messages, &requests);
        assert_eq!(only.len(), 2, "{only:?}");
        assert_eq!(only[0].ts, "2026-10-01T10:00:10Z");
        assert_eq!(only[1].session, "other");
    }

    #[test]
    fn a_request_far_from_the_message_is_a_different_request() {
        let messages = index(vec![("a", msg("s", "", "2026-10-01T10:00:00Z", (1, 2, 3, 4)))]);
        let requests = [req("s", "", "2026-10-01T11:00:00Z", (1, 2, 3, 4))];
        assert_eq!(only_in_otel(&messages, &requests).len(), 1);
    }

    #[test]
    fn request_of_needs_a_token_attribute() {
        let event = |attrs: Value| Event {
            kind: "api_request".into(),
            ts_ms: 5,
            attrs: serde_json::from_value(attrs).unwrap(),
            ..Event::default()
        };
        assert_eq!(request_of(&event(json!({"model": "m"}))), None);
        let got = request_of(&event(json!({
            "request_id": "req_1", "session.id": "s", "model": "m",
            "input_tokens": 7, "output_tokens": 2.0, "cache_read_tokens": 9
        })))
        .unwrap();
        assert_eq!((got.input, got.output, got.cache_read, got.cache_creation), (7, 2, 9, 0));
        assert_eq!((got.request_id.as_str(), got.session.as_str()), ("req_1", "s"));
        let other = Event {
            kind: "cost".into(),
            ..Event::default()
        };
        assert_eq!(request_of(&other), None);
    }
}
