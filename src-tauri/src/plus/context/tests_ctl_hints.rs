use super::config::ProfileSpec;
use super::layers::{deploy_client_locals, MANAGED_LOCAL_HEADER};
use super::{doctor, load_config, shims, Report, Roots};
use crate::plus::ctl;
use crate::plus::randutil::ScratchDir;
use regex::Regex;
use std::collections::BTreeMap;
use std::fs;

const SOURCES: [(&str, &str); 5] = [
    ("shims.rs", include_str!("shims.rs")),
    ("doctor.rs", include_str!("doctor.rs")),
    ("mod.rs", include_str!("mod.rs")),
    ("launch.rs", include_str!("launch.rs")),
    ("layers.rs", include_str!("layers.rs")),
];

const LEGACY_HEADER: &str = "<!-- Managed by `mcpm context` — edit the canonical layer (skills_repo/rules/client-*/SKILL.md); `mcpm context sync` regenerates. -->";

fn without_contract_names(text: &str) -> String {
    text.replace("mcpm_context_presync", "")
        .replace(".config/mcpm", "")
        .replace(".cache/mcpm", "")
}

#[test]
fn shim_output_has_no_mcpm_command() {
    let home = ScratchDir::new("ctl-hints-shims");
    let roots = Roots::from_home(home.path());
    let mut profiles = BTreeMap::new();
    profiles.insert("work".to_string(), ProfileSpec::default());
    for wrap in [true, false] {
        let text = shims::shim_snippet(&roots, &profiles, wrap);
        let rest = without_contract_names(&text);
        assert!(!rest.contains("mcpm"), "wrap={wrap}: {rest}");
    }
    let on = shims::shim_snippet(&roots, &profiles, true);
    assert!(on.contains("command toolportctl context sync >/dev/null 2>&1"));
    assert!(on.contains("command toolportctl compression run -- \"$@\""));
    assert!(on.contains("print -u2 \"toolportctl: context sync failed, retrying next launch (run 'toolportctl doctor')\""));
    assert!(on.starts_with("# Managed by `toolportctl context`"));
}

#[test]
fn profile_free_shims_equal_the_vendored_goldens() {
    let home = ScratchDir::new("ctl-hints-golden");
    let roots = Roots::from_home(home.path());
    let none = BTreeMap::new();
    assert_eq!(
        shims::shim_snippet(&roots, &none, true),
        include_str!("../../../tests/fixtures/parity/context/shims-wrap-on/tree/_golden/snippet.wrap-on.no-profiles.zsh")
    );
    assert_eq!(
        shims::shim_snippet(&roots, &none, false),
        include_str!("../../../tests/fixtures/parity/context/shims-wrap-off/tree/_golden/snippet.wrap-off.no-profiles.zsh")
    );
}

#[test]
fn generated_headers_name_toolportctl() {
    let rest = without_contract_names(&format!(
        "{MANAGED_LOCAL_HEADER}{}",
        super::launch::APPEND_HEADER
    ));
    assert!(!rest.contains("mcpm"), "{rest}");
    assert!(MANAGED_LOCAL_HEADER.contains("`toolportctl context sync` regenerates"));
    assert!(super::launch::APPEND_HEADER.contains("`toolportctl context sync`"));
}

#[test]
fn every_toolportctl_command_in_the_context_sources_exists() {
    let command = Regex::new(r"toolportctl((?: [a-z][a-z-]*)+)").unwrap();
    let mut seen = Vec::new();
    for (file, text) in SOURCES {
        for found in command.captures_iter(text) {
            let words: Vec<String> = found[1].split_whitespace().map(String::from).collect();
            let (resolved, _) = ctl::find_command(&words)
                .unwrap_or_else(|| panic!("{file}: no ctl command for `toolportctl{}`", &found[1]));
            assert!(
                resolved.handler.is_some(),
                "{file}: `toolportctl{}` is only a planned command",
                &found[1]
            );
            seen.push(resolved.path.join(" "));
        }
    }
    for expected in [
        "context sync",
        "context",
        "skills sync",
        "compression run",
        "doctor",
    ] {
        assert!(
            seen.iter().any(|s| s == expected),
            "{expected} not referenced: {seen:?}"
        );
    }
}

