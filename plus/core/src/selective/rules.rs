//! Rules a selection turns on are instruction files, not plugin components, so they go into the
//! Library as `rules/<name>/SKILL.md` (the layout `skills install` writes) and a Library folder
//! is only replaced or removed while it is still what Toolport wrote.

use super::discover::hash_path;
use super::slice::copy_tree;
use super::store::{Item, Kind, Linked, State};
use crate::context::fsx;
use crate::fsx::{atomic_write, atomic_write_bytes, Mode};
use crate::skills::origin::{forget_origin, record_origin, ORIGINS_FILE, TAP_PREFIX};
use crate::skills::parser::{split_frontmatter, valid_name};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const BUCKET: &str = "rules";
const ENTRY: &str = "SKILL.md";
const MAX_RULE: usize = 1024 * 1024;
const DESCRIPTION_CHARS: usize = 200;
pub const FORCE: &str = "force";
pub const SKIP: &str = "skip";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Install,
    Update,
    Remove,
    /// The Library folder is gone already; only the record goes.
    Forget,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Conflict {
    pub code: &'static str,
    pub reason: String,
    pub choices: Vec<&'static str>,
}

impl Conflict {
    fn new(code: &'static str, reason: String, choices: &[&'static str]) -> Self {
        Self {
            code,
            reason,
            choices: choices.to_vec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub action: Action,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict: Option<Conflict>,
    /// Hash of the Library folder as it is now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    #[serde(skip)]
    src: String,
    #[serde(skip)]
    source_path: Option<String>,
}

/// The folder name a rule gets in the Library: lowercase letters, digits and single hyphens.
pub fn library_name(id: &str) -> Result<String, String> {
    let mut name = String::new();
    for c in id.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_lowercase());
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    let name = name.trim_end_matches('-').to_string();
    valid_name(&name).map_err(|why| format!("the rule {id} cannot be a Library name: {why}"))?;
    Ok(name)
}

fn target(library: &Path, name: &str) -> PathBuf {
    library.join(BUCKET).join(name)
}

fn rel_path(name: &str) -> String {
    format!("{BUCKET}/{name}")
}

