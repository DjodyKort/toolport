//! Delivery of the folder layers: one marked block per layer in the `CLAUDE.local.md` of every
//! folder the layer names. Only the blocks are Toolport's; the rest of the file, including the
//! bundle block, is kept byte for byte. A ledger in the data directory records what each block
//! held, so a sync can tell a library change from a change by hand, and `undo` puts back exactly
//! what the last deploy replaced.

use super::bundle_apply::{folder_key, BLOCK_FILE};
use super::bundle_io;
use super::fsx;
use super::hash::sha256_hex;
use super::layer_defs::{self, Delivery, LayerDef, Scope};
use super::layers::ensure_exclude_line;
use super::roots::Roots;
use crate::op::OpError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use toolport_plus_api::Host;

const IMPORT_CAP: usize = 256 * 1024;
const FILE_CAP: usize = 16 * 1024 * 1024;
const BEFORE_CAP: usize = 64 * 1024;
const IMPORT_EXTENSIONS: [&str; 3] = ["md", "markdown", "txt"];

fn begin(name: &str) -> String {
    format!("<!-- toolport:layer:begin {name} -->")
}

fn end(name: &str) -> String {
    format!("<!-- toolport:layer:end {name} -->")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockRec {
    sha256: String,
    deployed_at: String,
    /// What the block held before the last deploy; `None` when there was no block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<String>,
    /// Blank lines this deploy put in front of a block that did not exist.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    prefix: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FolderRec {
    created_file: bool,
    layers: BTreeMap<String, BlockRec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Ledger {
    version: u32,
    folders: BTreeMap<String, FolderRec>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            version: 1,
            folders: BTreeMap::new(),
        }
    }
}

fn ledger_path(data_dir: &Path) -> PathBuf {
    data_dir.join("plus").join("layer-ledger.json")
}

fn load_ledger(data_dir: &Path) -> Ledger {
    bundle_io::read_exact(&ledger_path(data_dir))
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_ledger(data_dir: &Path, ledger: &Ledger) -> Result<(), String> {
    let text = serde_json::to_string_pretty(ledger).map_err(|e| e.to_string())? + "\n";
    crate::fsx::atomic_write(
        &ledger_path(data_dir),
        &text,
        crate::fsx::Mode::KeepOrDefault,
    )
    .map_err(|e| format!("{}: {e}", ledger_path(data_dir).display()))
}

pub(super) struct Env {
    pub home: PathBuf,
    pub data_dir: PathBuf,
    pub roots: Roots,
    pub clients_root: PathBuf,
}

pub(super) fn env(host: &dyn Host) -> Result<Env, OpError> {
    let home = host
        .home_dir()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data_dir = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    let roots = Roots::from_home(&home);
    let config = crate::sources::config::load(&roots.context_config_path());
    let clients_root = crate::sources::Places::from_env(&roots, &config).clients_root;
    Ok(Env {
        home,
        data_dir,
        roots,
        clients_root,
    })
}

fn span(text: &str, name: &str) -> Option<(usize, usize)> {
    let start = text.find(&begin(name))?;
    let close = end(name);
    let stop = start + text[start..].find(&close)? + close.len();
    let stop = if text[stop..].starts_with('\n') {
        stop + 1
    } else {
        stop
    };
    Some((start, stop))
}

fn home_relative(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// An import the layer may bring in: a text file below the home folder. A layer can come from a
/// tap, so what it reads is limited to files that are meant to be read as instructions.
fn import_file(env: &Env, layer: &LayerDef, raw: &str) -> Result<PathBuf, String> {
    let given = env.roots.expand_user(raw);
    let path = if given.is_absolute() {
        given
    } else {
        layer
            .path
            .parent()
            .map_or_else(|| given.clone(), |dir| dir.join(&given))
    };
    let real = fsx::canonical(&path);
    let extension = real
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !IMPORT_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!(
            "import {raw} is not a text file (.md, .markdown or .txt)"
        ));
    }
    if !real.starts_with(fsx::canonical(&env.home)) {
        return Err(format!("import {raw} is outside the home folder"));
    }
    if !fsx::is_file(&real) {
        return Err(format!("import {raw} does not exist"));
    }
    Ok(real)
}

