//! `toolportctl context loads`: read-only "what loads" report for a launch profile and directory.

use super::output::{CtlError, Output};
use crate::plus::context::{load_config, loads, Roots};
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
    let data = serde_json::to_value(&report).map_err(|e| CtlError::new("encode", e.to_string()))?;
    Ok(Output::new(data, human))
}
