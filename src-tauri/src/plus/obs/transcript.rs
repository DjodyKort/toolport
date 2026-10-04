use super::store::{day_from_iso, FileState, Locked, McpFailure, MsgRecord, State};
use crate::plus::fswalk::collect_files;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexReport {
    pub files_scanned: usize,
    pub files_skipped: usize,
    pub files_reset: usize,
    pub files_gone: usize,
    pub lines_parsed: usize,
    pub messages_upserted: usize,
}

pub enum Parsed {
    Message { id: String, record: MsgRecord },
    FailedMcp(Vec<McpFailure>),
}

pub fn mcp_server_of(tool: &str) -> Option<&str> {
    let rest = tool.strip_prefix("mcp__")?;
    let (server, name) = rest.split_once("__")?;
    (!server.is_empty() && !name.is_empty()).then_some(server)
}

fn u64_of(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

pub fn parse_line(line: &str, file_stem: &str, anon_key: &str) -> Option<Parsed> {
    parse_line_with(line, file_stem, || anon_key.to_string())
}

/// Only usage and deferred-tools records parse to something; `\u` could spell either key.
fn may_parse(line: &str) -> bool {
    line.contains("usage") || line.contains("deferred_tools_delta") || line.contains("\\u")
}

fn parse_line_with(
    line: &str,
    file_stem: &str,
    anon_key: impl FnOnce() -> String,
) -> Option<Parsed> {
    if !may_parse(line) {
        return None;
    }
    parse_value_line(line, file_stem, anon_key)
}

fn parse_value_line(
    line: &str,
    file_stem: &str,
    anon_key: impl FnOnce() -> String,
) -> Option<Parsed> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let session = v
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or(file_stem)
        .to_string();
    let ts = v.get("timestamp").and_then(Value::as_str).unwrap_or("");

    if v.get("type").and_then(Value::as_str) == Some("attachment") {
        let att = v.get("attachment")?;
        if att.get("type").and_then(Value::as_str) != Some("deferred_tools_delta") {
            return None;
        }
        let failed: Vec<McpFailure> = att
            .get("failedMcpServers")
            .and_then(Value::as_array)?
            .iter()
            .filter_map(Value::as_str)
            .map(|server| McpFailure {
                session: session.clone(),
                server: server.to_string(),
                ts: ts.to_string(),
            })
            .collect();
        return (!failed.is_empty()).then_some(Parsed::FailedMcp(failed));
    }

    let msg = v.get("message")?;
    let is_assistant = v.get("type").and_then(Value::as_str) == Some("assistant")
        || msg.get("role").and_then(Value::as_str) == Some("assistant");
    if !is_assistant {
        return None;
    }
    let usage = msg.get("usage")?;
    let model = msg.get("model").and_then(Value::as_str).unwrap_or("");
    if model == "<synthetic>" {
        return None;
    }
    let id = msg
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(anon_key);

    let mut tools = BTreeMap::new();
    if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
        for (i, block) in blocks.iter().enumerate() {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let Some(name) = block.get("name").and_then(Value::as_str) else {
                continue;
            };
            let key = block
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("{id}#{i}"));
            tools.insert(key, name.to_string());
        }
    }

    Some(Parsed::Message {
        id,
        record: MsgRecord {
            session,
            model: model.to_string(),
            cwd: v.get("cwd").and_then(Value::as_str).unwrap_or("").into(),
            ts: ts.to_string(),
            day: day_from_iso(ts).unwrap_or_default(),
            input: u64_of(usage, "input_tokens"),
            output: u64_of(usage, "output_tokens"),
            cache_creation: u64_of(usage, "cache_creation_input_tokens"),
            cache_read: u64_of(usage, "cache_read_input_tokens"),
            tools,
            request_id: v
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        },
    })
}

fn collect_jsonl(root: &Path, out: &mut Vec<PathBuf>) {
    match std::fs::metadata(root) {
        Ok(meta) if meta.is_file() => out.push(root.to_path_buf()),
        Ok(_) => collect_files(root, &|p| p.extension().is_some_and(|e| e == "jsonl"), out),
        Err(_) => {}
    }
}

#[cfg(unix)]
fn inode_of(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.ino()
}

