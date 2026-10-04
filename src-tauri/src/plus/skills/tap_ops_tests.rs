use super::git::{GitRunner, MockGit, SystemGit};
use super::tap_fixtures::{git_in, Remotes};
use super::tap_ops::{self, Env, InstallOptions, Kind};
use super::taps::{load_taps, parse_source, redact, valid_source_url, valid_tap_name, TAPS_FILE};
use crate::plus::testutil::tree_snapshot;
use std::fs;
use std::path::{Path, PathBuf};

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("tap-ops-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Tmp(fs::canonicalize(&p).unwrap())
    }

    fn config(&self) -> PathBuf {
        let dir = self.0.join("config");
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn target(&self) -> PathBuf {
        self.0.join("target")
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok()
}

fn env<'a>(config: &'a Path, git: &'a dyn GitRunner) -> Env<'a> {
    Env {
        config_dir: config,
        git,
    }
}

fn opts(target: &Path) -> InstallOptions<'_> {
    InstallOptions {
        target,
        no_audit: false,
        dry_run: false,
    }
}

fn names(report: &tap_ops::InstallReport) -> Vec<(String, bool)> {
    report
        .skills
        .iter()
        .map(|s| (s.name.clone(), s.installed))
        .collect()
}

#[test]
fn tap_names_reject_anything_that_could_leave_the_taps_directory() {
    let long = "a".repeat(65);
    for bad in [
        "",
        "..",
        "../x",
        "x/../y",
        ".hidden",
        "-x",
        "a/b",
        "a\\b",
        "/abs",
        "C:\\x",
        "a\0b",
        "a b",
        "a\nb",
        "a\u{1b}[0m",
        "é",
        &long,
    ] {
        assert!(valid_tap_name(bad).is_err(), "{bad:?}");
    }
    for ok in ["team", "acme-skills", "a.b_c", "X1", &"a".repeat(64)] {
        assert!(valid_tap_name(ok).is_ok(), "{ok}");
    }
    let message = valid_tap_name("a\u{1b}[31m").unwrap_err();
    assert!(!message.contains('\u{1b}'), "{message:?}");
}

#[test]
fn a_source_is_github_shorthand_a_url_or_an_absolute_path() {
    let github = parse_source("acme/skills").unwrap();
    assert_eq!(github.url, "https://github.com/acme/skills.git");
    assert_eq!(github.name, Ok("acme-skills".to_string()));
    for (spec, name) in [
        ("https://example.com/org/skills.git", "skills"),
        ("https://example.com/org/skills/", "skills"),
        ("git@example.com:org/team.git", "team"),
        ("ssh://git@example.com/org/tools", "tools"),
        ("git://example.com/org/tools.git", "tools"),
        ("file:///srv/git/shared.git", "shared"),
        ("/srv/git/team.git", "team"),
        ("C:\\git\\team.git", "team"),
    ] {
        let source = parse_source(spec).unwrap();
        assert_eq!(source.url, spec);
        assert_eq!(source.name, Ok(name.to_string()), "{spec}");
    }
    assert!(parse_source("/srv/git/.git").unwrap().name.is_err());
}

#[test]
fn a_source_that_git_would_treat_as_an_option_helper_or_credential_is_refused() {
    for bad in [
        "",
        " ",
        "x",
        "./x",
        "relative/path/x",
        "--upload-pack=touch /tmp/x",
        "-oProxyCommand=x",
        "ext::sh -c touch /tmp/pwned",
        "fd::17/x",
        "http://example.com/x.git",
        "ftp://example.com/x.git",
        "https://user:tok3n@example.com/x.git",
        "https://user:@example.com/x.git",
        " https://example.com/x.git",
        "https://example.com/x\n.git",
        "https://",
        "acme/skills/extra",
    ] {
        assert!(parse_source(bad).is_err(), "{bad:?}");
    }
    let error = valid_source_url("https://user:tok3n@example.com/x.git").unwrap_err();
    assert!(!error.contains("tok3n"), "{error}");
    assert!(valid_source_url("ssh://git@example.com/x.git").is_ok());
}

