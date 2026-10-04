use super::Style;
use crate::plus::skills::json::{self, J};
use crate::plus::skills::pyfs::{read_text, title};
use crate::plus::skills::transpiler::TranspileResult;
use crate::plus::skills::transpilers::windsurf::{py_prefix, WINDSURF_WORKSPACE_CHAR_LIMIT};
use std::fs;
use std::path::{Path, PathBuf};

pub const STYLE_BLOCK_START: &str = "<!-- mcpm-style:start -->";
pub const STYLE_BLOCK_END: &str = "<!-- mcpm-style:end -->";
pub const STYLE_SLUG_PREFIX: &str = "style-";
/// Keys whose styles all land in one file.
pub const STYLE_APPEND_MODE: &[&str] = &["roomodes-style"];
pub const OUTPUT_STYLE_RULE: &str = "mcpm-output-style";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// The client has a native style toggle: every style is written and the user picks one.
    Native = 1,
    /// The style is injected as an always-on rule; one active style per client.
    ApplyRemove = 2,
}

pub trait StyleTranspiler {
    fn client_key(&self) -> &str;

    /// The client's name in mcpm's status tables.
    fn display_name(&self) -> &str;

    fn tier(&self) -> Tier;

    fn transpile(&self, style: &Style, root: &Path) -> Result<TranspileResult, String>;

    fn get_output_path(&self, style: &Style, root: &Path) -> PathBuf;

    fn transpile_all(
        &self,
        _styles: &[Style],
        _root: &Path,
    ) -> Option<Result<TranspileResult, String>> {
        None
    }

    /// The paths [`StyleTranspiler::clean`] would remove or rewrite; nothing is touched.
    fn clean_targets(&self, root: &Path, managed: &[String]) -> Vec<PathBuf> {
        let mut targets: Vec<PathBuf> = Vec::new();
        for name in managed {
            let path = self.get_output_path(&Style::placeholder(name), root);
            if path.exists() && !targets.contains(&path) {
                targets.push(path);
            }
        }
        targets
    }

    /// Removes each managed style's output and one now-empty parent directory.
    fn clean(&self, root: &Path, managed: &[String]) -> Result<Vec<PathBuf>, String> {
        let mut removed = Vec::new();
        for name in managed {
            let path = self.get_output_path(&Style::placeholder(name), root);
            if remove_with_empty_parent(&path)? {
                removed.push(path);
            }
        }
        Ok(removed)
    }
}

pub(crate) fn remove_with_empty_parent(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(parent) = path.parent() {
        if parent.is_dir() && fs::read_dir(parent).is_ok_and(|mut r| r.next().is_none()) {
            fs::remove_dir(parent).map_err(|e| e.to_string())?;
        }
    }
    Ok(true)
}

#[derive(Clone, Copy)]
enum Shape {
    /// `description: "Output style: D"` + `alwaysApply: true` frontmatter.
    AlwaysRule,
    /// `name: mcpm-output-style` + `description: "Output style: D"` frontmatter.
    NamedSkill,
    PlainBody,
    AiderHeading,
    AmazonQComment,
    JetBrainsComments,
}

/// The styles that differ from each other only in target path and file shape.
struct Simple {
    key: &'static str,
    display: &'static str,
    path: &'static str,
    shape: Shape,
}

impl StyleTranspiler for Simple {
    fn client_key(&self) -> &str {
        self.key
    }

    fn display_name(&self) -> &str {
        self.display
    }

    fn tier(&self) -> Tier {
        Tier::ApplyRemove
    }

    fn transpile(&self, style: &Style, root: &Path) -> Result<TranspileResult, String> {
        let fm = &style.frontmatter;
        let body = &style.body;
        let content = match self.shape {
            Shape::AlwaysRule => format!(
                "---\ndescription: \"Output style: {}\"\nalwaysApply: true\n---\n\n{body}\n",
                fm.description
            ),
            Shape::NamedSkill => format!(
                "---\nname: {OUTPUT_STYLE_RULE}\ndescription: \"Output style: {}\"\n---\n\n{body}\n",
                fm.description
            ),
            Shape::PlainBody => format!("{body}\n"),
            Shape::AiderHeading => format!("# Output Style: {}\n\n{body}\n", fm.name),
            Shape::AmazonQComment => format!(
                "<!-- mcpm: {OUTPUT_STYLE_RULE} - Output style: {} -->\n\n{body}\n",
                fm.description
            ),
            Shape::JetBrainsComments => format!(
                "<!-- mcpm: name={OUTPUT_STYLE_RULE}, activation=always -->\n<!-- mcpm: description=\"Output style: {}\" -->\n\n{body}\n",
                fm.description
            ),
        };
        Ok(TranspileResult {
            output_path: self.get_output_path(style, root),
            content,
            warnings: Vec::new(),
        })
    }

