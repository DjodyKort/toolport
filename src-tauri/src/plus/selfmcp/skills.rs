//! The read and sync tools of `skills_*`: adapters over the typed operations in `skills::api`.

use super::ToolError;
use crate::plus::skills::api::{self, Args};
use serde_json::Value;

type Outcome = Result<Value, ToolError>;

pub(super) fn list(args: &Value) -> Outcome {
    Ok(api::list_skills(&Args::from_json(args))?)
}

pub(super) fn get(args: &Value) -> Outcome {
    Ok(api::get(&Args::from_json(args))?)
}

pub(super) fn lint(args: &Value) -> Outcome {
    Ok(api::lint(&Args::from_json(args))?)
}

pub(super) fn status(args: &Value) -> Outcome {
    Ok(api::status(&Args::from_json(args))?)
}

pub(super) fn diff(args: &Value) -> Outcome {
    Ok(api::diff(&Args::from_json(args))?)
}

pub(super) fn sync(args: &Value) -> Outcome {
    Ok(api::sync(&Args::from_json(args))?)
}

pub(super) fn list_transpilers(_args: &Value) -> Outcome {
    Ok(api::transpilers())
}
