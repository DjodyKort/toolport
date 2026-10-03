use super::backend::{ctl, read_registry, skills_repo};
use super::ToolError;
use crate::plus::args::{flag, flag_or, list, str_arg, str_nonempty};
use crate::plus::update::exec::{CmdOutput, GitRunner, ShellRunner, SystemGit, SystemShell};
use crate::plus::update::source::{self, Source};
use crate::plus::update::{execute, gitops, Mode, Options};
use crate::registry::{Registry, ServerEntry};
use crate::registry_controller::{self, ServerFields};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

type Outcome = Result<Value, ToolError>;

const GIT_TIMEOUT: Duration = Duration::from_secs(120);
const AUTH_WAIT: Duration = Duration::from_secs(20);
const CONFIG_KEYS: [&str; 5] = ["command", "args", "url", "transport", "cwd"];

pub(super) fn run(name: &str, args: &Value) -> Option<Outcome> {
    Some(match name {
        "servers_detect_source" => detect_source(args),
        "servers_git_status" => git_status(args),
        "servers_check_updates" => check_updates(args),
        "servers_add_profile_tag" => profile_tag(args, true),
        "servers_remove_profile_tag" => profile_tag(args, false),
        "servers_install" => install(args),
        "servers_update_config" => update_config(args),
        "servers_apply_update" => apply_update(args),
        "servers_set_mode" => set_mode(args),
        "servers_fork_sync" => fork_sync(args),
        "servers_auth" => auth(args),
        "servers_uninstall" => uninstall(args),
        "clients_sync" => clients_sync(args),
        "skills_git_push" => skills_git_push(args),
        "sync_push" => sync_push(args),
        _ => return None,
    })
}

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
    reg.servers
        .iter()
        .find(|s| s.id == key)
        .or_else(|| {
            reg.servers
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(key))
        })
        .cloned()
        .ok_or_else(|| ToolError::new("not_found", format!("server not found: {key}")))
}

fn load(args: &Value) -> Result<ServerEntry, ToolError> {
    resolve(&read_registry()?, name_arg(args)?)
}

fn source_value(source: &Source) -> Value {
    json!({"kind": source.kind(), "meta": source.to_meta()})
}

fn detect_source(args: &Value) -> Outcome {
    let server = load(args)?;
    let (src, stored) = source::effective(&server, crate::clients::home().as_deref());
    Ok(json!({"name": server.name, "stored": stored, "detected": source_value(&src)}))
}

fn git_status(args: &Value) -> Outcome {
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

fn check_updates(args: &Value) -> Outcome {
    let opts = update_options(args, Mode::Check)?;
    execute(&opts).map(|r| r.to_value()).map_err(ToolError::backend)
}

fn apply_update(args: &Value) -> Outcome {
    name_arg(args)?;
    let mut opts = update_options(args, Mode::Apply)?;
    opts.allow_commands = true;
    execute(&opts).map(|r| r.to_value()).map_err(ToolError::backend)
}

fn tags_of(reg: &Registry, server_id: &str) -> Vec<String> {
    reg.profiles
        .iter()
        .filter(|p| p.enabled_server_ids.iter().any(|s| s == server_id))
        .map(|p| p.name.clone())
        .collect()
}

fn find_profile(reg: &Registry, tag: &str) -> Option<String> {
    reg.profiles
        .iter()
        .find(|p| p.id == tag || p.name.eq_ignore_ascii_case(tag))
        .map(|p| p.id.clone())
}

fn join_profile(server: &ServerEntry, tag: &str, add: bool) -> Result<Registry, ToolError> {
    let mut reg = read_registry()?;
    let profile = match find_profile(&reg, tag) {
        Some(id) => id,
        None if add => {
            reg = registry_controller::create_profile(tag).map_err(ToolError::backend)?;
            find_profile(&reg, tag)
                .ok_or_else(|| ToolError::backend("profile was not created"))?
        }
        None => {
            return Err(ToolError::new(
                "not_found",
                format!("profile not found: {tag}"),
            ))
        }
    };
    registry_controller::set_server_enabled(&profile, &server.id, add, false).map_err(ToolError::backend)
}

fn profile_tag(args: &Value, add: bool) -> Outcome {
    let server = load(args)?;
    let tag = str_arg(args, "profile_tag").unwrap_or_default().trim();
    if tag.is_empty() {
        return Err(ToolError::new("invalid_arguments", "profile_tag is empty"));
    }
    let reg = join_profile(&server, tag, add)?;
    Ok(json!({"name": server.name, "profileTags": tags_of(&reg, &server.id)}))
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
        None => base.map(|b| b.args.clone()).unwrap_or_default(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(String::from))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| ToolError::new("invalid_arguments", "args must be strings"))?,
        Some(_) => return Err(ToolError::new("invalid_arguments", "args must be a list")),
    };
    let pick = |key: &str, from_base: Option<String>| -> Result<Option<String>, ToolError> {
        Ok(config_strings(cfg, key)?.or(from_base))
    };
    let command = pick("command", base.and_then(|b| b.command.clone()))?;
    let url = pick("url", base.and_then(|b| b.url.clone()))?;
    let transport = match config_strings(cfg, "transport")? {
        Some(t) => t,
        None => match base {
            Some(b) => b.transport.clone(),
            None if command.is_some() => "stdio".into(),
            None => "http".into(),
        },
    };
    Ok(ServerFields {
        name: name.to_string(),
        transport,
        command,
        args,
        url,
        cwd: pick("cwd", base.and_then(|b| b.cwd.clone()))?,
    })
}

