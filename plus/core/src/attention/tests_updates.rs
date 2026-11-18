use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use super::feed_updates::{self, TapFacts, UpdateInputs};
use super::store::{HideKind, Store, NEEDS_YOU_SNOOZE_MAX};
use super::{compute, updates, Inputs, Item, Urgency};
use crate::fsx::{enter, Paths};
use crate::skills::taps::{save_taps, tap_dir, taps_root, Tap};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const NOW: i64 = 1_792_000_000;
const DAY: i64 = 24 * 3600;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

struct World {
    base: PathBuf,
    data: PathBuf,
    upstream: PathBuf,
    _paths: crate::fsx::PathsGuard,
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "plus-updates-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let (data, upstream) = (base.join("data"), base.join("upstream"));
        std::fs::create_dir_all(base.join("home")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&upstream).unwrap();
        git(&upstream, &["init", "-q", "-b", "main"]);
        let world = Self {
            _paths: enter(Paths {
                home: Some(base.join("home")),
                data_dir: Some(data.clone()),
            }),
            base,
            data,
            upstream,
        };
        world.commit("first");
        world
    }

    fn commit(&self, name: &str) {
        std::fs::write(self.upstream.join(name), name).unwrap();
        git(&self.upstream, &["add", "."]);
        git(&self.upstream, &["commit", "-q", "-m", name]);
    }

    fn add_tap(&self, name: &str) -> PathBuf {
        let url = format!("file://{}", self.upstream.display());
        let tap = Tap {
            name: name.into(),
            repo: url.clone(),
            url: url.clone(),
        };
        let dir = tap_dir(&taps_root(&self.data), &tap);
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        git(
            &self.base,
            &["clone", "-q", "--depth", "1", &url, &dir.to_string_lossy()],
        );
        let mut taps = crate::skills::taps::load_taps(&self.data);
        taps.push(tap);
        save_taps(&self.data, &taps).unwrap();
        dir
    }

    fn head(&self, dir: &Path) -> String {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn inputs_of(world: &World) -> Inputs {
    let store = Store::read(&world.data);
    Inputs {
        now: NOW,
        updates: Ok(updates::inputs(
            &world.data,
            store.as_ref().unwrap_or(&Store::default()),
        )),
        store,
        access: Ok(super::feed_access::AccessInputs {
            unreached: Vec::new(),
            folders: Vec::new(),
        }),
        credentials: Ok(super::feed_credentials::CredentialInputs {
            servers: Vec::new(),
            vault_error: None,
            logins: Vec::new(),
        }),
        tasks: Ok(super::feed_tasks::TaskInputs {
            tasks: Vec::new(),
            runs: Vec::new(),
            missed: Vec::new(),
            records: Vec::new(),
        }),
        library: Ok(super::feed_library::LibraryInputs {
            auto: Ok(Default::default()),
            repo: None,
        }),
    }
}

fn real_fetch(dir: &Path) -> Result<(), String> {
    updates::fetch(dir)
}

fn row(id: &str, urgency: Urgency, skip: Option<&str>) -> Item {
    Item {
        id: id.into(),
        urgency,
        title: id.into(),
        detail: String::new(),
        source: "Updates",
        since: None,
        action: None,
        skip: skip.map(str::to_string),
    }
}

#[test]
fn test30_a_tap_that_is_behind_after_a_fetch_is_one_skippable_row_and_nothing_is_applied() {
    let world = World::new("behind");
    let dir = world.add_tap("team-skills");
    let before = world.head(&dir);
    world.commit("second");

    assert!(feed_updates::items(&updates::inputs(&world.data, &Store::default())).is_empty());
    let report = updates::check(&world.data, NOW, &real_fetch).unwrap();
    assert!(report.sources.iter().all(|s| s.error.is_none()));
    assert_eq!(world.head(&dir), before);

    let snapshot = compute(&inputs_of(&world));
    assert_eq!(snapshot.items.len(), 1);
    let item = &snapshot.items[0];
    assert_eq!(item.urgency, Urgency::WorthALook);
    assert_eq!(
        item.title,
        "The skill tap team-skills has 1 new commit upstream"
    );
    assert!(item.id.starts_with("updates:tap:team-skills:") && item.id.contains('>'));
    assert!(item.skip.as_deref().unwrap().starts_with("Skip "));
    assert_eq!(
        item.action
            .as_ref()
            .unwrap()
            .command
            .as_ref()
            .unwrap()
            .join(" "),
        "toolportctl skills tap update team-skills"
    );
    let updates_check = snapshot
        .checks
        .iter()
        .find(|c| c.name == "updates")
        .unwrap();
    assert_eq!(updates_check.checked_at, Some(NOW));
}

#[test]
fn test31_a_tap_with_a_plugin_marketplace_is_named_as_one() {
    let world = World::new("marketplace");
    let dir = world.add_tap("plugins");
    std::fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
    std::fs::write(dir.join(".claude-plugin/marketplace.json"), "{}").unwrap();
    world.commit("second");
    updates::check(&world.data, NOW, &real_fetch).unwrap();
    let snapshot = compute(&inputs_of(&world));
    assert_eq!(
        snapshot.items[0].title,
        "The plugin marketplace plugins has 1 new commit upstream"
    );
}

#[test]
fn test32_a_tap_that_is_current_raises_nothing() {
    let world = World::new("current");
    world.add_tap("team-skills");
    updates::check(&world.data, NOW, &real_fetch).unwrap();
    assert!(compute(&inputs_of(&world)).items.is_empty());
}

#[test]
fn test33_skipping_the_row_keeps_it_away_until_the_next_commit_changes_its_id() {
    let world = World::new("skip");
    world.add_tap("team-skills");
    world.commit("second");
    updates::check(&world.data, NOW, &real_fetch).unwrap();
    let first = compute(&inputs_of(&world)).items.remove(0);

    let mut store = Store::read(&world.data).unwrap();
    store.hide(&first, HideKind::Skip, None, NOW).unwrap();
    store.write(&world.data).unwrap();
    let hidden = compute(&inputs_of(&world));
    assert!(hidden.items.is_empty());
    assert_eq!(hidden.hidden.len(), 1);
    assert_eq!(hidden.hidden[0].hide.kind, HideKind::Skip);

    world.commit("third");
    updates::check(&world.data, NOW + DAY, &real_fetch).unwrap();
    let again = compute(&inputs_of(&world));
    assert_eq!(again.items.len(), 1);
    assert_ne!(again.items[0].id, first.id);
}

#[test]
fn test34_an_unreachable_remote_is_recorded_and_all_sources_failing_reads_as_offline() {
    let world = World::new("offline");
    world.add_tap("team-skills");
    std::fs::remove_dir_all(&world.upstream).unwrap();
    let report = updates::check(&world.data, NOW, &real_fetch).unwrap();
    assert_eq!(report.sources.len(), 1);
    assert!(report.sources[0].error.is_some());

    let snapshot = compute(&inputs_of(&world));
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].id, "updates:unreachable");
    assert_eq!(snapshot.items[0].title, "You look to be offline");
    assert_eq!(snapshot.items[0].urgency, Urgency::WorthALook);
    assert!(snapshot.checks.iter().all(|c| c.error.is_none()));
}

