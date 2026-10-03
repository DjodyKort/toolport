use super::{render_frontmatter, Field};
use crate::plus::skills::parser::{Activation, Skill};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

pub struct Trae;

impl Transpiler for Trae {
    fn client_key(&self) -> &str {
        "trae"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let description = format!("\"{}\"", fm.description);
        let mut fields = Vec::new();
        if !fm.description.is_empty() {
            fields.push(("description", Field::Raw(&description)));
        }
        if let Some(g) = fm.globs.as_deref().filter(|g| !g.is_empty()) {
            fields.push(("globs", Field::Raw(g)));
        }
        let mut warnings = Vec::new();
        if matches!(fm.activation, Activation::Agent | Activation::Manual) {
            warnings.push(format!(
                "trae: activation '{}' downgraded to 'auto'",
                fm.activation.as_str()
            ));
        }
        if fm.activation == Activation::Always {
            fields.push(("alwaysApply", Field::Bool(true)));
        }
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!("{}\n\n{}\n", render_frontmatter(&fields), skill.body),
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        root.join(".trae/rules")
            .join(format!("{}.md", skill.name()))
    }
}
