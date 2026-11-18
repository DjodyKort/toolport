//! The host-facing form of the operations: the real git, the real `claude` binary and this
//! machine's folders. `toolportctl library select` and the self-MCP tools call these.

use super::check;
use super::ops::{self, Choices, Env, Options, SetArgs};
use crate::op::OpError;
use crate::plugins::{self, SystemClaude};
use crate::skills::git::SystemGit;
use serde_json::Value;
use toolport_plus_api::Host;

fn with_env<T>(
    host: &dyn Host,
    run: impl FnOnce(&Env) -> Result<T, OpError>,
) -> Result<T, OpError> {
    let plugins = plugins::Env::host()
        .ok_or_else(|| OpError::conflict("home directory could not be resolved"))?;
    run(&Env {
        plugins: &plugins,
        git: &SystemGit,
        claude: &SystemClaude::from_env(),
        store: host,
        managed: crate::sources::managed_settings_path(),
        library: crate::skills::repo::find_repo(None),
    })
}

pub fn ls(host: &dyn Host, source: Option<&str>) -> Result<Value, OpError> {
    with_env(host, |env| ops::ls(env, source))
}

pub fn set(host: &dyn Host, source: &str, args: &SetArgs, dry_run: bool) -> Result<Value, OpError> {
    with_env(host, |env| ops::set(env, source, args, dry_run))
}

pub fn plan(host: &dyn Host, source: &str, opts: Options) -> Result<Value, OpError> {
    with_env(host, |env| ops::plan(env, source, opts))
}

pub fn apply(
    host: &dyn Host,
    source: &str,
    plan_hash: &str,
    opts: Options,
    choices: &Choices,
) -> Result<Value, OpError> {
    with_env(host, |env| {
        ops::apply(env, source, plan_hash, opts, choices)
    })
}

pub fn undo(host: &dyn Host, source: &str) -> Result<Value, OpError> {
    with_env(host, |env| ops::undo(env, source))
}

pub fn check(host: &dyn Host, request: check::Request) -> Result<Value, OpError> {
    with_env(host, |env| check::run(env, request))
}
