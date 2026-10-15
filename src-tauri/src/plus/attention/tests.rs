use super::*;
use crate::plus::tasks::store::{self, AttentionRecord};
use crate::plus::testutil::DataDirFx;

const NOW: i64 = 1_790_000_000;

fn ctx() -> Ctx {
    Ctx { now: NOW }
}

fn empty(_: &Ctx) -> Vec<Item> {
    Vec::new()
}

fn boom(_: &Ctx) -> Vec<Item> {
    panic!("a feed that breaks")
}

fn rows(ctx: &Ctx) -> Vec<Item> {
    vec![
        Item::new(ctx, "z:fyi", Level::Fyi, "sync", "late", "d").since_epoch(NOW - 10),
        Item::new(ctx, "b:look", Level::Look, "skills", "look b", "d").since_epoch(NOW - 5),
        Item::new(ctx, "a:look", Level::Look, "skills", "look a", "d").since_epoch(NOW - 50),
        Item::new(ctx, "n:need", Level::NeedsYou, "auth", "need", "d").since_epoch(NOW),
    ]
}

fn ids(data: &Value) -> Vec<String> {
    data["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

fn write_auth(fx: &DataDirFx, state: &str) {
    let dir = fx.dir.join("auth");
    std::fs::create_dir_all(&dir).unwrap();
    let status = json!({"version": 1, "servers": {"alpha": {
        "tracked": {"state": state, "reason": "http 401", "since": NOW - 3600, "transient": null},
        "lastProbeAt": NOW - 60, "nextDueAt": NOW + 600}}});
    std::fs::write(dir.join("status.json"), status.to_string()).unwrap();
}

#[test]
fn an_empty_world_lists_nothing() {
    let _fx = DataDirFx::new("attention", "empty");
    let data = ls(&ctx(), &[("none", empty)], None).unwrap();
    assert_eq!(data["counts"], json!({"needsYou": 0, "look": 0, "fyi": 0}));
    assert_eq!(data["items"], json!([]));
}

#[test]
fn items_sort_by_level_then_since_and_the_filter_keeps_the_counts() {
    let _fx = DataDirFx::new("attention", "sort");
    let data = ls(&ctx(), &[("rows", rows)], None).unwrap();
    assert_eq!(ids(&data), ["n:need", "a:look", "b:look", "z:fyi"]);
    assert_eq!(data["counts"], json!({"needsYou": 1, "look": 2, "fyi": 1}));
    let look = ls(&ctx(), &[("rows", rows)], Some(Level::Look)).unwrap();
    assert_eq!(ids(&look), ["a:look", "b:look"]);
    assert_eq!(look["counts"], data["counts"]);
}

#[test]
fn a_failing_feed_contributes_nothing_and_never_fails_the_list() {
    let _fx = DataDirFx::new("attention", "boom");
    let data = ls(&ctx(), &[("boom", boom), ("rows", rows)], None).unwrap();
    assert_eq!(data["items"].as_array().unwrap().len(), 4);
}

#[test]
fn a_dismissal_hides_a_row_until_its_date_and_the_row_returns_on_it() {
    let fx = DataDirFx::new("attention", "dismiss");
    let day = |n: i64| NOW + n * 86400;
    let date = |epoch| cron::rfc3339(epoch)[..10].to_string();
    let until = date(day(3));
    let preview = dismissals::dismiss("n:need", Some(&until), NOW, true).unwrap();
    assert_eq!(preview["dryRun"], true);
    assert!(!fx.dir.join("plus/attention.json").exists());
    let done = dismissals::dismiss("n:need", Some(&until), NOW, false).unwrap();
    assert_eq!(done["result"]["applied"], true);
    assert!(done["result"]["undo"].as_str().unwrap().starts_with("toolportctl attention dismiss n:need --until "));
    let hidden = ls(&Ctx { now: day(2) }, &[("rows", rows)], None).unwrap();
    assert!(!ids(&hidden).contains(&"n:need".to_string()));
    assert_eq!(hidden["counts"]["needsYou"], 0);
    let back = ls(&Ctx { now: day(3) }, &[("rows", rows)], None).unwrap();
    assert!(ids(&back).contains(&"n:need".to_string()));
    dismissals::dismiss("gone:later", None, NOW, false).unwrap();
    assert!(dismissals::load(&date(day(400))).contains_key("gone:later"));
}

#[test]
fn an_undo_date_that_has_passed_removes_the_entry_and_bad_input_is_a_usage_error() {
    let _fx = DataDirFx::new("attention", "undo");
    dismissals::dismiss("n:need", None, NOW, false).unwrap();
    let today = ctx().today();
    dismissals::dismiss("n:need", Some(&today), NOW, false).unwrap();
    assert!(dismissals::load(&today).is_empty());
    for bad in ["2026-02-30", "next week", "2026-1-1", ""] {
        let err = dismissals::dismiss("n:need", Some(bad), NOW, true).unwrap_err();
        assert_eq!(err.kind, crate::plus::op::ErrorKind::Usage, "{bad}");
    }
    assert!(dismissals::dismiss("", None, NOW, true).is_err());
}

#[test]
fn the_login_feed_reads_the_stored_probe_health_and_needs_you() {
    let fx = DataDirFx::new("attention", "auth");
    write_auth(&fx, "needs_reauth");
    let items = feed_auth::collect(&ctx());
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!((item.id.as_str(), item.level, item.from), ("auth:alpha", Level::NeedsYou, "auth"));
    assert_eq!(item.target.route, "servers");
    assert_eq!(item.target.params["tab"], "logins");
    assert_eq!(item.action.as_ref().unwrap().command, ["toolportctl", "auth", "probe", "--server", "alpha", "--force"]);
    write_auth(&fx, "ok");
    assert!(feed_auth::collect(&ctx()).is_empty());
}

fn write_registry_of_alpha(fx: &DataDirFx, enabled: &[&str]) {
    let registry = json!({
        "version": 1,
        "servers": [{"id": "alpha", "name": "alpha", "transport": "http", "url": "https://mcp.example.test/mcp"}],
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": enabled}],
        "activeProfileId": "default",
        "plus": {"directEntries": {"claude-code": {"alpha": {
            "server": "alpha", "command": "", "args": [], "env": {},
            "url": "https://mcp.example.test/mcp"}}}},
    });
    std::fs::write(fx.dir.join("registry.json"), registry.to_string()).unwrap();
}

#[test]
fn the_login_feed_skips_a_server_only_a_client_signs_in_to() {
    let fx = DataDirFx::new("attention", "auth-direct");
    write_auth(&fx, "needs_reauth");
    write_registry_of_alpha(&fx, &[]);
    assert!(
        feed_auth::collect(&ctx()).is_empty(),
        "a stale row of a direct-only server must not raise anything"
    );
    write_registry_of_alpha(&fx, &["alpha"]);
    assert_eq!(feed_auth::collect(&ctx()).len(), 1, "the gateway reaches it again");
}

#[test]
fn the_status_and_the_login_rows_leave_out_a_server_only_a_client_signs_in_to() {
    let fx = DataDirFx::new("attention", "auth-direct-status");
    write_auth(&fx, "needs_reauth");
    write_registry_of_alpha(&fx, &[]);
    let rows = crate::plus::dispatch("plus.auth.rows", json!({})).unwrap();
    assert_eq!(rows["rows"], json!([]), "{rows}");
    let summary = super::super::status::auth_summary();
    assert_eq!(summary["servers"], json!([]), "{summary}");
    write_registry_of_alpha(&fx, &["alpha"]);
    let rows = crate::plus::dispatch("plus.auth.rows", json!({})).unwrap();
    assert_eq!(rows["rows"][0]["server"], "alpha");
}

#[test]
fn a_login_of_a_server_no_profile_enables_is_not_reported() {
    use crate::registry::{registry_path, Registry};
    let fx = DataDirFx::new("attention", "auth-unused");
    write_auth(&fx, "needs_reauth");
    let mut reg = Registry::default();
    reg.servers = vec![serde_json::from_value(
        json!({"id": "alpha", "name": "alpha", "transport": "http", "url": "https://a.example/mcp"}),
    )
    .unwrap()];
    let path = registry_path().unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let save = |reg: &Registry| std::fs::write(&path, serde_json::to_string(reg).unwrap()).unwrap();
    save(&reg);
    assert!(feed_auth::collect(&ctx()).is_empty());
    reg.profiles[0].enabled_server_ids = vec!["alpha".into()];
    save(&reg);
    assert_eq!(feed_auth::collect(&ctx()).len(), 1);
}

#[test]
fn the_task_feed_reads_the_attention_records() {
    let _fx = DataDirFx::new("attention", "tasks");
    store::raise_attention(AttentionRecord {
        id: "tasks:refresh:auth:alpha".into(),
        level: "needs-you".into(),
        title: "alpha login failed: run task refresh?".into(),
        detail: "the task has no autoRun schedule".into(),
        from: "tasks".into(),
        task: "refresh".into(),
        since: cron::rfc3339(NOW - 100),
        action: Some(json!({"label": "Run task", "command": ["toolportctl", "task", "run", "refresh"]})),
    })
    .unwrap();
    let items = feed_tasks::collect(&ctx());
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].target.route, "tasks");
    assert_eq!(items[0].since, cron::rfc3339(NOW - 100));
    assert_eq!(items[0].action.as_ref().unwrap().label, "Run task");
}