fn block_text(env: &Env, layer: &LayerDef) -> Result<String, String> {
    let name = &layer.name;
    let mut out = format!(
        "{}\n<!-- Deployed by `toolportctl context sync` from the layer {name}; change the layer, not this block. -->\n\n{}\n",
        begin(name),
        layer.body.trim()
    );
    for raw in &layer.imports {
        let file = import_file(env, layer, raw)?;
        match layer.delivery {
            Delivery::Import => out.push_str(&format!("\n@{}\n", home_relative(&env.home, &file))),
            Delivery::Copy => {
                let text = fsx::read_text(&file, IMPORT_CAP)
                    .ok_or_else(|| format!("import {raw} cannot be read"))?;
                out.push_str(&format!(
                    "\n<!-- from {} -->\n{}\n",
                    home_relative(&env.home, &file),
                    text.trim()
                ));
            }
        }
    }
    out.push_str(&end(name));
    out.push('\n');
    Ok(out)
}

#[derive(Clone, Debug)]
struct Row {
    layer: String,
    folder: String,
    action: &'static str,
    reason: Option<String>,
    before: Option<String>,
    after: Option<String>,
}

impl Row {
    fn value(&self, file: &str, dry_run: bool) -> Value {
        let mut row = json!({
            "layer": self.layer, "folder": self.folder, "file": file, "action": self.action,
        });
        if let Some(reason) = &self.reason {
            row["reason"] = json!(reason);
        }
        if matches!(self.action, "update" | "replace" | "add" | "remove") {
            row["diff"] = json!({
                "before": self.before.clone().unwrap_or_default(),
                "after": self.after.clone().unwrap_or_default(),
            });
            row["applied"] = json!(!dry_run);
        }
        row
    }
}

/// A folder's file as the deploy will leave it, and what happened to each of its blocks.
struct FolderPlan {
    folder: String,
    file: PathBuf,
    before: Option<String>,
    after: String,
    rows: Vec<Row>,
    rec: FolderRec,
    backups: Vec<(String, String)>,
}

fn separator(text: &str) -> &'static str {
    if text.is_empty() || text.ends_with("\n\n") {
        ""
    } else if text.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    }
}

fn remove_block(text: &mut String, name: &str, prefix: &str) -> Option<String> {
    let (start, stop) = span(text, name)?;
    let removed = text[start..stop].to_string();
    let from = if !prefix.is_empty() && text[..start].ends_with(prefix) {
        start - prefix.len()
    } else {
        start
    };
    text.replace_range(from..stop, "");
    Some(removed)
}

#[derive(Clone, Copy)]
struct Mode {
    /// A person confirmed this run: a block that was changed by hand is replaced (its text is
    /// kept) and a block nothing asks for any more is removed. An unattended run holds both.
    attended: bool,
}

type Wanted<'a> = Vec<(&'a LayerDef, Result<String, String>)>;

