use toolport_plus_api::{FakeNotifier, MemoryHost};

use super::notify::{self, TEST_TITLE};
use super::store::{Store, NOTIFY_WINDOW_SECS};
use super::tests::{host, secret_server, Dir, NOW};

fn data(dir: &Dir) -> std::path::PathBuf {
    dir.0.clone()
}

fn switched_on(dir: &Dir, categories: &[(&str, bool)]) {
    let mut store = Store::read(&data(dir)).unwrap();
    store.settings.notify.enabled = true;
    for (name, on) in categories {
        store
            .settings
            .notify
            .categories
            .insert(name.to_string(), *on);
    }
    store.write(&data(dir)).unwrap();
}

fn pass(host: &MemoryHost, notifier: &FakeNotifier) -> notify::Report {
    notify::run(host, notifier).unwrap()
}

#[test]
fn test50_nothing_is_sent_until_the_master_switch_is_on() {
    let dir = Dir::new("notify-off");
    let host = host(&dir, Vec::new());
    let notifier = FakeNotifier::default();
    pass(&host, &notifier);
    host.registry
        .servers
        .borrow_mut()
        .push(secret_server("alpha", true));
    let report = pass(&host, &notifier);
    assert_eq!(report.reason, Some("notifications are off"));
    assert!(notifier.sent.borrow().is_empty());
}

#[test]
fn test51_a_row_that_was_open_at_the_first_pass_is_not_announced() {
    let dir = Dir::new("notify-first");
    let host = host(&dir, vec![secret_server("alpha", true)]);
    switched_on(&dir, &[]);
    let notifier = FakeNotifier::default();
    let first = pass(&host, &notifier);
    assert!(first.reason.unwrap().starts_with("first pass"));
    assert!(pass(&host, &notifier).ids.is_empty());
    assert!(notifier.sent.borrow().is_empty());
}

#[test]
fn test52_a_new_needs_you_row_is_one_notification_with_its_title_and_no_secret_or_path() {
    let dir = Dir::new("notify-one");
    let host = host(&dir, Vec::new());
    switched_on(&dir, &[]);
    let notifier = FakeNotifier::default();
    pass(&host, &notifier);
    host.registry
        .servers
        .borrow_mut()
        .push(secret_server("alpha", true));
    let report = pass(&host, &notifier);
    assert_eq!(report.sent, 1);
    let sent = notifier.sent.borrow();
    assert_eq!(sent[0].0, "alpha is missing a secret");
    assert_eq!(
        sent[0].1,
        "API_KEY is not set, so the server cannot start. Open Toolport+ to fix it."
    );
    assert!(!sent[0].1.contains('/'));
}

#[test]
fn test53_rows_that_appear_in_one_pass_are_batched_into_one_notification() {
    let dir = Dir::new("notify-batch");
    let host = host(&dir, Vec::new());
    switched_on(&dir, &[]);
    let notifier = FakeNotifier::default();
    pass(&host, &notifier);
    for id in ["alpha", "beta", "gamma"] {
        host.registry
            .servers
            .borrow_mut()
            .push(secret_server(id, true));
    }
    assert_eq!(pass(&host, &notifier).sent, 1);
    let sent = notifier.sent.borrow();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "3 things need you");
    assert_eq!(
        sent[0].1,
        "alpha is missing a secret, beta is missing a secret and 1 more. Open Toolport+ to see them."
    );
}

