use super::exec::{CmdOutput, GitRunner, ShellRunner, SystemGit};
use super::net::{host_of, url_allowed, HttpClient, HttpError};
use super::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

const API: &str = "https://api.example.invalid";
const FAKE_TOKEN: &str = "FAKE-TOKEN-do-not-print-91c2";

#[derive(Default, Clone)]
struct MockHttp {
    texts: Arc<Mutex<HashMap<String, Result<String, HttpError>>>>,
    blobs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    requests: Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>,
}

impl MockHttp {
    fn text(&self, url: &str, body: &str) {
        self.texts
            .lock()
            .unwrap()
            .insert(url.into(), Ok(body.into()));
    }
    fn fail(&self, url: &str, status: u16) {
        self.texts
            .lock()
            .unwrap()
            .insert(url.into(), Err(HttpError::new(Some(status), "mock")));
    }
    fn blob(&self, url: &str, bytes: &[u8]) {
        self.blobs
            .lock()
            .unwrap()
            .insert(url.into(), bytes.to_vec());
    }
}

impl HttpClient for MockHttp {
    fn get_text(&self, url: &str, headers: &[(String, String)]) -> Result<String, HttpError> {
        self.requests
            .lock()
            .unwrap()
            .push((url.into(), headers.to_vec()));
        self.texts
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(HttpError::new(Some(404), "no mock")))
    }

    fn download(&self, url: &str, dest: &Path) -> Result<String, HttpError> {
        let bytes = self
            .blobs
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .ok_or_else(|| HttpError::new(Some(404), "no mock"))?;
        std::fs::write(dest, &bytes).unwrap();
        Ok(crate::plus::hashing::hex(&Sha256::digest(&bytes)))
    }
}

#[derive(Clone, Default)]
struct MockShell {
    calls: Arc<Mutex<Vec<(String, PathBuf)>>>,
    exit: Arc<Mutex<i32>>,
}

impl ShellRunner for MockShell {
    fn run(&self, command: &str, cwd: &Path, _t: Duration) -> Result<CmdOutput, String> {
        self.calls
            .lock()
            .unwrap()
            .push((command.into(), cwd.into()));
        Ok(CmdOutput {
            code: *self.exit.lock().unwrap(),
            stdout: String::new(),
            stderr: "boom".into(),
        })
    }
}

struct ScriptedGit(Box<dyn Fn(&[&str]) -> CmdOutput + Send + Sync>);

impl GitRunner for ScriptedGit {
    fn git(&self, _r: &Path, args: &[&str], _t: Duration) -> Result<CmdOutput, String> {
        Ok((self.0)(args))
    }
}

fn out(code: i32, stdout: &str) -> CmdOutput {
    CmdOutput {
        code,
        stdout: stdout.into(),
        stderr: String::new(),
    }
}

fn env_with(http: &MockHttp, shell: &MockShell, git: Box<dyn GitRunner + Send + Sync>) -> Env {
    Env {
        git,
        shell: Box::new(shell.clone()),
        http: Box::new(http.clone()),
        github_api: API.into(),
        npm_registry: "https://npm.example.invalid".into(),
        pypi_index: "https://pypi.example.invalid".into(),
        platform: ("linux".into(), "amd64".into()),
        home: None,
        github_token: None,
        now: || "2026-01-02T03:04:05Z".into(),
    }
}

fn entry(value: Value) -> ServerEntry {
    let mut v = json!({"name": "x", "transport": "stdio"});
    for (k, val) in value.as_object().unwrap() {
        v[k] = val.clone();
    }
    if v.get("id").is_none() {
        v["id"] = v["name"].clone();
    }
    serde_json::from_value(v).unwrap()
}

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("plus-update-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn commit(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", &format!("edit {file}")]);
}

struct Repos {
    _tmp: Tmp,
    a: PathBuf,
    b: PathBuf,
}

fn repos(tag: &str) -> Repos {
    let tmp = Tmp::new(tag);
    let origin = tmp.path("origin.git");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    let a = tmp.path("a");
    let b = tmp.path("b");
    git(
        &tmp.0,
        &["clone", "-q", origin.to_str().unwrap(), a.to_str().unwrap()],
    );
    commit(&a, "one.txt", "1");
    git(&a, &["push", "-q", "-u", "origin", "HEAD:main"]);
    git(&a, &["branch", "-q", "-u", "origin/main"]);
    git(
        &tmp.0,
        &["clone", "-q", origin.to_str().unwrap(), b.to_str().unwrap()],
    );
    Repos { _tmp: tmp, a, b }
}

fn git_entry(path: &Path, post_update: Option<&str>) -> ServerEntry {
    let mut meta = json!({"type": "git", "path": path.to_str().unwrap()});
    if let Some(p) = post_update {
        meta["post_update"] = json!(p);
    }
    entry(json!({"name": "g", "command": "node", "args": ["x.js"], "mcpmSource": meta}))
}

fn sys_env(shell: &MockShell) -> Env {
    env_with(&MockHttp::default(), shell, Box::new(SystemGit))
}

#[test]
fn url_policy_requires_https_except_loopback() {
    assert!(url_allowed("https://example.com/a").is_ok());
    assert!(url_allowed("http://127.0.0.1:8080/a").is_ok());
    assert!(url_allowed("http://localhost/a").is_ok());
    assert!(url_allowed("http://[::1]:9/a").is_ok());
    assert!(url_allowed("http://example.com/a").is_err());
    assert!(url_allowed("http://127.0.0.1.example.com/a").is_err());
    assert!(url_allowed("file:///etc/passwd").is_err());
    assert!(url_allowed("ftp://example.com").is_err());
    assert_eq!(
        host_of("https://user:pw@Example.com:8443/x").as_deref(),
        Some("example.com")
    );
}

