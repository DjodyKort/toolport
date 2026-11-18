//! `library_status` (tier 1), `library_pull` (tier 2) and `skills_git_push` (tier 4): the same
//! operations as `toolportctl library status|pull|push`, over `crate::library`; and
//! `library_select_ls` and `library_select_plan` (tier 1) over `crate::selective`.

use super::ToolError;
use crate::args::{flag, flag_or, str_arg};
use crate::library::{self, connect, remote, sync};
use crate::op::OpError;
use crate::selective::handlers as selective;
use crate::selective::ops::Options;
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

pub(super) fn connect_repository(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_paths(host, || {
        let url = str_arg(args, "url").unwrap_or_default();
        connect::connect(
            url,
            str_arg(args, "clone_to"),
            flag_or(args, "dry_run", true),
        )
        .map_err(tool_error)
    })
}

pub(super) fn remote_add(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_paths(host, || {
        let repo = remote::locate().map_err(tool_error)?;
        let url = str_arg(args, "url").unwrap_or_default();
        connect::remote_add(&repo, url, flag_or(args, "dry_run", true)).map_err(tool_error)
    })
}

pub(super) fn commit(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_paths(host, || {
        let repo = remote::locate().map_err(tool_error)?;
        let message = str_arg(args, "message").unwrap_or_default();
        connect::commit(&repo, message, flag_or(args, "dry_run", true)).map_err(tool_error)
    })
}

fn source_arg(args: &Value) -> Result<&str, ToolError> {
    str_arg(args, "source")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ToolError::new("invalid_arguments", "source is required"))
}

pub(super) fn select_ls(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_paths(host, || {
        selective::ls(host, str_arg(args, "source")).map_err(tool_error)
    })
}

/// Only plans: applying a plan and ticking a hook are not offered through this server.
pub(super) fn select_plan(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let source = source_arg(args)?;
    with_paths(host, || {
        let opts = Options {
            fetch: flag_or(args, "fetch", true),
            switch: flag(args, "switch"),
            ..Default::default()
        };
        selective::plan(host, source, opts).map_err(tool_error)
    })
}
