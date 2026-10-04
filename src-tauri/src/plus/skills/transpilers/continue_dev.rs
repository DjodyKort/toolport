use super::{render_frontmatter, Field};
use crate::plus::skills::parser::{Activation, Skill};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

pub struct ContinueDev;

impl Transpiler for ContinueDev {
    fn client_key(&self) -> &str {
        "continue"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let priority = fm.priority.to_string();
        let mut fields = Vec::new();
        if !fm.description.is_empty() {
            fields.push(("description", Field::Quoted(&fm.description)));
        }
        if let Some(g) = fm.globs.as_deref().filter(|g| !g.is_empty()) {
            fields.push(("globs", Field::Loose(g)));
        }
        if fm.priority != 0 {
            fields.push(("priority", Field::Raw(&priority)));
        }
        let mut warnings = Vec::new();
        match fm.activation {
            Activation::Always => fields.push(("alwaysApply", Field::Bool(true))),
            Activation::Agent | Activation::Manual => warnings.push(format!(
                "continue: activation '{}' downgraded to 'auto'",
                fm.activation.as_str()
            )),
            Activation::Auto => {}
        }
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!("{}\n\n{}\n", render_frontmatter(&fields), skill.body),
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        root.join(".continue/rules")
            .join(format!("{}.md", skill.name()))
    }
}