fn plan_folder(
    mode: Mode,
    folder: &Path,
    wanted: &[(&LayerDef, Result<String, String>)],
    ledger: &Ledger,
    now: &str,
) -> FolderPlan {
    let key = folder_key(folder);
    let file = folder.join(BLOCK_FILE);
    let before = fsx::read_text(&file, FILE_CAP);
    let mut text = before.clone().unwrap_or_default();
    let mut rec = ledger
        .folders
        .get(&key)
        .cloned()
        .unwrap_or_else(|| FolderRec {
            created_file: before.is_none(),
            layers: BTreeMap::new(),
        });
    let wanted_names: BTreeSet<&str> = wanted.iter().map(|(l, _)| l.name.as_str()).collect();
    let mut rows = Vec::new();
    let mut backups = Vec::new();

    for (layer, built) in wanted {
        let name = layer.name.clone();
        let row =
            |action, reason: Option<String>, before: Option<String>, after: Option<String>| Row {
                layer: name.clone(),
                folder: key.clone(),
                action,
                reason,
                before,
                after,
            };
        let desired = match built {
            Ok(text) => text,
            Err(reason) => {
                rows.push(row("failed", Some(reason.clone()), None, None));
                continue;
            }
        };
        let current = span(&text, &name).map(|(a, b)| text[a..b].to_string());
        let recorded = rec.layers.get(&name).map(|r| r.sha256.clone());
        match current {
            None => {
                let prefix = separator(&text).to_string();
                text.push_str(&prefix);
                text.push_str(desired);
                rec.layers.insert(
                    name.clone(),
                    BlockRec {
                        sha256: sha256_hex(desired),
                        deployed_at: now.to_string(),
                        before: None,
                        prefix,
                    },
                );
                rows.push(row("add", None, None, Some(desired.clone())));
            }
            Some(block) if block == *desired => {
                if recorded.as_deref() != Some(sha256_hex(&block).as_str()) {
                    rec.layers.insert(
                        name.clone(),
                        BlockRec {
                            sha256: sha256_hex(&block),
                            deployed_at: now.to_string(),
                            ..Default::default()
                        },
                    );
                }
                rows.push(row("unchanged", None, None, None));
            }
            Some(block) => {
                let by_hand = recorded.as_deref() != Some(sha256_hex(&block).as_str());
                if by_hand && !mode.attended {
                    rows.push(row(
                        "held",
                        Some("changed by hand since the last deploy; the next sync you confirm replaces it and keeps a copy".into()),
                        Some(block),
                        Some(desired.clone()),
                    ));
                    continue;
                }
                let prefix = rec
                    .layers
                    .get(&name)
                    .map(|r| r.prefix.clone())
                    .unwrap_or_default();
                let (start, stop) = span(&text, &name).unwrap_or((0, 0));
                text.replace_range(start..stop, desired);
                if by_hand {
                    backups.push((name.clone(), block.clone()));
                }
                rec.layers.insert(
                    name.clone(),
                    BlockRec {
                        sha256: sha256_hex(desired),
                        deployed_at: now.to_string(),
                        before: (block.len() <= BEFORE_CAP).then(|| block.clone()),
                        prefix,
                    },
                );
                let action = if by_hand { "replace" } else { "update" };
                let reason = by_hand.then(|| "changed by hand since the last deploy".to_string());
                rows.push(row(action, reason, Some(block), Some(desired.clone())));
            }
        }
    }

    let stale: Vec<String> = rec
        .layers
        .keys()
        .filter(|name| !wanted_names.contains(name.as_str()))
        .cloned()
        .collect();
    for name in stale {
        let row = |action, reason: Option<String>, before: Option<String>| Row {
            layer: name.clone(),
            folder: key.clone(),
            action,
            reason,
            before,
            after: None,
        };
        let Some((start, stop)) = span(&text, &name) else {
            rec.layers.remove(&name);
            continue;
        };
        let block = text[start..stop].to_string();
        let ours = rec
            .layers
            .get(&name)
            .is_some_and(|r| r.sha256 == sha256_hex(&block));
        if !mode.attended {
            rows.push(row(
                "held",
                Some(
                    "no layer asks for this block any more; the next sync you confirm removes it"
                        .into(),
                ),
                Some(block),
            ));
            continue;
        }
        if !ours {
            backups.push((name.clone(), block.clone()));
        }
        let prefix = rec
            .layers
            .get(&name)
            .map(|r| r.prefix.clone())
            .unwrap_or_default();
        remove_block(&mut text, &name, &prefix);
        rec.layers.remove(&name);
        let reason = (!ours).then(|| "changed by hand since the last deploy".to_string());
        rows.push(row("remove", reason, Some(block)));
    }
    if text.trim().is_empty() && rec.created_file && rec.layers.is_empty() {
        text.clear();
    }
    FolderPlan {
        folder: key,
        file,
        before,
        after: text,
        rows,
        rec,
        backups,
    }
}

