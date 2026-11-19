use super::*;
use toolport_plus_api::RunDecision;

fn host_in(dir: &TempDir) -> MemoryHost {
    let mut host = MemoryHost::default();
    host.registry.path = Some(dir.path().join("registry.json"));
    *host.tasks.base.borrow_mut() = Some(dir.path().to_path_buf());
    host.now.set(NOW / 1000);
    host
}

fn dir_of(dir: &TempDir) -> std::path::PathBuf {
    super::super::spend_dir(dir.path())
}

fn stored(dir: &TempDir, ages_days: &[i64]) -> Store {
    let store = Store::new(&dir_of(dir));
    std::fs::create_dir_all(dir_of(dir)).unwrap();
    for (i, age) in ages_days.iter().enumerate() {
        let session = format!("s{i}");
        store
            .append(
                &[token_event(
                    &session,
                    TOKEN_INPUT,
                    10.0,
                    NOW - age * MS_PER_DAY,
                )],
                u64::MAX,
            )
            .unwrap();
    }
    store
}

fn config_edit<'a>() -> super::super::api::ConfigEdit<'a> {
    super::super::api::ConfigEdit::default()
}

#[test]
fn test50_every_price_behind_an_estimate_is_marked_unverified_in_the_report() {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path());
    let summary = report::summary(
        &inputs(&store, &[], vec![]),
        Range::Days7,
        &Filter::default(),
    );
    let prices = &summary["prices"];
    assert_eq!(prices["verified"], false);
    assert_eq!(prices["factorsVerified"], false);
    let families = prices["families"].as_array().unwrap();
    assert_eq!(families.len(), 3);
    assert!(families.iter().all(|f| f["verified"] == false));
}

#[test]
fn test51_prune_older_than_removes_what_is_inside_the_retention_but_never_extends_it() {
    let dir = TempDir::new().unwrap();
    let host = host_in(&dir);
    let store = stored(&dir, &[40, 10, 2]);
    let preview = super::super::api::prune(&host, true, false, Some("7d")).unwrap();
    assert_eq!(preview["removedEvents"], 2);
    assert_eq!(preview["olderThanDays"], 7);
    assert_eq!(store.stats().events, 3);
    let longer = super::super::api::prune(&host, true, false, Some("90d")).unwrap();
    assert_eq!(longer["olderThanDays"], 30);
    assert_eq!(longer["removedEvents"], 1);
    let done = super::super::api::prune(&host, false, false, Some("7d")).unwrap();
    assert_eq!(done["removedEvents"], 2);
    assert_eq!(store.stats().events, 1);
    assert_eq!(config::load(&dir_of(&dir)).retention_days, 30);
}

#[test]
fn test52_a_window_that_is_not_a_number_of_days_is_a_usage_error() {
    let dir = TempDir::new().unwrap();
    let host = host_in(&dir);
    for bad in ["0d", "soon", "-3d", ""] {
        let error = super::super::api::prune(&host, true, false, Some(bad)).unwrap_err();
        assert_eq!(error.code(), "usage", "{bad}");
    }
}

#[test]
fn test53_the_defaults_are_thirty_days_and_64_mb_and_prune_applies_both() {
    assert_eq!(
        (
            super::super::DEFAULT_RETENTION_DAYS,
            super::super::DEFAULT_MAX_MB
        ),
        (30, 64)
    );
    let dir = TempDir::new().unwrap();
    let host = host_in(&dir);
    let store = stored(&dir, &[31, 29]);
    let done = super::super::api::prune(&host, false, false, None).unwrap();
    assert_eq!(done["removedEvents"], 1);
    assert_eq!(done["olderThanDays"], 30);
    assert_eq!(done["maxBytes"], 64 * 1024 * 1024);
    assert_eq!(store.stats().events, 1);
    let small = store.stats().bytes / 2;
    let capped = store.prune(NOW, 30, small, true, &|_| None, true).unwrap();
    assert_eq!(capped.removed_events, 1);
}

