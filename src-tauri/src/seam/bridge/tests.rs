use super::*;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(30);
const CANARY: &str = "CANARY-bridge-stdin-9c41e7";

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

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

fn script_bridge(tag: &str, body: &str) -> (Bridge, PathBuf) {
    let dir = std::env::temp_dir().join(format!("bridge-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("ctl.sh");
    std::fs::write(&script, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bridge = Bridge::with_binary(script)
        .clear_env()
        .env("PATH", "/usr/bin:/bin");
    (bridge, dir)
}

// A script that was written a moment ago can still be open for writing in a child that another
// test thread forked, and then fails to exec with ETXTBSY.
fn start(bridge: &Bridge, args: &[&str], secret: Option<&str>, events: &Events) -> JobId {
    for _ in 0..50 {
        let secret = secret.map(|value| Secret::new(value).unwrap());
        match bridge.start(argv(args), secret, events.emitter()) {
            Err(BridgeError::Spawn(why)) if why.contains("busy") => {
                std::thread::sleep(Duration::from_millis(20));
            }
            other => return other.expect("start"),
        }
    }
    panic!("the script stayed busy");
}

fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[test]
fn home_and_data_dir_flags_never_pass_the_bridge() {
    for bad in [
        argv(&["skills", "ls", "--home", "/tmp/x"]),
        argv(&["skills", "ls", "--home=/tmp/x"]),
        argv(&["--data-dir", "/tmp/x", "status"]),
        argv(&["status", "--data-dir=/tmp/x"]),
    ] {
        assert!(
            matches!(validate_argv(&bad), Err(BridgeError::Forbidden(_))),
            "{bad:?}"
        );
    }
    assert!(validate_argv(&argv(&["status"])).is_ok());
    assert!(validate_argv(&argv(&["mcp", "install", "--access-set", "focus"])).is_ok());
    assert!(matches!(validate_argv(&[]), Err(BridgeError::Usage(_))));
}

#[test]
fn the_binary_is_found_next_to_the_executable() {
    let dir = std::env::temp_dir().join(format!("bridge-beside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join(format!("app{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&exe, b"").unwrap();
    assert_eq!(binary_beside(&exe), None);
    let ctl = dir.join(format!("{BINARY_NAME}{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&ctl, b"").unwrap();
    assert_eq!(binary_beside(&exe), Some(ctl));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_binary_is_reported_without_a_fallback() {
    let bridge = Bridge::with_binary("/nonexistent/toolportctl");
    let error = bridge
        .start(argv(&["status"]), None, Arc::new(|_: &JobEvent| {}))
        .unwrap_err();
    assert!(matches!(error, BridgeError::Spawn(_)), "{error}");
}

#[test]
fn an_unknown_job_is_an_error_for_wait_and_cancel() {
    let bridge = Bridge::new();
    assert!(matches!(
        bridge.wait("job-9"),
        Err(BridgeError::UnknownJob(_))
    ));
    assert!(matches!(
        bridge.cancel("job-9"),
        Err(BridgeError::UnknownJob(_))
    ));
}

#[test]
fn a_command_returns_its_envelope_with_json_before_the_argv() {
    let (bridge, dir) = script_bridge(
        "envelope",
        "printf '{\"ok\":true,\"command\":\"%s\",\"schemaVersion\":1,\"data\":{\"argv\":\"%s\"}}\\n' \"$2\" \"$*\"\n",
    );
    let events = Events::default();
    let job = start(&bridge, &["status"], None, &events);
    let result = bridge.wait(&job).unwrap();
    assert_eq!(result.exit_code, Some(0), "{result:?}");
    let envelope = result.envelope.expect("an envelope");
    assert_eq!(envelope["command"], "status");
    assert_eq!(envelope["data"]["argv"], "--json status");
    assert_eq!(
        events
            .all()
            .iter()
            .map(|e| e.kind.clone())
            .collect::<Vec<_>>(),
        vec![EventKind::Exit]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn output_that_is_not_one_envelope_is_reported_not_guessed() {
    let (bridge, dir) = script_bridge("bad", "echo one\necho two\necho 'oops' >&2\nexit 3\n");
    let events = Events::default();
    let job = start(&bridge, &["bad"], None, &events);
    let result = bridge.wait(&job).unwrap();
    assert_eq!(result.exit_code, Some(3));
    assert_eq!(result.envelope, None);
    assert!(result
        .parse_error
        .as_deref()
        .unwrap()
        .contains("more than one"));
    assert_eq!(result.stderr, vec!["oops".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancel_kills_the_whole_process_group_and_reaps_the_child() {
    let (bridge, dir) = script_bridge(
        "cancel",
        "echo 'step 1' >&2\nsleep 120 &\necho \"helper $!\" >&2\necho 'ready' >&2\nwait\n",
    );
    let events = Events::default();
    let job = start(&bridge, &["slow"], None, &events);
    let pid = bridge.pid(&job).unwrap();
    let helper = events.wait_for_line("helper ");
    events.wait_for_line("ready");
    let helper: u32 = helper.trim_start_matches("helper ").parse().unwrap();
    assert_eq!(events.lines()[0], "step 1");
    assert!(alive(pid));
    bridge.cancel(&job).unwrap();
    let result = bridge.wait(&job).unwrap();
    assert!(result.cancelled);
    assert_eq!(result.signal, Some(libc::SIGKILL));
    assert_eq!(result.envelope, None);
    assert!(!alive(pid), "the child was reaped");
    let deadline = Instant::now() + DEADLINE;
    while alive(helper) {
        assert!(Instant::now() < deadline, "the helper outlived the group");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(bridge.wait(&job).is_err(), "a result is handed over once");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_secret_travels_over_stdin_and_never_appears_in_events_or_results() {
    let (bridge, dir) = script_bridge(
        "secret",
        "IFS= read -r secret || true\n\
         echo \"got $secret\" >&2\n\
         printf '{\"ok\":true,\"command\":\"echo\",\"schemaVersion\":1,\"data\":{\"echo\":\"%s\"}}\\n' \"$secret\"\n",
    );
    let events = Events::default();
    assert!(!format!("{:?}", Secret::new(CANARY).unwrap()).contains(CANARY));
    let job = start(&bridge, &["echo"], Some(&format!("{CANARY}\n")), &events);
    let result = bridge.wait(&job).unwrap();
    assert_eq!(
        result.envelope.as_ref().unwrap()["data"]["echo"],
        "[redacted]"
    );
    assert!(events.lines().contains(&"got [redacted]".to_string()));
    for text in [format!("{result:?}"), format!("{:?}", events.all())] {
        assert!(!text.contains(CANARY), "leaked into: {text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
