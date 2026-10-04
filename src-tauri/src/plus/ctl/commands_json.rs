//! `toolportctl commands`: the registry the GUI builds its forms from. Rows come from `COMMANDS`
//! and the sub-commands of the dispatching rows, the safety fields from [`policy`], the flags
//! from the `Spec` each handler parses with, so none of it can drift from what the command
//! accepts.

use super::flags::{Flag, Spec};
use super::output::{no_args, CtlError, Output};
use super::policy::{self, Needs, Operand, Preview, Row, Surface, Tier, ToolPreview, SUBS};
use super::COMMANDS;
use crate::plus::selfmcp::TOOLS;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ty {
    Bool,
    Str,
    Int,
    Path,
    List,
    Paths,
    Choice(&'static [&'static str]),
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::Bool => "bool",
            Ty::Str => "string",
            Ty::Int => "integer",
            Ty::Path => "path",
            Ty::List => "list",
            Ty::Paths => "paths",
            Ty::Choice(_) => "choice",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Meta {
    pub flag: &'static str,
    pub ty: Ty,
    pub effect: &'static str,
    pub hidden: bool,
    pub sensitive: bool,
    pub repeatable: bool,
}

const fn m(flag: &'static str, ty: Ty, effect: &'static str) -> Meta {
    Meta {
        flag,
        ty,
        effect,
        hidden: false,
        sensitive: false,
        repeatable: false,
    }
}

impl Meta {
    const fn hidden(self) -> Self {
        Self {
            hidden: true,
            ..self
        }
    }

    const fn sensitive(self) -> Self {
        Self {
            sensitive: true,
            ..self
        }
    }

    const fn repeats(self) -> Self {
        Self {
            repeatable: true,
            ..self
        }
    }
}

use Ty::{Bool, Int, List, Path, Paths, Str};

const ON_OFF: Ty = Ty::Choice(&["on", "off"]);
const REFUSED: &str =
    "Always refused: a client reaches servers through the gateway and its profile";
const COMPAT: &str = "Accepted for mcpm compatibility; has no effect";

