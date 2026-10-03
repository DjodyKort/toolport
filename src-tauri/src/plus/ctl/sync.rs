//! `toolportctl sync init|push|pull|diff|status|reset|rotate-passphrase|add-project|remove-project|
//! git-sync|migrate`. Passphrases come from `--passphrase-env VAR` or `--passphrase-stdin`.

use super::output::{CtlError, Output};
use crate::plus::sync::handlers as h;
use serde_json::{json, Map, Value};
use std::io::Read;

const USAGE: &str = "usage: sync <init|push|pull|diff|status|reset|rotate-passphrase|add-project|remove-project|git-sync|migrate> [options]";

struct Spec {
    bools: &'static [(&'static str, &'static str)],
    values: &'static [(&'static str, &'static str)],
    positional: &'static [&'static str],
}

fn spec(sub: &str) -> Option<Spec> {
    let s = |bools, values, positional| {
        Some(Spec {
            bools,
            values,
            positional,
        })
    };
    match sub {
        "init" => s(
            &[("--reconfigure", "reconfigure")],
            &[
                ("--repo", "repo"),
                ("--branch", "branch"),
                ("--machine-id", "machineId"),
            ],
            &[],
        ),
        "push" => s(
            &[
                ("--include-projects", "includeProjects"),
                ("--dry-run", "dryRun"),
            ],
            &[],
            &[],
        ),
        "pull" => s(
            &[
                ("--include-projects", "includeProjects"),
                ("--force", "force"),
                ("--dry-run", "dryRun"),
                ("--no-resolve", "noResolve"),
                ("--run-setup", "runSetup"),
            ],
            &[],
            &[],
        ),
        "diff" | "status" | "reset" | "rotate-passphrase" => s(&[], &[], &[]),
        "add-project" => s(&[], &[("--name", "name"), ("--files", "files")], &["path"]),
        "remove-project" => s(&[], &[], &["name"]),
        "git-sync" => s(
            &[
                ("--auto", "auto"),
                ("--status", "status"),
                ("--clear", "clear"),
            ],
            &[("--repo", "repo"), ("--branch", "branch")],
            &[],
        ),
        "migrate" => s(
            &[("--include-projects", "includeProjects")],
            &[],
            &["bundleDir"],
        ),
        _ => None,
    }
}

fn secret_args(rest: &[String], target: &str) -> Result<(Vec<String>, Option<String>), CtlError> {
    let mut remaining = Vec::new();
    let mut secret = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--passphrase-env" => {
                let var = iter
                    .next()
                    .ok_or_else(|| CtlError::usage("--passphrase-env requires a variable name"))?;
                let value = std::env::var(var).map_err(|_| {
                    CtlError::usage(format!("environment variable {var} is not set"))
                })?;
                secret = Some(value);
            }
            "--passphrase-stdin" => {
                let mut line = String::new();
                std::io::stdin()
                    .read_to_string(&mut line)
                    .map_err(|e| CtlError::new("io", e.to_string()))?;
                secret = Some(line.trim_end_matches(['\r', '\n']).to_string());
            }
            _ => remaining.push(arg.clone()),
        }
    }
    let _ = target;
    Ok((remaining, secret))
}

fn parse_args(sub: &str, spec: &Spec, rest: &[String]) -> Result<Map<String, Value>, CtlError> {
    let (rest, secret) = secret_args(rest, sub)?;
    let mut args = Map::new();
    let mut positional = Vec::new();
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        if let Some((_, key)) = spec.bools.iter().find(|(f, _)| f == arg) {
            args.insert((*key).into(), Value::Bool(true));
        } else if let Some((flag, key)) = spec.values.iter().find(|(f, _)| f == arg) {
            let value = iter
                .next()
                .ok_or_else(|| CtlError::usage(format!("{flag} requires a value")))?;
            let value = if *key == "files" {
                json!(value
                    .split(',')
                    .map(str::trim)
                    .filter(|f| !f.is_empty())
                    .collect::<Vec<_>>())
            } else {
                Value::String(value.clone())
            };
            args.insert((*key).into(), value);
        } else if arg.starts_with('-') {
            return Err(CtlError::usage(format!("unknown option: {arg}")));
        } else {
            positional.push(arg.clone());
        }
    }
    if positional.len() > spec.positional.len() {
        return Err(CtlError::usage(USAGE));
    }
    for (name, value) in spec.positional.iter().zip(positional) {
        args.insert((*name).into(), Value::String(value));
    }
    if let Some(secret) = secret {
        let key = if sub == "rotate-passphrase" {
            "newPassphrase"
        } else {
            "passphrase"
        };
        args.insert(key.into(), Value::String(secret));
    }
    Ok(args)
}

