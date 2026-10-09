//! `context leave-out add|rm`: leaving one CLAUDE.md or rule file out of a folder's stack, and
//! turning auto memory off there. Both ride the plugin-control path of the bundle ledger
//! (`bundle_controls`): the keys are `claudeMdExcludes` entries and `autoMemoryEnabled` in the
//! folder's `settings.local.json`, recorded under the control `leave-out`, so a bundle and this
//! control never undo each other's keys. The file itself is never touched.

use super::bundle_api::op as bundle_op;
use super::bundle_apply::Desired;
use super::bundle_controls::{self as controls, Run};
use super::config::ContextConfig;
use super::loads::{what_loads_with, LoadsOptions};
use super::roots::Roots;
use crate::plus::op::OpError;
use crate::plus::plan::{Effects, PlanV1, ResultV1, TokenEffect};
use crate::plus::sources::fsx;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const CONTROL: &str = "leave-out";
pub const AUTO_MEMORY_OFF: &str = "auto memory is off (autoMemoryEnabled)";
const GLOB_CHARS: [char; 4] = ['*', '?', '[', '{'];
const NEXT_SESSION: &str = "Open sessions keep what they loaded; the change applies from the next session";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Rm,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    File(String),
    Memory,
}

pub struct Args<'a> {
    pub op: Op,
    pub target: Target,
    pub cwd: &'a Path,
    pub dry_run: bool,
}

pub struct World<'a> {
    pub roots: &'a Roots,
    pub config: &'a ContextConfig,
    pub data_dir: &'a Path,
}

/// What the control owns in a folder: the left-out files (as written) and whether auto memory is off.
pub fn owned(data_dir: &Path, cwd: &Path) -> (Vec<String>, bool) {
    let Some(rec) = controls::record(data_dir, cwd, CONTROL) else {
        return (Vec::new(), false);
    };
    let drift = controls::drifted(data_dir, cwd, CONTROL);
    let files = rec
        .settings
        .owned
        .iter()
        .filter(|o| o.kind == "entry" && o.path == ["claudeMdExcludes"])
        .filter_map(|o| serde_json::from_str::<String>(&o.written).ok())
        .collect();
    let memory = rec
        .settings
        .owned
        .iter()
        .any(|o| o.kind == "set" && o.path == ["autoMemoryEnabled"] && !drift.contains(&o.label()));
    (files, memory)
}

/// Say who keeps each left-out row out: the `leave-out` control, or settings Toolport did not write.
pub fn mark(items: &mut [super::loads::LoadItem], data_dir: &Path, cwd: &Path) {
    let (files, memory) = owned(data_dir, &fsx::canonical(cwd));
    for item in items.iter_mut().filter(|i| !i.loaded && !i.lazy) {
        let by_toolport = match (item.kind, item.path.as_deref()) {
            ("memory-index", _) if item.reason == AUTO_MEMORY_OFF => Some(memory),
            ("memory" | "rule", Some(path)) if item.reason.starts_with("excluded") => {
                Some(files.iter().any(|f| same_file(f, path)))
            }
            _ => None,
        };
        item.left_out_by = by_toolport.map(|own| if own { "toolport" } else { "settings" });
    }
}

fn quote(text: &str) -> String {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "_./-:@%+=,~".contains(c)) {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

fn absolute(path: &str) -> Result<String, OpError> {
    let full: PathBuf = match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()
            .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?
            .join(rest),
        None => PathBuf::from(path),
    };
    if !full.is_absolute() {
        return Err(OpError::usage(format!("path must be absolute or start with ~/: {path}")));
    }
    Ok(fsx::display(&full))
}

fn same_file(a: &str, b: &str) -> bool {
    fsx::canonical(Path::new(a)) == fsx::canonical(Path::new(b))
}

fn folder(cwd: &Path) -> Result<PathBuf, OpError> {
    if !fsx::is_dir(cwd) {
        return Err(OpError::usage(format!("cwd is not a folder: {}", cwd.display())));
    }
    Ok(fsx::canonical(cwd))
}

fn plan_of(summary: String, run: &Run, mut warnings: Vec<String>, undo: String, tokens: Option<TokenEffect>) -> PlanV1 {
    warnings.extend(run.warnings.iter().cloned());
    PlanV1 { summary, steps: run.steps.clone(), effects: Effects { tokens }, warnings, undo }
}

