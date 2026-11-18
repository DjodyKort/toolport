//! Setup export against a synthetic home: real git repositories in a temp dir, a memory host and
//! a Claude Code folder made up for the test. Nothing outside the temp dir is read.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use toolport_plus_api::memory::{MemoryHost, MemoryRegistry, MemorySecrets};
use toolport_plus_api::{ServerRecord, StaticSecret};

use super::export::{self, Options};
use super::mem::slug;
use super::zipio::ZipFile;
use super::{Ctx, PartId};

pub(super) const PASS: &str = "correct horse battery";
pub(super) const SECRET_VALUE: &str = "vaulted-value-7f3a-not-a-real-credential";

/// Spelled in pieces so no scanner reads it as a credential.
pub(super) fn token() -> String {
    ["gh", "p_", &"Q1w2E3r4".repeat(5)].concat()
}

pub(super) struct World {
    pub dir: tempfile::TempDir,
    pub home: PathBuf,
    pub data: PathBuf,
    pub host: MemoryHost,
    _paths: crate::fsx::PathsGuard,
}

impl World {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let data = dir.path().join("data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let server = |id: &str, name: &str| {
            let mut record = ServerRecord {
                id: id.into(),
                name: name.into(),
                enabled: true,
                ..Default::default()
            };
            record.extra.insert("transport".into(), json!("stdio"));
            record.extra.insert("command".into(), json!("alpha-mcp"));
            record
        };
        let registry = MemoryRegistry::with_servers(vec![server("srv-alpha", "alpha")]);
        let host = MemoryHost {
            registry,
            secrets: MemorySecrets {
                data_dir: Some(data.clone()),
                ..Default::default()
            },
            home: Some(home.clone()),
            ..Default::default()
        };
        host.secrets
            .vault
            .borrow_mut()
            .insert(("srv-alpha".into(), "API_KEY".into()), SECRET_VALUE.into());
        let paths = crate::fsx::enter(crate::fsx::Paths {
            home: Some(home.clone()),
            data_dir: Some(data.clone()),
        });
        Self {
            dir,
            home,
            data,
            host,
            _paths: paths,
        }
    }

    pub fn ctx(&self) -> Ctx<'_> {
        Ctx {
            host: &self.host,
            home: self.home.clone(),
            data_dir: self.data.clone(),
            claude_home: self.home.join(".claude"),
        }
    }

    pub fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
            .env("HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
            .stdin(Stdio::null())
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn put(&self, path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A bare remote, `name.git` next to the home folder.
    pub fn bare(&self, name: &str) -> PathBuf {
        let bare = self.dir.path().join("remotes").join(format!("{name}.git"));
        std::fs::create_dir_all(&bare).unwrap();
        self.git(&bare, &["init", "-q", "--bare", "-b", "main"]);
        bare
    }

    /// `~/code/<name>` with one pushed commit, tracking a bare remote.
    pub fn repo(&self, name: &str) -> PathBuf {
        let bare = self.bare(name);
        let repo = self.home.join("code").join(name);
        std::fs::create_dir_all(&repo).unwrap();
        self.git(&repo, &["init", "-q", "-b", "main"]);
        self.put(&repo.join("README.md"), "# readme\n");
        self.git(&repo, &["add", "-A"]);
        self.git(&repo, &["commit", "-q", "-m", "first"]);
        self.git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
        self.git(&repo, &["push", "-q", "-u", "origin", "main"]);
        repo
    }

    /// A repository with every kind of thing a wipe would lose.
    pub fn messy_repo(&self) -> PathBuf {
        let repo = self.repo("web");
        self.put(&repo.join("a.txt"), "one\n");
        self.git(&repo, &["add", "-A"]);
        self.git(&repo, &["commit", "-q", "-m", "second, not pushed"]);
        self.put(&repo.join("README.md"), "# changed\n");
        self.git(&repo, &["stash", "-q"]);
        self.put(&repo.join("README.md"), "# changed again\n");
        self.put(&repo.join("new.txt"), "untracked\n");
        self.git(&repo, &["checkout", "-q", "-b", "wip"]);
        self.put(&repo.join("wip.txt"), "wip\n");
        self.git(&repo, &["add", "wip.txt"]);
        self.git(&repo, &["commit", "-q", "-m", "wip"]);
        self.git(&repo, &["checkout", "-q", "main"]);
        self.put(&repo.join(".gitignore"), ".env\n");
        self.put(&repo.join(".env"), "KEY=value\n");
        let tree = self.home.join("code/web-feature");
        self.git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                &tree.to_string_lossy(),
            ],
        );
        repo
    }

    pub fn no_remote_repo(&self, name: &str) -> PathBuf {
        let repo = self.home.join("code").join(name);
        std::fs::create_dir_all(&repo).unwrap();
        self.git(&repo, &["init", "-q", "-b", "main"]);
        self.put(&repo.join("a.txt"), "x\n");
        self.git(&repo, &["add", "-A"]);
        self.git(&repo, &["commit", "-q", "-m", "only here"]);
        repo
    }

    pub fn claude(&self, rel: &str, text: &str) {
        self.put(&self.home.join(".claude").join(rel), text);
    }

    /// Auto memory and sessions for a repository, under Claude Code's own folder names.
    pub fn memory_of(&self, repo: &Path, files: &[(&str, &str)]) {
        for (name, text) in files {
            self.claude(&format!("projects/{}/memory/{name}", slug(repo)), text);
        }
    }

    pub fn session_of(&self, repo: &Path, id: &str, text: &str) {
        let folder = format!("projects/{}", slug(repo));
        self.claude(&format!("{folder}/{id}.jsonl"), text);
        self.claude(&format!("{folder}/{id}/subagents/agent.jsonl"), text);
    }

    pub fn out_dir(&self) -> PathBuf {
        let out = self.dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        out
    }

    pub fn snapshot(&self) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
        crate::testutil::tree_snapshot(self.dir.path())
    }

    /// Fails with the names of the files that differ, not with both trees.
    pub fn assert_unchanged(&self, before: &std::collections::BTreeMap<String, Option<Vec<u8>>>) {
        let after = self.snapshot();
        let mut changed: Vec<&String> = before
            .keys()
            .chain(after.keys())
            .filter(|k| before.get(*k) != after.get(*k))
            .collect();
        changed.sort();
        changed.dedup();
        assert!(changed.is_empty(), "changed: {changed:?}");
    }
}

