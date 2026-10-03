use crate::plus::skills::parser::{Activation, Skill};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

pub struct AmazonQ;

impl Transpiler for AmazonQ {
    fn client_key(&self) -> &str {
        "amazon-q"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let mut warnings = Vec::new();
        if fm.activation != Activation::Always {
            warnings.push(format!(
                "amazon-q: activation '{}' downgraded to 'always' (no frontmatter support)",
                fm.activation.as_str()
            ));
        }
        let header = format!("<!-- mcpm: {} - {} -->", fm.name, fm.description);
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!("{header}\n\n{}\n", skill.body),
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        root.join(".amazonq/rules")
            .join(format!("{}.md", skill.name()))
    }
}
