use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Map};
use toolport_plus_api::{
    HttpError, HttpResponse, McpmSource, MemoryHost, MemoryRegistry, MemorySecrets, ScriptedHttp,
    ServerRecord,
};

use super::journal::Journal;
use super::release::{self, Asset, Platform, Release};
use super::update::{self, UpdateArgs};
use super::*;
use crate::attention::updates::record_lookup;
use crate::fsx::{enter, Paths, PathsGuard};
use crate::servers::repo::update::RestartChoice;

static NEXT: AtomicUsize = AtomicUsize::new(0);

const MAC: Platform = Platform {
    os: "darwin",
    arch: "arm64",
};
const OLD: &[u8] = b"#!/bin/sh\necho 1.3.0\n";
const NEW: &[u8] = b"#!/bin/sh\necho 1.4.0\n";
const BASE: &str = "https://github.com/acme/golf-mcp/releases/download/v1.4.0";

struct World {
    base: PathBuf,
    data: PathBuf,
    binary: PathBuf,
    _paths: PathsGuard,
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "plus-release-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let (data, home) = (base.join("data"), base.join("home"));
        let binary = home.join(".claude/mcp-servers/golf/golf");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(&binary, OLD).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self {
            _paths: enter(Paths {
                home: Some(home),
                data_dir: Some(data.clone()),
            }),
            base,
            data,
            binary,
        }
    }

    fn server(&self, source: &[(&str, serde_json::Value)]) -> ServerRecord {
        let mut extra = Map::new();
        extra.insert("transport".into(), json!("stdio"));
        extra.insert("command".into(), json!(self.binary.to_string_lossy()));
        extra.insert("args".into(), json!(["--stdio"]));
        let mut fields = Map::new();
        fields.insert("current_version".into(), json!("1.3.0"));
        fields.insert("asset_pattern".into(), json!("golf_{version}_{os}_{arch}"));
        for (key, value) in source {
            fields.insert((*key).into(), value.clone());
        }
        ServerRecord {
            id: "srv-golf".into(),
            name: "golf".into(),
            enabled: true,
            extra,
            fork: toolport_plus_api::ForkServerFields {
                mcpm_source: Some(McpmSource {
                    kind: Some("github-release".into()),
                    repo: Some("acme/golf-mcp".into()),
                    path: Some("~/.claude/mcp-servers/golf/golf".into()),
                    extra: fields,
                    ..Default::default()
                }),
                ..Default::default()
            },
        }
    }

    fn host(
        &self,
        server: ServerRecord,
        replies: Vec<Result<HttpResponse, HttpError>>,
    ) -> MemoryHost {
        MemoryHost {
            registry: MemoryRegistry::with_servers(vec![server]),
            secrets: MemorySecrets {
                data_dir: Some(self.data.clone()),
                ..Default::default()
            },
            http: ScriptedHttp::answering(replies),
            ..Default::default()
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn reply(status: u16, body: &[u8]) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse {
        status,
        headers: Vec::new(),
        body: body.to_vec(),
        truncated: false,
        url: "https://github.test/".into(),
    })
}

fn asset(name: &str, size: usize) -> serde_json::Value {
    json!({"name": name, "browser_download_url": format!("{BASE}/{name}"), "size": size})
}

fn release_json(assets: &[serde_json::Value]) -> Result<HttpResponse, HttpError> {
    reply(
        200,
        json!({"tag_name": "v1.4.0", "assets": assets})
            .to_string()
            .as_bytes(),
    )
}

fn standard(file: &str, bytes: &[u8]) -> Vec<serde_json::Value> {
    vec![
        asset(file, bytes.len()),
        asset("golf_1.4.0_linux_amd64", 10),
        asset("checksums.txt", 200),
    ]
}

fn listing(file: &str, bytes: &[u8]) -> Result<HttpResponse, HttpError> {
    reply(
        200,
        format!(
            "{}  {file}\n{}  other_file\n",
            release::sha256_hex(bytes),
            "0".repeat(64)
        )
        .as_bytes(),
    )
}