#[test]
fn test54_with_daily_totals_off_pruned_events_go_without_a_summary() {
    let dir = TempDir::new().unwrap();
    let host = host_in(&dir);
    let store = stored(&dir, &[40, 2]);
    super::super::api::config_set(
        &host,
        &super::super::api::ConfigEdit {
            daily_totals: Some("off"),
            ..config_edit()
        },
    )
    .unwrap();
    let preview = super::super::api::prune(&host, true, false, None).unwrap();
    assert_eq!(preview["dailyTotals"], false);
    assert_eq!(preview["foldedDays"], 0);
    super::super::api::prune(&host, false, false, None).unwrap();
    assert!(store.read_daily().is_empty());
    assert_eq!(store.stats().events, 1);
}

#[test]
fn test55_with_daily_totals_on_the_same_prune_keeps_a_line_for_the_day() {
    let dir = TempDir::new().unwrap();
    let host = host_in(&dir);
    let store = stored(&dir, &[40, 2]);
    let done = super::super::api::prune(&host, false, false, None).unwrap();
    assert_eq!(done["dailyTotals"], true);
    assert_eq!(done["foldedDays"], 1);
    assert_eq!(store.read_daily().len(), 1);
}

#[test]
fn test56_a_config_file_from_before_daily_totals_keeps_every_setting_and_totals_stay_on() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir_of(&dir)).unwrap();
    std::fs::write(
        dir_of(&dir).join(config::CONFIG_FILE),
        r#"{"enabled":true,"port":4999,"retentionDays":14,"maxMb":32,"showCost":false}"#,
    )
    .unwrap();
    let cfg = config::load(&dir_of(&dir));
    assert_eq!(
        (cfg.port, cfg.retention_days, cfg.show_cost),
        (4999, 14, false)
    );
    assert!(cfg.daily_totals);
}

fn folder(dir: &TempDir, name: &str) -> String {
    let path = dir.path().join(name);
    std::fs::create_dir_all(&path).unwrap();
    path.to_string_lossy().into_owned()
}

fn weekly_on<'a>(folder: Option<&'a str>) -> super::super::api::ConfigEdit<'a> {
    super::super::api::ConfigEdit {
        weekly_measure: Some("on"),
        measure_folder: folder,
        ..config_edit()
    }
}

#[test]
fn test57_turning_the_weekly_measure_on_enables_the_shipped_task_for_that_folder() {
    let dir = TempDir::new().unwrap();
    let host = host_in(&dir);
    let work = folder(&dir, "work");
    let before = super::super::weekly::state(&host);
    assert_eq!(before["enabled"], false);
    assert_eq!(before["exists"], false);
    super::super::api::config_set(&host, &weekly_on(Some(&work))).unwrap();
    let after = super::super::weekly::state(&host);
    assert_eq!(after["enabled"], true);
    assert_eq!(after["exists"], true);
    assert_eq!(after["folder"], work);
    assert_eq!(after["model"], "haiku");
    let task =
        std::fs::read_to_string(dir.path().join("plus/tasks/spend-weekly-measure.json")).unwrap();
    assert!(task.contains("\"measure\""), "{task}");
    assert!(task.contains(&work));
}

#[test]
fn test58_a_dry_run_shows_the_plan_and_writes_no_task() {
    let dir = TempDir::new().unwrap();
    let host = host_in(&dir);
    let work = folder(&dir, "work");
    let out = super::super::api::config_edit(&host, &weekly_on(Some(&work)), true).unwrap();
    assert_eq!(out["weeklyMeasure"]["action"], "add");
    assert_eq!(out["weeklyMeasure"]["folder"], work);
    assert_eq!(super::super::weekly::state(&host)["exists"], false);
}

#[test]
fn test59_the_weekly_measure_needs_an_existing_folder_and_says_so() {
    let dir = TempDir::new().unwrap();
    let host = host_in(&dir);
    let missing = dir.path().join("nowhere").to_string_lossy().into_owned();
    let error = super::super::api::config_set(&host, &weekly_on(Some(&missing))).unwrap_err();
    assert_eq!(error.code(), "usage");
    let none = super::super::api::config_set(&host, &weekly_on(None)).unwrap_err();
    assert!(none.message.contains("--measure-folder"));
    assert_eq!(super::super::weekly::state(&host)["exists"], false);
}

