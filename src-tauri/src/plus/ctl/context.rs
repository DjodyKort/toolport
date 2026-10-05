//! `toolportctl context loads`: read-only "what loads" report for a launch profile and directory.

use super::flags::{switch, value, Flags, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::plus::context::measure::{
    self, Env, Launcher, MeasureError, MeasureReport, ProcessLauncher, Request,
};
use crate::plus::context::{compact, load_config, loads, Roots};
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

pub(super) const LOADS: Spec = Spec {
    flags: &[
        value("--profile"),
        value("--cwd"),
        switch("--no-lazy"),
        switch("--measured"),
    ],
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
    roots.read_env();
    roots.env_claude_config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
    compact::apply_env(&mut roots);
    let config = load_config(&roots.context_config_path());
    let cwd = match flags.one("--cwd") {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().unwrap_or_else(|_| home.clone()),
    };
    let cached = if flags.on("--measured") {
        crate::registry::conduit_dir().and_then(|dir| {
            let version = ProcessLauncher::from_env().version().ok();
            measure::cached_as_is(&dir, &cwd, version.as_deref())
        })
    } else {
        None
    };
    let options = loads::LoadsOptions {
        no_lazy: flags.on("--no-lazy"),
        context_window: cached.as_ref().map(|(_, info)| info.context_window),
    };
    let mut report =
        loads::what_loads_with(&roots, &config, flags.one("--profile"), &cwd, &options)
            .map_err(|e| CtlError::failed("context_invalid", e))?;
    if let Some((run, info)) = cached {
        report.measured = Some(run);
        report.measured_info = Some(info);
    }
    let mut human = format!(
        "{} tokens loaded in {} (estimate: bytes / 4, good for ordering, not a saving)\n",
        report.total_tokens, report.cwd
    );
    for item in &report.items {
        let mark = match (item.loaded, item.lazy) {
            (true, _) => "+",
            (false, true) => "~",
            (false, false) => "-",
        };
        let via = if item.via.is_empty() {
            String::new()
        } else {
            format!("  via {}", item.via.join(" > "))
        };
        human.push_str(&format!(
            "{mark} {:<12} {:<14} {:>6}  {}{via}\n",
            item.kind, item.source, item.tokens, item.name
        ));
    }
    if report.tokens_lazy > 0 {
        human.push_str(&format!(
            "{} more tokens load only when Claude reads there (~)\n",
            report.tokens_lazy
        ));
    }
    if let (Some(run), Some(info)) = (&report.measured, &report.measured_info) {
        human.push_str(&format!(
            "measured {} tokens at start ({}, Claude Code {}{})\n",
            run.total,
            info.model,
            info.claude_code_version,
            if info.stale { ", stale" } else { "" }
        ));
    }
    if report.partial {
        human.push_str("the search for some files stopped early (partial)\n");
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

pub(super) const MEASURE: Spec = Spec {
    flags: &[
        value("--cwd"),
        value("--without"),
        value("--bundle"),
        value("--model"),
        switch("--force"),
        switch("--yes"),
    ],
    ..LOADS
};

fn measure_error(e: MeasureError) -> CtlError {
    match e {
        MeasureError::Usage(message) => CtlError::usage(message),
        MeasureError::Failed { code, message } => CtlError::failed(code, message),
        MeasureError::Declined(n) => CtlError::failed(
            "confirm_required",
            format!(
                "this needs {n} request(s) to Claude Code, which spend model tokens; pass --yes to go ahead"
            ),
        ),
    }
}

fn describe(report: &MeasureReport) -> String {
    let mut human = String::new();
    for (index, run) in report.runs.iter().enumerate() {
        let tail = match index {
            0 => format!(
                "{} skills, {} agents, {} slash commands",
                run.skills, run.agents, run.slash_commands
            ),
            _ => {
                let d = &report.deltas[index - 1];
                format!("{:+} ({:+.1}%)", d.tokens, d.percent)
            }
        };
        human.push_str(&format!("{:<34} {:>8} tokens  {tail}\n", run.label, run.total));
    }
    human.push_str(&format!(
        "{} on Claude Code {}, measured {}{}\n",
        report.model,
        report.claude_code_version,
        report.measured_at,
        if report.cached { " (cached)" } else { "" }
    ));
    if report.stale {
        human.push_str("stale: Claude Code or a settings file changed since; --force measures again\n");
    }
    for skill in &report.invisible_skills {
        human.push_str(&format!("invisible skill {}: {}\n", skill.name, skill.reason));
    }
    for note in &report.notes {
        human.push_str(&format!("note: {note}\n"));
    }
    human
}

/// Starts nothing without a yes: `--yes`, or an answer at a terminal. Outside a terminal there is
/// nobody to ask, so the measurement stops before the first request.
fn approval<'a>(
    yes: bool,
    tty: bool,
    answers: &'a mut dyn BufRead,
) -> impl FnMut(usize) -> bool + 'a {
    move |requests| {
        if yes {
            return true;
        }
        if !tty {
            return false;
        }
        eprint!(
            "{requests} request(s) to Claude Code will spend model tokens. Continue? [y/N] "
        );
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        answers.read_line(&mut line).is_ok()
            && matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
    }
}

pub(super) fn measure_in(
    rest: &[String],
    env: &Env,
    tty: bool,
    answers: &mut dyn BufRead,
) -> Result<Output, CtlError> {
    let flags = MEASURE.parse(rest)?;
    let cwd = match flags.one("--cwd") {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().unwrap_or_else(|_| env.roots.home.clone()),
    };
    let request = Request {
        cwd,
        without: flags.all("--without"),
        bundle: flags.one("--bundle").map(str::to_string),
        model: flags.one("--model").map(str::to_string),
        force: flags.on("--force"),
    };
    let mut approve = approval(flags.on("--yes"), tty, answers);
    let report = measure::measure(env, &request, &mut approve).map_err(measure_error)?;
    let data =
        serde_json::to_value(&report).map_err(|e| CtlError::failed("encode", e.to_string()))?;
    Ok(Output::new(data, describe(&report)))
}

pub fn measure(rest: &[String]) -> Result<Output, CtlError> {
    let (roots, config) = crate::plus::sources::host_world()
        .ok_or_else(|| CtlError::failed("no_home", "home directory could not be resolved"))?;
    let launcher = ProcessLauncher::from_env();
    let data_dir = crate::registry::conduit_dir();
    let env = Env {
        roots: &roots,
        config: &config,
        data_dir: data_dir.as_deref(),
        launcher: &launcher,
    };
    let stdin = std::io::stdin();
    let tty = stdin.is_terminal() && std::io::stderr().is_terminal();
    measure_in(rest, &env, tty, &mut stdin.lock())
}

pub(super) const STATUS: Spec = Spec {
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

const GROUP_USAGE: &str = "usage: context init|status|client|profile|bundle|use|compose|disable|loads|measure|checkpoint-status|plan|apply|sync (bundle: ls|show|add|edit|rm|apply|undo|status|launch|config; use: <name>|--none [--cwd <dir>] [--dry-run]; compose: [--cwd <dir>]; measure: [--cwd <dir>] [--without plugin:<id>|skill:<name>]... [--bundle <name>] [--model <id>] [--force] [--yes]; plan|apply|sync: [--home <dir>] [--rules] [--no-persist] [--rewrite-zshrc] [--dry-run])";

pub(super) const DEPLOY: Spec = Spec {
    flags: &[
        value("--home"),
        switch("--rules"),
        switch("--no-persist"),
        switch("--rewrite-zshrc"),
    ],
    inline: Inline::Value,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};
pub(super) const DEPLOY_DRY: Spec = Spec {
    flags: &[
        value("--home"),
        switch("--rules"),
        switch("--no-persist"),
        switch("--rewrite-zshrc"),
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
    if flags.on("--rewrite-zshrc") {
        args["rewriteZshrc"] = serde_json::json!(true);
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
    let mut human = format!("plan:\n{}\napply:\n{}", render(&plan), render(&applied));
    let mut data = serde_json::json!({"dryRun": false, "plan": plan, "apply": applied});
    if let Some(bundles) = crate::plus::context::bundle_api::sync_bundles(false) {
        for b in bundles.as_array().into_iter().flatten() {
            human.push_str(&format!(
                "\nbundle {} in {}: {}",
                b["bundle"].as_str().unwrap_or(""),
                b["folder"].as_str().unwrap_or(""),
                b["error"].as_str().unwrap_or(if b["applied"] == true { "applied" } else { "not applied" })
            ));
        }
        data["bundles"] = bundles;
    }
    Ok(Output::new(data, human))
}