#[test]
fn redaction_hides_what_sits_before_the_at_sign() {
    for text in [
        "fatal: unable to access 'https://user:tok3n@example.com/x.git/': 403",
        "ssh://tok3n@example.com/x",
        "Cloning into https://a:b@host/x and https://c:d@host/y",
    ] {
        let shown = redact(text);
        assert!(
            !shown.contains("tok3n") && !shown.contains(":b@") && !shown.contains(":d@"),
            "{shown}"
        );
        assert!(shown.contains("://***@"), "{shown}");
    }
    assert_eq!(
        redact("https://example.com/x.git"),
        "https://example.com/x.git"
    );
}

#[test]
fn add_list_update_and_remove_drive_git_through_the_runner() {
    let t = Tmp::new("mock");
    let config = t.config();
    let git = MockGit::with_head("abc123");
    let env = env(&config, &git);

    let added = tap_ops::add(&env, "acme/skills", None, false).unwrap();
    assert_eq!(added.tap.name, "acme-skills");
    assert_eq!(added.tap.repo, "acme/skills");
    assert!(added.cloned);
    assert_eq!(added.head.as_deref(), Some("abc123"));
    assert!(git.calls.borrow()[0].starts_with("clone --depth 1 https://github.com/acme/skills.git"));
    tap_ops::add(&env, "/srv/git/team.git", Some("team"), false).unwrap();
    assert_eq!(
        load_taps(&config)
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["acme-skills", "team"]
    );
    assert_eq!(tap_ops::list(&env).len(), 2);
    assert!(tap_ops::list(&env).iter().all(|r| r.cloned));

    let all = tap_ops::update(&env, None, false).unwrap();
    assert!(all
        .iter()
        .all(|r| r.ok && r.head.as_deref() == Some("abc123")));
    let one = tap_ops::update(&env, Some("team"), false).unwrap();
    assert_eq!(one.len(), 1);
    *git.fail_with.borrow_mut() = Some("git pull failed: offline".into());
    let failed = tap_ops::update(&env, None, false).unwrap();
    assert!(failed
        .iter()
        .all(|r| !r.ok && r.error.as_deref() == Some("offline")));
    *git.fail_with.borrow_mut() = None;

    let removed = tap_ops::remove(&env, "acme-skills", false).unwrap();
    assert!(removed.had_clone && removed.removed);
    assert!(!config.join("taps/acme-skills").exists());
    assert_eq!(load_taps(&config).len(), 1);
    let again = tap_ops::remove(&env, "acme-skills", false).unwrap_err();
    assert_eq!(again.kind, Kind::NotFound);
    assert_eq!(again.message, "Tap 'acme-skills' not found.");
}

#[test]
fn a_duplicate_tap_is_refused_even_in_another_case() {
    let t = Tmp::new("dup");
    let config = t.config();
    let git = MockGit::with_head("abc");
    let env = env(&config, &git);
    tap_ops::add(&env, "acme/skills", None, false).unwrap();
    for (spec, alias) in [("acme/skills", None), ("acme/other", Some("ACME-skills"))] {
        let error = tap_ops::add(&env, spec, alias, false).unwrap_err();
        assert_eq!(error.kind, Kind::Conflict, "{spec}");
    }
    assert_eq!(
        git.calls.borrow().len(),
        2,
        "one clone and one head, no second clone"
    );
}

