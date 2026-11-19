//! Tools that edit and delete library files, and the agents and styles tools: the same operations
//! as `toolportctl agents|styles`, over `crate::skills`. A tool that writes defaults `dry_run` to
//! true and never works in the working directory.

use super::skills::{kebab, path_safe, repo_of as skills_repo};
use super::ToolError;
use crate::args::{flag, flag_or, nonempty_strings, str_arg};
use crate::hashing::lock_hash;
use crate::skills::agents::lint::lint_repo as lint_agent_repo;
use crate::skills::agents::{
    all_agent_transpilers, discover_agents, sync_scoped, Agent, AgentKind,
};
use crate::skills::api::{self, lint_json};
use crate::skills::frontmatter_patch::{fields_of, patch_frontmatter};
use crate::skills::kind::ContentKind;
use crate::skills::lint::{lint_description_lengths, lint_skill};
use crate::skills::lock::LockFile;
use crate::skills::ops::read_lock;
use crate::skills::parser::{build_frontmatter, parse_frontmatter, Skill, SkillKind};
use crate::skills::pyfs::write_text;
use crate::skills::styles::lint::lint_repo as lint_style_repo;
use crate::skills::styles::manage::{
    apply_scoped, remove_scoped, style_template, sync_scoped as sync_styles_scoped,
};
use crate::skills::styles::{all_style_transpilers, discover_styles, Style, StyleKind, Tier};
use crate::skills::SystemClock;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use toolport_plus_api::Host;

fn enter(host: &dyn Host) -> crate::fsx::PathsGuard {
    crate::fsx::enter(crate::fsx::Paths {
        home: None,
        data_dir: host.data_dir(),
    })
}

fn home() -> Result<PathBuf, ToolError> {
    Ok(api::home()?)
}

type Outcome = Result<Value, ToolError>;

fn global_mode(args: &Value) -> bool {
    flag_or(args, "global_mode", true)
}

fn dry_run(args: &Value) -> bool {
    flag_or(args, "dry_run", true)
}

fn lock_for_read(repo: &Path) -> Option<LockFile> {
    read_lock(repo).map(|(lock, _)| lock)
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
    let bytes = std::fs::read(path)?;
    Ok(lock_hash(bytes))
}

fn fence(line: &str) -> bool {
    line.starts_with("---") && line.trim() == "---"
}

fn rewrite_body(path: &Path, new_body: &str) -> Result<String, ToolError> {
    let raw = std::fs::read_to_string(path)?;
    let mut lines = raw.split_inclusive('\n');
    let first_ok = lines.next().is_some_and(fence);
    let mut yaml = String::new();
    let mut closed = false;
    if first_ok {
        for line in lines.by_ref() {
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
    // The blank lines between the closing fence and the text stay, so an edit does not move them.
    let gap: String = lines.take_while(|line| line.trim().is_empty()).collect();
    let content = format!("---\n{yaml}---\n{gap}{}\n", new_body.trim_end());
    write_text(path, &content).map_err(ToolError::backend)?;
    file_hash(path)
}

fn scaffold(path: PathBuf, kind: &str, name: &str, content: String, dry_run: bool) -> Outcome {
    if path.exists() {
        return Err(ToolError::new(
            "conflict",
            format!("{kind} {name} already exists at {}", path.display()),
        ));
    }
    if dry_run {
        return Ok(json!({"created_path": path.to_string_lossy(), "kind": kind, "dryRun": true}));
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

pub(super) fn skills_edit_body(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    edit_body(args, "skill")
}

pub(super) fn agents_edit_body(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    edit_body(args, "agent")
}

pub(super) fn styles_edit_body(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    edit_body(args, "style")
}

pub(super) fn agents_lint(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    skills_repo(args).map(|r| lint_json(&lint_agent_repo(&r).result))
}

pub(super) fn styles_lint(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    skills_repo(args).map(|r| lint_json(&lint_style_repo(&r).result))
}

pub(super) fn agents_list_transpilers(host: &dyn Host, _: &Value) -> Outcome {
    let _paths = enter(host);
    Ok(json!({"transpilers": all_agent_transpilers()
        .iter()
        .map(|t| t.client_key().to_string())
        .collect::<Vec<_>>()}))
}

pub(super) fn styles_list_transpilers(host: &dyn Host, _: &Value) -> Outcome {
    let _paths = enter(host);
    Ok(styles_transpilers())
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

/// The frontmatter text and what follows the closing fence, kept as the file has them.
fn split_document(path: &Path, raw: &str) -> Result<(String, String), ToolError> {
    let mut lines = raw.split_inclusive('\n');
    let mut yaml = String::new();
    let mut closed = false;
    if lines.next().is_some_and(fence) {
        for line in lines.by_ref() {
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
    Ok((yaml, lines.collect()))
}

/// Sets, changes or (with `null`) removes single frontmatter fields of a skill. Only the lines of
/// the fields that change are rewritten, and the result is checked like any skill before it is
/// written. `dry_run` reports the same without writing; it also answers an empty patch with the
/// fields as they are.
pub(super) fn skills_edit_frontmatter(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    let name = name_arg(args)?;
    let repo = skills_repo(args)?;
    let skill = find_skill(&repo, name)?;
    let empty = json!({});
    let patch = args
        .get("patch")
        .unwrap_or(&empty)
        .as_object()
        .ok_or_else(|| ToolError::new("invalid_arguments", "patch must be an object"))?;
    let raw = std::fs::read_to_string(&skill.source_path)?;
    let (before, rest) = split_document(&skill.source_path, &raw)?;
    let invalid = |e: String| ToolError::new("invalid_input", e);
    let patched = patch_frontmatter(&before, patch).map_err(invalid)?;
    let text = format!("---\n{}---\n{rest}", patched.yaml);
    let (fm, _) = parse_frontmatter(&text).map_err(invalid)?;
    if fm.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            format!("{} has no YAML frontmatter", skill.source_path.display()),
        ));
    }
    let frontmatter = build_frontmatter(&fm).map_err(invalid)?;
    let candidate = Skill {
        frontmatter,
        ..skill.clone()
    };
    let mut found = lint_skill(&candidate);
    found
        .messages
        .extend(lint_description_lengths(&[candidate], false).messages);
    let dry = flag(args, "dry_run");
    let mut out = json!({
        "sourcePath": skill.source_path.to_string_lossy(),
        "dryRun": dry,
        "changed": patched.changed,
        "rewritten": patched.rewritten,
        "before": before,
        "after": patched.yaml,
        "fields": fields_of(&before).map_err(invalid)?,
        "lint": found
            .messages
            .iter()
            .map(|m| json!({"level": m.level, "message": m.message}))
            .collect::<Vec<_>>(),
    });
    if !dry {
        if !patched.changed.is_empty() {
            write_text(&skill.source_path, &text).map_err(ToolError::backend)?;
        }
        out["newHash"] = json!(file_hash(&skill.source_path)?);
    }
    Ok(out)
}

pub(super) fn skills_delete(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
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
    std::fs::remove_dir_all(&dir)?;
    Ok(json!({"removedPath": dir.to_string_lossy()}))
}

pub(super) fn agents_list(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    let repo = skills_repo(args)?;
    let agents = discover_agents(&repo);
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "agents": agents.iter().map(agent_row).collect::<Vec<_>>(),
    }))
}

pub(super) fn agents_get(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    let agent = find_agent(&skills_repo(args)?, name_arg(args)?)?;
    let mut row = agent_row(&agent);
    row["body"] = json!(agent.body);
    Ok(row)
}

pub(super) fn agents_scaffold(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
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
        flag(args, "dry_run"),
    )
}

