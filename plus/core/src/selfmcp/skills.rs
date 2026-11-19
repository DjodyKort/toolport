//! The `skills_*` tools: adapters over `crate::skills`, the same operations `toolportctl skills`
//! renders. A tool that writes defaults `dry_run` to true and never works in the working directory.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use toolport_plus_api::Host;

use super::tasks::refused;
use super::ToolError;
use crate::args::{flag, flag_or, str_arg, str_nonempty};
use crate::op::ErrorKind;
use crate::skills::agents::handlers as agents;
use crate::skills::api::{self, Args};
use crate::skills::auto;
use crate::skills::compare::{compare, keep_files};
use crate::skills::files::skill_files;
use crate::skills::repo::{skill_bucket, skill_template};
use crate::skills::styles::handlers as styles;
use crate::skills::{repo_handlers, state, tap_handlers as taps, SystemClock};
use crate::tasks::approval;

type Outcome = Result<Value, ToolError>;
type Handler = fn(Value) -> Result<Value, String>;

const ROOT_MANIFEST: &str = "mcpm-skills.yaml";

fn has_zip_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

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

fn with_paths<R>(host: &dyn Host, run: impl FnOnce() -> R) -> R {
    let _guard = crate::fsx::enter(crate::fsx::Paths {
        home: None,
        data_dir: host.data_dir(),
    });
    run()
}

pub(super) fn repo_of(args: &Value) -> Result<PathBuf, ToolError> {
    let start = str_arg(args, "repo_path").map(PathBuf::from);
    Ok(api::resolve_repo(start.as_deref())?)
}

fn request(args: &Value) -> Result<Map<String, Value>, ToolError> {
    let mut map = Map::new();
    map.insert("repo_path".into(), json!(repo_of(args)?.to_string_lossy()));
    Ok(map)
}

pub(super) fn path_safe(name: &str) -> Result<&str, ToolError> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if ok {
        Ok(name)
    } else {
        Err(ToolError::new(
            "invalid_arguments",
            format!("invalid name: {name}"),
        ))
    }
}

pub(super) fn kebab(name: &str) -> Result<&str, ToolError> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(name)
    } else {
        Err(ToolError::new(
            "invalid_arguments",
            "name must be lowercase alphanumeric and hyphens",
        ))
    }
}

fn scoped(args: &Value, handler: Handler, extra: &[(&str, Value)]) -> Outcome {
    handler(scoped_request(args, extra)?).map_err(failed)
}

fn scoped_request(args: &Value, extra: &[(&str, Value)]) -> Result<Value, ToolError> {
    let mut map = request(args)?;
    map.insert("dry_run".into(), json!(flag_or(args, "dry_run", true)));
    if let Some(global) = args.get("global_mode").and_then(Value::as_bool) {
        map.insert("global_mode".into(), json!(global));
    }
    if let Some(client) = str_nonempty(args, "client") {
        map.insert("client".into(), json!(client));
    }
    for (key, value) in extra {
        map.insert((*key).to_string(), value.clone());
    }
    Ok(Value::Object(map))
}

pub(super) fn list(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || Ok(api::list_skills(&Args::from_json(args))?))
}

pub(super) fn get(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || Ok(api::get(&Args::from_json(args))?))
}

pub(super) fn lint(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || Ok(api::lint(&Args::from_json(args))?))
}

pub(super) fn files(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || Ok(skill_files(&Args::from_json(args))?))
}

pub(super) fn fix(host: &dyn Host, args: &Value) -> Outcome {
    let kind = str_nonempty(args, "fix")
        .ok_or_else(|| ToolError::new("invalid_arguments", "fix is empty"))?;
    let mut fix = Args::from_json(args);
    fix.dry_run = flag_or(args, "dry_run", true);
    with_paths(host, || Ok(api::fix(&fix, kind)?))
}

pub(super) fn status(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || Ok(api::status(&Args::from_json(args))?))
}

pub(super) fn diff(host: &dyn Host, args: &Value) -> Outcome {
    let diff = Args::from_json(args);
    with_paths(host, || {
        if diff.name.is_empty() {
            if diff.file.is_some() {
                return Err(ToolError::new(
                    "invalid_arguments",
                    "file needs a skill name",
                ));
            }
            return Ok(api::diff(&diff)?);
        }
        Ok(compare(&diff, diff.file.as_deref())?)
    })
}

pub(super) fn keep(host: &dyn Host, args: &Value) -> Outcome {
    let mut keep = Args::from_json(args);
    keep.dry_run = flag_or(args, "dry_run", true);
    let files = crate::args::nonempty_strings(args, "files").unwrap_or_default();
    with_paths(host, || Ok(keep_files(&keep, &files, keep.dry_run)?))
}

