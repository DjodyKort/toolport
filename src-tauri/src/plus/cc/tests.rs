use super::*;
use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("plugins")).unwrap();
        Self { dir }
    }

    fn write(&self, rel: &str, body: &str) {
        let p = self.dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn catalog(&self) {
        let mkt = self.dir.join("mkt-a");
        self.write(
            "plugins/known_marketplaces.json",
            &json!({"mkt-a": {"installLocation": mkt}}).to_string(),
        );
        self.write(
            "mkt-a/.claude-plugin/marketplace.json",
            &json!({"plugins": [
                {"name": "alpha", "version": "2.0.0"},
                {"name": "beta", "version": "1.0.0"},
                {"name": "gamma", "source": {"sha": "0123456789abcdef0123"}},
                {"name": "delta", "version": "5.0.0"}
            ]})
            .to_string(),
        );
    }

    fn opts(&self) -> Options {
        Options {
            claude_root: Some(self.dir.clone()),
            ..Options::default()
        }
    }

    fn mock(&self, list_json: &str, update_body: &str) -> SystemClaude {
        let script = self.dir.join("claude");
        let body = format!(
            "#!/bin/sh\necho \"$@\" >> \"{log}\"\ncase \"$1 $2 $3\" in\n\
             \"plugin list --json\") cat <<'EOF'\n{list_json}\nEOF\n;;\n\
             \"plugin marketplace update\") echo refreshed;;\n\
             \"plugin update \"*) {update_body};;\n\
             *) echo unexpected >&2; exit 9;;\nesac\n",
            log = self.dir.join("calls.log").display()
        );
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        SystemClaude::with_bin(script.to_string_lossy())
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const LISTED: &str = r#"[
 {"name":"alpha","marketplace":"mkt-a","version":"1.0.0"},
 {"name":"beta","marketplace":"mkt-a","version":"1.0.0"},
 {"name":"gamma","marketplace":"mkt-a","version":"x"},
 {"name":"delta","marketplace":"mkt-a","version":"4.0.0"},
 {"name":"orphan","marketplace":"mkt-z","version":"1.0.0"}
]"#;

fn row<'a>(r: &'a Report, name: &str) -> &'a PluginRow {
    r.rows.iter().find(|x| x.name == name).unwrap()
}

#[test]
fn list_reports_installed_vs_available_without_mutating() {
    let env = Env::new("list");
    env.catalog();
    let mock = env.mock(LISTED, "echo updated");
    let r = list(&mock, &env.opts()).unwrap();
    assert_eq!(row(&r, "alpha").status, "update");
    assert_eq!(row(&r, "alpha").available.as_deref(), Some("2.0.0"));
    assert_eq!(row(&r, "beta").status, "current");
    assert_eq!(row(&r, "gamma").available.as_deref(), Some("0123456789ab"));
    assert_eq!(row(&r, "orphan").status, "unknown");
    assert_eq!(env.calls(), vec!["plugin list --json"]);
    assert!(r.render().contains("alpha@mkt-a"));
    assert_eq!(r.to_value()["plugins"][0]["id"], "alpha@mkt-a");
}

#[test]
fn update_applies_only_updatable_plugins() {
    let env = Env::new("apply");
    env.catalog();
    let mock = env.mock(LISTED, "echo done");
    let r = update(&mock, &env.opts()).unwrap();
    assert_eq!(row(&r, "alpha").outcome.as_deref(), Some("updated"));
    assert_eq!(row(&r, "beta").outcome, None);
    assert!(r.restart_required);
    assert!(!r.has_errors());
    let calls = env.calls();
    assert_eq!(calls[0], "plugin marketplace update");
    assert!(calls.contains(&"plugin update alpha@mkt-a".to_string()));
    assert!(calls.contains(&"plugin update delta@mkt-a".to_string()));
    assert!(!calls
        .iter()
        .any(|c| c.contains("beta") || c.contains("orphan")));
}

#[test]
fn dry_run_never_calls_plugin_update() {
    let env = Env::new("dry");
    env.catalog();
    let mock = env.mock(LISTED, "echo done");
    let mut o = env.opts();
    o.dry_run = true;
    let r = update(&mock, &o).unwrap();
    assert_eq!(r.mode, "dry-run");
    assert_eq!(row(&r, "alpha").outcome.as_deref(), Some("would update"));
    assert!(!env.calls().iter().any(|c| c.starts_with("plugin update")));
    assert!(!r.restart_required);
}

