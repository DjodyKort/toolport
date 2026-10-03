use crate::plus::skills::parser::{Activation, Skill};
use crate::plus::skills::pyfs::read_text;
use crate::plus::skills::transpiler::{
    inject_managed_block, TranspileResult, Transpiler, MCPM_BLOCK_END, MCPM_BLOCK_START,
};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Zed;

impl Transpiler for Zed {
    fn client_key(&self) -> &str {
        "zed"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let mut warnings = Vec::new();
        if fm.activation != Activation::Always {
            warnings.push(format!(
                "zed: activation '{}' downgraded to 'always' (single .rules file)",
                fm.activation.as_str()
            ));
        }
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!("## {}\n\n{}", fm.name, skill.body),
            warnings,
        })
    }

    fn transpile_all(
        &self,
        skills: &[Skill],
        root: &Path,
    ) -> Option<Result<TranspileResult, String>> {
        Some(self.combine(skills, root))
    }

    fn get_output_path(&self, _skill: &Skill, root: &Path) -> PathBuf {
        root.join(".rules")
    }

    fn clean_targets(&self, root: &Path, _managed: &[String]) -> Vec<PathBuf> {
        clean_target(root).into_iter().collect()
    }

    fn clean(&self, root: &Path, _managed: &[String]) -> Result<Vec<PathBuf>, String> {
        clean(root)
    }
}

impl Zed {
    fn combine(&self, skills: &[Skill], root: &Path) -> Result<TranspileResult, String> {
        let mut warnings = Vec::new();
        let mut sections = Vec::new();
        for skill in skills {
            let result = self.transpile(skill, root)?;
            sections.push(result.content);
            warnings.extend(result.warnings);
        }
        let path = root.join(".rules");
        let existing = if path.exists() {
            read_text(&path)?
        } else {
            String::new()
        };
        Ok(TranspileResult {
            content: inject_managed_block(&existing, &sections.join("\n\n---\n\n")),
            output_path: path,
            warnings,
        })
    }
}

/// The file `clean` rewrites: present and still carrying the managed block.
fn clean_target(root: &Path) -> Option<PathBuf> {
    let path = root.join(".rules");
    let content = read_text(&path).ok()?;
    (content.contains(MCPM_BLOCK_START) && content.contains(MCPM_BLOCK_END)).then_some(path)
}

/// Removes the managed block from `.rules`; deletes the file when nothing else remains.
pub fn clean(root: &Path) -> Result<Vec<PathBuf>, String> {
    let path = root.join(".rules");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = read_text(&path)?;
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
