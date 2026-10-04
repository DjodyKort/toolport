use super::*;
use crate::plus::testutil::DataDirFx;
use serde_json::json;
use std::cell::Cell;

fn fx(tag: &str, registry: serde_json::Value) -> DataDirFx {
    let fx = DataDirFx::new("profiles-core", tag).with_secret_key(&"ab".repeat(32));
    fx.write_registry(&registry);
    fx
}

fn sample() -> serde_json::Value {
    json!({
        "version": 1,
        "servers": [
            {"id": "alpha", "name": "alpha", "transport": "stdio", "command": "alpha-mcp", "args": ["--x"]},
            {"id": "beta", "name": "Beta", "transport": "http", "url": "https://example.invalid/mcp"},
            {"id": "gamma", "name": "gamma", "transport": "stdio"}
        ],
        "profiles": [
            {"id": "work", "name": "Work", "enabledServerIds": ["alpha", "gone"]},
            {"id": "play", "name": "play", "enabledServerIds": []}
        ],
        "activeProfileId": "work"
    })
}

fn bytes(fx: &DataDirFx) -> Vec<u8> {
    std::fs::read(fx.dir.join("registry.json")).unwrap()
}

fn error_of<T>(result: Result<T, Error>) -> Error {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(error) => error,
    }
}

#[test]
fn resolve_takes_an_id_first_then_a_unique_name_in_any_case() {
    let mut reg: Registry = serde_json::from_value(sample()).unwrap();
    assert_eq!(resolve(&reg, "work").unwrap().id, "work");
    assert_eq!(resolve(&reg, " WORK ").unwrap().id, "work");
    assert_eq!(resolve(&reg, "Play").unwrap().id, "play");
    assert_eq!(error_of(resolve(&reg, "nope")).kind, Kind::NotFound);
    reg.profiles[1].name = "work".to_string();
    assert_eq!(resolve(&reg, "work").unwrap().id, "work");
    let ambiguous = error_of(resolve(&reg, "WORK"));
    assert_eq!(ambiguous.kind, Kind::Conflict);
    assert!(ambiguous.message.contains("use its id"));
}

#[test]
fn mutate_plans_on_a_copy_and_writes_only_a_real_change() {
    let fx = fx("mutate", sample());
    let before = bytes(&fx);
    let calls = Cell::new(0);
    let (reg, planned) = mutate(
        true,
        |reg| {
            calls.set(calls.get() + 1);
            reg.profiles.clear();
            Ok(1)
        },
        |_| true,
    )
    .unwrap();
    assert_eq!((planned, calls.get(), reg.profiles.len()), (1, 1, 0));
    assert_eq!(bytes(&fx), before, "a dry run writes nothing");

    calls.set(0);
    mutate(
        false,
        |_| {
            calls.set(calls.get() + 1);
            Ok(())
        },
        |_| false,
    )
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(bytes(&fx), before, "a no-op writes nothing");

    calls.set(0);
    mutate(
        false,
        |reg| {
            calls.set(calls.get() + 1);
            reg.profiles.retain(|p| p.id != "play");
            Ok(())
        },
        |_| true,
    )
    .unwrap();
    assert_eq!(calls.get(), 2, "planned once, applied once under the lock");
    assert_ne!(bytes(&fx), before);
}

#[test]
fn mutate_carries_a_typed_rejection_from_the_locked_pass_and_writes_nothing() {
    let fx = fx("reject", sample());
    let before = bytes(&fx);
    let calls = Cell::new(0);
    let result: Result<(Registry, ()), Error> = mutate(
        false,
        |reg| {
            calls.set(calls.get() + 1);
            if calls.get() > 1 {
                return Err(Error::conflict("changed under us"));
            }
            reg.profiles.clear();
            Ok(())
        },
        |_| true,
    );
    let error = result.unwrap_err();
    assert_eq!(
        (error.kind, error.message.as_str()),
        (Kind::Conflict, "changed under us")
    );
    assert_eq!(bytes(&fx), before);
}

#[test]
fn rows_list_members_that_exist_and_hide_dangling_ids() {
    let reg: Registry = serde_json::from_value(sample()).unwrap();
    let rows = rows(&reg);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].active && !rows[1].active);
    let names: Vec<&str> = rows[0].servers.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["alpha"]);
    assert_eq!(rows[0].servers[0].target, "alpha-mcp --x");
    assert_eq!(list_value(&rows)["activeProfile"], "work");
    let mut with_beta = reg.clone();
    with_beta.profiles[1].enabled_server_ids = vec!["beta".into(), "gamma".into()];
    let rows = self::rows(&with_beta);
    assert_eq!(rows[1].servers[0].target, "https://example.invalid/mcp");
    assert_eq!(rows[1].servers[1].target, "Custom");
}

