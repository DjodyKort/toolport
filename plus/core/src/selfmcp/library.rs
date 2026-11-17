//! `library_status` (tier 1), `library_pull` (tier 2) and `skills_git_push` (tier 4): the same
//! operations as `toolportctl library status|pull|push`, over `crate::library`.

use super::ToolError;
use crate::args::{flag, flag_or, str_arg};
use crate::library::{self, remote, sync};
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::PathBuf;
use toolport_plus_api::Host;

fn tool_error(error: OpError) -> ToolError {
    ToolError::from(error)
}

fn with_paths<R>(host: &dyn Host, run: impl FnOnce() -> R) -> R {
    let _guard = crate::fsx::enter(crate::fsx::Paths {
        home: None,
        data_dir: host.data_dir(),
    });
    run()
}

pub(super) fn status(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_paths(host, || {
        remote::status(flag(args, "fetch")).map_err(tool_error)
    })
}

pub(super) fn pull(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_paths(host, || {
        let repo = remote::locate().map_err(tool_error)?;
        library::pull(&repo, flag_or(args, "dry_run", true), flag(args, "review"))
            .map_err(tool_error)
    })
}

pub(super) fn git_push(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_paths(host, || {
        let start = str_arg(args, "repo_path").map(PathBuf::from);
        let repo = crate::skills::api::resolve_repo(start.as_deref()).map_err(tool_error)?;
        if !remote::is_git(&repo) {
            return Err(ToolError::new(
                "not_found",
                format!("{} is not a git repository", repo.display()),
            ));
        }
        let message = str_arg(args, "commit_message").unwrap_or_default();
        if message.trim().is_empty() {
            return Err(ToolError::new(
                "invalid_arguments",
                "commit_message is empty",
            ));
        }
        let done = sync::push(&repo, Some(message), false).map_err(tool_error)?;
        if done["pushed"] == true {
            Ok(json!({
                "pushed": true,
                "commitSha": done["commitSha"],
                "repo": repo.to_string_lossy(),
                "checks": done["checks"],
            }))
        } else {
            Ok(json!({"pushed": false, "message": done["message"]}))
        }
    })
}
