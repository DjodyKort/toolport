//! Where a restore may write. A setup file is data from outside: every destination is derived
//! here from a fixed list of places, never taken from the file. A name from the file is only ever
//! the last part of a path under a root that Toolport chose, and only when it matches the list.

use std::path::{Path, PathBuf};

use super::mem::INSTRUCTION_PATHS;

/// Files directly under `~/.claude` that a setup file may carry and a restore writes.
const CLAUDE_USER_FILES: [&str; 2] = ["CLAUDE.md", "keybindings.json"];

/// Folders under `~/.claude` that a setup file may carry and a restore writes into.
const CLAUDE_USER_DIRS: [&str; 5] = ["rules", "agents", "commands", "output-styles", "workflows"];

const MAX_PART_BYTES: usize = 255;

/// The parts of a relative path written with `/`. `.`, `..`, empty parts, an absolute path, a
/// backslash and control characters are refused instead of cleaned up, so a name is either plain
/// or does not get past this point.
pub fn plain_parts(rel: &str) -> Result<Vec<&str>, String> {
    if rel.is_empty() {
        return Err("the name is empty".into());
    }
    if rel.starts_with('/') {
        return Err("the path is absolute".into());
    }
    if rel.contains('\\') {
        return Err("the path has a backslash".into());
    }
    if rel.chars().any(char::is_control) {
        return Err("the path has a control character".into());
    }
    let parts: Vec<&str> = rel.split('/').collect();
    for part in &parts {
        match *part {
            "" => return Err("the path has an empty part".into()),
            "." | ".." => return Err(format!("the path has a {part:?} part")),
            _ if part.len() > MAX_PART_BYTES => return Err("a part of the path is too long".into()),
            _ => {}
        }
    }
    Ok(parts)
}

/// A path the file wants a repository or clone at: absolute or under `~`, never walking up.
pub fn no_parent_parts(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("the path is empty".into());
    }
    if text.chars().any(char::is_control) {
        return Err("the path has a control character".into());
    }
    if text.split(['/', '\\']).any(|part| part == "..") {
        return Err("the path walks up with ..".into());
    }
    Ok(())
}

/// A repository a restore may clone sits below the home folder: a setup file comes from another
/// machine and must not pick a place like `/Volumes/..` or `/etc` on this one.
pub fn clone_target(path: &str, abs: &Path, home: &Path) -> Result<(), String> {
    no_parent_parts(path)?;
    if abs == home || !abs.starts_with(home) {
        return Err(format!("{path} is not inside the home folder"));
    }
    Ok(())
}

/// `rel` under `~/.claude` is one of the files or folders a restore writes.
pub fn claude_user(parts: &[&str]) -> Result<(), String> {
    match parts {
        [file] if CLAUDE_USER_FILES.contains(file) => Ok(()),
        [dir, _, ..] if CLAUDE_USER_DIRS.contains(dir) => Ok(()),
        _ => Err("it is not a Claude Code user file that a restore writes".into()),
    }
}

/// `name` is an instruction file of a repository: one of the known paths, or a rule under
/// `.claude/rules`.
pub fn instruction(parts: &[&str]) -> Result<(), String> {
    let joined = parts.join("/");
    if INSTRUCTION_PATHS.contains(&joined.as_str()) {
        return Ok(());
    }
    match parts {
        [".claude", "rules", .., last] if last.ends_with(".md") && last.len() > ".md".len() => {
            Ok(())
        }
        _ => Err("it is not an instruction file of a repository".into()),
    }
}

/// `root` joined with `parts`, as long as nothing on the way is a symlink: a write through one
/// would land wherever it points. The root itself may be one (a home folder often is).
pub fn confined(root: &Path, parts: &[&str]) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for part in parts {
        path.push(part);
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(format!("{} is a symlink", path.display()));
        }
    }
    Ok(path)
}

/// `path` as the parts below `root`, for a second check right before a write.
pub fn below<'a>(root: &Path, path: &'a Path) -> Option<Vec<&'a str>> {
    path.strip_prefix(root)
        .ok()?
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_are_not_plain_are_refused_before_any_guard() {
        for bad in [
            "",
            ".",
            "..",
            "./settings.json",
            "a/./settings.json",
            "a/../settings.json",
            "../settings.json",
            "/etc/passwd",
            "a//b",
            "a/",
            "a\\b",
            "a\u{1b}[31m",
            "a\0b",
        ] {
            assert!(plain_parts(bad).is_err(), "{bad:?}");
        }
        assert_eq!(plain_parts("rules/a.md").unwrap(), ["rules", "a.md"]);
        assert!(plain_parts(&"x".repeat(256)).is_err());
    }

    #[test]
    fn only_the_known_claude_user_places_pass() {
        let ok = |rel: &str| claude_user(&plain_parts(rel).unwrap());
        for good in [
            "CLAUDE.md",
            "keybindings.json",
            "rules/a.md",
            "agents/x/y.md",
        ] {
            assert!(ok(good).is_ok(), "{good}");
        }
        for bad in [
            "settings.json",
            "settings.local.json",
            "hooks/pre.sh",
            "rules",
            "claude.md",
            "Rules/a.md",
            "projects/x/memory/a.md",
            ".credentials.json",
        ] {
            assert!(ok(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_instruction_files_pass_for_a_repository() {
        let ok = |rel: &str| instruction(&plain_parts(rel).unwrap());
        for good in [
            "CLAUDE.md",
            "CLAUDE.local.md",
            "AGENTS.md",
            ".claude/CLAUDE.md",
            ".claude/rules/a.md",
            ".claude/rules/sub/dir/a.md",
        ] {
            assert!(ok(good).is_ok(), "{good}");
        }
        for bad in [
            ".git/hooks/pre-commit",
            ".git/config",
            ".claude/settings.json",
            ".claude/settings.local.json",
            ".claude/rules/a.txt",
            ".claude/rules/.md",
            ".claude/rules",
            "notes.md",
            "claude.md",
            "src/CLAUDE.md",
        ] {
            assert!(ok(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn paths_that_walk_up_are_refused_for_repositories() {
        assert!(no_parent_parts("~/code/web").is_ok());
        assert!(no_parent_parts("/Volumes/work/web").is_ok());
        for bad in ["", "~/code/../.ssh", "..", "a\\..\\b", "~/a\nb"] {
            assert!(no_parent_parts(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_clone_goes_only_below_the_home_folder() {
        let home = Path::new("/Users/a");
        assert!(clone_target("~/code/web", Path::new("/Users/a/code/web"), home).is_ok());
        for (path, abs) in [
            ("/Volumes/work/web", "/Volumes/work/web"),
            ("/Users/b/web", "/Users/b/web"),
            ("/Users/ab/web", "/Users/ab/web"),
            ("~", "/Users/a"),
            ("~/code/../../b", "/Users/b"),
        ] {
            assert!(clone_target(path, Path::new(abs), home).is_err(), "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_anywhere_on_the_way_is_refused_but_the_root_may_be_one() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir_all(real.join("rules")).unwrap();
        let root = dir.path().join("root");
        std::os::unix::fs::symlink(&real, &root).unwrap();
        assert!(confined(&root, &["rules", "a.md"]).is_ok());
        std::os::unix::fs::symlink(&outside, real.join("agents")).unwrap();
        assert!(confined(&root, &["agents", "a.md"]).is_err());
        std::os::unix::fs::symlink(outside.join("t"), real.join("rules/leaf.md")).unwrap();
        assert!(confined(&root, &["rules", "leaf.md"]).is_err());
    }
}
