//! Scaffolding and management behind `toolportctl context init|status|client|profile|disable`.
//! The shared core of the ctl commands and the `plus.context.*` handlers: every handler takes the
//! root overrides of `plus.context.plan` plus `dryRun`, and a dry run reads and writes nothing
//! outside what it reports. Behavior and texts follow mcpm's `context` group.

use super::config::{ContextConfig, ProfileSpec};
use super::layers::{self, Layer};
use super::{
    apply, dedupe, launch, load_config, lock_real_run, preserve_unreadable, roots_from_args,
    save_config, settings, shims, ApplyOptions, Report, Roots,
};
use crate::plus::args::{flag, flag_or, str_arg, str_nonempty};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Component, Path};

/// What mcpm's `context init` tells the user to do next: label, command.
pub const NEXT_STEPS: [(&str, &str); 4] = [
    ("Edit your personal layer, then:", "toolportctl skills sync"),
    ("Per-client layer:", "toolportctl context client add <name>"),
    (
        "Profiles:",
        "toolportctl context profile add bare --no-org --rules none --servers none",
    ),
    ("Apply everything:", "toolportctl context sync"),
];

fn dry_run(args: &Value) -> bool {
    flag(args, "dryRun")
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    str_arg(args, key).ok_or_else(|| format!("{key} is required"))
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

fn report_value(report: &Report) -> (Value, Value) {
    (json!(report.actions), json!(report.warnings))
}

/// The part of a permission entry that is safe to show: its secret-looking tail is cut at the
/// first `=`, where a credential assignment starts.
pub fn redact_entry(entry: &str) -> String {
    match entry.find('=') {
        Some(at) => format!("{}…", &entry[..=at]),
        None => entry.to_string(),
    }
}

struct SettingsLocal {
    found: usize,
    clean: Vec<String>,
    credentials: Vec<String>,
}

enum LocalSettings {
    Absent,
    Unparseable,
    Entries(SettingsLocal),
}

fn read_settings_local(path: &Path, config: &ContextConfig) -> LocalSettings {
    if !path.exists() {
        return LocalSettings::Absent;
    }
    let Some(doc) = crate::plus::jsonfs::read_json::<Value>(path) else {
        return LocalSettings::Unparseable;
    };
    let allow: Vec<String> = doc["permissions"]["allow"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e.as_str().map(String::from))
        .collect();
    if allow.is_empty() {
        return LocalSettings::Absent;
    }
    let (secret, rest): (Vec<String>, Vec<String>) = allow
        .iter()
        .cloned()
        .partition(|e| settings::looks_secret_bearing(e));
    let clean = rest
        .into_iter()
        .filter(|e| !config.settings.ensure_allow.contains(e))
        .collect();
    LocalSettings::Entries(SettingsLocal {
        found: allow.len(),
        clean,
        credentials: secret.iter().map(|e| redact_entry(e)).collect(),
    })
}

/// `plus.context.init`: scaffolds the personal layer, moves the allow entries of the unsupported
/// user-level `settings.local.json` into `ensure_allow` (never removing the file, never moving an
/// entry that embeds a credential) and saves `context.json`.
pub fn init_handler(args: Value) -> Result<Value, String> {
    let roots = roots_from_args(&args)?;
    let dry = dry_run(&args);
    let _lock = lock_real_run(&roots, dry)?;
    let mut config = load_config(&roots.context_config_path());
    let personal = layers::personal_rule_path(&roots);
    let created = !personal.exists();
    if created && !dry {
        layers::scaffold_personal_rule(&roots)?;
    }
    let local = roots.claude_home.join("settings.local.json");
    let migration = match read_settings_local(&local, &config) {
        LocalSettings::Absent => Value::Null,
        LocalSettings::Unparseable => {
            json!({"path": path_text(&local), "unparseable": true})
        }
        LocalSettings::Entries(found) => {
            config
                .settings
                .ensure_allow
                .extend(found.clean.iter().cloned());
            json!({
                "path": path_text(&local),
                "unparseable": false,
                "found": found.found,
                "migratable": found.clean.len(),
                "migrated": found.clean.len(),
                "credentials": found.credentials,
            })
        }
    };
    let config_path = roots.context_config_path();
    let mut kept = Value::Null;
    if !dry {
        if let Some(copy) = preserve_unreadable(&config_path) {
            kept = json!(path_text(&copy));
        }
        save_config(&config_path, &config)?;
    }
    Ok(json!({
        "dryRun": dry,
        "personal": {"path": path_text(&personal), "created": created},
        "migration": migration,
        "config": {"path": path_text(&config_path), "saved": !dry, "keptUnreadable": kept},
        "nextSteps": NEXT_STEPS.iter().map(|(label, command)| json!({"label": label, "command": command})).collect::<Vec<_>>(),
    }))
}

fn layer_value(layer: &Layer) -> Value {
    json!({
        "name": layer.name,
        "path": path_text(&layer.path),
        "globs": layer.globs,
        "description": layer.description,
    })
}

/// `plus.context.clientAdd`: scaffolds `rules/client-<slug>/SKILL.md` with a path glob. Names the
/// skills pipeline would reject or that cannot sit in the frontmatter are refused.
pub fn client_add_handler(args: Value) -> Result<Value, String> {
    let roots = roots_from_args(&args)?;
    let name = required(&args, "name")?;
    if name.trim().is_empty() {
        return Err("client name must not be empty".into());
    }
    let glob = str_nonempty(&args, "glob");
    let rule = layers::plan_client_rule(&roots, name, glob)?;
    let dry = dry_run(&args);
    let created = !rule.path.exists();
    if created && !dry {
        layers::scaffold_client_rule(&roots, name, glob)?;
    }
    Ok(json!({
        "dryRun": dry,
        "name": name,
        "rule": format!("client-{}", layers::slug(name)),
        "path": path_text(&rule.path),
        "glob": rule.glob,
        "created": created,
    }))
}

/// `plus.context.clientList`: every context layer with its path globs (always on without any).
pub fn client_list_handler(args: Value) -> Result<Value, String> {
    let roots = roots_from_args(&args)?;
    let layers: Vec<Value> = layers::list_layers(&roots)
        .iter()
        .map(layer_value)
        .collect();
    Ok(json!({"layers": layers}))
}

/// Profile names become shell function names and directory names.
pub fn check_profile_name(name: &str) -> Result<(), String> {
    let mut config = ContextConfig::default();
    config
        .profiles
        .insert(name.to_string(), ProfileSpec::default());
    config.validate()
}

fn selection(value: Option<&Value>, field: &str) -> Result<Value, String> {
    match value {
        None | Some(Value::Null) => Ok(json!("inherit")),
        Some(Value::String(text)) if text == "inherit" || text == "none" => Ok(json!(text)),
        Some(Value::String(text)) => Ok(json!(text
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>())),
        Some(Value::Array(items)) if items.iter().all(Value::is_string) => {
            Ok(Value::Array(items.clone()))
        }
        Some(_) => Err(format!(
            "{field} must be \"inherit\", \"none\" or a list of names"
        )),
    }
}

fn profile_row(roots: &Roots, name: &str, spec: &ProfileSpec) -> Value {
    json!({
        "name": name,
        "shim": format!("claude-{name}"),
        "org": spec.org,
        "orgMode": spec.org_mode,
        "rules": spec.rules,
        "servers": spec.servers,
        "generated": launch::profile_dir(roots, name).is_dir(),
        "dir": path_text(&launch::profile_dir(roots, name)),
    })
}

/// `plus.context.profileAdd`: defines or replaces a profile and applies the whole config, like
/// mcpm. Fields the command has no option for (settings overrides, compaction, auth copy, ...)
/// survive a redefinition; mcpm resets them to defaults.
pub fn profile_add_handler(args: Value) -> Result<Value, String> {
    let roots = roots_from_args(&args)?;
    let name = required(&args, "name")?;
    check_profile_name(name)?;
    let org_mode = str_arg(&args, "orgMode").unwrap_or("import");
    if !["import", "copy"].contains(&org_mode) {
        return Err(format!(
            "orgMode must be \"import\" or \"copy\", not {org_mode:?}"
        ));
    }
    let rules = selection(args.get("rules"), "rules")?;
    let servers = selection(args.get("servers"), "servers")?;
    let dry = dry_run(&args);
    let _lock = lock_real_run(&roots, dry)?;
    let mut config = load_config(&roots.context_config_path());
    let existing = config.profiles.get(name).cloned();
    let mut spec = existing.clone().unwrap_or_default();
    spec.org = flag_or(&args, "org", true);
    spec.org_mode = org_mode.to_string();
    spec.rules = rules;
    spec.servers = servers;
    spec.commands = flag_or(&args, "commands", true);
    spec.skills = flag_or(&args, "skills", true);
    config.profiles.insert(name.to_string(), spec.clone());
    let report = apply(
        &roots,
        &mut config,
        ApplyOptions {
            persist: !dry,
            dry_run: dry,
        },
    )?;
    let (actions, warnings) = report_value(&report);
    let mut profile = profile_row(&roots, name, &spec);
    profile["created"] = json!(existing.is_none());
    Ok(json!({"dryRun": dry, "profile": profile, "actions": actions, "warnings": warnings}))
}

/// `plus.context.profileList`: the configured profiles, sorted by name.
pub fn profile_list_handler(args: Value) -> Result<Value, String> {
    let roots = roots_from_args(&args)?;
    let config = load_config(&roots.context_config_path());
    let profiles: Vec<Value> = config
        .profiles
        .iter()
        .map(|(name, spec)| profile_row(&roots, name, spec))
        .collect();
    Ok(json!({"profiles": profiles}))
}

/// A profile directory name that stays inside the profiles root; orphan directories may carry
/// names a profile cannot have, so this is looser than [`check_profile_name`].
pub fn check_profile_dir_name(name: &str) -> Result<(), String> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(only)), None) if only == name => Ok(()),
        _ => Err(format!(
            "profile name {name:?} is not a directory name under the profiles root"
        )),
    }
}

