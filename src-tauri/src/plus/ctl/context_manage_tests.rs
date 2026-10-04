use super::*;
use crate::plus::ctl::{find_command, run_with};
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const SECRET: &str = "SYNTHETIC-NOT-A-REAL-CREDENTIAL";

struct Fx {
    base: DataDirFx,
    home: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let base = DataDirFx::new("ctl-context-manage", tag);
        let home = base.dir.join("home");
        fs::create_dir_all(&home).unwrap();
        Self { base, home }
    }

    fn put(&self, rel: &str, text: &str) {
        let path = self.home.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn run(&self, argv: &[&str]) -> (i32, String, String) {
        self.run_at(&self.home, argv)
    }

    fn run_at(&self, home: &Path, argv: &[&str]) -> (i32, String, String) {
        let mut list: Vec<String> = ["context"].iter().map(|s| s.to_string()).collect();
        list.extend(argv.iter().map(|s| s.to_string()));
        list.extend(["--home".to_string(), home.to_string_lossy().into_owned()]);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(&list, &mut out, &mut err);
        let shown = |bytes: Vec<u8>| {
            String::from_utf8(bytes)
                .unwrap()
                .replace(&*home.to_string_lossy(), "<HOME>")
        };
        (code, shown(out), shown(err))
    }

    fn text(&self, argv: &[&str]) -> String {
        let (code, out, err) = self.run(argv);
        assert_eq!(code, 0, "{argv:?}: {out}{err}");
        out.trim_end_matches('\n').to_string()
    }

    fn json(&self, argv: &[&str]) -> Value {
        let mut list = vec!["--json"];
        list.extend_from_slice(argv);
        let (code, out, err) = self.run(&list);
        assert_eq!(code, 0, "{argv:?}: {out}{err}");
        assert_eq!(out.trim().lines().count(), 1, "{out}");
        serde_json::from_str(out.trim()).unwrap()
    }

    fn snapshot(&self) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
        tree_snapshot(&self.home)
    }
}

#[test]
fn py_repr_follows_python_quoting() {
    assert_eq!(py_repr("acme"), "'acme'");
    assert_eq!(py_repr("it's"), "\"it's\"");
    assert_eq!(py_repr("say \"hi\""), "'say \"hi\"'");
    assert_eq!(py_repr("both ' and \""), "'both \\' and \"'");
    assert_eq!(py_repr("a\\b"), "'a\\\\b'");
    assert_eq!(py_repr("a\nb\tc\r"), "'a\\nb\\tc\\r'");
    assert_eq!(py_repr("bell\u{7}"), "'bell\\x07'");
    assert_eq!(py_repr("ü"), "'ü'");
}

#[test]
fn empty_state_hints_name_the_wired_commands() {
    let fx = Fx::new("empty");
    assert_eq!(
        fx.text(&["status"]),
        "layers            none — run `toolportctl context init`\n\
         profiles          none\n\
         legacy MCP dupes  none\n\
         shims             none"
    );
    assert_eq!(
        fx.text(&["client", "list"]),
        "  no layers — run `toolportctl context init`"
    );
    assert_eq!(
        fx.text(&["profile", "list"]),
        "  no profiles — `toolportctl context profile add <name>`"
    );
    assert_eq!(fx.text(&["disable"]), "  nothing to remove");
}

#[test]
fn init_prints_the_scaffold_the_migration_and_the_next_steps() {
    let fx = Fx::new("init");
    fx.put(
        ".claude/settings.local.json",
        &serde_json::json!({"permissions": {"allow": ["Bash(ls:*)"]}}).to_string(),
    );
    let first = fx.text(&["init"]);
    assert_eq!(
        first,
        "  ✓ scaffolded personal layer: <HOME>/.config/mcpm/skills_repo/rules/personal/SKILL.md\n\
         \n\
         \x20 ! <HOME>/.claude/settings.local.json is not read by Claude Code at user level (unsupported).\n\
         \x20     1 allow entr(y/ies) found; 1 migratable.\n\
         \x20 ✓ migrated 1 entries into ensure_allow\n\
         \x20 ✓ saved context.json\n\
         \n\
         Next steps:\n\
         \x20 1. Edit your personal layer, then:  toolportctl skills sync\n\
         \x20 2. Per-client layer:                toolportctl context client add <name>\n\
         \x20 3. Profiles:                        toolportctl context profile add bare --no-org --rules none --servers none\n\
         \x20 4. Apply everything:                toolportctl context sync"
    );
    let again = fx.text(&["init"]);
    assert!(
        again.starts_with("  personal layer already scaffolded\n"),
        "{again}"
    );
    assert!(again.contains("0 migratable"), "{again}");
}

