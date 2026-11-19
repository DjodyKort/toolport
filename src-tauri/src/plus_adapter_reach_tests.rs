use serde_json::json;
use toolport_plus_api::ScopeKind;

use crate::plus_adapter::view_scope_reach;
use crate::registry::Registry;

fn registry(extra: serde_json::Value) -> Registry {
    let mut value = json!({
        "version": 3,
        "servers": [
            {"id": "a", "name": "a", "transport": "stdio", "command": "x", "enabled": true},
            {"id": "b", "name": "b", "transport": "stdio", "command": "x", "enabled": true},
            {"id": "c", "name": "c", "transport": "stdio", "command": "x", "enabled": false},
        ],
        "profiles": [
            {"id": "work", "name": "Work", "enabledServerIds": ["a", "c"]},
            {"id": "focus", "name": "Focus", "enabledServerIds": ["b"]},
        ],
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}

fn of(
    scopes: &[toolport_plus_api::ScopeReach],
    kind: ScopeKind,
    key: &str,
) -> toolport_plus_api::ScopeReach {
    scopes
        .iter()
        .find(|s| s.kind == kind && s.key == key)
        .unwrap()
        .clone()
}

#[test]
fn without_an_explicit_default_access_every_enabled_server_is_reached() {
    let scopes = view_scope_reach(&registry(json!({})));
    let default = of(&scopes, ScopeKind::Default, "");
    assert_eq!(default.server_ids, ["a", "b"]);
    assert_eq!((default.set_id, default.dangling), (None, false));
}

#[test]
fn an_explicit_default_access_reaches_its_enabled_members_only() {
    let scopes = view_scope_reach(&registry(json!({"defaultAccessProfileId": "work"})));
    let default = of(&scopes, ScopeKind::Default, "");
    assert_eq!(default.server_ids, ["a"], "c is a member but switched off");
    assert_eq!(default.set_id.as_deref(), Some("work"));
}

#[test]
fn a_client_scope_resolves_by_id_or_name_and_an_empty_one_follows_the_default() {
    let scopes = view_scope_reach(&registry(json!({
        "defaultAccessProfileId": "work",
        "clientScopes": {"by-id": "focus", "by-name": "FOCUS", "unscoped": "", "gone": "deleted"},
    })));
    assert_eq!(of(&scopes, ScopeKind::Client, "by-id").server_ids, ["b"]);
    let by_name = of(&scopes, ScopeKind::Client, "by-name");
    assert_eq!(by_name.set_id.as_deref(), Some("focus"));
    let unscoped = of(&scopes, ScopeKind::Client, "unscoped");
    assert_eq!(unscoped.server_ids, ["a"]);
    assert_eq!(unscoped.reference, "");
    let gone = of(&scopes, ScopeKind::Client, "gone");
    assert!(gone.dangling && gone.server_ids.is_empty());
}

#[test]
fn a_folder_mapping_resolves_the_way_the_gateway_resolves_a_root() {
    let reg = registry(json!({
        "folderProfiles": [{"path": "/w/proj", "profile": "Focus"}, {"path": "/w/x", "profile": "nope"}],
    }));
    let scopes = view_scope_reach(&reg);
    let folder = of(&scopes, ScopeKind::Folder, "/w/proj");
    assert_eq!(folder.server_ids, ["b"]);
    assert_eq!(folder.set_id.as_deref(), Some("focus"));
    assert!(of(&scopes, ScopeKind::Folder, "/w/x").dangling);
    assert_eq!(
        reg.profile_for_root("/w/proj/src").as_deref(),
        Some("focus")
    );
}

#[test]
fn a_registry_below_version_three_reaches_what_the_active_profile_lists() {
    let reg: Registry = serde_json::from_value(json!({
        "version": 2,
        "servers": [
            {"id": "a", "name": "a", "transport": "stdio", "command": "x"},
            {"id": "b", "name": "b", "transport": "stdio", "command": "x"},
        ],
        "profiles": [{"id": "p", "name": "P", "enabledServerIds": ["a"]}],
        "activeProfileId": "p",
    }))
    .unwrap();
    let default = of(&view_scope_reach(&reg), ScopeKind::Default, "");
    assert_eq!(default.server_ids, ["a"]);
    assert_eq!(default.set_id.as_deref(), Some("p"));
}

#[test]
fn the_read_only_view_serves_the_same_scopes() {
    let dir = std::env::temp_dir().join(format!("tp-reach-view-{}", std::process::id()));
    let _lock = crate::registry::data_dir_test_lock();
    let _data = crate::registry::DataDirOverride::set(&dir);
    let reg = registry(json!({"defaultAccessProfileId": "work", "clientScopes": {"c1": "focus"}}));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("registry.json"),
        serde_json::to_string(&reg).unwrap(),
    )
    .unwrap();
    let view = toolport_plus_api::RegistryAccess::read_view(&crate::plus_adapter::HostAdapter)
        .unwrap()
        .unwrap();
    let scopes = view.scope_reach();
    assert_eq!(scopes.len(), 2);
    assert_eq!(of(&scopes, ScopeKind::Client, "c1").server_ids, ["b"]);
    let _ = std::fs::remove_dir_all(&dir);
}
