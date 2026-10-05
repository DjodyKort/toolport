use std::sync::Arc;

use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{no_args, table, CtlError, Output};
use crate::plus::auth::login::{self, LoginError, LoginOptions};
use crate::plus::auth::scan::{self, ProbeRun, Selector};
use crate::plus::auth::surfaces::{self, AuthRow};
use crate::plus::auth::{AuthProber, Clock, StatusFile, SystemClock};
use crate::plus::servers;
use serde_json::Value;

const PROBE_USAGE: &str = "usage: auth probe [--server <id>] [--force]";
const LOGIN_USAGE: &str = "usage: auth login <server> [--no-open]";
pub const GROUP_USAGE: &str = "usage: auth statusline|hook|probe|login (probe: [--server <id>] [--force]; login: <server> [--no-open])";

pub(super) const PROBE: Spec = Spec {
    flags: &[
        value("--server").needs("a server id").nonempty(),
        switch("--force"),
    ],
    inline: Inline::Value,
    unknown: Unknown::ArgumentUsage(PROBE_USAGE),
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const LOGIN: Spec = Spec {
    flags: &[switch("--no-open")],
    inline: Inline::Value,
    unknown: Unknown::ArgumentUsage(LOGIN_USAGE),
    operands: Operands::Max(1, LOGIN_USAGE),
    ..Spec::PLAIN
};

fn read_status() -> StatusFile {
    match surfaces::auth_dir() {
        Some(dir) => surfaces::read_status(&dir),
        None => Default::default(),
    }
}

fn render(
    rest: &[String],
    build: fn(&StatusFile, i64, &[String]) -> Value,
) -> Result<Output, CtlError> {
    no_args(rest)?;
    let status = read_status();
    // An unreadable registry must not blank the warnings, so fall back to what is cached.
    let registered = scan::probed_servers()
        .unwrap_or_else(|_| status.servers.keys().cloned().collect());
    let data = build(&status, SystemClock.now(), &registered);
    let human = serde_json::to_string(&data).unwrap_or_default();
    Ok(Output::new(data, human))
}

pub fn statusline(rest: &[String]) -> Result<Output, CtlError> {
    render(rest, surfaces::statusline)
}

pub fn hook(rest: &[String]) -> Result<Output, CtlError> {
    render(rest, surfaces::hook)
}

pub fn group(_: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

fn probe_cell(run: &ProbeRun, server: &str) -> String {
    if let Some(failure) = run.failures.iter().find(|f| f.server == server) {
        return format!("failed: {}", failure.error);
    }
    match run.reports.iter().find(|r| r.server == server) {
        Some(report) => match report.skipped {
            Some(reason) => format!("skipped ({reason})"),
            None => "ran".to_string(),
        },
        None => String::new(),
    }
}

fn fix_cell(row: &AuthRow) -> String {
    match &row.fix {
        Some(fix) => fix.command.clone().unwrap_or_else(|| fix.label.clone()),
        None => String::new(),
    }
}

fn rows_table(rows: &[AuthRow], run: &ProbeRun) -> String {
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            vec![
                row.server.clone(),
                row.state.to_string(),
                row.reason.clone(),
                probe_cell(run, &row.server),
                fix_cell(row),
            ]
        })
        .collect();
    table(&["server", "state", "reason", "probe", "fix"], &cells)
}

fn probe_human(run: &ProbeRun, rows: &[AuthRow], registered: usize) -> String {
    let skipped = run.reports.len() - run.probed();
    let mut human = format!(
        "auth probe: {} probed, {skipped} skipped, {} failed",
        run.probed(),
        run.failures.len()
    );
    if run.reports.is_empty() && run.failures.is_empty() {
        human = format!("auth probe: nothing due ({registered} registered)");
    }
    if !rows.is_empty() {
        human.push('\n');
        human.push_str(&rows_table(rows, run));
    }
    for failure in run
        .failures
        .iter()
        .filter(|f| !rows.iter().any(|r| r.server == f.server))
    {
        human.push_str(&format!("\nfailed: {}: {}", failure.server, failure.error));
    }
    human
}

fn selector(flags: &super::flags::Flags, prober: &AuthProber) -> Result<Selector, CtlError> {
    let force = flags.on("--force");
    let Some(key) = flags.one("--server") else {
        return Ok(if force { Selector::All } else { Selector::Due });
    };
    let by_name = || {
        let registry = scan::read_registry().ok()?;
        let server = servers::find(&registry, key)?;
        prober.has_probe(&server.id).then(|| server.id.clone())
    };
    let id = if prober.has_probe(key) {
        Some(key.to_string())
    } else {
        by_name()
    };
    id.map(Selector::One).ok_or_else(|| {
        CtlError::not_found(format!(
            "no auth probe is registered for {key}; `toolportctl auth login {key}` shows its sign-in step"
        ))
    })
}

pub fn probe(rest: &[String]) -> Result<Output, CtlError> {
    let flags = PROBE.parse(rest)?;
    let prober = scan::default_prober().map_err(|e| CtlError::failed("auth_probe", e))?;
    let selector = selector(&flags, &prober)?;
    let run = scan::run(&prober, &selector, flags.on("--force"), scan::MAX_PARALLEL)
        .map_err(|e| CtlError::failed("auth_probe", e))?;
    let (status, now) = (read_status(), SystemClock.now());
    let data = scan::report_value(&run, selector.name(), &status, now);
    let rows = surfaces::rows_for(&status, now, &run.servers());
    let mut output = Output::new(data, probe_human(&run, &rows, prober.registered().len()));
    output.failed = !run.failures.is_empty();
    Ok(output)
}

fn consent_sink() -> login::UrlSink {
    Arc::new(|url| eprintln!("Open this URL to sign in:\n  {url}"))
}

pub fn login(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LOGIN.parse(rest)?;
    let key = flags.single(LOGIN_USAGE)?;
    let opts = LoginOptions {
        open_browser: !flags.on("--no-open"),
    };
    let report = login::login(key, opts, consent_sink()).map_err(|error| match error {
        LoginError::NotFound(message) => CtlError::not_found(message),
        error @ LoginError::Unsupported { .. } => CtlError::failed("unsupported", error.message()),
        LoginError::Failed(message) => CtlError::failed("auth_login", message),
    })?;
    let follow = login::follow_up(&report.server);
    let mut human = report.message.clone();
    if let Some(url) = &report.consent_url {
        human.push_str(&format!("\nConsent URL: {url}"));
    }
    if let Some(run) = &follow.run {
        human.push('\n');
        human.push_str(&rows_table(&follow.rows, run));
    }
    Ok(Output::new(login::report_value(&report, &follow), human))
}
