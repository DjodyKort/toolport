use super::*;
use crate::plus::context::load_config;
use std::path::PathBuf;

#[path = "../../../tests/common/sources_world.rs"]
mod world;
use world::SourcesWorld;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Fx {
    w: SourcesWorld,
    roots: Roots,
    config: ContextConfig,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "sources-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let w = world::build(&base);
        let roots = Roots::from_home(&w.home);
        let config = load_config(&roots.context_config_path());
        Self { w, roots, config }
    }

    fn scan(&self, opts: &ScanOptions) -> ScanReport {
        scan(&self.roots, &self.config, Some(&self.w.data), opts)
    }

    fn all(&self) -> ScanReport {
        self.scan(&ScanOptions {
            items: true,
            ..ScanOptions::default()
        })
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        make_writable(&self.w.base);
        let _ = std::fs::remove_dir_all(&self.w.base);
    }
}

fn make_writable(dir: &std::path::Path) {
    let _ = std::process::Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(dir)
        .status();
}

fn source<'a>(report: &'a ScanReport, id: &str) -> &'a Source {
    report
        .sources
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| {
            panic!(
                "no source {id}: {:?}",
                report.sources.iter().map(|s| &s.id).collect::<Vec<_>>()
            )
        })
}

fn items_of<'a>(report: &'a ScanReport, id: &str) -> Vec<&'a Item> {
    report.items.iter().filter(|i| i.source_id == id).collect()
}

fn item<'a>(report: &'a ScanReport, id: &str, kind: &str, name: &str) -> &'a Item {
    items_of(report, id)
        .into_iter()
        .find(|i| i.kind == kind && i.name == name)
        .unwrap_or_else(|| panic!("no {kind} {name} in {id}"))
}

#[test]
fn the_fixture_home_yields_every_detector_kind_in_result_order() {
    let fx = Fx::new("all");
    let report = fx.all();
    assert!(!report.partial, "{:?}", report.skipped);
    let ids: Vec<&str> = report.sources.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "repo:odh",
            "client:acme",
            "vendored:repos/core/odoo-20.0/skills",
            "vendored:repos/vve-stubs/skills",
            "vendored:vendor/ext-skills/skills",
            "plugin:alpha@market",
            "plugin:beta@market",
            "org",
            "account",
            "library",
            "tap:acme-tools",
            "loose",
            "inert",
        ]
    );
    let detectors: std::collections::BTreeSet<&str> =
        report.sources.iter().map(|s| s.detector).collect();
    assert_eq!(detectors.len(), 10);
}

#[test]
fn a_checkout_behind_its_remote_still_shows_what_origin_has() {
    let fx = Fx::new("odh");
    let report = fx.all();
    let odh = source(&report, "repo:odh");
    let fresh = odh.freshness.as_ref().unwrap();
    assert_eq!(
        (
            fresh.reference.as_str(),
            fresh.behind,
            fresh.ahead,
            fresh.in_checkout
        ),
        ("origin/main", world::ODH_BEHIND, 0, false)
    );
    assert!(fresh.last_sync.is_some());
    assert_eq!(odh.status.state, "behind");
    assert_eq!(odh.counts.skill, 3);
    let remote_only = item(&report, "repo:odh", "skill", "odh");
    assert!(!remote_only.in_checkout);
    assert_eq!(remote_only.path, "origin/main:.claude/skills/odh/SKILL.md");
    let local = item(&report, "repo:odh", "skill", "v18-local");
    assert!(
        local.in_checkout,
        "an untracked skill in the checkout is found"
    );
    assert!(local
        .path
        .ends_with("work/odh/.claude/skills/v18-local/SKILL.md"));
    let memory = item(&report, "repo:odh", "memory", "CLAUDE.md");
    assert!(memory.in_checkout && !memory.lazy);
    assert!(odh
        .warnings
        .iter()
        .any(|w| w.contains("only on the remote branch")));
}

