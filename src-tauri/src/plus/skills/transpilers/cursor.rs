use super::{render_frontmatter, Field};
use crate::plus::skills::parser::{Activation, Skill};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

pub struct Cursor;

impl Transpiler for Cursor {
    fn client_key(&self) -> &str {
        "cursor"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let description = format!("\"{}\"", fm.description);
        let mut fields = Vec::new();
        if !fm.description.is_empty() {
            fields.push(("description", Field::Raw(&description)));
        }
        if let Some(g) = fm.globs.as_deref().filter(|g| !g.is_empty()) {
            if matches!(fm.activation, Activation::Always | Activation::Auto) {
                fields.push(("globs", Field::Raw(g)));
            }
        }
        if fm.activation == Activation::Always {
            fields.push(("alwaysApply", Field::Bool(true)));
        }
        let content = format!("{}\n\n{}\n", render_frontmatter(&fields), skill.body);
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content,
            warnings: Vec::new(),
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        root.join(".cursor/rules")
            .join(skill.name())
            .join("RULE.md")
    }

    fn get_collision_paths(&self, skill: &Skill, root: &Path) -> Vec<PathBuf> {
        let n = skill.name();
        vec![
            root.join(".cursor/rules").join(format!("{n}.md")),
            root.join(".cursor/rules").join(format!("{n}.mdc")),
        ]
    }
}
