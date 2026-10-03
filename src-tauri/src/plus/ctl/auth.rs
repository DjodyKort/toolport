use super::output::{CtlError, Output};
use crate::plus::auth::surfaces;
use crate::plus::auth::{Clock, SystemClock};
use serde_json::Value;

fn render(
    rest: &[String],
    build: fn(&crate::plus::auth::StatusFile, i64) -> Value,
) -> Result<Output, CtlError> {
    if let Some(extra) = rest.first() {
        return Err(CtlError::usage(format!("unexpected argument: {extra}")));
    }
    let status = match surfaces::auth_dir() {
        Some(dir) => surfaces::read_status(&dir),
        None => Default::default(),
    };
    let data = build(&status, SystemClock.now());
    let human = serde_json::to_string(&data).unwrap_or_default();
    Ok(Output::new(data, human))
}

pub fn statusline(rest: &[String]) -> Result<Output, CtlError> {
    render(rest, surfaces::statusline)
}

pub fn hook(rest: &[String]) -> Result<Output, CtlError> {
    render(rest, surfaces::hook)
}
