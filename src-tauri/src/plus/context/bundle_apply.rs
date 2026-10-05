//! Apply, undo and status of a context bundle (D-064). A bundle reaches Claude Code through two
//! git-ignored files in the folder where Claude starts (F0 R4: a parent folder's settings are
//! ignored): the keys it owns in `.claude/settings.local.json` and one managed block in
//! `CLAUDE.local.md`. Every key Toolport writes is recorded in the ledger with the value it
//! replaced, so undo puts back exactly that and leaves anything changed since alone. All edits
//! are surgical (`bundle_json`): the bytes of every key Toolport does not own stay as they were.

use super::bundle::Bundle;
use super::bundle_io::{self, Action};
use super::bundle_json::{self as json, Found};
use super::bundle_ledger::{self as ledger, BlockRec, ExcludeRec, FolderRec, Owned, SettingsRec};
use super::bundle_store::{self, BundleError};
use super::globs::glob_match;
use super::layers;
use super::loads::{what_loads_with, LoadsOptions};
use super::roots::Roots;
use crate::plus::plugins::adapters::Registry;
use crate::plus::hashing::sha256_hex;
use crate::plus::plan::{Diff, Effects, PlanV1, ResultV1, Step, TokenEffect};
use crate::plus::sources::fsx;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const SETTINGS_REL: &str = ".claude/settings.local.json";
pub const BLOCK_FILE: &str = "CLAUDE.local.md";
const TRIES: usize = 5;
const HYGIENE: &str = "Hiding skills is hygiene, not savings (F0 measured 59 tokens for ten skills); the tokens are in plugins, agents and memory files";

#[derive(Clone, Copy)]
pub struct World<'a> {
    pub roots: &'a Roots,
    pub data_dir: &'a Path,
}

pub fn folder_key(cwd: &Path) -> String {
    fsx::display(&fsx::canonical(cwd))
}

fn failed(message: impl Into<String>) -> BundleError {
    BundleError::new("failed", message)
}

pub(super) fn settings_path(cwd: &Path) -> PathBuf {
    cwd.join(SETTINGS_REL)
}

fn block_path(cwd: &Path) -> PathBuf {
    cwd.join(BLOCK_FILE)
}

fn undo_command(cwd: &Path) -> String {
    format!("toolportctl context bundle undo --cwd {}", cwd.display())
}