#[test]
fn a_malicious_tap_name_is_rejected_before_git_or_the_disk_is_touched() {
    let t = Tmp::new("names");
    let config = t.config();
    fs::write(t.0.join("precious"), "keep").unwrap();
    let git = MockGit::with_head("abc");
    let env = env(&config, &git);
    let before = tree_snapshot(&t.0);
    let long = "a".repeat(65);
    for bad in [
        "../x",
        "../../precious",
        "/abs",
        "a/b",
        "a\\b",
        "a\0b",
        "..",
        ".x",
        &long,
    ] {
        for dry_run in [false, true] {
            let error = tap_ops::add(&env, "acme/skills", Some(bad), dry_run).unwrap_err();
            assert_eq!(error.kind, Kind::Invalid, "{bad:?}");
            assert_eq!(
                tap_ops::remove(&env, bad, dry_run).unwrap_err().kind,
                Kind::Invalid
            );
            assert_eq!(
                tap_ops::update(&env, Some(bad), dry_run).unwrap_err().kind,
                Kind::Invalid
            );
        }
    }
    assert!(git.calls.borrow().is_empty());
    assert_eq!(tree_snapshot(&t.0), before);
}

#[test]
fn a_hand_edited_index_cannot_aim_remove_or_update_outside_the_taps_directory() {
    let t = Tmp::new("tamper");
    let config = t.config();
    fs::create_dir_all(config.join("keep")).unwrap();
    fs::write(config.join("keep/data"), "x").unwrap();
    fs::write(
        config.join(TAPS_FILE),
        r#"{"../keep": {"repo": "x", "url": "/srv/git/x.git"},
            "/abs": {"repo": "x", "url": "/srv/git/x.git"},
            "ok": {"repo": "x", "url": "-oProxyCommand=x"},
            "creds": {"repo": "x", "url": "https://u:tok3n@example.com/x.git"},
            "fine": {"repo": "fine/repo", "url": "https://github.com/fine/repo.git"}}"#,
    )
    .unwrap();
    let git = MockGit::with_head("abc");
    let env = env(&config, &git);
    let taps = load_taps(&config);
    assert_eq!(taps.len(), 1);
    assert_eq!(taps[0].name, "fine");
    assert_eq!(
        tap_ops::remove(&env, "../keep", false).unwrap_err().kind,
        Kind::Invalid
    );
    assert!(config.join("keep/data").exists());
    assert!(tap_ops::list(&env)
        .iter()
        .all(|r| !r.path.to_string_lossy().contains("..")));
}

#[test]
fn a_dry_run_writes_nothing_and_never_starts_git() {
    let t = Tmp::new("dryrun");
    let config = t.config();
    let git = MockGit::with_head("abc");
    let env = env(&config, &git);
    let before = tree_snapshot(&t.0);
    let planned = tap_ops::add(&env, "acme/skills", None, true).unwrap();
    assert!(!planned.cloned);
    assert_eq!(planned.path, config.join("taps/acme-skills"));
    assert_eq!(tree_snapshot(&t.0), before);

    tap_ops::add(&env, "acme/skills", None, false).unwrap();
    let before = tree_snapshot(&t.0);
    let calls = git.calls.borrow().len();
    let removal = tap_ops::remove(&env, "acme-skills", true).unwrap();
    assert!(removal.had_clone && !removal.removed);
    let rows = tap_ops::update(&env, None, true).unwrap();
    assert!(rows.iter().all(|r| r.ok && r.head.is_none()));
    assert_eq!(git.calls.borrow().len(), calls);
    assert_eq!(tree_snapshot(&t.0), before);
}

#[test]
fn a_failed_clone_leaves_neither_a_directory_nor_an_index_entry() {
    let t = Tmp::new("clonefail");
    let config = t.config();
    let git = MockGit::with_head("abc");
    *git.fail_with.borrow_mut() =
        Some("git clone failed: fatal: unable to access 'https://u:tok3n@example.com/x/'".into());
    let env = env(&config, &git);
    let error = tap_ops::add(&env, "acme/skills", None, false).unwrap_err();
    assert_eq!(error.kind, Kind::Backend);
    assert!(
        error
            .message
            .starts_with("Failed to clone https://github.com/acme/skills.git: fatal"),
        "{}",
        error.message
    );
    assert!(!error.message.contains("tok3n"), "{}", error.message);
    assert!(!config.join("taps/acme-skills").exists());
    assert!(!config.join(TAPS_FILE).exists());
}

