use super::layer_spec::{self, Delivery, Scope};
use super::{compose, layer_manage, layers, load_config, Report, Roots};
use crate::plus::dispatch;
use crate::plus::testutil::DataDirFx;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

struct Fx {
    _base: DataDirFx,
    home: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let base = DataDirFx::new("ctx-layers", tag);
        let home = base.dir.join("home");
        fs::create_dir_all(&home).unwrap();
        Self { _base: base, home }
    }

    fn roots(&self) -> Roots {
        Roots::from_home(&self.home)
    }

    fn put(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.home.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn layer(&self, dir: &str, frontmatter: &str, body: &str) -> PathBuf {
        self.put(
            &format!(".config/mcpm/skills_repo/rules/{dir}/SKILL.md"),
            &format!("---\nname: {dir}\ndescription: d\nactivation: always\n{frontmatter}---\n\n{body}\n"),
        )
    }

    fn repo(&self, name: &str) -> PathBuf {
        let dir = self.home.join("work").join(name);
        fs::create_dir_all(dir.join(".git/info")).unwrap();
        dir
    }

    fn args(&self, mut args: Value) -> Value {
        args["home"] = json!(self.home.to_string_lossy());
        args
    }

    fn add(&self, args: Value) -> Value {
        layer_manage::add(&self.args(args)).unwrap_or_else(|e| panic!("add: {}", e.message))
    }

    fn edit(&self, args: Value) -> Value {
        layer_manage::edit(&self.args(args)).unwrap_or_else(|e| panic!("edit: {}", e.message))
    }

    fn list(&self) -> Vec<Value> {
        layer_manage::list(&self.args(json!({}))).unwrap()["layers"].as_array().unwrap().clone()
    }

    fn deploy(&self, dry: bool) -> Report {
        let roots = self.roots();
        let mut report = Report::default();
        let clients = roots.resolve_clients_root(&load_config(&roots.context_config_path()));
        layers::deploy_client_locals(&roots, &clients, &mut report, dry).unwrap();
        report
    }

    fn delivered(&self, name: &str) -> layer_spec::Delivered {
        let roots = self.roots();
        let all = layers::list_layers(&roots);
        let layer = all.iter().find(|l| l.name == name).unwrap();
        layer_spec::deliver(&roots.home, &all, layer)
    }
}

