//! Tap operations behind `toolportctl skills tap|search|install`, the `plus.skills.tap*`
//! handlers and the selfmcp tools: one function per operation, no rendering. mcpm keeps these in
//! its sync plugin (`TapManager`). Differences kept on purpose: tap names and install specs are
//! validated before they reach a path, a duplicate or unknown tap is an error, nothing the tap
//! ships is followed outside the clone (symlinks are skipped), and every write has a dry run.

use super::audit::{audit_skills, AuditFinding};
use super::git::GitRunner;
use super::parser::{discover_skills_report, valid_name, Discovery, Skill, SkillType};
use super::taps::{
    load_taps, parse_source, plain_segment, redact, save_taps, tap_dir, taps_path, taps_root,
    valid_tap_name, Tap,
};
use serde_yaml::Value as Yaml;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const LOCK_WAIT: Duration = Duration::from_secs(330);
const MAX_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Invalid,
    NotFound,
    Conflict,
    Backend,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TapError {
    pub kind: Kind,
    pub message: String,
}

impl TapError {
    fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new(Kind::Invalid, message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(Kind::NotFound, message)
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self::new(Kind::Conflict, message)
    }

    fn backend(message: impl Into<String>) -> Self {
        Self::new(Kind::Backend, message)
    }
}

impl From<TapError> for String {
    fn from(error: TapError) -> String {
        error.message
    }
}

pub struct Env<'a> {
    pub config_dir: &'a Path,
    pub git: &'a dyn GitRunner,
}

impl Env<'_> {
    fn root(&self) -> PathBuf {
        taps_root(self.config_dir)
    }

    fn dir(&self, tap: &Tap) -> PathBuf {
        tap_dir(&self.root(), tap)
    }

    fn lock(&self) -> Result<crate::registry::FileLock, TapError> {
        fs::create_dir_all(self.config_dir).map_err(|e| TapError::backend(e.to_string()))?;
        crate::registry::lock_at_for(&taps_path(self.config_dir), LOCK_WAIT)
            .map_err(TapError::backend)
    }
}

fn shown(text: &str) -> String {
    text.escape_debug().to_string()
}

fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn clone_failure(url: &str, error: &str) -> TapError {
    let reason = error.strip_prefix("git clone failed: ").unwrap_or(error);
    TapError::backend(format!(
        "Failed to clone {}: {}",
        redact(url),
        redact(reason)
    ))
}

#[derive(Debug)]
pub struct AddReport {
    pub tap: Tap,
    pub path: PathBuf,
    pub cloned: bool,
    pub head: Option<String>,
}

/// Registers a tap and clones it (`--depth 1`, like mcpm). On a dry run nothing is written and
/// git is not started.
pub fn add(
    env: &Env,
    spec: &str,
    alias: Option<&str>,
    dry_run: bool,
) -> Result<AddReport, TapError> {
    let source = parse_source(spec).map_err(TapError::invalid)?;
    let name = match alias {
        Some(alias) => {
            valid_tap_name(alias).map_err(TapError::invalid)?;
            alias.to_string()
        }
        None => source.name.map_err(TapError::invalid)?,
    };
    let tap = Tap {
        name,
        repo: redact(spec),
        url: source.url,
    };
    let _lock = if dry_run { None } else { Some(env.lock()?) };
    let mut taps = load_taps(env.config_dir);
    if taps.iter().any(|t| t.name.eq_ignore_ascii_case(&tap.name)) {
        return Err(TapError::conflict(format!(
            "Tap '{}' already exists",
            tap.name
        )));
    }
    let path = env.dir(&tap);
    if dry_run {
        return Ok(AddReport {
            tap,
            path,
            cloned: false,
            head: None,
        });
    }
    if exists(&path) {
        return Err(TapError::conflict(format!(
            "{} already exists and is not a registered tap",
            path.display()
        )));
    }
    fs::create_dir_all(env.root()).map_err(|e| TapError::backend(e.to_string()))?;
    if let Err(error) = env.git.clone_shallow(&tap.url, &path) {
        let _ = fs::remove_dir_all(&path);
        return Err(clone_failure(&tap.url, &error));
    }
    taps.push(tap.clone());
    if let Err(error) = save_taps(env.config_dir, &taps) {
        let _ = fs::remove_dir_all(&path);
        return Err(TapError::backend(error));
    }
    let head = env.git.head(&path).ok();
    Ok(AddReport {
        tap,
        path,
        cloned: true,
        head,
    })
}

#[derive(Debug)]
pub struct RemoveReport {
    pub name: String,
    pub path: PathBuf,
    pub had_clone: bool,
    pub removed: bool,
}