#[test]
fn version_comparison_handles_prefixes_and_prereleases() {
    use std::cmp::Ordering::*;
    assert_eq!(pins::compare_versions("0.0.5", "v0.0.6"), Some(Less));
    assert_eq!(pins::compare_versions("1.2", "1.2.0"), Some(Equal));
    assert_eq!(pins::compare_versions("1.10.0", "1.9.9"), Some(Greater));
    assert_eq!(pins::compare_versions("2.0.0-rc.1", "2.0.0"), Some(Less));
    assert_eq!(pins::compare_versions("nightly", "1.0.0"), None);
}

#[test]
fn pin_specs_parse_for_npx_and_uvx() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let s = pins::parse_spec(&a(&["-y", "@scope/pkg@1.2.3", "--flag"]), false).unwrap();
    assert_eq!(
        (s.name.as_str(), s.version.as_deref(), s.arg_index),
        ("@scope/pkg", Some("1.2.3"), 1)
    );
    let s = pins::parse_spec(&a(&["-y", "@scope/pkg"]), false).unwrap();
    assert_eq!(s.version, None);
    let s = pins::parse_spec(&a(&["--from", "tool==0.4.1", "tool-cli"]), true).unwrap();
    assert_eq!(
        (s.name.as_str(), s.version.as_deref(), s.separator),
        ("tool", Some("0.4.1"), "==")
    );
    let mut args = a(&["-y", "@scope/pkg@1.2.3"]);
    let spec = pins::parse_spec(&args, false).unwrap();
    assert!(pins::rewrite(&mut args, &spec, "1.3.0"));
    assert_eq!(args[1], "@scope/pkg@1.3.0");
    let again = pins::parse_spec(&args, false).unwrap();
    assert!(!pins::rewrite(&mut args, &again, "1.3.0"));
}

#[test]
fn detection_covers_each_launch_shape() {
    let tmp = Tmp::new("detect");
    let repo = tmp.path("proj");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("dist")).unwrap();
    std::fs::write(repo.join("dist/index.js"), "").unwrap();
    let home = tmp.path("home");
    std::fs::create_dir_all(home.join(".local/bin")).unwrap();
    std::fs::write(home.join(".local/bin/tool"), "").unwrap();
    let k = |e: ServerEntry| source::detect(&e, Some(&home), &SystemGit).kind();
    assert_eq!(
        k(entry(
            json!({"command": "npx", "args": ["-y", "pkg@1.0.0"]})
        )),
        "npx"
    );
    assert_eq!(k(entry(json!({"command": "uvx", "args": ["pkg"]}))), "uvx");
    assert_eq!(
        k(entry(
            json!({"transport": "http", "url": "https://example.invalid/mcp"})
        )),
        "remote"
    );
    assert_eq!(
        k(entry(
            json!({"command": "uv", "args": ["run", "--directory", repo.to_str().unwrap(), "srv"]})
        )),
        "git"
    );
    assert_eq!(
        k(entry(
            json!({"command": "node", "args": [repo.join("dist/index.js").to_str().unwrap()]})
        )),
        "git"
    );
    assert_eq!(
        k(entry(
            json!({"command": home.join(".local/bin/tool").to_str().unwrap()})
        )),
        "github-release"
    );
    assert_eq!(k(entry(json!({"command": "mystery"}))), "unknown");
}

#[test]
fn stored_metadata_wins_over_detection() {
    let e = entry(json!({"command": "npx", "args": ["pkg"],
        "mcpmSource": {"type": "git", "path": "/srv/x", "post_update": "make"}}));
    let (src, from_meta) = source::effective(&e, None, &SystemGit);
    assert!(from_meta);
    assert_eq!(src.kind(), "git");
}

#[test]
fn git_check_reports_ahead_and_behind_from_the_runner() {
    let runner = ScriptedGit(Box::new(|args| match args[0] {
        "rev-parse" if args.contains(&"--is-inside-work-tree") => out(0, "true\n"),
        "rev-parse" => out(0, "origin/main\n"),
        "status" => out(0, ""),
        "fetch" => out(0, ""),
        "branch" => out(0, "main\n"),
        "rev-list" => out(0, "0\t3\n"),
        "log" => out(0, "a1 one\nb2 two\nc3 three\n"),
        _ => out(1, ""),
    }));
    let tmp = Tmp::new("scripted");
    let s = gitops::check(&runner, &tmp.0, None).unwrap();
    assert_eq!((s.ahead, s.behind, s.dirty), (0, 3, false));
    assert_eq!(s.remote_ref, "origin/main");
    assert_eq!(s.summaries.len(), 3);
}

#[test]
fn git_fetch_failure_is_an_error_without_leaking_details() {
    let runner = ScriptedGit(Box::new(|args| match args[0] {
        "rev-parse" => out(0, "true\n"),
        "status" => out(0, ""),
        "fetch" => CmdOutput {
            code: 128,
            stdout: String::new(),
            stderr: "git@host: Permission denied (publickey).".into(),
        },
        _ => out(0, ""),
    }));
    let tmp = Tmp::new("fetchfail");
    let err = gitops::check(&runner, &tmp.0, None).unwrap_err();
    assert!(err.contains("authentication failed"), "{err}");
}

