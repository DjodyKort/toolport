use super::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TempHome(PathBuf);

impl TempHome {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ctx-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn roots(&self) -> Roots {
        Roots::from_home(&self.0)
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, text).unwrap();
        p
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.0.join(rel)).unwrap()
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn config(extra: Value) -> ContextConfig {
    let mut base = json!({"wrap_default_claude": false, "dedupe": {"enabled": false}});
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    ContextConfig::from_value(base).unwrap()
}

fn run(roots: &Roots, cfg: &mut ContextConfig) -> Report {
    apply(
        roots,
        cfg,
        ApplyOptions {
            persist: false,
            dry_run: false,
        },
    )
    .unwrap()
}

fn json_at(h: &TempHome, rel: &str) -> Value {
    serde_json::from_str(&h.read(rel)).unwrap()
}

fn init_git(dir: &Path) {
    fs::create_dir_all(dir.join(".git/info")).unwrap();
}

#[test]
fn plan_lists_changes_without_writing() {
    let h = TempHome::new();
    h.write(
        ".claude.json",
        r#"{"mcpServers":{"context7":{},"mcpm_context7":{}}}"#,
    );
    h.write(".config/mcpm/skills_repo/rules/client-acme/SKILL.md", "---\nname: client-acme\ndescription: \"Client context: acme\"\nactivation: always\n---\nbody\n");
    fs::create_dir_all(h.0.join("clients/acme")).unwrap();
    let mut cfg = config(json!({
        "dedupe": {"enabled": true},
        "clients_root": h.0.join("clients").to_string_lossy(),
        "settings": {"ensure_allow": ["Read(*)"]},
    }));
    let report = plan(&h.roots(), &mut cfg).unwrap();
    assert!(report
        .actions
        .iter()
        .any(|a| a.starts_with("removed legacy MCP entries")));
    assert!(report.actions.iter().any(|a| a.contains("re-unioned")));
    assert!(report.actions.iter().any(|a| a.starts_with("deployed ")));
    assert!(!h.0.join(".claude/settings.json").exists());
    assert!(!h.0.join("clients/acme/CLAUDE.local.md").exists());
    assert!(!h.0.join(".cache").exists());
    assert!(h.read(".claude.json").contains("\"context7\""));
}

#[test]
fn clobbered_arrays_are_reunioned_idempotently_with_a_backup() {
    let h = TempHome::new();
    let policy = json!({"settings": {"ensure_allow": ["Bash(ls:*)", "Read(*)"], "ensure_ask": ["Bash(rm:*)"]}});
    h.write(
        ".claude/settings.json",
        r#"{"permissions":{"allow":["org-a"],"deny":["x"]},"model":"m"}"#,
    );
    let roots = h.roots();
    let first = run(&roots, &mut config(policy.clone()));
    assert_eq!(
        first.actions,
        vec!["re-unioned ensure_allow/ensure_ask into settings.json"]
    );
    let after = json_at(&h, ".claude/settings.json");
    assert_eq!(
        after["permissions"]["allow"],
        json!(["org-a", "Bash(ls:*)", "Read(*)"])
    );
    assert_eq!(after["permissions"]["ask"], json!(["Bash(rm:*)"]));
    assert_eq!(after["permissions"]["deny"], json!(["x"]));
    assert!(run(&roots, &mut config(policy.clone())).actions.is_empty());

    h.write(
        ".claude/settings.json",
        r#"{"permissions":{"allow":["org-a","org-b"]},"model":"m"}"#,
    );
    let again = run(&roots, &mut config(policy));
    assert_eq!(again.actions.len(), 1);
    let names: Vec<String> = walk(&roots.backups_dir());
    assert_eq!(
        names.iter().filter(|n| n.starts_with("settings")).count(),
        2
    );
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(read) = fs::read_dir(dir) {
        for e in read.flatten() {
            if e.path().is_dir() {
                out.extend(walk(&e.path()));
            } else {
                out.push(e.file_name().to_string_lossy().into_owned());
            }
        }
    }
    out
}

#[test]
fn corrupt_missing_and_non_list_settings_are_handled() {
    let h = TempHome::new();
    let roots = h.roots();
    let policy = json!({"settings": {"ensure_allow": ["Read(*)"]}});

    let created = run(&roots, &mut config(policy.clone()));
    assert_eq!(created.actions.len(), 1);
    assert_eq!(
        json_at(&h, ".claude/settings.json")["permissions"]["allow"],
        json!(["Read(*)"])
    );

    let corrupt = "{\"permissions\": {\"allow\": [\"x\"]},\n  \"y\": ";
    h.write(".claude/settings.json", corrupt);
    let r = run(&roots, &mut config(policy.clone()));
    assert!(r.actions.is_empty() && r.warnings.is_empty());
    assert_eq!(h.read(".claude/settings.json"), corrupt);

    let odd = r#"{"permissions": {"allow": "not-a-list", "ask": []}}"#;
    h.write(".claude/settings.json", odd);
    let r = run(&roots, &mut config(policy));
    assert!(r.actions.is_empty());
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(h.read(".claude/settings.json"), odd);

    h.write(".claude/settings.json", "[1]");
    let r = run(
        &roots,
        &mut config(json!({"settings": {"ensure_ask": ["Bash(rm:*)"]}})),
    );
    assert_eq!(r.warnings.len(), 1);
}

