//! Settings composition that survives corp-dev-tools' `jq -s '.[0] * .[1]'` merge.
//!
//! jq replaces arrays wholesale, so personal `permissions.allow` entries vanish on every org sync.
//! `ensure_policy` re-unions the policy-declared entries (additive only; team entries stay).

use super::backup::snapshot;
use super::config::SettingsPolicy;
use super::roots::Roots;
use crate::plus::skills::json::{parse, J};
use std::fs;
use std::path::Path;

/// Recursive dict merge; lists become an ordered union (base first); scalars from `over` win.
pub fn deep_merge_union(base: &J, over: &J) -> J {
    let (J::Obj(base_items), J::Obj(over_items)) = (base, over) else {
        return over.clone();
    };
    let mut result = base_items.clone();
    for (key, value) in over_items {
        match result.iter_mut().find(|(k, _)| k == key) {
            Some((_, existing)) => {
                *existing = match (&*existing, value) {
                    (J::Obj(_), J::Obj(_)) => deep_merge_union(existing, value),
                    (J::Arr(have), J::Arr(add)) => {
                        let mut merged = have.clone();
                        for item in add {
                            if !merged.contains(item) {
                                merged.push(item.clone());
                            }
                        }
                        J::Arr(merged)
                    }
                    _ => value.clone(),
                };
            }
            None => result.push((key.clone(), value.clone())),
        }
    }
    J::Obj(result)
}

const SECRETISH_MARKERS: [&str; 6] = ["SECRET", "TOKEN", "PASSWORD", "APIKEY", "API_KEY", "BEARER"];

/// Permission strings embedding credential assignments. mcpm never calls this from
/// `ensure_policy` (recorded quirk), so it is exposed for callers that want the guard.
pub fn looks_secret_bearing(entry: &str) -> bool {
    let upper = entry.to_uppercase();
    entry.contains('=') && SECRETISH_MARKERS.iter().any(|m| upper.contains(m))
}

#[derive(Debug, PartialEq, Eq)]
pub enum PolicyOutcome {
    Unchanged,
    Changed,
    Skipped(String),
}

fn obj_get_mut<'a>(items: &'a mut Vec<(String, J)>, key: &str) -> Option<&'a mut J> {
    items.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Unions `ensure_allow`/`ensure_ask` into `permissions.*` of `settings_path`.
///
/// A file that does not parse is never "fixed" (doctor surfaces it), matching mcpm. Non-object
/// settings or non-list `allow`/`ask` that would need entries are skipped with a reason instead
/// of raising like mcpm does.
pub fn ensure_policy(
    roots: &Roots,
    settings_path: &Path,
    policy: &SettingsPolicy,
    backup: bool,
    dry_run: bool,
) -> Result<PolicyOutcome, String> {
    let wanted = [("allow", &policy.ensure_allow), ("ask", &policy.ensure_ask)];
    if wanted.iter().all(|(_, entries)| entries.is_empty()) {
        return Ok(PolicyOutcome::Unchanged);
    }
    let mut data = if settings_path.exists() {
        let text = fs::read_to_string(settings_path)
            .map_err(|e| format!("{}: {e}", settings_path.display()))?;
        match parse(&text) {
            Ok(v) => v,
            Err(_) => return Ok(PolicyOutcome::Unchanged),
        }
    } else {
        J::Obj(Vec::new())
    };
    let J::Obj(root) = &mut data else {
        return Ok(PolicyOutcome::Skipped(
            "settings.json is not a JSON object".into(),
        ));
    };
    if obj_get_mut(root, "permissions").is_none() {
        root.push(("permissions".into(), J::Obj(Vec::new())));
    }
    let Some(J::Obj(perms)) = obj_get_mut(root, "permissions") else {
        return Ok(PolicyOutcome::Skipped(
            "permissions is not an object".into(),
        ));
    };
    for (key, entries) in &wanted {
        if !entries.is_empty()
            && matches!(obj_get_mut(perms, key), Some(v) if !matches!(v, J::Arr(_)))
        {
            return Ok(PolicyOutcome::Skipped(format!(
                "permissions.{key} is not a list; left alone"
            )));
        }
    }
    let mut changed = false;
    for (key, entries) in &wanted {
        if entries.is_empty() {
            continue;
        }
        if obj_get_mut(perms, key).is_none() {
            perms.push(((*key).into(), J::Arr(Vec::new())));
        }
        let Some(J::Arr(current)) = obj_get_mut(perms, key) else {
            continue;
        };
        for entry in entries.iter() {
            let item = J::Str(entry.clone());
            if !current.contains(&item) {
                current.push(item);
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(PolicyOutcome::Unchanged);
    }
    if dry_run {
        return Ok(PolicyOutcome::Changed);
    }
    if backup {
        snapshot(roots, settings_path)?;
    }
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(settings_path, format!("{}\n", data.dumps()))
        .map_err(|e| format!("{}: {e}", settings_path.display()))?;
    Ok(PolicyOutcome::Changed)
}
