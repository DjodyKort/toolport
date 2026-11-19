use toolport_plus_api::{FakeNotifier, FakePlatform, MemoryHost, Notifier, Permission};

use super::notify::{self, TEST_TITLE};
use super::store::{Store, NOTIFY_WINDOW_SECS};
use super::system_notifier::{SystemNotifier, BLOCKED};
use super::test_toast;
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
fn test55_a_focused_window_gets_a_toast_instead_of_a_system_notification_and_it_is_not_retried() {
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
    assert_eq!(report.channel, "toast");
    assert_eq!((report.sent, report.reason), (0, None));
    assert_eq!(report.ids, ["credentials:secret:alpha"]);
    let toasts = notifier.toasts.borrow();
    assert_eq!(toasts[0].title, "alpha is missing a secret");
    assert_eq!(toasts[0].rows[0].id, "credentials:secret:alpha");
    assert_eq!(toasts[0].rows[0].screen, "servers");
    drop(toasts);
    notifier.focused.set(false);
    assert_eq!(pass(&host, &notifier).sent, 0);
    assert!(notifier.sent.borrow().is_empty());
    assert_eq!(notifier.toasts.borrow().len(), 1);
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
    assert!(notify::test(&notifier).sent);
    assert_eq!(notifier.sent.borrow()[0].0, TEST_TITLE);
}

fn asked_in_thread(
    base: std::path::PathBuf,
    now: i64,
) -> std::thread::JoinHandle<Result<(), String>> {
    std::thread::spawn(move || {
        test_toast::ask_at(&base, now, std::time::Duration::from_secs(5))
            .map(|_| ())
            .map_err(|e| e.message)
    })
}

