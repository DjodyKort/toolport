//! `toolportctl context wiring|layers|presync|checkpoint-status|checkpoint-config`: what puts the
//! rule layers and the launch wiring on this machine. Thin renderers over `context::wiring`,
//! `context::layer_list`, `context::layer_deploy`, `context::presync` and `context::checkpoint`,
//! which the self-MCP tools call as well.

use super::context::{flagged, plan_text};
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::context::{checkpoint, layer_deploy, layer_list, presync, wiring};
use serde_json::Value;
use toolport_plus_api::Host;

const PRESYNC_USAGE: &str = "usage: context presync [--quiet] [--force]";
const CHECKPOINT_USAGE: &str = "usage: context checkpoint-status [--cwd <dir>]";
const CONFIG_USAGE: &str =
    "usage: context checkpoint-config [--at <tokens before compaction> | --off] [--dry-run]";

const PLAIN: Spec = Spec {
    flags: &[],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const LAYERS_LS: Spec = PLAIN;

pub(super) const WIRING_STATUS: Spec = PLAIN;

pub(super) const DRY: Spec = Spec {
    flags: &[switch("--dry-run")],
    ..PLAIN
};

pub(super) const PRESYNC: Spec = Spec {
    flags: &[switch("--quiet"), switch("--force")],
    operands: Operands::Max(0, PRESYNC_USAGE),
    ..PLAIN
};

pub(super) const CHECKPOINT_STATUS: Spec = Spec {
    flags: &[value("--cwd")],
    operands: Operands::Max(0, CHECKPOINT_USAGE),
    ..PLAIN
};

pub(super) const CHECKPOINT_CONFIG: Spec = Spec {
    flags: &[value("--at"), switch("--off"), switch("--dry-run")],
    operands: Operands::Max(0, CONFIG_USAGE),
    ..PLAIN
};

fn texts(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

pub fn layers_ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    LAYERS_LS.parse(rest)?;
    let data = layer_list::ls(host)?;
    let mut human = String::new();
    for layer in data["layers"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "{:<24} {:<7} {:<13} {}\n",
            layer["name"].as_str().unwrap_or(""),
            layer["scope"].as_str().unwrap_or(""),
            layer["state"].as_str().unwrap_or(""),
            layer["detail"].as_str().unwrap_or("")
        ));
    }
    if human.is_empty() {
        human.push_str("no layers (rules/<name>/SKILL.md in the skills repository)\n");
    }
    Ok(Output::new(data, human))
}

pub fn layers_undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = DRY.parse(rest)?;
    let data = layer_deploy::undo(host, flags.on("--dry-run"))?;
    let mut human = String::new();
    for row in data["rows"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "{} {} in {}{}\n",
            row["action"].as_str().unwrap_or(""),
            row["layer"].as_str().unwrap_or(""),
            row["folder"].as_str().unwrap_or(""),
            row["reason"]
                .as_str()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        ));
    }
    if human.is_empty() {
        human.push_str("no layer blocks to undo\n");
    }
    Ok(Output::new(data, human))
}

fn check_lines(data: &Value) -> String {
    let mut human = String::new();
    for check in data["checks"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "  {:<5} {}\n",
            check["level"].as_str().unwrap_or(""),
            check["title"].as_str().unwrap_or("")
        ));
    }
    human
}

pub fn wiring_status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    WIRING_STATUS.parse(rest)?;
    let data = wiring::status(host)?;
    let mut human = format!("launch wiring: {}\n", data["state"].as_str().unwrap_or(""));
    human.push_str(&format!(
        "  shell file: {} ({})\n  startup file: {} ({})\n",
        data["shims"]["file"].as_str().unwrap_or(""),
        if data["shims"]["exists"] == Value::Bool(true) {
            "written"
        } else {
            "not written"
        },
        data["shell"]["file"].as_str().unwrap_or(""),
        if data["shell"]["sourced"] == Value::Bool(true) {
            "sourced"
        } else {
            "not sourced"
        },
    ));
    human.push_str(&check_lines(&data));
    if let Some(last) = data["presync"]["lastAttemptAt"].as_str() {
        human.push_str(&format!(
            "  last presync {last}: {}\n",
            if data["presync"]["ok"] == Value::Bool(true) {
                "ok"
            } else {
                "failed"
            }
        ));
    }
    Ok(Output::new(data, human))
}

pub fn wiring_init(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = DRY.parse(rest)?;
    let data = wiring::init(host, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn wiring_disable(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = DRY.parse(rest)?;
    let data = wiring::disable(host, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn presync_run(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = PRESYNC.parse(rest)?;
    let data = presync::run(
        host,
        &presync::ProcessOrgTool,
        presync::Opts {
            force: flags.on("--force"),
        },
    )?;
    let human = if flags.on("--quiet") {
        String::new()
    } else if data["ran"] == Value::Bool(false) {
        "presync: not due yet\n".to_string()
    } else {
        format!(
            "presync: done in {} ms{}\n",
            data["durationMs"],
            texts(&data["warnings"])
                .first()
                .map(|w| format!(" ({w})"))
                .unwrap_or_default()
        )
    };
    Ok(Output::new(data, human))
}

pub fn checkpoint_status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = CHECKPOINT_STATUS.parse(rest)?;
    let data = checkpoint::status(host, flags.one("--cwd")).map_err(flagged)?;
    let human = match data["usedTokens"].as_u64() {
        None => format!("{}\n", data["note"].as_str().unwrap_or("no session")),
        Some(used) => {
            let mut text = format!(
                "{used} of {} tokens used ({} window); compaction at {}\n",
                data["window"]["tokens"], data["window"]["source"], data["compactionAt"]
            );
            match data["checkpoint"]["point"].as_u64() {
                Some(point) => text.push_str(&format!(
                    "checkpoint at {point}: {}\n",
                    if data["checkpoint"]["reached"] == Value::Bool(true) {
                        "reached"
                    } else {
                        "not reached"
                    }
                )),
                None => {
                    text.push_str("no checkpoint set (context checkpoint-config --at <tokens>)\n")
                }
            }
            text
        }
    };
    Ok(Output::new(data, human))
}

pub fn checkpoint_config(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = CHECKPOINT_CONFIG.parse(rest)?;
    let at = match (flags.one("--at"), flags.on("--off")) {
        (Some(_), true) => return Err(CtlError::usage("--at and --off cannot be used together")),
        (Some(text), false) => Some(
            text.trim()
                .parse::<u64>()
                .map_err(|_| CtlError::usage("--at takes a number of tokens"))?,
        ),
        (None, true) => None,
        (None, false) => return Err(CtlError::usage(CONFIG_USAGE)),
    };
    let data = checkpoint::set(host, at, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}
