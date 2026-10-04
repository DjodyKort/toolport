use super::bundle_apply::World;
use super::bundle_use;
use super::{load_config, Roots};
use crate::plus::sources::fsx;
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use crate::registry::{self, Registry};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

const ACME: &str = include_str!("../../../tests/fixtures/bundles/acme-dev.yaml");

struct Fx {
    base: DataDirFx,
    roots: Roots,
    data: PathBuf,
    cwd: PathBuf,
}

fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

impl Fx {
    fn new(tag: &str, folders_on: bool) -> Self {
        let base = DataDirFx::new("bundle-use", tag);
        let dir = fsx::canonical(&base.dir);
        let roots = Roots::from_home(&dir.join("home"));
        let repo = roots.skills_repo_path();
        for skill in ["notes-helper", "scratch-1", "long-guide"] {
            put(&repo.join("skills").join(skill).join("SKILL.md"), &format!("---\nname: {skill}\ndescription: d\n---\nbody\n"));
        }
        put(&repo.join("rules/acme-knowledge/SKILL.md"), "---\nname: acme-knowledge\ndescription: d\nactivation: always\n---\n\nUse the ERP naming rules.\n");
        put(&repo.join("profiles/acme-dev.yaml"), ACME);
        let cwd = dir.join("proj");
        fs::create_dir_all(cwd.join(".git/info")).unwrap();
        let mut reg = Registry::default();
        reg.add_profile("acme-dev");
        reg.add_profile("servers-only");
        reg.folder_profiles_enabled = folders_on;
        registry::save(&reg).unwrap();
        Self { data: dir.join("plus-data"), roots, cwd, base }
    }

    fn world(&self) -> World<'_> {
        World { roots: &self.roots, data_dir: &self.data }
    }
}

#[test]
fn launch_writes_the_derived_settings_file_and_returns_the_command() {
    let fx = Fx::new("launch", false);
    let before = tree_snapshot(&fx.cwd);
    let out = bundle_use::launch(&fx.world(), "acme-dev", Some(&fx.cwd)).unwrap();
    let file = PathBuf::from(out["settingsFile"].as_str().unwrap());
    assert_eq!(file, fx.roots.home.join(".config/toolport/profiles/acme-dev.settings.json"));
    assert_eq!(out["command"], format!("claude --settings {}", file.display()));
    assert_eq!(out["cwd"], fsx::display(&fx.cwd));
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["skillOverrides"]["notes-helper"], "off");
    assert_eq!(v["skillOverrides"]["long-guide"], "name-only");
    assert_eq!(v["enabledPlugins"]["tools-pack@tools-market"], false);
    assert_eq!(v["claudeMdExcludes"], json!(["**/acme-erp/CLAUDE.md"]));
    assert_eq!(v["permissions"]["deny"], json!(["Agent(reviewer-bot)"]));
    assert!(out["notes"][0].as_str().unwrap().contains("layers.add (acme-knowledge) rides CLAUDE.local.md"));
    assert_eq!(tree_snapshot(&fx.cwd), before);
    assert_eq!(bundle_use::launch(&fx.world(), "nope", None).unwrap_err().code, "not_found");
    assert!(bundle_use::launch(&fx.world(), "acme-dev", None).unwrap()["cwd"].is_null());
    let _ = &fx.base;
}

