//! `toolportctl commands`: the registry the GUI builds its forms from. Rows come from `COMMANDS`
//! and the sub-commands of the dispatching rows, the safety fields from [`policy`], the flags
//! from the `Spec` each handler parses with, so none of it can drift from what the command
//! accepts.

use serde_json::{json, Value};

use super::flags::{Flag, Spec};
use super::output::{no_args, CtlError, Output};
use super::policy::{self, Needs, Operand, Preview, Row, Surface, Tier, ToolPreview, SUBS};
use super::COMMANDS;
use crate::selfmcp::TOOLS;
use toolport_plus_api::Host;

#[allow(dead_code)]
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

use Ty::{Bool, Str};

/// What each flag does, per command row. A flag without an entry fails the policy test.
pub(super) const META: &[(&str, Meta)] = &[
    (
        "mcp install",
        m(
            "--access-set",
            Str,
            "Access set to list the self-management server in, whatever was decided before (--profile is the old spelling)",
        ),
    ),
    (
        "mcp call",
        m(
            "--args",
            Str,
            "The tool's arguments as one JSON object, for values that are not secret",
        ),
    ),
    (
        "mcp call",
        m(
            "--args-stdin",
            Bool,
            "Read the tool's arguments as one JSON object from stdin; nothing reaches argv",
        ),
    ),
];

pub(super) fn meta_for(row: &str, flag: &str) -> Option<&'static Meta> {
    META.iter()
        .find(|(id, meta)| *id == row && meta.flag == flag)
        .map(|(_, meta)| meta)
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

fn tool_json(def: &crate::selfmcp::ToolDef) -> Value {
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

pub fn run(_: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
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
