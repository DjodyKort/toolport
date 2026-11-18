use super::feed_library::short;
use super::model::{Action, Item, Screen, Urgency};
use crate::servers::repo::peers::{PeerView, Relation};

const SOURCE: &str = "Updates";

/// Commits of one checkout that sit on no remote, while this machine publishes its checkouts.
pub struct Unpublished {
    pub server: String,
    pub head: String,
    pub commits: u64,
}

fn plural(count: u64) -> String {
    if count == 1 {
        "1 commit".into()
    } else {
        format!("{count} commits")
    }
}

fn row(view: &PeerView) -> Option<Item> {
    let (mine, theirs) = (short(Some(&view.mine_head)), short(Some(&view.theirs.head)));
    let same_branch = view.mine_branch == view.theirs.branch;
    if same_branch && matches!(view.relation, Relation::Same | Relation::Older) {
        return None;
    }
    let title = if same_branch {
        format!(
            "{}: {} is on {} at {theirs}, you are on {} at {mine}",
            view.server, view.machine, view.theirs.branch, view.mine_branch
        )
    } else {
        format!(
            "{}: {} is on {} at {theirs}, you are on {}",
            view.server, view.machine, view.theirs.branch, view.mine_branch
        )
    };
    let (detail, action) = if !view.theirs.pushed {
        (
            format!(
                "Its commits are on no remote, so this machine cannot install them. Push them from {} first.",
                view.machine
            ),
            None,
        )
    } else if !same_branch {
        (
            format!(
                "Switch branch checks {} out here when the work tree is clean. Nothing changes until you review it.",
                view.theirs.branch
            ),
            Some(Action::run(
                "Switch branch",
                Screen::Servers,
                &["toolportctl", "server", "repo", "switch", &view.server, "--dry-run"],
            )),
        )
    } else {
        match view.relation {
            Relation::Newer(count) => (
                format!(
                    "{} ahead of this machine. Pull fetches and only fast-forwards after you review it.",
                    plural(count)
                ),
                Some(pull(view)),
            ),
            Relation::Unknown if !view.fetched_since => (
                "This machine has not fetched that commit yet. Pull fetches first, then shows what changes.".into(),
                Some(pull(view)),
            ),
            Relation::Unknown => (
                format!(
                    "That commit is not on this machine's remote even after a fetch. Check that {} pushed it to the same remote.",
                    view.machine
                ),
                None,
            ),
            _ => (
                "Both machines have commits the other lacks. Toolport only fast-forwards, so rebase or merge it yourself.".into(),
                None,
            ),
        }
    };
    Some(Item {
        id: format!(
            "updates:peer:{}:{}:{mine}>{theirs}",
            view.server, view.machine
        ),
        urgency: Urgency::WorthALook,
        title,
        detail,
        source: SOURCE,
        since: None,
        action: action.or_else(|| Some(Action::open("Open Servers", Screen::Servers))),
        skip: Some(format!("Skip {theirs}")),
    })
}

fn pull(view: &PeerView) -> Action {
    Action::run(
        "Pull",
        Screen::Servers,
        &[
            "toolportctl",
            "server",
            "repo",
            "update",
            &view.server,
            "--fetch",
            "--dry-run",
        ],
    )
}

fn unpublished_row(item: &Unpublished) -> Item {
    let head = short(Some(&item.head));
    Item {
        id: format!("updates:unpublished:{}:{head}", item.server),
        urgency: Urgency::WorthALook,
        title: format!(
            "{} of {} {} on no remote; your other machine cannot see them",
            plural(item.commits),
            item.server,
            if item.commits == 1 { "is" } else { "are" }
        ),
        detail: "Push the branch from its checkout so the other machine can pull it.".into(),
        source: SOURCE,
        since: None,
        action: Some(Action::open("Open Servers", Screen::Servers)),
        skip: Some(format!("Skip {head}")),
    }
}

pub fn items(peers: &[PeerView], unpublished: &[Unpublished]) -> Vec<Item> {
    peers
        .iter()
        .filter_map(row)
        .chain(unpublished.iter().map(unpublished_row))
        .collect()
}
