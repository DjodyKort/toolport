//! `context measure` against a fake launcher: what it asks Claude Code, what it caches, and when
//! a cached answer counts as stale. The process side (argv, working directory, the real stream
//! format) is proven by the stub-driven tests in `tests/context_measure.rs`.

use super::measure::{
    cached_as_is, measure, parse_stream, window_for_model, Env, Launcher, MeasureError,
    MeasureReport, Request,
};
use super::tests_loads::{config, skill, Home};
use crate::plus::exec::CmdOutput;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

struct Seen {
    cwd: PathBuf,
    model: String,
    settings: Option<Value>,
    settings_file: Option<PathBuf>,
}

type Respond = Box<dyn Fn(Option<&Value>) -> String>;

struct Fake {
    version: RefCell<String>,
    seen: RefCell<Vec<Seen>>,
    respond: Respond,
}

impl Fake {
    fn new(respond: Respond) -> Self {
        Self {
            version: RefCell::new("2.1.0 (Claude Code)".to_string()),
            seen: RefCell::new(Vec::new()),
            respond,
        }
    }

    fn requests(&self) -> usize {
        self.seen.borrow().len()
    }
}

impl Launcher for Fake {
    fn version(&self) -> Result<String, String> {
        Ok(self.version.borrow().split_whitespace().next().unwrap().to_string())
    }

    fn request(
        &self,
        cwd: &Path,
        model: &str,
        settings: Option<&Path>,
    ) -> Result<CmdOutput, String> {
        let content = settings.map(|p| serde_json::from_str::<Value>(&fs::read_to_string(p).unwrap()).unwrap());
        let stdout = (self.respond)(content.as_ref());
        self.seen.borrow_mut().push(Seen {
            cwd: cwd.to_path_buf(),
            model: model.to_string(),
            settings: content,
            settings_file: settings.map(Path::to_path_buf),
        });
        Ok(CmdOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        })
    }
}

fn stream(total: [u64; 3], skills: &[&str], slash: &[&str], plugins: &[&str]) -> String {
    let init = json!({
        "type": "system", "subtype": "init", "cwd": "/x", "model": "claude-haiku-4-5",
        "claude_code_version": "2.1.0",
        "skills": skills, "agents": ["reviewer"], "slash_commands": slash, "tools": ["Bash"],
        "plugins": plugins.iter().map(|p| json!({
            "name": p.split('@').next().unwrap(), "path": "/p", "source": p, "version": "1"
        })).collect::<Vec<_>>(),
        "mcp_servers": [{"name": "docs-search", "status": "connected"}],
    });
    let result = json!({
        "type": "result", "subtype": "success", "is_error": false, "duration_ms": 1234,
        "usage": {
            "input_tokens": total[0],
            "cache_creation_input_tokens": total[1],
            "cache_read_input_tokens": total[2],
        },
    });
    format!("warning: something on stdout\n{init}\n{result}\n")
}

/// 60,000 tokens as is, 51,000 when the `ecc@ecc` plugin is off.
fn plugin_aware() -> Fake {
    Fake::new(Box::new(|settings| {
        let off = settings.is_some_and(|s| s["enabledPlugins"]["ecc@ecc"] == json!(false));
        if off {
            stream([10, 20_000, 30_990], &["alpha"], &["alpha"], &[])
        } else {
            stream([10, 30_000, 29_990], &["alpha", "beta"], &["alpha", "beta"], &["ecc@ecc"])
        }
    }))
}

struct World {
    h: Home,
    cwd: PathBuf,
    data: PathBuf,
}

fn world() -> World {
    let h = Home::new();
    let cwd = h.repo("work/repo");
    let data = h.at("data");
    World { h, cwd, data }
}

impl World {
    fn run(&self, fake: &Fake, req: &Request) -> Result<MeasureReport, MeasureError> {
        self.run_asking(fake, req, &mut |_| true)
    }

    fn run_asking(
        &self,
        fake: &Fake,
        req: &Request,
        approve: &mut dyn FnMut(usize) -> bool,
    ) -> Result<MeasureReport, MeasureError> {
        let roots = self.h.roots();
        let config = config(json!({}));
        let env = Env {
            roots: &roots,
            config: &config,
            data_dir: Some(&self.data),
            launcher: fake,
        };
        measure(&env, req, approve)
    }

