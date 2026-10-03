use super::flags::{switch, value, Dashes, Operands, Spec};
use super::output::{CtlError, Output};
use crate::plus::update::{execute, Mode, Options};

const USAGE: &str = "usage: update [server] [--check|--apply|--init|--dry-run] [--allow-commands] \
                     [--allow-unverified] [--force] [--repo owner/repo]";

const SPEC: Spec = Spec {
    flags: &[
        switch("--check").alias(&["-c"]),
        switch("--apply"),
        switch("--init"),
        switch("--dry-run"),
        switch("--allow-commands"),
        switch("--allow-unverified"),
        switch("--force"),
        value("--repo").needs("owner/repo"),
    ],
    operands: Operands::Max(1, USAGE),
    dashes: Dashes::Any,
    ..Spec::PLAIN
};

pub fn update(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SPEC.parse(rest)?;
    let mut opts = Options::new(Mode::Check);
    opts.server = flags.operands().first().cloned();
    opts.dry_run = flags.on("--dry-run");
    opts.allow_commands = flags.on("--allow-commands");
    opts.allow_unverified = flags.on("--allow-unverified");
    opts.force = flags.on("--force");
    opts.repo = flags.one("--repo").map(String::from);
    let (apply, init, check) = (flags.on("--apply"), flags.on("--init"), flags.on("--check"));
    if [apply, init, check].iter().filter(|f| **f).count() > 1 {
        return Err(CtlError::usage(
            "--check, --apply and --init are mutually exclusive",
        ));
    }
    opts.mode = if init {
        Mode::Init
    } else if opts.dry_run {
        Mode::DryRun
    } else if apply {
        Mode::Apply
    } else {
        Mode::Check
    };
    let report = execute(&opts).map_err(|e| CtlError::failed("update", e))?;
    let mut output = Output::new(report.to_value(), report.render());
    output.failed = report.has_errors();
    Ok(output)
}
