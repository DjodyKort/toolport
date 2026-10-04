use super::{render_frontmatter, Field};
use crate::plus::skills::parser::{Activation, Skill, SkillType};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

pub struct GeminiCli;

impl Transpiler for GeminiCli {
    fn client_key(&self) -> &str {
        "gemini-cli"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let mut fields = vec![
            ("name", Field::Raw(&fm.name)),
            ("description", Field::Quoted(&fm.description)),
        ];
        let mut warnings = Vec::new();
        if !(skill.skill_type == SkillType::Rule || fm.activation == Activation::Always) {
            if let Some(t) = fm.allowed_tools.as_deref().filter(|t| !t.is_empty()) {
                fields.push(("allowed-tools", Field::Text(t)));
            }
            if fm.activation == Activation::Manual {
                warnings.push("gemini-cli: activation 'manual' downgraded to 'agent'".to_string());
            }
        }
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!("{}\n\n{}\n", render_frontmatter(&fields), skill.body),
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        root.join(".gemini/skills")
            .join(skill.name())
            .join("SKILL.md")
    }
}
