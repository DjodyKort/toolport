#![allow(dead_code)]

use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

pub const CTL: &str = env!("CARGO_BIN_EXE_toolportctl");
pub const SELFMCP: &str = env!("CARGO_BIN_EXE_toolport-selfmcp");
const TIMEOUT: Duration = Duration::from_secs(180);

static SEQ: AtomicU64 = AtomicU64::new(0);

pub type Tree = BTreeMap<String, Vec<u8>>;

pub struct Canary {
    tag: String,
    issued: RefCell<Vec<String>>,
}

impl Canary {
    pub fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        Self {
            tag: format!(
                "FAKE-CANARY-{:08x}{:04x}",
                nanos,
                SEQ.fetch_add(1, Ordering::Relaxed) as u16 ^ std::process::id() as u16
            ),
            issued: RefCell::new(Vec::new()),
        }
    }

    pub fn val(&self, label: &str) -> String {
        let value = format!("{}-{label}", self.tag);
        let mut issued = self.issued.borrow_mut();
        if !issued.contains(&value) {
            issued.push(value.clone());
        }
        value
    }

    pub fn all(&self) -> Vec<String> {
        self.issued.borrow().clone()
    }
}

pub struct Run {
    pub args: Vec<String>,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn describe(&self) -> String {
        format!(
            "{} -> code {:?}\nstdout: {}\nstderr: {}",
            self.args.join(" "),
            self.code,
            self.stdout.trim(),
            self.stderr.trim()
        )
    }

    pub fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }

    pub fn is_crash(&self) -> bool {
        matches!(self.code, None | Some(101))
            || self.stderr.contains("panicked at")
            || self.stdout.contains("panicked at")
    }

    pub fn assert_orderly(&self) -> &Self {
        assert!(!self.is_crash(), "crashed: {}", self.describe());
        self
    }

    pub fn assert_ok(&self) -> &Self {
        assert_eq!(self.code, Some(0), "{}", self.describe());
        self
    }

    pub fn assert_failed(&self) -> &Self {
        self.assert_orderly();
        assert_eq!(self.code, Some(1), "{}", self.describe());
        self
    }

    pub fn envelope(&self) -> Value {
        serde_json::from_str(self.stdout.trim())
            .unwrap_or_else(|e| panic!("not a JSON envelope ({e}): {}", self.describe()))
    }

    pub fn data(&self) -> Value {
        self.envelope()["data"].clone()
    }

    pub fn error_message(&self) -> String {
        self.envelope()["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}

pub struct Sandbox {
    pub root: PathBuf,
    pub data: PathBuf,
    pub home: PathBuf,
    pub work: PathBuf,
    pub input: PathBuf,
    pub key: String,
    env: RefCell<Vec<(String, String)>>,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if std::env::var_os("HARDENING_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

impl Sandbox {
    pub fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "toolport-hardening-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let sandbox = Self {
            data: root.join("data"),
            home: root.join("home"),
            work: root.join("work"),
            input: root.join("input"),
            root,
            key: "ab".repeat(32),
            env: RefCell::new(Vec::new()),
        };
        for dir in [&sandbox.data, &sandbox.home, &sandbox.work, &sandbox.input] {
            std::fs::create_dir_all(dir).unwrap();
        }
        sandbox
    }

    pub fn set_env(&self, key: &str, value: &str) {
        let mut env = self.env.borrow_mut();
        env.retain(|(k, _)| k != key);
        env.push((key.to_string(), value.to_string()));
    }

    pub fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear();
        for key in ["PATH", "TMPDIR", "LANG"] {
            if let Ok(value) = std::env::var(key) {
                cmd.env(key, value);
            }
        }
        cmd.env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("TOOLPORT_DATA_DIR", &self.data)
            .env("TOOLPORT_SECRET_KEY", &self.key)
            .env("TOOLPORT_LOCK_TIMEOUT_MS", "30000")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Tester")
            .env("GIT_AUTHOR_EMAIL", "tester@example.invalid")
            .env("GIT_COMMITTER_NAME", "Tester")
            .env("GIT_COMMITTER_EMAIL", "tester@example.invalid")
            .env("RUST_BACKTRACE", "0")
            .current_dir(&self.work);
        for (key, value) in self.env.borrow().iter() {
            cmd.env(key, value);
        }
        cmd
    }

    pub fn spawn_ctl(&self, args: &[&str], stdin: Option<&str>, env: &[(&str, &str)]) -> Pending {
        let mut cmd = self.command(CTL);
        cmd.args(args);
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let child = cmd.spawn().expect("spawn toolportctl");
        Pending {
            child,
            args: args.iter().map(|s| s.to_string()).collect(),
            stdin: stdin.map(str::to_string),
        }
    }

    pub fn ctl(&self, args: &[&str]) -> Run {
        self.spawn_ctl(args, None, &[]).wait()
    }

    pub fn ctl_in(&self, args: &[&str], stdin: &str) -> Run {
        self.spawn_ctl(args, Some(stdin), &[]).wait()
    }

    pub fn ctl_env(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        self.spawn_ctl(args, None, env).wait()
    }

    pub fn registry_path(&self) -> PathBuf {
        self.data.join("registry.json")
    }

    pub fn write_registry(&self, registry: &Value) {
        std::fs::write(
            self.registry_path(),
            serde_json::to_string_pretty(registry).unwrap(),
        )
        .unwrap();
    }

    pub fn registry(&self) -> Value {
        let text = std::fs::read_to_string(self.registry_path()).unwrap();
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("registry.json is invalid ({e})"))
    }

    pub fn server_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.registry()["servers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| s["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    pub fn tree(&self) -> Tree {
        tree(&self.root, &["input"])
    }

    pub fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.work.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    pub fn skills_repo(&self) -> PathBuf {
        let repo = self.work.join("skills-repo");
        for (rel, text) in [
            (
                "skills/demo/SKILL.md",
                "---\nname: demo\ndescription: A synthetic demo skill\n---\nBody text\n",
            ),
            (
                "rules/house-rule/SKILL.md",
                "---\nname: house-rule\ndescription: A synthetic rule\nactivation: always\n---\nRule text\n",
            ),
            (
                "agents/helper/AGENT.md",
                "---\nname: helper\ndescription: A synthetic helper agent\nmodel: inherit\n---\nAgent prompt\n",
            ),
            (
                "styles/plain/STYLE.md",
                "---\nname: plain\ndescription: A synthetic plain style\nkeep-coding-instructions: true\n---\nStyle text\n",
            ),
        ] {
            let path = repo.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        repo
    }

    pub fn mcpm_root(&self, name: &str, servers: &Value, client: Option<&Value>) -> PathBuf {
        let root = self.input.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("servers.json"), servers.to_string()).unwrap();
        if let Some(client) = client {
            std::fs::write(root.join("claude-code.json"), client.to_string()).unwrap();
        }
        root
    }

    pub fn selfmcp(&self) -> Selfmcp {
        let mut cmd = self.command(SELFMCP);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn toolport-selfmcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let errors = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&errors);
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let mut sink = sink.lock().unwrap();
                sink.push_str(&line);
                sink.push('\n');
            }
        });
        let mut session = Selfmcp {
            child,
            stdin,
            lines,
            errors,
            next_id: 1,
            transcript: String::new(),
        };
        session.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "hardening", "version": "1"}}),
        );
        session
    }
}