fn remove_profile_dir(dir: &Path, dry: bool, report: &mut Report) -> Result<bool, String> {
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return Ok(false);
    };
    let kind = meta.file_type();
    if !kind.is_dir() && !kind.is_symlink() {
        return Ok(false);
    }
    if !dry {
        let removed = if kind.is_symlink() {
            fs::remove_file(dir)
        } else {
            fs::remove_dir_all(dir)
        };
        removed.map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let verb = if dry { "would remove" } else { "removed" };
    report.add(format!("{verb} profile dir {}", dir.display()));
    Ok(true)
}

/// `plus.context.profileRemove`: drops a profile from the config and applies it; `purge` also
/// deletes the generated directory, which also clears an orphan that is not in the config.
pub fn profile_remove_handler(args: Value) -> Result<Value, String> {
    let roots = roots_from_args(&args)?;
    let name = required(&args, "name")?;
    let purge = flag(&args, "purge");
    if purge {
        check_profile_dir_name(name)?;
    }
    let dry = dry_run(&args);
    let _lock = lock_real_run(&roots, dry)?;
    let mut config = load_config(&roots.context_config_path());
    let in_config = config.profiles.remove(name).is_some();
    let dir = launch::profile_dir(&roots, name);
    let mut report = Report::default();
    let purged = purge && remove_profile_dir(&dir, dry, &mut report)?;
    let applied = apply(
        &roots,
        &mut config,
        ApplyOptions {
            persist: !dry,
            dry_run: dry,
        },
    )?;
    report.actions.extend(applied.actions);
    let orphan = format!("orphan profile dir {} ", dir.display());
    report.warnings.extend(
        applied
            .warnings
            .into_iter()
            .filter(|w| !(dry && purged && w.starts_with(&orphan))),
    );
    let (actions, warnings) = report_value(&report);
    Ok(json!({
        "dryRun": dry,
        "name": name,
        "inConfig": in_config,
        "purged": purged,
        "actions": actions,
        "warnings": warnings,
    }))
}

