use super::backend::{read_registry, skills_repo};
use super::ToolError;
use crate::plus::profiles;
use crate::plus::args::{flag, flag_or, list, str_arg, str_nonempty};
use crate::plus::auth::login;
use crate::plus::auth::stdio::{self, LaunchFault};
use crate::plus::client_sync::{self, SyncArgs};
use crate::plus::servers::{self, AddError, Patch, UninstallArgs};
use crate::plus::update::exec::{CmdOutput, GitRunner, ShellRunner, SystemGit, SystemShell};
use crate::plus::update::source::{self, Source};
use crate::plus::update::{execute, gitops, Mode, Options};
use crate::registry::{Registry, ServerEntry};
use crate::registry_controller::{self, ServerFields};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

type Outcome = Result<Value, ToolError>;

const GIT_TIMEOUT: Duration = Duration::from_secs(120);
const AUTH_WAIT: Duration = Duration::from_secs(20);
const CONFIG_KEYS: [&str; 7] = [
    "command",
    "args",
    "url",
    "transport",
    "cwd",
    "declareClientCapabilities",
    "forwardInstructions",
];

fn safe_token(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-')
}

fn name_arg(args: &Value) -> Result<&str, ToolError> {
    let name = str_arg(args, "name").unwrap_or_default().trim();
    if !safe_token(name) {
        return Err(ToolError::new("invalid_arguments", "invalid server name"));
    }
    Ok(name)
}

fn resolve(reg: &Registry, key: &str) -> Result<ServerEntry, ToolError> {
    servers::find(reg, key)
        .cloned()
        .ok_or_else(|| ToolError::new("not_found", format!("server not found: {key}")))
}

fn load(args: &Value) -> Result<ServerEntry, ToolError> {
    resolve(&read_registry()?, name_arg(args)?)
}

fn source_value(source: &Source) -> Value {
    json!({"kind": source.kind(), "meta": source.to_meta()})
}

pub(super) fn detect_source(args: &Value) -> Outcome {
    let server = load(args)?;
    let home = crate::clients::home();
    let (src, stored) = source::effective(&server, home.as_deref(), &SystemGit);
    let mut out = json!({"name": server.name, "stored": stored, "detected": source_value(&src)});
    if let Source::Git { path, .. } = &src {
        let repo = source::expand_path(path, home.as_deref());
        if gitops::is_repo(&SystemGit, &repo) {
            let remotes = gitops::list_remotes(&SystemGit, &repo);
            let mut branches = serde_json::Map::new();
            for remote in &remotes {
                branches.insert(remote.clone(), json!(gitops::remote_branches(&SystemGit, &repo, remote)));
            }
            out["remotes"] = json!(remotes);
            out["branches"] = Value::Object(branches);
        }
    }
    Ok(out)
}

/// Parses the editable fields `servers_set_source` accepts into a `SourceEdit`, rejecting
/// combinations that cannot mean anything (clearing a field while also setting it) and names
/// that would otherwise be misread as git flags.
fn source_edit_from(args: &Value) -> Result<source::SourceEdit, ToolError> {
    let edit = source::SourceEdit {
        path: str_nonempty(args, "path").map(String::from),
        remote: str_nonempty(args, "remote").map(String::from),
        branch: str_nonempty(args, "branch").map(String::from),
        upstream_remote: str_nonempty(args, "upstream_remote").map(String::from),
        upstream_branch: str_nonempty(args, "upstream_branch").map(String::from),
        clear_upstream: flag(args, "clear_upstream"),
        post_update: str_nonempty(args, "post_update").map(String::from),
        clear_post_update: flag(args, "clear_post_update"),
    };
    for (label, value) in [
        ("remote", &edit.remote),
        ("branch", &edit.branch),
        ("upstream_remote", &edit.upstream_remote),
        ("upstream_branch", &edit.upstream_branch),
    ] {
        if value.as_deref().is_some_and(|v| !safe_token(v)) {
            return Err(ToolError::new("invalid_arguments", format!("invalid {label}")));
        }
    }
    if edit.clear_upstream && (edit.upstream_remote.is_some() || edit.upstream_branch.is_some()) {
        return Err(ToolError::new(
            "invalid_arguments",
            "clear_upstream cannot be combined with upstream_remote or upstream_branch",
        ));
    }
    if edit.clear_post_update && edit.post_update.is_some() {
        return Err(ToolError::new(
            "invalid_arguments",
            "clear_post_update cannot be combined with post_update",
        ));
    }
    Ok(edit)
}