#[test]
fn init_yes_is_accepted_and_changes_nothing() {
    let fx = Fx::new("init-yes");
    let other = fx.base.dir.join("other-home");
    let entries =
        serde_json::json!({"permissions": {"allow": ["Bash(ls:*)", "Bash(git log)"]}}).to_string();
    for home in [&fx.home, &other] {
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::write(home.join(".claude/settings.local.json"), &entries).unwrap();
    }
    let (code, plain, _) = fx.run_at(&fx.home, &["init"]);
    assert_eq!(code, 0);
    let (code, yes, _) = fx.run_at(&other, &["init", "--yes"]);
    assert_eq!(code, 0);
    assert_eq!(plain, yes);
    assert_eq!(tree_snapshot(&fx.home), tree_snapshot(&other));
    assert!(other.join(".claude/settings.local.json").is_file());
}

#[test]
fn init_never_prints_a_credential_in_text_or_json() {
    let fx = Fx::new("init-secret");
    let settings = serde_json::json!({"permissions": {"allow": [
        "Bash(ls:*)",
        format!("Bash(deploy --token={SECRET})"),
    ]}})
    .to_string();
    fx.put(".claude/settings.local.json", &settings);

    let text = fx.text(&["init", "--dry-run"]);
    assert!(
        text.contains("2 allow entr(y/ies) found; 1 migratable."),
        "{text}"
    );
    assert!(
        text.contains("1 entr(y/ies) embed credentials — NOT migrating; rotate + re-add manually:")
    );
    assert!(text.contains("\n        Bash(deploy --token=…\n"), "{text}");
    assert!(!text.contains(SECRET));

    let json = fx.json(&["init"]);
    assert!(!json.to_string().contains(SECRET));
    assert_eq!(
        json["data"]["migration"]["credentials"][0],
        "Bash(deploy --token=…"
    );
    let saved = fs::read_to_string(fx.home.join(".config/mcpm/context.json")).unwrap();
    assert!(!saved.contains(SECRET));
}

#[test]
fn client_add_texts_cover_new_existing_and_dry_run() {
    let fx = Fx::new("client-text");
    assert_eq!(
        fx.text(&["client", "add", "acme", "--dry-run"]),
        "  would scaffold <HOME>/.config/mcpm/skills_repo/rules/client-acme/SKILL.md\n  dry run: nothing was written"
    );
    assert_eq!(
        fx.text(&["client", "add", "acme"]),
        "  ✓ scaffolded <HOME>/.config/mcpm/skills_repo/rules/client-acme/SKILL.md\n  fill in the body, then:  toolportctl skills sync"
    );
    assert_eq!(
        fx.text(&["client", "add", "acme"]),
        "  client layer 'acme' already exists"
    );
    fx.text(&["client", "add", "v18_arp", "--glob", "**/v18/**"]);
    assert_eq!(
        fx.text(&["client", "list"]),
        "  client-acme  **/clients/acme/**\n  client-v18-arp  **/v18/**"
    );
}

#[test]
fn profile_add_and_remove_print_the_actions_and_warnings() {
    let fx = Fx::new("profile-text");
    let added = fx.text(&[
        "profile",
        "add",
        "work",
        "--rules",
        "none",
        "--servers",
        "none",
    ]);
    let shims = fx.base.dir.join("context-shims.zsh").display().to_string();
    assert_eq!(
        added,
        format!(
            "  ✓ generated launch profile work (0 server(s)) in <HOME>/.config/mcpm/claude-profiles/work\n\
             \x20 ✓ wrote shims: {shims}\n\
             \x20 ✓ saved config (1 profile(s))"
        )
    );
    assert_eq!(
        fx.text(&["profile", "list"]),
        "  claude-work  org=on(import)  rules=none  servers=none"
    );
    assert_eq!(
        fx.text(&["status"]),
        format!(
            "layers            none — run `toolportctl context init`\n\
             profile work      org=on(import) rules=none servers=none\n\
             legacy MCP dupes  none\n\
             shims             {shims}"
        )
    );

    let removed = fx.text(&["profile", "remove", "work"]);
    assert_eq!(
        removed,
        format!(
            "  ✓ wrote shims: {shims}\n\
             \x20 ✓ saved config (0 profile(s))\n\
             \x20 ! orphan profile dir <HOME>/.config/mcpm/claude-profiles/work (not in config) — `toolportctl context profile remove work --purge`"
        )
    );
    let purged = fx.text(&["profile", "remove", "work", "--purge"]);
    assert!(purged.starts_with(
        "  ! no profile 'work' in config\n  ✓ removed profile dir <HOME>/.config/mcpm/claude-profiles/work\n"
    ));
    assert!(!fx.home.join(".config/mcpm/claude-profiles/work").exists());
    assert_eq!(
        fx.text(&["profile", "remove", "it's"])
            .lines()
            .next()
            .unwrap(),
        "  ! no profile \"it's\" in config"
    );
}