#[test]
fn an_orphan_directory_is_never_overwritten() {
    let t = Tmp::new("orphan");
    let config = t.config();
    fs::create_dir_all(config.join("taps/acme-skills")).unwrap();
    fs::write(config.join("taps/acme-skills/mine"), "x").unwrap();
    let git = MockGit::with_head("abc");
    let error = tap_ops::add(&env(&config, &git), "acme/skills", None, false).unwrap_err();
    assert_eq!(error.kind, Kind::Conflict);
    assert!(config.join("taps/acme-skills/mine").exists());
    assert!(git.calls.borrow().is_empty());
}

fn published(t: &Tmp, repos: &[&str]) -> Remotes {
    let remotes = Remotes::new(&t.0);
    for repo in repos {
        remotes.publish(repo);
    }
    remotes
}

#[test]
fn a_tap_is_cloned_shallow_from_a_local_remote_and_listed() {
    if !git_available() {
        return;
    }
    let t = Tmp::new("clone");
    let remotes = published(&t, &["acme/skills"]);
    let config = t.config();
    let env = env(&config, &SystemGit);
    let url = remotes.url("acme/skills");
    let added = tap_ops::add(&env, &url, Some("team"), false).unwrap();
    assert!(added.cloned && added.head.is_some());
    assert!(config
        .join("taps/team/skills/code-review/SKILL.md")
        .exists());
    assert_eq!(
        git_in(
            &config.join("taps/team"),
            &["rev-parse", "--is-shallow-repository"]
        ),
        "true"
    );
    let rows = tap_ops::list(&env);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tap.url, url);
    assert!(rows[0].cloned);
}

#[test]
fn update_fast_forwards_a_local_tap_and_reports_a_vanished_remote_without_a_credential() {
    if !git_available() {
        return;
    }
    let t = Tmp::new("update");
    let remotes = published(&t, &["acme/skills", "acme/extras"]);
    let config = t.config();
    let env = env(&config, &SystemGit);
    tap_ops::add(&env, &remotes.url("acme/skills"), Some("one"), false).unwrap();
    tap_ops::add(&env, &remotes.url("acme/extras"), Some("two"), false).unwrap();

    remotes.commit_file(
        "acme/skills",
        "skills/new-one/SKILL.md",
        "---\nname: new-one\ndescription: Newly added\n---\nbody\n",
    );
    let rows = tap_ops::update(&env, Some("one"), false).unwrap();
    assert!(rows[0].ok && rows[0].head.is_some());
    assert!(config.join("taps/one/skills/new-one/SKILL.md").exists());
    assert!(!config.join("taps/two/skills/new-one/SKILL.md").exists());

    fs::remove_dir_all(remotes.remote("acme/extras")).unwrap();
    fs::remove_dir_all(config.join("taps/one")).unwrap();
    let rows = tap_ops::update(&env, None, false).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].ok && rows[0].error.as_deref() == Some("the clone is missing"));
    assert!(!rows[1].ok);
    let error = rows[1].error.clone().unwrap();
    assert!(!error.contains("tok3n") && !error.is_empty(), "{error}");
    assert_eq!(
        tap_ops::update(&env, Some("nosuch"), false)
            .unwrap_err()
            .kind,
        Kind::NotFound
    );
}

#[test]
fn search_matches_names_descriptions_and_tags_case_insensitively() {
    if !git_available() {
        return;
    }
    let t = Tmp::new("search");
    let remotes = published(&t, &["acme/skills", "acme/extras"]);
    let config = t.config();
    let env = env(&config, &SystemGit);
    assert_eq!(tap_ops::search(&env, "review").tap_count, 0);
    tap_ops::add(&env, &remotes.url("acme/skills"), Some("one"), false).unwrap();
    tap_ops::add(&env, &remotes.url("acme/extras"), Some("two"), false).unwrap();

    let by = |query: &str| -> Vec<(String, String)> {
        tap_ops::search(&env, query)
            .hits
            .into_iter()
            .map(|h| (h.tap, h.name))
            .collect()
    };
    assert_eq!(
        by("REVIEW"),
        [
            ("one".to_string(), "code-review".to_string()),
            ("one".to_string(), "terraform-helper".to_string()),
            ("two".to_string(), "release-notes".to_string()),
        ]
    );
    assert_eq!(
        by("infrastructure"),
        [("one".to_string(), "terraform-helper".to_string())]
    );
    assert_eq!(by("quality").len(), 1);
    assert_eq!(by("style-guide").len(), 1);
    assert!(by("no such thing").is_empty());
    let found = tap_ops::search(&env, "style");
    assert!(found.hits.iter().any(|h| h.kind == "rule"));
    assert_eq!(found.warnings.len(), 1);
    assert!(
        found.warnings[0].starts_with("Failed to parse "),
        "{:?}",
        found.warnings
    );
    assert!(found.warnings[0].contains("escape"), "{:?}", found.warnings);
}

