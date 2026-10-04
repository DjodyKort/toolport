use super::*;
use crate::plus::health::Health;
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
        "claude-work() {{ mcpm_context_presync; command claude --strict-mcp-config --mcp-config '{0}/.config/mcpm/claude-profiles/work/mcp.json' --settings '{0}/.config/mcpm/claude-profiles/work/settings.json' \"$@\"; }}",
        h.0.display()
    )));
    assert!(!on.contains("CLAUDE_CONFIG_DIR"));
    let off = shims::shim_snippet(&roots, &profiles, false);
    assert!(!off.contains("presync"));
    assert!(off.contains("claude-work() { command claude --strict-mcp-config --mcp-config"));
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
    h.write(".local/share/corp-dev-tools/claude/shell-wrapper.sh", "v1\n");
    let roots = h.roots();
    let mut cfg = config(json!({}));
    let r = apply(&roots, &mut cfg, ApplyOptions::default()).unwrap();
    assert!(r.actions.iter().any(|a| a.contains("baseline")));
    assert!(doctor::run_checks(&roots, &cfg)
        .iter()
        .any(|c| c.1.contains("unchanged since baseline")));
    h.write(".local/share/corp-dev-tools/claude/shell-wrapper.sh", "v2\n");
    h.write(".local/share/corp-dev-tools/claude/hooks.json", "{}");
    let checks = doctor::run_checks(&roots, &load_config(&roots.context_config_path()));
    assert!(checks
        .iter()
        .any(|c| c.1.contains("CHANGED since baseline")));
    assert!(checks.iter().any(|c| c.1.contains("hooks.json")));
}