#[test]
fn git_update_fast_forwards_a_real_clone() {
    let r = repos("ffwd");
    commit(&r.b, "two.txt", "2");
    git(&r.b, &["push", "-q", "origin", "HEAD:main"]);
    let shell = MockShell::default();
    let env = sys_env(&shell);
    let mut entries = vec![git_entry(&r.a, None)];

    let check = run(&env, &mut entries, &Options::new(Mode::Check)).unwrap();
    let s = &check.servers[0];
    assert_eq!((s.status, s.behind), (Status::UpdateAvailable, Some(1)));
    assert!(!r.a.join("two.txt").exists());

    let dry = run(&env, &mut entries, &Options::new(Mode::DryRun)).unwrap();
    assert!(dry.servers[0].plan[0].contains("merge --ff-only"));
    assert!(!r.a.join("two.txt").exists());

    let done = run(&env, &mut entries, &Options::new(Mode::Apply)).unwrap();
    assert_eq!(
        done.servers[0].status,
        Status::Updated,
        "{:?}",
        done.servers[0]
    );
    assert!(r.a.join("two.txt").exists());
    assert!(done.servers[0].changed);
    assert_eq!(
        entries[0].unknown_fields["mcpmSource"]["last_updated"],
        "2026-01-02T03:04:05Z"
    );
    let again = run(&env, &mut entries, &Options::new(Mode::Check)).unwrap();
    assert_eq!(again.servers[0].status, Status::UpToDate);
}

#[test]
fn post_update_runs_only_when_explicitly_allowed() {
    for allow in [false, true] {
        let r = repos(if allow { "post-allow" } else { "post-deny" });
        commit(&r.b, "two.txt", "2");
        git(&r.b, &["push", "-q", "origin", "HEAD:main"]);
        let shell = MockShell::default();
        let env = sys_env(&shell);
        let mut entries = vec![git_entry(&r.a, Some("make build"))];
        let mut opts = Options::new(Mode::Apply);
        opts.allow_commands = allow;
        let report = run(&env, &mut entries, &opts).unwrap();
        let calls = shell.calls.lock().unwrap();
        assert_eq!(report.servers[0].status, Status::Updated);
        if allow {
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, "make build");
            assert_eq!(calls[0].1, r.a);
        } else {
            assert!(calls.is_empty());
            assert!(report.servers[0]
                .steps
                .iter()
                .any(|s| s.name == "post_update" && !s.ok));
        }
    }
}

#[test]
fn failed_post_update_reports_error_but_keeps_the_pull() {
    let r = repos("post-fail");
    commit(&r.b, "two.txt", "2");
    git(&r.b, &["push", "-q", "origin", "HEAD:main"]);
    let shell = MockShell::default();
    *shell.exit.lock().unwrap() = 3;
    let env = sys_env(&shell);
    let mut entries = vec![git_entry(&r.a, Some("make build"))];
    let mut opts = Options::new(Mode::Apply);
    opts.allow_commands = true;
    let report = run(&env, &mut entries, &opts).unwrap();
    assert_eq!(report.servers[0].status, Status::Error);
    assert!(report.servers[0].message.contains("run manually"));
    assert!(r.a.join("two.txt").exists());
}

#[test]
fn dirty_and_diverged_repositories_are_skipped() {
    let r = repos("dirty");
    commit(&r.b, "two.txt", "2");
    git(&r.b, &["push", "-q", "origin", "HEAD:main"]);
    std::fs::write(r.a.join("one.txt"), "changed").unwrap();
    let shell = MockShell::default();
    let env = sys_env(&shell);
    let mut entries = vec![git_entry(&r.a, None)];
    let report = run(&env, &mut entries, &Options::new(Mode::Apply)).unwrap();
    assert_eq!(report.servers[0].status, Status::Skipped);
    assert!(report.servers[0].message.contains("uncommitted"));
    assert!(!r.a.join("two.txt").exists());

    git(&r.a, &["checkout", "--", "one.txt"]);
    commit(&r.a, "local.txt", "x");
    let report = run(&env, &mut entries, &Options::new(Mode::Apply)).unwrap();
    assert_eq!(report.servers[0].status, Status::Skipped);
    assert!(report.servers[0].message.contains("diverged"));
}

fn release_entry(target: &Path, current: &str, extra: Value) -> ServerEntry {
    let mut meta = json!({
        "type": "github-release", "path": target.to_str().unwrap(),
        "repo": "owner/tool", "current_version": current,
        "asset_pattern": "tool_{version}_{os}_{arch}"
    });
    for (k, v) in extra.as_object().unwrap() {
        meta[k] = v.clone();
    }
    entry(json!({"name": "rel", "command": target.to_str().unwrap(), "mcpmSource": meta}))
}

fn release_json(version: &str, assets: &[&str]) -> String {
    let list: Vec<Value> = assets
        .iter()
        .map(|n| json!({"name": n, "browser_download_url": format!("https://dl.example.invalid/{n}")}))
        .collect();
    json!({"tag_name": format!("v{version}"), "assets": list}).to_string()
}

fn sha_hex(bytes: &[u8]) -> String {
    crate::plus::hashing::hex(&Sha256::digest(bytes))
}

const LATEST: &str = "https://api.example.invalid/repos/owner/tool/releases/latest";

struct ReleaseWorld {
    tmp: Tmp,
    target: PathBuf,
    http: MockHttp,
    shell: MockShell,
}

fn release_world(tag: &str) -> ReleaseWorld {
    let tmp = Tmp::new(tag);
    let target = tmp.path("bin/tool");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "old-binary").unwrap();
    ReleaseWorld {
        tmp,
        target,
        http: MockHttp::default(),
        shell: MockShell::default(),
    }
}

fn run_release(w: &ReleaseWorld, e: ServerEntry, opts: Options) -> (Report, ServerEntry) {
    let env = env_with(&w.http, &w.shell, Box::new(SystemGit));
    let mut entries = vec![e];
    let report = run(&env, &mut entries, &opts).unwrap();
    (report, entries.remove(0))
}