#[test]
fn install_copies_skills_and_rules_and_skips_what_exists() {
    if !git_available() {
        return;
    }
    let t = Tmp::new("install");
    let remotes = published(&t, &["acme/skills"]);
    let config = t.config();
    let git = remotes.rewrite();
    let env = env(&config, &git);
    let target = t.target();

    let first = tap_ops::install(&env, "@acme/skills", &opts(&target)).unwrap();
    assert!(first.tap_added && !first.blocked);
    assert_eq!(first.found, 3);
    assert_eq!(
        names(&first),
        [
            ("code-review".to_string(), true),
            ("terraform-helper".to_string(), true),
            ("style-guide".to_string(), true),
        ]
    );
    assert_eq!(first.skills[2].kind, "rule");
    assert!(target.join("skills/code-review/SKILL.md").exists());
    assert!(target
        .join("skills/code-review/reference/checklist.md")
        .exists());
    assert!(target
        .join("skills/code-review/scripts/summarize.sh")
        .exists());
    assert!(target.join("rules/style-guide/SKILL.md").exists());
    assert_eq!(first.skills[0].files, 3);
    assert!(!target.join("skills/code-review/.git").exists());
    assert_eq!(load_taps(&config)[0].name, "acme-skills");

    let second = tap_ops::install(&env, "@acme/skills", &opts(&target)).unwrap();
    assert!(!second.tap_added);
    assert!(second.skills.iter().all(|s| !s.installed));

    let single_target = t.0.join("single");
    let one = tap_ops::install(
        &env,
        "@acme/skills/code-review@1.2.0",
        &opts(&single_target),
    )
    .unwrap();
    assert_eq!(one.version.as_deref(), Some("1.2.0"));
    assert_eq!(names(&one), [("code-review".to_string(), true)]);
    assert!(!single_target.join("skills/terraform-helper").exists());
}