pub(super) const GLOBAL: &[Meta] = &[
    m(
        "--accept",
        Bool,
        "Apply the move; without it the command only previews",
    ),
    m(
        "--add-profile",
        List,
        "Add these profiles to the client (comma separated)",
    ),
    m(
        "--add-server",
        List,
        "Add these servers (comma separated names or ids)",
    ),
    m("--after", Int, "Tokens after compression"),
    m("--all", Bool, "Import every direct entry of the client"),
    m(
        "--allow-commands",
        Bool,
        "Accept updates that change a server's command",
    ),
    m(
        "--allow-unverified",
        Bool,
        "Accept updates that could not be verified",
    ),
    m(
        "--api-key-env",
        Str,
        "Name of an environment variable holding the API key (the GUI stores keys with secret set)",
    )
    .hidden()
    .sensitive(),
    m(
        "--apply",
        Bool,
        "Apply the change; without it the command only reports",
    ),
    m(
        "--arg",
        Str,
        "One argument for the server command; repeat for several",
    )
    .repeats(),
    m("--auto", Bool, "Sync automatically from now on"),
    m("--before", Int, "Tokens before compression"),
    m("--branch", Str, "Git branch"),
    m(
        "--by-pin",
        Bool,
        "Group the measured sessions by engine pin",
    ),
    m("--check", Bool, "Only check for updates (the default)"),
    m(
        "--checkpoint-at",
        Int,
        "Token count at which a checkpoint is due",
    ),
    m("--clear", Bool, "Remove the git sync setup"),
    m("--client", Str, "Limit to one client (id or key)"),
    m("--command", Str, "Command that starts the server"),
    m(
        "--cwd",
        Path,
        "Folder where Claude Code starts (default: the current folder)",
    ),
    m(
        "--declare-client-capabilities",
        ON_OFF,
        "Declare the client's capabilities to the server",
    ),
    m("--disable", Bool, "Turn folder profiles off"),
    m("--disabled", Str, REFUSED).hidden(),
    m("--dry-run", Bool, "Preview the change and write nothing"),
    m("--enable", Bool, "Turn folder profiles on"),
    m("--external", Bool, REFUSED).hidden(),
    m("--file", Path, REFUSED).hidden(),
    m(
        "--files",
        List,
        "Files of the project to sync (comma separated)",
    ),
    m(
        "--force",
        Bool,
        "Go ahead although a check or a conflict would stop the command",
    ),
    m(
        "--forward-instructions",
        ON_OFF,
        "Forward the server's instructions to clients",
    ),
    m("--glob", Str, "Path pattern the layer applies to"),
    m(
        "--global",
        Bool,
        "Use the user-level location (the default)",
    ),
    m(
        "--home",
        Path,
        "Home directory override (test seam; the GUI never passes it)",
    )
    .hidden(),
    m(
        "--include-projects",
        Bool,
        "Include the registered project files",
    ),
    m(
        "--init",
        Bool,
        "Detect and store the update source of each server",
    ),
    m("--install", Bool, "Install the pinned engine version"),
    m(
        "--keep-clients",
        Bool,
        "Keep the client entries that point at the server",
    ),
    m(
        "--keep-orphans",
        Bool,
        "Keep client entries Toolport no longer manages",
    ),
    m(
        "--keep-secrets",
        Bool,
        "Keep the server's secrets in the vault",
    ),
    m("--latest", Bool, "Move to the latest release"),
    m("--limit", Int, "Show at most this many results"),
    m(
        "--machine-id",
        Str,
        "Name of this machine in the sync repository",
    ),
    m("--marketplace", Str, "Limit to one plugin marketplace"),
    m(
        "--mcpm-root",
        Path,
        "mcpm configuration root to adopt the compression policy from",
    ),
    m(
        "--migrate",
        Bool,
        "Back up files that shadow synced skills and take them over",
    ),
    m("--min-turns", Int, "Ignore sessions with fewer turns"),
    m(
        "--mode",
        Ty::Choice(&["cache", "token"]),
        "Compression mode",
    ),
    m("--name", Str, "Name"),
    m(
        "--name-map",
        Bool,
        "Also print the map from old mcpm tool names to Toolport tool names",
    ),
    m(
        "--no-audit",
        Bool,
        "Skip the security audit before installing",
    ),
    m("--no-clients", Bool, "Leave the client entries alone"),
    m(
        "--no-commands",
        Bool,
        "Leave the commands out of the profile",
    ),
    m(
        "--no-migrate",
        Bool,
        "Do not take over files that shadow synced skills",
    ),
    m(
        "--no-open",
        Bool,
        "Do not open the browser; print the sign-in link",
    ),
    m("--no-org", Bool, "Leave the organisation layer out"),
    m(
        "--no-persist",
        Bool,
        "Do not save the result in context.json",
    ),
    m("--no-refresh", Bool, "Skip the transcript index refresh"),
    m(
        "--no-resolve",
        Bool,
        "Do not resolve conflicts; keep both copies",
    ),
    m("--no-skills", Bool, "Leave skills out of the profile"),
    m("--offline", Bool, "Use the curated catalog only"),
    m(
        "--org-mode",
        Ty::Choice(&["import", "copy"]),
        "How the organisation layer joins the profile",
    ),
    m("--output", Path, "File to write"),
    m(
        "--passphrase-env",
        Str,
        "Name of an environment variable holding the passphrase (the GUI sends it on stdin)",
    )
    .hidden()
    .sensitive(),
    m("--passphrase-stdin", Bool, "Read the passphrase from stdin").sensitive(),
    m(
        "--path",
        Path,
        "Repository folder (default: the configured one)",
    ),
    m("--paths", Paths, "Files or folders to rewrite"),
    m(
        "--plan",
        Bool,
        "Print the launch plan and do not start Claude Code",
    ),
    m("--port", Int, "Local port"),
    m("--preset", Str, "Compression preset"),
    m("--profile", Str, "Profile id (default: the active profile)"),
    m(
        "--project",
        Bool,
        "Use the project-level location instead of the user level",
    ),
    m("--provider", Str, "Compression provider"),
    m(
        "--prune-orphans",
        Bool,
        "Also remove entries a previous import wrote that mcpm no longer has",
    ),
    m("--purge", Bool, "Also delete the profile directory"),
    m("--purge-key", Bool, "Also delete the stored API key"),
    m(
        "--purge-profiles",
        Bool,
        "Also delete the launch profile directories",
    ),
    m("--reconfigure", Bool, "Replace an existing sync setup"),
    m(
        "--refresh",
        Bool,
        "Re-snapshot preset values from the installed engine",
    ),
    m(
        "--remove-profile",
        List,
        "Remove these profiles from the client (comma separated)",
    ),
    m(
        "--remove-server",
        List,
        "Remove these servers (comma separated names or ids)",
    ),
    m("--repo", Str, "Repository"),
    m("--reveal", Bool, "Print the secret value").sensitive(),
    m(
        "--rewrite-zshrc",
        Bool,
        "Point the source lines in ~/.zshrc at the data directory (backs the file up first)",
    ),
    m("--root", Path, "Claude Code projects folder"),
    m("--rules", Bool, "Also deploy the rules files"),
    m(
        "--run-setup",
        Bool,
        "Run the setup step after applying pulled files",
    ),
    m(
        "--select",
        List,
        "Import only these direct entries (comma separated)",
    ),
    m("--server", Str, "Server id"),
    m("--servers", List, "Servers (comma separated names or ids)"),
    m("--session", Str, "Session id the record belongs to"),
    m(
        "--set-profiles",
        List,
        "Replace the client's profiles with these (comma separated)",
    ),
    m(
        "--set-servers",
        List,
        "Replace the servers with these (comma separated names or ids)",
    ),
    m(
        "--short-ids",
        Path,
        "File that maps old server ids to short ids",
    ),
    m("--since", Str, "Only entries since this time (RFC 3339)"),
    m("--skills", List, "Only these skills (comma separated)"),
    m("--skip-clients", Bool, "Do not write client configurations"),
    m("--source", Str, "Where the record came from"),
    m("--status", Bool, "Show the git sync status"),
    m("--strict", Bool, "Exit 1 when synced outputs drifted"),
    m(
        "--teardown",
        Bool,
        "Also remove the shims and the MCP entry",
    ),
    m("--telemetry", ON_OFF, "Send compression telemetry"),
    m("--to", Str, "Version to move to"),
    m(
        "--tools",
        Path,
        "File listing the tool names of each mcpm server",
    ),
    m("--transcripts", Path, "Transcripts folder to measure"),
    m(
        "--transport",
        Ty::Choice(&["http", "sse"]),
        "Remote transport",
    ),
    m("--type", Ty::Choice(&["skill", "rule"]), "What to create"),
    m("--url", Str, "Server URL for an http or sse server"),
    m(
        "--value-env",
        Str,
        "Name of an environment variable holding the value (the GUI sends it on stdin)",
    )
    .hidden()
    .sensitive(),
    m("--verbose", Bool, "Show the servers of each profile"),
    m("--window", Int, "Context window in tokens"),
    m(
        "--with-progressive",
        Bool,
        "Also create the progressive-disclosure files",
    ),
    m("--yes", Bool, COMPAT).hidden(),
];

