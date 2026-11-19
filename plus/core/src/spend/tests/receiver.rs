use super::*;
use crate::spend::runtime;
use crate::spend::store::AppendError;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::Ordering::Relaxed;

fn exchange(port: u16, head: &str, body: &[u8]) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let text = format!("{head}\r\nContent-Length: {}\r\n\r\n", body.len());
    stream.write_all(text.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
    reply
}

fn status_of(reply: &str) -> &str {
    reply.split(' ').nth(1).unwrap_or("")
}

struct Rx {
    dir: TempDir,
    receiver: Receiver,
    body: String,
}

impl Rx {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let receiver = Receiver::bind(0, dir.path().to_path_buf(), Limits::default()).unwrap();
        Self {
            dir,
            receiver,
            body: metric_payload("rx", "claude-sonnet-4", NOW).to_string(),
        }
    }

    fn send(&self, headers: &[&str]) -> String {
        let head = format!("POST /v1/metrics HTTP/1.1\r\n{}", headers.join("\r\n"));
        exchange(self.receiver.port(), &head, self.body.as_bytes())
    }

    fn host(&self) -> String {
        format!("Host: 127.0.0.1:{}", self.receiver.port())
    }

    fn stored(&self) -> usize {
        Store::new(self.dir.path()).read_events().0.len()
    }
}

const JSON: &str = "Content-Type: application/json";

#[test]
fn test40_a_post_with_the_loopback_host_and_json_is_stored() {
    let rx = Rx::new();
    let port = rx.receiver.port();
    for host in [
        format!("Host: 127.0.0.1:{port}"),
        format!("Host: localhost:{port}"),
        format!("Host: LocalHost:{port}"),
        format!("Host: [::1]:{port}"),
    ] {
        let reply = rx.send(&[&host, JSON]);
        assert_eq!(status_of(&reply), "200", "{host}: {reply}");
    }
    assert_eq!(rx.stored(), 20);
}

#[test]
fn test41_a_host_that_is_not_loopback_or_not_this_port_is_refused() {
    let rx = Rx::new();
    let port = rx.receiver.port();
    let other = if port == 4318 { 4319 } else { 4318 };
    for host in [
        "Host: evil.example".to_string(),
        format!("Host: evil.example:{port}"),
        format!("Host: 127.0.0.1.evil.example:{port}"),
        format!("Host: 127.0.0.1:{other}"),
        "Host: 127.0.0.1".to_string(),
        format!("Host: localhost.:{port}"),
        "Host: ".to_string(),
    ] {
        let reply = rx.send(&[&host, JSON]);
        assert_eq!(status_of(&reply), "403", "{host}: {reply}");
    }
    assert_eq!(status_of(&rx.send(&[JSON])), "403");
    let twice = rx.send(&[&rx.host(), "Host: evil.example", JSON]);
    assert_eq!(status_of(&twice), "403", "{twice}");
    assert_eq!(rx.stored(), 0);
    assert_eq!(rx.receiver.counters.rejected.load(Relaxed), 9);
}

#[test]
fn test42_a_request_from_a_web_page_is_refused_by_its_origin() {
    let rx = Rx::new();
    for origin in [
        "Origin: https://evil.example",
        "Origin: http://127.0.0.1:4318",
        "origin: http://localhost",
        "Origin: ",
    ] {
        let reply = rx.send(&[&rx.host(), JSON, origin]);
        assert_eq!(status_of(&reply), "403", "{origin}: {reply}");
    }
    assert_eq!(rx.stored(), 0);
    assert_eq!(status_of(&rx.send(&[&rx.host(), JSON])), "200");
    assert_eq!(
        status_of(&rx.send(&[&rx.host(), JSON, "Origin: null"])),
        "200"
    );
}

#[test]
fn test43_a_body_that_is_not_application_json_is_refused() {
    let rx = Rx::new();
    let host = rx.host();
    for content_type in [
        "Content-Type: text/plain",
        "Content-Type: application/x-www-form-urlencoded",
        "Content-Type: multipart/form-data; boundary=x",
        "Content-Type: application/x-protobuf",
        "Content-Type: application/jsonp",
        "Content-Type: text/plain; application/json",
    ] {
        let reply = rx.send(&[&host, content_type]);
        assert_eq!(status_of(&reply), "415", "{content_type}: {reply}");
    }
    assert_eq!(status_of(&rx.send(&[&host])), "415");
    assert_eq!(rx.stored(), 0);
    for content_type in [
        "Content-Type: application/json; charset=utf-8",
        "content-type: Application/JSON",
    ] {
        let reply = rx.send(&[&host, content_type]);
        assert_eq!(status_of(&reply), "200", "{content_type}: {reply}");
    }
}

#[test]
fn test44_a_preflight_from_a_web_page_gets_no_cors_approval() {
    let rx = Rx::new();
    let head = format!(
        "OPTIONS /v1/metrics HTTP/1.1\r\n{}\r\nOrigin: https://evil.example\r\nAccess-Control-Request-Method: POST",
        rx.host()
    );
    let reply = exchange(rx.receiver.port(), &head, b"");
    assert!(
        !reply.to_ascii_lowercase().contains("access-control-allow"),
        "{reply}"
    );
    assert_ne!(status_of(&reply), "200");
}

#[test]
fn test45_the_size_cap_is_enforced_when_events_are_appended() {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path());
    let one = [token_event("a", TOKEN_INPUT, 1.0, NOW)];
    store.append(&one, u64::MAX).unwrap();
    let size = std::fs::metadata(store.events_path()).unwrap().len();
    assert_eq!(store.append(&one, size), Err(AppendError::Full));
    assert_eq!(store.append(&one, size * 2 - 1), Err(AppendError::Full));
    store.append(&one, size * 2).unwrap();
    let batch = [
        token_event("b", TOKEN_INPUT, 1.0, NOW),
        token_event("c", TOKEN_INPUT, 1.0, NOW),
    ];
    assert_eq!(store.append(&batch, size * 3), Err(AppendError::Full));
    assert_eq!(
        std::fs::metadata(store.events_path()).unwrap().len(),
        size * 2
    );
    assert_eq!(store.read_events().0.len(), 2);
}

