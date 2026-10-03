use crate::plus::skills::parser::{Activation, Skill};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

pub struct Aider;

impl Transpiler for Aider {
    fn client_key(&self) -> &str {
        "aider"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let mut warnings = Vec::new();
        if fm.activation != Activation::Always {
            warnings.push(format!(
                "aider: activation '{}' downgraded to 'always'",
                fm.activation.as_str()
            ));
        }
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!("# {}\n\n{}\n", fm.name, skill.body),
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        root.join(".mcpm/skills")
            .join(skill.name())
            .join("SKILL.md")
    }
}