#[test]
fn profile_add_flags_reach_the_profile() {
    let fx = Fx::new("profile-flags");
    fx.text(&[
        "profile",
        "add",
        "lean",
        "--no-org",
        "--org-mode",
        "copy",
        "--rules",
        "personal,client-acme",
        "--servers",
        "alpha",
        "--no-commands",
        "--no-skills",
    ]);
    let data = fx.json(&["profile", "list"]);
    let profile = &data["data"]["profiles"][0];
    assert_eq!(profile["org"], false);
    assert_eq!(profile["orgMode"], "copy");
    assert_eq!(
        profile["rules"],
        serde_json::json!(["personal", "client-acme"])
    );
    assert_eq!(profile["servers"], serde_json::json!(["alpha"]));
    assert_eq!(
        fx.text(&["profile", "list"]),
        "  claude-lean  org=off(copy)  rules=personal,client-acme  servers=alpha"
    );
}

#[test]
fn status_lists_layers_with_their_globs_and_counts_server_selections() {
    let fx = Fx::new("status-rows");
    fx.text(&["init"]);
    fx.text(&["client", "add", "acme"]);
    fx.text(&["profile", "add", "bare", "--no-org", "--servers", "a,b,c"]);
    fx.put(
        ".claude.json",
        "{\"mcpServers\": {\"context7\": {}, \"mcpm_context7\": {}}}",
    );
    let status = fx.text(&["status"]);
    let lines: Vec<&str> = status.lines().collect();
    assert_eq!(
        lines[0],
        "layers            client-acme [**/clients/acme/**], personal"
    );
    assert_eq!(
        lines[1],
        "profile bare      org=off(import) rules=inherit servers=3 selected"
    );
    assert_eq!(lines[2], "legacy MCP dupes  context7");
}

#[test]
fn dry_run_writes_nothing_and_says_so() {
    let fx = Fx::new("dry");
    fx.text(&[
        "profile",
        "add",
        "work",
        "--rules",
        "none",
        "--servers",
        "none",
    ]);
    fx.put(".config/mcpm/claude-profiles/stale/CLAUDE.md", "old\n");
    let before = fx.snapshot();
    for argv in [
        vec!["init", "--dry-run"],
        vec!["client", "add", "acme", "--dry-run"],
        vec!["profile", "add", "other", "--dry-run"],
        vec!["profile", "remove", "work", "--purge", "--dry-run"],
        vec!["profile", "remove", "stale", "--purge", "--dry-run"],
        vec!["disable", "--purge-profiles", "--dry-run"],
    ] {
        let out = fx.text(&argv);
        assert!(
            out.ends_with("\n  dry run: nothing was written"),
            "{argv:?}: {out}"
        );
        assert_eq!(fx.snapshot(), before, "{argv:?}");
        let json = fx.json(&argv);
        assert_eq!(json["data"]["dryRun"], true, "{argv:?}");
        assert_eq!(fx.snapshot(), before, "{argv:?}");
    }
}

#[test]
fn json_output_is_one_envelope_with_the_handler_data() {
    let fx = Fx::new("json");
    let status = fx.json(&["status"]);
    assert_eq!(status["ok"], true);
    assert_eq!(status["command"], "context status");
    assert_eq!(status["data"]["layers"], serde_json::json!([]));
    let added = fx.json(&["client", "add", "acme"]);
    assert_eq!(added["command"], "context client add");
    assert_eq!(added["data"]["rule"], "client-acme");
    assert_eq!(added["data"]["created"], true);
    let removed = fx.json(&["profile", "remove", "ghost"]);
    assert_eq!(removed["data"]["inConfig"], false);
    let disabled = fx.json(&["disable"]);
    assert_eq!(
        disabled["data"]["shims"]["removed"], true,
        "the remove above wrote the shims file"
    );
    assert_eq!(fx.json(&["disable"])["data"]["shims"]["removed"], false);
}

