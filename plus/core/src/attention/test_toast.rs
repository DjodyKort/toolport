//! `attention notify --test` and `--open-settings`: only the app process can show a notification
//! or open the system's settings for it, so the command line leaves a request file in the data
//! dir and the app's notifier thread answers it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use toolport_plus_api::{Host, Notifier, Permission};

use super::notify;
use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;

const REQUEST: &str = "attention-test-toast.json";
const ANSWER: &str = "attention-test-toast-answer.json";
const KIND_TEST: &str = "test";
const KIND_SETTINGS: &str = "open-settings";
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
    ask_host(host, KIND_TEST, wait)
}

/// Asks the running app to open the system's notification settings for Toolport+.
pub fn request_settings(host: &dyn Host, wait: Duration) -> Result<Value, OpError> {
    ask_host(host, KIND_SETTINGS, wait)
}

fn ask_host(host: &dyn Host, kind: &str, wait: Duration) -> Result<Value, OpError> {
    let base = host
        .data_dir()
        .ok_or_else(|| OpError::failed("no_data_dir", "data directory could not be resolved"))?;
    ask(&base, kind, host.now_unix(), wait)
}

fn confirmation(answer: &Value) -> Value {
    let permission = answer["permission"].as_str().unwrap_or("unknown");
    let confirmed = permission == Permission::Allowed.word();
    let mut data = json!({
        "sent": true,
        "channel": notify::CHANNEL_SYSTEM,
        "permission": permission,
        "confirmed": confirmed,
    });
    if !confirmed {
        data["note"] = json!(UNCONFIRMED);
    }
    data
}

const UNCONFIRMED: &str = "Handed to the system. This platform does not tell Toolport+ whether notifications are allowed; if nothing appeared, check System Settings > Notifications > Toolport+";

fn ask(base: &Path, kind: &str, now: i64, wait: Duration) -> Result<Value, OpError> {
    let (request, answer) = (file(base, REQUEST), file(base, ANSWER));
    let id = nonce();
    let _ = std::fs::remove_file(&answer);
    write(&request, &json!({"id": id, "at": now, "kind": kind}))?;
    let started = Instant::now();
    loop {
        if let Some(found) = read(&answer).filter(|a| a["id"] == id) {
            let _ = std::fs::remove_file(&answer);
            return match found["error"].as_str() {
                None if kind == KIND_SETTINGS => Ok(json!({"opened": true})),
                None => Ok(confirmation(&found)),
                Some(error) => {
                    let code = match found["code"].as_str() {
                        Some("notifications_blocked") => "notifications_blocked",
                        _ if kind == KIND_SETTINGS => "open_settings_failed",
                        _ => "notify_failed",
                    };
                    Err(
                        OpError::failed(code, error.to_string()).with_details(json!({
                            "sent": false,
                            "permission": found["permission"],
                        })),
                    )
                }
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
    let permission = notifier.permission();
    let answer = match asked["kind"].as_str().unwrap_or(KIND_TEST) {
        KIND_SETTINGS => match notifier.open_settings() {
            Ok(()) => json!({"id": asked["id"], "permission": permission.word(), "error": null}),
            Err(error) => {
                json!({"id": asked["id"], "permission": permission.word(), "error": error})
            }
        },
        _ => {
            let outcome = notify::test(notifier);
            json!({
                "id": asked["id"],
                "permission": outcome.permission.word(),
                "error": outcome.error,
                "code": outcome.code(),
            })
        }
    };
    notify::record_permission(host, notifier);
    let _ = write(&file(&base, ANSWER), &answer);
    true
}

#[cfg(test)]
pub(super) fn ask_at(base: &Path, now: i64, wait: Duration) -> Result<Value, OpError> {
    ask(base, KIND_TEST, now, wait)
}

#[cfg(test)]
pub(super) fn ask_settings_at(base: &Path, now: i64, wait: Duration) -> Result<Value, OpError> {
    ask(base, KIND_SETTINGS, now, wait)
}
