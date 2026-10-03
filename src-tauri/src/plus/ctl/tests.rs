use super::*;
use serde_json::{json, Value};

const FAKE_SECRET: &str = "FAKE-SECRET-VALUE-do-not-print-7f3a";

struct Fixture {
    base: crate::plus::testutil::DataDirFx,
}

impl std::ops::Deref for Fixture {
    type Target = crate::plus::testutil::DataDirFx;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl Fixture {
    fn new(tag: &str, registry: Option<Value>) -> Self {
        let base = crate::plus::testutil::DataDirFx::new("toolportctl", tag);
        if let Some(registry) = registry {
            base.write_registry(&registry);
        }
        Self { base }
    }
}

fn sample_registry() -> Value {
    json!({
        "version": 1,
        "futureTopLevel": {"keep": [1, 2, 3]},
        "servers": [
            {
                "id": "srv-alpha", "name": "alpha", "transport": "stdio",
                "command": "alpha-mcp", "args": [],
                "env": [{"key": "API_KEY", "value": FAKE_SECRET, "secret": true}],
                "futureServerField": "keep-me"
            },
            {"id": "srv-beta", "name": "beta", "transport": "http", "url": "https://example.invalid/mcp"}
        ],
        "profiles": [
            {"id": "default", "name": "Default", "enabledServerIds": ["srv-alpha"]},
            {"id": "work", "name": "Work", "enabledServerIds": []}
        ],
        "activeProfileId": "default"
    })
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn run_cli(list: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&args(list), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn json_of(list: &[&str]) -> (i32, Value) {
    let (code, out, _) = run_cli(list);
    (code, serde_json::from_str(out.trim()).expect(&out))
}

fn without_machine_paths(mut value: Value, dir: &std::path::Path) -> Value {
    let text = value
        .to_string()
        .replace(&*dir.to_string_lossy(), "<DATA_DIR>");
    value = serde_json::from_str(&text).unwrap();
    if let Some(data) = value.get_mut("data") {
        if let Some(obj) = data.as_object_mut() {
            obj.remove("gateway");
        }
    }
    value
}

#[test]
fn status_json_golden() {
    let fx = Fixture::new("status", Some(sample_registry()));
    let (code, value) = json_of(&["--json", "status"]);
    assert_eq!(code, 0);
    let mut value = without_machine_paths(value, &fx.dir);
    value["data"]["secretsBackend"] = json!("<BACKEND>");
    value["data"]["version"] = json!("<VERSION>");
    assert_eq!(
        value,
        json!({
            "ok": true,
            "command": "status",
            "schemaVersion": 1,
            "data": {
                "version": "<VERSION>",
                "dataDir": "<DATA_DIR>",
                "registry": {
                    "path": "<DATA_DIR>/registry.json",
                    "exists": true,
                    "readable": true,
                    "error": null
                },
                "serverCount": 2,
                "profileCount": 2,
                "activeProfile": "default",
                "secretsBackend": "<BACKEND>",
                "auth": {
                    "counts": {
                        "ok": 0, "expiring": 0, "needs_reauth": 0, "revoked": 0,
                        "misconfigured": 0, "unreachable": 0, "unknown": 0
                    },
                    "servers": []
                }
            }
        })
    );
}

#[test]
fn status_reports_gateway_presence() {
    let fx = Fixture::new("gateway", Some(sample_registry()));
    let (_, value) = json_of(&["--json", "status"]);
    let present_before = value["data"]["gateway"]["present"].as_bool().unwrap();
    if !present_before {
        let bin = fx.dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            bin.join(format!("toolport-gateway{}", std::env::consts::EXE_SUFFIX)),
            b"",
        )
        .unwrap();
        let (_, value) = json_of(&["--json", "status"]);
        assert_eq!(value["data"]["gateway"]["present"], true);
    }
}

#[test]
fn status_on_first_run_has_no_registry_and_creates_nothing() {
    let fx = Fixture::new("first-run", None);
    let (code, value) = json_of(&["--json", "status"]);
    assert_eq!(code, 0);
    assert_eq!(value["data"]["registry"]["exists"], false);
    assert_eq!(value["data"]["serverCount"], 0);
    assert!(value["data"]["activeProfile"].is_null());
    assert_eq!(std::fs::read_dir(&fx.dir).unwrap().count(), 0);
}

#[test]
fn server_ls_json_golden() {
    let fx = Fixture::new("server-ls", Some(sample_registry()));
    let (code, value) = json_of(&["--json", "server", "ls"]);
    assert_eq!(code, 0);
    assert_eq!(
        without_machine_paths(value, &fx.dir),
        json!({
            "ok": true,
            "command": "server ls",
            "schemaVersion": 1,
            "data": {
                "activeProfile": "default",
                "servers": [
                    {"id": "srv-alpha", "name": "alpha", "transport": "stdio", "enabled": true},
                    {"id": "srv-beta", "name": "beta", "transport": "http", "enabled": false}
                ]
            }
        })
    );
}

#[test]
fn server_ls_human_text() {
    let _fx = Fixture::new("server-ls-human", Some(sample_registry()));
    let (code, out, _) = run_cli(&["server", "ls"]);
    assert_eq!(code, 0);
    assert!(out.contains("alpha") && out.contains("enabled"));
    assert!(out.contains("beta") && out.contains("disabled"));
}

#[test]
fn doctor_json_golden() {
    let fx = Fixture::new("doctor", Some(sample_registry()));
    let (_, value) = json_of(&["--json", "doctor"]);
    let value = without_machine_paths(value, &fx.dir);
    assert_eq!(value["command"], "doctor");
    let checks = value["data"]["checks"].as_array().unwrap();
    let names: Vec<&str> = checks.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "dataDir",
            "registry",
            "activeProfile",
            "secretsBackend",
            "gatewayBinary"
        ]
    );
    assert_eq!(checks[0]["status"], "ok");
    assert_eq!(checks[0]["detail"], "<DATA_DIR> is writable");
    assert_eq!(checks[1]["detail"], "2 servers, 2 profiles");
    assert_eq!(
        checks[2],
        json!({"name": "activeProfile", "status": "ok", "detail": "default"})
    );
    assert_eq!(value["data"]["healthy"], true);
    assert_eq!(value["ok"], true);
}

