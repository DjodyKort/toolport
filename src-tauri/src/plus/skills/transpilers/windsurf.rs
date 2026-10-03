use super::{render_frontmatter, Field};
use crate::plus::skills::parser::{Activation, Skill};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

pub const WINDSURF_GLOBAL_CHAR_LIMIT: usize = 6000;
pub const WINDSURF_WORKSPACE_CHAR_LIMIT: usize = 12000;

pub struct Windsurf;

/// Python `s[:n]` on code points, including its negative-index behaviour.
pub(crate) fn py_prefix(s: &str, n: isize) -> String {
    let total = s.chars().count() as isize;
    let take = if n < 0 {
        (total + n).max(0)
    } else {
        n.min(total)
    };
    s.chars().take(take as usize).collect()
}

impl Transpiler for Windsurf {
    fn client_key(&self) -> &str {
        "windsurf"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let globs = fm.globs.as_deref().filter(|g| !g.is_empty());
        let description = format!("\"{}\"", fm.description);
        let mut fields = Vec::new();
        if !fm.description.is_empty() {
            fields.push(("description", Field::Raw(&description)));
        }
        if let Some(g) = globs {
            fields.push(("globs", Field::Raw(g)));
        }
        let trigger = match fm.activation {
            Activation::Always => "always_on",
            Activation::Auto if globs.is_some() => "glob",
            Activation::Auto => "model_decision",
            Activation::Agent => "model_decision",
            Activation::Manual => "manual",
        };
        fields.push(("trigger", Field::Raw(trigger)));

        let frontmatter = render_frontmatter(&fields);
        let mut content = format!("{frontmatter}\n\n{}\n", skill.body);
        let mut warnings = Vec::new();
        if content.chars().count() > WINDSURF_WORKSPACE_CHAR_LIMIT {
            let keep =
                WINDSURF_WORKSPACE_CHAR_LIMIT as isize - frontmatter.chars().count() as isize - 100;
            let mut body = py_prefix(&skill.body, keep);
            body.push_str("\n\n[truncated -- see full skill at source]");
            content = format!("{frontmatter}\n\n{body}\n");
            warnings.push(format!(
                "windsurf: body truncated from {} to fit {WINDSURF_WORKSPACE_CHAR_LIMIT} char limit",
                skill.body.chars().count()
            ));
        }
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content,
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        root.join(".windsurf/rules")
            .join(format!("{}.md", skill.name()))
    }
}
