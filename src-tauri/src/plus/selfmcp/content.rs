use super::backend::skills_repo;
use super::ToolError;
use crate::plus::args::{flag, flag_or, nonempty_strings, str_arg};
use crate::plus::hashing::lock_hash;
use crate::plus::skills::agents::lint::lint_agents;
use crate::plus::skills::agents::{
    all_agent_transpilers, discover_agents, sync_scoped, Agent, AgentKind,
};
use crate::plus::skills::api::{self, lint_json};
use crate::plus::skills::kind::ContentKind;
use crate::plus::skills::lock::LockFile;
use crate::plus::skills::ops::read_lock;
use crate::plus::skills::parser::{
    build_frontmatter, discover_skills, parse_frontmatter, Skill, SkillKind,
};
use crate::plus::skills::pyfs::write_text;
use crate::plus::skills::repo::{skill_bucket, skill_template};
use crate::plus::skills::styles::lint::lint_styles;
use crate::plus::skills::styles::manage::{
    apply_scoped, remove_scoped, style_template, sync_scoped as sync_styles_scoped,
};
use crate::plus::skills::styles::{
    all_style_transpilers, discover_styles, Style, StyleKind, Tier,
};
use crate::plus::skills::tap_handlers as taps;
use crate::plus::skills::tap_ops::{Kind, TapError};
use crate::plus::skills::SystemClock;
use serde_json::{json, Value};
use serde_yaml::{Mapping, Value as Yaml};
use std::path::{Path, PathBuf};

type Outcome = Result<Value, ToolError>;

fn home() -> Result<PathBuf, ToolError> {
    Ok(api::home()?)
}

fn global_mode(args: &Value) -> bool {
    flag_or(args, "global_mode", true)
}

fn dry_run(args: &Value) -> bool {
    flag(args, "dry_run")
}

fn lock_for_read(repo: &Path) -> Option<LockFile> {
    read_lock(repo).map(|(lock, _)| lock)
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

fn kebab(name: &str) -> Result<&str, ToolError> {
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

fn name_arg(args: &Value) -> Result<&str, ToolError> {
    path_safe(str_arg(args, "name").unwrap_or_default())
}

fn find<K: ContentKind>(repo: &Path, name: &str, noun: &str) -> Result<K::Item, ToolError> {
    for dir in K::DIRS {
        let path = repo.join(dir).join(name).join(K::FILE);
        if path.exists() {
            return K::parse(&path).map_err(|e| ToolError::new("invalid_input", e));
        }
    }
    Err(ToolError::new(
        "not_found",
        format!("no {noun} named {name}"),
    ))
}

fn find_skill(repo: &Path, name: &str) -> Result<Skill, ToolError> {
    find::<SkillKind>(repo, name, "skill or rule")
}

fn find_agent(repo: &Path, name: &str) -> Result<Agent, ToolError> {
    find::<AgentKind>(repo, name, "agent")
}

fn find_style(repo: &Path, name: &str) -> Result<Style, ToolError> {
    find::<StyleKind>(repo, name, "style")
}

fn file_hash(path: &Path) -> Result<String, ToolError> {
    let bytes = std::fs::read(path).map_err(|e| ToolError::backend(e.to_string()))?;
    Ok(lock_hash(bytes))
}

fn fence(line: &str) -> bool {
    line.starts_with("---") && line.trim() == "---"
}

fn rewrite_body(path: &Path, new_body: &str) -> Result<String, ToolError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| ToolError::backend(e.to_string()))?;
    let mut lines = raw.split_inclusive('\n');
    let first_ok = lines.next().is_some_and(fence);
    let mut yaml = String::new();
    let mut closed = false;
    if first_ok {
        for line in lines {
            if fence(line) {
                closed = true;
                break;
            }
            yaml.push_str(line);
        }
    }
    if !closed {
        return Err(ToolError::new(
            "invalid_input",
            format!("{} has no YAML frontmatter", path.display()),
        ));
    }
    let content = format!("---\n{yaml}---\n{}\n", new_body.trim_end());
    write_text(path, &content).map_err(ToolError::backend)?;
    file_hash(path)
}

