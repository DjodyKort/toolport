use super::Agent;
use crate::plus::skills::json::J;
use crate::plus::skills::pyfs::title;
use crate::plus::skills::transpiler::TranspileResult;
use std::fs;
use std::path::{Path, PathBuf};

/// Keys of transpilers that write one combined file for all agents.
pub const AGENT_APPEND_MODE: &[&str] = &["roomodes"];
/// Output paths that only make sense inside a project; skipped in global mode.
pub const AGENT_PROJECT_ONLY: &[&str] = &["vscode", "roomodes"];

pub trait AgentTranspiler {
    fn client_key(&self) -> &str;

    fn transpile(&self, agent: &Agent, root: &Path) -> Result<TranspileResult, String>;

    fn get_output_path(&self, agent: &Agent, root: &Path) -> PathBuf;

    fn transpile_all(
        &self,
        _agents: &[Agent],
        _root: &Path,
    ) -> Option<Result<TranspileResult, String>> {
        None
    }

    /// Removes each managed agent's output and one now-empty parent directory.
    fn clean(&self, root: &Path, managed: &[String]) -> Result<Vec<PathBuf>, String> {
        let mut removed = Vec::new();
        for name in managed {
            let path = self.get_output_path(&Agent::placeholder(name), root);
            if !path.exists() {
                continue;
            }
            fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if let Some(parent) = path.parent() {
                if parent.is_dir() && fs::read_dir(parent).is_ok_and(|mut r| r.next().is_none()) {
                    fs::remove_dir(parent).map_err(|e| e.to_string())?;
                }
            }
            removed.push(path);
        }
        Ok(removed)
    }
}

enum F {
    Raw(String),
    Bool(bool),
    List(Vec<String>),
}

/// `BaseAgentTranspiler._render_frontmatter`: lists render as `[a, b]` and are skipped when empty.
fn render(fields: Vec<(&str, F)>) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let mut lines = vec!["---".to_string()];
    for (key, value) in fields {
        match value {
            F::Bool(b) => lines.push(format!("{key}: {b}")),
            F::List(items) if !items.is_empty() => {
                lines.push(format!("{key}: [{}]", items.join(", ")))
            }
            F::List(_) => {}
            F::Raw(v) => lines.push(format!("{key}: {v}")),
        }
    }
    lines.push("---".into());
    lines.join("\n")
}

fn raw(s: &str) -> F {
    F::Raw(s.to_string())
}

fn quoted(s: &str) -> F {
    F::Raw(format!("\"{s}\""))
}

fn result(
    path: PathBuf,
    frontmatter: String,
    body: &str,
    warnings: Vec<String>,
) -> Result<TranspileResult, String> {
    Ok(TranspileResult {
        output_path: path,
        content: format!("{frontmatter}\n\n{body}\n"),
        warnings,
    })
}

pub struct ClaudeCodeAgent;

impl AgentTranspiler for ClaudeCodeAgent {
    fn client_key(&self) -> &str {
        "claude-code"
    }

    fn transpile(&self, agent: &Agent, root: &Path) -> Result<TranspileResult, String> {
        let fm = &agent.frontmatter;
        let mut fields = vec![
            ("name", raw(&fm.name)),
            ("description", quoted(&fm.description)),
        ];
        if let Some(m) = fm.model.as_deref().filter(|m| !m.is_empty()) {
            fields.push(("model", raw(m)));
        }
        fields.push(("tools", F::List(fm.tools.clone())));
        fields.push(("disallowedTools", F::List(fm.disallowed_tools.clone())));
        if let Some(n) = fm.max_turns.filter(|n| *n != 0) {
            fields.push(("maxTurns", F::Raw(n.to_string())));
        }
        fields.push(("mcpServers", F::List(fm.mcp_servers.clone())));
        fields.push(("skills", F::List(fm.skills.clone())));
        if let Some(mode) = fm.permission_mode {
            fields.push(("permissionMode", raw(mode.as_str())));
        } else if fm.readonly {
            fields.push(("permissionMode", raw("plan")));
        }
        if let Some(e) = fm.effort.as_deref().filter(|e| !e.is_empty()) {
            fields.push(("effort", raw(e)));
        }
        if let Some(c) = fm.color.as_deref().filter(|c| !c.is_empty()) {
            fields.push(("color", raw(c)));
        }
        result(
            self.get_output_path(agent, root),
            render(fields),
            &agent.body,
            Vec::new(),
        )
    }

    fn get_output_path(&self, agent: &Agent, root: &Path) -> PathBuf {
        root.join(".claude/agents")
            .join(format!("{}.md", agent.name()))
    }
}

pub struct CodexCliAgent;

fn sandbox_for(mode: &str) -> &'static str {
    match mode {
        "plan" => "sandbox",
        "full-auto" => "full-auto",
        _ => "lenient",
    }
}

impl AgentTranspiler for CodexCliAgent {
    fn client_key(&self) -> &str {
        "codex-cli"
    }