fn render(sub: &str, data: &Value) -> String {
    let list = |key: &str| -> Vec<String> {
        data.get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    match sub {
        "push" => format!(
            "{} {} file(s)",
            if data["dryRun"] == true {
                "Would push"
            } else {
                "Pushed"
            },
            list("entries").len()
        ),
        "pull" if data["noRemote"] == true => "No sync data found on the remote.".into(),
        "pull" => {
            let conflicts = data["conflicts"].as_array().map(Vec::len).unwrap_or(0);
            let mut text = format!(
                "Applied {} file(s), {conflicts} conflict(s)",
                list("applied").len()
            );
            if let Some(c) = data["conflicts"].as_array() {
                for item in c {
                    text.push_str(&format!(
                        "\n  ! {} (remote saved as {})",
                        item["entryKey"].as_str().unwrap_or(""),
                        item["remoteFileSavedAs"].as_str().unwrap_or("")
                    ));
                }
            }
            text
        }
        "diff" if data["noRemote"] == true => "No remote sync data found.".into(),
        "diff" => {
            let c = &data["changes"];
            let mut lines = Vec::new();
            for (key, mark) in [
                ("new", "+"),
                ("modified", "~"),
                ("removed", "-"),
                ("conflicts", "!"),
            ] {
                for item in c[key].as_array().into_iter().flatten() {
                    lines.push(format!("  {mark} {} ({key})", item.as_str().unwrap_or("")));
                }
            }
            if lines.is_empty() {
                "Everything is in sync.".into()
            } else {
                lines.join("\n")
            }
        }
        "status" => serde_json::to_string_pretty(data).unwrap_or_default(),
        "reset" => format!("Removed {} item(s)", list("removed").len()),
        "rotate-passphrase" => format!(
            "Rotated {} blob(s). Other machines must re-run init with the new passphrase.",
            data["rotated"]
        ),
        "init" => format!(
            "Sync configured for machine {}",
            data["machineId"].as_str().unwrap_or("")
        ),
        "migrate" => format!("Imported {} file(s)", list("written").len()),
        _ => serde_json::to_string_pretty(data).unwrap_or_default(),
    }
}

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    let sub = rest.first().ok_or_else(|| CtlError::usage(USAGE))?;
    let spec = spec(sub).ok_or_else(|| CtlError::usage(USAGE))?;
    let args = Value::Object(parse_args(sub, &spec, &rest[1..])?);
    let handler: fn(Value) -> Result<Value, String> = match sub.as_str() {
        "init" => h::init_handler,
        "push" => h::push_handler,
        "pull" => h::pull_handler,
        "diff" => h::diff_handler,
        "status" => h::status_handler,
        "reset" => h::reset_handler,
        "rotate-passphrase" => h::rotate_handler,
        "add-project" => h::add_project_handler,
        "remove-project" => h::remove_project_handler,
        "git-sync" => h::git_sync_handler,
        _ => h::migrate_handler,
    };
    let data = handler(args).map_err(|e| {
        if e.starts_with("missing argument") {
            CtlError::usage(e)
        } else {
            CtlError::new("sync", e)
        }
    })?;
    let human = render(sub, &data);
    Ok(Output::new(data, human))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn usage_errors_are_reported() {
        assert_eq!(run(&[]).err().unwrap().code, "usage");
        assert_eq!(run(&strings(&["bogus"])).err().unwrap().code, "usage");
        assert_eq!(
            run(&strings(&["push", "--nope"])).err().unwrap().code,
            "usage"
        );
        assert_eq!(
            run(&strings(&["init", "--repo"])).err().unwrap().code,
            "usage"
        );
        assert_eq!(run(&strings(&["init"])).err().unwrap().code, "usage");
        assert_eq!(
            run(&strings(&["remove-project"])).err().unwrap().code,
            "usage"
        );
    }

    #[test]
    fn flags_map_to_handler_arguments() {
        let s = spec("add-project").unwrap();
        let args = parse_args(
            "add-project",
            &s,
            &strings(&["/tmp/x", "--name", "app", "--files", "A.md, B.md"]),
        )
        .unwrap();
        assert_eq!(args["path"], "/tmp/x");
        assert_eq!(args["files"], json!(["A.md", "B.md"]));
        let s = spec("pull").unwrap();
        let args = parse_args("pull", &s, &strings(&["--force", "--no-resolve"])).unwrap();
        assert_eq!(args["force"], true);
        assert_eq!(args["noResolve"], true);
    }

    #[test]
    fn passphrase_flag_never_takes_a_literal_value() {
        std::env::set_var("TOOLPORT_SYNC_TEST_PASS", "synthetic passphrase");
        let s = spec("rotate-passphrase").unwrap();
        let args = parse_args(
            "rotate-passphrase",
            &s,
            &strings(&["--passphrase-env", "TOOLPORT_SYNC_TEST_PASS"]),
        )
        .unwrap();
        assert_eq!(args["newPassphrase"], "synthetic passphrase");
        let err = parse_args(
            "init",
            &spec("init").unwrap(),
            &strings(&["--passphrase-env", "TOOLPORT_SYNC_UNSET_VAR"]),
        )
        .err()
        .unwrap();
        assert!(!err.message.contains("synthetic"));
    }
}