#[test]
fn bad_input_is_a_usage_error_and_writes_nothing() {
    let fx = Fx::new("usage");
    let before = fx.snapshot();
    let cases: Vec<Vec<&str>> = vec![
        vec!["client"],
        vec!["profile"],
        vec!["client", "add"],
        vec!["client", "add", "acme", "extra"],
        vec!["client", "add", ""],
        vec!["client", "add", "  "],
        vec!["client", "add", "a\nb"],
        vec!["client", "add", "a---b"],
        vec!["client", "add", "acme", "--glob", "x\n---\nname: evil"],
        vec!["client", "list", "extra"],
        vec!["profile", "add"],
        vec!["profile", "add", "Bad Name"],
        vec!["profile", "add", "../x"],
        vec!["profile", "add", "ok", "--org-mode", "weird"],
        vec!["profile", "add", "ok", "--rules"],
        vec!["profile", "remove"],
        vec!["profile", "remove", "../victim", "--purge"],
        vec!["profile", "remove", "a/b", "--purge"],
        vec!["profile", "remove", "..", "--purge"],
        vec!["profile", "list", "extra"],
        vec!["init", "extra"],
        vec!["init", "--nope"],
        vec!["status", "extra"],
        vec!["disable", "extra"],
        vec!["disable", "--purge"],
    ];
    for argv in cases {
        let (code, out, err) = fx.run(&argv);
        assert_eq!(code, 2, "{argv:?}: {out}{err}");
        assert!(
            err.contains("toolportctl: ") || out.contains("toolportctl: "),
            "{argv:?}"
        );
        assert_eq!(fx.snapshot(), before, "{argv:?}");
    }
}

#[test]
fn bare_groups_print_their_usage() {
    let fx = Fx::new("groups");
    let (code, out, err) = fx.run(&["client"]);
    assert_eq!(code, 2);
    assert!(format!("{out}{err}").contains(CLIENT_GROUP_USAGE));
    let (code, out, err) = fx.run(&["profile"]);
    assert_eq!(code, 2);
    assert!(format!("{out}{err}").contains(PROFILE_GROUP_USAGE));
}

#[test]
fn every_new_command_is_resolved_by_the_table_and_not_planned() {
    for path in [
        "context init",
        "context status",
        "context client add",
        "context client list",
        "context profile add",
        "context profile list",
        "context profile remove",
        "context disable",
    ] {
        let words: Vec<String> = path.split(' ').map(String::from).collect();
        let (command, rest) = find_command(&words).unwrap_or_else(|| panic!("{path}"));
        assert_eq!(command.path.join(" "), path);
        assert!(rest.is_empty());
        assert!(!command.planned(), "{path}");
    }
}

const MCPM_ZSHRC: &str = "source \"$HOME/.local/share/corp-dev-tools/claude/shell-wrapper.sh\"\n\
source ~/.config/mcpm/compression-shims.zsh\n\
source ~/.config/mcpm/context-shims.zsh\n\
source ~/.config/mcpm/local-aliases.zsh\n";

fn mcpm_home(fx: &Fx) {
    fx.put(".zshrc", MCPM_ZSHRC);
    fx.put(
        ".config/mcpm/local-aliases.zsh",
        "alias mcpmup='cd ~/mcpm.sh && ./scripts/update.sh'\nalias gs='git status'\n",
    );
    fx.put(".config/mcpm/compression-shims.zsh", "# mcpm\n");
    fx.put(".config/mcpm/context-shims.zsh", "# mcpm\n");
    fs::write(fx.base.dir.join("compression-shims.zsh"), "# toolport\n").unwrap();
}

fn plain(fx: &Fx, text: String) -> String {
    text.replace(&*fx.base.dir.to_string_lossy(), "<DATA>")
}