    fn get_output_path(&self, _style: &Style, root: &Path) -> PathBuf {
        root.join(self.path)
    }
}

pub struct ClaudeCodeStyle;

impl StyleTranspiler for ClaudeCodeStyle {
    fn client_key(&self) -> &str {
        "claude-code"
    }

    fn display_name(&self) -> &str {
        "Claude Code"
    }

    fn tier(&self) -> Tier {
        Tier::Native
    }

    fn transpile(&self, style: &Style, root: &Path) -> Result<TranspileResult, String> {
        let fm = &style.frontmatter;
        Ok(TranspileResult {
            output_path: self.get_output_path(style, root),
            content: format!(
                "---\nname: {}\ndescription: \"{}\"\nkeep-coding-instructions: {}\n---\n\n{}\n",
                fm.name, fm.description, fm.keep_coding_instructions, style.body
            ),
            warnings: Vec::new(),
        })
    }

    fn get_output_path(&self, style: &Style, root: &Path) -> PathBuf {
        root.join(".claude/output-styles")
            .join(format!("{}.md", style.name()))
    }
}

pub struct WindsurfStyle;

impl StyleTranspiler for WindsurfStyle {
    fn client_key(&self) -> &str {
        "windsurf"
    }

    fn display_name(&self) -> &str {
        "Windsurf"
    }

    fn tier(&self) -> Tier {
        Tier::ApplyRemove
    }

    fn transpile(&self, style: &Style, root: &Path) -> Result<TranspileResult, String> {
        let fm = &style.frontmatter;
        let frontmatter = format!(
            "---\ndescription: \"Output style: {}\"\ntrigger: always_on\n---",
            fm.description
        );
        let mut content = format!("{frontmatter}\n\n{}\n", style.body);
        let mut warnings = Vec::new();
        if content.chars().count() > WINDSURF_WORKSPACE_CHAR_LIMIT {
            let keep =
                WINDSURF_WORKSPACE_CHAR_LIMIT as isize - frontmatter.chars().count() as isize - 100;
            let mut body = py_prefix(&style.body, keep);
            body.push_str("\n\n[truncated -- see full style at source]");
            content = format!("{frontmatter}\n\n{body}\n");
            warnings.push(format!(
                "windsurf: style body truncated from {} to fit {WINDSURF_WORKSPACE_CHAR_LIMIT} char limit",
                style.body.chars().count()
            ));
        }
        Ok(TranspileResult {
            output_path: self.get_output_path(style, root),
            content,
            warnings,
        })
    }

    fn get_output_path(&self, _style: &Style, root: &Path) -> PathBuf {
        root.join(".windsurf/rules/mcpm-output-style.md")
    }
}

fn inject_style_block(existing: &str, block: &str) -> String {
    let managed = format!("{STYLE_BLOCK_START}\n{block}\n{STYLE_BLOCK_END}");
    if let (Some(start), Some(end)) = (
        existing.find(STYLE_BLOCK_START),
        existing.find(STYLE_BLOCK_END),
    ) {
        let before = existing[..start].trim_end();
        let after = existing[end + STYLE_BLOCK_END.len()..].trim_start();
        let mut parts = vec![before, managed.as_str()];
        if !after.is_empty() {
            parts.push(after);
        }
        return format!("{}\n", parts.join("\n\n"));
    }
    if !existing.trim().is_empty() {
        return format!("{}\n\n{managed}\n", existing.trim_end());
    }
    format!("{managed}\n")
}

fn remove_style_block(content: &str) -> String {
    let (Some(start), Some(end)) = (
        content.find(STYLE_BLOCK_START),
        content.find(STYLE_BLOCK_END),
    ) else {
        return content.to_string();
    };
    let before = content[..start].trim_end();
    let after = content[end + STYLE_BLOCK_END.len()..].trim_start();
    format!("{before}\n\n{after}").trim().to_string()
}

pub struct ZedStyle;

impl StyleTranspiler for ZedStyle {
    fn client_key(&self) -> &str {
        "zed"
    }

    fn display_name(&self) -> &str {
        "Zed"
    }

    fn tier(&self) -> Tier {
        Tier::ApplyRemove
    }

