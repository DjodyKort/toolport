//! `attention notify --test`: only the app process can show a notification, so the command line
//! leaves a request file in the data dir and the app's notifier thread answers it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use toolport_plus_api::{Host, Notifier};

use super::notify;
use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;

const REQUEST: &str = "attention-test-toast.json";
const ANSWER: &str = "attention-test-toast-answer.json";
/// A request nobody answered in this long is dropped, so a test never fires late.
const TTL_SECS: i64 = 30;
pub const DEFAULT_WAIT: Duration = Duration::from_secs(6);
const POLL: Duration = Duration::from_millis(150);

fn file(base: &Path, name: &str) -> PathBuf {
    base.join("plus").join(name)
}

fn read(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn write(path: &Path, value: &Value) -> Result<(), OpError> {
    atomic_write(path, &(value.to_string() + "\n"), Mode::Private)
        .map_err(|e| OpError::failed("attention_notify", format!("{}: {e}", path.display())))
}

fn nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{nanos:x}")
}

/// Asks the running app to show the test notification and waits for its answer.
pub fn request(host: &dyn Host, wait: Duration) -> Result<Value, OpError> {
    let base = host
        .data_dir()
        .ok_or_else(|| OpError::failed("no_data_dir", "data directory could not be resolved"))?;
    ask(&base, host.now_unix(), wait)
}

fn ask(base: &Path, now: i64, wait: Duration) -> Result<Value, OpError> {
    let (request, answer) = (file(base, REQUEST), file(base, ANSWER));
    let id = nonce();
    let _ = std::fs::remove_file(&answer);
    write(&request, &json!({"id": id, "at": now}))?;
    let started = Instant::now();
    loop {
        if let Some(found) = read(&answer).filter(|a| a["id"] == id) {
            let _ = std::fs::remove_file(&answer);
            return match found["error"].as_str() {
                None => Ok(json!({"sent": true})),
                Some(error) => Err(OpError::failed("attention_notify", error.to_string())),
            };
        }
        if started.elapsed() >= wait {
            let _ = std::fs::remove_file(&request);
            return Err(OpError::failed(
                "app_not_running",
                "the Toolport+ app did not answer; notifications are shown by the running app, so open it and try again",
            ));
        }
        std::thread::sleep(POLL);
    }
}

/// The app's side: shows the test notification for a fresh request and writes the answer.
/// Returns whether there was one.
pub fn answer(host: &dyn Host, notifier: &dyn Notifier) -> bool {
    let Some(base) = host.data_dir() else {
        return false;
    };
    let request = file(&base, REQUEST);
    let Some(asked) = read(&request) else {
        return false;
    };
    let _ = std::fs::remove_file(&request);
    let fresh = asked["at"]
        .as_i64()
        .is_some_and(|at| host.now_unix() - at <= TTL_SECS);
    if !fresh {
        return false;
    }
    let outcome = notify::test(notifier);
    let _ = write(
        &file(&base, ANSWER),
        &json!({"id": asked["id"], "error": outcome.err()}),
    );
    true
}

#[cfg(test)]
pub(super) fn ask_at(base: &Path, now: i64, wait: Duration) -> Result<Value, OpError> {
    ask(base, now, wait)
}