    fn request(&self) -> Request {
        Request {
            cwd: self.cwd.clone(),
            ..Default::default()
        }
    }
}

#[test]
fn the_total_is_the_first_requests_input_in_three_parts_and_noise_lines_are_ignored() {
    let text = stream([10, 54_639, 13_796], &["a", "b"], &["a", "b", "compact"], &["ecc@ecc"]);
    let parsed = parse_stream("as is", &text, 5).unwrap();
    assert_eq!(parsed.run.total, 68_445);
    assert_eq!(parsed.run.parts.input, 10);
    assert_eq!(parsed.run.parts.cache_creation, 54_639);
    assert_eq!(parsed.run.parts.cache_read, 13_796);
    assert_eq!(
        (parsed.run.skills, parsed.run.agents, parsed.run.slash_commands),
        (2, 1, 3)
    );
    assert_eq!(parsed.run.plugins[0].source, "ecc@ecc");
    assert_eq!(parsed.run.mcp_servers[0].status, "connected");
    assert_eq!(parsed.run.duration_ms, 1234);
    assert_eq!(parsed.model, "claude-haiku-4-5");
}

#[test]
fn an_error_result_or_a_missing_result_is_an_error() {
    let failed = r#"{"type":"system","subtype":"init","skills":[]}
{"type":"result","is_error":true,"result":"Not logged in\nplease run /login"}
"#;
    assert_eq!(parse_stream("as is", failed, 1).unwrap_err(), "Not logged in");
    let cut = r#"{"type":"system","subtype":"init","skills":[]}"#;
    assert!(parse_stream("as is", cut, 1).unwrap_err().contains("no result"));
    assert!(parse_stream("as is", "", 1).is_err());
}

#[test]
fn a_plugin_variant_runs_with_a_temporary_settings_file_and_reports_a_signed_delta() {
    let w = world();
    let fake = plugin_aware();
    let req = Request {
        without: vec!["plugin:ecc@ecc".into()],
        ..w.request()
    };
    let report = w.run(&fake, &req).unwrap();
    assert_eq!(
        report.runs.iter().map(|r| (r.label.as_str(), r.total)).collect::<Vec<_>>(),
        vec![("as is", 60_000), ("without plugin:ecc@ecc", 51_000)]
    );
    assert_eq!(report.deltas.len(), 1);
    assert_eq!(report.deltas[0].label, "without plugin:ecc@ecc");
    assert_eq!(report.deltas[0].tokens, -9_000);
    assert_eq!(report.deltas[0].percent, -15.0);
    assert!(!report.cached && !report.stale);
    assert_eq!(report.model, "claude-haiku-4-5");
    assert_eq!(report.claude_code_version, "2.1.0");
    assert_eq!(report.visible_skills, vec!["alpha", "beta"]);

    let seen = fake.seen.borrow();
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().all(|s| s.cwd == fs::canonicalize(&w.cwd).unwrap()));
    assert!(seen.iter().all(|s| s.model == "haiku"));
    assert!(seen[0].settings.is_none());
    assert_eq!(seen[1].settings, Some(json!({"enabledPlugins": {"ecc@ecc": false}})));
}

#[test]
fn nothing_is_written_in_the_folder_and_the_temporary_settings_file_is_removed() {
    let w = world();
    fs::create_dir_all(w.cwd.join(".claude")).unwrap();
    fs::write(w.cwd.join(".claude/settings.json"), "{}\n").unwrap();
    let listing = |dir: &Path| {
        let mut names: Vec<String> = walk(dir);
        names.sort();
        names
    };
    let before = listing(&w.cwd);
    let fake = plugin_aware();
    let req = Request {
        without: vec!["plugin:ecc@ecc".into()],
        ..w.request()
    };
    w.run(&fake, &req).unwrap();
    assert_eq!(listing(&w.cwd), before);
    let file = fake.seen.borrow()[1].settings_file.clone().unwrap();
    assert!(!file.exists(), "the temporary settings file was left behind");
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        out.push(path.display().to_string());
        if path.is_dir() && entry.file_name() != ".git" {
            out.extend(walk(&path));
        }
    }
    out
}