pub(super) fn agents_sync(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    let repo = skills_repo(args)?;
    let agents = discover_agents(&repo);
    let global = global_mode(args);
    if global {
        home()?;
    }
    let dry = dry_run(args);
    let synced = sync_scoped(
        &repo,
        &agents,
        global,
        dry,
        nonempty_strings(args, "client_keys"),
        &SystemClock,
    )
    .map_err(ToolError::backend)?;
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "dryRun": dry,
        "globalMode": global,
        "syncedAt": synced.lock.synced_at,
        "agentCount": synced.lock.agents.len(),
    }))
}

pub(super) fn styles_list(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    let repo = skills_repo(args)?;
    let styles = discover_styles(&repo);
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "styles": styles.iter().map(style_row).collect::<Vec<_>>(),
    }))
}

pub(super) fn styles_get(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    let style = find_style(&skills_repo(args)?, name_arg(args)?)?;
    let mut row = style_row(&style);
    row["body"] = json!(style.body);
    Ok(row)
}

pub(super) fn styles_active(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
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

pub(super) fn styles_scaffold(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    let name = kebab(str_arg(args, "name").unwrap_or_default())?;
    let repo = skills_repo(args)?;
    scaffold(
        repo.join("styles").join(name).join("STYLE.md"),
        "style",
        name,
        style_template(name),
        flag(args, "dry_run"),
    )
}

pub(super) fn styles_sync_tier1(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    let repo = skills_repo(args)?;
    let styles = discover_styles(&repo);
    home()?;
    let dry = dry_run(args);
    let synced = sync_styles_scoped(
        &repo,
        &styles,
        true,
        dry,
        nonempty_strings(args, "client_keys"),
        &SystemClock,
    )
    .map_err(ToolError::backend)?;
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "dryRun": dry,
        "syncedAt": synced.lock.synced_at,
        "styleCount": synced.lock.styles.len(),
    }))
}

pub(super) fn styles_apply(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    let name = name_arg(args)?;
    let repo = skills_repo(args)?;
    let style = find_style(&repo, name)?;
    home()?;
    let dry = dry_run(args);
    let applied = apply_scoped(
        &repo,
        &style,
        true,
        dry,
        nonempty_strings(args, "client_keys"),
        &SystemClock,
    )
    .map_err(ToolError::backend)?;
    Ok(json!({
        "applied": name,
        "dryRun": dry,
        "activeStyles": active_styles(Some(&applied.lock)),
    }))
}

pub(super) fn styles_remove(host: &dyn Host, args: &Value) -> Outcome {
    let _paths = enter(host);
    let repo = skills_repo(args)?;
    home()?;
    let dry = dry_run(args);
    let removed = remove_scoped(
        &repo,
        true,
        dry,
        nonempty_strings(args, "client_keys"),
        &SystemClock,
    )
    .map_err(ToolError::backend)?;
    Ok(json!({
        "dryRun": dry,
        "activeStyles": active_styles(removed.lock.as_ref()),
    }))
}