#[test]
fn noop_update_needs_no_restart_and_failure_is_reported() {
    let env = Env::new("noop");
    env.catalog();
    let mock = env.mock(LISTED, "echo 'already up to date'");
    let r = update(&mock, &env.opts()).unwrap();
    assert_eq!(row(&r, "alpha").outcome.as_deref(), Some("already current"));
    assert!(!r.restart_required);

    let failing = env.mock(LISTED, "echo boom >&2; exit 3");
    let r = update(&failing, &env.opts()).unwrap();
    assert_eq!(row(&r, "alpha").error.as_deref(), Some("boom"));
    assert!(r.has_errors());
}

#[test]
fn blocked_plugins_are_skipped_and_explicit_forces_unknown() {
    let env = Env::new("blocked");
    env.catalog();
    env.write(
        "plugins/blocklist.json",
        r#"{"plugins":[{"plugin":"alpha@mkt-a"}]}"#,
    );
    env.write(
        "settings.json",
        r#"{"enabledPlugins":{"beta@mkt-a":false}}"#,
    );
    let mock = env.mock(LISTED, "echo done");
    let r = update(&mock, &env.opts()).unwrap();
    assert!(row(&r, "alpha").blocked);
    assert_eq!(
        row(&r, "alpha").outcome.as_deref(),
        Some("skipped: blocked")
    );
    assert!(!row(&r, "beta").enabled);
    assert!(!env
        .calls()
        .contains(&"plugin update alpha@mkt-a".to_string()));

    let mut o = env.opts();
    o.plugin = Some("orphan".into());
    let r = update(&mock, &o).unwrap();
    assert_eq!(r.rows.len(), 1);
    assert_eq!(r.rows[0].outcome.as_deref(), Some("updated"));
}

#[test]
fn missing_plugin_and_bad_names_are_rejected() {
    let env = Env::new("errs");
    env.catalog();
    let mock = env.mock(LISTED, "echo done");
    let mut o = env.opts();
    o.plugin = Some("nope".into());
    assert!(update(&mock, &o).unwrap_err().contains("not installed"));
    o.plugin = Some("--scope=user".into());
    assert!(list(&mock, &o).unwrap_err().contains("invalid plugin"));
    o.plugin = Some("a b".into());
    assert!(list(&mock, &o).is_err());
}

#[test]
fn wrapped_list_shape_and_garbage_output() {
    let env = Env::new("shape");
    env.catalog();
    let wrapped = env.mock(
        r#"{"plugins":[{"name":"beta@mkt-a","version":"1.0.0"}]}"#,
        "echo x",
    );
    let r = list(&wrapped, &env.opts()).unwrap();
    assert_eq!(r.rows[0].marketplace.as_deref(), Some("mkt-a"));
    assert_eq!(r.rows[0].status, "current");
    let garbage = env.mock("not json", "echo x");
    assert!(list(&garbage, &env.opts())
        .unwrap_err()
        .contains("unparseable"));
    let missing = SystemClaude::with_bin(env.dir.join("absent").to_string_lossy());
    assert!(list(&missing, &env.opts()).is_err());
}

struct Recorder(RefCell<Vec<Vec<String>>>);

impl ClaudeRunner for Recorder {
    fn run(&self, args: &[&str], _t: Duration) -> Result<CmdOutput, String> {
        self.0
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(CmdOutput {
            code: 0,
            stdout: "[]".into(),
            stderr: String::new(),
        })
    }
}

#[test]
fn only_fixed_plugin_subcommands_are_issued() {
    let env = Env::new("fixed");
    let rec = Recorder(RefCell::new(Vec::new()));
    let mut o = env.opts();
    o.marketplace = Some("mkt-a".into());
    update(&rec, &o).unwrap();
    for call in rec.0.borrow().iter() {
        assert_eq!(call[0], "plugin");
        assert!(matches!(
            call[1].as_str(),
            "list" | "update" | "marketplace"
        ));
    }
}

#[test]
fn handlers_are_registered() {
    let err = crate::plus::dispatch("plus.cc.list", json!({"plugin": "-x"})).unwrap_err();
    assert!(err.contains("invalid plugin"));
    let err = crate::plus::dispatch("plus.cc.update", json!({"plugin": "-x"})).unwrap_err();
    assert!(err.contains("invalid plugin"));
}
