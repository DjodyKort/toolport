use super::*;
use crate::plus::ctl::run_with;

const FAKE_KEY: &str = "FAKE-OPENROUTER-KEY-do-not-print-91ac";

fn ctl(list: &[&str]) -> (i32, String) {
    let args: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&args, &mut out, &mut err);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn entry_declares_key_as_vault_secret_only() {
    let entry = server_entry();
    assert_eq!(entry.transport, "stdio");
    assert!(entry
        .env
        .iter()
        .all(|v| v.value.is_none() && v.secret && v.key == API_KEY_ENV));
    assert!(!serde_json::to_string(&entry).unwrap().contains("FAKE"));
}

#[test]
fn manifest_lists_four_tools_and_two_resources() {
    let m = manifest();
    assert_eq!(m["tools"].as_array().unwrap().len(), 4);
    assert_eq!(m["resources"].as_array().unwrap().len(), 2);
    let (code, out) = ctl(&["--json", "council", "tools"]);
    assert_eq!(code, 0);
    assert!(out.contains("council_ask") && out.contains("council://config"));
}

#[test]
fn install_doctor_uninstall_round_trip() {
    crate::secrets::tests::with_isolated_vault(|| {
        let (code, out) = ctl(&["--json", "council", "doctor"]);
        assert_eq!(code, 1, "{out}");

        std::env::set_var("FAKE_COUNCIL_KEY_SRC", FAKE_KEY);
        let (code, out) = ctl(&[
            "--json",
            "council",
            "install",
            "--api-key-env",
            "FAKE_COUNCIL_KEY_SRC",
        ]);
        assert_eq!(code, 0, "{out}");
        assert!(!out.contains(FAKE_KEY));

        let reg = std::fs::read_to_string(registry::registry_path().unwrap()).unwrap();
        assert!(!reg.contains(FAKE_KEY));
        assert!(reg.contains("plus:council"));
        assert_eq!(
            crate::secrets::get_secret("council", API_KEY_ENV).as_deref(),
            Some(FAKE_KEY)
        );

        let (code, out) = ctl(&["--json", "council", "install"]);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("\"created\":false"), "{out}");

        let (_, out) = ctl(&["--json", "council", "doctor"]);
        assert!(!out.contains(FAKE_KEY));
        for name in [
            "registry_entry",
            "key_declared_secret",
            "key_in_vault",
            "enabled_in_active_profile",
        ] {
            let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
            let checks = v["data"]["checks"].as_array().unwrap();
            let c = checks.iter().find(|c| c["name"] == name).unwrap();
            assert_eq!(c["ok"], true, "{name}: {out}");
        }

        let (code, _) = ctl(&["--json", "council", "uninstall", "--purge-key"]);
        assert_eq!(code, 0);
        assert!(crate::secrets::get_secret("council", API_KEY_ENV).is_none());
        let reg = std::fs::read_to_string(registry::registry_path().unwrap()).unwrap();
        assert!(!reg.contains("plus:council"));
        let (code, out) = ctl(&["--json", "council", "uninstall"]);
        assert_eq!(code, 0);
        assert!(out.contains("\"removed\":false"));
        std::env::remove_var("FAKE_COUNCIL_KEY_SRC");
    });
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(ctl(&["council"]).0, 2);
    assert_eq!(ctl(&["council", "nope"]).0, 2);
    assert_eq!(ctl(&["council", "tools", "x"]).0, 2);
}
