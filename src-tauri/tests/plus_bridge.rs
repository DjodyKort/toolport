//! Spike proof for the `plus_ctl` bridge (D-060): the real `toolportctl` runs as a child process
//! against a synthetic data directory, and a script fixture stands in for a long, chatty command.

#![cfg(unix)]

use conduit_lib::plus::bridge::{
    Bridge, BridgeError, Emitter, EventKind, JobEvent, JobResult, Secret,
};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[path = "common/exec.rs"]
mod exec;
#[path = "common/ctl_world.rs"]
mod ctl_world;

use ctl_world::CtlWorld;

const DEADLINE: Duration = Duration::from_secs(30);
const CANARY: &str = "CANARY-bridge-stdin-9c41e7";

#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<JobEvent>>>);

impl Events {
    fn emitter(&self) -> Arc<dyn Emitter> {
        let sink = self.0.clone();
        Arc::new(move |event: &JobEvent| sink.lock().unwrap().push(event.clone()))
    }

    fn all(&self) -> Vec<JobEvent> {
        self.0.lock().unwrap().clone()
    }

    fn lines(&self) -> Vec<String> {
        self.all().into_iter().filter_map(|e| e.line).collect()
    }

    fn wait_for_line(&self, needle: &str) -> String {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(line) = self.lines().into_iter().find(|l| l.contains(needle)) {
                return line;
            }
            assert!(
                Instant::now() < deadline,
                "no stderr line containing {needle:?}; saw {:?}",
                self.lines()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn real_bridge(world: &CtlWorld) -> Bridge {
    world
        .env()
        .into_iter()
        .fold(
            Bridge::with_binary(env!("CARGO_BIN_EXE_toolportctl")).clear_env(),
            |bridge, (key, value)| bridge.env(key, value),
        )
}

fn script_bridge(dir: &Path, name: &str, body: &str) -> Bridge {
    let script = dir.join(name);
    exec::write_executable(&script, &format!("#!/bin/sh\n{body}"));
    Bridge::with_binary(script)
        .clear_env()
        .env("PATH", "/usr/bin:/bin")
}

fn world(tag: &str) -> CtlWorld {
    CtlWorld::new(tag, env!("CARGO_BIN_EXE_mock-mcp-server"))
}

fn run(bridge: &Bridge, args: &[&str]) -> (JobResult, Events) {
    let events = Events::default();
    let job = bridge
        .start(argv(args), None, events.emitter())
        .expect("start");
    (bridge.wait(&job).expect("wait"), events)
}

#[derive(Debug, PartialEq)]
enum Proc {
    Gone,
    Zombie,
    Alive,
}

fn proc_state(pid: u32) -> Proc {
    if unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
        return Proc::Gone;
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    match stat.rsplit(") ").next() {
        Some(rest) if rest.starts_with('Z') => Proc::Zombie,
        _ => Proc::Alive,
    }
}

fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn cmdline(pid: u32) -> String {
    match std::fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(raw) => String::from_utf8_lossy(&raw).replace('\0', " "),
        Err(_) => std::process::Command::new("ps")
            .args(["-ww", "-o", "command=", "-p", &pid.to_string()])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .unwrap_or_default(),
    }
}

#[test]
fn a_read_command_returns_the_envelope() {
    let world = world("read");
    let bridge = real_bridge(&world);
    let (result, events) = run(&bridge, &["status"]);
    assert_eq!(result.exit_code, Some(0), "{result:?}");
    assert!(!result.cancelled);
    assert_eq!(result.parse_error, None);
    let envelope = result.envelope.expect("an envelope");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["command"], "status");
    assert_eq!(envelope["schemaVersion"], 1);
    assert_eq!(envelope["data"]["serverCount"], 2);
    assert_eq!(envelope["data"]["activeProfile"], "default");
    assert_eq!(
        std::fs::canonicalize(envelope["data"]["dataDir"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(&world.data).unwrap()
    );
    let kinds: Vec<EventKind> = events.all().into_iter().map(|e| e.kind).collect();
    assert_eq!(kinds, vec![EventKind::Exit], "a quiet command only reports its exit");
}

#[test]
fn a_write_previews_with_dry_run_then_applies() {
    let world = world("write");
    let bridge = real_bridge(&world);
    let before = world.registry();
    let edit = ["profile", "edit", "default", "--name", "Main", "--add-server", "beta"];

    let mut preview = edit.to_vec();
    preview.push("--dry-run");
    let (result, _) = run(&bridge, &preview);
    let envelope = result.envelope.expect("an envelope");
    assert_eq!(envelope["ok"], true, "{envelope}");
    assert_eq!(envelope["data"]["dryRun"], true);
    assert_eq!(envelope["data"]["servers"]["added"], json!(["beta"]));
    assert_eq!(world.registry(), before, "the dry run wrote nothing");

    let (result, _) = run(&bridge, &edit);
    let envelope = result.envelope.expect("an envelope");
    assert_eq!(envelope["ok"], true, "{envelope}");
    assert_ne!(envelope["data"]["dryRun"], true);
    let after = world.registry();
    assert_eq!(after["profiles"][0]["name"], "Main");
    assert_eq!(
        after["profiles"][0]["enabledServerIds"],
        json!(["srv-alpha", "srv-beta"])
    );
}

#[test]
fn a_failing_command_still_yields_an_envelope_and_a_nonzero_exit() {
    let world = world("fail");
    let bridge = real_bridge(&world);
    let (result, _) = run(&bridge, &["server", "info", "no-such-server"]);
    assert_eq!(result.exit_code, Some(1));
    let envelope = result.envelope.expect("an envelope");
    assert_eq!(envelope["ok"], false);
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("no-such-server"));
    let (result, _) = run(&bridge, &["server", "info"]);
    assert_eq!(result.exit_code, Some(2));
    assert_eq!(result.envelope.unwrap()["error"]["code"], "usage");
}

#[test]
fn home_and_data_dir_are_refused_and_nothing_is_spawned() {
    let world = world("refuse");
    let bridge = real_bridge(&world);
    let events = Events::default();
    for bad in [
        argv(&["status", "--data-dir", "/tmp/elsewhere"]),
        argv(&["skills", "ls", "--home=/tmp/elsewhere"]),
    ] {
        let error = bridge.start(bad, None, events.emitter()).unwrap_err();
        assert!(matches!(error, BridgeError::Forbidden(_)), "{error}");
    }
    assert!(events.all().is_empty());
}

#[test]
fn a_long_command_streams_stderr_in_order_and_cancel_reaps_the_group() {
    let dir = std::env::temp_dir().join(format!("bridge-long-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bridge = script_bridge(
        &dir,
        "long.sh",
        "echo 'step 1' >&2\n\
         echo 'step 2' >&2\n\
         sleep 120 &\n\
         echo \"helper $!\" >&2\n\
         echo 'ready' >&2\n\
         wait\n",
    );
    let events = Events::default();
    let job = bridge.start(argv(&["auth", "login"]), None, events.emitter()).unwrap();
    let pid = bridge.pid(&job).unwrap();

    let helper = events.wait_for_line("helper ");
    events.wait_for_line("ready");
    let helper: u32 = helper.trim_start_matches("helper ").parse().unwrap();
    assert_eq!(
        events.lines()[..2],
        ["step 1", "step 2"],
        "lines arrive in the order the child wrote them"
    );
    let seqs: Vec<u64> = events.all().iter().map(|e| e.seq).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    assert!(
        events.all().iter().all(|e| e.kind == EventKind::Stderr),
        "the child is still running, so no exit event yet"
    );
    assert!(unsafe { libc::kill(pid as libc::pid_t, 0) } == 0, "child is alive");

    bridge.cancel(&job).unwrap();
    let result = bridge.wait(&job).unwrap();
    assert!(result.cancelled);
    assert_eq!(result.exit_code, None);
    assert_eq!(result.signal, Some(libc::SIGKILL));
    assert_eq!(result.envelope, None);
    assert_eq!(&result.stderr[..2], ["step 1", "step 2"]);
    assert_eq!(proc_state(pid), Proc::Gone, "the child was reaped, not left as a zombie");
    eventually("the helper in the child's group died with it", || {
        proc_state(helper) != Proc::Alive
    });
    let last = events.all().pop().unwrap();
    assert_eq!(last.kind, EventKind::Exit);
    assert_eq!(last.cancelled, Some(true));
    assert!(bridge.wait(&job).is_err(), "a result is handed over once");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancelling_a_finished_job_is_a_no_op() {
    let dir = std::env::temp_dir().join(format!("bridge-done-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bridge = script_bridge(
        &dir,
        "quick.sh",
        "printf '{\"ok\":true,\"command\":\"quick\",\"schemaVersion\":1,\"data\":{}}\\n'\n",
    );
    let events = Events::default();
    let job = bridge.start(argv(&["quick"]), None, events.emitter()).unwrap();
    let deadline = Instant::now() + DEADLINE;
    while !events.all().iter().any(|e| e.kind == EventKind::Exit) {
        assert!(Instant::now() < deadline, "the job never finished");
        std::thread::sleep(Duration::from_millis(10));
    }
    bridge.cancel(&job).unwrap();
    let result = bridge.wait(&job).unwrap();
    assert!(!result.cancelled);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.envelope.unwrap()["command"], "quick");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dropping_the_bridge_kills_running_jobs() {
    let dir = std::env::temp_dir().join(format!("bridge-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bridge = script_bridge(&dir, "forever.sh", "echo 'ready' >&2\nsleep 120\n");
    let events = Events::default();
    let job = bridge.start(argv(&["forever"]), None, events.emitter()).unwrap();
    let pid = bridge.pid(&job).unwrap();
    events.wait_for_line("ready");
    drop(bridge);
    assert_eq!(proc_state(pid), Proc::Gone);
    let _ = std::fs::remove_dir_all(&dir);
}

const ECHO_SECRET: &str = "IFS= read -r secret || true\n\
     echo \"got $secret\" >&2\n\
     printf '{\"ok\":true,\"command\":\"echo\",\"schemaVersion\":1,\"data\":{\"echo\":\"%s\"}}\\n' \"$secret\"\n\
     case \"$3\" in block) sleep 120 ;; esac\n";

#[test]
fn a_secret_travels_over_stdin_and_never_appears_in_argv_events_or_results() {
    let dir = std::env::temp_dir().join(format!("bridge-secret-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bridge = script_bridge(&dir, "echo.sh", ECHO_SECRET);
    let events = Events::default();
    let secret = Secret::new(format!("{CANARY}\n")).unwrap();
    assert!(!format!("{secret:?}").contains(CANARY));
    let job = bridge
        .start(argv(&["echo", "block"]), Some(secret), events.emitter())
        .unwrap();
    let pid = bridge.pid(&job).unwrap();

    let line = events.wait_for_line("got ");
    assert_eq!(line, "got [redacted]", "the child did receive it, the bridge masked it");
    let args = cmdline(pid);
    assert!(args.contains("--json echo block"), "{args}");
    assert!(!args.contains(CANARY), "argv: {args}");
    let environ = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    assert!(!String::from_utf8_lossy(&environ).contains(CANARY));

    bridge.cancel(&job).unwrap();
    let cancelled = bridge.wait(&job).unwrap();
    assert!(cancelled.cancelled);

    let finished = {
        let events = Events::default();
        let secret = Secret::new(CANARY).unwrap();
        let job = bridge
            .start(argv(&["echo", "exit"]), Some(secret), events.emitter())
            .unwrap();
        let result = bridge.wait(&job).unwrap();
        (result, events)
    };
    let (result, finished_events) = finished;
    let envelope = result.envelope.clone().expect("an envelope");
    assert_eq!(envelope["data"]["echo"], "[redacted]");
    for text in [
        format!("{result:?}"),
        format!("{:?}", finished_events.all()),
        format!("{:?}", events.all()),
        format!("{:?}", cancelled),
    ] {
        assert!(!text.contains(CANARY), "leaked into: {text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn secret_set_through_the_bridge_stores_the_value_without_echoing_it() {
    let world = world("vault");
    let bridge = real_bridge(&world);
    let events = Events::default();
    let job = bridge
        .start(
            argv(&["secret", "set", "srv-alpha", "BRIDGE_KEY"]),
            Some(Secret::new(CANARY).unwrap()),
            events.emitter(),
        )
        .unwrap();
    let result = bridge.wait(&job).unwrap();
    let envelope = result.envelope.clone().expect("an envelope");
    assert_eq!(envelope["ok"], true, "{envelope}");
    assert_eq!(envelope["data"]["stored"], true);

    let (check, _) = run(&bridge, &["secret", "get", "srv-alpha", "BRIDGE_KEY"]);
    let envelope = check.envelope.expect("an envelope");
    assert_eq!(envelope["data"]["set"], true);
    assert!(envelope["data"].get("value").is_none());

    let (revealed, _) = run(&bridge, &["secret", "get", "srv-alpha", "BRIDGE_KEY", "--reveal"]);
    assert_eq!(
        revealed.envelope.unwrap()["data"]["value"],
        CANARY,
        "positive control: the vault holds exactly what went over stdin"
    );
    for text in [format!("{result:?}"), format!("{:?}", events.all())] {
        assert!(!text.contains(CANARY), "leaked into: {text}");
    }
}

#[test]
fn output_that_is_not_one_envelope_is_reported_not_guessed() {
    let dir = std::env::temp_dir().join(format!("bridge-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bridge = script_bridge(
        &dir,
        "bad.sh",
        "echo one\necho two\necho 'oops' >&2\nexit 3\n",
    );
    let (result, events) = run(&bridge, &["bad"]);
    assert_eq!(result.exit_code, Some(3));
    assert_eq!(result.envelope, None);
    assert!(result.parse_error.as_deref().unwrap().contains("more than one line"));
    assert_eq!(result.stderr, vec!["oops".to_string()]);
    assert_eq!(events.lines(), vec!["oops".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}
