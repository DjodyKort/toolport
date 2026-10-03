use super::clock::{FixedClock, Instant};
use super::collisions::{detect_collisions, resolve_mode, ResolutionMode};
use super::json::{self, J};
use super::lock::{LockEntry, LockFile};
use super::parser::{self, Activation, SkillType};
use super::sync::{sync_skills, SyncOptions};
use super::transpiler::inject_managed_block;
use super::*;
use std::fs;
use std::path::{Path, PathBuf};

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("skills-core-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn put(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn skill_md(name: &str, extra: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Does {name}\n{extra}---\n# {name}\n\nBody of {name}.\n"
    )
}

struct Identity;

impl Transpiler for Identity {
    fn client_key(&self) -> &str {
        "identity"
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!("{}\n", skill.body),
            warnings: Vec::new(),
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        match skill.skill_type {
            SkillType::Rule => root
                .join(".identity/rules")
                .join(format!("{}.md", skill.name())),
            SkillType::Skill => root
                .join(".identity/skills")
                .join(skill.name())
                .join("SKILL.md"),
        }
    }

    fn get_collision_paths(&self, skill: &Skill, root: &Path) -> Vec<PathBuf> {
        vec![root
            .join(".identity/legacy")
            .join(format!("{}.md", skill.name()))]
    }
}

fn registry() -> TranspilerRegistry {
    let mut r = TranspilerRegistry::new();
    r.register(Box::new(Identity));
    r
}

fn clock() -> FixedClock {
    FixedClock(Instant {
        unix_secs: 1_767_225_600,
        micros: 0,
    })
}

fn opts<'a>(root: &Path, clock: &'a FixedClock) -> SyncOptions<'a> {
    SyncOptions {
        output_root: root.to_path_buf(),
        lock_dir: root.to_path_buf(),
        global_mode: false,
        dry_run: false,
        migrate: None,
        client_keys: None,
        clock,
    }
}

#[test]
fn frontmatter_splits_and_normalises_hyphenated_keys() {
    let src = "---\nname: demo\ndescription: A demo\nallowed-tools: Read Grep\n---\n\n# Title\n\ntext\n\n";
    let (fm, body) = parser::parse_frontmatter(src).unwrap();
    assert_eq!(body, "# Title\n\ntext");
    assert!(fm.iter().any(|(k, _)| k == "allowed_tools"));
    let built = parser::build_frontmatter(&fm).unwrap();
    assert_eq!(built.allowed_tools.as_deref(), Some("Read Grep"));
    assert_eq!(built.activation, Activation::Auto);
    assert_eq!(built.priority, 0);
}

#[test]
fn missing_frontmatter_returns_whole_content() {
    let (fm, body) = parser::parse_frontmatter("# just text\n").unwrap();
    assert!(fm.is_empty());
    assert_eq!(body, "# just text\n");
}

#[test]
fn frontmatter_stops_at_first_dash_run() {
    let (_, body) =
        parser::parse_frontmatter("---\nname: a\ndescription: b\n---\nx\n---\ny\n").unwrap();
    assert_eq!(body, "x\n---\ny");
}

#[test]
fn validation_rejects_bad_names_and_lengths() {
    let build = |yaml: &str| {
        let (fm, _) = parser::parse_frontmatter(&format!("---\n{yaml}\n---\nb")).unwrap();
        parser::build_frontmatter(&fm)
    };
    assert!(build("name: Bad\ndescription: d").is_err());
    assert!(build("name: -bad\ndescription: d").is_err());
    assert!(build("name: a--b\ndescription: d").is_err());
    assert!(build("name: ok\ndescription: ''").is_err());
    assert!(build("name: ok").is_err());
    assert!(build("name: ok\ndescription: d\nactivation: sometimes").is_err());
    assert!(build("name: ok\ndescription: d\nglobs: [a, b]").is_err());
    let long = "x".repeat(1025);
    assert!(build(&format!("name: ok\ndescription: {long}")).is_err());
    let ok = build("name: ok\ndescription: d\nactivation: manual\npriority: 3").unwrap();
    assert_eq!(ok.activation, Activation::Manual);
    assert_eq!(ok.priority, 3);
}

