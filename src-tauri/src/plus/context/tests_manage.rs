use super::manage::{check_profile_dir_name, check_profile_name, redact_entry};
use super::{launch, layers, load_config, Roots};
use crate::plus::dispatch;
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const SECRET: &str = "SYNTHETIC-NOT-A-REAL-CREDENTIAL";

struct Fx {
    base: DataDirFx,
    home: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let base = DataDirFx::new("ctx-manage", tag);
        let home = base.dir.join("home");
        fs::create_dir_all(&home).unwrap();
        Self { base, home }
    }

    fn roots(&self) -> Roots {
        Roots::from_home(&self.home)
    }

    fn shims_path(&self) -> PathBuf {
        self.base.dir.join("context-shims.zsh")
    }

    fn put(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.home.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn call(&self, command: &str, mut args: Value) -> Result<Value, String> {
        args["home"] = json!(self.home.to_string_lossy());
        dispatch(&format!("plus.context.{command}"), args)
    }

    fn ok(&self, command: &str, args: Value) -> Value {
        self.call(command, args)
            .unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    fn snapshot(&self) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
        tree_snapshot(&self.home)
    }

    fn add_profile(&self, name: &str) -> Value {
        self.ok(
            "profileAdd",
            json!({"name": name, "rules": "none", "servers": "none"}),
        )
    }

    fn profile_dir(&self, name: &str) -> PathBuf {
        launch::profile_dir(&self.roots(), name)
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn init_scaffolds_the_personal_layer_once_and_saves_the_config() {
    let fx = Fx::new("init");
    let first = fx.ok("init", json!({}));
    assert_eq!(first["dryRun"], false);
    assert_eq!(first["personal"]["created"], true);
    assert_eq!(first["migration"], Value::Null);
    assert_eq!(first["config"]["saved"], true);
    let personal = layers::personal_rule_path(&fx.roots());
    assert!(fs::read_to_string(&personal)
        .unwrap()
        .contains("name: personal"));
    assert!(fx.roots().context_config_path().is_file());
    let steps: Vec<String> = first["nextSteps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["command"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        steps,
        [
            "toolportctl skills sync",
            "toolportctl context client add <name>",
            "toolportctl context profile add bare --no-org --rules none --servers none",
            "toolportctl context sync",
        ]
    );

    fs::write(&personal, "mine\n").unwrap();
    let again = fx.ok("init", json!({}));
    assert_eq!(again["personal"]["created"], false);
    assert_eq!(fs::read_to_string(&personal).unwrap(), "mine\n");
}

#[test]
fn init_moves_clean_allow_entries_and_never_shows_a_credential() {
    let fx = Fx::new("init-migrate");
    let leaky = format!("Bash(deploy --token={SECRET})");
    let local = fx.put(
        ".claude/settings.local.json",
        &json!({"permissions": {"allow": ["Bash(ls:*)", leaky, "Bash(git status)"]}}).to_string(),
    );
    let before = fs::read(&local).unwrap();

    let out = fx.ok("init", json!({}));

    assert_eq!(out["migration"]["found"], 3);
    assert_eq!(out["migration"]["migratable"], 2);
    assert_eq!(out["migration"]["migrated"], 2);
    assert_eq!(out["migration"]["unparseable"], false);
    assert_eq!(
        strings(&out["migration"]["credentials"]),
        ["Bash(deploy --token=…"]
    );
    assert!(!out.to_string().contains(SECRET));
    let saved = fs::read_to_string(fx.roots().context_config_path()).unwrap();
    assert!(!saved.contains(SECRET));
    assert_eq!(
        load_config(&fx.roots().context_config_path())
            .settings
            .ensure_allow,
        ["Bash(ls:*)", "Bash(git status)"]
    );
    assert_eq!(
        fs::read(&local).unwrap(),
        before,
        "settings.local.json stays"
    );

    let rerun = fx.ok("init", json!({}));
    assert_eq!(rerun["migration"]["migratable"], 0);
    assert_eq!(
        load_config(&fx.roots().context_config_path())
            .settings
            .ensure_allow
            .len(),
        2
    );
}

#[test]
fn init_skips_an_unparseable_settings_local_and_keeps_an_unreadable_config() {
    let fx = Fx::new("init-broken");
    let local = fx.put(".claude/settings.local.json", "{ not json");
    let config = fx.put(".config/mcpm/context.json", "{ broken");

    let out = fx.ok("init", json!({}));

    assert_eq!(out["migration"]["unparseable"], true);
    assert_eq!(fs::read_to_string(&local).unwrap(), "{ not json");
    let kept = PathBuf::from(out["config"]["keptUnreadable"].as_str().unwrap());
    assert_eq!(fs::read_to_string(kept).unwrap(), "{ broken");
    assert!(fs::read_to_string(config).unwrap().contains("profiles"));
}

#[test]
fn init_dry_run_reports_and_writes_nothing() {
    let fx = Fx::new("init-dry");
    fx.put(
        ".claude/settings.local.json",
        &json!({"permissions": {"allow": ["Bash(ls:*)"]}}).to_string(),
    );
    fx.put(".config/mcpm/context.json", "{ broken");
    let before = fx.snapshot();

    let out = fx.ok("init", json!({"dryRun": true}));

    assert_eq!(out["dryRun"], true);
    assert_eq!(out["personal"]["created"], true);
    assert_eq!(out["migration"]["migratable"], 1);
    assert_eq!(out["config"]["saved"], false);
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn client_add_scaffolds_with_the_default_and_an_explicit_glob() {
    let fx = Fx::new("client-add");
    let acme = fx.ok("clientAdd", json!({"name": "acme"}));
    assert_eq!(acme["rule"], "client-acme");
    assert_eq!(acme["glob"], "**/clients/acme/**");
    assert_eq!(acme["created"], true);
    let path = PathBuf::from(acme["path"].as_str().unwrap());
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("globs: \"**/clients/acme/**\""));
    assert!(text.contains("activation: always"));

    fs::write(&path, "edited\n").unwrap();
    let again = fx.ok("clientAdd", json!({"name": "acme"}));
    assert_eq!(again["created"], false);
    assert_eq!(fs::read_to_string(&path).unwrap(), "edited\n");

    let scoped = fx.ok("clientAdd", json!({"name": "v18_arp", "glob": "**/v18/**"}));
    assert_eq!(scoped["rule"], "client-v18-arp");
    assert_eq!(scoped["glob"], "**/v18/**");
    let text = fs::read_to_string(scoped["path"].as_str().unwrap()).unwrap();
    assert!(text.contains("globs: \"**/v18/**\""));
}

#[test]
fn client_add_refuses_what_would_break_the_frontmatter_and_writes_nothing() {
    let fx = Fx::new("client-refuse");
    let before = fx.snapshot();
    for (name, glob) in [
        ("a\nb", None),
        ("a---b", None),
        ("a\u{7}b", None),
        ("  ", None),
        ("", None),
        ("acme", Some("**/x\n---\nname: evil")),
        ("acme", Some("**/---/**")),
    ] {
        let mut args = json!({"name": name});
        if let Some(glob) = glob {
            args["glob"] = json!(glob);
        }
        assert!(fx.call("clientAdd", args).is_err(), "{name:?} {glob:?}");
        assert_eq!(fx.snapshot(), before, "{name:?} {glob:?}");
    }
}

#[test]
fn client_add_keeps_the_rule_inside_the_rules_dir_whatever_the_name_looks_like() {
    let fx = Fx::new("client-slug");
    let out = fx.ok("clientAdd", json!({"name": "../up"}));
    let path = PathBuf::from(out["path"].as_str().unwrap());
    assert_eq!(out["rule"], "client-up");
    assert!(path.starts_with(fx.roots().rules_dir()));
    assert_eq!(path.parent().unwrap().file_name().unwrap(), "client-up");
    assert_eq!(out["glob"], "**/clients/../up/**");
}

#[test]
fn client_add_escapes_quotes_so_the_frontmatter_parses_back() {
    let fx = Fx::new("client-quote");
    let out = fx.ok("clientAdd", json!({"name": "ac\"me", "glob": "**/a\"b/**"}));
    let text = fs::read_to_string(out["path"].as_str().unwrap()).unwrap();
    let front = layers::frontmatter_of(&text);
    let get = |k: &str| layers::yaml_text(front.get(serde_yaml::Value::String(k.into())).unwrap());
    assert_eq!(get("description"), "Client context: ac\"me");
    assert_eq!(get("globs"), "**/a\"b/**");
}

#[test]
fn client_add_dry_run_writes_nothing() {
    let fx = Fx::new("client-dry");
    let before = fx.snapshot();
    let out = fx.ok("clientAdd", json!({"name": "acme", "dryRun": true}));
    assert_eq!(out["dryRun"], true);
    assert_eq!(out["created"], true);
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn client_list_and_status_report_layers_profiles_and_the_shims_file() {
    let fx = Fx::new("status");
    let empty = fx.ok("status", json!({}));
    assert_eq!(empty["layers"], json!([]));
    assert_eq!(empty["profiles"], json!([]));
    assert_eq!(empty["shims"]["exists"], false);
    assert_eq!(empty["config"]["exists"], false);
    assert_eq!(fx.ok("clientList", json!({}))["layers"], json!([]));
    assert_eq!(fx.ok("profileList", json!({}))["profiles"], json!([]));

    fx.ok("init", json!({}));
    fx.ok("clientAdd", json!({"name": "acme"}));
    fx.add_profile("work");
    fx.put(
        ".claude.json",
        &json!({"mcpServers": {"context7": {"command": "x"}, "mcpm_context7": {"command": "x"}}})
            .to_string(),
    );

    let list = fx.ok("clientList", json!({}));
    let names: Vec<&str> = list["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["client-acme", "personal"]);
    assert_eq!(list["layers"][0]["globs"], json!(["**/clients/acme/**"]));
    assert_eq!(list["layers"][1]["globs"], json!([]));

    let status = fx.ok("status", json!({}));
    assert_eq!(status["layers"], list["layers"]);
    assert_eq!(status["legacyDupes"], json!(["context7"]));
    assert_eq!(status["shims"]["exists"], true);
    assert_eq!(status["config"]["exists"], true);
    assert_eq!(status["profiles"][0]["name"], "work");
    assert_eq!(status["profiles"][0]["generated"], true);
}

#[test]
fn profile_add_defines_generates_and_lists() {
    let fx = Fx::new("profile-add");
    let out = fx.add_profile("work");
    let profile = &out["profile"];
    assert_eq!(profile["name"], "work");
    assert_eq!(profile["shim"], "claude-work");
    assert_eq!(profile["created"], true);
    assert_eq!(profile["generated"], true);
    assert_eq!(profile["org"], true);
    assert_eq!(profile["orgMode"], "import");
    assert_eq!(profile["rules"], "none");
    assert_eq!(profile["servers"], "none");
    assert!(fx.profile_dir("work").join("mcp.json").is_file());
    let actions = strings(&out["actions"]);
    assert!(actions
        .iter()
        .any(|a| a.starts_with("generated launch profile work (0 server(s)) in ")));
    assert!(actions.iter().any(|a| a.starts_with("wrote shims: ")));
    assert!(actions.contains(&"saved config (1 profile(s))".to_string()));
    assert!(fx.shims_path().is_file());

    let list = fx.ok("profileList", json!({}));
    assert_eq!(list["profiles"].as_array().unwrap().len(), 1);
    assert_eq!(list["profiles"][0]["dir"], profile["dir"]);
}

#[test]
fn profile_add_takes_lists_and_the_flags_the_command_has() {
    let fx = Fx::new("profile-flags");
    let out = fx.ok(
        "profileAdd",
        json!({
            "name": "lean", "org": false, "orgMode": "copy", "rules": "personal, client-acme",
            "servers": ["alpha", "beta"], "commands": false, "skills": false,
        }),
    );
    let profile = &out["profile"];
    assert_eq!(profile["org"], false);
    assert_eq!(profile["orgMode"], "copy");
    assert_eq!(profile["rules"], json!(["personal", "client-acme"]));
    assert_eq!(profile["servers"], json!(["alpha", "beta"]));
    let saved = load_config(&fx.roots().context_config_path());
    let spec = &saved.profiles["lean"];
    assert!(!spec.commands && !spec.skills && !spec.org);
}

#[test]
fn profile_add_keeps_the_fields_it_has_no_option_for() {
    let fx = Fx::new("profile-keep");
    fx.put(
        ".config/mcpm/context.json",
        &json!({"profiles": {"work": {
            "settings_overrides": {"model": "haiku"}, "copy_auth": false, "link_rules": false,
        }}})
        .to_string(),
    );
    let out = fx.add_profile("work");
    assert_eq!(out["profile"]["created"], false);
    let saved = load_config(&fx.roots().context_config_path());
    let spec = &saved.profiles["work"];
    assert_eq!(spec.settings_overrides["model"], "haiku");
    assert!(!spec.copy_auth && !spec.link_rules);
    assert_eq!(spec.rules, json!("none"));
}

#[test]
fn profile_add_refuses_bad_names_and_modes_and_writes_nothing() {
    let fx = Fx::new("profile-refuse");
    let before = fx.snapshot();
    for name in ["Bad Name", "", "../x", "a/b", "-lead", "has space", "ü"] {
        assert!(
            fx.call("profileAdd", json!({"name": name})).is_err(),
            "{name:?}"
        );
    }
    assert!(fx
        .call("profileAdd", json!({"name": "ok", "orgMode": "weird"}))
        .is_err());
    assert!(fx
        .call("profileAdd", json!({"name": "ok", "rules": 3}))
        .is_err());
    assert!(fx.call("profileAdd", json!({})).is_err());
    assert_eq!(fx.snapshot(), before);
    assert!(check_profile_name("work").is_ok());
    assert!(check_profile_name("Work").is_err());
}

#[test]
fn profile_add_dry_run_writes_nothing_and_says_it_would() {
    let fx = Fx::new("profile-dry");
    fx.put(".claude.json", "{}\n");
    let before = fx.snapshot();
    let out = fx.ok(
        "profileAdd",
        json!({"name": "work", "rules": "none", "servers": "none", "dryRun": true}),
    );
    assert_eq!(out["dryRun"], true);
    assert_eq!(out["profile"]["generated"], false);
    assert!(strings(&out["actions"])
        .iter()
        .any(|a| a.starts_with("would generate launch profile work (0 server(s)) in ")));
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn profile_remove_drops_the_config_entry_and_leaves_the_dir_as_an_orphan() {
    let fx = Fx::new("profile-remove");
    fx.add_profile("work");
    let dir = fx.profile_dir("work");

    let out = fx.ok("profileRemove", json!({"name": "work"}));

    assert_eq!(out["inConfig"], true);
    assert_eq!(out["purged"], false);
    assert!(dir.is_dir());
    assert!(load_config(&fx.roots().context_config_path())
        .profiles
        .is_empty());
    assert_eq!(
        strings(&out["warnings"]),
        [format!(
            "orphan profile dir {} (not in config) — `toolportctl context profile remove work --purge`",
            dir.display()
        )]
    );

    let purged = fx.ok("profileRemove", json!({"name": "work", "purge": true}));
    assert_eq!(purged["inConfig"], false);
    assert_eq!(purged["purged"], true);
    assert!(!dir.exists());
    assert_eq!(
        strings(&purged["actions"])[0],
        format!("removed profile dir {}", dir.display())
    );
    assert_eq!(purged["warnings"], json!([]));

    let ghost = fx.ok("profileRemove", json!({"name": "ghost", "purge": true}));
    assert_eq!(ghost["purged"], false);
    assert_eq!(ghost["inConfig"], false);
}

#[test]
fn profile_remove_purge_clears_an_orphan_that_is_not_in_the_config() {
    let fx = Fx::new("profile-orphan");
    let orphan = fx.put(".config/mcpm/claude-profiles/stale/CLAUDE.md", "old\n");
    let out = fx.ok("profileRemove", json!({"name": "stale", "purge": true}));
    assert_eq!(out["inConfig"], false);
    assert_eq!(out["purged"], true);
    assert!(!orphan.exists());
    assert!(!orphan.parent().unwrap().exists());
}

#[test]
fn profile_remove_purge_refuses_names_that_leave_the_profiles_root() {
    let fx = Fx::new("profile-traversal");
    let victim = fx.put(".config/mcpm/victim/keep.txt", "keep\n");
    let sibling = fx.put(".config/mcpm/claude-profiles-extra/keep.txt", "keep\n");
    let before = fx.snapshot();
    for name in ["..", ".", "../victim", "a/b", "/etc", "", "x/../../victim"] {
        let result = fx.call("profileRemove", json!({"name": name, "purge": true}));
        assert!(result.is_err(), "{name:?} {result:?}");
    }
    assert_eq!(fx.snapshot(), before);
    assert!(victim.is_file() && sibling.is_file());
    assert!(check_profile_dir_name("stale-orphan").is_ok());
    assert!(check_profile_dir_name("Odd Name.d").is_ok());
    assert!(check_profile_dir_name("a/b").is_err());
    assert!(check_profile_dir_name("..").is_err());
}

#[cfg(unix)]
#[test]
fn profile_remove_purge_unlinks_a_symlink_and_never_follows_it() {
    let fx = Fx::new("profile-symlink");
    let target = fx.put("elsewhere/keep.txt", "keep\n");
    let root = fx.roots().profiles_root();
    fs::create_dir_all(&root).unwrap();
    let link = root.join("link");
    std::os::unix::fs::symlink(target.parent().unwrap(), &link).unwrap();

    let out = fx.ok("profileRemove", json!({"name": "link", "purge": true}));

    assert_eq!(out["purged"], true);
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep\n");
}

#[test]
fn profile_remove_dry_run_writes_nothing_and_does_not_warn_about_the_dir_it_would_remove() {
    let fx = Fx::new("profile-remove-dry");
    fx.add_profile("work");
    fx.put(".config/mcpm/claude-profiles/stale/CLAUDE.md", "old\n");
    let before = fx.snapshot();

    let out = fx.ok(
        "profileRemove",
        json!({"name": "work", "purge": true, "dryRun": true}),
    );

    assert_eq!(out["dryRun"], true);
    assert_eq!(out["purged"], true);
    assert!(strings(&out["actions"])[0].starts_with("would remove profile dir "));
    let warnings = strings(&out["warnings"]);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("claude-profiles/stale"));
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn disable_removes_the_shims_and_only_with_the_flag_the_profile_dirs() {
    let fx = Fx::new("disable");
    fx.add_profile("a");
    fx.add_profile("b");
    let stray = fx.put(".config/mcpm/claude-profiles/notes.txt", "keep\n");
    let config = fx.roots().context_config_path();
    let config_before = fs::read(&config).unwrap();

    let dry_before = fx.snapshot();
    let dry = fx.ok("disable", json!({"dryRun": true, "purgeProfiles": true}));
    assert_eq!(dry["shims"]["removed"], true);
    assert_eq!(dry["purgedProfiles"].as_array().unwrap().len(), 2);
    assert_eq!(fx.snapshot(), dry_before);

    let out = fx.ok("disable", json!({}));
    assert_eq!(out["shims"]["removed"], true);
    assert_eq!(out["purgedProfiles"], json!([]));
    assert_eq!(strings(&out["actions"]), ["removed shims file"]);
    assert!(!fx.shims_path().exists());
    assert!(fx.profile_dir("a").is_dir());

    let nothing = fx.ok("disable", json!({}));
    assert_eq!(nothing["shims"]["removed"], false);
    assert_eq!(nothing["actions"], json!([]));

    let purged = fx.ok("disable", json!({"purgeProfiles": true}));
    assert_eq!(purged["purgedProfiles"].as_array().unwrap().len(), 2);
    assert!(!fx.profile_dir("a").exists() && !fx.profile_dir("b").exists());
    assert!(
        stray.is_file(),
        "files in the profiles root are not profiles"
    );
    assert_eq!(fs::read(&config).unwrap(), config_before, "config stays");
}

#[test]
fn redact_entry_cuts_at_the_first_assignment() {
    assert_eq!(redact_entry("Bash(x --token=abc)"), "Bash(x --token=…");
    assert_eq!(redact_entry("Bash(ls)"), "Bash(ls)");
    assert_eq!(redact_entry("=lead"), "=…");
}

#[test]
fn every_handler_is_registered_under_its_plus_name() {
    let fx = Fx::new("registered");
    for command in ["init", "status", "clientList", "profileList", "disable"] {
        assert!(
            fx.call(command, json!({"dryRun": true})).is_ok(),
            "{command}"
        );
    }
    for command in ["clientAdd", "profileAdd", "profileRemove"] {
        let err = fx.call(command, json!({})).unwrap_err();
        assert!(err.contains("name is required"), "{command}: {err}");
    }
}

#[test]
fn a_handler_run_in_a_missing_home_does_not_create_it_on_a_dry_run() {
    let fx = Fx::new("missing-home");
    let ghost = fx.home.join("not-there");
    let out = dispatch(
        "plus.context.init",
        json!({"home": ghost.to_string_lossy(), "dryRun": true}),
    )
    .unwrap();
    assert_eq!(out["dryRun"], true);
    assert!(!Path::new(&ghost).exists());
}
