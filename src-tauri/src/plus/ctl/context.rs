//! `toolportctl context loads`: read-only "what loads" report for a launch profile and directory.

use super::output::{CtlError, Output};
use crate::plus::context::{compact, load_config, loads, Roots};
use std::path::PathBuf;

fn parse(rest: &[String]) -> Result<(Option<String>, Option<PathBuf>), CtlError> {
    let (mut profile, mut cwd) = (None, None);
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        let value = |inline: Option<String>, iter: &mut std::slice::Iter<String>| {
            inline
                .or_else(|| iter.next().cloned())
                .ok_or_else(|| CtlError::usage(format!("{key} requires a value")))
        };
        match key {
            "--profile" => profile = Some(value(inline, &mut iter)?),
            "--cwd" => cwd = Some(PathBuf::from(value(inline, &mut iter)?)),
            other => return Err(CtlError::usage(format!("unexpected argument: {other}"))),
        }
    }
    Ok((profile, cwd))
}

pub fn loads(rest: &[String]) -> Result<Output, CtlError> {
    let (profile, cwd) = parse(rest)?;
    let home = dirs::home_dir()
        .ok_or_else(|| CtlError::new("no_home", "home directory could not be resolved"))?;
    let mut roots = Roots::from_home(&home);
    roots.env_claude_config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
    compact::apply_env(&mut roots);
    let config = load_config(&roots.context_config_path());
    let cwd = match cwd {
        Some(c) => c,
        None => std::env::current_dir().unwrap_or_else(|_| home.clone()),
    };
    let report = loads::what_loads(&roots, &config, profile.as_deref(), &cwd)
        .map_err(|e| CtlError::new("context_invalid", e))?;
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
    let data = serde_json::to_value(&report).map_err(|e| CtlError::new("encode", e.to_string()))?;
    Ok(Output::new(data, human))
}

fn parse_status(rest: &[String]) -> Result<(Option<String>, Option<u64>, Option<u64>), CtlError> {
    let (mut profile, mut window, mut at) = (None, None, None);
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        let mut value = || {
            inline
                .clone()
                .or_else(|| iter.next().cloned())
                .ok_or_else(|| CtlError::usage(format!("{key} requires a value")))
        };
        let number = |text: String| {
            text.parse::<u64>()
                .map_err(|_| CtlError::usage(format!("{key} requires a token count")))
        };
        match key {
            "--profile" => profile = Some(value()?),
            "--window" => window = Some(number(value()?)?),
            "--checkpoint-at" => at = Some(number(value()?)?),
            other => return Err(CtlError::usage(format!("unexpected argument: {other}"))),
        }
    }
    Ok((profile, window, at))
}

pub fn checkpoint_status_from(
    rest: &[String],
    input: &mut dyn std::io::Read,
    roots: &Roots,
) -> Result<Output, CtlError> {
    let (profile, window, at) = parse_status(rest)?;
    let mut text = String::new();
    input
        .read_to_string(&mut text)
        .map_err(|e| CtlError::new("stdin", e.to_string()))?;
    let statusline: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| CtlError::new("bad_input", format!("statusline JSON: {e}")))?;
    let config = load_config(&roots.context_config_path());
    let spec = match &profile {
        Some(name) => Some(
            config
                .profiles
                .get(name)
                .ok_or_else(|| CtlError::new("context_invalid", format!("unknown profile: {name}")))?,
        ),
        None => None,
    };
    let data = compact::checkpoint_status(roots, &statusline, spec, window, at)
        .map_err(|e| CtlError::new("bad_input", e))?;
    let human = format!(
        "{} tokens used, checkpoint at {}\n",
        data["used_tokens"],
        data["checkpoint_point"]
    );
    Ok(Output::new(data, human))
}

pub fn checkpoint_status(rest: &[String]) -> Result<Output, CtlError> {
    let home = dirs::home_dir()
        .ok_or_else(|| CtlError::new("no_home", "home directory could not be resolved"))?;
    let mut roots = Roots::from_home(&home);
    compact::apply_env(&mut roots);
    checkpoint_status_from(rest, &mut std::io::stdin().lock(), &roots)
}

const GROUP_USAGE: &str = "usage: context loads|checkpoint-status|plan|apply|sync (plan|apply|sync: [--home <dir>] [--rules] [--no-persist] [--dry-run])";

struct DeployFlags {
    args: serde_json::Value,
    dry_run: bool,
}

fn parse_deploy(rest: &[String], allow_dry_run: bool) -> Result<DeployFlags, CtlError> {
    let mut args = serde_json::json!({});
    let mut dry_run = false;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        match key {
            "--home" => {
                let value = inline
                    .or_else(|| iter.next().cloned())
                    .ok_or_else(|| CtlError::usage("--home requires a value"))?;
                args["home"] = serde_json::json!(value);
            }
            "--rules" if inline.is_none() => args["rules"] = serde_json::json!(true),
            "--no-persist" if inline.is_none() => args["persist"] = serde_json::json!(false),
            "--dry-run" if allow_dry_run && inline.is_none() => dry_run = true,
            _ => return Err(CtlError::usage(format!("unexpected argument: {arg}"))),
        }
    }
    Ok(DeployFlags { args, dry_run })
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
    crate::plus::dispatch(command, args).map_err(|e| CtlError::new("context_invalid", e))
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn plan(rest: &[String]) -> Result<Output, CtlError> {
    let flags = parse_deploy(rest, false)?;
    let data = deploy("plus.context.plan", flags.args)?;
    let human = render(&data);
    Ok(Output::new(data, human))
}

pub fn apply(rest: &[String]) -> Result<Output, CtlError> {
    let flags = parse_deploy(rest, true)?;
    let command = if flags.dry_run {
        "plus.context.plan"
    } else {
        "plus.context.apply"
    };
    let data = deploy(command, flags.args)?;
    let human = render(&data);
    Ok(Output::new(data, human))
}

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    let flags = parse_deploy(rest, true)?;
    let plan = deploy("plus.context.plan", flags.args.clone())?;
    if flags.dry_run {
        let human = render(&plan);
        return Ok(Output::new(
            serde_json::json!({"dryRun": true, "plan": plan, "apply": null}),
            human,
        ));
    }
    let applied = deploy("plus.context.apply", flags.args)?;
    let human = format!("plan:\n{}\napply:\n{}", render(&plan), render(&applied));
    Ok(Output::new(
        serde_json::json!({"dryRun": false, "plan": plan, "apply": applied}),
        human,
    ))
}