fn wanted_layers(env: &Env, defs: &[LayerDef]) -> Vec<(PathBuf, usize, Result<String, String>)> {
    let home = fsx::canonical(&env.home);
    let mut out = Vec::new();
    for (index, layer) in defs.iter().enumerate() {
        if layer.scope != Scope::Folder {
            continue;
        }
        for folder in layer_defs::resolve_folders(layer, &env.roots, &env.clients_root) {
            let built = if fsx::canonical(&folder).starts_with(&home) {
                block_text(env, layer)
            } else {
                Err(format!(
                    "{} is outside the home folder; a layer is delivered below it only",
                    folder.display()
                ))
            };
            out.push((folder, index, built));
        }
    }
    out
}

fn backup_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plus").join("context-backups").join("layers")
}

fn write_backups(
    data_dir: &Path,
    now: i64,
    folder: &str,
    backups: &[(String, String)],
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (index, (layer, text)) in backups.iter().enumerate() {
        let slug: String = folder
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let path = backup_dir(data_dir).join(format!("{now}-{index}-{layer}-{slug}.md"));
        crate::fsx::atomic_write(&path, text, crate::fsx::Mode::Private)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        out.push(fsx::display(&path));
    }
    Ok(out)
}

/// Deploys the folder layers, or with `dry_run` lists what it would do. A folder that cannot be
/// written does not stop the others; its rows say `failed`.
pub fn deploy(host: &dyn Host, dry_run: bool, attended: bool) -> Result<Value, OpError> {
    let env = env(host)?;
    let defs = layer_defs::list(&env.roots);
    let mut ledger = load_ledger(&env.data_dir);
    let now_secs = host.now_unix();
    let now = fsx::zulu(now_secs);
    let wanted = wanted_layers(&env, &defs);
    let mut by_folder: BTreeMap<PathBuf, Wanted> = BTreeMap::new();
    for (folder, index, built) in wanted {
        by_folder
            .entry(folder)
            .or_default()
            .push((&defs[index], built));
    }
    for key in ledger.folders.keys() {
        let path = PathBuf::from(key);
        if fsx::is_dir(&path) {
            by_folder.entry(path).or_default();
        }
    }
    let mode = Mode { attended };
    let mut rows = Vec::new();
    let mut files = Vec::new();
    let mut changed = Vec::new();
    let mut backups = Vec::new();
    let mut up_to_date = 0;
    for (folder, wanted) in &by_folder {
        let plan = plan_folder(mode, folder, wanted, &ledger, &now);
        let file = fsx::display(&plan.file);
        let writes = plan.before.as_deref().unwrap_or_default() != plan.after;
        let mut failure: Option<String> = None;
        if writes && !dry_run {
            let kept = write_backups(&env.data_dir, now_secs, &plan.folder, &plan.backups)
                .and_then(|made| {
                    backups.extend(made);
                    if plan.after.is_empty() && plan.before.is_some() {
                        std::fs::remove_file(&plan.file)
                            .map_err(|e| format!("{}: {e}", plan.file.display()))
                    } else {
                        crate::fsx::atomic_write(
                            &plan.file,
                            &plan.after,
                            crate::fsx::Mode::KeepOrDefault,
                        )
                        .map_err(|e| format!("{}: {e}", plan.file.display()))
                    }
                })
                .and_then(|()| {
                    ensure_exclude_line(folder, BLOCK_FILE)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                });
            failure = kept.err();
        }
        for row in &plan.rows {
            if row.action == "unchanged" {
                up_to_date += 1;
                continue;
            }
            let mut value = row.value(&file, dry_run);
            if let (Some(why), true) = (&failure, row.action != "held" && row.action != "failed") {
                value["action"] = json!("failed");
                value["reason"] = json!(why);
                value["applied"] = json!(false);
            }
            rows.push(value);
        }
        if failure.is_none() {
            if writes {
                files.push(file.clone());
            }
            if !dry_run {
                if writes {
                    changed.push(file);
                }
                if plan.rec.layers.is_empty() {
                    ledger.folders.remove(&plan.folder);
                } else {
                    ledger.folders.insert(plan.folder.clone(), plan.rec.clone());
                }
            }
        }
    }
    if !dry_run {
        save_ledger(&env.data_dir, &ledger).map_err(|e| OpError::failed("ledger", e))?;
    }
    Ok(json!({
        "rows": rows,
        "upToDate": up_to_date,
        "files": files,
        "changed": changed,
        "backups": backups,
        "undo": if changed.is_empty() { Value::Null } else { json!("toolportctl context layers undo") },
    }))
}

