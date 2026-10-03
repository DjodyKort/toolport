use super::output::{CtlError, Output};
use crate::plus::cc::{list, update, Options, SystemClaude};

const USAGE: &str = "usage: cc list|update [plugin] [--marketplace M] [--dry-run]";

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    let (sub, rest) = rest.split_first().ok_or_else(|| CtlError::usage(USAGE))?;
    let mut opts = Options::default();
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--dry-run" | "--check" => opts.dry_run = true,
            "--marketplace" | "-m" => {
                let v = iter
                    .next()
                    .ok_or_else(|| CtlError::usage("--marketplace requires a name"))?;
                opts.marketplace = Some(v.clone());
            }
            flag if flag.starts_with('-') => {
                return Err(CtlError::usage(format!("unknown option: {flag}")))
            }
            _ if opts.plugin.is_none() => opts.plugin = Some(arg.clone()),
            _ => return Err(CtlError::usage(USAGE)),
        }
    }
    let runner = SystemClaude::from_env();
    let report = match sub.as_str() {
        "list" => list(&runner, &opts),
        "update" => update(&runner, &opts),
        _ => return Err(CtlError::usage(USAGE)),
    }
    .map_err(|e| CtlError::new("cc", e))?;
    let mut output = Output::new(report.to_value(), report.render());
    output.failed = report.has_errors();
    Ok(output)
}