const SKILLS_REPO: &str = "Skills repository (default: the configured one)";
const CLIENT_KEYS: &str = "Limit to one client key; repeat for several";

pub(super) const OVERRIDES: &[(&str, Meta)] = &[
    (
        "profile create",
        m(
            "--force",
            Bool,
            "Succeed when the profile already exists (nothing changes)",
        ),
    ),
    ("profile edit", m("--force", Bool, COMPAT).hidden()),
    ("profile edit", m("--name", Str, "New name for the profile")),
    ("profile rm", m("--force", Bool, COMPAT).hidden()),
    (
        "client edit",
        m(
            "--force",
            Bool,
            "Replace a customised Toolport entry with the default gateway entry",
        ),
    ),
    ("client edit", m("--add-server", List, REFUSED).hidden()),
    ("client edit", m("--remove-server", List, REFUSED).hidden()),
    ("client edit", m("--set-servers", List, REFUSED).hidden()),
    (
        "client direct add",
        m(
            "--force",
            Bool,
            "Replace an entry that was changed after Toolport wrote it",
        ),
    ),
    (
        "client direct rm",
        m(
            "--force",
            Bool,
            "Remove an entry that was changed after Toolport wrote it",
        ),
    ),
    (
        "client sync",
        m(
            "--client",
            Str,
            "Limit to one client id; repeat for several",
        )
        .repeats(),
    ),
    (
        "update",
        m(
            "--force",
            Bool,
            "With --init: detect the source again even when one is stored",
        ),
    ),
    (
        "update",
        m("--repo", Str, "GitHub repository (owner/repo) to check"),
    ),
    (
        "sync pull",
        m(
            "--force",
            Bool,
            "Overwrite local files that changed since the last sync",
        ),
    ),
    (
        "sync init",
        m(
            "--repo",
            Str,
            "Git repository that holds the encrypted bundle",
        ),
    ),
    (
        "sync git-sync",
        m(
            "--repo",
            Str,
            "Git repository to sync the data directory with",
        ),
    ),
    (
        "sync add-project",
        m("--name", Str, "Name of the project in the sync set"),
    ),
    (
        "auth probe",
        m(
            "--force",
            Bool,
            "Probe every server, not only the ones that are due",
        ),
    ),
    (
        "compression run",
        m(
            "--force",
            Bool,
            "Launch even when the installed engine does not match the pin",
        ),
    ),
    (
        "compression seal",
        m("--apply", Bool, "Write the declared posture to the policy"),
    ),
    (
        "context profile add",
        m(
            "--rules",
            Str,
            "How the profile treats rules (default: inherit)",
        ),
    ),
    (
        "context profile add",
        m(
            "--servers",
            Str,
            "Which servers the profile gets (default: inherit)",
        ),
    ),
    ("context init", m("--yes", Bool, COMPAT).hidden()),
    (
        "skills init",
        m("--name", Str, "Name of the skills repository"),
    ),
    ("skills tap add", m("--name", Str, "Alias for the tap")),
    ("skills lint", m("--name", Str, "Lint only this skill")),
    ("skills lint", m("--repo", Path, SKILLS_REPO)),
    ("skills sync", m("--repo", Path, SKILLS_REPO)),
    ("skills ls", m("--repo", Path, SKILLS_REPO)),
    ("skills diff", m("--repo", Path, SKILLS_REPO)),
    ("server edit", m("--name", Str, "New name for the server")),
    ("skills status", m("--client", Str, CLIENT_KEYS).repeats()),
    ("agents sync", m("--client", Str, CLIENT_KEYS).repeats()),
    ("styles sync", m("--client", Str, CLIENT_KEYS).repeats()),
    ("styles apply", m("--client", Str, CLIENT_KEYS).repeats()),
    ("styles remove", m("--client", Str, CLIENT_KEYS).repeats()),
    (
        "mcp install",
        m(
            "--profile",
            Str,
            "Profile to enable the self-management server in",
        ),
    ),
];

