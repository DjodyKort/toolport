//! The operations behind `library select ls|set|plan|apply|undo|check`. Reads and `set` change
//! only the selection store; `apply` is the one writer of the generated plugin and the settings,
//! and it takes the hash of a plan the caller has seen.

use super::discover::{self, Discovery, Found};
use super::plan::{self, Computed, Diffs, Row};
use super::slice::{self, MARKETPLACE};
use super::store::{self, Kind, Selections, SourceSel, State};
use crate::context::fsx;
use crate::context::plan::{Effects, PlanV1, ResultV1, Step};
use crate::fsx::{atomic_write, Mode};
use crate::hashing::lock_hash;
use crate::op::OpError;
use crate::plugins::claude::{ClaudeRunner, NOT_FOUND};
use crate::plugins::link::{
    edit_steps, note, write_edits, Edit, Settings, ENABLED_KEY, MARKETS_KEY, USER,
};
use crate::plugins::{marketplace, Env as PluginEnv};
use crate::skills::git::GitRunner;
use crate::skills::taps::{load_taps, redact, tap_dir, taps_root, valid_tap_name, Tap};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use toolport_plus_api::SettingsStore;

const VALIDATE_TIMEOUT: Duration = Duration::from_secs(120);
const JOURNAL: &str = "journal.json";
const RESTART: &str =
    "Run /reload-plugins in running Claude Code sessions, or restart them, to load the change";

pub struct Env<'a> {
    pub plugins: &'a PluginEnv,
    pub git: &'a dyn GitRunner,
    pub claude: &'a dyn ClaudeRunner,
    pub store: &'a dyn SettingsStore,
}