#[test]
fn client_repos_are_audited_and_a_repo_without_content_is_not_a_source() {
    let fx = Fx::new("client");
    let report = fx.all();
    assert!(report.sources.iter().all(|s| s.id != "client:empty"));
    let acme = source(&report, "client:acme");
    assert_eq!(acme.owner, "project");
    assert!(!acme.writable);
    assert!(acme.freshness.is_none());
    assert_eq!(item(&report, "client:acme", "skill", "risky").audit, "high");
    assert_eq!(
        item(&report, "client:acme", "skill", "acme-skill").audit,
        "clean"
    );
    assert!(acme.warnings.iter().any(|w| w.contains("high-severity")));
    assert_eq!(acme.counts.command, 1);
    assert_eq!(item(&report, "repo:odh", "skill", "odh").audit, "clean");
}

#[test]
fn a_linked_worktree_and_a_duplicate_clone_are_never_repo_sources() {
    let fx = Fx::new("worktree");
    assert!(fx.w.home.join("dups/odh-wt/.git").is_file());
    let report = fx.all();
    for id in ["repo:odh-wt", "repo:ai-skills-copy"] {
        assert!(report.sources.iter().all(|s| s.id != id), "{id}");
    }
}

#[test]
fn vendored_folders_come_from_repos_and_gitmodules() {
    let fx = Fx::new("vendored");
    let report = fx.all();
    let api = source(&report, "vendored:repos/core/odoo-20.0/skills");
    assert_eq!(
        (api.counts.skill, api.owner, api.writable),
        (2, "third-party", false)
    );
    assert_eq!(
        source(&report, "vendored:vendor/ext-skills/skills")
            .counts
            .skill,
        1
    );
    assert_eq!(
        item(
            &report,
            "vendored:repos/vve-stubs/skills",
            "skill",
            "stub-gen"
        )
        .audit,
        "unchecked"
    );
}

#[test]
fn plugin_enabled_state_follows_user_settings_and_project_local_overrides() {
    let fx = Fx::new("plugin");
    let report = fx.all();
    let alpha = source(&report, "plugin:alpha@market");
    let beta = source(&report, "plugin:beta@market");
    assert_eq!((alpha.enabled, beta.enabled), (Some(true), Some(false)));
    assert_eq!(alpha.managed_by.as_deref(), Some("marketplace"));
    assert_eq!(
        (alpha.counts.skill, alpha.counts.command, alpha.counts.agent),
        (1, 1, 1)
    );
    assert!(
        alpha.tokens.value > 0 && beta.tokens.value > 0,
        "tokens count even when off"
    );

    let project = fx.w.base.join("project");
    world::put(
        &project.join(".claude/settings.local.json"),
        "{\"enabledPlugins\":{\"beta@market\":true,\"alpha@market\":false}}",
    );
    let with_cwd = fx.scan(&ScanOptions {
        cwd: Some(project),
        ..ScanOptions::default()
    });
    assert_eq!(
        source(&with_cwd, "plugin:alpha@market").enabled,
        Some(false)
    );
    assert_eq!(source(&with_cwd, "plugin:beta@market").enabled, Some(true));
}

#[test]
fn org_compares_hashes_names_its_sync_and_never_claims_a_user_file() {
    let fx = Fx::new("org");
    let report = fx.all();
    let org = source(&report, "org");
    assert_eq!((org.owner, org.writable), ("org", false));
    assert_eq!(org.origin.name, "corp-tools");
    assert_eq!(
        org.managed_by.as_deref(),
        Some("corp-tools sync (about every 4 h)")
    );
    assert_eq!(org.status.state, "stale");
    assert!(org
        .warnings
        .iter()
        .any(|w| w.contains("command ship differs")));
    assert!(org
        .warnings
        .iter()
        .any(|w| w.contains("shell-wrapper.sh changed")));
    let fresh = org.freshness.as_ref().unwrap();
    assert_eq!((fresh.behind, fresh.in_checkout), (1, true));
    assert_eq!(
        fresh.last_sync.as_deref(),
        Some(super::fsx::zulu(world::SYNC_STAMP as i64).as_str())
    );
    assert_eq!(org.counts.memory, 1);
    assert_eq!(org.counts.command, 2);
    assert!(items_of(&report, "org")
        .iter()
        .all(|i| !i.writable && i.origin.kind == "org"));
    assert!(org
        .status
        .detail
        .contains("clone command(s) are not deployed"));
}

