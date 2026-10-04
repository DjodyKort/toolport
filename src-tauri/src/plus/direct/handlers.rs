//! `plus.client.directAdd|directRm|directLs`: the `data` of the matching `toolportctl client
//! direct --json` envelope. `dryRun` plans the change and writes nothing.

use crate::plus::args::{flag, str_arg};
use serde_json::Value;

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    str_arg(args, key).ok_or_else(|| format!("{key} is required"))
}

pub fn add_handler(args: Value) -> Result<Value, String> {
    let outcome = super::add(
        required(&args, "server")?,
        required(&args, "client")?,
        flag(&args, "force"),
        flag(&args, "dryRun"),
    )?;
    Ok(outcome.to_value())
}

pub fn remove_handler(args: Value) -> Result<Value, String> {
    let outcome = super::remove(
        required(&args, "server")?,
        required(&args, "client")?,
        flag(&args, "force"),
        flag(&args, "dryRun"),
    )?;
    Ok(outcome.to_value())
}

pub fn list_handler(args: Value) -> Result<Value, String> {
    Ok(super::list_value(&super::list(str_arg(&args, "client"))?))
}