pub(super) fn options(world: &World) -> Options {
    Options {
        roots: vec![world.home.join("code")],
        out: Some(world.out_dir()),
        ..Options::default()
    }
}

fn read_part(file: &Path, name: &str) -> Value {
    let mut zip = ZipFile::open(file).unwrap();
    serde_json::from_slice(&zip.read(name).unwrap()).unwrap()
}

fn names(file: &Path) -> Vec<String> {
    ZipFile::open(file)
        .unwrap()
        .names()
        .map(String::from)
        .collect()
}

fn file_of(data: &Value) -> PathBuf {
    PathBuf::from(data["file"].as_str().unwrap())
}

#[test]
fn test01_the_export_is_one_zip_with_a_manifest_that_every_part_matches() {
    let world = World::new();
    world.messy_repo();
    world.claude("CLAUDE.md", "global rules\n");
    let data = export::run(&world.ctx(), &options(&world), None).unwrap();
    let file = file_of(&data);
    assert!(file
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("toolport-setup-"));
    assert_eq!(data["verified"], true);

    let manifest = read_part(&file, "manifest.json");
    assert_eq!(manifest["format"], "toolport-setup");
    assert_eq!(manifest["version"], 1);
    let ids: Vec<&str> = manifest["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "repos",
            "servers",
            "access",
            "library",
            "tasks",
            "settings",
            "claudeUser",
            "memory"
        ]
    );
    assert!(!ids.contains(&"secrets") && !ids.contains(&"history"));
    let all = names(&file);
    assert_eq!(all[0], "manifest.json");
    assert!(all.contains(&"claude-user/CLAUDE.md".to_string()));
    assert!(all.iter().all(|n| super::zipio::safe_name(n)));
}