    fn transpile(&self, style: &Style, root: &Path) -> Result<TranspileResult, String> {
        let path = root.join(".rules");
        let existing = if path.exists() {
            read_text(&path)?
        } else {
            String::new()
        };
        let section = format!("## Output Style: {}\n\n{}", style.name(), style.body);
        Ok(TranspileResult {
            content: inject_style_block(&existing, &section),
            output_path: path,
            warnings: Vec::new(),
        })
    }

    fn get_output_path(&self, _style: &Style, root: &Path) -> PathBuf {
        root.join(".rules")
    }

    fn clean_targets(&self, root: &Path, _managed: &[String]) -> Vec<PathBuf> {
        let path = root.join(".rules");
        let managed = read_text(&path).is_ok_and(|text| text.contains(STYLE_BLOCK_START));
        if managed {
            vec![path]
        } else {
            Vec::new()
        }
    }

    fn clean(&self, root: &Path, _managed: &[String]) -> Result<Vec<PathBuf>, String> {
        let path = root.join(".rules");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = read_text(&path)?;
        if !content.contains(STYLE_BLOCK_START) {
            return Ok(Vec::new());
        }
        let cleaned = remove_style_block(&content);
        if cleaned.is_empty() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        } else {
            fs::write(&path, format!("{cleaned}\n")).map_err(|e| e.to_string())?;
        }
        Ok(vec![path])
    }
}

pub struct RooCodeStyle;

fn style_mode(style: &Style) -> J {
    let fm = &style.frontmatter;
    let group = |name: &str, options: J| J::Arr(vec![J::str(name), options]);
    J::Obj(vec![
        (
            "slug".into(),
            J::str(format!("{STYLE_SLUG_PREFIX}{}", fm.name)),
        ),
        ("name".into(), J::str(title(&fm.name.replace('-', " ")))),
        ("roleDefinition".into(), J::str(&fm.description)),
        ("customInstructions".into(), J::str(&style.body)),
        (
            "groups".into(),
            J::Arr(vec![
                group("read", J::Obj(Vec::new())),
                group("edit", J::Obj(vec![("fileRegex".into(), J::str(".*"))])),
                group("command", J::Obj(Vec::new())),
                group("mcp", J::Obj(Vec::new())),
            ]),
        ),
    ])
}

fn is_style_slug(mode: &J) -> Result<bool, ()> {
    match mode {
        J::Obj(_) => match mode.get("slug") {
            None => Ok(false),
            Some(J::Str(s)) => Ok(s.starts_with(STYLE_SLUG_PREFIX)),
            Some(_) => Err(()),
        },
        _ => Err(()),
    }
}

impl StyleTranspiler for RooCodeStyle {
    fn client_key(&self) -> &str {
        "roomodes-style"
    }

    fn display_name(&self) -> &str {
        "Roo Code"
    }

    fn tier(&self) -> Tier {
        Tier::Native
    }

    fn transpile(&self, style: &Style, root: &Path) -> Result<TranspileResult, String> {
        Ok(TranspileResult {
            output_path: self.get_output_path(style, root),
            content: style_mode(style).dumps(),
            warnings: Vec::new(),
        })
    }

    /// Keeps non-style modes already in `.roomodes`; unreadable or malformed files count as empty.
    fn transpile_all(
        &self,
        styles: &[Style],
        root: &Path,
    ) -> Option<Result<TranspileResult, String>> {
        let path = root.join(".roomodes");
        let mut modes: Vec<J> = Vec::new();
        if path.exists() {
            let parsed = read_text(&path).ok().and_then(|t| json::parse(&t).ok());
            if let Some(doc) = parsed {
                if let Some(J::Arr(existing)) = doc.get("customModes") {
                    for mode in existing {
                        match is_style_slug(mode) {
                            Ok(false) => modes.push(mode.clone()),
                            Ok(true) => {}
                            Err(()) => break,
                        }
                    }
                }
            }
        }
        modes.extend(styles.iter().map(style_mode));
        let doc = J::Obj(vec![("customModes".into(), J::Arr(modes))]);
        Some(Ok(TranspileResult {
            output_path: path,
            content: format!("{}\n", doc.dumps()),
            warnings: Vec::new(),
        }))
    }

    fn get_output_path(&self, _style: &Style, root: &Path) -> PathBuf {
        root.join(".roomodes")
    }

