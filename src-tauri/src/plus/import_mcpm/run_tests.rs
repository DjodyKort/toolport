use super::*;
use crate::plus::ctl;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn input_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/import_mcpm/input")
}

fn fakeify(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("CANARY_") {
        out.push_str(&rest[..i]);
        out.push_str("FAKE-");
        rest = &rest[i + "CANARY_".len()..];
    }
    out.push_str(rest);
    out
}

struct World {
    base: PathBuf,
    root: PathBuf,
    short_ids: PathBuf,
    _dir: crate::registry::DataDirOverride,
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!("import-mcpm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("mcpm");
        std::fs::create_dir_all(&root).unwrap();
        for entry in std::fs::read_dir(input_dir()).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(root.join(path.file_name().unwrap()), fakeify(&text)).unwrap();
        }
        let short_ids = base.join("short-ids.json");
        std::fs::write(
            &short_ids,
            json!({"anna-mcp":"anna","google-docs-mcp":"gdocs","google-docs-a":"gdocs-a",
                   "google-docs-b":"gdocs-b","moodle-mcp":"moodle"})
            .to_string(),
        )
        .unwrap();
        let data = base.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let dir = crate::registry::DataDirOverride::set(&data);
        World {
            base,
            root,
            short_ids,
            _dir: dir,
        }
    }

    fn opts(&self, dry_run: bool) -> RunOptions {
        RunOptions {
            root: self.root.clone(),
            short_ids_path: Some(self.short_ids.clone()),
            home: Some("{{HOME}}".into()),
            dry_run,
        }
    }

    fn data(&self) -> PathBuf {
        self.base.join("data")
    }

    fn registry_text(&self) -> Option<String> {
        std::fs::read_to_string(self.data().join("registry.json")).ok()
    }

    fn secrets(&self) -> Vec<(String, String, String)> {
        let (input, clients) = load_input(&self.opts(true)).unwrap();
        map_all(&input, &clients)
            .secret_writes()
            .into_iter()
            .map(|s| (s.server_id.clone(), s.key.clone(), s.value.clone()))
            .collect()
    }
}

fn with_world(tag: &str, test: impl FnOnce(&World)) {
    crate::secrets::tests::with_isolated_vault(|| {
        let world = World::new(tag);
        test(&world);
    });
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn assert_no_plaintext(world: &World, values: &[String]) {
    let mut files = Vec::new();
    files_under(&world.data(), &mut files);
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for v in values {
            assert!(
                !text.contains(v.as_str()),
                "{} leaked in {}",
                v,
                f.display()
            );
        }
    }
}

fn leaks(text: &str, values: &[String]) -> bool {
    values.iter().any(|v| text.contains(v.as_str()))
}

#[test]
fn dry_run_writes_nothing_and_prints_plan() {
    with_world("dry", |w| {
        let secrets = w.secrets();
        assert!(secrets.len() > 5);
        let plan = run(&w.opts(true)).unwrap();
        assert!(plan.dry_run);
        assert_eq!(plan.servers.len(), 20);
        assert!(plan.servers.iter().all(|c| c.action == Action::Created));
        assert!(plan.secrets.iter().all(|c| c.action == Action::Created));
        assert!(w.registry_text().is_none());
        let mut files = Vec::new();
        files_under(&w.data(), &mut files);
        assert!(files.is_empty(), "{files:?}");
        for (id, key, _) in &secrets {
            assert_eq!(
                crate::secrets::get_vault_secret_result(id, key).unwrap(),
                None
            );
        }
        let values: Vec<String> = secrets.iter().map(|s| s.2.clone()).collect();
        assert!(!leaks(&plan.to_value().to_string(), &values));
        assert!(!leaks(&plan.summary(), &values));
    });
}

#[test]
fn real_run_writes_registry_and_vault_only_secrets() {
    with_world("real", |w| {
        let secrets = w.secrets();
        let values: Vec<String> = secrets.iter().map(|s| s.2.clone()).collect();
        let plan = run(&w.opts(false)).unwrap();
        assert!(!plan.dry_run);
        assert!(plan.changed());
        let reg: Value = serde_json::from_str(&w.registry_text().unwrap()).unwrap();
        assert_eq!(reg["servers"].as_array().unwrap().len(), 20);
        assert_eq!(reg["secretsGeneration"], 1);
        assert!(!reg["clientScopes"].as_object().unwrap().is_empty());
        for (id, key, value) in &secrets {
            assert_eq!(
                crate::secrets::get_vault_secret_result(id, key)
                    .unwrap()
                    .as_deref(),
                Some(value.as_str()),
                "{id}::{key}"
            );
        }
        assert_no_plaintext(&w, &values);
        assert!(!leaks(&plan.to_value().to_string(), &values));
        assert!(plan
            .secrets
            .iter()
            .all(|c| c.id.contains("::") && c.action == Action::Created));
    });
}

