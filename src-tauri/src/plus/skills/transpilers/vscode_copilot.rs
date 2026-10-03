use super::{render_frontmatter, Field};
use crate::plus::skills::parser::{Activation, Skill, SkillType};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

/// Not part of `register_all`: mcpm's `transpilers/__init__.py` never imports this module, so
/// the `vscode` client is unreachable there. Parity keeps the gap; see `register_vscode_copilot`.
pub struct VsCodeCopilot;

fn is_instruction(skill: &Skill) -> bool {
    skill.skill_type == SkillType::Rule || skill.frontmatter.activation == Activation::Always
}

impl Transpiler for VsCodeCopilot {
    fn client_key(&self) -> &str {
        "vscode"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let mut warnings = Vec::new();
        let content = if is_instruction(skill) {
            let mut fields = Vec::new();
            if let Some(g) = fm.globs.as_deref().filter(|g| !g.is_empty()) {
                fields.push(("applyTo", Field::Raw(g)));
            }
            let frontmatter = render_frontmatter(&fields);
            if frontmatter.is_empty() {
                format!("{}\n", skill.body)
            } else {
                format!("{frontmatter}\n\n{}\n", skill.body)
            }
        } else {
            let description = format!("\"{}\"", fm.description);
            let mut fields = vec![
                ("name", Field::Raw(&fm.name)),
                ("description", Field::Raw(&description)),
            ];
            if let Some(t) = fm.allowed_tools.as_deref().filter(|t| !t.is_empty()) {
                fields.push(("allowed-tools", Field::Raw(t)));
            }
            if matches!(fm.activation, Activation::Agent | Activation::Manual) {
                warnings.push(format!(
                    "vscode: activation '{}' downgraded to 'auto'",
                    fm.activation.as_str()
                ));
            }
            format!("{}\n\n{}\n", render_frontmatter(&fields), skill.body)
        };
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content,
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        if is_instruction(skill) {
            return root
                .join(".github/instructions")
                .join(format!("{}.instructions.md", skill.name()));
        }
        root.join(".github/skills")
            .join(skill.name())
            .join("SKILL.md")
    }
}