fn answer_when_asked(host: &MemoryHost, notifier: &FakeNotifier) {
    for _ in 0..100 {
        if test_toast::answer(host, notifier) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("no request arrived");
}

#[test]
fn test60_a_test_request_from_the_command_line_shows_a_notification_in_the_app() {
    let dir = Dir::new("toast-ok");
    let host = host(&dir, Vec::new());
    let notifier = FakeNotifier::default();
    let asked = asked_in_thread(data(&dir), NOW);
    answer_when_asked(&host, &notifier);
    assert_eq!(asked.join().unwrap(), Ok(()));
    assert_eq!(notifier.sent.borrow()[0].0, TEST_TITLE);
}

#[test]
fn test61_a_refused_test_notification_tells_the_command_line_why() {
    let dir = Dir::new("toast-refused");
    let host = host(&dir, Vec::new());
    let notifier = FakeNotifier::default();
    *notifier.refuse.borrow_mut() = Some("notifications are blocked".into());
    let asked = asked_in_thread(data(&dir), NOW);
    answer_when_asked(&host, &notifier);
    assert_eq!(
        asked.join().unwrap(),
        Err("notifications are blocked".into())
    );
}

#[test]
fn test62_without_a_running_app_the_test_fails_and_leaves_no_request_behind() {
    let dir = Dir::new("toast-none");
    let error =
        test_toast::ask_at(&data(&dir), NOW, std::time::Duration::from_millis(300)).unwrap_err();
    assert_eq!(error.code(), "app_not_running");
    let host = host(&dir, Vec::new());
    assert!(!test_toast::answer(&host, &FakeNotifier::default()));
}

#[test]
fn test63_a_request_older_than_thirty_seconds_is_dropped_not_shown_late() {
    let dir = Dir::new("toast-stale");
    let host = host(&dir, Vec::new());
    let notifier = FakeNotifier::default();
    let asked = asked_in_thread(data(&dir), NOW - 120);
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(!test_toast::answer(&host, &notifier));
    assert!(notifier.sent.borrow().is_empty());
    drop(asked);
}

#[test]
fn test64_switching_notifications_off_forgets_what_was_known_so_the_next_pass_is_silent() {
    let dir = Dir::new("notify-forget");
    let host = host(&dir, vec![secret_server("alpha", true)]);
    switched_on(&dir, &[]);
    let notifier = FakeNotifier::default();
    notify::pass(&host, &notifier).unwrap();
    assert!(Store::read(&data(&dir)).unwrap().announced.is_some());
    let mut store = Store::read(&data(&dir)).unwrap();
    store.settings.notify.enabled = false;
    store.write(&data(&dir)).unwrap();
    assert!(notify::pass(&host, &notifier).unwrap().is_none());
    assert!(Store::read(&data(&dir)).unwrap().announced.is_none());
}

fn news_pass(notifier: &dyn Notifier, dir: &Dir) -> notify::Report {
    let host = host(dir, Vec::new());
    switched_on(dir, &[]);
    notify::run(&host, notifier).unwrap();
    host.registry
        .servers
        .borrow_mut()
        .push(secret_server("alpha", true));
    notify::run(&host, notifier).unwrap()
}

#[test]
fn test100_a_blocked_system_gets_no_notification_and_the_report_says_why_and_stays_quiet() {
    let dir = Dir::new("notify-blocked");
    let notifier = FakeNotifier::default();
    notifier.permission.set(Permission::Blocked);
    let report = news_pass(&notifier, &dir);
    assert_eq!(report.sent, 0);
    assert_eq!(report.channel, "none");
    assert_eq!(
        report.reason,
        Some("the system blocks notifications from Toolport+")
    );
    assert!(notifier.sent.borrow().is_empty());
    assert!(notifier.toasts.borrow().is_empty());
}

#[test]
fn test101_a_blocked_system_does_not_stop_the_toast_of_a_focused_window() {
    let dir = Dir::new("notify-blocked-focus");
    let notifier = FakeNotifier::default();
    notifier.permission.set(Permission::Blocked);
    notifier.focused.set(true);
    let report = news_pass(&notifier, &dir);
    assert_eq!(report.channel, "toast");
    assert_eq!(notifier.toasts.borrow().len(), 1);
}

#[test]
fn test102_the_system_notifier_refuses_to_hand_over_when_the_platform_says_blocked() {
    let notifier = SystemNotifier(FakePlatform::default());
    notifier.0.permission.set(Permission::Blocked);
    assert_eq!(notifier.notify("t", "b"), Err(BLOCKED.to_string()));
    assert!(notifier.0.shown.borrow().is_empty());
}

#[test]
fn test103_the_system_notifier_passes_title_and_body_through_and_reports_the_platforms_errors() {
    let notifier = SystemNotifier(FakePlatform::default());
    notifier.notify("title", "body").unwrap();
    assert_eq!(
        notifier.0.shown.borrow()[0],
        ("title".into(), "body".into())
    );
    *notifier.0.show_error.borrow_mut() = Some("no notification daemon".into());
    assert_eq!(
        notifier.notify("t", "b"),
        Err("no notification daemon".to_string())
    );
}

#[test]
fn test104_a_toast_and_the_settings_request_reach_the_platform() {
    let notifier = SystemNotifier(FakePlatform::default());
    notifier.0.focused.set(true);
    assert!(notifier.window_focused());
    let toast = toolport_plus_api::Toast {
        title: "t".into(),
        body: "b".into(),
        rows: Vec::new(),
    };
    notifier.toast(&toast).unwrap();
    assert_eq!(notifier.0.toasts.borrow()[0], toast);
    notifier.open_settings().unwrap();
    assert_eq!(notifier.0.settings_opened.get(), 1);
}

#[test]
fn test105_the_test_says_unconfirmed_on_a_platform_that_does_not_report_permission() {
    let notifier = SystemNotifier(FakePlatform::default());
    let outcome = notify::test(&notifier);
    assert!(outcome.sent);
    assert!(!outcome.confirmed());
    assert_eq!(outcome.permission, Permission::Unknown);
}

#[test]
fn test106_the_test_is_confirmed_only_when_the_platform_reports_allowed_and_blocked_is_a_failure() {
    let notifier = SystemNotifier(FakePlatform::default());
    notifier.0.permission.set(Permission::Allowed);
    assert!(notify::test(&notifier).confirmed());
    notifier.0.permission.set(Permission::Blocked);
    let blocked = notify::test(&notifier);
    assert!(!blocked.sent);
    assert_eq!(blocked.code(), Some("notifications_blocked"));
    assert_eq!(notifier.0.shown.borrow().len(), 1);
}

fn answered_test(
    notifier: &FakeNotifier,
    kind_settings: bool,
) -> Result<serde_json::Value, super::super::op::OpError> {
    let dir = Dir::new("toast-truth");
    let host = host(&dir, Vec::new());
    let base = data(&dir);
    let asked = std::thread::spawn(move || {
        if kind_settings {
            test_toast::ask_settings_at(&base, NOW, std::time::Duration::from_secs(5))
        } else {
            test_toast::ask_at(&base, NOW, std::time::Duration::from_secs(5))
        }
    });
    answer_when_asked(&host, notifier);
    asked.join().unwrap()
}

#[test]
fn test107_the_command_line_hears_the_permission_and_never_a_bare_sent_true() {
    let notifier = FakeNotifier::default();
    let unknown = answered_test(&notifier, false).unwrap();
    assert_eq!(unknown["sent"], true);
    assert_eq!(unknown["permission"], "unknown");
    assert_eq!(unknown["confirmed"], false);
    assert!(unknown["note"]
        .as_str()
        .unwrap()
        .contains("System Settings"));

    notifier.permission.set(Permission::Allowed);
    let allowed = answered_test(&notifier, false).unwrap();
    assert_eq!(allowed["confirmed"], true);
    assert!(allowed.get("note").is_none());
}

#[test]
fn test108_a_blocked_system_fails_the_test_with_its_own_code_and_how_to_fix_it() {
    let notifier = FakeNotifier::default();
    notifier.permission.set(Permission::Blocked);
    let error = answered_test(&notifier, false).unwrap_err();
    assert_eq!(error.code(), "notifications_blocked");
    assert!(error.message.contains("System Settings"));
    assert_eq!(error.details.unwrap()["permission"], "blocked");
    assert!(notifier.sent.borrow().is_empty());
}

#[test]
fn test109_the_settings_request_opens_the_system_settings_once() {
    let notifier = FakeNotifier::default();
    let opened = answered_test(&notifier, true).unwrap();
    assert_eq!(opened["opened"], true);
    assert_eq!(notifier.settings_opened.get(), 1);
}

#[test]
fn test110_the_permission_the_app_saw_is_what_attention_settings_reports() {
    let dir = Dir::new("notify-state");
    let host = host(&dir, Vec::new());
    assert_eq!(
        super::ops::settings(&host).unwrap()["notify"]["permission"],
        serde_json::json!({"state": "unknown", "checkedAt": null})
    );
    let notifier = FakeNotifier::default();
    notifier.permission.set(Permission::Blocked);
    notify::pass(&host, &notifier).unwrap();
    let seen = super::ops::settings(&host).unwrap();
    assert_eq!(seen["notify"]["permission"]["state"], "blocked");
    assert_eq!(
        seen["notify"]["permission"]["checkedAt"],
        crate::tasks::cron::rfc3339(NOW)
    );
}
