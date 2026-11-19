use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Map};
use toolport_plus_api::{
    GatewayRouter, HttpError, HttpResponse, McpmSource, MemoryGateway, MemoryHost, MemoryRegistry,
    MemorySecrets, ScriptedHttp, ServerRecord,
};

use super::journal::Journal;
use super::update::{self, UpdateArgs};
use super::*;
use crate::attention::updates::record_lookup;
use crate::fsx::{enter, Paths, PathsGuard};
use crate::servers::repo::update::RestartChoice;
use crate::servers::restart::{self, Timing};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const FAST: Timing = Timing {
    poll: Duration::from_millis(15),
    settle: Duration::from_millis(120),
    ack_wait: Duration::from_millis(300),
};

struct World {
    base: PathBuf,
    data: PathBuf,
    _paths: PathsGuard,
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "plus-update-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let (data, home) = (base.join("data"), base.join("home"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        Self {
            _paths: enter(Paths {
                home: Some(home),
                data_dir: Some(data.clone()),
            }),
            base,
            data,
        }
    }

    fn host(
        &self,
        servers: Vec<ServerRecord>,
        replies: Vec<Result<HttpResponse, HttpError>>,
    ) -> MemoryHost {
        MemoryHost {
            registry: MemoryRegistry::with_servers(servers),
            secrets: MemorySecrets {
                data_dir: Some(self.data.clone()),
                ..Default::default()
            },
            http: ScriptedHttp::answering(replies),
            ..Default::default()
        }
    }

    /// What the last update check found for a target, as the check records it.
    fn checked(&self, target: &Target, latest: &str) {
        record_lookup(&self.data, 1_000, target, &Ok(Some(latest.into()))).unwrap();
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn args(key: &str) -> UpdateArgs {
    UpdateArgs {
        timing: FAST,
        restart_wait: Duration::from_secs(5),
        restart: RestartChoice::Skip,
        ..UpdateArgs::new(key)
    }
}

fn server(id: &str, name: &str, command: &str, argv: &[&str]) -> ServerRecord {
    let mut extra = Map::new();
    extra.insert("transport".into(), json!("stdio"));
    extra.insert("command".into(), json!(command));
    extra.insert("args".into(), json!(argv));
    extra.insert("cwd".into(), json!("/work/files"));
    extra.insert("disabledTools".into(), json!(["write_file"]));
    ServerRecord {
        id: id.into(),
        name: name.into(),
        enabled: true,
        extra,
        ..Default::default()
    }
}

fn files() -> ServerRecord {
    server(
        "srv-files",
        "files",
        "npx",
        &["-y", "@scope/files@1.4.2", "/data"],
    )
}

fn npm_target() -> Target {
    Target {
        kind: Kind::Npm,
        name: "@scope/files".into(),
        flavor: String::new(),
    }
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

fn arguments(host: &MemoryHost, id: &str) -> Vec<String> {
    host.registry
        .servers
        .borrow()
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .strings("args")
}

fn rewritten(command: &str, argv: &[&str], to: &str) -> Result<(usize, String), String> {
    rewrite(&server("s", "s", command, argv), to).map(|rw| (rw.index, rw.after))
}

#[test]
fn test01_a_pin_is_rewritten_in_the_argument_that_holds_it_and_nothing_else() {
    for (command, argv, index, after) in [
        (
            "npx",
            vec!["-y", "@scope/files@1.4.2", "/data"],
            1,
            "@scope/files@1.5.0",
        ),
        ("bunx", vec!["@scope/files@1.4.2"], 0, "@scope/files@1.5.0"),
        (
            "pnpm",
            vec!["dlx", "@scope/files@1.4.2"],
            1,
            "@scope/files@1.5.0",
        ),
        (
            "npx",
            vec!["-y", "--registry", "https://r.test", "@scope/files@1.4.2"],
            3,
            "@scope/files@1.5.0",
        ),
        (
            "npx",
            vec!["-y", "--package=@scope/files@1.4.2", "files-cli"],
            1,
            "--package=@scope/files@1.5.0",
        ),
        (
            "npx",
            vec!["-y", "-p", "@scope/files@1.4.2", "files-cli"],
            2,
            "@scope/files@1.5.0",
        ),
    ] {
        assert_eq!(
            rewritten(command, &argv, "1.5.0"),
            Ok((index, after.to_string())),
            "{command} {argv:?}"
        );
    }
}

#[test]
fn test02_a_python_pin_keeps_its_separator_extras_and_flag() {
    for (command, argv, index, after) in [
        ("uvx", vec!["files-mcp==0.9.1"], 0, "files-mcp==1.0.0"),
        ("uvx", vec!["files-mcp@0.9.1"], 0, "files-mcp@1.0.0"),
        (
            "uvx",
            vec!["--python", "3.12", "files-mcp==0.9.1", "--root", "/data"],
            2,
            "files-mcp==1.0.0",
        ),
        (
            "uvx",
            vec!["--from", "files-mcp==0.9.1", "files"],
            1,
            "files-mcp==1.0.0",
        ),
        (
            "uvx",
            vec!["--from=files-mcp[extra]==0.9.1", "files"],
            0,
            "--from=files-mcp[extra]==1.0.0",
        ),
        (
            "uv",
            vec!["tool", "run", "files-mcp==0.9.1"],
            2,
            "files-mcp==1.0.0",
        ),
        (
            "pipx",
            vec!["run", "files-mcp==0.9.1"],
            1,
            "files-mcp==1.0.0",
        ),
    ] {
        assert_eq!(
            rewritten(command, &argv, "1.0.0"),
            Ok((index, after.to_string())),
            "{command} {argv:?}"
        );
    }
}

#[test]
fn test03_an_image_tag_is_rewritten_only_to_a_tag_of_the_same_shape() {
    let run = [
        "run",
        "-i",
        "--rm",
        "-e",
        "API_KEY",
        "acme/files:1.2.3-alpine",
    ];
    assert_eq!(
        rewritten("docker", &run, "1.3.0-alpine"),
        Ok((5, "acme/files:1.3.0-alpine".into()))
    );
    let error = rewritten("docker", &run, "1.3.0").unwrap_err();
    assert!(error.contains("would not read back"), "{error}");
    let error = rewritten("docker", &run, "latest").unwrap_err();
    assert!(error.contains("would not read back"), "{error}");
}

#[test]
fn test04_a_version_that_is_a_range_or_a_tag_is_never_written() {
    for bad in ["latest", "^1.5.0", "1.x", "1.5", "1.5.0 --evil", "1.5.0;x"] {
        assert!(
            rewritten("npx", &["-y", "@scope/files@1.4.2"], bad).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn test05_a_pin_toolport_cannot_rewrite_says_why_and_is_left_for_the_person() {
    let mut recipe = files();
    recipe
        .extra
        .insert("launch".into(), json!({"inputs": [{"key": "ROOT"}]}));
    let reason = rewrite(&recipe, "1.5.0").unwrap_err();
    assert!(reason.contains("launch recipe"), "{reason}");
    assert!(pinned(&recipe)
        .unwrap()
        .manual
        .unwrap()
        .contains("launch recipe"));
    assert_eq!(pinned(&files()).unwrap().manual, None);

    let mut wrapper = server("w", "wrapper", "run-files", &["--serve"]);
    let mut extra = Map::new();
    extra.insert("image".into(), json!("acme/files"));
    extra.insert("tag".into(), json!("3.1.0"));
    wrapper.fork.mcpm_source = Some(McpmSource {
        kind: Some("docker".into()),
        extra,
        ..Default::default()
    });
    let reason = rewrite(&wrapper, "3.2.0").unwrap_err();
    assert!(reason.contains("not in its command"), "{reason}");
    assert!(pinned(&wrapper).unwrap().manual.is_some());
}

#[test]
fn test10_the_plan_takes_the_version_the_last_check_found_and_shows_the_exact_change() {
    let world = World::new("plan");
    let host = world.host(vec![files()], vec![]);
    world.checked(&npm_target(), "1.5.0");
    let plan = update::plan(&host, &args("files"), false).unwrap();
    assert!(plan.refusal.is_none());
    assert_eq!((plan.from.as_str(), plan.to.as_str()), ("1.4.2", "1.5.0"));
    assert!(!plan.verified, "no registry was asked");
    let value = plan.to_value();
    assert_eq!(
        value["changes"],
        json!([{"field": "args[1]", "from": "@scope/files@1.4.2", "to": "@scope/files@1.5.0"}])
    );
    assert_eq!(value["undo"], "toolportctl server update files --undo");
    assert!(plan
        .human()
        .contains("args[1]: @scope/files@1.4.2 -> @scope/files@1.5.0"));
    assert_eq!(
        update::plan(&host, &args("files"), false).unwrap().hash,
        plan.hash
    );
    assert!(
        host.http.requests.borrow().is_empty(),
        "the plan from the store asks nobody"
    );
    assert_eq!(
        arguments(&host, "srv-files"),
        ["-y", "@scope/files@1.4.2", "/data"]
    );
}

#[test]
fn test11_without_a_check_or_a_version_the_plan_says_what_to_do_and_names_no_change() {
    let world = World::new("unchecked");
    let host = world.host(vec![files()], vec![]);
    let plan = update::plan(&host, &args("files"), false).unwrap();
    assert_eq!(plan.refusal.as_ref().unwrap().code, "not_checked");
    assert!(plan.changes.is_empty());
    let error = update::apply(&host, &args("files")).unwrap_err();
    assert_eq!(error.details.unwrap()["reason"], "not_checked");
}

#[test]
fn test12_a_current_pin_is_up_to_date_and_an_older_explicit_version_is_a_downgrade() {
    let world = World::new("current");
    let host = world.host(vec![files()], vec![]);
    world.checked(&npm_target(), "1.4.2");
    let plan = update::plan(&host, &args("files"), false).unwrap();
    assert_eq!(plan.refusal.as_ref().unwrap().code, "up_to_date");
    let done = update::apply(&host, &args("files")).unwrap();
    assert!(!done.changed);

    let older = update::plan(
        &host,
        &UpdateArgs {
            to: Some("1.3.0".into()),
            ..args("files")
        },
        false,
    )
    .unwrap();
    assert!(older.refusal.is_none() && older.downgrade);
    assert!(older.human().contains("Go back from 1.4.2 to 1.3.0"));
}

#[test]
fn test13_fetch_asks_the_registry_once_and_the_row_and_the_plan_read_the_same_version() {
    let world = World::new("fetch");
    let host = world.host(vec![files()], vec![answer(200, r#"{"version":"1.6.0"}"#)]);
    let plan = update::plan(
        &host,
        &UpdateArgs {
            fetch: true,
            ..args("files")
        },
        false,
    )
    .unwrap();
    assert_eq!(plan.to, "1.6.0");
    assert!(plan.verified);
    assert_eq!(host.http.requests.borrow().len(), 1);
    let again = update::plan(&host, &args("files"), false).unwrap();
    assert_eq!(
        again.hash, plan.hash,
        "the store holds what the fetch found"
    );
    assert_eq!(host.http.requests.borrow().len(), 1);
}

#[test]
fn test14_an_explicit_version_the_registry_does_not_have_is_refused_when_it_is_asked() {
    let world = World::new("noversion");
    let host = world.host(
        vec![files()],
        vec![answer(404, "{}"), answer(200, r#"{"version":"1.9.0"}"#)],
    );
    let ask = |to: &str| UpdateArgs {
        to: Some(to.into()),
        fetch: true,
        ..args("files")
    };
    let plan = update::plan(&host, &ask("1.9.9"), false).unwrap();
    assert_eq!(plan.refusal.as_ref().unwrap().code, "unknown_version");
    assert_eq!(
        host.http.requests.borrow()[0].url,
        "https://registry.npmjs.org/@scope%2Ffiles/1.9.9"
    );
    let plan = update::plan(&host, &ask("1.9.0"), false).unwrap();
    assert!(plan.refusal.is_none() && plan.verified);
    let error = update::plan(
        &host,
        &UpdateArgs {
            to: Some("latest".into()),
            ..args("files")
        },
        false,
    )
    .unwrap_err();
    assert!(error.message.contains("is not one version"));
}

#[test]
fn test15_an_unreachable_registry_stops_the_plan_with_the_reason() {
    let world = World::new("offline");
    let host = world.host(
        vec![files()],
        vec![Err(HttpError::Connect("no route".into()))],
    );
    let error = update::plan(
        &host,
        &UpdateArgs {
            fetch: true,
            ..args("files")
        },
        false,
    )
    .unwrap_err();
    assert_eq!(error.kind.code(), "lookup_failed");
    assert!(error.message.contains("no route"));
}

#[test]
fn test16_a_server_that_pins_nothing_is_not_a_package_server() {
    let world = World::new("nopin");
    let host = world.host(
        vec![server("srv-free", "free", "npx", &["-y", "free-files"])],
        vec![],
    );
    let error = update::plan(&host, &args("free"), false).unwrap_err();
    assert!(error.message.contains("pins no package version"));
    assert_eq!(
        update::plan(&host, &args("ghost"), false)
            .unwrap_err()
            .kind
            .code(),
        "not_found"
    );
}

#[test]
fn test20_apply_rewrites_only_the_pinned_argument_and_records_the_undo() {
    let world = World::new("apply");
    let host = world.host(vec![files()], vec![]);
    world.checked(&npm_target(), "1.5.0");
    let before = host.registry.servers.borrow()[0].clone();
    let done = update::apply(&host, &args("files")).unwrap();
    assert!(done.changed && done.restart.is_none());
    assert_eq!(
        arguments(&host, "srv-files"),
        ["-y", "@scope/files@1.5.0", "/data"]
    );
    let after = host.registry.servers.borrow()[0].clone();
    assert_eq!(after.extra["cwd"], before.extra["cwd"]);
    assert_eq!(after.extra["disabledTools"], before.extra["disabledTools"]);
    let journal = Journal::read(&world.data).unwrap();
    let kept = &journal.applied["srv-files"];
    assert_eq!((kept.from.as_str(), kept.to.as_str()), ("1.4.2", "1.5.0"));
    assert_eq!(kept.arg.as_ref().unwrap().before, "@scope/files@1.4.2");
    assert_eq!(
        done.to_value()["undo"],
        "toolportctl server update files --undo"
    );
    assert_eq!(
        update::plan(&host, &args("files"), false)
            .unwrap()
            .refusal
            .unwrap()
            .code,
        "up_to_date"
    );
}

#[test]
fn test21_a_reviewed_plan_that_changed_is_not_applied() {
    let world = World::new("hash");
    let host = world.host(vec![files()], vec![]);
    world.checked(&npm_target(), "1.5.0");
    let reviewed = update::plan(&host, &args("files"), false).unwrap().hash;
    world.checked(&npm_target(), "1.6.0");
    let error = update::apply(
        &host,
        &UpdateArgs {
            plan_hash: Some(reviewed.clone()),
            ..args("files")
        },
    )
    .unwrap_err();
    assert_eq!(error.kind.code(), "conflict");
    assert_eq!(arguments(&host, "srv-files")[1], "@scope/files@1.4.2");
    let done = update::apply(
        &host,
        &UpdateArgs {
            plan_hash: Some(reviewed),
            to: Some("1.5.0".into()),
            ..args("files")
        },
    )
    .unwrap();
    assert_eq!(
        done.to, "1.5.0",
        "the version in the reviewed plan is the one installed"
    );
}

#[test]
fn test22_an_edit_made_after_the_plan_is_not_written_over() {
    let world = World::new("race");
    let host = world.host(vec![files()], vec![]);
    let plan = update::plan(
        &host,
        &UpdateArgs {
            to: Some("1.5.0".into()),
            ..args("files")
        },
        false,
    )
    .unwrap();
    let rw = plan.arg.unwrap();
    host.registry.servers.borrow_mut()[0]
        .extra
        .insert("args".into(), json!(["-y", "@scope/files@1.4.9", "/data"]));
    let error = crate::servers::replace_arg(&host, "srv-files", rw.index, &rw.before, &rw.after)
        .unwrap_err();
    assert!(
        matches!(error, crate::servers::ServerError::Rejected(m) if m.contains("changed since the plan"))
    );
    assert_eq!(arguments(&host, "srv-files")[1], "@scope/files@1.4.9");
}

#[test]
fn test23_undo_puts_the_argument_back_and_forgets_the_record() {
    let world = World::new("undo");
    let host = world.host(vec![files()], vec![]);
    let go = UpdateArgs {
        to: Some("1.5.0".into()),
        ..args("files")
    };
    update::apply(&host, &go).unwrap();
    let preview = update::undo_plan(&host, "files").unwrap();
    assert!(preview.refusal.is_none());
    assert!(preview.human().contains("back from 1.5.0 to 1.4.2"));
    let done = update::undo(&host, &args("files")).unwrap();
    assert!(done.changed);
    assert_eq!((done.from.as_str(), done.to.as_str()), ("1.5.0", "1.4.2"));
    assert_eq!(
        arguments(&host, "srv-files"),
        ["-y", "@scope/files@1.4.2", "/data"]
    );
    assert!(Journal::read(&world.data).unwrap().applied.is_empty());
    assert_eq!(
        update::undo_plan(&host, "files").unwrap_err().kind.code(),
        "not_found"
    );
}

#[test]
fn test24_undo_stops_when_the_argument_was_edited_since_and_leaves_it_alone() {
    let world = World::new("undoguard");
    let host = world.host(vec![files()], vec![]);
    update::apply(
        &host,
        &UpdateArgs {
            to: Some("1.5.0".into()),
            ..args("files")
        },
    )
    .unwrap();
    host.registry.servers.borrow_mut()[0]
        .extra
        .insert("args".into(), json!(["-y", "@scope/files@2.0.0", "/data"]));
    let error = update::undo(&host, &args("files")).unwrap_err();
    assert_eq!(error.details.unwrap()["reason"], "moved");
    assert_eq!(arguments(&host, "srv-files")[1], "@scope/files@2.0.0");
    assert!(!Journal::read(&world.data).unwrap().applied.is_empty());
}

#[test]
fn test25_a_moved_image_tag_moves_the_tag_the_source_records_and_undo_restores_both() {
    let world = World::new("docker");
    let mut record = server(
        "srv-img",
        "img",
        "docker",
        &["run", "-i", "--rm", "acme/files:3.1.0"],
    );
    let mut extra = Map::new();
    extra.insert("image".into(), json!("acme/files"));
    extra.insert("tag".into(), json!("3.1.0"));
    extra.insert("note".into(), json!("kept"));
    record.fork.mcpm_source = Some(McpmSource {
        kind: Some("docker".into()),
        extra,
        ..Default::default()
    });
    let host = world.host(vec![record], vec![]);
    let go = UpdateArgs {
        to: Some("3.2.0".into()),
        ..args("img")
    };
    let plan = update::plan(&host, &go, false).unwrap();
    assert_eq!(
        plan.to_value()["changes"],
        json!([
            {"field": "args[3]", "from": "acme/files:3.1.0", "to": "acme/files:3.2.0"},
            {"field": "mcpmSource.tag", "from": "3.1.0", "to": "3.2.0"}
        ])
    );
    update::apply(&host, &go).unwrap();
    let source = |host: &MemoryHost| {
        host.registry.servers.borrow()[0]
            .fork
            .mcpm_source
            .clone()
            .unwrap()
    };
    assert_eq!(arguments(&host, "srv-img")[3], "acme/files:3.2.0");
    assert_eq!(source(&host).extra["tag"], "3.2.0");
    assert_eq!(source(&host).extra["note"], "kept");
    assert_eq!(
        pinned(&host.registry.servers.borrow()[0]).unwrap().version,
        "3.2.0"
    );
    update::undo(&host, &args("img")).unwrap();
    assert_eq!(arguments(&host, "srv-img")[3], "acme/files:3.1.0");
    assert_eq!(source(&host).extra["tag"], "3.1.0");
}

struct Gateway {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Gateway {
    fn start(base: &Path, gateway: Arc<MemoryGateway>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (base, flag) = (base.to_path_buf(), Arc::clone(&stop));
        let thread = std::thread::spawn(move || {
            let claims = restart::Claims::default();
            while !flag.load(Ordering::SeqCst) {
                let router: Arc<dyn GatewayRouter> = gateway.clone();
                for work in restart::serve_with(&base, router, 4242, &claims) {
                    work.join().unwrap();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn test30_the_update_restarts_the_server_through_the_gateway_and_waits_for_a_running_call() {
    let world = World::new("restart");
    let host = world.host(vec![files()], vec![]);
    let gateway = Arc::new(MemoryGateway::holding(&[("srv-files", 1)]));
    let _running = Gateway::start(&world.data, Arc::clone(&gateway));
    let go = |to: &str, restart: RestartChoice| UpdateArgs {
        to: Some(to.into()),
        restart,
        ..args("files")
    };
    let plan = update::plan(&host, &go("1.5.0", RestartChoice::IfIdle), true).unwrap();
    assert_eq!(plan.in_flight, Some(1));
    assert!(plan.human().contains("1 call(s) are running"));

    let busy = update::apply(&host, &go("1.5.0", RestartChoice::IfIdle)).unwrap();
    assert!(busy.changed);
    assert_eq!(busy.restart.as_ref().unwrap().0, "busy");
    assert!(
        gateway.restarted().is_empty(),
        "a running call is never cut by default"
    );

    let forced = update::apply(&host, &go("1.6.0", RestartChoice::Now)).unwrap();
    assert_eq!(forced.restart.unwrap().0, "restarted");
    assert_eq!(gateway.restarted(), ["srv-files"]);

    gateway.held.lock().unwrap().insert("srv-files".into(), 0);
    let idle = update::undo(
        &host,
        &UpdateArgs {
            restart: RestartChoice::IfIdle,
            ..args("files")
        },
    )
    .unwrap();
    assert_eq!(idle.restart.unwrap().0, "restarted");
    assert_eq!(gateway.restarted().len(), 2);
}

#[test]
fn test31_with_no_gateway_running_the_update_still_lands_and_says_so() {
    let world = World::new("nogateway");
    let host = world.host(vec![files()], vec![]);
    let done = update::apply(
        &host,
        &UpdateArgs {
            to: Some("1.5.0".into()),
            restart: RestartChoice::IfIdle,
            ..args("files")
        },
    )
    .unwrap();
    assert_eq!(done.restart.as_ref().unwrap().0, "not_running");
    assert!(!done.failed());
    assert_eq!(arguments(&host, "srv-files")[1], "@scope/files@1.5.0");
}

#[test]
fn test32_the_plan_and_the_result_carry_no_other_argument_of_the_server() {
    let world = World::new("noleak");
    let host = world.host(
        vec![server(
            "srv-secret",
            "secret",
            "npx",
            &[
                "-y",
                "@scope/files@1.4.2",
                "--api-key",
                "hunter2-hunter2-hunter2",
            ],
        )],
        vec![],
    );
    let go = UpdateArgs {
        to: Some("1.5.0".into()),
        ..args("secret")
    };
    let plan = update::plan(&host, &go, false).unwrap();
    let done = update::apply(&host, &go).unwrap();
    for text in [
        plan.human(),
        plan.to_value().to_string(),
        done.human("Updated"),
        done.to_value().to_string(),
        std::fs::read_to_string(world.data.join("plus/server-updates.json")).unwrap(),
    ] {
        assert!(!text.contains("hunter2"), "{text}");
    }
}

const DIGEST_OLD: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_NEW: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn image_server() -> ServerRecord {
    server(
        "srv-img",
        "img",
        "docker",
        &[
            "run",
            "-i",
            "--rm",
            &format!("acme/files:latest@{DIGEST_OLD}"),
        ],
    )
}

fn docker_digest_target() -> Target {
    Target {
        kind: Kind::Docker,
        name: "acme/files".into(),
        flavor: "@latest".into(),
    }
}

fn tag_json(digest: &str) -> Result<HttpResponse, HttpError> {
    answer(200, &format!(r#"{{"name":"latest","digest":"{digest}"}}"#))
}

#[test]
fn test40_an_image_pinned_by_digest_follows_the_tag_it_names() {
    let pin = pinned(&image_server()).unwrap();
    assert_eq!(pin.target, docker_digest_target());
    assert_eq!(pin.version, DIGEST_OLD);
    assert_eq!(pin.target.followed_tag(), Some("latest"));
    assert_eq!(pin.manual, None);
    assert_eq!(pin.target.source(), "package:docker:acme/files:@latest");
    for image in [
        format!("acme/files@{DIGEST_OLD}"),
        "acme/files:latest@sha256:abcd".to_string(),
        format!("ghcr.io/acme/files:latest@{DIGEST_OLD}"),
    ] {
        let record = server("s", "s", "docker", &["run", "--rm", &image]);
        assert_eq!(pinned(&record), None, "{image}");
    }
    assert_eq!(
        rewrite(&image_server(), DIGEST_NEW).map(|rw| (rw.index, rw.after)),
        Ok((3, format!("acme/files:latest@{DIGEST_NEW}")))
    );
    assert!(rewrite(&image_server(), "sha256:short").is_err());
}

#[test]
fn test41_the_digest_a_tag_points_at_comes_from_the_hub_and_a_gone_tag_is_no_answer() {
    let http = ScriptedHttp::answering(vec![
        tag_json(DIGEST_NEW),
        answer(404, "{}"),
        answer(200, r#"{"name":"latest","digest":"junk"}"#),
    ]);
    assert_eq!(
        lookup::latest(&http, &docker_digest_target())
            .unwrap()
            .as_deref(),
        Some(DIGEST_NEW)
    );
    assert_eq!(lookup::latest(&http, &docker_digest_target()), Ok(None));
    assert_eq!(lookup::latest(&http, &docker_digest_target()), Ok(None));
    assert_eq!(
        http.requests.borrow()[0].url,
        "https://hub.docker.com/v2/repositories/acme/files/tags/latest"
    );
}

#[test]
fn test42_a_digest_pin_is_planned_applied_and_put_back_like_a_version() {
    let world = World::new("digest");
    let host = world.host(vec![image_server()], vec![tag_json(DIGEST_NEW)]);
    let go = UpdateArgs {
        fetch: true,
        ..args("img")
    };
    let plan = update::plan(&host, &go, false).unwrap();
    assert!(plan.digest && plan.refusal.is_none() && !plan.downgrade);
    assert_eq!(
        plan.to_value()["changes"],
        json!([{"field": "args[3]", "from": format!("acme/files:latest@{DIGEST_OLD}"),
                "to": format!("acme/files:latest@{DIGEST_NEW}")}])
    );
    let human = plan.human();
    assert!(
        human.contains("Update sha256:aaaaaaaaaaaa... to sha256:bbbbbbbbbbbb..."),
        "{human}"
    );
    assert!(
        !human.lines().take(2).any(|line| line.contains(DIGEST_NEW)),
        "digests are shortened in the summary; the registry change keeps them whole"
    );
    let done = update::apply(
        &host,
        &UpdateArgs {
            to: Some(DIGEST_NEW.into()),
            plan_hash: Some(plan.hash.clone()),
            ..args("img")
        },
    )
    .unwrap();
    assert!(done.changed);
    assert_eq!(
        arguments(&host, "srv-img")[3],
        format!("acme/files:latest@{DIGEST_NEW}")
    );
    assert_eq!(
        pinned(&host.registry.servers.borrow()[0]).unwrap().version,
        DIGEST_NEW
    );
    update::undo(&host, &args("img")).unwrap();
    assert_eq!(
        arguments(&host, "srv-img")[3],
        format!("acme/files:latest@{DIGEST_OLD}")
    );
}

#[test]
fn test43_the_same_digest_is_up_to_date_and_only_a_digest_is_taken_as_a_version() {
    let world = World::new("digest-same");
    let host = world.host(vec![image_server()], vec![tag_json(DIGEST_OLD)]);
    let plan = update::plan(
        &host,
        &UpdateArgs {
            fetch: true,
            ..args("img")
        },
        false,
    )
    .unwrap();
    assert_eq!(plan.refusal.unwrap().code, "up_to_date");
    let error = update::plan(
        &host,
        &UpdateArgs {
            to: Some("1.2.3".into()),
            ..args("img")
        },
        false,
    )
    .unwrap_err();
    assert!(error.message.contains("is not an image digest"));
}

#[test]
fn test44_a_new_digest_is_one_skippable_row_and_the_same_digest_is_none() {
    let world = World::new("digest-row");
    let host = world.host(vec![image_server()], vec![]);
    let rows = |host: &MemoryHost| {
        crate::attention::snapshot(host)
            .items
            .into_iter()
            .filter(|item| item.id.starts_with("updates:package:"))
            .collect::<Vec<_>>()
    };
    record_lookup(
        &world.data,
        1_000,
        &docker_digest_target(),
        &Ok(Some(DIGEST_OLD.into())),
    )
    .unwrap();
    assert!(rows(&host).is_empty());
    record_lookup(
        &world.data,
        2_000,
        &docker_digest_target(),
        &Ok(Some(DIGEST_NEW.into())),
    )
    .unwrap();
    let found = rows(&host);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].id,
        "updates:package:img:sha256:aaaaaaaaaaaa...>sha256:bbbbbbbbbbbb..."
    );
    assert_eq!(
        found[0].title,
        "img: acme/files:latest has a new image, pinned to sha256:aaaaaaaaaaaa..."
    );
    assert_eq!(found[0].action.as_ref().unwrap().label, "Update");
}

fn two_servers() -> Vec<ServerRecord> {
    vec![
        files(),
        server("srv-tools", "tools", "uvx", &["tools-mcp==0.9.1"]),
    ]
}

fn pypi_target() -> Target {
    Target {
        kind: Kind::Pypi,
        name: "tools-mcp".into(),
        flavor: String::new(),
    }
}

#[test]
fn test50_several_servers_share_one_plan_whose_hash_covers_each_of_them() {
    let world = World::new("batch");
    let host = world.host(two_servers(), vec![]);
    world.checked(&npm_target(), "1.5.0");
    world.checked(&pypi_target(), "1.0.0");
    let batch = update::plan_batch(&host, &[], &args("")).unwrap();
    assert_eq!(
        batch
            .plans
            .iter()
            .map(|p| p.server_name.as_str())
            .collect::<Vec<_>>(),
        ["files", "tools"]
    );
    assert!(batch.plans.iter().all(|p| p.refusal.is_none()));
    let value = batch.to_value();
    assert_eq!(value["count"], 2);
    assert!(value["apply"]
        .as_str()
        .unwrap()
        .contains("files tools --plan-hash"));
    world.checked(&pypi_target(), "1.1.0");
    assert_ne!(
        update::plan_batch(&host, &[], &args("")).unwrap().hash,
        batch.hash
    );
}

#[test]
fn test51_all_leaves_out_servers_with_nothing_to_update_and_a_list_keeps_them_with_the_reason() {
    let world = World::new("batch-all");
    let host = world.host(two_servers(), vec![]);
    world.checked(&npm_target(), "1.5.0");
    let all = update::plan_batch(&host, &[], &args("")).unwrap();
    assert_eq!(all.plans.len(), 1, "tools was never checked");
    let named = update::plan_batch(
        &host,
        &["files".into(), "tools".into(), "files".into()],
        &args(""),
    )
    .unwrap();
    assert_eq!(named.plans.len(), 2, "a server named twice counts once");
    assert_eq!(named.plans[1].refusal.as_ref().unwrap().code, "not_checked");
    assert_eq!(
        update::plan_batch(&host, &["ghost".into()], &args(""))
            .unwrap_err()
            .kind
            .code(),
        "not_found"
    );
}

#[test]
fn test52_one_confirm_applies_every_server_and_one_failure_does_not_stop_the_others() {
    let world = World::new("batch-apply");
    let mut recipe = server(
        "srv-recipe",
        "recipe",
        "npx",
        &["-y", "@scope/recipe@2.0.0"],
    );
    recipe
        .extra
        .insert("launch".into(), json!({"inputs": [], "bindings": []}));
    let mut servers = two_servers();
    servers.push(recipe);
    let host = world.host(servers, vec![]);
    world.checked(&npm_target(), "1.5.0");
    world.checked(&pypi_target(), "1.0.0");
    world.checked(
        &Target {
            name: "@scope/recipe".into(),
            ..npm_target()
        },
        "2.1.0",
    );
    let batch = update::plan_batch(&host, &[], &args("")).unwrap();
    let stale = update::apply_batch(
        &host,
        &[],
        &UpdateArgs {
            plan_hash: Some("sha256:0000000000000000".into()),
            ..args("")
        },
    )
    .unwrap_err();
    assert_eq!(stale.kind.code(), "conflict");
    assert_eq!(arguments(&host, "srv-files")[1], "@scope/files@1.4.2");

    let done = update::apply_batch(
        &host,
        &[],
        &UpdateArgs {
            plan_hash: Some(batch.hash.clone()),
            ..args("")
        },
    )
    .unwrap();
    let outcome = |name: &str| done.items.iter().find(|i| i.server == name).unwrap();
    assert!(outcome("files").outcome.as_ref().unwrap().changed);
    assert!(outcome("tools").outcome.as_ref().unwrap().changed);
    assert_eq!(
        outcome("recipe").outcome.as_ref().unwrap_err().kind.code(),
        "update_refused"
    );
    assert!(done.failed());
    assert_eq!(arguments(&host, "srv-files")[1], "@scope/files@1.5.0");
    assert_eq!(arguments(&host, "srv-tools")[0], "tools-mcp==1.0.0");
    let value = done.to_value();
    assert_eq!(
        (value["applied"].clone(), value["failed"].clone()),
        (json!(2), json!(1))
    );
    assert_eq!(Journal::read(&world.data).unwrap().applied.len(), 2);
}

#[test]
fn test53_one_undo_puts_back_every_update_on_record_and_names_those_that_cannot_go() {
    let world = World::new("batch-undo");
    let host = world.host(two_servers(), vec![]);
    world.checked(&npm_target(), "1.5.0");
    world.checked(&pypi_target(), "1.0.0");
    update::apply_batch(&host, &[], &args("")).unwrap();
    host.registry.servers.borrow_mut()[1]
        .extra
        .insert("args".into(), json!(["tools-mcp==2.0.0"]));
    let previews = update::undo_plan_batch(&host, &[]).unwrap();
    assert_eq!(previews.len(), 2);
    assert!(previews.iter().any(|p| p.refusal.is_some()));
    let done = update::undo_batch(&host, &[], &args("")).unwrap();
    assert_eq!(done.items.len(), 2);
    assert_eq!(arguments(&host, "srv-files")[1], "@scope/files@1.4.2");
    assert_eq!(arguments(&host, "srv-tools")[0], "tools-mcp==2.0.0");
    assert!(done.failed());
    assert_eq!(Journal::read(&world.data).unwrap().applied.len(), 1);
}

#[test]
fn test54_a_registry_that_cannot_be_reached_refuses_its_server_and_not_the_batch() {
    let world = World::new("batch-offline");
    let host = world.host(
        two_servers(),
        vec![
            answer(200, r#"{"version":"1.5.0"}"#),
            Err(HttpError::Connect("no route".into())),
        ],
    );
    let batch = update::plan_batch(
        &host,
        &[],
        &UpdateArgs {
            fetch: true,
            ..args("")
        },
    )
    .unwrap();
    assert_eq!(batch.plans.len(), 2);
    assert!(batch.plans[0].refusal.is_none());
    let refusal = batch.plans[1].refusal.as_ref().unwrap();
    assert_eq!(refusal.code, "lookup_failed");
    assert!(refusal.message.contains("no route"));
}

#[test]
fn test60_the_source_row_says_what_each_local_server_runs_and_what_is_known_about_it() {
    use super::source::{rows, State};
    let world = World::new("source");
    let mut remote = server("srv-remote", "remote", "npx", &["x"]);
    remote.extra.insert("transport".into(), json!("http"));
    let mut servers = two_servers();
    servers.push(server("srv-free", "free", "npx", &["-y", "free-files"]));
    servers.push(server("srv-script", "script", "node", &["server.js"]));
    servers.push(image_server());
    servers.push(remote);
    let host = world.host(servers, vec![]);
    world.checked(&npm_target(), "1.5.0");
    record_lookup(&world.data, 1_000, &pypi_target(), &Err("no route".into())).unwrap();
    record_lookup(
        &world.data,
        1_000,
        &docker_digest_target(),
        &Ok(Some(DIGEST_OLD.into())),
    )
    .unwrap();
    let found = rows(&host, None).unwrap();
    let state = |name: &str| found.iter().find(|r| r.server == name).unwrap();
    assert_eq!(state("files").state, State::UpdateAvailable);
    assert_eq!(state("files").latest.as_deref(), Some("1.5.0"));
    assert_eq!(state("files").checked_at, Some(1_000));
    assert_eq!(state("tools").state, State::Error);
    assert_eq!(state("tools").error.as_deref(), Some("no route"));
    assert_eq!(state("free").state, State::Automatic);
    assert_eq!(state("script").state, State::Unknown);
    assert_eq!(state("img").state, State::UpToDate);
    assert_eq!(state("img").tracks.as_deref(), Some("latest"));
    assert!(
        found.iter().all(|r| r.server != "remote"),
        "a remote server has no code to update"
    );
    assert!(
        host.http.requests.borrow().is_empty(),
        "the row asks nobody"
    );
    assert_eq!(rows(&host, Some("files")).unwrap().len(), 1);
    assert_eq!(
        rows(&host, Some("remote")).unwrap_err().kind.code(),
        "not_found"
    );
    assert!(state("files").human().contains("1.5.0 is out"));
}

#[test]
fn test61_a_pin_never_looked_up_is_unchecked_and_one_toolport_cannot_rewrite_says_so() {
    use super::source::{rows, State};
    let world = World::new("source-manual");
    let mut recipe = files();
    recipe
        .extra
        .insert("launch".into(), json!({"inputs": [], "bindings": []}));
    let host = world.host(vec![recipe], vec![]);
    let row = rows(&host, Some("files")).unwrap().remove(0);
    assert_eq!(row.state, State::Unchecked);
    assert!(row.manual.unwrap().contains("launch recipe"));
}