pub struct Pending {
    child: Child,
    args: Vec<String>,
    stdin: Option<String>,
}

impl Pending {
    pub fn wait(mut self) -> Run {
        let pid = self.child.id();
        if let (Some(mut stdin), Some(data)) = (self.child.stdin.take(), self.stdin.take()) {
            let _ = stdin.write_all(data.as_bytes());
        }
        let args = self.args.clone();
        let (sender, receiver) = mpsc::channel();
        let child = self.child;
        std::thread::spawn(move || {
            let _ = sender.send(child.wait_with_output());
        });
        match receiver.recv_timeout(TIMEOUT) {
            Ok(Ok(out)) => Run {
                args,
                code: out.status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            },
            Ok(Err(e)) => panic!("{}: {e}", args.join(" ")),
            Err(_) => {
                unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                panic!("timed out: toolportctl {}", args.join(" "));
            }
        }
    }
}

pub struct Selfmcp {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    errors: Arc<Mutex<String>>,
    next_id: i64,
    pub transcript: String,
}

impl Selfmcp {
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{message}").expect("write request");
        self.stdin.flush().expect("flush request");
        loop {
            let line = self
                .lines
                .recv_timeout(TIMEOUT)
                .unwrap_or_else(|e| panic!("no answer to {method}: {e}"));
            self.transcript.push_str(&line);
            self.transcript.push('\n');
            let value: Value = serde_json::from_str(&line).expect("json line");
            if value["id"] == id {
                return value;
            }
        }
    }

    pub fn call(&mut self, tool: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": args}))
    }

    pub fn read(&mut self, uri: &str) -> Value {
        self.request("resources/read", json!({"uri": uri}))
    }

    pub fn stderr(&self) -> String {
        self.errors.lock().unwrap().clone()
    }
}

impl Drop for Selfmcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn tree(root: &Path, skip_top: &[&str]) -> Tree {
    fn walk(dir: &Path, root: &Path, skip_top: &[&str], out: &mut Tree) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if dir == root && skip_top.contains(&rel.as_str()) {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                walk(&path, root, skip_top, out);
            } else if meta.is_file() {
                out.insert(rel, std::fs::read(&path).unwrap_or_default());
            } else {
                out.insert(rel, b"<special>".to_vec());
            }
        }
    }
    let mut out = Tree::new();
    walk(root, root, skip_top, &mut out);
    out
}

pub fn tree_diff(before: &Tree, after: &Tree) -> Vec<String> {
    let mut out = Vec::new();
    for (path, bytes) in after {
        match before.get(path) {
            None => out.push(format!("added {path}")),
            Some(old) if old != bytes => out.push(format!(
                "changed {path} ({} -> {} bytes)",
                old.len(),
                bytes.len()
            )),
            Some(_) => {}
        }
    }
    for path in before.keys().filter(|p| !after.contains_key(*p)) {
        out.push(format!("removed {path}"));
    }
    out
}

pub fn find_leaks(label: &str, haystack: &[u8], needles: &[String]) -> Vec<String> {
    needles
        .iter()
        .filter(|n| contains(haystack, n.as_bytes()))
        .map(|n| format!("{label}: {n}"))
        .collect()
}

pub fn scan_tree(tree: &Tree, needles: &[String]) -> Vec<String> {
    tree.iter()
        .flat_map(|(path, bytes)| find_leaks(path, bytes, needles))
        .collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

pub fn truncate_half(path: &Path) {
    let bytes = std::fs::read(path).unwrap();
    std::fs::write(path, &bytes[..bytes.len() / 2]).unwrap();
}

pub fn stray_temp_files(tree: &Tree) -> Vec<String> {
    tree.keys()
        .filter(|p| p.ends_with(".conduit-tmp"))
        .cloned()
        .collect()
}
