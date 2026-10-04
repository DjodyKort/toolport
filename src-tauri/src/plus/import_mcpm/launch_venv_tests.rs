use super::launch::relocate_entry;
use super::run::{run, Action, RunOptions};
use super::run_tests::{read_json, with_world, World};
use crate::registry::ServerEntry;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn entry(command: &str, args: &[&str]) -> ServerEntry {
    ServerEntry {
        id: "x".into(),
        name: "x".into(),
        transport: "stdio".into(),
        command: Some(command.into()),
        args: args.iter().map(|a| a.to_string()).collect(),
        launch: None,
        env: vec![],
        url: None,
        cwd: None,
        source: None,
        disabled_tools: vec![],
        client_credentials: None,
        request_timeout_ms: None,
        max_request_timeout_ms: None,
        declare_client_capabilities: false,
        forward_instructions: false,
        initialize_timeout_ms: None,
        unknown_fields: Default::default(),
    }
}

/// `<home>/.claude/mcp-servers/<name>` laid out like a real uv/venv install: `pyvenv.cfg`,
/// `bin/python`, a `site-packages` with the server's dependency, the script and its manifest.
fn python_package(home: &Path, name: &str, venv: &str) -> PathBuf {
    let pkg = home.join(".claude/mcp-servers").join(name);
    let env = pkg.join(venv);
    write(
        &env.join("pyvenv.cfg"),
        "home = /usr/bin\ninclude-system-site-packages = false\nversion = 3.13.0\n",
    );
    write(&env.join("bin/python"), "#!/bin/sh\nexit 0\n");
    write(
        &env.join("lib/python3.13/site-packages/mcp/__init__.py"),
        "VALUE = 1\n",
    );
    write(&pkg.join("server.py"), "import mcp\n");
    write(&pkg.join("requirements.txt"), "mcp\n");
    pkg
}

