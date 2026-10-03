//! `toolportctl council install|uninstall|doctor|tools`.

use super::output::{CtlError, Output};
use crate::plus::council as c;
use crate::registry;
use serde_json::json;

const USAGE: &str =
    "usage: council <install [--api-key-env VAR] | uninstall [--purge-key] | doctor | tools>";

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    let Some((sub, args)) = rest.split_first() else {
        return Err(CtlError::usage(USAGE));
    };
    match sub.as_str() {
        "install" => install(args),
        "uninstall" => uninstall(args),
        "doctor" => doctor(args),
        "tools" => tools(args),
        _ => Err(CtlError::usage(USAGE)),
    }
}

fn install(args: &[String]) -> Result<Output, CtlError> {
    let mut key_env: Option<&String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--api-key-env" => {
                key_env = Some(
                    iter.next()
                        .ok_or_else(|| CtlError::usage("--api-key-env requires a variable name"))?,
                )
            }
            other => return Err(CtlError::usage(format!("unknown argument: {other}"))),
        }
    }
    let key = match key_env {
        Some(name) => Some(
            std::env::var(name)
                .ok()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| CtlError::new("input", format!("{name} is not set or empty")))?,
        ),
        None => None,
    };
    let done = c::install(key.as_deref()).map_err(|e| CtlError::new("council", e))?;
    let human = format!(
        "council {} as '{}'{}",
        if done.created { "installed" } else { "updated" },
        done.id,
        if done.key_stored {
            "; key stored in the vault"
        } else {
            "; set the key with `toolportctl secret set council OPENROUTER_API_KEY`"
        }
    );
    Ok(Output::new(
        json!({"id": done.id, "created": done.created, "keyStored": done.key_stored}),
        human,
    ))
}

fn uninstall(args: &[String]) -> Result<Output, CtlError> {
    let mut purge = false;
    for arg in args {
        match arg.as_str() {
            "--purge-key" => purge = true,
            other => return Err(CtlError::usage(format!("unknown argument: {other}"))),
        }
    }
    let removed = c::uninstall(purge).map_err(|e| CtlError::new("council", e))?;
    let human = match &removed {
        Some(id) => format!("council '{id}' removed"),
        None => "council is not installed".to_string(),
    };
    Ok(Output::new(
        json!({"removed": removed.is_some(), "id": removed, "keyPurged": purge && removed.is_some()}),
        human,
    ))
}

fn doctor(args: &[String]) -> Result<Output, CtlError> {
    if let Some(extra) = args.first() {
        return Err(CtlError::usage(format!("unknown argument: {extra}")));
    }
    let reg = registry::registry_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<registry::Registry>(&t).ok());
    let checks = c::doctor(reg.as_ref());
    let failed = checks.iter().any(|k| !k.ok);
    let mut human = String::new();
    for k in &checks {
        human.push_str(&format!(
            "{:<28} {:<4} {}\n",
            k.name,
            if k.ok { "ok" } else { "FAIL" },
            k.detail
        ));
    }
    let data = json!({
        "checks": checks.iter().map(|k| json!({"name": k.name, "ok": k.ok, "detail": k.detail})).collect::<Vec<_>>(),
    });
    let mut out = Output::new(data, human);
    out.failed = failed;
    Ok(out)
}

fn tools(args: &[String]) -> Result<Output, CtlError> {
    if let Some(extra) = args.first() {
        return Err(CtlError::usage(format!("unknown argument: {extra}")));
    }
    let mut human = String::from("tools\n");
    for t in c::TOOLS {
        human.push_str(&format!(
            "  {:<22} tier {}  {}\n",
            t.name, t.tier, t.summary
        ));
    }
    human.push_str("resources\n");
    for (uri, summary) in c::RESOURCES {
        human.push_str(&format!("  {uri:<22} {summary}\n"));
    }
    Ok(Output::new(c::manifest(), human))
}
