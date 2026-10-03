use crate::plus::skills::parser::Skill;
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

pub struct JetBrains;

impl Transpiler for JetBrains {
    fn client_key(&self) -> &str {
        "jetbrains"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let mut header = format!(
            "<!-- mcpm: name={}, activation={}",
            fm.name,
            fm.activation.as_str()
        );
        if let Some(g) = fm.globs.as_deref().filter(|g| !g.is_empty()) {
            header.push_str(&format!(", globs={g}"));
        }
        if !fm.description.is_empty() {
            header.push_str(&format!(
                " -->\n<!-- mcpm: description=\"{}\"",
                fm.description
            ));
        }
        header.push_str(" -->");
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!("{header}\n\n{}\n", skill.body),
            warnings: Vec::new(),
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        root.join(".aiassistant/rules")
            .join(format!("{}.md", skill.name()))
    }
}
