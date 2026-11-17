//! The one-click fix for a link in SKILL.md that leaves the skill folder: the file it points at is
//! copied into the folder (`references/` unless told otherwise) and every use of the link is
//! rewritten, so a sync installs the file and the link stays alive in Claude Code.

use super::assets::{denied_by, is_plain_relative, rel_string, SKILL_FILE};
use super::lint::{links_of, LinkState};
use super::parser::Skill;
use crate::fsx::{atomic_write, atomic_write_bytes, Mode};
use crate::op::OpError;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

pub const FIX_MOVE_LINK: &str = "move-link";
pub const DEFAULT_DEST: &str = "references";
const SIZE_LIMIT: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rewrite {
    pub line: usize,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug)]
pub struct Moved {
    pub from: PathBuf,
    /// Relative to the skill folder, and the text the link has afterwards.
    pub to: String,
    /// The folder already held the same content, so nothing is copied.
    pub reused: bool,
    pub size: u64,
    pub rewrites: Vec<Rewrite>,
}

fn refused(message: impl Into<String>) -> OpError {
    OpError::failed("refused", message)
}

/// The folder that holds `skills/`, `rules/` and the rest of the library the skill lives in.
fn library_root(skill: &Skill) -> Option<PathBuf> {
    skill.source_dir().parent()?.parent().map(Path::to_path_buf)
}

/// The file `link` points at, when it lies inside the library but outside the skill folder.
pub(crate) fn movable_source(skill: &Skill, link: &str) -> Result<PathBuf, OpError> {
    let dir = skill.source_dir();
    let outside = links_of(skill, &super::assets::inventory(dir))
        .into_iter()
        .any(|l| l.target == link && l.state == LinkState::OutsideFolder);
    if !outside {
        return Err(refused(format!(
            "{link} is not a link of {SKILL_FILE} that leaves the skill folder"
        )));
    }
    let found = fs::canonicalize(dir.join(link))
        .map_err(|_| OpError::not_found(format!("{link} points at a file that does not exist")))?;
    let root = library_root(skill)
        .and_then(|root| fs::canonicalize(root).ok())
        .ok_or_else(|| refused("the library folder of this skill is unknown"))?;
    if !found.starts_with(&root) {
        return Err(refused(format!(
            "{link} points outside the library; copy that file in by hand"
        )));
    }
    let meta = fs::metadata(&found).map_err(|e| refused(format!("{}: {e}", found.display())))?;
    if !meta.is_file() {
        return Err(refused(format!(
            "{link} points at a folder; move it into the skill by hand"
        )));
    }
    if meta.len() > SIZE_LIMIT {
        return Err(refused(format!("{link} is larger than 1 MB")));
    }
    Ok(found)
}

fn destination(dest: &str, file: &str) -> Result<String, OpError> {
    let dest = dest.trim().trim_end_matches('/');
    if dest.is_empty() || !is_plain_relative(dest) || dest == "." {
        return Err(OpError::usage(format!(
            "dest must be a folder inside the skill, not {dest:?}"
        )));
    }
    if let Some(rule) = dest
        .split('/')
        .find_map(|part| denied_by(part, true))
        .or_else(|| denied_by(file, false))
    {
        return Err(refused(format!(
            "{dest}/{file} would not be installed: it is on the deny-list ({rule})"
        )));
    }
    if file == SKILL_FILE && dest == "." {
        return Err(refused("the copy would replace SKILL.md"));
    }
    Ok(format!("{dest}/{file}"))
}

fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// SKILL.md with every markdown link to `link` outside code pointing at `new` instead; fragments
/// and queries after the target stay.
fn rewrite(text: &str, link: &str, new: &str) -> (String, Vec<Rewrite>) {
    let pattern = Regex::new(&format!(
        r#"(\]\(\s*<?){}([#?\s)>"]|$)"#,
        regex::escape(link)
    ))
    .expect("link pattern");
    let mut out = String::with_capacity(text.len());
    let mut changes = Vec::new();
    let mut fenced = false;
    let mut in_front = text.starts_with("---");
    for (index, raw) in text.split_inclusive('\n').enumerate() {
        let line = raw.trim_end_matches(['\n', '\r']);
        let ending = &raw[line.len()..];
        let skip = if in_front {
            in_front = index == 0 || !line.starts_with("---");
            true
        } else if is_fence(line) {
            fenced = !fenced;
            true
        } else {
            fenced
        };
        if skip {
            out.push_str(raw);
            continue;
        }
        let rewritten: String = line
            .split('`')
            .enumerate()
            .map(|(i, part)| {
                if i % 2 == 0 {
                    pattern
                        .replace_all(part, format!("${{1}}{new}${{2}}"))
                        .into_owned()
                } else {
                    part.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("`");
        if rewritten != line {
            changes.push(Rewrite {
                line: index + 1,
                before: line.to_string(),
                after: rewritten.clone(),
            });
        }
        out.push_str(&rewritten);
        out.push_str(ending);
    }
    (out, changes)
}

#[cfg(unix)]
fn copy_mode(source: &Path) -> Mode {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(source).map_or(Mode::KeepOrDefault, |m| {
        Mode::Exact(m.permissions().mode() & 0o777)
    })
}

#[cfg(not(unix))]
fn copy_mode(_source: &Path) -> Mode {
    Mode::KeepOrDefault
}

/// Copies the file `link` points at into `dest` of the skill folder and rewrites the link.
/// `dry_run` reports the same result and writes nothing.
pub fn move_link(skill: &Skill, link: &str, dest: &str, dry_run: bool) -> Result<Moved, OpError> {
    let from = movable_source(skill, link)?;
    let file = from
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| refused(format!("{link} has no file name")))?
        .to_string();
    let to = destination(dest, &file)?;
    let target = skill.source_dir().join(&to);
    let bytes = fs::read(&from).map_err(|e| refused(format!("{}: {e}", from.display())))?;
    let reused = match fs::read(&target) {
        Ok(existing) if existing == bytes => true,
        Ok(_) => {
            return Err(OpError::failed(
                "conflict",
                format!("{to} already exists in the skill with other content"),
            ))
        }
        Err(_) => false,
    };
    let text = fs::read_to_string(&skill.source_path)
        .map_err(|e| refused(format!("{}: {e}", skill.source_path.display())))?;
    let (rewritten, rewrites) = rewrite(&text, link, &to);
    if rewrites.is_empty() {
        return Err(refused(format!(
            "{link} is not used as a markdown link outside code in {SKILL_FILE}"
        )));
    }
    if !dry_run {
        if !reused {
            atomic_write_bytes(&target, &bytes, copy_mode(&from))
                .map_err(|e| refused(format!("{}: {e}", target.display())))?;
        }
        atomic_write(&skill.source_path, &rewritten, Mode::KeepOrDefault)
            .map_err(|e| refused(format!("{}: {e}", skill.source_path.display())))?;
    }
    Ok(Moved {
        from,
        to: rel_string(Path::new(&to)),
        reused,
        size: bytes.len() as u64,
        rewrites,
    })
}
