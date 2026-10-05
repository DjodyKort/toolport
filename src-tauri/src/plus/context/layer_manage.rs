//! `context client add|edit|rm|list` (MIG-CTX-11): the layers of `rules/client-*` with the scope,
//! imports and delivery of `layer_spec`. A change writes the canonical rule in the skills
//! repository and refreshes the `CLAUDE.local.md` of the folders the layer is delivered into, and
//! says so in a plan first. Global and glob layers reach `~/.claude/rules` through the skills
//! pipeline (`skills sync`, `context sync --rules`), as before.

use super::bundle::Issue;
use super::config::{load_config, ContextConfig};
use super::layer_spec::{self, Delivery, Scope, SpecEdit};
use super::layers::{self, FolderEffect, Layer};
use super::{roots_from_args, Report, Roots};
use crate::plus::args::{flag, str_nonempty};
use crate::plus::op::OpError;
use crate::plus::skills::pyfs::write_text;
use crate::plus::sources::fsx;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

struct World {
    roots: Roots,
    config: ContextConfig,
    clients_root: PathBuf,
}

fn world(args: &Value) -> Result<World, OpError> {
    let roots = roots_from_args(args).map_err(|e| OpError::failed("context_invalid", e))?;
    let config = load_config(&roots.context_config_path());
    let roots = roots.resolved(&config);
    let clients_root = roots.resolve_clients_root(&config);
    Ok(World { roots, config, clients_root })
}

fn strings(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    })
}

fn edit_of(args: &Value) -> Result<SpecEdit, OpError> {
    let scope = match str_nonempty(args, "scope") {
        None => None,
        Some(s) => Some(Scope::parse(s).ok_or_else(|| OpError::usage(format!("--scope must be global, glob or folder, not {s}")))?),
    };
    let delivery = match str_nonempty(args, "delivery") {
        None => None,
        Some(s) => Some(Delivery::parse(s).ok_or_else(|| OpError::usage(format!("--delivery must be import or copy, not {s}")))?),
    };
    Ok(SpecEdit {
        glob: str_nonempty(args, "glob").map(String::from),
        scope,
        folders: strings(args, "folders"),
        imports: strings(args, "imports"),
        delivery,
    })
}

fn find<'a>(layers: &'a [Layer], name: &str) -> Result<&'a Layer, OpError> {
    let full = format!("client-{}", layers::slug(name));
    layers
        .iter()
        .find(|l| l.name == name || l.name == full)
        .ok_or_else(|| OpError::not_found(format!("no layer named {name} in the skills repository's rules/")))
}

fn shell_arg(text: &str) -> String {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "-_./~=,:+@".contains(c)) {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

fn spec_flags(layer: &Layer) -> String {
    let spec = &layer.spec;
    let mut out = String::new();
    if !layer.globs.is_empty() {
        out.push_str(&format!(" --glob {}", shell_arg(&layer.globs.join(","))));
    }
    out.push_str(&format!(" --scope {} --delivery {}", spec.scope.as_str(), spec.delivery.as_str()));
    for (flag, items) in [("--folder", &spec.folders), ("--import", &spec.imports)] {
        if items.is_empty() {
            out.push_str(&format!(" {flag} ''"));
        }
        for item in items {
            out.push_str(&format!(" {flag} {}", shell_arg(item)));
        }
    }
    out
}

fn level_text(issue: &Issue) -> String {
    format!("{}: {}", issue.key, issue.message)
}

fn refuse_errors(issues: &[Issue]) -> Result<(), OpError> {
    let errors: Vec<String> = issues.iter().filter(|i| i.level == "error").map(level_text).collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(OpError::failed("invalid", errors.join("; ")))
    }
}

fn exclude_missing(folder: &Path) -> bool {
    let git = folder.join(".git");
    git.is_dir()
        && !fs::read_to_string(git.join("info/exclude"))
            .unwrap_or_default()
            .lines()
            .any(|l| l == "CLAUDE.local.md")
}

fn changed(effect: &FolderEffect) -> bool {
    !effect.foreign && effect.content != effect.existing
}

fn effect_steps(effects: &[FolderEffect], warnings: &mut Vec<String>) -> Vec<Value> {
    let mut steps = Vec::new();
    for e in effects {
        let path = fsx::display(&e.path);
        if e.foreign {
            if !e.layers.is_empty() {
                warnings.push(format!("{path} exists and is not managed; it is left alone"));
            }
            continue;
        }
        match (&e.content, &e.existing) {
            (Some(content), existing) if existing.as_deref() != Some(content.as_str()) => {
                steps.push(json!({
                    "op": if existing.is_some() { "update" } else { "create" },
                    "path": path,
                    "detail": format!("deliver {} into the managed CLAUDE.local.md", e.layers.join(", ")),
                }));
            }
            (None, Some(_)) => steps.push(json!({
                "op": "delete", "path": path,
                "detail": "remove the managed CLAUDE.local.md: no layer is delivered here any more",
            })),
            _ => {}
        }
        if e.content.is_some() && exclude_missing(&e.folder) {
            steps.push(json!({
                "op": "update",
                "path": fsx::display(&e.folder.join(".git/info/exclude")),
                "detail": "git-ignore CLAUDE.local.md",
            }));
        }
    }
    steps
}

