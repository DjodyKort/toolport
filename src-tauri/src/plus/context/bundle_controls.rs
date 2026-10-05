//! The plugin controls (`plugins config`, `plugins mcp`) on the bundle ledger: the same surgical
//! read-merge-write of `.claude/settings.local.json`, the same re-read before writing and the same
//! git-exclude line, with a record per control (`ledger.controls`) so a bundle and a control never
//! undo each other's keys. A control owns `env.<NAME>` keys or `deniedMcpServers` entries only.

use super::bundle_apply::{
    checked_cwd, compute_settings, controls_in, exclude_file, folder_key, git_dir, read, remove_excludes,
    settings_diff, settings_drift, settings_path, top_keys, undo_action, Desired,
};
use super::bundle_io::{self, Action};
use super::bundle_ledger::{self as ledger, ControlRec, ExcludeRec, Owned};
use super::bundle_store::BundleError;
use super::layers;
use crate::plus::plan::Step;
use crate::plus::sources::fsx;
use std::path::Path;

const TRIES: usize = 5;

fn failed(message: impl Into<String>) -> BundleError {
    BundleError::new("failed", message)
}

pub struct Run {
    pub dry_run: bool,
    pub folder: String,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
    pub conflicts: Vec<String>,
    pub changed: Vec<String>,
    pub owned: Vec<Owned>,
    pub ledger: String,
}

pub fn record(data_dir: &Path, cwd: &Path, control: &str) -> Option<ControlRec> {
    ledger::load(data_dir)
        .controls
        .get(&folder_key(cwd))
        .and_then(|m| m.get(control))
        .cloned()
}

/// The owned keys of a control that no longer hold what Toolport wrote (labels such as `env.X`).
pub fn drifted(data_dir: &Path, cwd: &Path, control: &str) -> Vec<String> {
    record(data_dir, cwd, control).map_or_else(Vec::new, |rec| settings_drift(&rec.settings, cwd).0)
}

fn merge_step(file: &Path, rec: &ControlRec, existed: bool, control: &str) -> Step {
    let mut step = Step::merge(
        &fsx::display(file),
        format!("{} key(s) owned by {control}", rec.settings.owned.len()),
        &top_keys(&rec.settings).iter().map(String::as_str).collect::<Vec<_>>(),
        settings_diff(&rec.settings),
    );
    if !existed {
        step.op = "create";
    }
    step
}

/// Make `want` the whole of what the control owns in the folder: what it wrote before is put back
/// first (keys changed since are conflicts and are left alone), then `want` is written.
pub fn apply(data_dir: &Path, cwd: &Path, control: &str, want: &Desired, dry_run: bool) -> Result<Run, BundleError> {
    let cwd = checked_cwd(cwd)?;
    let folder = folder_key(&cwd);
    if want.env.is_empty() && want.deny_servers.is_empty() {
        return undo(data_dir, &cwd, control, dry_run);
    }
    let prior = record(data_dir, &cwd, control);
    let dir_missing = !cwd.join(".claude").is_dir();
    let file = settings_path(&cwd);
    let now = read(&file)?;
    let planned = compute_settings(now.as_deref(), prior.as_ref().map(|p| &p.settings), want, &cwd, dir_missing).map_err(failed)?;
    let mut steps = Vec::new();
    let preview = ControlRec { settings: planned.rec.clone(), ..ControlRec::default() };
    if !planned.rec.owned.is_empty() {
        steps.push(merge_step(&file, &preview, now.is_some(), control));
    }
    let repo = git_dir(&cwd);
    if repo.is_some() {
        let existing = std::fs::read_to_string(exclude_file(&cwd)).unwrap_or_default();
        if !existing.lines().any(|l| l == super::bundle_apply::SETTINGS_REL) {
            steps.push(Step {
                op: "update",
                path: Some(fsx::display(&exclude_file(&cwd))),
                detail: format!("git-ignore {}", super::bundle_apply::SETTINGS_REL),
                keys: None,
                diff: None,
            });
        }
    }
    let mut warnings = planned.warnings.clone();
    if repo.is_none() {
        warnings.push(format!(
            "{} is not a git repository root: the file is written here and not git-ignored, so keep it out of commits yourself",
            cwd.display()
        ));
    }
    steps.push(Step {
        op: "note",
        path: Some(fsx::display(&ledger::path(data_dir))),
        detail: "record every key Toolport owns with the value it replaces, so undo restores exactly that".into(),
        keys: None,
        diff: None,
    });
    let ledger_file = fsx::display(&ledger::path(data_dir));
    if dry_run {
        return Ok(Run {
            dry_run,
            folder,
            steps,
            warnings,
            conflicts: planned.conflicts,
            changed: Vec::new(),
            owned: planned.rec.owned,
            ledger: ledger_file,
        });
    }

    let done = bundle_io::update(&file, TRIES, |text| {
        let out = compute_settings(text, prior.as_ref().map(|p| &p.settings), want, &cwd, dir_missing)?;
        Ok((out.action.clone(), out))
    })
    .map_err(failed)?;
    let mut changed = Vec::new();
    if !matches!(done.action, Action::Keep) {
        changed.push(fsx::display(&file));
    }
    let mut excludes: Vec<ExcludeRec> = Vec::new();
    if repo.is_some() {
        let mut report = super::Report::default();
        let existed = exclude_file(&cwd).exists();
        if layers::ensure_exclude_line(&cwd, super::bundle_apply::SETTINGS_REL, &mut report, false).map_err(failed)? {
            excludes.push(ExcludeRec {
                file: fsx::display(&exclude_file(&cwd)),
                line: super::bundle_apply::SETTINGS_REL.to_string(),
                created_file: !existed,
            });
            changed.push(fsx::display(&exclude_file(&cwd)));
        }
    }
    if let Some(p) = &prior {
        excludes.extend(p.excludes.iter().filter(|e| !excludes.contains(e)).cloned().collect::<Vec<_>>());
    }
    let rec = ControlRec { applied_at: fsx::now_zulu(), settings: done.rec.clone(), excludes };
    ledger::update(data_dir, |l| {
        l.controls.entry(folder.clone()).or_default().insert(control.to_string(), rec.clone());
    })
    .map_err(failed)?;
    Ok(Run {
        dry_run,
        folder,
        steps,
        warnings,
        conflicts: done.conflicts,
        changed,
        owned: done.rec.owned,
        ledger: ledger_file,
    })
}

