use crate::plus::skills::parser::Skill;
use crate::plus::skills::transpiler::{
    inject_managed_block, TranspileResult, Transpiler, MCPM_BLOCK_END, MCPM_BLOCK_START,
};
use std::fs;
use std::path::{Path, PathBuf};

pub struct AgentsMd;

/// Python `html.escape(s, quote=True)`.
pub(crate) fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

fn skill_line(skill: &Skill) -> String {
    format!(
        "<skill name=\"{}\" description=\"{}\" />",
        skill.name(),
        html_escape(&skill.frontmatter.description)
    )
}

impl Transpiler for AgentsMd {
    fn client_key(&self) -> &str {
        "agents-md"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: skill_line(skill),
            warnings: Vec::new(),
        })
    }

    fn get_output_path(&self, _skill: &Skill, root: &Path) -> PathBuf {
        root.join("AGENTS.md")
    }

    fn transpile_all(
        &self,
        skills: &[Skill],
        root: &Path,
    ) -> Option<Result<TranspileResult, String>> {
        let mut lines = vec!["<available_skills>".to_string()];
        lines.extend(skills.iter().map(skill_line));
        lines.push("</available_skills>".into());
        let path = root.join("AGENTS.md");
        let existing = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => return Some(Err(format!("{}: {e}", path.display()))),
            }
        } else {
            String::new()
        };
        Some(Ok(TranspileResult {
            content: inject_managed_block(&existing, &lines.join("\n")),
            output_path: path,
            warnings: Vec::new(),
        }))
    }
}

/// Removes the managed block from AGENTS.md; deletes the file when nothing else remains.
pub fn clean(root: &Path) -> Result<Vec<PathBuf>, String> {
    let path = root.join("AGENTS.md");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (Some(start), Some(end)) = (content.find(MCPM_BLOCK_START), content.find(MCPM_BLOCK_END))
    else {
        return Ok(Vec::new());
    };
    let before = content[..start].trim_end();
    let after = content[end + MCPM_BLOCK_END.len()..].trim_start();
    let cleaned = format!("{before}\n\n{after}");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    } else {
        fs::write(&path, format!("{cleaned}\n")).map_err(|e| e.to_string())?;
    }
    Ok(vec![path])
}
