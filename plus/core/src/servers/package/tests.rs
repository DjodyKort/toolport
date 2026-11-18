use serde_json::{json, Map};
use toolport_plus_api::{HttpError, HttpResponse, McpmSource, ScriptedHttp, ServerRecord};

use super::lookup;
use super::version::{exact, newer, newest_tag, tag_flavor};
use super::*;

fn server(command: &str, args: &[&str]) -> ServerRecord {
    let mut extra = Map::new();
    extra.insert("transport".into(), json!("stdio"));
    extra.insert("command".into(), json!(command));
    extra.insert("args".into(), json!(args));
    ServerRecord {
        id: "srv".into(),
        name: "files".into(),
        enabled: true,
        extra,
        ..Default::default()
    }
}

fn pin(command: &str, args: &[&str]) -> Option<(Kind, String, String, String)> {
    pinned(&server(command, args))
        .map(|p| (p.target.kind, p.target.name, p.version, p.target.flavor))
}

fn answer(status: u16, body: &str) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse {
        status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
        truncated: false,
        url: "https://registry.test/".into(),
    })
}

#[test]
fn test01_an_npx_server_pins_the_version_after_the_at_sign() {
    for (command, args) in [
        ("npx", vec!["-y", "@scope/files@1.4.2", "/data"]),
        ("npx.cmd", vec!["--yes", "@scope/files@1.4.2"]),
        ("bunx", vec!["@scope/files@1.4.2"]),
        (
            "/usr/local/bin/npx",
            vec!["-y", "--registry", "https://r.test", "@scope/files@1.4.2"],
        ),
        ("pnpm", vec!["dlx", "@scope/files@1.4.2"]),
        (
            "npx",
            vec!["-y", "--package=@scope/files@1.4.2", "files-cli"],
        ),
        ("npx", vec!["-y", "-p", "@scope/files@1.4.2", "files-cli"]),
    ] {
        assert_eq!(
            pin(command, &args),
            Some((
                Kind::Npm,
                "@scope/files".into(),
                "1.4.2".into(),
                String::new()
            )),
            "{command} {args:?}"
        );
    }
    assert_eq!(
        pin("npx", &["-y", "files@2.0.0-rc.1"]).map(|p| p.2),
        Some("2.0.0-rc.1".into())
    );
}

#[test]
fn test02_an_unpinned_or_ranged_npm_package_has_no_version_to_judge() {
    for args in [
        vec!["-y", "files"],
        vec!["-y", "@scope/files"],
        vec!["-y", "files@latest"],
        vec!["-y", "files@^1.2.0"],
        vec!["-y", "files@1"],
        vec!["-y", "files@1.x"],
        vec!["-y", "files", "--package=other@1.0.0"],
        vec![],
    ] {
        assert_eq!(pin("npx", &args), None, "{args:?}");
    }
}

#[test]
fn test03_a_uvx_or_pipx_server_pins_with_double_equals_or_at() {
    for (command, args) in [
        ("uvx", vec!["files-mcp==0.9.1"]),
        ("uvx", vec!["files-mcp@0.9.1"]),
        (
            "uvx",
            vec!["--python", "3.12", "files-mcp==0.9.1", "--root", "/data"],
        ),
        ("uvx", vec!["--from", "files-mcp==0.9.1", "files"]),
        ("uvx", vec!["--from=files-mcp[extra]==0.9.1", "files"]),
        ("uv", vec!["tool", "run", "files-mcp==0.9.1"]),
        ("pipx", vec!["run", "files-mcp==0.9.1"]),
    ] {
        assert_eq!(
            pin(command, &args),
            Some((
                Kind::Pypi,
                "files-mcp".into(),
                "0.9.1".into(),
                String::new()
            )),
            "{command} {args:?}"
        );
    }
    for args in [
        vec!["files-mcp"],
        vec!["files-mcp>=0.9"],
        vec!["files-mcp@latest"],
        vec!["--from", "files-mcp", "files"],
    ] {
        assert_eq!(pin("uvx", &args), None, "{args:?}");
    }
}