#[test]
fn handlers_plan_and_apply_against_an_explicit_home() {
    let _data = crate::plus::testutil::DataDirFx::new("ctx-handlers", "explicit-home");
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

fn profile_spec(value: serde_json::Value) -> config::ProfileSpec {
    serde_json::from_value(value).unwrap()
}

#[test]
fn launch_argv_goldens() {
    let h = TempHome::new();
    let roots = h.roots();
    let dir = format!("{}/.config/mcpm/claude-profiles/bare", h.0.display());
    let plain = launch::launch_argv(&roots, "bare", &profile_spec(json!({})));
    assert_eq!(
        plain,
        [
            "--strict-mcp-config".to_string(),
            "--mcp-config".to_string(),
            format!("{dir}/mcp.json"),
            "--settings".to_string(),
            format!("{dir}/settings.json"),
        ]
    );
    let with_rules = launch::launch_argv(&roots, "bare", &profile_spec(json!({"rules": ["personal"]})));
    assert_eq!(&with_rules[5..], ["--append-system-prompt-file".to_string(), format!("{dir}/append-system-prompt.md")]);
    let none_rules = launch::launch_argv(&roots, "bare", &profile_spec(json!({"rules": "none"})));
    assert_eq!(none_rules.len(), 5);
}

#[test]
fn shim_golden_for_launch_profile() {
    let h = TempHome::new();
    let roots = h.roots();
    let mut profiles = BTreeMap::new();
    profiles.insert("bare".to_string(), profile_spec(json!({"rules": ["personal"]})));
    let text = shims::shim_snippet(&roots, &profiles, false);
    let dir = format!("{}/.config/mcpm/claude-profiles/bare", h.0.display());
    let last = text.lines().last().unwrap();
    assert_eq!(
        last,
        format!(
            "claude-bare() {{ command claude --strict-mcp-config --mcp-config '{dir}/mcp.json' --settings '{dir}/settings.json' --append-system-prompt-file '{dir}/append-system-prompt.md' \"$@\"; }}"
        )
    );
}

#[test]
fn shell_quote_survives_apostrophes() {
    assert_eq!(shims::shell_quote("/a b/it's"), "'/a b/it'\\''s'");
}

#[test]
fn generate_profile_writes_strict_files_and_is_idempotent() {
    let h = TempHome::new();
    let roots = h.roots();
    h.write(
        ".claude.json",
        r#"{"mcpServers":{"toolport":{"command":"toolport-gateway"},"other":{"command":"x"}}}"#,
    );
    h.write(".claude/settings.json", r#"{"permissions":{"allow":["Bash(ls)"]},"model":"sonnet"}"#);
    h.write(
        ".config/mcpm/skills_repo/rules/personal/SKILL.md",
        "---\nname: personal\n---\n\nbe brief\n",
    );
    let mut config = ContextConfig::default();
    config.profiles.insert(
        "bare".into(),
        profile_spec(json!({
            "servers": ["toolport", "missing"],
            "rules": ["personal", "nope"],
            "org": false,
            "settings_overrides": {"model": "opus"},
            "skills": false
        })),
    );
    let report = apply(&roots, &mut config, ApplyOptions::default()).unwrap();
    let dir = ".config/mcpm/claude-profiles/bare";
    let mcp = json_at(&h, &format!("{dir}/mcp.json"));
    assert_eq!(mcp, json!({"mcpServers": {"toolport": {"command": "toolport-gateway"}}}));
    let settings = json_at(&h, &format!("{dir}/settings.json"));
    assert_eq!(settings["model"], "opus");
    assert_eq!(settings["permissions"]["allow"], json!(["Bash(ls)"]));
    assert_eq!(
        settings["claudeMdExcludes"],
        json!([format!("{}/.claude/CLAUDE.md", h.0.display())])
    );
    let append = h.read(&format!("{dir}/append-system-prompt.md"));
    assert!(append.starts_with(launch::APPEND_HEADER));
    assert!(append.contains("be brief"));
    assert!(report.warnings.iter().any(|w| w.contains("server 'missing'")));
    assert!(report.warnings.iter().any(|w| w.contains("rule layer 'nope'")));
    assert!(report.warnings.iter().any(|w| w.contains("skills=false")));
    assert!(report.warnings.iter().all(|w| !w.contains("not implemented")));
    assert!(!report.actions.iter().any(|a| a.contains("removed")));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(h.0.join(dir).join("mcp.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let before = h.read(&format!("{dir}/mcp.json"));
    apply(&roots, &mut config, ApplyOptions::default()).unwrap();
    assert_eq!(h.read(&format!("{dir}/mcp.json")), before);
    let checks = doctor::run_checks(&roots, &config);
    assert!(checks.iter().any(|(l, m)| *l == Health::Ok && m.contains("profile 'bare' healthy")), "{checks:?}");
}

#[test]
fn profiles_generated_in_one_run_match_profiles_generated_one_by_one() {
    let h = TempHome::new();
    let roots = h.roots();
    h.write(
        ".claude.json",
        r#"{"mcpServers":{"toolport":{"command":"toolport-gateway"},"other":{"command":"x"},"third":{"url":"https://example.com/mcp"}}}"#,
    );
    h.write(".claude/settings.json", r#"{"permissions":{"allow":["Bash(ls)"]},"model":"sonnet"}"#);
    for rule in ["personal", "client-acme"] {
        h.write(
            &format!(".config/mcpm/skills_repo/rules/{rule}/SKILL.md"),
            &format!("---\nname: {rule}\n---\n\nbody of {rule}\n"),
        );
    }
    let mut config = ContextConfig::default();
    for (name, spec) in [
        ("all", json!({})),
        ("none", json!({"servers": "none", "rules": "none"})),
        ("named", json!({"servers": ["toolport", "missing"], "rules": ["personal", "nope"]})),
        ("bare", json!({"org": false, "settings_overrides": {"model": "opus"}, "skills": false})),
        ("rules", json!({"servers": ["third"], "rules": ["client-acme", "personal"], "org": false})),
        ("compact", json!({"auto_compact_window": 300000, "compact_instructions": "keep decisions"})),
    ] {
        config.profiles.insert(name.into(), profile_spec(spec));
    }
    let together = apply(&roots, &mut config, ApplyOptions { persist: false, dry_run: false }).unwrap();
    let base = ".config/mcpm/claude-profiles";
    let files = [
        launch::MCP_FILE,
        launch::SETTINGS_FILE,
        launch::APPEND_FILE,
        doctor::PROFILE_STATE_FILE,
    ];
    let snapshot = |h: &TempHome| -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();
        for name in config.profiles.keys() {
            for file in files {
                let path = h.0.join(base).join(name).join(file);
                out.push((format!("{name}/{file}"), fs::read_to_string(path).ok()));
            }
        }
        out
    };
    let shared = snapshot(&h);
    assert!(shared.iter().filter(|(_, text)| text.is_some()).count() >= 18);

    let mut separate = Report::default();
    fs::remove_dir_all(h.0.join(base)).unwrap();
    for (name, spec) in &config.profiles {
        launch::generate_profile(&roots, name, spec, &mut separate, false).unwrap();
    }
    assert_eq!(snapshot(&h), shared);
    let profile_lines = |r: &Report| -> Vec<String> {
        r.warnings.iter().chain(r.actions.iter()).filter(|l| l.contains("profile ")).cloned().collect()
    };
    assert_eq!(profile_lines(&together), profile_lines(&separate));
}

#[test]
fn profile_selection_none_and_dry_run() {
    let h = TempHome::new();
    let roots = h.roots();
    h.write(".claude.json", r#"{"mcpServers":{"a":{"command":"x"}}}"#);
    let mut config = ContextConfig::default();
    config.profiles.insert("empty".into(), profile_spec(json!({"servers": "none"})));
    config.profiles.insert("all".into(), profile_spec(json!({})));
    plan(&roots, &mut config).unwrap();
    assert!(!h.0.join(".config/mcpm/claude-profiles").exists());
    apply(&roots, &mut config, ApplyOptions { persist: false, dry_run: false }).unwrap();
    let base = ".config/mcpm/claude-profiles";
    assert_eq!(json_at(&h, &format!("{base}/empty/mcp.json")), json!({"mcpServers": {}}));
    assert_eq!(json_at(&h, &format!("{base}/all/mcp.json"))["mcpServers"]["a"]["command"], "x");
    config.profiles.insert("bad".into(), profile_spec(json!({"servers": 3})));
    assert!(apply(&roots, &mut config, ApplyOptions::default()).unwrap_err().contains("servers"));
}

mod loads_tests {
    use super::*;
    use crate::plus::context::layers::MANAGED_LOCAL_HEADER;
    use crate::plus::context::loads::what_loads;

    fn word(n: usize) -> String {
        "x".repeat(n)
    }

    fn fixture() -> TempHome {
        let h = TempHome::new();
        h.write(".claude/CLAUDE.md", &word(400));
        h.write(".claude/rules/org-style.md", &word(40));
        h.write(".claude/rules/scoped.md", "---\npaths: [\"src/**\"]\n---\n\nbody\n");
        h.write(
            ".config/mcpm/skills_repo/rules/personal/SKILL.md",
            "---\nname: personal\nactivation: always\n---\n\nmine\n",
        );
        h.write(".claude/rules/personal.md", &word(80));
        h.write(
            ".claude/settings.json",
            r#"{"model":"a","permissions":{"allow":["x"]},"env":{"A":"1"}}"#,
        );
        h.write(
            ".claude.json",
            r#"{"mcpServers":{"alpha":{"command":"a"},"beta":{"command":"b"},"context7":{"command":"c"}}}"#,
        );
        h.write(".claude/skills/one/SKILL.md", "---\nname: one\ndescription: user skill\n---\nbody");
        h.write("work/app/CLAUDE.md", &word(200));
        h.write("work/app/CLAUDE.local.md", &format!("{MANAGED_LOCAL_HEADER}\n\nlayer\n"));
        h.write("work/app/.claude/settings.json", r#"{"model":"b","permissions":{"allow":["y"]}}"#);
        h.write("work/app/.claude/settings.local.json", r#"{"model":"c"}"#);
        h.write("work/app/.claude/rules/org-style.md", &word(20));
        h.write("work/app/.mcp.json", r#"{"mcpServers":{"alpha":{"command":"proj"}}}"#);
        h.write(
            "work/app/.claude/skills/one/SKILL.md",
            "---\nname: one\ndescription: project skill\n---\nbody",
        );
        h
    }

    fn cfg_with_profile() -> ContextConfig {
        config(json!({"profiles": {"lean": {
            "org": false, "rules": "none", "servers": ["alpha"],
            "settings_overrides": {"model": "p"}
        }}}))
    }

    #[test]
    fn plain_launch_counts_layers_and_reports_clobbers() {
        let h = fixture();
        let cwd = h.0.join("work/app");
        let r = what_loads(&h.roots(), &config(json!({})), None, &cwd).unwrap();
        let org = r.items.iter().find(|i| i.name == "CLAUDE.md" && i.source == "org").unwrap();
        assert_eq!((org.tokens, org.loaded), (100, true));
        let local = r.items.iter().find(|i| i.name == "CLAUDE.local.md").unwrap();
        assert_eq!(local.source, "client-layer");
        let scoped = r.items.iter().find(|i| i.name == "scoped").unwrap();
        assert!(!scoped.loaded);
        let personal = r.items.iter().find(|i| i.name == "personal").unwrap();
        assert_eq!(personal.source, "personal");
        let k = |kind: &str, key: &str| {
            r.clobbers.iter().rfind(|c| c.kind == kind && c.key == key).cloned()
        };
        let model = k("settings", "model").unwrap();
        assert!(model.winner.ends_with("settings.local.json"));
        assert_eq!(k("settings", "permissions").unwrap().relation, "merges");
        assert!(k("settings", "env").is_none());
        assert!(k("rule", "org-style").unwrap().winner.contains("work/app"));
        assert!(k("mcp", "alpha").unwrap().winner.ends_with(".mcp.json"));
        assert!(k("skill", "one").unwrap().winner.contains("work/app"));
        let shadowed = r.items.iter().filter(|i| i.kind == "mcp" && i.name == "alpha").collect::<Vec<_>>();
        assert_eq!(shadowed.iter().filter(|i| i.loaded).count(), 1);
        let ctx7 = r.items.iter().find(|i| i.name == "context7").unwrap();
        assert_eq!(ctx7.source, "org");
        let sum: u64 = r.items.iter().filter(|i| i.loaded).map(|i| i.tokens).sum();
        assert_eq!(r.total_tokens, sum);
        assert_eq!(r.tokens_by_kind["memory"], 100 + 50 + text_tokens_of(&format!("{MANAGED_LOCAL_HEADER}\n\nlayer\n")));
    }

    fn text_tokens_of(s: &str) -> u64 {
        crate::savings::estimated_tokens(s.len() as u64)
    }

    #[test]
    fn strict_profile_drops_org_and_project_servers() {
        let h = fixture();
        let cwd = h.0.join("work/app");
        let r = what_loads(&h.roots(), &cfg_with_profile(), Some("lean"), &cwd).unwrap();
        let org = r.items.iter().find(|i| i.source == "org" && i.kind == "memory").unwrap();
        assert!(!org.loaded);
        let servers: Vec<_> = r.items.iter().filter(|i| i.kind == "mcp").map(|i| i.name.as_str()).collect();
        assert_eq!(servers, ["alpha"]);
        assert!(r.notes.iter().any(|n| n.contains("strict-mcp-config")));
        let model = r.clobbers.iter().rfind(|c| c.key == "model").unwrap();
        assert_eq!(model.winner, "profile settings_overrides");
        assert!(what_loads(&h.roots(), &cfg_with_profile(), Some("nope"), &cwd).is_err());
    }

    #[test]
    fn named_rule_layers_classify_user_rules_and_add_append_items() {
        let h = fixture();
        h.write(
            ".config/mcpm/skills_repo/rules/client-acme/SKILL.md",
            "---\nname: client-acme\nactivation: always\n---\n\nacme body\n",
        );
        h.write(".claude/rules/client-acme.md", &word(60));
        h.write(".claude/rules/paths-scoped.md", "---\nglobs: \"a/**\"\n---\n\nbody\n");
        h.write(".claude/rules/plain-fm.md", "---\ndescription: x\n---\n\nbody\n");
        let cfg = config(json!({"profiles": {"named": {
            "rules": ["personal", "client-acme", "nope"], "servers": "none"
        }}}));
        let cwd = h.0.join("work/app");
        let r = what_loads(&h.roots(), &cfg, Some("named"), &cwd).unwrap();
        let rule = |name: &str| r.items.iter().find(|i| i.kind == "rule" && i.name == name).unwrap();
        assert_eq!((rule("personal").source, rule("personal").loaded), ("personal", true));
        assert_eq!(rule("client-acme").source, "client-layer");
        assert_eq!(rule("org-style").source, "loose");
        assert!(!rule("scoped").loaded && !rule("paths-scoped").loaded);
        assert!(rule("plain-fm").loaded);
        let appended = rule("personal (append-system-prompt)");
        assert_eq!((appended.source, appended.tokens), ("personal", text_tokens_of("mine")));
        assert_eq!(rule("client-acme (append-system-prompt)").source, "client-layer");
        assert!(r.notes.iter().any(|n| n == "profile rule layer 'nope' not found"));
        assert_eq!(r.items.iter().filter(|i| i.name.ends_with("(append-system-prompt)")).count(), 2);
    }

    #[test]
    fn what_loads_handler_serializes_report() {
        let h = fixture();
        let out = crate::plus::dispatch(
            "plus.context.whatLoads",
            json!({"home": h.0.display().to_string(), "cwd": h.0.join("work/app").display().to_string()}),
        )
        .unwrap();
        assert!(out["total_tokens"].as_u64().unwrap() > 0);
    }
}

#[test]
fn corp_tools_dir_and_clients_root_default_config_env_precedence() {
    let h = TempHome::new();
    let mut roots = h.roots();
    let mut cfg = config(json!({}));
    assert_eq!(roots.resolve_corp_tools_dir(&cfg), h.0.join(".local/share/corp-dev-tools"));
    assert_eq!(
        roots.resolve_clients_root(&cfg),
        h.0.join("Documents/GitHub/ExampleWorkspace/clients")
    );
    cfg.corp_tools_dir = Some("~/tools/from-config".into());
    cfg.clients_root = "~/work/from-config".into();
    assert_eq!(roots.resolve_corp_tools_dir(&cfg), h.0.join("tools/from-config"));
    assert_eq!(roots.resolve_clients_root(&cfg), h.0.join("work/from-config"));
    roots.env_corp_tools_dir = Some("~/tools/from-env".into());
    roots.env_clients_root = Some(h.0.join("work/from-env").to_string_lossy().into_owned());
    assert_eq!(roots.resolve_corp_tools_dir(&cfg), h.0.join("tools/from-env"));
    assert_eq!(roots.resolve_clients_root(&cfg), h.0.join("work/from-env"));
    roots.env_corp_tools_dir = Some(String::new());
    assert_eq!(roots.resolve_corp_tools_dir(&cfg), h.0.join("tools/from-config"));
}

#[test]
fn process_env_feeds_the_resolver() {
    let _lock = crate::clients::env_test_lock();
    let h = TempHome::new();
    let tools = h.0.join("env-tools");
    let clients = h.0.join("env-clients");
    let _a = crate::clients::EnvRestore::set(roots::ENV_CORP_TOOLS_DIR, &tools);
    let _b = crate::clients::EnvRestore::set(roots::ENV_CLIENTS_ROOT, &clients);
    let mut roots = h.roots();
    roots.read_env();
    let cfg = config(json!({"clients_root": "~/ignored", "corp_tools_dir": "~/ignored"}));
    assert_eq!(roots.resolve_corp_tools_dir(&cfg), tools);
    assert_eq!(roots.resolve_clients_root(&cfg), clients);
}

#[test]
fn doctor_and_shims_use_the_configured_corp_tools_dir() {
    let h = TempHome::new();
    h.write("custom/tools/claude/shell-wrapper.sh", "v1\n");
    let roots = h.roots();
    let mut cfg = config(json!({
        "wrap_default_claude": true,
        "corp_tools_dir": h.0.join("custom/tools").to_string_lossy(),
    }));
    let r = run(&roots, &mut cfg);
    assert!(r.actions.iter().any(|a| a.contains("baseline")));
    assert!(cfg.cf_wrapper_hash.is_some());
    let checks = doctor::run_checks(&roots, &cfg);
    assert!(checks.iter().any(|c| c.1.contains("unchanged since baseline")));
    h.write("custom/tools/claude/extra-asset.json", "{}");
    assert!(doctor::run_checks(&roots, &cfg)
        .iter()
        .any(|c| c.1.contains("extra-asset.json")));
    let shims = h.read(".config/mcpm/context-shims.zsh");
    assert!(shims.contains(&format!(
        "local cf_dir='{}'",
        h.0.join("custom/tools").display()
    )));
    assert!(!shims.contains("${HOME}/.local/share/corp-dev-tools"));
}

#[test]
fn imported_context_json_values_are_honored() {
    let h = TempHome::new();
    h.write(
        ".config/mcpm/context.json",
        &format!(
            r#"{{"clients_root":"{0}/c","corp_tools_dir":"{0}/t"}}"#,
            h.0.display()
        ),
    );
    let roots = h.roots();
    let cfg = load_config(&roots.context_config_path());
    assert_eq!(roots.resolve_clients_root(&cfg), h.0.join("c"));
    assert_eq!(roots.resolve_corp_tools_dir(&cfg), h.0.join("t"));
}
