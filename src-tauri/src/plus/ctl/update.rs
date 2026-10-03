use super::output::{CtlError, Output};
use crate::plus::update::{execute, Mode, Options};

const USAGE: &str = "usage: update [server] [--check|--apply|--init|--dry-run] [--allow-commands] \
                     [--allow-unverified] [--force] [--repo owner/repo]";

pub fn update(rest: &[String]) -> Result<Output, CtlError> {
    let mut opts = Options::new(Mode::Check);
    let (mut apply, mut init, mut check) = (false, false, false);
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--check" | "-c" => check = true,
            "--apply" => apply = true,
            "--init" => init = true,
            "--dry-run" => opts.dry_run = true,
            "--allow-commands" => opts.allow_commands = true,
            "--allow-unverified" => opts.allow_unverified = true,
            "--force" => opts.force = true,
            "--repo" => {
                let v = iter
                    .next()
                    .ok_or_else(|| CtlError::usage("--repo requires owner/repo"))?;
                opts.repo = Some(v.clone());
            }
            flag if flag.starts_with('-') => {
                return Err(CtlError::usage(format!("unknown option: {flag}")))
            }
            _ if opts.server.is_none() => opts.server = Some(arg.clone()),
            _ => return Err(CtlError::usage(USAGE)),
        }
    }
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
    let report = execute(&opts).map_err(|e| CtlError::new("update", e))?;
    let mut output = Output::new(report.to_value(), report.render());
    output.failed = report.has_errors();
    Ok(output)
}
