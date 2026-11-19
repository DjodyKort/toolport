//! `context layers ls` and the rules half of `context sync`: the layers of the skills repository
//! with where each lands and in what state. Global and glob layers are rules that `skills sync`
//! writes to `~/.claude/rules`, so their state is that sync's own plan; folder layers are judged
//! by their blocks (`layer_deploy`).

use super::fsx;
use super::layer_defs::{self, LayerDef, Scope};
use super::layer_deploy::{self, env, Env};
use crate::op::OpError;
use crate::skills::api::{self as skills, Args};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use toolport_plus_api::Host;

const CLIENT: &str = "claude-code";

fn rule_args(env: &Env, names: Vec<String>, dry_run: bool) -> Args {
    Args {
        repo: Some(env.roots.skills_repo_path()),
        dry_run,
        global: true,
        clients: Some(vec![CLIENT.into()]),
        only: Some(names),
        ..Args::default()
    }
}

fn rule_names(defs: &[LayerDef]) -> Vec<String> {
    defs.iter()
        .filter(|layer| layer.scope != Scope::Folder)
        .map(|layer| layer.name.clone())
        .collect()
}

/// `skills sync`'s plan for the given rules: per rule, the ops that are not "unchanged".
fn rule_ops(env: &Env, names: Vec<String>) -> Result<BTreeMap<String, Vec<String>>, String> {
    if names.is_empty() {
        return Ok(BTreeMap::new());
    }
    let data = skills::sync(&rule_args(env, names, true), None).map_err(|e| e.message)?;
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for change in data["plan"]["changes"].as_array().into_iter().flatten() {
        if change["kind"].as_str().is_some_and(|k| k != "skill") {
            continue;
        }
        if let (Some(name), Some(op)) = (change["skill"].as_str(), change["op"].as_str()) {
            out.entry(name.to_string())
                .or_default()
                .push(op.to_string());
        }
    }
    for failure in data["plan"]["failed"].as_array().into_iter().flatten() {
        if let Some(name) = failure["name"].as_str() {
            out.entry(name.to_string())
                .or_default()
                .push("failed".into());
        }
    }
    Ok(out)
}

fn state_of_rule(ops: &[String]) -> (&'static str, &'static str) {
    let has = |op: &str| ops.iter().any(|o| o == op);
    if has("edited") {
        ("edited", "A file at the destination was changed by hand since the last deploy. A sync would replace it and keep a backup.")
    } else if has("differs") {
        ("shadowed", "A file at the destination was not written by Toolport. A sync would replace it and keep a backup.")
    } else if has("failed") {
        ("problem", "The last attempt to write it failed.")
    } else if has("add") {
        ("not-deployed", "Not written to the destination yet.")
    } else if has("update") || has("remove") || has("hook") {
        (
            "outdated",
            "The library copy changed; a sync updates the destination.",
        )
    } else {
        ("deployed", "")
    }
}

fn row_of(
    host: &dyn Host,
    env: &Env,
    layer: &LayerDef,
    rules: &Result<BTreeMap<String, Vec<String>>, String>,
) -> Value {
    let tokens = host.count_tokens(layer.body.trim());
    let mut row = json!({
        "name": layer.name,
        "path": fsx::display(&layer.path),
        "scope": layer.scope.as_str(),
        "delivery": layer.delivery.as_str(),
        "globs": layer.globs,
        "folderPatterns": layer.folders,
        "imports": layer.imports,
        "tokens": { "value": tokens, "basis": "estimate" },
        "problems": layer.problems,
    });
    if layer.scope == Scope::Folder {
        let landing = layer_deploy::landing(env, layer);
        let (state, detail) = if let Some(problem) = &landing.problem {
            ("problem", problem.clone())
        } else if landing.folders.is_empty() {
            (
                "nowhere",
                "No folder matches this layer's folders, so it is deployed nowhere. Fix the pattern or delete the layer.".to_string(),
            )
        } else if landing.edited > 0 {
            (
                "edited",
                "A block at the destination was changed by hand since the last deploy. A sync would replace it and keep a backup.".to_string(),
            )
        } else if landing.missing > 0 {
            (
                "not-deployed",
                "Not written to every folder yet.".to_string(),
            )
        } else if landing.outdated > 0 {
            (
                "outdated",
                "The layer changed; a sync updates the blocks.".to_string(),
            )
        } else {
            ("deployed", String::new())
        };
        row["state"] = json!(state);
        row["detail"] = json!(detail);
        row["target"] = json!({
            "kind": "folders",
            "folders": landing.folders,
            "deployed": landing.deployed,
            "file": "CLAUDE.local.md",
        });
        return row;
    }
    let file = crate::fsx::claude_dir_in(&env.home)
        .join("rules")
        .join(format!("{}.md", layer.name));
    row["target"] = json!({ "kind": "rules", "file": fsx::display(&file) });
    match rules {
        Ok(ops) => {
            let (state, detail) =
                state_of_rule(ops.get(&layer.name).map_or(&[][..], Vec::as_slice));
            row["state"] = json!(state);
            row["detail"] = json!(detail);
        }
        Err(why) => {
            row["state"] = json!("problem");
            row["detail"] = json!(why);
        }
    }
    row
}

pub fn ls(host: &dyn Host) -> Result<Value, OpError> {
    let env = env(host)?;
    let defs = layer_defs::list(&env.roots);
    let rules = rule_ops(&env, rule_names(&defs));
    let rows: Vec<Value> = defs
        .iter()
        .map(|layer| row_of(host, &env, layer, &rules))
        .collect();
    Ok(json!({
        "layers": rows,
        "library": fsx::display(&env.roots.rules_dir()),
    }))
}

/// The rules half of `context sync --rules`: `skills sync` for the global and glob layers only,
/// through the same engine and with the same holds for files that need a decision.
pub fn deploy_rules(host: &dyn Host, dry_run: bool) -> Result<Value, OpError> {
    let env = env(host)?;
    let defs = layer_defs::list(&env.roots);
    let names = rule_names(&defs);
    if names.is_empty() {
        return Ok(json!({ "rows": [], "held": [], "upToDate": 0 }));
    }
    let data = skills::sync(&rule_args(&env, names, dry_run), Some(host))?;
    let changes: Vec<Value> = data["plan"]["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["kind"].as_str().is_none_or(|k| k == "skill"))
        .map(|c| {
            json!({
                "name": c["skill"], "path": c["path"], "op": c["op"], "held": c["held"],
            })
        })
        .collect();
    Ok(json!({
        "rows": changes,
        "held": data["held"],
        "upToDate": data["plan"]["counts"]["unchanged"],
        "snapshot": data["snapshot"],
    }))
}