#[test]
fn an_org_command_with_a_library_skill_name_shadows_it_and_both_files_are_left_alone() {
    let fx = Fx::new("shadow");
    let report = fx.all();
    let skill = item(&report, "library", "skill", "odoo-upgrade");
    assert_eq!(
        skill.shadowed_by.as_deref(),
        Some("org:command:odoo-upgrade")
    );
    assert!(item(&report, "library", "skill", "review")
        .shadowed_by
        .is_none());
    assert!(source(&report, "library")
        .warnings
        .iter()
        .any(|w| w.contains("odoo-upgrade is shadowed")));
    assert!(source(&report, "org")
        .warnings
        .iter()
        .any(|w| w.contains("odoo-upgrade shadows")));
}

#[test]
fn the_library_flags_a_duplicate_clone_unpushed_commits_and_hidden_skills() {
    let fx = Fx::new("library");
    let report = fx.all();
    let lib = source(&report, "library");
    assert_eq!(lib.status.state, "duplicate");
    assert!(lib
        .warnings
        .iter()
        .any(|w| w.contains("another clone of the same remote")));
    assert!(lib
        .warnings
        .iter()
        .any(|w| w.contains("1 commit(s) not pushed")));
    assert_eq!(lib.freshness.as_ref().unwrap().ahead, 1);
    assert_eq!((lib.owner, lib.writable), ("me", true));
    assert_eq!(
        (lib.counts.skill, lib.counts.rule, lib.counts.agent),
        (4, 1, 1)
    );
    let hidden = item(&report, "library", "skill", "old-style");
    assert_eq!(hidden.visible, Some(false));
    assert!(hidden
        .invisible_reason
        .as_deref()
        .unwrap()
        .contains("deployed copy is rejected"));
    assert_eq!(
        item(&report, "library", "skill", "review").visible,
        Some(true)
    );
    assert_eq!(lib.visible.skill_total, 4);
    assert_eq!(lib.visible.skill, 3);
    assert_eq!(item(&report, "library", "rule", "style").lazy, false);
}

#[test]
fn loose_files_exclude_org_library_managed_and_retired_ones() {
    let fx = Fx::new("loose");
    let report = fx.all();
    let loose = source(&report, "loose");
    let names: Vec<(&str, &str)> = items_of(&report, "loose")
        .iter()
        .map(|i| (i.kind, i.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("skill", "hoot-test"),
            ("command", "moodle-token"),
            ("agent", "scratch")
        ]
    );
    assert!(loose.status.detail.contains("1 managed by toolportctl"));
    assert!(loose.status.detail.contains("1 retired backup"));
    assert!(loose.writable && loose.owner == "me");
}

#[test]
fn account_inert_and_tap_sources_read_their_inputs() {
    let fx = Fx::new("rest");
    let report = fx.all();
    let account = source(&report, "account");
    assert_eq!((account.counts.skill, account.owner), (2, "anthropic"));
    assert_eq!(
        item(&report, "account", "skill", "docx")
            .path
            .ends_with("manifest.json"),
        true
    );
    let inert = source(&report, "inert");
    assert_eq!(inert.counts.memory, 1);
    assert!(items_of(&report, "inert")[0]
        .path
        .ends_with("clients/acme/_claude/CLAUDE.md"));
    let tap = source(&report, "tap:acme-tools");
    assert_eq!((tap.counts.skill, tap.owner), (1, "third-party"));
}