fn present(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn is_link(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
}

fn row(id: &str, name: &str, action: Action, src: &str, path: Option<&str>) -> Row {
    Row {
        id: id.to_string(),
        name: name.to_string(),
        action,
        path: rel_path(name),
        conflict: None,
        current: None,
        src: src.to_string(),
        source_path: path.map(String::from),
    }
}

fn blocked(mut row: Row, conflict: Conflict) -> Row {
    row.conflict = Some(conflict);
    row
}

fn wanted_row(
    library: Option<&Path>,
    item: &Item,
    linked: Option<&Linked>,
    taken: &mut BTreeSet<String>,
) -> Option<Row> {
    let path = item.path.as_deref();
    let name = match library_name(&item.id) {
        Ok(name) => name,
        Err(why) => {
            let base = row(&item.id, &item.id, Action::Install, &item.hash, path);
            let mut bad = blocked(base, Conflict::new("name", why, &[SKIP]));
            bad.path = String::new();
            return Some(bad);
        }
    };
    let action = if linked.is_some() {
        Action::Update
    } else {
        Action::Install
    };
    let base = row(&item.id, &name, action, &item.hash, path);
    if !taken.insert(name.clone()) {
        let reason = format!(
            "another rule of this source is already named {}",
            rel_path(&name)
        );
        return Some(blocked(base, Conflict::new("name", reason, &[SKIP])));
    }
    let Some(library) = library else {
        if linked.is_some_and(|l| l.src == item.hash) {
            return None;
        }
        return Some(blocked(
            base,
            Conflict::new(
                "no-library",
                "no Library repository was found; run `toolportctl library connect` or `skills init`"
                    .into(),
                &[SKIP],
            ),
        ));
    };
    let dir = target(library, &name);
    if is_link(&dir) {
        let reason = format!(
            "{} is a symbolic link; Toolport does not write through it",
            base.path
        );
        return Some(blocked(base, Conflict::new("symlink", reason, &[SKIP])));
    }
    let current = present(&dir).then(|| hash_path(&dir).unwrap_or_default());
    let mut base = base;
    base.current.clone_from(&current);
    match (linked, current) {
        (None, None) => Some(Row {
            action: Action::Install,
            ..base
        }),
        (None, Some(_)) => {
            let reason = format!(
                "{} already exists in the Library and Toolport did not write it",
                base.path
            );
            Some(blocked(
                Row {
                    action: Action::Install,
                    ..base
                },
                Conflict::new("foreign", reason, &[FORCE, SKIP]),
            ))
        }
        (Some(_), None) => Some(Row {
            action: Action::Install,
            ..base
        }),
        (Some(l), Some(now)) if now == l.hash => (l.src != item.hash).then_some(base),
        (Some(l), Some(_)) if l.src == item.hash => None,
        (Some(_), Some(_)) => {
            let reason = format!(
                "{} was edited in the Library after Toolport wrote it",
                base.path
            );
            Some(blocked(
                base,
                Conflict::new("edited", reason, &[FORCE, SKIP]),
            ))
        }
    }
}

fn unwanted_row(library: Option<&Path>, id: &str, linked: &Linked) -> Row {
    let base = row(id, &linked.name, Action::Remove, &linked.src, None);
    let Some(library) = library else {
        return blocked(
            base,
            Conflict::new(
                "no-library",
                "no Library repository was found, so the rule cannot be removed from it".into(),
                &[SKIP],
            ),
        );
    };
    let dir = target(library, &linked.name);
    if is_link(&dir) {
        let reason = format!(
            "{} is a symbolic link; Toolport does not remove through it",
            base.path
        );
        return blocked(base, Conflict::new("symlink", reason, &[SKIP]));
    }
    if !present(&dir) {
        return Row {
            action: Action::Forget,
            ..base
        };
    }
    let now = hash_path(&dir).unwrap_or_default();
    let mut base = base;
    base.current = Some(now.clone());
    if now == linked.hash {
        return base;
    }
    let reason = format!(
        "{} was edited in the Library after Toolport wrote it",
        base.path
    );
    blocked(base, Conflict::new("edited", reason, &[FORCE, SKIP]))
}

/// What a selection has to do in the Library: install the rules that are on, update the ones that
/// changed upstream, remove the ones this selection linked before and no longer wants.
pub fn plan(library: Option<&Path>, items: &[Item], linked: &BTreeMap<String, Linked>) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut taken = BTreeSet::new();
    for item in items
        .iter()
        .filter(|i| i.kind == Kind::Rule && i.state == State::On)
    {
        rows.extend(wanted_row(library, item, linked.get(&item.id), &mut taken));
    }
    for (id, entry) in linked {
        let kept = items
            .iter()
            .any(|i| i.kind == Kind::Rule && i.id == *id && i.state.wanted());
        if !kept {
            rows.push(unwanted_row(library, id, entry));
        }
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Go,
    Skip,
}

/// Pairs every row with what the caller chose for it. A conflict needs a choice; a choice for
/// anything else is refused, so a typo never passes silently.
pub fn decide<'a>(
    rows: &'a [Row],
    force: &[String],
    skip: &[String],
) -> Result<Vec<(&'a Row, Verdict)>, String> {
    let conflicts: Vec<&Row> = rows.iter().filter(|r| r.conflict.is_some()).collect();
    for id in force.iter().chain(skip) {
        if !conflicts.iter().any(|r| r.id == *id) {
            return Err(format!(
                "rule:{id} has no conflict to choose for; the plan lists: {}",
                list(&conflicts)
            ));
        }
    }
    if let Some(id) = force.iter().find(|id| skip.contains(id)) {
        return Err(format!("rule:{id} cannot be both forced and skipped"));
    }
    let mut out = Vec::new();
    let mut open = Vec::new();
    for row in rows {
        let Some(conflict) = &row.conflict else {
            out.push((row, Verdict::Go));
            continue;
        };
        if force.contains(&row.id) {
            if !conflict.choices.contains(&FORCE) {
                return Err(format!(
                    "rule:{} cannot be forced: {}",
                    row.id, conflict.reason
                ));
            }
            out.push((row, Verdict::Go));
        } else if skip.contains(&row.id) {
            out.push((row, Verdict::Skip));
        } else {
            open.push(row);
        }
    }
    if !open.is_empty() {
        let open: Vec<&Row> = open;
        return Err(format!(
            "{} rule(s) need a choice before apply: {}; pass --force <rule> to replace or remove \
             the Library copy, or --skip <rule> to leave it alone",
            open.len(),
            list(&open)
        ));
    }
    Ok(out)
}