#[test]
fn hooks_must_be_relative_and_exist_inside_the_skill() {
    let t = Tmp::new("hooks");
    put(
        &t.0,
        "skills/a/SKILL.md",
        &skill_md("a", "hooks:\n  PreCompact:\n    command: scripts/h.sh\n"),
    );
    assert!(discover_skills(&t.0).is_empty());
    put(&t.0, "skills/a/scripts/h.sh", "#!/bin/sh\n");
    let found = discover_skills(&t.0);
    assert_eq!(found.len(), 1);
    let hooks = found[0].frontmatter.hooks.as_ref().unwrap();
    assert_eq!(hooks[0].1.matcher, "*");
    put(
        &t.0,
        "skills/b/SKILL.md",
        &skill_md("b", "hooks:\n  Stop:\n    command: ../a/scripts/h.sh\n"),
    );
    put(
        &t.0,
        "skills/c/SKILL.md",
        &skill_md("c", "hooks:\n  Stop:\n    command: /bin/sh\n"),
    );
    let report = parser::discover_skills_report(&t.0);
    assert_eq!(report.skills.len(), 1);
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn discovery_is_sorted_skills_then_rules_and_infers_type() {
    let t = Tmp::new("discover");
    put(&t.0, "skills/zeta/SKILL.md", &skill_md("zeta", ""));
    put(
        &t.0,
        "skills/alpha/SKILL.md",
        &skill_md("alpha", "activation: always\n"),
    );
    put(&t.0, "skills/Beta/SKILL.md", &skill_md("beta", ""));
    put(&t.0, "skills/empty/readme.md", "x");
    put(&t.0, "skills/broken/SKILL.md", "no frontmatter");
    put(&t.0, "skills/loose.md", "x");
    put(&t.0, "rules/style/SKILL.md", &skill_md("style", ""));
    let report = parser::discover_skills_report(&t.0);
    let names: Vec<_> = report.skills.iter().map(|s| s.name().to_string()).collect();
    assert_eq!(names, ["beta", "alpha", "zeta", "style"]);
    let types: Vec<_> = report.skills.iter().map(|s| s.skill_type).collect();
    assert_eq!(
        types,
        [
            SkillType::Skill,
            SkillType::Rule,
            SkillType::Skill,
            SkillType::Rule
        ]
    );
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn asset_allowlist_matches_mcpm_quirks() {
    let t = Tmp::new("assets");
    let d = t.0.join("skills/lab");
    for rel in [
        "SKILL.md",
        "README.md",
        "extra/out.md",
        "modules/a.md",
        "modules/UPPER.MD",
        "modules/page.html",
        "modules/deep/nested/n.txt",
        "templates/.hidden.md",
        "templates/.hidden",
        "templates/noext",
        "scripts/run.SH",
        "scripts/x.rb",
        "assets/p.webp",
        "reference/c.yml",
        "reference/c.pdf",
    ] {
        put(&d, rel, "x");
    }
    let found: Vec<String> = with_asset_policy(AssetPolicy::Mcpm, || discover_assets(&d))
        .iter()
        .map(|p| assets::rel_string(p))
        .collect();
    assert_eq!(
        found,
        [
            "assets/p.webp",
            "modules/UPPER.MD",
            "modules/a.md",
            "modules/deep/nested/n.txt",
            "reference/c.yml",
            "scripts/run.SH",
            "templates/.hidden.md",
        ]
    );
}

#[test]
fn default_allowlist_adds_html_csv_js_but_not_zip() {
    let t = Tmp::new("assets-d027");
    let d = t.0.join("skills/lab");
    for rel in [
        "templates/desk.html",
        "reference/t.CSV",
        "scripts/app.js",
        "modules/bundle.zip",
        "modules/x.rb",
    ] {
        put(&d, rel, "x");
    }
    let found: Vec<String> = discover_assets(&d)
        .iter()
        .map(|p| assets::rel_string(p))
        .collect();
    assert_eq!(
        found,
        ["reference/t.CSV", "scripts/app.js", "templates/desk.html"]
    );
}

#[test]
fn extra_extensions_are_user_configurable() {
    assert_eq!(
        assets::parse_extensions(" ZIP, .Rb ,,. pdf\t.toml"),
        [".zip", ".rb", ".pdf", ".toml"]
    );
    let t = Tmp::new("assets-extra");
    let d = t.0.join("skills/lab");
    for rel in ["modules/bundle.zip", "modules/x.rb", "modules/a.md"] {
        put(&d, rel, "x");
    }
    let found = with_asset_policy(AssetPolicy::Extra(assets::parse_extensions("zip")), || {
        discover_assets(&d)
    });
    let found: Vec<String> = found.iter().map(|p| assets::rel_string(p)).collect();
    assert_eq!(found, ["modules/a.md", "modules/bundle.zip"]);
}

#[test]
fn sync_copies_templates_desk_html_to_the_client_skill_dir() {
    let t = Tmp::new("desk");
    put(&t.0, "skills/reviewdesk/SKILL.md", &skill_md("reviewdesk", ""));
    put(&t.0, "skills/reviewdesk/templates/desk.html", "<html></html>");
    let clock = clock();
    let res = sync_skills(&discover_skills(&t.0), &registry(), &opts(&t.0, &clock)).unwrap();
    assert_eq!(
        fs::read_to_string(t.0.join(".identity/skills/reviewdesk/templates/desk.html")).unwrap(),
        "<html></html>"
    );
    assert!(res.lockfile.skills[0].1.output_files[0]
        .1
        .contains(&".identity/skills/reviewdesk/templates/desk.html".to_string()));
}

#[test]
fn hash_covers_skill_md_then_sorted_assets() {
    let t = Tmp::new("hash");
    put(&t.0, "skills/h/SKILL.md", &skill_md("h", ""));
    let load = || discover_skills(&t.0).remove(0);
    let base = compute_skill_hash(&load()).unwrap();
    assert!(base.starts_with("sha256:") && base.len() == 7 + 16);
    put(&t.0, "skills/h/modules/a.md", "one");
    let with_asset = compute_skill_hash(&load()).unwrap();
    assert_ne!(base, with_asset);
    put(&t.0, "skills/h/modules/page.rb", "ignored");
    assert_eq!(with_asset, compute_skill_hash(&load()).unwrap());
    put(&t.0, "skills/h/modules/a.md", "two");
    assert_ne!(with_asset, compute_skill_hash(&load()).unwrap());

    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"SKILL.md\n");
    h.update(skill_md("h", "").as_bytes());
    h.update(b"\n---\nmodules/a.md\ntwo\n---\n");
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        compute_skill_hash(&load()).unwrap(),
        format!("sha256:{}", &hex[..16])
    );
}