#[test]
fn test35_some_sources_failing_names_how_many_instead_of_offline() {
    let inputs = UpdateInputs {
        servers: Vec::new(),
        taps: Vec::new(),
        failures: vec![("tap:a".into(), "network unreachable".into())],
        checked: 3,
        peers: Vec::new(),
        unpublished: Vec::new(),
    };
    let items = feed_updates::items(&inputs);
    assert_eq!(
        items[0].title,
        "1 source could not be reached for the update check"
    );
    assert_eq!(items[0].detail, "tap:a: network unreachable");
}

#[test]
fn test36_the_check_never_contacts_a_host_the_egress_list_refuses() {
    let world = World::new("egress");
    let repo = world.base.join("clone");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &["remote", "add", "origin", "https://example.invalid/x"],
    );
    for (url, refused) in [
        ("https://updates.toolport.app/x.git", true),
        ("git@toolport.app:team/x.git", true),
        ("https://github.com/team/x.git", false),
        ("file:///tmp/x", false),
        ("/srv/git/x", false),
    ] {
        git(&repo, &["remote", "set-url", "origin", url]);
        let result = updates::egress_check(&repo);
        assert_eq!(result.is_err(), refused, "{url}");
        if refused {
            assert!(crate::egress::is_refusal(&result.unwrap_err()));
        }
    }
}

