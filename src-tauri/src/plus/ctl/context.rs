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
