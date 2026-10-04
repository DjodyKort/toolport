use crate::plus::jsonfs::read_json;
use crate::usage_report::civil_from_days;
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_id: String,
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

pub(crate) fn open_private(path: &Path) -> std::io::Result<File> {
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

    fn history_path(&self) -> PathBuf {
        self.dir.join("monitor-history.json")
    }

    pub fn load_history(&self) -> Option<Value> {
        read_json(&self.history_path())
    }

    pub fn save_history(&self, history: &Value) -> Result<(), String> {
        let text = serde_json::to_string(history).map_err(|e| e.to_string())?;
        crate::registry::atomic_write(&self.history_path(), &text)
    }

    pub fn load_state(&self) -> State {
        self.load_saved_state().unwrap_or_default()
    }

    pub fn load_saved_state(&self) -> Option<State> {
        read_json::<State>(&self.state_path()).filter(|state| state.version == STATE_VERSION)
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

    pub fn events(&self) -> impl Iterator<Item = Event> {
        stream_events(&self.dir)
    }

    pub fn read_events(&self) -> Vec<Event> {
        self.events().collect()
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

/// Reads the event log without the lock: appends are single writes, so a reader sees at worst a
/// torn last line, which parsing skips.
pub fn stream_events(dir: &Path) -> impl Iterator<Item = Event> {
    File::open(dir.join("events.jsonl"))
        .into_iter()
        .flat_map(|file| {
            BufReader::new(file)
                .lines()
                .map_while(Result::ok)
                .filter_map(|line| serde_json::from_str::<Event>(&line).ok())
        })
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn iso_to_ms(ts: &str) -> Option<i64> {
    let b = ts.as_bytes();
    let shape_ok = b.len() >= 19
        && b[4] == b'-'
        && b[7] == b'-'
        && (b[10] == b'T' || b[10] == b' ')
        && b[13] == b':'
        && b[16] == b':';
    if !shape_ok {
        return None;
    }
    let num = |from: usize, to: usize| ts.get(from..to)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, s) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let mut rest = &ts[19..];
    let mut millis = 0;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        let head = &frac[..digits.min(3)];
        millis = head.parse::<i64>().ok()? * 10_i64.pow(3 - head.len() as u32);
        rest = &frac[digits..];
    }
    let offset_min = match rest {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let (oh, om) = (rest.get(1..3)?.parse::<i64>().ok()?, rest.get(4..6)?.parse::<i64>().ok()?);
            sign * (oh * 60 + om)
        }
    };
    let secs = days_from_civil(y, m, d) * 86_400 + h * 3600 + mi * 60 + s - offset_min * 60;
    Some(secs * 1000 + millis)
}

pub fn iso_from_ms(ms: i64) -> String {
    let rem = ms.div_euclid(1000).rem_euclid(86_400);
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        day_from_ms(ms),
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub fn day_from_ms(ms: i64) -> String {
    let (y, m, d) = civil_from_days(ms.div_euclid(86_400_000));
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
    fn iso_timestamps_convert_both_ways() {
        assert_eq!(iso_to_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_ms("2025-10-03T12:00:00Z"), Some(1_759_492_800_000));
        assert_eq!(iso_to_ms("2025-10-03T12:00:00.250Z"), Some(1_759_492_800_250));
        assert_eq!(iso_to_ms("2025-10-03T12:00:00.5Z"), Some(1_759_492_800_500));
        assert_eq!(iso_to_ms("2025-10-03T14:00:00+02:00"), Some(1_759_492_800_000));
        assert_eq!(iso_to_ms("2025-10-03T12:00:00"), Some(1_759_492_800_000));
        assert_eq!(iso_to_ms("2024-02-29T23:59:59.999Z"), Some(1_709_251_199_999));
        for bad in ["", "garbage", "2025-13-03T12:00:00Z", "2025-10-03T25:00:00Z", "2025-10-03T12:00:00Q"] {
            assert_eq!(iso_to_ms(bad), None, "{bad}");
        }
        assert_eq!(iso_from_ms(1_759_492_801_500), "2025-10-03T12:00:01Z");
        assert_eq!(iso_from_ms(0), "1970-01-01T00:00:00Z");
        for ms in [0, 1_709_251_199_000, 1_759_492_800_000, 4_102_444_799_000] {
            assert_eq!(iso_to_ms(&iso_from_ms(ms)), Some(ms));
        }
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