fn leftovers(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn release_update_verifies_checksum_and_replaces_atomically() {
    let w = release_world("rel-ok");
    let new = b"new-binary-bytes";
    let asset = "tool_1.2.0_linux_amd64";
    w.http.text(
        LATEST,
        &release_json(
            "1.2.0",
            &[asset, "checksums.txt", "tool_1.2.0_darwin_arm64"],
        ),
    );
    w.http
        .blob(&format!("https://dl.example.invalid/{asset}"), new);
    w.http.text(
        "https://dl.example.invalid/checksums.txt",
        &format!(
            "{}  {asset}\n{}  tool_1.2.0_darwin_arm64\n",
            sha_hex(new),
            "0".repeat(64)
        ),
    );
    let (check, _) = run_release(
        &w,
        release_entry(&w.target, "1.0.0", json!({})),
        Options::new(Mode::Check),
    );
    assert_eq!(check.servers[0].status, Status::UpdateAvailable);
    assert_eq!(std::fs::read(&w.target).unwrap(), b"old-binary");

    let (done, e) = run_release(
        &w,
        release_entry(&w.target, "1.0.0", json!({})),
        Options::new(Mode::Apply),
    );
    assert_eq!(
        done.servers[0].status,
        Status::Updated,
        "{:?}",
        done.servers[0]
    );
    assert_eq!(std::fs::read(&w.target).unwrap(), new);
    assert_eq!(e.unknown_fields["mcpmSource"]["current_version"], "1.2.0");
    assert_eq!(leftovers(w.target.parent().unwrap()), vec!["tool"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert!(std::fs::metadata(&w.target).unwrap().permissions().mode() & 0o111 != 0);
    }
    let (again, _) = run_release(
        &w,
        release_entry(&w.target, "1.2.0", json!({})),
        Options::new(Mode::Check),
    );
    assert_eq!(again.servers[0].status, Status::UpToDate);
}

#[test]
fn release_checksum_mismatch_keeps_the_old_binary() {
    let w = release_world("rel-bad");
    let asset = "tool_1.2.0_linux_amd64";
    w.http.text(
        LATEST,
        &release_json("1.2.0", &[asset, &format!("{asset}.sha256")]),
    );
    w.http
        .blob(&format!("https://dl.example.invalid/{asset}"), b"tampered");
    w.http.text(
        &format!("https://dl.example.invalid/{asset}.sha256"),
        &"a".repeat(64),
    );
    let (done, e) = run_release(
        &w,
        release_entry(&w.target, "1.0.0", json!({})),
        Options::new(Mode::Apply),
    );
    assert_eq!(done.servers[0].status, Status::Error);
    assert!(done.servers[0].message.contains("checksum mismatch"));
    assert_eq!(std::fs::read(&w.target).unwrap(), b"old-binary");
    assert_eq!(e.unknown_fields["mcpmSource"]["current_version"], "1.0.0");
    assert_eq!(leftovers(w.target.parent().unwrap()), vec!["tool"]);
}

#[test]
fn release_without_checksum_needs_explicit_allow() {
    let w = release_world("rel-nosum");
    let asset = "tool_1.2.0_linux_amd64";
    w.http.text(LATEST, &release_json("1.2.0", &[asset]));
    w.http
        .blob(&format!("https://dl.example.invalid/{asset}"), b"fresh");
    let (refused, _) = run_release(
        &w,
        release_entry(&w.target, "1.0.0", json!({})),
        Options::new(Mode::Apply),
    );
    assert_eq!(refused.servers[0].status, Status::Error);
    assert!(refused.servers[0].message.contains("--allow-unverified"));
    assert_eq!(std::fs::read(&w.target).unwrap(), b"old-binary");
    let mut opts = Options::new(Mode::Apply);
    opts.allow_unverified = true;
    let (done, _) = run_release(&w, release_entry(&w.target, "1.0.0", json!({})), opts);
    assert_eq!(done.servers[0].status, Status::Updated);
    assert_eq!(std::fs::read(&w.target).unwrap(), b"fresh");
}

#[test]
fn release_verify_command_is_gated_and_failure_restores() {
    let w = release_world("rel-verify");
    let asset = "tool_1.2.0_linux_amd64";
    w.http.text(
        LATEST,
        &release_json("1.2.0", &[asset, &format!("{asset}.sha256")]),
    );
    w.http
        .blob(&format!("https://dl.example.invalid/{asset}"), b"v2");
    w.http.text(
        &format!("https://dl.example.invalid/{asset}.sha256"),
        &sha_hex(b"v2"),
    );
    let e = || {
        release_entry(
            &w.target,
            "1.0.0",
            json!({"verify_command": "./tool --version"}),
        )
    };

    let (done, _) = run_release(&w, e(), Options::new(Mode::Apply));
    assert_eq!(done.servers[0].status, Status::Updated);
    assert!(w.shell.calls.lock().unwrap().is_empty());
    std::fs::write(&w.target, "old-binary").unwrap();

    *w.shell.exit.lock().unwrap() = 1;
    let mut opts = Options::new(Mode::Apply);
    opts.allow_commands = true;
    let (failed, _) = run_release(&w, e(), opts);
    assert_eq!(failed.servers[0].status, Status::Error);
    assert!(failed.servers[0]
        .message
        .contains("restored the previous version"));
    assert_eq!(std::fs::read(&w.target).unwrap(), b"old-binary");
    assert_eq!(w.shell.calls.lock().unwrap().len(), 1);
    assert_eq!(leftovers(w.target.parent().unwrap()), vec!["tool"]);
}

#[test]
fn release_extracts_a_tar_archive() {
    let w = release_world("rel-tar");
    let stage = w.tmp.path("stage");
    std::fs::create_dir_all(stage.join("pkg")).unwrap();
    std::fs::write(stage.join("pkg/tool"), "from-archive").unwrap();
    std::fs::write(stage.join("pkg/README"), "docs").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            stage.join("pkg/tool"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let tarball = w.tmp.path("a.tar.gz");
    let status = Command::new("tar")
        .args([
            "-czf",
            tarball.to_str().unwrap(),
            "-C",
            stage.to_str().unwrap(),
            "pkg",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let bytes = std::fs::read(&tarball).unwrap();
    let asset = "tool_1.2.0_linux_amd64.tar.gz";
    w.http
        .text(LATEST, &release_json("1.2.0", &[asset, "checksums.txt"]));
    w.http
        .blob(&format!("https://dl.example.invalid/{asset}"), &bytes);
    w.http.text(
        "https://dl.example.invalid/checksums.txt",
        &format!("{}  {asset}\n", sha_hex(&bytes)),
    );
    let (done, _) = run_release(
        &w,
        release_entry(&w.target, "1.0.0", json!({})),
        Options::new(Mode::Apply),
    );
    assert_eq!(
        done.servers[0].status,
        Status::Updated,
        "{:?}",
        done.servers[0]
    );
    assert_eq!(std::fs::read(&w.target).unwrap(), b"from-archive");
}

#[test]
fn release_errors_are_mapped_and_tokens_stay_off_downloads() {
    let w = release_world("rel-err");
    w.http.fail(LATEST, 403);
    let (r, _) = run_release(
        &w,
        release_entry(&w.target, "1.0.0", json!({})),
        Options::new(Mode::Check),
    );
    assert!(r.servers[0].message.contains("rate limited"));
    w.http.fail(LATEST, 404);
    let (r, _) = run_release(
        &w,
        release_entry(&w.target, "1.0.0", json!({})),
        Options::new(Mode::Check),
    );
    assert!(r.servers[0].message.contains("repo not found"));
    w.http
        .text(LATEST, &release_json("1.2.0", &["other_asset"]));
    let (r, _) = run_release(
        &w,
        release_entry(&w.target, "1.0.0", json!({})),
        Options::new(Mode::Check),
    );
    assert!(r.servers[0].message.contains("no asset matching"));
    w.http.text(LATEST, &release_json("nightly", &["x"]));
    let (r, _) = run_release(
        &w,
        release_entry(&w.target, "1.0.0", json!({})),
        Options::new(Mode::Check),
    );
    assert!(r.servers[0].message.contains("could not parse version"));
    let (r, _) = run_release(
        &w,
        release_entry(&w.target, "1.0.0", json!({"repo": "../etc"})),
        Options::new(Mode::Check),
    );
    assert!(r.servers[0].message.contains("invalid repo"));

    let mut env = env_with(&w.http, &w.shell, Box::new(SystemGit));
    env.github_token = Some(FAKE_TOKEN.into());
    w.http.text(LATEST, &release_json("1.0.0", &["x"]));
    let mut entries = vec![release_entry(&w.target, "1.0.0", json!({}))];
    let report = run(&env, &mut entries, &Options::new(Mode::Check)).unwrap();
    assert!(!report.to_value().to_string().contains(FAKE_TOKEN));
    let requests = w.http.requests.lock().unwrap();
    let last = requests.last().unwrap();
    assert!(last
        .1
        .iter()
        .any(|(k, v)| k == "Authorization" && v.contains(FAKE_TOKEN)));
}

#[test]
fn archive_entries_that_escape_are_unsafe() {
    use super::release::expected_hash;
    assert!(expected_hash("abc", "x").is_none());
    let h = "b".repeat(64);
    assert_eq!(
        expected_hash(&format!("{h} *x\n"), "x").as_deref(),
        Some(h.as_str())
    );
    assert_eq!(
        expected_hash(&format!("{h}\n"), "anything").as_deref(),
        Some(h.as_str())
    );
    assert!(expected_hash(&format!("{h}  y\n"), "x").is_none());
}

#[test]
fn npx_pin_update_rewrites_the_registry_args() {
    let http = MockHttp::default();
    http.text(
        "https://npm.example.invalid/@scope%2fpkg/latest",
        r#"{"version":"1.4.0"}"#,
    );
    let shell = MockShell::default();
    let env = env_with(&http, &shell, Box::new(SystemGit));
    let mut entries = vec![entry(
        json!({"name": "n", "command": "npx", "args": ["-y", "@scope/pkg@1.2.3"]}),
    )];
    let check = run(&env, &mut entries, &Options::new(Mode::Check)).unwrap();
    assert_eq!(check.servers[0].status, Status::UpdateAvailable);
    assert!(check.servers[0].detected);
    assert_eq!(entries[0].args[1], "@scope/pkg@1.2.3");
    let done = run(&env, &mut entries, &Options::new(Mode::Apply)).unwrap();
    assert_eq!(done.servers[0].status, Status::Updated);
    assert_eq!(entries[0].args[1], "@scope/pkg@1.4.0");
    let again = run(&env, &mut entries, &Options::new(Mode::Check)).unwrap();
    assert_eq!(again.servers[0].status, Status::UpToDate);
}

#[test]
fn uvx_pin_uses_the_pypi_index_and_unpinned_is_informational() {
    let http = MockHttp::default();
    http.text(
        "https://pypi.example.invalid/pypi/tool/json",
        r#"{"info":{"version":"0.5.0"}}"#,
    );
    let env = env_with(&http, &MockShell::default(), Box::new(SystemGit));
    let mut entries = vec![
        entry(
            json!({"name": "u", "command": "uvx", "args": ["--from", "tool==0.4.1", "tool-cli"]}),
        ),
        entry(json!({"name": "free", "command": "uvx", "args": ["tool"]})),
    ];
    let done = run(&env, &mut entries, &Options::new(Mode::Apply)).unwrap();
    assert_eq!(done.servers[0].status, Status::Updated);
    assert_eq!(entries[0].args[1], "tool==0.5.0");
    assert_eq!(done.servers[1].status, Status::Auto);
}

#[test]
fn registry_lookup_failures_surface_as_errors() {
    let http = MockHttp::default();
    http.fail("https://npm.example.invalid/pkg/latest", 500);
    let env = env_with(&http, &MockShell::default(), Box::new(SystemGit));
    let mut entries = vec![entry(
        json!({"name": "n", "command": "npx", "args": ["pkg@1.0.0"]}),
    )];
    let r = run(&env, &mut entries, &Options::new(Mode::Check)).unwrap();
    assert_eq!(r.servers[0].status, Status::Error);
}

#[test]
fn init_detects_stores_and_respects_force() {
    let r = repos("init");
    std::fs::write(r.a.join("package.json"), r#"{"scripts":{"build":"tsc"}}"#).unwrap();
    let env = env_with(
        &MockHttp::default(),
        &MockShell::default(),
        Box::new(SystemGit),
    );
    let mut entries = vec![
        entry(
            json!({"name": "g", "command": "node", "args": [r.a.join("x.js").to_str().unwrap()]}),
        ),
        entry(json!({"name": "p", "command": "npx", "args": ["pkg"]})),
    ];
    let mut dry = Options::new(Mode::Init);
    dry.dry_run = true;
    let report = run(&env, &mut entries, &dry).unwrap();
    assert!(report.servers[0].message.contains("would configure"));
    assert!(entries[0].unknown_fields.get("mcpmSource").is_none());

    let report = run(&env, &mut entries, &Options::new(Mode::Init)).unwrap();
    assert_eq!(report.count("configured"), 2);
    let meta = &entries[0].unknown_fields["mcpmSource"];
    assert_eq!(meta["type"], "git");
    assert_eq!(meta["branch"], "main");
    assert_eq!(meta["remote"], "origin");
    assert_eq!(meta["post_update"], "npm install && npm run build");
    assert!(meta["upstream"].is_null());
    assert_eq!(entries[1].unknown_fields["mcpmSource"]["package"], "pkg");

    let again = run(&env, &mut entries, &Options::new(Mode::Init)).unwrap();
    assert_eq!(again.count("skipped"), 2);
    let mut forced = Options::new(Mode::Init);
    forced.force = true;
    assert_eq!(
        run(&env, &mut entries, &forced)
            .unwrap()
            .count("configured"),
        2
    );
}

#[test]
fn init_can_tag_a_release_repo() {
    let env = env_with(
        &MockHttp::default(),
        &MockShell::default(),
        Box::new(SystemGit),
    );
    let tmp = Tmp::new("init-rel");
    let home = tmp.path("home");
    std::fs::create_dir_all(home.join(".local/bin")).unwrap();
    let bin = home.join(".local/bin/tool");
    std::fs::write(&bin, "").unwrap();
    let mut env = env;
    env.home = Some(home);
    let mut entries = vec![entry(
        json!({"name": "t", "command": bin.to_str().unwrap()}),
    )];
    let mut opts = Options::new(Mode::Init);
    opts.repo = Some("owner/tool".into());
    run(&env, &mut entries, &opts).unwrap();
    assert_eq!(
        entries[0].unknown_fields["mcpmSource"]["repo"],
        "owner/tool"
    );
}

#[test]
fn unknown_server_filter_is_an_error() {
    let env = env_with(
        &MockHttp::default(),
        &MockShell::default(),
        Box::new(SystemGit),
    );
    let mut opts = Options::new(Mode::Check);
    opts.server = Some("missing".into());
    assert!(run(&env, &mut [], &opts).unwrap_err().contains("not found"));
}

struct Fixture {
    base: crate::plus::testutil::DataDirFx,
}

impl Fixture {
    fn new(tag: &str, servers: Value) -> Self {
        let base = crate::plus::testutil::DataDirFx::new("plus-update-reg", tag);
        base.write_registry(&json!({"version": 1, "servers": servers, "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}], "activeProfileId": "default"}));
        Self { base }
    }

    fn servers(&self) -> Value {
        let text = std::fs::read_to_string(self.base.dir.join("registry.json")).unwrap();
        serde_json::from_str::<Value>(&text).unwrap()["servers"].clone()
    }
}

#[test]
fn execute_persists_only_on_apply_and_init() {
    let fx = Fixture::new(
        "persist",
        json!([{"id": "n", "name": "n", "transport": "stdio", "command": "npx", "args": ["-y", "pkg@1.0.0"]},
               {"id": "keep", "name": "keep", "transport": "http", "url": "https://example.invalid/mcp"}]),
    );
    let http = MockHttp::default();
    http.text(
        "https://npm.example.invalid/pkg/latest",
        r#"{"version":"2.0.0"}"#,
    );
    let env = env_with(&http, &MockShell::default(), Box::new(SystemGit));
    let before = fx.servers();

    execute_with(&env, &Options::new(Mode::Check)).unwrap();
    execute_with(&env, &Options::new(Mode::DryRun)).unwrap();
    assert_eq!(fx.servers(), before);

    let report = execute_with(&env, &Options::new(Mode::Apply)).unwrap();
    assert_eq!(report.count("updated"), 1);
    let after = fx.servers();
    assert_eq!(after[0]["args"][1], "pkg@2.0.0");
    assert!(after[1].get("mcpmSource").is_none());

    let report = execute_with(&env, &Options::new(Mode::Init)).unwrap();
    assert_eq!(report.count("configured"), 2);
    assert_eq!(fx.servers()[0]["mcpmSource"]["type"], "npx");
}

#[test]
fn handlers_and_ctl_are_wired() {
    let fx = Fixture::new(
        "wired",
        json!([{"id": "r", "name": "r", "transport": "http", "url": "https://example.invalid/mcp"}]),
    );
    let value = crate::plus::dispatch("plus.update.check", json!({})).unwrap();
    assert_eq!(value["mode"], "check");
    assert_eq!(value["servers"][0]["status"], "skipped");
    let value =
        crate::plus::dispatch("plus.update.apply", json!({"dryRun": true, "server": "r"})).unwrap();
    assert_eq!(value["mode"], "dry-run");
    assert!(crate::plus::dispatch("plus.update.apply", json!({"server": "nope"})).is_err());

    let run_ctl = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let code = crate::plus::ctl::run_with(&args, &mut o, &mut e);
        (
            code,
            String::from_utf8(o).unwrap(),
            String::from_utf8(e).unwrap(),
        )
    };
    let (code, out, _) = run_ctl(&["--json", "update", "--check"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["command"], "update");
    assert_eq!(v["data"]["servers"][0]["id"], "r");
    let (code, _, err) = run_ctl(&["update", "--check", "--apply"]);
    assert_eq!(code, 2, "{err}");
    let (code, out, _) = run_ctl(&["update", "r", "--dry-run"]);
    assert_eq!(code, 0);
    assert!(out.contains("dry run"));
    drop(fx);
}

#[test]
fn now_iso_formats_known_instants() {
    assert_eq!(now_iso().len(), 20);
    assert!(now_iso().ends_with('Z'));
}

fn serial_reference(env: &Env, entries: &mut [ServerEntry], opts: &Options) -> Report {
    let reports = entries
        .iter_mut()
        .filter(|e| {
            opts.server
                .as_ref()
                .is_none_or(|w| &e.id == w || &e.name == w)
        })
        .map(|e| {
            let (src, from_meta) = source::effective(e, env.home.as_deref(), env.git.as_ref());
            check_planned(env, opts, (e, src, from_meta))
        })
        .collect();
    Report::new(opts.mode, reports)
}

fn mixed_entries(release_target: &Path) -> Vec<ServerEntry> {
    let git_meta =
        json!({"type": "git", "path": release_target.parent().unwrap().to_str().unwrap()});
    let pinned = |name: &str, spec: &str| {
        entry(json!({"name": name, "id": name, "command": "npx", "args": ["-y", spec]}))
    };
    vec![
        pinned("n0", "pkg0@1.0.0"),
        entry(json!({"name": "remote", "transport": "http", "url": "https://example.invalid/mcp"})),
        pinned("n1", "pkg1@2.0.0"),
        entry(json!({"name": "mystery", "command": "mystery"})),
        release_entry(release_target, "1.0.0", json!({})),
        pinned("n2", "missing@1.0.0"),
        entry(json!({"name": "g", "command": "node", "args": ["x.js"], "mcpmSource": git_meta})),
        entry(
            json!({"name": "u0", "command": "uvx", "args": ["--from", "tool==0.4.1", "tool-cli"]}),
        ),
        entry(json!({"name": "free", "command": "npx", "args": ["unpinned"]})),
        pinned("n3", "pkg3@3.0.0"),
        entry(json!({"name": "u1", "command": "uvx", "args": ["--from", "gone==1.0", "gone-cli"]})),
        pinned("n4", "pkg4@4.0.0"),
    ]
}

fn mixed_world(tag: &str) -> (ReleaseWorld, Env) {
    let w = release_world(tag);
    for (n, v) in [
        ("pkg0", "1.1.0"),
        ("pkg1", "2.0.0"),
        ("pkg3", "3.0.1"),
        ("pkg4", "5.0.0"),
    ] {
        w.http.text(
            &format!("https://npm.example.invalid/{n}/latest"),
            &format!(r#"{{"version":"{v}"}}"#),
        );
    }
    w.http.text(
        "https://pypi.example.invalid/pypi/tool/json",
        r#"{"info":{"version":"0.5.0"}}"#,
    );
    w.http
        .fail("https://pypi.example.invalid/pypi/gone/json", 500);
    w.http
        .text(LATEST, &release_json("1.2.0", &["tool_1.2.0_linux_amd64"]));
    let runner = ScriptedGit(Box::new(|args| match args[0] {
        "rev-parse" if args.contains(&"--is-inside-work-tree") => out(0, "true\n"),
        "rev-parse" => out(0, "origin/main\n"),
        "branch" => out(0, "main\n"),
        "rev-list" => out(0, "0\t2\n"),
        "log" => out(0, "a1 one\nb2 two\n"),
        "status" | "fetch" => out(0, ""),
        _ => out(1, ""),
    }));
    let env = env_with(&w.http, &w.shell, Box::new(runner));
    (w, env)
}

fn sorted_requests(http: &MockHttp) -> Vec<String> {
    let mut urls: Vec<String> = http
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|(u, _)| u.clone())
        .collect();
    urls.sort();
    urls
}

#[test]
fn parallel_checks_report_exactly_like_a_serial_pass() {
    for mode in [Mode::Check, Mode::DryRun] {
        let (w, env) = mixed_world("par-eq");
        let mut serial = mixed_entries(&w.target);
        let mut pooled = mixed_entries(&w.target);
        let before = serde_json::to_value(&pooled).unwrap();
        let expected = serial_reference(&env, &mut serial, &Options::new(mode));
        let expected_requests = sorted_requests(&w.http);
        w.http.requests.lock().unwrap().clear();
        let got = run(&env, &mut pooled, &Options::new(mode)).unwrap();
        assert_eq!(got.to_value(), expected.to_value(), "{mode:?}");
        assert_eq!(sorted_requests(&w.http), expected_requests, "{mode:?}");
        assert_eq!(serde_json::to_value(&pooled).unwrap(), before, "{mode:?}");
        let ids: Vec<&str> = got.servers.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            ["n0", "remote", "n1", "mystery", "rel", "n2", "g", "u0", "free", "n3", "u1", "n4"]
        );
        let status = |id: &str| got.servers.iter().find(|s| s.id == id).unwrap().status;
        assert_eq!(status("n0"), Status::UpdateAvailable);
        assert_eq!(status("n1"), Status::UpToDate);
        assert_eq!(status("n2"), Status::Error);
        assert_eq!(status("g"), Status::UpdateAvailable);
        assert_eq!(status("u1"), Status::Error);
        assert_eq!(status("remote"), Status::Skipped);
    }
}

#[test]
fn server_filters_select_the_same_entries_when_checks_run_in_parallel() {
    let (w, env) = mixed_world("par-filter");
    for wanted in ["n3", "u0", "mystery", "pkg-none"] {
        let mut a = mixed_entries(&w.target);
        let mut b = mixed_entries(&w.target);
        let mut opts = Options::new(Mode::Check);
        opts.server = Some(wanted.into());
        if wanted == "pkg-none" {
            assert!(run(&env, &mut a, &opts).unwrap_err().contains(wanted));
            continue;
        }
        let expected = serial_reference(&env, &mut a, &opts);
        let got = run(&env, &mut b, &opts).unwrap();
        assert_eq!(got.to_value(), expected.to_value());
        assert_eq!(got.servers.len(), 1);
    }
    let mut twins = vec![
        entry(json!({"name": "dup", "id": "d1", "command": "npx", "args": ["-y", "pkg0@1.0.0"]})),
        entry(json!({"name": "other", "command": "npx", "args": ["-y", "pkg1@1.0.0"]})),
        entry(json!({"name": "dup", "id": "d2", "command": "npx", "args": ["-y", "pkg3@1.0.0"]})),
    ];
    let mut opts = Options::new(Mode::Check);
    opts.server = Some("dup".into());
    let got = run(&env, &mut twins, &opts).unwrap();
    let ids: Vec<&str> = got.servers.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["d1", "d2"]);
}

#[derive(Clone, Default)]
struct SlowHttp {
    live: Arc<std::sync::atomic::AtomicUsize>,
    peak: Arc<std::sync::atomic::AtomicUsize>,
    delay_ms: u64,
    gate: Option<Arc<crate::plus::testutil::Gate>>,
}

impl HttpClient for SlowHttp {
    fn get_text(&self, url: &str, _h: &[(String, String)]) -> Result<String, HttpError> {
        use std::sync::atomic::Ordering::SeqCst;
        let now = self.live.fetch_add(1, SeqCst) + 1;
        self.peak.fetch_max(now, SeqCst);
        if let Some(gate) = &self.gate {
            gate.pass();
        }
        std::thread::sleep(Duration::from_millis(self.delay_ms));
        self.live.fetch_sub(1, SeqCst);
        let name = url.rsplit('/').nth(1).unwrap_or("x");
        Ok(format!(r#"{{"version":"9.{}.0"}}"#, name.len()))
    }

    fn download(&self, _url: &str, _dest: &Path) -> Result<String, HttpError> {
        Err(HttpError::new(Some(404), "no mock"))
    }
}

fn many_pinned(count: usize) -> Vec<ServerEntry> {
    (0..count)
        .map(|i| {
            entry(json!({
                "name": format!("s{i}"), "command": "npx",
                "args": ["-y", format!("pkg{i}@1.0.0")]
            }))
        })
        .collect()
}

fn slow_env(http: &SlowHttp) -> Env {
    let mut env = env_with(
        &MockHttp::default(),
        &MockShell::default(),
        Box::new(SystemGit),
    );
    env.http = Box::new(http.clone());
    env
}

#[test]
fn the_check_pool_is_bounded_keeps_order_and_overlaps_lookups() {
    use std::sync::atomic::Ordering::SeqCst;
    let gate = crate::plus::testutil::Gate::new(CHECK_WORKERS);
    let http = SlowHttp {
        delay_ms: 40,
        gate: Some(Arc::clone(&gate)),
        ..Default::default()
    };
    let env = slow_env(&http);
    let mut entries = many_pinned(13);
    let report = run(&env, &mut entries, &Options::new(Mode::Check)).unwrap();
    let ids: Vec<String> = report.servers.iter().map(|s| s.id.clone()).collect();
    assert_eq!(ids, (0..13).map(|i| format!("s{i}")).collect::<Vec<_>>());
    assert!(report
        .servers
        .iter()
        .all(|s| s.status == Status::UpdateAvailable));
    assert!(
        !gate.timed_out(),
        "the first {CHECK_WORKERS} lookups never ran at the same time"
    );
    assert_eq!(http.peak.load(SeqCst), CHECK_WORKERS);
}

#[test]
fn apply_and_init_stay_serial_and_a_single_lookup_skips_the_pool() {
    use std::sync::atomic::Ordering::SeqCst;
    let http = SlowHttp {
        delay_ms: 10,
        ..Default::default()
    };
    let env = slow_env(&http);
    let mut entries = many_pinned(6);
    let done = run(&env, &mut entries, &Options::new(Mode::Apply)).unwrap();
    assert!(done.servers.iter().all(|s| s.status == Status::Updated));
    assert_eq!(http.peak.load(SeqCst), 1);
    assert_eq!(entries[3].args[1], "pkg3@9.4.0");

    let http = SlowHttp {
        delay_ms: 10,
        ..Default::default()
    };
    let env = slow_env(&http);
    let mut one = many_pinned(1);
    one.push(entry(
        json!({"name": "remote", "transport": "http", "url": "https://example.invalid/mcp"}),
    ));
    let report = run(&env, &mut one, &Options::new(Mode::Check)).unwrap();
    assert_eq!(report.servers.len(), 2);
    assert_eq!(http.peak.load(SeqCst), 1);
}
