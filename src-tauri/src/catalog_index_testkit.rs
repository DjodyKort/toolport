use super::{RegistryIndex, Tuning, INDEX_FILE, INDEX_FILE_VERSION};
use crate::catalog::CatalogEntry;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) const TEST_TUNING: Tuning = Tuning {
    connect_timeout: Duration::from_secs(2),
    read_timeout: Duration::from_secs(2),
    retry_pause: Duration::from_millis(10),
    max_pages: 20,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct Scratch {
    pub dir: PathBuf,
    pub index: PathBuf,
}

impl Scratch {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "toolport-catalog-index-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let index = dir.join("plus").join(INDEX_FILE);
        Self { dir, index }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub(crate) fn server_item(name: &str, description: &str, status: &str) -> Value {
    json!({
        "server": {
            "name": format!("io.example/{}", name.to_lowercase()),
            "title": name,
            "description": description,
            "remotes": [{"type": "streamable-http", "url": "https://example.com/mcp"}],
        },
        "_meta": {"io.modelcontextprotocol.registry/official": {
            "status": status,
            "isLatest": true,
        }},
    })
}

pub(crate) fn page_body(items: Vec<Value>, next_cursor: Option<&str>) -> String {
    let mut metadata = json!({"count": items.len()});
    if let Some(cursor) = next_cursor {
        metadata["nextCursor"] = json!(cursor);
    }
    json!({"servers": items, "metadata": metadata}).to_string()
}

pub(crate) fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| urlencoding::decode(value).unwrap().into_owned())
    })
}

pub(crate) fn entry(name: &str, description: &str) -> CatalogEntry {
    CatalogEntry {
        name: name.into(),
        description: description.into(),
        transport: "http".into(),
        command: None,
        args: vec![],
        launch: None,
        url: Some("https://example.com/mcp".into()),
        env_keys: vec![],
        source: "registry".into(),
        homepage: None,
        publisher: Some("io.example".into()),
        category: String::new(),
        credentials_url: None,
        setup_hint: None,
        url_hint: None,
    }
}

pub(crate) fn index_of(
    entries: &[CatalogEntry],
    last_synced_at: Option<&str>,
    complete: bool,
) -> RegistryIndex {
    RegistryIndex {
        version: INDEX_FILE_VERSION,
        last_synced_at: last_synced_at.map(String::from),
        complete,
        entries: entries
            .iter()
            .map(|e| (e.name.to_lowercase(), e.clone()))
            .collect(),
        ..RegistryIndex::default()
    }
}

pub(crate) fn write_index(path: &std::path::Path, index: &RegistryIndex) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string(index).unwrap()).unwrap();
}

pub(crate) struct MockRegistry {
    pub base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl MockRegistry {
    pub fn start(handler: impl Fn(&str) -> (u16, String) + Send + 'static) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind the mock registry");
        let port = server.server_addr().to_ip().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log, halt) = (Arc::clone(&requests), Arc::clone(&stop));
        let worker = std::thread::spawn(move || {
            while !halt.load(Ordering::SeqCst) {
                let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(20)) else {
                    continue;
                };
                let url = request.url().to_string();
                log.lock().unwrap().push(url.clone());
                let (status, body) = handler(&url);
                let json = tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/json"[..],
                )
                .unwrap();
                let _ = request.respond(
                    tiny_http::Response::from_string(body)
                        .with_status_code(status)
                        .with_header(json),
                );
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}/v0.1/servers"),
            requests,
            stop,
            worker: Some(worker),
        }
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for MockRegistry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) fn closed_port_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://127.0.0.1:{port}/v0.1/servers")
}

pub(crate) fn hold_sync(path: &std::path::Path) -> impl Drop {
    super::Flight::acquire(path).expect("no sync is running yet")
}
