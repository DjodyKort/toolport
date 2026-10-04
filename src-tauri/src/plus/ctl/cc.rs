use super::flags::{switch, value, Dashes, Operands, Spec};
use super::output::{CtlError, Output};
use crate::plus::cc::{list, update, Options, SystemClaude};

const USAGE: &str = "usage: cc list|update [plugin] [--marketplace M] [--dry-run]";

pub(super) const SPEC: Spec = Spec {
    flags: &[
        switch("--dry-run").alias(&["--check"]),
        value("--marketplace").alias(&["-m"]).needs("a name"),
    ],
    operands: Operands::Max(1, USAGE),
    dashes: Dashes::Any,
    ..Spec::PLAIN
};

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    let (sub, rest) = rest.split_first().ok_or_else(|| CtlError::usage(USAGE))?;
    let flags = SPEC.parse(rest)?;
    let opts = Options {
        dry_run: flags.on("--dry-run"),
        marketplace: flags.one("--marketplace").map(String::from),
        plugin: flags.operands().first().cloned(),
        ..Options::default()
    };
    let runner = SystemClaude::from_env();
    let report = match sub.as_str() {
        "list" => list(&runner, &opts),
        "update" => update(&runner, &opts),
        _ => return Err(CtlError::usage(USAGE)),
    }
    .map_err(|e| CtlError::failed("cc", e))?;
    let mut output = Output::new(report.to_value(), report.render());
    output.failed = report.has_errors();
    Ok(output)
}
