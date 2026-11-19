//! `toolportctl auth statusline|hook|probe|login`: login health of the servers that sign in
//! outside upstream's own OAuth (stdio servers with an `auth` command, Google Workspace), and the
//! sign-in itself. `statusline` and `hook` only read the status file and are wired to no client.

use std::sync::Arc;

use serde_json::Value;
use toolport_plus_api::Host;

use super::flags::{switch, value, Flags, Inline, Operands, Spec, Unknown};
use super::output::{no_args, CtlError, Output};
use crate::auth::{
    self, AuthRow, AuthStore, LoginError, LoginOptions, ProbeRun, Prober, Selector, StatusFile,
};

const PROBE_USAGE: &str = "usage: auth probe [--server <id>] [--force]";
const LOGIN_USAGE: &str = "usage: auth login <server> [--no-open]";

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

fn read_status(host: &dyn Host) -> StatusFile {
    match auth::auth_dir(host) {
        Some(dir) => AuthStore::new(&dir).read_status(),
        None => StatusFile::default(),
    }
}

fn render(
    host: &dyn Host,
    rest: &[String],
    build: fn(&StatusFile, i64, &[String]) -> Value,
) -> Result<Output, CtlError> {
    no_args(rest)?;
    let status = read_status(host);
    let data = build(
        &status,
        host.now_unix(),
        &auth::visible_servers(host, &status),
    );
    let human = serde_json::to_string(&data).unwrap_or_default();
    Ok(Output::new(data, human))
}

pub fn statusline(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    render(host, rest, auth::statusline)
}

pub fn hook(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    render(host, rest, auth::hook)
}

fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..headers.len())
        .map(|i| {
            rows.iter()
                .map(|row| row[i].chars().count())
                .chain([headers[i].chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: Vec<&str>| {
        cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    let mut lines = vec![line(headers.to_vec())];
    lines.extend(
        rows.iter()
            .map(|row| line(row.iter().map(String::as_str).collect())),
    );
    lines.join("\n")
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

fn selector(flags: &Flags, prober: &Prober) -> Result<Selector, CtlError> {
    let force = flags.on("--force");
    let Some(key) = flags.one("--server") else {
        return Ok(if force { Selector::All } else { Selector::Due });
    };
    prober.selector_for(key).ok_or_else(|| {
        CtlError::not_found(format!(
            "no auth probe is registered for {key}; `toolportctl auth login {key}` shows its sign-in step"
        ))
    })
}

pub fn probe(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = PROBE.parse(rest)?;
    let prober = Prober::new(host).map_err(|e| CtlError::failed("auth_probe", e))?;
    let selector = selector(&flags, &prober)?;
    let run = prober
        .run(&selector, flags.on("--force"))
        .map_err(|e| CtlError::failed("auth_probe", e))?;
    let (status, now) = (prober.status(), host.now_unix());
    let data = auth::report_value(&run, selector.name(), &status, now);
    let rows = auth::rows_for(&status, now, &run.servers());
    let mut output = Output::new(data, probe_human(&run, &rows, prober.registered().len()));
    output.failed = !run.failures.is_empty();
    Ok(output)
}

fn consent_sink() -> auth::UrlSink {
    Arc::new(|url| eprintln!("Open this URL to sign in:\n  {url}"))
}

pub fn login(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LOGIN.parse(rest)?;
    let key = flags.single(LOGIN_USAGE)?;
    let opts = LoginOptions {
        open_browser: !flags.on("--no-open"),
    };
    let report = auth::login(host, key, opts, consent_sink()).map_err(|error| match error {
        LoginError::NotFound(message) => CtlError::not_found(message),
        error @ LoginError::Unsupported { .. } => CtlError::failed("unsupported", error.message()),
        error => CtlError::failed("auth_login", error.message()),
    })?;
    let follow = auth::follow_up(host, &report.server);
    let mut human = report.message.clone();
    if let Some(url) = &report.consent_url {
        human.push_str(&format!("\nConsent URL: {url}"));
    }
    if !follow.rows.is_empty() {
        human.push('\n');
        human.push_str(&rows_table(&follow.rows, &ProbeRun::default()));
    }
    Ok(Output::new(
        auth::login_report_value(&report, &follow),
        human,
    ))
}
