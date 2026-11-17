use super::{render_frontmatter, Field};
use crate::skills::parser::{Activation, Skill, SkillType};
use crate::skills::scalar;
use crate::skills::transpiler::{TranspileResult, Transpiler};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Component, Path, PathBuf};
use toolport_plus_api::SettingsStore;

pub struct ClaudeCode;

/// Claude Code expects `paths:` as a YAML list while `globs` is a comma-separated string.
fn paths_list(globs: &str) -> String {
    let items: Vec<String> = globs
        .split(',')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(scalar::quoted_item)
        .collect();
    format!("[{}]", items.join(", "))
}

fn settings_path(root: &Path) -> PathBuf {
    root.join(".claude").join("settings.json")
}

/// The settings file text is returned for [`SettingsStore::write_hooks`], which keeps comments
/// and every key other than `hooks`; a file that does not parse is an error, never empty.
fn load_settings(
    store: &dyn SettingsStore,
    root: &Path,
) -> Result<(PathBuf, Value, Option<String>), String> {
    let path = settings_path(root);
    let (settings, text) = store.read_settings(&path)?;
    if !settings.is_object() {
        return Err(format!("{}: top level is not an object", path.display()));
    }
    Ok((path, settings, text))
}

fn entry_slot<'a>(map: &'a mut Map<String, Value>, key: &str, default: Value) -> &'a mut Value {
    map.entry(key).or_insert(default)
}

/// Python `Path.resolve()`: symlinks resolved for the existing prefix, the rest normalised lexically.
fn resolve(path: &Path) -> PathBuf {
    let mut lexical = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                lexical.pop();
            }
            Component::CurDir => {}
            c => lexical.push(c.as_os_str()),
        }
    }
    let mut existing = lexical.as_path();
    let mut tail = Vec::new();
    loop {
        if let Ok(real) = fs::canonicalize(existing) {
            return tail.iter().rev().fold(real, |p, s| p.join(s));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_os_string());
                existing = parent;
            }
            _ => return lexical,
        }
    }
}

fn entry_has_command(entry: &Value, cmd: &str) -> bool {
    entry["hooks"]
        .as_array()
        .is_some_and(|hooks| hooks.iter().any(|h| h["command"].as_str() == Some(cmd)))
}

#[cfg(unix)]
fn ensure_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mode = meta.permissions().mode();
        if mode & 0o111 == 0 {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o755));
        }
    }
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) {}

impl Transpiler for ClaudeCode {
    fn client_key(&self) -> &str {
        "claude-code"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let globs = fm.globs.as_deref().filter(|g| !g.is_empty());
        let content = if skill.skill_type == SkillType::Rule {
            let paths = globs.map(paths_list);
            let mut fields = Vec::new();
            if let Some(p) = &paths {
                fields.push(("paths", Field::Raw(p)));
            }
            let frontmatter = render_frontmatter(&fields);
            let body = skill.body.clone();
            if frontmatter.is_empty() {
                format!("{body}\n")
            } else {
                format!("{frontmatter}\n\n{body}\n")
            }
        } else {
            let mut fields = vec![
                ("name", Field::Raw(&fm.name)),
                ("description", Field::Quoted(&fm.description)),
            ];
            if let Some(g) = globs {
                fields.push(("paths", Field::Text(g)));
            }
            if let Some(t) = fm.allowed_tools.as_deref().filter(|t| !t.is_empty()) {
                fields.push(("allowed-tools", Field::Text(t)));
            }
            if fm.activation == Activation::Manual {
                fields.push(("disable-model-invocation", Field::Bool(true)));
            }
            format!("{}\n\n{}\n", render_frontmatter(&fields), skill.body)
        };
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content,
            warnings: Vec::new(),
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        if skill.skill_type == SkillType::Rule {
            root.join(".claude/rules")
                .join(format!("{}.md", skill.name()))
        } else {
            root.join(".claude/skills")
                .join(skill.name())
                .join("SKILL.md")
        }
    }

