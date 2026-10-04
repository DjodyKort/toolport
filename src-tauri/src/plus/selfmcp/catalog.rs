use super::{servers, ToolError};
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    None,
    Always,
    UnlessDryRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Str,
    Bool,
    Obj,
    StrList,
}

pub struct Param {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
    pub desc: &'static str,
    pub default: Option<bool>,
}

pub type Runner = fn(&Value) -> Result<Value, ToolError>;

pub struct ToolDef {
    pub name: &'static str,
    pub tier: u8,
    pub gate: Gate,
    pub description: &'static str,
    pub params: &'static [Param],
    pub run: Option<Runner>,
}

pub struct ResourceDef {
    pub uri: &'static str,
    pub name: &'static str,
    pub mime: &'static str,
    pub description: &'static str,
}

const fn p(name: &'static str, ty: Ty, required: bool, desc: &'static str) -> Param {
    Param {
        name,
        ty,
        required,
        desc,
        default: None,
    }
}

const fn on_by_default(param: Param) -> Param {
    Param {
        default: Some(true),
        ..param
    }
}

const REPO: Param = p("repo_path", Ty::Str, false, "Skills repository root");
const NAME: Param = p("name", Ty::Str, true, "Entity or server name");
const CLIENTS: Param = p("client_keys", Ty::StrList, false, "Client keys to target");
const DRY: Param = p("dry_run", Ty::Bool, false, "Report without writing");
const BODY: Param = p("new_body", Ty::Str, true, "Replacement body");
const DRY_ON: Param = on_by_default(p(
    "dry_run",
    Ty::Bool,
    false,
    "Report without writing; true unless you pass false to apply",
));
const COMMIT: Param = p("commit_message", Ty::Str, true, "Commit message");

macro_rules! tool {
    (@def $name:literal, $tier:literal, $gate:ident, $desc:literal, [$($param:expr),*], $run:expr) => {
        ToolDef {
            name: $name,
            tier: $tier,
            gate: Gate::$gate,
            description: $desc,
            params: &[$($param),*],
            run: $run,
        }
    };
    ($name:literal, $tier:literal, $gate:ident, $desc:literal, [$($param:expr),* $(,)?]) => {
        tool!(@def $name, $tier, $gate, $desc, [$($param),*], None)
    };
    ($name:literal, $tier:literal, $gate:ident, $desc:literal, [$($param:expr),* $(,)?], $run:path) => {
        tool!(@def $name, $tier, $gate, $desc, [$($param),*], Some($run))
    };
}

