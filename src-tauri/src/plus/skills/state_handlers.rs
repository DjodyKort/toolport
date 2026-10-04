//! `plus.skills.{clean,uninstall,resolve}`: lifecycle operations over `skills::ops`. Arguments use
//! snake_case keys; `global_mode` (default true) selects user-level scope like `plus.skills.sync`.
//! `clean` and `uninstall` take `repo_path` literally (default: the working directory), as mcpm does.

use super::clock::SystemClock;
use super::collisions::{Action, CollisionSummary, BACKUP_DIR_NAME};
use super::lock::load_lockfile;
use super::ops::{self, ResolveRequest, Scope};
use super::parser::discover_skills;
use super::repo::{find_repo, resolve_path};
use super::transpilers::registry_with_home;
use crate::plus::args::{flag, flag_or, str_nonempty};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(in crate::plus::skills) fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(in crate::plus::skills) fn paths(list: &[PathBuf]) -> Vec<String> {
    list.iter().map(|p| display(p)).collect()
}

pub(in crate::plus::skills) fn scope_name(global: bool) -> &'static str {
    if global {
        "global"
    } else {
        "project"
    }
}

pub(in crate::plus::skills) fn literal_repo(args: &Value) -> PathBuf {
    resolve_path(Path::new(str_nonempty(args, "repo_path").unwrap_or(".")))
}

pub fn clean_handler(args: Value) -> Result<Value, String> {
    let global = flag_or(&args, "global_mode", true);
    let dry_run = flag(&args, "dry_run");
    let scope = Scope::new(global, &literal_repo(&args))?;
    let lock = load_lockfile(&scope.lock_dir);
    let registry = registry_with_home(crate::clients::home());
    let out = ops::clean_skills(
        &scope.lock_dir,
        &scope.output_root,
        &registry,
        str_nonempty(&args, "client"),
        lock.as_ref(),
        dry_run,
    );
    Ok(json!({
        "scope": scope_name(global),
        "lockDir": display(&scope.lock_dir),
        "cleanRoot": display(&scope.output_root),
        "dryRun": dry_run,
        "lockfilePresent": lock.is_some(),
        "managed": out.managed,
        "removed": paths(&out.removed),
        "lockfileRemoved": out.lockfile_removed,
        "skipped": out
            .skipped
            .iter()
            .map(|(client, error)| json!({"client": client, "error": error}))
            .collect::<Vec<_>>(),
        "ignored": out.ignored,
    }))
}

pub fn uninstall_handler(args: Value) -> Result<Value, String> {
    let global = flag_or(&args, "global_mode", true);
    let dry_run = flag(&args, "dry_run");
    let name = str_nonempty(&args, "name").ok_or("name is required")?;
    let repo = literal_repo(&args);
    let scope = Scope::new(global, &repo)?;
    let registry = registry_with_home(crate::clients::home());
    let out = ops::uninstall_skill(&repo, name, &scope, &registry, dry_run)?;
    Ok(json!({
        "repo": display(&repo),
        "name": name,
        "scope": scope_name(global),
        "lockDir": display(&scope.lock_dir),
        "outputRoot": display(&scope.output_root),
        "dryRun": dry_run,
        "sourcePath": display(&out.source),
        "outputs": paths(&out.outputs),
        "lockUpdated": out.lock_updated,
    }))
}

fn action_name(action: &Action) -> &'static str {
    match action {
        Action::Replaced => "replaced",
        Action::Kept => "kept",
        Action::SkippedDryRun => "skipped-dry-run",
    }
}

pub fn resolve_handler(args: Value) -> Result<Value, String> {
    let global = flag_or(&args, "global_mode", true);
    let dry_run = flag(&args, "dry_run");
    let migrate = args.get("migrate").and_then(Value::as_bool);
    let start = str_nonempty(&args, "repo_path").map(|p| resolve_path(Path::new(p)));
    let repo = find_repo(start.as_deref()).ok_or("no skills repository found")?;
    let skills = discover_skills(&repo);
    let scope = Scope::new(global, &repo)?;
    let summary = if skills.is_empty() {
        CollisionSummary::default()
    } else {
        let request = ResolveRequest {
            client: str_nonempty(&args, "client"),
            global_mode: global,
            dry_run,
            migrate,
            output_root: &scope.output_root,
            clock: &SystemClock,
        };
        let registry = registry_with_home(crate::clients::home());
        ops::resolve_skill_collisions(&skills, &registry, &request)?.1
    };
    Ok(json!({
        "repo": display(&repo),
        "scope": scope_name(global),
        "outputRoot": display(&scope.output_root),
        "backupRoot": display(&scope.output_root.join(BACKUP_DIR_NAME)),
        "dryRun": dry_run,
        "migrate": migrate,
        "skillCount": skills.len(),
        "replaced": summary.replaced().count(),
        "kept": summary.kept().count(),
        "collisions": summary
            .resolutions
            .iter()
            .map(|r| json!({
                "skill": r.collision.skill_name,
                "client": r.collision.client_key,
                "collisionPath": display(&r.collision.collision_path),
                "syncedPath": display(&r.collision.synced_path),
                "action": action_name(&r.action),
                "backupPath": r.backup_path.as_deref().map(display),
            }))
            .collect::<Vec<_>>(),
    }))
}
