//! What an apply would change, worked out from the stored selection and the items the latest
//! version of the source offers. Nothing here touches a file.

use super::discover::Found;
use super::store::{Item, Kind, Pending, SourceSel, State};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub kind: Kind,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Row {
    fn new(kind: Kind, id: &str) -> Self {
        Self {
            kind,
            id: id.to_string(),
            from: None,
            to: None,
            note: None,
        }
    }

    fn moved(kind: Kind, id: &str, from: &str, to: &str) -> Self {
        Self {
            from: Some(from.to_string()),
            to: Some(to.to_string()),
            ..Self::new(kind, id)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diffs {
    /// Items that enter the generated plugin.
    pub added: Vec<Row>,
    /// Items that leave it.
    pub dropped: Vec<Row>,
    /// Items that are on and whose content changed upstream; applied with the plan.
    pub changed: Vec<Row>,
    /// Ticked hooks whose command changed upstream; they keep the old command until ticked again.
    pub retick: Vec<Row>,
    /// Items the source gained; they stay off.
    pub fresh: Vec<Row>,
    /// Items the source lost; the pinned copy stays until decided.
    pub gone: Vec<Row>,
    /// Rules that are on: linked through the Library, not part of the generated plugin.
    pub rules: Vec<Row>,
}

impl Diffs {
    pub fn touches_slice(&self) -> bool {
        !(self.added.is_empty() && self.dropped.is_empty() && self.changed.is_empty())
    }
}

pub struct Computed {
    pub next: SourceSel,
    pub diffs: Diffs,
    /// Items copied from the latest version; every other item in the slice keeps its pinned copy.
    pub refresh: BTreeSet<String>,
}

fn carried(found: &Found, state: State) -> Item {
    Item {
        id: found.id.clone(),
        kind: found.kind,
        hash: found.hash.clone(),
        state,
        in_slice: false,
        path: found.rel.clone(),
        spec: found.spec.clone(),
        pending: None,
        extra: Default::default(),
    }
}

fn settle(prev: &Item, found: &Found, diffs: &mut Diffs, refresh: &mut BTreeSet<String>) -> Item {
    let mut next = prev.clone();
    next.path = found.rel.clone();
    if next.state == State::Removed {
        next.state = State::On;
    }
    if prev.kind == Kind::Hook && prev.state.wanted() {
        if found.hash == prev.hash {
            next.pending = None;
        } else {
            next.pending = Some(Pending {
                hash: found.hash.clone(),
                spec: found.spec.clone().unwrap_or_default(),
                extra: Default::default(),
            });
            diffs
                .retick
                .push(Row::moved(prev.kind, &prev.id, &prev.hash, &found.hash));
        }
    } else if found.hash != prev.hash {
        next.hash = found.hash.clone();
        next.spec = found.spec.clone();
        if prev.state.wanted() && prev.in_slice && prev.kind.in_slice() {
            diffs
                .changed
                .push(Row::moved(prev.kind, &prev.id, &prev.hash, &found.hash));
            refresh.insert(prev.key());
        }
    }
    let wants = next.state.wanted() && next.kind.in_slice();
    if wants && !prev.in_slice {
        diffs.added.push(Row::new(next.kind, &next.id));
        if next.kind != Kind::Hook {
            refresh.insert(next.key());
        }
    }
    if !wants && prev.in_slice {
        diffs.dropped.push(Row::new(next.kind, &next.id));
    }
    next.in_slice = wants;
    next
}

fn mark_renames(diffs: &mut Diffs, found: &[Found], prev: &SourceSel) {
    for gone in &mut diffs.gone {
        let Some(old) = prev.item(gone.kind, &gone.id) else {
            continue;
        };
        if let Some(same) = diffs.fresh.iter_mut().find(|f| {
            f.kind == gone.kind && found.iter().any(|x| x.id == f.id && x.hash == old.hash)
        }) {
            gone.note = Some(format!("same content as new {}", same.id));
            same.note = Some(format!("same content as {}", gone.id));
        }
    }
}

pub fn compute(prev: Option<&SourceSel>, found: &[Found], to_ref: &str, slice: &str) -> Computed {
    let empty = SourceSel::default();
    let before = prev.unwrap_or(&empty);
    let mut diffs = Diffs::default();
    let mut refresh = BTreeSet::new();
    let mut items: Vec<Item> = Vec::new();
    for f in found {
        match before.item(f.kind, &f.id) {
            None => {
                diffs.fresh.push(Row::new(f.kind, &f.id));
                items.push(carried(f, State::New));
            }
            Some(old) => items.push(settle(old, f, &mut diffs, &mut refresh)),
        }
    }
    for old in &before.items {
        if found.iter().any(|f| f.kind == old.kind && f.id == old.id) {
            continue;
        }
        let linked_rule = old.kind == Kind::Rule && before.linked.contains_key(&old.id);
        if old.state.wanted() && ((old.kind.in_slice() && old.in_slice) || linked_rule) {
            let mut kept = old.clone();
            kept.state = State::Removed;
            diffs.gone.push(Row::new(old.kind, &old.id));
            items.push(kept);
        } else if old.in_slice {
            diffs.dropped.push(Row::new(old.kind, &old.id));
        }
    }
    for item in &items {
        if item.kind == Kind::Rule && item.state == State::On {
            diffs.rules.push(Row::new(item.kind, &item.id));
        }
    }
    mark_renames(&mut diffs, found, before);
    items.sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    let slice_changed = diffs.touches_slice();
    let next = SourceSel {
        pinned: Some(to_ref.to_string()),
        slice: slice.to_string(),
        rev: before.rev + u64::from(slice_changed),
        support: before.support.clone(),
        items,
        linked: before.linked.clone(),
        extra: before.extra.clone(),
    };
    Computed {
        next,
        diffs,
        refresh,
    }
}
