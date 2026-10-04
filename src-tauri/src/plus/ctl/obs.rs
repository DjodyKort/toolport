//! `toolportctl obs otel`: opt-in Claude Code telemetry into the local OTLP receiver.

use super::flags::{switch, value, Flags, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::plus::dispatch;
use serde_json::{json, Value};

const GROUP_USAGE: &str = "usage: obs otel enable|disable|status";
const OTEL_USAGE: &str = "usage: obs otel enable [--port <n>] [--home <dir>] [--dry-run] | disable [--home <dir>] [--dry-run] | status [--home <dir>]";
const ENABLE_USAGE: &str = "usage: obs otel enable [--port <n>] [--home <dir>] [--dry-run]";
const DISABLE_USAGE: &str = "usage: obs otel disable [--home <dir>] [--dry-run]";
const STATUS_USAGE: &str = "usage: obs otel status [--home <dir>]";

const ENABLE: Spec = Spec {
    flags: &[
        value("--port").needs("a port number").nonempty(),
        value("--home").needs("a directory").nonempty(),
        switch("--dry-run"),
    ],
    inline: Inline::Value,
    unknown: Unknown::ArgumentUsage(ENABLE_USAGE),
    operands: Operands::Reject,
    ..Spec::PLAIN
};

const DISABLE: Spec = Spec {
    flags: &[value("--home").needs("a directory").nonempty(), switch("--dry-run")],
    unknown: Unknown::ArgumentUsage(DISABLE_USAGE),
    ..ENABLE
};

const STATUS: Spec = Spec {
    flags: &[value("--home").needs("a directory").nonempty()],
    unknown: Unknown::ArgumentUsage(STATUS_USAGE),
    ..ENABLE
};

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn otel_group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(OTEL_USAGE))
}

fn args_of(flags: &Flags) -> Result<Value, CtlError> {
    let mut args = json!({"dryRun": flags.on("--dry-run")});
    if let Some(port) = flags.one("--port") {
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| CtlError::usage("--port must be a number between 1 and 65535"))?;
        args["port"] = json!(port);
    }
    if let Some(home) = flags.one("--home") {
        args["home"] = json!(home);
    }
    Ok(args)
}

fn lines(human: &mut String, data: &Value, key: &str, prefix: &str) {
    for line in data[key].as_array().into_iter().flatten() {
        human.push_str(&format!("{prefix}{}\n", line.as_str().unwrap_or("")));
    }
}

fn text<'a>(data: &'a Value, key: &str) -> &'a str {
    data[key].as_str().unwrap_or("")
}

pub fn otel_enable(rest: &[String]) -> Result<Output, CtlError> {
    let flags = ENABLE.parse(rest)?;
    let data = dispatch("plus.obs.otel.enable", args_of(&flags)?)
        .map_err(|e| CtlError::failed("otel_enable", e))?;
    let conflicts: Vec<&str> = data["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !conflicts.is_empty() {
        return Err(CtlError::conflict(format!(
            "{} already sets {} to another value; not overwritten. Remove or change those keys, then run again",
            text(&data, "settingsPath"),
            conflicts.join(", ")
        )));
    }
    let mut human = if data["dryRun"] == true {
        format!(
            "dry run: would enable the receiver on {}; nothing written\n",
            text(&data, "endpoint")
        )
    } else if data["changed"] == true {
        format!("enabled: Claude Code sends telemetry to {}\n", text(&data, "endpoint"))
    } else {
        format!("already enabled on {}; nothing changed\n", text(&data, "endpoint"))
    };
    lines(&mut human, &data, "actions", "  ");
    lines(&mut human, &data, "warnings", "warning: ");
    human.push_str(&format!(
        "settings: {}\nThe receiver runs inside the Toolport gateway. Restart running Claude Code sessions, then check with `toolportctl obs otel status`.\n",
        text(&data, "settingsPath")
    ));
    Ok(Output::new(data, human))
}

pub fn otel_disable(rest: &[String]) -> Result<Output, CtlError> {
    let flags = DISABLE.parse(rest)?;
    let data = dispatch("plus.obs.otel.disable", args_of(&flags)?)
        .map_err(|e| CtlError::failed("otel_disable", e))?;
    let mut human = if data["dryRun"] == true {
        "dry run: would disable the receiver; nothing written\n".to_string()
    } else if data["changed"] == true {
        "disabled: the receiver stops and the telemetry keys Toolport wrote are removed\n".to_string()
    } else {
        "already disabled; nothing changed\n".to_string()
    };
    lines(&mut human, &data, "actions", "  ");
    human.push_str(&format!("settings: {}\n", text(&data, "settingsPath")));
    Ok(Output::new(data, human))
}

pub fn otel_status(rest: &[String]) -> Result<Output, CtlError> {
    let flags = STATUS.parse(rest)?;
    let data = dispatch("plus.obs.otel.status", args_of(&flags)?)
        .map_err(|e| CtlError::failed("otel_status", e))?;
    let receiver = &data["receiver"];
    let mut human = format!(
        "otel receiver: {} at {} (config: {})\n",
        text(receiver, "state"),
        text(&data, "endpoint"),
        if data["enabled"] == true { "enabled" } else { "disabled" },
    );
    if let Some(error) = receiver["error"].as_str() {
        human.push_str(&format!("  {error}\n"));
    }
    let settings = &data["settings"];
    human.push_str(&format!(
        "claude settings: {} ({})\n",
        text(settings, "path"),
        text(settings, "state")
    ));
    lines(&mut human, settings, "warnings", "warning: ");
    let events = &data["events"];
    human.push_str(&format!(
        "otel events stored: {}{}\n",
        events["count"].as_u64().unwrap_or(0),
        events["latest"]
            .as_str()
            .map(|t| format!(", latest {t}"))
            .unwrap_or_default()
    ));
    Ok(Output::new(data, human))
}