#[test]
fn invalid_profile_names_are_rejected() {
    let err = ContextConfig::from_value(json!({"profiles": {"Bad_Name": {}}})).unwrap_err();
    assert!(err.contains("Bad_Name"));
    assert!(ContextConfig::from_value(json!({"profiles": {"ok-1": {}}})).is_ok());
}

#[test]
fn dedupe_requires_the_mcpm_twin_unless_disabled() {
    let h = TempHome::new();
    h.write(
        ".claude.json",
        r#"{"theme":"x","mcpServers":{"context7":{},"mcpm_context7":{},"playwright":{}}}"#,
    );
    let roots = h.roots();
    let r = run(&roots, &mut config(json!({"dedupe": {"enabled": true}})));
    assert_eq!(
        r.actions,
        vec!["removed legacy MCP entries from .claude.json: context7"]
    );
    let data = json_at(&h, ".claude.json");
    assert!(data["mcpServers"].get("playwright").is_some());
    assert_eq!(data["theme"], "x");
    let r = run(
        &roots,
        &mut config(json!({"dedupe": {"enabled": true, "require_mcpm_twin": false}})),
    );
    assert_eq!(
        r.actions,
        vec!["removed legacy MCP entries from .claude.json: playwright"]
    );
    assert!(
        run(&roots, &mut config(json!({"dedupe": {"enabled": true}})))
            .actions
            .is_empty()
    );
}

#[test]
fn client_local_deploy_handles_git_variants_and_redeploys_after_edit() {
    let h = TempHome::new();
    let layer = |name: &str, body: &str| {
        h.write(
            &format!(".config/mcpm/skills_repo/rules/client-{name}/SKILL.md"),
            &format!(
                "---\nname: client-{name}\ndescription: \"d\"\nactivation: always\n---\n{body}\n"
            ),
        );
    };
    for name in ["a", "b", "c", "d"] {
        layer(name, &format!("## {name}"));
        fs::create_dir_all(h.0.join("clients").join(name)).unwrap();
    }
    init_git(&h.0.join("clients/a"));
    h.write("clients/a/.git/info/exclude", "*.log");
    h.write("clients/b/.git", "gitdir: /nonexistent\n");
    init_git(&h.0.join("clients/c"));
    h.write("clients/d/CLAUDE.local.md", "mine\n");
    let roots = h.roots();
    let cfg = json!({"clients_root": h.0.join("clients").to_string_lossy()});

    let r = run(&roots, &mut config(cfg.clone()));
    assert_eq!(
        h.read("clients/a/.git/info/exclude"),
        "*.log\nCLAUDE.local.md\n"
    );
    assert_eq!(h.read("clients/c/.git/info/exclude"), "CLAUDE.local.md\n");
    assert!(!h.0.join("clients/b/.git/info/exclude").exists());
    assert!(h.0.join("clients/b/CLAUDE.local.md").exists());
    assert_eq!(h.read("clients/d/CLAUDE.local.md"), "mine\n");
    assert_eq!(r.warnings.len(), 1);
    assert!(r.warnings[0].contains("not managed"));

    assert!(run(&roots, &mut config(cfg.clone())).actions.is_empty());

    layer("a", "## a edited");
    let r = run(&roots, &mut config(cfg));
    assert_eq!(r.actions.len(), 1);
    assert!(h.read("clients/a/CLAUDE.local.md").contains("## a edited"));
}

#[test]
fn rescaffold_never_overwrites_an_edited_body() {
    let h = TempHome::new();
    let roots = h.roots();
    let path = layers::scaffold_personal_rule(&roots).unwrap().unwrap();
    fs::write(
        &path,
        "---\nname: personal\ndescription: x\nactivation: always\n---\nedited\n",
    )
    .unwrap();
    assert!(layers::scaffold_personal_rule(&roots).unwrap().is_none());
    assert!(fs::read_to_string(&path).unwrap().contains("edited"));
    let client = layers::scaffold_client_rule(&roots, "v18_arp", None)
        .unwrap()
        .unwrap();
    assert!(client.ends_with("client-v18-arp/SKILL.md"));
    assert!(fs::read_to_string(&client)
        .unwrap()
        .contains("globs: \"**/clients/v18_arp/**\""));
    assert!(layers::scaffold_client_rule(&roots, "v18_arp", None)
        .unwrap()
        .is_none());
}