pub fn run(world: &World, args: &Args) -> Result<Value, OpError> {
    let cwd = folder(args.cwd)?;
    let (mut files, mut memory) = owned(world.data_dir, &cwd);
    let cwd_text = quote(&fsx::display(&cwd));
    let mut warnings = Vec::new();
    let mut tokens = None;
    let (summary, undo) = match (&args.op, &args.target) {
        (Op::Add, Target::File(raw)) => {
            let wanted = absolute(raw)?;
            let report = what_loads_with(world.roots, world.config, None, &cwd, &LoadsOptions::default())
                .map_err(|e| OpError::failed("context_invalid", e))?;
            let item = report
                .items
                .iter()
                .find(|i| {
                    matches!(i.kind, "memory" | "rule") && i.path.as_deref().is_some_and(|p| same_file(p, &wanted))
                })
                .ok_or_else(|| {
                    OpError::not_found(format!("{wanted} is not an instruction file Claude Code reads in this folder"))
                })?;
            let path = item.path.clone().unwrap_or(wanted);
            if item.source == "policy" {
                return Err(OpError::conflict(
                    "a managed policy CLAUDE.md cannot be left out: Claude Code always loads it",
                ));
            }
            if !item.loaded && !item.lazy && item.reason.starts_with("excluded") && !files.contains(&path) {
                warnings.push(format!("{path} is already left out by settings Toolport did not write; nothing is added"));
            } else if !files.contains(&path) {
                files.push(path.clone());
                if item.loaded {
                    tokens = Some(TokenEffect {
                        before: report.total_tokens,
                        after: report.total_tokens.saturating_sub(item.tokens),
                        basis: "estimate",
                    });
                }
            }
            if path.contains(GLOB_CHARS) {
                warnings.push(format!("{path} contains a glob character; Claude Code reads the entry as a pattern"));
            }
            if item.source == "org" {
                warnings.push("The org file is replaced by its owner from time to time; the entry matches the path, not the content".into());
            }
            warnings.push(NEXT_SESSION.into());
            (
                format!("Leave out {path} in {}", cwd.display()),
                format!("toolportctl context leave-out rm {} --cwd {cwd_text}", quote(&path)),
            )
        }
        (Op::Rm, Target::File(raw)) => {
            let path = absolute(raw)?;
            if let Some(at) = files.iter().position(|f| same_file(f, &path)) {
                files.remove(at);
            } else {
                warnings.push(format!(
                    "{path} is not left out by Toolport here; settings Toolport did not write are left alone"
                ));
            }
            warnings.push(NEXT_SESSION.into());
            (
                format!("Load {path} again in {}", cwd.display()),
                format!("toolportctl context leave-out add {} --cwd {cwd_text}", quote(&path)),
            )
        }
        (Op::Add, Target::Memory) => {
            memory = true;
            warnings.push("Claude then neither reads from nor writes to this project's auto memory directory".into());
            warnings.push(NEXT_SESSION.into());
            (
                format!("Turn auto memory off in {}", cwd.display()),
                format!("toolportctl context leave-out rm --memory --cwd {cwd_text}"),
            )
        }
        (Op::Rm, Target::Memory) => {
            memory = false;
            warnings.push(NEXT_SESSION.into());
            (
                format!("Turn auto memory back on in {}", cwd.display()),
                format!("toolportctl context leave-out add --memory --cwd {cwd_text}"),
            )
        }
    };
    let want = Desired { excludes: files.clone(), memory_off: memory, ..Desired::default() };
    let run = controls::apply(world.data_dir, &cwd, CONTROL, &want, args.dry_run).map_err(bundle_op)?;
    let plan = plan_of(summary, &run, warnings, undo.clone(), tokens);
    let result = (!args.dry_run).then(|| ResultV1 {
        applied: true,
        changed: run.changed.clone(),
        undo,
        backups: Vec::new(),
    });
    Ok(json!({
        "id": CONTROL,
        "scope": "folder",
        "cwd": fsx::display(&cwd),
        "dryRun": args.dry_run,
        "plan": plan,
        "result": result,
        "leftOut": files,
        "memoryOff": memory,
    }))
}

/// The command-line and self-MCP entry: the host's roots, config and data directory, then [`run`].
pub fn api(op: Op, target: Target, cwd: Option<&str>, dry_run: bool) -> Result<Value, OpError> {
    let cwd = cwd.ok_or_else(|| OpError::usage("cwd is required: a file is left out per folder"))?;
    let (roots, _) = crate::plus::sources::host_world()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data = crate::registry::conduit_dir()
        .ok_or_else(|| OpError::failed("no_data_dir", "the data directory could not be resolved"))?;
    let config = super::load_config(&roots.context_config_path());
    run(
        &World { roots: &roots, config: &config, data_dir: &data },
        &Args { op, target, cwd: Path::new(cwd), dry_run },
    )
}
