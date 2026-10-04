#![cfg(unix)]

//! `toolportctl context measure` against the stream-json `claude` stub: what the command asks
//! Claude Code, what it caches, when a cached answer is stale and that nothing starts without a
//! yes. The real run is a Mac check; the world is synthetic (`common/loads_world.rs`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

#[path = "common/claude_stub.rs"]
mod claude_stub;
#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/loads_world.rs"]
mod loads_world;

use claude_stub::ClaudeStub;
use ctl_world::CtlWorld;

struct Fx {
    world: CtlWorld,
    stub: ClaudeStub,
    client: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let world = CtlWorld::new(tag, env!("CARGO_BIN_EXE_mock-mcp-server"));
        let built = loads_world::build_in(
            &world.base,
            conduit_lib::plus::context::layers::MANAGED_LOCAL_HEADER,
        );
        let unlisted = built.claude.join("skills/handoff/SKILL.md");
        std::fs::create_dir_all(unlisted.parent().unwrap()).unwrap();
        std::fs::write(
            unlisted,
            "---\nname: handoff\ndescription: \"Write a handoff\nfor the next session\"\n---\nBody\n",
        )
        .unwrap();
        let stub = ClaudeStub::install(&world.claude, &world.base);
        Self {
            client: built.client,
            world,
            stub,
        }
    }

    fn run(&self, args: &[&str]) -> (i32, Value) {
        self.run_with(args, &[])
    }

    fn run_with(&self, args: &[&str], env: &[(&str, &str)]) -> (i32, Value) {
        let out = Command::new(env!("CARGO_BIN_EXE_toolportctl"))
            .arg("--json")
            .args(args)
            .env_clear()
            .envs(self.world.env())
            .envs(env.iter().copied())
            .current_dir(&self.world.home)
            .stdin(Stdio::null())
            .output()
            .expect("spawn toolportctl");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let envelope: Value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("{args:?}: not one envelope ({e}): {stdout}"));
        (out.status.code().expect("exit code, not a signal"), envelope)
    }

    fn measure(&self, extra: &[&str]) -> (i32, Value) {
        let cwd = self.client.display().to_string();
        let mut args = vec!["context", "measure", "--cwd", cwd.as_str()];
        args.extend_from_slice(extra);
        self.run(&args)
    }
}

fn totals(data: &Value) -> Vec<(String, u64)> {
    data["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["label"].as_str().unwrap().to_string(), r["total"].as_u64().unwrap()))
        .collect()
}

fn names(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect()
}

#[test]
fn measure_reports_runs_deltas_and_which_skills_the_model_sees() {
    let fx = Fx::new("measure-basic");
    let (code, envelope) = fx.measure(&["--without", "plugin:kit@market", "--yes"]);
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["command"], "context measure");
    let data = &envelope["data"];

    assert_eq!(
        totals(data),
        vec![
            ("as is".to_string(), 68_445),
            ("without plugin:kit@market".to_string(), 59_794)
        ]
    );
    let as_is = &data["runs"][0];
    assert_eq!(as_is["parts"]["input"], 10);
    assert_eq!(as_is["parts"]["cacheCreation"], 54_639);
    assert_eq!(as_is["parts"]["cacheRead"], 13_796);
    assert_eq!(as_is["plugins"][0]["source"], "kit@market");
    assert_eq!(as_is["mcpServers"][0]["status"], "connected");
    assert_eq!(as_is["skills"], 32);
    assert_eq!(as_is["agents"], 2);
    assert_eq!(data["runs"][1]["plugins"].as_array().unwrap().len(), 0);

    assert_eq!(data["deltas"][0]["label"], "without plugin:kit@market");
    assert_eq!(data["deltas"][0]["tokens"], -8_651);
    assert_eq!(data["deltas"][0]["percent"], -12.6);

    let visible = names(&data["visibleSkills"]);
    assert!(visible.contains(&"skill-01") && visible.contains(&"kit:kit-build"));
    assert!(!visible.contains(&"handoff"));
    let invisible = data["invisibleSkills"].as_array().unwrap();
    assert_eq!(invisible.len(), 1, "{invisible:?}");
    assert_eq!(invisible[0]["name"], "handoff");
    assert!(
        invisible[0]["reason"].as_str().unwrap().contains("multi-line"),
        "{invisible:?}"
    );

    assert_eq!(data["cached"], false);
    assert_eq!(data["stale"], false);
    assert_eq!(data["claudeCodeVersion"], "2.1.289");
    assert_eq!(data["model"], "claude-haiku-4-5-20251001");

    let requests = fx.stub.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    let physical = std::fs::canonicalize(&fx.client).unwrap();
    for line in &requests {
        assert!(
            line.contains(&format!("cwd={}", physical.display())),
            "requests start in the folder: {line}"
        );
        assert!(line.contains("model=haiku"), "{line}");
    }
    assert!(requests[0].ends_with("settings=no"), "{}", requests[0]);
    assert!(requests[1].ends_with("settings=yes"), "{}", requests[1]);
}

