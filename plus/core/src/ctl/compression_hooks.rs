//! `toolportctl compression hooks ...` owns the rtk and read-once entries of Claude Code's
//! `settings.json`; `compression hook` is the wrapper those entries run.

use std::io::{Read, Write};

use super::compression::spec;
use super::flags::{switch, value, Spec};
use super::output::{no_args, CtlError, Output};
use crate::compression::hooks::{self, HOOK_PROVIDERS};
use crate::compression::ledger::now_ms;
use crate::compression::store::Paths;
use serde_json::Value;
use toolport_plus_api::Host;

pub(super) const HOOKS: Spec = spec(&[switch("--dry-run"), value("--provider")]);
pub(super) const PAUSE: Spec = spec(&[value("--for"), value("--until")]);

const USAGE: &str = "usage: toolportctl compression hooks [status | apply | undo | pause | resume]";

fn paths(host: &dyn Host) -> Result<Paths, CtlError> {
    Paths::from_host(host)
        .ok_or_else(|| CtlError::failed("no_data_dir", "data directory could not be resolved"))
}

fn failed(error: String) -> CtlError {
    CtlError::failed("hooks_failed", error)
}

fn provider(flags: &super::flags::Flags) -> Result<Option<String>, CtlError> {
    let Some(id) = flags.one("--provider") else {
        return Ok(None);
    };
    if HOOK_PROVIDERS.iter().any(|p| p.id == id) {
        return Ok(Some(id.to_string()));
    }
    Err(CtlError::usage(format!(
        "unknown hook provider '{id}' (have: {})",
        HOOK_PROVIDERS.map(|p| p.id).join(", ")
    )))
}

fn rows_text(data: &Value) -> Vec<String> {
    data["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            format!(
                "  {} / {}: {}",
                r["provider"].as_str().unwrap_or("-"),
                r["file"].as_str().unwrap_or("-"),
                r["action"].as_str().unwrap_or("-")
            )
        })
        .collect()
}

pub fn hooks(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let (sub, tail) = match rest.first().map(String::as_str) {
        Some(word) if !word.starts_with('-') => (word, &rest[1..]),
        _ => ("status", rest),
    };
    let paths = paths(host)?;
    match sub {
        "status" => {
            no_args(tail)?;
            let data = hooks::status(host, &paths).map_err(failed)?;
            let mut out = vec![if data["paused"] == true {
                "Hooks are paused".to_string()
            } else {
                "Hooks are running".to_string()
            }];
            for e in data["entries"].as_array().into_iter().flatten() {
                out.push(format!(
                    "  {} in {}: {}",
                    e["provider"].as_str().unwrap_or("-"),
                    e["file"].as_str().unwrap_or("-"),
                    e["state"].as_str().unwrap_or("-")
                ));
            }
            for c in data["counters"].as_array().into_iter().flatten() {
                out.push(format!(
                    "  {}: {} calls, median {} ms (mechanical)",
                    c["provider"].as_str().unwrap_or("-"),
                    c["calls"],
                    c["medianLatencyMs"]
                        .as_u64()
                        .map_or("-".to_string(), |v| v.to_string())
                ));
            }
            out.push(format!("  {}", hooks::CANNOT_TELL));
            Ok(Output::new(data, out.join("\n")))
        }
        "apply" | "undo" => {
            let flags = HOOKS.parse(tail)?;
            no_args(flags.operands())?;
            let only = provider(&flags)?;
            let dry = flags.on("--dry-run");
            let data = if sub == "apply" {
                hooks::apply(host, &paths, only.as_deref(), dry)
            } else {
                hooks::undo(host, &paths, only.as_deref(), dry)
            }
            .map_err(failed)?;
            let mut out = vec![format!(
                "{} hook entries{}",
                if sub == "apply" {
                    "Wrapping"
                } else {
                    "Restoring"
                },
                if dry { " (dry run)" } else { "" }
            )];
            out.extend(rows_text(&data));
            Ok(Output::new(data, out.join("\n")))
        }
        "pause" => {
            let flags = PAUSE.parse(tail)?;
            no_args(flags.operands())?;
            let now = now_ms();
            let end = hooks::pause_end(flags.one("--for"), flags.one("--until"), now)
                .map_err(CtlError::usage)?;
            let data = hooks::set_paused(&paths, true, end, now).map_err(failed)?;
            let text = match data["pausedUntil"].as_str() {
                Some(until) => format!(
                    "Hooks paused until {until}: the wrapped entries answer nothing, then answer again by themselves"
                ),
                None => "Hooks paused: the wrapped entries answer nothing until resumed".into(),
            };
            Ok(Output::new(data, text))
        }
        "resume" => {
            no_args(tail)?;
            let data = hooks::set_paused(&paths, false, None, now_ms()).map_err(failed)?;
            Ok(Output::new(data, "Hooks resumed".to_string()))
        }
        _ => Err(CtlError::usage(USAGE)),
    }
}

/// `compression hook <id> -- <command...>`: run by Claude Code, never by a person. Prints the
/// wrapped command's own output and exits with its code.
pub fn hook(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let split = rest.iter().position(|a| a == "--");
    let (Some(id), Some(split)) = (rest.first().filter(|a| *a != "--"), split) else {
        return Err(CtlError::usage(
            "usage: toolportctl compression hook <provider> -- <command...>",
        ));
    };
    let paths = paths(host)?;
    let mut input = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut input);
    let out = hooks::run_wrapper(&paths, id, &rest[split + 1..], &input);
    let _ = std::io::stdout().write_all(&out.stdout);
    let _ = std::io::stderr().write_all(&out.stderr);
    let _ = std::io::stdout().flush();
    std::process::exit(out.code);
}