/// The replies one plan asks for: the release by tag, then its checksum file.
fn planned(file: &str, bytes: &[u8]) -> Vec<Result<HttpResponse, HttpError>> {
    vec![release_json(&standard(file, bytes)), listing(file, bytes)]
}

fn args(to: &str) -> UpdateArgs {
    UpdateArgs {
        to: Some(to.into()),
        platform: MAC,
        restart: RestartChoice::Skip,
        ..UpdateArgs::new("golf")
    }
}

fn target() -> Target {
    Target {
        kind: Kind::Github,
        name: "acme/golf-mcp".into(),
        flavor: String::new(),
    }
}

fn on_disk(world: &World) -> Vec<u8> {
    std::fs::read(&world.binary).unwrap()
}

fn recorded(host: &MemoryHost, key: &str) -> serde_json::Value {
    host.registry.servers.borrow()[0]
        .fork
        .mcpm_source
        .as_ref()
        .unwrap()
        .extra
        .get(key)
        .cloned()
        .unwrap_or_default()
}

fn rel(names: &[&str]) -> Release {
    Release {
        tag: "v1.4.0".into(),
        assets: names
            .iter()
            .map(|name| Asset {
                name: (*name).into(),
                url: format!("{BASE}/{name}"),
                size: 1,
            })
            .collect(),
    }
}

#[test]
fn test01_a_release_install_is_known_only_when_its_source_names_the_file_and_the_server_runs_it() {
    let world = World::new("method");
    let method = release::method(&world.server(&[])).unwrap();
    assert_eq!(method.pattern, "golf_{version}_{os}_{arch}");
    assert_eq!(method.binary, "golf");
    let pin = pinned(&world.server(&[])).unwrap();
    assert_eq!(
        (
            pin.target.kind,
            pin.target.name.as_str(),
            pin.version.as_str(),
            pin.manual
        ),
        (Kind::Github, "acme/golf-mcp", "1.3.0", None)
    );

    let mut bare = world.server(&[]);
    bare.fork
        .mcpm_source
        .as_mut()
        .unwrap()
        .extra
        .remove("asset_pattern");
    assert!(release::method(&bare)
        .unwrap_err()
        .contains("asset_pattern"));
    assert!(
        pinned(&bare).unwrap().manual.is_some(),
        "the row then says what to do by hand"
    );

    let mut elsewhere = world.server(&[]);
    elsewhere
        .extra
        .insert("command".into(), json!("/opt/other/golf"));
    assert!(release::method(&elsewhere)
        .unwrap_err()
        .contains("/opt/other/golf"));

    let mut gone = world.server(&[]);
    gone.fork.mcpm_source.as_mut().unwrap().path = Some("~/.claude/nowhere/golf".into());
    assert!(release::method(&gone).unwrap_err().contains("is not there"));

    let mut versionless = world.server(&[]);
    versionless
        .fork
        .mcpm_source
        .as_mut()
        .unwrap()
        .extra
        .remove("current_version");
    assert!(pinned(&versionless).is_none());
    let mut remote = world.server(&[]);
    remote.extra.insert("transport".into(), json!("http"));
    assert!(pinned(&remote).is_none());
}

#[test]
fn test02_the_pattern_names_one_release_file_by_prefix_or_by_glob() {
    let release = rel(&[
        "golf_1.4.0_darwin_arm64.tar.gz",
        "golf_1.4.0_darwin_arm64.tar.gz.sha256",
        "golf_1.4.0_linux_amd64.tar.gz",
        "checksums.txt",
    ]);
    let pick = |pattern: &str| release::pick_asset(&release, pattern, MAC).map(|a| a.name.clone());
    assert_eq!(
        pick("golf_{version}_{os}_{arch}").as_deref(),
        Ok("golf_1.4.0_darwin_arm64.tar.gz")
    );
    assert_eq!(
        pick("*darwin_arm64*").as_deref(),
        Ok("golf_1.4.0_darwin_arm64.tar.gz")
    );
    let none = pick("golf_{tag}_*_arm64*").unwrap_err();
    assert!(
        none.contains("matches golf_v1.4.0_*_arm64*")
            && none.contains("golf_1.4.0_linux_amd64.tar.gz"),
        "{none}"
    );
    let ambiguous = pick("golf_").unwrap_err();
    assert!(ambiguous.contains("2 files") && ambiguous.contains("narrow the pattern"));
}

