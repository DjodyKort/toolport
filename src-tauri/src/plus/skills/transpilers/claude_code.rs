use super::{render_frontmatter, Field};
use crate::plus::skills::json::{self, J};
use crate::plus::skills::parser::{Activation, Skill, SkillType};
use crate::plus::skills::pyfs::write_text;
use crate::plus::skills::scalar;
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::fs;
use std::path::{Component, Path, PathBuf};

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

/// Unreadable or malformed settings count as empty so a sync never aborts on them.
fn load_settings(root: &Path) -> Result<Vec<(String, J)>, String> {
    let path = settings_path(root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match json::parse(&text) {
        Ok(J::Obj(items)) => Ok(items),
        Ok(_) => Err(format!("{}: top level is not an object", path.display())),
        Err(_) => Ok(Vec::new()),
    }
}

fn write_settings(root: &Path, settings: Vec<(String, J)>) -> Result<(), String> {
    let path = settings_path(root);
    write_text(&path, &format!("{}\n", J::Obj(settings).dumps()))
}

fn slot<'a>(items: &'a mut Vec<(String, J)>, key: &str, default: J) -> &'a mut J {
    let idx = match items.iter().position(|(k, _)| k == key) {
        Some(i) => i,
        None => {
            items.push((key.to_string(), default));
            items.len() - 1
        }
    };
    &mut items[idx].1
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

fn entry_has_command(entry: &J, cmd: &str) -> bool {
    match entry.get("hooks") {
        Some(J::Arr(hooks)) => hooks
            .iter()
            .any(|h| h.get("command").and_then(J::as_str) == Some(cmd)),
        _ => false,
    }
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
            let body = crate::plus::context::layer_spec::rule_body(root, &skill.source_path)
                .unwrap_or_else(|| skill.body.clone());
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

    fn install_hooks(&self, skill: &Skill, root: &Path) -> Result<Vec<String>, String> {
        let Some(hooks) = skill.frontmatter.hooks.as_ref().filter(|h| !h.is_empty()) else {
            return Ok(Vec::new());
        };
        let mut settings = load_settings(root)?;
        let hooks_section = slot(&mut settings, "hooks", J::Obj(Vec::new()));
        let J::Obj(events) = hooks_section else {
            return Err("settings.json: `hooks` is not an object".into());
        };
        let skill_dir = root.join(".claude/skills").join(skill.name());
        let mut installed = Vec::new();
        for (event, hook) in hooks {
            let target = resolve(&skill_dir.join(&hook.command));
            let cmd = target.to_string_lossy().into_owned();
            let J::Arr(entries) = slot(events, event, J::Arr(Vec::new())) else {
                return Err(format!("settings.json: hooks.{event} is not a list"));
            };
            if !entries.iter().any(|e| entry_has_command(e, &cmd)) {
                entries.push(J::Obj(vec![
                    ("matcher".into(), J::str(&hook.matcher)),
                    (
                        "hooks".into(),
                        J::Arr(vec![J::Obj(vec![
                            ("type".into(), J::str(&hook.hook_type)),
                            ("command".into(), J::str(&cmd)),
                        ])]),
                    ),
                ]));
            }
            ensure_executable(&target);
            installed.push(cmd);
        }
        write_settings(root, settings)?;
        Ok(installed)
    }

    fn uninstall_hooks(&self, root: &Path, ids: &[String]) -> Result<Vec<String>, String> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut settings = load_settings(root)?;
        let Some(idx) = settings.iter().position(|(k, _)| k == "hooks") else {
            return Ok(Vec::new());
        };
        let J::Obj(events) = &mut settings[idx].1 else {
            return Ok(Vec::new());
        };
        if events.is_empty() {
            return Ok(Vec::new());
        }
        let mut removed = Vec::new();
        let mut kept_events = Vec::new();
        for (event, value) in std::mem::take(events) {
            let J::Arr(entries) = value else {
                kept_events.push((event, value));
                continue;
            };
            let mut kept = Vec::new();
            for mut entry in entries {
                let hooks = match entry.get("hooks") {
                    Some(J::Arr(h)) => h.clone(),
                    _ => Vec::new(),
                };
                let mut filtered = Vec::new();
                for h in hooks {
                    match h.get("command").and_then(J::as_str) {
                        Some(c) if ids.iter().any(|i| i == c) => removed.push(c.to_string()),
                        _ => filtered.push(h),
                    }
                }
                if filtered.is_empty() {
                    continue;
                }
                if let J::Obj(fields) = &mut entry {
                    *slot(fields, "hooks", J::Null) = J::Arr(filtered);
                }
                kept.push(entry);
            }
            if !kept.is_empty() {
                kept_events.push((event, J::Arr(kept)));
            }
        }
        let empty = kept_events.is_empty();
        *events = kept_events;
        if empty {
            settings.remove(idx);
        }
        write_settings(root, settings)?;
        Ok(removed)
    }
}
