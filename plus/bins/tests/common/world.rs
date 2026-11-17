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
