//! The skills library as a source: where its clone stands against the remote, and the
//! fast-forward pull and the guarded push.

pub mod remote;
pub mod sync;

use crate::op::OpError;
use crate::skills::api::{self, Args};
use serde_json::{json, Value};
use std::path::Path;

/// A pull that applied, followed by the plan `skills sync` would carry out with the pulled files.
/// Nothing is synced: the plan is a dry run, so the caller reviews it first.
pub fn pull(repo: &Path, dry_run: bool, review: bool) -> Result<Value, OpError> {
    let mut data = sync::pull(repo, dry_run)?;
    if review && !dry_run && data["pulled"] == Value::Bool(true) {
        let plan = api::sync(&Args {
            repo: Some(repo.to_path_buf()),
            dry_run: true,
            ..Args::default()
        });
        data["syncPlan"] = match plan {
            Ok(plan) => plan,
            Err(error) => json!({"error": error.message}),
        };
    }
    Ok(data)
}

#[cfg(all(test, unix))]
mod tests;