#[test]
fn the_second_call_is_answered_from_the_cache_without_asking() {
    let w = world();
    let fake = plugin_aware();
    let req = Request {
        without: vec!["plugin:ecc@ecc".into()],
        ..w.request()
    };
    let first = w.run(&fake, &req).unwrap();
    assert!(!first.cached);
    assert_eq!(fake.requests(), 2);

    let mut asked = Vec::new();
    let second = w
        .run_asking(&fake, &req, &mut |n| {
            asked.push(n);
            true
        })
        .unwrap();
    assert!(second.cached && !second.stale);
    assert!(asked.is_empty(), "a cache hit must not ask to spend requests");
    assert_eq!(fake.requests(), 2, "a cache hit makes no request");
    assert_eq!(second.runs, first.runs);
    assert_eq!(second.deltas, first.deltas);
    assert_eq!(second.measured_at, first.measured_at);
}

#[test]
fn only_the_missing_variant_is_measured_and_asked_for() {
    let w = world();
    let fake = plugin_aware();
    w.run(&fake, &w.request()).unwrap();
    assert_eq!(fake.requests(), 1);
    let req = Request {
        without: vec!["plugin:ecc@ecc".into()],
        ..w.request()
    };
    let mut asked = Vec::new();
    let report = w
        .run_asking(&fake, &req, &mut |n| {
            asked.push(n);
            true
        })
        .unwrap();
    assert_eq!(asked, vec![1]);
    assert_eq!(fake.requests(), 2);
    assert!(!report.cached);
    assert_eq!(report.runs.len(), 2);
}

#[test]
fn a_new_claude_code_version_serves_the_cached_run_as_stale_until_forced() {
    let w = world();
    let fake = plugin_aware();
    let req = w.request();
    let first = w.run(&fake, &req).unwrap();
    assert!(!first.stale);

    *fake.version.borrow_mut() = "2.2.0 (Claude Code)".to_string();
    let after = w
        .run_asking(&fake, &req, &mut |_| panic!("serving a stale run must not ask"))
        .unwrap();
    assert!(after.stale && after.cached);
    assert_eq!(after.claude_code_version, "2.1.0");
    assert_eq!(after.runs, first.runs);
    assert_eq!(fake.requests(), 1);

    let forced = Request {
        force: true,
        ..req.clone()
    };
    let fresh = w.run(&fake, &forced).unwrap();
    assert!(!fresh.stale && !fresh.cached);
    assert_eq!(fake.requests(), 2);
    assert_eq!(fresh.claude_code_version, "2.2.0");

    let again = w.run(&fake, &req).unwrap();
    assert!(again.cached && !again.stale);
    assert_eq!(fake.requests(), 2);
    let kept = fs::read_dir(w.data.join("plus/cache/measure")).unwrap().count();
    assert_eq!(kept, 1, "the entry of the old version is replaced, not piled up");
}

#[test]
fn a_changed_settings_file_of_the_folder_makes_the_cached_run_stale() {
    let w = world();
    let fake = plugin_aware();
    w.run(&fake, &w.request()).unwrap();
    fs::create_dir_all(w.cwd.join(".claude")).unwrap();
    fs::write(
        w.cwd.join(".claude/settings.local.json"),
        "{\"enabledPlugins\": {\"ecc@ecc\": false}}\n",
    )
    .unwrap();
    let report = w.run(&fake, &w.request()).unwrap();
    assert!(report.cached && report.stale);
    assert_eq!(fake.requests(), 1);
}

#[test]
fn declining_stops_before_any_request_and_force_measures_again() {
    let w = world();
    let fake = plugin_aware();
    let req = Request {
        without: vec!["plugin:ecc@ecc".into()],
        ..w.request()
    };
    let mut asked = Vec::new();
    let err = w
        .run_asking(&fake, &req, &mut |n| {
            asked.push(n);
            false
        })
        .unwrap_err();
    assert!(matches!(err, MeasureError::Declined(2)));
    assert_eq!(asked, vec![2]);
    assert_eq!(fake.requests(), 0);
    assert!(!w.data.join("plus/cache/measure").exists());

    w.run(&fake, &req).unwrap();
    let forced = Request {
        force: true,
        ..req
    };
    let mut asked = Vec::new();
    w.run_asking(&fake, &forced, &mut |n| {
        asked.push(n);
        true
    })
    .unwrap();
    assert_eq!(asked, vec![2], "force spends the requests again");
    assert_eq!(fake.requests(), 4);
}

