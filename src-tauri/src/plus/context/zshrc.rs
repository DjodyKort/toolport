//! The shell side of "Toolport owns its shims": finds the `~/.zshrc` lines that still point into
//! mcpm's config directory, moves the source lines that can move to Toolport's own directory
//! (preview, timestamped backup, order check) and lists the mcpm aliases that stop working with
//! mcpm. Nothing here deletes a line or an alias; the old files stay where they are.

use super::backup::stamp_for;
use super::roots::{Roots, CONTEXT_SHIMS_FILE};
use crate::plus::compression::store::SHIMS_FILE as COMPRESSION_SHIMS_FILE;
use crate::plus::skills::clock::now_unix_secs;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_SCANNED_BYTES: u64 = 1024 * 1024;
const COPIED_EXTENSIONS: [&str; 3] = ["zsh", "sh", "bash"];
const GENERATED: [&str; 2] = [CONTEXT_SHIMS_FILE, COMPRESSION_SHIMS_FILE];
const BACKUP_INFIX: &str = ".toolport-backup-";

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Pointer {
    pub line: usize,
    pub file: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeadAlias {
    pub name: String,
    pub file: String,
    pub line: usize,
    pub command: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub line: usize,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Skip {
    pub line: usize,
    pub file: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Copied {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub path: String,
    pub exists: bool,
    pub legacy_lines: Vec<Pointer>,
    pub dead_aliases: Vec<DeadAlias>,
}

impl Inspection {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub path: PathBuf,
    pub exists: bool,
    /// False when Toolport's shell files share mcpm's directory, so no line points anywhere old.
    pub moves: bool,
    pub changes: Vec<Change>,
    pub skipped: Vec<Skip>,
    pub copies: Vec<(PathBuf, PathBuf)>,
    pub order: Vec<String>,
    pub dead_aliases: Vec<DeadAlias>,
    new_text: String,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub plan: Plan,
    pub dry_run: bool,
    pub backup: Option<PathBuf>,
    pub copied: Vec<(PathBuf, PathBuf)>,
}

#[derive(Clone, Debug)]
struct Hit {
    start: usize,
    end: usize,
    file: String,
    nested: bool,
    home_token: Option<String>,
    quote: Option<char>,
    source: bool,
}

fn dir_spellings(home: &Path, dir: &Path) -> Vec<String> {
    let mut out = vec![dir.to_string_lossy().into_owned()];
    if let Ok(rel) = dir.strip_prefix(home) {
        let rel = rel.to_string_lossy();
        for token in ["~", "$HOME", "${HOME}"] {
            out.push(format!("{token}/{rel}"));
        }
    }
    out
}

fn path_regex(home: &Path, dir: &Path) -> Regex {
    let alternatives: Vec<String> = dir_spellings(home, dir)
        .iter()
        .map(|s| regex::escape(s))
        .collect();
    Regex::new(&format!(
        r"(?P<dir>{})/(?P<file>[A-Za-z0-9._+-]+)(?P<next>/?)",
        alternatives.join("|")
    ))
    .expect("escaped alternatives form a valid regex")
}

fn source_before() -> Regex {
    Regex::new(r#"(?:^|[\s;&|(])(?:source|\.)\s+["']?$"#).expect("static regex")
}

fn path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "._/~$-".contains(c)
}

fn hits_in(line: &str, re: &Regex, source: &Regex) -> Vec<Hit> {
    re.captures_iter(line)
        .filter_map(|caps| {
            let whole = caps.get(0)?;
            let dir = caps.name("dir")?;
            let before = line[..whole.start()].chars().next_back();
            if before.is_some_and(path_char) {
                return None;
            }
            let quote = before.filter(|c| *c == '"' || *c == '\'');
            let nested = caps.name("next").is_some_and(|m| !m.as_str().is_empty());
            let end = if nested {
                caps.name("file")?.end()
            } else {
                whole.end()
            };
            let home_token = ["~", "$HOME", "${HOME}"]
                .iter()
                .find(|t| dir.as_str().starts_with(**t))
                .map(|t| t.to_string());
            Some(Hit {
                start: whole.start(),
                end,
                file: caps.name("file")?.as_str().to_string(),
                nested,
                home_token,
                quote,
                source: source.is_match(&line[..whole.start()]),
            })
        })
        .collect()
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

fn body_of(raw: &str) -> &str {
    raw.trim_end_matches(['\n', '\r'])
}

fn read_text(path: &Path) -> Option<String> {
    if fs::metadata(path).ok()?.len() > MAX_SCANNED_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn safe_unquoted(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_ascii_alphanumeric() || "_./~$@%+=:,{}-".contains(c))
}

/// How a path reads in a shell: `~/...` when it sits under the home directory and needs no
/// quoting, `"$HOME/..."` when it does, an absolute (quoted) path otherwise.
pub fn shell_path(home: &Path, path: &Path) -> String {
    let under_home = path
        .strip_prefix(home)
        .ok()
        .map(|rel| format!("~/{}", rel.to_string_lossy()));
    let plain = under_home
        .clone()
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    if safe_unquoted(&plain) {
        return plain;
    }
    let inner = match under_home {
        Some(text) => format!("$HOME{}", &text[1..]),
        None => plain,
    };
    format!("\"{inner}\"")
}

fn replacement(home: &Path, target: &Path, hit: &Hit) -> Result<String, String> {
    let rel = target
        .strip_prefix(home)
        .ok()
        .map(|r| r.to_string_lossy().into_owned());
    let raw = match (&hit.home_token, rel, hit.quote) {
        (Some(token), Some(rel), None) => format!("{token}/{rel}"),
        (Some(_), Some(rel), Some('"')) => format!("$HOME/{rel}"),
        _ => target.to_string_lossy().into_owned(),
    };
    let tail = raw
        .strip_prefix("${HOME}")
        .or_else(|| raw.strip_prefix("$HOME"))
        .unwrap_or(&raw);
    if tail.contains(['"', '`', '\\', '$']) || (hit.quote == Some('\'') && raw.contains('\'')) {
        return Err(format!("{raw} cannot be quoted safely"));
    }
    if hit.quote.is_some() || safe_unquoted(&raw) {
        return Ok(raw);
    }
    Ok(match raw.strip_prefix('~') {
        Some(rest) => format!("\"$HOME{rest}\""),
        None => format!("\"{raw}\""),
    })
}

fn legacy_applies(roots: &Roots) -> bool {
    roots.shims_dir != roots.config_dir
}

fn lines_of(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

pub fn legacy_pointers(roots: &Roots, text: &str) -> Vec<Pointer> {
    if !legacy_applies(roots) {
        return Vec::new();
    }
    let (re, source) = (path_regex(&roots.home, &roots.config_dir), source_before());
    let mut out = Vec::new();
    for (idx, raw) in lines_of(text).into_iter().enumerate() {
        let body = body_of(raw);
        if is_comment(body) {
            continue;
        }
        for hit in hits_in(body, &re, &source) {
            out.push(Pointer {
                line: idx + 1,
                file: if hit.nested {
                    format!("{}/…", hit.file)
                } else {
                    hit.file
                },
                text: body.trim().to_string(),
            });
        }
    }
    out.dedup_by(|a, b| a.line == b.line && a.file == b.file);
    out
}

fn first_line_with(text: &str, needle: &str) -> Option<usize> {
    lines_of(text)
        .into_iter()
        .position(|raw| !is_comment(raw) && raw.contains(needle))
        .map(|idx| idx + 1)
}

fn order_problems(text: &str) -> Vec<String> {
    let Some(ours) = first_line_with(text, CONTEXT_SHIMS_FILE) else {
        return Vec::new();
    };
    ["shell-wrapper.sh", COMPRESSION_SHIMS_FILE]
        .iter()
        .filter_map(|other| {
            let line = first_line_with(text, other).filter(|line| *line > ours)?;
            Some(format!(
                "{CONTEXT_SHIMS_FILE} (line {ours}) is sourced before {other} (line {line}); the order has to be cf's shell-wrapper, compression-shims, then context-shims"
            ))
        })
        .collect()
}

enum Decision {
    Ready,
    Copy(PathBuf, PathBuf),
    Skip(String),
}

fn decide(roots: &Roots, hit: &Hit, will_write_shims: bool) -> Decision {
    if hit.nested {
        return Decision::Skip("points into a subdirectory of mcpm's config; not a shell file Toolport owns".into());
    }
    let target = roots.shims_dir.join(&hit.file);
    if hit.file == CONTEXT_SHIMS_FILE {
        return if target.exists() || will_write_shims {
            Decision::Ready
        } else {
            Decision::Skip("not written yet: run `toolportctl context sync`".into())
        };
    }
    if hit.file == COMPRESSION_SHIMS_FILE {
        return if target.exists() {
            Decision::Ready
        } else {
            Decision::Skip("not generated yet: run `toolportctl compression sync`".into())
        };
    }
    let from = roots.config_dir.join(&hit.file);
    let extension = Path::new(&hit.file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if !COPIED_EXTENSIONS.contains(&extension) {
        return Decision::Skip("not a shell file Toolport owns; left as it is".into());
    }
    let Ok(meta) = fs::metadata(&from) else {
        return Decision::Skip("the file does not exist".into());
    };
    if !meta.is_file() || meta.len() > MAX_SCANNED_BYTES {
        return Decision::Skip("not a regular file of a size Toolport copies".into());
    }
    let source = match fs::read(&from) {
        Ok(bytes) => bytes,
        Err(e) => return Decision::Skip(format!("{}: {e}", from.display())),
    };
    match fs::read(&target) {
        Ok(existing) if existing == source => Decision::Ready,
        Ok(_) => Decision::Skip(format!(
            "{} exists and differs from the mcpm copy; left as it is",
            target.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Decision::Copy(from, target),
        Err(e) => Decision::Skip(format!("{}: {e}", target.display())),
    }
}

/// Plans the rewrite of every `source` line that points at a file in mcpm's config directory.
/// `will_write_shims` is true when this run writes `context-shims.zsh` itself, so a preview
/// shows the line it will point at before the file exists.
pub fn plan(roots: &Roots, will_write_shims: bool) -> Result<Plan, String> {
    let path = roots.zshrc_path();
    let mut plan = Plan {
        path: path.clone(),
        exists: false,
        moves: legacy_applies(roots),
        changes: Vec::new(),
        skipped: Vec::new(),
        copies: Vec::new(),
        order: Vec::new(),
        dead_aliases: Vec::new(),
        new_text: String::new(),
    };
    if !path.exists() {
        return Ok(plan);
    }
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("{} is not readable as text: {e}", path.display()))?;
    plan.exists = true;
    plan.dead_aliases = dead_aliases(roots, &text);
    if !legacy_applies(roots) {
        plan.new_text = text;
        return Ok(plan);
    }
    let (re, source) = (path_regex(&roots.home, &roots.config_dir), source_before());
    for (idx, raw) in lines_of(&text).into_iter().enumerate() {
        let body = body_of(raw);
        let hits = if is_comment(body) {
            Vec::new()
        } else {
            hits_in(body, &re, &source)
        };
        if hits.is_empty() {
            plan.new_text.push_str(raw);
            continue;
        }
        let eligible = hits.iter().any(|h| h.source);
        let mut line = body.to_string();
        for hit in hits.iter().rev() {
            let skip = |reason: String| Skip {
                line: idx + 1,
                file: hit.file.clone(),
                reason,
            };
            if !eligible {
                plan.skipped
                    .push(skip("not a source line; left as it is".into()));
                continue;
            }
            let target = roots.shims_dir.join(&hit.file);
            match decide(roots, hit, will_write_shims) {
                Decision::Skip(reason) => plan.skipped.push(skip(reason)),
                ready => match replacement(&roots.home, &target, hit) {
                    Err(reason) => plan.skipped.push(skip(reason)),
                    Ok(text) => {
                        line.replace_range(hit.start..hit.end, &text);
                        if let Decision::Copy(from, to) = ready {
                            if !plan.copies.iter().any(|(_, t)| *t == to) {
                                plan.copies.push((from, to));
                            }
                        }
                    }
                },
            }
        }
        if line != body {
            plan.changes.push(Change {
                line: idx + 1,
                before: body.trim().to_string(),
                after: line.trim().to_string(),
            });
        }
        plan.new_text.push_str(&line);
        plan.new_text.push_str(&raw[body.len()..]);
    }
    plan.skipped.sort_by_key(|s| s.line);
    plan.order = order_problems(&plan.new_text);
    Ok(plan)
}

fn backup_path(path: &Path) -> Result<PathBuf, String> {
    let stamp = stamp_for(now_unix_secs());
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or("the shell rc file has no name")?;
    for n in 0..1000 {
        let suffix = if n == 0 {
            String::new()
        } else {
            format!("-{n}")
        };
        let candidate = path.with_file_name(format!("{name}{BACKUP_INFIX}{stamp}{suffix}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(format!("no free backup name next to {}", path.display()))
}

fn replace_file(path: &Path, text: &str) -> Result<(), String> {
    let dest = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let perms = fs::metadata(&dest)
        .map_err(|e| format!("{}: {e}", dest.display()))?
        .permissions();
    let tmp = dest.with_file_name(format!(
        ".{}.toolport-tmp-{}",
        dest.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
        std::process::id()
    ));
    let written = fs::write(&tmp, text)
        .and_then(|_| fs::set_permissions(&tmp, perms))
        .and_then(|_| fs::rename(&tmp, &dest));
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(format!("{}: {e}", dest.display()));
    }
    Ok(())
}

fn copy_new(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut source = fs::File::open(from).map_err(|e| format!("{}: {e}", from.display()))?;
    let mut target = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(|e| format!("{}: {e}", to.display()))?;
    std::io::copy(&mut source, &mut target).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(())
}

/// Writes the plan: copies of user-owned files first (never over an existing file), then a
/// timestamped backup of the rc file next to it, then the rewritten rc file.
pub fn apply(plan: Plan, dry_run: bool) -> Result<Outcome, String> {
    let mut outcome = Outcome {
        plan,
        dry_run,
        backup: None,
        copied: Vec::new(),
    };
    if outcome.plan.changes.is_empty() {
        return Ok(outcome);
    }
    let backup = backup_path(&outcome.plan.path)?;
    if dry_run {
        outcome.backup = Some(backup);
        return Ok(outcome);
    }
    for (from, to) in outcome.plan.copies.clone() {
        copy_new(&from, &to)?;
        outcome.copied.push((from, to));
    }
    fs::copy(&outcome.plan.path, &backup)
        .map_err(|e| format!("{} -> {}: {e}", outcome.plan.path.display(), backup.display()))?;
    replace_file(&outcome.plan.path, &outcome.plan.new_text)?;
    outcome.backup = Some(backup);
    Ok(outcome)
}

impl Outcome {
    /// The report lines (actions, warnings) of this run, one per line as the ctl prints them.
    pub fn lines(&self) -> (Vec<String>, Vec<String>) {
        let (mut actions, mut warnings) = (Vec::new(), Vec::new());
        let rc = self.plan.path.display();
        if !self.plan.exists {
            actions.push(format!("{rc} does not exist; nothing to rewrite"));
        } else if !self.plan.moves {
            actions.push(format!(
                "{rc}: Toolport's shell files are in mcpm's config directory here; nothing to rewrite"
            ));
        } else if self.plan.changes.is_empty() {
            actions.push(format!("{rc}: no source line points into mcpm's config directory"));
        } else {
            let (verb, backup_verb) = if self.dry_run {
                ("would rewrite", "would back up")
            } else {
                ("rewrote", "backed up")
            };
            for (from, to) in &self.plan.copies {
                let verb = if self.dry_run { "would copy" } else { "copied" };
                actions.push(format!("{verb} {} to {} (the original stays)", from.display(), to.display()));
            }
            actions.push(format!("{verb} {} line(s) of {rc}", self.plan.changes.len()));
            if let Some(backup) = &self.backup {
                actions.push(format!("{backup_verb} {rc} as {}", backup.display()));
            }
            for change in &self.plan.changes {
                actions.push(format!("  - {}: {}", change.line, change.before));
                actions.push(format!("  + {}: {}", change.line, change.after));
            }
        }
        for skip in &self.plan.skipped {
            warnings.push(format!("{rc}:{} {}: {}", skip.line, skip.file, skip.reason));
        }
        for problem in &self.plan.order {
            warnings.push(format!("{rc}: {problem}"));
        }
        warnings.extend(self.plan.dead_aliases.iter().map(dead_alias_line));
        (actions, warnings)
    }

    pub fn to_value(&self) -> Value {
        let (actions, warnings) = self.lines();
        let paths = |pairs: &[(PathBuf, PathBuf)]| -> Vec<Copied> {
            pairs
                .iter()
                .map(|(from, to)| Copied {
                    from: from.display().to_string(),
                    to: to.display().to_string(),
                })
                .collect()
        };
        json!({
            "path": self.plan.path.display().to_string(),
            "exists": self.plan.exists,
            "dryRun": self.dry_run,
            "changes": self.plan.changes,
            "skipped": self.plan.skipped,
            "copies": paths(&self.plan.copies),
            "backup": self.backup.as_ref().map(|b| b.display().to_string()),
            "order": {"ok": self.plan.order.is_empty(), "problems": self.plan.order},
            "deadAliases": self.plan.dead_aliases,
            "actions": actions,
            "warnings": warnings,
        })
    }
}

pub fn dead_alias_line(alias: &DeadAlias) -> String {
    format!(
        "mcpm alias {} ({}:{}) runs `{}`; it stops working once mcpm is gone, remove it yourself (nothing is deleted)",
        alias.name, alias.file, alias.line, alias.command
    )
}

fn unquote(body: &str) -> &str {
    let body = body.trim();
    match body.chars().next() {
        Some(q @ ('\'' | '"')) => {
            let inner = &body[1..];
            &inner[..inner.rfind(q).unwrap_or(inner.len())]
        }
        _ => body,
    }
}

fn basename(token: &str) -> &str {
    token.trim_matches(['"', '\'']).rsplit('/').next().unwrap_or("")
}

fn mcpm_name(name: &str) -> bool {
    (name == "mcpm" || name.starts_with("mcpm-") || name.starts_with("mcpm_"))
        && !name.starts_with("mcpm_context_")
}

fn runs_mcpm(roots: &Roots, command: &str) -> bool {
    let config = roots.config_dir.to_string_lossy();
    let spellings = dir_spellings(&roots.home, &roots.config_dir);
    if command.contains(config.as_ref()) || spellings.iter().any(|s| command.contains(s.as_str())) {
        return true;
    }
    command
        .split(['\n', ';', '|', '&'])
        .any(|segment| {
            let tokens: Vec<&str> = segment
                .split_whitespace()
                .filter(|t| !t.contains('=') && !matches!(*t, "env" | "sudo" | "command" | "exec"))
                .collect();
            let Some(first) = tokens.first() else {
                return false;
            };
            if mcpm_name(basename(first)) {
                return true;
            }
            let launcher = matches!(*first, "uvx" | "uv" | "pipx" | "npx" | "python" | "python3");
            let moves_to_checkout = matches!(*first, "cd" | "pushd");
            tokens.iter().skip(1).any(|t| {
                let name = basename(t);
                (launcher && mcpm_name(name)) || ((launcher || moves_to_checkout) && name == "mcpm.sh")
            })
        })
}

/// Aliases and functions in `text` that name or run mcpm.
fn aliases_in(roots: &Roots, text: &str, file: &str) -> Vec<DeadAlias> {
    let alias = Regex::new(r"^\s*alias\s+(?:-\w+\s+)*(?P<name>[^=\s]+)=(?P<body>.*)$")
        .expect("static regex");
    let function = Regex::new(
        r"^\s*(?:function\s+)?(?P<name>[A-Za-z_][A-Za-z0-9_-]*)\s*\(\)\s*\{?(?P<body>.*)$",
    )
    .expect("static regex");
    let mut out = Vec::new();
    for (idx, raw) in lines_of(text).into_iter().enumerate() {
        let body = body_of(raw);
        if is_comment(body) {
            continue;
        }
        let Some(caps) = alias.captures(body).or_else(|| function.captures(body)) else {
            continue;
        };
        let (name, command) = (&caps["name"], unquote(&caps["body"]));
        if mcpm_name(name) || runs_mcpm(roots, command) {
            out.push(DeadAlias {
                name: name.to_string(),
                file: file.to_string(),
                line: idx + 1,
                command: command.trim_end_matches(['}', ' ', ';']).trim().to_string(),
            });
        }
    }
    out
}

/// The mcpm aliases of the rc file and of the shell files it sources from mcpm's config
/// directory or Toolport's own. Files Toolport generates are not scanned.
pub fn dead_aliases(roots: &Roots, rc_text: &str) -> Vec<DeadAlias> {
    let source = source_before();
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in [&roots.config_dir, &roots.shims_dir] {
        let re = path_regex(&roots.home, dir);
        for raw in lines_of(rc_text) {
            let body = body_of(raw);
            if is_comment(body) {
                continue;
            }
            for hit in hits_in(body, &re, &source).iter().filter(|h| h.source && !h.nested) {
                let path = dir.join(&hit.file);
                if !GENERATED.contains(&hit.file.as_str()) && !files.contains(&path) {
                    files.push(path);
                }
            }
        }
    }
    let mut out = aliases_in(roots, rc_text, &roots.zshrc_path().display().to_string());
    for path in files {
        if let Some(text) = read_text(&path) {
            out.extend(aliases_in(roots, &text, &path.display().to_string()));
        }
    }
    out
}

pub fn inspect(roots: &Roots) -> Inspection {
    let path = roots.zshrc_path();
    let text = read_text(&path);
    Inspection {
        path: path.display().to_string(),
        exists: path.exists(),
        legacy_lines: text
            .as_deref()
            .map(|t| legacy_pointers(roots, t))
            .unwrap_or_default(),
        dead_aliases: text
            .as_deref()
            .map(|t| dead_aliases(roots, t))
            .unwrap_or_default(),
    }
}

/// Where the rc file sources `target` from, as a 1-based line, whatever spelling it uses.
pub fn source_line_of(roots: &Roots, text: &str, target: &Path) -> Option<usize> {
    let dir = target.parent()?;
    let name = target.file_name()?.to_string_lossy().into_owned();
    let (re, source) = (path_regex(&roots.home, dir), source_before());
    lines_of(text).into_iter().enumerate().find_map(|(idx, raw)| {
        let body = body_of(raw);
        (!is_comment(body)
            && hits_in(body, &re, &source)
                .iter()
                .any(|h| !h.nested && h.file == name))
        .then_some(idx + 1)
    })
}
