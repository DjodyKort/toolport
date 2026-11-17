//! The auto memory switch: three positions per repository (Default, On, Off) written as
//! `autoMemoryEnabled` in the local settings file at the repository root, and a global default
//! written in the user settings. `context leave-out add|rm --memory` is the same handler.

use super::overview;
use super::resolve::{Layers, Repo};
use super::World;
use crate::context::fsx;
use crate::context::leave_out;
use crate::context::loads::{InstructionLoads, LoadSource};
use crate::context::plan::{Diff, Effects, PlanV1, ResultV1, Step, TokenEffect};
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::Path;
use toolport_plus_api::SettingsStore;

const NEXT_SESSION: &str =
    "Open sessions keep what they loaded; the change applies from the next session";
const ROOT_FILE_NOTE: &str = "Claude Code documents reading the local settings file at the repository root; that it does so for a session started in a subfolder or a linked worktree is not verified on Claude Code 2.1.326, so the card shows Confirmed only after a measurement";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Default,
    On,
    Off,
}

impl Position {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "default" => Some(Self::Default),
            "on" => Some(Self::On),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    pub fn of(value: Option<bool>) -> Self {
        match value {
            None => Self::Default,
            Some(true) => Self::On,
            Some(false) => Self::Off,
        }
    }

    fn value(self) -> Option<bool> {
        match self {
            Self::Default => None,
            Self::On => Some(true),
            Self::Off => Some(false),
        }
    }
}

fn effects(world: &World, repo: &Repo, layers: &Layers, becomes: Option<bool>) -> Effects {
    let loads = InstructionLoads {
        home: world.home,
        policy_file: world.policy_claude_md,
        count_tokens: world.count_tokens,
    };
    let Ok(found) = loads.what_loads(&repo.cwd) else {
        return Effects::default();
    };
    let (dir, _) = layers.directory(world.home, repo);
    let Some(index) = super::inventory::index(&dir, world.count_tokens) else {
        return Effects::default();
    };
    let before = found.total_tokens;
    let enabled_now = layers.effective().0;
    let mut simulated = layers.clone();
    simulated.local_root.enabled = becomes;
    simulated.local_cwd.enabled = None;
    let enabled_after = simulated.effective().0;
    let after = match (enabled_now, enabled_after) {
        (true, false) => before.saturating_sub(index.tokens),
        (false, true) => before + index.tokens,
        _ => before,
    };
    Effects {
        tokens: Some(TokenEffect {
            before,
            after,
            basis: "estimate",
        }),
    }
}

fn state_json(layers: &Layers) -> Value {
    let (enabled, by) = layers.effective();
    json!({
        "enabled": enabled,
        "setBy": by.as_str(),
        "switch": Position::of(layers.switch()).as_str(),
    })
}

/// Sets one repository's switch. `cwd` is any folder of the repository.
pub fn set_repository(
    world: &World,
    cwd: &Path,
    position: Position,
    dry_run: bool,
) -> Result<Value, OpError> {
    if let Some(problem) = fsx::folder_problem(cwd) {
        return Err(OpError::usage(format!("cwd {problem}")));
    }
    let repo = Repo::of(cwd);
    let layers = world.layers(&repo);
    if layers.managed.enabled.is_some() {
        return Err(OpError::conflict(
            "managed settings decide auto memory on this Mac; a repository switch cannot override them",
        ));
    }
    let held = leave_out::held_in(world.data_dir, &repo.cwd);
    let previous = Position::of(layers.switch());
    let run = leave_out::write_state(
        world.data_dir,
        &repo.cwd,
        &held.files,
        position.value(),
        dry_run,
    )?;
    let mut warnings = run.warnings.clone();
    let by_hand = previous != Position::Default && held.memory.is_none();
    if by_hand {
        warnings.push(
            "autoMemoryEnabled is already set in this repository's local settings by hand; Toolport replaces it and puts it back on undo"
                .into(),
        );
    }
    if position == Position::On && layers.user.enabled != Some(false) {
        warnings.push(
            "The global default is on, so On changes nothing now; it keeps memory on if the default is turned off later"
                .into(),
        );
    }
    if position == Position::Default && by_hand {
        warnings.push(
            "The file still holds the value that was there before; Toolport does not own it".into(),
        );
    }
    if let Some(value) = layers.env.as_deref().filter(|v| !v.trim().is_empty()) {
        warnings.push(format!(
            "{} is set to {value} in Toolport's environment and takes precedence over this file",
            super::resolve::ENV_DISABLE
        ));
    }
    if repo.cwd != repo.root {
        warnings.push(ROOT_FILE_NOTE.into());
    }
    warnings.push(NEXT_SESSION.into());
    let cwd_text = fsx::display(&repo.cwd);
    let undo = format!(
        "toolportctl memory set {} --cwd {cwd_text}",
        previous.as_str()
    );
    let plan = PlanV1 {
        summary: format!(
            "Set auto memory to {} for {}",
            position.as_str(),
            repo.name()
        ),
        steps: run.steps.clone(),
        effects: effects(world, &repo, &layers, position.value()),
        warnings,
        undo: undo.clone(),
    };
    let result = (!dry_run).then(|| ResultV1 {
        applied: true,
        changed: run.changed.clone(),
        undo,
        backups: Vec::new(),
    });
    let mut after = layers.clone();
    after.local_root.enabled = position.value();
    after.local_cwd.enabled = None;
    Ok(json!({
        "scope": "repository",
        "cwd": cwd_text,
        "root": fsx::display(&repo.root),
        "name": repo.name(),
        "position": position.as_str(),
        "previous": previous.as_str(),
        "dryRun": dry_run,
        "plan": plan,
        "result": result,
        "state": state_json(&after),
        "conflicts": run.conflicts,
    }))
}