/// When the resulting checkout is a real, reachable git repository, checks that the remote(s)
/// and branch(es) the edit asks for actually exist there -- a stale path (or one phase 1's
/// docker placeholder will eventually cover) skips this and trusts the edit as given.
fn validate_against_repo(updated: &Source, home: Option<&Path>) -> Result<(), ToolError> {
    let Source::Git {
        path,
        remote,
        branch,
        upstream,
        ..
    } = updated
    else {
        return Ok(());
    };
    let repo = source::expand_path(path, home);
    if !gitops::is_repo(&SystemGit, &repo) {
        return Ok(());
    }
    let remotes = gitops::list_remotes(&SystemGit, &repo);
    let branch_exists = |remote: &str, branch: &str| {
        gitops::remote_branches(&SystemGit, &repo, remote)
            .iter()
            .any(|b| b == branch)
    };
    if !remotes.iter().any(|r| r == remote) {
        return Err(ToolError::new("invalid_arguments", format!("no such remote: {remote}")));
    }
    if !branch.is_empty() && !branch_exists(remote, branch) {
        return Err(ToolError::new(
            "invalid_arguments",
            format!("remote {remote} has no branch {branch}"),
        ));
    }
    if let Some(u) = upstream {
        if !remotes.iter().any(|r| r == &u.remote) {
            return Err(ToolError::new(
                "invalid_arguments",
                format!("no such remote: {}", u.remote),
            ));
        }
        if !branch_exists(&u.remote, &u.branch) {
            return Err(ToolError::new(
                "invalid_arguments",
                format!("remote {} has no branch {}", u.remote, u.branch),
            ));
        }
    }
    Ok(())
}

pub(super) fn set_source(args: &Value) -> Outcome {
    let server = load(args)?;
    let home = crate::clients::home();
    let (current, _) = source::effective(&server, home.as_deref(), &SystemGit);
    if !matches!(current, Source::Git { .. }) {
        return Err(ToolError::new(
            "invalid_input",
            format!("server {} is not git-backed", server.name),
        ));
    }
    let edit = source_edit_from(args)?;
    if edit.is_empty() {
        return Err(ToolError::new("invalid_arguments", "no changes requested"));
    }
    let updated = source::apply_edit(&current, &edit).map_err(|e| ToolError::new("invalid_arguments", e))?;
    validate_against_repo(&updated, home.as_deref())?;
    let rechecked = source::recheck_drift(&updated, &server, home.as_deref(), &SystemGit);
    registry_controller::set_server_source(&server.id, rechecked.to_meta()).map_err(ToolError::backend)?;
    Ok(json!({"name": server.name, "source": source_value(&rechecked)}))
}

pub(super) fn git_status(args: &Value) -> Outcome {
    let server = load(args)?;
    let (src, _) = source::effective(&server, crate::clients::home().as_deref(), &SystemGit);
    let Source::Git {
        path,
        remote,
        branch,
        upstream,
        ..
    } = src
    else {
        return Ok(json!({
            "name": server.name,
            "isGit": false,
            "message": format!("server is tracked as {}, not git", src.kind()),
        }));
    };
    let expanded = source::expand_path(&path, crate::clients::home().as_deref());
    if !expanded.exists() {
        return Ok(json!({"name": server.name, "isGit": true, "pathExists": false, "path": path}));
    }
    let target = gitops::Target {
        remote: Some(remote.as_str()),
        branch: (!branch.is_empty()).then(|| branch.as_str()),
        upstream: upstream
            .as_ref()
            .map(|u| (u.remote.as_str(), u.branch.as_str())),
    };
    match gitops::check_target(&SystemGit, &expanded, &target) {
        Ok(s) => {
            let mut value = json!({
                "name": server.name,
                "isGit": true,
                "path": expanded.to_string_lossy(),
                "branch": s.branch,
                "remoteRef": s.remote_ref,
                "ahead": s.ahead,
                "behind": s.behind,
                "dirty": s.dirty,
                "summaries": s.summaries,
            });
            if let Some(u) = s.upstream {
                value["upstreamRef"] = json!(u.remote_ref);
                value["upstreamAhead"] = json!(u.ahead);
                value["upstreamBehind"] = json!(u.behind);
                value["upstreamSummaries"] = json!(u.summaries);
            }
            Ok(value)
        }
        Err(e) => Ok(json!({
            "name": server.name,
            "isGit": true,
            "path": expanded.to_string_lossy(),
            "error": e,
        })),
    }
}

