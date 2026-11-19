//! `setup check`: what a reset would lose, read from local git only. Blockers are work that exists
//! nowhere else; the rest is for a person to weigh. Ticks and acknowledgements are kept in the
//! setup state folder so the list survives the app.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::parts::{self, Build};
use super::{export, repos, search_roots, Ctx, PartId};
use crate::fsx::{atomic_write_bytes, Mode};
use crate::op::OpError;
use crate::skills::gitexec::{git_command, run_command};

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
    /// Also say what changed since the last export: it builds the readable parts again.
    pub since: bool,
}

const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// A path as a shell reads it: `~/` stays unquoted so it expands, the rest is quoted when needed.
fn shell_path(path: &str) -> String {
    let (head, rest) = match path.strip_prefix("~/") {
        Some(rest) => ("~/", rest),
        None => ("", path),
    };
    let plain = !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '+'));
    if plain {
        format!("{head}{rest}")
    } else {
        format!("{head}'{}'", rest.replace('\'', "'\\''"))
    }
}

/// The remote a branch pushes to, as git has it configured: origin when none is set.
fn push_remote(repo: &repos::Repo, branch: &str) -> String {
    let key = format!("branch.{branch}.remote");
    run_command(
        git_command(Some(&repo.abs), &["config", "--get", &key]),
        GIT_TIMEOUT,
    )
    .ok()
    .filter(|out| out.ok())
    .map(|out| out.stdout.trim().to_string())
    .filter(|remote| !remote.is_empty() && remote != ".")
    .unwrap_or_else(|| "origin".to_string())
}

/// The commands that would push the work of this repository. The check shows them to be copied;
/// nothing here runs one.
fn push_commands(repo: &repos::Repo, branches: &[String], first_push: bool) -> Vec<String> {
    let path = shell_path(&repo.path);
    branches
        .iter()
        .map(|branch| {
            let remote = push_remote(repo, branch);
            let flag = if first_push { "-u " } else { "" };
            format!("git -C {path} push {flag}{remote} {}", shell_path(branch))
        })
        .collect()
}

/// The past conversations that an export could carry, per repository: what the choice of sessions
/// is made from. Sized, never read.
fn sessions_of(ctx: &Ctx<'_>, read: &[repos::Repo]) -> Value {
    let mut export_opts = export::Options::default();
    export_opts.parts.insert(PartId::History);
    let memory = ctx.memory();
    let build = Build {
        ctx,
        opts: &export_opts,
        repos: read,
        memory: &memory,
        scratch: std::path::Path::new(""),
        content: false,
    };
    let part = parts::history_part(&build, None);
    json!({
        "count": part.count,
        "fileSize": part.bytes(),
        "repos": part.info["repos"],
    })
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
    let mut read: Vec<repos::Repo> = Vec::new();
    for dir in &found {
        let repo = repos::read(dir, &ctx.home, true);
        read.push(repo.clone());
        let path = repo.path.clone();
        let add = |list: &mut Vec<Value>, kind: &str, note: String, commands: Vec<String>| {
            let id = format!("{kind}:{path}");
            list.push(json!({
                "id": id, "repo": path, "kind": kind, "note": note,
                "commands": commands,
                "acked": marks.acked.contains(&id),
            }));
        };
        if let Some(why) = &repo.error {
            add(
                &mut amber,
                "unreadable",
                format!("could not be read ({why}): counted as unknown, not clean"),
                Vec::new(),
            );
            continue;
        }
        if repo.unpushed_commits() > 0 {
            add(
                &mut blockers,
                "unpushed",
                format!("{} commits are on no remote", repo.unpushed_commits()),
                push_commands(
                    &repo,
                    &repo
                        .unpushed
                        .iter()
                        .map(|b| b.branch.clone())
                        .collect::<Vec<_>>(),
                    false,
                ),
            );
        }
        if !repo.never_pushed.is_empty() {
            add(
                &mut blockers,
                "never-pushed",
                format!("never pushed: {}", repo.never_pushed.join(", ")),
                push_commands(&repo, &repo.never_pushed, true),
            );
        }
        if repo.no_remote {
            add(
                &mut blockers,
                "no-remote",
                "has no remote: it exists only here".into(),
                Vec::new(),
            );
        }
        if repo.dirty + repo.untracked > 0 {
            add(
                &mut amber,
                "dirty",
                format!("{} changed, {} untracked files", repo.dirty, repo.untracked),
                vec![format!("git -C {} status --short", shell_path(&repo.path))],
            );
        }
        if repo.stashes > 0 {
            add(
                &mut amber,
                "stash",
                format!("{} stashes", repo.stashes),
                vec![format!("git -C {} stash list", shell_path(&repo.path))],
            );
        }
        if !repo.ignored_names.is_empty() {
            add(
                &mut amber,
                "ignored-secrets",
                format!(
                    "ignored files that look like secrets: {}",
                    repo.ignored_names.join(", ")
                ),
                Vec::new(),
            );
        }
    }
    let carry: Vec<Value> = CARRY
        .iter()
        .map(|(id, label)| json!({"id": id, "label": label, "ticked": marks.ticked.contains(*id)}))
        .collect();
    let last = export::last_export(ctx);
    let since = match (&last, opts.since) {
        (Some(last), true) => export::changed_since(ctx, last),
        _ => None,
    };
    let open_blockers = blockers.iter().filter(|b| b["acked"] != true).count();
    let ticked = carry.iter().filter(|c| c["ticked"] == true).count();
    Ok(json!({
        "repos": found.len(),
        "blockers": blockers,
        "amber": amber,
        "carry": carry,
        "lastExport": last,
        "since": since,
        "sessions": sessions_of(ctx, &read),
        "ready": open_blockers == 0 && ticked == CARRY.len() && last.is_some(),
        "open": {"blockers": open_blockers, "carry": CARRY.len() - ticked},
    }))
}

#[cfg(test)]
mod tests {
    use super::shell_path;

    #[test]
    fn a_path_is_quoted_for_a_shell_only_where_it_needs_to_be() {
        assert_eq!(shell_path("~/code/web-app"), "~/code/web-app");
        assert_eq!(shell_path("~/code/my app"), "~/'code/my app'");
        assert_eq!(shell_path("/Volumes/x y/it's"), "'/Volumes/x y/it'\\''s'");
        assert_eq!(shell_path("feat/rate-limits"), "feat/rate-limits");
        assert_eq!(shell_path("a;rm -rf ~"), "'a;rm -rf ~'");
    }
}