/// `plus.context.status`: layers, profiles, legacy MCP duplicates and the shims file.
pub fn status_handler(args: Value) -> Result<Value, String> {
    let roots = roots_from_args(&args)?;
    let config = load_config(&roots.context_config_path());
    let layers: Vec<Value> = layers::list_layers(&roots)
        .iter()
        .map(layer_value)
        .collect();
    let profiles: Vec<Value> = config
        .profiles
        .iter()
        .map(|(name, spec)| profile_row(&roots, name, spec))
        .collect();
    let dupes = dedupe::plan_dedupe(&roots.claude_json, &config.dedupe);
    let shims_path = roots.shims_path();
    Ok(json!({
        "layers": layers,
        "profiles": profiles,
        "legacyDupes": dupes,
        "shims": {"path": path_text(&shims_path), "exists": shims_path.exists()},
        "config": {
            "path": path_text(&roots.context_config_path()),
            "exists": roots.context_config_path().is_file(),
        },
    }))
}

/// `plus.context.disable`: removes the generated shims file and, with `purgeProfiles`, every
/// generated profile directory. The config, the layer rules and `~/.claude` stay as they are.
pub fn disable_handler(args: Value) -> Result<Value, String> {
    let roots = roots_from_args(&args)?;
    let dry = dry_run(&args);
    let _lock = lock_real_run(&roots, dry)?;
    let mut report = Report::default();
    let shims_removed = shims::remove_shims(&roots, dry)?;
    if shims_removed {
        let verb = if dry { "would remove" } else { "removed" };
        report.add(format!("{verb} shims file"));
    }
    let mut purged: Vec<String> = Vec::new();
    if flag(&args, "purgeProfiles") {
        let mut dirs: Vec<_> = fs::read_dir(roots.profiles_root())
            .map(|read| read.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        dirs.sort();
        for dir in dirs.iter().filter(|d| d.is_dir()) {
            if remove_profile_dir(dir, dry, &mut report)? {
                purged.push(path_text(dir));
            }
        }
    }
    let (actions, warnings) = report_value(&report);
    let mut out = Map::new();
    out.insert("dryRun".into(), json!(dry));
    out.insert(
        "shims".into(),
        json!({"path": path_text(&roots.shims_path()), "removed": shims_removed}),
    );
    out.insert("purgedProfiles".into(), json!(purged));
    out.insert("actions".into(), actions);
    out.insert("warnings".into(), warnings);
    Ok(Value::Object(out))
}
