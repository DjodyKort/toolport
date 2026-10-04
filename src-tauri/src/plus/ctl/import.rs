use super::flags::{greedy, switch, value, Dashes, Flag, Flags, Operands, Spec};
use super::output::{CtlError, Output};
use crate::plus::compression::{legacy, store::Paths};
use crate::plus::import_mcpm::{name_map, rename_refs, run, RunOptions};
use serde_json::json;
use std::path::{Path, PathBuf};

const USAGE: &str =
    "usage: import mcpm <config-root> [--dry-run] [--short-ids <file>] [--home <dir>] [--skip-clients] [--prune-orphans] [--name-map --tools <file>]";

const RENAME_USAGE: &str =
    "usage: import rename-refs <config-root> --tools <file> --paths <path>... [--short-ids <file>] [--home <dir>] [--dry-run]";

const TOOLS: Flag = value("--tools").needs("a file");
const SHORT_IDS: Flag = value("--short-ids").needs("a file");
const HOME: Flag = value("--home").needs("a directory");

pub(super) const MCPM: Spec = Spec {
    flags: &[
        switch("--dry-run"),
        switch("--skip-clients"),
        switch("--prune-orphans"),
        switch("--name-map"),
        TOOLS,
        SHORT_IDS,
        HOME,
    ],
    operands: Operands::Max(1, USAGE),
    dashes: Dashes::Any,
    ..Spec::PLAIN
};

pub(super) const RENAME: Spec = Spec {
    flags: &[
        switch("--dry-run"),
        TOOLS,
        SHORT_IDS,
        HOME,
        greedy("--paths").needs("at least one path"),
    ],
    operands: Operands::Max(1, RENAME_USAGE),
    ..MCPM
};

fn run_options(flags: &Flags, usage: &str) -> Result<RunOptions, CtlError> {
    let root = flags
        .operands()
        .first()
        .ok_or_else(|| CtlError::usage(usage))?;
    Ok(RunOptions {
        root: PathBuf::from(root),
        short_ids_path: flags.one("--short-ids").map(PathBuf::from),
        home: flags.one("--home").map(String::from),
        ..RunOptions::default()
    })
}

pub fn rename_refs_cmd(rest: &[String]) -> Result<Output, CtlError> {
    let flags = RENAME.parse(rest)?;
    let opts = run_options(&flags, RENAME_USAGE)?;
    let tools = flags
        .one("--tools")
        .ok_or_else(|| CtlError::usage("--tools is required"))?;
    let paths: Vec<PathBuf> = flags.all("--paths").into_iter().map(PathBuf::from).collect();
    if paths.is_empty() {
        return Err(CtlError::usage("--paths is required"));
    }
    let map = name_map(&opts, &PathBuf::from(tools)).map_err(|e| CtlError::failed("import", e))?;
    let report = rename_refs(&paths, &map, flags.on("--dry-run"))
        .map_err(|e| CtlError::failed("import", e))?;
    Ok(Output::new(report.to_value(), report.summary()))
}

pub fn mcpm(rest: &[String]) -> Result<Output, CtlError> {
    let flags = MCPM.parse(rest)?;
    let opts = RunOptions {
        dry_run: flags.on("--dry-run"),
        write_clients: !flags.on("--skip-clients"),
        prune_orphans: flags.on("--prune-orphans"),
        ..run_options(&flags, USAGE)?
    };
    if flags.on("--name-map") {
        let tools = flags
            .one("--tools")
            .ok_or_else(|| CtlError::usage("--name-map requires --tools <file>"))?;
        let map = name_map(&opts, &PathBuf::from(tools))
            .map_err(|e| CtlError::failed("import", e))?;
        let summary = format!("{} tool names mapped", map.map.len());
        return Ok(Output::new(map.to_value(), summary));
    }
    let plan = run(&opts).map_err(|e| CtlError::failed("import", e))?;
    let mut out = Output::new(plan.to_value(), plan.summary());
    adopt_compression(&mut out, &opts.root, opts.dry_run);
    Ok(out)
}

fn adopt_compression(out: &mut Output, root: &Path, dry_run: bool) {
    let Some(paths) = Paths::from_data_dir() else {
        return;
    };
    let (adoption, warnings) = match legacy::adopt(&paths, root, dry_run) {
        Ok(p) => (p.adoption, p.warnings),
        Err(why) => (
            None,
            vec![format!("could not adopt the compression policy: {why}")],
        ),
    };
    if adoption.is_none() && warnings.is_empty() {
        return;
    }
    if let Some(adoption) = &adoption {
        let verb = if dry_run { "would adopt" } else { "adopted" };
        out.human.push_str(&format!(
            "\ncompression: {verb} the policy from {}",
            adoption.from.display()
        ));
        for note in &adoption.notes {
            out.human.push_str(&format!("\n  {note}"));
        }
        if !dry_run {
            out.human
                .push_str("\n  run `toolportctl compression sync` to write the shims");
        }
    }
    for warning in &warnings {
        out.human.push_str(&format!("\ncompression: {warning}"));
    }
    out.data["compression"] = json!({
        "adopted": adoption.as_ref().map(legacy::Adoption::to_value),
        "applied": adoption.is_some() && !dry_run,
        "warnings": warnings,
    });
}