/// Puts back what the last deploy replaced and removes the blocks it added, in every folder
/// where the block still is what the deploy wrote. A block changed since is left and named.
pub fn undo(host: &dyn Host, dry_run: bool) -> Result<Value, OpError> {
    let env = env(host)?;
    let mut ledger = load_ledger(&env.data_dir);
    let mut rows = Vec::new();
    let mut changed = Vec::new();
    for (key, rec) in ledger.folders.clone() {
        let file = PathBuf::from(&key).join(BLOCK_FILE);
        let Some(original) = fsx::read_text(&file, FILE_CAP) else {
            ledger.folders.remove(&key);
            continue;
        };
        let mut text = original.clone();
        let mut kept = rec.clone();
        for (name, block) in &rec.layers {
            let Some((start, stop)) = span(&text, name) else {
                kept.layers.remove(name);
                continue;
            };
            if sha256_hex(&text[start..stop]) != block.sha256 {
                rows.push(json!({
                    "layer": name, "folder": key, "action": "left",
                    "reason": "the block was changed by hand after the deploy",
                }));
                continue;
            }
            match &block.before {
                Some(before) => text.replace_range(start..stop, before),
                None => {
                    remove_block(&mut text, name, &block.prefix);
                }
            }
            kept.layers.remove(name);
            rows.push(json!({
                "layer": name, "folder": key,
                "action": if block.before.is_some() { "restore" } else { "remove" },
            }));
        }
        if text.trim().is_empty() && rec.created_file && kept.layers.is_empty() {
            text.clear();
        }
        if text != original {
            if !dry_run {
                if text.is_empty() {
                    std::fs::remove_file(&file).map_err(|e| {
                        OpError::failed("write", format!("{}: {e}", file.display()))
                    })?;
                } else {
                    crate::fsx::atomic_write(&file, &text, crate::fsx::Mode::KeepOrDefault)
                        .map_err(|e| {
                            OpError::failed("write", format!("{}: {e}", file.display()))
                        })?;
                }
            }
            changed.push(fsx::display(&file));
        }
        if kept.layers.is_empty() {
            ledger.folders.remove(&key);
        } else {
            ledger.folders.insert(key, kept);
        }
    }
    if !dry_run {
        save_ledger(&env.data_dir, &ledger).map_err(|e| OpError::failed("ledger", e))?;
    }
    Ok(json!({ "dryRun": dry_run, "rows": rows, "changed": changed }))
}

/// For `layers ls`: where each folder layer lands and what state its blocks are in.
pub(super) struct Landing {
    pub folders: Vec<String>,
    pub deployed: usize,
    pub edited: usize,
    pub outdated: usize,
    pub missing: usize,
    pub problem: Option<String>,
}

pub(super) fn landing(env: &Env, layer: &LayerDef) -> Landing {
    let ledger = load_ledger(&env.data_dir);
    let wanted = block_text(env, layer);
    let folders = layer_defs::resolve_folders(layer, &env.roots, &env.clients_root);
    let (mut deployed, mut edited, mut outdated, mut missing) = (0, 0, 0, 0);
    for folder in &folders {
        let file = folder.join(BLOCK_FILE);
        let text = fsx::read_text(&file, FILE_CAP).unwrap_or_default();
        match span(&text, &layer.name).map(|(a, b)| &text[a..b]) {
            None => missing += 1,
            Some(block) => {
                let recorded = ledger
                    .folders
                    .get(&folder_key(folder))
                    .and_then(|rec| rec.layers.get(&layer.name))
                    .map(|r| r.sha256.as_str());
                let same = wanted.as_ref().is_ok_and(|w| w == block);
                if same {
                    deployed += 1;
                } else if recorded == Some(sha256_hex(block).as_str()) {
                    outdated += 1;
                } else {
                    edited += 1;
                }
            }
        }
    }
    Landing {
        folders: folders.iter().map(|f| fsx::display(f)).collect(),
        deployed,
        edited,
        outdated,
        missing,
        problem: wanted.err(),
    }
}