/// The folders whose `CLAUDE.local.md` changes when the layer set goes from `before` to `after`.
fn effects_between(w: &World, before: &[Layer], after: &[Layer], warnings: &mut Vec<String>) -> Vec<FolderEffect> {
    let mut ignored = Vec::new();
    let was = layers::local_targets(&w.roots, &w.clients_root, before, &mut ignored);
    let now = layers::local_targets(&w.roots, &w.clients_root, after, warnings);
    let folders: BTreeSet<PathBuf> = was.keys().chain(now.keys()).cloned().collect();
    let touched: Vec<PathBuf> = folders
        .into_iter()
        .filter(|f| {
            let names = |t: &std::collections::BTreeMap<PathBuf, Vec<layers::Entry>>| {
                t.get(f).map(|es| es.iter().map(|e| (e.name.clone(), e.text.clone())).collect::<Vec<_>>())
            };
            names(&was) != names(&now) || was.contains_key(f) != now.contains_key(f)
        })
        .collect();
    layers::local_effects(touched, &now)
}

fn plan(summary: String, steps: Vec<Value>, warnings: Vec<String>, undo: String) -> Value {
    json!({ "summary": summary, "steps": steps, "effects": {}, "warnings": warnings, "undo": undo })
}

fn apply_effects(effects: &[FolderEffect], changed_paths: &mut Vec<String>) -> Result<(), OpError> {
    let mut report = Report::default();
    for effect in effects {
        if changed(effect) {
            changed_paths.push(fsx::display(&effect.path));
        }
        layers::write_local_effect(effect, &mut report, false).map_err(|e| OpError::failed("write_failed", e))?;
    }
    Ok(())
}

fn issues_of(w: &World, layers: &[Layer], layer: &Layer) -> Vec<Issue> {
    layer_spec::lint(&w.roots.home, layers, layer)
}

fn diff(before: &str, after: &str) -> Value {
    json!({ "before": before, "after": after })
}

fn prospective(path: &Path, text: &str, dir_name: &str) -> Layer {
    layers::layer_from(path.to_path_buf(), &layers::frontmatter_of(text), dir_name.to_string())
}

fn scaffolded(w: &World, name: &str, mut edit: SpecEdit) -> SpecEdit {
    let slug = layers::slug(name);
    let Some(scaffold) = w.config.layer_scaffolds.get(name).or_else(|| w.config.layer_scaffolds.get(&slug)) else {
        return edit;
    };
    edit.glob = edit.glob.or_else(|| scaffold.glob.clone());
    edit.scope = edit.scope.or_else(|| scaffold.scope.as_deref().and_then(Scope::parse));
    edit.delivery = edit.delivery.or_else(|| scaffold.delivery.as_deref().and_then(Delivery::parse));
    if edit.folders.is_none() && !scaffold.folders.is_empty() {
        edit.folders = Some(scaffold.folders.clone());
    }
    if edit.imports.is_none() && !scaffold.imports.is_empty() {
        edit.imports = Some(scaffold.imports.clone());
    }
    edit
}

fn spec_json(layer: &Layer) -> Value {
    json!({
        "scope": layer.spec.scope.as_str(), "folders": layer.spec.folders,
        "imports": layer.spec.imports, "delivery": layer.spec.delivery.as_str(),
    })
}