#[cfg(not(unix))]
fn inode_of(_meta: &std::fs::Metadata) -> u64 {
    0
}

fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn upsert(state: &mut State, id: String, mut record: MsgRecord) {
    match state.messages.get_mut(&id) {
        Some(prev) => {
            let mut tools = std::mem::take(&mut prev.tools);
            tools.extend(std::mem::take(&mut record.tools));
            record.tools = tools;
            *prev = record;
        }
        None => {
            state.messages.insert(id, record);
        }
    }
}

fn index_file(state: &mut State, path: &Path, report: &mut IndexReport) -> Result<(), String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let key = path.to_string_lossy().to_string();
    let (size, mtime, inode) = (meta.len(), mtime_ms(&meta), inode_of(&meta));
    report.files_scanned += 1;

    let mut start = 0u64;
    if let Some(prev) = state.files.get(&key) {
        if prev.inode == inode && size >= prev.offset {
            if prev.size == size && prev.mtime_ms == mtime {
                report.files_skipped += 1;
                return Ok(());
            }
            start = prev.offset;
        } else {
            report.files_reset += 1;
        }
    }

    let mut reader =
        BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?);
    reader
        .seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let mut offset = start;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let read = reader.read_until(b'\n', &mut buf).map_err(|e| e.to_string())?;
        if buf.last() != Some(&b'\n') {
            break;
        }
        let line_start = offset;
        offset += read as u64;
        let Ok(line) = std::str::from_utf8(&buf[..read - 1]) else {
            continue;
        };
        report.lines_parsed += 1;
        match parse_line_with(line, stem, || format!("anon:{key}:{line_start}")) {
            Some(Parsed::Message { id, record }) => {
                report.messages_upserted += 1;
                upsert(state, id, record);
            }
            Some(Parsed::FailedMcp(list)) => {
                for f in list {
                    state
                        .transcript_mcp_failures
                        .insert(format!("{}|{}", f.session, f.server), f);
                }
            }
            None => {}
        }
    }

    state.files.insert(
        key,
        FileState {
            offset,
            size,
            mtime_ms: mtime,
            inode,
        },
    );
    Ok(())
}

pub fn index(lock: &Locked, root: &Path) -> Result<IndexReport, String> {
    index_state(lock, root).map(|(report, _)| report)
}

/// The state is rewritten only when it changed or was not saved yet.
pub fn index_state(lock: &Locked, root: &Path) -> Result<(IndexReport, State), String> {
    let saved = lock.load_saved_state();
    let was_saved = saved.is_some();
    let mut state = saved.unwrap_or_default();
    let mut report = IndexReport::default();
    let mut files = Vec::new();
    collect_jsonl(root, &mut files);
    files.sort();
    for path in &files {
        index_file(&mut state, path, &mut report)?;
    }
    let root_prefix = root.to_string_lossy().to_string();
    let gone: Vec<String> = state
        .files
        .keys()
        .filter(|k| k.starts_with(&root_prefix) && !Path::new(k.as_str()).exists())
        .cloned()
        .collect();
    for k in gone {
        state.files.remove(&k);
        report.files_gone += 1;
    }
    if !was_saved || report.files_skipped != report.files_scanned || report.files_gone > 0 {
        lock.save_state(&state)?;
    }
    Ok((report, state))
}

#[cfg(test)]
pub(crate) mod fixtures {
    use serde_json::{json, Value};

    pub fn with_request_id(line: &str, request_id: &str) -> String {
        let mut value: Value = serde_json::from_str(line).unwrap();
        value["requestId"] = json!(request_id);
        value.to_string()
    }

