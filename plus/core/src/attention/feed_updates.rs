use super::feed_credentials::one_line;
use super::feed_library::short;
use super::model::{Action, Item, Screen, Urgency};
use crate::servers::repo::state::Verdict;

const SOURCE: &str = "Updates";

pub struct TapFacts {
    pub name: String,
    /// The clone ships a Claude Code plugin marketplace, not only skills.
    pub marketplace: bool,
    pub behind: u64,
    pub head: Option<String>,
    pub upstream_head: Option<String>,
}

/// A checkout one or more local servers run from, as git last saw it.
pub struct RepoFacts {
    /// The server the row is named after; `also` are the others that run from the same checkout.
    pub server: String,
    pub also: Vec<String>,
    pub branch: String,
    pub upstream: String,
    pub head: Option<String>,
    pub upstream_head: Option<String>,
    pub behind: u64,
    pub ahead: u64,
    pub dirty: u64,
    pub verdict: Verdict,
}

pub struct UpdateInputs {
    pub taps: Vec<TapFacts>,
    pub servers: Vec<RepoFacts>,
    /// Sources the last network pass could not reach, with the reason.
    pub failures: Vec<(String, String)>,
    /// How many sources the last pass looked at; a pass that reached none says nothing is wrong.
    pub checked: usize,
}

fn plural(count: u64) -> String {
    if count == 1 {
        "1 new commit".into()
    } else {
        format!("{count} new commits")
    }
}

fn tap_row(tap: &TapFacts) -> Item {
    let (from, to) = (
        short(tap.head.as_deref()),
        short(tap.upstream_head.as_deref()),
    );
    let what = if tap.marketplace {
        "plugin marketplace"
    } else {
        "skill tap"
    };
    Item {
        id: format!("updates:tap:{}:{from}>{to}", tap.name),
        urgency: Urgency::WorthALook,
        title: format!(
            "The {what} {} has {} upstream",
            tap.name,
            plural(tap.behind)
        ),
        detail: "Nothing is applied until you update the tap; the update only fast-forwards it."
            .into(),
        source: SOURCE,
        since: None,
        action: Some(Action::run(
            "Update",
            Screen::Library,
            &["toolportctl", "skills", "tap", "update", &tap.name],
        )),
        skip: Some(format!("Skip {to}")),
    }
}

fn repo_row(repo: &RepoFacts) -> Option<Item> {
    let (from, to) = (
        short(repo.head.as_deref()),
        short(repo.upstream_head.as_deref()),
    );
    let shared = if repo.also.is_empty() {
        String::new()
    } else {
        format!(" {} runs from the same checkout.", repo.also.join(", "))
    };
    let (id, title, detail, skip, action) = match repo.verdict {
        Verdict::Behind => (
            format!("updates:server:{}:{from}>{to}", repo.server),
            format!(
                "{}: {} on {}",
                repo.server,
                plural(repo.behind),
                repo.branch
            ),
            format!(
                "Nothing is applied until you review the update; it only fast-forwards {} to {}.{shared}",
                repo.branch, repo.upstream
            ),
            Some(format!("Skip {to}")),
            Action::run(
                "Review update",
                Screen::Servers,
                &["toolportctl", "server", "repo", "update", &repo.server, "--dry-run"],
            ),
        ),
        Verdict::Dirty => (
            format!("updates:server-blocked:{}:{to}", repo.server),
            format!("{}: local changes block the update", repo.server),
            format!(
                "{} on {} waits, but {} tracked file(s) have local changes. Commit or stash them, then review the update.{shared}",
                plural(repo.behind),
                repo.upstream,
                repo.dirty
            ),
            None,
            Action::open("Open Servers", Screen::Servers),
        ),
        Verdict::Diverged => (
            format!("updates:server-diverged:{}:{from}>{to}", repo.server),
            format!("{}: your branch diverged", repo.server),
            format!(
                "{} has {} local commit(s) and {} new commit(s) on {}. Toolport only fast-forwards, so rebase or merge it yourself.{shared}",
                repo.branch,
                repo.ahead,
                repo.behind,
                repo.upstream
            ),
            None,
            Action::open("Open Servers", Screen::Servers),
        ),
        _ => return None,
    };
    Some(Item {
        id,
        urgency: Urgency::WorthALook,
        title,
        detail,
        source: SOURCE,
        since: None,
        action: Some(action),
        skip,
    })
}

fn unreachable(inputs: &UpdateInputs) -> Option<Item> {
    let (_, first) = inputs.failures.first()?;
    let all = inputs.failures.len() >= inputs.checked.max(1);
    let (title, detail) = if all {
        (
            "You look to be offline".to_string(),
            format!(
                "No source could be reached for the update check: {}",
                one_line(first)
            ),
        )
    } else {
        (
            format!(
                "{} could not be reached for the update check",
                match inputs.failures.len() {
                    1 => "1 source".to_string(),
                    count => format!("{count} sources"),
                }
            ),
            format!("{}: {}", inputs.failures[0].0, one_line(first)),
        )
    };
    Some(Item {
        id: "updates:unreachable".into(),
        urgency: Urgency::WorthALook,
        title,
        detail,
        source: SOURCE,
        since: None,
        action: Some(Action::open("Open Library", Screen::Library)),
        skip: None,
    })
}

pub fn items(inputs: &UpdateInputs) -> Vec<Item> {
    inputs
        .taps
        .iter()
        .filter(|tap| tap.behind > 0 && tap.upstream_head.is_some())
        .map(tap_row)
        .chain(inputs.servers.iter().filter_map(repo_row))
        .chain(unreachable(inputs))
        .collect()
}
