//! What `context sync` rewrites besides the bundles: the launch profile files and the direct
//! entries whose generated content went out of date. Only drift Toolport can fix on its own is
//! rewritten; everything that needs a decision (a deleted access set, an entry edited by hand) is
//! listed with the command that settles it.

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::launch_profile::{self, AddArgs};
use crate::direct::{self, State};
use crate::op::OpError;

const REGENERABLE_PROFILE: [&str; 3] = ["unbound", "file-missing", "stale-file"];

fn left(problems: &Value) -> String {
    problems
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(Value::as_str)
        .unwrap_or("needs a decision")
        .to_string()
}

fn profiles(host: &dyn Host, dry_run: bool) -> Result<Value, OpError> {
    let listing = launch_profile::ls(host, None, None)?;
    let mut rows = Vec::new();
    let mut up_to_date = 0;
    for profile in listing["profiles"].as_array().into_iter().flatten() {
        let name = profile["name"].as_str().unwrap_or_default();
        let state = profile["state"].as_str().unwrap_or("ok");
        if state == "ok" {
            up_to_date += 1;
            continue;
        }
        if !REGENERABLE_PROFILE.contains(&state) {
            rows.push(json!({
                "name": name, "state": state, "action": "left",
                "reason": left(&profile["problems"]),
                "fix": format!("toolportctl context launch-profile add {name} --set <access set>"),
            }));
            continue;
        }
        let run = launch_profile::add(
            host,
            &AddArgs {
                name,
                set: None,
                deny_direct: None,
                dry_run,
            },
        );
        rows.push(match run {
            Ok(_) => json!({
                "name": name, "state": state,
                "action": if dry_run { "would regenerate" } else { "regenerated" },
            }),
            Err(error) => json!({
                "name": name, "state": state, "action": "failed", "reason": error.message,
            }),
        });
    }
    Ok(json!({ "rows": rows, "upToDate": up_to_date }))
}

fn entries(host: &dyn Host, dry_run: bool) -> Result<Value, OpError> {
    let mut rows = Vec::new();
    let mut up_to_date = 0;
    for row in direct::list(host, None)? {
        let entry = json!({"client": row.client, "entry": row.entry, "state": row.state.as_str()});
        let mut value = match row.state {
            State::Ok => {
                up_to_date += 1;
                continue;
            }
            State::Unrecorded => continue,
            State::Stale => {
                let run = direct::add(
                    host,
                    &direct::Args {
                        server: &row.server,
                        client: &row.client,
                        dry_run,
                        ..Default::default()
                    },
                );
                match run {
                    Ok(_) => json!({
                        "action": if dry_run { "would rewrite" } else { "rewritten" },
                    }),
                    Err(error) => json!({"action": "failed", "reason": error.message}),
                }
            }
            other => json!({
                "action": "left",
                "reason": other.action().unwrap_or("needs a decision"),
                "fix": format!("toolportctl client direct check --client {}", row.client),
            }),
        };
        value
            .as_object_mut()
            .into_iter()
            .for_each(|fields| fields.extend(entry.as_object().cloned().unwrap_or_default()));
        rows.push(value);
    }
    Ok(json!({ "rows": rows, "upToDate": up_to_date }))
}

fn settled(result: Result<Value, OpError>) -> Value {
    result.unwrap_or_else(|error| json!({ "rows": [], "upToDate": 0, "error": error.message }))
}

/// Regenerates what drifted and says what was left. A failure on one row or one part never stops
/// the others, and a part that cannot be read reports why instead of failing `context sync`.
pub fn run(host: &dyn Host, dry_run: bool) -> Value {
    json!({
        "launchProfiles": settled(profiles(host, dry_run)),
        "directEntries": settled(entries(host, dry_run)),
    })
}