    pub fn assistant(
        id: &str,
        session: &str,
        ts: &str,
        model: &str,
        usage: (u64, u64, u64, u64),
        tools: &[(&str, &str)],
    ) -> String {
        let content: Vec<Value> = if tools.is_empty() {
            vec![json!({"type": "text", "text": "synthetic"})]
        } else {
            tools
                .iter()
                .map(
                    |(tid, name)| json!({"type": "tool_use", "id": tid, "name": name, "input": {}}),
                )
                .collect()
        };
        json!({
            "type": "assistant",
            "uuid": format!("u-{id}"),
            "sessionId": session,
            "cwd": "/work/demo",
            "timestamp": ts,
            "futureField": {"nested": [1, 2, 3]},
            "message": {
                "id": id,
                "role": "assistant",
                "model": model,
                "content": content,
                "usage": {
                    "input_tokens": usage.0,
                    "output_tokens": usage.1,
                    "cache_creation_input_tokens": usage.2,
                    "cache_read_input_tokens": usage.3,
                    "service_tier": "standard"
                }
            }
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::assistant;
    use super::*;
    use crate::plus::obs::store::Locked;
    use std::io::Write;

    fn scratch(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("obs-tr-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn append(path: &Path, text: &str) {
        let mut f = crate::registry::open_append_private(path).unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn mcp_prefix_parsing() {
        assert_eq!(mcp_server_of("mcp__github__create_issue"), Some("github"));
        assert_eq!(mcp_server_of("mcp__my_srv__do__it"), Some("my_srv"));
        assert_eq!(mcp_server_of("mcp__x__"), None);
        assert_eq!(mcp_server_of("Bash"), None);
    }

    #[test]
    fn streamed_partials_dedupe_keeping_last_and_union_tools() {
        let base = scratch("dedupe");
        let proj = base.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let t = proj.join("s1.jsonl");
        let lines = [
            assistant(
                "msg_1",
                "s1",
                "2026-10-01T10:00:00Z",
                "claude-a",
                (10, 1, 500, 0),
                &[],
            ),
            assistant(
                "msg_1",
                "s1",
                "2026-10-01T10:00:01Z",
                "claude-a",
                (10, 5, 500, 0),
                &[("tu1", "mcp__github__list")],
            ),
            assistant(
                "msg_1",
                "s1",
                "2026-10-01T10:00:02Z",
                "claude-a",
                (10, 42, 500, 7000),
                &[("tu2", "Bash")],
            ),
            "not json at all".to_string(),
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#.to_string(),
            assistant(
                "msg_2",
                "s1",
                "2026-10-02T09:00:00Z",
                "claude-b",
                (3, 4, 0, 100),
                &[("tu3", "mcp__github__get")],
            ),
        ];
        append(&t, &(lines.join("\n") + "\n"));
        let lock = Locked::acquire(&base.join("data")).unwrap();
        let report = index(&lock, &proj).unwrap();
        assert_eq!(report.messages_upserted, 4);
        let state = lock.load_state();
        assert_eq!(state.messages.len(), 2);
        let m1 = &state.messages["msg_1"];
        assert_eq!(
            (m1.input, m1.output, m1.cache_creation, m1.cache_read),
            (10, 42, 500, 7000)
        );
        assert_eq!(m1.ts, "2026-10-01T10:00:02Z");
        assert_eq!(m1.tools.len(), 2);
        assert_eq!(m1.cwd, "/work/demo");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn truncated_last_line_is_deferred_until_completed() {
        let base = scratch("trunc");
        let proj = base.join("p");
        std::fs::create_dir_all(&proj).unwrap();
        let t = proj.join("s.jsonl");
        let full = assistant("msg_9", "s", "2026-10-01T10:00:00Z", "m", (1, 2, 3, 4), &[]);
        let (head, tail) = full.split_at(full.len() / 2);
        append(
            &t,
            &format!(
                "{}\n{head}",
                assistant("msg_8", "s", "2026-10-01T09:00:00Z", "m", (1, 1, 0, 0), &[])
            ),
        );
        let lock = Locked::acquire(&base.join("data")).unwrap();
        index(&lock, &proj).unwrap();
        assert_eq!(lock.load_state().messages.len(), 1);
        append(&t, &format!("{tail}\n"));
        index(&lock, &proj).unwrap();
        let state = lock.load_state();
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.messages["msg_9"].cache_read, 4);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn incremental_reindex_reads_only_appended_bytes() {
        let base = scratch("incr");
        let proj = base.join("p");
        std::fs::create_dir_all(&proj).unwrap();
        let t = proj.join("s.jsonl");
        append(
            &t,
            &(assistant("a", "s", "2026-10-01T10:00:00Z", "m", (1, 1, 0, 0), &[]) + "\n"),
        );
        let lock = Locked::acquire(&base.join("data")).unwrap();
        let r1 = index(&lock, &proj).unwrap();
        assert_eq!((r1.lines_parsed, r1.files_skipped), (1, 0));
        let r2 = index(&lock, &proj).unwrap();
        assert_eq!((r2.lines_parsed, r2.files_skipped), (0, 1));
        append(
            &t,
            &(assistant("b", "s", "2026-10-01T11:00:00Z", "m", (2, 2, 0, 0), &[]) + "\n"),
        );
        let r3 = index(&lock, &proj).unwrap();
        assert_eq!((r3.lines_parsed, r3.messages_upserted), (1, 1));
        assert_eq!(lock.load_state().messages.len(), 2);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rotation_reparses_and_deletion_keeps_history() {
        let base = scratch("rot");
        let proj = base.join("p");
        std::fs::create_dir_all(&proj).unwrap();
        let t = proj.join("s.jsonl");
        let big = assistant(
            "a",
            "s",
            "2026-10-01T10:00:00Z",
            "m",
            (1, 1, 0, 0),
            &[("t1", "Bash")],
        );
        append(&t, &format!("{big}\n{big}\n"));
        let lock = Locked::acquire(&base.join("data")).unwrap();
        index(&lock, &proj).unwrap();
        std::fs::remove_file(&t).unwrap();
        append(
            &t,
            &(assistant("c", "s", "2026-10-02T10:00:00Z", "m", (5, 5, 0, 0), &[]) + "\n"),
        );
        let r = index(&lock, &proj).unwrap();
        assert_eq!(r.files_reset, 1);
        let state = lock.load_state();
        assert!(state.messages.contains_key("a") && state.messages.contains_key("c"));
        std::fs::remove_file(&t).unwrap();
        let r = index(&lock, &proj).unwrap();
        assert_eq!(r.files_gone, 1);
        let state = lock.load_state();
        assert!(state.files.is_empty());
        assert_eq!(state.messages.len(), 2);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn deferred_tools_delta_records_failed_mcp_servers() {
        let line = r#"{"type":"attachment","sessionId":"s","timestamp":"2026-10-01T10:00:00Z","attachment":{"type":"deferred_tools_delta","failedMcpServers":["figma","slack"],"extra":1}}"#;
        match parse_line(line, "s", "x") {
            Some(Parsed::FailedMcp(list)) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].server, "figma");
            }
            _ => panic!("expected failures"),
        }
        let other = r#"{"type":"attachment","attachment":{"type":"instructions","files":[]}}"#;
        assert!(parse_line(other, "s", "x").is_none());
    }

    #[test]
    fn synthetic_and_usage_less_messages_are_ignored() {
        let synth = assistant(
            "m",
            "s",
            "2026-10-01T10:00:00Z",
            "<synthetic>",
            (0, 0, 0, 0),
            &[],
        );
        assert!(parse_line(&synth, "s", "x").is_none());
        let no_usage =
            r#"{"type":"assistant","message":{"id":"m","role":"assistant","content":[]}}"#;
        assert!(parse_line(no_usage, "s", "x").is_none());
    }

    fn shape(parsed: Option<Parsed>) -> Option<Value> {
        parsed.map(|p| match p {
            Parsed::Message { id, record } => serde_json::json!({"id": id, "record": record}),
            Parsed::FailedMcp(list) => serde_json::json!({"failed": list}),
        })
    }

    #[test]
    fn the_prefilter_never_changes_what_a_line_parses_to() {
        use crate::plus::randutil::{run_cases, Rng};
        let backslash = '\\';
        let escaped_usage = format!(
            r#"{{"type":"assistant","message":{{"id":"e1","role":"assistant","model":"m","{backslash}u0075sage":{{"input_tokens":4}}}}}}"#
        );
        let escaped_delta = format!(
            r#"{{"type":"attachment","attachment":{{"type":"deferred_tools_{backslash}u0064elta","failedMcpServers":["figma"]}}}}"#
        );
        let fixed = [
            escaped_usage,
            escaped_delta,
            assistant("a1", "s", "2026-10-01T10:00:00Z", "m", (1, 2, 3, 4), &[]),
            assistant("a2", "s", "2026-10-01T10:00:00Z", "<synthetic>", (1, 2, 3, 4), &[]),
            r#"{"type":"user","message":{"role":"user","content":"usage in prose"}}"#.into(),
            r#"{"type":"assistant","message":{"id":"n","role":"assistant","content":[]}}"#.into(),
            r#"{"type":"attachment","sessionId":"s","attachment":{"type":"deferred_tools_delta","failedMcpServers":["x"]}}"#.into(),
            r#"{"type":"attachment","attachment":{"type":"other","failedMcpServers":["x"]}}"#.into(),
            String::new(),
            "   ".into(),
        ];
        let (mut parsed, mut skipped) = (0, 0);
        let mut check = |line: &str| {
            let want = shape(parse_value_line(line, "stem", || "anon".into()));
            let got = shape(parse_line_with(line, "stem", || "anon".into()));
            assert_eq!(got, want, "{line}");
            if want.is_some() {
                parsed += 1;
            } else {
                skipped += 1;
            }
        };
        for line in &fixed {
            check(line);
        }
        run_cases("obs-prefilter", 1500, |_, rng: &mut Rng| {
            let mut line = rng.pick(&fixed).clone();
            if rng.chance(10) {
                let cut = rng.below(line.len() + 1);
                line = line.chars().take(cut).collect();
            }
            if rng.chance(10) {
                line.push_str(&rng.garbage(12).replace('\n', " "));
            }
            check(&line);
            check(&rng.garbage(60).replace('\n', " "));
        });
        assert!(parsed > 300 && skipped > 300, "{parsed} {skipped}");
    }

    #[test]
    fn the_anonymous_key_is_only_built_for_a_message_without_an_id() {
        let built = std::cell::Cell::new(0);
        let anon = || {
            built.set(built.get() + 1);
            "anon:k".to_string()
        };
        let with_id = assistant("m1", "s", "2026-10-01T10:00:00Z", "m", (1, 1, 1, 1), &[]);
        assert!(parse_line_with(&with_id, "s", anon).is_some());
        assert!(parse_line_with(r#"{"type":"user"}"#, "s", anon).is_none());
        assert_eq!(built.get(), 0);
        let no_id = r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":1}}}"#;
        match parse_line_with(no_id, "s", anon) {
            Some(Parsed::Message { id, .. }) => assert_eq!(id, "anon:k"),
            _ => panic!("expected a message"),
        }
        assert_eq!(built.get(), 1);
    }

    #[test]
    fn streamed_indexing_keeps_offsets_anonymous_keys_and_the_torn_tail() {
        let base = scratch("stream");
        let proj = base.join("p");
        std::fs::create_dir_all(&proj).unwrap();
        let t = proj.join("s.jsonl");
        let anon_line = |text: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}],"usage":{{"input_tokens":1}}}}}}"#
            )
        };
        let long = "q".repeat(70_000);
        let first = anon_line("a") + "\r\n";
        let mut bytes = first.clone().into_bytes();
        bytes.extend_from_slice(b"\xff\xfe not utf8\n");
        let third_at = bytes.len();
        bytes.extend_from_slice((anon_line(&long) + "\n").as_bytes());
        let complete = bytes.len();
        bytes.extend_from_slice(anon_line("tail").as_bytes());
        std::fs::write(&t, &bytes).unwrap();

        let lock = Locked::acquire(&base.join("data")).unwrap();
        let report = index(&lock, &proj).unwrap();
        assert_eq!((report.lines_parsed, report.messages_upserted), (2, 2));
        let state = lock.load_state();
        let key = t.to_string_lossy();
        let mut ids: Vec<&String> = state.messages.keys().collect();
        ids.sort();
        assert_eq!(ids, [&format!("anon:{key}:0"), &format!("anon:{key}:{third_at}")]);
        assert_eq!(state.files[key.as_ref()].offset, complete as u64);

        append(&t, "\n");
        let report = index(&lock, &proj).unwrap();
        assert_eq!((report.lines_parsed, report.messages_upserted), (1, 1));
        let state = lock.load_state();
        assert!(state.messages.contains_key(&format!("anon:{key}:{complete}")));
        assert_eq!(state.files[key.as_ref()].offset, std::fs::metadata(&t).unwrap().len());
        let _ = std::fs::remove_dir_all(&base);
    }
}
