use crate::registry::ServerEntry;
use std::path::{Path, PathBuf};

pub const SCRIPTS_SUBDIR: &str = "imported-scripts";

const SCRIPT_SOURCE_DIRS: &[&str] = &[".config/mcpm/bin/", ".claude/mcp-servers/"];

pub fn scripts_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(SCRIPTS_SUBDIR)
}

pub fn relocate_script(home: &str, data_dir: &Path, path: &str) -> Option<PathBuf> {
    let home = home.trim_end_matches('/');
    SCRIPT_SOURCE_DIRS.iter().find_map(|dir| {
        let rel = path.strip_prefix(&format!("{home}/{dir}"))?;
        let rel = rel.trim_start_matches('/');
        let clean = !rel.is_empty()
            && rel
                .split('/')
                .all(|seg| !seg.is_empty() && seg != "." && seg != "..");
        clean.then(|| {
            let tag = dir.trim_end_matches('/').replace('/', "_");
            scripts_dir(data_dir)
                .join(tag.trim_start_matches('.'))
                .join(rel)
        })
    })
}

pub fn relocate_entry(
    entry: &mut ServerEntry,
    home: &str,
    data_dir: &Path,
) -> Vec<(String, PathBuf)> {
    let mut moves = Vec::new();
    let mut map = |s: &mut String| {
        if let Some(to) = relocate_script(home, data_dir, s) {
            moves.push((s.clone(), to.clone()));
            *s = to.to_string_lossy().into_owned();
        }
    };
    if let Some(c) = entry.command.as_mut() {
        map(c);
    }
    for a in entry.args.iter_mut() {
        map(a);
    }
    moves
}

pub fn screen_entry(entry: &ServerEntry) -> Result<(), String> {
    match entry.command.as_deref() {
        Some(c) => crate::downstream::screen_spawn_command(c, &entry.args),
        None => Ok(()),
    }
}