pub(super) fn verify(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || Ok(api::verify(&Args::from_json(args))?))
}

pub(super) fn undo(host: &dyn Host, args: &Value) -> Outcome {
    let mut undo = Args::from_json(args);
    undo.dry_run = flag_or(args, "dry_run", true);
    with_paths(host, || Ok(api::undo(&undo, Some(host))?))
}

pub(super) fn auto_status(host: &dyn Host, _args: &Value) -> Outcome {
    with_paths(host, || Ok(auto::full_status_op(host)?))
}

fn auto_switch(host: &dyn Host, args: &Value, enabled: bool) -> Outcome {
    with_paths(host, || {
        let data = approval::set_enabled(auto::TASK_ID, enabled, flag(args, "dry_run"), host)
            .map_err(refused)?;
        Ok(auto::finish_switch(host, data, enabled)?)
    })
}

pub(super) fn auto_on(host: &dyn Host, args: &Value) -> Outcome {
    auto_switch(host, args, true)
}

pub(super) fn auto_off(host: &dyn Host, args: &Value) -> Outcome {
    auto_switch(host, args, false)
}

pub(super) fn auto_pause(host: &dyn Host, _args: &Value) -> Outcome {
    with_paths(host, || Ok(auto::pause_op(&SystemClock)?))
}

pub(super) fn auto_resume(host: &dyn Host, _args: &Value) -> Outcome {
    with_paths(host, || Ok(auto::resume_op()?))
}

pub(super) fn sync(host: &dyn Host, args: &Value) -> Outcome {
    let mut sync = Args::from_json(args);
    sync.dry_run = flag_or(args, "dry_run", true);
    with_paths(host, || Ok(api::sync(&sync, Some(host))?))
}

pub(super) fn list_transpilers(_host: &dyn Host, _args: &Value) -> Outcome {
    Ok(api::transpilers())
}

pub(super) fn audit(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || {
        repo_handlers::audit_handler(Value::Object(request(args)?)).map_err(failed)
    })
}

pub(super) fn scaffold(host: &dyn Host, args: &Value) -> Outcome {
    let name = kebab(str_arg(args, "name").unwrap_or_default())?;
    let skill_type = str_arg(args, "skill_type").unwrap_or("skill");
    if !["skill", "rule"].contains(&skill_type) {
        return Err(ToolError::new(
            "invalid_arguments",
            "skill_type must be skill or rule",
        ));
    }
    let repo = with_paths(host, || repo_of(args))?;
    let path = repo
        .join(skill_bucket(skill_type))
        .join(name)
        .join("SKILL.md");
    if path.exists() {
        return Err(ToolError::new(
            "conflict",
            format!("{skill_type} {name} already exists at {}", path.display()),
        ));
    }
    crate::skills::pyfs::write_text(&path, &skill_template(name, skill_type))
        .map_err(|e| ToolError::new("backend_error", e))?;
    Ok(json!({"created_path": path.to_string_lossy(), "kind": skill_type}))
}

pub(super) fn init(host: &dyn Host, args: &Value) -> Outcome {
    let name = str_nonempty(args, "name").map(path_safe).transpose()?;
    let dry_run = flag_or(args, "dry_run", true);
    with_paths(host, || {
        let path = super::paths::under_home(str_arg(args, "path").unwrap_or_default())?;
        let report = crate::skills::repo::init_repo(&path, name, dry_run).map_err(failed)?;
        Ok(json!({
            "repo": report.repo.to_string_lossy(),
            "name": report.name,
            "alreadyExists": report.already_exists,
            "created": report.created,
            "dryRun": dry_run,
        }))
    })
}

pub(super) fn clean(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || typed(state::clean(scoped_request(args, &[])?)))
}

fn typed(result: Result<Value, crate::op::OpError>) -> Outcome {
    result.map_err(|error| match error.kind {
        ErrorKind::Usage => ToolError::from(error),
        _ => failed(error.message),
    })
}

pub(super) fn uninstall(host: &dyn Host, args: &Value) -> Outcome {
    let name = path_safe(str_arg(args, "name").unwrap_or_default())?;
    with_paths(host, || {
        scoped(args, state::uninstall_handler, &[("name", json!(name))])
    })
}

