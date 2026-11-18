//! `sync_status` and `sync_plan`: the same reads as `toolportctl sync status|plan`. The passphrase
//! is read from the keychain without ever prompting and is neither an argument nor part of a
//! result, and applying or pushing is left to a person.

use serde_json::Value;
use toolport_plus_api::Host;

use super::setup::with_ctx;
use super::ToolError;
use crate::args::nonempty_strings;
use crate::skills::git::SystemGit;
use crate::sync::engine::PullOptions;

pub(super) fn status(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    with_ctx(host, crate::sync::status)
}

pub(super) fn plan(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let opts = PullOptions {
        take: nonempty_strings(args, "take")
            .unwrap_or_default()
            .into_iter()
            .collect(),
        no_prompt: true,
        ..Default::default()
    };
    with_ctx(host, |ctx| {
        if crate::sync::config::read_config(ctx)?.is_none() {
            return Ok(serde_json::json!({
                "configured": false,
                "rows": [],
                "note": "sync is not set up on this machine: a person runs `toolportctl sync setup`",
            }));
        }
        crate::sync::pull(ctx, &opts, &SystemGit)
    })
}

pub(super) fn repos(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    with_ctx(host, |_| crate::sync::repos::view(host))
}