#[test]
fn doctor_fails_on_corrupt_registry_with_data_and_error() {
    let fx = Fixture::new("doctor-bad", None);
    std::fs::write(fx.dir.join("registry.json"), "{not json").unwrap();
    let (code, value) = json_of(&["--json", "doctor"]);
    assert_eq!(code, 1);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "unhealthy");
    assert_eq!(value["data"]["healthy"], false);
}

#[test]
fn server_ls_reports_registry_error() {
    let fx = Fixture::new("ls-bad", None);
    std::fs::write(fx.dir.join("registry.json"), "{not json").unwrap();
    let (code, value) = json_of(&["--json", "server", "ls"]);
    assert_eq!(code, 1);
    assert_eq!(value["error"]["code"], "registry_error");
}

#[test]
fn usage_errors_exit_2() {
    let _fx = Fixture::new("usage", Some(sample_registry()));
    for list in [
        &["--bogus", "status"][..],
        &["frobnicate"][..],
        &["status", "extra"][..],
        &["--data-dir"][..],
        &["--data-dir="][..],
        &[][..],
    ] {
        let (code, _, _) = run_cli(list);
        assert_eq!(code, 2, "{list:?}");
    }
    let (code, value) = json_of(&["--json", "frobnicate"]);
    assert_eq!(code, 2);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "usage");
    let (_, _, stderr) = run_cli(&["frobnicate"]);
    assert!(stderr.contains("unknown command"));
}

#[test]
fn help_and_version_exit_0() {
    let (code, out, _) = run_cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("Usage: toolportctl"));
    let (code, value) = json_of(&["--json", "--version"]);
    assert_eq!(code, 0);
    assert_eq!(value["data"]["name"], "toolportctl");
}

#[test]
fn planned_commands_are_not_implemented() {
    let _fx = Fixture::new("planned", Some(sample_registry()));
    for list in [
        &["compression", "use", "agent"][..],
        &["server", "add"][..],
    ] {
        let (code, out, err) = run_cli(list);
        assert_eq!(code, 1, "{list:?}");
        assert!(out.is_empty());
        assert!(err.contains("not implemented"), "{err}");
    }
    let (code, value) = json_of(&["--json", "secret", "ls"]);
    assert_eq!(code, 1);
    assert_eq!(value["error"]["code"], "not_implemented");
    assert_eq!(value["command"], "secret");
}

#[test]
fn envelope_schema_snapshot() {
    let ok = Envelope::success("status", json!({"x": 1})).to_value();
    assert_eq!(
        ok,
        json!({"ok": true, "command": "status", "schemaVersion": 1, "data": {"x": 1}})
    );
    let err = Envelope::failure("status", CtlError::new("io", "boom")).to_value();
    assert_eq!(
        err,
        json!({
            "ok": false, "command": "status", "schemaVersion": 1,
            "error": {"code": "io", "message": "boom"}
        })
    );
    let keys: Vec<&str> = ok.as_object().unwrap().keys().map(String::as_str).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(sorted, ["command", "data", "ok", "schemaVersion"]);
}

#[test]
fn no_secret_value_ever_reaches_output() {
    let fx = Fixture::new("secrets", Some(sample_registry()));
    for list in [
        &["status"][..],
        &["--json", "status"][..],
        &["doctor"][..],
        &["--json", "doctor"][..],
        &["server", "ls"][..],
        &["--json", "server", "ls"][..],
    ] {
        let (_, out, err) = run_cli(list);
        assert!(!out.contains(FAKE_SECRET), "{list:?}");
        assert!(!err.contains(FAKE_SECRET), "{list:?}");
    }
    drop(fx);
}

