use super::api::{self, Args};
use crate::fsx::{enter, enter_claude_dir, ClaudeDirGuard, Paths, PathsGuard};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use toolport_plus_api::memory::MemoryHost;

struct World {
    base: PathBuf,
    _paths: PathsGuard,
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!("skills-extras-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        for dir in ["repo", "home", "data"] {
            fs::create_dir_all(base.join(dir)).unwrap();
        }
        let paths = enter(Paths {
            home: Some(base.join("home")),
            data_dir: Some(base.join("data")),
        });
        let world = World {
            base,
            _paths: paths,
        };
        world.put(
            "skills/nacht/SKILL.md",
            "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\n---\n\n# Nacht\n",
        );
        world.put(
            "agents/scout/AGENT.md",
            "---\nname: scout\ndescription: Reads a repository and reports what it finds.\nmodel: sonnet\n---\n\nYou read repositories.\n",
        );
        world.put(
            "styles/plain/STYLE.md",
            "---\nname: plain\ndescription: Short answers\n---\n\nAnswer in two sentences.\n",
        );
        world
    }

    fn repo(&self) -> PathBuf {
        self.base.join("repo")
    }

    fn home(&self, rel: &str) -> PathBuf {
        self.base.join("home").join(rel)
    }

    fn put(&self, rel: &str, body: &str) {
        let path = self.repo().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn args(&self) -> Args {
        Args {
            repo: Some(self.repo()),
            ..Args::default()
        }
    }

    fn sync(&self, args: Args) -> Value {
        api::sync(&args, Some(&MemoryHost::default())).unwrap()
    }

    fn preview(&self) -> Value {
        self.sync(Args {
            dry_run: true,
            ..self.args()
        })
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn rows<'a>(data: &'a Value, kind: &str) -> Vec<&'a Value> {
    data["plan"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["kind"] == kind)
        .collect()
}

const AGENT_FILE: &str = ".claude/agents/scout.md";
const STYLE_FILE: &str = ".claude/output-styles/plain.md";

#[test]
fn test01_the_plan_names_agent_and_style_files_next_to_the_skills() {
    let w = World::new("plan");
    let data = w.preview();
    let agents = rows(&data, "agent");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["skill"], "scout");
    assert_eq!(agents[0]["path"], AGENT_FILE);
    assert_eq!(agents[0]["op"], "add");
    let styles = rows(&data, "style");
    assert_eq!(styles.len(), 1);
    assert_eq!(styles[0]["path"], STYLE_FILE);
    assert!(data["plan"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c.get("kind").is_none() && c["skill"] == "nacht"));
    assert!(!w.home(AGENT_FILE).exists());
}

#[test]
fn test02_one_apply_writes_skills_agents_and_styles_and_verifies_them() {
    let w = World::new("apply");
    let data = w.sync(w.args());
    assert!(w.home(AGENT_FILE).is_file());
    assert!(w.home(STYLE_FILE).is_file());
    assert!(w.home(".claude/skills/nacht/SKILL.md").is_file());
    assert_eq!(data["verify"]["clean"], true);
    assert_eq!(data["verify"]["total"], 3);
    assert_eq!(data["snapshot"]["files"], 3);
    let again = w.preview();
    assert!(rows(&again, "agent").is_empty());
    assert_eq!(again["plan"]["counts"]["unchanged"], 3);
}

#[test]
fn test03_undo_puts_back_the_agent_and_the_style_with_the_skills() {
    let w = World::new("undo");
    w.sync(w.args());
    let undone = api::undo(&w.args(), Some(&MemoryHost::default())).unwrap();
    assert_eq!(undone["removed"], 3);
    assert!(!w.home(AGENT_FILE).exists());
    assert!(!w.home(STYLE_FILE).exists());
    assert!(!w.home(".claude/skills/nacht/SKILL.md").exists());
}

#[test]
fn test04_a_hand_edited_agent_is_held_and_apply_held_replaces_it_after_a_backup() {
    let w = World::new("edited");
    w.sync(w.args());
    fs::write(w.home(AGENT_FILE), "my own scout\n").unwrap();
    let data = w.preview();
    let rows = rows(&data, "agent");
    assert_eq!(rows[0]["op"], "edited");
    assert_eq!(rows[0]["held"], true);
    assert_eq!(data["held"][0]["kind"], "agent");
    w.sync(w.args());
    assert_eq!(
        fs::read_to_string(w.home(AGENT_FILE)).unwrap(),
        "my own scout\n"
    );
    w.sync(Args {
        apply_held: true,
        ..w.args()
    });
    assert!(fs::read_to_string(w.home(AGENT_FILE))
        .unwrap()
        .contains("You read repositories."));
}

#[test]
fn test05_verify_counts_agent_and_style_files_and_names_a_changed_one() {
    let w = World::new("verify");
    w.sync(w.args());
    assert_eq!(api::verify(&w.args()).unwrap()["total"], 3);
    fs::write(w.home(STYLE_FILE), "changed\n").unwrap();
    let data = api::verify(&w.args()).unwrap();
    assert_eq!(data["clean"], false);
    assert_eq!(data["mismatched"][0]["skill"], "plain");
    assert_eq!(data["mismatched"][0]["path"], STYLE_FILE);
}

#[test]
fn test06_only_names_an_agent_with_its_kind_and_leaves_skills_and_styles_alone() {
    let w = World::new("only");
    let data = w.sync(Args {
        only: Some(vec!["agent:scout".into()]),
        ..w.args()
    });
    assert!(w.home(AGENT_FILE).is_file());
    assert!(!w.home(STYLE_FILE).exists());
    assert!(!w.home(".claude/skills/nacht/SKILL.md").exists());
    assert_eq!(rows(&data, "agent").len(), 1);
    assert!(rows(&data, "style").is_empty());
}

