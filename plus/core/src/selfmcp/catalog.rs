use serde_json::{json, Map, Value};
use toolport_plus_api::Host;

use super::{backend, context, servers, ToolError};

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
    Int,
}

pub struct Param {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
    pub desc: &'static str,
    pub default: Option<bool>,
}

pub type Runner = fn(&dyn Host, &Value) -> Result<Value, ToolError>;

pub struct ToolDef {
    pub name: &'static str,
    pub tier: u8,
    pub gate: Gate,
    pub description: &'static str,
    pub params: &'static [Param],
    pub run: Runner,
}

pub struct ResourceDef {
    pub uri: &'static str,
    pub name: &'static str,
    pub mime: &'static str,
    pub description: &'static str,
}

#[allow(dead_code)]
pub const fn p(name: &'static str, ty: Ty, required: bool, desc: &'static str) -> Param {
    Param {
        name,
        ty,
        required,
        desc,
        default: None,
    }
}

#[allow(dead_code)]
pub const fn on_by_default(param: Param) -> Param {
    Param {
        default: Some(true),
        ..param
    }
}

const NAME: Param = p("name", Ty::Str, true, "Server id or name");
const DRY: Param = p(
    "dry_run",
    Ty::Bool,
    false,
    "Report the plan and write nothing",
);
const CWD: Param = p("cwd", Ty::Str, true, "The folder where Claude Code starts");
const DRY_ON: Param = on_by_default(p(
    "dry_run",
    Ty::Bool,
    false,
    "Report without writing; true unless you pass false to apply",
));

macro_rules! tool {
    ($name:literal, $tier:literal, $gate:ident, $desc:literal, [$($param:expr),* $(,)?], $run:path) => {
        ToolDef {
            name: $name,
            tier: $tier,
            gate: Gate::$gate,
            description: $desc,
            params: &[$($param),*],
            run: $run,
        }
    };
}

