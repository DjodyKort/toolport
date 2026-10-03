//! Progressive-disclosure assets shipped next to a SKILL.md, and the content hash that covers them.

use super::parser::Skill;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Replicates mcpm's current allowlist (`SKILL_ASSET_DIRS` / `SKILL_ASSET_EXTENSIONS`);
/// Q-ASSETS may widen it, so every consumer goes through this constant.
pub struct AssetAllowlist {
    pub dirs: &'static [&'static str],
    pub extensions: &'static [&'static str],
}

pub const ASSET_ALLOWLIST: AssetAllowlist = AssetAllowlist {
    dirs: &[
        "modules",
        "reference",
        "templates",
        "examples",
        "assets",
        "scripts",
    ],
    extensions: &[
        ".md", ".txt", ".json", ".yaml", ".yml", ".png", ".svg", ".jpg", ".jpeg", ".webp", ".sh",
        ".bash", ".py",
    ],
};

/// Python `Path.suffix.lower()`: text after the last dot of the final component, unless the dot
/// is the first character (so `.hidden` has no suffix but `.hidden.md` has `.md`) or the last.
fn suffix_lower(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => name[i..].to_lowercase(),
        _ => String::new(),
    }
}

fn allowed(allow: &AssetAllowlist, name: &str) -> bool {
    let suffix = suffix_lower(name);
    allow.extensions.contains(&suffix.as_str())
}

fn walk(dir: &Path, base: &Path, allow: &AssetAllowlist, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let is_real_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_real_dir {
            walk(&path, base, allow, out);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if allowed(allow, &name) {
            if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_path_buf());
            }
        }
    }
}

pub fn rel_string(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Allowed asset files relative to `src_dir`, sorted by their `/`-joined relative path.
pub fn discover_assets_with(src_dir: &Path, allow: &AssetAllowlist) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for sub in allow.dirs {
        let dir = src_dir.join(sub);
        if dir.is_dir() {
            walk(&dir, src_dir, allow, &mut out);
        }
    }
    out.sort_by_key(|p| rel_string(p));
    out
}

pub fn discover_assets(src_dir: &Path) -> Vec<PathBuf> {
    discover_assets_with(src_dir, &ASSET_ALLOWLIST)
}

/// `sha256:` plus the first 16 hex digits over `SKILL.md\n<bytes>\n---\n` followed by
/// `<rel>\n<bytes>\n---\n` for each asset in sorted relative-path order.
pub fn compute_skill_hash(skill: &Skill) -> Result<String, String> {
    let src_dir = skill.source_dir();
    let mut h = Sha256::new();
    h.update(b"SKILL.md\n");
    h.update(
        fs::read(&skill.source_path)
            .map_err(|e| format!("{}: {e}", skill.source_path.display()))?,
    );
    h.update(b"\n---\n");
    for rel in discover_assets(src_dir) {
        h.update(format!("{}\n", rel_string(&rel)).as_bytes());
        let full = src_dir.join(&rel);
        h.update(fs::read(&full).map_err(|e| format!("{}: {e}", full.display()))?);
        h.update(b"\n---\n");
    }
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("sha256:{}", &hex[..16]))
}

/// Copies allowed assets into `dst_dir` and returns the written paths relative to
/// `output_root` (absolute when `dst_dir` lies outside it).
pub fn copy_skill_assets(
    src_dir: &Path,
    dst_dir: &Path,
    output_root: &Path,
) -> Result<Vec<String>, String> {
    let mut written = Vec::new();
    for rel in discover_assets(src_dir) {
        let dst = dst_dir.join(&rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::copy(src_dir.join(&rel), &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        written.push(match dst.strip_prefix(output_root) {
            Ok(r) => rel_string(r),
            Err(_) => dst.to_string_lossy().into_owned(),
        });
    }
    Ok(written)
}