fn same(raw_a: &str, raw_b: &str) -> bool {
    match (serde_json::from_str::<Value>(raw_a), serde_json::from_str::<Value>(raw_b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[derive(Default)]
pub struct Desired {
    pub overrides: Vec<(String, &'static str)>,
    pub plugins: Vec<String>,
    pub excludes: Vec<String>,
    pub denies: Vec<String>,
    pub env: Vec<(String, String)>,
    pub deny_servers: Vec<String>,
    pub layers: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

fn library_skills(roots: &Roots) -> BTreeSet<String> {
    fsx::list_dir(&roots.skills_repo_path().join("skills"))
        .into_iter()
        .filter(|e| e.kind == fsx::Kind::Dir)
        .map(|e| e.name)
        .collect()
}

fn expand(label: &str, patterns: &[String], known: &BTreeSet<String>, warnings: &mut Vec<String>) -> Vec<String> {
    let mut out = BTreeSet::new();
    for pattern in patterns {
        if pattern.contains(['*', '?', '[', '{']) {
            let hits: Vec<&String> = known
                .iter()
                .filter(|n| !n.contains(':') && glob_match(pattern, n))
                .collect();
            if hits.is_empty() {
                warnings.push(format!("{label}: {pattern} matches no skill that is known here"));
            }
            out.extend(hits.into_iter().cloned());
        } else {
            out.insert(pattern.clone());
        }
    }
    out.into_iter().collect()
}

pub fn desired(w: &World, bundle: &Bundle, cwd: &Path) -> Desired {
    let mut out = Desired::default();
    let library = library_skills(w.roots);
    let visible: BTreeSet<String> = super::measure::cached_as_is(w.data_dir, cwd, None)
        .map(|(run, _)| run.skill_names.into_iter().collect())
        .unwrap_or_default();
    let known: BTreeSet<String> = library.union(&visible).cloned().collect();
    let mut map: BTreeMap<String, &'static str> = BTreeMap::new();
    if !bundle.skills_allow.is_empty() {
        let keep: BTreeSet<String> =
            expand("skills.allow", &bundle.skills_allow, &known, &mut out.warnings).into_iter().collect();
        for name in library.difference(&keep) {
            map.insert(name.clone(), "off");
        }
    }
    for name in expand("skills.name_only", &bundle.skills_name_only, &known, &mut out.warnings) {
        map.insert(name, "name-only");
    }
    for name in expand("skills.off", &bundle.skills_off, &known, &mut out.warnings) {
        map.insert(name, "off");
    }
    out.overrides = map.into_iter().collect();
    out.plugins = bundle.plugins_off.clone();
    out.excludes = bundle.layers_exclude.clone();
    out.denies = bundle.agents_off.iter().map(|a| format!("Agent({a})")).collect();
    let available = layers::list_layers(w.roots);
    for name in &bundle.layers_add {
        match available.iter().find(|l| &l.name == name) {
            Some(layer) => out
                .layers
                .push((name.clone(), layers::body_of(&layer.path).trim().to_string())),
            None => out
                .warnings
                .push(format!("layers.add: no layer named {name} in the skills repo's rules/")),
        }
    }
    let registry = Registry::load(Some(w.data_dir));
    let (env, mut notes) = crate::plus::plugins::config::bundle_env(&registry, &bundle.plugins_config);
    out.env = env;
    out.warnings.append(&mut notes);
    out.deny_servers = bundle.mcp_deny.clone();
    out
}

fn markers(name: &str) -> (String, String) {
    (
        format!("<!-- toolport:bundle:begin {name} -->"),
        format!("<!-- toolport:bundle:end {name} -->"),
    )
}

fn block_text(name: &str, layers: &[(String, String)]) -> String {
    let (begin, end) = markers(name);
    let mut out = format!(
        "{begin}\n<!-- Written by `toolportctl context bundle apply {name}`; `context bundle undo` removes this block. -->\n"
    );
    for (layer, body) in layers {
        out.push_str(&format!("\n## {layer}\n\n{body}\n"));
    }
    out.push_str(&format!("{end}\n"));
    out
}

fn block_span(text: &str, name: &str) -> Option<(usize, usize)> {
    let (begin, end) = markers(name);
    let start = text.find(&begin)?;
    let stop = start + text[start..].find(&end)? + end.len();
    let stop = if text[stop..].starts_with('\n') { stop + 1 } else { stop };
    Some((start, stop))
}

/// The bundle block of a layer-managed `CLAUDE.local.md`, which a layer deploy rewrites and has
/// to carry over so that `context sync` does not drop what `bundle apply` put there.
pub(super) fn carried_block(text: &str) -> Option<&str> {
    let start = text.find("<!-- toolport:bundle:begin ")?;
    let marker = "<!-- toolport:bundle:end ";
    let end = start + text[start..].find(marker)?;
    let close = end + text[end..].find("-->")? + "-->".len();
    let stop = if text[close..].starts_with('\n') { close + 1 } else { close };
    Some(&text[start..stop])
}

fn block_intact(text: &str, name: &str, rec: &BlockRec) -> bool {
    block_span(text, name).is_some_and(|(a, b)| sha256_hex(&text[a..b]) == rec.sha256)
}

fn undo_block(working: &mut String, name: &str, rec: &BlockRec) -> Result<(), ()> {
    let Some((start, stop)) = block_span(working, name) else {
        return Ok(());
    };
    if sha256_hex(&working[start..stop]) != rec.sha256 {
        return Err(());
    }
    let from = if working[..start].ends_with(&rec.prefix) {
        start - rec.prefix.len()
    } else {
        start
    };
    working.replace_range(from..stop, "");
    Ok(())
}

/// What undoing a settings record does to the file's current text: the keys put back, the file
/// deleted when Toolport created it and nothing else is left in it.
pub(super) fn undo_action(text: Option<&str>, rec: &SettingsRec) -> Result<(Action, Vec<String>), String> {
    let Some(text) = text else {
        return Ok((Action::Keep, Vec::new()));
    };
    let mut working = text.to_string();
    let conflicts = undo_settings(&mut working, rec)?;
    let action = if rec.created_file && !json::has_members(&working, &[]) {
        Action::Delete
    } else if working == text {
        Action::Keep
    } else {
        Action::Write(working)
    };
    Ok((action, conflicts))
}

pub(super) fn undo_settings(working: &mut String, rec: &SettingsRec) -> Result<Vec<String>, String> {
    let mut conflicts = Vec::new();
    for owned in rec.owned.iter().rev() {
        let path: Vec<&str> = owned.path.iter().map(String::as_str).collect();
        if owned.kind == "entry" {
            let item: Value = serde_json::from_str(&owned.written).map_err(|e| e.to_string())?;
            json::pull(working, &path, &item)?;
            continue;
        }
        match json::get(working, &path) {
            None => {}
            Some(Found::Scalar(raw)) if same(&raw, &owned.written) => {
                if owned.existed {
                    json::set(working, &path, owned.before.as_deref().unwrap_or("null"))?;
                } else {
                    json::remove(working, &path)?;
                }
            }
            Some(_) => conflicts.push(owned.label()),
        }
    }
    for container in rec.created_containers.iter().rev() {
        let parts: Vec<&str> = container.split('.').collect();
        json::prune_empty(working, &parts)?;
    }
    Ok(conflicts)
}

fn own_set(text: &mut String, rec: &mut SettingsRec, path: &[&str], written: &str, warnings: &mut Vec<String>) {
    let (existed, before) = match json::get(text, path) {
        Some(Found::Container) => {
            warnings.push(format!("{} holds an object or list, not a value; left alone", path.join(".")));
            return;
        }
        Some(Found::Scalar(raw)) if same(&raw, written) => return,
        Some(Found::Scalar(raw)) if raw.len() > 80 => {
            warnings.push(format!("{} holds a long value; left alone", path.join(".")));
            return;
        }
        Some(Found::Scalar(raw)) => (true, Some(raw)),
        None => (false, None),
    };
    match json::set(text, path, written) {
        Ok(created) => {
            rec.created_containers.extend(created);
            rec.owned.push(Owned {
                path: path.iter().map(|s| s.to_string()).collect(),
                kind: "set".into(),
                existed,
                before,
                written: written.to_string(),
            });
        }
        Err(e) => warnings.push(format!("{}: {e}; left alone", path.join("."))),
    }
}

fn own_entry(text: &mut String, rec: &mut SettingsRec, path: &[&str], entry: &str, warnings: &mut Vec<String>) {
    own_item(text, rec, path, Value::String(entry.to_string()), warnings);
}

pub(super) fn own_item(text: &mut String, rec: &mut SettingsRec, path: &[&str], item: Value, warnings: &mut Vec<String>) {
    match json::value(text, path) {
        Some(Value::Array(items)) if items.contains(&item) => return,
        Some(Value::Array(_)) | None => {}
        Some(_) => {
            warnings.push(format!("{} is not a list; left alone", path.join(".")));
            return;
        }
    }
    let raw = item.to_string();
    match json::push(text, path, &raw) {
        Ok(created) => {
            rec.created_containers.extend(created);
            rec.owned.push(Owned {
                path: path.iter().map(|s| s.to_string()).collect(),
                kind: "entry".into(),
                existed: false,
                before: None,
                written: raw,
            });
        }
        Err(e) => warnings.push(format!("{}: {e}; left alone", path.join("."))),
    }
}

pub(super) struct SettingsOut {
    pub action: Action,
    pub rec: SettingsRec,
    pub conflicts: Vec<String>,
    pub warnings: Vec<String>,
}

pub(super) fn compute_settings(
    text: Option<&str>,
    prior: Option<&SettingsRec>,
    want: &Desired,
    cwd: &Path,
    dir_missing: bool,
) -> Result<SettingsOut, String> {
    let existed = text.is_some();
    let mut working = text.unwrap_or("{}").to_string();
    if existed {
        json::check(&working).map_err(|e| format!("{SETTINGS_REL}: {e}; left alone"))?;
    }
    let (mut created_file, mut created_dir) = (!existed, dir_missing);
    let mut conflicts = Vec::new();
    if let Some(p) = prior {
        conflicts = undo_settings(&mut working, p)?;
        created_file = !existed || (p.created_file && !json::has_members(&working, &[]));
        created_dir = created_dir || p.created_dir;
    }
    let mut rec = SettingsRec {
        file: fsx::display(&settings_path(cwd)),
        created_file,
        created_dir: created_dir && created_file,
        ..SettingsRec::default()
    };
    let mut warnings = Vec::new();
    for (name, mode) in &want.overrides {
        own_set(&mut working, &mut rec, &["skillOverrides", name], &json!(mode).to_string(), &mut warnings);
    }
    for id in &want.plugins {
        own_set(&mut working, &mut rec, &["enabledPlugins", id], "false", &mut warnings);
    }
    for glob in &want.excludes {
        own_entry(&mut working, &mut rec, &["claudeMdExcludes"], glob, &mut warnings);
    }
    for deny in &want.denies {
        own_entry(&mut working, &mut rec, &["permissions", "deny"], deny, &mut warnings);
    }
    for (name, value) in &want.env {
        own_set(&mut working, &mut rec, &["env", name], &json!(value).to_string(), &mut warnings);
    }
    for server in &want.deny_servers {
        own_item(&mut working, &mut rec, &["deniedMcpServers"], json!({"serverName": server}), &mut warnings);
    }
    let action = if rec.owned.is_empty() && created_file {
        if existed {
            Action::Delete
        } else {
            Action::Keep
        }
    } else {
        if !existed {
            working.push('\n');
        }
        rec.sha256 = sha256_hex(&working);
        if Some(working.as_str()) == text {
            Action::Keep
        } else {
            Action::Write(working)
        }
    };
    Ok(SettingsOut { action, rec, conflicts, warnings })
}

struct BlockOut {
    action: Action,
    rec: Option<BlockRec>,
}

fn compute_block(
    text: Option<&str>,
    prior: Option<&FolderRec>,
    name: &str,
    want: &Desired,
    cwd: &Path,
) -> Result<BlockOut, String> {
    let existed = text.is_some();
    let mut working = text.unwrap_or("").to_string();
    let mut created_file = !existed;
    if let Some((old, old_name)) = prior.and_then(|p| p.block.as_ref().map(|b| (b, &p.bundle))) {
        undo_block(&mut working, old_name, old)
            .map_err(|()| format!("{BLOCK_FILE}: the block of bundle {old_name} was changed by hand; undo or remove it first"))?;
        created_file = !existed || (old.created_file && working.is_empty());
    }
    let mut rec = None;
    if !want.layers.is_empty() {
        let prefix = match working.as_str() {
            "" => "",
            t if t.ends_with("\n\n") => "",
            t if t.ends_with('\n') => "\n",
            _ => "\n\n",
        };
        let block = block_text(name, &want.layers);
        working.push_str(prefix);
        working.push_str(&block);
        rec = Some(BlockRec {
            file: fsx::display(&block_path(cwd)),
            created_file,
            prefix: prefix.to_string(),
            layers: want.layers.iter().map(|(n, _)| n.clone()).collect(),
            lines: block.lines().count(),
            sha256: sha256_hex(&block),
            file_sha256: sha256_hex(&working),
        });
    }
    let action = if working.is_empty() && created_file {
        if existed {
            Action::Delete
        } else {
            Action::Keep
        }
    } else if Some(working.as_str()) == text {
        Action::Keep
    } else {
        Action::Write(working)
    };
    Ok(BlockOut { action, rec })
}

pub(super) fn git_dir(cwd: &Path) -> Option<PathBuf> {
    let dir = cwd.join(".git");
    dir.is_dir().then_some(dir)
}

pub(super) fn exclude_file(cwd: &Path) -> PathBuf {
    cwd.join(".git").join("info").join("exclude")
}

fn exclude_lines(want_block: bool) -> Vec<&'static str> {
    let mut lines = vec![SETTINGS_REL];
    if want_block {
        lines.push(BLOCK_FILE);
    }
    lines
}

/// Whether any plugin control still owns keys in the folder (the git-ignore lines stay then).
pub(super) fn controls_in(data_dir: &Path, folder: &str) -> bool {
    ledger::load(data_dir).controls.get(folder).is_some_and(|m| !m.is_empty())
}

pub(super) fn remove_excludes(excludes: &[ExcludeRec], changed: &mut Vec<String>) -> Result<(), String> {
    for e in excludes {
        let file = PathBuf::from(&e.file);
        if let Ok(Some(text)) = bundle_io::read_exact(&file) {
            if let Some(after) = remove_line(&text, &e.line) {
                let created = excludes.iter().any(|x| x.file == e.file && x.created_file);
                if after.is_empty() && created {
                    let _ = fs::remove_file(&file);
                } else {
                    bundle_io::write_atomic(&file, after.as_bytes()).map_err(|x| format!("{}: {x}", file.display()))?;
                }
                if !changed.contains(&e.file) {
                    changed.push(e.file.clone());
                }
            }
        }
    }
    Ok(())
}

fn remove_line(text: &str, line: &str) -> Option<String> {
    let mut out = String::new();
    let mut removed = false;
    for piece in text.split_inclusive('\n') {
        if !removed && piece.trim_end_matches(['\n', '\r']) == line {
            removed = true;
            continue;
        }
        out.push_str(piece);
    }
    removed.then_some(out)
}

fn shown(raw: Option<&str>) -> String {
    raw.map_or_else(|| "(absent)".to_string(), str::to_string)
}

pub(super) fn settings_diff(rec: &SettingsRec) -> Diff {
    let mut before = String::new();
    let mut after = String::new();
    for o in &rec.owned {
        let (b, a) = if o.kind == "entry" {
            ("(absent)".to_string(), o.written.clone())
        } else {
            (shown(o.before.as_deref()), o.written.clone())
        };
        before.push_str(&format!("{}: {b}\n", o.label()));
        after.push_str(&format!("{}: {a}\n", o.label()));
    }
    Diff { before, after }
}

pub(super) fn top_keys(rec: &SettingsRec) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for o in &rec.owned {
        let key = if o.path[0] == "permissions" { "permissions.deny".to_string() } else { o.path[0].clone() };
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn estimate(w: &World, cwd: &Path, bundle: &Bundle, want: &Desired) -> Option<TokenEffect> {
    let config = super::load_config(&w.roots.context_config_path());
    let options = LoadsOptions { no_lazy: true, ..Default::default() };
    let report = what_loads_with(w.roots, &config, None, cwd, &options).ok()?;
    let off: BTreeSet<&str> = want
        .overrides
        .iter()
        .filter(|(_, mode)| *mode == "off")
        .map(|(n, _)| n.as_str())
        .collect();
    let saved: u64 = report
        .items
        .iter()
        .filter(|i| i.loaded)
        .filter(|i| {
            (i.kind == "skill" && off.contains(i.name.as_str()))
                || (i.kind == "agent" && bundle.agents_off.contains(&i.name))
                || (i.origin.kind == "plugin" && want.plugins.contains(&i.origin.name))
        })
        .map(|i| i.tokens)
        .sum();
    Some(TokenEffect {
        before: report.total_tokens,
        after: report.total_tokens.saturating_sub(saved),
        basis: "estimate",
    })
}

pub(super) fn checked_cwd(cwd: &Path) -> Result<PathBuf, BundleError> {
    if !fsx::is_dir(cwd) {
        return Err(BundleError::new("usage", "cwd is not a folder"));
    }
    Ok(fsx::canonical(cwd))
}

pub(super) fn read(path: &Path) -> Result<Option<String>, BundleError> {
    bundle_io::read_exact(path).map_err(failed)
}

pub fn apply(w: &World, name: &str, cwd: &Path, dry_run: bool) -> Result<Value, BundleError> {
    let cwd = checked_cwd(cwd)?;
    let loaded = bundle_store::load(w.roots, name)?;
    let key = fsx::display(&cwd);
    let prior = ledger::load(w.data_dir).folders.get(&key).cloned();
    let want = desired(w, &loaded.bundle, &cwd);
    let dir_missing = !cwd.join(".claude").is_dir();
    let settings_file = settings_path(&cwd);
    let md_file = block_path(&cwd);

    let s_now = read(&settings_file)?;
    let m_now = read(&md_file)?;
    let s_plan = compute_settings(s_now.as_deref(), prior.as_ref().map(|p| &p.settings), &want, &cwd, dir_missing).map_err(failed)?;
    let m_plan = compute_block(m_now.as_deref(), prior.as_ref(), name, &want, &cwd).map_err(failed)?;

    let mut steps = Vec::new();
    let mut warnings = want.warnings.clone();
    warnings.extend(s_plan.warnings.iter().cloned());
    if let Some(p) = &prior {
        steps.push(Step {
            op: "note",
            path: None,
            detail: format!("replaces bundle {} applied here on {}: its keys are put back first", p.bundle, p.applied_at),
            keys: None,
            diff: None,
        });
    }
    if !s_plan.rec.owned.is_empty() {
        let mut step = Step::merge(
            &fsx::display(&settings_file),
            format!("{} key(s) owned by bundle {name}", s_plan.rec.owned.len()),
            &top_keys(&s_plan.rec).iter().map(String::as_str).collect::<Vec<_>>(),
            settings_diff(&s_plan.rec),
        );
        if s_now.is_none() {
            step.op = "create";
        }
        steps.push(step);
    }
    if let Some(block) = &m_plan.rec {
        let text = block_text(name, &want.layers);
        let diff = Diff { before: String::new(), after: text };
        let mut step = Step::merge(
            &fsx::display(&md_file),
            format!("one managed block with {} layer(s)", block.layers.len()),
            &[],
            diff,
        );
        step.keys = None;
        if m_now.is_none() {
            step.op = "create";
        }
        steps.push(step);
    }
    let repo = git_dir(&cwd);
    if repo.is_some() {
        let existing = fs::read_to_string(exclude_file(&cwd)).unwrap_or_default();
        for line in exclude_lines(m_plan.rec.is_some()) {
            if !existing.lines().any(|l| l == line) {
                steps.push(Step {
                    op: "update",
                    path: Some(fsx::display(&exclude_file(&cwd))),
                    detail: format!("git-ignore {line}"),
                    keys: None,
                    diff: None,
                });
            }
        }
    } else {
        warnings.push(format!(
            "{} is not a git repository root: the files are written here and not git-ignored, so keep them out of commits yourself",
            cwd.display()
        ));
    }
    steps.push(Step {
        op: "note",
        path: Some(fsx::display(&ledger::path(w.data_dir))),
        detail: "record every key Toolport owns with the value it replaces, so undo restores exactly that".into(),
        keys: None,
        diff: None,
    });
    steps.push(Step {
        op: "note",
        path: None,
        detail: HYGIENE.into(),
        keys: None,
        diff: None,
    });
    let plan = PlanV1 {
        summary: format!("Apply bundle {name} in {}", cwd.display()),
        steps,
        effects: Effects { tokens: estimate(w, &cwd, &loaded.bundle, &want) },
        warnings,
        undo: undo_command(&cwd),
    };
    let plan_value = serde_json::to_value(&plan).map_err(|e| failed(e.to_string()))?;
    if dry_run {
        return Ok(json!({
            "dryRun": true, "bundle": name, "cwd": key, "plan": plan_value, "result": null,
            "conflicts": s_plan.conflicts,
        }));
    }

    let mut changed = Vec::new();
    let s_out = bundle_io::update(&settings_file, TRIES, |text| {
        let out = compute_settings(text, prior.as_ref().map(|p| &p.settings), &want, &cwd, dir_missing)?;
        Ok((out.action.clone(), out))
    })
    .map_err(failed)?;
    if !matches!(s_out.action, Action::Keep) {
        changed.push(fsx::display(&settings_file));
    }
    let m_out = bundle_io::update(&md_file, TRIES, |text| {
        let out = compute_block(text, prior.as_ref(), name, &want, &cwd)?;
        Ok((out.action.clone(), out))
    })
    .map_err(failed)?;
    if !matches!(m_out.action, Action::Keep) {
        changed.push(fsx::display(&md_file));
    }
    let mut excludes = Vec::new();
    if repo.is_some() {
        let mut report = super::Report::default();
        let existed = exclude_file(&cwd).exists();
        for line in exclude_lines(m_out.rec.is_some()) {
            if layers::ensure_exclude_line(&cwd, line, &mut report, false).map_err(failed)? {
                excludes.push(ExcludeRec {
                    file: fsx::display(&exclude_file(&cwd)),
                    line: line.to_string(),
                    created_file: !existed && excludes.is_empty(),
                });
            }
        }
        if !excludes.is_empty() {
            changed.push(fsx::display(&exclude_file(&cwd)));
        }
    }
    if let Some(p) = &prior {
        excludes.extend(p.excludes.iter().filter(|e| !excludes.contains(e)).cloned().collect::<Vec<_>>());
    }
    let record = FolderRec {
        bundle: name.to_string(),
        applied_at: fsx::now_zulu(),
        settings: s_out.rec.clone(),
        block: m_out.rec.clone(),
        excludes,
    };
    ledger::update(w.data_dir, |l| {
        l.folders.insert(key.clone(), record.clone());
    })
    .map_err(failed)?;
    let mut result = serde_json::to_value(ResultV1 {
        applied: true,
        changed,
        undo: undo_command(&cwd),
        backups: Vec::new(),
    })
    .map_err(|e| failed(e.to_string()))?;
    result["ledger"] = json!(fsx::display(&ledger::path(w.data_dir)));
    Ok(json!({
        "dryRun": false, "bundle": name, "cwd": key, "plan": plan_value, "result": result,
        "conflicts": s_out.conflicts,
    }))
}

pub fn undo(w: &World, cwd: &Path, dry_run: bool) -> Result<Value, BundleError> {
    let cwd = checked_cwd(cwd)?;
    let key = fsx::display(&cwd);
    let Some(prior) = ledger::load(w.data_dir).folders.get(&key).cloned() else {
        return Err(BundleError::new("not_applied", format!("no bundle is applied in {}", cwd.display())));
    };
    let settings_file = settings_path(&cwd);
    let md_file = block_path(&cwd);
    let mut conflicts = Vec::new();
    let mut steps = Vec::new();

    let compute_s = |text: Option<&str>| undo_action(text, &prior.settings);
    let compute_m = |text: Option<&str>| -> Result<(Action, bool), String> {
        let (Some(text), Some(rec)) = (text, prior.block.as_ref()) else {
            return Ok((Action::Keep, false));
        };
        let mut working = text.to_string();
        let conflict = undo_block(&mut working, &prior.bundle, rec).is_err();
        let action = if conflict {
            Action::Keep
        } else if working.is_empty() && rec.created_file {
            Action::Delete
        } else if working == text {
            Action::Keep
        } else {
            Action::Write(working)
        };
        Ok((action, conflict))
    };

    let s_now = read(&settings_file)?;
    let m_now = read(&md_file)?;
    let (s_action, s_conflicts) = compute_s(s_now.as_deref()).map_err(failed)?;
    let (m_action, m_conflict) = compute_m(m_now.as_deref()).map_err(failed)?;
    if !matches!(s_action, Action::Keep) {
        let op = if matches!(s_action, Action::Delete) { "delete" } else { "merge" };
        steps.push(Step {
            op,
            path: Some(fsx::display(&settings_file)),
            detail: format!("put back {} key(s) of bundle {}", prior.settings.owned.len(), prior.bundle),
            keys: Some(top_keys(&prior.settings)),
            diff: None,
        });
    }
    if !matches!(m_action, Action::Keep) {
        steps.push(Step {
            op: if matches!(m_action, Action::Delete) { "delete" } else { "update" },
            path: Some(fsx::display(&md_file)),
            detail: "remove the managed block".into(),
            keys: None,
            diff: None,
        });
    }
    for e in &prior.excludes {
        steps.push(Step {
            op: "update",
            path: Some(e.file.clone()),
            detail: format!("stop git-ignoring {}", e.line),
            keys: None,
            diff: None,
        });
    }
    steps.push(Step {
        op: "note",
        path: Some(fsx::display(&ledger::path(w.data_dir))),
        detail: "drop the record of this folder".into(),
        keys: None,
        diff: None,
    });
    conflicts.extend(s_conflicts.iter().cloned());
    if m_conflict {
        conflicts.push(BLOCK_FILE.to_string());
    }
    let mut warnings = Vec::new();
    if !conflicts.is_empty() {
        warnings.push(format!(
            "changed since the apply and left as they are: {}",
            conflicts.join(", ")
        ));
    }
    let reapply = format!("toolportctl context bundle apply {} --cwd {}", prior.bundle, cwd.display());
    let plan = PlanV1 {
        summary: format!("Undo bundle {} in {}", prior.bundle, cwd.display()),
        steps,
        effects: Effects::default(),
        warnings,
        undo: reapply.clone(),
    };
    let plan_value = serde_json::to_value(&plan).map_err(|e| failed(e.to_string()))?;
    if dry_run {
        return Ok(json!({
            "dryRun": true, "bundle": prior.bundle, "cwd": key, "plan": plan_value, "result": null,
            "conflicts": conflicts,
        }));
    }

    let mut changed = Vec::new();
    let (s_done, s_conflicts) = bundle_io::update(&settings_file, TRIES, |text| {
        let (action, c) = compute_s(text)?;
        Ok((action.clone(), (action, c)))
    })
    .map_err(failed)?;
    if !matches!(s_done, Action::Keep) {
        changed.push(fsx::display(&settings_file));
        if matches!(s_done, Action::Delete) && prior.settings.created_dir {
            let dir = cwd.join(".claude");
            if fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_none()) {
                let _ = fs::remove_dir(&dir);
            }
        }
    }
    let (m_done, m_conflict) = bundle_io::update(&md_file, TRIES, |text| {
        let (action, c) = compute_m(text)?;
        Ok((action.clone(), (action, c)))
    })
    .map_err(failed)?;
    if !matches!(m_done, Action::Keep) {
        changed.push(fsx::display(&md_file));
    }
    let settings_gone = matches!(s_done, Action::Delete);
    let mut conflicts = s_conflicts;
    if m_conflict {
        conflicts.push(BLOCK_FILE.to_string());
    }
    ledger::update(w.data_dir, |l| {
        l.folders.remove(&key);
    })
    .map_err(failed)?;
    super::bundle_controls::leave(w.data_dir, &key, settings_gone, &prior.settings, &prior.excludes, &mut changed).map_err(failed)?;
    let mut result = serde_json::to_value(ResultV1 { applied: true, changed, undo: reapply, backups: Vec::new() })
        .map_err(|e| failed(e.to_string()))?;
    result["ledger"] = json!(fsx::display(&ledger::path(w.data_dir)));
    Ok(json!({
        "dryRun": false, "bundle": prior.bundle, "cwd": key, "plan": plan_value, "result": result,
        "conflicts": conflicts,
    }))
}

/// The keys of a folder's record whose value is no longer what Toolport wrote.
/// The owned keys of a settings record whose value is no longer what Toolport wrote, and those of
/// them that now hold something else (as opposed to being gone).
pub(super) fn settings_drift(rec: &SettingsRec, cwd: &Path) -> (Vec<String>, Vec<String>) {
    let mut changed = Vec::new();
    let mut conflicts = Vec::new();
    let text = fs::read_to_string(settings_path(cwd)).unwrap_or_default();
    for owned in &rec.owned {
        let path: Vec<&str> = owned.path.iter().map(String::as_str).collect();
        if owned.kind == "entry" {
            let item: Value = serde_json::from_str(&owned.written).unwrap_or(Value::Null);
            let present = matches!(json::value(&text, &path), Some(Value::Array(a)) if a.contains(&item));
            if !present {
                changed.push(owned.label());
            }
            continue;
        }
        match json::get(&text, &path) {
            Some(Found::Scalar(raw)) if same(&raw, &owned.written) => {}
            None => changed.push(owned.label()),
            Some(_) => {
                changed.push(owned.label());
                conflicts.push(owned.label());
            }
        }
    }
    (changed, conflicts)
}

pub fn drift_of(rec: &FolderRec, cwd: &Path) -> (Vec<String>, Vec<String>) {
    let (mut changed, mut conflicts) = settings_drift(&rec.settings, cwd);
    if let Some(block) = &rec.block {
        let md = fs::read_to_string(block_path(cwd)).unwrap_or_default();
        if !block_intact(&md, &rec.bundle, block) {
            changed.push(BLOCK_FILE.to_string());
            if block_span(&md, &rec.bundle).is_some() {
                conflicts.push(BLOCK_FILE.to_string());
            }
        }
    }
    (changed, conflicts)
}

pub(super) fn names_under(rec: &FolderRec, top: &str) -> Vec<String> {
    let mut out: Vec<String> = rec
        .settings
        .owned
        .iter()
        .filter(|o| o.path.first().map(String::as_str) == Some(top))
        .map(|o| {
            if o.kind == "entry" {
                match serde_json::from_str::<Value>(&o.written) {
                    Ok(Value::String(s)) => s,
                    Ok(item) => item["serverName"].as_str().unwrap_or_default().to_string(),
                    Err(_) => String::new(),
                }
            } else {
                o.path[1].clone()
            }
        })
        .collect();
    out.sort();
    out
}

pub fn status(w: &World, cwd: &Path) -> Result<Value, BundleError> {
    let cwd = checked_cwd(cwd)?;
    let key = fsx::display(&cwd);
    let Some(rec) = ledger::load(w.data_dir).folders.get(&key).cloned() else {
        return Ok(json!({ "folder": key, "applied": null, "conflicts": [] }));
    };
    let (changed, conflicts) = drift_of(&rec, &cwd);
    Ok(json!({
        "folder": key,
        "applied": {
            "bundle": rec.bundle,
            "appliedAt": rec.applied_at,
            "drift": !changed.is_empty(),
            "changedKeys": changed,
            "ownedKeys": {
                "skillOverrides": names_under(&rec, "skillOverrides"),
                "enabledPlugins": names_under(&rec, "enabledPlugins"),
                "claudeMdExcludes": names_under(&rec, "claudeMdExcludes"),
                "permissionsDeny": names_under(&rec, "permissions"),
                "env": names_under(&rec, "env"),
                "deniedMcpServers": names_under(&rec, "deniedMcpServers"),
            },
        },
        "conflicts": conflicts,
    }))
}

/// Folders a bundle is applied in, for `bundle ls` and the guard of `bundle rm`.
pub fn applied_to(data_dir: &Path, name: &str) -> Vec<Value> {
    ledger::load(data_dir)
        .folders
        .iter()
        .filter(|(_, rec)| rec.bundle == name)
        .map(|(folder, rec)| {
            let drift = !drift_of(rec, Path::new(folder)).0.is_empty();
            json!({ "folder": folder, "appliedAt": rec.applied_at, "drift": drift })
        })
        .collect()
}