    /// `name` and `description` are interpolated without escaping, exactly like mcpm, so a quote
    /// or backslash in the description yields invalid TOML; only the body is escaped.
    fn transpile(&self, agent: &Agent, root: &Path) -> Result<TranspileResult, String> {
        let fm = &agent.frontmatter;
        let mut lines = vec![
            format!("name = \"{}\"", fm.name),
            format!("description = \"{}\"", fm.description),
        ];
        let escaped = agent
            .body
            .replace('\\', "\\\\")
            .replace("\"\"\"", "\\\"\\\"\\\"");
        lines.push(format!(
            "developer_instructions = \"\"\"\n{escaped}\n\"\"\""
        ));
        if let Some(m) = fm.model.as_deref().filter(|m| !m.is_empty()) {
            lines.push(format!("model = \"{m}\""));
        }
        if let Some(e) = fm.effort.as_deref().filter(|e| !e.is_empty()) {
            lines.push(format!("model_reasoning_effort = \"{e}\""));
        }
        let mut sandbox = fm.permission_mode.map(|m| sandbox_for(m.as_str()));
        if fm.readonly {
            sandbox = Some("sandbox");
        }
        if let Some(s) = sandbox {
            lines.push(format!("sandbox_mode = \"{s}\""));
        }
        if !fm.mcp_servers.is_empty() {
            lines.push(String::new());
            lines.push("[mcp_servers]".into());
            for server in &fm.mcp_servers {
                lines.push(format!("[mcp_servers.\"{server}\"]"));
                lines.push(format!(
                    "# Configure via toolportctl: toolportctl server install {server}"
                ));
            }
        }
        let mut warnings = Vec::new();
        if !fm.tools.is_empty() {
            warnings.push("codex-cli: 'tools' field not supported in agent TOML, dropped".into());
        }
        Ok(TranspileResult {
            output_path: self.get_output_path(agent, root),
            content: format!("{}\n", lines.join("\n")),
            warnings,
        })
    }

    fn get_output_path(&self, agent: &Agent, root: &Path) -> PathBuf {
        root.join(".codex/agents")
            .join(format!("{}.toml", agent.name()))
    }
}

pub struct CursorAgent;

impl AgentTranspiler for CursorAgent {
    fn client_key(&self) -> &str {
        "cursor"
    }

    fn transpile(&self, agent: &Agent, root: &Path) -> Result<TranspileResult, String> {
        let fm = &agent.frontmatter;
        let mut fields = vec![
            ("name", raw(&fm.name)),
            ("description", quoted(&fm.description)),
        ];
        if let Some(m) = fm.model.as_deref().filter(|m| !m.is_empty()) {
            fields.push(("model", raw(m)));
        }
        if fm.readonly {
            fields.push(("readonly", F::Bool(true)));
        }
        let mut warnings = Vec::new();
        if !fm.tools.is_empty() {
            warnings.push("cursor: 'tools' field not supported, dropped".to_string());
        }
        if !fm.mcp_servers.is_empty() {
            warnings.push("cursor: 'mcp-servers' field not supported, dropped".to_string());
        }
        if fm.max_turns.is_some_and(|n| n != 0) {
            warnings.push("cursor: 'max-turns' field not supported, dropped".to_string());
        }
        result(
            self.get_output_path(agent, root),
            render(fields),
            &agent.body,
            warnings,
        )
    }

    fn get_output_path(&self, agent: &Agent, root: &Path) -> PathBuf {
        root.join(".cursor/agents")
            .join(format!("{}.md", agent.name()))
    }
}

pub struct GeminiCliAgent;

impl AgentTranspiler for GeminiCliAgent {
    fn client_key(&self) -> &str {
        "gemini-cli"
    }

    fn transpile(&self, agent: &Agent, root: &Path) -> Result<TranspileResult, String> {
        let fm = &agent.frontmatter;
        let mut fields = vec![
            ("name", raw(&fm.name)),
            ("description", quoted(&fm.description)),
        ];
        if let Some(m) = fm.model.as_deref().filter(|m| !m.is_empty()) {
            fields.push(("model", raw(m)));
        }
        fields.push(("tools", F::List(fm.tools.clone())));
        if let Some(n) = fm.max_turns.filter(|n| *n != 0) {
            fields.push(("max_turns", F::Raw(n.to_string())));
        }
        fields.push(("mcpServers", F::List(fm.mcp_servers.clone())));
        let mut warnings = Vec::new();
        if !fm.disallowed_tools.is_empty() {
            warnings.push("gemini-cli: 'disallowed-tools' not supported, dropped".to_string());
        }
        if fm.permission_mode.is_some() {
            warnings.push("gemini-cli: 'permission-mode' not supported, dropped".to_string());
        }
        result(
            self.get_output_path(agent, root),
            render(fields),
            &agent.body,
            warnings,
        )
    }

    fn get_output_path(&self, agent: &Agent, root: &Path) -> PathBuf {
        root.join(".gemini/agents")
            .join(format!("{}.md", agent.name()))
    }
}

pub struct VsCodeAgent;

impl AgentTranspiler for VsCodeAgent {
    fn client_key(&self) -> &str {
        "vscode"
    }

