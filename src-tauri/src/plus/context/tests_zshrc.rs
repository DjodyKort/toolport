//! Toolport owns its shims: where they are written, which `~/.zshrc` lines still point into
//! mcpm's config directory, the preview/backup/rewrite of those lines and the mcpm aliases. All on
//! a synthetic HOME; the data dir is a scratch directory.

use super::{doctor, load_config, zshrc, Roots};
use crate::plus::dispatch;
use crate::plus::testutil::DataDirFx;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const ZSHRC: &str = r#"# synthetic zshrc
export PATH="$HOME/bin:$PATH"
source "$HOME/.local/share/corp-dev-tools/claude/shell-wrapper.sh"

# compression shims
source ~/.config/mcpm/compression-shims.zsh

# context shims
source ~/.config/mcpm/context-shims.zsh

[ -f ~/.config/mcpm/local-aliases.zsh ] && source ~/.config/mcpm/local-aliases.zsh
alias ll='ls -la'
"#;

const ALIASES: &str = "alias mcpmup='cd ~/mcpm.sh && ./scripts/update.sh'\n\
alias mcpmls=\"mcpm ls\"\n\
alias imp='toolportctl import mcpm'\n\
alias gs='git status'\n\
alias runmcpm='uvx mcpm run demo'\n";

const LEGACY_SHIMS: &str = "# stale copy from before the move\n";

struct World {
    _data: DataDirFx,
    home: PathBuf,
}

impl World {
    fn new(tag: &str) -> Self {
        Self::at(tag, "data")
    }

    fn at(tag: &str, data_rel: &str) -> Self {
        let data = DataDirFx::with_data_subdir("ctx-zshrc", tag, data_rel);
        let home = data.dir.join("home");
        fs::create_dir_all(&home).unwrap();
        Self { _data: data, home }
    }

    fn data_dir(&self) -> PathBuf {
        crate::registry::conduit_dir().unwrap()
    }

    fn put(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.home.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn mcpm_fixture(&self) {
        self.put(".zshrc", ZSHRC);
        self.put(".config/mcpm/local-aliases.zsh", ALIASES);
        self.put(".config/mcpm/context-shims.zsh", LEGACY_SHIMS);
        self.put(".config/mcpm/compression-shims.zsh", "# mcpm compression shims\n");
    }

    fn compression_shims_written(&self) {
        fs::create_dir_all(self.data_dir()).unwrap();
        fs::write(self.data_dir().join("compression-shims.zsh"), "# toolport shims\n").unwrap();
    }

    fn zshrc(&self) -> String {
        fs::read_to_string(self.home.join(".zshrc")).unwrap()
    }

    fn backups(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = fs::read_dir(&self.home)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(".zshrc.toolport-backup-"))
            })
            .collect();
        found.sort();
        found
    }

    fn call(&self, command: &str, mut args: Value) -> Value {
        args["home"] = json!(self.home.to_string_lossy());
        dispatch(&format!("plus.context.{command}"), args)
            .unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    fn roots(&self) -> Roots {
        let mut roots = Roots::from_home(&self.home);
        roots.shims_dir = self.data_dir();
        roots
    }
}