#[test]
fn rules_deploy_keeps_the_skills_bucket_of_the_lock() {
    let h = TempHome::new();
    h.write(
        ".config/mcpm/skills_repo/rules/personal/SKILL.md",
        "---\nname: personal\ndescription: d\nactivation: always\nglobs: \"a/**, b/**\"\n---\nbody\n",
    );
    h.write(
        ".config/mcpm/skills_repo/skills/demo/SKILL.md",
        "---\nname: demo\ndescription: d\n---\nbody\n",
    );
    h.write(".claude/skills/demo/SKILL.md", "existing\n");
    let roots = h.roots();
    let prior = r#"{"active_styles":{},"agents":{},"output_root":"/x","rules":{},"scope":"global","skills":{"demo":{"clients_synced":["claude-code"],"hash":"sha256:abc","hooks_installed":{},"output_files":{"claude-code":[".claude/skills/demo/SKILL.md"]},"source":"local","version":null,"warnings":[]}},"styles":{},"synced_at":"t","version":1}"#;
    h.write(".config/mcpm/mcpm-skills.lock", prior);
    let written = rules::deploy_rules_now(&roots, false).unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(
        h.read(".claude/rules/personal.md"),
        "---\npaths: [\"a/**\", \"b/**\"]\n---\n\nbody\n"
    );
    assert_eq!(h.read(".claude/skills/demo/SKILL.md"), "existing\n");
    let lock = json_at(&h, ".config/mcpm/mcpm-skills.lock");
    assert_eq!(lock["skills"]["demo"]["hash"], "sha256:abc");
    assert!(lock["rules"].get("personal").is_some());
    assert!(rules::deploy_rules_now(&roots, true).unwrap().is_empty());
}

#[test]
fn shim_snippet_variants() {
    let h = TempHome::new();
    let roots = h.roots();
    let mut profiles = BTreeMap::new();
    profiles.insert("work".to_string(), config::ProfileSpec::default());
    let on = shims::shim_snippet(&roots, &profiles, true);
    assert!(on.contains("mcpm_context_presync() {"));
    assert!(!on.contains("_mcpm_context_presync"));
    assert!(on.contains("claude() { mcpm_context_presync; command claude \"$@\"; }"));
    assert!(on.contains(&format!(
        "claude-work() {{ mcpm_context_presync; CLAUDE_CONFIG_DIR=\"{}/.config/mcpm/claude-profiles/work\" command claude \"$@\"; }}",
        h.0.display()
    )));
    let off = shims::shim_snippet(&roots, &profiles, false);
    assert!(!off.contains("presync"));
    assert!(off.contains("claude-work() { CLAUDE_CONFIG_DIR="));
    assert_eq!(
        shims::shim_snippet(&roots, &BTreeMap::new(), false)
            .lines()
            .count(),
        4
    );

    if let Ok(zsh) = std::process::Command::new("zsh")
        .arg("-c")
        .arg(":")
        .output()
    {
        assert!(zsh.status.success());
        for (i, text) in [on, off].iter().enumerate() {
            let p = h.write(&format!("s{i}.zsh"), text);
            let out = std::process::Command::new("zsh")
                .arg("-n")
                .arg(&p)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

#[test]
fn doctor_flags_a_changed_wrapper_and_new_cf_assets() {
    let h = TempHome::new();
    h.write(".local/share/cf-dev-tools/claude/shell-wrapper.sh", "v1\n");
    let roots = h.roots();
    let mut cfg = config(json!({}));
    let r = apply(&roots, &mut cfg, ApplyOptions::default()).unwrap();
    assert!(r.actions.iter().any(|a| a.contains("baseline")));
    assert!(doctor::run_checks(&roots, &cfg)
        .iter()
        .any(|c| c.1.contains("unchanged since baseline")));
    h.write(".local/share/cf-dev-tools/claude/shell-wrapper.sh", "v2\n");
    h.write(".local/share/cf-dev-tools/claude/hooks.json", "{}");
    let checks = doctor::run_checks(&roots, &load_config(&roots.context_config_path()));
    assert!(checks
        .iter()
        .any(|c| c.1.contains("CHANGED since baseline")));
    assert!(checks.iter().any(|c| c.1.contains("hooks.json")));
}

#[test]
fn handlers_plan_and_apply_against_an_explicit_home() {
    let h = TempHome::new();
    let args = json!({
        "home": h.0.to_string_lossy(),
        "config": {"wrap_default_claude": false, "dedupe": {"enabled": false},
                    "settings": {"ensure_allow": ["Read(*)"]}, "clients_root": h.0.join("none").to_string_lossy()},
    });
    let planned = crate::plus::dispatch("plus.context.plan", args.clone()).unwrap();
    assert_eq!(planned["dryRun"], true);
    assert_eq!(planned["actions"].as_array().unwrap().len(), 1);
    assert!(!h.0.join(".claude/settings.json").exists());
    let applied = crate::plus::dispatch("plus.context.apply", args).unwrap();
    assert_eq!(applied["dryRun"], false);
    assert!(h.0.join(".claude/settings.json").exists());
    assert!(h.0.join(".config/mcpm/context.json").exists());
}
