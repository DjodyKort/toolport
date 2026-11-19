//! A restore reads a setup file that may come from anywhere. Each test builds a hostile file with
//! correct hashes, restores it into a synthetic home and proves that nothing is written outside
//! the places a restore owns and that the refused entries are reported.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::export;
use super::parts::{Entry, Part};
use super::restore::{self, RestoreOptions};
use super::tests::{options, World};
use super::zipio::StreamZip;
use super::{PartId, FORMAT, MANIFEST, VERSION};

type Snapshot = BTreeMap<String, Option<Vec<u8>>>;
type Entries = Vec<(String, Vec<u8>)>;

fn text(name: &str, body: &str) -> (String, Vec<u8>) {
    (name.to_string(), body.as_bytes().to_vec())
}

fn json_entry(name: &str, value: &Value) -> (String, Vec<u8>) {
    (name.to_string(), serde_json::to_vec_pretty(value).unwrap())
}

fn manifest_key(id: PartId) -> (&'static str, &'static str) {
    match id {
        PartId::Repos => ("file", "repos.json"),
        PartId::Settings => ("file", "settings.json"),
        PartId::ClaudeUser => ("dir", "claude-user/"),
        PartId::Memory => ("dir", "memory/"),
        PartId::Secrets => ("file", "secrets.enc"),
        other => panic!("not needed by these tests: {other:?}"),
    }
}

/// A setup file whose parts hold exactly these entries, hashed the way an export hashes them.
fn setup_file(world: &World, name: &str, parts: Vec<(PartId, Entries)>) -> PathBuf {
    let path = world.out_dir().join(name);
    let mut zip = StreamZip::create(&path).unwrap();
    let mut rows = Vec::new();
    for (id, entries) in &parts {
        let mut part = Part::new(*id);
        part.entries = entries
            .iter()
            .map(|(name, bytes)| Entry::bytes(name, bytes.clone()))
            .collect();
        part.count = entries.len() as u64;
        let (key, file) = manifest_key(*id);
        let mut row = json!({
            "id": id.id(),
            key: file,
            "sha256": part.hash().unwrap(),
            "bytes": part.bytes(),
            "count": part.count,
        });
        if id.sealed() {
            row["encrypted"] = json!(true);
        }
        rows.push(row);
    }
    let manifest = json!({"format": FORMAT, "version": VERSION, "parts": rows});
    zip.add(MANIFEST, &serde_json::to_vec_pretty(&manifest).unwrap())
        .unwrap();
    for (_, entries) in &parts {
        for (name, bytes) in entries {
            zip.add(name, bytes).unwrap();
        }
    }
    zip.finish().unwrap();
    path
}

fn memory_part(repos: Value, switches: Value, extra: Entries) -> Entries {
    let mut entries = vec![json_entry(
        "memory/index.json",
        &json!({"version": 1, "repos": repos, "switches": switches}),
    )];
    entries.extend(extra);
    entries
}

fn memory_repo(dir: &str, path: &str, instructions: &[&str]) -> Value {
    json!({
        "dir": dir, "key": dir, "name": dir, "remote": null, "path": path,
        "files": [], "instructions": instructions, "flagged": [],
    })
}

fn instruction_entries(dir: &str, names: &[&str]) -> Entries {
    names
        .iter()
        .map(|name| {
            text(
                &format!("memory/{dir}/instructions/{name}"),
                "from the file\n",
            )
        })
        .collect()
}

fn plan_and_apply(to: &World, file: &Path) -> (Value, Value) {
    let dry = restore::run(
        &to.ctx(),
        &RestoreOptions {
            file: file.into(),
            ..Default::default()
        },
    )
    .unwrap();
    let applied = restore::run(
        &to.ctx(),
        &RestoreOptions {
            file: file.into(),
            apply: true,
            plan: Some(dry["plan"].as_str().unwrap().into()),
            ..Default::default()
        },
    )
    .unwrap();
    (dry, applied)
}