pub(super) fn meta_for(row: &str, flag: &str) -> Option<&'static Meta> {
    OVERRIDES
        .iter()
        .find(|(id, meta)| *id == row && meta.flag == flag)
        .map(|(_, meta)| meta)
        .or_else(|| GLOBAL.iter().find(|meta| meta.flag == flag))
}

fn fallback(flag: &Flag) -> Ty {
    if flag.is_greedy() {
        Ty::Paths
    } else if flag.is_whole_number() {
        Ty::Int
    } else if flag.takes_value() {
        Ty::Str
    } else {
        Ty::Bool
    }
}

pub(super) fn row_flags(id: &str, row: Option<&Row>) -> Vec<Flag> {
    let mut specs: Vec<&'static Spec> = row.map(|r| r.specs.to_vec()).unwrap_or_default();
    if let Some((spec, _)) = policy::sub_spec(id) {
        specs.push(spec);
    }
    let only = row.map_or(&[][..], |r| r.only);
    let mut flags: Vec<Flag> = Vec::new();
    for spec in specs {
        for flag in spec.flags {
            if (only.is_empty() || only.contains(&flag.name()))
                && !flags.iter().any(|f| f.name() == flag.name())
            {
                flags.push(*flag);
            }
        }
    }
    for flag in row.map_or(&[][..], |r| r.extra) {
        if !flags.iter().any(|f| f.name() == flag.name()) {
            flags.push(*flag);
        }
    }
    flags
}

