//! `toolportctl skills fix`: the one safe lint fix, over `skills::api`.

use super::flags::{switch, value, Spec};
use super::output::{CtlError, Output};
use super::skills::{enter_paths, served};
use super::skills_repo::{spec, str_of, PATH};
use crate::skills::api::{self, Args};
use crate::skills::lint::FIX_CHMOD;
use std::path::PathBuf;
use toolport_plus_api::Host;

const FIX_USAGE: &str =
    "usage: skills fix chmod [<skill>...] [--repo <dir>] [--home <dir>] [--dry-run]";

pub(super) const FIX: Spec = spec(&[PATH, value("--home"), switch("--dry-run")], FIX_USAGE);

pub fn fix(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = FIX.parse(rest)?;
    let Some((kind, names)) = flags.operands().split_first() else {
        return Err(CtlError::usage(format!("missing fix\n{FIX_USAGE}")));
    };
    if kind != FIX_CHMOD {
        return Err(CtlError::usage(format!(
            "unknown fix '{kind}'; the only safe fix is {FIX_CHMOD}\n{FIX_USAGE}"
        )));
    }
    let _paths = enter_paths(host, flags.one("--home"));
    let dry_run = flags.on("--dry-run");
    let args = Args {
        repo: flags.one("--path").map(PathBuf::from),
        names: (!names.is_empty()).then(|| names.to_vec()),
        dry_run,
        ..Args::default()
    };
    let data = served(api::fix(&args, kind))?;
    let verb = if dry_run {
        "Would make executable"
    } else {
        "Made executable"
    };
    let mut lines: Vec<String> = data["fixed"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            format!(
                "{verb}: {}/{} ({} -> {})",
                str_of(row, "skill"),
                str_of(row, "file"),
                str_of(row, "from"),
                str_of(row, "to")
            )
        })
        .collect();
    if lines.is_empty() {
        lines.push("No hook script needs the fix".to_string());
    }
    Ok(Output::new(data, lines.join("\n")))
}
