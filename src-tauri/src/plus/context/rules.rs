//! Deploys canonical layer rules to `~/.claude/rules/*.md` through the skills core (lock, stale
//! cleanup, collision backups). Only the claude-code rule output is implemented here; the full
//! per-client transpiler set lands with the skills transpilers.

use super::roots::Roots;
use crate::plus::skills::parser::Skill;
use crate::plus::skills::transpiler::TranspileResult;
use crate::plus::skills::{
    discover_skills, load_lockfile, save_lockfile, sync_skills, Clock, SkillType, SyncOptions,
    SystemClock, Transpiler, TranspilerRegistry,
};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ClaudeRuleTranspiler;

fn paths_list(globs: &str) -> String {
    let items: Vec<String> = globs
        .split(',')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(|g| format!("\"{g}\""))
        .collect();
    format!("[{}]", items.join(", "))
}

impl Transpiler for ClaudeRuleTranspiler {
    fn client_key(&self) -> &str {
        "claude-code"
    }

    fn transpile(&self, skill: &Skill, output_root: &Path) -> Result<TranspileResult, String> {
        if skill.skill_type != SkillType::Rule {
            return Err("only rules are transpiled by the context deployer".into());
        }
        let globs = skill.frontmatter.globs.as_deref().filter(|g| !g.is_empty());
        let content = match globs {
            Some(g) => format!("---\npaths: {}\n---\n\n{}\n", paths_list(g), skill.body),
            None => format!("{}\n", skill.body),
        };
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, output_root),
            content,
            warnings: Vec::new(),
        })
    }

    fn get_output_path(&self, skill: &Skill, output_root: &Path) -> PathBuf {
        output_root
            .join(".claude/rules")
            .join(format!("{}.md", skill.name()))
    }

    fn get_collision_paths(&self, skill: &Skill, output_root: &Path) -> Vec<PathBuf> {
        if skill.skill_type != SkillType::Rule {
            return Vec::new();
        }
        let n = skill.name();
        vec![
            output_root.join(".claude/commands").join(format!("{n}.md")),
            output_root.join(".claude/agents").join(format!("{n}.md")),
        ]
    }
}

/// Syncs `rules/*` to `<home>/.claude/rules`; returns the paths that were (or would be) written.
/// Non-rule skills in the repo are left to the skills pipeline.
pub fn deploy_rules(
    roots: &Roots,
    clock: &dyn Clock,
    dry_run: bool,
) -> Result<Vec<PathBuf>, String> {
    let repo = roots.skills_repo_path();
    let all = discover_skills(&repo);
    let transpiler = ClaudeRuleTranspiler;
    let mut written = Vec::new();
    for rule in all.iter().filter(|s| s.skill_type == SkillType::Rule) {
        let out = transpiler.transpile(rule, &roots.home)?;
        if fs::read_to_string(&out.output_path).ok().as_deref() != Some(out.content.as_str()) {
            written.push(out.output_path);
        }
    }
    let mut registry = TranspilerRegistry::new();
    registry.register(Box::new(ClaudeRuleTranspiler));
    let opts = SyncOptions {
        output_root: roots.home.clone(),
        lock_dir: roots.config_dir.clone(),
        global_mode: true,
        dry_run,
        migrate: None,
        client_keys: Some(vec!["claude-code".into()]),
        clock,
    };
    let previous = load_lockfile(&roots.config_dir);
    let mut result = sync_skills(&all, &registry, &opts)?;
    // The skills pipeline owns the skills bucket; every skill is passed in only so stale
    // cleanup cannot mistake it for a removed one.
    result.lockfile.skills = previous.map(|p| p.skills).unwrap_or_default();
    if !dry_run {
        save_lockfile(&roots.config_dir, &result.lockfile)?;
    }
    Ok(written)
}

pub fn deploy_rules_now(roots: &Roots, dry_run: bool) -> Result<Vec<PathBuf>, String> {
    deploy_rules(roots, &SystemClock, dry_run)
}