fn flag_json(id: &str, row: Option<&Row>, flag: &Flag) -> Value {
    let meta = meta_for(id, flag.name());
    let ty = meta.map_or_else(|| fallback(flag), |m| m.ty);
    let mut value = json!({
        "name": flag.name(),
        "aliases": flag.aliases(),
        "valueType": ty.name(),
        "required": row.is_some_and(|r| r.requires.contains(&flag.name())),
        "repeatable": meta.is_some_and(|m| m.repeatable),
        "escalates": row.is_some_and(|r| r.escalators.contains(&flag.name())),
        "hidden": meta.is_some_and(|m| m.hidden),
        "sensitive": meta.is_some_and(|m| m.sensitive),
        "effect": meta.map_or("", |m| m.effect),
    });
    if let Ty::Choice(choices) = ty {
        value["choices"] = json!(choices);
    }
    value
}

fn max_operands(row: &Row, id: &str) -> Option<usize> {
    let mut limits: Vec<Option<usize>> =
        row.specs.iter().map(|spec| spec.operand_limit()).collect();
    if let Some((spec, _)) = policy::sub_spec(id) {
        limits.push(spec.operand_limit());
    }
    let mut max = Some(0);
    for limit in &limits {
        match (max, limit) {
            (_, None) => return None,
            (Some(a), Some(b)) => max = Some(a.max(*b)),
            (None, _) => return None,
        }
    }
    if limits.is_empty() {
        return None;
    }
    max
}

fn operand_json(operand: &Operand) -> Value {
    json!({
        "name": operand.name,
        "required": operand.required,
        "variadic": operand.variadic,
    })
}

fn preview_json(preview: Preview) -> Value {
    match preview {
        Preview::None => json!({"mode": "none", "flag": null}),
        Preview::Flag(flag) => json!({"mode": "flag", "flag": flag}),
        Preview::UnlessApplied(flag) => json!({"mode": "unless-applied", "flag": flag}),
    }
}

struct Entry {
    id: String,
    path: Vec<&'static str>,
    summary: &'static str,
    planned: bool,
}

fn entries() -> Vec<Entry> {
    let mut out = Vec::new();
    for command in COMMANDS {
        out.push(Entry {
            id: command.path.join(" "),
            path: command.path.to_vec(),
            summary: command.summary,
            planned: command.planned(),
        });
        for sub in SUBS.iter().filter(|s| s.parent == command.path.join(" ")) {
            let mut path = command.path.to_vec();
            path.push(sub.name);
            out.push(Entry {
                id: sub.id(),
                path,
                summary: sub.summary,
                planned: false,
            });
        }
    }
    out
}

fn parent_of(entry: &Entry, all: &[Entry]) -> Option<String> {
    (1..entry.path.len())
        .rev()
        .map(|n| entry.path[..n].join(" "))
        .find(|candidate| all.iter().any(|e| e.id == *candidate))
}

fn tools_for(command: &str) -> Vec<&'static str> {
    policy::TOOL_ROWS
        .iter()
        .filter(|t| t.command == Some(command))
        .map(|t| t.name)
        .collect()
}