fn rewrite_frontmatter(path: &Path, patch: &Value, skill: bool) -> Result<String, ToolError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| ToolError::backend(e.to_string()))?;
    let (mut fm, body) = parse_frontmatter(&raw).map_err(|e| ToolError::new("invalid_input", e))?;
    if fm.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            format!("{} has no YAML frontmatter", path.display()),
        ));
    }
    let patch = patch
        .as_object()
        .ok_or_else(|| ToolError::new("invalid_arguments", "patch must be an object"))?;
    for (key, value) in patch {
        let key = key.replace('-', "_");
        let value: Yaml = serde_yaml::to_value(value)
            .map_err(|e| ToolError::new("invalid_arguments", e.to_string()))?;
        match fm.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => fm.push((key, value)),
        }
    }
    if skill {
        build_frontmatter(&fm).map_err(|e| ToolError::new("invalid_input", e))?;
    }
    let mut map = Mapping::new();
    for (k, v) in fm {
        map.insert(Yaml::String(k), v);
    }
    let yaml =
        serde_yaml::to_string(&map).map_err(|e| ToolError::backend(e.to_string()))?;
    let content = format!("---\n{yaml}---\n{}\n", body.trim_end());
    write_text(path, &content).map_err(ToolError::backend)?;
    file_hash(path)
}

fn scaffold(path: PathBuf, kind: &str, name: &str, content: String) -> Outcome {
    if path.exists() {
        return Err(ToolError::new(
            "conflict",
            format!("{kind} {name} already exists at {}", path.display()),
        ));
    }
    write_text(&path, &content).map_err(ToolError::backend)?;
    Ok(json!({"created_path": path.to_string_lossy(), "kind": kind}))
}

fn agent_row(agent: &Agent) -> Value {
    json!({
        "name": agent.name(),
        "description": agent.frontmatter.description,
        "model": agent.frontmatter.model,
        "tools": agent.frontmatter.tools,
        "path": agent.source_path.to_string_lossy(),
    })
}

fn style_row(style: &Style) -> Value {
    json!({
        "name": style.name(),
        "description": style.frontmatter.description,
        "keepCodingInstructions": style.frontmatter.keep_coding_instructions,
        "path": style.source_path.to_string_lossy(),
    })
}

fn active_styles(lock: Option<&LockFile>) -> Value {
    let mut map = serde_json::Map::new();
    if let Some(lock) = lock {
        for (client, style) in &lock.active_styles {
            map.insert(client.clone(), json!(style));
        }
    }
    Value::Object(map)
}

pub(super) fn inventory(kind: &str) -> Result<String, ToolError> {
    let repo = match skills_repo(&json!({})) {
        Ok(repo) => repo,
        Err(e) if e.kind == "not_found" => return Ok(String::new()),
        Err(e) => return Err(e),
    };
    let rows: Vec<String> = match kind {
        "skills" => discover_skills(&repo)
            .iter()
            .map(|s| format!("{} - {}", s.name(), s.frontmatter.description))
            .collect(),
        "agents" => discover_agents(&repo)
            .iter()
            .map(|a| format!("{} - {}", a.name(), a.frontmatter.description))
            .collect(),
        _ => discover_styles(&repo)
            .iter()
            .map(|s| format!("{} - {}", s.name(), s.frontmatter.description))
            .collect(),
    };
    Ok(rows.join("\n"))
}

pub(super) fn run(name: &str, args: &Value) -> Option<Outcome> {
    Some(match name {
        "skills_scaffold" => skills_scaffold(args),
        "skills_tap_list" => tapped(taps::tap_list_value(args)),
        "skills_search" => tapped(taps::search_value(args)),
        "skills_tap_add" => tap_add(args),
        "skills_tap_remove" => tap_remove(args),
        "skills_tap_update" => tap_update(args),
        "skills_install" => skills_install(args),
        "skills_edit_body" => edit_body(args, "skill"),
        "skills_edit_frontmatter" => skills_edit_frontmatter(args),
        "skills_delete" => skills_delete(args),
        "agents_list" => agents_list(args),
        "agents_get" => agents_get(args),
        "agents_lint" => skills_repo(args).map(|r| lint_json(&lint_agents(&discover_agents(&r)))),
        "agents_list_transpilers" => Ok(json!({"transpilers": all_agent_transpilers()
            .iter()
            .map(|t| t.client_key().to_string())
            .collect::<Vec<_>>()})),
        "agents_scaffold" => agents_scaffold(args),
        "agents_sync" => agents_sync(args),
        "agents_edit_body" => edit_body(args, "agent"),
        "styles_list" => styles_list(args),
        "styles_get" => styles_get(args),
        "styles_lint" => skills_repo(args).map(|r| lint_json(&lint_styles(&discover_styles(&r)))),
        "styles_active" => styles_active(args),
        "styles_list_transpilers" => Ok(styles_transpilers()),
        "styles_scaffold" => styles_scaffold(args),
        "styles_sync_tier1" => styles_sync_tier1(args),
        "styles_apply" => styles_apply(args),
        "styles_edit_body" => edit_body(args, "style"),
        "styles_remove" => styles_remove(args),
        _ => return None,
    })
}