#[test]
fn a_python_in_a_virtualenv_and_its_script_stay_where_they_are() {
    let tmp = std::env::temp_dir().join(format!("tp-venv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let home = tmp.join("home");
    let data = tmp.join("data");
    let pkg = python_package(&home, "sci", ".venv");
    let home_str = home.to_string_lossy().into_owned();
    let python = pkg.join(".venv/bin/python").to_string_lossy().into_owned();
    let script = pkg.join("server.py").to_string_lossy().into_owned();

    let mut e = entry(&python, &[&script]);
    let relocation = relocate_entry(&mut e, &home_str, &data);

    assert!(relocation.moves.is_empty(), "{:?}", relocation.moves);
    assert_eq!(relocation.kept.len(), 2);
    assert_eq!(e.command.as_deref(), Some(python.as_str()));
    assert_eq!(e.args, [script]);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn a_virtualenv_with_any_name_is_found_by_its_pyvenv_cfg() {
    let tmp = std::env::temp_dir().join(format!("tp-venv-named-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let home = tmp.join("home");
    let pkg = python_package(&home, "tool", "runtime-x");
    let python = pkg
        .join("runtime-x/bin/python")
        .to_string_lossy()
        .into_owned();
    let script = pkg.join("server.py").to_string_lossy().into_owned();

    let mut e = entry(&python, &[&script]);
    let relocation = relocate_entry(&mut e, &home.to_string_lossy(), &tmp.join("data"));

    assert!(relocation.moves.is_empty());
    assert!(relocation
        .kept
        .iter()
        .any(|(p, why)| *p == python && why.contains("virtualenv")));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn a_package_with_its_dependencies_beside_the_script_is_not_split() {
    let tmp = std::env::temp_dir().join(format!("tp-venv-node-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let home = tmp.join("home");
    let pkg = home.join(".claude/mcp-servers/nodepkg");
    write(&pkg.join("index.js"), "require('dep');\n");
    write(&pkg.join("package.json"), "{}\n");
    write(
        &pkg.join("node_modules/dep/index.js"),
        "module.exports = 1;\n",
    );
    let script = pkg.join("index.js").to_string_lossy().into_owned();

    let mut e = entry("node", &[&script]);
    let relocation = relocate_entry(&mut e, &home.to_string_lossy(), &tmp.join("data"));

    assert!(relocation.moves.is_empty());
    assert_eq!(relocation.kept.len(), 1);
    assert_eq!(e.args, [script]);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn a_lone_script_still_moves_and_so_does_a_binary_in_its_own_directory() {
    let tmp = std::env::temp_dir().join(format!("tp-venv-loose-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let home = tmp.join("home");
    let bin = home.join(".config/mcpm/bin/run.sh");
    let tool = home.join(".claude/mcp-servers/annas/annas-mcp");
    write(&bin, "#!/bin/sh\n");
    write(&tool, "binary\n");

    let mut e = entry(&tool.to_string_lossy(), &[&bin.to_string_lossy()]);
    let relocation = relocate_entry(&mut e, &home.to_string_lossy(), &tmp.join("data"));

    assert_eq!(relocation.moves.len(), 2);
    assert!(relocation.kept.is_empty());
    assert!(e.command.unwrap().contains("imported-scripts"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn a_path_that_does_not_exist_is_judged_by_its_name() {
    let home = "/nonexistent-home";
    let mut venv = entry(
        &format!("{home}/.claude/mcp-servers/p/.venv/bin/python"),
        &[],
    );
    let kept = relocate_entry(&mut venv, home, Path::new("/nonexistent-data"));
    assert!(kept.moves.is_empty());
    assert_eq!(kept.kept.len(), 1);

    let mut plain = entry("node", &[&format!("{home}/.claude/mcp-servers/p/run.js")]);
    let moved = relocate_entry(&mut plain, home, Path::new("/nonexistent-data"));
    assert_eq!(moved.moves.len(), 1);
}

fn pin(w: &World, servers: Value) -> RunOptions {
    write(&w.root.join("servers.json"), &servers.to_string());
    for file in std::fs::read_dir(&w.root).unwrap().flatten() {
        if file.file_name() != "servers.json" {
            std::fs::remove_file(file.path()).unwrap();
        }
    }
    RunOptions {
        root: w.root.clone(),
        short_ids_path: None,
        home: Some(w.home.to_string_lossy().into_owned()),
        dry_run: false,
        write_clients: false,
        prune_orphans: false,
    }
}

#[test]
fn import_leaves_a_virtualenv_server_runnable_and_says_so() {
    with_world("venv-run", |w| {
        let pkg = python_package(&w.home, "sci", ".venv");
        write(
            &w.home.join(".config/mcpm/bin/helper.sh"),
            "#!/bin/sh\nexit 0\n",
        );
        let home = w.home.to_string_lossy().into_owned();
        let python = pkg.join(".venv/bin/python").to_string_lossy().into_owned();
        let script = pkg.join("server.py").to_string_lossy().into_owned();
        let opts = pin(
            w,
            json!({
                "sci": {"name": "sci", "command": python, "args": [script]},
                "helper": {"name": "helper", "command": "bash",
                           "args": [format!("{home}/.config/mcpm/bin/helper.sh")]},
            }),
        );

        let plan = run(&opts).unwrap();

        let reg = read_json(&w.data().join("registry.json"));
        let sci = reg["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "sci")
            .unwrap();
        assert_eq!(sci["command"], python.as_str());
        assert_eq!(sci["args"][0], script.as_str());
        let kept = Path::new(sci["command"].as_str().unwrap());
        assert!(kept
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("pyvenv.cfg")
            .is_file());
        assert!(pkg
            .join(".venv/lib/python3.13/site-packages/mcp/__init__.py")
            .is_file());

        assert_eq!(plan.scripts.len(), 1, "{:?}", plan.scripts);
        assert!(w
            .data()
            .join("imported-scripts/config_mcpm_bin/helper.sh")
            .is_file());
        assert!(!w
            .data()
            .join("imported-scripts/claude_mcp-servers")
            .exists());

        let warnings = plan.warnings.as_array().unwrap();
        let kept_rows: Vec<&Value> = warnings
            .iter()
            .filter(|x| x["kind"] == "keep-in-place")
            .collect();
        assert_eq!(kept_rows.len(), 2, "{warnings:?}");
        assert!(kept_rows.iter().all(|r| r["server"] == "sci"));
        assert!(kept_rows[0]["detail"]
            .as_str()
            .unwrap()
            .contains("must not be deleted"));
        assert!(warnings
            .iter()
            .all(|x| !(x["kind"] == "relocate-path" && x["server"] == "sci")));
        assert!(plan.summary().contains("kept in place sci"));
        assert!(plan.servers.iter().all(|c| c.action == Action::Created));
    });
}

#[cfg(unix)]
#[test]
fn the_kept_python_still_imports_what_its_virtualenv_holds() {
    use std::process::Command;
    let tmp = std::env::temp_dir().join(format!("tp-venv-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg = tmp.join("home/.claude/mcp-servers/real");
    std::fs::create_dir_all(&pkg).unwrap();
    let made = Command::new("python3")
        .args(["-m", "venv", "--without-pip"])
        .arg(pkg.join(".venv"))
        .output();
    let Ok(made) = made else {
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    };
    if !made.status.success() {
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    }
    let site = std::fs::read_dir(pkg.join(".venv/lib"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path()
        .join("site-packages");
    write(&site.join("mcp_probe/__init__.py"), "VALUE = 7\n");
    write(
        &pkg.join("server.py"),
        "import mcp_probe\nprint(mcp_probe.VALUE)\n",
    );
    let python = pkg.join(".venv/bin/python").to_string_lossy().into_owned();
    let script = pkg.join("server.py").to_string_lossy().into_owned();
    let home = tmp.join("home").to_string_lossy().into_owned();

    let mut e = entry(&python, &[&script]);
    let relocation = relocate_entry(&mut e, &home, &tmp.join("data"));
    assert!(relocation.moves.is_empty());

    let out = Command::new(e.command.as_deref().unwrap())
        .args(&e.args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "7");
    let _ = std::fs::remove_dir_all(&tmp);
}
