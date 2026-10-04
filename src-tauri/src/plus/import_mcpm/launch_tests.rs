use super::launch::*;
use super::servers::{map_servers, McpmInput};
use crate::registry::{data_dir_test_lock, DataDirOverride};
use serde_json::{json, Map, Value};

const HOME: &str = "/home/tester";

fn specs() -> Vec<(&'static str, Value)> {
    let s = |c: &str, a: &[&str]| json!({"command": c, "args": a});
    vec![
        ("npx-scoped", s("npx", &["-y", "@scope/pkg-one"])),
        ("npx-version", s("npx", &["-y", "pkg-two@1.2.3", "--flag"])),
        ("npx-bin", s("/usr/bin/npx", &["pkg-three"])),
        ("uvx-plain", s("uvx", &["tool-four"])),
        ("uvx-from", s("uvx", &["--from", "pkg-five", "tool-five"])),
        (
            "uv-run",
            s("uv", &["run", "--directory", "/srv/six", "server.py"]),
        ),
        ("node-script", s("node", &["/srv/seven/index.js"])),
        (
            "node-flags",
            s("node", &["--max-old-space-size=512", "/srv/eight/main.js"]),
        ),
        (
            "node-relocated",
            s(
                "node",
                &[&format!("{HOME}/.claude/mcp-servers/nine/run.js")],
            ),
        ),
        ("python-script", s("python3", &["/srv/ten/server.py"])),
        ("python-module", s("python3", &["-m", "eleven_server"])),
        (
            "python-venv",
            s("/srv/twelve/.venv/bin/python", &["-u", "main.py"]),
        ),
        (
            "bash-script",
            s("bash", &[&format!("{HOME}/.config/mcpm/bin/thirteen.sh")]),
        ),
        ("sh-script", s("sh", &["/srv/fourteen/start.sh"])),
        (
            "docker-run",
            s(
                "docker",
                &[
                    "run",
                    "-i",
                    "--rm",
                    "-v",
                    "/srv/data:/data",
                    "image-fifteen:latest",
                ],
            ),
        ),
        (
            "docker-env",
            s(
                "docker",
                &["run", "-i", "--rm", "-e", "VAR_ONE", "image-sixteen"],
            ),
        ),
        (
            "podman-run",
            s("podman", &["run", "-i", "--rm", "image-seventeen"]),
        ),
        ("bunx", s("bunx", &["pkg-eighteen"])),
        (
            "deno-run",
            s("deno", &["run", "--allow-read", "/srv/nineteen/main.ts"]),
        ),
        (
            "binary",
            s(&format!("{HOME}/.config/mcpm/bin/twenty"), &["--stdio"]),
        ),
    ]
}

fn input() -> McpmInput {
    let servers: Map<String, Value> = specs()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    McpmInput {
        servers,
        home: HOME.into(),
        ..Default::default()
    }
}

#[test]
fn all_twenty_launch_specs_pass_spawn_screening() {
    let (servers, _) = map_servers(&input());
    assert_eq!(servers.len(), 20);
    for s in &servers {
        screen_entry(&s.entry).unwrap_or_else(|e| panic!("{}: {e}", s.entry.id));
    }
}

#[test]
fn all_twenty_pass_after_relocation() {
    let _lock = data_dir_test_lock();
    let tmp = std::env::temp_dir().join(format!("tp-launch-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let _guard = DataDirOverride::set(&tmp);
    let (mut servers, _) = map_servers(&input());
    let mut moved = 0;
    for s in &mut servers {
        moved += relocate_entry(&mut s.entry, HOME, &tmp).moves.len();
        screen_entry(&s.entry).unwrap_or_else(|e| panic!("{}: {e}", s.entry.id));
    }
    assert_eq!(moved, 3);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn relocation_maps_into_stable_data_dir_location() {
    let data = std::path::Path::new("/data");
    let to = relocate_script(HOME, data, &format!("{HOME}/.config/mcpm/bin/tool.sh")).unwrap();
    assert_eq!(to, data.join("imported-scripts/config_mcpm_bin/tool.sh"));
    let to = relocate_script(HOME, data, &format!("{HOME}/.claude/mcp-servers/a/b.js")).unwrap();
    assert_eq!(to, data.join("imported-scripts/claude_mcp-servers/a/b.js"));
    assert!(relocate_script(HOME, data, "/srv/other/x.js").is_none());
    assert!(relocate_script(HOME, data, &format!("{HOME}/.config/mcpm/bin/../x")).is_none());
}

#[test]
fn screening_still_rejects_inline_eval() {
    let mut entry = map_servers(&input()).0.remove(0).entry;
    entry.command = Some("node".into());
    entry.args = vec!["-e".into(), "1".into()];
    assert!(screen_entry(&entry).is_err());
}