#[cfg(unix)]
#[test]
fn install_keeps_file_modes_and_leaves_symlinks_and_git_data_behind() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    if !git_available() {
        return;
    }
    let t = Tmp::new("modes");
    let remotes = published(&t, &["acme/skills"]);
    let config = t.config();
    let git = remotes.rewrite();
    let env = env(&config, &git);
    tap_ops::add(&env, "acme/skills", None, false).unwrap();
    let skill = config.join("taps/acme-skills/skills/code-review");
    fs::set_permissions(
        skill.join("scripts/summarize.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::write(t.0.join("secret.txt"), "outside the tap").unwrap();
    symlink(t.0.join("secret.txt"), skill.join("leak.txt")).unwrap();
    symlink(t.0.join("config"), skill.join("leakdir")).unwrap();
    fs::create_dir_all(skill.join(".git")).unwrap();
    fs::write(skill.join(".git/config"), "x").unwrap();

    let target = t.target();
    let report = tap_ops::install(&env, "@acme/skills/code-review", &opts(&target)).unwrap();
    let mode = fs::metadata(target.join("skills/code-review/scripts/summarize.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0);
    assert!(!target.join("skills/code-review/leak.txt").exists());
    assert!(!target.join("skills/code-review/leakdir").exists());
    assert!(!target.join("skills/code-review/.git").exists());
    assert_eq!(report.symlinks_skipped.len(), 2);
    assert_eq!(report.skills[0].files, 3);
}

#[cfg(unix)]
#[test]
fn a_skill_directory_that_is_a_symlink_out_of_the_tap_is_not_offered() {
    use std::os::unix::fs::symlink;
    if !git_available() {
        return;
    }
    let t = Tmp::new("linkdir");
    let remotes = published(&t, &["acme/skills"]);
    let config = t.config();
    let git = remotes.rewrite();
    let env = env(&config, &git);
    tap_ops::add(&env, "acme/skills", None, false).unwrap();
    let outside = t.0.join("outside/stolen");
    fs::create_dir_all(&outside).unwrap();
    fs::write(
        outside.join("SKILL.md"),
        "---\nname: stolen\ndescription: Lives outside the tap\n---\nbody\n",
    )
    .unwrap();
    symlink(&outside, config.join("taps/acme-skills/skills/stolen")).unwrap();

    let target = t.target();
    let report = tap_ops::install(&env, "@acme/skills", &opts(&target)).unwrap();
    assert!(report.skills.iter().all(|s| s.name != "stolen"));
    assert!(!target.join("skills/stolen").exists());
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("resolve outside the tap")),
        "{:?}",
        report.warnings
    );
    let hits = tap_ops::search(&env, "stolen");
    assert!(hits.hits.is_empty());
    assert_eq!(
        tap_ops::install(&env, "@acme/skills/stolen", &opts(&target))
            .unwrap_err()
            .kind,
        Kind::NotFound
    );
}

#[test]
fn a_skill_named_like_a_path_in_the_tap_never_reaches_the_target() {
    if !git_available() {
        return;
    }
    let t = Tmp::new("escape");
    let remotes = published(&t, &["acme/skills"]);
    let config = t.config();
    let git = remotes.rewrite();
    let env = env(&config, &git);
    let target = t.0.join("deep/er/target");

    let report = tap_ops::install(&env, "@acme/skills", &opts(&target)).unwrap();
    assert!(report
        .skills
        .iter()
        .all(|s| s.name != "../escape" && s.name != "escape"));
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("escape") && w.starts_with("Failed to parse")));
    let written: Vec<String> = tree_snapshot(&target)
        .keys()
        .filter(|k| !k.ends_with('/'))
        .cloned()
        .collect();
    assert!(written.iter().all(|p| !p.contains("escape")), "{written:?}");
    assert!(!t.0.join("deep/er/escape").exists());
    assert!(!t.0.join("deep/escape").exists());

    let wanted = tap_ops::install(&env, "@acme/skills/escape", &opts(&target)).unwrap_err();
    assert_eq!(wanted.kind, Kind::NotFound);
    assert!(
        wanted
            .message
            .starts_with("No skills found for '@acme/skills/escape'"),
        "{}",
        wanted.message
    );
}

#[test]
fn install_specs_that_could_name_a_path_are_rejected() {
    let t = Tmp::new("specs");
    let config = t.config();
    let git = MockGit::with_head("abc");
    let env = env(&config, &git);
    let target = t.target();
    let before = tree_snapshot(&t.0);
    for bad in [
        "",
        "acme",
        "@acme",
        "@acme/",
        "@/skills",
        "@acme/skills/",
        "@acme/skills/a/b",
        "@../skills",
        "@acme/..",
        "@acme/skills/..",
        "@acme/skills/../escape",
        "@acme/skills/Upper",
        "@acme/skills/a--b",
        "@acme/skills/-a",
        "@acme/skills@",
        "@acme/sk\0ills",
        "@-acme/skills",
        "@acme/skills\n",
        "@acme/skills/x\u{1b}[0m",
    ] {
        let error = tap_ops::install(&env, bad, &opts(&target)).unwrap_err();
        assert_eq!(error.kind, Kind::Invalid, "{bad:?}");
        assert!(
            !error.message.contains('\u{1b}') && !error.message.contains('\0'),
            "{:?}",
            error.message
        );
    }
    assert!(git.calls.borrow().is_empty());
    assert_eq!(tree_snapshot(&t.0), before);
}

