use super::*;
use serde_json::Value;
use std::path::{Path, PathBuf};

struct Fx {
    dir: PathBuf,
    home: PathBuf,
    repo: PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
    _override: crate::registry::DataDirOverride,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let lock = crate::registry::data_dir_test_lock();
        let dir = std::env::temp_dir().join(format!("ctl-skills-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (home, repo, data) = (dir.join("home"), dir.join("repo"), dir.join("data"));
        for d in [&home, &data] {
            std::fs::create_dir_all(d).unwrap();
        }
        for (rel, text) in [
            (
                "skills/demo/SKILL.md",
                "---\nname: demo\ndescription: A synthetic demo skill\n---\nBody text\n",
            ),
            (
                "skills/other/SKILL.md",
                "---\nname: other\ndescription: Another synthetic skill\n---\nMore text\n",
            ),
        ] {
            let path = repo.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let guard = crate::registry::DataDirOverride::set(&data);
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        Self {
            dir,
            home,
            repo,
            _lock: lock,
            _override: guard,
        }
    }

    fn repo(&self) -> String {
        self.repo.to_string_lossy().into_owned()
    }

    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![self.dir.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push((p.clone(), std::fs::read(&p).unwrap()));
                }
            }
        }
        out.sort();
        out
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn cli(list: &[&str]) -> (i32, Value, String) {
    let list: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&list, &mut out, &mut err);
    let out = String::from_utf8(out).unwrap();
    let value = serde_json::from_str(out.trim()).unwrap_or(Value::Null);
    (code, value, String::from_utf8(err).unwrap())
}

#[test]
fn ls_and_lint_report_synthetic_skills() {
    let fx = Fx::new("ls");
    let (code, v, _) = cli(&["--json", "skills", "ls", "--repo", &fx.repo()]);
    assert_eq!(code, 0);
    assert_eq!(v["command"], "skills ls");
    let names: Vec<&str> = v["data"]["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["demo", "other"]);
    let (code, v, _) = cli(&[
        "--json",
        "skills",
        "lint",
        "--repo",
        &fx.repo(),
        "--name",
        "demo",
    ]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["errors"], 0);
}

#[test]
fn lint_errors_exit_one() {
    let fx = Fx::new("lint-bad");
    std::fs::create_dir_all(fx.repo.join("rules/demo")).unwrap();
    std::fs::write(
        fx.repo.join("rules/demo/SKILL.md"),
        "---\nname: demo\ndescription: A clashing synthetic rule\n---\nBody\n",
    )
    .unwrap();
    let (code, v, _) = cli(&["--json", "skills", "lint", "--repo", &fx.repo()]);
    assert_eq!(code, 1);
    assert!(v["data"]["errors"].as_u64().unwrap() > 0);
}

#[test]
fn sync_dry_run_writes_nothing_then_real_sync_makes_diff_clean() {
    let fx = Fx::new("sync");
    let before = fx.snapshot();
    let (code, v, _) = cli(&[
        "--json",
        "skills",
        "sync",
        "--dry-run",
        "--repo",
        &fx.repo(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["dryRun"], true);
    assert_eq!(v["data"]["skillCount"], 2);
    assert_eq!(fx.snapshot(), before);

    let (code, v, _) = cli(&["--json", "skills", "diff", "--repo", &fx.repo()]);
    assert_eq!(code, 1);
    assert_eq!(v["data"]["noLockfile"], true);
    assert_eq!(v["data"]["new"], serde_json::json!(["demo", "other"]));

    let (code, v, _) = cli(&["--json", "skills", "sync", "--repo", &fx.repo()]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["dryRun"], false);
    assert_ne!(fx.snapshot(), before);

    let (code, v, _) = cli(&["--json", "skills", "diff", "--repo", &fx.repo()]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["clean"], true);
    assert_eq!(v["data"]["unchanged"], 2);

    std::fs::write(
        fx.repo.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: A synthetic demo skill\n---\nChanged\n",
    )
    .unwrap();
    let (code, v, _) = cli(&["--json", "skills", "diff", "--repo", &fx.repo()]);
    assert_eq!(code, 1);
    assert_eq!(v["data"]["modified"], serde_json::json!(["demo"]));
}

#[test]
fn skills_usage_errors_exit_two() {
    let _fx = Fx::new("usage");
    for list in [
        &["skills"][..],
        &["skills", "ls", "--bogus"][..],
        &["skills", "ls", "--repo"][..],
        &["skills", "diff", "--dry-run"][..],
        &["context"][..],
        &["context", "plan", "--dry-run"][..],
    ] {
        let (code, _, err) = cli(list);
        assert_eq!(code, 2, "{list:?}: {err}");
    }
}

fn file_count(dir: &Path) -> usize {
    walk(dir)
}

fn walk(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |rd| {
        rd.flatten()
            .map(|e| {
                if e.path().is_dir() {
                    walk(&e.path())
                } else {
                    1
                }
            })
            .sum()
    })
}

#[test]
fn context_plan_apply_sync_dry_run_and_real() {
    let fx = Fx::new("context");
    let h = fx.home.to_string_lossy().into_owned();
    let before = file_count(&fx.home);
    let (code, v, err) = cli(&["--json", "context", "plan", "--home", &h]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["dryRun"], true);
    let (code, v, _) = cli(&["--json", "context", "apply", "--dry-run", "--home", &h]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["dryRun"], true);
    let (code, v, _) = cli(&["--json", "context", "sync", "--dry-run", "--home", &h]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["dryRun"], true);
    assert!(v["data"]["apply"].is_null());
    assert_eq!(file_count(&fx.home), before);

    let (code, v, err) = cli(&["--json", "context", "sync", "--home", &h]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["dryRun"], false);
    assert_eq!(v["data"]["plan"]["dryRun"], true);
    assert_eq!(v["data"]["apply"]["dryRun"], false);
}

#[test]
fn plus_handlers_are_registered() {
    let fx = Fx::new("handlers");
    let args = serde_json::json!({"repo_path": fx.repo()});
    for name in ["plus.skills.list", "plus.skills.lint", "plus.skills.diff"] {
        crate::plus::dispatch(name, args.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    for name in [
        "plus.compression.status",
        "plus.compression.ledger",
        "plus.compression.plan",
    ] {
        let value = crate::plus::dispatch(name, serde_json::json!({}))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(value.is_object(), "{name}");
    }
}
