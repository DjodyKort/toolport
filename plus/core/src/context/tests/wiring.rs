use super::world::{script, world, World};
use crate::context::{presync, shell_rc, shims, wiring};
use serde_json::{json, Value};
use std::fs;
use std::process::Command;

fn shims_file(w: &World) -> std::path::PathBuf {
    w.data.join(shims::SHIMS_FILE)
}

fn plan_steps(data: &Value) -> Vec<String> {
    data["plan"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            format!(
                "{} {}",
                s["op"].as_str().unwrap(),
                s["path"].as_str().unwrap_or("")
            )
        })
        .collect()
}

#[test]
fn test01_a_plan_writes_nothing_and_names_the_two_files_and_the_backup() {
    let w = world("wiring-plan");
    w.write(".zshrc", "export A=1\n");
    let before = w.tree();
    let data = wiring::init(&w.host, true).unwrap();
    assert_eq!(w.tree(), before, "a dry run must not write");
    assert_eq!(data["dryRun"], true);
    assert_eq!(data["result"], Value::Null);
    assert_eq!(data["plan"]["undo"], "toolportctl context wiring disable");
    let steps = plan_steps(&data);
    assert!(
        steps[0].starts_with("create ") && steps[0].ends_with("context-shims.zsh"),
        "{steps:?}"
    );
    assert!(
        steps[1].starts_with("update ") && steps[1].ends_with(".zshrc"),
        "{steps:?}"
    );
    assert!(steps[2].starts_with("note "), "{steps:?}");
    let diff = &data["plan"]["steps"][1]["diff"];
    assert_eq!(diff["before"], "");
    assert!(diff["after"]
        .as_str()
        .unwrap()
        .ends_with("# toolport:context-shims"));
}

#[test]
fn test02_apply_adds_the_marked_line_and_keeps_a_copy_of_the_file() {
    let w = world("wiring-apply");
    w.write(".zshrc", "export A=1\nalias ll='ls -l'\n");
    let data = wiring::init(&w.host, false).unwrap();
    let rc = w.read(".zshrc");
    assert!(rc.starts_with("export A=1\nalias ll='ls -l'\n"));
    assert_eq!(rc.lines().count(), 3);
    assert!(rc.lines().last().unwrap().contains(shell_rc::MARKER));
    assert!(shims_file(&w).is_file());
    let backups = data["result"]["backups"].as_array().unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        fs::read_to_string(backups[0].as_str().unwrap()).unwrap(),
        "export A=1\nalias ll='ls -l'\n"
    );
    assert_eq!(data["result"]["undo"], "toolportctl context wiring disable");
}

#[test]
fn test03_a_second_init_changes_nothing() {
    let w = world("wiring-twice");
    w.write(".zshrc", "export A=1\n");
    wiring::init(&w.host, false).unwrap();
    let before = w.tree();
    let again = wiring::init(&w.host, false).unwrap();
    assert_eq!(again["plan"]["steps"], json!([]));
    assert_eq!(
        again["plan"]["summary"],
        "The launch wiring is already set up"
    );
    assert_eq!(again["result"], Value::Null);
    assert_eq!(w.tree(), before);
}

#[test]
fn test04_disable_gives_the_startup_file_back_byte_for_byte() {
    for original in ["export A=1\n", "export A=1", "", "a\r\nb\r\n"] {
        let w = world("wiring-disable");
        if !original.is_empty() {
            w.write(".zshrc", original);
        }
        wiring::init(&w.host, false).unwrap();
        let off = wiring::disable(&w.host, false).unwrap();
        assert_eq!(off["result"]["applied"], true);
        if original.is_empty() {
            assert!(
                !w.exists(".zshrc"),
                "a file that init created is removed again"
            );
        } else {
            assert_eq!(w.read(".zshrc"), original);
        }
        assert!(!shims_file(&w).exists());
        let ledger = fs::read_to_string(w.data.join("plus/context-wiring.json")).unwrap();
        assert!(!ledger.contains("addedNewline"), "{ledger}");
    }
}