#[test]
fn inspection_leaves_registry_bytes_and_unknown_fields_untouched() {
    let fx = Fixture::new("roundtrip", Some(sample_registry()));
    let path = fx.dir.join("registry.json");
    let before = std::fs::read(&path).unwrap();
    for list in [&["status"][..], &["doctor"][..], &["server", "ls"][..]] {
        run_cli(list);
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut names: Vec<String> = std::fs::read_dir(&fx.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["registry.json"]);
    let reg: crate::registry::Registry = serde_json::from_slice(&before).unwrap();
    let again = serde_json::to_value(&reg).unwrap();
    assert_eq!(again["futureTopLevel"], json!({"keep": [1, 2, 3]}));
    assert_eq!(again["servers"][0]["futureServerField"], "keep-me");
}

#[test]
fn parse_handles_flags_in_any_position() {
    let parsed = parse(&args(&["server", "--json", "ls", "--data-dir=/x"])).unwrap();
    assert!(parsed.json);
    assert_eq!(parsed.positional, ["server", "ls"]);
    assert_eq!(parsed.data_dir.as_deref(), Some("/x"));
    let parsed = parse(&args(&["--data-dir", "/y", "status"])).unwrap();
    assert_eq!(parsed.data_dir.as_deref(), Some("/y"));
}

#[test]
fn command_table_paths_are_unique() {
    let mut seen = std::collections::HashSet::new();
    for command in COMMANDS {
        assert!(seen.insert(command.path), "{:?}", command.path);
    }
}

#[test]
fn context_loads_rejects_unknown_arguments() {
    let _fx = Fixture::new("ctx-loads", Some(sample_registry()));
    let (code, _, err) = run_cli(&["context", "loads", "--bogus"]);
    assert_eq!(code, 2, "{err}");
    let (code, _, _) = run_cli(&["context", "loads", "--profile"]);
    assert_eq!(code, 2);
}

fn write_auth_status(dir: &std::path::Path) {
    let now = chrono_now();
    let status = json!({
        "version": 1,
        "servers": {
            "alpha": {"tracked": {"state": "ok", "reason": "ok", "since": now, "transient": null},
                      "lastProbeAt": now, "nextDueAt": now + 60},
            "beta": {"tracked": {"state": "needs_reauth", "reason": "invalid_grant", "since": now, "transient": null},
                     "lastProbeAt": now, "nextDueAt": now + 60}
        },
        "profiles": {}
    });
    std::fs::create_dir_all(dir.join("auth")).unwrap();
    std::fs::write(dir.join("auth/status.json"), status.to_string()).unwrap();
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[test]
fn status_json_includes_auth_rows_without_secrets() {
    let fx = Fixture::new("authstatus", Some(sample_registry()));
    write_auth_status(&fx.dir);
    let (code, value) = json_of(&["--json", "status"]);
    assert_eq!(code, 0);
    let auth = &value["data"]["auth"];
    assert_eq!(auth["counts"]["ok"], 1);
    assert_eq!(auth["counts"]["needs_reauth"], 1);
    assert_eq!(auth["servers"][0]["server"], "beta");
    assert_eq!(auth["servers"][0]["fix"]["action"], "reauth");
    assert!(auth["servers"][0].get("token").is_none());
    assert!(!value.to_string().contains(FAKE_SECRET));
}

#[test]
fn auth_statusline_and_hook_print_compact_json() {
    let fx = Fixture::new("authline", Some(sample_registry()));
    write_auth_status(&fx.dir);
    let (code, out, _) = run_cli(&["auth", "statusline"]);
    assert_eq!(code, 0);
    let line: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(line["auth"]["needs_reauth"], 1);
    assert_eq!(line["auth"]["worst"], json!(["beta"]));
    assert_eq!(out.trim().lines().count(), 1);

    let (code, value) = json_of(&["--json", "auth", "hook"]);
    assert_eq!(code, 0);
    assert_eq!(value["command"], "auth hook");
    assert_eq!(
        value["data"]["hookSpecificOutput"]["hookEventName"],
        "SessionStart"
    );

    let (code, _, err) = run_cli(&["auth", "statusline", "extra"]);
    assert_eq!(code, 2);
    assert!(err.contains("unexpected argument"));
}

#[test]
fn auth_surfaces_write_nothing_on_first_run() {
    let fx = Fixture::new("authfirst", None);
    let (code, out, _) = run_cli(&["auth", "statusline"]);
    assert_eq!(code, 0);
    assert!(out.contains("auth ok (0)"));
    assert!(!fx.dir.join("auth").exists());
}