pub const TOOLS: &[ToolDef] = &[
    tool!(
        "skills_list",
        1,
        None,
        "List skills and rules in the repository",
        [REPO]
    ),
    tool!(
        "skills_get",
        1,
        None,
        "Read one skill including its body",
        [NAME, REPO]
    ),
    tool!(
        "skills_lint",
        1,
        None,
        "Lint skills",
        [
            REPO,
            p("names", Ty::StrList, false, "Limit to these skills")
        ]
    ),
    tool!(
        "skills_status",
        1,
        None,
        "Drift between repository and client outputs",
        [REPO, CLIENTS]
    ),
    tool!(
        "skills_list_transpilers",
        1,
        None,
        "List client transpilers",
        []
    ),
    tool!(
        "skills_scaffold",
        2,
        None,
        "Create a new skill skeleton",
        [NAME, p("skill_type", Ty::Str, false, "skill or rule"), REPO]
    ),
    tool!(
        "skills_sync",
        2,
        None,
        "Transpile skills to client outputs",
        [
            REPO,
            CLIENTS,
            DRY,
            p(
                "global_mode",
                Ty::Bool,
                false,
                "Write to user-level locations"
            ),
            p(
                "migrate",
                Ty::Bool,
                false,
                "Back up and replace files that shadow a synced skill; otherwise they are only reported"
            )
        ]
    ),
    tool!(
        "skills_tap_list",
        1,
        None,
        "List the registered skill taps (git sources) and whether each is cloned",
        []
    ),
    tool!(
        "skills_search",
        1,
        None,
        "Search the cloned taps for skills by name, description or tags",
        [p("query", Ty::Str, true, "Text to look for")]
    ),
    tool!(
        "skills_tap_add",
        2,
        None,
        "Register a tap and clone it; dry_run is on by default",
        [
            p("repo", Ty::Str, true, "user/repo on GitHub, or an https, ssh or file git URL"),
            p("name", Ty::Str, false, "Tap name, default derived from the source"),
            DRY_ON
        ]
    ),
    tool!(
        "skills_tap_remove",
        2,
        None,
        "Unregister a tap and delete its clone; dry_run is on by default",
        [NAME, DRY_ON]
    ),
    tool!(
        "skills_tap_update",
        2,
        None,
        "Pull one tap or all of them; dry_run is on by default",
        [p("name", Ty::Str, false, "Limit to one tap"), DRY_ON]
    ),
    tool!(
        "skills_install",
        2,
        None,
        "Install the skills of @user/repo[/skill] from a tap into the skills repository after a security audit; dry_run is on by default",
        [
            p("spec", Ty::Str, true, "@user/repo, @user/repo/skill or with @version"),
            REPO,
            DRY_ON
        ]
    ),
    tool!(
        "skills_edit_body",
        3,
        Always,
        "Replace a skill body in the canonical repository",
        [NAME, BODY, REPO]
    ),
    tool!(
        "skills_edit_frontmatter",
        3,
        Always,
        "Patch a skill's frontmatter",
        [
            NAME,
            p("patch", Ty::Obj, true, "Frontmatter fields to set"),
            REPO
        ]
    ),
    tool!(
        "skills_delete",
        3,
        Always,
        "Delete a skill from the canonical repository",
        [NAME, REPO]
    ),
    tool!(
        "skills_git_push",
        4,
        Always,
        "Commit and push the skills repository to its remote",
        [COMMIT, REPO],
        servers::skills_git_push
    ),
    tool!("agents_list", 1, None, "List agents", [REPO]),
    tool!(
        "agents_get",
        1,
        None,
        "Read one agent including its body",
        [NAME, REPO]
    ),
    tool!("agents_lint", 1, None, "Lint agents", [REPO]),
    tool!(
        "agents_list_transpilers",
        1,
        None,
        "List agent transpilers",
        []
    ),
    tool!(
        "agents_scaffold",
        2,
        None,
        "Create a new agent skeleton",
        [
            NAME,
            p("model", Ty::Str, false, "Model, default inherit"),
            REPO
        ]
    ),
    tool!(
        "agents_sync",
        2,
        None,
        "Transpile agents to client outputs",
        [
            REPO,
            CLIENTS,
            DRY,
            p(
                "global_mode",
                Ty::Bool,
                false,
                "Write to user-level locations"
            )
        ]
    ),
    tool!(
        "agents_edit_body",
        3,
        Always,
        "Replace an agent body in the canonical repository",
        [NAME, BODY, REPO]
    ),
    tool!("styles_list", 1, None, "List styles", [REPO]),
    tool!(
        "styles_get",
        1,
        None,
        "Read one style including its body",
        [NAME, REPO]
    ),
    tool!("styles_lint", 1, None, "Lint styles", [REPO]),
    tool!(
        "styles_active",
        1,
        None,
        "Show the applied style per client",
        [REPO]
    ),
    tool!(
        "styles_list_transpilers",
        1,
        None,
        "List style transpilers",
        []
    ),
    tool!(
        "styles_scaffold",
        2,
        None,
        "Create a new style skeleton",
        [NAME, REPO]
    ),
    tool!(
        "styles_sync_tier1",
        2,
        None,
        "Sync tier-1 style outputs",
        [REPO, CLIENTS, DRY]
    ),
    tool!(
        "styles_apply",
        3,
        Always,
        "Apply a style to tier-2 clients",
        [NAME, REPO, CLIENTS, DRY]
    ),
    tool!(
        "styles_edit_body",
        3,
        Always,
        "Replace a style body in the canonical repository",
        [NAME, BODY, REPO]
    ),
    tool!(
        "styles_remove",
        4,
        Always,
        "Remove the applied style from tier-2 clients",
        [REPO, CLIENTS, DRY]
    ),
    tool!(
        "servers_list",
        1,
        None,
        "List registered servers",
        [p(
            "include_sources",
            Ty::Bool,
            false,
            "Include source information"
        )]
    ),
    tool!(
        "servers_get",
        1,
        None,
        "Read one server entry without secret values",
        [NAME]
    ),
    tool!(
        "servers_list_profiles",
        1,
        None,
        "List profiles and their enabled servers",
        []
    ),
    tool!(
        "servers_detect_source",
        1,
        None,
        "Detect where a server was installed from",
        [NAME],
        servers::detect_source
    ),
    tool!(
        "servers_git_status",
        1,
        None,
        "Git status of a source-installed server",
        [NAME],
        servers::git_status
    ),
    tool!(
        "servers_check_updates",
        1,
        None,
        "Check servers for available updates",
        [
            p("name", Ty::Str, false, "Limit to one server"),
            p(
                "include_prerelease",
                Ty::Bool,
                false,
                "Consider prereleases"
            )
        ],
        servers::check_updates
    ),
    tool!(
        "servers_add_profile_tag",
        2,
        None,
        "Add a server to a profile",
        [NAME, p("profile_tag", Ty::Str, true, "Profile name")],
        servers::add_profile_tag
    ),
    tool!(
        "servers_remove_profile_tag",
        2,
        None,
        "Remove a server from a profile",
        [NAME, p("profile_tag", Ty::Str, true, "Profile name")],
        servers::remove_profile_tag
    ),
    tool!(
        "servers_install",
        3,
        Always,
        "Register a new server",
        [
            NAME,
            p("config", Ty::Obj, true, "Server configuration"),
            p("profile_tags", Ty::StrList, false, "Profiles to join"),
            p("force", Ty::Bool, false, "Replace an existing entry")
        ],
        servers::install
    ),
    tool!(
        "servers_update_config",
        3,
        Always,
        "Patch a server's configuration",
        [NAME, p("patch", Ty::Obj, true, "Fields to change")],
        servers::update_config
    ),
    tool!(
        "servers_apply_update",
        3,
        Always,
        "Apply an available server update",
        [
            NAME,
            p("rebase", Ty::Bool, false, "Rebase local changes"),
            p(
                "include_prerelease",
                Ty::Bool,
                false,
                "Consider prereleases"
            )
        ],
        servers::apply_update
    ),
    tool!(
        "servers_set_mode",
        3,
        Always,
        "Change how a server is exposed to clients",
        [
            NAME,
            p(
                "mode",
                Ty::Str,
                true,
                "auto, direct, router, legacy or bridge"
            )
        ],
        servers::set_mode
    ),
    tool!(
        "servers_fork_sync",
        3,
        Always,
        "Sync a forked source server with its upstream",
        [
            NAME,
            p("upstream_remote", Ty::Str, false, "Default upstream"),
            p("upstream_branch", Ty::Str, false, "Default main"),
            p("mode", Ty::Str, false, "Default rebase"),
            p("author_email", Ty::Str, false, "Commit author email"),
            p("target_branch", Ty::Str, false, "Branch to update"),
            p(
                "run_post_update",
                Ty::Bool,
                false,
                "Run the post-update command"
            )
        ],
        servers::fork_sync
    ),
    tool!(
        "servers_auth",
        3,
        Always,
        "Start the authorization flow for a server",
        [NAME],
        servers::auth
    ),
    tool!(
        "servers_uninstall",
        4,
        Always,
        "Remove a server and its client entries",
        [
            NAME,
            p(
                "propagate_to_clients",
                Ty::Bool,
                false,
                "Remove client entries too"
            )
        ],
        servers::uninstall
    ),
    tool!("clients_list", 1, None, "List supported client keys", []),
    tool!(
        "clients_sync",
        2,
        UnlessDryRun,
        "Reconcile client configs with the registry",
        [
            p("client", Ty::Str, false, "Single client key"),
            p("safe", Ty::Bool, false, "Skip risky rewrites"),
            p(
                "force_legacy",
                Ty::Bool,
                false,
                "Use the legacy entry shape"
            ),
            p("keep_orphans", Ty::Bool, false, "Keep unmatched entries"),
            DRY
        ],
        servers::clients_sync
    ),
    tool!(
        "sync_push",
        4,
        UnlessDryRun,
        "Publish the encrypted sync bundle to its remote",
        [DRY],
        servers::sync_push
    ),
    tool!(
        "where_am_i",
        1,
        None,
        "Absolute paths and state of this installation",
        []
    ),
    tool!("doctor", 1, None, "Read-only health checks", []),
    tool!(
        "flow_diagram",
        1,
        None,
        "Data-flow diagram of skills and servers",
        []
    ),
];

