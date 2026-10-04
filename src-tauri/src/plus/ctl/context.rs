//! `toolportctl context loads`: read-only "what loads" report for a launch profile and directory.

use super::flags::{switch, value, Flags, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::plus::context::{compact, load_config, loads, Roots};
use std::path::PathBuf;

const LOADS: Spec = Spec {
    flags: &[value("--profile"), value("--cwd")],
    inline: Inline::Value,
    unknown: Unknown::ArgumentKey,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub fn loads(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LOADS.parse(rest)?;
    let home = dirs::home_dir()
        .ok_or_else(|| CtlError::failed("no_home", "home directory could not be resolved"))?;
    let mut roots = Roots::from_home(&home);
    roots.env_claude_config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
    compact::apply_env(&mut roots);
    let config = load_config(&roots.context_config_path());
    let cwd = match flags.one("--cwd") {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().unwrap_or_else(|_| home.clone()),
    };
    let report = loads::what_loads(&roots, &config, flags.one("--profile"), &cwd)
        .map_err(|e| CtlError::failed("context_invalid", e))?;
    let mut human = format!("{} tokens loaded in {}\n", report.total_tokens, report.cwd);
    for item in &report.items {
        let mark = if item.loaded { "+" } else { "-" };
        human.push_str(&format!(
            "{mark} {:<8} {:<14} {:>6}  {}\n",
            item.kind, item.source, item.tokens, item.name
        ));
    }
    for c in &report.clobbers {
        human.push_str(&format!(
            "{} {} {}: {} over {}\n",
            c.kind,
            c.relation,
            c.key,
            c.winner,
            c.overridden.join(", ")
        ));
    }
    if let Some(c) = &report.compact {
        human.push_str(&format!(
            "compact window {} ({}), checkpoint {}\n",
            c.window,
            c.window_source,
            c.checkpoint_point
                .map_or_else(|| "none".to_string(), |p| p.to_string())
        ));
    }
    let data =
        serde_json::to_value(&report).map_err(|e| CtlError::failed("encode", e.to_string()))?;
    Ok(Output::new(data, human))
}

const STATUS: Spec = Spec {
    flags: &[
        value("--profile"),
        value("--window").count("requires a token count"),
        value("--checkpoint-at").count("requires a token count"),
    ],
    ..LOADS
};

pub fn checkpoint_status_from(
    rest: &[String],
    input: &mut dyn std::io::Read,
    roots: &Roots,
) -> Result<Output, CtlError> {
    let flags = STATUS.parse(rest)?;
    let mut text = String::new();
    input
        .read_to_string(&mut text)
        .map_err(|e| CtlError::failed("stdin", e.to_string()))?;
    let statusline: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| CtlError::failed("bad_input", format!("statusline JSON: {e}")))?;
    let config = load_config(&roots.context_config_path());
    let spec = match flags.one("--profile") {
        Some(name) => Some(
            config
                .profiles
                .get(name)
                .ok_or_else(|| {
                    CtlError::failed("context_invalid", format!("unknown profile: {name}"))
                })?,
        ),
        None => None,
    };
    let data = compact::checkpoint_status(
        roots,
        &statusline,
        spec,
        flags.count("--window"),
        flags.count("--checkpoint-at"),
    )
        .map_err(|e| CtlError::failed("bad_input", e))?;
    let human = format!(
        "{} tokens used, checkpoint at {}\n",
        data["used_tokens"],
        data["checkpoint_point"]
    );
    Ok(Output::new(data, human))
}

pub fn checkpoint_status(rest: &[String]) -> Result<Output, CtlError> {
    let home = dirs::home_dir()
        .ok_or_else(|| CtlError::failed("no_home", "home directory could not be resolved"))?;
    let mut roots = Roots::from_home(&home);
    compact::apply_env(&mut roots);
    checkpoint_status_from(rest, &mut std::io::stdin().lock(), &roots)
}

const GROUP_USAGE: &str = "usage: context init|status|client|profile|disable|loads|checkpoint-status|plan|apply|sync (plan|apply|sync: [--home <dir>] [--rules] [--no-persist] [--dry-run])";

const DEPLOY: Spec = Spec {
    flags: &[value("--home"), switch("--rules"), switch("--no-persist")],
    inline: Inline::Value,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};
const DEPLOY_DRY: Spec = Spec {
    flags: &[
        value("--home"),
        switch("--rules"),
        switch("--no-persist"),
        switch("--dry-run"),
    ],
    ..DEPLOY
};

fn deploy_args(flags: &Flags) -> serde_json::Value {
    let mut args = serde_json::json!({});
    if let Some(home) = flags.one("--home") {
        args["home"] = serde_json::json!(home);
    }
    if flags.on("--rules") {
        args["rules"] = serde_json::json!(true);
    }
    if flags.on("--no-persist") {
        args["persist"] = serde_json::json!(false);
    }
    args
}

fn render(data: &serde_json::Value) -> String {
    let mut human = String::new();
    for key in ["actions", "warnings"] {
        for line in data[key].as_array().into_iter().flatten() {
            let prefix = if key == "warnings" { "warning: " } else { "" };
            human.push_str(&format!("{prefix}{}\n", line.as_str().unwrap_or("")));
        }
    }
    let checks = data["checks"].as_array().map_or(0, Vec::len);
    human.push_str(&format!("{checks} doctor check(s)"));
    human
}

fn deploy(command: &str, args: serde_json::Value) -> Result<serde_json::Value, CtlError> {
    crate::plus::dispatch(command, args).map_err(|e| CtlError::failed("context_invalid", e))
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn plan(rest: &[String]) -> Result<Output, CtlError> {
    let flags = DEPLOY.parse(rest)?;
    let data = deploy("plus.context.plan", deploy_args(&flags))?;
    let human = render(&data);
    Ok(Output::new(data, human))
}

pub fn apply(rest: &[String]) -> Result<Output, CtlError> {
    let flags = DEPLOY_DRY.parse(rest)?;
    let command = if flags.on("--dry-run") {
        "plus.context.plan"
    } else {
        "plus.context.apply"
    };
    let data = deploy(command, deploy_args(&flags))?;
    let human = render(&data);
    Ok(Output::new(data, human))
}

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    let flags = DEPLOY_DRY.parse(rest)?;
    let args = deploy_args(&flags);
    let plan = deploy("plus.context.plan", args.clone())?;
    if flags.on("--dry-run") {
        let human = render(&plan);
        return Ok(Output::new(
            serde_json::json!({"dryRun": true, "plan": plan, "apply": null}),
            human,
        ));
    }
    let applied = deploy("plus.context.apply", args)?;
    let human = format!("plan:\n{}\napply:\n{}", render(&plan), render(&applied));
    Ok(Output::new(
        serde_json::json!({"dryRun": false, "plan": plan, "apply": applied}),
        human,
    ))
}