#[test]
fn test03_a_published_checksum_is_read_from_a_listing_a_bsd_line_or_a_file_of_its_own() {
    let digest = "ab".repeat(32);
    let file = "golf_1.4.0_darwin_arm64";
    let release = rel(&[file, "SHA256SUMS"]);
    let found = |text: String| {
        let http = ScriptedHttp::answering(vec![reply(200, text.as_bytes())]);
        release::checksum_for(&http, &release, &release.assets[0]).unwrap()
    };
    for text in [
        format!("{digest}  {file}\n"),
        format!("{digest} *{file}\n"),
        format!("{}  other\n{digest}  ./{file}\n", "00".repeat(32)),
        format!("SHA256 ({file}) = {}\n", digest.to_uppercase()),
    ] {
        assert_eq!(
            found(text.clone()).map(|c| c.sha256),
            Some(digest.clone()),
            "{text}"
        );
    }
    assert_eq!(found(format!("{digest}  something_else\n")), None);

    let own = rel(&[file, &format!("{file}.sha256")]);
    let http = ScriptedHttp::answering(vec![reply(200, format!("{digest}\n").as_bytes())]);
    assert_eq!(
        release::checksum_for(&http, &own, &own.assets[0])
            .unwrap()
            .unwrap()
            .source,
        format!("{file}.sha256")
    );
    let none = rel(&[file, "notes.md"]);
    let http = ScriptedHttp::answering(vec![]);
    assert_eq!(
        release::checksum_for(&http, &none, &none.assets[0]),
        Ok(None)
    );
    assert!(http.requests.borrow().is_empty());
}

#[test]
fn test04_the_newest_release_comes_from_the_api_and_a_missing_one_is_no_answer() {
    let http = ScriptedHttp::answering(vec![
        reply(200, br#"{"tag_name":"v1.4.0"}"#),
        reply(404, b"{}"),
        reply(503, b"down"),
        Err(HttpError::Connect("no route".into())),
    ]);
    assert_eq!(
        release::latest(&http, &target()).unwrap().as_deref(),
        Some("v1.4.0")
    );
    assert_eq!(release::latest(&http, &target()), Ok(None));
    assert_eq!(
        release::latest(&http, &target()),
        Err("api.github.com answered 503".into())
    );
    assert!(release::latest(&http, &target())
        .unwrap_err()
        .contains("no route"));
    let first = http.requests.borrow()[0].clone();
    assert_eq!(
        first.url,
        "https://api.github.com/repos/acme/golf-mcp/releases/latest"
    );
    assert!(first.has_header("user-agent") && first.has_header("accept"));
}

#[test]
fn test05_a_source_that_follows_pre_releases_takes_the_newest_published_one_and_no_draft() {
    let http = ScriptedHttp::answering(vec![reply(
        200,
        br#"[{"tag_name":"v1.5.0-rc.1"},{"tag_name":"v2.0.0","draft":true},{"tag_name":"v1.4.0"}]"#,
    )]);
    let pre = Target {
        flavor: release::PRERELEASE_FLAVOR.into(),
        ..target()
    };
    assert_eq!(
        release::latest(&http, &pre).unwrap().as_deref(),
        Some("v1.5.0-rc.1")
    );
    assert!(http.requests.borrow()[0]
        .url
        .contains("/releases?per_page="));
}

#[test]
fn test06_the_check_row_offers_the_update_when_the_install_is_known_and_says_what_to_do_when_not() {
    let world = World::new("row");
    let server = world.server(&[]);
    let host = world.host(server, vec![]);
    record_lookup(&world.data, 1_000, &target(), &Ok(Some("v1.4.0".into()))).unwrap();
    let rows = |host: &MemoryHost| {
        crate::attention::snapshot(host)
            .items
            .into_iter()
            .filter(|item| item.id.starts_with("updates:package:"))
            .collect::<Vec<_>>()
    };
    let found = rows(&host);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "updates:package:golf:1.3.0>v1.4.0");
    assert_eq!(
        found[0].title,
        "golf: release v1.4.0 of acme/golf-mcp is out, installed 1.3.0"
    );
    let action = found[0].action.as_ref().unwrap();
    assert_eq!(action.label, "Update");
    assert_eq!(
        action.command.as_deref().unwrap(),
        [
            "toolportctl",
            "server",
            "update",
            "golf",
            "--dry-run",
            "--fetch"
        ]
    );

    host.registry.servers.borrow_mut()[0]
        .fork
        .mcpm_source
        .as_mut()
        .unwrap()
        .extra
        .remove("asset_pattern");
    let manual = rows(&host);
    let action = manual[0].action.as_ref().unwrap();
    assert_eq!(action.label, "Open Servers");
    assert!(manual[0].detail.contains("asset_pattern"));
    assert!(manual[0]
        .detail
        .contains("https://github.com/acme/golf-mcp/releases"));
    assert_eq!(manual[0].skip.as_deref(), Some("Skip v1.4.0"));
}