#[test]
fn rerun_changes_nothing() {
    with_world("idem", |w| {
        run(&w.opts(false)).unwrap();
        let before = w.registry_text().unwrap();
        let second = run(&w.opts(false)).unwrap();
        assert!(!second.changed());
        assert_eq!(
            second.counts.get("unchanged").copied().unwrap_or(0) > 0,
            true
        );
        assert_eq!(w.registry_text().unwrap(), before);
        let dry = run(&w.opts(true)).unwrap();
        assert!(!dry.changed());
    });
}

#[test]
fn changed_secret_and_server_report_updated() {
    with_world("upd", |w| {
        run(&w.opts(false)).unwrap();
        let path = w.root.join("servers.json");
        let text = std::fs::read_to_string(&path).unwrap();
        let (_, key, old) = w.secrets().into_iter().next().unwrap();
        std::fs::write(&path, text.replace(&old, "FAKE-rotated-0001")).unwrap();
        let plan = run(&w.opts(false)).unwrap();
        let updated: Vec<_> = plan
            .secrets
            .iter()
            .filter(|c| c.action == Action::Updated)
            .collect();
        assert_eq!(updated.len(), 1, "{key}");
        let reg: Value = serde_json::from_str(&w.registry_text().unwrap()).unwrap();
        assert_eq!(reg["secretsGeneration"], 2);
        assert_no_plaintext(&w, &["FAKE-rotated-0001".to_string()]);
    });
}

#[test]
fn user_owned_server_with_same_id_is_a_conflict() {
    with_world("conflict", |w| {
        let mut reg = crate::registry::Registry::default();
        let mut entry = crate::registry::ServerEntry {
            id: "anna".into(),
            name: "mine".into(),
            transport: "stdio".into(),
            command: Some("true".into()),
            args: vec![],
            launch: None,
            env: vec![],
            url: None,
            cwd: None,
            source: None,
            disabled_tools: vec![],
            client_credentials: None,
            request_timeout_ms: None,
            initialize_timeout_ms: None,
            unknown_fields: Default::default(),
        };
        entry.source = Some("manual".into());
        reg.servers.push(entry);
        crate::registry::save_to(&w.data().join("registry.json"), &reg).unwrap();
        let plan = run(&w.opts(false)).unwrap();
        let anna = plan.servers.iter().find(|c| c.id == "anna").unwrap();
        assert_eq!(anna.action, Action::Conflict);
        assert!(plan
            .secrets
            .iter()
            .filter(|c| c.id.starts_with("anna::"))
            .all(|c| c.action == Action::Conflict));
        assert_eq!(
            crate::secrets::get_vault_secret_result("anna", "ANNAS_SECRET_KEY").unwrap(),
            None
        );
        let after: Value = serde_json::from_str(&w.registry_text().unwrap()).unwrap();
        let kept = after["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "anna")
            .unwrap();
        assert_eq!(kept["name"], "mine");
    });
}

#[test]
fn handler_and_cli_share_the_flow_without_leaking() {
    with_world("cli", |w| {
        let values: Vec<String> = w.secrets().iter().map(|s| s.2.clone()).collect();
        let root = w.root.to_string_lossy().to_string();
        let ids = w.short_ids.to_string_lossy().to_string();
        let args: Vec<String> = [
            "--json",
            "import",
            "mcpm",
            &root,
            "--dry-run",
            "--short-ids",
            &ids,
            "--home",
            "{{HOME}}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(ctl::run_with(&args, &mut out, &mut err), 0);
        let text = String::from_utf8(out).unwrap() + &String::from_utf8(err).unwrap();
        assert!(!leaks(&text, &values));
        let envelope: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(envelope["data"]["dryRun"], true);
        assert!(w.registry_text().is_none());

        let value = crate::plus::dispatch(
            "plus.import_mcpm.run",
            json!({"root": root, "shortIds": ids, "home": "{{HOME}}"}),
        )
        .unwrap();
        assert_eq!(value["dryRun"], false);
        assert!(!leaks(&value.to_string(), &values));
        assert!(w.registry_text().is_some());
    });
}

#[test]
fn missing_root_is_an_error() {
    with_world("missing", |w| {
        let mut opts = w.opts(true);
        opts.root = w.base.join("nope");
        assert!(run(&opts).unwrap_err().contains("servers.json"));
    });
}
