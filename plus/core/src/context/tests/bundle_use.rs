use crate::context::bundle_apply::World;
use crate::context::bundle_use;
use crate::context::config;
use crate::context::fsx;
use crate::context::loads::NoLoads;
use crate::context::testkit::{tree_snapshot, ScratchDir};
use crate::context::Roots;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use toolport_plus_api::{AccessSetInfo, MemoryHost, MemoryRegistry};

const ACME: &str = include_str!("../../../../fixtures/bundles/acme-dev.yaml");

struct Fx {
    scratch: ScratchDir,
    roots: Roots,
    data: PathBuf,
    cwd: PathBuf,
    host: MemoryHost,
}

fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn sets(names: &[&str]) -> MemoryRegistry {
    let registry = MemoryRegistry {
        exists: true,
        ..Default::default()
    };
    *registry.access_sets.borrow_mut() = names
        .iter()
        .map(|name| AccessSetInfo {
            id: format!("id-{name}"),
            name: name.to_string(),
            server_ids: Vec::new(),
        })
        .collect();
    registry
}

impl Fx {
    fn new(tag: &str) -> Self {
        let scratch = ScratchDir::new(tag);
        let dir = fsx::canonical(scratch.path());
        let roots = Roots::from_home(&dir.join("home"));
        let repo = roots.skills_repo_path();
        for skill in ["notes-helper", "scratch-1", "long-guide"] {
            put(
                &repo.join("skills").join(skill).join("SKILL.md"),
                &format!("---\nname: {skill}\ndescription: d\n---\nbody\n"),
            );
        }
        put(&repo.join("rules/acme-knowledge/SKILL.md"), "---\nname: acme-knowledge\ndescription: d\nactivation: always\n---\n\nUse the ERP naming rules.\n");
        put(&repo.join("profiles/acme-dev.yaml"), ACME);
        let cwd = dir.join("proj");
        fs::create_dir_all(cwd.join(".git/info")).unwrap();
        let host = MemoryHost {
            registry: sets(&["acme-dev", "servers-only"]),
            ..Default::default()
        };
        Self {
            data: dir.join("plus-data"),
            roots,
            cwd,
            scratch,
            host,
        }
    }

    fn world(&self) -> World<'_> {
        World {
            roots: &self.roots,
            data_dir: &self.data,
            loads: &NoLoads,
        }
    }

    fn routes(&self) -> Vec<toolport_plus_api::FolderRoute> {
        self.host.registry.folder_routes.borrow().clone()
    }
}

#[test]
fn launch_writes_the_derived_settings_file_and_returns_the_command() {
    let fx = Fx::new("launch");
    let before = tree_snapshot(&fx.cwd);
    let out = bundle_use::launch(&fx.world(), "acme-dev", Some(&fx.cwd)).unwrap();
    let file = PathBuf::from(out["settingsFile"].as_str().unwrap());
    assert_eq!(
        file,
        fx.roots
            .home
            .join(".config/toolport/profiles/acme-dev.settings.json")
    );
    assert_eq!(
        out["command"],
        format!("claude --settings {}", file.display())
    );
    assert_eq!(out["cwd"], fsx::display(&fx.cwd));
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["skillOverrides"]["notes-helper"], "off");
    assert_eq!(v["skillOverrides"]["long-guide"], "name-only");
    assert_eq!(v["enabledPlugins"]["tools-pack@tools-market"], false);
    assert_eq!(v["claudeMdExcludes"], json!(["**/acme-erp/CLAUDE.md"]));
    assert_eq!(v["permissions"]["deny"], json!(["Agent(reviewer-bot)"]));
    assert!(out["notes"][0]
        .as_str()
        .unwrap()
        .contains("layers.add (acme-knowledge) rides CLAUDE.local.md"));
    assert_eq!(tree_snapshot(&fx.cwd), before);
    assert_eq!(
        bundle_use::launch(&fx.world(), "nope", None)
            .unwrap_err()
            .code,
        "not_found"
    );
    assert!(bundle_use::launch(&fx.world(), "acme-dev", None).unwrap()["cwd"].is_null());
    let _ = &fx.scratch;
}