#[test]
fn test10_the_plan_names_the_file_its_checksum_and_the_registry_change() {
    let world = World::new("plan");
    let host = world.host(world.server(&[]), planned("golf_1.4.0_darwin_arm64", NEW));
    let plan = update::plan(&host, &args("1.4.0"), false).unwrap();
    assert!(plan.refusal.is_none(), "{:?}", plan.refusal);
    assert_eq!(plan.to, "v1.4.0");
    assert!(plan.verified);
    let value = plan.to_value();
    assert_eq!(value["release"]["asset"]["name"], "golf_1.4.0_darwin_arm64");
    assert_eq!(value["release"]["checksum"]["source"], "checksums.txt");
    let fields: Vec<&str> = value["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields[0], "mcpmSource.current_version");
    assert!(fields[1].starts_with("file ") && fields[1].ends_with("/golf/golf"));
    assert_eq!(
        value["changes"][0]["to"], "1.4.0",
        "the source keeps spelling versions without the v"
    );
    assert_eq!(on_disk(&world), OLD, "a plan changes nothing");
    assert_eq!(recorded(&host, "current_version"), "1.3.0");
}

#[test]
fn test11_applying_replaces_the_binary_records_the_version_and_keeps_the_old_one_for_undo() {
    let world = World::new("apply");
    let mut replies = planned("golf_1.4.0_darwin_arm64", NEW);
    replies.push(reply(200, NEW));
    let host = world.host(world.server(&[]), replies);
    let done = update::apply(&host, &args("1.4.0")).unwrap();
    assert!(done.changed);
    assert_eq!(on_disk(&world), NEW);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&world.binary)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the new binary stays executable");
    }
    assert_eq!(recorded(&host, "current_version"), "1.4.0");
    assert!(recorded(&host, "last_updated")
        .as_str()
        .unwrap()
        .ends_with('Z'));
    assert_eq!(
        recorded(&host, "asset_pattern"),
        "golf_{version}_{os}_{arch}"
    );
    let journal = Journal::read(&world.data).unwrap();
    let file = journal.applied["srv-golf"].file.as_ref().unwrap();
    assert_eq!(std::fs::read(&file.backup).unwrap(), OLD);
    assert_eq!(file.installed_sha256, release::sha256_hex(NEW));
    assert_eq!(
        host.http.requests.borrow().last().unwrap().url,
        format!("{BASE}/golf_1.4.0_darwin_arm64")
    );

    let back = update::undo(&host, &args("1.4.0")).unwrap();
    assert!(back.changed);
    assert_eq!(on_disk(&world), OLD);
    assert_eq!(recorded(&host, "current_version"), "1.3.0");
    assert!(
        recorded(&host, "last_updated").is_null(),
        "the source reads as it did before"
    );
    assert!(Journal::read(&world.data).unwrap().applied.is_empty());
    assert!(!std::path::Path::new(&file.backup).exists());
}