fn update_options(args: &Value, mode: Mode) -> Result<Options, ToolError> {
    let mut opts = Options::new(mode);
    if let Some(name) = str_nonempty(args, "name") {
        let server = resolve(&read_registry()?, name)?;
        opts.server = Some(server.id);
    }
    Ok(opts)
}

pub(super) fn check_updates(args: &Value) -> Outcome {
    let opts = update_options(args, Mode::Check)?;
    execute(&opts).map(|r| r.to_value()).map_err(ToolError::backend)
}

pub(super) fn apply_update(args: &Value) -> Outcome {
    name_arg(args)?;
    let mut opts = update_options(args, Mode::Apply)?;
    opts.allow_commands = true;
    execute(&opts).map(|r| r.to_value()).map_err(ToolError::backend)
}

fn join_profile(server: &ServerEntry, tag: &str, add: bool) -> Result<Registry, ToolError> {
    profiles::set_member(tag, &server.id, add, add).map_err(|error| match error.kind {
        profiles::Kind::NotFound => {
            ToolError::new("not_found", format!("profile not found: {tag}"))
        }
        _ => ToolError::backend(error.message),
    })
}

pub(super) fn add_profile_tag(args: &Value) -> Outcome {
    profile_tag(args, true)
}

pub(super) fn remove_profile_tag(args: &Value) -> Outcome {
    profile_tag(args, false)
}

fn profile_tag(args: &Value, add: bool) -> Outcome {
    let server = load(args)?;
    let tag = str_arg(args, "profile_tag").unwrap_or_default().trim();
    if tag.is_empty() {
        return Err(ToolError::new("invalid_arguments", "profile_tag is empty"));
    }
    let reg = join_profile(&server, tag, add)?;
    Ok(json!({"name": server.name, "profileTags": profiles::tags_of(&reg, &server.id)}))
}

fn config_strings(
    cfg: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, ToolError> {
    match cfg.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ToolError::new(
            "invalid_arguments",
            format!("{key} must be a string"),
        )),
    }
}

fn config_switch(
    cfg: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<bool>, ToolError> {
    match cfg.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(on)) => Ok(Some(*on)),
        Some(_) => Err(ToolError::new(
            "invalid_arguments",
            format!("{key} must be true or false"),
        )),
    }
}

fn check_config(cfg: &serde_json::Map<String, Value>) -> Result<(), ToolError> {
    if cfg.contains_key("env") {
        return Err(ToolError::new(
            "invalid_arguments",
            "env values never pass through this tool; set them with toolportctl secret set",
        ));
    }
    if let Some(key) = cfg
        .keys()
        .find(|k| !CONFIG_KEYS.contains(&k.as_str()) && k.as_str() != "name")
    {
        return Err(ToolError::new(
            "invalid_arguments",
            format!("unsupported config field: {key}"),
        ));
    }
    Ok(())
}

fn fields_from(
    name: &str,
    cfg: &serde_json::Map<String, Value>,
    base: Option<&ServerEntry>,
) -> Result<ServerFields, ToolError> {
    check_config(cfg)?;
    let args = match cfg.get("args") {
        None => None,
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .map(|v| v.as_str().map(String::from))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| ToolError::new("invalid_arguments", "args must be strings"))?,
        ),
        Some(_) => return Err(ToolError::new("invalid_arguments", "args must be a list")),
    };
    let command = config_strings(cfg, "command")?;
    let url = config_strings(cfg, "url")?;
    // A patch never moves a server between transports on its own, unlike `toolportctl server edit`.
    let transport = match (config_strings(cfg, "transport")?, base) {
        (Some(t), _) => t,
        (None, Some(b)) => b.transport.clone(),
        (None, None) if command.is_some() => "stdio".into(),
        (None, None) => "http".into(),
    };
    let patch = Patch {
        name: Some(name.to_string()),
        transport: Some(transport),
        command,
        args,
        url,
        cwd: config_strings(cfg, "cwd")?,
        declare_client_capabilities: config_switch(cfg, "declareClientCapabilities")?,
        forward_instructions: config_switch(cfg, "forwardInstructions")?,
    };
    Ok(servers::fields_from(patch, base))
}