#[test]
fn a_high_severity_finding_blocks_the_install_until_the_audit_is_skipped() {
    if !git_available() {
        return;
    }
    let t = Tmp::new("audit");
    let remotes = published(&t, &["acme/audit"]);
    let config = t.config();
    let git = remotes.rewrite();
    let env = env(&config, &git);
    let target = t.target();

    let blocked = tap_ops::install(&env, "@acme/audit", &opts(&target)).unwrap();
    assert!(blocked.blocked && blocked.skills.is_empty());
    assert!(blocked
        .findings
        .iter()
        .any(|f| f.severity == "high" && f.skill_name == "injector"));
    assert!(!target.exists());

    let skipped = InstallOptions {
        no_audit: true,
        ..opts(&target)
    };
    let done = tap_ops::install(&env, "@acme/audit", &skipped).unwrap();
    assert!(!done.blocked && done.findings.is_empty() && !done.audited);
    assert_eq!(done.skills.len(), 2);
    assert!(target.join("skills/injector/SKILL.md").exists());
}

#[test]
fn install_dry_run_writes_nothing_for_a_known_tap() {
    if !git_available() {
        return;
    }
    let t = Tmp::new("installdry");
    let remotes = published(&t, &["acme/skills", "acme/audit"]);
    let config = t.config();
    let git = remotes.rewrite();
    let env = env(&config, &git);
    tap_ops::add(&env, "acme/skills", None, false).unwrap();
    tap_ops::add(&env, "acme/audit", None, false).unwrap();
    let target = t.target();
    fs::create_dir_all(target.join("skills/terraform-helper")).unwrap();
    fs::write(target.join("skills/terraform-helper/SKILL.md"), "mine").unwrap();
    let before = tree_snapshot(&t.0);

    let dry = InstallOptions {
        dry_run: true,
        ..opts(&target)
    };
    let report = tap_ops::install(&env, "@acme/skills", &dry).unwrap();
    assert_eq!(
        names(&report),
        [
            ("code-review".to_string(), true),
            ("terraform-helper".to_string(), false),
            ("style-guide".to_string(), true),
        ]
    );
    assert_eq!(report.skills[0].files, 3);
    let blocked = tap_ops::install(&env, "@acme/audit", &dry).unwrap();
    assert!(blocked.blocked);
    assert_eq!(tree_snapshot(&t.0), before);
}

#[test]
fn install_dry_run_for_an_unknown_tap_reports_the_clone_and_touches_nothing() {
    let t = Tmp::new("installunknown");
    let config = t.config();
    let git = MockGit::with_head("abc");
    let env = env(&config, &git);
    let target = t.target();
    let before = tree_snapshot(&t.0);
    let dry = InstallOptions {
        dry_run: true,
        ..opts(&target)
    };
    let report = tap_ops::install(&env, "@acme/skills", &dry).unwrap();
    assert!(report.tap_missing && !report.tap_added);
    assert_eq!(report.clone_url, "https://github.com/acme/skills.git");
    assert!(git.calls.borrow().is_empty());
    assert_eq!(tree_snapshot(&t.0), before);
}

#[test]
fn a_registered_tap_without_its_clone_is_reported_not_recloned() {
    let t = Tmp::new("noclone");
    let config = t.config();
    let git = MockGit::with_head("abc");
    let env = env(&config, &git);
    tap_ops::add(&env, "acme/skills", None, false).unwrap();
    fs::remove_dir_all(config.join("taps/acme-skills")).unwrap();
    let error = tap_ops::install(&env, "@acme/skills", &opts(&t.target())).unwrap_err();
    assert_eq!(error.kind, Kind::NotFound);
    assert!(
        error
            .message
            .contains("clone of tap 'acme-skills' is missing"),
        "{}",
        error.message
    );
    assert_eq!(git.calls.borrow().len(), 2);
}
