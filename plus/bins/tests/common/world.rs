#![allow(dead_code)]

//! A synthetic world for tests that run the real binaries: data directory and home. Nothing in it
//! is a real credential and nothing outside its temp directory is read or written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value};

pub const SECRET_KEY: &str = "abababababababababababababababababababababababababababababababab";
pub const FAKE_SECRET: &str = "FAKE-SECRET-VALUE-do-not-print-7f3a";
/// The mock server: its `auth` command exits 0 as a server that is already signed in does.
const MOCK_SCRIPT: &str = "#!/bin/sh\n[ \"$1\" = auth ] && exit 0\nexit 2\n";
/// What the stub gateway holds so the doctor's seam check finds the marker in it.
pub const SEAM_MARKER: &str = toolport_plus_api::SEAM_MARKER;

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct World {
    pub base: PathBuf,
    pub repo: PathBuf,
    pub data: PathBuf,
    pub home: PathBuf,
    pub mock: String,
    /// A loopback authorization server, once a step has started one.
    pub idp: std::cell::RefCell<Option<super::oauth_mock::OauthMock>>,
}

impl World {
    pub fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "plus-world-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let mock = base.join("mock-mcp-server");
        let world = Self {
            data: base.join("data"),
            home: base.join("home"),
            repo: base.join("skills-repo"),
            mock: mock.to_string_lossy().into_owned(),
            idp: Default::default(),
            base,
        };
        for dir in [&world.data, &world.home, &world.data.join("bin")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(&mock, MOCK_SCRIPT).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mock, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        write_json(
            &world.data.join("registry.json"),
            &json!({
                "version": 1,
                "servers": [
                    {"id": "srv-alpha", "name": "alpha", "transport": "stdio",
                     "command": world.mock, "args": [],
                     "mcpmSource": {"type": "unknown", "reason": "synthetic fixture"},
                     "env": [{"key": "API_KEY", "value": FAKE_SECRET, "secret": true}]},
                    {"id": "srv-beta", "name": "beta", "transport": "http",
                     "url": "https://example.invalid/mcp"}
                ],
                "profiles": [{"id": "default", "name": "Default",
                              "enabledServerIds": ["srv-alpha"]}],
                "activeProfileId": "default"
            }),
        );
        let demo = world.repo.join("skills/demo/SKILL.md");
        std::fs::create_dir_all(demo.parent().unwrap()).unwrap();
        std::fs::write(
            demo,
            "---\nname: demo\ndescription: A synthetic demo skill\n---\nBody text\n",
        )
        .unwrap();
        for (rel, text) in [
            (
                "agents/helper/AGENT.md",
                "---\nname: helper\ndescription: A synthetic helper agent\nmodel: inherit\n---\nAgent prompt\n",
            ),
            (
                "styles/plain/STYLE.md",
                "---\nname: plain\ndescription: A synthetic plain style\nkeep-coding-instructions: true\n---\nStyle text\n",
            ),
        ] {
            let path = world.repo.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        world.stub_gateway(SEAM_MARKER.as_bytes());
        world
    }

    /// A skills repository with one bundle, a global and a project CLAUDE.md and a git folder.
    pub fn context_files(&self) {
        let world = self;
        let put = |rel: &str, text: &str| {
            let path = world.home.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        let repo = ".config/mcpm/skills_repo";
        for skill in ["notes-helper", "scratch-1", "long-guide"] {
            put(
                &format!("{repo}/skills/{skill}/SKILL.md"),
                &format!("---\nname: {skill}\ndescription: d\n---\nbody\n"),
            );
        }
        put(
            &format!("{repo}/rules/acme-knowledge/SKILL.md"),
            "---\nname: acme-knowledge\ndescription: d\n---\n\nUse the ERP naming rules.\n",
        );
        put(
            &format!("{repo}/profiles/acme-dev.yaml"),
            include_str!("../../../fixtures/bundles/acme-dev.yaml")
                .replace("servers: acme-dev", "servers: Default")
                .as_str(),
        );
        put(".claude/CLAUDE.md", "global rules\n");
        put("proj/CLAUDE.md", "project rules\n");
        std::fs::create_dir_all(world.home.join("proj/.git/info")).unwrap();
    }

    /// What the memory and history commands read, on top of `context_files`: a project with
    /// auto memory, a linked worktree, sessions of three ages, a second repository whose memory
    /// is switched off by hand, `~/.claude.json` naming the folders Claude Code has run in, and
    /// a transcript folder that belongs to no repository.
    pub fn memory_files(&self) {
        self.context_files();
        let home = &self.home;
        let put = |rel: &str, text: &str| {
            let path = home.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            path
        };
        let slug = |path: &Path| -> String {
            std::fs::canonicalize(path)
                .unwrap()
                .to_string_lossy()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect()
        };
        let proj = home.join("proj");
        let memory = format!(".claude/projects/{}/memory", slug(&proj));
        put(
            &format!("{memory}/MEMORY.md"),
            "- [Role](user_role.md): who the user is\n- [Testing](feedback_testing.md): how to test\n",
        );
        put(
            &format!("{memory}/user_role.md"),
            "---\nname: Role\ndescription: who the user is\ntype: user\n---\nA data scientist.\n",
        );
        put(
            &format!("{memory}/feedback_testing.md"),
            "---\nname: Testing\ndescription: how to test\ntype: feedback\n---\nUse a real database.\n",
        );
        let linked = home.join("proj-wt");
        std::fs::create_dir_all(&linked).unwrap();
        let registered = proj.join(".git/worktrees/proj-wt");
        std::fs::create_dir_all(&registered).unwrap();
        put(
            "proj-wt/.git",
            &format!("gitdir: {}\n", registered.display()),
        );
        std::fs::write(registered.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            registered.join("gitdir"),
            format!("{}\n", linked.join(".git").display()),
        )
        .unwrap();
        let old = |path: std::path::PathBuf, days: u64| {
            let when = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(when)
                .unwrap();
        };
        for (folder, id, text, days) in [
            (&proj, "sess-new", "{\"a\":1}\n", 1),
            (&proj, "sess-old", "{\"a\":1}{\"b\":2}\n", 27),
            (&linked, "sess-wt", "{\"w\":1}\n", 10),
        ] {
            let file = put(
                &format!(".claude/projects/{}/{id}.jsonl", slug(folder)),
                text,
            );
            old(file, days);
        }
        put(
            &format!(
                ".claude/projects/{}/sess-old/subagents/a.jsonl",
                slug(&proj)
            ),
            "{}\n",
        );
        let other = home.join("other");
        std::fs::create_dir_all(other.join(".git/info")).unwrap();
        put("other/CLAUDE.local.md", "my notes\n");
        put(
            "other/.claude/settings.local.json",
            "{\n  \"autoMemoryEnabled\": false\n}\n",
        );
        put(
            &format!(".claude/projects/{}/memory/MEMORY.md", slug(&other)),
            "- [x](x.md)\n",
        );
        put(".claude/projects/-gone-folder/sess-lost.jsonl", "{}\n");
        put(
            ".claude.json",
            &serde_json::json!({
                "projects": {
                    proj.to_string_lossy(): {"hasClaudeMdExternalIncludesApproved": true},
                    linked.to_string_lossy(): {},
                    other.to_string_lossy(): {},
                },
                "oauthAccount": {"token": "never read"},
            })
            .to_string(),
        );
    }

    /// `memory_files`, then the memory of `proj` and its oldest session sit in Toolport's trash as
    /// a clear would have left them.
    pub fn trashed_files(&self) {
        self.memory_files();
        let proj = std::fs::canonicalize(self.home.join("proj")).unwrap();
        let slug: String = proj
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let project =
            std::fs::canonicalize(self.home.join(format!(".claude/projects/{slug}"))).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let trash = |id: &str, kind: &str, moves: &[(&str, u64)]| {
            let dir = self.data.join("plus/memory/trash").join(id);
            let mut items = Vec::new();
            for (index, (name, bytes)) in moves.iter().enumerate() {
                let from = project.join(name);
                let payload = format!("{index}/{name}");
                let to = dir.join("payload").join(&payload);
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::rename(&from, &to).unwrap();
                items.push(
                    json!({"from": from.to_string_lossy(), "payload": payload, "bytes": bytes}),
                );
            }
            write_json(
                &dir.join("manifest.json"),
                &json!({
                    "id": id, "kind": kind, "created": now,
                    "repoRoot": proj.to_string_lossy(), "repoName": "proj", "items": items,
                }),
            );
        };
        trash("memory-20260101-000000", "memory", &[("memory", 246)]);
        trash(
            "sessions-20260101-000000",
            "sessions",
            &[("sess-old.jsonl", 18), ("sess-old", 3)],
        );
    }

    /// What the checks of a library report: a skill with `references/`, a top-level `SETUP.md`,
    /// a dotfile in `templates/`, a file the sync leaves out, a dead link, a hook script without
    /// the execute bit and a 1,200 character description, next to a skill whose frontmatter does
    /// not parse.
    pub fn checks_files(&self) {
        let put = |rel: &str, text: &str| {
            let path = self.repo.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        let lead = "Use when checking that a sync copies a whole skill folder. ";
        let description = format!("{lead}{}", "x".repeat(1200 - lead.len()));
        put(
            "skills/acceptance/SKILL.md",
            &format!("---\nname: acceptance\ndescription: \"{description}\"\nhooks:\n  Stop:\n    command: scripts/stop.sh\n---\n\n# Acceptance\n\nSee [setup](SETUP.md), [guide](references/guide.md) and [gone](references/gone.md).\n"),
        );
        put("skills/acceptance/SETUP.md", "# setup\n");
        put("skills/acceptance/references/guide.md", "guide\n");
        put("skills/acceptance/templates/x/.gitignore", "*.tmp\n");
        put("skills/acceptance/scripts/stop.sh", "#!/bin/sh\n");
        put("skills/acceptance/.DS_Store", "x");
        put(
            "skills/broken/SKILL.md",
            "---\nname: broken\ndescription: [unclosed\n---\n\nbody\n",
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                self.repo.join("skills/acceptance/scripts/stop.sh"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        }
    }

    /// A gateway binary in the data dir's `bin`, holding `content`.
    pub fn stub_gateway(&self, content: &[u8]) {
        std::fs::write(self.data.join("bin/toolport-gateway"), content).unwrap();
    }

    pub fn path(&self, p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    /// The environment a child binary runs in: nothing inherited from the real machine.
    pub fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("PATH", "/usr/bin:/bin".to_string()),
            ("HOME", self.path(&self.home)),
            ("XDG_CONFIG_HOME", self.path(&self.home.join(".config"))),
            ("XDG_DATA_HOME", self.path(&self.home.join(".local/share"))),
            ("XDG_CACHE_HOME", self.path(&self.home.join(".cache"))),
            ("TOOLPORT_DATA_DIR", self.path(&self.data)),
            ("TOOLPORT_SECRET_KEY", SECRET_KEY.to_string()),
            ("TOOLPORT_GITLEAKS_BIN", "/nonexistent/gitleaks".to_string()),
            (
                "TOOLPORT_CLAUDE_BIN",
                self.path(&self.home.join("no-claude")),
            ),
            ("TOOLPORT_CLAUDE_MANAGED_SETTINGS", String::new()),
        ]
    }

    /// Every file of the scratch tree, for "this step changed nothing" checks. Lock files are
    /// left out: taking the registry lock creates one and is not state, and neither is the scan
    /// cache under `plus/cache`, which a read of the sources fills.
    pub fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn collect(dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if !path.ends_with("plus/cache") {
                        collect(&path, files);
                    }
                } else if path.extension().is_some_and(|ext| ext == "lock") {
                    continue;
                } else if let Ok(bytes) = std::fs::read(&path) {
                    files.insert(path, bytes);
                }
            }
        }
        let mut files = BTreeMap::new();
        collect(&self.base, &mut files);
        files
    }

    pub fn registry(&self) -> Value {
        read_json(&self.data.join("registry.json"))
    }

    /// Replaces the registry with `value`.
    pub fn write_registry(&self, value: &Value) {
        write_json(&self.data.join("registry.json"), value);
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

pub fn write_json(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

pub fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