#[test]
fn a_selector_names_a_source_a_detector_or_a_kind_of_origin() {
    let fx = Fx::new("select");
    let by = |s: &str| {
        fx.scan(&ScanOptions {
            source: Some(s.into()),
            items: true,
            ..ScanOptions::default()
        })
    };
    assert_eq!(by("library").sources.len(), 1);
    assert_eq!(by("plugin").sources.len(), 2);
    assert_eq!(by("plugin:beta@market").sources.len(), 1);
    assert!(by("plugin:beta@market")
        .items
        .iter()
        .all(|i| i.source_id == "plugin:beta@market"));
    assert!(by("nothing").sources.is_empty());
    let kinds = fx.scan(&ScanOptions {
        kind: Some("agent".into()),
        items: true,
        ..ScanOptions::default()
    });
    assert!(kinds.items.iter().all(|i| i.kind == "agent"));
    assert!(kinds.sources.iter().all(|s| s.counts.agent > 0));
}

#[test]
fn a_detector_budget_forced_to_zero_is_skipped_and_the_scan_is_partial() {
    let fx = Fx::new("zero");
    let report = fx.scan(&ScanOptions {
        budgets_ms: vec![("repo".into(), 0)],
        ..ScanOptions::default()
    });
    assert!(report.partial);
    assert_eq!(
        report.skipped,
        [Skipped {
            detector: "repo".into(),
            reason: "time budget exhausted".into()
        }]
    );
    assert!(report.sources.iter().all(|s| s.id != "repo:odh"));
    assert!(report.sources.iter().any(|s| s.id == "library"));
}

#[test]
fn a_partial_scan_names_the_detector_and_the_reason_in_json_and_text() {
    let fx = Fx::new("reasons");
    let report = fx.scan(&ScanOptions {
        budgets_ms: vec![("repo".into(), 0)],
        ..ScanOptions::default()
    });
    let json = report.to_json(false);
    assert_eq!(json["partial"], true);
    assert_eq!(
        json["skipped"],
        serde_json::json!([{ "detector": "repo", "reason": "time budget exhausted" }])
    );
    assert!(report
        .to_text()
        .contains("partial: repo stopped: time budget exhausted\n"));
}

#[test]
fn an_entry_cap_stops_a_walk_marks_its_sources_partial_and_reports_why() {
    let fx = Fx::new("cap");
    let busy = fx.w.odh.join("clients/acme/_claude/notes");
    for i in 0..60 {
        world::put(&busy.join(format!("n{i}.txt")), "x");
    }
    let report = fx.scan(&ScanOptions {
        max_entries: Some(5),
        ..ScanOptions::default()
    });
    assert!(report.partial);
    assert!(
        report
            .skipped
            .iter()
            .all(|s| s.reason.contains("entry budget exhausted")),
        "{:?}",
        report.skipped
    );
    assert!(report
        .sources
        .iter()
        .any(|s| s.status.state == "partial" && s.status.detail.contains("scan stopped early")));
}

#[test]
fn deep_doubles_the_depth_budget() {
    let fx = Fx::new("deep");
    let nested = fx.w.odh.join("a/b/c/d/_claude");
    world::put(&nested.join("CLAUDE.md"), "# Deep\n");
    let shallow = fx.all();
    assert!(!items_of(&shallow, "inert")
        .iter()
        .any(|i| i.path.contains("/a/b/c/d/")));
    let deep = fx.scan(&ScanOptions {
        deep: true,
        items: true,
        ..ScanOptions::default()
    });
    assert!(items_of(&deep, "inert")
        .iter()
        .any(|i| i.path.contains("/a/b/c/d/")));
}

