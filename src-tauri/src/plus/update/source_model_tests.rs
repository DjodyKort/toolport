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
fn docker_is_recognized_and_left_for_mig_upd_8() {
    let e = entry(json!({"command": "docker", "args": ["run", "--rm", "image"]}));
    let src = source::detect(&e, None, &SystemGit);
    let Source::Unknown { reason } = src else {
        panic!("expected unknown, got {src:?}");
    };
    assert!(reason.contains("docker"), "{reason}");
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