pub(super) fn install(args: &Value) -> Outcome {
    let name = name_arg(args)?;
    let cfg = args
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(|| ToolError::new("invalid_arguments", "config must be an object"))?;
    if let Some(inner) = config_strings(cfg, "name")? {
        if inner != name {
            return Err(ToolError::new(
                "invalid_arguments",
                "config.name must match name",
            ));
        }
    }
    let reg = read_registry()?;
    let existing = servers::named(&reg, name).cloned();
    let fields = fields_from(name, cfg, existing.as_ref())?;
    let exists = || {
        ToolError::new(
            "conflict",
            format!("server {name} already exists; pass force=true to replace"),
        )
    };
    let id = match existing {
        Some(_) if !flag(args, "force") => return Err(exists()),
        Some(server) => {
            registry_controller::update_server_fields(&server.id, fields).map_err(ToolError::backend)?;
            server.id
        }
        None => servers::add_returning_id(fields).map_err(|e| match e {
            AddError::Exists => exists(),
            AddError::Failed(message) => ToolError::backend(message),
        })?,
    };
    let server = resolve(&read_registry()?, &id)?;
    if let Some(tags) = list(args, "profile_tags") {
        for tag in tags.iter().filter_map(Value::as_str) {
            join_profile(&server, tag, true)?;
        }
    }
    Ok(json!({"installed": true, "id": id, "name": server.name}))
}

pub(super) fn update_config(args: &Value) -> Outcome {
    let server = load(args)?;
    let patch = args
        .get("patch")
        .and_then(Value::as_object)
        .ok_or_else(|| ToolError::new("invalid_arguments", "patch must be an object"))?;
    if patch.contains_key("name") {
        return Err(ToolError::new(
            "invalid_arguments",
            "a patch cannot rename a server",
        ));
    }
    let fields = fields_from(&server.name, patch, Some(&server))?;
    registry_controller::update_server_fields(&server.id, fields).map_err(ToolError::backend)?;
    let mut keys: Vec<&String> = patch.keys().collect();
    keys.sort();
    Ok(json!({"id": server.id, "updatedKeys": keys}))
}

pub(super) fn set_mode(args: &Value) -> Outcome {
    let server = load(args)?;
    let mode = str_arg(args, "mode").unwrap_or_default();
    if !["auto", "direct", "router", "legacy", "bridge"].contains(&mode) {
        return Err(ToolError::new(
            "invalid_arguments",
            "mode must be auto, direct, router, legacy or bridge",
        ));
    }
    Ok(json!({
        "name": server.name,
        "mode": mode,
        "changed": false,
        "dropped": true,
        "note": "Toolport+ has no per-server proxy modes; the shared gateway daemon exposes every server (D-008).",
    }))
}

pub(super) fn uninstall(args: &Value) -> Outcome {
    let name = name_arg(args)?;
    let propagate = flag_or(args, "propagate_to_clients", true);
    let done = servers::uninstall(&UninstallArgs {
        key: name,
        dry_run: false,
        keep_clients: !propagate,
        keep_secrets: false,
    })?;
    Ok(done.to_value())
}

pub(super) fn clients_sync(args: &Value) -> Outcome {
    let mut clients = Vec::new();
    if let Some(client) = str_nonempty(args, "client") {
        if !safe_token(client) {
            return Err(ToolError::new("invalid_arguments", "invalid client key"));
        }
        clients.push(client.to_string());
    }
    let mut out = client_sync::sync(&SyncArgs {
        clients,
        dry_run: flag_or(args, "dry_run", true),
        keep_orphans: flag(args, "keep_orphans"),
    })?
    .to_value();
    let ignored: Vec<&str> = ["safe", "force_legacy"]
        .into_iter()
        .filter(|k| flag(args, k))
        .collect();
    if !ignored.is_empty() {
        out["ignoredOptions"] = json!(ignored);
    }
    Ok(out)
}