#[test]
fn a_second_scan_reuses_the_cache_and_a_touched_file_is_read_again() {
    let fx = Fx::new("cache");
    let first = fx.all();
    assert!(first.cache_misses > 0);
    assert!(super::cache::path_in(&fx.w.data).is_file());
    let second = fx.all();
    assert_eq!(
        second.cache_misses, 0,
        "nothing changed, everything is a hit"
    );
    assert!(second.cache_hits >= first.cache_misses);
    let skill = fx.w.home.join(".claude/skills/hoot-test/SKILL.md");
    std::fs::write(
        &skill,
        "---\nname: hoot-test\ndescription: Changed words here\n---\nBody\n",
    )
    .unwrap();
    let third = fx.all();
    assert_eq!(third.cache_misses, 1);
    assert_eq!(
        item(&third, "loose", "skill", "hoot-test").description,
        "Changed words here"
    );
    let refreshed = fx.scan(&ScanOptions {
        refresh: true,
        ..ScanOptions::default()
    });
    assert_eq!(refreshed.cache_hits, 0);
    assert_eq!(first.sources.len(), refreshed.sources.len());
}

#[test]
fn a_new_commit_on_the_remote_is_seen_because_the_tree_is_keyed_by_commit() {
    let fx = Fx::new("commit");
    let first = fx.all();
    assert_eq!(
        item(&first, "repo:odh", "skill", "odh").description,
        "Work on the odh area"
    );
    let work = fx.w.base.join("odh-push");
    world::git(
        &fx.w.base,
        &[
            "clone",
            "-q",
            fx.w.base.join("remotes/odh.git").to_str().unwrap(),
            work.to_str().unwrap(),
        ],
    );
    world::put(
        &work.join(".claude/skills/odh/SKILL.md"),
        "---\nname: odh\ndescription: Edited on the remote\n---\nBody\n",
    );
    world::git(&work, &["add", "-A"]);
    world::git(&work, &["commit", "-q", "-m", "edit"]);
    world::git(&work, &["push", "-q", "origin", "HEAD:main"]);
    world::git(&fx.w.odh, &["fetch", "-q"]);
    let second = fx.all();
    assert_eq!(
        item(&second, "repo:odh", "skill", "odh").description,
        "Edited on the remote"
    );
    assert_eq!(
        source(&second, "repo:odh")
            .freshness
            .as_ref()
            .unwrap()
            .behind,
        world::ODH_BEHIND + 1
    );
}

fn snapshot(root: &std::path::Path) -> std::collections::BTreeMap<PathBuf, (u64, u128)> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let mtime = meta
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            out.insert(path.clone(), (meta.len(), mtime));
            if meta.is_dir() {
                stack.push(path);
            }
        }
    }
    out
}

#[test]
fn no_detector_writes_anything_in_a_read_only_tree() {
    let fx = Fx::new("readonly");
    let writable_first = fx.all();
    let status = std::process::Command::new("chmod")
        .args(["-R", "a-w"])
        .arg(&fx.w.home)
        .status()
        .unwrap();
    assert!(status.success());
    let before = snapshot(&fx.w.home);
    let read_only = fx.all();
    let after = snapshot(&fx.w.home);
    make_writable(&fx.w.home);
    assert_eq!(before, after, "a scan changed the fixture home");
    assert_eq!(writable_first.sources.len(), read_only.sources.len());
}

