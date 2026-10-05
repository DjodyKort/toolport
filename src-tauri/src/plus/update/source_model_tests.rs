//! MIG-UPD-4 phase 1: the `remote`/`branch`/`upstream`/`drift` model, migration of pre-existing
//! stored metadata, and detection against the three-remote fixture (`tests/fixtures/update_source.rs`).

use super::exec::SystemGit;
use super::source::{self, Source};
use super::*;
use serde_json::json;
use std::path::Path;

#[path = "../../../tests/fixtures/update_source.rs"]
mod update_source;

fn entry(value: serde_json::Value) -> ServerEntry {
    let mut v = json!({"name": "x", "transport": "stdio"});
    for (k, val) in value.as_object().unwrap() {
        v[k] = val.clone();
    }
    if v.get("id").is_none() {
        v["id"] = v["name"].clone();
    }
    serde_json::from_value(v).unwrap()
}

fn node_entry(path: &Path) -> ServerEntry {
    entry(json!({"command": "node", "args": [path.to_str().unwrap()]}))
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("plus-update-source-model-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

#[test]
fn old_stored_metadata_migrates_with_origin_default_and_no_upstream() {
    let old = json!({
        "type": "git",
        "path": "/srv/x",
        "remote_url": "git@github.invalid:a/b.git",
        "branch": "main",
        "post_update": "make",
    });
    let migrated = source::from_meta(&old).unwrap();
    assert_eq!(
        migrated,
        Source::Git {
            path: "/srv/x".into(),
            remote: "origin".into(),
            branch: "main".into(),
            upstream: None,
            post_update: Some("make".into()),
            drift: false,
        }
    );
}

#[test]
fn old_stored_metadata_without_a_branch_migrates_to_an_empty_one() {
    let old = json!({"type": "git", "path": "/srv/y"});
    let migrated = source::from_meta(&old).unwrap();
    let Source::Git { remote, branch, upstream, drift, .. } = migrated else {
        panic!("expected git");
    };
    assert_eq!((remote.as_str(), branch.as_str(), upstream, drift), ("origin", "", None, false));
}

#[test]
fn node_env_file_does_not_hide_the_real_script_path() {
    let r = update_source::build(&tmp("envfile"));
    let e = entry(json!({
        "command": "node",
        "args": ["--env-file=/x/.env", r.work.join("dist/index.js").to_str().unwrap()],
    }));
    let src = source::detect(&e, None, &SystemGit);
    let Source::Git { path, .. } = src else {
        panic!("expected git, got {src:?}");
    };
    assert_eq!(Path::new(&path), r.work);
}

#[test]
fn a_bare_long_flag_before_the_script_is_also_skipped() {
    let r = update_source::build(&tmp("bareflag"));
    let e = entry(json!({
        "command": "node",
        "args": ["--experimental-foo", r.work.join("dist/index.js").to_str().unwrap()],
    }));
    let Source::Git { path, .. } = source::detect(&e, None, &SystemGit) else {
        panic!("expected git");
    };
    assert_eq!(Path::new(&path), r.work);
}

#[test]
fn docker_is_recognized_with_an_image_and_tag() {
    let e = entry(json!({"command": "docker", "args": ["run", "--rm", "image"]}));
    let src = source::detect(&e, None, &SystemGit);
    assert_eq!(
        src,
        Source::Docker {
            image: "image".into(),
            tag: Some("latest".into()),
            digest: None,
        }
    );
}

#[test]
fn detect_fills_branch_and_tracked_remote_from_the_real_checkout() {
    let r = update_source::build(&tmp("branchfill"));
    let e = node_entry(&r.work.join("x.js"));
    let Source::Git {
        path,
        remote,
        branch,
        post_update,
        drift,
        ..
    } = source::detect(&e, None, &SystemGit)
    else {
        panic!("expected git");
    };
    assert_eq!(Path::new(&path), r.work);
    assert_eq!(remote, "fork");
    assert_eq!(branch, "main");
    assert_eq!(post_update, None);
    assert!(!drift);
}

#[test]
fn detect_fills_a_non_default_branch_when_that_is_what_is_checked_out() {
    let r = update_source::build(&tmp("featurebranch"));
    update_source::git(&r.work, &["checkout", "-q", "feature-x"]);
    let e = node_entry(&r.work.join("x.js"));
    let Source::Git { remote, branch, .. } = source::detect(&e, None, &SystemGit) else {
        panic!("expected git");
    };
    assert_eq!((remote.as_str(), branch.as_str()), ("fork", "feature-x"));
}

#[test]
fn upstream_detection_prefers_the_remote_literally_named_upstream() {
    let r = update_source::build(&tmp("upstreampref"));
    let e = node_entry(&r.work.join("x.js"));
    let Source::Git { upstream, .. } = source::detect(&e, None, &SystemGit) else {
        panic!("expected git");
    };
    let upstream = upstream.expect("an upstream ahead of the shared base should be detected");
    assert_eq!(upstream.remote, "upstream");
}

#[test]
fn upstream_detection_falls_back_to_the_remote_not_tracked() {
    let r = update_source::build(&tmp("upstreamfallback"));
    let e = node_entry(&r.work_local_only.join("x.js"));
    let Source::Git { remote, upstream, .. } = source::detect(&e, None, &SystemGit) else {
        panic!("expected git");
    };
    assert_eq!(remote, "fork");
    let upstream = upstream.expect("local is the only other remote, so it is the fallback");
    assert_eq!(upstream.remote, "local");
}

#[test]
fn a_mirror_remote_with_identical_history_is_not_reported_as_upstream() {
    // `local` only ever received the shared base commit; once it matches HEAD exactly there is
    // nothing to report. Reuse `work_local_only` right after cloning it from a `fork` that was
    // still at the base commit to build that case directly.
    let base = tmp("mirror");
    let _ = std::fs::create_dir_all(&base);
    update_source::git(&base, &["init", "-q"]);
    update_source::git(&base, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "--allow-empty", "-q", "-m", "base"]);
    let mirror = base.join("mirror.git");
    std::fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    update_source::git(&base, &["clone", "-q", "--bare", ".", mirror.to_str().unwrap()]);
    let work = base.join("work");
    update_source::git(&base, &["clone", "-q", mirror.to_str().unwrap(), work.to_str().unwrap()]);
    update_source::git(&work, &["remote", "rename", "origin", "fork"]);
    update_source::git(&work, &["remote", "add", "local", mirror.to_str().unwrap()]);
    update_source::git(&work, &["fetch", "-q", "local"]);
    let e = node_entry(&work.join("x.js"));
    let Source::Git { upstream, .. } = source::detect(&e, None, &SystemGit) else {
        panic!("expected git");
    };
    assert_eq!(upstream, None);
}

#[test]
fn a_linked_worktree_is_detected_at_its_own_path_and_branch() {
    let r = update_source::build(&tmp("worktree"));
    assert!(!source::is_worktree(&r.work));
    assert!(source::is_worktree(&r.worktree));
    let e = node_entry(&r.worktree.join("x.js"));
    let Source::Git { path, branch, .. } = source::detect(&e, None, &SystemGit) else {
        panic!("expected git");
    };
    assert_eq!(Path::new(&path), r.worktree);
    assert_eq!(branch, "wt-branch");
}

#[test]
fn redetect_flags_drift_only_when_the_launch_directory_moved() {
    let r = update_source::build(&tmp("drift"));
    let e = node_entry(&r.work.join("x.js"));
    let fresh = source::detect(&e, None, &SystemGit);
    let Source::Git { drift, .. } = source::recheck_drift(&fresh, &e, None, &SystemGit) else {
        panic!("expected git");
    };
    assert!(!drift, "the stored path matches the launch directory");

    let moved = Source::Git {
        path: r.base.join("nowhere").to_string_lossy().into_owned(),
        remote: "fork".into(),
        branch: "main".into(),
        upstream: None,
        post_update: Some("make".into()),
        drift: false,
    };
    let Source::Git {
        path,
        remote,
        branch,
        post_update,
        drift,
        ..
    } = source::recheck_drift(&moved, &e, None, &SystemGit)
    else {
        panic!("expected git");
    };
    assert!(drift, "the launch directory moved away from the stored path");
    // everything the user configured survives a drift recheck untouched.
    assert_eq!(path, r.base.join("nowhere").to_string_lossy());
    assert_eq!(remote, "fork");
    assert_eq!(branch, "main");
    assert_eq!(post_update, Some("make".into()));
}

#[test]
fn effective_prefers_stored_metadata_and_detect_otherwise() {
    let r = update_source::build(&tmp("effective"));
    let e = node_entry(&r.work.join("x.js"));
    let (detected, from_meta) = source::effective(&e, None, &SystemGit);
    assert!(!from_meta);
    assert_eq!(detected.kind(), "git");

    let stored_meta = json!({"type": "git", "path": "/configured/elsewhere", "remote": "upstream", "branch": "main"});
    let e2 = entry(json!({
        "command": "node",
        "args": [r.work.join("x.js").to_str().unwrap()],
        "mcpmSource": stored_meta,
    }));
    let (stored, from_meta) = source::effective(&e2, None, &SystemGit);
    assert!(from_meta);
    let Source::Git { path, remote, .. } = stored else {
        panic!("expected git");
    };
    assert_eq!(path, "/configured/elsewhere");
    assert_eq!(remote, "upstream");
}

#[test]
fn set_can_switch_the_tracked_remote_to_another_configured_remote() {
    let r = update_source::build(&tmp("setremote"));
    let e = node_entry(&r.work.join("x.js"));
    let detected = source::detect(&e, None, &SystemGit);
    assert!(gitops::remote_branches(&SystemGit, &r.work, "upstream").contains(&"main".to_string()));

    let edit = source::SourceEdit {
        remote: Some("upstream".into()),
        ..Default::default()
    };
    let Source::Git { remote, branch, upstream, post_update, .. } =
        source::apply_edit(&detected, &edit).unwrap()
    else {
        panic!("expected git");
    };
    assert_eq!(remote, "upstream");
    assert_eq!(branch, "main", "an untouched field carries over as-is");
    assert_eq!(upstream, None);
    assert_eq!(post_update, None);
}

#[test]
fn set_can_switch_to_a_branch_that_only_exists_on_the_new_remote() {
    let r = update_source::build(&tmp("setbranch"));
    let e = node_entry(&r.work.join("x.js"));
    let detected = source::detect(&e, None, &SystemGit);
    assert!(gitops::remote_branches(&SystemGit, &r.work, "fork").contains(&"feature-x".to_string()));

    let edit = source::SourceEdit {
        branch: Some("feature-x".into()),
        ..Default::default()
    };
    let Source::Git { remote, branch, .. } = source::apply_edit(&detected, &edit).unwrap() else {
        panic!("expected git");
    };
    assert_eq!(remote, "fork");
    assert_eq!(branch, "feature-x");
}

#[test]
fn set_requires_both_halves_of_an_upstream_the_first_time_it_is_set() {
    let current = Source::Git {
        path: "/srv/x".into(),
        remote: "fork".into(),
        branch: "main".into(),
        upstream: None,
        post_update: None,
        drift: false,
    };
    let edit = source::SourceEdit {
        upstream_remote: Some("upstream".into()),
        ..Default::default()
    };
    let err = source::apply_edit(&current, &edit).unwrap_err();
    assert!(err.contains("branch"), "{err}");
}

#[test]
fn set_can_update_only_the_named_half_of_an_existing_upstream() {
    let current = Source::Git {
        path: "/srv/x".into(),
        remote: "fork".into(),
        branch: "main".into(),
        upstream: Some(source::Upstream {
            remote: "upstream".into(),
            branch: "main".into(),
        }),
        post_update: None,
        drift: false,
    };
    let edit = source::SourceEdit {
        upstream_branch: Some("develop".into()),
        ..Default::default()
    };
    let Source::Git { upstream, .. } = source::apply_edit(&current, &edit).unwrap() else {
        panic!("expected git");
    };
    let upstream = upstream.unwrap();
    assert_eq!(upstream.remote, "upstream", "untouched half carries over");
    assert_eq!(upstream.branch, "develop");
}

#[test]
fn set_can_clear_upstream_and_post_update_together() {
    let current = Source::Git {
        path: "/srv/x".into(),
        remote: "fork".into(),
        branch: "main".into(),
        upstream: Some(source::Upstream {
            remote: "upstream".into(),
            branch: "main".into(),
        }),
        post_update: Some("make".into()),
        drift: false,
    };
    let edit = source::SourceEdit {
        clear_upstream: true,
        clear_post_update: true,
        ..Default::default()
    };
    let Source::Git { upstream, post_update, remote, branch, .. } =
        source::apply_edit(&current, &edit).unwrap()
    else {
        panic!("expected git");
    };
    assert_eq!(upstream, None);
    assert_eq!(post_update, None);
    assert_eq!(remote, "fork", "clearing other fields does not touch remote/branch");
    assert_eq!(branch, "main");
}

#[test]
fn set_rejects_a_source_that_is_not_git_backed() {
    let current = Source::Npx { package: "some-pkg".into() };
    let err = source::apply_edit(&current, &source::SourceEdit::default()).unwrap_err();
    assert!(err.contains("git"), "{err}");
}

#[test]
fn set_then_recheck_drift_reports_drift_against_the_new_path() {
    let r = update_source::build(&tmp("setdrift"));
    let e = node_entry(&r.work.join("x.js"));
    let detected = source::detect(&e, None, &SystemGit);
    let edit = source::SourceEdit {
        path: Some(r.base.join("nowhere").to_string_lossy().into_owned()),
        ..Default::default()
    };
    let edited = source::apply_edit(&detected, &edit).unwrap();
    let Source::Git { drift, path, .. } = source::recheck_drift(&edited, &e, None, &SystemGit) else {
        panic!("expected git");
    };
    assert!(drift, "the edited path no longer matches the launch directory");
    assert_eq!(path, r.base.join("nowhere").to_string_lossy());
}

#[test]
fn replace_meta_drops_keys_that_set_meta_fields_would_have_left_behind() {
    let mut e = node_entry(Path::new("/srv/x.js"));
    source::set_meta_fields(
        &mut e,
        json!({"type": "git", "path": "/srv/x", "remote": "fork", "branch": "main", "upstream": {"remote": "upstream", "branch": "main"}, "post_update": "make"})
            .as_object()
            .unwrap()
            .clone(),
    );
    let stored = source::stored(&e).unwrap();
    let edit = source::SourceEdit {
        clear_upstream: true,
        clear_post_update: true,
        ..Default::default()
    };
    let edited = source::apply_edit(&stored, &edit).unwrap();
    source::replace_meta(&mut e, edited.to_meta());
    let reloaded = source::stored(&e).unwrap();
    let Source::Git { upstream, post_update, .. } = reloaded else {
        panic!("expected git");
    };
    assert_eq!(upstream, None);
    assert_eq!(post_update, None);
}
