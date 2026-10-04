//! `plus.profile.*` and `plus.client.edit|import` handlers. Each returns the `data` of the matching
//! `toolportctl --json` envelope; `dryRun` plans the change and writes nothing.

use super::client::{self, ImportSpec, ProfileOp};
use super::{EditSpec, ServerOp};
use crate::plus::args::{flag, str_arg};
use serde_json::Value;

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    str_arg(args, key).ok_or_else(|| format!("{key} is required"))
}

/// A list argument: a JSON array of strings or one comma-separated string.
fn names(args: &Value, key: &str) -> Result<Option<Vec<String>>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(
            text.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
        )),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .ok_or_else(|| format!("{key} must hold strings"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(format!("{key} must be a list of names")),
    }
}

pub fn list_handler(_args: Value) -> Result<Value, String> {
    Ok(super::list_value(&super::list()?))
}

pub fn create_handler(args: Value) -> Result<Value, String> {
    let created = super::create(
        required(&args, "name")?,
        flag(&args, "force"),
        flag(&args, "dryRun"),
    )?;
    Ok(created.to_value())
}

pub fn edit_handler(args: Value) -> Result<Value, String> {
    let ops: Vec<ServerOp> = [
        names(&args, "servers")?.map(ServerOp::Set),
        names(&args, "setServers")?.map(ServerOp::Set),
        names(&args, "addServers")?.map(ServerOp::Add),
        names(&args, "removeServers")?.map(ServerOp::Remove),
    ]
    .into_iter()
    .flatten()
    .collect();
    if ops.len() > 1 {
        return Err("use one of servers, addServers or removeServers".into());
    }
    let spec = EditSpec {
        name: str_arg(&args, "name").map(String::from),
        servers: ops.into_iter().next(),
    };
    let edited = super::edit(required(&args, "profile")?, &spec, flag(&args, "dryRun"))?;
    Ok(edited.to_value())
}

pub fn remove_handler(args: Value) -> Result<Value, String> {
    let removed = super::remove(
        required(&args, "profile")?,
        flag(&args, "keepClients"),
        flag(&args, "dryRun"),
    )?;
    Ok(removed.to_value())
}

pub fn client_edit_handler(args: Value) -> Result<Value, String> {
    let ops: Vec<ProfileOp> = [
        names(&args, "addProfiles")?.map(ProfileOp::Add),
        names(&args, "removeProfiles")?.map(ProfileOp::Remove),
        names(&args, "setProfiles")?.map(ProfileOp::Set),
    ]
    .into_iter()
    .flatten()
    .collect();
    if ops.len() > 1 {
        return Err("use one of addProfiles, removeProfiles or setProfiles".into());
    }
    let edited = client::edit(
        required(&args, "client")?,
        ops.first(),
        flag(&args, "force"),
        flag(&args, "dryRun"),
    )?;
    Ok(edited.to_value())
}

pub fn client_import_handler(args: Value) -> Result<Value, String> {
    let spec = ImportSpec {
        select: names(&args, "select")?.unwrap_or_default(),
        all: flag(&args, "all"),
        profile: str_arg(&args, "profile").map(String::from),
    };
    if spec.all && !spec.select.is_empty() {
        return Err("use select or all, not both".into());
    }
    let imported = client::import(required(&args, "client")?, &spec, flag(&args, "dryRun"))?;
    Ok(imported.to_value())
}