impl Env<'_> {
    pub fn base(&self) -> PathBuf {
        super::base_dir(&self.plugins.data_dir)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Fetch the latest version into a temporary clone; without it the local clone is used.
    pub fetch: bool,
    /// Also switch off the whole plugin that is installed beside the chosen items.
    pub switch: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SetArgs {
    pub on: Vec<String>,
    pub off: Vec<String>,
    pub tick_hook: Vec<String>,
}

struct Src {
    key: String,
    tap: Tap,
    clone: PathBuf,
    rel: String,
    base_name: String,
    whole_id: Option<String>,
}

impl Src {
    fn slice(&self) -> String {
        slice::slice_name(&self.base_name)
    }
}

fn resolve(env: &Env, key: &str) -> Result<Src, OpError> {
    let (tap_name, plugin) = match key.split_once('/') {
        Some((tap, plugin)) => (tap, Some(plugin)),
        None => (key, None),
    };
    valid_tap_name(tap_name).map_err(OpError::usage)?;
    let data = &env.plugins.data_dir;
    let tap = load_taps(data)
        .into_iter()
        .find(|t| t.name == tap_name)
        .ok_or_else(|| OpError::not_found(format!("Tap '{tap_name}' not found")))?;
    let clone = tap_dir(&taps_root(data), &tap);
    if !fsx::is_dir(&clone) {
        return Err(OpError::not_found(format!(
            "the clone of tap '{tap_name}' is missing; run `toolportctl skills tap update`"
        )));
    }
    match marketplace::read(&clone) {
        Some(Err(why)) => Err(OpError::conflict(why)),
        Some(Ok(market)) => {
            let chosen = match plugin {
                Some(name) => market.plugin(name).ok_or_else(|| {
                    OpError::not_found(format!(
                        "tap '{tap_name}' has no plugin '{name}' (it has: {})",
                        names(&market)
                    ))
                })?,
                None if market.plugins.len() == 1 => &market.plugins[0],
                None => {
                    return Err(OpError::usage(format!(
                        "tap '{tap_name}' is a plugin marketplace; name one of its plugins as \
                         {tap_name}/<plugin> ({})",
                        names(&market)
                    )))
                }
            };
            let rel = chosen
                .dir
                .strip_prefix(fsx::canonical(&clone))
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok(Src {
                key: format!("{tap_name}/{}", chosen.name),
                whole_id: Some(market.id_of(&chosen.name)),
                base_name: chosen.name.clone(),
                tap,
                clone,
                rel,
            })
        }
        None => match plugin {
            Some(_) => Err(OpError::usage(format!(
                "tap '{tap_name}' is not a plugin marketplace, so it has no plugins to name"
            ))),
            None => Ok(Src {
                key: tap_name.to_string(),
                base_name: tap.name.clone(),
                whole_id: None,
                tap,
                clone,
                rel: String::new(),
            }),
        },
    }
}

fn names(market: &marketplace::Marketplace) -> String {
    market
        .plugins
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

struct Latest {
    root: PathBuf,
    head: String,
    checked: bool,
    error: Option<String>,
    temp: Option<PathBuf>,
}

impl Drop for Latest {
    fn drop(&mut self) {
        if let Some(temp) = &self.temp {
            let _ = std::fs::remove_dir_all(temp);
            if let Some(parent) = temp.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }
}

fn join_rel(root: &Path, rel: &str) -> PathBuf {
    if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    }
}

fn head_of(env: &Env, dir: &Path) -> Result<String, OpError> {
    env.git.head(dir).map_err(|e| {
        OpError::conflict(format!(
            "cannot read the version of {}: {}",
            dir.display(),
            redact(&e)
        ))
    })
}

fn local(env: &Env, src: &Src, error: Option<String>) -> Result<Latest, OpError> {
    Ok(Latest {
        root: join_rel(&src.clone, &src.rel),
        head: head_of(env, &src.clone)?,
        checked: false,
        error,
        temp: None,
    })
}

fn fetch(env: &Env, src: &Src, online: bool) -> Result<Latest, OpError> {
    if !online {
        return local(env, src, None);
    }
    let temp = env
        .base()
        .join("tmp")
        .join(format!("{}-{}", src.base_name, std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    if let Some(parent) = temp.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cloned = env
        .git
        .clone_shallow(&src.tap.url, &temp)
        .and_then(|()| env.git.head(&temp));
    match cloned {
        Ok(head) => Ok(Latest {
            root: join_rel(&temp, &src.rel),
            head,
            checked: true,
            error: None,
            temp: Some(temp),
        }),
        Err(why) => {
            let _ = std::fs::remove_dir_all(&temp);
            local(env, src, Some(redact(&why)))
        }
    }
}

struct Prepared {
    selections: Selections,
    prev: Option<SourceSel>,
    latest: Latest,
    discovery: Discovery,
    computed: Computed,
}

fn prepare(env: &Env, src: &Src, online: bool) -> Result<Prepared, OpError> {
    let selections = store::load(&env.base())?;
    let slice = src.slice();
    if let Some((other, _)) = selections
        .sources
        .iter()
        .find(|(k, s)| **k != src.key && s.slice == slice)
    {
        return Err(OpError::conflict(format!(
            "{other} already uses the generated plugin name {slice}; rename one of the taps"
        )));
    }
    let latest = fetch(env, src, online)?;
    let discovery = discover::discover(&latest.root);
    let prev = selections.sources.get(&src.key).cloned();
    let computed = plan::compute(prev.as_ref(), &discovery.items, &latest.head, &slice);
    Ok(Prepared {
        selections,
        prev,
        latest,
        discovery,
        computed,
    })
}

fn whole_enabled(env: &Env, src: &Src) -> bool {
    let Some(id) = &src.whole_id else {
        return false;
    };
    Settings::load(env.store, env.plugins)
        .ok()
        .is_some_and(|s| s.doc[ENABLED_KEY][id] == json!(true))
}

fn offer(env: &Env, src: &Src) -> Value {
    match (&src.whole_id, whole_enabled(env, src)) {
        (Some(whole), true) => json!({
            "kind": "switch-to-chosen-items",
            "whole": whole,
            "slice": slice::plugin_id(&src.slice()),
            "text": "This plugin is switched on whole. `library select plan --switch` shows the change to load only the chosen items.",
        }),
        _ => Value::Null,
    }
}

fn item_json(item: &store::Item) -> Value {
    let mut row = json!({
        "id": item.id,
        "kind": item.kind,
        "state": item.state,
        "hash": item.hash,
        "inSlice": item.in_slice,
    });
    if let Some(spec) = &item.spec {
        row["command"] = spec["handler"]["command"].clone();
        row["event"] = spec["event"].clone();
        row["matcher"] = spec.get("matcher").cloned().unwrap_or(Value::Null);
    }
    if let Some(pending) = &item.pending {
        row["pending"] = json!({
            "hash": pending.hash,
            "command": pending.spec["handler"]["command"],
        });
    }
    row
}

fn count(items: &[store::Item], state: State) -> usize {
    items.iter().filter(|i| i.state == state).count()
}

/// The recorded selection of a source merged with what its local clone offers now.
pub fn ls(env: &Env, source: Option<&str>) -> Result<Value, OpError> {
    let selections = store::load(&env.base())?;
    let Some(key) = source else {
        return list(env, &selections);
    };
    let src = resolve(env, key)?;
    let found = discover::discover(&join_rel(&src.clone, &src.rel));
    let mut sel = selections
        .sources
        .get(&src.key)
        .cloned()
        .unwrap_or_default();
    for f in &found.items {
        if sel.item(f.kind, &f.id).is_none() {
            sel.items.push(seed(f));
        }
    }
    sel.items
        .sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    Ok(json!({
        "source": src.key,
        "tap": src.tap.name,
        "ref": sel.pinned,
        "slice": slice::plugin_id(&src.slice()),
        "recorded": selections.sources.contains_key(&src.key),
        "on": count(&sel.items, State::On),
        "total": sel.items.len(),
        "new": count(&sel.items, State::New),
        "removed": count(&sel.items, State::Removed),
        "items": sel.items.iter().map(item_json).collect::<Vec<_>>(),
        "offer": offer(env, &src),
        "warnings": found.warnings,
    }))
}

fn list(env: &Env, selections: &Selections) -> Result<Value, OpError> {
    let sources: Vec<Value> = selections
        .sources
        .iter()
        .map(|(key, sel)| {
            json!({
                "source": key,
                "ref": sel.pinned,
                "slice": slice::plugin_id(&sel.slice),
                "on": count(&sel.items, State::On),
                "total": sel.items.len(),
                "new": count(&sel.items, State::New),
                "removed": count(&sel.items, State::Removed),
            })
        })
        .collect();
    let mut offers = Vec::new();
    for tap in load_taps(&env.plugins.data_dir) {
        let clone = tap_dir(&taps_root(&env.plugins.data_dir), &tap);
        let Some(Ok(market)) = marketplace::read(&clone) else {
            continue;
        };
        for plugin in &market.plugins {
            if let Ok(src) = resolve(env, &format!("{}/{}", tap.name, plugin.name)) {
                let o = offer(env, &src);
                if !o.is_null() && !selections.sources.contains_key(&src.key) {
                    offers.push(json!({"source": src.key, "offer": o}));
                }
            }
        }
    }
    Ok(json!({"sources": sources, "offers": offers}))
}

fn seed(found: &Found) -> store::Item {
    store::Item {
        id: found.id.clone(),
        kind: found.kind,
        hash: found.hash.clone(),
        state: State::New,
        in_slice: false,
        path: found.rel.clone(),
        spec: found.spec.clone(),
        pending: None,
        extra: Default::default(),
    }
}

fn find_item(sel: &SourceSel, reference: &str) -> Result<(Kind, String), OpError> {
    let exact = reference.split_once(':').and_then(|(kind, id)| {
        let kind = Kind::parse(kind)?;
        sel.item(kind, id).map(|i| (i.kind, i.id.clone()))
    });
    if let Some(found) = exact {
        return Ok(found);
    }
    let hits: Vec<&store::Item> = sel.items.iter().filter(|i| i.id == reference).collect();
    match hits.as_slice() {
        [one] => Ok((one.kind, one.id.clone())),
        [] => Err(OpError::not_found(format!(
            "no item '{reference}' in this source; `library select ls <source>` lists them"
        ))),
        many => Err(OpError::usage(format!(
            "'{reference}' is ambiguous: {}",
            many.iter().map(|i| i.key()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

/// Records what is wanted on and off. Nothing reaches Claude Code until a plan is applied.
pub fn set(env: &Env, source: &str, args: &SetArgs, dry_run: bool) -> Result<Value, OpError> {
    let src = resolve(env, source)?;
    let mut selections = store::load(&env.base())?;
    let mut sel = selections
        .sources
        .get(&src.key)
        .cloned()
        .unwrap_or_default();
    if sel.slice.is_empty() {
        sel.slice = src.slice();
    }
    if !selections.sources.contains_key(&src.key) {
        let found = discover::discover(&join_rel(&src.clone, &src.rel));
        sel.items = found.items.iter().map(seed).collect();
    }
    let mut changed: Vec<String> = Vec::new();
    for reference in &args.on {
        let (kind, id) = find_item(&sel, reference)?;
        if kind == Kind::Hook {
            return Err(OpError::usage(format!(
                "hook '{id}' runs a command in every session; tick it with --tick-hook so the \
                 command is read first"
            )));
        }
        let item = sel.item_mut(kind, &id).expect("found above");
        item.state = State::On;
        changed.push(format!("{} on", item.key()));
    }
    for reference in &args.tick_hook {
        let (kind, id) = find_item(&sel, reference)?;
        if kind != Kind::Hook {
            return Err(OpError::usage(format!(
                "'{reference}' is not a hook; use --on"
            )));
        }
        let item = sel.item_mut(kind, &id).expect("found above");
        if let Some(pending) = item.pending.take() {
            item.hash = pending.hash;
            item.spec = Some(pending.spec);
            item.in_slice = false;
        }
        item.state = State::On;
        changed.push(format!("{} ticked", item.key()));
    }
    for reference in &args.off {
        let (kind, id) = find_item(&sel, reference)?;
        let item = sel.item_mut(kind, &id).expect("found above");
        item.state = State::Off;
        item.pending = None;
        changed.push(format!("{} off", item.key()));
    }
    if !dry_run {
        selections.sources.insert(src.key.clone(), sel.clone());
        store::save(&env.base(), &selections)?;
    }
    Ok(json!({
        "source": src.key,
        "dryRun": dry_run,
        "changed": changed,
        "on": count(&sel.items, State::On),
        "total": sel.items.len(),
        "next": format!("toolportctl library select plan {}", src.key),
    }))
}

fn settings_edits(
    settings: &Settings,
    market_root: &Path,
    src: &Src,
    next: &SourceSel,
    others: usize,
    switch: bool,
) -> Vec<Edit> {
    let id = slice::plugin_id(&next.slice);
    let root = fsx::display(market_root);
    let declared = settings.doc[MARKETS_KEY].get(MARKETPLACE).cloned();
    let in_place = declared
        .as_ref()
        .is_some_and(|d| d["source"]["path"].as_str() == Some(root.as_str()));
    let has = slice::has_content(next);
    let mut edits = Vec::new();
    if has {
        if !in_place {
            edits.push(Edit {
                key: MARKETS_KEY,
                entry: MARKETPLACE.to_string(),
                action: "declare",
                value: Some(json!({"source": {"source": "directory", "path": root}})),
            });
        }
        if settings.doc[ENABLED_KEY][&id] != json!(true) {
            edits.push(Edit {
                key: ENABLED_KEY,
                entry: id,
                action: "enable",
                value: Some(json!(true)),
            });
        }
    } else {
        if settings.doc[ENABLED_KEY].get(&id).is_some() {
            edits.push(Edit {
                key: ENABLED_KEY,
                entry: id,
                action: "remove",
                value: None,
            });
        }
        if others == 0 && declared.is_some() {
            edits.push(Edit {
                key: MARKETS_KEY,
                entry: MARKETPLACE.to_string(),
                action: "remove",
                value: None,
            });
        }
    }
    if let (true, Some(whole)) = (switch, &src.whole_id) {
        if settings.doc[ENABLED_KEY][whole] == json!(true) {
            edits.push(Edit {
                key: ENABLED_KEY,
                entry: whole.clone(),
                action: "disable",
                value: Some(json!(false)),
            });
        }
    }
    edits
}

fn others_with_content(selections: &Selections, key: &str) -> usize {
    selections
        .sources
        .iter()
        .filter(|(k, s)| k.as_str() != key && slice::has_content(s))
        .count()
}

fn step(op: &'static str, detail: String) -> Step {
    Step {
        op,
        path: None,
        detail,
        keys: None,
        diff: None,
    }
}

fn ids(rows: &[Row]) -> String {
    rows.iter()
        .take(8)
        .map(|r| format!("{}:{}", r.kind.as_str(), r.id))
        .collect::<Vec<_>>()
        .join(", ")
        + if rows.len() > 8 { ", ..." } else { "" }
}

fn steps_of(diffs: &Diffs, slice_id: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    for row in &diffs.added {
        steps.push(step(
            "add",
            format!("{}:{} joins {slice_id}", row.kind.as_str(), row.id),
        ));
    }
    for row in &diffs.dropped {
        steps.push(step(
            "remove",
            format!("{}:{} leaves {slice_id}", row.kind.as_str(), row.id),
        ));
    }
    for row in &diffs.changed {
        steps.push(step(
            "update",
            format!(
                "{}:{} content changed upstream; the copy is replaced",
                row.kind.as_str(),
                row.id
            ),
        ));
    }
    for row in &diffs.retick {
        steps.push(step(
            "hold",
            format!(
                "hook {} changed its command upstream; the old command keeps running until \
                 `library select set --tick-hook` ticks it again",
                row.id
            ),
        ));
    }
    for row in &diffs.gone {
        steps.push(step(
            "hold",
            format!(
                "{}:{} is no longer in the source; the pinned copy stays until you turn it off",
                row.kind.as_str(),
                row.id
            ),
        ));
    }
    if !diffs.fresh.is_empty() {
        steps.push(note(format!(
            "{} new item(s) in the source stay off: {}",
            diffs.fresh.len(),
            ids(&diffs.fresh)
        )));
    }
    if !diffs.rules.is_empty() {
        steps.push(note(format!(
            "rules are instruction files and are not part of the generated plugin; link them \
             through the Library: {}",
            ids(&diffs.rules)
        )));
    }
    steps
}

struct Built {
    data: Value,
    hash: String,
    edits: Vec<Edit>,
    settings: Settings,
}

fn build_plan(env: &Env, src: &Src, prepared: &Prepared, opts: Options) -> Result<Built, OpError> {
    let Computed { next, diffs, .. } = &prepared.computed;
    let settings = Settings::load(env.store, env.plugins)?;
    let others = others_with_content(&prepared.selections, &src.key);
    let edits = settings_edits(
        &settings,
        &slice::market_root(&env.base()),
        src,
        next,
        others,
        opts.switch,
    );
    let slice_id = slice::plugin_id(&next.slice);
    let mut steps = steps_of(diffs, &slice_id);
    steps.extend(edit_steps(&settings, &edits));
    steps.push(note(RESTART.to_string()));
    let mut warnings = prepared.discovery.warnings.clone();
    if let Some(why) = &prepared.latest.error {
        warnings.push(format!(
            "could not check the source ({why}); this plan uses the local clone at {}",
            short(&prepared.latest.head)
        ));
    }
    if next
        .items
        .iter()
        .any(|i| i.kind == Kind::Hook && i.state.wanted())
    {
        warnings.push("ticked hooks run commands in every session once the plan is applied".into());
    }
    if opts.switch && !whole_enabled(env, src) {
        warnings.push("--switch has nothing to switch: the whole plugin is not switched on".into());
    }
    let undo = format!("toolportctl library select undo {}", src.key);
    let plan = PlanV1 {
        summary: format!(
            "Load the chosen items of {} through {slice_id}: {}",
            src.key,
            summary_line(next)
        ),
        steps,
        effects: Effects::default(),
        warnings,
        undo,
    };
    let hash = plan_hash(&src.key, prepared, &edits);
    let waiting = diffs.touches_slice()
        || !diffs.retick.is_empty()
        || !diffs.fresh.is_empty()
        || !diffs.gone.is_empty()
        || !edits.is_empty()
        || prepared.prev.as_ref().and_then(|p| p.pinned.as_deref()) != Some(&prepared.latest.head);
    let data = json!({
        "id": src.key,
        "scope": USER,
        "cwd": null,
        "dryRun": true,
        "plan": plan,
        "result": null,
        "changes": edits.iter().map(|e| e.change(&settings.path)).collect::<Vec<_>>(),
        "conflicts": [],
        "source": src.key,
        "slice": slice_id,
        "planHash": hash,
        "waiting": waiting,
        "ref": {
            "pinned": prepared.prev.as_ref().and_then(|p| p.pinned.clone()),
            "latest": prepared.latest.head,
        },
        "checked": prepared.latest.checked,
        "fetchError": prepared.latest.error,
        "diffs": diffs,
        "offer": offer(env, src),
    });
    Ok(Built {
        data,
        hash,
        edits,
        settings,
    })
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

fn summary_line(next: &SourceSel) -> String {
    let on = next
        .items
        .iter()
        .filter(|i| i.kind.in_slice() && i.state.wanted())
        .count();
    format!("{on} of {} item(s) on", next.items.len())
}

fn plan_hash(key: &str, prepared: &Prepared, edits: &[Edit]) -> String {
    let body = json!({
        "source": key,
        "from": prepared.prev.as_ref().and_then(|p| p.pinned.clone()),
        "to": prepared.latest.head,
        "diffs": prepared.computed.diffs,
        "next": prepared.computed.next,
        "edits": edits.iter().map(|e| json!([e.key, e.entry, e.action])).collect::<Vec<_>>(),
    });
    lock_hash(body.to_string())
}

/// What an apply would do. Fetches the latest version of the source into a temporary clone when
/// asked to and writes nothing else.
pub fn plan(env: &Env, source: &str, opts: Options) -> Result<Value, OpError> {
    let src = resolve(env, source)?;
    let prepared = prepare(env, &src, opts.fetch)?;
    Ok(build_plan(env, &src, &prepared, opts)?.data)
}

/// One line per source for the scheduled check.
pub fn summary(env: &Env, source: &str, online: bool) -> Result<Value, OpError> {
    let src = resolve(env, source)?;
    let prepared = prepare(env, &src, online)?;
    let built = build_plan(
        env,
        &src,
        &prepared,
        Options {
            fetch: online,
            switch: false,
        },
    )?;
    let d = &prepared.computed.diffs;
    let moved =
        prepared.prev.as_ref().and_then(|p| p.pinned.as_deref()) != Some(&prepared.latest.head);
    let waiting = moved
        && (!d.changed.is_empty()
            || !d.retick.is_empty()
            || !d.fresh.is_empty()
            || !d.gone.is_empty());
    Ok(json!({
        "source": src.key,
        "checked": prepared.latest.checked,
        "fetchError": prepared.latest.error,
        "waiting": waiting,
        "latest": prepared.latest.head,
        "changed": d.changed.len(),
        "retick": d.retick.len(),
        "fresh": d.fresh.len(),
        "gone": d.gone.len(),
        "planHash": built.hash,
    }))
}

#[derive(Serialize, Deserialize)]
struct Undone {
    key: String,
    entry: String,
    before: Option<Value>,
}

#[derive(Serialize, Deserialize)]
struct Journal {
    prev: Option<SourceSel>,
    slice: String,
    files: bool,
    had_slice: bool,
    settings: Vec<Undone>,
}

fn undo_dir(base: &Path, key: &str) -> PathBuf {
    base.join("undo").join(key.replace('/', "~"))
}

fn entries_of(selections: &Selections) -> Vec<Value> {
    selections
        .sources
        .iter()
        .filter(|(_, s)| slice::has_content(s))
        .map(|(key, s)| slice::entry(s, key))
        .collect()
}

fn validate(env: &Env, base: &Path) -> Result<Option<String>, OpError> {
    if !env.claude.present() {
        return Ok(Some(
            "claude was not found, so the generated marketplace was not validated".into(),
        ));
    }
    let root = fsx::display(&slice::market_root(base));
    match env
        .claude
        .run(&["plugin", "validate", root.as_str()], VALIDATE_TIMEOUT)
    {
        Ok(out) if out.ok() => Ok(None),
        Ok(out) => Err(OpError::conflict(format!(
            "claude plugin validate refused the generated marketplace: {}; nothing was changed",
            out.first_error_line()
        ))),
        Err(why) if why == NOT_FOUND => Ok(Some(
            "claude was not found, so the generated marketplace was not validated".into(),
        )),
        Err(why) => Err(OpError::conflict(format!(
            "claude plugin validate could not run: {why}; nothing was changed"
        ))),
    }
}

fn io(why: impl std::fmt::Display) -> OpError {
    OpError::conflict(why.to_string())
}

/// Puts the slice folder, the record and the marketplace file back as the journal has them.
fn restore_files(
    base: &Path,
    key: &str,
    journal: &Journal,
    selections: &mut Selections,
) -> Result<(), OpError> {
    if journal.files {
        let now = slice::slice_dir(base, &journal.slice);
        let _ = std::fs::remove_dir_all(&now);
        let kept = undo_dir(base, key).join("slice");
        if journal.had_slice {
            std::fs::rename(&kept, &now).map_err(io)?;
        }
    }
    match &journal.prev {
        Some(prev) => selections.sources.insert(key.to_string(), prev.clone()),
        None => selections.sources.remove(key),
    };
    slice::write_marketplace(base, entries_of(selections)).map_err(io)?;
    store::save(base, selections)
}

fn restore_settings(env: &Env, journal: &Journal) -> Result<(), OpError> {
    let settings = Settings::load(env.store, env.plugins)?;
    let edits: Vec<Edit> = journal
        .settings
        .iter()
        .map(|u| Edit {
            key: if u.key == MARKETS_KEY {
                MARKETS_KEY
            } else {
                ENABLED_KEY
            },
            entry: u.entry.clone(),
            action: "restore",
            value: u.before.clone(),
        })
        .collect();
    write_edits(env.store, &settings, &edits)
}

fn changed_lines(edits: &[Edit], settings: &Settings) -> Vec<String> {
    edits
        .iter()
        .map(|e| format!("{} in {}", e.path(), fsx::display(&settings.path)))
        .collect()
}

/// Applies the plan whose hash is `plan_hash`. The plan is rebuilt first; when the source or the
/// settings moved since it was made the hashes differ and nothing is written.
pub fn apply(env: &Env, source: &str, plan_hash: &str, opts: Options) -> Result<Value, OpError> {
    let src = resolve(env, source)?;
    let base = env.base();
    let prepared = prepare(env, &src, opts.fetch)?;
    let built = build_plan(env, &src, &prepared, opts)?;
    if built.hash != plan_hash {
        return Err(OpError::conflict(format!(
            "the plan changed since it was made (now {}); run `toolportctl library select plan {}` \
             again and apply that",
            built.hash, src.key
        ))
        .with_details(json!({"planHash": built.hash})));
    }
    let Prepared {
        mut selections,
        prev,
        latest,
        computed,
        ..
    } = prepared;
    let Computed {
        next,
        diffs,
        refresh,
    } = computed;
    let old_slice = slice::slice_dir(&base, &next.slice);
    let regenerate = diffs.touches_slice();
    let journal_dir = undo_dir(&base, &src.key);
    let _ = std::fs::remove_dir_all(&journal_dir);
    let mut journal = Journal {
        prev: prev.clone(),
        slice: next.slice.clone(),
        files: regenerate,
        had_slice: false,
        settings: Vec::new(),
    };
    let mut warnings: Vec<String> = Vec::new();
    if regenerate {
        let staging = base.join("tmp").join(format!("{}.new", next.slice));
        let _ = std::fs::remove_dir_all(&staging);
        if let Err(why) = slice::build(&staging, &next, &latest.root, &old_slice, &refresh) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(OpError::conflict(format!("{why}; nothing was changed")));
        }
        std::fs::create_dir_all(&journal_dir).map_err(io)?;
        if old_slice.exists() {
            std::fs::rename(&old_slice, journal_dir.join("slice")).map_err(io)?;
            journal.had_slice = true;
        }
        if slice::has_content(&next) {
            std::fs::create_dir_all(slice::market_root(&base)).map_err(io)?;
            std::fs::rename(&staging, &old_slice).map_err(io)?;
        } else {
            let _ = std::fs::remove_dir_all(&staging);
        }
    }
    let backup = selections.clone();
    selections.sources.insert(src.key.clone(), next.clone());
    slice::write_marketplace(&base, entries_of(&selections)).map_err(io)?;
    if regenerate && slice::has_content(&next) {
        match validate(env, &base) {
            Ok(warning) => warnings.extend(warning),
            Err(error) => {
                let mut restored = backup;
                restore_files(&base, &src.key, &journal, &mut restored)?;
                let _ = std::fs::remove_dir_all(&journal_dir);
                return Err(error);
            }
        }
    }
    store::save(&base, &selections)?;
    journal.settings = built
        .edits
        .iter()
        .map(|e| Undone {
            key: e.key.to_string(),
            entry: e.entry.clone(),
            before: built.settings.doc[e.key].get(&e.entry).cloned(),
        })
        .collect();
    std::fs::create_dir_all(&journal_dir).map_err(io)?;
    let text = serde_json::to_string_pretty(&journal).map_err(io)?;
    atomic_write(
        &journal_dir.join(JOURNAL),
        &format!("{text}\n"),
        Mode::Private,
    )
    .map_err(io)?;
    if let Err(error) = write_edits(env.store, &built.settings, &built.edits) {
        let mut restored = backup_without(&selections, &journal, &src.key);
        restore_files(&base, &src.key, &journal, &mut restored)?;
        let _ = std::fs::remove_dir_all(&journal_dir);
        return Err(error);
    }
    let mut data = built.data;
    data["dryRun"] = json!(false);
    data["waiting"] = json!(false);
    data["result"] = json!(ResultV1 {
        applied: true,
        changed: changed_lines(&built.edits, &built.settings),
        undo: format!("toolportctl library select undo {}", src.key),
        backups: Vec::new(),
    });
    data["warnings"] = json!(warnings);
    Ok(data)
}

fn backup_without(selections: &Selections, journal: &Journal, key: &str) -> Selections {
    let mut copy = selections.clone();
    match &journal.prev {
        Some(prev) => copy.sources.insert(key.to_string(), prev.clone()),
        None => copy.sources.remove(key),
    };
    copy
}

/// Puts back what the last apply for this source changed: the record, the generated folder and
/// the settings entries it wrote.
pub fn undo(env: &Env, source: &str) -> Result<Value, OpError> {
    let src = resolve(env, source)?;
    let base = env.base();
    let dir = undo_dir(&base, &src.key);
    let text = std::fs::read_to_string(dir.join(JOURNAL))
        .map_err(|_| OpError::not_found(format!("nothing to undo for {}", src.key)))?;
    let journal: Journal = serde_json::from_str(&text).map_err(io)?;
    let mut selections = store::load(&base)?;
    restore_files(&base, &src.key, &journal, &mut selections)?;
    restore_settings(env, &journal)?;
    let _ = std::fs::remove_dir_all(&dir);
    let settings = Settings::load(env.store, env.plugins)?;
    let plan = PlanV1 {
        summary: format!(
            "Restore the selection of {} from before the last apply",
            src.key
        ),
        steps: vec![note(RESTART.to_string())],
        effects: Effects::default(),
        warnings: Vec::new(),
        undo: format!("toolportctl library select plan {}", src.key),
    };
    Ok(json!({
        "id": src.key,
        "scope": USER,
        "cwd": null,
        "dryRun": false,
        "plan": plan,
        "result": ResultV1 {
            applied: true,
            changed: journal
                .settings
                .iter()
                .map(|u| format!("{}.{} in {}", u.key, u.entry, fsx::display(&settings.path)))
                .collect(),
            undo: plan.undo.clone(),
            backups: Vec::new(),
        },
        "changes": [],
        "conflicts": [],
        "source": src.key,
    }))
}