#[test]
fn use_with_folder_profiles_off_warns_and_applies_the_bundle_only() {
    let fx = Fx::new("use-off", false);
    let dry = bundle_use::use_bundle(&fx.world(), "acme-dev", &fx.cwd, true).unwrap();
    assert!(!fx.cwd.join(".claude").exists());
    let warnings = dry["plan"]["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str().unwrap().starts_with("folder profiles are off, so the server set acme-dev is not routed")));
    assert_eq!(dry["plan"]["summary"], format!("Use acme-dev in {}", fsx::display(&fx.cwd)));
    let out = bundle_use::use_bundle(&fx.world(), "acme-dev", &fx.cwd, false).unwrap();
    assert_eq!(out["server"], json!({"profile": "acme-dev", "foldersEnabled": false, "bound": false}));
    assert!(fx.cwd.join(".claude/settings.local.json").exists());
    assert!(registry::load().unwrap().folder_profiles.is_empty());
    assert!(out["result"]["undo"].as_str().unwrap().starts_with("toolportctl context use --none --cwd "));
}

#[test]
fn use_with_folder_profiles_on_routes_the_server_set_and_none_undoes_both() {
    let fx = Fx::new("use-on", true);
    let key = fsx::display(&fx.cwd);
    let out = bundle_use::use_bundle(&fx.world(), "acme-dev", &fx.cwd, false).unwrap();
    assert_eq!(out["server"]["bound"], true);
    let reg = registry::load().unwrap();
    assert_eq!(reg.folder_profiles.len(), 1);
    assert_eq!(reg.folder_profiles[0].path, key);
    let dry = bundle_use::use_none(&fx.world(), &fx.cwd, true).unwrap();
    assert_eq!(dry["dryRun"], true);
    assert_eq!(registry::load().unwrap().folder_profiles.len(), 1);
    bundle_use::use_none(&fx.world(), &fx.cwd, false).unwrap();
    assert!(registry::load().unwrap().folder_profiles.is_empty());
    assert!(!fx.cwd.join(".claude").exists() && !fx.cwd.join("CLAUDE.local.md").exists());
    assert_eq!(bundle_use::use_none(&fx.world(), &fx.cwd, false).unwrap_err().code, "not_applied");
}

#[test]
fn use_needs_a_bundle_or_a_server_profile_of_that_name() {
    let fx = Fx::new("use-parts", true);
    let only_servers = bundle_use::use_bundle(&fx.world(), "servers-only", &fx.cwd, false).unwrap();
    assert_eq!(only_servers["bundlePart"], false);
    assert_eq!(only_servers["server"]["bound"], true);
    assert!(!fx.cwd.join(".claude").exists());
    assert_eq!(only_servers["result"]["applied"], true);
    bundle_use::use_none(&fx.world(), &fx.cwd, false).unwrap();
    assert_eq!(bundle_use::use_bundle(&fx.world(), "nothing", &fx.cwd, false).unwrap_err().code, "not_found");
}

#[test]
fn the_auto_apply_switch_defaults_off_and_persists_in_context_json() {
    let fx = Fx::new("config", false);
    let w = fx.world();
    assert_eq!(bundle_use::config(&w, None).unwrap()["autoApply"], false);
    assert!(!fx.roots.context_config_path().exists());
    assert_eq!(bundle_use::config(&w, Some(true)).unwrap()["autoApply"], true);
    assert!(fs::read_to_string(fx.roots.context_config_path()).unwrap().contains("\"bundleAutoApply\": true"));
    assert!(load_config(&fx.roots.context_config_path()).bundle_auto_apply);
    assert_eq!(bundle_use::config(&w, Some(false)).unwrap()["autoApply"], false);
    assert!(!fs::read_to_string(fx.roots.context_config_path()).unwrap().contains("bundleAutoApply"));
}

#[test]
fn a_bound_bundle_applies_to_matching_folders_that_have_none() {
    let fx = Fx::new("auto", false);
    let clients = fx.roots.home.join("work/acme-erp/clients");
    for name in ["v18", "v19"] {
        fs::create_dir_all(clients.join(name).join(".git/info")).unwrap();
    }
    fs::write(clients.join("notes.txt"), "x").unwrap();
    let w = fx.world();
    let matches = bundle_use::unapplied_matches(&w);
    let folders: Vec<String> = matches.iter().map(|(n, f)| format!("{n}:{}", f.file_name().unwrap().to_string_lossy())).collect();
    assert_eq!(folders, ["acme-dev:v18", "acme-dev:v19"]);
    bundle_use::use_bundle(&w, "acme-dev", &clients.join("v18"), false).unwrap();
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
