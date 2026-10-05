use super::library_remote as remote;
use super::library_sync as sync;
use super::*;
use crate::plus::context::load_config;
use std::sync::Mutex;

#[path = "../../../tests/common/sources_world.rs"]
mod world;
use world::git;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static ENV: Mutex<()> = Mutex::new(());

struct Fx {
    w: world::SourcesWorld,
    roots: Roots,
    config: ContextConfig,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "library-remote-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let w = world::build(&base);
        let roots = Roots::from_home(&w.home);
        let config = load_config(&roots.context_config_path());
        Self { w, roots, config }
    }

    fn status(&self) -> serde_json::Value {
        remote::status(&self.roots, &self.config, Some(&self.w.data), false).unwrap()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.w.base);
    }
}

fn identity() {
    for (key, value) in [
        ("GIT_AUTHOR_NAME", "t"),
        ("GIT_COMMITTER_NAME", "t"),
        ("GIT_AUTHOR_EMAIL", "t@example.invalid"),
        ("GIT_COMMITTER_EMAIL", "t@example.invalid"),
    ] {
        std::env::set_var(key, value);
    }
}

fn canary() -> String {
    format!("{}{}_{}", "gh", "p", "CANARY0123456789".repeat(3))
}

fn no_gitleaks() {
    std::env::set_var("TOOLPORT_GITLEAKS_BIN", "/nonexistent/gitleaks");
}

#[test]
fn status_reports_the_clone_its_duplicate_and_no_fetch_without_the_flag() {
    let fx = Fx::new("status");
    let data = fx.status();
    assert_eq!(data["repo"], fx.w.library.to_string_lossy().to_string());
    assert_eq!((data["ahead"].as_u64(), data["behind"].as_u64()), (Some(1), Some(0)));
    assert_eq!(data["branch"], "main");
    assert_eq!(data["fetch"]["requested"], false);
    assert_eq!(data["auth"]["checked"], false);
    let twins = data["duplicateClones"].as_array().unwrap();
    assert_eq!(twins.len(), 1);
    assert_eq!(twins[0]["path"], fx.w.duplicate.to_string_lossy().to_string());
    assert_eq!((twins[0]["ahead"].as_u64(), twins[0]["behind"].as_u64()), (Some(0), Some(0)));
    assert!(twins[0]["skills"].as_u64().unwrap() > 0);
    assert_eq!(twins[0]["sameRemote"], true);
}

#[test]
fn a_dirty_clone_counts_its_uncommitted_files() {
    let fx = Fx::new("dirty");
    std::fs::write(fx.w.library.join("notes.md"), "a\n").unwrap();
    std::fs::write(fx.w.library.join("skills/review/extra.md"), "b\n").unwrap();
    assert_eq!(fx.status()["uncommitted"], 2);
}

#[test]
fn status_with_fetch_sees_new_remote_commits_and_checks_the_remote() {
    let fx = Fx::new("fetch");
    let twin = &fx.w.duplicate;
    std::fs::write(twin.join("fresh.md"), "x\n").unwrap();
    git(twin, &["add", "-A"]);
    git(twin, &["commit", "-q", "-m", "fresh"]);
    git(twin, &["push", "-q", "origin", "main"]);
    assert_eq!(fx.status()["behind"], 0);
    let data = remote::status(&fx.roots, &fx.config, Some(&fx.w.data), true).unwrap();
    assert_eq!(data["behind"], 1);
    assert_eq!((data["fetch"]["ok"].as_bool(), data["auth"]["ok"].as_bool()), (Some(true), Some(true)));
    assert_eq!(data["auth"]["checked"], true);
    assert!(data["lastFetch"].as_str().is_some());
}

#[test]
fn pull_refuses_when_dirty_and_fast_forwards_when_behind() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    identity();
    no_gitleaks();
    let fx = Fx::new("pull");
    let pushed = sync::push(&fx.w.library, None, false).unwrap();
    assert_eq!(pushed["pushed"], true);
    let twin = &fx.w.duplicate;
    std::fs::write(twin.join("local.md"), "dirty\n").unwrap();
    let refused = sync::pull(twin, false).unwrap_err();
    assert_eq!(refused.kind.code(), "refused");
    assert!(refused.message.contains("uncommitted"));
    std::fs::remove_file(twin.join("local.md")).unwrap();
    let plan = sync::pull(twin, true).unwrap();
    assert_eq!(plan["dryRun"], true);
    assert_eq!(plan["plan"]["summary"], "Fast-forward 1 commit(s) from origin/main");
    let before = git(twin, &["rev-parse", "HEAD"]);
    let done = sync::pull(twin, false).unwrap();
    assert_eq!((done["pulled"].as_bool(), done["commits"].as_u64()), (Some(true), Some(1)));
    assert_eq!(git(twin, &["rev-parse", "HEAD"]), git(&fx.w.library, &["rev-parse", "HEAD"]));
    assert!(done["result"]["undo"].as_str().unwrap().ends_with(&before));
    assert!(!done["result"]["changed"].as_array().unwrap().is_empty());
    let again = sync::pull(twin, false).unwrap();
    assert_eq!(again["pulled"], false);
}