fn command_json(entry: &Entry, all: &[Entry]) -> Value {
    let is_group = all
        .iter()
        .any(|e| e.path.len() > entry.path.len() && e.path.starts_with(&entry.path));
    let mut value = json!({
        "id": entry.id,
        "path": entry.path,
        "group": entry.path[0],
        "kind": if is_group { "group" } else { "command" },
        "parent": parent_of(entry, all),
        "summary": entry.summary,
        "planned": entry.planned,
    });
    let row = policy::row_of(&entry.id);
    let policy_fields = match row {
        Some(row) if !is_group => json!({
            "tier": row.tier.as_str(),
            "baseTier": if row.reads_by_default { Tier::Read } else { row.tier }.as_str(),
            "dryRun": row.preview != Preview::None,
            "preview": preview_json(row.preview),
            "needs": row.needs.iter().map(|n| n.as_str()).collect::<Vec<_>>(),
            "cost": row.cost,
            "surface": if row.surface == Surface::Terminal { "terminal" } else { "screen" },
            "operands": row.operands.iter().map(operand_json).collect::<Vec<_>>(),
            "maxOperands": max_operands(row, &entry.id),
            "flags": row_flags(&entry.id, Some(row)).iter().map(|f| flag_json(&entry.id, Some(row), f)).collect::<Vec<_>>(),
            "operandEscalates": row.operand_escalates,
            "oneOf": row.one_of,
            "tools": tools_for(&entry.id),
        }),
        _ => json!({
            "tier": null,
            "baseTier": null,
            "dryRun": false,
            "preview": null,
            "needs": [],
            "cost": false,
            "surface": null,
            "operands": [],
            "maxOperands": null,
            "flags": [],
            "operandEscalates": false,
            "oneOf": [],
            "tools": [],
        }),
    };
    if let (Some(target), Some(source)) = (value.as_object_mut(), policy_fields.as_object()) {
        target.extend(source.clone());
    }
    value
}

fn tool_json(def: &crate::plus::selfmcp::ToolDef) -> Value {
    let row = policy::TOOL_ROWS.iter().find(|t| t.name == def.name);
    json!({
        "name": def.name,
        "tier": row.map(|t| t.tier.as_str()),
        "toolTier": def.tier,
        "dryRun": row.map_or(ToolPreview::None, |t| t.preview).as_str(),
        "command": row.and_then(|t| t.command),
    })
}

/// The data of `toolportctl commands --json`.
pub fn registry() -> Value {
    let all = entries();
    let commands: Vec<Value> = all.iter().map(|e| command_json(e, &all)).collect();
    let leaves = commands.iter().filter(|c| c["kind"] == "command").count();
    let needs = [
        Needs::Stdin,
        Needs::Browser,
        Needs::LongRunning,
        Needs::Network,
        Needs::TerminalOnly,
    ]
    .map(Needs::as_str);
    json!({
        "tiers": [Tier::Read.as_str(), Tier::Write.as_str(), Tier::Destructive.as_str()],
        "needs": needs,
        "counts": {
            "rows": commands.len(),
            "commands": leaves,
            "groups": commands.len() - leaves,
            "tools": TOOLS.len(),
        },
        "commands": commands,
        "tools": TOOLS.iter().map(tool_json).collect::<Vec<_>>(),
    })
}

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let data = registry();
    let mut human = format!(
        "{} commands in {} groups, {} self-management tools\n\
         tier: read changes nothing, write changes files, destructive deletes or overwrites; \
         * previews with --dry-run\n",
        data["counts"]["commands"], data["counts"]["groups"], data["counts"]["tools"]
    );
    for command in data["commands"].as_array().into_iter().flatten() {
        if command["kind"] != "command" {
            continue;
        }
        let tier = command["tier"].as_str().unwrap_or("");
        let preview = if command["dryRun"] == true { "*" } else { " " };
        human.push_str(&format!(
            "  {:<28} {:<11}{preview} {}\n",
            command["id"].as_str().unwrap_or(""),
            tier,
            command["summary"].as_str().unwrap_or("")
        ));
    }
    Ok(Output::new(data, human))
}