#[test]
fn test37_a_switched_off_updates_check_fetches_nothing_and_counts_as_fresh() {
    let world = World::new("off");
    world.add_tap("team-skills");
    let mut store = Store::default();
    store.settings.checks.insert("updates".into(), false);
    store.write(&world.data).unwrap();
    let fetched = std::cell::Cell::new(0);
    let report = updates::check(&world.data, NOW, &|_| {
        fetched.set(fetched.get() + 1);
        Ok(())
    })
    .unwrap();
    assert!(report.off);
    assert_eq!(fetched.get(), 0);
    let snapshot = compute(&inputs_of(&world));
    let check = snapshot
        .checks
        .iter()
        .find(|c| c.name == "updates")
        .unwrap();
    assert!(check.off && check.error.is_none());
    assert_eq!(snapshot.to_json()["checks"][1]["off"], json!(true));
}

#[test]
fn test38_a_store_that_cannot_be_read_stops_the_check_and_is_named_in_the_snapshot() {
    let world = World::new("badstore");
    world.add_tap("team-skills");
    std::fs::create_dir_all(world.data.join("plus")).unwrap();
    std::fs::write(world.data.join("plus/attention.json"), "{not json").unwrap();
    assert!(updates::check(&world.data, NOW, &|_| Ok(())).is_err());
    let snapshot = compute(&inputs_of(&world));
    let hidden = snapshot.checks.iter().find(|c| c.name == "hidden").unwrap();
    assert!(hidden.error.as_ref().unwrap().contains("is not valid"));
}

#[test]
fn test39_the_store_keeps_keys_it_does_not_know() {
    let world = World::new("roundtrip");
    std::fs::create_dir_all(world.data.join("plus")).unwrap();
    std::fs::write(
        world.data.join("plus/attention.json"),
        r#"{"futureKey": {"a": 1}, "hidden": {}}"#,
    )
    .unwrap();
    let mut store = Store::read(&world.data).unwrap();
    store.updates.checked_at = Some(NOW);
    store.write(&world.data).unwrap();
    let text = std::fs::read_to_string(world.data.join("plus/attention.json")).unwrap();
    assert!(text.contains("futureKey"));
}

#[test]
fn test40_a_needs_you_row_is_snoozed_for_at_most_a_week_and_never_skipped_or_dismissed() {
    let mut store = Store::default();
    let urgent = row("auth:alpha", Urgency::NeedsYou, None);
    let error = store
        .hide(
            &urgent,
            HideKind::Snooze,
            Some(NOW + NEEDS_YOU_SNOOZE_MAX + 1),
            NOW,
        )
        .unwrap_err();
    assert!(error.contains("at most 7 days"));
    assert!(store.hide(&urgent, HideKind::Skip, None, NOW).is_err());
    assert!(store.hide(&urgent, HideKind::Dismiss, None, NOW).is_err());
    store
        .hide(
            &urgent,
            HideKind::Snooze,
            Some(NOW + NEEDS_YOU_SNOOZE_MAX),
            NOW,
        )
        .unwrap();
    assert!(store.hiding(&urgent, NOW + 1).is_some());
    assert!(store.hiding(&urgent, NOW + NEEDS_YOU_SNOOZE_MAX).is_none());
}