#[test]
fn test12_a_download_that_does_not_match_its_checksum_changes_nothing() {
    let world = World::new("mismatch");
    let mut replies = planned("golf_1.4.0_darwin_arm64", NEW);
    replies.push(reply(200, b"tampered"));
    let host = world.host(world.server(&[]), replies);
    let error = update::apply(&host, &args("1.4.0")).unwrap_err();
    assert!(error
        .message
        .contains("does not match the checksum in checksums.txt"));
    assert_eq!(on_disk(&world), OLD);
    assert_eq!(recorded(&host, "current_version"), "1.3.0");
    assert!(Journal::read(&world.data).unwrap().applied.is_empty());
}

#[test]
fn test13_a_release_without_a_checksum_is_installed_only_when_the_person_allows_it() {
    let world = World::new("unverified");
    let file = "golf_1.4.0_darwin_arm64";
    let bare = || release_json(&[asset(file, NEW.len()), asset("golf_1.4.0_linux_amd64", 1)]);
    let host = world.host(
        world.server(&[]),
        vec![bare(), bare(), bare(), reply(200, NEW)],
    );
    let plan = update::plan(&host, &args("1.4.0"), false).unwrap();
    assert!(plan.release.as_ref().unwrap().checksum.is_none());
    assert!(plan.human().contains("--allow-unverified"));
    assert_eq!(
        plan.to_value()["release"]["checksum"],
        serde_json::Value::Null
    );
    let error = update::apply(&host, &args("1.4.0")).unwrap_err();
    assert_eq!(error.details.unwrap()["reason"], "unverified");
    assert_eq!(on_disk(&world), OLD);
    let done = update::apply(
        &host,
        &UpdateArgs {
            allow_unverified: true,
            ..args("1.4.0")
        },
    )
    .unwrap();
    assert!(done.changed);
    assert_eq!(on_disk(&world), NEW);
}

#[test]
fn test14_a_binary_that_fails_its_check_is_put_back_and_the_registry_stays_as_it_was() {
    let world = World::new("putback");
    let server = world.server(&[("verify_command", json!("exit 3"))]);
    let mut replies = planned("golf_1.4.0_darwin_arm64", NEW);
    replies.push(reply(200, NEW));
    let host = world.host(server, replies);
    let error = update::apply(&host, &args("1.4.0")).unwrap_err();
    assert!(
        error.message.contains("the previous one is back"),
        "{}",
        error.message
    );
    assert_eq!(error.details.unwrap()["putBack"], true);
    assert_eq!(on_disk(&world), OLD);
    assert_eq!(recorded(&host, "current_version"), "1.3.0");
    assert!(Journal::read(&world.data).unwrap().applied.is_empty());
}

#[test]
fn test15_a_passing_verify_command_lets_the_update_stand() {
    let world = World::new("verify");
    let server = world.server(&[("verify_command", json!("./golf | grep -q 1.4.0"))]);
    let mut replies = planned("golf_1.4.0_darwin_arm64", NEW);
    replies.push(reply(200, NEW));
    let host = world.host(server, replies);
    assert!(update::apply(&host, &args("1.4.0")).unwrap().changed);
    assert_eq!(on_disk(&world), NEW);
}

fn tar_gz(world: &World, member: &str, content: &[u8]) -> Option<Vec<u8>> {
    let dir = world.base.join("pack");
    let inner = dir.join(member);
    std::fs::create_dir_all(inner.parent().unwrap()).unwrap();
    std::fs::write(&inner, content).unwrap();
    std::fs::write(dir.join("README.md"), "docs").unwrap();
    let out = world.base.join("pack.tar.gz");
    let made = Command::new("tar")
        .args(["-czf"])
        .arg(&out)
        .arg("-C")
        .arg(&dir)
        .arg(".")
        .status()
        .ok()?;
    made.success().then(|| std::fs::read(out).unwrap())
}