fn tapped(outcome: Result<Value, TapError>) -> Outcome {
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

/// The tap tools that write apply only when `dry_run` is passed as false.
fn applies(args: &Value) -> bool {
    !flag_or(args, "dry_run", true)
}

fn tap_add(args: &Value) -> Outcome {
    tapped(taps::tap_add_value(&json!({
        "repo": str_arg(args, "repo"),
        "name": str_arg(args, "name"),
        "dry_run": !applies(args),
    })))
}

fn tap_remove(args: &Value) -> Outcome {
    tapped(taps::tap_remove_value(&json!({
        "name": str_arg(args, "name"),
        "dry_run": !applies(args),
    })))
}

fn tap_update(args: &Value) -> Outcome {
    tapped(taps::tap_update_value(&json!({
        "name": str_arg(args, "name"),
        "dry_run": !applies(args),
    })))
}

/// Installs into the discovered skills repository, never into the working directory, and always
/// with the audit on.
fn skills_install(args: &Value) -> Outcome {
    let repo = skills_repo(args)?;
    tapped(taps::install_value(&json!({
        "spec": str_arg(args, "spec"),
        "repo_path": repo.to_string_lossy(),
        "dry_run": !applies(args),
        "no_audit": false,
    })))
}

fn skills_scaffold(args: &Value) -> Outcome {
    let name = kebab(str_arg(args, "name").unwrap_or_default())?;
    let skill_type = str_arg(args, "skill_type").unwrap_or("skill");
    if !["skill", "rule"].contains(&skill_type) {
        return Err(ToolError::new(
            "invalid_arguments",
            "skill_type must be skill or rule",
        ));
    }
    let repo = skills_repo(args)?;
    scaffold(
        repo.join(skill_bucket(skill_type)).join(name).join("SKILL.md"),
        skill_type,
        name,
        skill_template(name, skill_type),
    )
}

fn edit_body(args: &Value, kind: &str) -> Outcome {
    let name = name_arg(args)?;
    let body = str_arg(args, "new_body").unwrap_or_default();
    let repo = skills_repo(args)?;
    let path = match kind {
        "skill" => find_skill(&repo, name)?.source_path,
        "agent" => find_agent(&repo, name)?.source_path,
        _ => find_style(&repo, name)?.source_path,
    };
    let hash = rewrite_body(&path, body)?;
    Ok(json!({"sourcePath": path.to_string_lossy(), "newHash": hash}))
}

fn skills_edit_frontmatter(args: &Value) -> Outcome {
    let name = name_arg(args)?;
    let repo = skills_repo(args)?;
    let skill = find_skill(&repo, name)?;
    let patch = args.get("patch").cloned().unwrap_or_else(|| json!({}));
    let hash = rewrite_frontmatter(&skill.source_path, &patch, true)?;
    Ok(json!({"sourcePath": skill.source_path.to_string_lossy(), "newHash": hash}))
}

fn skills_delete(args: &Value) -> Outcome {
    let name = name_arg(args)?;
    let repo = skills_repo(args)?;
    let skill = find_skill(&repo, name)?;
    let dir = skill.source_dir().to_path_buf();
    let inside = match (dir.canonicalize(), repo.canonicalize()) {
        (Ok(d), Ok(r)) => d != r && d.starts_with(&r),
        _ => false,
    };
    if !inside {
        return Err(ToolError::new(
            "refused",
            "skill directory is outside the repository",
        ));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| ToolError::backend(e.to_string()))?;
    Ok(json!({"removedPath": dir.to_string_lossy()}))
}

fn agents_list(args: &Value) -> Outcome {
    let repo = skills_repo(args)?;
    let agents = discover_agents(&repo);
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "agents": agents.iter().map(agent_row).collect::<Vec<_>>(),
    }))
}