/// Unregisters a tap and deletes its clone. The clone path comes from the validated name, never
/// from the stored file.
pub fn remove(env: &Env, name: &str, dry_run: bool) -> Result<RemoveReport, TapError> {
    valid_tap_name(name).map_err(TapError::invalid)?;
    let _lock = if dry_run { None } else { Some(env.lock()?) };
    let mut taps = load_taps(env.config_dir);
    let Some(at) = taps.iter().position(|t| t.name == name) else {
        return Err(TapError::not_found(format!("Tap '{name}' not found.")));
    };
    let path = env.dir(&taps[at]);
    let had_clone = exists(&path);
    if !dry_run {
        if had_clone {
            let is_dir = path.symlink_metadata().is_ok_and(|m| m.is_dir());
            let gone = if is_dir {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            gone.map_err(|e| TapError::backend(format!("{}: {e}", path.display())))?;
        }
        taps.remove(at);
        save_taps(env.config_dir, &taps).map_err(TapError::backend)?;
    }
    Ok(RemoveReport {
        name: name.to_string(),
        path,
        had_clone,
        removed: !dry_run,
    })
}

#[derive(Debug)]
pub struct UpdateRow {
    pub name: String,
    pub ok: bool,
    pub head: Option<String>,
    pub error: Option<String>,
}

/// Fast-forwards one tap or all of them; a failing tap is a failed row and never stops the rest.
pub fn update(env: &Env, name: Option<&str>, dry_run: bool) -> Result<Vec<UpdateRow>, TapError> {
    let mut taps = load_taps(env.config_dir);
    if let Some(name) = name {
        valid_tap_name(name).map_err(TapError::invalid)?;
        taps.retain(|t| t.name == name);
        if taps.is_empty() {
            return Err(TapError::not_found(format!("Tap '{name}' not found.")));
        }
    }
    Ok(taps
        .iter()
        .map(|tap| {
            let path = env.dir(tap);
            let row = |ok, head, error: Option<String>| UpdateRow {
                name: tap.name.clone(),
                ok,
                head,
                error,
            };
            if !env.git.is_repo(&path) {
                return row(false, None, Some("the clone is missing".into()));
            }
            if dry_run {
                return row(true, None, None);
            }
            match env.git.pull(&path) {
                Ok(head) => row(true, Some(head), None),
                Err(error) => row(
                    false,
                    None,
                    Some(redact(
                        error.strip_prefix("git pull failed: ").unwrap_or(&error),
                    )),
                ),
            }
        })
        .collect())
}

#[derive(Debug)]
pub struct TapRow {
    pub tap: Tap,
    pub path: PathBuf,
    pub cloned: bool,
}

pub fn list(env: &Env) -> Vec<TapRow> {
    load_taps(env.config_dir)
        .into_iter()
        .map(|tap| {
            let path = env.dir(&tap);
            let cloned = env.git.is_repo(&path);
            TapRow { tap, path, cloned }
        })
        .collect()
}

fn kind_name(kind: SkillType) -> &'static str {
    match kind {
        SkillType::Rule => "rule",
        SkillType::Skill => "skill",
    }
}

/// The skills of a tap clone. A skill whose files resolve outside the clone (a symlink in the
/// tap) is dropped with a warning.
fn discover(clone: &Path) -> Discovery {
    let mut found = discover_skills_report(clone);
    let Ok(root) = clone.canonicalize() else {
        return found;
    };
    let mut kept = Vec::new();
    for skill in found.skills {
        let inside = skill
            .source_path
            .canonicalize()
            .is_ok_and(|p| p.starts_with(&root));
        if inside {
            kept.push(skill);
        } else {
            found.warnings.push(format!(
                "Skipping skill {}: its files resolve outside the tap",
                shown(skill.name())
            ));
        }
    }
    found.skills = kept;
    found
}

/// Python's `str()` of a frontmatter value, which is what mcpm searches for `tags`.
fn py_text(value: Option<&Yaml>) -> String {
    match value {
        None | Some(Yaml::Null) => String::new(),
        Some(Yaml::String(s)) => s.clone(),
        Some(Yaml::Bool(b)) => if *b { "True" } else { "False" }.to_string(),
        Some(Yaml::Number(n)) => n.to_string(),
        Some(Yaml::Sequence(items)) => {
            let parts: Vec<String> = items
                .iter()
                .map(|item| match item {
                    Yaml::String(s) => format!("'{s}'"),
                    other => py_text(Some(other)),
                })
                .collect();
            format!("[{}]", parts.join(", "))
        }
        Some(_) => String::new(),
    }
}