fn text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn messages(issues: &[super::bundle::Issue]) -> String {
    issues.iter().map(|i| format!("{}: {}", i.key, i.message)).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_layer_without_the_new_keys_keeps_its_defaults_and_its_scaffold() {
    let fx = Fx::new("defaults");
    let added = fx.add(json!({"name": "acme"}));
    let rule = text(Path::new(added["path"].as_str().unwrap()));
    for key in ["scope:", "folders:", "imports:", "delivery:"] {
        assert!(!rule.contains(key), "{key} must not appear in a plain scaffold");
    }
    assert_eq!((added["scope"].as_str(), added["delivery"].as_str()), (Some("glob"), Some("copy")));
    fx.layer("legacy", "globs: \"**/x/**\"\n", "Old layer.");
    let rows = fx.list();
    let legacy = rows.iter().find(|r| r["name"] == "legacy").unwrap();
    assert_eq!(legacy["scope"], "glob");
    assert_eq!(legacy["delivery"], "copy");
    assert_eq!(legacy["folders"], json!([]));
    assert_eq!(legacy["imports"], json!([]));
    assert_eq!(legacy["deployedTo"], json!([]));
    assert_eq!(legacy["issues"], json!([]));
}

#[test]
fn the_frontmatter_values_are_linted() {
    let fx = Fx::new("lint");
    fx.layer("bad-scope", "scope: elsewhere\ndelivery: paste\n", "x");
    fx.layer("no-folder", "scope: folder\n", "x");
    fx.layer("relative", "scope: folder\nfolders: [\"work/here\"]\n", "x");
    fx.layer("stray", "scope: glob\nfolders: [\"/srv/a\"]\ndelivery: import\n", "x");
    let roots = fx.roots();
    let all = layers::list_layers(&roots);
    let lint = |name: &str| messages(&layer_spec::lint(&roots.home, &all, all.iter().find(|l| l.name == name).unwrap()));
    assert!(lint("bad-scope").contains("scope: \"elsewhere\" is not global, glob or folder"));
    assert!(lint("bad-scope").contains("delivery: \"paste\" is not import or copy"));
    assert!(lint("no-folder").contains("scope folder needs at least one folder"));
    assert!(lint("relative").contains("must be an absolute folder"));
    assert!(lint("stray").contains("folders is only used when scope is folder"));
    assert!(lint("stray").contains("delivery import has nothing to import"));
    let by = |name: &str| all.iter().find(|l| l.name == name).unwrap().spec.clone();
    assert_eq!((by("bad-scope").scope, by("bad-scope").delivery), (Scope::Glob, Delivery::Copy));
}

#[test]
fn a_copy_layer_inlines_what_it_imports_and_an_import_layer_writes_the_path() {
    let fx = Fx::new("delivery");
    let kb = fx.put("kb/CLAUDE.md", "# Knowledge\nPost before closing.\n");
    let imports = format!("imports: [\"{}\"]\n", kb.display());
    fx.layer("by-copy", &imports, "Own text.");
    fx.layer("by-import", &format!("{imports}delivery: import\n"), "Own text.");
    let copied = fx.delivered("by-copy");
    assert!(copied.text.starts_with("Own text."), "{}", copied.text);
    assert!(copied.text.contains("Post before closing."));
    assert!(copied.issues.is_empty());
    let linked = fx.delivered("by-import");
    assert!(linked.text.contains("@~/kb/CLAUDE.md"), "{}", linked.text);
    assert!(!linked.text.contains("Post before closing."));
}

#[test]
fn a_chain_of_four_hops_is_followed_and_a_fifth_is_reported_and_not_followed() {
    let fx = Fx::new("chain");
    for n in 1..=6 {
        let next = if n < 6 { format!("@hop{}.md\n", n + 1) } else { String::new() };
        fx.put(&format!("kb/hop{n}.md"), &format!("Text of hop {n}.\n{next}"));
    }
    let first = fx.home.join("kb/hop1.md");
    fx.layer("deep", &format!("imports: [\"{}\"]\n", first.display()), "Own.");
    let out = fx.delivered("deep");
    for n in 1..=4 {
        assert!(out.text.contains(&format!("Text of hop {n}.")), "hop {n} is inlined");
    }
    assert!(!out.text.contains("Text of hop 5."), "{}", out.text);
    assert!(out.text.contains("@hop5.md"), "the line that was not followed stays");
    assert_eq!(out.issues.len(), 1, "{}", messages(&out.issues));
    assert!(out.issues[0].message.contains("more than 4 hops"));
    fx.put("kb/hop4.md", "Text of hop 4.\n");
    let exact = fx.delivered("deep");
    assert!(exact.issues.is_empty(), "a chain of exactly four hops is fine");
}

#[test]
fn a_cycle_is_reported_and_not_followed() {
    let fx = Fx::new("cycle");
    fx.put("kb/a.md", "Text of a.\n@b.md\n");
    fx.put("kb/b.md", "Text of b.\n@a.md\n");
    fx.layer("loop", &format!("imports: [\"{}\"]\n", fx.home.join("kb/a.md").display()), "Own.");
    fx.layer("one", "imports: [\"two\"]\n", "Layer one.");
    fx.layer("two", "imports: [\"one\"]\n", "Layer two.");
    let out = fx.delivered("loop");
    assert_eq!(out.text.matches("Text of a.").count(), 1, "{}", out.text);
    assert_eq!(out.text.matches("Text of b.").count(), 1);
    assert!(out.issues.iter().any(|i| i.message.contains("import cycle")), "{}", messages(&out.issues));
    let layers_cycle = fx.delivered("one");
    assert!(layers_cycle.text.contains("Layer two."));
    assert!(layers_cycle.issues.iter().any(|i| i.message.contains("import cycle one -> two -> one")));
}

#[test]
fn a_layer_can_import_another_layer_and_a_missing_import_is_reported() {
    let fx = Fx::new("layer-import");
    fx.layer("base", "", "Shared text.");
    fx.layer("top", "imports: [\"base\", \"nowhere\", \"/no/such/file.md\"]\n", "Top text.");
    let out = fx.delivered("top");
    assert!(out.text.contains("Top text.") && out.text.contains("Shared text."));
    let reported = messages(&out.issues);
    assert!(reported.contains("nowhere: no layer or file of that name"), "{reported}");
    assert!(reported.contains("/no/such/file.md"), "{reported}");
}

#[test]
fn a_folder_layer_is_delivered_into_its_folders_and_refreshed_when_its_import_changes() {
    let fx = Fx::new("folder");
    let repo = fx.repo("erp-client");
    let other = fx.repo("other");
    let kb = fx.put("kb/CLAUDE.md", "Knowledge, version one.\n");
    fx.layer(
        "client-knowledge",
        &format!("scope: folder\nfolders: [\"{}\", \"{}\"]\nimports: [\"{}\"]\n", repo.display(), other.display(), kb.display()),
        "Own text.",
    );
    let first = fx.deploy(false);
    assert_eq!(first.actions.iter().filter(|a| a.starts_with("deployed ")).count(), 2, "{:?}", first.actions);
    let local = repo.join("CLAUDE.local.md");
    let written = text(&local);
    assert!(written.starts_with(layers::MANAGED_LOCAL_HEADER));
    assert!(written.contains("<!-- toolport:layer:begin client-knowledge -->"));
    assert!(written.contains("Own text.") && written.contains("Knowledge, version one."));
    assert!(text(&repo.join(".git/info/exclude")).lines().any(|l| l == "CLAUDE.local.md"));
    assert!(fx.deploy(false).actions.is_empty(), "a second deploy changes nothing");

    fs::write(&kb, "Knowledge, version two.\n").unwrap();
    let preview = fx.deploy(true);
    assert!(preview.actions.iter().any(|a| a.starts_with("deployed ")), "{:?}", preview.actions);
    assert_eq!(text(&local), written, "a dry run writes nothing");
    fx.deploy(false);
    assert!(text(&local).contains("Knowledge, version two.") && !text(&local).contains("version one"));

    let rows = fx.list();
    let row = rows.iter().find(|r| r["name"] == "client-knowledge").unwrap();
    let deployed: Vec<&str> = row["deployedTo"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(deployed.len(), 2, "{deployed:?}");
}

#[test]
fn layers_that_share_a_folder_share_the_managed_file_and_a_user_file_is_left_alone() {
    let fx = Fx::new("share");
    let repo = fx.repo("shared");
    let mine = fx.repo("mine");
    fx.put("work/mine/CLAUDE.local.md", "# My own notes\n");
    for (name, body) in [("client-a", "Layer a."), ("client-b", "Layer b.")] {
        fx.layer(name, &format!("scope: folder\nfolders: [\"{}\", \"{}\"]\n", repo.display(), mine.display()), body);
    }
    let report = fx.deploy(false);
    let shared = text(&repo.join("CLAUDE.local.md"));
    assert!(shared.contains("Layer a.") && shared.contains("Layer b."));
    assert!(shared.find("client-a").unwrap() < shared.find("client-b").unwrap());
    assert_eq!(text(&mine.join("CLAUDE.local.md")), "# My own notes\n");
    assert!(report.warnings.iter().any(|w| w.contains("is not managed")), "{:?}", report.warnings);
}

#[test]
fn a_client_layer_named_after_its_folder_keeps_the_old_file_format() {
    let fx = Fx::new("legacy");
    let clients = fx.home.join("Documents/GitHub/ExampleWorkspace/clients");
    let client = clients.join("acme");
    fs::create_dir_all(client.join(".git/info")).unwrap();
    fx.layer("client-acme", "globs: \"**/clients/acme/**\"\n", "Plain client body.");
    fx.deploy(false);
    assert_eq!(text(&client.join("CLAUDE.local.md")), format!("{}\n\nPlain client body.\n", layers::MANAGED_LOCAL_HEADER));
}

#[test]
fn add_edit_and_rm_plan_first_and_keep_the_folders_in_step() {
    let fx = Fx::new("manage");
    let repo = fx.repo("erp-client");
    let other = fx.repo("other");
    let kb = fx.put("kb/CLAUDE.md", "Shared knowledge.\n");
    let args = json!({
        "name": "knowledge", "scope": "folder", "folders": [repo.to_string_lossy()],
        "imports": [kb.to_string_lossy()],
    });
    let mut preview_args = args.clone();
    preview_args["dryRun"] = json!(true);
    let preview = fx.add(preview_args);
    assert_eq!(preview["result"], Value::Null);
    assert!(!repo.join("CLAUDE.local.md").exists());
    let ops: Vec<&str> = preview["plan"]["steps"].as_array().unwrap().iter().map(|s| s["op"].as_str().unwrap()).collect();
    assert_eq!(ops, ["create", "create", "update"], "{}", preview["plan"]);
    assert_eq!(preview["plan"]["undo"], "toolportctl context client rm knowledge");
    assert!(!preview["plan"].to_string().contains("Shared knowledge"), "imported text stays out of the plan");

    let applied = fx.add(args);
    assert_eq!(applied["result"]["applied"], true);
    assert!(text(&repo.join("CLAUDE.local.md")).contains("Shared knowledge."));
    let rule = text(Path::new(applied["path"].as_str().unwrap()));
    assert!(rule.contains("scope: folder") && rule.contains("imports: ["), "{rule}");

    let moved = fx.edit(json!({"name": "knowledge", "folders": [other.to_string_lossy()], "delivery": "copy"}));
    assert_eq!(moved["scope"], "folder");
    assert!(!repo.join("CLAUDE.local.md").exists(), "the folder the layer left loses its managed file");
    assert!(text(&other.join("CLAUDE.local.md")).contains("Shared knowledge."));
    assert!(moved["plan"]["undo"].as_str().unwrap().starts_with("toolportctl context client edit client-knowledge"));
    let rule = text(Path::new(moved["path"].as_str().unwrap()));
    assert!(rule.contains("activation: always") && rule.contains("name: client-knowledge"), "other keys survive: {rule}");

    let removal = layer_manage::rm(&fx.args(json!({"name": "knowledge", "dryRun": true}))).unwrap();
    assert!(other.join("CLAUDE.local.md").exists() && Path::new(moved["path"].as_str().unwrap()).exists());
    assert_eq!(removal["plan"]["steps"].as_array().unwrap().len(), 2);
    layer_manage::rm(&fx.args(json!({"name": "client-knowledge"}))).unwrap();
    assert!(!other.join("CLAUDE.local.md").exists());
    assert!(!Path::new(moved["path"].as_str().unwrap()).exists());
    let gone = layer_manage::rm(&fx.args(json!({"name": "knowledge"}))).unwrap_err();
    assert_eq!(gone.code(), "not_found");
}

#[test]
fn a_scaffold_in_the_config_fills_what_the_command_leaves_out() {
    let fx = Fx::new("scaffold");
    let repo = fx.repo("erp-client");
    let kb = fx.put("kb/CLAUDE.md", "Tree knowledge.\n");
    fx.put(
        ".config/mcpm/context.json",
        &json!({"layerScaffolds": {"tree-knowledge": {
            "scope": "folder", "folders": [repo.to_string_lossy()], "imports": [kb.to_string_lossy()],
        }}})
        .to_string(),
    );
    let added = fx.add(json!({"name": "tree-knowledge"}));
    assert_eq!(added["scope"], "folder");
    assert_eq!(added["imports"], json!([kb.to_string_lossy()]));
    assert!(text(&repo.join("CLAUDE.local.md")).contains("Tree knowledge."));
    let override_folder = fx.repo("elsewhere");
    let again = fx.add(json!({"name": "tree-knowledge-2", "scope": "glob"}));
    assert_eq!(again["scope"], "glob");
    drop(override_folder);
}

#[test]
fn invalid_requests_are_refused_before_anything_is_written() {
    let fx = Fx::new("refuse");
    for args in [
        json!({"name": "a", "scope": "folder"}),
        json!({"name": "b", "scope": "folder", "folders": ["relative/dir"]}),
        json!({"name": "c", "scope": "sideways"}),
        json!({"name": "d", "delivery": "paste"}),
    ] {
        assert!(layer_manage::add(&fx.args(args.clone())).is_err(), "{args}");
    }
    assert!(!fx.home.join(".config/mcpm/skills_repo/rules").exists());
}

#[test]
fn rules_deploy_with_their_imports_and_a_folder_layer_is_not_a_user_rule() {
    let fx = Fx::new("rules");
    let repo = fx.repo("erp-client");
    let kb = fx.put("kb/CLAUDE.md", "Imported rule text.\n");
    fx.layer("client-global", &format!("scope: global\nimports: [\"{}\"]\n", kb.display()), "Rule body.");
    fx.layer("client-folder", &format!("scope: folder\nfolders: [\"{}\"]\n", repo.display()), "Folder only.");
    let roots = fx.roots();
    super::rules::deploy_rules_now(&roots, false).unwrap();
    let deployed = text(&fx.home.join(".claude/rules/client-global.md"));
    assert!(deployed.contains("Rule body.") && deployed.contains("Imported rule text."), "{deployed}");
    assert!(!fx.home.join(".claude/rules/client-folder.md").exists());
    fx.layer("client-global", "scope: folder\n", "Rule body.");
    super::rules::deploy_rules_now(&roots, false).unwrap();
    assert!(!fx.home.join(".claude/rules/client-global.md").exists(), "a rule that became folder-scoped is removed as stale");
}

#[test]
fn the_org_file_is_never_written_and_compose_reports_it_as_the_org_s() {
    let fx = Fx::new("org");
    let org = fx.put(".claude/CLAUDE.md", "Org text, version one.\n");
    fx.put(".local/share/corp-dev-tools/claude/CLAUDE.md", "Org text, version one.\n");
    let repo = fx.repo("erp-client");
    let kb = fx.put("kb/CLAUDE.md", "Own knowledge.\n");
    fx.layer("client-own", &format!("scope: folder\nfolders: [\"{}\"]\nimports: [\"{}\"]\n", repo.display(), kb.display()), "Own text.");
    let apply = |_: &str| dispatch("plus.context.apply", fx.args(json!({"rules": true}))).unwrap();
    apply("first");
    fs::write(&org, "Org text, version two, rewritten by the org tooling.\n").unwrap();
    let before = fs::read(&org).unwrap();
    apply("second");
    assert_eq!(fs::read(&org).unwrap(), before, "a deploy leaves the org file byte-identical");

    let roots = fx.roots();
    let config = load_config(&roots.context_config_path());
    let composed = compose::compose(&roots, &config, &repo).unwrap();
    let parts = composed["parts"].as_array().unwrap();
    let org_part = parts.iter().find(|p| p["path"] == json!(org.to_string_lossy())).expect("the org file is a part");
    assert_eq!(org_part["origin"]["kind"], "org");
    assert_eq!(org_part["source"], "org");
    assert_eq!(org_part["writable"], false);
    assert!(org_part["text"].as_str().unwrap().contains("version two"));
    let local = parts.iter().find(|p| p["path"].as_str().unwrap().ends_with("CLAUDE.local.md")).expect("the layer is a part");
    assert_eq!(local["layers"], json!(["client-own"]));
    assert!(local["text"].as_str().unwrap().contains("Own knowledge."));
    let order: Vec<&str> = parts.iter().map(|p| p["path"].as_str().unwrap()).collect();
    assert!(order.iter().position(|p| *p == org.to_str().unwrap()) < order.iter().position(|p| p.ends_with("CLAUDE.local.md")));
    assert_eq!(composed["total"]["basis"], "estimate");
}