fn lines(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn shims_are_written_to_the_data_dir_and_never_to_mcpms_directory() {
    let w = World::new("owner");
    w.mcpm_fixture();
    let before_legacy = fs::read(w.home.join(".config/mcpm/context-shims.zsh")).unwrap();
    w.call("profileAdd", json!({"name": "work", "rules": "none", "servers": "none"}));
    let shims = w.data_dir().join("context-shims.zsh");
    let text = fs::read_to_string(&shims).unwrap();
    assert!(text.contains(&format!("#   source {}", shims.display())), "{text}");
    assert!(text.contains("claude-work()"), "{text}");
    assert_eq!(
        fs::read(w.home.join(".config/mcpm/context-shims.zsh")).unwrap(),
        before_legacy,
        "the old file is left as it is"
    );
    let status = w.call("status", json!({}));
    assert_eq!(status["shims"]["path"], shims.display().to_string());
    assert_eq!(status["shims"]["exists"], true);
    assert_eq!(status["shims"]["legacyExists"], true);
}

#[test]
fn status_lists_the_zshrc_lines_that_still_point_into_mcpm_and_the_mcpm_aliases() {
    let w = World::new("status");
    w.mcpm_fixture();
    let status = w.call("status", json!({}));
    let rc = &status["zshrc"];
    assert_eq!(rc["exists"], true);
    let pointing: Vec<(u64, &str)> = rc["legacyLines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["line"].as_u64().unwrap(), p["file"].as_str().unwrap()))
        .collect();
    assert_eq!(
        pointing,
        [
            (6, "compression-shims.zsh"),
            (9, "context-shims.zsh"),
            (11, "local-aliases.zsh"),
        ]
    );
    let aliases: Vec<&str> = rc["deadAliases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(aliases, ["mcpmup", "mcpmls", "runmcpm"]);
    let up = &rc["deadAliases"][0];
    assert_eq!(up["line"], 1);
    assert_eq!(up["command"], "cd ~/mcpm.sh && ./scripts/update.sh");
    assert!(up["file"].as_str().unwrap().ends_with(".config/mcpm/local-aliases.zsh"));
    assert_eq!(w.zshrc(), ZSHRC, "status never writes");
}

#[test]
fn a_zshrc_with_nothing_old_or_no_zshrc_reports_nothing() {
    let w = World::new("clean");
    let none = w.call("status", json!({}));
    assert_eq!(none["zshrc"]["exists"], false);
    assert_eq!(none["zshrc"]["legacyLines"], json!([]));
    let target = w.data_dir().join("context-shims.zsh");
    w.put(".zshrc", &format!("source {}\nalias gs='git status'\n", target.display()));
    let clean = w.call("status", json!({}));
    assert_eq!(clean["zshrc"]["legacyLines"], json!([]));
    assert_eq!(clean["zshrc"]["deadAliases"], json!([]));
}

#[test]
fn the_rewrite_preview_shows_the_diff_and_writes_nothing() {
    let w = World::new("preview");
    w.mcpm_fixture();
    w.compression_shims_written();
    let before = fs::read_dir(w.data_dir()).unwrap().count();
    let plan = w.call("plan", json!({"rewriteZshrc": true}));
    let rc = &plan["zshrc"];
    assert_eq!(rc["dryRun"], true);
    let data = w.data_dir();
    let at = |name: &str| data.join(name).display().to_string();
    assert_eq!(
        rc["changes"],
        json!([
            {"line": 6, "before": "source ~/.config/mcpm/compression-shims.zsh",
             "after": format!("source {}", at("compression-shims.zsh"))},
            {"line": 9, "before": "source ~/.config/mcpm/context-shims.zsh",
             "after": format!("source {}", at("context-shims.zsh"))},
            {"line": 11,
             "before": "[ -f ~/.config/mcpm/local-aliases.zsh ] && source ~/.config/mcpm/local-aliases.zsh",
             "after": format!("[ -f {0} ] && source {0}", at("local-aliases.zsh"))},
        ])
    );
    assert_eq!(rc["order"], json!({"ok": true, "problems": []}));
    assert!(rc["backup"].as_str().unwrap().contains(".zshrc.toolport-backup-"));
    let shown = lines(&plan["actions"]);
    assert!(shown.contains(&"  - 9: source ~/.config/mcpm/context-shims.zsh".to_string()), "{shown:?}");
    assert!(
        shown.contains(&format!("  + 9: source {}", at("context-shims.zsh"))),
        "{shown:?}"
    );
    assert!(shown.iter().any(|l| l.starts_with("would rewrite 3 line(s) of ")), "{shown:?}");
    assert!(shown.iter().any(|l| l.starts_with("would copy ") && l.ends_with("(the original stays)")), "{shown:?}");
    assert_eq!(w.zshrc(), ZSHRC);
    assert!(w.backups().is_empty());
    assert_eq!(fs::read_dir(w.data_dir()).unwrap().count(), before, "a preview writes nothing");
    assert!(!data.join("local-aliases.zsh").exists());
}

#[test]
fn the_rewrite_backs_up_the_zshrc_copies_user_files_and_keeps_the_order() {
    let w = World::new("apply");
    w.mcpm_fixture();
    w.compression_shims_written();
    let out = w.call("apply", json!({"rewriteZshrc": true}));
    let data = w.data_dir();
    let at = |name: &str| data.join(name).display().to_string();

    let rewritten = w.zshrc();
    assert_eq!(
        rewritten,
        ZSHRC
            .replace("~/.config/mcpm/compression-shims.zsh", &at("compression-shims.zsh"))
            .replace("~/.config/mcpm/context-shims.zsh", &at("context-shims.zsh"))
            .replace("~/.config/mcpm/local-aliases.zsh", &at("local-aliases.zsh"))
    );
    let backups = w.backups();
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert_eq!(fs::read_to_string(&backups[0]).unwrap(), ZSHRC);
    assert_eq!(out["zshrc"]["backup"], backups[0].display().to_string());
    assert_eq!(out["zshrc"]["order"]["ok"], true);

    assert!(data.join("context-shims.zsh").is_file(), "the shims are written before the line points at them");
    assert_eq!(fs::read_to_string(data.join("local-aliases.zsh")).unwrap(), ALIASES);
    assert_eq!(
        fs::read_to_string(w.home.join(".config/mcpm/local-aliases.zsh")).unwrap(),
        ALIASES,
        "the original is not moved or edited"
    );
    assert_eq!(
        fs::read_to_string(w.home.join(".config/mcpm/context-shims.zsh")).unwrap(),
        LEGACY_SHIMS
    );
    let last = |name: &str| rewritten.lines().position(|l| l.contains(name)).unwrap();
    assert!(last("shell-wrapper.sh") < last("compression-shims.zsh"));
    assert!(last("compression-shims.zsh") < last("context-shims.zsh"));

    let status = w.call("status", json!({}));
    assert_eq!(status["zshrc"]["legacyLines"], json!([]));
    let aliases = status["zshrc"]["deadAliases"].as_array().unwrap();
    assert_eq!(aliases.len(), 3, "the copy is what the rc file sources now: {aliases:?}");
    assert!(aliases.iter().all(|a| a["file"] == at("local-aliases.zsh")));
}

#[test]
fn a_second_rewrite_changes_nothing_and_makes_no_backup() {
    let w = World::new("rerun");
    w.mcpm_fixture();
    w.compression_shims_written();
    w.call("apply", json!({"rewriteZshrc": true}));
    let after_first = w.zshrc();
    let again = w.call("apply", json!({"rewriteZshrc": true}));
    assert_eq!(again["zshrc"]["changes"], json!([]));
    assert_eq!(again["zshrc"]["backup"], Value::Null);
    assert_eq!(w.zshrc(), after_first);
    assert_eq!(w.backups().len(), 1);
}

#[test]
fn without_the_flag_sync_never_touches_the_zshrc() {
    let w = World::new("noflag");
    w.mcpm_fixture();
    let out = w.call("apply", json!({}));
    assert!(out.get("zshrc").is_none());
    assert_eq!(w.zshrc(), ZSHRC);
    assert!(w.backups().is_empty());
}

#[test]
fn lines_whose_target_does_not_exist_yet_are_left_and_explained() {
    let w = World::new("skip");
    w.mcpm_fixture();
    let out = w.call("init", json!({"rewriteZshrc": true}));
    let rc = &out["zshrc"];
    let skipped: Vec<(u64, String)> = rc["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["line"].as_u64().unwrap(), s["reason"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    assert!(skipped[0].1.contains("toolportctl compression sync"), "{skipped:?}");
    assert!(skipped[1].1.contains("toolportctl context sync"), "{skipped:?}");
    let changed: Vec<u64> = rc["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["line"].as_u64().unwrap())
        .collect();
    assert_eq!(changed, [11], "only the user-owned file can move before the shims exist");
    let zshrc = w.zshrc();
    assert!(zshrc.contains("source ~/.config/mcpm/compression-shims.zsh"));
    assert!(zshrc.contains("source ~/.config/mcpm/context-shims.zsh"));
}

#[test]
fn a_context_shims_line_above_compression_is_reported_not_reordered() {
    let w = World::new("order");
    let text = "source ~/.config/mcpm/context-shims.zsh\nsource ~/.config/mcpm/compression-shims.zsh\n";
    w.put(".zshrc", text);
    w.put(".config/mcpm/context-shims.zsh", LEGACY_SHIMS);
    w.compression_shims_written();
    let out = w.call("plan", json!({"rewriteZshrc": true}));
    let rc = &out["zshrc"];
    assert_eq!(rc["order"]["ok"], false);
    let problem = rc["order"]["problems"][0].as_str().unwrap();
    assert!(problem.contains("context-shims.zsh (line 1) is sourced before compression-shims.zsh (line 2)"), "{problem}");
    assert_eq!(rc["changes"][0]["line"], 1, "the lines still move, in place");
    assert!(lines(&out["warnings"]).iter().any(|l| l.contains("is sourced before compression-shims.zsh")));
}

#[test]
fn the_rewrite_keeps_the_home_style_quotes_symlinks_and_modes() {
    let w = World::at("style", "home/Library/Application Support/Toolport");
    w.put(".config/mcpm/local-aliases.zsh", "alias gs='git status'\n");
    let dotfiles = w.put("dotfiles/zshrc", "source \"$HOME/.config/mcpm/local-aliases.zsh\"\n. ${HOME}/.config/mcpm/local-aliases.zsh\nsource '/nowhere/.config/mcpm/x.zsh'\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};
        fs::set_permissions(&dotfiles, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&dotfiles, w.home.join(".zshrc")).unwrap();
    }
    let data = w.data_dir();
    assert!(data.starts_with(&w.home) && data.to_string_lossy().contains(' '));
    w.call("apply", json!({"rewriteZshrc": true}));
    let written = fs::read_to_string(&dotfiles).unwrap();
    assert_eq!(
        written,
        "source \"$HOME/Library/Application Support/Toolport/local-aliases.zsh\"\n\
         . \"${HOME}/Library/Application Support/Toolport/local-aliases.zsh\"\n\
         source '/nowhere/.config/mcpm/x.zsh'\n"
    );
    if let Ok(checked) = std::process::Command::new("zsh").arg("-n").arg(&dotfiles).output() {
        assert!(checked.status.success(), "{}", String::from_utf8_lossy(&checked.stderr));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert!(fs::symlink_metadata(w.home.join(".zshrc")).unwrap().file_type().is_symlink());
        assert_eq!(fs::metadata(&dotfiles).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn an_unquoted_tilde_line_stays_a_tilde_line_when_the_data_dir_is_under_home() {
    let w = World::at("tilde", "home/.local/share/toolport");
    w.put(".zshrc", "source ~/.config/mcpm/local-aliases.zsh\n");
    w.put(".config/mcpm/local-aliases.zsh", "alias gs='git status'\n");
    w.call("apply", json!({"rewriteZshrc": true}));
    assert_eq!(w.zshrc(), "source ~/.local/share/toolport/local-aliases.zsh\n");
}

#[test]
fn files_that_are_not_shell_files_or_already_differ_are_not_overwritten() {
    let w = World::new("keep");
    w.put(".zshrc", "source ~/.config/mcpm/notes.txt\nsource ~/.config/mcpm/other.zsh\nsource ~/.config/mcpm/sub/x.zsh\nsource ~/.config/mcpm/gone.zsh\n");
    w.put(".config/mcpm/notes.txt", "x");
    w.put(".config/mcpm/other.zsh", "echo mcpm\n");
    fs::create_dir_all(w.data_dir()).unwrap();
    fs::write(w.data_dir().join("other.zsh"), "echo different\n").unwrap();
    let out = w.call("apply", json!({"rewriteZshrc": true}));
    assert_eq!(out["zshrc"]["changes"], json!([]));
    assert_eq!(w.zshrc().matches("~/.config/mcpm/").count(), 4);
    assert_eq!(fs::read_to_string(w.data_dir().join("other.zsh")).unwrap(), "echo different\n");
    let reasons: Vec<String> = out["zshrc"]["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["reason"].as_str().unwrap().to_string())
        .collect();
    assert!(reasons.iter().any(|r| r.contains("not a shell file")), "{reasons:?}");
    assert!(reasons.iter().any(|r| r.contains("exists and differs")), "{reasons:?}");
    assert!(reasons.iter().any(|r| r.contains("subdirectory")), "{reasons:?}");
    assert!(reasons.iter().any(|r| r.contains("does not exist")), "{reasons:?}");
    assert!(w.backups().is_empty());
}

#[test]
fn only_aliases_that_run_mcpm_are_listed() {
    let w = World::new("aliases");
    let roots = w.roots();
    let found = zshrc::dead_aliases(
        &roots,
        "alias ll='ls -la'\n\
         alias imp='toolportctl import mcpm'\n\
         alias mcpmup='cd ~/mcpm.sh && ./update.sh'\n\
         alias up2='uv run mcpm update'\n\
         alias cfg='vim ~/.config/mcpm/servers.json'\n\
         mcpm_helper() { echo hi; }\n\
         claude() { mcpm_context_presync; command claude \"$@\"; }\n\
         # alias mcpmcomment='mcpm ls'\n",
    );
    let names: Vec<&str> = found.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["mcpmup", "up2", "cfg", "mcpm_helper"]);
}

#[test]
fn doctor_points_a_legacy_context_shims_line_at_the_rewrite() {
    let w = World::new("doctor");
    w.mcpm_fixture();
    w.call("apply", json!({}));
    let roots = w.roots();
    let config = load_config(&roots.context_config_path());
    let checks = doctor::run_checks(&roots, &config);
    let text: Vec<&str> = checks.iter().map(|c| c.1.as_str()).collect();
    assert!(
        text.iter().any(|t| t.contains("context-shims is still sourced from")
            && t.contains("context sync --rewrite-zshrc --dry-run")),
        "{text:?}"
    );
    assert!(
        text.iter().any(|t| t.contains("still points into") && t.contains("compression-shims.zsh")),
        "{text:?}"
    );
    w.compression_shims_written();
    w.call("apply", json!({"rewriteZshrc": true}));
    let checks = doctor::run_checks(&roots, &load_config(&roots.context_config_path()));
    let text: Vec<&str> = checks.iter().map(|c| c.1.as_str()).collect();
    assert!(text.contains(&"shims written and sourced (last)"), "{text:?}");
    assert!(!text.iter().any(|t| t.contains("still points into")), "{text:?}");
}

#[test]
fn roots_that_keep_the_legacy_directory_have_nothing_to_rewrite() {
    let w = World::new("legacy-roots");
    w.mcpm_fixture();
    let roots = Roots::from_home(&w.home);
    assert_eq!(roots.shims_path(), roots.legacy_shims_path());
    assert!(zshrc::inspect(&roots).legacy_lines.is_empty());
    let plan = zshrc::plan(&roots, true).unwrap();
    assert!(plan.changes.is_empty() && !plan.moves);
    let lines = zshrc::apply(plan, false).unwrap().lines().0;
    assert!(lines[0].contains("nothing to rewrite"), "{lines:?}");
    assert_eq!(w.zshrc(), ZSHRC);
    let header = super::shims::shim_snippet(&roots, &Default::default(), true);
    assert!(header.contains("#   source ~/.config/mcpm/context-shims.zsh"), "{header}");
}

#[test]
fn the_shell_path_quotes_what_the_shell_would_split() {
    let home = Path::new("/h");
    assert_eq!(zshrc::shell_path(home, Path::new("/h/.config/x/a.zsh")), "~/.config/x/a.zsh");
    assert_eq!(
        zshrc::shell_path(home, Path::new("/h/Library/Application Support/x/a.zsh")),
        "\"$HOME/Library/Application Support/x/a.zsh\""
    );
    assert_eq!(zshrc::shell_path(home, Path::new("/srv/data dir/a.zsh")), "\"/srv/data dir/a.zsh\"");
}
