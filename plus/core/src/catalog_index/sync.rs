use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::Value;
use toolport_plus_api::{Host, HttpRequest};

use super::store::{self, Part, Row, State, StateError};
use crate::op::OpError;
use crate::tasks::cron::rfc3339;
use crate::tasks::store::lock;

pub const REGISTRY_URL: &str = "https://registry.modelcontextprotocol.io/v0.1/servers";
const PAGE_SIZE: u32 = 100;
/// A first sync lists well over a hundred pages; a flush every tenth keeps a stopped pass
/// resumable without writing after each one.
const FLUSH_EVERY_PAGES: u32 = 10;
pub(super) const CAPPED: &str = "the registry listed more than";
const MAX_PAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct Tuning {
    pub page_timeout: Duration,
    pub retry_pause: Duration,
    pub max_pages: u32,
}

pub(super) const TUNING: Tuning = Tuning {
    page_timeout: Duration::from_secs(60),
    retry_pause: Duration::from_secs(5),
    max_pages: 2_000,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Resume an unfinished pass, else fetch what changed since the last one, else everything.
    Incremental,
    /// Start a new pass over the whole registry, dropping what it no longer lists.
    Full,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub kind: &'static str,
    pub pages: u32,
    pub fetched: usize,
    pub count: usize,
    pub complete: bool,
    pub last_synced_at: Option<String>,
}

struct Plan {
    full: bool,
    cursor: Option<String>,
    since: Option<String>,
    started_at: String,
    pages: u32,
    working: BTreeMap<String, Box<RawValue>>,
}

fn rows_into(working: &mut BTreeMap<String, Box<RawValue>>, rows: Vec<Row>) {
    for row in rows {
        working.insert(row.id, row.entry);
    }
}

fn plan(dir: &Path, mode: Mode, now: &str) -> Plan {
    let main = store::read_main(dir);
    let parts = store::read_parts(dir);
    let fresh = || {
        store::clear_parts(dir);
        Plan {
            full: true,
            cursor: None,
            since: None,
            started_at: now.to_string(),
            pages: 0,
            working: BTreeMap::new(),
        }
    };
    if mode == Mode::Full || parts.len() != store::part_count(dir) {
        return fresh();
    }
    if let Some(last) = parts.last() {
        let mut working = BTreeMap::new();
        if !last.full {
            rows_into(&mut working, main.entries);
        }
        let (full, cursor, since, started_at, pages) = (
            last.full,
            Some(last.next_cursor.clone()),
            last.since.clone(),
            last.started_at.clone(),
            last.pages,
        );
        for part in parts {
            rows_into(&mut working, part.entries);
            for id in part.retired {
                working.remove(&id);
            }
        }
        return Plan {
            full,
            cursor,
            since,
            started_at,
            pages,
            working,
        };
    }
    match main.last_synced_at {
        Some(since) if !main.entries.is_empty() => {
            let mut working = BTreeMap::new();
            rows_into(&mut working, main.entries);
            Plan {
                full: false,
                cursor: None,
                since: Some(since),
                started_at: now.to_string(),
                pages: 0,
                working,
            }
        }
        _ => fresh(),
    }
}

fn page_url(cursor: Option<&str>, since: Option<&str>) -> String {
    let mut url = format!("{REGISTRY_URL}?limit={PAGE_SIZE}&version=latest");
    for (name, value) in [("cursor", cursor), ("updated_since", since)] {
        if let Some(value) = value {
            url.push_str(&format!("&{name}={}", encode(value)));
        }
    }
    url
}

fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn registry_error(message: impl Into<String>) -> OpError {
    OpError::failed("registry", message)
}

fn fetch(host: &dyn Host, url: &str, tuning: Tuning) -> Result<Value, OpError> {
    let attempt = || {
        let request = HttpRequest::get(url)
            .header("Accept", "application/json")
            .timeout(tuning.page_timeout)
            .max_body_bytes(MAX_PAGE_BYTES);
        let response = host
            .send(&request)
            .map_err(|error| registry_error(format!("the MCP Registry did not answer: {error}")))?;
        if !response.is_success() {
            return Err(registry_error(format!(
                "the MCP Registry answered {}",
                response.status
            )));
        }
        response
            .json()
            .ok_or_else(|| registry_error("the MCP Registry sent a page that is not JSON"))
    };
    attempt().or_else(|_| {
        std::thread::sleep(tuning.retry_pause);
        attempt()
    })
}

fn next_cursor(body: &Value) -> Option<String> {
    let metadata = &body["metadata"];
    metadata["nextCursor"]
        .as_str()
        .or_else(|| metadata["next_cursor"].as_str())
        .filter(|cursor| !cursor.is_empty())
        .map(String::from)
}

