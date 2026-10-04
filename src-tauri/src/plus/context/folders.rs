//! Folder profiles preview: which gateway profile applies per reported root, the matching rule
//! and the token cost of what loads there, plus the `plus.folderProfiles.enabled` switch.

use super::{config_from_args, loads, roots_from_args};
use crate::registry;
use serde_json::{json, Value};
use std::path::PathBuf;

fn requested_roots(args: &Value, home: &PathBuf) -> Vec<String> {
    let mut out: Vec<String> = args
        .get("roots")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default();
    if let Some(cwd) = args.get("cwd").and_then(Value::as_str) {
        out.push(cwd.to_string());
    }
    if out.is_empty() {
        out.push(home.display().to_string());
    }
    out
}

/// `plus.context.folderProfiles`: takes the `whatLoads` root overrides plus `cwd` and/or `roots`
/// and returns the enabled flag and, per root, the applying profile, the matching rule, the
/// launch-profile token cost and the registry mappings. Read-only.
pub fn folder_profiles_handler(args: Value) -> Result<Value, String> {
    let roots = roots_from_args(&args)?;
    let config = config_from_args(&roots, &args)?;
    let reg = registry::load()?;
    let mut folders = Vec::new();
    for root in requested_roots(&args, &roots.home) {
        let rule = reg.matching_folder_profile(&root);
        let applies = reg.folder_profiles_enabled && rule.is_some();
        let profile_id = rule.map(|fp| reg.resolve_profile_id(&fp.profile));
        let profile_name = profile_id.as_ref().map(|id| {
            reg.profiles
                .iter()
                .find(|p| &p.id == id)
                .map_or_else(|| id.clone(), |p| p.name.clone())
        });
        let launch = rule.and_then(|fp| {
            [Some(fp.profile.as_str()), profile_name.as_deref()]
                .into_iter()
                .flatten()
                .find(|k| config.profiles.contains_key(*k))
                .map(String::from)
        });
        let only_loaded = loads::LoadsOptions {
            no_lazy: true,
            ..Default::default()
        };
        let cost = loads::what_loads_with(
            &roots,
            &config,
            launch.as_deref(),
            &PathBuf::from(&root),
            &only_loaded,
        )?;
        let reason = match (rule, reg.folder_profiles_enabled) {
            (None, _) => "no mapping matches this folder".to_string(),
            (Some(fp), true) => format!("longest matching mapping: {}", fp.path),
            (Some(fp), false) => format!("mapping {} matches but folder profiles are disabled", fp.path),
        };
        folders.push(json!({
            "root": root,
            "applies": applies,
            "profile": if applies { profile_name.clone() } else { None },
            "wouldApply": profile_name,
            "rule": rule.map(|fp| fp.path.clone()),
            "reason": reason,
            "launchProfile": launch,
            "tokens": cost.total_tokens,
        }));
    }
    Ok(json!({
        "enabled": reg.folder_profiles_enabled,
        "mappings": reg.folder_profiles.iter().map(|fp| json!({"path": fp.path, "profile": fp.profile})).collect::<Vec<_>>(),
        "folders": folders,
    }))
}

/// `plus.context.folderProfilesSet`: takes `enabled` (bool) and persists the switch.
pub fn folder_profiles_set_handler(args: Value) -> Result<Value, String> {
    let enabled = args
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or("enabled (bool) is required")?;
    registry::update(|r| {
        r.folder_profiles_enabled = enabled;
        Ok(())
    })?;
    Ok(json!({"enabled": enabled}))
}
