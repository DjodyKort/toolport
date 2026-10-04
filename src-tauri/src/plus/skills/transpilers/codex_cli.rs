use super::{render_frontmatter, Field};
use crate::plus::skills::parser::{Activation, Skill};
use crate::plus::skills::transpiler::{TranspileResult, Transpiler};
use std::path::{Path, PathBuf};

/// `home` is mcpm's `Path.home()`: when the output root equals it (global mode) skills go to
/// `~/.codex/skills` so `~/.agents/skills`, which Gemini CLI aliases, stays untouched.
pub struct CodexCli {
    home: Option<PathBuf>,
}

impl CodexCli {
    pub fn new(home: Option<PathBuf>) -> Self {
        Self { home }
    }
}

impl Transpiler for CodexCli {
    fn client_key(&self) -> &str {
        "codex-cli"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let fm = &skill.frontmatter;
        let mut fields = vec![
            ("name", Field::Raw(&fm.name)),
            ("description", Field::Quoted(&fm.description)),
        ];
        if let Some(t) = fm.allowed_tools.as_deref().filter(|t| !t.is_empty()) {
            fields.push(("allowed-tools", Field::Text(t)));
        }
        let mut warnings = Vec::new();
        if fm.activation == Activation::Manual {
            warnings.push("codex-cli: activation 'manual' downgraded to 'agent'".to_string());
        }
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!("{}\n\n{}\n", render_frontmatter(&fields), skill.body),
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        let base = if self.home.as_deref() == Some(root) {
            ".codex/skills"
        } else {
            ".agents/skills"
        };
        root.join(base).join(skill.name()).join("SKILL.md")
    }
}