fn list(rows: &[&Row]) -> String {
    rows.iter()
        .map(|r| format!("rule:{} ({})", r.id, r.path))
        .collect::<Vec<_>>()
        .join(", ")
}

fn description_of(frontmatter: Option<&str>, body: &str) -> String {
    let named = frontmatter
        .and_then(|yaml| serde_yaml::from_str::<serde_yaml::Value>(yaml).ok())
        .and_then(|doc| doc.get("description")?.as_str().map(str::to_string));
    let line = named.unwrap_or_else(|| {
        body.lines()
            .map(|l| l.trim().trim_start_matches('#').trim())
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string()
    });
    line.chars().take(DESCRIPTION_CHARS).collect()
}

/// The Library form of a flat rule file: the frontmatter the Library needs, then the rule text.
pub fn library_text(name: &str, source: &str) -> String {
    let (frontmatter, body) = match split_frontmatter(source) {
        Some((yaml, body)) => (Some(yaml), body.to_string()),
        None => (None, source.trim().to_string()),
    };
    let description = description_of(frontmatter, &body);
    let description = if description.trim().is_empty() {
        format!("Rule {name}")
    } else {
        description
    };
    let mut head = serde_yaml::Mapping::new();
    for (key, value) in [
        ("name", name),
        ("description", description.as_str()),
        ("activation", "always"),
    ] {
        head.insert(key.into(), value.into());
    }
    let yaml = serde_yaml::to_string(&head).unwrap_or_default();
    format!("---\n{yaml}---\n\n{body}\n")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Undone {
    pub name: String,
    pub had: bool,
    /// The folder as Toolport left it; `None` when it left no folder.
    pub wrote: Option<String>,
}

/// What an apply changed in the Library, enough to put every byte back.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Journal {
    pub library: PathBuf,
    pub rules: Vec<Undone>,
    /// Whether the origins file existed before; `None` when it was not touched.
    pub origins: Option<bool>,
    pub made_dir: bool,
}

pub struct Done {
    pub journal: Journal,
    pub linked: Vec<(String, Option<Linked>)>,
    pub changed: Vec<String>,
}

fn remove_path(path: &Path) -> Result<(), String> {
    let result = match path.symlink_metadata() {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(_) => return Ok(()),
    };
    result.map_err(|e| format!("{}: {e}", path.display()))
}

