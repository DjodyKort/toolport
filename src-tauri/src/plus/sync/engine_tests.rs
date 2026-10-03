use super::engine::*;
use super::exec::{Exec, SystemExec};
use super::gitsync::{git_sync, GitSyncOptions};
use super::origins::{detect_origins, resolve_servers, ResolveOptions};
use super::schema::ServerOrigins;
use super::*;
use crate::plus::skills::clock::{FixedClock, Instant};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PASS: &str = "correct horse battery";
const CLOCK: FixedClock = FixedClock(Instant {
    unix_secs: 1_790_000_000,
    micros: 0,
});

struct Lab {
    base: PathBuf,
}

impl Lab {
    fn new(label: &str) -> Self {
        let base =
            std::env::temp_dir().join(format!("toolport-sync2-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        Lab { base }
    }

    fn remote(&self, head: &str) -> String {
        let path = self.base.join("remote.git");
        let out = Command::new("git")
            .args(["init", "--bare", "--quiet", "-b", head])
            .arg(&path)
            .output()
            .unwrap();
        assert!(out.status.success());
        path.to_string_lossy().into_owned()
    }

    fn machine(&self, name: &str) -> Machine {
        let home = self.base.join(name).join("home");
        let config = home.join(".config/mcpm");
        fs::create_dir_all(&config).unwrap();
        Machine {
            home,
            config,
            state: self.base.join(name).join("state"),
        }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

struct Machine {
    home: PathBuf,
    config: PathBuf,
    state: PathBuf,
}

impl Machine {
    fn ctx<'a>(&self, exec: &'a dyn Exec) -> SyncContext<'a> {
        SyncContext {
            config_dir: self.config.clone(),
            state_dir: self.state.clone(),
            roots: PortableRoots {
                home: self.home.to_string_lossy().into_owned(),
                mcpm_home: self.config.to_string_lossy().into_owned(),
            },
            clock: &CLOCK,
            exec,
        }
    }

    fn write(&self, rel: &str, bytes: &[u8]) {
        let path = self.config.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn read(&self, rel: &str) -> Vec<u8> {
        fs::read(self.config.join(rel)).unwrap()
    }

    fn seed(&self) {
        let servers = format!(
            r#"{{"alpha":{{"command":"node","args":["{0}/srv/index.js","{1}/data"]}},"beta":{{"url":"https://example.invalid/mcp"}},"gamma":{{"command":"npx","args":["-y","pkg"]}}}}"#,
            self.home.display(),
            self.config.display()
        );
        self.write("servers.json", servers.as_bytes());
        self.write(
            "skills_repo/skills/demo/SKILL.md",
            "---\nname: demo\n---\nbody \u{e9}\n".as_bytes(),
        );
        self.write(
            "skills_repo/skills/demo/blob.bin",
            &[0, 159, 146, 150, 255, 1],
        );
        self.write("skills_repo/skills/demo/.hidden", b"nope");
        self.write("skills_repo/lib/helper.py", b"print('lib')\n");
        self.write("skills_repo/mcpm-skills.yaml", b"skills: []\n");
        self.write("skills_repo/.git/config", b"never");
        self.write("../../srv/index.js", b"//");
        self.write("bin/tool", b"#!/bin/sh\necho tool\n");
        self.write("keys/id_synthetic", b"synthetic-private-key-material");
        self.write("skills_repo/keys/also-not", b"x");
    }
}

fn init_opts<'a>(repo: &'a str, passphrase: &'a str) -> InitOptions<'a> {
    InitOptions {
        repo,
        branch: "main",
        machine_id: Some("m"),
        passphrase,
        reconfigure: false,
    }
}

fn pull_opts() -> PullOptions {
    PullOptions::default()
}

fn remote_keys(m: &Machine) -> Vec<String> {
    let manifest = bundle::read_manifest(&m.state.join("sync_repo")).unwrap();
    manifest.entries.keys().cloned().collect()
}

#[test]
fn bundle_scope_carries_bin_and_lib_never_keys() {
    let lab = Lab::new("scope");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    a.seed();
    let ctx = a.ctx(&SystemExec);
    init(&ctx, &init_opts(&remote, PASS)).unwrap();
    let report = push(&ctx, &PushOptions::default()).unwrap();
    assert!(report.pushed && report.committed);
    let keys = remote_keys(&a);
    assert_eq!(
        keys,
        vec![
            "bin/tool",
            "global/server_origins.json",
            "global/servers.json",
            "skills_repo/lib/helper.py",
            "skills_repo/mcpm-skills.yaml",
            "skills_repo/skills/demo/SKILL.md",
            "skills_repo/skills/demo/blob.bin",
        ]
    );
    assert!(keys
        .iter()
        .all(|k| !k.contains("keys") && !k.contains("hidden")));
    let blobs: Vec<String> = fs::read_dir(a.state.join("sync_repo/blobs"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(blobs.iter().all(|b| !b.contains("keys")));
    let on_disk = fs::read_to_string(a.state.join("sync_repo/sync_manifest.json")).unwrap();
    assert!(!on_disk.contains(a.home.to_str().unwrap()));
}

#[test]
fn two_machines_round_trip_with_portable_paths() {
    let lab = Lab::new("roundtrip");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    let b = lab.machine("b");
    a.seed();
    init(&a.ctx(&SystemExec), &init_opts(&remote, PASS)).unwrap();
    push(&a.ctx(&SystemExec), &PushOptions::default()).unwrap();

    let ctx_b = b.ctx(&SystemExec);
    let report = init(&ctx_b, &init_opts(&remote, PASS)).unwrap();
    assert!(!report.fresh_remote);
    let pulled = pull(
        &ctx_b,
        &PullOptions {
            resolve: true,
            ..pull_opts()
        },
    )
    .unwrap();
    assert_eq!(pulled.applied.len(), 6, "{pulled:?}");
    assert!(pulled.conflicts.is_empty());

    let servers = String::from_utf8(b.read("servers.json")).unwrap();
    assert!(servers.contains(&format!("{}/srv/index.js", b.home.display())));
    assert!(servers.contains(&format!("{}/data", b.config.display())));
    assert!(!servers.contains(a.home.to_str().unwrap()));
    assert_eq!(
        b.read("skills_repo/skills/demo/blob.bin"),
        vec![0, 159, 146, 150, 255, 1]
    );
    assert_eq!(b.read("skills_repo/lib/helper.py"), b"print('lib')\n");
    assert_eq!(b.read("bin/tool"), b"#!/bin/sh\necho tool\n");
    assert!(!b.config.join("keys").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(b.config.join("bin/tool"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
        let key_mode = fs::metadata(b.state.join("sync_keyfile"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(key_mode & 0o777, 0o600);
    }
    let origins: ServerOrigins = serde_json::from_str(
        &fs::read_to_string(b.state.join("sync_repo/blobs/global__server_origins.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(origins.servers["beta"].origin_type, "remote");
    assert_eq!(origins.servers["gamma"].origin_type, "npx");
    assert_eq!(origins.servers["alpha"].origin_type, "manual");
    assert_eq!(
        origins.servers["alpha"].install_path.as_deref(),
        Some("${HOME}/srv")
    );

    let diff_b = diff(&ctx_b).unwrap();
    assert!(diff_b.changes.is_clean(), "{diff_b:?}");
    assert_eq!(diff_b.changes.unchanged.len(), 7);
}

#[test]
fn wrong_passphrase_is_rejected_by_init() {
    let lab = Lab::new("wrongpass");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    let b = lab.machine("b");
    a.seed();
    init(&a.ctx(&SystemExec), &init_opts(&remote, PASS)).unwrap();
    push(&a.ctx(&SystemExec), &PushOptions::default()).unwrap();
    let err = init(
        &b.ctx(&SystemExec),
        &init_opts(&remote, "a different passphrase"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("does not match"), "{err}");
    assert!(!b.state.join("sync_keyfile").exists());
    assert!(!b.state.join("sync.json").exists());
    let short = init(&b.ctx(&SystemExec), &init_opts(&remote, "short")).unwrap_err();
    assert!(short.to_string().contains("at least 8"));
}

#[test]
fn tampered_remote_blob_aborts_pull_before_writing() {
    let lab = Lab::new("tamper");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    let b = lab.machine("b");
    a.seed();
    init(&a.ctx(&SystemExec), &init_opts(&remote, PASS)).unwrap();
    push(&a.ctx(&SystemExec), &PushOptions::default()).unwrap();
    let ctx_b = b.ctx(&SystemExec);
    init(&ctx_b, &init_opts(&remote, PASS)).unwrap();
    let blob = b
        .state
        .join("sync_repo/blobs/skills_repo__lib__helper.py.enc");
    let mut bytes = fs::read(&blob).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] = if bytes[mid] == b'A' { b'B' } else { b'A' };
    fs::write(&blob, bytes).unwrap();
    let err = pull(&ctx_b, &pull_opts()).unwrap_err();
    assert!(matches!(err, SyncError::Crypto(_)), "{err}");
    assert!(!b.config.join("servers.json").exists());
    assert!(!b.state.join("sync_state.json").exists());
}

#[test]
fn three_way_conflict_saves_remote_copy_and_force_overrides() {
    let lab = Lab::new("conflict");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    let b = lab.machine("b");
    a.seed();
    init(&a.ctx(&SystemExec), &init_opts(&remote, PASS)).unwrap();
    push(&a.ctx(&SystemExec), &PushOptions::default()).unwrap();
    let ctx_b = b.ctx(&SystemExec);
    init(&ctx_b, &init_opts(&remote, PASS)).unwrap();
    pull(&ctx_b, &pull_opts()).unwrap();

    b.write("skills_repo/lib/helper.py", b"print('b edit')\n");
    b.write("skills_repo/skills/demo/SKILL.md", b"b only edit\n");
    a.write("skills_repo/lib/helper.py", b"print('a edit')\n");
    push(&a.ctx(&SystemExec), &PushOptions::default()).unwrap();

    let dry = diff(&ctx_b).unwrap();
    assert_eq!(dry.changes.conflicts, vec!["skills_repo/lib/helper.py"]);
    let preview = pull(
        &ctx_b,
        &PullOptions {
            dry_run: true,
            ..pull_opts()
        },
    )
    .unwrap();
    assert_eq!(
        preview.changes.unwrap().conflicts,
        vec!["skills_repo/lib/helper.py"]
    );
    assert_eq!(b.read("skills_repo/lib/helper.py"), b"print('b edit')\n");

    let pulled = pull(&ctx_b, &pull_opts()).unwrap();
    assert_eq!(pulled.conflicts.len(), 1);
    let conflict = &pulled.conflicts[0];
    assert_eq!(conflict.entry_key, "skills_repo/lib/helper.py");
    assert_ne!(conflict.local_hash, conflict.remote_hash);
    assert_eq!(b.read("skills_repo/lib/helper.py"), b"print('b edit')\n");
    assert_eq!(
        b.read("skills_repo/lib/helper.py.remote"),
        b"print('a edit')\n"
    );
    assert_eq!(pulled.kept_local, vec!["skills_repo/skills/demo/SKILL.md"]);
    assert_eq!(b.read("skills_repo/skills/demo/SKILL.md"), b"b only edit\n");

    let forced = pull(
        &ctx_b,
        &PullOptions {
            force: true,
            ..pull_opts()
        },
    )
    .unwrap();
    assert!(forced.conflicts.is_empty());
    assert_eq!(b.read("skills_repo/lib/helper.py"), b"print('a edit')\n");
    assert!(forced
        .applied
        .contains(&"skills_repo/skills/demo/SKILL.md".to_string()));
}

#[test]
fn rotate_passphrase_reencrypts_and_old_key_stops_working() {
    let lab = Lab::new("rotate");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    let b = lab.machine("b");
    a.seed();
    let ctx_a = a.ctx(&SystemExec);
    init(&ctx_a, &init_opts(&remote, PASS)).unwrap();
    push(&ctx_a, &PushOptions::default()).unwrap();
    let ctx_b = b.ctx(&SystemExec);
    init(&ctx_b, &init_opts(&remote, PASS)).unwrap();
    pull(&ctx_b, &pull_opts()).unwrap();
    let manifest_before = fs::read_to_string(a.state.join("sync_repo/sync_manifest.json")).unwrap();

    let short = rotate_passphrase(&ctx_a, "tiny").unwrap_err();
    assert!(short.to_string().contains("at least 8"));
    let report = rotate_passphrase(&ctx_a, "brand new passphrase").unwrap();
    assert_eq!(report.rotated, 6);
    assert_eq!(
        manifest_before,
        fs::read_to_string(a.state.join("sync_repo/sync_manifest.json")).unwrap()
    );

    let stale = pull(&ctx_b, &pull_opts()).unwrap_err();
    assert!(stale.to_string().contains("cannot decrypt"), "{stale}");
    let relogin = init(
        &ctx_b,
        &InitOptions {
            reconfigure: true,
            ..init_opts(&remote, PASS)
        },
    )
    .unwrap_err();
    assert!(relogin.to_string().contains("does not match"));
    init(
        &ctx_b,
        &InitOptions {
            reconfigure: true,
            ..init_opts(&remote, "brand new passphrase")
        },
    )
    .unwrap();
    a.write("skills_repo/lib/helper.py", b"after rotate\n");
    push(&ctx_a, &PushOptions::default()).unwrap();
    let pulled = pull(&ctx_b, &pull_opts()).unwrap();
    assert!(pulled
        .applied
        .contains(&"skills_repo/lib/helper.py".to_string()));
    assert_eq!(b.read("skills_repo/lib/helper.py"), b"after rotate\n");
}

#[test]
fn failed_rotate_push_keeps_old_key_and_clean_repo() {
    let lab = Lab::new("rotatefail");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    a.seed();
    let ctx = a.ctx(&SystemExec);
    init(&ctx, &init_opts(&remote, PASS)).unwrap();
    push(&ctx, &PushOptions::default()).unwrap();
    let key_before = fs::read_to_string(a.state.join("sync_keyfile")).unwrap();
    fs::remove_dir_all(&remote).unwrap();
    let err = rotate_passphrase(&ctx, "brand new passphrase").unwrap_err();
    assert!(matches!(err, SyncError::Git(_)), "{err}");
    assert_eq!(
        key_before,
        fs::read_to_string(a.state.join("sync_keyfile")).unwrap()
    );
    let status = SystemExec
        .git(Some(&a.state.join("sync_repo")), &["status", "--porcelain"])
        .unwrap();
    assert!(status.trim().is_empty(), "{status}");
    fs::create_dir_all(&remote).unwrap();
    Command::new("git")
        .args(["init", "--bare", "--quiet", "-b", "main"])
        .arg(&remote)
        .output()
        .unwrap();
    assert!(push(&ctx, &PushOptions::default()).is_ok());
}

#[test]
fn projects_sync_by_name_and_unregistered_projects_are_skipped() {
    let lab = Lab::new("projects");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    let b = lab.machine("b");
    a.seed();
    let proj_a = lab.base.join("a/work/app");
    fs::create_dir_all(&proj_a).unwrap();
    fs::write(proj_a.join("CLAUDE.md"), "project rules\n").unwrap();
    fs::write(proj_a.join("NOTES.md"), "notes\n").unwrap();
    let ctx_a = a.ctx(&SystemExec);
    assert!(add_project(&ctx_a, "app", &proj_a, &[]).is_err());
    init(&ctx_a, &init_opts(&remote, PASS)).unwrap();
    assert!(!add_project(
        &ctx_a,
        "app",
        &proj_a,
        &["CLAUDE.md".into(), "NOTES.md".into()]
    )
    .unwrap());
    assert!(add_project(
        &ctx_a,
        "app",
        &proj_a,
        &["CLAUDE.md".into(), "NOTES.md".into()]
    )
    .unwrap());
    assert!(add_project(&ctx_a, "a/b", &proj_a, &[]).is_err());
    assert!(add_project(&ctx_a, "bad", &proj_a, &["../escape".into()]).is_err());
    push(&ctx_a, &PushOptions::default()).unwrap();
    assert!(!remote_keys(&a).iter().any(|k| k.starts_with("projects/")));
    let report = push(
        &ctx_a,
        &PushOptions {
            include_projects: true,
            dry_run: true,
        },
    )
    .unwrap();
    assert!(report
        .entries
        .contains(&"projects/app/NOTES.md".to_string()));
    assert!(!report.pushed);
    push(
        &ctx_a,
        &PushOptions {
            include_projects: true,
            dry_run: false,
        },
    )
    .unwrap();

    let ctx_b = b.ctx(&SystemExec);
    init(&ctx_b, &init_opts(&remote, PASS)).unwrap();
    let skipped = pull(
        &ctx_b,
        &PullOptions {
            include_projects: true,
            ..pull_opts()
        },
    )
    .unwrap();
    assert!(skipped
        .skipped
        .contains(&"projects/app/CLAUDE.md".to_string()));
    let proj_b = lab.base.join("b/elsewhere");
    fs::create_dir_all(&proj_b).unwrap();
    add_project(
        &ctx_b,
        "app",
        &proj_b,
        &["CLAUDE.md".into(), "NOTES.md".into()],
    )
    .unwrap();
    pull(
        &ctx_b,
        &PullOptions {
            include_projects: true,
            ..pull_opts()
        },
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(proj_b.join("CLAUDE.md")).unwrap(),
        "project rules\n"
    );
    assert!(remove_project(&ctx_b, "app").unwrap());
    assert!(!remove_project(&ctx_b, "app").unwrap());
    assert!(status(&ctx_b).projects.is_empty());
}

#[test]
fn status_reset_and_unconfigured_errors() {
    let lab = Lab::new("status");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    a.seed();
    let ctx = a.ctx(&SystemExec);
    assert!(!status(&ctx).configured);
    assert!(push(&ctx, &PushOptions::default()).is_err());
    assert!(reset(&ctx).unwrap().is_empty());
    init(&ctx, &init_opts(&remote, PASS)).unwrap();
    let again = init(&ctx, &init_opts(&remote, PASS)).unwrap_err();
    assert!(again.to_string().contains("already configured"));
    let empty_pull = pull(&ctx, &pull_opts()).unwrap();
    assert!(empty_pull.no_remote);
    push(&ctx, &PushOptions::default()).unwrap();
    let s = status(&ctx);
    assert!(s.configured && s.keyfile_present);
    assert_eq!(s.last_direction, "push");
    assert_eq!(s.last_sync_at, "2026-09-21T14:13:20+00:00");
    assert_eq!(s.tracked, 7);
    let removed = reset(&ctx).unwrap();
    assert_eq!(removed.len(), 4);
    assert!(!status(&ctx).configured);
    assert!(a.config.join("servers.json").exists());
}

#[test]
fn empty_remote_with_other_default_branch_still_works() {
    let lab = Lab::new("trunk");
    let remote = lab.remote("trunk");
    let a = lab.machine("a");
    a.seed();
    let ctx = a.ctx(&SystemExec);
    let report = init(&ctx, &init_opts(&remote, PASS)).unwrap();
    assert!(report.fresh_remote);
    assert!(push(&ctx, &PushOptions::default()).unwrap().committed);
    let again = push(&ctx, &PushOptions::default()).unwrap();
    assert!(again.pushed && !again.committed);
}

#[test]
fn migrate_imports_python_golden_bundle() {
    let lab = Lab::new("migrate");
    let a = lab.machine("a");
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plus/sync/fixtures/old-bundle");
    let ctx = a.ctx(&SystemExec);
    let report = migrate_bundle(
        &ctx,
        &golden,
        Credential::Key("WrGla9wzWbq8AcLDRsyivn44CMMRn7UnEAsT65gp7Pw="),
        false,
    )
    .unwrap();
    assert_eq!(report.written.len(), 6);
    assert!(String::from_utf8(a.read("servers.json"))
        .unwrap()
        .contains(a.home.to_str().unwrap()));
    assert!(migrate_bundle(&ctx, &lab.base, Credential::Key("x"), false).is_err());
}

#[test]
fn rust_written_golden_is_readable_and_stable() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plus/sync/fixtures/rust-bundle");
    let roots = PortableRoots {
        home: "/home/new-machine".into(),
        mcpm_home: "/home/new-machine/.config/mcpm".into(),
    };
    let files = read_bundle(
        &dir,
        Credential::Key("WrGla9wzWbq8AcLDRsyivn44CMMRn7UnEAsT65gp7Pw="),
        &roots,
    )
    .unwrap();
    let keys: Vec<&str> = files.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "bin/tool",
            "global/server_origins.json",
            "global/servers.json",
            "projects/app/CLAUDE.md",
            "skills_repo/lib/helper.py",
            "skills_repo/skills/demo/blob.bin",
        ]
    );
    let servers = files
        .iter()
        .find(|f| f.key == "global/servers.json")
        .unwrap();
    assert!(String::from_utf8_lossy(&servers.bytes).contains("/home/new-machine/srv/index.js"));
}

#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<String>>,
    git_ok: bool,
}

impl Exec for Recorder {
    fn git(&self, dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
        self.calls.borrow_mut().push(format!(
            "git {} {}",
            dir.map(|d| d.display().to_string()).unwrap_or_default(),
            args.join(" ")
        ));
        if !self.git_ok {
            return Err("offline".into());
        }
        if args[0] == "clone" {
            let dest = args.last().unwrap();
            fs::create_dir_all(Path::new(dest).join(".git")).unwrap();
        }
        Ok("Already up to date.".into())
    }

    fn shell(&self, dir: &Path, command: &str) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("sh {} {command}", dir.display()));
        Ok(())
    }
}

fn origins_with(path: &str, git: bool) -> ServerOrigins {
    let mut origin = schema::ServerOrigin::plain("srv", if git { "git" } else { "manual" });
    origin.install_path = Some(path.into());
    if git {
        origin.git_url = Some("https://example.invalid/srv.git".into());
        origin.setup_command = Some("make setup".into());
    }
    let mut origins = ServerOrigins::default();
    origins.servers.insert("srv".into(), origin);
    origins
}

#[test]
fn resolve_clones_updates_and_confines_paths_to_home() {
    let lab = Lab::new("resolve");
    let a = lab.machine("a");
    let roots = a.ctx(&SystemExec).roots;
    let rec = Recorder {
        git_ok: true,
        ..Recorder::default()
    };

    let cloned = resolve_servers(
        &origins_with("${HOME}/servers/srv", true),
        &rec,
        &roots,
        &ResolveOptions { run_setup: false },
    );
    assert_eq!(cloned[0].status, "cloned");
    assert!(cloned[0].message.contains("setup not run: make setup"));
    assert!(!rec.calls.borrow().iter().any(|c| c.starts_with("sh ")));

    let again = resolve_servers(
        &origins_with("${HOME}/servers/srv", true),
        &rec,
        &roots,
        &ResolveOptions { run_setup: true },
    );
    assert_eq!(again[0].status, "ready");
    assert_eq!(again[0].message, "already up to date");

    let outside = resolve_servers(
        &origins_with("/etc/elsewhere", true),
        &rec,
        &roots,
        &ResolveOptions { run_setup: true },
    );
    assert_eq!(outside[0].status, "failed");
    let traversal = resolve_servers(
        &origins_with("${HOME}/../../etc/x", false),
        &rec,
        &roots,
        &ResolveOptions { run_setup: true },
    );
    assert_eq!(traversal[0].status, "failed");

    let fresh = lab.machine("b");
    let rec2 = Recorder {
        git_ok: true,
        ..Recorder::default()
    };
    let with_setup = resolve_servers(
        &origins_with("${HOME}/s2", true),
        &rec2,
        &fresh.ctx(&SystemExec).roots,
        &ResolveOptions { run_setup: true },
    );
    assert_eq!(with_setup[0].status, "cloned");
    assert!(rec2
        .calls
        .borrow()
        .iter()
        .any(|c| c.ends_with("make setup")));

    let missing = resolve_servers(
        &origins_with("${HOME}/nowhere", false),
        &rec,
        &roots,
        &ResolveOptions { run_setup: true },
    );
    assert_eq!(missing[0].status, "manual");
}

#[test]
fn detect_origins_finds_git_repos_and_setup_commands() {
    let lab = Lab::new("detect");
    let a = lab.machine("a");
    let repo = a.home.join("srv/tool");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("package.json"), r#"{"scripts":{"build":"tsc"}}"#).unwrap();
    fs::write(repo.join("src/main.js"), "x").unwrap();
    for args in [
        vec!["init", "--quiet", "-b", "dev"],
        vec![
            "remote",
            "add",
            "origin",
            "https://example.invalid/tool.git",
        ],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(&args)
            .output()
            .unwrap()
            .status
            .success());
    }
    a.write(
        "servers.json",
        format!(
            r#"{{"tool":{{"command":"node","args":["{}"]}},"rel":{{"command":"node","args":["rel.js"]}}}}"#,
            repo.join("src/main.js").display()
        )
        .as_bytes(),
    );
    a.write(
        "sources.json",
        br#"{"rel":{"type":"github-release","repo":"owner/repo","asset_pattern":"*linux*"}}"#,
    );
    let ctx = a.ctx(&SystemExec);
    let origins = detect_origins(
        &a.config.join("servers.json"),
        &a.config.join("sources.json"),
        &SystemExec,
        &ctx.roots,
    );
    let tool = &origins.servers["tool"];
    assert_eq!(tool.origin_type, "git");
    assert_eq!(
        tool.git_url.as_deref(),
        Some("https://example.invalid/tool.git")
    );
    assert_eq!(tool.git_branch.as_deref(), Some("dev"));
    assert_eq!(tool.install_path.as_deref(), Some("${HOME}/srv/tool"));
    assert_eq!(
        tool.setup_command.as_deref(),
        Some("npm install && npm run build")
    );
    assert_eq!(origins.servers["rel"].origin_type, "manual");
}

#[test]
fn git_sync_configures_clones_and_clears() {
    let lab = Lab::new("gitsync");
    let a = lab.machine("a");
    let rec = Recorder {
        git_ok: true,
        ..Recorder::default()
    };
    let ctx = a.ctx(&rec);
    assert!(
        !git_sync(&ctx, &GitSyncOptions::default())
            .unwrap()
            .configured
    );
    assert!(git_sync(
        &ctx,
        &GitSyncOptions {
            repo: Some("--evil".into()),
            ..Default::default()
        }
    )
    .is_err());
    let report = git_sync(
        &ctx,
        &GitSyncOptions {
            repo: Some("https://example.invalid/skills.git".into()),
            auto: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(report.configured && report.pulled && report.cloned && report.auto_sync);
    assert!(
        rec.calls
            .borrow()
            .iter()
            .any(|c| c
                .contains("clone --branch main --depth 1 -- https://example.invalid/skills.git"))
    );
    let second = git_sync(&ctx, &GitSyncOptions::default()).unwrap();
    assert!(second.pulled);
    assert!(rec
        .calls
        .borrow()
        .iter()
        .any(|c| c.contains("pull --ff-only")));
    let status = git_sync(
        &ctx,
        &GitSyncOptions {
            status: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!status.pulled && status.configured);
    assert!(
        git_sync(
            &ctx,
            &GitSyncOptions {
                clear: true,
                ..Default::default()
            }
        )
        .unwrap()
        .cleared
    );
    assert!(
        !git_sync(&ctx, &GitSyncOptions::default())
            .unwrap()
            .configured
    );
}

#[test]
fn passphrase_and_credentials_never_reach_persisted_files() {
    let lab = Lab::new("leak");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    a.seed();
    let ctx = a.ctx(&SystemExec);
    init(&ctx, &init_opts(&remote, PASS)).unwrap();
    push(&ctx, &PushOptions::default()).unwrap();
    for name in ["sync.json", "sync_state.json"] {
        let text = fs::read_to_string(a.state.join(name)).unwrap();
        assert!(!text.contains(PASS), "{name}");
    }
    let log = SystemExec
        .git(
            Some(&a.state.join("sync_repo")),
            &["log", "--format=%an %ae %s"],
        )
        .unwrap();
    assert!(!log.contains(PASS));
}

#[test]
#[ignore]
fn regenerate_rust_golden() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plus/sync/fixtures/rust-bundle");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let roots = PortableRoots {
        home: "/home/old-machine".into(),
        mcpm_home: "/home/old-machine/.config/mcpm".into(),
    };
    let file = |key: &str, category: &str, project: Option<&str>, bytes: &[u8]| SourceFile {
        key: key.into(),
        category: category.into(),
        project_name: project.map(str::to_string),
        bytes: bytes.to_vec(),
    };
    let files = vec![
        file(
            "global/servers.json",
            "global",
            None,
            br#"{"demo":{"command":"node","args":["/home/old-machine/srv/index.js"]}}"#,
        ),
        file(
            "skills_repo/lib/helper.py",
            "global",
            None,
            b"print('lib')\n",
        ),
        file(
            "skills_repo/skills/demo/blob.bin",
            "global",
            None,
            &[0, 159, 146, 150, 255, 1, 2, 3],
        ),
        file("bin/tool", "global", None, b"#!/bin/sh\necho tool\n"),
        file(
            "projects/app/CLAUDE.md",
            "project",
            Some("app"),
            "project rules \u{e9}\n".as_bytes(),
        ),
    ];
    let origins = r#"{"version":1,"servers":{"demo":{"server_name":"demo","origin_type":"manual","git_url":null,"git_branch":"main","github_repo":null,"asset_pattern":null,"install_path":"${HOME}/srv","setup_command":null}}}"#;
    bundle::write_bundle_with_origins(
        &dir,
        Credential::Key("WrGla9wzWbq8AcLDRsyivn44CMMRn7UnEAsT65gp7Pw="),
        None,
        "synthetic-machine",
        "2026-10-03T12:00:00+00:00",
        &files,
        &roots,
        Some(origins),
    )
    .unwrap();
    let salt =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plus/sync/fixtures/old-bundle/salt.txt");
    fs::copy(salt, dir.join("salt.txt")).unwrap();
}

#[test]
fn plus_handlers_drive_the_engine_through_dispatch() {
    let lab = Lab::new("handlers");
    let remote = lab.remote("main");
    let a = lab.machine("a");
    a.seed();
    let base = serde_json::json!({
        "configDir": a.config.to_string_lossy(),
        "stateDir": a.state.to_string_lossy(),
        "home": a.home.to_string_lossy(),
    });
    let with = |extra: serde_json::Value| {
        let mut args = base.clone();
        for (k, v) in extra.as_object().unwrap() {
            args[k] = v.clone();
        }
        args
    };
    let status = crate::plus::dispatch("plus.sync.status", base.clone()).unwrap();
    assert_eq!(status["configured"], false);
    let missing = crate::plus::dispatch("plus.sync.init", base.clone()).unwrap_err();
    assert!(missing.contains("missing argument"));
    crate::plus::dispatch(
        "plus.sync.init",
        with(serde_json::json!({"repo": remote, "passphrase": PASS, "machineId": "m"})),
    )
    .unwrap();
    let dry =
        crate::plus::dispatch("plus.sync.push", with(serde_json::json!({"dryRun": true}))).unwrap();
    assert_eq!(dry["pushed"], false);
    assert_eq!(dry["entries"].as_array().unwrap().len(), 7);
    let pushed = crate::plus::dispatch("plus.sync.push", base.clone()).unwrap();
    assert_eq!(pushed["committed"], true);
    let diff = crate::plus::dispatch("plus.sync.diff", base.clone()).unwrap();
    assert_eq!(diff["noRemote"], false);
    let reset = crate::plus::dispatch("plus.sync.reset", base).unwrap();
    assert_eq!(reset["removed"].as_array().unwrap().len(), 4);
}