pub fn add(args: &Value) -> Result<Value, OpError> {
    let w = world(args)?;
    let name = str_nonempty(args, "name").ok_or_else(|| OpError::usage("name is required"))?;
    if name.trim().is_empty() {
        return Err(OpError::usage("client name must not be empty"));
    }
    let dry = flag(args, "dryRun");
    let edit = scaffolded(&w, name, edit_of(args)?);
    let glob = edit.glob.clone();
    let rule = layers::plan_client_rule_with(&w.roots, name, glob.as_deref(), &edit).map_err(OpError::usage)?;
    let created = !rule.path.exists();
    let path = fsx::display(&rule.path);
    let rule_name = format!("client-{}", layers::slug(name));
    let before = layers::list_layers(&w.roots);
    if !created {
        let layer = find(&before, &rule_name)?;
        return Ok(json!({
            "dryRun": dry, "name": name, "rule": rule_name, "path": path, "glob": rule.glob,
            "created": false, "scope": layer.spec.scope.as_str(), "folders": layer.spec.folders,
            "imports": layer.spec.imports, "delivery": layer.spec.delivery.as_str(),
            "plan": plan(format!("Client layer {name} already exists"), Vec::new(), Vec::new(), String::new()),
            "result": Value::Null, "issues": issues_of(&w, &before, layer),
        }));
    }
    let dir_name = rule.path.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let layer = prospective(&rule.path, &rule.content, &dir_name);
    let mut after = before.clone();
    after.push(layer.clone());
    let issues = issues_of(&w, &after, &layer);
    refuse_errors(&issues)?;
    let mut warnings: Vec<String> = issues.iter().map(level_text).collect();
    let effects = effects_between(&w, &before, &after, &mut warnings);
    let mut steps = vec![json!({
        "op": "create", "path": path,
        "detail": format!("scaffold the layer {rule_name} ({} scope, {} delivery)", layer.spec.scope.as_str(), layer.spec.delivery.as_str()),
        "diff": diff("", &rule.content),
    })];
    steps.extend(effect_steps(&effects, &mut warnings));
    let undo = format!("toolportctl context client rm {}", shell_arg(name));
    let mut result = Value::Null;
    if !dry {
        layers::scaffold_client_rule_with(&w.roots, name, glob.as_deref(), &edit)
            .map_err(|e| OpError::failed("write_failed", e))?;
        let mut files = vec![path.clone()];
        apply_effects(&effects, &mut files)?;
        result = json!({ "applied": true, "changed": files, "undo": undo, "backups": [] });
    }
    let mut out = json!({
        "dryRun": dry, "name": name, "rule": rule_name, "path": path, "glob": rule.glob,
        "created": true, "plan": plan(format!("Add the client layer {name}"), steps, warnings, undo),
        "result": result, "issues": issues,
    });
    out["scope"] = json!(layer.spec.scope.as_str());
    out["folders"] = json!(layer.spec.folders);
    out["imports"] = json!(layer.spec.imports);
    out["delivery"] = json!(layer.spec.delivery.as_str());
    Ok(out)
}

pub fn edit(args: &Value) -> Result<Value, OpError> {
    let w = world(args)?;
    let name = str_nonempty(args, "name").ok_or_else(|| OpError::usage("name is required"))?;
    let dry = flag(args, "dryRun");
    let change = edit_of(args)?;
    let before = layers::list_layers(&w.roots);
    let layer = find(&before, name)?;
    let text = fs::read_to_string(&layer.path).map_err(|e| OpError::failed("read_failed", format!("{}: {e}", layer.path.display())))?;
    let edits = change.edits();
    let new_text = if edits.is_empty() {
        text.clone()
    } else {
        layer_spec::rewrite_frontmatter(&text, &edits).ok_or_else(|| OpError::failed("invalid", "the layer has no frontmatter"))?
    };
    let dir_name = layer.path.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let updated = prospective(&layer.path, &new_text, &dir_name);
    let after: Vec<Layer> = before.iter().map(|l| if l.path == layer.path { updated.clone() } else { l.clone() }).collect();
    let issues = issues_of(&w, &after, &updated);
    refuse_errors(&issues)?;
    let mut warnings: Vec<String> = issues.iter().map(level_text).collect();
    let effects = effects_between(&w, &before, &after, &mut warnings);
    let path = fsx::display(&layer.path);
    let mut steps = Vec::new();
    if new_text != text {
        steps.push(json!({
            "op": "update", "path": path,
            "detail": format!("change the delivery of {}: {} scope, {} delivery", layer.name, updated.spec.scope.as_str(), updated.spec.delivery.as_str()),
            "diff": diff(&text, &new_text),
        }));
    }
    steps.extend(effect_steps(&effects, &mut warnings));
    let undo = if new_text == text {
        String::new()
    } else {
        format!("toolportctl context client edit {}{}", shell_arg(&layer.name), spec_flags(layer))
    };
    let mut result = Value::Null;
    if !dry {
        let mut files = Vec::new();
        if new_text != text {
            write_text(&layer.path, &new_text).map_err(|e| OpError::failed("write_failed", e))?;
            files.push(path.clone());
        }
        apply_effects(&effects, &mut files)?;
        result = json!({ "applied": true, "changed": files, "undo": undo, "backups": [] });
    }
    let summary = if steps.is_empty() { format!("Nothing to change in {}", layer.name) } else { format!("Edit the client layer {}", layer.name) };
    let mut out = json!({
        "dryRun": dry, "name": layer.name, "path": path, "changed": new_text != text,
        "plan": plan(summary, steps, warnings, undo), "result": result, "issues": issues,
    });
    out["scope"] = json!(updated.spec.scope.as_str());
    out["folders"] = json!(updated.spec.folders);
    out["imports"] = json!(updated.spec.imports);
    out["delivery"] = json!(updated.spec.delivery.as_str());
    Ok(out)
}

