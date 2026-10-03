//! `toolportctl compression status|presets|run`. Read-only except that `run` launches
//! `claude`; mutation commands arrive with proxy control.

use super::output::{CtlError, Output};
use crate::plus::compression::launch::{plan_launch, run_plan, Probe, SystemOps};
use crate::plus::compression::model::{CompressionConfig, ProviderName};
use crate::plus::compression::store::{self, Loaded, Paths};
use serde_json::{json, Value};

fn load() -> Result<(Paths, Loaded), CtlError> {
    let paths = Paths::from_data_dir()
        .ok_or_else(|| CtlError::new("no_data_dir", "data directory could not be resolved"))?;
    let loaded = store::read(&paths).map_err(|e| CtlError::new("config_invalid", e))?;
    Ok((paths, loaded))
}

fn no_args(rest: &[String]) -> Result<(), CtlError> {
    match rest.first() {
        Some(extra) => Err(CtlError::usage(format!("unexpected argument: {extra}"))),
        None => Ok(()),
    }
}

fn preset_json(name: &str, config: &CompressionConfig) -> Value {
    let p = config.preset_for(Some(name));
    json!({
        "name": name,
        "mode": p.mode.as_str(),
        "savingsProfile": p.savings_profile,
        "port": p.port,
        "knobCount": p.knobs.len(),
        "snapshotVersion": p.snapshot_version,
    })
}

pub fn status(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let (paths, loaded) = load()?;
    let config = &loaded.config;
    let installed = (config.provider == ProviderName::Headroom)
        .then(|| SystemOps::new().headroom_version())
        .flatten();
    let drift = (config.provider == ProviderName::Headroom)
        .then(|| installed.as_deref() != Some(config.provider_version.pin.as_str()));
    let pv = &config.provider_version;
    let data = json!({
        "configPath": paths.config().to_string_lossy(),
        "configExists": loaded.existed,
        "migrationNotes": loaded.notes,
        "provider": config.provider.as_str(),
        "runtime": config.runtime,
        "preset": preset_json(&config.active_preset, config),
        "scope": config.scope,
        "contexts": config.contexts.len(),
        "pin": {
            "package": pv.package,
            "pin": pv.pin,
            "requirement": pv.requirement(),
            "installed": installed,
            "drift": drift,
        },
        "shims": {"path": paths.shims().to_string_lossy(), "exists": paths.shims().exists()},
    });
    let p = &data["preset"];
    let mut human = format!(
        "provider  {}\nruntime   {}\npreset    {} (mode={}, profile={}, port={})\npin       {} ({})",
        config.provider.as_str(),
        data["runtime"].as_str().unwrap_or(""),
        config.active_preset,
        p["mode"].as_str().unwrap_or(""),
        p["savingsProfile"].as_str().unwrap_or("-"),
        p["port"],
        pv.pin,
        pv.requirement(),
    );
    if let Some(drift) = drift {
        human.push_str(&match (&installed, drift) {
            (Some(v), false) => format!("\nheadroom  {v} (pinned)"),
            (Some(v), true) => format!("\nheadroom  {v} != pin {} (drift)", pv.pin),
            (None, _) => format!("\nheadroom  not on PATH (pin {})", pv.pin),
        });
    }
    if !loaded.existed {
        human.push_str("\n(no compression.json yet: defaults)");
    }
    Ok(Output::new(data, human))
}

pub fn presets(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let (_, loaded) = load()?;
    let config = &loaded.config;
    let rows: Vec<Value> = config
        .presets
        .keys()
        .map(|name| {
            let mut row = preset_json(name, config);
            row["active"] = json!(*name == config.active_preset);
            row
        })
        .collect();
    let mut human = String::from("  preset       mode    savings   port  knobs  snapshot");
    for row in &rows {
        human.push_str(&format!(
            "\n{} {:<12} {:<7} {:<9} {:<5} {:<6} {}",
            if row["active"] == true { "*" } else { " " },
            row["name"].as_str().unwrap_or(""),
            row["mode"].as_str().unwrap_or(""),
            row["savingsProfile"].as_str().unwrap_or("-"),
            row["port"],
            row["knobCount"],
            row["snapshotVersion"].as_str().unwrap_or("unknown"),
        ));
    }
    Ok(Output::new(
        json!({"presets": rows, "active": config.active_preset}),
        human,
    ))
}

struct RunArgs {
    plan_only: bool,
    force: bool,
    cwd: Option<String>,
    claude_args: Vec<String>,
}

/// Leading `--plan`, `--force`, `--cwd <dir>` belong to us; `--` or the first other token
/// starts the passthrough, so `-h` and friends reach claude untouched.
fn parse_run(rest: &[String]) -> Result<RunArgs, CtlError> {
    let mut out = RunArgs {
        plan_only: false,
        force: false,
        cwd: None,
        claude_args: Vec::new(),
    };
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--plan" => out.plan_only = true,
            "--force" => out.force = true,
            "--cwd" => {
                i += 1;
                out.cwd = Some(
                    rest.get(i)
                        .ok_or_else(|| CtlError::usage("--cwd requires a value"))?
                        .clone(),
                );
            }
            "--" => {
                out.claude_args = rest[i + 1..].to_vec();
                return Ok(out);
            }
            other if other.starts_with("--cwd=") => out.cwd = Some(other[6..].to_string()),
            _ => {
                out.claude_args = rest[i..].to_vec();
                return Ok(out);
            }
        }
        i += 1;
    }
    Ok(out)
}

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    let args = parse_run(rest)?;
    let (_, loaded) = load()?;
    let cwd = match args.cwd {
        Some(c) => c,
        None => std::env::current_dir()
            .map_err(|e| CtlError::new("cwd", e.to_string()))?
            .to_string_lossy()
            .into_owned(),
    };
    let mut ops = SystemOps::new();
    let plan = plan_launch(&loaded.config, &cwd, args.force, &args.claude_args, &ops);
    if args.plan_only {
        let human = format!(
            "{} {}\n{}",
            plan.program,
            plan.argv.join(" "),
            if plan.routed {
                format!(
                    "routed via {} (preset {}, port {})",
                    plan.provider.as_str(),
                    plan.preset,
                    plan.ledger.port.unwrap_or(0)
                )
            } else {
                format!("plain (provider {})", plan.provider.as_str())
            }
        );
        let data =
            serde_json::to_value(&plan).map_err(|e| CtlError::new("internal", e.to_string()))?;
        return Ok(Output::new(data, human));
    }
    let outcome = run_plan(plan, &mut ops).map_err(|e| CtlError::new("launch_failed", e))?;
    for warning in &outcome.plan.warnings {
        eprintln!("toolportctl: {warning}");
    }
    std::process::exit(outcome.exit_code);
}