/// Sets the global default: `autoMemoryEnabled` in `~/.claude/settings.json`.
pub fn set_global(
    world: &World,
    store: &dyn SettingsStore,
    position: Position,
    dry_run: bool,
) -> Result<Value, OpError> {
    let file = world.home.join(".claude/settings.json");
    let (mut root, original) = store
        .read_settings(&file)
        .map_err(|e| OpError::failed("settings_unreadable", e))?;
    if !root.is_object() {
        return Err(OpError::failed(
            "settings_unreadable",
            format!("{} is not a JSON object", file.display()),
        ));
    }
    let previous = Position::of(root.get("autoMemoryEnabled").and_then(Value::as_bool));
    match position.value() {
        Some(on) => root["autoMemoryEnabled"] = Value::Bool(on),
        None => {
            root.as_object_mut().map(|m| m.remove("autoMemoryEnabled"));
        }
    }
    let show = |p: Position| match p.value() {
        Some(on) => format!("autoMemoryEnabled: {on}\n"),
        None => "autoMemoryEnabled: (absent)\n".to_string(),
    };
    let mut steps = Vec::new();
    let mut warnings = Vec::new();
    if previous != position {
        let diff = Diff {
            before: show(previous),
            after: show(position),
        };
        let path = fsx::display(&file);
        steps.push(if original.is_some() {
            Step::merge(
                &path,
                "autoMemoryEnabled in the user settings",
                &["autoMemoryEnabled"],
                diff,
            )
        } else {
            Step::create(
                &path,
                "autoMemoryEnabled in the user settings",
                &["autoMemoryEnabled"],
                diff,
            )
        });
    } else {
        warnings.push("The global default already has this value; nothing changes".into());
    }
    let overrides = overview::repositories_with_own_switch(world);
    if overrides > 0 {
        warnings.push(if overrides == 1 {
            "1 repository has its own switch and keeps it".to_string()
        } else {
            format!("{overrides} repositories have their own switch and keep it")
        });
    }
    if let Some(value) = world
        .env_disable
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        warnings.push(format!(
            "{} is set to {value} in Toolport's environment and takes precedence",
            super::resolve::ENV_DISABLE
        ));
    }
    warnings.push(NEXT_SESSION.into());
    let undo = format!("toolportctl memory set {} --global", previous.as_str());
    let mut changed = Vec::new();
    if !dry_run && previous != position {
        store
            .write_key(&file, original.as_deref(), &root, "autoMemoryEnabled")
            .map_err(|e| OpError::failed("settings_unwritable", e))?;
        changed.push(fsx::display(&file));
    }
    let plan = PlanV1 {
        summary: format!(
            "Set the global auto memory default to {}",
            position.as_str()
        ),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let result = (!dry_run).then(|| ResultV1 {
        applied: true,
        changed,
        undo,
        backups: Vec::new(),
    });
    Ok(json!({
        "scope": "global",
        "file": fsx::display(&file),
        "position": position.as_str(),
        "previous": previous.as_str(),
        "overrides": overrides,
        "dryRun": dry_run,
        "plan": plan,
        "result": result,
    }))
}
