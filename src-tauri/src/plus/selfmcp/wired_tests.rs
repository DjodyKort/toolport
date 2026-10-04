use super::tests::Fixture;
use super::*;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

fn call(name: &str, args: Value) -> Result<Value, ToolError> {
    call_tool(name, &args)
}

fn kind(result: Result<Value, ToolError>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(e) => e.kind,
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git_identity(dir: &Path) {
    git(dir, &["config", "user.email", "tester@example.invalid"]);
    git(dir, &["config", "user.name", "Tester"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

#[test]
fn no_tool_or_resource_is_left_unimplemented() {
    let _fixture = Fixture::new("wired-all");
    for tool in TOOLS {
        let mut args = serde_json::Map::new();
        for param in tool.params.iter().filter(|p| p.required) {
            let value = match param.ty {
                catalog::Ty::Str => json!("x"),
                catalog::Ty::Bool => json!(true),
                catalog::Ty::Obj => json!({}),
                catalog::Ty::StrList => json!(["x"]),
            };
            args.insert(param.name.to_string(), value);
        }
        if tool.gate != Gate::None {
            args.insert("confirm".into(), json!(true));
        }
        if tool.params.iter().any(|p| p.name == "dry_run") {
            args.insert("dry_run".into(), json!(true));
        }
        assert_ne!(
            kind(call_tool(tool.name, &Value::Object(args))),
            "not_implemented",
            "{}",
            tool.name
        );
    }
    for def in RESOURCES {
        let text = read_resource(def.uri).unwrap_or_else(|e| panic!("{}: {e:?}", def.uri));
        assert!(!text.0.is_empty() || def.uri.starts_with("mcpm://inventory"));
    }
}

#[test]
fn resources_describe_agents_styles_and_the_dropped_router() {
    let _fixture = Fixture::new("wired-res");
    let agents = read_resource("mcpm://inventory/agents").unwrap().0;
    assert_eq!(agents, "helper - A synthetic helper agent");
    let styles = read_resource("mcpm://inventory/styles").unwrap().0;
    assert_eq!(styles, "plain - A synthetic plain style");
    let arch = read_resource("mcpm://architecture").unwrap();
    assert_eq!(arch.1, "text/markdown");
    assert!(arch.0.contains("gateway"));
    assert!(read_resource("mcpm://workflows")
        .unwrap()
        .0
        .contains("servers_install"));
    let router: Value =
        serde_json::from_str(&read_resource("mcpm://router/status").unwrap().0).unwrap();
    assert_eq!(router["router"]["present"], false);
    assert_eq!(router["router"]["decision"], "D-008");
}

#[test]
fn skills_scaffold_sync_and_status_round_trip() {
    let fixture = Fixture::new("wired-skills");
    let made = call("skills_scaffold", json!({"name": "fresh"})).unwrap();
    assert!(made["created_path"]
        .as_str()
        .unwrap()
        .ends_with("skills/fresh/SKILL.md"));
    assert_eq!(
        kind(call("skills_scaffold", json!({"name": "fresh"}))),
        "conflict"
    );
    assert_eq!(
        kind(call("skills_scaffold", json!({"name": "../escape"}))),
        "invalid_arguments"
    );
    let rule = call(
        "skills_scaffold",
        json!({"name": "house-rule", "skill_type": "rule"}),
    )
    .unwrap();
    assert!(rule["created_path"]
        .as_str()
        .unwrap()
        .contains("/rules/house-rule/"));

    let dry = call(
        "skills_sync",
        json!({"dry_run": true, "client_keys": ["claude-code"]}),
    )
    .unwrap();
    assert_eq!(dry["dryRun"], true);
    assert!(!fixture.dir.join("mcpm-skills.lock").exists());

    let synced = call("skills_sync", json!({"client_keys": ["claude-code"]})).unwrap();
    assert_eq!(synced["skillCount"], 2);
    assert_eq!(synced["ruleCount"], 1);
    assert!(fixture.dir.join("mcpm-skills.lock").exists());
    assert!(fixture.home.join(".claude/skills/demo/SKILL.md").exists());

    let status = call("skills_status", json!({})).unwrap();
    assert_eq!(status["lockfilePresent"], true);
    let demo = status["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "demo")
        .unwrap();
    assert_eq!(demo["drifted"], false);
    assert_eq!(demo["clientsSynced"], json!(["claude-code"]));
    assert_eq!(status["lockedCount"], 3);
    assert_eq!(status["drift"], false);
    assert_eq!(
        std::fs::canonicalize(status["outputRoot"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(&fixture.home).unwrap()
    );
    let present = |status: &Value| {
        status["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "demo" && o["client"] == "claude-code")
            .map(|o| o["present"].clone())
    };
    assert_eq!(present(&status), Some(json!(true)));
    let only_cursor = call("skills_status", json!({"client_keys": ["cursor"]})).unwrap();
    assert_eq!(present(&only_cursor), None);
    std::fs::remove_file(fixture.home.join(".claude/skills/demo/SKILL.md")).unwrap();
    let missing = call("skills_status", json!({})).unwrap();
    assert_eq!(missing["drift"], true);
    assert_eq!(present(&missing), Some(json!(false)));
    call("skills_sync", json!({"client_keys": ["claude-code"]})).unwrap();

    call(
        "skills_edit_body",
        json!({"name": "demo", "new_body": "Changed", "confirm": true}),
    )
    .unwrap();
    let status = call("skills_status", json!({})).unwrap();
    let demo = status["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "demo")
        .unwrap();
    assert_eq!(demo["drifted"], true);
}

#[test]
fn tier_three_skill_edits_need_confirm_and_validate() {
    let fixture = Fixture::new("wired-edit");
    let file = fixture.repo.join("skills/demo/SKILL.md");
    let before = std::fs::read_to_string(&file).unwrap();

    let refused = call(
        "skills_edit_body",
        json!({"name": "demo", "new_body": "New body"}),
    );
    assert_eq!(kind(refused), "refused");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);

    let done = call(
        "skills_edit_body",
        json!({"name": "demo", "new_body": "New body\n\n", "confirm": true}),
    )
    .unwrap();
    assert!(done["newHash"].as_str().unwrap().starts_with("sha256:"));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "---\nname: demo\ndescription: A synthetic demo skill\n---\nNew body\n"
    );

    assert_eq!(
        kind(call(
            "skills_edit_frontmatter",
            json!({"name": "demo", "patch": {"description": "Patched"}})
        )),
        "refused"
    );
    call(
        "skills_edit_frontmatter",
        json!({"name": "demo", "patch": {"description": "Patched"}, "confirm": true}),
    )
    .unwrap();
    let got = call("skills_get", json!({"name": "demo"})).unwrap();
    assert_eq!(got["description"], "Patched");
    assert!(got["body"].as_str().unwrap().contains("New body"));

    let after = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        kind(call(
            "skills_edit_frontmatter",
            json!({"name": "demo", "patch": {"description": ""}, "confirm": true})
        )),
        "invalid_input"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), after);

    assert_eq!(
        kind(call("skills_delete", json!({"name": "demo"}))),
        "refused"
    );
    assert!(file.exists());
    assert_eq!(
        kind(call(
            "skills_delete",
            json!({"name": "missing", "confirm": true})
        )),
        "not_found"
    );
    call("skills_delete", json!({"name": "demo", "confirm": true})).unwrap();
    assert!(!fixture.repo.join("skills/demo").exists());
}

#[test]
fn agents_flow_scaffold_sync_edit() {
    let fixture = Fixture::new("wired-agents");
    let listed = call("agents_list", json!({})).unwrap();
    assert_eq!(listed["agents"][0]["name"], "helper");
    let got = call("agents_get", json!({"name": "helper"})).unwrap();
    assert!(got["body"].as_str().unwrap().contains("Agent prompt"));
    assert_eq!(
        kind(call("agents_get", json!({"name": "nope"}))),
        "not_found"
    );
    assert!(call("agents_lint", json!({})).unwrap()["messages"].is_array());
    assert!(
        !call("agents_list_transpilers", json!({})).unwrap()["transpilers"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let made = call(
        "agents_scaffold",
        json!({"name": "scout", "model": "sonnet"}),
    )
    .unwrap();
    let text = std::fs::read_to_string(made["created_path"].as_str().unwrap()).unwrap();
    assert!(text.contains("model: sonnet"));
    assert_eq!(
        kind(call("agents_scaffold", json!({"name": "scout"}))),
        "conflict"
    );

    let synced = call("agents_sync", json!({"client_keys": ["claude-code"]})).unwrap();
    assert_eq!(synced["agentCount"], 2);
    assert!(fixture.home.join(".claude/agents/helper.md").exists());
    assert!(fixture.dir.join("mcpm-skills.lock").exists());

    assert_eq!(
        kind(call(
            "agents_edit_body",
            json!({"name": "helper", "new_body": "Z"})
        )),
        "refused"
    );
    call(
        "agents_edit_body",
        json!({"name": "helper", "new_body": "Z", "confirm": true}),
    )
    .unwrap();
    let got = call("agents_get", json!({"name": "helper"})).unwrap();
    assert_eq!(got["body"], "Z");
    assert_eq!(got["description"], "A synthetic helper agent");
}

#[test]
fn styles_flow_with_apply_and_remove_tiers() {
    let fixture = Fixture::new("wired-styles");
    assert_eq!(
        call("styles_list", json!({})).unwrap()["styles"][0]["name"],
        "plain"
    );
    assert!(
        call("styles_get", json!({"name": "plain"})).unwrap()["body"]
            .as_str()
            .unwrap()
            .contains("Style text")
    );
    let transpilers = call("styles_list_transpilers", json!({})).unwrap();
    assert!(!transpilers["tier1"].as_array().unwrap().is_empty());
    assert!(!transpilers["tier2"].as_array().unwrap().is_empty());
    call("styles_scaffold", json!({"name": "terse"})).unwrap();
    assert_eq!(
        kind(call("styles_scaffold", json!({"name": "terse"}))),
        "conflict"
    );

    let synced = call("styles_sync_tier1", json!({})).unwrap();
    assert_eq!(synced["styleCount"], 2);

    assert_eq!(
        kind(call("styles_apply", json!({"name": "plain"}))),
        "refused"
    );
    assert_eq!(
        call("styles_active", json!({})).unwrap()["activeStyles"],
        json!({})
    );
    let applied = call("styles_apply", json!({"name": "plain", "confirm": true})).unwrap();
    assert_eq!(applied["applied"], "plain");
    let active = call("styles_active", json!({})).unwrap();
    assert!(!active["activeStyles"].as_object().unwrap().is_empty());
    assert!(active["activeStyles"]
        .as_object()
        .unwrap()
        .values()
        .all(|v| v == "plain"));
    assert_eq!(
        kind(call(
            "styles_apply",
            json!({"name": "ghost", "confirm": true})
        )),
        "not_found"
    );

    assert_eq!(kind(call("styles_remove", json!({}))), "refused");
    call("styles_remove", json!({"confirm": true})).unwrap();
    assert_eq!(
        call("styles_active", json!({})).unwrap()["activeStyles"],
        json!({})
    );

    call(
        "styles_edit_body",
        json!({"name": "plain", "new_body": "Short", "confirm": true}),
    )
    .unwrap();
    let file = fixture.repo.join("styles/plain/STYLE.md");
    assert!(std::fs::read_to_string(file)
        .unwrap()
        .ends_with("---\nShort\n"));
}

#[test]
fn skills_git_push_commits_and_pushes_only_when_confirmed() {
    let fixture = Fixture::new("wired-push");
    let remote = fixture.dir.join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare"]);
    let repo = &fixture.repo;
    git(repo, &["init", "-q"]);
    git_identity(repo);
    git(repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "seed"]);
    let branch = git(repo, &["branch", "--show-current"]);
    git(repo, &["push", "-q", "-u", "origin", &branch]);

    std::fs::write(repo.join("skills/demo/extra.md"), "more\n").unwrap();
    let args = json!({"commit_message": "add extra"});
    assert_eq!(kind(call("skills_git_push", args.clone())), "refused");
    assert_eq!(git(&remote, &["rev-list", "--count", &branch]), "1");

    let pushed = call(
        "skills_git_push",
        json!({"commit_message": "add extra", "confirm": true}),
    )
    .unwrap();
    assert_eq!(pushed["pushed"], true);
    assert_eq!(git(&remote, &["rev-list", "--count", &branch]), "2");
    assert_eq!(
        git(&remote, &["log", "-1", "--format=%s", &branch]),
        "add extra"
    );

    let clean = call(
        "skills_git_push",
        json!({"commit_message": "again", "confirm": true}),
    )
    .unwrap();
    assert_eq!(clean["pushed"], false);
}

#[test]
fn server_mutations_follow_their_tiers() {
    let _fixture = Fixture::new("wired-servers");
    let config = json!({"command": "gamma-mcp", "args": ["--flag"]});
    assert_eq!(
        kind(call(
            "servers_install",
            json!({"name": "gamma", "config": config})
        )),
        "refused"
    );
    assert_eq!(
        call("servers_list", json!({})).unwrap()["servers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let installed = call(
        "servers_install",
        json!({"name": "gamma", "config": config, "profile_tags": ["extra"], "confirm": true}),
    )
    .unwrap();
    assert_eq!(installed["installed"], true);
    let got = call("servers_get", json!({"name": "gamma"})).unwrap();
    assert_eq!(got["command"], "gamma-mcp");
    assert_eq!(got["args"], json!(["--flag"]));
    assert_eq!(got["transport"], "stdio");

    assert_eq!(
        kind(call(
            "servers_install",
            json!({"name": "gamma", "config": config, "confirm": true})
        )),
        "conflict"
    );
    assert_eq!(
        kind(call(
            "servers_install",
            json!({"name": "delta", "config": {"command": "d", "env": {"K": "v"}}, "confirm": true})
        )),
        "invalid_arguments"
    );
    call(
        "servers_install",
        json!({"name": "gamma", "config": {"command": "gamma2-mcp"}, "force": true, "confirm": true}),
    )
    .unwrap();
    assert_eq!(
        call("servers_get", json!({"name": "gamma"})).unwrap()["command"],
        "gamma2-mcp"
    );

    let profiles = call("servers_list_profiles", json!({})).unwrap();
    let extra = profiles["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "extra")
        .unwrap();
    assert_eq!(extra["enabledServerIds"].as_array().unwrap().len(), 1);
    let removed = call(
        "servers_remove_profile_tag",
        json!({"name": "gamma", "profile_tag": "extra"}),
    )
    .unwrap();
    assert_eq!(removed["profileTags"], json!([]));
    let added = call(
        "servers_add_profile_tag",
        json!({"name": "gamma", "profile_tag": "Default"}),
    )
    .unwrap();
    assert_eq!(added["profileTags"], json!(["Default"]));
    assert_eq!(
        kind(call(
            "servers_remove_profile_tag",
            json!({"name": "gamma", "profile_tag": "nope"})
        )),
        "not_found"
    );

    assert_eq!(
        kind(call(
            "servers_update_config",
            json!({"name": "gamma", "patch": {"cwd": "/tmp"}})
        )),
        "refused"
    );
    call(
        "servers_update_config",
        json!({"name": "gamma", "patch": {"cwd": "/tmp", "args": ["-v"]}, "confirm": true}),
    )
    .unwrap();
    let got = call("servers_get", json!({"name": "gamma"})).unwrap();
    assert_eq!(got["cwd"], "/tmp");
    assert_eq!(got["args"], json!(["-v"]));
    assert_eq!(got["command"], "gamma2-mcp");
    assert_eq!(
        kind(call(
            "servers_update_config",
            json!({"name": "gamma", "patch": {"name": "x"}, "confirm": true})
        )),
        "invalid_arguments"
    );

    assert_eq!(got["declareClientCapabilities"], false);
    assert_eq!(got["forwardInstructions"], false);
    let switched = call(
        "servers_update_config",
        json!({"name": "gamma", "patch": {"declareClientCapabilities": true, "forwardInstructions": true}, "confirm": true}),
    )
    .unwrap();
    let updated = switched["updatedKeys"].to_string();
    assert!(updated.contains("declareClientCapabilities"), "{updated}");
    assert!(updated.contains("forwardInstructions"), "{updated}");
    let got = call("servers_get", json!({"name": "gamma"})).unwrap();
    assert_eq!(got["declareClientCapabilities"], true);
    assert_eq!(got["forwardInstructions"], true);
    assert_eq!(got["command"], "gamma2-mcp", "other fields are kept");
    for bad in [json!("on"), json!(1), json!([true])] {
        assert_eq!(
            kind(call(
                "servers_update_config",
                json!({"name": "gamma", "patch": {"forwardInstructions": bad}, "confirm": true})
            )),
            "invalid_arguments"
        );
    }
    call(
        "servers_update_config",
        json!({"name": "gamma", "patch": {"declareClientCapabilities": false}, "confirm": true}),
    )
    .unwrap();
    let got = call("servers_get", json!({"name": "gamma"})).unwrap();
    assert_eq!(got["declareClientCapabilities"], false);
    assert_eq!(
        got["forwardInstructions"], true,
        "an edit keeps what it does not name"
    );

    call(
        "servers_install",
        json!({"name": "epsilon", "config": {"command": "epsilon-mcp", "forwardInstructions": true}, "confirm": true}),
    )
    .unwrap();
    let got = call("servers_get", json!({"name": "epsilon"})).unwrap();
    assert_eq!(got["forwardInstructions"], true);
    assert_eq!(got["declareClientCapabilities"], false);
    assert_eq!(
        kind(call(
            "servers_install",
            json!({"name": "zeta", "config": {"command": "z", "declareClientCapabilities": "yes"}, "confirm": true})
        )),
        "invalid_arguments"
    );
    call(
        "servers_uninstall",
        json!({"name": "epsilon", "confirm": true}),
    )
    .unwrap();

    let mode = call(
        "servers_set_mode",
        json!({"name": "gamma", "mode": "router", "confirm": true}),
    )
    .unwrap();
    assert_eq!(mode["changed"], false);
    assert_eq!(mode["dropped"], true);
    assert_eq!(
        kind(call(
            "servers_set_mode",
            json!({"name": "gamma", "mode": "weird", "confirm": true})
        )),
        "invalid_arguments"
    );

    let source = call("servers_detect_source", json!({"name": "gamma"})).unwrap();
    assert_eq!(source["detected"]["kind"], "unknown");
    assert_eq!(
        call("servers_git_status", json!({"name": "gamma"})).unwrap()["isGit"],
        false
    );

    assert_eq!(
        kind(call("servers_uninstall", json!({"name": "gamma"}))),
        "refused"
    );
    assert!(call("servers_get", json!({"name": "gamma"})).is_ok());
    let gone = call(
        "servers_uninstall",
        json!({"name": "gamma", "confirm": true}),
    )
    .unwrap();
    assert_eq!(gone["name"], "gamma");
    assert_eq!(
        kind(call("servers_get", json!({"name": "gamma"}))),
        "not_found"
    );
    assert_eq!(
        kind(call(
            "servers_uninstall",
            json!({"name": "gamma", "confirm": true})
        )),
        "not_found"
    );
}

#[test]
fn clients_sync_is_tier_two_and_reports_ignored_legacy_flags() {
    let _fixture = Fixture::new("wired-clients");
    assert_eq!(kind(call("clients_sync", json!({}))), "refused");
    let dry = call(
        "clients_sync",
        json!({"dry_run": true, "safe": true, "force_legacy": true}),
    )
    .unwrap();
    assert_eq!(dry["dryRun"], true);
    assert_eq!(dry["ignoredOptions"], json!(["safe", "force_legacy"]));
    assert_eq!(
        kind(call(
            "clients_sync",
            json!({"dry_run": true, "client": "no-such-client"})
        )),
        "not_found"
    );
    let applied = call("clients_sync", json!({"confirm": true})).unwrap();
    assert_eq!(applied["dryRun"], false);
}

#[test]
fn sync_push_dry_run_waives_the_gate_and_reaches_the_sync_engine() {
    let _fixture = Fixture::new("wired-sync");
    assert_eq!(kind(call("sync_push", json!({}))), "refused");
    let outcome = call("sync_push", json!({"dry_run": true}));
    assert!(matches!(kind(outcome), "backend_error" | "ok"));
}

#[test]
fn fork_sync_replays_local_commits_onto_a_second_remote() {
    let fixture = Fixture::new("wired-fork");
    let upstream = fixture.dir.join("upstream.git");
    std::fs::create_dir_all(&upstream).unwrap();
    git(&upstream, &["init", "-q", "--bare", "-b", "main"]);
    let work = fixture.dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git_identity(&work);
    std::fs::write(work.join("a.txt"), "a\n").unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "base"]);
    git(
        &work,
        &["remote", "add", "origin", upstream.to_str().unwrap()],
    );
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    git(
        &work,
        &["remote", "add", "upstream", upstream.to_str().unwrap()],
    );
    std::fs::write(work.join("local.txt"), "mine\n").unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "local change"]);

    let other = fixture.dir.join("other");
    git(
        &fixture.dir,
        &[
            "clone",
            "-q",
            upstream.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git_identity(&other);
    std::fs::write(other.join("b.txt"), "b\n").unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "upstream change"]);
    git(&other, &["push", "-q", "origin", "main"]);

    let registry_path = fixture.dir.join("registry.json");
    let mut reg: Value =
        serde_json::from_str(&std::fs::read_to_string(&registry_path).unwrap()).unwrap();
    reg["servers"].as_array_mut().unwrap().push(json!({
        "id": "srv-fork", "name": "forked", "transport": "stdio", "command": "forked-mcp",
        "args": [],
        "mcpmSource": {"type": "git", "path": work.to_string_lossy(), "branch": "main"}
    }));
    std::fs::write(&registry_path, reg.to_string()).unwrap();

    let status = call("servers_git_status", json!({"name": "forked"})).unwrap();
    assert_eq!(status["isGit"], true);
    assert_eq!(status["ahead"], 1);
    assert_eq!(status["dirty"], false);
    assert_eq!(
        call("servers_detect_source", json!({"name": "forked"})).unwrap()["detected"]["kind"],
        "git"
    );

    assert_eq!(
        kind(call("servers_fork_sync", json!({"name": "forked"}))),
        "refused"
    );
    let synced = call(
        "servers_fork_sync",
        json!({"name": "forked", "target_branch": "main-synced", "confirm": true}),
    )
    .unwrap();
    assert_eq!(synced["synced"], true);
    assert_eq!(synced["previousBranch"], "main");
    assert_eq!(git(&work, &["branch", "--show-current"]), "main-synced");
    assert!(work.join("b.txt").exists());
    assert!(work.join("local.txt").exists());

    git(&work, &["checkout", "-q", "main"]);
    let picked = call(
        "servers_fork_sync",
        json!({"name": "forked", "mode": "onto-author", "author_email": "tester@example.invalid",
               "target_branch": "main-picked", "confirm": true}),
    )
    .unwrap();
    assert_eq!(picked["synced"], true);
    assert_eq!(picked["picked"], 1);
    assert!(work.join("b.txt").exists());
}

#[cfg(unix)]
#[test]
fn servers_auth_captures_the_consent_url_from_stderr() {
    let fixture = Fixture::new("wired-auth");
    let registry_path = fixture.dir.join("registry.json");
    let mut reg: Value =
        serde_json::from_str(&std::fs::read_to_string(&registry_path).unwrap()).unwrap();
    reg["servers"].as_array_mut().unwrap().push(json!({
        "id": "srv-auth", "name": "authy", "transport": "stdio", "command": "sh",
        "args": ["-c", "echo 'Visit https://example.invalid/consent?x=1 to log in.' >&2; sleep 0.2"]
    }));
    std::fs::write(&registry_path, reg.to_string()).unwrap();
    assert_eq!(
        kind(call("servers_auth", json!({"name": "authy"}))),
        "refused"
    );
    let out = call("servers_auth", json!({"name": "authy", "confirm": true})).unwrap();
    assert_eq!(out["authUrl"], "https://example.invalid/consent?x=1");
    assert_eq!(out["timedOut"], false);
    assert_eq!(
        kind(call(
            "servers_auth",
            json!({"name": "beta", "confirm": true})
        )),
        "invalid_input"
    );
}

#[test]
fn self_server_registration_is_idempotent_and_in_the_registry_golden() {
    use crate::registry::Registry;
    let mut reg = Registry::default();
    assert_eq!(
        register::apply_ensure_self_server(&mut reg, "/opt/tp/toolport-selfmcp"),
        register::Ensured::Created
    );
    assert_eq!(
        register::apply_ensure_self_server(&mut reg, "/opt/tp/toolport-selfmcp"),
        register::Ensured::Unchanged
    );
    assert_eq!(reg.servers.len(), 1);
    assert_eq!(
        register::apply_ensure_self_server(&mut reg, "/opt/other/toolport-selfmcp"),
        register::Ensured::Updated
    );
    assert_eq!(reg.servers.len(), 1);
    let golden = serde_json::to_value(&reg.servers[0]).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/selfmcp/expected-self-server.json"
    ))
    .unwrap();
    let mut normalized = golden.clone();
    normalized["command"] = json!("<selfmcp>");
    assert_eq!(normalized, expected);
}

#[test]
fn ensure_self_server_persists_and_never_duplicates() {
    let fixture = Fixture::new("wired-ensure");
    let (id, first) = register::ensure_self_server().unwrap();
    assert_eq!(first, register::Ensured::Created);
    let (again, second) = register::ensure_self_server().unwrap();
    assert_eq!(second, register::Ensured::Unchanged);
    assert_eq!(id, again);
    let listed = call("servers_list", json!({})).unwrap();
    let rows = listed["servers"].as_array().unwrap();
    assert_eq!(rows.iter().filter(|s| s["name"] == SERVER_NAME).count(), 1);
    let row = rows.iter().find(|s| s["name"] == SERVER_NAME).unwrap();
    assert_eq!(row["source"], register::SELF_SOURCE);
    assert_eq!(row["transport"], "stdio");
    let got = call("servers_get", json!({"name": SERVER_NAME})).unwrap();
    assert!(got["command"].as_str().unwrap().contains(BINARY_NAME));
    drop(fixture);
}

#[test]
fn skills_sync_reports_and_migrates_files_that_shadow_a_skill() {
    let fixture = Fixture::new("wired-collisions");
    let shadow = fixture.home.join(".claude/commands/demo.md");
    std::fs::create_dir_all(shadow.parent().unwrap()).unwrap();
    std::fs::write(&shadow, "hand written").unwrap();
    let same = |path: &Value, expected: &Path| {
        std::fs::canonicalize(path.as_str().unwrap()).unwrap()
            == std::fs::canonicalize(expected).unwrap()
    };

    let kept = call("skills_sync", json!({"client_keys": ["claude-code"]})).unwrap();
    assert_eq!(kept["kept"], 1);
    assert_eq!(kept["replaced"], 0);
    assert_eq!(kept["collisions"][0]["skill"], "demo");
    assert_eq!(kept["collisions"][0]["action"], "kept");
    assert!(same(&kept["collisions"][0]["collisionPath"], &shadow));
    assert_eq!(kept["entries"][0]["name"], "demo");
    assert_eq!(kept["entries"][0]["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(kept["clientCount"], 1);
    assert!(shadow.exists());

    let dry = call(
        "skills_sync",
        json!({"client_keys": ["claude-code"], "migrate": true, "dry_run": true}),
    )
    .unwrap();
    assert_eq!(dry["collisions"][0]["action"], "skipped-dry-run");
    assert_eq!((dry["replaced"].clone(), dry["kept"].clone()), (json!(0), json!(0)));
    assert!(shadow.exists());

    let moved = call(
        "skills_sync",
        json!({"client_keys": ["claude-code"], "migrate": true}),
    )
    .unwrap();
    assert_eq!(moved["replaced"], 1);
    assert_eq!(moved["collisions"][0]["action"], "replaced");
    let backup = Path::new(moved["collisions"][0]["backupPath"].as_str().unwrap());
    assert_eq!(std::fs::read_to_string(backup).unwrap(), "hand written");
    assert!(!shadow.exists());

    let clean = call("skills_sync", json!({"client_keys": ["claude-code"]})).unwrap();
    assert_eq!(clean["collisions"], json!([]));
    assert_eq!(
        kind(call("skills_sync", json!({"migrate": "yes"}))),
        "invalid_arguments"
    );
}
