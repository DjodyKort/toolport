use super::backend::{ctl, read_registry, skills_repo};
use super::ToolError;
use crate::plus::profiles;
use crate::plus::args::{flag, flag_or, list, str_arg, str_nonempty};
use crate::plus::auth::login;
use crate::plus::auth::stdio::{self, LaunchFault};
use crate::plus::servers::{self, AddError, Patch};
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
    let (src, stored) = source::effective(&server, crate::clients::home().as_deref());
    Ok(json!({"name": server.name, "stored": stored, "detected": source_value(&src)}))
}

pub(super) fn git_status(args: &Value) -> Outcome {
    let server = load(args)?;
    let (src, _) = source::effective(&server, crate::clients::home().as_deref());
    let Source::Git { path, branch, .. } = src else {
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
    match gitops::check(&SystemGit, &expanded, branch.as_deref()) {
        Ok(s) => Ok(json!({
            "name": server.name,
            "isGit": true,
            "path": expanded.to_string_lossy(),
            "branch": s.branch,
            "remoteRef": s.remote_ref,
            "ahead": s.ahead,
            "behind": s.behind,
            "dirty": s.dirty,
            "summaries": s.summaries,
        })),
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
    let mut cmd = vec!["server", "uninstall", name];
    if !propagate {
        cmd.push("--keep-clients");
    }
    ctl(&cmd)
}

pub(super) fn clients_sync(args: &Value) -> Outcome {
    let mut cmd = vec!["client", "sync"];
    if let Some(client) = str_nonempty(args, "client") {
        if !safe_token(client) {
            return Err(ToolError::new("invalid_arguments", "invalid client key"));
        }
        cmd.extend(["--client", client]);
    }
    if flag(args, "dry_run") {
        cmd.push("--dry-run");
    }
    if flag(args, "keep_orphans") {
        cmd.push("--keep-orphans");
    }
    let mut out = ctl(&cmd)?;
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
    crate::plus::sync::handlers::push_handler(json!({"dryRun": flag(args, "dry_run")}))
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
    git_ok(&repo, &["add", "-A"])?;
    if git_ok(&repo, &["status", "--porcelain"])?.trim().is_empty() {
        return Ok(json!({"pushed": false, "message": "working tree clean; nothing to commit"}));
    }
    git_ok(&repo, &["commit", "-m", message])?;
    let sha = git_ok(&repo, &["rev-parse", "HEAD"])?.trim().to_string();
    git_ok(&repo, &["push"])?;
    Ok(json!({"pushed": true, "commitSha": sha, "repo": repo.to_string_lossy()}))
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
    let (src, _) = source::effective(&server, crate::clients::home().as_deref());
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
mod tests {
    use super::super::call_tool;
    use super::super::tests::Fixture;
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
}