pub const RESOURCES: &[ResourceDef] = &[
    ResourceDef {
        uri: "mcpm://paths",
        name: "paths",
        mime: "application/json",
        description: "Canonical, sync and output roots",
    },
    ResourceDef {
        uri: "mcpm://status",
        name: "status",
        mime: "application/json",
        description: "Registry summary and drift counts",
    },
    ResourceDef {
        uri: "mcpm://flow",
        name: "flow",
        mime: "text/markdown",
        description: "Data-flow diagram",
    },
    ResourceDef {
        uri: "mcpm://inventory/skills",
        name: "inventory-skills",
        mime: "text/plain",
        description: "Skills, name and description",
    },
    ResourceDef {
        uri: "mcpm://inventory/agents",
        name: "inventory-agents",
        mime: "text/plain",
        description: "Agents, name and description",
    },
    ResourceDef {
        uri: "mcpm://inventory/styles",
        name: "inventory-styles",
        mime: "text/plain",
        description: "Styles, name and description",
    },
    ResourceDef {
        uri: "mcpm://inventory/servers",
        name: "inventory-servers",
        mime: "text/plain",
        description: "Servers, transport and source",
    },
    ResourceDef {
        uri: "mcpm://clients",
        name: "clients",
        mime: "application/json",
        description: "Supported client keys",
    },
    ResourceDef {
        uri: "mcpm://architecture",
        name: "architecture",
        mime: "text/markdown",
        description: "Architecture reference",
    },
    ResourceDef {
        uri: "mcpm://workflows",
        name: "workflows",
        mime: "text/markdown",
        description: "Recipes for multi-step operations",
    },
    ResourceDef {
        uri: "mcpm://router/status",
        name: "router-status",
        mime: "application/json",
        description: "Live gateway state",
    },
];