/// Put back everything the control owns in the folder and drop its record. Without a record the
/// run owns nothing and changes nothing.
pub fn undo(data_dir: &Path, cwd: &Path, control: &str, dry_run: bool) -> Result<Run, BundleError> {
    let cwd = checked_cwd(cwd)?;
    let folder = folder_key(&cwd);
    let ledger_file = fsx::display(&ledger::path(data_dir));
    let Some(prior) = record(data_dir, &cwd, control) else {
        return Ok(Run {
            dry_run,
            folder,
            steps: Vec::new(),
            warnings: Vec::new(),
            conflicts: Vec::new(),
            changed: Vec::new(),
            owned: Vec::new(),
            ledger: ledger_file,
        });
    };
    let file = settings_path(&cwd);
    let now = read(&file)?;
    let (action, conflicts) = undo_action(now.as_deref(), &prior.settings).map_err(failed)?;
    let mut steps = Vec::new();
    if !matches!(action, Action::Keep) {
        steps.push(Step {
            op: if matches!(action, Action::Delete) { "delete" } else { "merge" },
            path: Some(fsx::display(&file)),
            detail: format!("put back {} key(s) owned by {control}", prior.settings.owned.len()),
            keys: Some(top_keys(&prior.settings)),
            diff: None,
        });
    }
    steps.push(Step {
        op: "note",
        path: Some(ledger_file.clone()),
        detail: format!("drop the record of {control} in this folder"),
        keys: None,
        diff: None,
    });
    let mut warnings = Vec::new();
    if !conflicts.is_empty() {
        warnings.push(format!("changed since the apply and left as they are: {}", conflicts.join(", ")));
    }
    if dry_run {
        return Ok(Run { dry_run, folder, steps, warnings, conflicts, changed: Vec::new(), owned: Vec::new(), ledger: ledger_file });
    }
    let (done, conflicts) = bundle_io::update(&file, TRIES, |text| {
        let (action, c) = undo_action(text, &prior.settings)?;
        Ok((action.clone(), (action, c)))
    })
    .map_err(failed)?;
    let mut changed = Vec::new();
    if !matches!(done, Action::Keep) {
        changed.push(fsx::display(&file));
        if matches!(done, Action::Delete) && prior.settings.created_dir {
            let dir = cwd.join(".claude");
            if std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_none()) {
                let _ = std::fs::remove_dir(&dir);
            }
        }
    }
    ledger::update(data_dir, |l| {
        if let Some(m) = l.controls.get_mut(&folder) {
            m.remove(control);
            if m.is_empty() {
                l.controls.remove(&folder);
            }
        }
    })
    .map_err(failed)?;
    leave(data_dir, &folder, matches!(done, Action::Delete), &prior.settings, &prior.excludes, &mut changed).map_err(failed)?;
    Ok(Run { dry_run, folder, steps, warnings, conflicts, changed, owned: Vec::new(), ledger: ledger_file })
}

fn owned_elsewhere(data_dir: &Path, folder: &str) -> bool {
    controls_in(data_dir, folder) || ledger::load(data_dir).folders.contains_key(folder)
}

/// A record that leaves a folder while another still owns keys in it passes on what only the last
/// owner may undo: that Toolport created the settings file, `.claude` or a container in it, and
/// the git-ignore lines.
pub(super) fn hand_over(data_dir: &Path, folder: &str, passed: &ledger::SettingsRec, excludes: &[ExcludeRec]) -> Result<(), String> {
    ledger::update(data_dir, |l| {
        let merge = |settings: &mut ledger::SettingsRec, list: &mut Vec<ExcludeRec>| {
            settings.created_file |= passed.created_file;
            settings.created_dir |= passed.created_dir;
            for c in &passed.created_containers {
                if !settings.created_containers.contains(c) {
                    settings.created_containers.push(c.clone());
                }
            }
            list.extend(excludes.iter().filter(|e| !list.contains(e)).cloned().collect::<Vec<_>>());
        };
        if let Some(rec) = l.folders.get_mut(folder) {
            merge(&mut rec.settings, &mut rec.excludes);
        } else if let Some(rec) = l.controls.get_mut(folder).and_then(|m| m.values_mut().next()) {
            merge(&mut rec.settings, &mut rec.excludes);
        }
    })
}

/// What leaves with a record: the exclude lines go with the last owner, everything else is passed on.
pub(super) fn leave(
    data_dir: &Path,
    folder: &str,
    settings_deleted: bool,
    rec: &ledger::SettingsRec,
    excludes: &[ExcludeRec],
    changed: &mut Vec<String>,
) -> Result<(), String> {
    if owned_elsewhere(data_dir, folder) {
        let passed = if settings_deleted { ledger::SettingsRec::default() } else { rec.clone() };
        hand_over(data_dir, folder, &passed, excludes)
    } else {
        remove_excludes(excludes, changed)
    }
}