#[derive(Debug)]
pub struct Hit {
    pub tap: String,
    pub repo: String,
    pub name: String,
    pub description: String,
    pub kind: &'static str,
}

#[derive(Debug)]
pub struct SearchReport {
    pub query: String,
    pub tap_count: usize,
    pub hits: Vec<Hit>,
    pub warnings: Vec<String>,
}

/// Skills of every cloned tap whose name, description or `metadata.tags` contain `query`
/// (case-insensitive), in tap order.
pub fn search(env: &Env, query: &str) -> SearchReport {
    let needle = query.to_lowercase();
    let taps = load_taps(env.config_dir);
    let mut report = SearchReport {
        query: query.to_string(),
        tap_count: taps.len(),
        hits: Vec::new(),
        warnings: Vec::new(),
    };
    for tap in &taps {
        let path = env.dir(tap);
        if !env.git.is_repo(&path) {
            continue;
        }
        let found = discover(&path);
        report.warnings.extend(found.warnings);
        for skill in found.skills {
            let fm = &skill.frontmatter;
            let tags = py_text(fm.metadata.get("tags"));
            let text = format!("{} {} {tags}", fm.name, fm.description).to_lowercase();
            if text.contains(&needle) {
                report.hits.push(Hit {
                    tap: tap.name.clone(),
                    repo: tap.repo.clone(),
                    name: fm.name.clone(),
                    description: fm.description.clone(),
                    kind: kind_name(skill.skill_type),
                });
            }
        }
    }
    report
}

#[derive(Debug, PartialEq, Eq)]
pub struct InstallSpec {
    pub owner: String,
    pub repo: String,
    pub skill: Option<String>,
    pub version: Option<String>,
}

/// `@user/repo[/skill][@version]`.
pub fn parse_install_spec(spec: &str) -> Result<InstallSpec, TapError> {
    let usage = || {
        TapError::invalid(format!(
            "invalid install spec {:?}: expected @user/repo[/skill][@version]",
            spec
        ))
    };
    let rest = spec.trim_start_matches('@');
    let (path, version) = match rest.rsplit_once('@') {
        Some((path, version)) => (path, Some(version)),
        None => (rest, None),
    };
    if version.is_some_and(|v| v.is_empty() || v.chars().any(char::is_control)) {
        return Err(usage());
    }
    let parts: Vec<&str> = path.split('/').collect();
    if !(2..=3).contains(&parts.len()) || !plain_segment(parts[0]) || !plain_segment(parts[1]) {
        return Err(usage());
    }
    let skill = match parts.get(2) {
        Some(name) => {
            valid_name(name).map_err(|reason| {
                TapError::invalid(format!("invalid skill name {:?}: {reason}", shown(name)))
            })?;
            Some(name.to_string())
        }
        None => None,
    };
    Ok(InstallSpec {
        owner: parts[0].to_string(),
        repo: parts[1].to_string(),
        skill,
        version: version.map(String::from),
    })
}

pub struct InstallOptions<'a> {
    pub target: &'a Path,
    pub no_audit: bool,
    pub dry_run: bool,
}

#[derive(Debug)]
pub struct Planned {
    pub name: String,
    pub kind: &'static str,
    pub path: PathBuf,
    pub files: usize,
    pub installed: bool,
}

#[derive(Debug)]
pub struct InstallReport {
    pub spec: String,
    pub tap: String,
    pub tap_added: bool,
    pub tap_missing: bool,
    pub clone_url: String,
    pub version: Option<String>,
    pub found: usize,
    pub warnings: Vec<String>,
    pub audited: bool,
    pub findings: Vec<AuditFinding>,
    pub blocked: bool,
    pub skills: Vec<Planned>,
    pub symlinks_skipped: Vec<String>,
    pub target: PathBuf,
}

fn bucket(kind: SkillType) -> &'static str {
    match kind {
        SkillType::Rule => "rules",
        SkillType::Skill => "skills",
    }
}

/// Copies regular files and directories; symlinks, `.git` and special files are left behind.
fn copy_tree(
    from: &Path,
    to: &Path,
    depth: usize,
    skipped: &mut Vec<String>,
) -> Result<usize, String> {
    if depth > MAX_DEPTH {
        return Err(format!("{} is nested too deeply", from.display()));
    }
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut files = 0;
    let mut entries: Vec<_> = fs::read_dir(from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let (source, dest) = (entry.path(), to.join(entry.file_name()));
        if kind.is_symlink() {
            skipped.push(source.to_string_lossy().into_owned());
        } else if kind.is_dir() {
            if entry.file_name() != ".git" {
                files += copy_tree(&source, &dest, depth + 1, skipped)?;
            }
        } else if kind.is_file() {
            fs::copy(&source, &dest).map_err(|e| format!("{}: {e}", source.display()))?;
            files += 1;
        }
    }
    Ok(files)
}