#[test]
fn test46_a_receiver_over_the_cap_answers_507_and_a_prune_makes_room() {
    let rx = Rx::new();
    let spend = rx.dir.path();
    std::fs::write(
        spend.join(config::CONFIG_FILE),
        serde_json::to_string(&SpendConfig {
            max_mb: 1,
            ..SpendConfig::default()
        })
        .unwrap(),
    )
    .unwrap();
    let store = Store::new(spend);
    let filler: Vec<Event> = (0..4_000)
        .map(|i| token_event(&format!("s{i}"), TOKEN_INPUT, 1.0, NOW - 4_000 + i))
        .collect();
    store.append(&filler, u64::MAX).unwrap();
    let cap = 1024 * 1024;
    while std::fs::metadata(store.events_path()).unwrap().len() < cap - 64 {
        store.append(&filler[..100], u64::MAX).unwrap();
    }
    let before = std::fs::metadata(store.events_path()).unwrap().len();
    let reply = rx.send(&[&rx.host(), JSON]);
    assert_eq!(status_of(&reply), "507", "{reply}");
    assert_eq!(
        std::fs::metadata(store.events_path()).unwrap().len(),
        before
    );
    assert!(rx.receiver.counters.store_full.load(Relaxed));
    assert_eq!(rx.receiver.counters.dropped.load(Relaxed), 5);

    let cfg = config::load(spend);
    runtime::prune_full(spend, &[], &cfg, NOW).unwrap();
    assert!(std::fs::metadata(store.events_path()).unwrap().len() <= cap - cap / 10);
    assert_eq!(status_of(&rx.send(&[&rx.host(), JSON])), "200");
}

#[test]
fn test47_a_prune_reads_the_transcripts_before_it_takes_the_store_lock() {
    use std::ffi::CString;
    let dir = TempDir::new().unwrap();
    let spend = dir.path().join("spend");
    let projects = dir.path().join("projects/folder");
    std::fs::create_dir_all(&projects).unwrap();
    let fifo = projects.join("slow.jsonl");
    let path = CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let store = Store::new(&spend);
    store
        .append(
            &[token_event("old", TOKEN_INPUT, 1.0, NOW - 40 * MS_PER_DAY)],
            u64::MAX,
        )
        .unwrap();
    let roots = vec![dir.path().join("projects")];
    let (spend_dir, cfg) = (spend.clone(), SpendConfig::default());
    let pruning =
        std::thread::spawn(move || runtime::prune_with(&spend_dir, &roots, &cfg, NOW, false));
    std::thread::sleep(std::time::Duration::from_millis(300));
    let started = std::time::Instant::now();
    store
        .append(&[token_event("new", TOKEN_INPUT, 1.0, NOW)], u64::MAX)
        .unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    drop(std::fs::OpenOptions::new().write(true).open(&fifo).unwrap());
    let report = pruning.join().unwrap().unwrap();
    assert_eq!(report.removed_events, 1);
}

fn read_only_dir_with(dir: &std::path::Path, name: &str, text: &str) -> std::path::PathBuf {
    let ro = dir.join(format!("ro-{name}"));
    std::fs::create_dir_all(&ro).unwrap();
    std::fs::write(ro.join(name), text).unwrap();
    std::os::unix::fs::symlink(ro.join(name), dir.join("spend").join(name)).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o500)).unwrap();
    ro
}

fn allow_writes(dir: &std::path::Path) {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

fn old_and_new() -> [Event; 2] {
    [
        token_event("old", TOKEN_INPUT, 500.0, NOW - 40 * MS_PER_DAY),
        token_event("new", TOKEN_INPUT, 7.0, NOW),
    ]
}

#[test]
fn test48_a_failed_events_write_leaves_the_daily_totals_alone() {
    if is_root() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let spend = dir.path().join("spend");
    std::fs::create_dir_all(&spend).unwrap();
    let store = Store::new(&spend);
    let text: String = old_and_new()
        .iter()
        .map(|e| serde_json::to_string(e).unwrap() + "\n")
        .collect();
    let ro = read_only_dir_with(dir.path(), "events.jsonl", &text);
    let failed = store.prune(NOW, 30, u64::MAX, &|_| None, false);
    assert!(failed.is_err());
    assert!(store.read_daily().is_empty());
    allow_writes(&ro);
    store.prune(NOW, 30, u64::MAX, &|_| None, false).unwrap();
    assert_eq!(store.read_daily()[0].input, 500);
}

#[test]
fn test49_a_failed_daily_write_puts_the_events_back_so_nothing_is_lost_or_counted_twice() {
    if is_root() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let spend = dir.path().join("spend");
    std::fs::create_dir_all(&spend).unwrap();
    let store = Store::new(&spend);
    store.append(&old_and_new(), u64::MAX).unwrap();
    let before = std::fs::read_to_string(store.events_path()).unwrap();
    let ro = read_only_dir_with(dir.path(), "daily.jsonl", "");
    let failed = store.prune(NOW, 30, u64::MAX, &|_| None, false);
    assert!(failed.is_err());
    assert_eq!(
        std::fs::read_to_string(store.events_path()).unwrap(),
        before
    );
    allow_writes(&ro);
    store.prune(NOW, 30, u64::MAX, &|_| None, false).unwrap();
    let daily = store.read_daily();
    assert_eq!((daily.len(), daily[0].input), (1, 500));
}