pub fn find_tool(name: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.name == name)
}

pub fn find_resource(uri: &str) -> Option<&'static ResourceDef> {
    RESOURCES.iter().find(|r| r.uri == uri)
}

fn ty_schema(ty: Ty) -> Value {
    match ty {
        Ty::Str => json!({"type": "string"}),
        Ty::Bool => json!({"type": "boolean"}),
        Ty::Obj => json!({"type": "object"}),
        Ty::StrList => json!({"type": "array", "items": {"type": "string"}}),
    }
}

pub fn input_schema(tool: &ToolDef) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for param in tool.params {
        let mut schema = ty_schema(param.ty);
        schema["description"] = json!(param.desc);
        if let Some(default) = param.default {
            schema["default"] = json!(default);
        }
        props.insert(param.name.to_string(), schema);
        if param.required {
            required.push(json!(param.name));
        }
    }
    if tool.gate != Gate::None {
        props.insert(
            "confirm".to_string(),
            json!({"type": "boolean", "default": false, "description": "Must be true to run this tool"}),
        );
    }
    let mut schema = json!({
        "type": "object",
        "properties": props,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    schema
}

pub fn tier_label(tier: u8) -> &'static str {
    match tier {
        1 => "read-only",
        2 => "write",
        3 => "confirm",
        _ => "destructive",
    }
}

pub fn tool_descriptor(tool: &ToolDef) -> Value {
    json!({
        "name": tool.name,
        "description": format!("[tier {} {}] {}", tool.tier, tier_label(tool.tier), tool.description),
        "inputSchema": input_schema(tool),
        "annotations": {
            "readOnlyHint": tool.tier == 1,
            "destructiveHint": tool.tier >= 4,
        },
    })
}