#[test]
fn test04_a_docker_server_pins_a_tag_on_docker_hub_only() {
    let hub = pin(
        "docker",
        &[
            "run",
            "-i",
            "--rm",
            "-e",
            "API_KEY",
            "-v",
            "/data:/data",
            "acme/files:1.2.3",
        ],
    )
    .unwrap();
    assert_eq!(
        (hub.0, hub.1.as_str(), hub.2.as_str(), hub.3.as_str()),
        (Kind::Docker, "acme/files", "1.2.3", "|3|")
    );
    let official = pin("podman", &["run", "--rm", "node:22.4.1-alpine"]).unwrap();
    assert_eq!(
        (
            official.1.as_str(),
            official.2.as_str(),
            official.3.as_str()
        ),
        ("library/node", "22.4.1-alpine", "|3|-alpine")
    );
    assert_eq!(
        pin("docker", &["run", "docker.io/acme/files:v2.0"]).map(|p| p.1),
        Some("acme/files".into())
    );
    for image in [
        "acme/files",
        "acme/files:latest",
        "ghcr.io/acme/files:1.2.3",
        "localhost:5000/files:1.2.3",
        "acme/files@sha256:abcd",
    ] {
        assert_eq!(pin("docker", &["run", "--rm", image]), None, "{image}");
    }
}

#[test]
fn test05_the_image_the_server_source_records_counts_when_the_command_is_a_wrapper() {
    let mut record = server("run-files", &["--serve"]);
    let mut extra = Map::new();
    extra.insert("image".into(), json!("acme/files"));
    extra.insert("tag".into(), json!("3.1.0"));
    record.fork.mcpm_source = Some(McpmSource {
        kind: Some("docker".into()),
        extra,
        ..Default::default()
    });
    let found = pinned(&record).unwrap();
    assert_eq!(found.target.name, "acme/files");
    assert_eq!(found.version, "3.1.0");
    record
        .fork
        .mcpm_source
        .as_mut()
        .unwrap()
        .extra
        .insert("tag".into(), json!("latest"));
    assert_eq!(pinned(&record), None);
}

