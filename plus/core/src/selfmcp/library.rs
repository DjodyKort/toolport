//! `library_status` (tier 1), `library_pull` (tier 2) and `skills_git_push` (tier 4, previews
//! unless dry_run is false): the same
//! operations as `toolportctl library status|pull|push`, over `crate::library`; and
//! `library_select_ls` and `library_select_plan` (tier 1), `library_select_check` (tier 2),
//! `library_select_set`, `library_select_apply` and `library_select_undo` (tier 3) over
//! `crate::selective`. Ticking a hook and forcing over a Library folder stay with the person.

use super::ToolError;
use crate::args::{flag, flag_or, nonempty_strings, str_arg};
use crate::library::{self, connect, remote, sync};
use crate::op::OpError;
use crate::selective::check::Request;
use crate::selective::handlers as selective;
use crate::selective::ops::{Choices, Options, SetArgs};
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
        let dry_run = flag_or(args, "dry_run", true);
        let message = str_arg(args, "commit_message").unwrap_or_default();
        if !dry_run && message.trim().is_empty() {
            return Err(ToolError::new(
                "invalid_arguments",
                "commit_message is empty",
            ));
        }
        if dry_run {
            let message = Some(message).filter(|m| !m.trim().is_empty());
            return sync::push(&repo, message, true).map_err(tool_error);
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

pub(super) fn select_plan(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let source = source_arg(args)?;
    with_paths(host, || {
        let opts = Options {
            fetch: flag_or(args, "fetch", true),
            switch: flag(args, "switch"),
            deepen: flag(args, "deepen"),
        };
        selective::plan(host, source, opts).map_err(tool_error)
    })
}

fn names(args: &Value, key: &str) -> Vec<String> {
    nonempty_strings(args, key)
        .unwrap_or_default()
        .into_iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

pub(super) fn select_set(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let source = source_arg(args)?;
    let set = SetArgs {
        on: names(args, "on"),
        off: names(args, "off"),
        tick_hook: Vec::new(),
    };
    if set.on.is_empty() && set.off.is_empty() {
        return Err(ToolError::new(
            "invalid_arguments",
            "nothing to set: pass on or off",
        ));
    }
    with_paths(host, || {
        selective::set(host, source, &set, flag_or(args, "dry_run", true)).map_err(tool_error)
    })
}

pub(super) fn select_apply(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let source = source_arg(args)?;
    let opts = Options {
        fetch: flag_or(args, "fetch", true),
        switch: flag(args, "switch"),
        ..Default::default()
    };
    with_paths(host, || {
        if flag_or(args, "dry_run", true) {
            let mut plan = selective::plan(host, source, opts).map_err(tool_error)?;
            plan["applyWith"] = json!(
                "call library_select_apply with dry_run=false, this plan_hash and confirm=true"
            );
            return Ok(plan);
        }
        let hash = str_arg(args, "plan_hash")
            .map(str::trim)
            .filter(|hash| !hash.is_empty())
            .ok_or_else(|| {
                ToolError::new(
                    "invalid_arguments",
                    "plan_hash is required: take it from library_select_apply with dry_run=true",
                )
            })?;
        let choices = Choices {
            force: Vec::new(),
            skip: names(args, "skip"),
        };
        let mut done = selective::apply(host, source, hash, opts, &choices).map_err(tool_error)?;
        done["followUp"] = json!([{
            "tool": "skills_sync",
            "why": "carries chosen rules of the Library to the clients; this tool does not run it",
        }]);
        Ok(done)
    })
}

pub(super) fn select_undo(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let source = source_arg(args)?;
    with_paths(host, || selective::undo(host, source).map_err(tool_error))
}

pub(super) fn select_check(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_paths(host, || {
        selective::check(
            host,
            Request {
                fetch: flag_or(args, "fetch", true),
                source: str_arg(args, "source").filter(|s| !s.trim().is_empty()),
                deepen: flag(args, "deepen"),
            },
        )
        .map_err(tool_error)
    })
}