#[test]
fn test41_a_snooze_ends_when_the_row_escalates() {
    let mut store = Store::default();
    let amber = row("tasks:nightly:failed", Urgency::WorthALook, None);
    store
        .hide(&amber, HideKind::Snooze, Some(NOW + DAY), NOW)
        .unwrap();
    assert!(store.hiding(&amber, NOW).is_some());
    let red = row("tasks:nightly:failed", Urgency::NeedsYou, None);
    assert!(store.hiding(&red, NOW).is_none());
}

#[test]
fn test42_skip_needs_a_signature_and_dismiss_needs_none() {
    let mut store = Store::default();
    let plain = row("updates:unreachable", Urgency::WorthALook, None);
    let signed = row("updates:tap:a:1>2", Urgency::WorthALook, Some("Skip 2"));
    assert!(store.hide(&plain, HideKind::Skip, None, NOW).is_err());
    assert!(store.hide(&signed, HideKind::Dismiss, None, NOW).is_err());
    store.hide(&plain, HideKind::Dismiss, None, NOW).unwrap();
    store.hide(&signed, HideKind::Skip, None, NOW).unwrap();
    assert!(store.hiding(&plain, NOW + 400 * DAY).is_some());
    assert!(store.hiding(&signed, NOW + 400 * DAY).is_some());
}

#[test]
fn test43_undo_puts_back_what_the_last_change_replaced() {
    let mut store = Store::default();
    let amber = row("tasks:a:failed", Urgency::WorthALook, None);
    store
        .hide(&amber, HideKind::Snooze, Some(NOW + DAY), NOW)
        .unwrap();
    store
        .hide(&amber, HideKind::Dismiss, None, NOW + 1)
        .unwrap();
    assert_eq!(store.hidden["tasks:a:failed"].kind, HideKind::Dismiss);
    assert_eq!(store.undo().as_deref(), Some("tasks:a:failed"));
    assert_eq!(store.hidden["tasks:a:failed"].kind, HideKind::Snooze);
    assert_eq!(store.undo(), None);

    assert!(store.restore("tasks:a:failed"));
    assert!(store.hidden.is_empty());
    assert!(!store.restore("tasks:a:failed"));
    store.undo();
    assert_eq!(store.hidden["tasks:a:failed"].kind, HideKind::Snooze);
}

#[test]
fn test44_entries_nobody_returns_for_thirty_days_and_old_notifications_are_dropped() {
    let mut store = Store::default();
    let amber = row("tasks:a:failed", Urgency::WorthALook, None);
    let other = row("tasks:b:failed", Urgency::WorthALook, None);
    store.hide(&amber, HideKind::Dismiss, None, NOW).unwrap();
    store.hide(&other, HideKind::Dismiss, None, NOW).unwrap();
    store.notified.insert("old".into(), NOW);
    store.tidy(&["tasks:a:failed"], NOW + 29 * DAY);
    assert_eq!(store.hidden.len(), 2);
    store.tidy(&["tasks:a:failed"], NOW + 31 * DAY);
    assert_eq!(store.hidden.keys().collect::<Vec<_>>(), ["tasks:a:failed"]);
    assert!(store.notified.is_empty());
}

#[test]
fn test45_a_tap_row_needs_the_remote_commit_the_fetch_saw() {
    let inputs = UpdateInputs {
        servers: Vec::new(),
        taps: vec![TapFacts {
            name: "a".into(),
            marketplace: false,
            behind: 2,
            head: Some("1111111122222222".into()),
            upstream_head: None,
        }],
        failures: Vec::new(),
        checked: 1,
        peers: Vec::new(),
        unpublished: Vec::new(),
    };
    assert!(feed_updates::items(&inputs).is_empty());
}
