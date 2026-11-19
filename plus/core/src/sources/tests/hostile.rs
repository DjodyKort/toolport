use super::{items_of, source, Fx};
use crate::untrusted::fixture::{hostile_tap, put, skill_text, SECRET};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

fn every_text(report: &crate::sources::ScanReport) -> String {
    let mut text = String::new();
    for item in &report.items {
        text.push_str(&format!(
            "{} {} {}\n",
            item.name, item.path, item.description
        ));
    }
    for source in &report.sources {
        text.push_str(&source.warnings.join("\n"));
    }
    text
}

fn names(report: &crate::sources::ScanReport, id: &str) -> Vec<String> {
    items_of(report, id)
        .iter()
        .map(|i| format!("{}:{}", i.kind, i.name))
        .collect()
}

fn marketplace(clone: &Path, outside: &Path) {
    put(
        &clone.join(".claude-plugin/marketplace.json"),
        r#"{"name": "acme", "plugins": [
            {"name": "ok", "source": "./plugins/ok"},
            {"name": "linked", "source": "./plugins/linked"},
            {"name": "up", "source": "./../outside/plugin-dir"},
            {"name": "deep", "source": "./plugins/../../outside/plugin-dir"},
            {"name": "root", "source": "/etc"}]}"#,
    );
    put(
        &clone.join("plugins/ok/skills/fine/SKILL.md"),
        &skill_text("fine", "An honest plugin skill."),
    );
    symlink(outside.join("plugin-dir"), clone.join("plugins/linked")).unwrap();
}

#[test]
fn a_tap_with_links_everywhere_shows_only_its_honest_items() {
    let fx = Fx::new("hostile-tap");
    let clone = fx.w.data.join("taps/acme-tools");
    let outside = fx.w.base.join("outside");
    fs::remove_dir_all(&clone).unwrap();
    hostile_tap(&clone, &outside);
    marketplace(&clone, &outside);
    let report = fx.all();
    let got = names(&report, "tap:acme-tools");
    assert!(got.contains(&"skill:good".to_string()), "{got:?}");
    assert!(got.contains(&"skill:fine".to_string()), "{got:?}");
    for leaked in ["linked-dir", "linked-file", "top", "leak"] {
        assert!(
            !got.iter().any(|n| n.ends_with(leaked)),
            "{leaked}: {got:?}"
        );
    }
    let text = every_text(&report);
    assert!(!text.contains(SECRET), "{text}");
    let tap = source(&report, "tap:acme-tools");
    assert!(
        tap.warnings
            .iter()
            .any(|w| w.contains("is a symbolic link")),
        "{:?}",
        tap.warnings
    );
}

#[test]
fn a_plugin_install_with_links_everywhere_shows_only_its_honest_items() {
    let fx = Fx::new("hostile-plugin");
    let outside = fx.w.base.join("outside");
    let plugin = fx.w.base.join("hostile-plugin");
    hostile_tap(&plugin, &outside);
    put(
        &fx.w.claude.join("plugins/installed_plugins.json"),
        &format!(
            r#"{{"version":2,"plugins":{{"evil@market":[{{"scope":"user","installPath":"{}","version":"1.0.0"}}]}}}}"#,
            plugin.display()
        ),
    );
    let report = fx.all();
    let got = names(&report, "plugin:evil@market");
    assert!(got.contains(&"skill:good".to_string()), "{got:?}");
    for leaked in ["linked-dir", "linked-file", "top", "leak"] {
        assert!(
            !got.iter().any(|n| n.ends_with(leaked)),
            "{leaked}: {got:?}"
        );
    }
    let text = every_text(&report);
    assert!(!text.contains(SECRET), "{text}");
    assert!(
        source(&report, "plugin:evil@market")
            .warnings
            .iter()
            .any(|w| w.contains("is a symbolic link")),
        "{text}"
    );
}

#[test]
fn a_linked_skill_file_of_a_tap_is_never_read() {
    let fx = Fx::new("hostile-skill-file");
    let clone = fx.w.data.join("taps/acme-tools");
    let outside = fx.w.base.join("outside");
    put(&outside.join("secret.txt"), SECRET);
    fs::create_dir_all(clone.join("skills/sneaky")).unwrap();
    symlink(
        outside.join("secret.txt"),
        clone.join("skills/sneaky/SKILL.md"),
    )
    .unwrap();
    let report = fx.all();
    assert!(!names(&report, "tap:acme-tools").contains(&"skill:sneaky".to_string()));
    assert!(!every_text(&report).contains(SECRET));
}

#[test]
fn a_users_own_folder_keeps_its_links() {
    let fx = Fx::new("own-links");
    let shared = fx.w.base.join("shared/linked-skill");
    put(
        &shared.join("SKILL.md"),
        &skill_text("linked-skill", "A skill the user keeps elsewhere."),
    );
    let loose = fx.w.home.join(".claude/skills");
    fs::create_dir_all(&loose).unwrap();
    symlink(&shared, loose.join("linked-skill")).unwrap();
    let report = fx.all();
    assert!(report
        .items
        .iter()
        .any(|i| i.kind == "skill" && i.name == "linked-skill"));
}
