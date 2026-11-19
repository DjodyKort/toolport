//! A tap checkout that tries to read local files: every kind of link, in every place a reader
//! looks, next to a few honest items.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

pub const SECRET: &str = "TOPSECRET-local-file";

pub fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

pub fn skill_text(name: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: A skill\n---\n{body}\n")
}

/// Builds the hostile tap in `tap`; the files it points at go to `outside`.
pub fn hostile_tap(tap: &Path, outside: &Path) {
    let secret = outside.join("secret.txt");
    put(&secret, SECRET);
    put(
        &outside.join("skill-dir/SKILL.md"),
        &skill_text("linked-dir", SECRET),
    );
    put(&outside.join("agents-dir/leak.md"), SECRET);
    put(
        &outside.join("hooks.json"),
        &format!(
            r#"{{"hooks": {{"PostToolUse": [{{"matcher": "Write", "hooks": [
                {{"type": "command", "command": "echo {SECRET}"}}]}}]}}}}"#
        ),
    );
    put(&outside.join("plugin-dir/skills/leak/SKILL.md"), SECRET);

    put(
        &tap.join("skills/good/SKILL.md"),
        &skill_text("good", "An honest skill."),
    );
    put(&tap.join("rules/style.md"), "Keep functions short.\n");
    symlink(outside.join("skill-dir"), tap.join("skills/linked-dir")).unwrap();
    fs::create_dir_all(tap.join("skills/linked-file")).unwrap();
    symlink(&secret, tap.join("skills/linked-file/SKILL.md")).unwrap();
    put(
        &tap.join("skills/nested/SKILL.md"),
        &skill_text("nested", "Has a link inside."),
    );
    symlink(&secret, tap.join("skills/nested/notes.txt")).unwrap();
    symlink(outside, tap.join("skills/nested/more")).unwrap();
    fs::create_dir_all(tap.join("commands")).unwrap();
    symlink(&secret, tap.join("commands/top.md")).unwrap();
    symlink(outside.join("agents-dir"), tap.join("agents")).unwrap();
    fs::create_dir_all(tap.join("hooks")).unwrap();
    symlink(outside.join("hooks.json"), tap.join("hooks/hooks.json")).unwrap();
}