fn count_files(from: &Path, depth: usize) -> usize {
    if depth > MAX_DEPTH {
        return 0;
    }
    fs::read_dir(from)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() && e.file_name() != ".git" => count_files(&e.path(), depth + 1),
            Ok(t) if t.is_file() => 1,
            _ => 0,
        })
        .sum()
}

fn stage_and_place(
    skill: &Skill,
    dest: &Path,
    skipped: &mut Vec<String>,
) -> Result<usize, TapError> {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let staging = parent.join(format!(".{}.installing", skill.name()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(parent)
        .map_err(|e| TapError::backend(format!("{}: {e}", parent.display())))?;
    let placed = copy_tree(skill.source_dir(), &staging, 0, skipped).and_then(|files| {
        fs::rename(&staging, dest)
            .map(|_| files)
            .map_err(|e| e.to_string())
    });
    placed.map_err(|e| {
        let _ = fs::remove_dir_all(&staging);
        TapError::backend(e)
    })
}

/// Installs the skills of `@user/repo[/skill]` into `<target>/skills/<name>` or
/// `<target>/rules/<name>`, registering the GitHub tap `user-repo` first when it is unknown. A
/// high-severity audit finding blocks the whole install unless `no_audit`. A dry run reads the
/// registered tap and writes nothing; for an unknown tap it only reports what it would clone.
pub fn install(env: &Env, spec: &str, opts: &InstallOptions) -> Result<InstallReport, TapError> {
    let parsed = parse_install_spec(spec)?;
    let key = format!("{}-{}", parsed.owner, parsed.repo);
    valid_tap_name(&key).map_err(TapError::invalid)?;
    let mut report = InstallReport {
        spec: spec.to_string(),
        tap: key.clone(),
        tap_added: false,
        tap_missing: false,
        clone_url: String::new(),
        version: parsed.version.clone(),
        found: 0,
        warnings: Vec::new(),
        audited: !opts.no_audit,
        findings: Vec::new(),
        blocked: false,
        skills: Vec::new(),
        symlinks_skipped: Vec::new(),
        target: opts.target.to_path_buf(),
    };
    let registered = load_taps(env.config_dir)
        .into_iter()
        .find(|t| t.name.eq_ignore_ascii_case(&key));
    let tap = match registered {
        Some(tap) => tap,
        None => {
            let added = add(
                env,
                &format!("{}/{}", parsed.owner, parsed.repo),
                None,
                opts.dry_run,
            )?;
            report.clone_url = added.tap.url.clone();
            if opts.dry_run {
                report.tap_missing = true;
                return Ok(report);
            }
            report.tap_added = true;
            added.tap
        }
    };
    report.tap = tap.name.clone();
    report.clone_url = tap.url.clone();
    let clone = env.dir(&tap);
    if !env.git.is_repo(&clone) {
        return Err(TapError::not_found(format!(
            "the clone of tap '{}' is missing; remove the tap and add it again",
            tap.name
        )));
    }
    let found = discover(&clone);
    report.warnings = found.warnings;
    let mut skills = found.skills;
    if let Some(wanted) = &parsed.skill {
        skills.retain(|s| s.name() == wanted);
    }
    if skills.is_empty() {
        let mut message = format!("No skills found for '{}'", shown(spec));
        for warning in &report.warnings {
            message.push('\n');
            message.push_str(warning);
        }
        return Err(TapError::not_found(message));
    }
    report.found = skills.len();
    if !opts.no_audit {
        report.findings = audit_skills(&skills).findings;
        report.blocked = report.findings.iter().any(|f| f.severity == "high");
        if report.blocked {
            return Ok(report);
        }
    }
    let mut taken = BTreeSet::new();
    for skill in &skills {
        valid_name(skill.name()).map_err(TapError::invalid)?;
        let planned = |files, installed| Planned {
            name: skill.name().to_string(),
            kind: kind_name(skill.skill_type),
            path: opts
                .target
                .join(bucket(skill.skill_type))
                .join(skill.name()),
            files,
            installed,
        };
        let dest = planned(0, false).path;
        if exists(&dest) || !taken.insert(dest.clone()) {
            report.skills.push(planned(0, false));
        } else if opts.dry_run {
            report
                .skills
                .push(planned(count_files(skill.source_dir(), 0), true));
        } else {
            let files = stage_and_place(skill, &dest, &mut report.symlinks_skipped)?;
            report.skills.push(planned(files, true));
        }
    }
    Ok(report)
}
