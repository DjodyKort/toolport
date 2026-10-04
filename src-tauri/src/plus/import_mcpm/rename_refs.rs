use super::NameMap;
use crate::plus::args::flag_or;
use crate::registry::atomic_write;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const OLD_PREFIX: &str = "mcp__mcpm_";
const EXTENSIONS: &[&str] = &[
    "md", "mdc", "markdown", "txt", "yaml", "yml", "toml", "json",
];
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target"];

fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"mcp__mcpm_[A-Za-z0-9_-]+").expect("static regex"))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileRewrite {
    pub path: String,
    pub replaced: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RenameReport {
    pub dry_run: bool,
    pub scanned: usize,
    pub files: Vec<FileRewrite>,
    pub orphans: Vec<Orphan>,
    pub replaced: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct Orphan {
    pub path: String,
    pub reference: String,
}

impl RenameReport {
    #[cfg(test)]
    pub fn changed(&self) -> bool {
        !self.files.is_empty()
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn summary(&self) -> String {
        let mut lines = vec![format!(
            "{}: {} files scanned, {} files changed, {} references rewritten, {} orphans",
            if self.dry_run { "dry run" } else { "applied" },
            self.scanned,
            self.files.len(),
            self.replaced,
            self.orphans.len()
        )];
        for o in &self.orphans {
            lines.push(format!("orphan {} in {}", o.reference, o.path));
        }
        lines.join("\n")
    }
}

fn resolve<'a>(token: &str, map: &'a NameMap) -> Option<(usize, &'a str)> {
    let mut end = token.len();
    loop {
        if let Some(new) = map.map.get(&token[..end]) {
            return Some((end, new));
        }
        end = token[..end]
            .rfind(['-', '_'])
            .filter(|i| *i > OLD_PREFIX.len())?;
    }
}

pub fn rewrite_text(text: &str, map: &NameMap) -> (String, usize, Vec<String>) {
    let mut replaced = 0;
    let mut orphans = BTreeSet::new();
    let out = token_re().replace_all(text, |caps: &regex::Captures| {
        let token = &caps[0];
        match resolve(token, map) {
            Some((len, new)) if len == token.len() => {
                replaced += 1;
                new.to_string()
            }
            Some((len, new)) => {
                replaced += 1;
                format!("{new}{}", &token[len..])
            }
            None => {
                orphans.insert(token.trim_end_matches(['-', '_']).to_string());
                token.to_string()
            }
        }
    });
    (out.into_owned(), replaced, orphans.into_iter().collect())
}

pub(super) fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            let name = e.file_name();
            if !SKIP_DIRS.iter().any(|s| name == *s) {
                collect(&path, out)?;
            }
        } else if path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| EXTENSIONS.contains(&x.to_ascii_lowercase().as_str()))
        {
            out.push(path);
        }
    }
    Ok(())
}

pub fn rename_refs(
    roots: &[PathBuf],
    map: &NameMap,
    dry_run: bool,
) -> Result<RenameReport, String> {
    let mut report = RenameReport {
        dry_run,
        ..RenameReport::default()
    };
    let mut orphans = BTreeSet::new();
    for root in roots {
        let mut files = Vec::new();
        if root.is_dir() {
            collect(root, &mut files)?;
        } else if root.is_file() {
            files.push(root.clone());
        } else {
            return Err(format!("{} does not exist", root.display()));
        }
        for path in files {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            report.scanned += 1;
            if !text.contains(OLD_PREFIX) {
                continue;
            }
            let (out, replaced, found) = rewrite_text(&text, map);
            let shown = path.display().to_string();
            for reference in found {
                orphans.insert(Orphan {
                    path: shown.clone(),
                    reference,
                });
            }
            if replaced > 0 && out != text {
                if !dry_run {
                    atomic_write(&path, &out)?;
                }
                report.replaced += replaced;
                report.files.push(FileRewrite {
                    path: shown,
                    replaced,
                });
            }
        }
    }
    report.orphans = orphans.into_iter().collect();
    Ok(report)
}

pub fn rename_refs_handler(args: Value) -> Result<Value, String> {
    let root = args
        .get("root")
        .and_then(Value::as_str)
        .ok_or("root is required")?;
    let tools = args
        .get("tools")
        .and_then(Value::as_str)
        .ok_or("tools is required")?;
    let paths: Vec<PathBuf> = args
        .get("paths")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(PathBuf::from)
                .collect()
        })
        .filter(|p: &Vec<PathBuf>| !p.is_empty())
        .ok_or("paths is required")?;
    let dry_run = flag_or(&args, "dryRun", true);
    let opts = super::RunOptions {
        root: PathBuf::from(root),
        short_ids_path: args
            .get("shortIds")
            .and_then(Value::as_str)
            .map(PathBuf::from),
        home: args.get("home").and_then(Value::as_str).map(String::from),
        ..super::RunOptions::default()
    };
    let map = super::name_map(&opts, Path::new(tools))?;
    let report = rename_refs(&paths, &map, dry_run)?;
    Ok(json!({ "report": report.to_value(), "summary": report.summary() }))
}