#[test]
fn the_second_call_hits_the_cache_and_a_new_version_reports_stale() {
    let fx = Fx::new("measure-cache");
    let args = ["--without", "plugin:kit@market"];
    let (_, first) = fx.measure(&[&args[..], &["--yes"]].concat());
    assert_eq!(first["data"]["cached"], false);
    assert_eq!(fx.stub.requests().len(), 2);

    let (code, second) = fx.measure(&args);
    assert_eq!(code, 0, "a cache hit needs no --yes: {second}");
    assert_eq!(second["data"]["cached"], true);
    assert_eq!(second["data"]["stale"], false);
    assert_eq!(second["data"]["runs"], first["data"]["runs"]);
    assert_eq!(second["data"]["measuredAt"], first["data"]["measuredAt"]);
    assert_eq!(fx.stub.requests().len(), 2, "no request on a cache hit");

    fx.stub.set_version("2.2.0");
    let (code, stale) = fx.measure(&args);
    assert_eq!(code, 0, "{stale}");
    assert_eq!(stale["data"]["cached"], true);
    assert_eq!(stale["data"]["stale"], true);
    assert_eq!(stale["data"]["claudeCodeVersion"], "2.1.289");
    assert_eq!(fx.stub.requests().len(), 2, "a stale answer is still free");

    let (code, loads) = fx.run(&[
        "context",
        "loads",
        "--cwd",
        &fx.client.display().to_string(),
        "--measured",
    ]);
    assert_eq!(code, 0, "{loads}");
    assert_eq!(loads["data"]["measured_info"]["stale"], true);
    assert_eq!(loads["data"]["measured"]["total"], 68_445);

    let (code, fresh) = fx.measure(&[&args[..], &["--force", "--yes"]].concat());
    assert_eq!(code, 0, "{fresh}");
    assert_eq!(fresh["data"]["cached"], false);
    assert_eq!(fresh["data"]["stale"], false);
    assert_eq!(fresh["data"]["claudeCodeVersion"], "2.2.0");
    assert_eq!(fx.stub.requests().len(), 4);
}

#[test]
fn no_request_is_made_without_yes_outside_a_terminal() {
    let fx = Fx::new("measure-confirm");
    let (code, envelope) = fx.measure(&["--without", "plugin:kit@market"]);
    assert_eq!(code, 1, "{envelope}");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "confirm_required");
    assert!(envelope["error"]["message"].as_str().unwrap().contains("--yes"));
    assert!(
        fx.stub.requests().is_empty(),
        "nothing may be asked of Claude Code: {:?}",
        fx.stub.calls()
    );
    assert!(
        !fx.world.data.join("plus/cache/measure").exists(),
        "nothing was measured, so nothing is cached"
    );
}