#[test]
fn test02_repositories_come_as_a_list_with_branch_remote_and_what_exists_only_here() {
    let world = World::new();
    world.messy_repo();
    world.no_remote_repo("scratch");
    let data = export::run(&world.ctx(), &options(&world), None).unwrap();
    let repos = read_part(&file_of(&data), "repos.json");
    let list = repos["repos"].as_array().unwrap();
    assert_eq!(
        list.len(),
        2,
        "the linked worktree is not a repository of its own: {list:?}"
    );
    let web = list.iter().find(|r| r["name"] == "web").unwrap();
    assert_eq!(web["path"], "~/code/web");
    assert_eq!(web["branch"], "main");
    assert_eq!(web["upstream"], "origin/main");
    assert_eq!(web["ahead"], 1);
    assert_eq!(web["dirty"], 1);
    assert_eq!(web["untracked"], 2);
    assert_eq!(web["stashes"], 1);
    assert_eq!(web["unpushed"], json!([{"branch": "main", "ahead": 1}]));
    assert_eq!(web["neverPushed"], json!(["feature", "wip"]));
    assert_eq!(web["worktrees"][0]["path"], "~/code/web-feature");
    assert_eq!(web["worktrees"][0]["branch"], "feature");
    assert!(web["remotes"][0]["url"]
        .as_str()
        .unwrap()
        .ends_with("web.git"));
    assert!(web.get("ignoredNames").is_none());
    let scratch = list.iter().find(|r| r["name"] == "scratch").unwrap();
    assert_eq!(scratch["noRemote"], true);
}

#[test]
fn test03_worktrees_and_repositories_without_a_remote_can_be_left_out() {
    let world = World::new();
    world.messy_repo();
    world.no_remote_repo("scratch");
    let mut opts = options(&world);
    opts.worktrees = false;
    opts.skip_no_remote = true;
    let data = export::run(&world.ctx(), &opts, None).unwrap();
    let repos = read_part(&file_of(&data), "repos.json");
    let list = repos["repos"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["worktrees"], json!([]));
}

#[test]
fn test04_a_dry_run_writes_nothing_and_a_peek_shows_the_exact_part() {
    let world = World::new();
    world.repo("web");
    let mut opts = options(&world);
    let before = world.snapshot();
    opts.dry_run = true;
    let data = export::run(&world.ctx(), &opts, None).unwrap();
    assert_eq!(data["dryRun"], true);
    world.assert_unchanged(&before);

    opts.peek = Some(PartId::Servers);
    let peek = export::run(&world.ctx(), &opts, None).unwrap();
    assert_eq!(peek["peek"]["part"], "servers");
    world.assert_unchanged(&before);
}

#[test]
fn test05_an_existing_file_is_never_replaced_and_a_failed_run_leaves_nothing() {
    let world = World::new();
    world.repo("web");
    let mut opts = options(&world);
    let out = world.out_dir().join("setup.zip");
    opts.out = Some(out.clone());
    let first = export::run(&world.ctx(), &opts, None).unwrap();
    assert_eq!(file_of(&first), out);
    let error = export::run(&world.ctx(), &opts, None).unwrap_err();
    assert!(
        error.message.contains("never replaces"),
        "{}",
        error.message
    );

    let left: Vec<String> = std::fs::read_dir(world.out_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["setup.zip"]);
}

