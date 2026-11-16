use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use toolport_plus_api::{
    GatewayRouter, MemoryGateway, MemoryHost, MemoryRegistry, MemorySecrets, ServerRecord,
};

use serde_json::json;

use super::*;

const FAST: Timing = Timing {
    poll: Duration::from_millis(15),
    settle: Duration::from_millis(120),
    ack_wait: Duration::from_millis(300),
};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("toolport-restart-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn host(dir: &Path) -> MemoryHost {
    let server = |id: &str, name: &str| ServerRecord {
        id: id.into(),
        name: name.into(),
        enabled: true,
        ..Default::default()
    };
    MemoryHost {
        registry: MemoryRegistry::with_servers(vec![
            server("srv-alpha", "alpha"),
            server("srv-beta", "beta"),
        ]),
        secrets: MemorySecrets {
            data_dir: Some(dir.to_path_buf()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn args(key: &str) -> RestartArgs {
    RestartArgs {
        wait: Duration::from_secs(5),
        timing: FAST,
        ..RestartArgs::new(key)
    }
}

/// A gateway process: ticks `serve` every few milliseconds until stopped.
struct Running {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Running {
    fn start(base: &Path, gateway: Arc<MemoryGateway>, pid: u32) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (base, flag) = (base.to_path_buf(), Arc::clone(&stop));
        let thread = std::thread::spawn(move || {
            let claims = Claims::default();
            while !flag.load(Ordering::SeqCst) {
                let router: Arc<dyn GatewayRouter> = gateway.clone();
                for work in serve_with(&base, router, pid, &claims) {
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

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn test01_a_restart_reaches_the_gateway_that_holds_the_server_once() {
    let base = scratch("once");
    let gateway = Arc::new(MemoryGateway::holding(&[("srv-alpha", 0)]));
    let _running = Running::start(&base, Arc::clone(&gateway), 4001);
    let done = restart(&host(&base), &args("alpha")).unwrap();
    assert_eq!(done.status, Status::Restarted);
    assert_eq!(done.outcomes.len(), 1);
    assert_eq!(done.outcomes[0].pid, 4001);
    assert_eq!(
        gateway.restarted(),
        ["srv-alpha"],
        "one request restarts it once"
    );
    assert_eq!(
        gateway.stale_marks(),
        1,
        "tools, resources and prompts are read again"
    );
    assert!(done
        .line()
        .starts_with("Restarted alpha (srv-alpha) in 1 gateway process;"));
    assert!(!dir(&base).join("anything").exists());
    assert_eq!(
        std::fs::read_dir(dir(&base)).unwrap().count(),
        0,
        "the request is cleaned up"
    );
}

#[test]
fn test02_every_gateway_answers_for_itself_and_one_failure_is_named() {
    let base = scratch("two");
    let good = Arc::new(MemoryGateway::holding(&[("srv-alpha", 0)]));
    let bad = Arc::new(MemoryGateway::holding(&[("srv-alpha", 0)]));
    bad.failing.lock().unwrap().insert(
        "srv-alpha".into(),
        "could not bring a fresh process up".into(),
    );
    let elsewhere = Arc::new(MemoryGateway::holding(&[("srv-beta", 0)]));
    let _a = Running::start(&base, Arc::clone(&good), 11);
    let _b = Running::start(&base, Arc::clone(&bad), 22);
    let _c = Running::start(&base, Arc::clone(&elsewhere), 33);
    let done = restart(&host(&base), &args("srv-alpha")).unwrap();
    assert_eq!(done.status, Status::Failed);
    let pids: Vec<u32> = done.outcomes.iter().map(|o| o.pid).collect();
    assert_eq!(
        pids,
        [11, 22],
        "a gateway that does not hold the server stays out of it"
    );
    assert!(done.outcomes[0].restarted && !done.outcomes[1].restarted);
    assert!(done
        .line()
        .contains("pid 22: could not bring a fresh process up"));
    assert!(elsewhere.restarted().is_empty() && good.restarted() == ["srv-alpha"]);
    assert!(done.failed());
}

#[test]
fn test03_with_no_gateway_running_the_answer_is_not_running_and_the_request_is_withdrawn() {
    let base = scratch("none");
    let done = restart(&host(&base), &args("alpha")).unwrap();
    assert_eq!(done.status, Status::NotRunning);
    assert!(done.line().contains("starts fresh at its next call"));
    assert!(pending(&base, now_ms()).is_empty(), "it cannot run late");
}

#[test]
fn test04_a_preview_and_an_unknown_server_send_nothing() {
    let base = scratch("preview");
    let preview = restart(
        &host(&base),
        &RestartArgs {
            dry_run: true,
            ..args("alpha")
        },
    )
    .unwrap();
    assert_eq!(preview.status, Status::DryRun);
    assert!(!dir(&base).exists());
    let error = restart(&host(&base), &args("ghost")).err().unwrap();
    assert_eq!(error.message, "no server 'ghost'");
    assert!(!dir(&base).exists());
}

#[test]
fn test05_if_idle_stops_at_a_call_in_flight_and_goes_ahead_when_idle() {
    let base = scratch("idle");
    let gateway = Arc::new(MemoryGateway::holding(&[("srv-alpha", 2)]));
    let _running = Running::start(&base, Arc::clone(&gateway), 4005);
    let idle_only = RestartArgs {
        if_idle: true,
        ..args("alpha")
    };
    let busy = restart(&host(&base), &idle_only).unwrap();
    assert_eq!(busy.status, Status::Busy);
    assert_eq!(busy.outcomes[0].in_flight, Some(2));
    assert_eq!(busy.to_value()["gateways"][0]["inFlight"], 2);
    assert!(busy.line().contains("2 calls in flight"));
    assert!(gateway.restarted().is_empty(), "nothing was restarted");
    assert!(pending(&base, now_ms()).is_empty());

    gateway.held.lock().unwrap().insert("srv-alpha".into(), 0);
    let done = restart(&host(&base), &idle_only).unwrap();
    assert_eq!(done.status, Status::Restarted);
    assert_eq!(gateway.restarted(), ["srv-alpha"]);
}

#[test]
fn test06_a_request_for_a_server_the_gateway_does_not_hold_waits_for_one_that_does() {
    let base = scratch("elsewhere");
    let sent = request(&base, Kind::Restart, "srv-alpha", "alpha").unwrap();
    let nothing = Arc::new(MemoryGateway::default());
    let claims = Claims::default();
    assert!(serve_with(&base, nothing.clone(), 1, &claims).is_empty());
    assert_eq!(pending(&base, now_ms()), vec![sent.clone()]);
    nothing.held.lock().unwrap().insert("srv-alpha".into(), 0);
    for work in serve_with(&base, nothing.clone(), 1, &claims) {
        work.join().unwrap();
    }
    for work in serve_with(&base, nothing.clone(), 1, &claims) {
        work.join().unwrap();
    }
    assert_eq!(
        nothing.restarted(),
        ["srv-alpha"],
        "claimed once per process"
    );
    assert_eq!(acknowledged(&base, &sent.nonce), [1]);
    assert_eq!(outcomes(&base, &sent.nonce).len(), 1);
}

#[test]
fn test07_a_stale_request_is_dropped_instead_of_run_late() {
    let base = scratch("stale");
    let sent = request(&base, Kind::Restart, "srv-alpha", "alpha").unwrap();
    assert!(pending(&base, sent.at_ms + TTL_MS + 1).is_empty());
    assert!(read_all::<Request>(&base, REQUEST).is_empty());
}

#[test]
fn test08_an_answer_still_missing_at_the_deadline_is_reported_not_hidden() {
    let base = scratch("slow");
    let sent = request(&base, Kind::Restart, "srv-alpha", "alpha").unwrap();
    acknowledge(&base, &sent.nonce, 77).unwrap();
    let found = ask(
        &base,
        &sent,
        Duration::from_millis(200),
        FAST,
        "still restarting",
    );
    assert_eq!(found.len(), 1);
    assert!(!found[0].restarted && found[0].detail.starts_with("still restarting after"));
}

#[test]
fn test09_a_server_that_runs_per_folder_is_restarted_in_every_folder() {
    let base = scratch("folders");
    let gateway = Arc::new(MemoryGateway::holding_rooted(
        "srv-alpha",
        &[("/work/b", 0), ("/work/a", 0)],
    ));
    let _running = Running::start(&base, Arc::clone(&gateway), 4009);
    let done = restart(&host(&base), &args("alpha")).unwrap();
    assert_eq!(done.status, Status::Restarted);
    assert_eq!(
        gateway.restarted(),
        ["srv-alpha in /work/a", "srv-alpha in /work/b"]
    );
    assert_eq!(gateway.stale_marks(), 1);
    assert_eq!(done.outcomes[0].detail, "restarted in 2 project folders");
    assert_eq!(
        done.to_value()["gateways"][0]["roots"],
        json!([
            {"root": "/work/a", "restarted": true, "detail": "restarted"},
            {"root": "/work/b", "restarted": true, "detail": "restarted"},
        ])
    );
    assert!(done
        .line()
        .starts_with("Restarted alpha (srv-alpha) in 1 gateway process (2 project folders);"));
}

#[test]
fn test10_one_folder_that_fails_fails_the_gateway_and_is_named_with_its_reason() {
    let base = scratch("folder-failure");
    let gateway = Arc::new(MemoryGateway::holding_rooted(
        "srv-alpha",
        &[("/work/a", 0), ("/work/b", 0)],
    ));
    gateway.failing_roots.lock().unwrap().insert(
        "srv-alpha".into(),
        [(
            "/work/b".to_string(),
            "could not bring a fresh process up".to_string(),
        )]
        .into(),
    );
    let _running = Running::start(&base, Arc::clone(&gateway), 4010);
    let done = restart(&host(&base), &args("alpha")).unwrap();
    assert_eq!(done.status, Status::Failed);
    assert_eq!(gateway.restarted(), ["srv-alpha in /work/a"]);
    assert_eq!(
        gateway.stale_marks(),
        1,
        "the folder that restarted reads its lists again"
    );
    let roots = &done.outcomes[0].roots;
    assert!(roots[0].restarted && !roots[1].restarted);
    assert!(done
        .line()
        .contains("pid 4010: /work/b: could not bring a fresh process up"));
}

#[test]
fn test11_if_idle_counts_every_folder_and_names_the_busy_ones() {
    let base = scratch("folder-idle");
    let gateway = Arc::new(MemoryGateway::holding_rooted(
        "srv-alpha",
        &[("/work/a", 0), ("/work/b", 2), ("/work/c", 1)],
    ));
    let _running = Running::start(&base, Arc::clone(&gateway), 4011);
    let idle_only = RestartArgs {
        if_idle: true,
        ..args("alpha")
    };
    let busy = restart(&host(&base), &idle_only).unwrap();
    assert_eq!(busy.status, Status::Busy);
    assert_eq!(busy.outcomes[0].in_flight, Some(3));
    assert!(gateway.restarted().is_empty(), "no folder was restarted");
    assert!(busy
        .line()
        .contains("pid 4011: 3 calls in flight: /work/b (2), /work/c (1)."));
    let value = busy.to_value();
    assert_eq!(value["gateways"][0]["roots"][0]["detail"], "idle");
    assert_eq!(value["gateways"][0]["roots"][1]["inFlight"], 2);

    gateway
        .rooted
        .lock()
        .unwrap()
        .insert("srv-alpha".into(), [("/work/a".to_string(), 0)].into());
    let done = restart(&host(&base), &idle_only).unwrap();
    assert_eq!(done.status, Status::Restarted);
    assert_eq!(gateway.restarted(), ["srv-alpha in /work/a"]);
}

#[test]
fn test12_a_server_with_one_shared_connection_answers_without_folders() {
    let base = scratch("no-folders");
    let gateway = Arc::new(MemoryGateway::holding(&[("srv-alpha", 0)]));
    let _running = Running::start(&base, Arc::clone(&gateway), 4012);
    let done = restart(&host(&base), &args("alpha")).unwrap();
    assert!(done.outcomes[0].roots.is_empty());
    assert!(done.to_value()["gateways"][0].get("roots").is_none());
}
