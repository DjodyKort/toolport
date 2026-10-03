use super::output::{CtlError, Output};
use crate::plus::import_mcpm::{name_map, run, RunOptions};
use std::path::PathBuf;

const USAGE: &str =
    "usage: import mcpm <config-root> [--dry-run] [--short-ids <file>] [--home <dir>] [--skip-clients] [--prune-orphans] [--name-map --tools <file>]";

pub fn mcpm(rest: &[String]) -> Result<Output, CtlError> {
    let mut opts = RunOptions {
        write_clients: true,
        ..RunOptions::default()
    };
    let mut root: Option<PathBuf> = None;
    let mut want_map = false;
    let mut tools: Option<PathBuf> = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--dry-run" => opts.dry_run = true,
            "--skip-clients" => opts.write_clients = false,
            "--prune-orphans" => opts.prune_orphans = true,
            "--name-map" => want_map = true,
            "--tools" => {
                let v = iter
                    .next()
                    .ok_or_else(|| CtlError::usage("--tools requires a file"))?;
                tools = Some(PathBuf::from(v));
            }
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
    if want_map {
        let tools = tools.ok_or_else(|| CtlError::usage("--name-map requires --tools <file>"))?;
        let map = name_map(&opts, &tools).map_err(|e| CtlError::new("import", e))?;
        let summary = format!("{} tool names mapped", map.map.len());
        return Ok(Output::new(map.to_value(), summary));
    }
    let plan = run(&opts).map_err(|e| CtlError::new("import", e))?;
    Ok(Output::new(plan.to_value(), plan.summary()))
}