#[test]
fn test06_user_settings_leave_secret_looking_values_behind_and_name_them() {
    let world = World::new();
    world.claude("CLAUDE.md", "my rules\n");
    world.claude("rules/style.md", "short\n");
    world.claude(
        "settings.json",
        &json!({"model": "opus", "env": {"MY_VAR": "x"}, "apiKeyHelper": "echo", "theme": "dark"})
            .to_string(),
    );
    world.claude(
        "plugins/installed_plugins.json",
        &json!({"version": 2, "plugins": {"ecc@ecc": [{"scope": "user"}]}}).to_string(),
    );
    world.claude("agents/mine.md", "an agent\n");
    let data = export::run(&world.ctx(), &options(&world), None).unwrap();
    let file = file_of(&data);
    let settings = read_part(&file, "claude-user/settings.json");
    assert_eq!(settings["model"], "opus");
    assert_eq!(settings["theme"], "dark");
    assert_eq!(settings["env"], json!({}));
    assert!(settings.get("apiKeyHelper").is_none());
    let removed = read_part(&file, "claude-user/removed.json");
    assert_eq!(removed["removed"], json!(["apiKeyHelper", "env.MY_VAR"]));
    let plugins = read_part(&file, "claude-user/plugins.json");
    assert_eq!(plugins["plugins"][0]["id"], "ecc@ecc");
    let all = names(&file);
    for name in [
        "claude-user/CLAUDE.md",
        "claude-user/rules/style.md",
        "claude-user/agents/mine.md",
    ] {
        assert!(all.contains(&name.to_string()), "{name} in {all:?}");
    }
}