#[test]
fn doctor_hints_name_toolportctl_and_no_mcpm_command() {
    let home = ScratchDir::new("ctl-hints-doctor");
    let roots = Roots::from_home(home.path());
    let checks = doctor::run_checks(&roots, &load_config(&roots.context_config_path()));
    let text: Vec<&str> = checks.iter().map(|c| c.1.as_str()).collect();
    assert!(text
        .iter()
        .any(|m| *m == "shims file missing — run `toolportctl context sync`"));
    assert!(text
        .iter()
        .any(|m| m.starts_with("no personal layer scaffolded")
            && m.contains("rules/personal/SKILL.md")));
    for message in &text {
        assert!(
            !message.contains("mcpm context") && !message.contains("mcpm skills"),
            "{message}"
        );
    }
}

#[test]
fn skills_sync_hint_matches_the_ctl_global_default() {
    let home = ScratchDir::new("ctl-hints-skills");
    let roots = Roots::from_home(home.path());
    fs::create_dir_all(roots.rules_dir().join("personal")).unwrap();
    fs::write(
        roots.rules_dir().join("personal/SKILL.md"),
        "---\nname: personal\nactivation: always\n---\nmine\n",
    )
    .unwrap();
    let checks = doctor::run_checks(&roots, &load_config(&roots.context_config_path()));
    let hint = checks
        .iter()
        .map(|c| c.1.as_str())
        .find(|m| m.starts_with("personal layer not transpiled"))
        .expect("transpile hint");
    assert!(hint.ends_with("run `toolportctl skills sync`"), "{hint}");
    assert!(!hint.contains("--global"));
}

#[test]
fn files_written_by_mcpm_stay_managed_and_get_the_new_header() {
    let home = ScratchDir::new("ctl-hints-legacy");
    let roots = Roots::from_home(home.path());
    let clients = home.path().join("clients");
    for child in ["old", "mine"] {
        fs::create_dir_all(clients.join(child)).unwrap();
        fs::create_dir_all(roots.rules_dir().join(format!("client-{child}"))).unwrap();
        fs::write(
            roots.rules_dir().join(format!("client-{child}/SKILL.md")),
            format!("---\nname: client-{child}\nactivation: always\n---\nbody of {child}\n"),
        )
        .unwrap();
    }
    let old = clients.join("old/CLAUDE.local.md");
    let mine = clients.join("mine/CLAUDE.local.md");
    fs::write(&old, format!("{LEGACY_HEADER}\n\nstale\n")).unwrap();
    fs::write(&mine, "hand written\n").unwrap();

    let mut report = Report::default();
    deploy_client_locals(&roots, &clients, &mut report, false).unwrap();

    assert_eq!(
        fs::read_to_string(&old).unwrap(),
        format!("{MANAGED_LOCAL_HEADER}\n\nbody of old\n")
    );
    assert_eq!(fs::read_to_string(&mine).unwrap(), "hand written\n");
    assert!(report
        .actions
        .iter()
        .any(|a| a.starts_with("deployed") && a.contains("old")));
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("mine") && w.contains("not managed")));
}

#[test]
fn orphan_profile_hint_is_not_an_mcpm_command() {
    let home = ScratchDir::new("ctl-hints-orphan");
    let roots = Roots::from_home(home.path());
    fs::create_dir_all(roots.profiles_root().join("stale")).unwrap();
    let mut config = super::ContextConfig::from_value(serde_json::json!({})).unwrap();
    let report = super::apply(&roots, &mut config, super::ApplyOptions::default()).unwrap();
    let warning = report
        .warnings
        .iter()
        .find(|w| w.starts_with("orphan profile dir"))
        .expect("orphan warning");
    assert!(warning
        .ends_with("(not in config) — delete the directory or add the profile to context.json"));
    assert!(!warning.contains("mcpm context"));
}