fn install(args: &Value) -> Outcome {
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
    let existing = reg
        .servers
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
        .cloned();
    let fields = fields_from(name, cfg, existing.as_ref())?;
    let id = match existing {
        Some(_) if !flag(args, "force") => {
            return Err(ToolError::new(
                "conflict",
                format!("server {name} already exists; pass force=true to replace"),
            ))
        }
        Some(server) => {
            registry_controller::update_server_fields(&server.id, fields).map_err(ToolError::backend)?;
            server.id
        }
        None => {
            let before: Vec<String> = reg.servers.iter().map(|s| s.id.clone()).collect();
            let after = registry_controller::add_server(fields).map_err(ToolError::backend)?;
            after
                .servers
                .iter()
                .find(|s| !before.contains(&s.id))
                .map(|s| s.id.clone())
                .ok_or_else(|| ToolError::backend("server was not added"))?
        }
    };
    let server = resolve(&read_registry()?, &id)?;
    if let Some(tags) = list(args, "profile_tags") {
        for tag in tags.iter().filter_map(Value::as_str) {
            join_profile(&server, tag, true)?;
        }
    }
    Ok(json!({"installed": true, "id": id, "name": server.name}))
}

fn update_config(args: &Value) -> Outcome {
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

fn set_mode(args: &Value) -> Outcome {
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

fn uninstall(args: &Value) -> Outcome {
    let name = name_arg(args)?;
    let propagate = flag_or(args, "propagate_to_clients", true);
    let mut cmd = vec!["server", "uninstall", name];
    if !propagate {
        cmd.push("--keep-clients");
    }
    ctl(&cmd)
}

fn clients_sync(args: &Value) -> Outcome {
    let mut cmd = vec!["client", "sync"];
    if let Some(client) = str_nonempty(args, "client") {
        if client.starts_with('-') {
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

fn sync_push(args: &Value) -> Outcome {
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

fn skills_git_push(args: &Value) -> Outcome {
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

fn fork_sync(args: &Value) -> Outcome {
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
            let out = SystemShell
                .run(&cmd, &repo, Duration::from_secs(600))
                .map_err(ToolError::backend)?;
            result["postUpdate"] = json!({
                "ok": out.ok(),
                "stderrTail": out.stderr.lines().rev().take(10).collect::<Vec<_>>(),
            });
        }
    }
    Ok(result)
}

fn find_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|t| t.starts_with("https://") || t.starts_with("http://"))
        .map(|t| t.trim_end_matches([',', '.', ')', '"', '\'']).to_string())
}

fn auth(args: &Value) -> Outcome {
    let server = load(args)?;
    let command = match (&server.command, server.transport.as_str()) {
        (Some(c), "stdio") => c.clone(),
        _ => {
            return Err(ToolError::new(
                "invalid_input",
                "the auth flow only applies to stdio servers",
            ))
        }
    };
    let mut cmd = Command::new(&command);
    cmd.args(&server.args).arg("auth");
    if let Some(cwd) = &server.cwd {
        cmd.current_dir(cwd);
    }
    let configured: std::collections::HashSet<&str> =
        server.env.iter().map(|var| var.key.as_str()).collect();
    crate::downstream::strip_gateway_control_env(&mut cmd, &configured);
    let mut env: Vec<(String, String)> = Vec::new();
    for var in &server.env {
        let value = if var.secret {
            crate::secrets::get_secret(&server.id, &var.key)
        } else {
            var.value.clone()
        };
        if let Some(value) = value {
            cmd.env(&var.key, &value);
            env.push((var.key.clone(), value));
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| ToolError::backend(format!("could not start: {e}")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::backend("no stderr"))?;
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + AUTH_WAIT;
    let mut tail: Vec<String> = Vec::new();
    let mut url = None;
    let mut exited = false;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                url = find_url(&line);
                tail.push(crate::launch_inputs::redact_env_secrets(
                    &server, &env, line,
                ));
                if tail.len() > 10 {
                    tail.remove(0);
                }
                if url.is_some() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                exited = true;
                break;
            }
        }
    }
    if url.is_some() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    } else {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(json!({
        "name": server.name,
        "started": true,
        "authUrl": url,
        "exited": exited,
        "timedOut": url.is_none() && !exited,
        "stderrTail": tail,
        "hint": url.as_ref().map(|_| "Open authUrl in a browser; the server's local callback saves the token on completion."),
    }))
}