pub(super) fn sync_push(args: &Value) -> Outcome {
    crate::plus::sync::handlers::push_handler(json!({"dryRun": flag_or(args, "dry_run", true)}))
        .map_err(ToolError::backend)
}

fn git(repo: &Path, cmd: &[&str]) -> Result<CmdOutput, ToolError> {
    SystemGit.git(repo, cmd, GIT_TIMEOUT).map_err(ToolError::backend)
}

fn git_ok(repo: &Path, cmd: &[&str]) -> Result<String, ToolError> {
    let out = git(repo, cmd)?;
    if out.ok() {
        Ok(out.stdout)
    } else {
        Err(ToolError::backend(format!("git {} failed: {}", cmd.join(" "), out.first_error_line()),
        ))
    }
}

pub(super) fn skills_git_push(args: &Value) -> Outcome {
    let repo = skills_repo(args)?;
    if !repo.join(".git").exists() {
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
    let done = super::library::push(&repo, message)?;
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
}

fn conflict_report(repo: &Path, branch: &str, resume: &str) -> Value {
    let paths: Vec<String> = git(repo, &["diff", "--name-only", "--diff-filter=U"])
        .map(|o| o.stdout.lines().map(String::from).collect())
        .unwrap_or_default();
    json!({
        "synced": false,
        "conflict": true,
        "branch": branch,
        "conflictedPaths": paths,
        "next": format!("resolve the conflicts, git add the files, then run {resume}"),
    })
}

fn fork_rebase(repo: &Path, target: &str, upstream: &str) -> Outcome {
    git_ok(repo, &["checkout", "-b", target])?;
    Ok(if git(repo, &["rebase", upstream])?.ok() {
        json!({"synced": true, "branch": target, "mode": "rebase"})
    } else {
        conflict_report(repo, target, "git rebase --continue")
    })
}

fn fork_onto_author(repo: &Path, target: &str, upstream: &str, email: &str) -> Outcome {
    let base = git_ok(repo, &["merge-base", "HEAD", upstream])?
        .trim()
        .to_string();
    let range = format!("{base}..HEAD");
    let author = format!("--author={email}");
    let commits: Vec<String> = git_ok(
        repo,
        &[
            "log",
            "--reverse",
            "--no-merges",
            "--format=%H",
            &author,
            &range,
        ],
    )?
    .lines()
    .map(String::from)
    .collect();
    git_ok(repo, &["checkout", "-b", target, upstream])?;
    for sha in &commits {
        if !git(repo, &["cherry-pick", sha])?.ok() {
            return Ok(conflict_report(repo, target, "git cherry-pick --continue"));
        }
    }
    Ok(json!({"synced": true, "branch": target, "mode": "onto-author", "picked": commits.len()}))
}

pub(super) fn fork_sync(args: &Value) -> Outcome {
    let server = load(args)?;
    let (src, _) = source::effective(&server, crate::clients::home().as_deref(), &SystemGit);
    let Source::Git {
        path, post_update, ..
    } = src
    else {
        return Err(ToolError::new(
            "invalid_input",
            format!("server {} is not git-backed", server.name),
        ));
    };
    let repo = source::expand_path(&path, crate::clients::home().as_deref());
    if !gitops::is_repo(&SystemGit, &repo) {
        return Err(ToolError::new(
            "not_found",
            "source path is not a git repository",
        ));
    }
    let (remote, branch, mode) = fork_options(args)?;
    if !git_ok(&repo, &["status", "--porcelain"])?.trim().is_empty() {
        return Err(ToolError::new(
            "conflict",
            "working tree has uncommitted changes",
        ));
    }
    let current = git_ok(&repo, &["branch", "--show-current"])?
        .trim()
        .to_string();
    let upstream = format!("{remote}/{branch}");
    git_ok(&repo, &["fetch", "--quiet", remote])?;
    let date = crate::plus::update::now_iso()[..10].replace('-', "");
    let target = str_arg(args, "target_branch")
        .filter(|t| safe_token(t))
        .map(String::from)
        .unwrap_or_else(|| format!("{current}-synced-{date}"));
    let mut result = if mode == "rebase" {
        fork_rebase(&repo, &target, &upstream)?
    } else {
        let email = str_arg(args, "author_email").unwrap_or_default();
        if email.is_empty() {
            return Err(ToolError::new(
                "invalid_arguments",
                "author_email is required for onto-author",
            ));
        }
        fork_onto_author(&repo, &target, &upstream, email)?
    };
    result["previousBranch"] = json!(current);
    if result["synced"] == true && flag(args, "run_post_update") {
        if let Some(cmd) = post_update {
            result["postUpdate"] = run_post_update(&repo, &cmd)?;
        }
    }
    Ok(result)
}

fn fork_options(args: &Value) -> Result<(&str, &str, &str), ToolError> {
    let remote = str_arg(args, "upstream_remote").unwrap_or("upstream");
    let branch = str_arg(args, "upstream_branch").unwrap_or("main");
    let mode = str_arg(args, "mode").unwrap_or("rebase");
    if !safe_token(remote) || !safe_token(branch) {
        return Err(ToolError::new(
            "invalid_arguments",
            "invalid remote or branch",
        ));
    }
    if !["rebase", "onto-author"].contains(&mode) {
        return Err(ToolError::new(
            "invalid_arguments",
            "mode must be rebase or onto-author",
        ));
    }
    Ok((remote, branch, mode))
}

fn run_post_update(repo: &Path, cmd: &str) -> Outcome {
    let out = SystemShell
        .run(cmd, repo, Duration::from_secs(600))
        .map_err(ToolError::backend)?;
    Ok(json!({
        "ok": out.ok(),
        "stderrTail": out.stderr.lines().rev().take(10).collect::<Vec<_>>(),
    }))
}

fn launch_error(error: stdio::LaunchError) -> ToolError {
    match error.fault {
        LaunchFault::Unsupported => ToolError::new("invalid_input", error.message),
        LaunchFault::Refused => ToolError::new("refused", error.message),
        LaunchFault::Spawn => ToolError::backend(error.message),
    }
}

pub(super) fn auth(args: &Value) -> Outcome {
    let reg = read_registry()?;
    let server = resolve(&reg, name_arg(args)?)?;
    if let Some(login::Plan::Unsupported { reason, .. }) = login::review_gate(&reg, &server) {
        return Err(ToolError::new("refused", reason));
    }
    let mut session = stdio::start(&server).map_err(launch_error)?;
    let capture = session.wait_url(AUTH_WAIT);
    let signing_in = capture.url.is_some();
    if signing_in {
        session.detach();
    } else {
        session.kill();
    }
    Ok(json!({
        "name": server.name,
        "started": true,
        "authUrl": capture.url,
        "exited": capture.exit.is_some(),
        "timedOut": capture.timed_out,
        "stderrTail": capture.tail,
        "hint": signing_in.then_some("Open authUrl in a browser; the server's local callback saves the token on completion."),
    }))
}

#[cfg(test)]
#[path = "../../../tests/fixtures/update_source.rs"]
mod update_source;

#[cfg(test)]
mod tests {
    use super::super::call_tool;
    use super::super::tests::Fixture;
    use super::update_source;
    use crate::registry_controller;
    use serde_json::{json, Value};

    fn call(name: &str, args: Value) -> Result<Value, super::ToolError> {
        call_tool(name, &args)
    }

    fn failure(name: &str, args: Value) -> (&'static str, String) {
        let error = call(name, args).unwrap_err();
        (error.kind, error.message)
    }

    #[test]
    fn a_patch_keeps_the_transport_it_does_not_name() {
        let _fixture = Fixture::new("servers-patch");
        call(
            "servers_update_config",
            json!({"name": "alpha", "patch": {"url": "https://example.invalid/m"}, "confirm": true}),
        )
        .unwrap();
        let alpha = call("servers_get", json!({"name": "alpha"})).unwrap();
        assert_eq!(alpha["transport"], "stdio");
        assert_eq!(alpha["command"], "alpha-mcp");
        assert_eq!(alpha["url"], Value::Null);

        call(
            "servers_update_config",
            json!({"name": "beta", "patch": {"command": "beta-mcp"}, "confirm": true}),
        )
        .unwrap();
        let beta = call("servers_get", json!({"name": "beta"})).unwrap();
        assert_eq!(beta["transport"], "http");
        assert_eq!(beta["url"], "https://example.invalid/mcp");

        call(
            "servers_update_config",
            json!({"name": "alpha", "patch": {"args": []}, "confirm": true}),
        )
        .unwrap();
        let cleared = call("servers_get", json!({"name": "alpha"})).unwrap();
        assert_eq!(cleared["args"], json!([]));
    }

    #[test]
    fn install_defaults_a_new_server_to_http_without_a_command() {
        let _fixture = Fixture::new("servers-install");
        let (kind, message) = failure(
            "servers_install",
            json!({"name": "gamma", "config": {}, "confirm": true}),
        );
        assert_eq!(
            (kind, message.as_str()),
            ("backend_error", "enter an http:// or https:// server URL")
        );
        let added = call(
            "servers_install",
            json!({"name": "gamma", "config": {"url": "https://example.invalid/g"}, "confirm": true}),
        )
        .unwrap();
        assert_eq!(added["installed"], true);
        let got = call("servers_get", json!({"name": "gamma"})).unwrap();
        assert_eq!(got["transport"], "http");
        assert_eq!(got["id"], added["id"]);
    }

    #[test]
    fn install_names_the_existing_server_and_force_keeps_its_id() {
        let _fixture = Fixture::new("servers-force");
        let (kind, message) = failure(
            "servers_install",
            json!({"name": "ALPHA", "config": {"command": "other"}, "confirm": true}),
        );
        assert_eq!(
            (kind, message.as_str()),
            (
                "conflict",
                "server ALPHA already exists; pass force=true to replace"
            )
        );
        let replaced = call(
            "servers_install",
            json!({"name": "ALPHA", "config": {"command": "other"}, "force": true, "confirm": true}),
        )
        .unwrap();
        assert_eq!(replaced["id"], "srv-alpha");
        assert_eq!(replaced["name"], "ALPHA");
        let got = call("servers_get", json!({"name": "srv-alpha"})).unwrap();
        assert_eq!(got["command"], "other");
    }

    #[test]
    fn a_missing_server_is_not_found_for_every_server_tool() {
        let _fixture = Fixture::new("servers-missing");
        for (tool, args) in [
            ("servers_detect_source", json!({"name": "ghost"})),
            ("servers_git_status", json!({"name": "ghost"})),
            ("servers_set_source", json!({"name": "ghost", "branch": "main", "confirm": true})),
            ("servers_auth", json!({"name": "ghost", "confirm": true})),
        ] {
            let (kind, message) = failure(tool, args);
            assert_eq!(
                (kind, message.as_str()),
                ("not_found", "server not found: ghost"),
                "{tool}"
            );
        }
        let (kind, _) = failure(
            "servers_update_config",
            json!({"name": "ghost", "patch": {"cwd": "/tmp"}, "confirm": true}),
        );
        assert_eq!(kind, "not_found");
    }

    fn seed_git_meta(id: &str, meta: Value) {
        registry_controller::set_server_source(id, meta.as_object().unwrap().clone()).unwrap();
    }

    #[test]
    fn set_source_requires_a_git_backed_server() {
        let _fixture = Fixture::new("source-set-non-git");
        let (kind, message) = failure(
            "servers_set_source",
            json!({"name": "beta", "branch": "main", "confirm": true}),
        );
        assert_eq!(kind, "invalid_input");
        assert!(message.contains("not git-backed"), "{message}");
    }

    #[test]
    fn set_source_rejects_an_empty_edit() {
        let fixture = Fixture::new("source-set-empty");
        seed_git_meta(
            "srv-alpha",
            json!({
                "type": "git",
                "path": fixture.dir.join("nowhere").to_string_lossy(),
                "remote": "origin",
                "branch": "main",
            }),
        );
        let (kind, message) = failure("servers_set_source", json!({"name": "alpha", "confirm": true}));
        assert_eq!((kind, message.as_str()), ("invalid_arguments", "no changes requested"));
    }

    #[test]
    fn set_source_rejects_clear_upstream_combined_with_upstream_fields() {
        let fixture = Fixture::new("source-set-conflict");
        seed_git_meta(
            "srv-alpha",
            json!({
                "type": "git",
                "path": fixture.dir.join("nowhere").to_string_lossy(),
                "remote": "origin",
                "branch": "main",
            }),
        );
        let (kind, _) = failure(
            "servers_set_source",
            json!({"name": "alpha", "clear_upstream": true, "upstream_branch": "main", "confirm": true}),
        );
        assert_eq!(kind, "invalid_arguments");
    }

    #[test]
    fn set_source_persists_cleared_upstream_and_post_update() {
        let fixture = Fixture::new("source-set-clear");
        let path = fixture.dir.join("nowhere").to_string_lossy().into_owned();
        seed_git_meta(
            "srv-alpha",
            json!({
                "type": "git", "path": path, "remote": "origin", "branch": "main",
                "upstream": {"remote": "upstream", "branch": "main"},
                "post_update": "npm install",
            }),
        );
        let out = call(
            "servers_set_source",
            json!({"name": "alpha", "clear_upstream": true, "clear_post_update": true, "confirm": true}),
        )
        .unwrap();
        assert_eq!(out["source"]["meta"]["upstream"], Value::Null);
        assert_eq!(out["source"]["meta"]["post_update"], Value::Null);
        let detected = call("servers_detect_source", json!({"name": "alpha"})).unwrap();
        let meta = detected["detected"]["meta"].as_object().unwrap();
        assert!(!meta.contains_key("upstream"), "{meta:?}");
        assert!(!meta.contains_key("post_update"), "{meta:?}");
    }

    #[test]
    fn detect_source_lists_remote_branches_for_a_real_repo() {
        let fixture = Fixture::new("source-detect-real-repo");
        let repo = update_source::build(&fixture.dir.join("three-remotes"));
        seed_git_meta(
            "srv-alpha",
            json!({"type": "git", "path": repo.work.to_string_lossy(), "remote": "fork", "branch": "main"}),
        );
        let detected = call("servers_detect_source", json!({"name": "alpha"})).unwrap();
        let remotes: Vec<&str> = detected["remotes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(remotes.contains(&"fork"), "{remotes:?}");
        assert!(remotes.contains(&"upstream"), "{remotes:?}");
        assert!(remotes.contains(&"local"), "{remotes:?}");
        let fork_branches: Vec<&str> = detected["branches"]["fork"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(fork_branches.contains(&"main"), "{fork_branches:?}");
        assert!(fork_branches.contains(&"feature-x"), "{fork_branches:?}");
    }

    #[test]
    fn set_source_switches_branch_and_rejects_one_missing_on_the_remote() {
        let fixture = Fixture::new("source-set-real-repo");
        let repo = update_source::build(&fixture.dir.join("three-remotes"));
        seed_git_meta(
            "srv-alpha",
            json!({"type": "git", "path": repo.work.to_string_lossy(), "remote": "fork", "branch": "main"}),
        );
        let out = call(
            "servers_set_source",
            json!({"name": "alpha", "branch": "feature-x", "confirm": true}),
        )
        .unwrap();
        assert_eq!(out["source"]["meta"]["branch"], "feature-x");

        let (kind, message) = failure(
            "servers_set_source",
            json!({"name": "alpha", "branch": "no-such-branch", "confirm": true}),
        );
        assert_eq!(kind, "invalid_arguments");
        assert!(message.contains("no-such-branch"), "{message}");

        let (kind, _) = failure(
            "servers_set_source",
            json!({"name": "alpha", "remote": "ghost", "confirm": true}),
        );
        assert_eq!(kind, "invalid_arguments");
    }

    #[test]
    fn set_source_can_add_and_validate_an_upstream() {
        let fixture = Fixture::new("source-set-upstream");
        let repo = update_source::build(&fixture.dir.join("three-remotes"));
        seed_git_meta(
            "srv-alpha",
            json!({"type": "git", "path": repo.work.to_string_lossy(), "remote": "fork", "branch": "main"}),
        );
        let out = call(
            "servers_set_source",
            json!({"name": "alpha", "upstream_remote": "upstream", "upstream_branch": "main", "confirm": true}),
        )
        .unwrap();
        assert_eq!(out["source"]["meta"]["upstream"]["remote"], "upstream");
        assert_eq!(out["source"]["meta"]["upstream"]["branch"], "main");

        let (kind, _) = failure(
            "servers_set_source",
            json!({"name": "alpha", "upstream_branch": "no-such-branch", "confirm": true}),
        );
        assert_eq!(kind, "invalid_arguments");
    }
}
