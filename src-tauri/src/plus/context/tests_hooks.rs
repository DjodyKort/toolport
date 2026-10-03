use serde_json::{json, Value};
use std::fs;

const PAYLOADS: [(&str, &str); 3] = [
    (
        "SessionStart",
        include_str!("../../../../src/plus/fixtures/hook-payloads/session-start.json"),
    ),
    (
        "UserPromptSubmit",
        include_str!("../../../../src/plus/fixtures/hook-payloads/user-prompt-submit.json"),
    ),
    (
        "PreCompact",
        include_str!("../../../../src/plus/fixtures/hook-payloads/pre-compact.json"),
    ),
];

fn render(raw: &str, home: &str, cwd: &str) -> Value {
    serde_json::from_str(&raw.replace("{{HOME}}", home).replace("{{CWD}}", cwd)).unwrap()
}

#[test]
fn hook_payload_cwd_drives_what_loads_shape() {
    let home = std::env::temp_dir().join(format!("ctx-hooks-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let cwd = home.join("work/app");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(home.join("work/app/CLAUDE.md"), "project memory ".repeat(40)).unwrap();
    fs::write(
        home.join("work/app/.mcp.json"),
        r#"{"mcpServers":{"alpha":{"command":"a"}}}"#,
    )
    .unwrap();

    for (event, raw) in PAYLOADS {
        let payload = render(raw, &home.display().to_string(), &cwd.display().to_string());
        assert_eq!(payload["hook_event_name"], event);
        assert!(payload["session_id"].as_str().unwrap().starts_with("00000000-"));
        let out = crate::plus::dispatch(
            "plus.context.whatLoads",
            json!({"home": home.display().to_string(), "cwd": payload["cwd"]}),
        )
        .unwrap();
        assert_eq!(out["cwd"], payload["cwd"]);
        assert!(out["total_tokens"].as_u64().unwrap() > 0);
        for key in ["items", "clobbers", "notes"] {
            assert!(out[key].is_array(), "{event}: {key}");
        }
        assert!(out["tokens_by_kind"].is_object());
        for item in out["items"].as_array().unwrap() {
            for key in ["kind", "name", "source", "loaded", "reason", "tokens"] {
                assert!(item.get(key).is_some(), "{event}: item.{key}");
            }
        }
        let kinds: Vec<_> = out["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["kind"].as_str().unwrap())
            .collect();
        assert!(kinds.contains(&"memory") && kinds.contains(&"mcp"), "{event}");
    }
    let _ = fs::remove_dir_all(&home);
}