#[test]
fn use_applies_the_bundle_and_routes_the_folder_to_the_paired_access_set() {
    let fx = Fx::new("use-on");
    let key = fsx::display(&fx.cwd);
    let dry = bundle_use::use_bundle(&fx.world(), &fx.host, "acme-dev", &fx.cwd, true).unwrap();
    assert!(!fx.cwd.join(".claude").exists());
    assert!(fx.routes().is_empty());
    assert_eq!(dry["plan"]["summary"], format!("Use acme-dev in {key}"));
    let out = bundle_use::use_bundle(&fx.world(), &fx.host, "acme-dev", &fx.cwd, false).unwrap();
    assert_eq!(
        out["server"],
        json!({"accessSet": "acme-dev", "bound": true})
    );
    assert!(fx.cwd.join(".claude/settings.local.json").exists());
    assert_eq!(fx.routes().len(), 1);
    assert_eq!(fx.routes()[0].path, key);
    assert_eq!(fx.routes()[0].access_set_id, "id-acme-dev");
    assert!(out["result"]["undo"]
        .as_str()
        .unwrap()
        .starts_with("toolportctl context use --none --cwd "));
    let dry = bundle_use::use_none(&fx.world(), &fx.host, &fx.cwd, true).unwrap();
    assert_eq!(dry["dryRun"], true);
    assert_eq!(fx.routes().len(), 1);
    bundle_use::use_none(&fx.world(), &fx.host, &fx.cwd, false).unwrap();
    assert!(fx.routes().is_empty());
    assert!(!fx.cwd.join(".claude").exists() && !fx.cwd.join("CLAUDE.local.md").exists());
    assert_eq!(
        bundle_use::use_none(&fx.world(), &fx.host, &fx.cwd, false)
            .unwrap_err()
            .code,
        "not_applied"
    );
}

#[test]
fn use_needs_a_bundle_or_an_access_set_of_that_name() {
    let fx = Fx::new("use-parts");
    let only_sets =
        bundle_use::use_bundle(&fx.world(), &fx.host, "servers-only", &fx.cwd, false).unwrap();
    assert_eq!(only_sets["bundlePart"], false);
    assert_eq!(only_sets["server"]["bound"], true);
    assert!(!fx.cwd.join(".claude").exists());
    assert_eq!(only_sets["result"]["applied"], true);
    bundle_use::use_none(&fx.world(), &fx.host, &fx.cwd, false).unwrap();
    assert_eq!(
        bundle_use::use_bundle(&fx.world(), &fx.host, "nothing", &fx.cwd, false)
            .unwrap_err()
            .code,
        "not_found"
    );
}

#[test]
fn a_bundle_without_a_matching_access_set_applies_alone() {
    let mut fx = Fx::new("use-alone");
    fx.host.registry = sets(&["other"]);
    let out = bundle_use::use_bundle(&fx.world(), &fx.host, "acme-dev", &fx.cwd, false).unwrap();
    assert_eq!(out["server"], json!({"accessSet": null, "bound": false}));
    assert!(fx.routes().is_empty());
    assert!(fx.cwd.join(".claude/settings.local.json").exists());
}

#[test]
fn the_auto_apply_switch_defaults_off_and_persists_in_context_json() {
    let fx = Fx::new("config");
    let w = fx.world();
    assert_eq!(bundle_use::config(&w, None).unwrap()["autoApply"], false);
    assert!(!fx.roots.context_config_path().exists());
    assert_eq!(
        bundle_use::config(&w, Some(true)).unwrap()["autoApply"],
        true
    );
    assert!(fs::read_to_string(fx.roots.context_config_path())
        .unwrap()
        .contains("\"bundleAutoApply\": true"));
    assert!(config::bundle_auto_apply(&fx.roots.context_config_path()));
    assert_eq!(
        bundle_use::config(&w, Some(false)).unwrap()["autoApply"],
        false
    );
    assert!(!fs::read_to_string(fx.roots.context_config_path())
        .unwrap()
        .contains("bundleAutoApply"));
}

#[test]
fn a_bound_bundle_applies_to_matching_folders_that_have_none() {
    let fx = Fx::new("auto");
    let clients = fx.roots.home.join("work/acme-erp/clients");
    for name in ["v18", "v19"] {
        fs::create_dir_all(clients.join(name).join(".git/info")).unwrap();
    }
    fs::write(clients.join("notes.txt"), "x").unwrap();
    let w = fx.world();
    let matches = bundle_use::unapplied_matches(&w);
    let folders: Vec<String> = matches
        .iter()
        .map(|(n, f)| format!("{n}:{}", f.file_name().unwrap().to_string_lossy()))
        .collect();
    assert_eq!(folders, ["acme-dev:v18", "acme-dev:v19"]);
    bundle_use::use_bundle(&w, &fx.host, "acme-dev", &clients.join("v18"), false).unwrap();
    assert_eq!(bundle_use::unapplied_matches(&w).len(), 1);
    let dry = bundle_use::auto_apply(&w, true);
    assert_eq!(dry[0]["applied"], false);
    assert!(!clients.join("v19/.claude").exists());
    let done = bundle_use::auto_apply(&w, false);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0]["applied"], true);
    assert!(clients.join("v19/.claude/settings.local.json").exists());
    assert!(bundle_use::unapplied_matches(&w).is_empty());
}
