use crate::plus::dispatch;
use crate::registry::{self, FolderProfile, Registry};
use serde_json::{json, Value};

struct Scratch {
    dir: std::path::PathBuf,
    _guard: registry::DataDirOverride,
    _lock: std::sync::MutexGuard<'static, ()>,
}

fn scratch(label: &str) -> Scratch {
    let lock = registry::data_dir_test_lock();
    let dir = std::env::temp_dir().join(format!("folders-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let guard = registry::DataDirOverride::set(dir.join("data"));
    let mut reg = Registry::default();
    let work = reg.add_profile("Work");
    reg.folder_profiles = vec![
        FolderProfile {
            path: "/proj/work".into(),
            profile: work,
        },
    ];
    registry::save(&reg).unwrap();
    Scratch {
        dir,
        _guard: guard,
        _lock: lock,
    }
}

fn query(s: &Scratch, roots: Value) -> Value {
    dispatch(
        "plus.context.folderProfiles",
        json!({"home": s.dir.join("home").display().to_string(), "roots": roots}),
    )
    .unwrap()
}

#[test]
fn disabled_by_default_and_gateway_ignores_mappings() {
    let s = scratch("off");
    let reg = registry::load().unwrap();
    assert!(!reg.folder_profiles_enabled);
    assert_eq!(reg.profile_for_root("/proj/work/app"), None);
    let out = query(&s, json!(["/proj/work/app"]));
    assert_eq!(out["enabled"], false);
    assert_eq!(out["folders"][0]["applies"], false);
    assert_eq!(out["folders"][0]["wouldApply"], "Work");
    assert!(out["folders"][0]["reason"].as_str().unwrap().contains("disabled"));
}

#[test]
fn enabled_routes_longest_rule_and_reports_cost() {
    let s = scratch("on");
    dispatch("plus.context.folderProfilesSet", json!({"enabled": true})).unwrap();
    let reg = registry::load().unwrap();
    assert!(reg.folder_profiles_enabled);
    assert!(reg.profile_for_root("/proj/work/app").is_some());
    assert_eq!(reg.profile_for_root("/elsewhere"), None);
    let out = query(&s, json!(["/proj/work/app", "/elsewhere"]));
    assert_eq!(out["enabled"], true);
    let hit = &out["folders"][0];
    assert_eq!(hit["applies"], true);
    assert_eq!(hit["profile"], "Work");
    assert_eq!(hit["rule"], "/proj/work");
    assert!(hit["tokens"].is_u64());
    let miss = &out["folders"][1];
    assert_eq!(miss["applies"], false);
    assert!(miss["profile"].is_null());
    dispatch("plus.context.folderProfilesSet", json!({"enabled": false})).unwrap();
    assert_eq!(registry::load().unwrap().profile_for_root("/proj/work/app"), None);
}

#[test]
fn set_requires_a_bool() {
    let _s = scratch("bad");
    assert!(dispatch("plus.context.folderProfilesSet", json!({})).is_err());
}