fn refused(report: &Value) -> Vec<String> {
    let mut ids: Vec<String> = report["refused"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| r["id"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

/// The files that differ from the snapshot, leaving out Toolport's own data folder (the undo
/// journal) and the folder the setup files are made in.
fn changed(world: &World, before: &Snapshot) -> Vec<String> {
    let after = world.snapshot();
    let mut names: Vec<String> = before
        .keys()
        .chain(after.keys())
        .filter(|k| before.get(*k) != after.get(*k))
        .filter(|k| !k.ends_with('/') && !k.starts_with("data/") && !k.starts_with("out/"))
        .cloned()
        .collect();
    names.sort();
    names.dedup();
    names
}

#[test]
fn test01_claude_user_entries_outside_the_known_places_are_refused_and_reported() {
    let to = World::new();
    to.claude("settings.json", "{\"mine\": true}\n");
    let file = setup_file(
        &to,
        "claude.zip",
        vec![(
            PartId::ClaudeUser,
            vec![
                text("claude-user/CLAUDE.md", "global rules\n"),
                text("claude-user/rules/a.md", "a rule\n"),
                text("claude-user/./settings.json", "{\"hooks\": \"evil\"}\n"),
                text("claude-user/rules/./b.md", "dot part\n"),
                text("claude-user/hooks/evil.sh", "#!/bin/sh\n"),
                text("claude-user/settings.local.json", "{}\n"),
                text("claude-user/.credentials.json", "{}\n"),
                text("claude-user/Rules/c.md", "wrong case\n"),
            ],
        )],
    );
    let before = to.snapshot();
    let (dry, applied) = plan_and_apply(&to, &file);
    assert_eq!(
        refused(&dry),
        [
            "claude:./settings.json",
            "claude:.credentials.json",
            "claude:Rules/c.md",
            "claude:hooks/evil.sh",
            "claude:rules/./b.md",
            "claude:settings.local.json",
        ]
    );
    assert_eq!(refused(&applied), refused(&dry));
    assert_eq!(
        changed(&to, &before),
        ["home/.claude/CLAUDE.md", "home/.claude/rules/a.md"]
    );
    assert_eq!(
        std::fs::read_to_string(to.home.join(".claude/settings.json")).unwrap(),
        "{\"mine\": true}\n"
    );
}

#[test]
fn test02_a_name_that_walks_up_or_is_absolute_damages_its_part_and_nothing_is_written() {
    let to = World::new();
    let outside = to.dir.path().join("outside.md");
    for (n, hostile) in [
        "claude-user/rules/../../../outside.md".to_string(),
        "claude-user//abs.md".to_string(),
        format!("claude-user/{}", outside.display()),
        "claude-user/rules\\x.md".to_string(),
    ]
    .into_iter()
    .enumerate()
    {
        let file = setup_file(
            &to,
            &format!("walk{n}.zip"),
            vec![
                (
                    PartId::Repos,
                    vec![json_entry("repos.json", &json!({"repos": []}))],
                ),
                (
                    PartId::ClaudeUser,
                    vec![
                        text("claude-user/CLAUDE.md", "fine\n"),
                        text(&hostile, "x\n"),
                    ],
                ),
            ],
        );
        let before = to.snapshot();
        let (dry, _) = plan_and_apply(&to, &file);
        let row = dry["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["part"] == "claudeUser")
            .unwrap();
        assert_eq!(row["state"], "skip", "{hostile}: {dry}");
        assert!(changed(&to, &before).is_empty(), "{hostile}");
    }
    assert!(!outside.exists());
}

#[test]
fn test03_instruction_names_are_limited_to_the_known_files_and_rules() {
    let to = World::new();
    let web = to.repo("web");
    let hooks = web.join(".git/hooks/pre-commit");
    let config_before = std::fs::read(web.join(".git/config")).unwrap();
    let names = [
        "CLAUDE.md",
        ".claude/rules/a.md",
        ".git/hooks/pre-commit",
        ".git/config",
        ".claude/settings.json",
        ".claude/settings.local.json",
        ".claude/rules/b.txt",
        "notes.txt",
        "./CLAUDE.local.md",
    ];
    let file = setup_file(
        &to,
        "memory.zip",
        vec![(
            PartId::Memory,
            memory_part(
                json!([memory_repo("web-1", "~/code/web", &names)]),
                json!([]),
                instruction_entries("web-1", &names),
            ),
        )],
    );
    let before = to.snapshot();
    let (dry, applied) = plan_and_apply(&to, &file);
    assert_eq!(
        refused(&dry),
        [
            "memory:web-1/instructions/./CLAUDE.local.md",
            "memory:web-1/instructions/.claude/rules/b.txt",
            "memory:web-1/instructions/.claude/settings.json",
            "memory:web-1/instructions/.claude/settings.local.json",
            "memory:web-1/instructions/.git/config",
            "memory:web-1/instructions/.git/hooks/pre-commit",
            "memory:web-1/instructions/notes.txt",
        ]
    );
    assert_eq!(refused(&applied), refused(&dry));
    assert_eq!(
        changed(&to, &before),
        [
            "home/code/web/.claude/rules/a.md",
            "home/code/web/CLAUDE.md"
        ]
    );
    assert!(!hooks.exists());
    assert_eq!(
        std::fs::read(web.join(".git/config")).unwrap(),
        config_before
    );
}

#[test]
fn test04_a_memory_path_must_be_a_git_repository_that_exists_or_is_cloned_here() {
    let to = World::new();
    let ssh = to.home.join(".ssh");
    to.put(&ssh.join("known_hosts"), "host key\n");
    to.repo("web");
    let names = ["CLAUDE.md"];
    let absolute = ssh.to_string_lossy().into_owned();
    let paths = [
        ("plain", "~/.ssh".to_string()),
        ("absolute", absolute),
        ("walks", "~/code/web/../../.ssh".to_string()),
        ("home", "~".to_string()),
    ];
    let mut repos = Vec::new();
    let mut entries = Vec::new();
    let mut switches = Vec::new();
    for (dir, path) in &paths {
        repos.push(memory_repo(dir, path, &names));
        entries.extend(instruction_entries(dir, &names));
        switches.push(json!({"path": path, "autoMemory": "off", "claudeMdExcludes": []}));
    }
    repos.push(memory_repo("gone", "~/code/nowhere", &names));
    entries.extend(instruction_entries("gone", &names));
    let file = setup_file(
        &to,
        "memory.zip",
        vec![(
            PartId::Memory,
            memory_part(json!(repos), json!(switches), entries),
        )],
    );
    let before = to.snapshot();
    let (dry, applied) = plan_and_apply(&to, &file);
    let mut want: Vec<String> = paths
        .iter()
        .flat_map(|(dir, path)| [format!("memory:{dir}"), format!("switch:{path}")])
        .collect();
    want.sort();
    assert_eq!(refused(&dry), want);
    assert_eq!(refused(&applied), want);
    assert!(changed(&to, &before).is_empty());
    assert!(!ssh.join("CLAUDE.md").exists());
    assert!(!to.home.join("CLAUDE.md").exists());
    let gone = dry["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "memory:gone")
        .unwrap();
    assert_eq!(gone["state"], "skip");
    assert!(gone.get("refused").is_none());
}

#[test]
fn test05_a_repository_cloned_in_this_run_takes_its_instruction_files() {
    let from = World::new();
    let web = from.repo("web");
    from.put(&web.join("CLAUDE.md"), "local rules\n");
    from.put(&web.join(".claude/rules/style.md"), "style\n");
    let data = export::run(&from.ctx(), &options(&from), None).unwrap();
    let file = PathBuf::from(data["file"].as_str().unwrap());
    let to = World::new();
    let (dry, applied) = plan_and_apply(&to, &file);
    assert!(refused(&dry).is_empty(), "{dry}");
    assert_eq!(applied["failed"], 0, "{applied}");
    let clone = to.home.join("code/web");
    assert_eq!(
        std::fs::read_to_string(clone.join("CLAUDE.md")).unwrap(),
        "local rules\n"
    );
    assert!(clone.join(".claude/rules/style.md").exists());
}

#[test]
fn test06_a_clone_that_fails_leaves_no_instruction_file_behind() {
    let to = World::new();
    let missing = to.dir.path().join("remotes/dead.git");
    let names = ["CLAUDE.md"];
    let file = setup_file(
        &to,
        "dead.zip",
        vec![
            (
                PartId::Repos,
                vec![json_entry(
                    "repos.json",
                    &json!({"repos": [{
                        "path": "~/code/dead", "name": "dead", "branch": "main",
                        "remotes": [{"name": "origin", "url": missing.to_string_lossy()}],
                    }]}),
                )],
            ),
            (
                PartId::Memory,
                memory_part(
                    json!([memory_repo("dead-1", "~/code/dead", &names)]),
                    json!([]),
                    instruction_entries("dead-1", &names),
                ),
            ),
        ],
    );
    let (_, applied) = plan_and_apply(&to, &file);
    assert_eq!(applied["failed"], 2, "{applied}");
    assert!(!to.home.join("code/dead").exists());
}

#[cfg(unix)]
#[test]
fn test07_a_symlink_on_the_way_to_a_destination_is_refused() {
    use std::os::unix::fs::symlink;

    let to = World::new();
    let web = to.repo("web");
    let outside = to.dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    to.put(&outside.join("precious.txt"), "keep\n");
    symlink(&outside, web.join(".claude")).unwrap();
    std::fs::create_dir_all(to.home.join(".claude")).unwrap();
    symlink(&outside, to.home.join(".claude/rules")).unwrap();
    symlink(
        outside.join("new.json"),
        to.home.join(".claude/keybindings.json"),
    )
    .unwrap();

    let names = ["CLAUDE.md", ".claude/rules/a.md"];
    let file = setup_file(
        &to,
        "links.zip",
        vec![
            (
                PartId::ClaudeUser,
                vec![
                    text("claude-user/rules/a.md", "through a linked folder\n"),
                    text("claude-user/keybindings.json", "through a linked file\n"),
                ],
            ),
            (
                PartId::Memory,
                memory_part(
                    json!([memory_repo("web-1", "~/code/web", &names)]),
                    json!([{"path": "~/code/web", "autoMemory": "off", "claudeMdExcludes": []}]),
                    instruction_entries("web-1", &names),
                ),
            ),
        ],
    );
    let before = to.snapshot();
    let (dry, applied) = plan_and_apply(&to, &file);
    assert_eq!(
        refused(&dry),
        [
            "claude:keybindings.json",
            "claude:rules/a.md",
            "memory:web-1/instructions/.claude/rules/a.md",
            "switch:~/code/web",
        ]
    );
    assert_eq!(refused(&applied), refused(&dry));
    assert_eq!(changed(&to, &before), ["home/code/web/CLAUDE.md"]);
    assert!(!outside.join("a.md").exists());
    assert!(!outside.join("new.json").exists());
    assert!(!outside.join("rules/a.md").exists());
    assert!(!outside.join("settings.local.json").exists());
    assert_eq!(
        std::fs::read_to_string(outside.join("precious.txt")).unwrap(),
        "keep\n"
    );
}

#[cfg(unix)]
#[test]
fn test08_a_link_created_after_the_plan_makes_the_plan_stale_and_writes_nothing() {
    use std::os::unix::fs::symlink;

    let to = World::new();
    let outside = to.dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let file = setup_file(
        &to,
        "late.zip",
        vec![(
            PartId::ClaudeUser,
            vec![text("claude-user/agents/a.md", "an agent\n")],
        )],
    );
    let dry = restore::run(
        &to.ctx(),
        &RestoreOptions {
            file: file.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(refused(&dry).is_empty());
    std::fs::create_dir_all(to.home.join(".claude")).unwrap();
    symlink(&outside, to.home.join(".claude/agents")).unwrap();
    let error = restore::run(
        &to.ctx(),
        &RestoreOptions {
            file,
            apply: true,
            plan: Some(dry["plan"].as_str().unwrap().into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        error.message.contains("changed since the plan"),
        "{}",
        error.message
    );
    assert!(!outside.join("a.md").exists());
}

#[test]
fn test09_bundle_names_cannot_leave_the_bundle_folder() {
    let to = World::new();
    let roots = crate::context::Roots::from_home(&to.home);
    let dir = crate::context::bundle_store::dir(&roots);
    let absolute = to.dir.path().join("abs-evil");
    let file = setup_file(
        &to,
        "bundles.zip",
        vec![(
            PartId::Settings,
            vec![json_entry(
                "settings.json",
                &json!({"registry": {}, "bundles": {
                    "ok": "name: ok\n",
                    "../evil": "x\n",
                    "..": "x\n",
                    "a/b": "x\n",
                    "a\\b": "x\n",
                    absolute.to_string_lossy(): "x\n",
                }}),
            )],
        )],
    );
    let before = to.snapshot();
    let (dry, _) = plan_and_apply(&to, &file);
    assert_eq!(refused(&dry).len(), 5, "{dry}");
    assert_eq!(
        changed(&to, &before),
        [format!("home/{}", {
            let rel = dir.join("ok.yaml");
            rel.strip_prefix(&to.home)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })]
    );
    assert!(!absolute.with_extension("yaml").exists());
    assert!(!dir.parent().unwrap().join("evil.yaml").exists());
}

#[test]
fn test10_a_clone_destination_and_memory_file_names_with_odd_parts_are_refused() {
    let to = World::new();
    let file = setup_file(
        &to,
        "odd.zip",
        vec![
            (
                PartId::Repos,
                vec![json_entry(
                    "repos.json",
                    &json!({"repos": [{
                        "path": "~/code/../.ssh/evil", "name": "evil", "branch": "main",
                        "remotes": [{"name": "origin", "url": "file:///nonexistent/x.git"}],
                    }]}),
                )],
            ),
            (
                PartId::Memory,
                memory_part(
                    json!([{
                        "dir": "web-1", "key": "k", "name": "web", "remote": null,
                        "path": "~/code/web", "files": ["ok.md", "sub/./x.md"],
                        "instructions": [], "flagged": [],
                    }]),
                    json!([]),
                    vec![
                        text("memory/web-1/ok.md", "fine\n"),
                        text("memory/web-1/sub/./x.md", "odd\n"),
                    ],
                ),
            ),
        ],
    );
    let web = to.repo("web");
    let before = to.snapshot();
    let (dry, applied) = plan_and_apply(&to, &file);
    assert_eq!(
        refused(&dry),
        ["memory:web-1/sub/./x.md", "repo:~/code/../.ssh/evil"]
    );
    assert_eq!(applied["failed"], 0, "{applied}");
    let written = changed(&to, &before);
    assert_eq!(written.len(), 1, "{written:?}");
    assert!(written[0].ends_with("memory/ok.md"), "{written:?}");
    assert!(!to.home.join(".ssh").exists());
    assert!(web.exists());
}

#[test]
fn test12_a_repository_is_cloned_only_below_the_home_folder() {
    let to = World::new();
    let outside = to.home.parent().unwrap().join("volume/web");
    let outside_text = outside.to_string_lossy().into_owned();
    let file = setup_file(
        &to,
        "outside.zip",
        vec![(
            PartId::Repos,
            vec![json_entry(
                "repos.json",
                &json!({"repos": [{
                    "path": outside_text, "name": "web", "branch": "main",
                    "remotes": [{"name": "origin", "url": "file:///nonexistent/x.git"}],
                }]}),
            )],
        )],
    );
    let before = to.snapshot();
    let (dry, applied) = plan_and_apply(&to, &file);
    assert_eq!(refused(&dry), [format!("repo:{outside_text}")]);
    assert_eq!(applied["failed"], 0, "{applied}");
    assert!(changed(&to, &before).is_empty());
    assert!(!outside.exists());
}

#[test]
fn test11_everything_an_export_writes_comes_back_through_the_lists() {
    let from = World::new();
    let web = from.repo("web");
    from.put(&web.join("CLAUDE.local.md"), "mine\n");
    from.put(&web.join(".claude/rules/deep/style.md"), "style\n");
    from.put(&web.join(".claude/rules/notes.txt"), "not a rule\n");
    for rel in [
        "CLAUDE.md",
        "keybindings.json",
        "rules/a.md",
        "rules/sub/deeper/b.md",
        "agents/x.md",
        "commands/y.md",
        "output-styles/z.md",
        "workflows/w.yaml",
    ] {
        from.claude(rel, &format!("{rel}\n"));
    }
    let data = export::run(&from.ctx(), &options(&from), None).unwrap();
    let file = PathBuf::from(data["file"].as_str().unwrap());
    let to = World::new();
    let (dry, applied) = plan_and_apply(&to, &file);
    assert!(refused(&dry).is_empty(), "{dry}");
    assert_eq!(applied["failed"], 0, "{applied}");
    for rel in [
        "CLAUDE.md",
        "keybindings.json",
        "rules/a.md",
        "rules/sub/deeper/b.md",
        "agents/x.md",
        "commands/y.md",
        "output-styles/z.md",
        "workflows/w.yaml",
    ] {
        assert!(to.home.join(".claude").join(rel).exists(), "{rel}");
    }
    let clone = to.home.join("code/web");
    assert!(clone.join("CLAUDE.local.md").exists());
    assert!(clone.join(".claude/rules/deep/style.md").exists());
    assert!(!clone.join(".claude/rules/notes.txt").exists());
}

#[cfg(feature = "cipher")]
mod sealed_claude_files {
    use std::path::PathBuf;

    use super::super::crypto::{self, Cost};
    use super::super::tests::{token, PASS};
    use super::super::PartId;
    use super::*;

    const CHEAP: Cost = Cost {
        memory_kib: 64,
        iterations: 1,
        lanes: 1,
    };

    fn hex_of(text: &str) -> String {
        super::super::hex(text.as_bytes())
    }

    fn secrets_entry(doc: &Value) -> (String, Vec<u8>) {
        crypto::TEST_COST.with(|cost| cost.set(Some(CHEAP)));
        let sealed = crypto::seal_bytes(PASS, &serde_json::to_vec(doc).unwrap()).unwrap();
        ("secrets.enc".to_string(), sealed)
    }

    fn with_passphrase(file: &Path, apply: Option<&str>) -> RestoreOptions {
        RestoreOptions {
            file: file.into(),
            passphrase: Some(PASS.into()),
            apply: apply.is_some(),
            plan: apply.map(Into::into),
            ..Default::default()
        }
    }

    fn plan_and_apply_unlocked(to: &World, file: &Path) -> (Value, Value) {
        let dry = restore::run(&to.ctx(), &with_passphrase(file, None)).unwrap();
        let hash = dry["plan"].as_str().unwrap().to_string();
        let applied = restore::run(&to.ctx(), &with_passphrase(file, Some(&hash))).unwrap();
        (dry, applied)
    }

    #[test]
    fn test12_files_the_scan_held_back_come_back_to_their_own_places_from_the_sealed_part() {
        crypto::TEST_COST.with(|cost| cost.set(Some(CHEAP)));
        let from = World::new();
        let web = from.repo("web");
        let leaky_global = format!("deploy with {}\n", token());
        let leaky_rule = format!("export KEY={}\n", token());
        let leaky_local = format!("token {}\n", token());
        from.claude("CLAUDE.md", &leaky_global);
        from.claude("rules/nested/deploy.md", &leaky_rule);
        from.claude("rules/style.md", "short answers\n");
        from.put(&web.join("AGENTS.md"), &leaky_local);
        from.put(&web.join(".claude/rules/key.md"), &leaky_rule);
        let mut opts = options(&from);
        opts.parts.insert(PartId::Secrets);
        let data = export::run(&from.ctx(), &opts, Some(PASS)).unwrap();
        let file = PathBuf::from(data["file"].as_str().unwrap());
        let bytes = std::fs::read(&file).unwrap();
        assert!(!bytes
            .windows(token().len())
            .any(|w| w == token().as_bytes()));

        let to = World::new();
        let (dry, applied) = plan_and_apply_unlocked(&to, &file);
        assert!(refused(&dry).is_empty(), "{dry}");
        assert_eq!(applied["failed"], 0, "{applied}");
        let claude = to.home.join(".claude");
        for (rel, want) in [
            ("CLAUDE.md", leaky_global.as_str()),
            ("rules/nested/deploy.md", leaky_rule.as_str()),
            ("rules/style.md", "short answers\n"),
        ] {
            assert_eq!(
                std::fs::read_to_string(claude.join(rel)).unwrap(),
                want,
                "{rel}"
            );
        }
        let clone = to.home.join("code/web");
        assert_eq!(
            std::fs::read_to_string(clone.join("AGENTS.md")).unwrap(),
            leaky_local
        );
        assert_eq!(
            std::fs::read_to_string(clone.join(".claude/rules/key.md")).unwrap(),
            leaky_rule
        );
    }

    #[test]
    fn test13_without_the_passphrase_the_held_back_files_stay_in_the_file() {
        crypto::TEST_COST.with(|cost| cost.set(Some(CHEAP)));
        let from = World::new();
        from.repo("web");
        from.claude("CLAUDE.md", &format!("deploy with {}\n", token()));
        let mut opts = options(&from);
        opts.parts.insert(PartId::Secrets);
        let data = export::run(&from.ctx(), &opts, Some(PASS)).unwrap();
        let file = PathBuf::from(data["file"].as_str().unwrap());
        let to = World::new();
        let before = to.snapshot();
        let (dry, _) = plan_and_apply(&to, &file);
        assert!(!dry["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "claude:CLAUDE.md"));
        assert!(!to.home.join(".claude/CLAUDE.md").exists());
        assert!(!changed(&to, &before)
            .iter()
            .any(|f| f.ends_with("CLAUDE.md")));
    }

    #[test]
    fn test14_a_sealed_entry_outside_the_lists_is_refused_and_nothing_else_is_touched() {
        let to = World::new();
        to.repo("web");
        let outside = to.dir.path().join("outside.md");
        let ok = hex_of("fine\n");
        let claude_names = [
            "CLAUDE.md",
            "rules/ok.md",
            "settings.json",
            "settings.local.json",
            ".credentials.json",
            "hooks/evil.sh",
            "rules/./dot.md",
            "../escape.md",
            "/abs.md",
        ];
        let instruction_names = [
            "CLAUDE.md",
            ".git/hooks/pre-commit",
            ".git/config",
            ".claude/settings.json",
            "notes.txt",
            "../CLAUDE.md",
        ];
        let doc = json!({
            "version": 1,
            "values": [],
            "memoryFiles": [],
            "claudeFiles": claude_names
                .iter()
                .map(|n| json!({"name": n, "hex": ok}))
                .collect::<Vec<_>>(),
            "instructionFiles": instruction_names
                .iter()
                .map(|n| json!({"dir": "web-1", "name": n, "hex": ok}))
                .chain([json!({"dir": "elsewhere", "name": "CLAUDE.md", "hex": ok})])
                .collect::<Vec<_>>(),
        });
        let file = setup_file(
            &to,
            "sealed.zip",
            vec![
                (
                    PartId::ClaudeUser,
                    vec![text("claude-user/rules/plain.md", "plain\n")],
                ),
                (
                    PartId::Memory,
                    memory_part(
                        json!([memory_repo("web-1", "~/code/web", &[])]),
                        json!([]),
                        Vec::new(),
                    ),
                ),
                (PartId::Secrets, vec![secrets_entry(&doc)]),
            ],
        );
        let hooks = to.home.join("code/web/.git/hooks/pre-commit");
        let config_before = std::fs::read(to.home.join("code/web/.git/config")).unwrap();
        let before = to.snapshot();
        let (dry, applied) = plan_and_apply_unlocked(&to, &file);
        let mut want: Vec<String> = [
            "claude:.credentials.json",
            "claude:../escape.md",
            "claude:/abs.md",
            "claude:hooks/evil.sh",
            "claude:rules/./dot.md",
            "claude:settings.json",
            "claude:settings.local.json",
            "memory:web-1/instructions/.claude/settings.json",
            "memory:web-1/instructions/.git/config",
            "memory:web-1/instructions/.git/hooks/pre-commit",
            "memory:web-1/instructions/../CLAUDE.md",
            "memory:web-1/instructions/notes.txt",
            "memory:elsewhere/instructions/CLAUDE.md",
        ]
        .map(String::from)
        .to_vec();
        want.sort();
        assert_eq!(refused(&dry), want);
        assert_eq!(refused(&applied), want);
        assert_eq!(applied["failed"], 0, "{applied}");
        assert_eq!(
            changed(&to, &before),
            [
                "home/.claude/CLAUDE.md",
                "home/.claude/rules/ok.md",
                "home/.claude/rules/plain.md",
                "home/code/web/CLAUDE.md"
            ]
        );
        assert!(!hooks.exists());
        assert!(!outside.exists());
        assert_eq!(
            std::fs::read(to.home.join("code/web/.git/config")).unwrap(),
            config_before
        );
    }
}