fn left_out_report(servers: &[(&str, &str)]) -> crate::plus::reach::Report {
    use crate::plus::reach::{ClientRow, Finding, Report, ServerReach};
    Report {
        clients: vec![ClientRow {
            id: "claude-code".into(),
            name: "Claude Code".into(),
            gateway: "managed",
            profile: Some("claude-code".into()),
        }],
        servers: servers
            .iter()
            .map(|(id, name)| ServerReach {
                id: id.to_string(),
                name: name.to_string(),
                transport: "http".into(),
                profiles: vec!["Default".into()],
                routes: Vec::new(),
            })
            .collect(),
        unregistered: Vec::new(),
        findings: servers
            .iter()
            .map(|(id, _)| Finding {
                level: "look",
                kind: "left-out-by-profile",
                server: Some(id.to_string()),
                client: Some("claude-code".into()),
                text: String::new(),
            })
            .collect(),
    }
}

#[test]
fn servers_a_clients_profile_leaves_out_are_one_row_for_that_client() {
    let items = feed_profiles::rows(&ctx(), &left_out_report(&[("slack", "Slack"), ("biblio", "biblio")]));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].level, Level::Look);
    assert_eq!(items[0].title, "Claude Code does not get 2 servers that are on in the active profile");
    assert!(items[0].detail.starts_with("The profile claude-code leaves out biblio, Slack."), "{}", items[0].detail);
    assert!(items[0].id.starts_with("profiles:claude-code:left-out:"), "{}", items[0].id);
}

#[test]
fn a_row_comes_back_under_a_new_id_when_another_server_is_left_out() {
    let one = feed_profiles::rows(&ctx(), &left_out_report(&[("slack", "Slack")]));
    let two = feed_profiles::rows(&ctx(), &left_out_report(&[("slack", "Slack"), ("biblio", "biblio")]));
    let again = feed_profiles::rows(&ctx(), &left_out_report(&[("slack", "Slack")]));
    assert_ne!(one[0].id, two[0].id);
    assert_eq!(one[0].id, again[0].id);
}

#[test]
fn a_report_without_left_out_servers_raises_nothing() {
    assert!(feed_profiles::rows(&ctx(), &left_out_report(&[])).is_empty());
}
