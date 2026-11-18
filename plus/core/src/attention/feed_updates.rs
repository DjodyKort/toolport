use super::feed_credentials::one_line;
use super::feed_library::short;
use super::model::{Action, Item, Screen, Urgency};

const SOURCE: &str = "Updates";

pub struct TapFacts {
    pub name: String,
    /// The clone ships a Claude Code plugin marketplace, not only skills.
    pub marketplace: bool,
    pub behind: u64,
    pub head: Option<String>,
    pub upstream_head: Option<String>,
}

pub struct UpdateInputs {
    pub taps: Vec<TapFacts>,
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
        .chain(unreachable(inputs))
        .collect()
}