#[test]
fn test05_disable_leaves_what_was_written_after_our_line() {
    let w = world("wiring-after");
    w.write(".zshrc", "export A=1\n");
    wiring::init(&w.host, false).unwrap();
    let mut rc = w.read(".zshrc");
    rc.push_str("export LATER=1\n");
    w.write(".zshrc", &rc);
    wiring::disable(&w.host, false).unwrap();
    assert_eq!(w.read(".zshrc"), "export A=1\nexport LATER=1\n");
}

#[test]
fn test06_a_linked_startup_file_is_changed_in_place_and_the_link_stays() {
    let w = world("wiring-link");
    let real = w.write("dotfiles/zshrc", "export A=1\n");
    std::os::unix::fs::symlink(&real, w.home.join(".zshrc")).unwrap();
    let plan = wiring::init(&w.host, true).unwrap();
    let warnings = plan["plan"]["warnings"].as_array().unwrap();
    assert!(
        warnings[0].as_str().unwrap().contains("is a link to"),
        "{warnings:?}"
    );
    wiring::init(&w.host, false).unwrap();
    assert!(fs::symlink_metadata(w.home.join(".zshrc"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::read_to_string(&real)
        .unwrap()
        .contains(shell_rc::MARKER));
    wiring::disable(&w.host, false).unwrap();
    assert_eq!(fs::read_to_string(&real).unwrap(), "export A=1\n");
}

#[test]
fn test07_a_startup_file_that_is_not_utf8_is_refused_and_untouched() {
    let w = world("wiring-utf8");
    fs::write(w.home.join(".zshrc"), [0x66, 0xff, 0xfe, 0x0a]).unwrap();
    let err = wiring::init(&w.host, false).unwrap_err();
    assert!(err.message.contains("not valid UTF-8"), "{}", err.message);
    assert_eq!(
        fs::read(w.home.join(".zshrc")).unwrap(),
        [0x66, 0xff, 0xfe, 0x0a]
    );
    assert!(!shims_file(&w).exists());
}

#[test]
fn test08_status_walks_from_not_set_up_through_a_source_order_finding_to_healthy() {
    let w = world("wiring-status");
    let fresh = wiring::status(&w.host).unwrap();
    assert_eq!(fresh["state"], "not_set_up");
    assert_eq!(fresh["setUp"], false);

    w.write(".zshrc", "source ~/tools/shell-wrapper.sh\n");
    wiring::init(&w.host, false).unwrap();
    assert_eq!(wiring::status(&w.host).unwrap()["state"], "healthy");

    let line = shell_rc::source_line(&w.home, &shims_file(&w));
    w.write(
        ".zshrc",
        &format!("{line}\nsource ~/tools/shell-wrapper.sh\n"),
    );
    let look = wiring::status(&w.host).unwrap();
    assert_eq!(look["state"], "look");
    let order = look["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "source-order")
        .unwrap();
    assert_eq!(order["level"], "look");
    assert!(order["title"]
        .as_str()
        .unwrap()
        .contains("BEFORE shell-wrapper.sh"));
    assert_eq!(look["shell"]["sourcedBefore"], json!(["shell-wrapper.sh"]));

    let fix = wiring::init(&w.host, true).unwrap();
    assert_eq!(
        fix["plan"]["summary"],
        "Fix the source order of the launch wiring"
    );
    wiring::init(&w.host, false).unwrap();
    assert_eq!(
        w.read(".zshrc"),
        format!("source ~/tools/shell-wrapper.sh\n{line}\n")
    );
    assert_eq!(wiring::status(&w.host).unwrap()["state"], "healthy");
}

#[test]
fn test09_status_says_failed_after_a_failed_presync_and_shows_what_it_said() {
    let w = world("wiring-failed");
    wiring::init(&w.host, false).unwrap();
    fs::create_dir_all(w.data.join("plus")).unwrap();
    fs::write(
        w.data.join("plus/context-presync.json"),
        r#"{"lastAttemptAt":1799990000,"durationMs":1800,"ok":false,"warnings":["context sync: could not reach the remote"],"output":"fatal: unable to access"}"#,
    )
    .unwrap();
    let data = wiring::status(&w.host).unwrap();
    assert_eq!(data["state"], "failed");
    assert_eq!(data["presync"]["ok"], false);
    assert_eq!(data["presync"]["output"], "fatal: unable to access");
    assert_eq!(data["presync"]["nextAfter"], Value::Null);
}

#[test]
fn test10_the_org_tool_checks_name_a_missing_tool_a_changed_wrapper_and_a_stale_org_file() {
    let w = world("wiring-org");
    wiring::init(&w.host, false).unwrap();
    let find = |data: &Value, id: &str| {
        data["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .cloned()
    };
    let none = wiring::status(&w.host).unwrap();
    assert_eq!(find(&none, "org-tool").unwrap()["level"], "ok");
    assert!(find(&none, "org-file").is_none());

    w.write(".local/share/corp-dev-tools/claude/CLAUDE.md", "org text\n");
    w.write(
        ".local/share/corp-dev-tools/claude/shell-wrapper.sh",
        "wrapper v1\n",
    );
    w.write(".claude/CLAUDE.md", "org text\n");
    let ok = wiring::status(&w.host).unwrap();
    assert_eq!(find(&ok, "org-tool").unwrap()["level"], "ok");
    assert_eq!(find(&ok, "org-file").unwrap()["level"], "ok");
    assert_eq!(ok["org"]["installed"], true);

    w.write(".claude/CLAUDE.md", "edited by hand\n");
    let edited = wiring::status(&w.host).unwrap();
    assert_eq!(find(&edited, "org-file").unwrap()["level"], "look");
    assert_eq!(edited["state"], "look");

    let hash = crate::context::hash::sha256_hex("wrapper v0\n");
    w.write(
        ".config/mcpm/context.json",
        &format!("{{\"cf_wrapper_hash\": \"{hash}\"}}\n"),
    );
    let changed = wiring::status(&w.host).unwrap();
    assert_eq!(find(&changed, "org-tool").unwrap()["level"], "look");

    w.write(".config/mcpm/context.json", "{}\n");
    w.write(".local/share/corp-dev-tools/claude/new-thing.sh", "x\n");
    let grew = wiring::status(&w.host).unwrap();
    assert!(find(&grew, "org-tool").unwrap()["detail"]
        .as_str()
        .unwrap()
        .contains("new-thing.sh"));
}

#[test]
fn test11_a_sync_rewrites_the_shell_file_only_once_the_wiring_exists() {
    let w = world("wiring-refresh");
    let skipped = wiring::refresh_shims(&w.host, false).unwrap();
    assert_eq!(skipped["action"], "skipped");
    assert!(!shims_file(&w).exists(), "a sync never sets the wiring up");

    wiring::init(&w.host, false).unwrap();
    assert_eq!(
        wiring::refresh_shims(&w.host, false).unwrap()["action"],
        "unchanged"
    );
    w.write(
        ".config/mcpm/context.json",
        r#"{"launchProfiles": {"bare": {"accessSet": "s1"}}}"#,
    );
    let planned = wiring::refresh_shims(&w.host, true).unwrap();
    assert_eq!(planned["action"], "would rewrite");
    assert!(!fs::read_to_string(shims_file(&w))
        .unwrap()
        .contains("claude-bare"));
    assert_eq!(
        wiring::refresh_shims(&w.host, false).unwrap()["action"],
        "rewritten"
    );
    let text = fs::read_to_string(shims_file(&w)).unwrap();
    assert!(
        text.contains(
            "claude-bare() { context_presync; command claude --strict-mcp-config --mcp-config"
        ),
        "{text}"
    );
    assert!(
        text.contains(".config/toolport/profiles/bare/mcp.json"),
        "{text}"
    );
}

#[test]
fn test12_the_startup_file_is_never_touched_by_status_plan_or_sync() {
    let w = world("wiring-readonly");
    w.write(".zshrc", "export A=1\n");
    wiring::init(&w.host, false).unwrap();
    let rc = w.read(".zshrc");
    let _ = wiring::status(&w.host).unwrap();
    let _ = wiring::init(&w.host, true).unwrap();
    let _ = wiring::disable(&w.host, true).unwrap();
    let _ = wiring::refresh_shims(&w.host, false).unwrap();
    assert_eq!(w.read(".zshrc"), rc);
}

fn zsh() -> Option<&'static str> {
    ["/bin/zsh", "/usr/bin/zsh"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
}

/// Runs the generated file in a bare zsh with fake `toolportctl` and `claude` programs on PATH.
fn in_zsh(
    w: &World,
    presync_exit: i32,
    with_toolportctl: bool,
    command: &str,
) -> (String, String, String) {
    let bin = w.home.join("bin");
    if with_toolportctl {
        script(
            &bin.join("toolportctl"),
            &format!("echo \"toolportctl $*\" >> \"$HOME/calls.log\"\nexit {presync_exit}"),
        );
    }
    script(
        &bin.join("claude"),
        "echo \"claude $*\" >> \"$HOME/calls.log\"",
    );
    let out = Command::new(zsh().unwrap())
        .args([
            "-f",
            "-c",
            &format!("source '{}'; {command}", shims_file(w).display()),
        ])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &w.home)
        .output()
        .unwrap();
    (
        fs::read_to_string(w.home.join("calls.log")).unwrap_or_default(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn test13_claude_runs_the_presync_first_and_passes_its_arguments_on() {
    if zsh().is_none() {
        eprintln!("zsh is not installed; skipped");
        return;
    }
    let w = world("wiring-zsh");
    wiring::init(&w.host, false).unwrap();
    let (calls, _, err) = in_zsh(&w, 0, true, "claude --resume 'a b'");
    assert_eq!(
        calls,
        "toolportctl context presync --quiet\nclaude --resume a b\n"
    );
    assert_eq!(err, "");
}

#[test]
fn test14_a_failed_presync_only_warns_and_claude_still_starts() {
    if zsh().is_none() {
        return;
    }
    let w = world("wiring-zsh-fail");
    wiring::init(&w.host, false).unwrap();
    let (calls, _, err) = in_zsh(&w, 1, true, "claude one");
    assert_eq!(calls, "toolportctl context presync --quiet\nclaude one\n");
    assert!(
        err.contains("the presync failed; Claude starts anyway"),
        "{err}"
    );
}

#[test]
fn test15_without_toolportctl_claude_starts_without_a_message() {
    if zsh().is_none() {
        return;
    }
    let w = world("wiring-zsh-none");
    wiring::init(&w.host, false).unwrap();
    let (calls, _, err) = in_zsh(&w, 0, false, "claude one");
    assert_eq!(calls, "claude one\n");
    assert_eq!(err, "");
}

#[test]
fn test16_a_launch_profile_function_passes_its_own_mcp_file_after_the_presync() {
    if zsh().is_none() {
        return;
    }
    let w = world("wiring-zsh-profile");
    w.write(
        ".config/mcpm/context.json",
        r#"{"launchProfiles": {"bare": {"accessSet": "s1"}}}"#,
    );
    wiring::init(&w.host, false).unwrap();
    let (calls, _, _) = in_zsh(&w, 0, true, "claude-bare --print hi");
    let file = w.home.join(".config/toolport/profiles/bare/mcp.json");
    assert_eq!(
        calls,
        format!(
            "toolportctl context presync --quiet\nclaude --strict-mcp-config --mcp-config {} --print hi\n",
            file.display()
        )
    );
}

#[test]
fn test17_hrclaude_gets_the_presync_only_when_the_compression_file_defined_it() {
    if zsh().is_none() {
        return;
    }
    let w = world("wiring-zsh-hr");
    wiring::init(&w.host, false).unwrap();
    let (_, out, _) = in_zsh(&w, 0, true, "type hrclaude");
    assert!(out.contains("not found"), "{out}");
    let defined = format!(
        "hrclaude() {{ echo old; }}; source '{}'; hrclaude x",
        shims_file(&w).display()
    );
    let (calls, _, _) = in_zsh(&w, 0, true, &defined);
    assert_eq!(
        calls,
        "toolportctl context presync --quiet\ntoolportctl compression run -- x\n"
    );
}

#[test]
fn test18_the_generated_file_is_valid_zsh_and_names_no_function_with_an_underscore_first() {
    let text = shims::snippet(&[shims::ProfileShim {
        name: "bare".into(),
        mcp_file: "/data/it's here/mcp.json".into(),
    }]);
    for name in crate::compression::shims::defined_functions(&text) {
        assert!(!name.starts_with('_'), "{name}");
    }
    if let Some(zsh) = zsh() {
        let w = world("wiring-zsh-n");
        let file = w.home.join("shims.zsh");
        fs::write(&file, &text).unwrap();
        let ok = Command::new(zsh).arg("-n").arg(&file).status().unwrap();
        assert!(ok.success(), "zsh -n rejected the file:\n{text}");
    }
}

#[test]
fn test19_the_org_tool_is_recorded_once_a_presync_ran() {
    let w = world("wiring-arm");
    w.write(
        ".local/share/corp-dev-tools/claude/shell-wrapper.sh",
        "wrapper v1\n",
    );
    w.write(".local/share/corp-dev-tools/.git/HEAD", "x\n");
    let tool = super::presync::Noop;
    presync::run(&w.host, &tool, presync::Opts::default()).unwrap();
    let ctx = w.read(".config/mcpm/context.json");
    let want = crate::context::hash::sha256_hex("wrapper v1\n");
    assert!(ctx.contains(&want), "{ctx}");
    w.write(
        ".local/share/corp-dev-tools/claude/shell-wrapper.sh",
        "wrapper v2\n",
    );
    presync::run(&w.host, &tool, presync::Opts { force: true }).unwrap();
    assert!(
        w.read(".config/mcpm/context.json").contains(&want),
        "a recorded hash is never replaced"
    );
}

#[test]
fn test20_the_status_tells_the_org_pull_command_and_where_to_change_it() {
    let w = world("wiring-pull-command");
    wiring::init(&w.host, false).unwrap();
    let find = |data: &Value, id: &str| {
        data["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .cloned()
    };
    let default = wiring::status(&w.host).unwrap();
    let pull = &default["org"]["pull"];
    assert_eq!(pull["source"], "default");
    assert_eq!(pull["command"], Value::Null);
    assert_eq!(pull["key"], "orgPull.command");
    assert!(pull["configFile"]
        .as_str()
        .unwrap()
        .ends_with("context.json"));
    assert!(find(&default, "org-pull").is_none());

    w.write(
        ".config/mcpm/context.json",
        "{\"orgPull\": {\"command\": [\"org-pull\", \"--quiet\", \"two words\"], \"intervalSecs\": 7200}}\n",
    );
    let set = wiring::status(&w.host).unwrap();
    assert_eq!(set["org"]["pull"]["source"], "configured");
    assert_eq!(
        set["org"]["pull"]["command"],
        json!(["org-pull", "--quiet", "two words"])
    );
    assert_eq!(
        set["org"]["pull"]["display"],
        "org-pull --quiet \"two words\""
    );
    assert_eq!(set["org"]["intervalSecs"], 7200);
    assert_eq!(set["intervalSecs"], 7200);

    w.write(
        ".config/mcpm/context.json",
        "{\"orgPull\": {\"command\": \"git pull\"}}\n",
    );
    let bad = wiring::status(&w.host).unwrap();
    assert_eq!(bad["org"]["pull"]["source"], "invalid");
    let check = find(&bad, "org-pull").unwrap();
    assert_eq!(check["level"], "look");
    assert!(check["detail"]
        .as_str()
        .unwrap()
        .contains("not a shell string"));
    assert_eq!(bad["state"], "look");
}
