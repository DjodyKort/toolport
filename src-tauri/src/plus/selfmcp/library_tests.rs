use super::*;
use crate::plus::direct::tests::world;
use serde_json::json;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(["-c", "commit.gpgsign=false", "-c", "protocol.file.allow=always"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn library(home: &Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let bare = home.join("remotes/library.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    let lib = home.join(".config/mcpm/skills_repo");
    std::fs::create_dir_all(lib.join("skills/notes-helper")).unwrap();
    std::fs::write(
        lib.join("skills/notes-helper/SKILL.md"),
        "---\nname: notes-helper\ndescription: d\n---\nbody\n",
    )
    .unwrap();
    git(&lib, &["init", "-q", "-b", "main"]);
    git(&lib, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&lib, &["add", "-A"]);
    git(&lib, &["commit", "-q", "-m", "seed"]);
    git(&lib, &["push", "-q", "-u", "origin", "main"]);
    let other = home.join(".config/mcpm/library_copy");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    git(home, &["clone", "-q", bare.to_str().unwrap(), other.to_str().unwrap()]);
    (bare, lib, other)
}

fn advance(other: &Path) {
    std::fs::write(other.join("news.md"), "new\n").unwrap();
    git(other, &["add", "-A"]);
    git(other, &["commit", "-q", "-m", "news"]);
    git(other, &["push", "-q", "origin", "main"]);
}

#[test]
fn library_status_reads_without_the_network_unless_fetch_is_passed() {
    world(|w| {
        for (name, tier) in [("library_status", 1), ("library_pull", 2)] {
            let tool = find_tool(name).unwrap();
            assert_eq!((tool.tier, tool.gate), (tier, Gate::None), "{name}");
        }
        let (_, lib, other) = library(&w.home);
        advance(&other);
        let quiet = call_tool("library_status", &json!({})).unwrap();
        assert_eq!((quiet["behind"].as_u64(), quiet["ahead"].as_u64()), (Some(0), Some(0)));
        assert_eq!(quiet["fetch"]["requested"], false);
        assert_eq!(quiet["duplicateClones"].as_array().unwrap().len(), 1);
        let fetched = call_tool("library_status", &json!({"fetch": true})).unwrap();
        assert_eq!(fetched["behind"], 1, "{}", fetched["fetch"]);
        assert_eq!(fetched["auth"]["ok"], true);
        assert_eq!(fetched["repo"], lib.to_string_lossy().to_string());
    });
}

#[test]
fn library_pull_previews_by_default_and_fast_forwards_only_when_applied() {
    world(|w| {
        let schema = catalog::input_schema(find_tool("library_pull").unwrap());
        assert!(schema["properties"]["dry_run"]["description"].as_str().unwrap().contains("true unless"));
        let (_, lib, other) = library(&w.home);
        advance(&other);
        let before = git(&lib, &["rev-parse", "HEAD"]);
        let plan = call_tool("library_pull", &json!({})).unwrap();
        assert_eq!(plan["dryRun"], true);
        assert_eq!(plan["plan"]["summary"], "Fast-forward 1 commit(s) from origin/main");
        assert_eq!(git(&lib, &["rev-parse", "HEAD"]), before);

        std::fs::write(lib.join("local.md"), "mine\n").unwrap();
        let refused = call_tool("library_pull", &json!({"dry_run": false})).unwrap_err();
        assert_eq!(refused.kind, "refused");
        std::fs::remove_file(lib.join("local.md")).unwrap();

        let done = call_tool("library_pull", &json!({"dry_run": false})).unwrap();
        assert_eq!(done["pulled"], true);
        assert_eq!(git(&lib, &["rev-parse", "HEAD"]), git(&other, &["rev-parse", "HEAD"]));
    });
}