#[test]
fn create_conflicts_on_an_existing_name_and_force_is_a_no_op() {
    let fx = fx("create", sample());
    let before = bytes(&fx);
    assert_eq!(error_of(create("WORK", false, false)).kind, Kind::Conflict);
    assert_eq!(error_of(create(" ", false, false)).kind, Kind::Invalid);
    let again = create("WORK", true, false).unwrap();
    assert_eq!((again.id.as_str(), again.created), ("work", false));
    assert_eq!(bytes(&fx), before);
    let planned = create("fresh", false, true).unwrap();
    assert!(planned.created && planned.dry_run);
    assert_eq!(bytes(&fx), before);
    let made = create("fresh", false, false).unwrap();
    assert!(made.created);
    assert!(list().unwrap().iter().any(|row| row.id == made.id));
}

#[test]
fn edit_set_drops_ids_of_servers_that_no_longer_exist() {
    let fx = fx("set", sample());
    let edited = edit(
        "work",
        &EditSpec {
            name: None,
            servers: Some(ServerOp::Set(vec!["beta".into(), "GAMMA".into()])),
        },
        false,
    )
    .unwrap();
    assert_eq!(edited.added(), ["Beta", "gamma"]);
    assert_eq!(edited.removed(), ["alpha"]);
    let saved: serde_json::Value = serde_json::from_slice(&bytes(&fx)).unwrap();
    assert_eq!(
        saved["profiles"][0]["enabledServerIds"],
        json!(["beta", "gamma"])
    );
}

#[test]
fn edit_resolves_servers_by_id_or_name_and_lists_the_choices_on_a_miss() {
    let fx = fx("servers", sample());
    let before = bytes(&fx);
    let spec = |servers| EditSpec {
        name: Some("zzz".into()),
        servers: Some(servers),
    };
    let missing = error_of(edit(
        "work",
        &spec(ServerOp::Add(vec!["beta".into(), "ghost".into()])),
        false,
    ));
    assert_eq!(missing.kind, Kind::NotFound);
    assert_eq!(
        missing.message,
        "Server(s) not found: ghost\n\nAvailable servers:\n  • alpha\n  • Beta\n  • gamma"
    );
    assert_eq!(
        bytes(&fx),
        before,
        "the rename in the same call is not applied either"
    );
    let kept = edit(
        "work",
        &spec(ServerOp::Remove(vec!["gamma".into(), "alpha".into()])),
        true,
    )
    .unwrap();
    assert_eq!(kept.not_in_profile, ["gamma"]);
    assert_eq!(kept.removed(), ["alpha"]);
    assert!(kept.renamed() && kept.dry_run);
    assert_eq!(bytes(&fx), before);
}

#[test]
fn remove_refuses_the_last_profile_and_reports_scoped_clients() {
    let mut registry = sample();
    registry["clientScopes"] = json!({"cursor": "work", "zed": "play", "ghost-scope": ""});
    let fx = fx("remove", registry);
    let before = bytes(&fx);
    let planned = remove("Work", true, true).unwrap();
    assert_eq!(
        (planned.servers, planned.left.as_slice()),
        (1, ["cursor".to_string()].as_slice())
    );
    assert!(planned.cleanups.is_empty());
    assert_eq!(bytes(&fx), before);
    assert_eq!(error_of(remove("ghost", true, false)).kind, Kind::NotFound);
    remove("play", true, false).unwrap();
    let last = error_of(remove("work", true, false));
    assert_eq!(last.kind, Kind::Conflict);
    assert!(last.message.contains("last profile"), "{}", last.message);
}

#[test]
fn set_member_creates_a_profile_only_when_asked_and_skips_no_ops() {
    let fx = fx("member", sample());
    let before = bytes(&fx);
    let missing = error_of(set_member("fresh", "alpha", true, false));
    assert_eq!(missing.kind, Kind::NotFound);
    assert_eq!(bytes(&fx), before);
    set_member("work", "alpha", true, true).unwrap();
    assert_eq!(bytes(&fx), before, "already a member");
    let reg = set_member("fresh", "alpha", true, true).unwrap();
    assert_eq!(tags_of(&reg, "alpha"), ["Work", "fresh"]);
    let reg = set_member("work", "alpha", false, false).unwrap();
    assert_eq!(tags_of(&reg, "alpha"), ["fresh"]);
    assert_eq!(
        error_of(set_member(" ", "alpha", true, true)).kind,
        Kind::Invalid
    );
}
