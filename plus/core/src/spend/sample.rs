//! Events reduced to per-request samples under the source rule: for one session, token and cost
//! metrics win; a session that only has `api_request` log events is counted from those.

use std::collections::{BTreeMap, BTreeSet};

use super::event::*;
use super::prices;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Otel,
    Transcript,
}

#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub ts: i64,
    pub session: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub cost: f64,
    pub cost_estimated: f64,
    pub requests: u64,
    pub transcript: bool,
}

impl Sample {
    pub fn tokens(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_creation
    }
}

fn session_key(event: &Event) -> String {
    event.session.clone().unwrap_or_default()
}

pub fn from_events(events: &[Event]) -> Vec<Sample> {
    let mut metric_sessions: BTreeSet<String> = BTreeSet::new();
    let mut cost_sessions: BTreeSet<String> = BTreeSet::new();
    for event in events {
        match event.kind.as_str() {
            KIND_TOKENS => {
                metric_sessions.insert(session_key(event));
            }
            KIND_COST => {
                cost_sessions.insert(session_key(event));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let mut estimated_from: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for event in events {
        let key = session_key(event);
        let mut sample = Sample {
            ts: event.ts,
            session: event.session.clone(),
            project: event.project.clone(),
            model: event.model.clone(),
            ..Sample::default()
        };
        match event.kind.as_str() {
            KIND_TOKENS => {
                let n = event.value as u64;
                match event.token_type.as_deref() {
                    Some(TOKEN_INPUT) => sample.input = n,
                    Some(TOKEN_OUTPUT) => sample.output = n,
                    Some(TOKEN_CACHE_READ) => sample.cache_read = n,
                    Some(TOKEN_CACHE_CREATION) => sample.cache_creation = n,
                    _ => continue,
                }
                if !cost_sessions.contains(&key) {
                    estimated_from.entry(key).or_default().push(out.len());
                }
            }
            KIND_COST => sample.cost = event.value,
            KIND_REQUEST if !metric_sessions.contains(&key) => {
                sample.input = event.input.unwrap_or(0);
                sample.output = event.output.unwrap_or(0);
                sample.cache_read = event.cache_read.unwrap_or(0);
                sample.cache_creation = event.cache_creation.unwrap_or(0);
                sample.cost = if cost_sessions.contains(&key) {
                    0.0
                } else {
                    event.value
                };
                sample.requests = 1;
            }
            KIND_REQUEST => sample.requests = 1,
            _ => continue,
        }
        out.push(sample);
    }
    for indexes in estimated_from.values() {
        for &i in indexes {
            let sample = &mut out[i];
            sample.cost_estimated = prices::cost(
                sample.model.as_deref(),
                sample.input,
                sample.output,
                sample.cache_read,
                sample.cache_creation,
            );
        }
    }
    out
}