#[test]
fn a_skill_pattern_hides_only_the_listed_skills_that_match() {
    let w = world();
    let fake = Fake::new(Box::new(|settings| {
        let hidden: Vec<String> = settings
            .and_then(|s| s["skillOverrides"].as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        let listed: Vec<&str> = ["odoo-a", "odoo-b", "notes", "kit:plug"]
            .into_iter()
            .filter(|n| !hidden.iter().any(|h| h == n))
            .collect();
        stream([10, 0, 1_000 + 100 * listed.len() as u64], &listed, &listed, &[])
    }));
    let req = Request {
        without: vec!["skill:odoo-*".into(), "skill:*".into(), "skill:none-*".into()],
        ..w.request()
    };
    let report = w.run(&fake, &req).unwrap();
    let seen = fake.seen.borrow();
    assert_eq!(
        seen[1].settings,
        Some(json!({"skillOverrides": {"odoo-a": "off", "odoo-b": "off"}}))
    );
    assert_eq!(
        seen[2].settings,
        Some(json!({"skillOverrides": {"notes": "off", "odoo-a": "off", "odoo-b": "off"}})),
        "plugin skills are turned off through their plugin, never through a pattern"
    );
    assert_eq!(seen.len(), 3, "a pattern with no match makes no request");
    assert!(report.notes.iter().any(|n| n.contains("skill:none-*")));
    assert_eq!(report.runs.len(), 3);
    assert_eq!(report.deltas.iter().map(|d| d.tokens).collect::<Vec<_>>(), vec![-200, -300]);
}

#[test]
fn a_bundle_becomes_settings_keys_and_layers_add_is_reported() {
    let w = world();
    w.h.put(
        ".config/mcpm/skills_repo/profiles/odoo-dev.yaml",
        "format: 1\nname: odoo-dev\nskills:\n  off: [\"scratch-*\", legacy]\n  name_only: [wide]\nplugins:\n  off: [\"ecc@ecc\"]\n  config:\n    ecc@ecc: {hook_profile: minimal, gateguard: off}\nmcp:\n  deny: [\"plugin:ecc:chrome-devtools\"]\nlayers:\n  add: [odh-knowledge]\n  exclude: [\"**/parent/CLAUDE.md\"]\nagents:\n  off: [reviewer]\n",
    );
    w.h.put(".config/mcpm/skills_repo/skills/scratch-one/SKILL.md", &skill("scratch-one", "x"));
    w.h.put(".config/mcpm/skills_repo/skills/wide/SKILL.md", &skill("wide", "x"));
    let fake = plugin_aware();
    let req = Request {
        bundle: Some("odoo-dev".into()),
        ..w.request()
    };
    let report = w.run(&fake, &req).unwrap();
    assert_eq!(report.runs[1].label, "bundle odoo-dev");
    assert_eq!(
        fake.seen.borrow()[1].settings,
        Some(json!({
            "skillOverrides": {"legacy": "off", "scratch-one": "off", "wide": "name-only"},
            "enabledPlugins": {"ecc@ecc": false},
            "claudeMdExcludes": ["**/parent/CLAUDE.md"],
            "env": {"ECC_GATEGUARD": "off", "ECC_HOOK_PROFILE": "minimal"},
            "deniedMcpServers": [{"serverName": "plugin:ecc:chrome-devtools"}],
            "permissions": {"deny": ["Agent(reviewer)"]},
        }))
    );
    assert!(report.notes.iter().any(|n| n.contains("layers.add") && n.contains("odh-knowledge")));
}

#[test]
fn an_allow_list_bundle_turns_off_every_other_library_skill() {
    let w = world();
    w.h.put(".config/mcpm/skills_repo/profiles/lean.yaml", "skills:\n  allow: [keep]\n");
    for name in ["keep", "drop-a", "drop-b"] {
        w.h.put(&format!(".config/mcpm/skills_repo/skills/{name}/SKILL.md"), &skill(name, "x"));
    }
    let fake = plugin_aware();
    let req = Request {
        bundle: Some("lean".into()),
        ..w.request()
    };
    w.run(&fake, &req).unwrap();
    assert_eq!(
        fake.seen.borrow()[1].settings,
        Some(json!({"skillOverrides": {"drop-a": "off", "drop-b": "off"}}))
    );
}

#[test]
fn editing_a_bundle_measures_it_again_instead_of_serving_the_old_number() {
    let w = world();
    let path = ".config/mcpm/skills_repo/profiles/lean.yaml";
    w.h.put(path, "plugins:\n  off: [\"ecc@ecc\"]\n");
    let fake = plugin_aware();
    let req = Request {
        bundle: Some("lean".into()),
        ..w.request()
    };
    w.run(&fake, &req).unwrap();
    assert_eq!(fake.requests(), 2);
    w.run(&fake, &req).unwrap();
    assert_eq!(fake.requests(), 2);
    w.h.put(path, "plugins:\n  off: [\"ecc@ecc\", \"other@market\"]\n");
    w.run(&fake, &req).unwrap();
    assert_eq!(fake.requests(), 3, "only the bundle run repeats");
}

#[test]
fn skills_that_are_commands_but_not_listed_are_invisible_with_the_reason() {
    let w = world();
    w.h.put(
        ".claude/skills/handoff/SKILL.md",
        "---\nname: handoff\ndescription: \"Write a handoff\nfor the next session\"\n---\nBody\n",
    );
    w.h.put(".claude/skills/fine/SKILL.md", &skill("fine", "A skill that works"));
    w.h.put(".claude/skills/quiet/SKILL.md", &skill("quiet", "Valid but not listed"));
    let fake = Fake::new(Box::new(|_| {
        stream([10, 0, 500], &["fine"], &["fine", "handoff", "quiet", "compact", "init"], &[])
    }));
    let report = w.run(&fake, &w.request()).unwrap();
    assert_eq!(report.visible_skills, vec!["fine"]);
    let names: Vec<&str> = report.invisible_skills.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, vec!["handoff", "quiet"], "built-in commands are not skills");
    assert!(report.invisible_skills[0].reason.contains("multi-line"));
    assert!(report.invisible_skills[1].reason.contains("not offered to the model"));
}

