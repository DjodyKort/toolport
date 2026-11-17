//! `toolportctl skills files|fix`: the Files section of a skill and the one safe lint fix, over
//! `skills::api` and `skills::files`.

use super::flags::{switch, value, Flags, Spec};
use super::output::{table, CtlError, Output};
use super::skills::{enter_paths, served};
use super::skills_repo::{spec, str_of, PATH};
use super::skills_state::global_mode;
use crate::skills::api::{self, Args};
use crate::skills::files::skill_files;
use crate::skills::lint::FIX_CHMOD;
use std::path::PathBuf;
use toolport_plus_api::Host;

const FILES_USAGE: &str =
    "usage: skills files <skill> [--repo <dir>] [--home <dir>] [--client <key>]... [--project]";
const FIX_USAGE: &str =
    "usage: skills fix chmod [<skill>...] [--repo <dir>] [--home <dir>] [--dry-run]";

pub(super) const FILES: Spec = spec(
    &[
        PATH,
        value("--home"),
        value("--client"),
        switch("--project"),
        switch("--global"),
    ],
    FILES_USAGE,
);
pub(super) const FIX: Spec = spec(&[PATH, value("--home"), switch("--dry-run")], FIX_USAGE);

fn repo_of(flags: &Flags) -> Option<PathBuf> {
    flags.one("--path").map(PathBuf::from)
}

pub fn files(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = FILES.parse(rest)?;
    let name = match flags.operands() {
        [one] => one.clone(),
        [] => {
            return Err(CtlError::usage(format!(
                "missing skill name\n{FILES_USAGE}"
            )))
        }
        [_, extra, ..] => {
            return Err(CtlError::usage(format!(
                "unexpected argument: {extra}\n{FILES_USAGE}"
            )))
        }
    };
    let global = global_mode(&flags)?;
    let _paths = enter_paths(host, flags.one("--home"));
    let clients = flags.all("--client");
    let args = Args {
        repo: repo_of(&flags),
        name,
        clients: (!clients.is_empty()).then_some(clients),
        global,
        ..Args::default()
    };
    let data = served(skill_files(&args))?;
    let rows: Vec<Vec<String>> = data["files"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|file| {
            vec![
                str_of(file, "status").to_string(),
                str_of(file, "path").to_string(),
                file["size"].to_string(),
                str_of(file, "reason").to_string(),
            ]
        })
        .collect();
    let human = if rows.is_empty() {
        "no files".to_string()
    } else {
        table(&["Status", "File", "Bytes", "Why"], &rows)
    };
    Ok(Output::new(data, human))
}

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
        repo: repo_of(&flags),
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
