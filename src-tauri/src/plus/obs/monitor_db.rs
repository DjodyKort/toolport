use super::store::{day_from_iso, Locked};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub fn default_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config").join("mcpm").join("monitor.db"))
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Agg {
    calls: u64,
    failures: u64,
    duration_ms: u64,
    request_bytes: u64,
    response_bytes: u64,
}

impl Agg {
    fn add(&mut self, success: bool, dur: i64, req: i64, resp: i64) {
        self.calls += 1;
        if !success {
            self.failures += 1;
        }
        self.duration_ms += dur.max(0) as u64;
        self.request_bytes += req.max(0) as u64;
        self.response_bytes += resp.max(0) as u64;
    }
}

struct Row {
    event_type: String,
    server: String,
    session: String,
    client: String,
    ts: String,
    duration: i64,
    req: i64,
    resp: i64,
    success: bool,
}

fn for_each_row(path: &Path, mut each: impl FnMut(Row)) -> Result<(), String> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut stmt = conn
        .prepare(
            "SELECT COALESCE(event_type,''), COALESCE(server_id,''), COALESCE(session_id,''), \
             COALESCE(client_id,''), COALESCE(CAST(timestamp AS TEXT),''), COALESCE(duration_ms,0), \
             COALESCE(request_size,0), COALESCE(response_size,0), COALESCE(success,1) \
             FROM monitor_events ORDER BY id",
        )
        .map_err(|e| format!("not a monitor.db: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Row {
                event_type: r.get(0)?,
                server: r.get(1)?,
                session: r.get(2)?,
                client: r.get(3)?,
                ts: r.get(4)?,
                duration: r.get(5)?,
                req: r.get(6)?,
                resp: r.get(7)?,
                success: r.get::<_, i64>(8)? != 0,
            })
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        each(row.map_err(|e| e.to_string())?);
    }
    Ok(())
}

pub fn aggregate(path: &Path) -> Result<Value, String> {
    let mut total = Agg::default();
    let mut by_day: BTreeMap<String, Agg> = BTreeMap::new();
    let mut by_server: BTreeMap<String, Agg> = BTreeMap::new();
    let mut by_type: BTreeMap<String, Agg> = BTreeMap::new();
    let mut by_client: BTreeMap<String, Agg> = BTreeMap::new();
    let mut sessions: BTreeSet<String> = BTreeSet::new();
    let (mut first, mut last) = (String::new(), String::new());
    let mut skipped = 0u64;
    let mut row_count = 0usize;

    for_each_row(path, |r| {
        row_count += 1;
        let Some(day) = day_from_iso(&r.ts) else {
            skipped += 1;
            return;
        };
        if first.is_empty() || r.ts < first {
            first = r.ts.clone();
        }
        if r.ts > last {
            last = r.ts.clone();
        }
        if !r.session.is_empty() {
            sessions.insert(r.session.clone());
        }
        total.add(r.success, r.duration, r.req, r.resp);
        by_day
            .entry(day)
            .or_default()
            .add(r.success, r.duration, r.req, r.resp);
        let non_empty = |s: &str| {
            if s.is_empty() {
                "(none)".to_string()
            } else {
                s.to_string()
            }
        };
        by_type
            .entry(non_empty(&r.event_type))
            .or_default()
            .add(r.success, r.duration, r.req, r.resp);
        if !r.server.is_empty() {
            by_server
                .entry(r.server.clone())
                .or_default()
                .add(r.success, r.duration, r.req, r.resp);
        }
        if !r.client.is_empty() {
            by_client
                .entry(r.client.clone())
                .or_default()
                .add(r.success, r.duration, r.req, r.resp);
        }
    })?;

    Ok(json!({
        "source": path.to_string_lossy(),
        "rows": row_count,
        "skippedRows": skipped,
        "sessions": sessions.len(),
        "firstTs": first,
        "lastTs": last,
        "totals": total,
        "byDay": by_day,
        "byServer": by_server,
        "byEventType": by_type,
        "byClient": by_client,
    }))
}

