//! Agents: AGENT.md parsing, per-client transpilers, sync into the shared lockfile, lint.

pub mod handlers;
pub mod lint;
pub mod manage;
pub mod sync;
pub mod transpilers;

pub use sync::{sync_agents, sync_scoped, AgentSyncOptions};
pub use transpilers::{all_agent_transpilers, AgentTranspiler};

use super::kind::{discover, discover_report, read_document, ContentKind};
use super::schema::{self, Fm};
use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionMode {
    Default,
    Plan,
    FullAuto,
}

impl PermissionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionMode::Default => "default",
            PermissionMode::Plan => "plan",
            PermissionMode::FullAuto => "full-auto",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AgentFrontmatter {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub max_turns: Option<i64>,
    pub mcp_servers: Vec<String>,
    pub skills: Vec<String>,
    pub permission_mode: Option<PermissionMode>,
    pub readonly: bool,
    pub effort: Option<String>,
    pub color: Option<String>,
    pub metadata: Mapping,
    pub license: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Agent {
    pub frontmatter: AgentFrontmatter,
    pub body: String,
    pub source_path: PathBuf,
}

impl Agent {
    pub fn name(&self) -> &str {
        &self.frontmatter.name
    }

    pub fn version(&self) -> Option<String> {
        schema::version_of(&self.frontmatter.metadata)
    }

    /// Stand-in used only to ask a transpiler for an output path.
    pub fn placeholder(name: &str) -> Agent {
        Agent {
            frontmatter: AgentFrontmatter {
                name: name.to_string(),
                description: "dummy".into(),
                model: None,
                tools: Vec::new(),
                disallowed_tools: Vec::new(),
                max_turns: None,
                mcp_servers: Vec::new(),
                skills: Vec::new(),
                permission_mode: None,
                readonly: false,
                effort: None,
                color: None,
                metadata: Mapping::new(),
                license: None,
            },
            body: String::new(),
            source_path: PathBuf::from("dummy"),
        }
    }
}

fn build_frontmatter(fm: &Fm) -> Result<AgentFrontmatter, String> {
    let (name, description) = schema::name_and_description(fm)?;
    let permission_mode = match schema::get(fm, "permission_mode") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(match s.as_str() {
            "default" => PermissionMode::Default,
            "plan" => PermissionMode::Plan,
            "full-auto" => PermissionMode::FullAuto,
            other => return Err(format!("permission_mode: invalid value {other:?}")),
        }),
        Some(_) => return Err("permission_mode: invalid value".into()),
    };
    Ok(AgentFrontmatter {
        name,
        description,
        model: schema::opt_string(fm, "model")?,
        tools: schema::string_list(fm, "tools")?,
        disallowed_tools: schema::string_list(fm, "disallowed_tools")?,
        max_turns: schema::lax_opt_int(fm, "max_turns")?,
        mcp_servers: schema::string_list(fm, "mcp_servers")?,
        skills: schema::string_list(fm, "skills")?,
        permission_mode,
        readonly: schema::lax_bool(fm, "readonly", false)?,
        effort: schema::opt_string(fm, "effort")?,
        color: schema::opt_string(fm, "color")?,
        metadata: schema::metadata(fm)?,
        license: schema::opt_string(fm, "license")?,
    })
}

pub struct AgentKind;

impl ContentKind for AgentKind {
    type Item = Agent;
    const NOUN: &'static str = "Agent";
    const DIRS: &'static [&'static str] = &["agents"];
    const FILE: &'static str = "AGENT.md";

    fn parse(path: &Path) -> Result<Agent, String> {
        parse_agent_file(path)
    }
}

pub fn parse_agent_file(path: &Path) -> Result<Agent, String> {
    let (fm_data, body) = read_document(AgentKind::NOUN, path)?;
    Ok(Agent {
        frontmatter: build_frontmatter(&fm_data)?,
        body,
        source_path: path.to_path_buf(),
    })
}

pub fn discover_agents_report(repo: &Path) -> (Vec<Agent>, Vec<String>) {
    discover_report::<AgentKind>(repo)
}

pub fn discover_agents(repo: &Path) -> Vec<Agent> {
    discover::<AgentKind>(repo)
}
