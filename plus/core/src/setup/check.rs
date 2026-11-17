//! `setup check`: what a reset would lose, read from local git only. Blockers are work that exists
//! nowhere else; the rest is for a person to weigh. Ticks and acknowledgements are kept in the
//! setup state folder so the list survives the app.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{export, repos, search_roots, Ctx};
use crate::fsx::{atomic_write_bytes, Mode};
use crate::op::OpError;

const CARRY: [(&str, &str); 4] = [
    ("browser", "Browser profiles, bookmarks and logins"),
    ("keys", "SSH and signing keys"),
    ("licenses", "App licences and 2FA devices"),
    ("outside", "Files outside the repositories and Toolport"),
];

#[derive(Default, Serialize, Deserialize)]
struct Marks {
    #[serde(default)]
    ticked: BTreeSet<String>,
    #[serde(default)]
    acked: BTreeSet<String>,
}

fn marks_path(ctx: &Ctx<'_>) -> PathBuf {
    ctx.state_dir().join("check.json")
}

fn load(ctx: &Ctx<'_>) -> Marks {
    std::fs::read(marks_path(ctx))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub struct Options {
    pub roots: Vec<PathBuf>,
    pub tick: Vec<String>,
    pub untick: Vec<String>,
    pub ack: Vec<String>,
}

pub fn run(ctx: &Ctx<'_>, opts: &Options) -> Result<Value, OpError> {
    let mut marks = load(ctx);
    for id in &opts.tick {
        if !CARRY.iter().any(|(k, _)| k == id) {
            return Err(OpError::usage(format!("--tick does not know {id:?}")));
        }
        marks.ticked.insert(id.clone());
    }
    for id in &opts.untick {
        marks.ticked.remove(id);
    }
    marks.acked.extend(opts.ack.iter().cloned());
    if !(opts.tick.is_empty() && opts.untick.is_empty() && opts.ack.is_empty()) {
        let text = serde_json::to_string_pretty(&marks).unwrap_or_default() + "\n";
        atomic_write_bytes(&marks_path(ctx), text.as_bytes(), Mode::Private)
            .map_err(|e| OpError::failed("check", e.to_string()))?;
    }

    let (mut blockers, mut amber) = (Vec::new(), Vec::new());
    let found = repos::find(&search_roots(ctx, &opts.roots));
    for dir in &found {
        let repo = repos::read(dir, &ctx.home, true);
        let path = repo.path.clone();
        let add = |list: &mut Vec<Value>, kind: &str, note: String| {
            let id = format!("{kind}:{path}");
            list.push(json!({
                "id": id, "repo": path, "kind": kind, "note": note,
                "acked": marks.acked.contains(&id),
            }));
        };
        if let Some(why) = &repo.error {
            add(
                &mut amber,
                "unreadable",
                format!("could not be read ({why}): counted as unknown, not clean"),
            );
            continue;
        }
        if repo.unpushed_commits() > 0 {
            add(
                &mut blockers,
                "unpushed",
                format!("{} commits are on no remote", repo.unpushed_commits()),
            );
        }
        if !repo.never_pushed.is_empty() {
            add(
                &mut blockers,
                "never-pushed",
                format!("never pushed: {}", repo.never_pushed.join(", ")),
            );
        }
        if repo.no_remote {
            add(
                &mut blockers,
                "no-remote",
                "has no remote: it exists only here".into(),
            );
        }
        if repo.dirty + repo.untracked > 0 {
            add(
                &mut amber,
                "dirty",
                format!("{} changed, {} untracked files", repo.dirty, repo.untracked),
            );
        }
        if repo.stashes > 0 {
            add(&mut amber, "stash", format!("{} stashes", repo.stashes));
        }
        if !repo.ignored_names.is_empty() {
            add(
                &mut amber,
                "ignored-secrets",
                format!(
                    "ignored files that look like secrets: {}",
                    repo.ignored_names.join(", ")
                ),
            );
        }
    }
    let carry: Vec<Value> = CARRY
        .iter()
        .map(|(id, label)| json!({"id": id, "label": label, "ticked": marks.ticked.contains(*id)}))
        .collect();
    let last = export::last_export(ctx);
    let open_blockers = blockers.iter().filter(|b| b["acked"] != true).count();
    let ticked = carry.iter().filter(|c| c["ticked"] == true).count();
    Ok(json!({
        "repos": found.len(),
        "blockers": blockers,
        "amber": amber,
        "carry": carry,
        "lastExport": last,
        "ready": open_blockers == 0 && ticked == CARRY.len() && last.is_some(),
        "open": {"blockers": open_blockers, "carry": CARRY.len() - ticked},
    }))
}