#[test]
fn test07_a_step_that_cannot_write_is_named_and_the_rest_of_the_run_goes_on() {
    let w = World::new("failed");
    fs::create_dir_all(w.home(".claude")).unwrap();
    fs::write(
        w.home(".claude/agents"),
        "a file where the folder should be",
    )
    .unwrap();
    let data = w.sync(w.args());
    let failed = data["plan"]["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["id"], "agent:scout");
    assert_eq!(failed[0]["step"], "files");
    assert_eq!(failed[0]["client"], "claude-code");
    assert!(w.home(STYLE_FILE).is_file());
    assert!(w.home(".claude/skills/nacht/SKILL.md").is_file());

    fs::remove_file(w.home(".claude/agents")).unwrap();
    let retry = w.sync(Args {
        only: Some(vec!["agent:scout".into()]),
        ..w.args()
    });
    assert!(retry["plan"]["failed"].as_array().unwrap().is_empty());
    assert!(w.home(AGENT_FILE).is_file());
}

/// A Claude folder moved by `CLAUDE_CONFIG_DIR`, with a decoy `~/.claude` that must stay as it is.
struct Moved {
    w: World,
    profile: PathBuf,
    _dir: ClaudeDirGuard,
}

impl Moved {
    fn new(tag: &str) -> Self {
        let w = World::new(tag);
        let profile = w.base.join("profile");
        fs::create_dir_all(&profile).unwrap();
        fs::create_dir_all(w.home(".claude/agents")).unwrap();
        fs::write(w.home(AGENT_FILE), "decoy scout\n").unwrap();
        let guard = enter_claude_dir(&profile);
        Self {
            w,
            profile,
            _dir: guard,
        }
    }

    fn decoy_intact(&self) {
        assert_eq!(
            fs::read_to_string(self.w.home(AGENT_FILE)).unwrap(),
            "decoy scout\n"
        );
        assert!(!self.w.home(".claude/skills").exists());
        assert!(!self.w.home(STYLE_FILE).exists());
    }
}

#[test]
fn test08_a_user_sync_writes_skills_agents_and_styles_into_the_config_dir() {
    let m = Moved::new("moved-sync");
    let data = m.w.sync(m.w.args());
    assert!(m.profile.join("skills/nacht/SKILL.md").is_file());
    assert!(m.profile.join("agents/scout.md").is_file());
    assert!(m.profile.join("output-styles/plain.md").is_file());
    assert_eq!(data["verify"]["clean"], true);
    assert_eq!(data["verify"]["total"], 3);
    m.decoy_intact();
    assert_eq!(m.w.preview()["plan"]["counts"]["unchanged"], 3);
    let status = api::status(&m.w.args()).unwrap();
    assert_eq!(status["drift"], false);
    fs::write(m.profile.join("output-styles/plain.md"), "changed\n").unwrap();
    let verify = api::verify(&m.w.args()).unwrap();
    assert_eq!(verify["mismatched"][0]["skill"], "plain");
}

#[test]
fn test09_files_compare_undo_clean_and_uninstall_look_where_the_sync_wrote() {
    let m = Moved::new("moved-look");
    m.w.sync(m.w.args());
    let args = Args {
        name: "nacht".into(),
        global: true,
        ..m.w.args()
    };
    let files = super::files::skill_files(&args).unwrap();
    let dir = files["installed"][0]["dir"].as_str().unwrap();
    assert_eq!(
        PathBuf::from(dir),
        m.profile.join("skills/nacht"),
        "{files}"
    );
    assert_eq!(files["installed"][0]["present"], true);
    let compared = super::compare::compare(&args, Some("SKILL.md")).unwrap();
    assert_eq!(compared["files"][0]["identical"], true, "{compared}");

    let undone = api::undo(&m.w.args(), Some(&MemoryHost::default())).unwrap();
    assert_eq!(undone["removed"], 3);
    assert!(!m.profile.join("skills").exists());
    assert!(!m.profile.join("agents").exists());
    assert!(m.profile.is_dir());
    m.decoy_intact();

    m.w.sync(m.w.args());
    let params = |extra: Value| {
        let mut args = json!({"repo_path": m.w.repo()});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        args
    };
    let preview = super::state::clean(params(json!({"dry_run": true}))).unwrap();
    assert!(preview["removed"].to_string().contains(
        &m.profile
            .join("skills/nacht/SKILL.md")
            .to_string_lossy()
            .to_string()
    ));
    let cleaned = super::state::clean(params(json!({}))).unwrap();
    assert!(!cleaned["removed"].as_array().unwrap().is_empty());
    assert!(!m.profile.join("skills/nacht").exists());
    m.decoy_intact();

    m.w.sync(m.w.args());
    let uninstalled = super::state::uninstall_handler(params(json!({"name": "nacht"}))).unwrap();
    assert!(uninstalled["outputs"].to_string().contains(
        &m.profile
            .join("skills/nacht/SKILL.md")
            .to_string_lossy()
            .to_string()
    ));
    assert!(!m.profile.join("skills/nacht").exists());
    m.decoy_intact();
}

#[test]
fn test10_a_project_sync_stays_in_the_project_even_with_a_config_dir() {
    let m = Moved::new("moved-project");
    m.w.sync(Args {
        global: false,
        ..m.w.args()
    });
    assert!(m.w.repo().join(".claude/skills/nacht/SKILL.md").is_file());
    assert!(m.w.repo().join(".claude/agents/scout.md").is_file());
    assert!(!m.profile.join("skills").exists());
    m.decoy_intact();
}