#[test]
fn test60_turning_it_off_disables_the_task_and_keeps_its_folder() {
    let dir = TempDir::new().unwrap();
    let host = host_in(&dir);
    let work = folder(&dir, "work");
    super::super::api::config_set(&host, &weekly_on(Some(&work))).unwrap();
    super::super::api::config_set(
        &host,
        &super::super::api::ConfigEdit {
            weekly_measure: Some("off"),
            ..config_edit()
        },
    )
    .unwrap();
    let state = super::super::weekly::state(&host);
    assert_eq!(
        (state["enabled"].clone(), state["folder"].clone()),
        (json!(false), json!(work))
    );
}

#[test]
fn test61_a_caller_that_needs_approval_enables_nothing_until_the_person_says_yes() {
    let dir = TempDir::new().unwrap();
    let host = host_in(&dir);
    let work = folder(&dir, "work");
    let asked = super::super::api::ConfigEdit {
        ask_broker: true,
        ..weekly_on(Some(&work))
    };
    host.tasks.decision.set(RunDecision::Denied);
    assert!(super::super::api::config_set(&host, &asked).is_err());
    assert_eq!(super::super::weekly::state(&host)["enabled"], false);
    assert_eq!(host.tasks.approvals.borrow().len(), 1);
    host.tasks.decision.set(RunDecision::Approved);
    super::super::api::config_set(&host, &asked).unwrap();
    assert_eq!(super::super::weekly::state(&host)["enabled"], true);
}

#[test]
fn test62_the_setup_plan_lists_the_weekly_task_as_an_optional_third_step() {
    let dir = TempDir::new().unwrap();
    let (mut host, _settings) = setup_host(&dir, None);
    host.registry.path = Some(dir.path().join("registry.json"));
    *host.tasks.base.borrow_mut() = Some(dir.path().to_path_buf());
    let work = folder(&dir, "work");
    let plain =
        super::super::api::setup_plan(&host, &super::super::api::SetupChoice::default()).unwrap();
    assert_eq!(plain["weeklyMeasure"]["requested"], false);
    assert_eq!(plain["weeklyMeasure"]["action"], "off");
    let asked = super::super::api::setup_plan(
        &host,
        &super::super::api::SetupChoice {
            weekly_measure: true,
            measure_folder: Some(work.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(asked["weeklyMeasure"]["action"], "add");
    assert_eq!(asked["weeklyMeasure"]["folder"], work);
}

#[test]
fn test63_setup_apply_with_the_weekly_flag_writes_the_keys_and_enables_the_task() {
    let dir = TempDir::new().unwrap();
    let (mut host, settings) = setup_host(&dir, None);
    host.registry.path = Some(dir.path().join("registry.json"));
    *host.tasks.base.borrow_mut() = Some(dir.path().to_path_buf());
    let work = folder(&dir, "work");
    let choice = super::super::api::SetupChoice {
        weekly_measure: true,
        measure_folder: Some(work.clone()),
        ..Default::default()
    };
    let applied = super::super::api::setup_apply(&host, &choice, false).unwrap();
    assert_eq!(applied["weeklyMeasure"]["applied"], true);
    assert_eq!(applied["weeklyMeasure"]["folder"], work);
    assert_eq!(env_of(&settings).as_object().unwrap().len(), 5);
    assert_eq!(super::super::weekly::state(&host)["enabled"], true);
}

#[test]
fn test64_a_weekly_folder_that_does_not_exist_stops_the_setup_before_anything_is_written() {
    let dir = TempDir::new().unwrap();
    let (mut host, settings) = setup_host(&dir, None);
    host.registry.path = Some(dir.path().join("registry.json"));
    *host.tasks.base.borrow_mut() = Some(dir.path().to_path_buf());
    let choice = super::super::api::SetupChoice {
        weekly_measure: true,
        measure_folder: Some(dir.path().join("nowhere").to_string_lossy().into_owned()),
        ..Default::default()
    };
    assert!(super::super::api::setup_apply(&host, &choice, false).is_err());
    assert!(!settings.exists());
}