#[test]
fn the_detector_sources_hold_no_call_that_opens_a_file_for_writing() {
    let sources: &[(&str, &str)] = &[
        ("account.rs", include_str!("account.rs")),
        ("client.rs", include_str!("client.rs")),
        ("fsx.rs", include_str!("fsx.rs")),
        ("gitx.rs", include_str!("gitx.rs")),
        ("inert.rs", include_str!("inert.rs")),
        ("item.rs", include_str!("item.rs")),
        ("layout.rs", include_str!("layout.rs")),
        ("library.rs", include_str!("library.rs")),
        ("loose.rs", include_str!("loose.rs")),
        ("org.rs", include_str!("org.rs")),
        ("plugin.rs", include_str!("plugin.rs")),
        ("repo.rs", include_str!("repo.rs")),
        ("scope.rs", include_str!("scope.rs")),
        ("tap.rs", include_str!("tap.rs")),
        ("vendored.rs", include_str!("vendored.rs")),
    ];
    let forbidden = [
        "File::create",
        "OpenOptions",
        "fs::write",
        "write_all",
        "create_dir",
        "remove_file",
        "remove_dir",
        "fs::rename",
        "fs::copy",
        "set_permissions",
        "set_modified",
        "atomic_write",
        "fs::symlink",
        "hard_link",
    ];
    for (name, text) in sources {
        let production = text.split("#[cfg(test)]").next().unwrap();
        for word in forbidden {
            if *name == "gitx.rs" && word == "write_all" {
                continue;
            }
            assert!(!production.contains(word), "{name} contains {word}");
        }
    }
    let git = include_str!("gitx.rs")
        .split("#[cfg(test)]")
        .next()
        .unwrap();
    for verb in [
        "\"fetch\"",
        "\"pull\"",
        "\"checkout\"",
        "\"reset\"",
        "\"commit\"",
        "\"add\"",
        "\"status\"",
        "\"gc\"",
    ] {
        assert!(!git.contains(verb), "gitx runs {verb}");
    }
}

#[test]
fn source_roots_list_add_and_remove_edit_only_context_json() {
    let fx = Fx::new("roots");
    let listing = roots::list(&fx.roots, &fx.config);
    let rows = listing["roots"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .map(|r| r["origin"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["default", "default", "config"]
    );
    assert_eq!(rows[0]["repo"], true);

    let extra = fx.w.base.join("extra-root");
    std::fs::create_dir_all(&extra).unwrap();
    let path = fx.roots.context_config_path();
    let before = std::fs::read_to_string(&path).unwrap();
    let preview = roots::change(
        &fx.roots,
        roots::Change::Add,
        extra.to_str().unwrap(),
        &fx.w.base,
        true,
    )
    .unwrap();
    assert_eq!(preview["dryRun"], true);
    assert!(preview["result"].is_null());
    assert_eq!(preview["plan"]["steps"][0]["op"], "merge");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        before,
        "a dry run writes nothing"
    );

    let applied = roots::change(
        &fx.roots,
        roots::Change::Add,
        extra.to_str().unwrap(),
        &fx.w.base,
        false,
    )
    .unwrap();
    assert_eq!(applied["result"]["applied"], true);
    assert_eq!(applied["result"]["changed"][0], path.to_str().unwrap());
    assert_eq!(applied["result"]["backups"].as_array().unwrap().len(), 1);
    let config = load_config(&path);
    assert_eq!(config.source_roots.len(), 2);
    assert_eq!(
        config.clients_root, "~/work/odh/clients",
        "foreign keys stay"
    );
    assert_eq!(
        roots::change(
            &fx.roots,
            roots::Change::Add,
            extra.to_str().unwrap(),
            &fx.w.base,
            true
        )
        .unwrap_err()
        .code(),
        "conflict"
    );

    let removed = roots::change(
        &fx.roots,
        roots::Change::Remove,
        extra.to_str().unwrap(),
        &fx.w.base,
        false,
    )
    .unwrap();
    assert_eq!(
        removed["plan"]["undo"],
        format!(
            "toolportctl sources root add {}",
            super::fsx::canonical(&extra).display()
        )
    );
    assert_eq!(load_config(&path).source_roots, ["~/dups"]);
    assert_eq!(
        roots::change(
            &fx.roots,
            roots::Change::Remove,
            fx.w.clients.to_str().unwrap(),
            &fx.w.base,
            true
        )
        .unwrap_err()
        .code(),
        "not_found"
    );
    assert_eq!(
        roots::change(
            &fx.roots,
            roots::Change::Add,
            fx.w.base.join("missing").to_str().unwrap(),
            &fx.w.base,
            true
        )
        .unwrap_err()
        .code(),
        "not_found"
    );
}