fn backup_origins(journal: &mut Journal, dir: &Path) -> Result<(), String> {
    if journal.origins.is_some() {
        return Ok(());
    }
    let file = journal.library.join(ORIGINS_FILE);
    journal.origins = Some(file.exists());
    if file.exists() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::copy(&file, dir.join(ORIGINS_FILE))
            .map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

fn back_up(journal: &mut Journal, dir: &Path, name: &str) -> Result<(), String> {
    let dest = target(&journal.library, name);
    let had = present(&dest);
    journal.rules.push(Undone {
        name: name.to_string(),
        had,
        wrote: None,
    });
    if had {
        copy_tree(&dest, &dir.join(BUCKET).join(name))?;
        remove_path(&dest)?;
    }
    Ok(())
}

fn install(
    journal: &mut Journal,
    dir: &Path,
    tap: &str,
    row: &Row,
    root: &Path,
) -> Result<Linked, String> {
    let relative = row
        .source_path
        .as_deref()
        .ok_or_else(|| format!("rule:{} has no file in the source", row.id))?;
    let file = root.join(relative);
    let text = fsx::read_text(&file, MAX_RULE + 1)
        .filter(|t| t.len() <= MAX_RULE)
        .ok_or_else(|| format!("rule:{} cannot be read from the source", row.id))?;
    if !journal.library.join(BUCKET).exists() {
        journal.made_dir = true;
        std::fs::create_dir_all(journal.library.join(BUCKET)).map_err(|e| e.to_string())?;
    }
    back_up(journal, dir, &row.name)?;
    backup_origins(journal, dir)?;
    let dest = target(&journal.library, &row.name);
    atomic_write(
        &dest.join(ENTRY),
        &library_text(&row.name, &text),
        Mode::Exact(0o644),
    )
    .map_err(|e| format!("{}: {e}", dest.display()))?;
    record_origin(&journal.library, &row.name, &format!("{TAP_PREFIX}{tap}"))?;
    let hash = hash_path(&dest)?;
    if let Some(last) = journal.rules.last_mut() {
        last.wrote = Some(hash.clone());
    }
    Ok(Linked {
        name: row.name.clone(),
        hash,
        src: row.src.clone(),
        extra: Map::new(),
    })
}

fn remove(journal: &mut Journal, dir: &Path, tap: &str, row: &Row) -> Result<(), String> {
    back_up(journal, dir, &row.name)?;
    backup_origins(journal, dir)?;
    forget_origin(&journal.library, &row.name, &format!("{TAP_PREFIX}{tap}"))
}

/// Writes what the plan says into the Library, keeping the journal in `dir`. When a step fails
/// the steps before it are put back and nothing is left half done.
pub fn apply(
    library: &Path,
    tap: &str,
    steps: &[(&Row, Verdict)],
    root: &Path,
    dir: &Path,
) -> Result<Done, String> {
    let mut done = Done {
        journal: Journal {
            library: library.to_path_buf(),
            ..Journal::default()
        },
        linked: Vec::new(),
        changed: Vec::new(),
    };
    for (row, verdict) in steps {
        let result = step(&mut done, dir, tap, row, *verdict, root);
        if let Err(why) = result {
            let _ = restore(&done.journal, dir);
            return Err(why);
        }
    }
    Ok(done)
}

fn step(
    done: &mut Done,
    dir: &Path,
    tap: &str,
    row: &Row,
    verdict: Verdict,
    root: &Path,
) -> Result<(), String> {
    let shown = row.path.clone();
    match (row.action, verdict) {
        (Action::Install | Action::Update, Verdict::Go) => {
            let linked = install(&mut done.journal, dir, tap, row, root)?;
            done.linked.push((row.id.clone(), Some(linked)));
            done.changed.push(format!("{shown} written"));
        }
        (Action::Remove, Verdict::Go) => {
            remove(&mut done.journal, dir, tap, row)?;
            done.linked.push((row.id.clone(), None));
            done.changed.push(format!("{shown} removed"));
        }
        (Action::Remove, Verdict::Skip)
            if row
                .conflict
                .as_ref()
                .is_some_and(|c| c.code != "no-library") =>
        {
            done.linked.push((row.id.clone(), None));
        }
        (Action::Forget, _) => done.linked.push((row.id.clone(), None)),
        _ => {}
    }
    Ok(())
}

/// The names whose Library folder is no longer what the last apply left, so undoing would
/// overwrite somebody's later edit.
pub fn edited_since(journal: &Journal) -> Vec<String> {
    journal
        .rules
        .iter()
        .filter(|u| {
            let dir = target(&journal.library, &u.name);
            match &u.wrote {
                Some(hash) => hash_path(&dir).ok().as_ref() != Some(hash),
                None => present(&dir),
            }
        })
        .map(|u| rel_path(&u.name))
        .collect()
}

pub fn restore(journal: &Journal, dir: &Path) -> Result<(), String> {
    for undone in journal.rules.iter().rev() {
        let dest = target(&journal.library, &undone.name);
        remove_path(&dest)?;
        if undone.had {
            copy_tree(&dir.join(BUCKET).join(&undone.name), &dest)?;
        }
    }
    let origins = journal.library.join(ORIGINS_FILE);
    match journal.origins {
        Some(true) => {
            let bytes = std::fs::read(dir.join(ORIGINS_FILE)).map_err(|e| e.to_string())?;
            atomic_write_bytes(&origins, &bytes, Mode::Private)
                .map_err(|e| format!("{}: {e}", origins.display()))?;
        }
        Some(false) => remove_path(&origins)?,
        None => {}
    }
    if journal.made_dir {
        let _ = std::fs::remove_dir(journal.library.join(BUCKET));
    }
    Ok(())
}
