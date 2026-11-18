use crate::library::remote::LocalState;
use crate::skills::auto::State;
use crate::tasks::cron::parse_rfc3339;

use super::feed_credentials::one_line;
use super::model::{Action, Item, Screen, Urgency};

const SOURCE: &str = "Library";

pub struct LibraryInputs {
    /// What the unattended sync left; `Err` when its state file cannot be read.
    pub auto: Result<State, String>,
    /// The library's git state; `None` when no library repository was found.
    pub repo: Option<LocalState>,
}

fn open() -> Action {
    Action::open("Open Library", Screen::Library)
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

pub fn items(inputs: &LibraryInputs) -> Vec<Item> {
    let mut out = Vec::new();
    match &inputs.auto {
        Err(error) => out.push(Item {
            id: "library:auto-state".into(),
            urgency: Urgency::NeedsYou,
            title: "The library's auto-sync state cannot be read".into(),
            detail: one_line(error),
            source: SOURCE,
            since: None,
            action: Some(open()),
        }),
        Ok(state) => {
            if let Some(run) = &state.last_run {
                let since = parse_rfc3339(&run.at);
                if !run.held.is_empty() {
                    let first = &run.held[0];
                    out.push(Item {
                        id: "library:held".into(),
                        urgency: Urgency::NeedsYou,
                        title: match run.held.len() {
                            1 => "1 change waits for you in the library".to_string(),
                            count => format!("{count} changes wait for you in the library"),
                        },
                        detail: format!(
                            "Auto-sync held back {} for {}: {}.",
                            first.skill, first.client, first.reason
                        ),
                        source: SOURCE,
                        since,
                        action: Some(Action::open("Review", Screen::Library)),
                    });
                }
                if let Some(error) = &run.error {
                    out.push(Item {
                        id: "library:sync-failed".into(),
                        urgency: Urgency::NeedsYou,
                        title: "The last auto-sync failed".into(),
                        detail: one_line(error),
                        source: SOURCE,
                        since,
                        action: Some(open()),
                    });
                }
            }
        }
    }
    if let Some(repo) = &inputs.repo {
        if repo.ahead > 0 && repo.behind > 0 {
            out.push(Item {
                id: "library:diverged".into(),
                urgency: Urgency::NeedsYou,
                title: "The library has diverged from its remote".into(),
                detail: format!(
                    "{} ahead and {} behind, so a pull cannot fast-forward.",
                    plural(repo.ahead as usize, "commit", "commits"),
                    repo.behind
                ),
                source: SOURCE,
                since: None,
                action: Some(open()),
            });
        } else if repo.behind > 0 {
            out.push(Item {
                id: "library:behind".into(),
                urgency: Urgency::WorthALook,
                title: format!(
                    "The library is {} behind its remote",
                    plural(repo.behind as usize, "commit", "commits")
                ),
                detail: "A pull would fast-forward it.".into(),
                source: SOURCE,
                since: None,
                action: Some(Action::open("Review", Screen::Library)),
            });
        }
    }
    out
}