#[test]
fn json_printer_matches_python_dumps() {
    let v = J::Obj(vec![
        ("a".into(), J::Arr(vec![])),
        ("b".into(), J::Obj(vec![])),
        (
            "c".into(),
            J::Arr(vec![
                J::str("é\u{1F600}\n\"\\\u{7f}\u{1}"),
                J::Null,
                J::Bool(true),
                J::int(-3),
            ]),
        ),
    ]);
    let want = "{\n  \"a\": [],\n  \"b\": {},\n  \"c\": [\n    \"\\u00e9\\ud83d\\ude00\\n\\\"\\\\\u{7f}\\u0001\",\n    null,\n    true,\n    -3\n  ]\n}";
    assert_eq!(v.dumps(), want);
    assert_eq!(json::parse(want).unwrap(), v);
}

#[test]
fn json_parse_keeps_order_and_last_duplicate() {
    let v = json::parse(r#"{"z":1,"a":2,"z":3}"#).unwrap();
    assert_eq!(
        v,
        J::Obj(vec![
            ("z".into(), J::Num("3".into())),
            ("a".into(), J::Num("2".into()))
        ])
    );
    assert!(json::parse("{").is_err());
    assert!(json::parse("[1] x").is_err());
}

fn sample_lock() -> LockFile {
    let mut lock = LockFile::new("2026-01-01T00:00:00+00:00".into());
    lock.scope = "project".into();
    lock.output_root = "/r".into();
    let mut e = LockEntry::new(Some("1.2".into()), "sha256:0123456789abcdef".into());
    e.clients_synced.push("identity".into());
    e.warnings.push("w".into());
    e.push_output("identity", ".identity/skills/a/SKILL.md".into());
    e.set_hooks("identity", vec!["/h".into()]);
    lock.skills.push(("a".into(), e));
    lock.rules.push((
        "r".into(),
        LockEntry::new(None, "sha256:ffffffffffffffff".into()),
    ));
    lock.active_styles.push(("cursor".into(), "terse".into()));
    lock
}

const SAMPLE_LOCK_TEXT: &str = r#"{
  "version": 1,
  "synced_at": "2026-01-01T00:00:00+00:00",
  "scope": "project",
  "output_root": "/r",
  "skills": {
    "a": {
      "source": "local",
      "version": "1.2",
      "hash": "sha256:0123456789abcdef",
      "clients_synced": [
        "identity"
      ],
      "warnings": [
        "w"
      ],
      "output_files": {
        "identity": [
          ".identity/skills/a/SKILL.md"
        ]
      },
      "hooks_installed": {
        "identity": [
          "/h"
        ]
      }
    }
  },
  "rules": {
    "r": {
      "source": "local",
      "version": null,
      "hash": "sha256:ffffffffffffffff",
      "clients_synced": [],
      "warnings": [],
      "output_files": {},
      "hooks_installed": {}
    }
  },
  "agents": {},
  "styles": {},
  "active_styles": {
    "cursor": "terse"
  }
}"#;