#[test]
fn test16_a_release_archive_is_unpacked_and_only_the_named_binary_is_taken() {
    let world = World::new("archive");
    let Some(archive) = tar_gz(&world, "golf-1.4.0/golf", NEW) else {
        return;
    };
    let file = "golf_1.4.0_darwin_arm64.tar.gz";
    let mut replies = planned(file, &archive);
    replies.push(reply(200, &archive));
    let host = world.host(world.server(&[]), replies);
    update::apply(&host, &args("1.4.0")).unwrap();
    assert_eq!(on_disk(&world), NEW);
    assert!(
        std::fs::read_dir(world.data.join("plus/server-updates/srv-golf"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("unpack-")),
        "the unpacked files are removed"
    );
}

#[test]
fn test17_an_archive_without_the_binary_changes_nothing() {
    let world = World::new("nobinary");
    let Some(archive) = tar_gz(&world, "golf-1.4.0/other", NEW) else {
        return;
    };
    let file = "golf_1.4.0_darwin_arm64.tar.gz";
    let mut replies = planned(file, &archive);
    replies.push(reply(200, &archive));
    let host = world.host(world.server(&[]), replies);
    let error = update::apply(&host, &args("1.4.0")).unwrap_err();
    assert!(
        error.message.contains("holds no file called golf"),
        "{}",
        error.message
    );
    assert_eq!(on_disk(&world), OLD);
}

#[test]
fn test18_undo_does_not_overwrite_a_binary_that_changed_after_the_update() {
    let world = World::new("undoguard");
    let mut replies = planned("golf_1.4.0_darwin_arm64", NEW);
    replies.push(reply(200, NEW));
    let host = world.host(world.server(&[]), replies);
    update::apply(&host, &args("1.4.0")).unwrap();
    std::fs::write(&world.binary, b"rebuilt by hand").unwrap();
    let error = update::undo(&host, &args("1.4.0")).unwrap_err();
    assert_eq!(error.details.unwrap()["reason"], "moved");
    assert_eq!(on_disk(&world), b"rebuilt by hand");
    assert_eq!(recorded(&host, "current_version"), "1.4.0");
}

#[test]
fn test19_an_unreachable_github_stops_the_plan_and_an_unknown_release_is_refused() {
    let world = World::new("offline");
    let host = world.host(
        world.server(&[]),
        vec![
            Err(HttpError::Connect("no route".into())),
            reply(404, b"{}"),
            reply(404, b"{}"),
            reply(404, b"{}"),
        ],
    );
    let error = update::plan(&host, &args("1.4.0"), false).unwrap_err();
    assert_eq!(error.kind.code(), "lookup_failed");
    assert!(error.message.contains("no route"));
    let plan = update::plan(&host, &args("9.9.9"), false).unwrap();
    assert_eq!(plan.refusal.unwrap().code, "unknown_version");
    assert_eq!(on_disk(&world), OLD);
}

#[test]
fn test20_a_release_file_with_no_match_or_too_large_is_refused_with_what_the_release_has() {
    let world = World::new("noasset");
    let host = world.host(
        world.server(&[]),
        vec![
            release_json(&[asset("golf_1.4.0_linux_amd64", 5)]),
            release_json(&[asset(
                "golf_1.4.0_darwin_arm64",
                (release::MAX_ASSET_BYTES + 1) as usize,
            )]),
        ],
    );
    let none = update::plan(&host, &args("1.4.0"), false)
        .unwrap()
        .refusal
        .unwrap();
    assert_eq!(none.code, "no_asset");
    assert!(none.message.contains("golf_1.4.0_linux_amd64"));
    let big = update::plan(&host, &args("1.4.0"), false)
        .unwrap()
        .refusal
        .unwrap();
    assert_eq!(big.code, "too_large");
}

#[test]
fn test21_an_archive_member_outside_its_own_folder_is_never_unpacked() {
    let world = World::new("escape");
    let dir = world.base.join("evil");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("golf"), NEW).unwrap();
    let out = world.base.join("evil.tar");
    let made = Command::new("tar")
        .arg("-cf")
        .arg(&out)
        .args(["-s", "|^|../|", "-C"])
        .arg(&dir)
        .arg("golf")
        .status();
    if !made.is_ok_and(|status| status.success()) {
        return;
    }
    let bytes = std::fs::read(&out).unwrap();
    let error = release::binary_from(
        "golf_1.4.0_darwin_arm64.tar",
        &bytes,
        "golf",
        &world.base.join("work"),
    )
    .unwrap_err();
    assert!(error.contains("outside its own folder"), "{error}");
    assert!(!world.base.join("golf").exists());
}