    fn transpile(&self, agent: &Agent, root: &Path) -> Result<TranspileResult, String> {
        let fm = &agent.frontmatter;
        let mut fields = vec![
            ("name", raw(&fm.name)),
            ("description", quoted(&fm.description)),
        ];
        if let Some(m) = fm.model.as_deref().filter(|m| !m.is_empty()) {
            fields.push(("model", raw(m)));
        }
        fields.push(("tools", F::List(fm.tools.clone())));
        fields.push(("mcp-servers", F::List(fm.mcp_servers.clone())));
        let mut warnings = Vec::new();
        if !fm.disallowed_tools.is_empty() {
            warnings.push("vscode: 'disallowed-tools' not supported, dropped".to_string());
        }
        if fm.max_turns.is_some_and(|n| n != 0) {
            warnings.push("vscode: 'max-turns' not supported, dropped".to_string());
        }
        if fm.permission_mode.is_some() {
            warnings.push("vscode: 'permission-mode' not supported, dropped".to_string());
        }
        result(
            self.get_output_path(agent, root),
            render(fields),
            &agent.body,
            warnings,
        )
    }

    fn get_output_path(&self, agent: &Agent, root: &Path) -> PathBuf {
        root.join(".github/agents")
            .join(format!("{}.agent.md", agent.name()))
    }
}

fn tool_group(tool: &str) -> Option<&'static str> {
    match tool {
        "Read" | "Glob" | "Grep" => Some("read"),
        "Write" | "Edit" => Some("edit"),
        "Bash" => Some("command"),
        "Browser" => Some("browser"),
        _ => None,
    }
}

fn group_entry(name: &str) -> J {
    let options = if name == "edit" {
        J::Obj(vec![("fileRegex".into(), J::str(".*"))])
    } else {
        J::Obj(Vec::new())
    };
    J::Arr(vec![J::str(name), options])
}

fn groups_for(tools: &[String]) -> J {
    let mut names: Vec<&str> = tools.iter().filter_map(|t| tool_group(t)).collect();
    names.sort_unstable();
    names.dedup();
    J::Arr(names.into_iter().map(group_entry).collect())
}

pub(crate) fn roo_mode(agent: &Agent) -> (J, Vec<String>) {
    let fm = &agent.frontmatter;
    let groups = if fm.tools.is_empty() {
        J::Arr(vec![
            group_entry("read"),
            group_entry("edit"),
            group_entry("command"),
            group_entry("mcp"),
        ])
    } else {
        groups_for(&fm.tools)
    };
    let mode = J::Obj(vec![
        ("slug".into(), J::str(&fm.name)),
        ("name".into(), J::str(title(&fm.name.replace('-', " ")))),
        ("roleDefinition".into(), J::str(&fm.description)),
        ("customInstructions".into(), J::str(&agent.body)),
        ("groups".into(), groups),
    ]);
    let mut warnings = Vec::new();
    if fm.model.as_deref().is_some_and(|m| !m.is_empty()) {
        warnings.push(
            "roomodes: 'model' not directly supported, included in customInstructions".to_string(),
        );
    }
    (mode, warnings)
}

/// `.roomodes` is rewritten from scratch: existing user modes are lost (parity with mcpm).
pub struct RooCodeAgent;

impl AgentTranspiler for RooCodeAgent {
    fn client_key(&self) -> &str {
        "roomodes"
    }

    fn transpile(&self, agent: &Agent, root: &Path) -> Result<TranspileResult, String> {
        let (mode, warnings) = roo_mode(agent);
        Ok(TranspileResult {
            output_path: self.get_output_path(agent, root),
            content: mode.dumps(),
            warnings,
        })
    }

    fn transpile_all(
        &self,
        agents: &[Agent],
        root: &Path,
    ) -> Option<Result<TranspileResult, String>> {
        let mut warnings = Vec::new();
        let mut modes = Vec::new();
        for agent in agents {
            let (mode, w) = roo_mode(agent);
            modes.push(mode);
            warnings.extend(w);
        }
        let doc = J::Obj(vec![("customModes".into(), J::Arr(modes))]);
        Some(Ok(TranspileResult {
            output_path: root.join(".roomodes"),
            content: format!("{}\n", doc.dumps()),
            warnings,
        }))
    }

    fn get_output_path(&self, _agent: &Agent, root: &Path) -> PathBuf {
        root.join(".roomodes")
    }

    fn clean(&self, root: &Path, _managed: &[String]) -> Result<Vec<PathBuf>, String> {
        let path = root.join(".roomodes");
        if !path.exists() {
            return Ok(Vec::new());
        }
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        Ok(vec![path])
    }
}

/// mcpm's import order in `agents/transpilers/__init__.py`, which fixes lock output order.
pub fn all_agent_transpilers() -> Vec<Box<dyn AgentTranspiler>> {
    vec![
        Box::new(ClaudeCodeAgent),
        Box::new(CodexCliAgent),
        Box::new(CursorAgent),
        Box::new(GeminiCliAgent),
        Box::new(RooCodeAgent),
        Box::new(VsCodeAgent),
    ]
}
