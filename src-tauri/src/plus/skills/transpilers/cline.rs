use super::{render_frontmatter, Field};
use crate::plus::skills::parser::{Activation, Skill};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

pub struct Cline;

impl Transpiler for Cline {
    fn client_key(&self) -> &str {
        "cline"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let globs = fm.globs.as_deref().filter(|g| !g.is_empty());
        let mut fields = Vec::new();
        if let Some(g) = globs {
            fields.push(("paths", Field::Raw(g)));
        }
        let mut warnings = Vec::new();
        if matches!(fm.activation, Activation::Agent | Activation::Manual) {
            let target = if globs.is_some() { "auto" } else { "always" };
            let suffix = if globs.is_some() {
                " (paths-based)"
            } else {
                ""
            };
            warnings.push(format!(
                "cline: activation '{}' downgraded to '{target}'{suffix}",
                fm.activation.as_str()
            ));
        }
        let frontmatter = render_frontmatter(&fields);
        let content = if frontmatter.is_empty() {
            format!("{}\n", skill.body)
        } else {
            format!("{frontmatter}\n\n{}\n", skill.body)
        };
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content,
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        root.join(".clinerules")
            .join(format!("{}.md", skill.name()))
    }

    fn get_collision_paths(&self, skill: &Skill, root: &Path) -> Vec<PathBuf> {
        vec![root.join(".clinerules").join(skill.name())]
    }
}