#[test]
fn test07_memory_is_keyed_by_remote_and_a_token_shaped_file_stays_out_without_secrets() {
    let world = World::new();
    let repo = world.repo("web");
    world.memory_of(
        &repo,
        &[
            ("MEMORY.md", "- [topic](topic.md)\n"),
            ("topic.md", "the user likes short answers\n"),
            ("leak.md", &format!("deploy with {}\n", token())),
        ],
    );
    world.put(&repo.join("CLAUDE.local.md"), "local notes\n");
    let data = export::run(&world.ctx(), &options(&world), None).unwrap();
    let file = file_of(&data);
    let index = read_part(&file, "memory/index.json");
    let entry = &index["repos"][0];
    assert!(
        entry["key"].as_str().unwrap().ends_with("/web.git")
            || entry["key"].as_str().unwrap().contains("web")
    );
    assert_eq!(entry["files"], json!(["MEMORY.md", "topic.md"]));
    assert_eq!(entry["flagged"], json!(["leak.md"]));
    assert_eq!(entry["instructions"], json!(["CLAUDE.local.md"]));
    let all = names(&file);
    assert!(all.iter().any(|n| n.ends_with("/topic.md")));
    assert!(!all.iter().any(|n| n.ends_with("/leak.md")));
    let bytes = std::fs::read(&file).unwrap();
    assert!(!bytes
        .windows(token().len())
        .any(|w| w == token().as_bytes()));
    let memory = data["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "memory")
        .unwrap();
    assert_eq!(memory["flagged"], json!(["web/leak.md"]));
}

#[cfg(feature = "cipher")]
mod sealed {
    use super::*;
    use crate::setup::crypto;

    fn with_secrets(world: &World) -> Options {
        let mut opts = options(world);
        opts.parts.insert(PartId::Secrets);
        opts
    }

    #[test]
    fn test01_secrets_are_sealed_and_no_readable_part_holds_the_value() {
        let world = World::new();
        let data = export::run(&world.ctx(), &with_secrets(&world), Some(PASS)).unwrap();
        let file = file_of(&data);
        let bytes = std::fs::read(&file).unwrap();
        assert!(!bytes
            .windows(SECRET_VALUE.len())
            .any(|w| w == SECRET_VALUE.as_bytes()));
        let mut zip = ZipFile::open(&file).unwrap();
        let sealed = zip.read("secrets.enc").unwrap();
        let plain = crypto::open_bytes(PASS, &sealed).unwrap();
        let doc: Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(doc["values"][0]["server"], "alpha");
        assert_eq!(doc["values"][0]["value"], SECRET_VALUE);
        assert_eq!(
            crypto::open_bytes("another passphrase", &sealed),
            Err(crypto::OpenError::Wrong)
        );
        let manifest = read_part(&file, "manifest.json");
        let row = manifest["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "secrets")
            .unwrap();
        assert_eq!(row["kdf"], "argon2id");
        assert_eq!(row["cipher"], "xchacha20poly1305");
        assert!(!manifest.to_string().contains(SECRET_VALUE));
    }

    #[test]
    fn test02_a_short_passphrase_or_none_stops_the_export_before_anything_is_written() {
        let world = World::new();
        let opts = with_secrets(&world);
        let before = world.snapshot();
        let short = export::run(&world.ctx(), &opts, Some("too short")).unwrap_err();
        assert!(short.message.contains("at least 12"), "{}", short.message);
        let none = export::run(&world.ctx(), &opts, None).unwrap_err();
        assert!(none.message.contains("passphrase"), "{}", none.message);
        world.assert_unchanged(&before);
    }

    #[test]
    fn test03_flagged_memory_goes_into_the_sealed_part_when_secrets_are_on() {
        let world = World::new();
        let repo = world.repo("web");
        world.memory_of(&repo, &[("leak.md", &format!("deploy with {}\n", token()))]);
        let data = export::run(&world.ctx(), &with_secrets(&world), Some(PASS)).unwrap();
        let file = file_of(&data);
        let bytes = std::fs::read(&file).unwrap();
        assert!(!bytes
            .windows(token().len())
            .any(|w| w == token().as_bytes()));
        let sealed = ZipFile::open(&file).unwrap().read("secrets.enc").unwrap();
        let doc: Value =
            serde_json::from_slice(&crypto::open_bytes(PASS, &sealed).unwrap()).unwrap();
        assert_eq!(doc["memoryFiles"][0]["name"], "leak.md");
    }

    #[test]
    fn test04_sessions_are_sealed_per_repository_and_off_unless_asked() {
        let world = World::new();
        let repo = world.repo("web");
        world.session_of(&repo, "abc", "a transcript that mentions nothing special\n");
        let mut opts = options(&world);
        opts.out = Some(world.out_dir().join("plain.zip"));
        let plain = export::run(&world.ctx(), &opts, None).unwrap();
        assert!(!names(&file_of(&plain)).contains(&"history.enc".to_string()));

        opts.out = Some(world.out_dir().join("sealed.zip"));
        opts.parts.insert(PartId::History);
        let data = export::run(&world.ctx(), &opts, Some(PASS)).unwrap();
        let file = file_of(&data);
        let bytes = std::fs::read(&file).unwrap();
        assert!(!bytes.windows(10).any(|w| w == b"transcript"));
        let mut zip = ZipFile::open(&file).unwrap();
        let mut sealed = Vec::new();
        zip.copy("history.enc", &mut sealed).unwrap();
        let inner = crypto::open_bytes(PASS, &sealed).unwrap();
        assert!(inner.windows(10).any(|w| w == b"transcript"));
        let history = data["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "history")
            .unwrap();
        assert_eq!(history["count"], 1);
    }

    #[test]
    fn test05_the_secret_values_never_reach_the_result_or_the_preview() {
        let world = World::new();
        let mut opts = with_secrets(&world);
        opts.dry_run = true;
        let data = export::run(&world.ctx(), &opts, Some(PASS)).unwrap();
        assert!(!data.to_string().contains(SECRET_VALUE));
        let secrets = StaticSecret {
            server: "alpha".into(),
            key: "K".into(),
            kind: "env".into(),
            value: "v".into(),
        };
        assert!(!format!("{secrets:?}").contains("\"v\""));
    }
}

#[test]
fn test08_the_parts_listing_counts_what_is_there_and_reads_no_secret() {
    let world = World::new();
    let repo = world.messy_repo();
    world.session_of(&repo, "abc", "one\n");
    world.claude("CLAUDE.md", "rules\n");
    let data = export::list(&world.ctx(), &[world.home.join("code")]).unwrap();
    let parts = data["parts"].as_array().unwrap();
    let find = |id: &str| parts.iter().find(|p| p["id"] == id).unwrap();
    assert_eq!(find("repos")["count"], 1);
    assert_eq!(find("servers")["count"], 1);
    assert_eq!(find("history")["count"], 1);
    assert_eq!(find("history")["sealed"], true);
    assert_eq!(find("history")["default"], false);
    assert_eq!(find("repos")["default"], true);
    assert!(data["repos"][0]["unpushed"] == 1);
    assert!(!data.to_string().contains(SECRET_VALUE));
}