pub fn import(lock: &Locked, path: &Path) -> Result<Value, String> {
    if !path.is_file() {
        return Err(format!("monitor.db not found: {}", path.display()));
    }
    let history = aggregate(path)?;
    lock.save_history(&history)?;
    Ok(json!({
        "source": history["source"],
        "rows": history["rows"],
        "skippedRows": history["skippedRows"],
        "sessions": history["sessions"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("obs-mon-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn make_db(path: &Path) {
        let c = Connection::open(path).unwrap();
        c.execute_batch(
            "CREATE TABLE monitor_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT, event_type TEXT, server_id TEXT,
                resource_id TEXT, session_id TEXT, client_id TEXT, timestamp DATETIME,
                duration_ms INTEGER, request_size INTEGER, response_size INTEGER,
                success BOOLEAN, error_message TEXT, metadata TEXT, raw_request TEXT,
                raw_response TEXT);
             CREATE VIEW access_events AS SELECT * FROM monitor_events;
             INSERT INTO monitor_events (event_type,server_id,resource_id,session_id,client_id,timestamp,duration_ms,request_size,response_size,success)
              VALUES ('TOOL_INVOCATION','github','list','s1','claude-code','2026-09-01T10:00:00',100,10,200,1),
                     ('TOOL_INVOCATION','github','get','s1','claude-code','2026-09-01T10:00:05.123456',50,5,20,0),
                     ('RESOURCE_ACCESS','figma','file','s2','cursor','2026-09-02T08:00:00',30,1,2,1),
                     ('SESSION_START','','','s2',NULL,'2026-09-02T07:59:00',NULL,NULL,NULL,1),
                     ('TOOL_INVOCATION','github','x','s3','claude-code','not-a-date',1,1,1,1);",
        )
        .unwrap();
    }

    #[test]
    fn fixture_import_aggregates() {
        let base = scratch("agg");
        let db = base.join("monitor.db");
        make_db(&db);
        let got = aggregate(&db).unwrap();
        let agg = |c, f, d, rq, rs| json!({"calls": c, "failures": f, "durationMs": d, "requestBytes": rq, "responseBytes": rs});
        assert_eq!(got["rows"], 5);
        assert_eq!(got["skippedRows"], 1);
        assert_eq!(got["sessions"], 2);
        assert_eq!(got["firstTs"], "2026-09-01T10:00:00");
        assert_eq!(got["lastTs"], "2026-09-02T08:00:00");
        assert_eq!(got["totals"], agg(4, 1, 180, 16, 222));
        assert_eq!(
            got["byDay"],
            json!({"2026-09-01": agg(2, 1, 150, 15, 220), "2026-09-02": agg(2, 0, 30, 1, 2)})
        );
        assert_eq!(
            got["byServer"],
            json!({"figma": agg(1, 0, 30, 1, 2), "github": agg(2, 1, 150, 15, 220)})
        );
        assert_eq!(
            got["byEventType"],
            json!({
                "RESOURCE_ACCESS": agg(1, 0, 30, 1, 2),
                "SESSION_START": agg(1, 0, 0, 0, 0),
                "TOOL_INVOCATION": agg(2, 1, 150, 15, 220)
            })
        );
        assert_eq!(
            got["byClient"],
            json!({"claude-code": agg(2, 1, 150, 15, 220), "cursor": agg(1, 0, 30, 1, 2)})
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_large_db_aggregates_every_row_like_an_independent_tally() {
        let base = scratch("large");
        let db = base.join("monitor.db");
        let c = Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE monitor_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT, event_type TEXT, server_id TEXT,
                resource_id TEXT, session_id TEXT, client_id TEXT, timestamp DATETIME,
                duration_ms INTEGER, request_size INTEGER, response_size INTEGER,
                success BOOLEAN);
             BEGIN;",
        )
        .unwrap();
        let mut insert = c
            .prepare(
                "INSERT INTO monitor_events (event_type,server_id,session_id,client_id,timestamp,duration_ms,request_size,response_size,success)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            )
            .unwrap();
        let mut want: BTreeMap<String, [u64; 5]> = BTreeMap::new();
        let (mut calls, mut failures, mut skipped) = (0u64, 0u64, 0u64);
        let (mut first, mut last) = (String::from("~"), String::new());
        let n = 30_000u64;
        for i in 0..n {
            let day = 1 + i % 28;
            let ts = if i % 1000 == 999 {
                "garbage".to_string()
            } else {
                format!("2026-09-{day:02}T10:{:02}:{:02}", i % 60, (i / 60) % 60)
            };
            let kind = if i % 2 == 0 { "TOOL_INVOCATION" } else { "RESOURCE_ACCESS" };
            let client = (i % 3 != 0).then(|| format!("c{}", i % 3));
            let ok = i % 5 != 0;
            insert
                .execute(rusqlite::params![
                    kind,
                    format!("s{}", i % 7),
                    format!("sess{}", i % 50),
                    client,
                    ts,
                    (i % 100) as i64,
                    (i % 10) as i64,
                    (i % 20) as i64,
                    ok
                ])
                .unwrap();
            if ts == "garbage" {
                skipped += 1;
                continue;
            }
            calls += 1;
            failures += u64::from(!ok);
            first = first.min(ts.clone());
            last = last.max(ts.clone());
            let slot = want.entry(format!("2026-09-{day:02}")).or_default();
            slot[0] += 1;
            slot[1] += u64::from(!ok);
            slot[2] += i % 100;
            slot[3] += i % 10;
            slot[4] += i % 20;
        }
        drop(insert);
        c.execute_batch("COMMIT").unwrap();
        drop(c);

        let got = aggregate(&db).unwrap();
        let tally = |v: &Value| {
            ["calls", "failures", "durationMs", "requestBytes", "responseBytes"]
                .map(|key| v[key].as_u64().unwrap())
        };
        assert_eq!(got["rows"], n);
        assert_eq!(got["skippedRows"], skipped);
        assert_eq!(got["sessions"], 50);
        assert_eq!(got["firstTs"], first);
        assert_eq!(got["lastTs"], last);
        assert_eq!(tally(&got["totals"])[..2], [calls, failures]);
        let by_day = got["byDay"].as_object().unwrap();
        assert_eq!(by_day.len(), want.len());
        for (day, expected) in &want {
            assert_eq!(tally(&by_day[day]), *expected, "{day}");
        }
        let calls_of = |group: &str| -> u64 {
            got[group].as_object().unwrap().values().map(|a| a["calls"].as_u64().unwrap()).sum()
        };
        assert_eq!(calls_of("byServer"), calls);
        assert_eq!(calls_of("byEventType"), calls);
        assert!(calls_of("byClient") < calls);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn import_is_idempotent_and_surfaces_in_summary() {
        let _guard = crate::registry::data_dir_test_lock();
        let base = scratch("handler");
        make_db(&base.join("monitor.db"));
        let _dir = crate::registry::DataDirOverride::set(base.join("data"));
        let args = json!({"path": base.join("monitor.db").to_string_lossy()});
        let first = crate::plus::dispatch("plus.obs.importMonitor", args.clone()).unwrap();
        let second = crate::plus::dispatch("plus.obs.importMonitor", args).unwrap();
        assert_eq!(first, second);
        assert_eq!(first["rows"], 5);
        let sum = crate::plus::dispatch(
            "plus.obs.summary",
            json!({"root": base.join("none").to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(sum["mcpmHistory"]["totals"]["calls"], 4);
        assert_eq!(sum["totals"]["messages"], 0);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_and_foreign_files_error() {
        let base = scratch("err");
        let dir = base.join("obs");
        let lock = Locked::acquire(&dir).unwrap();
        assert!(import(&lock, &base.join("nope.db")).is_err());
        let other = base.join("other.db");
        Connection::open(&other)
            .unwrap()
            .execute_batch("CREATE TABLE t(a)")
            .unwrap();
        assert!(import(&lock, &other)
            .unwrap_err()
            .contains("not a monitor.db"));
        std::fs::write(base.join("junk.db"), b"hello").unwrap();
        assert!(import(&lock, &base.join("junk.db")).is_err());
        assert!(lock.load_history().is_none());
        let _ = std::fs::remove_dir_all(&base);
    }
}