#[test]
fn lockfile_bytes_match_mcpm_layout_and_roundtrip() {
    let lock = sample_lock();
    assert_eq!(lock.serialize(), SAMPLE_LOCK_TEXT);
    assert_eq!(LockFile::parse(SAMPLE_LOCK_TEXT).unwrap(), lock);
}

#[test]
fn lockfile_load_rejects_invalid_and_defaults_missing() {
    assert!(LockFile::parse("not json").is_none());
    assert!(LockFile::parse(r#"{"skills":{"a":{"source":"x"}}}"#).is_none());
    let minimal = LockFile::parse(r#"{"skills":{"a":{"hash":"h"}},"extra":1}"#).unwrap();
    assert_eq!(minimal.version, 1);
    assert_eq!(minimal.skills[0].1.source, "local");
}

#[test]
fn lockfile_save_is_atomic_and_private() {
    let t = Tmp::new("lockperm");
    save_lockfile(&t.0, &sample_lock()).unwrap();
    assert_eq!(
        fs::read_to_string(t.0.join(LOCKFILE_NAME)).unwrap(),
        SAMPLE_LOCK_TEXT
    );
    let leftovers: Vec<_> = fs::read_dir(&t.0).unwrap().flatten().collect();
    assert_eq!(leftovers.len(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(t.0.join(LOCKFILE_NAME))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0);
    }
    assert_eq!(load_lockfile(&t.0).unwrap(), sample_lock());
}

#[test]
fn clock_formats_like_python() {
    let c = Instant {
        unix_secs: 1_767_225_600,
        micros: 0,
    };
    assert_eq!(c.isoformat(), "2026-01-01T00:00:00+00:00");
    assert_eq!(c.backup_stamp(), "20260101T000000Z");
    let c = Instant {
        unix_secs: 1_709_251_199,
        micros: 5,
    };
    assert_eq!(c.isoformat(), "2024-02-29T23:59:59.000005+00:00");
    assert_eq!(
        Instant {
            unix_secs: 0,
            micros: 0
        }
        .isoformat(),
        "1970-01-01T00:00:00+00:00"
    );
}

#[test]
fn sync_writes_outputs_assets_and_lock_entries() {
    let t = Tmp::new("sync");
    put(
        &t.0,
        "skills/a/SKILL.md",
        &skill_md("a", "metadata:\n  version: '2.0'\n"),
    );
    put(&t.0, "skills/a/modules/m.md", "m");
    put(&t.0, "skills/a/modules/skip.rb", "x");
    put(&t.0, "rules/r/SKILL.md", &skill_md("r", ""));
    let skills = discover_skills(&t.0);
    let clock = clock();
    let res = sync_skills(&skills, &registry(), &opts(&t.0, &clock)).unwrap();
    assert!(t.0.join(".identity/skills/a/SKILL.md").is_file());
    assert!(t.0.join(".identity/skills/a/modules/m.md").is_file());
    assert!(!t.0.join(".identity/skills/a/modules/skip.rb").exists());
    assert!(t.0.join(".identity/rules/r.md").is_file());
    let a = &res.lockfile.skills[0].1;
    assert_eq!(a.version.as_deref(), Some("2.0"));
    assert_eq!(
        a.output_files[0].1,
        [
            ".identity/skills/a/SKILL.md",
            ".identity/skills/a/modules/m.md"
        ]
    );
    assert_eq!(
        res.lockfile.rules[0].1.output_files[0].1,
        [".identity/rules/r.md"]
    );
    assert_eq!(res.lockfile.synced_at, "2026-01-01T00:00:00+00:00");
    assert_eq!(res.lockfile.scope, "project");
}

#[test]
fn second_sync_deletes_only_lock_recorded_files_and_prunes_dirs() {
    let t = Tmp::new("stale");
    put(&t.0, "skills/a/SKILL.md", &skill_md("a", ""));
    put(&t.0, "skills/b/SKILL.md", &skill_md("b", ""));
    put(&t.0, "skills/b/modules/m.md", "m");
    let clock = clock();
    let first = sync_skills(&discover_skills(&t.0), &registry(), &opts(&t.0, &clock)).unwrap();
    save_lockfile(&t.0, &first.lockfile).unwrap();
    put(&t.0, ".identity/skills/b/user-note.txt", "keep me");
    put(&t.0, ".identity/skills/a/user-note.txt", "keep me too");
    fs::remove_dir_all(t.0.join("skills/b")).unwrap();
    let second = sync_skills(&discover_skills(&t.0), &registry(), &opts(&t.0, &clock)).unwrap();
    let mut cleaned: Vec<_> = second
        .cleaned
        .iter()
        .map(|p| p.strip_prefix(&t.0).unwrap().to_string_lossy().into_owned())
        .collect();
    cleaned.sort();
    assert_eq!(
        cleaned,
        [
            ".identity/skills/b/SKILL.md",
            ".identity/skills/b/modules/m.md"
        ]
    );
    assert!(t.0.join(".identity/skills/b/user-note.txt").is_file());
    assert!(t.0.join(".identity/skills/a/SKILL.md").is_file());

    fs::remove_file(t.0.join(".identity/skills/b/user-note.txt")).unwrap();
    put(&t.0, "skills/b/SKILL.md", &skill_md("b", ""));
    let third = sync_skills(&discover_skills(&t.0), &registry(), &opts(&t.0, &clock)).unwrap();
    save_lockfile(&t.0, &third.lockfile).unwrap();
    fs::remove_dir_all(t.0.join("skills/b")).unwrap();
    let fourth = sync_skills(&discover_skills(&t.0), &registry(), &opts(&t.0, &clock)).unwrap();
    assert_eq!(fourth.cleaned.len(), 1);
    assert!(!t.0.join(".identity/skills/b").exists());
}

#[test]
fn tampered_lock_cannot_delete_outside_output_root() {
    let t = Tmp::new("traversal");
    let outside = t.0.join("outside.txt");
    fs::write(&outside, "x").unwrap();
    let root = t.0.join("root");
    fs::create_dir_all(&root).unwrap();
    let mut lock = LockFile::new("t".into());
    let mut e = LockEntry::new(None, "h".into());
    e.push_output("identity", "../outside.txt".into());
    lock.skills.push(("gone".into(), e));
    save_lockfile(&root, &lock).unwrap();
    let clock = clock();
    let res = sync_skills(&[], &registry(), &opts(&root, &clock)).unwrap();
    assert!(res.cleaned.is_empty());
    assert!(outside.is_file());
}

#[test]
fn dry_run_reports_stale_without_touching_disk() {
    let t = Tmp::new("dry");
    put(&t.0, "skills/a/SKILL.md", &skill_md("a", ""));
    let clock = clock();
    let first = sync_skills(&discover_skills(&t.0), &registry(), &opts(&t.0, &clock)).unwrap();
    save_lockfile(&t.0, &first.lockfile).unwrap();
    fs::remove_dir_all(t.0.join("skills/a")).unwrap();
    let mut o = opts(&t.0, &clock);
    o.dry_run = true;
    let res = sync_skills(&[], &registry(), &o).unwrap();
    assert_eq!(res.cleaned.len(), 1);
    assert!(t.0.join(".identity/skills/a/SKILL.md").is_file());
}

#[test]
fn sync_carries_agents_styles_forward() {
    let t = Tmp::new("carry");
    let mut prev = sample_lock();
    prev.skills.clear();
    prev.rules.clear();
    prev.agents
        .push(("ag".into(), LockEntry::new(None, "h".into())));
    prev.styles
        .push(("st".into(), LockEntry::new(None, "h2".into())));
    save_lockfile(&t.0, &prev).unwrap();
    let clock = clock();
    let res = sync_skills(&[], &registry(), &opts(&t.0, &clock)).unwrap();
    assert_eq!(res.lockfile.agents, prev.agents);
    assert_eq!(res.lockfile.styles, prev.styles);
    assert_eq!(res.lockfile.active_styles, prev.active_styles);
}

#[test]
fn collisions_warn_by_default_and_back_up_when_migrating() {
    let t = Tmp::new("collide");
    put(&t.0, "skills/a/SKILL.md", &skill_md("a", ""));
    put(&t.0, ".identity/legacy/a.md", "hand written");
    let clock = clock();
    let mut o = opts(&t.0, &clock);
    o.migrate = Some(false);
    let warn = sync_skills(&discover_skills(&t.0), &registry(), &o).unwrap();
    assert!(t.0.join(".identity/legacy/a.md").is_file());
    assert_eq!(
        warn.lockfile.skills[0].1.warnings,
        [format!(
            "identity: shadowed by existing file at {}/.identity/legacy/a.md",
            t.0.display()
        )]
    );

    o.migrate = Some(true);
    let res = sync_skills(&discover_skills(&t.0), &registry(), &o).unwrap();
    assert!(!t.0.join(".identity/legacy/a.md").exists());
    let backup =
        t.0.join(".mcpm-backups/.identity/legacy/a.md.20260101T000000Z");
    assert_eq!(fs::read_to_string(&backup).unwrap(), "hand written");
    assert!(res.lockfile.skills[0].1.warnings.is_empty());
    let index = fs::read_to_string(t.0.join(".mcpm-backups/INDEX.json")).unwrap();
    let parsed = json::parse(&index).unwrap();
    let J::Arr(items) = parsed.get("backups").unwrap() else {
        panic!()
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].get("reason").unwrap().as_str(),
        Some("collision-with-synced-skill")
    );
    assert!(index.starts_with("{\n  \"backups\": [\n    {\n      \"timestamp\""));

    put(&t.0, ".identity/legacy/a.md", "again");
    let later = FixedClock(Instant {
        unix_secs: 1_767_225_661,
        micros: 0,
    });
    let mut o = opts(&t.0, &later);
    o.migrate = Some(true);
    sync_skills(&discover_skills(&t.0), &registry(), &o).unwrap();
    assert!(t
        .0
        .join(".mcpm-backups/.identity/legacy/a.md.20260101T000101Z")
        .is_file());
    let J::Arr(items) =
        json::parse(&fs::read_to_string(t.0.join(".mcpm-backups/INDEX.json")).unwrap())
            .unwrap()
            .get("backups")
            .unwrap()
            .clone()
    else {
        panic!()
    };
    assert_eq!(items.len(), 2);
}

#[test]
fn user_edited_output_path_is_overwritten_without_backup() {
    let t = Tmp::new("overwrite");
    put(&t.0, "skills/a/SKILL.md", &skill_md("a", ""));
    put(&t.0, ".identity/skills/a/SKILL.md", "user edit");
    let clock = clock();
    let mut o = opts(&t.0, &clock);
    o.migrate = Some(true);
    sync_skills(&discover_skills(&t.0), &registry(), &o).unwrap();
    assert!(!t.0.join(".mcpm-backups").exists());
    assert!(fs::read_to_string(t.0.join(".identity/skills/a/SKILL.md"))
        .unwrap()
        .contains("Body of a"));
}

#[test]
fn detect_collisions_reads_synced_content_from_disk() {
    let t = Tmp::new("detect");
    put(&t.0, "skills/a/SKILL.md", &skill_md("a", ""));
    put(&t.0, ".identity/legacy/a.md", "x");
    let skills = discover_skills(&t.0);
    let id = Identity;
    let before = detect_collisions(&skills, &[&id], &t.0);
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].synced_content, "");
    put(&t.0, ".identity/skills/a/SKILL.md", "synced");
    assert_eq!(
        detect_collisions(&skills, &[&id], &t.0)[0].synced_content,
        "synced"
    );
}