#[test]
fn status_and_the_zshrc_rewrite_print_the_old_lines_the_diff_and_the_backup() {
    let fx = Fx::new("zshrc-text");
    mcpm_home(&fx);
    let status = plain(&fx, fx.text(&["status"]));
    assert_eq!(
        status,
        "layers            none — run `toolportctl context init`\n\
         profiles          none\n\
         legacy MCP dupes  none\n\
         shims             none\n\
         zshrc             3 line(s) still point into mcpm's config directory: line 2 compression-shims.zsh, line 3 context-shims.zsh, line 4 local-aliases.zsh — `toolportctl context sync --rewrite-zshrc --dry-run` previews the new paths\n\
         mcpm alias        mcpmup (<HOME>/.config/mcpm/local-aliases.zsh:1) runs `cd ~/mcpm.sh && ./scripts/update.sh`; remove it when mcpm is gone"
    );

    let before = fx.snapshot();
    let preview = plain(&fx, fx.text(&["sync", "--rewrite-zshrc", "--dry-run"]));
    assert_eq!(fx.snapshot(), before, "the preview writes nothing");
    let plan_part = preview.as_str();
    for line in [
        "would copy <HOME>/.config/mcpm/local-aliases.zsh to <DATA>/local-aliases.zsh (the original stays)",
        "would rewrite 3 line(s) of <HOME>/.zshrc",
        "  - 2: source ~/.config/mcpm/compression-shims.zsh",
        "  + 2: source <DATA>/compression-shims.zsh",
        "  - 3: source ~/.config/mcpm/context-shims.zsh",
        "  + 3: source <DATA>/context-shims.zsh",
        "  - 4: source ~/.config/mcpm/local-aliases.zsh",
        "  + 4: source <DATA>/local-aliases.zsh",
        "warning: mcpm alias mcpmup (<HOME>/.config/mcpm/local-aliases.zsh:1) runs `cd ~/mcpm.sh && ./scripts/update.sh`; it stops working once mcpm is gone, remove it yourself (nothing is deleted)",
    ] {
        assert!(plan_part.lines().any(|l| l == line), "missing {line:?} in\n{plan_part}");
    }
    assert!(
        plan_part.lines().any(|l| l.starts_with("would back up <HOME>/.zshrc as <HOME>/.zshrc.toolport-backup-")),
        "{plan_part}"
    );
    assert_eq!(fx.json(&["sync", "--rewrite-zshrc", "--dry-run"])["data"]["plan"]["zshrc"]["order"]["ok"], true);

    let applied = plain(&fx, fx.text(&["sync", "--rewrite-zshrc"]));
    let (_, apply_part) = applied.split_once("apply:\n").expect("sync prints both");
    assert!(apply_part.contains("rewrote 3 line(s) of <HOME>/.zshrc"), "{apply_part}");
    assert!(apply_part.contains("backed up <HOME>/.zshrc as <HOME>/.zshrc.toolport-backup-"), "{apply_part}");
    assert_eq!(
        fs::read_to_string(fx.home.join(".zshrc")).unwrap(),
        MCPM_ZSHRC
            .replace("~/.config/mcpm/", &format!("{}/", fx.base.dir.display()))
    );
    let backup = fs::read_dir(&fx.home)
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with(".zshrc.toolport-backup-"))
        .expect("a timestamped backup next to the rc file");
    assert_eq!(fs::read_to_string(backup.path()).unwrap(), MCPM_ZSHRC);

    let after = plain(&fx, fx.text(&["status"]));
    assert!(after.contains("shims             <DATA>/context-shims.zsh"), "{after}");
    assert!(!after.contains("still point into"), "{after}");
    assert!(after.contains("mcpm alias        mcpmup (<DATA>/local-aliases.zsh:1)"), "{after}");
}

#[test]
fn init_can_rewrite_the_zshrc_and_previews_it_with_dry_run() {
    let fx = Fx::new("zshrc-init");
    mcpm_home(&fx);
    let before = fx.snapshot();
    let preview = plain(&fx, fx.text(&["init", "--rewrite-zshrc", "--dry-run"]));
    assert_eq!(fx.snapshot(), before);
    assert!(preview.contains("  would rewrite 2 line(s) of <HOME>/.zshrc"), "{preview}");
    assert!(preview.contains("    + 2: source <DATA>/compression-shims.zsh"), "{preview}");
    assert!(preview.contains("    + 4: source <DATA>/local-aliases.zsh"), "{preview}");
    assert!(
        preview.contains("  ! <HOME>/.zshrc:3 context-shims.zsh: not written yet: run `toolportctl context sync`"),
        "{preview}"
    );
    assert!(preview.contains("Next steps:"), "{preview}");
}