#[test]
fn bad_variants_are_usage_errors_before_claude_is_asked() {
    let w = world();
    let fake = plugin_aware();
    for without in ["ecc@ecc", "plugin:", "skill:", "agent:x"] {
        let req = Request {
            without: vec![without.to_string()],
            ..w.request()
        };
        assert!(
            matches!(w.run(&fake, &req), Err(MeasureError::Usage(_))),
            "{without}"
        );
    }
    for bundle in ["missing", "../escape", ""] {
        let req = Request {
            bundle: Some(bundle.to_string()),
            ..w.request()
        };
        assert!(
            matches!(w.run(&fake, &req), Err(MeasureError::Usage(_))),
            "{bundle}"
        );
    }
    let req = Request {
        cwd: w.h.at("no/such/folder"),
        ..Default::default()
    };
    assert!(matches!(w.run(&fake, &req), Err(MeasureError::Usage(_))));
    assert_eq!(fake.requests(), 0);
}

#[test]
fn the_cached_as_is_run_is_found_without_a_request_and_goes_stale_with_the_version() {
    let w = world();
    assert!(cached_as_is(&w.data, &w.cwd, Some("2.1.0")).is_none());
    let fake = plugin_aware();
    w.run(&fake, &w.request()).unwrap();
    let (run, info) = cached_as_is(&w.data, &w.cwd, Some("2.1.0")).unwrap();
    assert_eq!(run.total, 60_000);
    assert_eq!(info.model, "claude-haiku-4-5");
    assert_eq!(info.claude_code_version, "2.1.0");
    assert!(!info.stale);
    assert!(cached_as_is(&w.data, &w.cwd, Some("2.9.0")).unwrap().1.stale);
    assert!(!cached_as_is(&w.data, &w.cwd, None).unwrap().1.stale);
    assert!(cached_as_is(&w.data, &w.h.at("elsewhere"), None).is_none());
    assert_eq!(fake.requests(), 1);
}

#[test]
fn a_one_million_window_is_read_from_the_model_name() {
    assert_eq!(window_for_model("claude-haiku-4-5"), 200_000);
    assert_eq!(window_for_model("sonnet[1m]"), 1_000_000);
    assert_eq!(window_for_model(""), 200_000);
}
