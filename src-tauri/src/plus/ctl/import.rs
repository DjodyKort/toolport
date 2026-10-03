use super::output::{CtlError, Output};
use crate::plus::import_mcpm::{run, RunOptions};
use std::path::PathBuf;

const USAGE: &str =
    "usage: import mcpm <config-root> [--dry-run] [--short-ids <file>] [--home <dir>] [--skip-clients] [--prune-orphans]";

pub fn mcpm(rest: &[String]) -> Result<Output, CtlError> {
    let mut opts = RunOptions {
        write_clients: true,
        ..RunOptions::default()
    };
    let mut root: Option<PathBuf> = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--dry-run" => opts.dry_run = true,
            "--skip-clients" => opts.write_clients = false,
            "--prune-orphans" => opts.prune_orphans = true,
            "--short-ids" => {
                let v = iter
                    .next()
                    .ok_or_else(|| CtlError::usage("--short-ids requires a file"))?;
                opts.short_ids_path = Some(PathBuf::from(v));
            }
            "--home" => {
                let v = iter
                    .next()
                    .ok_or_else(|| CtlError::usage("--home requires a directory"))?;
                opts.home = Some(v.clone());
            }
            flag if flag.starts_with('-') => {
                return Err(CtlError::usage(format!("unknown option: {flag}")))
            }
            _ if root.is_none() => root = Some(PathBuf::from(arg)),
            _ => return Err(CtlError::usage(USAGE)),
        }
    }
    opts.root = root.ok_or_else(|| CtlError::usage(USAGE))?;
    let plan = run(&opts).map_err(|e| CtlError::new("import", e))?;
    Ok(Output::new(plan.to_value(), plan.summary()))
}
