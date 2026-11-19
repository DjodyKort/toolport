use super::discover::{discover, Found};
use super::slice;
use super::store::{Item, Kind, SourceSel, State};
use super::tests::{put, skill, Fx};
use crate::untrusted::fixture::{hostile_tap, SECRET};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

/// A hostile tap of its own beside the fixture's honest one: `(tap, outside)`.
fn hostile(fx: &Fx) -> (PathBuf, PathBuf) {
    let base = fx.upstream.parent().unwrap();
    let (tap, outside) = (base.join("hostile-tap"), base.join("outside"));
    hostile_tap(&tap, &outside);
    (tap, outside)
}

fn ids(found: &[Found]) -> Vec<String> {
    found
        .iter()
        .map(|f| format!("{}:{}", f.kind.as_str(), f.id))
        .collect()
}

fn item(kind: Kind, id: &str, path: &str) -> Item {
    Item {
        id: id.into(),
        kind,
        hash: "h".into(),
        state: State::On,
        in_slice: true,
        path: Some(path.into()),
        spec: None,
        pending: None,
        extra: Default::default(),
    }
}

fn sel(items: Vec<Item>) -> SourceSel {
    SourceSel {
        slice: "example-tap-selected".into(),
        items,
        ..Default::default()
    }
}

fn holds_secret(root: &Path) -> bool {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if fs::read_to_string(&path).is_ok_and(|t| t.contains(SECRET)) {
                return true;
            }
        }
    }
    false
}

#[test]
fn test01_discovery_offers_only_the_honest_items_of_a_hostile_tap() {
    let fx = Fx::new();
    let (tap, _) = hostile(&fx);
    let found = discover(&tap);
    let got: BTreeSet<String> = ids(&found.items).into_iter().collect();
    for leaked in [
        "skill:linked-dir",
        "skill:linked-file",
        "skill:nested",
        "command:top",
        "agent:leak",
        "hook:PostToolUse:Write",
    ] {
        assert!(!got.contains(leaked), "{leaked} was offered: {got:?}");
    }
    assert!(
        got.contains("skill:good") && got.contains("rule:style"),
        "{got:?}"
    );
    let text = found.warnings.join("\n");
    for named in [
        "skills/linked-dir is a symbolic link",
        "skills/linked-file/SKILL.md is a symbolic link",
        "commands/top.md is a symbolic link",
        "agents is a symbolic link",
        "hooks/hooks.json is not read",
    ] {
        assert!(text.contains(named), "{named} not in:\n{text}");
    }
    assert!(text.contains("nested is skipped"), "{text}");
}

#[test]
fn test02_a_slice_copy_of_a_linked_or_escaping_item_is_refused() {
    let fx = Fx::new();
    let (tap, outside) = hostile(&fx);
    let staging = outside.join("staging");
    for (kind, path) in [
        (Kind::Skill, "skills/linked-dir"),
        (Kind::Skill, "skills/nested"),
        (Kind::Command, "commands/top.md"),
        (Kind::Agent, "agents/leak.md"),
        (Kind::Skill, "../outside"),
        (Kind::Skill, "skills/../../outside"),
    ] {
        let sel = sel(vec![item(kind, "x", path)]);
        let built = slice::build(&staging, &sel, &tap, &tap, &BTreeSet::new());
        assert!(built.is_err(), "{path} was copied");
    }
    assert!(!holds_secret(&staging));
}

#[test]
fn test03_a_hook_script_that_is_a_link_is_refused() {
    let fx = Fx::new();
    let (tap, outside) = hostile(&fx);
    fs::create_dir_all(tap.join("scripts")).unwrap();
    symlink(outside.join("secret.txt"), tap.join("scripts/format.sh")).unwrap();
    let mut hook = item(Kind::Hook, "PostToolUse:Write", "hooks");
    hook.path = None;
    hook.spec = Some(json!({
        "event": "PostToolUse",
        "matcher": "Write",
        "handler": {"type": "command", "command": "\"${CLAUDE_PLUGIN_ROOT}\"/scripts/format.sh"},
    }));
    let staging = outside.join("staging");
    let built = slice::build(&staging, &sel(vec![hook]), &tap, &tap, &BTreeSet::new());
    assert!(built.unwrap_err().contains("symbolic link"));
    assert!(!holds_secret(&staging));
}

#[test]
fn test04_an_item_copy_from_an_honest_source_still_works_with_its_mode() {
    let fx = Fx::new();
    skill(&fx.upstream, "plain", "Body.");
    put(&fx.upstream.join("skills/plain/scripts/run.sh"), "echo");
    let staging = fx.upstream.parent().unwrap().join("staging");
    let sel = sel(vec![item(Kind::Skill, "plain", "skills/plain")]);
    slice::build(&staging, &sel, &fx.upstream, &fx.upstream, &BTreeSet::new()).unwrap();
    assert!(staging.join("skills/plain/scripts/run.sh").is_file());
}

#[test]
fn test05_choosing_from_a_hostile_tap_only_ever_writes_honest_files() {
    let fx = Fx::new();
    let outside = fx.upstream.parent().unwrap().join("outside");
    for root in [
        fx.upstream.clone(),
        fx.plugins.data_dir.join("taps").join(super::tests::TAP),
    ] {
        fs::remove_dir_all(&root).unwrap();
        hostile_tap(&root, &outside);
    }
    fx.set(&["good"], &[], &[]).unwrap();
    assert!(fx.set(&["nested"], &[], &[]).is_err());
    fx.apply().unwrap();
    assert!(fx.slice().join("skills/good/SKILL.md").is_file());
    assert!(!holds_secret(&fx.base()));
}