#[test]
fn test54_the_same_row_is_not_announced_again_inside_the_window_even_when_it_flaps() {
    let dir = Dir::new("notify-window");
    let host = host(&dir, Vec::new());
    switched_on(&dir, &[]);
    let notifier = FakeNotifier::default();
    pass(&host, &notifier);
    host.registry
        .servers
        .borrow_mut()
        .push(secret_server("alpha", true));
    pass(&host, &notifier);
    host.secrets
        .vault
        .borrow_mut()
        .insert(("alpha".into(), "API_KEY".into()), "synthetic".into());
    pass(&host, &notifier);
    host.secrets.vault.borrow_mut().clear();
    host.now.set(NOW + NOTIFY_WINDOW_SECS - 1);
    assert_eq!(pass(&host, &notifier).sent, 0);
    assert_eq!(notifier.sent.borrow().len(), 1);

    host.secrets
        .vault
        .borrow_mut()
        .insert(("alpha".into(), "API_KEY".into()), "synthetic".into());
    pass(&host, &notifier);
    host.secrets.vault.borrow_mut().clear();
    host.now.set(NOW + NOTIFY_WINDOW_SECS);
    assert_eq!(pass(&host, &notifier).sent, 1);
}

#[test]
fn test55_no_system_notification_while_the_window_has_the_focus_and_it_is_not_retried() {
    let dir = Dir::new("notify-focus");
    let host = host(&dir, Vec::new());
    switched_on(&dir, &[]);
    let notifier = FakeNotifier::default();
    pass(&host, &notifier);
    notifier.focused.set(true);
    host.registry
        .servers
        .borrow_mut()
        .push(secret_server("alpha", true));
    let report = pass(&host, &notifier);
    assert_eq!(report.reason, Some("the window has the focus"));
    assert_eq!(report.ids, ["credentials:secret:alpha"]);
    notifier.focused.set(false);
    assert_eq!(pass(&host, &notifier).sent, 0);
    assert!(notifier.sent.borrow().is_empty());
}

#[test]
fn test56_a_category_that_is_off_and_a_worth_a_look_row_never_notify() {
    let dir = Dir::new("notify-category");
    let host = host(&dir, Vec::new());
    switched_on(&dir, &[("credentials", false)]);
    let notifier = FakeNotifier::default();
    pass(&host, &notifier);
    host.registry
        .servers
        .borrow_mut()
        .push(secret_server("alpha", true));
    assert_eq!(pass(&host, &notifier).reason, Some("nothing new"));
    assert!(notifier.sent.borrow().is_empty());
}

#[test]
fn test57_a_snoozed_row_stays_quiet_and_comes_back_without_a_notification() {
    let dir = Dir::new("notify-snooze");
    let host = host(&dir, Vec::new());
    switched_on(&dir, &[]);
    let notifier = FakeNotifier::default();
    pass(&host, &notifier);
    host.registry
        .servers
        .borrow_mut()
        .push(secret_server("alpha", true));
    let item = super::snapshot(&host).items.remove(0);
    let mut store = Store::read(&data(&dir)).unwrap();
    store
        .hide(&item, super::store::HideKind::Snooze, Some(NOW + 3600), NOW)
        .unwrap();
    store.write(&data(&dir)).unwrap();
    assert_eq!(pass(&host, &notifier).sent, 0);
    host.now.set(NOW + 7200);
    assert_eq!(pass(&host, &notifier).sent, 0);
    assert!(notifier.sent.borrow().is_empty());
}

#[test]
fn test58_a_system_that_refuses_is_reported_and_not_retried() {
    let dir = Dir::new("notify-refused");
    let host = host(&dir, Vec::new());
    switched_on(&dir, &[]);
    let notifier = FakeNotifier::default();
    pass(&host, &notifier);
    *notifier.refuse.borrow_mut() = Some("notifications are blocked".into());
    host.registry
        .servers
        .borrow_mut()
        .push(secret_server("alpha", true));
    let report = pass(&host, &notifier);
    assert_eq!(report.error.as_deref(), Some("notifications are blocked"));
    *notifier.refuse.borrow_mut() = None;
    assert_eq!(pass(&host, &notifier).sent, 0);
}

#[test]
fn test59_the_test_notification_says_it_is_a_test() {
    let notifier = FakeNotifier::default();
    notify::test(&notifier).unwrap();
    assert_eq!(notifier.sent.borrow()[0].0, TEST_TITLE);
}