pub fn rm(args: &Value) -> Result<Value, OpError> {
    let w = world(args)?;
    let name = str_nonempty(args, "name").ok_or_else(|| OpError::usage("name is required"))?;
    let dry = flag(args, "dryRun");
    let before = layers::list_layers(&w.roots);
    let layer = find(&before, name)?;
    if !layer.name.starts_with("client-") {
        return Err(OpError::usage(format!("{} is not a client layer; only client-* layers can be removed here", layer.name)));
    }
    let after: Vec<Layer> = before.iter().filter(|l| l.path != layer.path).cloned().collect();
    let mut warnings = Vec::new();
    let importers: Vec<&str> = after.iter().filter(|l| l.spec.imports.iter().any(|i| *i == layer.name)).map(|l| l.name.as_str()).collect();
    if !importers.is_empty() {
        warnings.push(format!("{} imports this layer and will report it missing", importers.join(", ")));
    }
    let used: Vec<String> = super::bundle_store::list(&w.roots)
        .into_iter()
        .filter(|b| b.parsed.as_ref().is_ok_and(|p| p.layers_add.iter().any(|n| *n == layer.name)))
        .map(|b| b.name)
        .collect();
    if !used.is_empty() {
        warnings.push(format!("bundle {} lists it in layers.add", used.join(", ")));
    }
    let deployed = w.roots.claude_home.join("rules").join(format!("{}.md", layer.name));
    if deployed.exists() {
        warnings.push(format!("{} stays until the next `toolportctl skills sync`", deployed.display()));
    }
    let effects = effects_between(&w, &before, &after, &mut warnings);
    let path = fsx::display(&layer.path);
    let text = fs::read_to_string(&layer.path).unwrap_or_default();
    let mut steps = vec![json!({
        "op": "delete", "path": path,
        "detail": format!("delete the layer {}", layer.name),
        "diff": diff(&text, ""),
    })];
    steps.extend(effect_steps(&effects, &mut warnings));
    let short = layer.name.strip_prefix("client-").unwrap_or(&layer.name);
    let undo = format!(
        "toolportctl context client add {}{} (the body comes back from the skills repository's git history)",
        shell_arg(short),
        spec_flags(layer)
    );
    let mut result = Value::Null;
    if !dry {
        let mut files = vec![path.clone()];
        fs::remove_file(&layer.path).map_err(|e| OpError::failed("write_failed", format!("{path}: {e}")))?;
        if let Some(dir) = layer.path.parent() {
            let _ = fs::remove_dir(dir);
        }
        apply_effects(&effects, &mut files)?;
        result = json!({ "applied": true, "changed": files, "undo": undo, "backups": [] });
    }
    Ok(json!({
        "dryRun": dry, "name": layer.name, "path": path,
        "plan": plan(format!("Remove the client layer {}", layer.name), steps, warnings, undo),
        "result": result,
    }))
}

pub fn list(args: &Value) -> Result<Value, OpError> {
    let w = world(args)?;
    let all = layers::list_layers(&w.roots);
    let mut ignored = Vec::new();
    let targets = layers::local_targets(&w.roots, &w.clients_root, &all, &mut ignored);
    let rows: Vec<Value> = all
        .iter()
        .map(|layer| {
            let mut deployed: Vec<String> = Vec::new();
            let rule = w.roots.claude_home.join("rules").join(format!("{}.md", layer.name));
            if layer.spec.scope != Scope::Folder && rule.is_file() {
                deployed.push(fsx::display(&rule));
            }
            for (folder, entries) in &targets {
                let file = folder.join("CLAUDE.local.md");
                let managed = fs::read_to_string(&file).is_ok_and(|t| layers::is_managed_local(&t));
                if managed && entries.iter().any(|e| e.name == layer.name) {
                    deployed.push(fsx::display(&file));
                }
            }
            let mut row = json!({
                "name": layer.name, "path": fsx::display(&layer.path), "globs": layer.globs,
                "description": layer.description, "deployedTo": deployed,
                "issues": issues_of(&w, &all, layer),
            });
            if let (Some(row), Some(spec)) = (row.as_object_mut(), spec_json(layer).as_object()) {
                row.extend(spec.clone());
            }
            row
        })
        .collect();
    Ok(json!({ "layers": rows }))
}