pub(super) fn resolve(host: &dyn Host, args: &Value) -> Outcome {
    let migrate: Vec<(&str, Value)> = args
        .get("migrate")
        .and_then(Value::as_bool)
        .map(|on| ("migrate", json!(on)))
        .into_iter()
        .collect();
    with_paths(host, || {
        typed(state::resolve(scoped_request(args, &migrate)?))
    })
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

pub(super) fn bundle(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || bundle_in(args))
}

fn bundle_in(args: &Value) -> Outcome {
    let mut map = request(args)?;
    if let Some(output) = str_nonempty(args, "output") {
        map.insert("output".into(), json!(output));
    }
    if let Some(names) = crate::args::list(args, "skills") {
        map.insert("skills".into(), Value::Array(names.clone()));
    }
    map.insert("dry_run".into(), json!(true));
    let planned = repo_handlers::bundle_handler(Value::Object(map.clone())).map_err(failed)?;
    check_bundle_target(&planned)?;
    if flag_or(args, "dry_run", true) {
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
pub(super) fn unbundle(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || unbundle_in(args))
}

fn unbundle_in(args: &Value) -> Outcome {
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
    if flag_or(args, "dry_run", true) {
        return Ok(planned);
    }
    map.insert("dry_run".into(), json!(false));
    repo_handlers::unbundle_handler(Value::Object(map)).map_err(failed)
}

fn tapped(outcome: Result<Value, crate::skills::tap_ops::TapError>) -> Outcome {
    use crate::skills::tap_ops::Kind;
    outcome.map_err(|e| {
        let kind = match e.kind {
            Kind::Invalid => "invalid_arguments",
            Kind::NotFound => "not_found",
            Kind::Conflict => "conflict",
            Kind::Backend => "backend_error",
        };
        ToolError::new(kind, e.message)
    })
}

fn applies(args: &Value) -> bool {
    !flag_or(args, "dry_run", true)
}

pub(super) fn tap_list(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || tapped(taps::tap_list_value(host, args)))
}

pub(super) fn search(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || tapped(taps::search_value(args)))
}

pub(super) fn tap_add(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || {
        tapped(taps::tap_add_value(&json!({
            "repo": str_arg(args, "repo"),
            "name": str_arg(args, "name"),
            "dry_run": !applies(args),
        })))
    })
}

pub(super) fn tap_remove(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || {
        tapped(taps::tap_remove_value(
            host,
            &json!({
                "name": str_arg(args, "name"),
                "dry_run": !applies(args),
            }),
        ))
    })
}

pub(super) fn tap_update(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || {
        tapped(taps::tap_update_value(
            host,
            &json!({
                "name": str_arg(args, "name"),
                "dry_run": !applies(args),
            }),
        ))
    })
}

pub(super) fn tap_link(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || {
        tapped(taps::tap_link_value(
            host,
            &json!({
                "name": str_arg(args, "name"),
                "plugins": args.get("plugins").cloned().unwrap_or(Value::Null),
                "dry_run": !applies(args),
            }),
        ))
    })
}

pub(super) fn tap_unlink(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || {
        tapped(taps::tap_unlink_value(
            host,
            &json!({
                "name": str_arg(args, "name"),
                "dry_run": !applies(args),
            }),
        ))
    })
}

/// Installs into the discovered skills repository, never into the working directory, and always
/// with the audit on.
pub(super) fn install(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || {
        let repo = repo_of(args)?;
        tapped(taps::install_value(&json!({
            "spec": str_arg(args, "spec"),
            "repo_path": repo.to_string_lossy(),
            "dry_run": !applies(args),
            "no_audit": false,
        })))
    })
}

pub(super) fn agents_audit(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || read(args, agents::audit_handler))
}

pub(super) fn agents_diff(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || read(args, agents::diff_handler))
}

pub(super) fn agents_status(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || read(args, agents::status_handler))
}

pub(super) fn styles_diff(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || read(args, styles::diff_handler))
}

pub(super) fn styles_status(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || read(args, styles::status_handler))
}

pub(super) fn agents_clean(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || scoped(args, agents::clean_handler, &[]))
}

pub(super) fn agents_uninstall(host: &dyn Host, args: &Value) -> Outcome {
    let name = path_safe(str_arg(args, "name").unwrap_or_default())?;
    with_paths(host, || {
        scoped(args, agents::uninstall_handler, &[("name", json!(name))])
    })
}

pub(super) fn styles_clean(host: &dyn Host, args: &Value) -> Outcome {
    with_paths(host, || scoped(args, styles::clean_handler, &[]))
}

fn read(args: &Value, handler: Handler) -> Outcome {
    handler(Value::Object(request(args)?)).map_err(failed)
}