    fn clean_targets(&self, root: &Path, _managed: &[String]) -> Vec<PathBuf> {
        match roo_clean_plan(root) {
            RooClean::Nothing => Vec::new(),
            RooClean::Rewrite(_) | RooClean::Remove => vec![root.join(".roomodes")],
        }
    }

    /// Removes only `style-` modes and keeps the rest of the file.
    fn clean(&self, root: &Path, _managed: &[String]) -> Result<Vec<PathBuf>, String> {
        let path = root.join(".roomodes");
        match roo_clean_plan(root) {
            RooClean::Nothing => return Ok(Vec::new()),
            RooClean::Rewrite(doc) => fs::write(&path, format!("{}\n", doc.dumps()))
                .map_err(|e| format!("{}: {e}", path.display()))?,
            RooClean::Remove => fs::remove_file(&path).map_err(|e| e.to_string())?,
        }
        Ok(vec![path])
    }
}

enum RooClean {
    Nothing,
    Rewrite(J),
    Remove,
}

fn roo_clean_plan(root: &Path) -> RooClean {
    let path = root.join(".roomodes");
    if !path.exists() {
        return RooClean::Nothing;
    }
    let Some(J::Obj(mut doc)) = read_text(&path).ok().and_then(|t| json::parse(&t).ok()) else {
        return RooClean::Nothing;
    };
    let modes = match doc.iter().find(|(k, _)| k == "customModes") {
        Some((_, J::Arr(m))) => m.clone(),
        None => Vec::new(),
        Some(_) => return RooClean::Nothing,
    };
    let mut filtered = Vec::new();
    for mode in &modes {
        match is_style_slug(mode) {
            Ok(true) => {}
            Ok(false) => filtered.push(mode.clone()),
            Err(()) => return RooClean::Nothing,
        }
    }
    if !filtered.is_empty() {
        match doc.iter_mut().find(|(k, _)| k == "customModes") {
            Some(slot) => slot.1 = J::Arr(filtered),
            None => doc.push(("customModes".into(), J::Arr(filtered))),
        }
        RooClean::Rewrite(J::Obj(doc))
    } else if modes.is_empty() {
        RooClean::Nothing
    } else {
        RooClean::Remove
    }
}

fn simple(
    key: &'static str,
    display: &'static str,
    path: &'static str,
    shape: Shape,
) -> Box<dyn StyleTranspiler> {
    Box::new(Simple {
        key,
        display,
        path,
        shape,
    })
}

/// mcpm's import order in `styles/transpilers/__init__.py`, which fixes iteration order.
pub fn all_style_transpilers() -> Vec<Box<dyn StyleTranspiler>> {
    vec![
        simple(
            "aider",
            "Aider",
            ".mcpm/skills/mcpm-output-style/SKILL.md",
            Shape::AiderHeading,
        ),
        simple(
            "amazon-q",
            "Amazon Q",
            ".amazonq/rules/mcpm-output-style.md",
            Shape::AmazonQComment,
        ),
        Box::new(ClaudeCodeStyle),
        simple(
            "cline",
            "Cline",
            ".clinerules/mcpm-output-style.md",
            Shape::PlainBody,
        ),
        simple(
            "codex-cli",
            "Codex CLI",
            ".agents/skills/mcpm-output-style/SKILL.md",
            Shape::NamedSkill,
        ),
        simple(
            "continue",
            "Continue.dev",
            ".continue/rules/mcpm-output-style.md",
            Shape::AlwaysRule,
        ),
        simple(
            "cursor",
            "Cursor",
            ".cursor/rules/mcpm-output-style/RULE.md",
            Shape::AlwaysRule,
        ),
        simple(
            "gemini-cli",
            "Gemini CLI",
            ".gemini/skills/mcpm-output-style/SKILL.md",
            Shape::NamedSkill,
        ),
        simple(
            "goose",
            "Goose",
            ".goose/rules/mcpm-output-style.md",
            Shape::NamedSkill,
        ),
        simple(
            "jetbrains",
            "JetBrains AI",
            ".aiassistant/rules/mcpm-output-style.md",
            Shape::JetBrainsComments,
        ),
        Box::new(RooCodeStyle),
        simple(
            "trae",
            "Trae",
            ".trae/rules/mcpm-output-style.md",
            Shape::AlwaysRule,
        ),
        simple(
            "vscode-copilot",
            "VS Code Copilot",
            ".github/instructions/mcpm-output-style.instructions.md",
            Shape::PlainBody,
        ),
        Box::new(WindsurfStyle),
        Box::new(ZedStyle),
    ]
}