#[test]
fn a_skill_variant_and_force_each_make_their_own_requests() {
    let fx = Fx::new("measure-skill");
    let (code, first) = fx.measure(&["--without", "skill:skill-1*", "--yes"]);
    assert_eq!(code, 0, "{first}");
    assert_eq!(
        totals(&first["data"]),
        vec![
            ("as is".to_string(), 68_445),
            ("without skill:skill-1*".to_string(), 68_386)
        ]
    );
    assert_eq!(first["data"]["deltas"][0]["tokens"], -59);
    let requests = fx.stub.requests();
    assert!(requests[1].ends_with("settings=yes"), "{requests:?}");

    let (_, again) = fx.measure(&["--without", "skill:skill-1*", "--yes"]);
    assert_eq!(again["data"]["cached"], true, "--yes alone does not measure again");
    assert_eq!(fx.stub.requests().len(), 2);
    let (_, forced) = fx.measure(&["--without", "skill:skill-1*", "--force", "--yes"]);
    assert_eq!(forced["data"]["cached"], false);
    assert_eq!(fx.stub.requests().len(), 4);
}

fn tree(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            tree(&path, out);
        } else {
            out.push((path.clone(), std::fs::read(&path).unwrap()));
        }
    }
}

#[test]
fn measuring_writes_nothing_in_the_folder() {
    let fx = Fx::new("measure-readonly");
    let mut before = Vec::new();
    tree(&fx.client, &mut before);
    let (code, envelope) = fx.measure(&["--without", "plugin:kit@market", "--yes"]);
    assert_eq!(code, 0, "{envelope}");
    let mut after = Vec::new();
    tree(&fx.client, &mut after);
    assert!(before == after, "the folder changed while it was measured");
}

#[test]
fn loads_measured_attaches_the_cached_run_and_reads_the_window_from_the_model() {
    let fx = Fx::new("measure-loads");
    let cwd = fx.client.display().to_string();
    let (_, plain) = fx.run(&["context", "loads", "--cwd", &cwd, "--measured"]);
    assert!(plain["data"]["measured"].is_null(), "nothing measured yet");
    assert!(plain["data"]["measured_info"].is_null());
    assert!(fx.stub.requests().is_empty(), "--measured never starts a measurement");
    assert_eq!(plain["data"]["skill_budget"]["context_window"], 200_000);

    let (code, _) = fx.measure(&["--model", "sonnet[1m]", "--yes"]);
    assert_eq!(code, 0);
    let (_, loads) = fx.run(&["context", "loads", "--cwd", &cwd, "--measured"]);
    let data = &loads["data"];
    assert_eq!(data["measured"]["label"], "as is");
    assert_eq!(data["measured"]["total"], 68_445);
    assert_eq!(data["measured_info"]["stale"], false);
    assert_eq!(data["skill_budget"]["context_window"], 1_000_000);
    assert_eq!(data["skill_budget"]["limit_tokens"], 10_000);
    assert_eq!(data["basis"], "estimate", "the estimate stays an estimate beside it");

    let (_, without) = fx.run(&["context", "loads", "--cwd", &cwd]);
    assert!(without["data"]["measured"].is_null(), "only --measured attaches it");
    assert_eq!(without["data"]["skill_budget"]["context_window"], 200_000);
    assert_eq!(fx.stub.requests().len(), 1);
}

#[test]
fn a_missing_claude_is_a_failed_envelope_and_a_bad_variant_a_usage_error() {
    let fx = Fx::new("measure-errors");
    let missing = fx.world.base.join("no-such-claude").display().to_string();
    let cwd = fx.client.display().to_string();
    let (code, envelope) = fx.run_with(
        &["context", "measure", "--cwd", &cwd, "--yes"],
        &[("TOOLPORT_CLAUDE_BIN", &missing)],
    );
    assert_eq!(code, 1, "{envelope}");
    assert_eq!(envelope["error"]["code"], "claude_unavailable");

    let (code, envelope) = fx.measure(&["--without", "ecc", "--yes"]);
    assert_eq!(code, 2, "{envelope}");
    assert_eq!(envelope["error"]["code"], "usage");
    assert!(fx.stub.requests().is_empty());
}