fn agents_get(args: &Value) -> Outcome {
    let agent = find_agent(&skills_repo(args)?, name_arg(args)?)?;
    let mut row = agent_row(&agent);
    row["body"] = json!(agent.body);
    Ok(row)
}

fn agents_scaffold(args: &Value) -> Outcome {
    let name = kebab(str_arg(args, "name").unwrap_or_default())?;
    let model = str_arg(args, "model").unwrap_or("inherit");
    if model.contains(['\n', '\r']) {
        return Err(ToolError::new("invalid_arguments", "invalid model"));
    }
    let repo = skills_repo(args)?;
    let content = format!(
        "---\nname: {name}\ndescription: \"TODO: Describe when this agent should be invoked and what it produces.\"\nmodel: {model}\n---\n\nTODO: Add the agent's system prompt here. Describe the role, constraints,\nexpected output format, and any tools the agent should prefer or avoid.\n"
    );
    scaffold(
        repo.join("agents").join(name).join("AGENT.md"),
        "agent",
        name,
        content,
    )
}

fn agents_sync(args: &Value) -> Outcome {
    let repo = skills_repo(args)?;
    let agents = discover_agents(&repo);
    let global = global_mode(args);
    if global {
        home()?;
    }
    let dry = dry_run(args);
    let synced = sync_scoped(&repo, &agents, global, dry, nonempty_strings(args, "client_keys"), &SystemClock)
        .map_err(ToolError::backend)?;
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "dryRun": dry,
        "globalMode": global,
        "syncedAt": synced.lock.synced_at,
        "agentCount": synced.lock.agents.len(),
    }))
}

fn styles_list(args: &Value) -> Outcome {
    let repo = skills_repo(args)?;
    let styles = discover_styles(&repo);
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "styles": styles.iter().map(style_row).collect::<Vec<_>>(),
    }))
}

fn styles_get(args: &Value) -> Outcome {
    let style = find_style(&skills_repo(args)?, name_arg(args)?)?;
    let mut row = style_row(&style);
    row["body"] = json!(style.body);
    Ok(row)
}

fn styles_active(args: &Value) -> Outcome {
    let repo = skills_repo(args)?;
    let lock = lock_for_read(&repo);
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "activeStyles": active_styles(lock.as_ref()),
        "lockfilePresent": lock.is_some(),
    }))
}

fn styles_transpilers() -> Value {
    let keys = |tier: Tier| -> Vec<String> {
        all_style_transpilers()
            .iter()
            .filter(|t| t.tier() == tier)
            .map(|t| t.client_key().to_string())
            .collect()
    };
    json!({"tier1": keys(Tier::Native), "tier2": keys(Tier::ApplyRemove)})
}

fn styles_scaffold(args: &Value) -> Outcome {
    let name = kebab(str_arg(args, "name").unwrap_or_default())?;
    let repo = skills_repo(args)?;
    scaffold(
        repo.join("styles").join(name).join("STYLE.md"),
        "style",
        name,
        style_template(name),
    )
}

fn styles_sync_tier1(args: &Value) -> Outcome {
    let repo = skills_repo(args)?;
    let styles = discover_styles(&repo);
    home()?;
    let dry = dry_run(args);
    let synced = sync_styles_scoped(&repo, &styles, true, dry, nonempty_strings(args, "client_keys"), &SystemClock)
        .map_err(ToolError::backend)?;
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "dryRun": dry,
        "syncedAt": synced.lock.synced_at,
        "styleCount": synced.lock.styles.len(),
    }))
}

fn styles_apply(args: &Value) -> Outcome {
    let name = name_arg(args)?;
    let repo = skills_repo(args)?;
    let style = find_style(&repo, name)?;
    home()?;
    let dry = dry_run(args);
    let applied = apply_scoped(&repo, &style, true, dry, nonempty_strings(args, "client_keys"), &SystemClock)
        .map_err(ToolError::backend)?;
    Ok(json!({
        "applied": name,
        "dryRun": dry,
        "activeStyles": active_styles(Some(&applied.lock)),
    }))
}

fn styles_remove(args: &Value) -> Outcome {
    let repo = skills_repo(args)?;
    home()?;
    let dry = dry_run(args);
    let removed = remove_scoped(&repo, true, dry, nonempty_strings(args, "client_keys"), &SystemClock)
        .map_err(ToolError::backend)?;
    Ok(json!({
        "dryRun": dry,
        "activeStyles": active_styles(removed.lock.as_ref()),
    }))
}