#[test]
fn pull_refuses_a_diverged_clone_without_touching_it() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    identity();
    let fx = Fx::new("diverged");
    let twin = &fx.w.duplicate;
    std::fs::write(twin.join("theirs.md"), "x\n").unwrap();
    git(twin, &["add", "-A"]);
    git(twin, &["commit", "-q", "-m", "theirs"]);
    git(twin, &["push", "-q", "origin", "main"]);
    let head = git(&fx.w.library, &["rev-parse", "HEAD"]);
    let err = sync::pull(&fx.w.library, false).unwrap_err();
    assert!(err.message.starts_with("diverged"), "{}", err.message);
    assert_eq!(git(&fx.w.library, &["rev-parse", "HEAD"]), head);
}

#[test]
fn push_dry_run_shows_the_audit_and_the_scan_and_pushes_nothing() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    no_gitleaks();
    let fx = Fx::new("push-dry");
    let remote_head = git(&fx.w.library, &["rev-parse", "origin/main"]);
    let plan = sync::push(&fx.w.library, Some("tidy"), true).unwrap();
    assert_eq!(plan["plan"]["summary"], "Push 1 commit(s) to origin/main");
    assert_eq!(plan["checks"]["audit"]["ran"], true);
    assert_eq!(plan["checks"]["gitleaks"]["available"], false);
    assert_eq!(plan["checks"]["blocked"], false);
    assert_eq!(plan["commits"].as_array().unwrap().len(), 1);
    let steps = plan["plan"]["steps"].as_array().unwrap();
    assert!(steps.iter().any(|s| s["detail"].as_str().unwrap().starts_with("audit:")));
    assert!(steps.iter().any(|s| s["detail"].as_str().unwrap().starts_with("gitleaks not installed")));
    assert_eq!(git(&fx.w.base.join("remotes/ai-skills.git"), &["rev-parse", "main"]), remote_head);
}

#[test]
fn a_canary_in_a_commit_or_the_working_tree_makes_push_refuse() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    identity();
    no_gitleaks();
    let fx = Fx::new("push-canary");
    let lib = &fx.w.library;
    let bare = fx.w.base.join("remotes/ai-skills.git");
    let before = git(&bare, &["rev-parse", "main"]);
    std::fs::write(lib.join("skills/review/notes.md"), format!("deploy with {}\n", canary())).unwrap();
    let dry = sync::push(lib, None, true).unwrap();
    assert_eq!(dry["checks"]["blocked"], true);
    assert!(dry["plan"]["warnings"][0].as_str().unwrap().contains("would be refused"));
    let err = sync::push(lib, None, false).unwrap_err();
    assert_eq!(err.kind.code(), "refused");
    assert!(err.message.contains("github-token in skills/review/notes.md:1"), "{}", err.message);
    assert!(!err.message.contains("CANARY"));
    git(lib, &["add", "-A"]);
    git(lib, &["commit", "-q", "-m", "oops"]);
    let committed = sync::push(lib, None, false).unwrap_err();
    assert_eq!(committed.kind.code(), "refused");
    assert!(!serde_json::to_string(&sync::push(lib, None, true).unwrap()).unwrap().contains("CANARY"));
    assert_eq!(git(&bare, &["rev-parse", "main"]), before);
}

#[test]
fn a_clean_push_commits_pending_files_and_pushes_them() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    identity();
    no_gitleaks();
    let fx = Fx::new("push-apply");
    std::fs::write(fx.w.library.join("skills/review/extra.md"), "more\n").unwrap();
    let done = sync::push(&fx.w.library, Some("add extra"), false).unwrap();
    assert_eq!(done["pushed"], true);
    let bare = fx.w.base.join("remotes/ai-skills.git");
    assert_eq!(git(&bare, &["log", "-1", "--format=%s", "main"]), "add extra");
    assert_eq!(git(&bare, &["rev-list", "--count", "main"]), git(&fx.w.library, &["rev-list", "--count", "HEAD"]));
    assert_eq!(sync::push(&fx.w.library, None, false).unwrap()["pushed"], false);
}

#[cfg(unix)]
#[test]
fn the_gitleaks_report_is_read_and_blocks_without_printing_a_match() {
    use std::os::unix::fs::PermissionsExt;
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let fx = Fx::new("push-stub");
    let stub = fx.w.base.join("gitleaks-stub.sh");
    std::fs::write(
        &stub,
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do [ \"$1\" = --report-path ] && out=$2; shift; done\nprintf '[{\"RuleID\":\"stub-rule\",\"File\":\"skills/review/SKILL.md\",\"StartLine\":3,\"Commit\":\"abc1234def\",\"Secret\":\"REDACTED\"}]' > \"$out\"\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("TOOLPORT_GITLEAKS_BIN", &stub);
    let plan = sync::push(&fx.w.library, None, true).unwrap();
    std::env::remove_var("TOOLPORT_GITLEAKS_BIN");
    assert_eq!(plan["checks"]["gitleaks"]["count"], 1);
    assert_eq!(plan["checks"]["gitleaks"]["findings"][0]["rule"], "stub-rule");
    assert_eq!(plan["checks"]["gitleaks"]["findings"][0]["commit"], "abc1234");
    assert_eq!(plan["checks"]["blocked"], true);
    assert!(!serde_json::to_string(&plan).unwrap().contains("REDACTED"));
}