#[test]
fn test06_only_enabled_stdio_servers_count_and_a_package_is_asked_once() {
    let mut off = server("npx", &["files@1.0.0"]);
    off.enabled = false;
    off.id = "off".into();
    let mut remote = server("npx", &["files@1.0.0"]);
    remote.id = "remote".into();
    remote.extra.insert("transport".into(), json!("http"));
    let mut first = server("npx", &["files@1.0.0"]);
    first.id = "a".into();
    first.name = "alpha".into();
    let mut second = server("npx", &["files@1.0.0"]);
    second.id = "b".into();
    second.name = "beta".into();
    let found = pins(&[off, remote, second, first]);
    assert_eq!(
        found
            .iter()
            .map(|p| p.server_name.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    assert_eq!(targets(&found).len(), 1);
    assert_eq!(targets(&found)[0].source(), "package:npm:files");
}

#[test]
fn test07_versions_compare_by_number_and_a_pre_release_sorts_below_its_release() {
    for (latest, pinned) in [
        ("1.4.3", "1.4.2"),
        ("1.10.0", "1.9.9"),
        ("2.0.0", "1.99.99"),
        ("1.0.0", "1.0.0-rc.2"),
        ("1.0.0-rc.10", "1.0.0-rc.2"),
        ("2024.2", "2024.1.5"),
        ("v1.3", "v1.2.9"),
        ("1.1.0", "1.0.0+build5"),
    ] {
        assert!(newer(latest, pinned), "{latest} over {pinned}");
        assert!(!newer(pinned, latest), "{pinned} over {latest}");
    }
    for same in [
        ("1.0", "1.0.0"),
        ("1.2.3", "1.2.3+meta"),
        ("v1.0.0", "1.0.0"),
    ] {
        assert!(!newer(same.0, same.1) && !newer(same.1, same.0), "{same:?}");
    }
    assert!(!newer("latest", "1.0.0") && !newer("1.0.0", "next"));
}

#[test]
fn test08_exact_tells_a_release_from_a_range_a_tag_or_a_wildcard() {
    for yes in [
        "1.2.3",
        "0.0.1",
        "1.2.3-beta.1",
        "2.0rc1",
        "1.0.post2",
        "2024.11",
    ] {
        assert!(exact(yes), "{yes}");
    }
    for no in [
        "latest",
        "^1.2.3",
        "~1",
        ">=1.0",
        "1.x",
        "*",
        "1.2.3 - 2",
        "next",
        "",
    ] {
        assert!(!exact(no), "{no}");
    }
}

#[test]
fn test09_a_tag_is_compared_with_tags_of_its_own_shape_only() {
    let tags: Vec<String> = [
        "2.0.0",
        "1.9.9",
        "2.1.0-alpine",
        "2.0.1-alpine",
        "latest",
        "2.0",
        "3",
        "v4.0.0",
    ]
    .map(String::from)
    .to_vec();
    let flavor = tag_flavor("1.0.0-alpine").unwrap();
    assert_eq!(newest_tag(&tags, &flavor).as_deref(), Some("2.1.0-alpine"));
    assert_eq!(
        newest_tag(&tags, &tag_flavor("1.0.0").unwrap()).as_deref(),
        Some("2.0.0")
    );
    assert_eq!(
        newest_tag(&tags, &tag_flavor("1.0").unwrap()).as_deref(),
        Some("2.0")
    );
    assert_eq!(
        newest_tag(&tags, &tag_flavor("v1.0.0").unwrap()).as_deref(),
        Some("v4.0.0")
    );
    assert_eq!(tag_flavor("latest"), None);
    assert_eq!(tag_flavor("1.2.3_alpine"), None);
}

fn target(kind: Kind, name: &str, flavor: &str) -> Target {
    Target {
        kind,
        name: name.into(),
        flavor: flavor.into(),
    }
}

#[test]
fn test10_each_registry_is_asked_in_its_own_way_and_answers_with_the_newest_version() {
    let http = ScriptedHttp::answering(vec![
        answer(200, r#"{"name":"@scope/files","version":"1.5.0"}"#),
        answer(200, r#"{"info":{"version":"0.10.0"},"releases":{}}"#),
        answer(
            200,
            r#"{"results":[{"name":"latest"},{"name":"1.3.0"},{"name":"1.4.0-alpine"},{"name":"1.2.3"}]}"#,
        ),
    ]);
    assert_eq!(
        lookup::latest(&http, &target(Kind::Npm, "@scope/files", ""))
            .unwrap()
            .as_deref(),
        Some("1.5.0")
    );
    assert_eq!(
        lookup::latest(&http, &target(Kind::Pypi, "files-mcp", ""))
            .unwrap()
            .as_deref(),
        Some("0.10.0")
    );
    assert_eq!(
        lookup::latest(&http, &target(Kind::Docker, "acme/files", "|3|"))
            .unwrap()
            .as_deref(),
        Some("1.3.0")
    );
    let urls: Vec<String> = http
        .requests
        .borrow()
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(urls[0], "https://registry.npmjs.org/@scope%2Ffiles/latest");
    assert_eq!(urls[1], "https://pypi.org/pypi/files-mcp/json");
    assert!(urls[2].starts_with("https://hub.docker.com/v2/repositories/acme/files/tags?"));
}

#[test]
fn test11_a_missing_package_is_no_answer_and_a_broken_registry_is_an_error() {
    let http = ScriptedHttp::answering(vec![
        answer(404, r#"{"error":"not found"}"#),
        answer(503, "down"),
        answer(200, "<html>"),
        Err(HttpError::Connect("no route".into())),
    ]);
    let npm = target(Kind::Npm, "private-files", "");
    assert_eq!(lookup::latest(&http, &npm), Ok(None));
    assert_eq!(
        lookup::latest(&http, &npm),
        Err("registry.npmjs.org answered 503".into())
    );
    assert!(lookup::latest(&http, &npm)
        .unwrap_err()
        .contains("not JSON"));
    assert!(lookup::latest(&http, &npm)
        .unwrap_err()
        .contains("no route"));
}