fn remember_state(dir: &Path, attempt: &str, error: Option<StateError>) {
    let _ = store::write_state(
        dir,
        &State {
            last_attempt_at: Some(attempt.to_string()),
            error,
        },
    );
}

pub(super) fn sync_with(
    host: &dyn Host,
    mode: Mode,
    tuning: Tuning,
    progress: &mut dyn FnMut(&Report),
) -> Result<Report, OpError> {
    let data_dir = host
        .data_dir()
        .ok_or_else(|| OpError::failed("data_dir", "the data directory could not be resolved"))?;
    let dir = store::plus_dir(&data_dir);
    let _lock = lock(&dir.join(store::LOCK_FILE), Duration::ZERO)
        .ok_or_else(|| OpError::conflict("a registry sync is already running"))?;
    let now = rfc3339(host.now_unix());
    remember_state(&dir, &now, store::read_state(&dir).error);
    let result = run(host, &dir, mode, tuning, &now, progress);
    let error = result.as_ref().err().map(|error| StateError {
        kind: error.code().to_string(),
        message: error.message.clone(),
    });
    remember_state(&dir, &now, error);
    result
}

struct Pass<'a> {
    dir: &'a Path,
    full: bool,
    since: Option<String>,
    started_at: String,
    part_number: usize,
    pending: Vec<Row>,
    retired: Vec<String>,
}

impl Pass<'_> {
    fn flush(&mut self, next_cursor: &str, pages: u32) -> Result<(), OpError> {
        if self.pending.is_empty() && self.retired.is_empty() {
            return Ok(());
        }
        let mut part = Part::new(
            self.full,
            self.started_at.clone(),
            self.since.clone(),
            next_cursor.to_string(),
            pages,
        );
        part.entries = std::mem::take(&mut self.pending);
        part.retired = std::mem::take(&mut self.retired);
        self.part_number += 1;
        store::write_part(self.dir, self.part_number, &part)
    }
}

fn run(
    host: &dyn Host,
    dir: &Path,
    mode: Mode,
    tuning: Tuning,
    now: &str,
    progress: &mut dyn FnMut(&Report),
) -> Result<Report, OpError> {
    let Plan {
        full,
        mut cursor,
        since,
        started_at,
        mut pages,
        mut working,
    } = plan(dir, mode, now);
    let mut pass = Pass {
        dir,
        full,
        since: since.clone(),
        started_at: started_at.clone(),
        part_number: store::part_count(dir),
        pending: Vec::new(),
        retired: Vec::new(),
    };
    let kind = match (cursor.is_some(), full) {
        (true, _) => "resume",
        (false, true) => "full",
        (false, false) => "incremental",
    };
    let mut report = Report {
        kind,
        count: working.len(),
        ..Report::default()
    };
    loop {
        let stop = |pass: &mut Pass, cursor: &Option<String>, pages: u32, error: OpError| {
            if let Some(cursor) = cursor {
                let _ = pass.flush(cursor, pages);
            }
            error
        };
        if report.pages >= tuning.max_pages {
            let error = registry_error(format!("{CAPPED} {} pages", tuning.max_pages));
            return Err(stop(&mut pass, &cursor, pages, error));
        }
        let body = match fetch(host, &page_url(cursor.as_deref(), since.as_deref()), tuning) {
            Ok(body) => body,
            Err(error) => return Err(stop(&mut pass, &cursor, pages, error)),
        };
        let next = next_cursor(&body);
        if next.is_some() && next == cursor {
            let error = registry_error("the registry repeated a page cursor");
            return Err(stop(&mut pass, &cursor, pages, error));
        }
        let page = host.registry_entries(&body);
        pages += 1;
        report.pages += 1;
        report.fetched += page.entries.len() + page.retired.len();
        for id in page.retired {
            let id = id.to_lowercase();
            working.remove(&id);
            pass.retired.push(id);
        }
        for (id, record) in page.entries {
            let Some(entry) = store::raw(&record) else {
                continue;
            };
            let id = id.to_lowercase();
            working.insert(id.clone(), entry.clone());
            pass.pending.push(Row { id, entry });
        }
        report.count = working.len();
        let Some(next) = next else {
            break;
        };
        if pages % FLUSH_EVERY_PAGES == 0 {
            pass.flush(&next, pages)?;
        }
        cursor = Some(next);
        progress(&report);
    }
    let entries = working
        .into_iter()
        .map(|(id, entry)| Row { id, entry })
        .collect();
    store::write_main(dir, &started_at, entries)?;
    store::clear_parts(dir);
    if full {
        store::forget_live(dir);
    }
    report.complete = true;
    report.last_synced_at = Some(started_at);
    progress(&report);
    Ok(report)
}