#[test]
fn global_mode_skips_project_only_clients_and_filters_by_key() {
    struct Keyed(&'static str);
    impl Transpiler for Keyed {
        fn client_key(&self) -> &str {
            self.0
        }
        fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
            Ok(TranspileResult {
                output_path: self.get_output_path(skill, root),
                content: "x".into(),
                warnings: vec![],
            })
        }
        fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
            root.join(self.0).join(format!("{}.txt", skill.name()))
        }
    }
    let t = Tmp::new("global");
    put(&t.0, "skills/a/SKILL.md", &skill_md("a", ""));
    let mut reg = TranspilerRegistry::new();
    for k in ["claude-code", "vscode-copilot", "cursor"] {
        reg.register(Box::new(Keyed(k)));
    }
    let skills = discover_skills(&t.0);
    let clock = clock();
    let mut o = opts(&t.0, &clock);
    o.global_mode = true;
    o.client_keys = Some(vec!["claude-code".into(), "vscode-copilot".into()]);
    let res = sync_skills(&skills, &reg, &o).unwrap();
    assert_eq!(res.lockfile.skills[0].1.clients_synced, ["claude-code"]);
    assert_eq!(res.lockfile.scope, "global");
    assert!(!t.0.join("cursor").exists());
}

#[test]
fn registry_replaces_duplicate_keys_in_place() {
    let mut r = registry();
    r.register(Box::new(Identity));
    assert_eq!(r.len(), 1);
    assert!(r.get("identity").is_some());
    assert!(r.get("nope").is_none());
}

#[test]
fn resolve_mode_never_prompts() {
    assert_eq!(resolve_mode(Some(true)), ResolutionMode::AutoReplace);
    assert_eq!(resolve_mode(Some(false)), ResolutionMode::WarnOnly);
    assert_eq!(resolve_mode(None), ResolutionMode::WarnOnly);
}

#[test]
fn managed_block_injection_normalises_the_whitespace_around_the_block() {
    assert_eq!(
        inject_managed_block("", "x"),
        "<!-- mcpm:start -->\nx\n<!-- mcpm:end -->\n"
    );
    assert_eq!(
        inject_managed_block("user text\n\n", "x"),
        "user text\n\n<!-- mcpm:start -->\nx\n<!-- mcpm:end -->\n"
    );
    let once = inject_managed_block("head\n", "old");
    let twice = inject_managed_block(&format!("{once}tail\n"), "new");
    assert_eq!(
        twice,
        "head\n\n<!-- mcpm:start -->\nnew\n<!-- mcpm:end -->\n\ntail\n"
    );
    assert_eq!(inject_managed_block(&twice, "new"), twice);
}
