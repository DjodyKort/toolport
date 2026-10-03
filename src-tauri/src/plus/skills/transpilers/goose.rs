use super::{render_frontmatter, Field};
use crate::plus::skills::parser::{Activation, Skill, SkillType};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

pub struct Goose;

impl Transpiler for Goose {
    fn client_key(&self) -> &str {
        "goose-cli"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let description = format!("\"{}\"", fm.description);
        let mut fields = vec![
            ("name", Field::Raw(&fm.name)),
            ("description", Field::Raw(&description)),
        ];
        if let Some(t) = fm.allowed_tools.as_deref().filter(|t| !t.is_empty()) {
            fields.push(("allowed-tools", Field::Raw(t)));
        }
        let mut warnings = Vec::new();
        if fm.activation == Activation::Manual {
            warnings.push("goose-cli: activation 'manual' downgraded to 'agent'".to_string());
        }
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!("{}\n\n{}\n", render_frontmatter(&fields), skill.body),
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        if skill.skill_type == SkillType::Rule {
            return root
                .join(".goose/rules")
                .join(format!("{}.md", skill.name()));
        }
        root.join(".goose/skills")
            .join(skill.name())
            .join("SKILL.md")
    }
}