pub const TOOLS: &[ToolDef] = &[
    tool!(
        "where_am_i",
        1,
        None,
        "Absolute paths and state of this installation",
        [],
        backend::where_am_i
    ),
    tool!(
        "doctor",
        1,
        None,
        "Read-only health checks",
        [],
        backend::doctor
    ),
    tool!(
        "flow_diagram",
        1,
        None,
        "Data-flow diagram of skills and servers",
        [],
        backend::flow_diagram
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
        )],
        servers::list_servers
    ),
    tool!(
        "servers_get",
        1,
        None,
        "Read one server entry without secret values",
        [NAME],
        servers::get
    ),
    tool!(
        "servers_list_profiles",
        1,
        None,
        "List access sets (profiles) and the servers each lists",
        [],
        servers::list_profiles
    ),
    tool!(
        "servers_add_profile_tag",
        2,
        None,
        "List a server in an access set, by the set's name or id; a server that is switched off stays unreachable until it is switched on",
        [NAME, p("profile_tag", Ty::Str, true, "Access set name or id")],
        servers::add_profile_tag
    ),
    tool!(
        "servers_remove_profile_tag",
        2,
        None,
        "Take a server out of an access set, by the set's name or id",
        [NAME, p("profile_tag", Ty::Str, true, "Access set name or id")],
        servers::remove_profile_tag
    ),
    tool!(
        "servers_install",
        3,
        Always,
        "Register a new server",
        [
            NAME,
            p(
                "config",
                Ty::Obj,
                true,
                "Server configuration; env is {NAME: value} for plain settings, never a credential; inheritEnv gives the process the whole shell environment"
            ),
            p("profile_tags", Ty::StrList, false, "Access sets to list it in"),
            p("force", Ty::Bool, false, "Replace an existing entry")
        ],
        servers::install
    ),
    tool!(
        "servers_update_config",
        3,
        Always,
        "Patch a server's configuration, including plain env entries and inheritEnv",
        [
            NAME,
            p(
                "patch",
                Ty::Obj,
                true,
                "Fields to change; env is {NAME: value}, null removes a name, a credential is refused"
            )
        ],
        servers::update_config
    ),
    tool!(
        "servers_set_mode",
        3,
        Always,
        "Choose whether a server's tools are listed to clients that get the full tool list (auto) or only found through toolport_search_tools and called by name (search)",
        [NAME, p("mode", Ty::Str, true, "auto or search")],
        servers::set_mode
    ),
    tool!(
        "servers_uninstall",
        4,
        Always,
        "Remove a server and its vault entries",
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
    tool!(
        "servers_restart",
        3,
        UnlessDryRun,
        "Restart one server's process in every running gateway, and in every project folder of a server that runs once per folder, so it runs its current code and its tool list is read again; dry_run is on by default (apply with dry_run=false and confirm=true); if_idle=true restarts only when no call is in flight in any of them and names the busy folders, because a restart ends the calls",
        [
            NAME,
            DRY_ON,
            p("if_idle", Ty::Bool, false, "Restart only when no call is in flight")
        ],
        servers::restart_server
    ),
    tool!(
        "servers_auth",
        3,
        Always,
        "Start a server's sign-in. A stdio server's own auth command runs and its consent URL comes back to open in a browser; a remote server runs upstream's browser sign-in and the call returns when it ends. The server's own callback stores the token; no credential is returned",
        [NAME],
        servers::auth
    ),
    tool!(
        "context_bundle_ls",
        1,
        None,
        "List the context bundles (profiles/<name>.yaml in the skills repository) with what each hides and adds and the folders it is applied in",
        [],
        context::ls
    ),
    tool!(
        "context_bundle_status",
        1,
        None,
        "Report which context bundle is applied in a folder, whether a key it owns changed since, and the keys an undo would leave alone",
        [CWD],
        context::status
    ),
    tool!(
        "context_bundle_apply",
        2,
        None,
        "Apply a context bundle in a folder: skill, plugin, agent and CLAUDE.md hiding goes into the folder's git-ignored .claude/settings.local.json and layers into one managed block of CLAUDE.local.md, with a ledger that undo restores from. Pass dry_run to see the plan and write nothing",
        [
            p("name", Ty::Str, true, "The bundle's name (profiles/<name>.yaml)"),
            p("cwd", Ty::Str, true, "The folder where Claude Code starts; nothing is written above it"),
            DRY
        ],
        context::apply
    ),
    tool!(
        "context_bundle_undo",
        2,
        None,
        "Undo the bundle applied in a folder: put back the keys it replaced and remove its block; a key changed since is left alone and listed in conflicts. Pass dry_run to see the plan and write nothing",
        [CWD, DRY],
        context::undo
    ),
    tool!(
        "context_leave_out",
        2,
        None,
        "Leave one CLAUDE.md or rule file out of a folder's stack, or turn auto memory off there: add writes the file's absolute path into claudeMdExcludes (or autoMemoryEnabled: false) in the folder's git-ignored .claude/settings.local.json, rm puts it back. Ledgered like the other folder controls, so undo removes only what Toolport wrote; the file itself is never changed and a managed policy CLAUDE.md cannot be left out. dry_run is on by default: the call previews and writes only with dry_run=false",
        [
            p("action", Ty::Str, true, "add (leave out) or rm (load again)"),
            p("path", Ty::Str, false, "Absolute path, or ~/..., of the instruction file; leave empty when memory is true"),
            p("memory", Ty::Bool, false, "Act on auto memory instead of a file"),
            p("cwd", Ty::Str, true, "Folder where Claude Code starts; nothing is written above it"),
            DRY_ON
        ],
        context::leave_out
    ),
    tool!(
        "context_loads",
        1,
        None,
        "List the instruction files a Claude Code session in a folder loads (CLAUDE.md files, rules, imports, auto memory index) with an estimated token cost, which are left out and by whom, and the folder's calibration factor from its last measurement. With measured=true the rows the last measured run's InstructionsLoaded log confirms are marked loaded, absent or mismatch. Skills, plugins, agents and tools are not rows. Read only",
        [CWD, p("measured", Ty::Bool, false, "Add the last measurement and mark the rows its log confirms")],
        context::loads
    ),
    tool!(
        "context_compose",
        1,
        None,
        "Show the instruction text Claude Code gets when it starts in a folder, part by part in load order, each with its origin, whether it loads lazily and an estimated token count. Read only",
        [CWD],
        context::compose
    ),
    tool!(
        "context_measure",
        2,
        None,
        "Measure what a folder really loads, in tokens, by asking Claude Code itself: one request per variant (as is, each without entry, the bundle), which spends model tokens and is cached per Claude Code version, model, settings and variant. Without start=true and without a cached answer it only returns the plan and starts nothing. The result separates the estimated instruction rows from the part that is not itemised, gives each variant's delta, verifies the rows against the InstructionsLoaded log and carries the calibration factor",
        [
            CWD,
            p("without", Ty::StrList, false, "Also measure without plugin:<id> or skill:<name> (a skill glob is allowed)"),
            p("bundle", Ty::Str, false, "Also measure what the named bundle would leave"),
            p("model", Ty::Str, false, "Model to ask, haiku by default"),
            p("force", Ty::Bool, false, "Measure again instead of answering from the cache"),
            p("start", Ty::Bool, false, "Start the request; without it a cache miss returns the plan only")
        ],
        context::measure
    ),
    tool!(
        "context_status",
        1,
        None,
        "Everything Toolport knows about a folder's context, read only: the applied bundle, what is left out, which plugins are switched off, the estimate of the instruction rows and the last measurement with its calibration factor. Starts nothing and costs no tokens",
        [CWD],
        context::context_status
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
        uri: "mcpm://inventory/servers",
        name: "inventory-servers",
        mime: "text/plain",
        description: "Servers, transport and source",
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
        Ty::Int => json!({"type": "integer"}),
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
