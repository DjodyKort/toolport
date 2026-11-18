//! The `serverRepos` part: where each local server's git checkout stands, written by the machine
//! that owns the checkout into a row set of its own (`server-repos/<machine>.json`). Records of
//! other machines are carried through a push unchanged, so two machines never write one row.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use toolport_plus_api::ServerRepoHead;

use super::parts::{Build, Entry, Part};
use super::{hex, PartId};
use crate::tasks::cron::rfc3339;

pub const DIR: &str = "server-repos/";

/// One machine's published checkouts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineHeads {
    pub machine: String,
    #[serde(rename = "madeAt")]
    pub made_at: String,
    pub heads: Vec<ServerRepoHead>,
}

pub fn entry_name(machine: &str) -> String {
    let slug: String = machine
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    let tag = hex(&Sha256::digest(machine.as_bytes()));
    format!("{DIR}{slug}-{}.json", &tag[..8])
}

pub fn own_record(host: &dyn toolport_plus_api::Host, machine: &str) -> MachineHeads {
    let repos = crate::servers::repo::repos(host).unwrap_or_default();
    MachineHeads {
        machine: machine.to_string(),
        made_at: rfc3339(host.now_unix()),
        heads: crate::servers::repo::head_records(&repos),
    }
}

pub fn part(b: &Build<'_>) -> Part {
    let mut part = Part::new(PartId::ServerRepos);
    let machine = super::export::machine_name();
    let own = own_record(b.ctx.host, &machine);
    part.count = own.heads.len() as u64;
    let unpushed = own.heads.iter().filter(|h| !h.pushed).count();
    if unpushed > 0 {
        part.notes
            .push(format!("{unpushed} checkout(s) have commits on no remote"));
    }
    part.entries.push(Entry::json(
        &entry_name(&machine),
        &serde_json::to_value(&own).unwrap_or(Value::Null),
    ));
    for (name, record) in &b.opts.peer_records {
        if *name != machine {
            part.entries.push(Entry::json(&entry_name(name), record));
        }
    }
    part.info = json!({"machines": part.entries.len()});
    part
}