    fn get_collision_paths(&self, skill: &Skill, root: &Path) -> Vec<PathBuf> {
        let n = skill.name();
        vec![
            root.join(".claude/commands").join(format!("{n}.md")),
            root.join(".claude/agents").join(format!("{n}.md")),
        ]
    }

    fn hook_ids(&self, skill: &Skill, root: &Path) -> Vec<String> {
        let skill_dir = root.join(".claude/skills").join(skill.name());
        skill
            .frontmatter
            .hooks
            .iter()
            .flatten()
            .map(|(_, hook)| {
                resolve(&skill_dir.join(&hook.command))
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn install_hooks(
        &self,
        skill: &Skill,
        root: &Path,
        store: &dyn SettingsStore,
    ) -> Result<Vec<String>, String> {
        let Some(hooks) = skill.frontmatter.hooks.as_ref().filter(|h| !h.is_empty()) else {
            return Ok(Vec::new());
        };
        let (path, mut settings, text) = load_settings(store, root)?;
        let before = settings.get("hooks").cloned();
        let Some(map) = settings.as_object_mut() else {
            return Err(format!("{}: top level is not an object", path.display()));
        };
        let Some(events) = entry_slot(map, "hooks", json!({})).as_object_mut() else {
            return Err("settings.json: `hooks` is not an object".into());
        };
        let skill_dir = root.join(".claude/skills").join(skill.name());
        let mut installed = Vec::new();
        for (event, hook) in hooks {
            let target = resolve(&skill_dir.join(&hook.command));
            let cmd = target.to_string_lossy().into_owned();
            let Some(entries) = entry_slot(events, event, json!([])).as_array_mut() else {
                return Err(format!("settings.json: hooks.{event} is not a list"));
            };
            if !entries.iter().any(|e| entry_has_command(e, &cmd)) {
                entries.push(json!({
                    "matcher": hook.matcher,
                    "hooks": [{"type": hook.hook_type, "command": cmd}],
                }));
            }
            ensure_executable(&target);
            installed.push(cmd);
        }
        if settings.get("hooks") != before.as_ref() {
            store.write_hooks(&path, text.as_deref(), &settings)?;
        }
        Ok(installed)
    }

    fn uninstall_hooks(
        &self,
        root: &Path,
        ids: &[String],
        store: &dyn SettingsStore,
    ) -> Result<Vec<String>, String> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let (path, mut settings, text) = load_settings(store, root)?;
        let Some(events) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
            return Ok(Vec::new());
        };
        if events.is_empty() {
            return Ok(Vec::new());
        }
        let mut removed = Vec::new();
        let mut kept_events = Map::new();
        for (event, value) in std::mem::take(events) {
            let Value::Array(entries) = value else {
                kept_events.insert(event, value);
                continue;
            };
            let mut kept = Vec::new();
            for mut entry in entries {
                let hooks = entry["hooks"].as_array().cloned().unwrap_or_default();
                let mut filtered = Vec::new();
                for h in hooks {
                    match h["command"].as_str() {
                        Some(c) if ids.iter().any(|i| i == c) => removed.push(c.to_string()),
                        _ => filtered.push(h),
                    }
                }
                if filtered.is_empty() {
                    continue;
                }
                if let Some(fields) = entry.as_object_mut() {
                    fields.insert("hooks".into(), Value::Array(filtered));
                }
                kept.push(entry);
            }
            if !kept.is_empty() {
                kept_events.insert(event, Value::Array(kept));
            }
        }
        if kept_events.is_empty() {
            if let Some(map) = settings.as_object_mut() {
                map.remove("hooks");
            }
        } else {
            settings["hooks"] = Value::Object(kept_events);
        }
        if !removed.is_empty() {
            store.write_hooks(&path, text.as_deref(), &settings)?;
        }
        Ok(removed)
    }
}
