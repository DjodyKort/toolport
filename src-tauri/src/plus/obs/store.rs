use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub const STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MsgRecord {
    pub session: String,
    pub model: String,
    pub cwd: String,
    pub ts: String,
    pub day: String,
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    #[serde(default)]
    pub tools: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileState {
    pub offset: u64,
    pub size: u64,
    pub mtime_ms: i64,
    pub inode: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpFailure {
    pub session: String,
    pub server: String,
    pub ts: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub version: u32,
    #[serde(default)]
    pub files: BTreeMap<String, FileState>,
    #[serde(default)]
    pub messages: BTreeMap<String, MsgRecord>,
    #[serde(default)]
    pub transcript_mcp_failures: BTreeMap<String, McpFailure>,
}

impl Default for State {
    fn default() -> Self {
        State {
            version: STATE_VERSION,
            files: BTreeMap::new(),
            messages: BTreeMap::new(),
            transcript_mcp_failures: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub kind: String,
    pub ts_ms: i64,
    pub day: String,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub attrs: BTreeMap<String, Value>,
}

pub struct Locked {
    dir: PathBuf,
    _lock: File,
}

fn open_private(path: &Path) -> std::io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

impl Locked {
    pub fn acquire(dir: &Path) -> Result<Locked, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let lock = open_private(&dir.join("obs.lock")).map_err(|e| e.to_string())?;
        lock.lock_exclusive().map_err(|e| e.to_string())?;
        Ok(Locked {
            dir: dir.to_path_buf(),
            _lock: lock,
        })
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("transcripts.json")
    }

    fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }

    pub fn load_state(&self) -> State {
        let Ok(text) = std::fs::read_to_string(self.state_path()) else {
            return State::default();
        };
        match serde_json::from_str::<State>(&text) {
            Ok(state) if state.version == STATE_VERSION => state,
            _ => State::default(),
        }
    }

    pub fn save_state(&self, state: &State) -> Result<(), String> {
        let text = serde_json::to_string(state).map_err(|e| e.to_string())?;
        crate::registry::atomic_write(&self.state_path(), &text)
    }

    pub fn append_events(&self, events: &[Event]) -> Result<(), String> {
        if events.is_empty() {
            return Ok(());
        }
        let mut file =
            crate::registry::open_append_private(&self.events_path()).map_err(|e| e.to_string())?;
        let mut buf = String::new();
        for event in events {
            buf.push_str(&serde_json::to_string(event).map_err(|e| e.to_string())?);
            buf.push('\n');
        }
        file.write_all(buf.as_bytes()).map_err(|e| e.to_string())
    }

    pub fn read_events(&self) -> Vec<Event> {
        let Ok(file) = File::open(self.events_path()) else {
            return Vec::new();
        };
        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_str::<Event>(&line).ok())
            .collect()
    }

    pub fn prune_events_before(&self, ts_ms: i64) -> Result<usize, String> {
        let events = self.read_events();
        let keep: Vec<&Event> = events.iter().filter(|e| e.ts_ms >= ts_ms).collect();
        let removed = events.len() - keep.len();
        if removed == 0 {
            return Ok(0);
        }
        let mut text = String::new();
        for event in keep {
            text.push_str(&serde_json::to_string(event).map_err(|e| e.to_string())?);
            text.push('\n');
        }
        crate::registry::atomic_write(&self.events_path(), &text)?;
        Ok(removed)
    }
}

pub fn day_from_ms(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn day_from_iso(ts: &str) -> Option<String> {
    let head = ts.get(..10)?;
    let b = head.as_bytes();
    let ok = b.iter().enumerate().all(|(i, c)| match i {
        4 | 7 => *c == b'-',
        _ => c.is_ascii_digit(),
    });
    ok.then(|| head.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_conversions() {
        assert_eq!(day_from_ms(0), "1970-01-01");
        assert_eq!(day_from_ms(1_759_492_800_000), "2025-10-03");
        assert_eq!(day_from_ms(-1), "1969-12-31");
        assert_eq!(
            day_from_iso("2026-10-03T12:00:00Z").as_deref(),
            Some("2026-10-03")
        );
        assert_eq!(day_from_iso("garbage"), None);
        assert_eq!(day_from_iso("2026/10/03x"), None);
    }

    #[test]
    fn events_roundtrip_prune_and_skip_torn_line() {
        let dir = std::env::temp_dir().join(format!("obs-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lock = Locked::acquire(&dir).unwrap();
        let ev = |ts: i64| Event {
            kind: "cost".into(),
            ts_ms: ts,
            day: day_from_ms(ts),
            value: Some(1.0),
            attrs: BTreeMap::new(),
        };
        lock.append_events(&[ev(1000), ev(2000)]).unwrap();
        let mut f = crate::registry::open_append_private(&dir.join("events.jsonl")).unwrap();
        f.write_all(b"{\"kind\":\"co").unwrap();
        assert_eq!(lock.read_events().len(), 2);
        assert_eq!(lock.prune_events_before(1500).unwrap(), 1);
        assert_eq!(lock.read_events(), vec![ev(2000)]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("events.jsonl"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_roundtrip_and_version_mismatch() {
        let dir = std::env::temp_dir().join(format!("obs-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lock = Locked::acquire(&dir).unwrap();
        let mut state = State::default();
        state.messages.insert("m1".into(), MsgRecord::default());
        lock.save_state(&state).unwrap();
        assert_eq!(lock.load_state().messages.len(), 1);
        std::fs::write(dir.join("transcripts.json"), r#"{"version":99}"#).unwrap();
        assert!(lock.load_state().messages.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
