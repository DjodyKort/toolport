//! `sync repos`: the server checkouts this machine publishes and the ones the other machines
//! published, each set against this machine's. Local reads only; the last pull supplies the
//! other machines' records.

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::peers::Peers;
use crate::op::OpError;
use crate::servers::repo::{peers::unpublished, peers::views, repos};
use crate::setup::{export, repo_heads};
use crate::tasks::cron::rfc3339;

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

pub fn view(host: &dyn Host) -> Result<Value, OpError> {
    let data_dir = host
        .data_dir()
        .ok_or_else(|| OpError::failed("sync", "the data directory could not be resolved"))?;
    let found = repos(host)?;
    let machine = export::machine_name();
    let own = repo_heads::own_record(host, &machine);
    let peers = Peers::read(&data_dir);
    let compared = views(&found, &peers);
    let mine: Vec<Value> = own
        .heads
        .iter()
        .map(|head| {
            let commits = found
                .iter()
                .find(|r| r.servers.iter().any(|(_, name)| *name == head.server))
                .map_or(0, unpublished);
            json!({
                "server": head.server,
                "remote": head.remote,
                "branch": head.branch,
                "head": head.head,
                "pushed": head.pushed,
                "unpublished": commits,
            })
        })
        .collect();
    let others: Vec<Value> = peers
        .machines
        .iter()
        .map(|record| {
            let heads: Vec<Value> = record
                .heads
                .iter()
                .map(|head| {
                    let view = compared
                        .iter()
                        .find(|v| v.machine == record.machine && v.server == head.server);
                    json!({
                        "server": head.server,
                        "remote": head.remote,
                        "branch": head.branch,
                        "head": head.head,
                        "pushed": head.pushed,
                        "relation": view.map_or("not_here", |v| v.relation.word()),
                        "sameBranch": view.is_some_and(|v| v.mine_branch == head.branch),
                    })
                })
                .collect();
            json!({"machine": record.machine, "madeAt": record.made_at, "heads": heads})
        })
        .collect();
    Ok(json!({
        "publishing": super::config::publishes_repos(&data_dir),
        "machine": machine,
        "mine": mine,
        "others": others,
        "seenAt": (peers.seen_at > 0).then(|| rfc3339(peers.seen_at)),
    }))
}

pub fn human(data: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} {} this machine's checkouts\n",
        if data["publishing"] == true {
            "Publishing"
        } else {
            "Not publishing (sync is off or the Server repositories part is off)"
        },
        data["machine"].as_str().unwrap_or("")
    ));
    for head in data["mine"].as_array().into_iter().flatten() {
        let unpushed = head["unpublished"].as_u64().unwrap_or(0);
        out.push_str(&format!(
            "  {}  {} at {}{}\n",
            head["server"].as_str().unwrap_or(""),
            head["branch"].as_str().unwrap_or(""),
            short(head["head"].as_str().unwrap_or("")),
            if unpushed > 0 {
                format!("  ({unpushed} commit(s) on no remote)")
            } else {
                String::new()
            }
        ));
    }
    let others = data["others"].as_array().cloned().unwrap_or_default();
    if others.is_empty() {
        out.push_str("No other machine has published its checkouts yet.\n");
    }
    for record in others {
        out.push_str(&format!(
            "{} (published {})\n",
            record["machine"].as_str().unwrap_or(""),
            record["madeAt"].as_str().unwrap_or("")
        ));
        for head in record["heads"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  {}  {} at {}  {}{}\n",
                head["server"].as_str().unwrap_or(""),
                head["branch"].as_str().unwrap_or(""),
                short(head["head"].as_str().unwrap_or("")),
                head["relation"].as_str().unwrap_or(""),
                if head["pushed"] == true {
                    ""
                } else {
                    "  (not pushed)"
                }
            ));
        }
    }
    out
}
