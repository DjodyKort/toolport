use super::*;
use crate::clients::EnvRestore;
use crate::plus::direct::tests::{tree, world};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn call(args: Value) -> Result<Value, ToolError> {
    call_tool("context_measure", &args)
}

fn kind(result: Result<Value, ToolError>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(e) => e.kind,
    }
}

struct Stub {
    log: PathBuf,
    _vars: Vec<EnvRestore>,
}

impl Stub {
    fn install(home: &Path) -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/loads/claude-stub.sh");
        let bin = home.join("bin/claude");
        let log = home.join("claude-stub.log");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\nexec sh '{}' \"$@\"\n", script.display()),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _vars: vec![
                EnvRestore::set("TOOLPORT_CLAUDE_BIN", &bin),
                EnvRestore::set("STUB_CLAUDE_LOG", &log),
            ],
            log,
        }
    }

    fn requests(&self) -> usize {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("request"))
            .count()
    }
}

fn repo(home: &Path) -> PathBuf {
    let cwd = home.join("work/client-repo");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    cwd
}

#[test]
fn context_measure_is_a_tier_two_tool_without_a_confirm_or_dry_run_parameter() {
    let tool = find_tool("context_measure").unwrap();
    assert_eq!((tool.tier, tool.gate), (2, Gate::None));
    let schema = catalog::input_schema(tool);
    assert_eq!(schema["required"], json!(["cwd"]));
    for absent in ["yes", "dry_run", "confirm"] {
        assert!(schema["properties"].get(absent).is_none(), "{absent}");
    }
    assert!(tool.description.contains("spends model tokens"));
}

#[test]
fn a_call_measures_the_variants_and_a_second_call_is_answered_from_the_cache() {
    world(|w| {
        let stub = Stub::install(&w.home);
        let cwd = repo(&w.home);
        let args = json!({"cwd": cwd, "without": ["skill:skill-1*"]});

        let first = call_tool("context_measure", &args).unwrap();
        assert_eq!(stub.requests(), 2);
        assert_eq!(first["cached"], false);
        assert_eq!(first["runs"][0]["total"], 68_445);
        assert_eq!(first["runs"][1]["label"], "without skill:skill-1*");
        assert!(first["deltas"][0]["tokens"].as_i64().unwrap() < 0, "{first}");

        let second = call(args).unwrap();
        assert_eq!(stub.requests(), 2, "the cache answers without a request");
        assert_eq!(second["cached"], true);
        assert_eq!(second["deltas"], first["deltas"]);

        let forced = call(json!({"cwd": cwd, "force": true})).unwrap();
        assert_eq!(stub.requests(), 3);
        assert_eq!(forced["cached"], false);
    });
}

#[test]
fn a_call_leaves_the_home_directory_alone_and_writes_only_the_measure_cache() {
    world(|w| {
        let stub = Stub::install(&w.home);
        let cwd = repo(&w.home);
        let home = |stub: &Stub| {
            tree(&w.home)
                .into_iter()
                .filter(|(path, _)| path != &stub.log)
                .collect::<Vec<_>>()
        };
        let before = home(&stub);
        call(json!({"cwd": cwd})).unwrap();
        assert_eq!(home(&stub), before);
        assert!(w.data.join("plus/cache/measure").is_dir());
    });
}

#[test]
fn bad_arguments_and_a_missing_claude_are_refused_before_any_request() {
    world(|w| {
        let stub = Stub::install(&w.home);
        let cwd = repo(&w.home);
        assert_eq!(kind(call(json!({}))), "invalid_arguments");
        assert_eq!(kind(call(json!({"cwd": cwd, "yes": true}))), "invalid_arguments");
        assert_eq!(
            kind(call(json!({"cwd": cwd, "without": ["bogus"]}))),
            "invalid_arguments"
        );
        assert_eq!(
            kind(call(json!({"cwd": w.home.join("missing")}))),
            "invalid_arguments"
        );
        assert_eq!(
            kind(call(json!({"cwd": cwd, "bundle": "nowhere"}))),
            "invalid_arguments"
        );
        assert_eq!(stub.requests(), 0);

        let _gone = EnvRestore::set("TOOLPORT_CLAUDE_BIN", &w.home.join("bin/absent"));
        assert_eq!(kind(call(json!({"cwd": cwd}))), "backend_error");
    });
}
