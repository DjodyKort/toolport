//! Lifecycle tools for skills, agents and styles: thin adapters over the `plus.skills.*`,
//! `plus.agents.*` and `plus.styles.*` handlers that `toolportctl skills|agents|styles` render.
//! The handlers default `dry_run` to false; every writing tool here defaults it to true, and the
//! repository is always the discovered one, never the working directory.

use super::backend::skills_repo;
use super::content::path_safe;
use super::ToolError;
use crate::plus::args::{flag_or, list, str_arg, str_nonempty};
use crate::plus::skills::{repo_handlers, state_handlers};
use serde_json::{json, Map, Value};
use std::path::Path;

type Outcome = Result<Value, ToolError>;
type Handler = fn(Value) -> Result<Value, String>;

const ROOT_MANIFEST: &str = "mcpm-skills.yaml";

fn failed(message: String) -> ToolError {
    let kind = if message.contains("not found") || message.contains("No such file") {
        "not_found"
    } else if message.starts_with("Invalid ") || message.starts_with("Not a valid") {
        "invalid_arguments"
    } else if message.contains("outside the repository") {
        "refused"
    } else {
        "backend_error"
    };
    ToolError::new(kind, message)
}

fn dry_run(args: &Value) -> bool {
    flag_or(args, "dry_run", true)
}

fn request(args: &Value) -> Result<Map<String, Value>, ToolError> {
    let repo = skills_repo(args)?;
    let mut map = Map::new();
    map.insert("repo_path".into(), json!(repo.to_string_lossy()));
    Ok(map)
}

fn scoped(args: &Value, handler: Handler, extra: &[(&str, Value)]) -> Outcome {
    let mut map = request(args)?;
    map.insert("dry_run".into(), json!(dry_run(args)));
    if let Some(global) = args.get("global_mode").and_then(Value::as_bool) {
        map.insert("global_mode".into(), json!(global));
    }
    if let Some(client) = str_nonempty(args, "client") {
        map.insert("client".into(), json!(client));
    }
    for (key, value) in extra {
        map.insert((*key).to_string(), value.clone());
    }
    handler(Value::Object(map)).map_err(failed)
}

fn named(args: &Value, handler: Handler) -> Outcome {
    let name = path_safe(str_arg(args, "name").unwrap_or_default())?;
    scoped(args, handler, &[("name", json!(name))])
}

fn has_zip_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

pub(super) fn skills_audit(args: &Value) -> Outcome {
    repo_handlers::audit_handler(Value::Object(request(args)?)).map_err(failed)
}

/// A bundle is only ever a new `.zip` in an existing directory; the core would overwrite any
/// path the caller names.
fn check_bundle_target(planned: &Value) -> Result<(), ToolError> {
    let output = Path::new(planned["output"].as_str().unwrap_or_default());
    if !has_zip_extension(output) {
        return Err(ToolError::new(
            "invalid_arguments",
            "output must end in .zip",
        ));
    }
    if output.symlink_metadata().is_ok() {
        return Err(ToolError::new(
            "conflict",
            format!("{} already exists; pass another output", output.display()),
        ));
    }
    if !output.parent().is_some_and(Path::is_dir) {
        return Err(ToolError::new(
            "invalid_arguments",
            "the directory of output must exist",
        ));
    }
    Ok(())
}

pub(super) fn skills_bundle(args: &Value) -> Outcome {
    let mut map = request(args)?;
    if let Some(output) = str_nonempty(args, "output") {
        map.insert("output".into(), json!(output));
    }
    if let Some(names) = list(args, "skills") {
        map.insert("skills".into(), Value::Array(names.clone()));
    }
    map.insert("dry_run".into(), json!(true));
    let planned = repo_handlers::bundle_handler(Value::Object(map.clone())).map_err(failed)?;
    check_bundle_target(&planned)?;
    if dry_run(args) {
        return Ok(planned);
    }
    map.insert("dry_run".into(), json!(false));
    repo_handlers::bundle_handler(Value::Object(map)).map_err(failed)
}

fn outside_the_trees(planned: &Value) -> Vec<String> {
    planned["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|file| {
            !(file.starts_with("skills/") || file.starts_with("rules/") || *file == ROOT_MANIFEST)
        })
        .take(5)
        .map(String::from)
        .collect()
}

/// A bundle may only carry skill and rule trees and the repository manifest: the core writes any
/// relative path, which would let a bundle drop `.git/hooks/pre-commit` into the repository.
pub(super) fn skills_unbundle(args: &Value) -> Outcome {
    let bundle = str_nonempty(args, "bundle_path")
        .ok_or_else(|| ToolError::new("invalid_arguments", "bundle_path is empty"))?;
    if !has_zip_extension(Path::new(bundle)) {
        return Err(ToolError::new(
            "invalid_arguments",
            "bundle_path must end in .zip",
        ));
    }
    let mut map = request(args)?;
    map.insert("bundle_path".into(), json!(bundle));
    map.insert("dry_run".into(), json!(true));
    let planned = repo_handlers::unbundle_handler(Value::Object(map.clone())).map_err(failed)?;
    let stray = outside_the_trees(&planned);
    if !stray.is_empty() {
        return Err(ToolError::new(
            "refused",
            format!(
                "the bundle holds files outside skills/ and rules/: {}",
                stray.join(", ")
            ),
        ));
    }
    if dry_run(args) {
        return Ok(planned);
    }
    map.insert("dry_run".into(), json!(false));
    repo_handlers::unbundle_handler(Value::Object(map)).map_err(failed)
}

pub(super) fn skills_clean(args: &Value) -> Outcome {
    scoped(args, state_handlers::clean_handler, &[])
}

pub(super) fn skills_uninstall(args: &Value) -> Outcome {
    named(args, state_handlers::uninstall_handler)
}

pub(super) fn skills_resolve(args: &Value) -> Outcome {
    let migrate: Vec<(&str, Value)> = args
        .get("migrate")
        .and_then(Value::as_bool)
        .map(|on| ("migrate", json!(on)))
        .into_iter()
        .collect();
    scoped(args, state_handlers::resolve_handler, &migrate)
}
