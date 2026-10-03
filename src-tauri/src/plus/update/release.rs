use super::exec::ShellRunner;
use super::net::HttpClient;
use super::pins::compare_versions;
use crate::plus::compression::engine::is_executable;
use crate::plus::fswalk::collect_files;
use serde_json::Value;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseCheck {
    pub tag: String,
    pub version: String,
    pub asset_name: String,
    pub asset_url: String,
    pub checksum_name: Option<String>,
    pub checksum_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    UpToDate { latest: String },
    Available(ReleaseCheck),
}

#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
    pub allow_commands: bool,
    pub allow_unverified: bool,
    pub verify_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub version: String,
    pub sha256: String,
    pub checksum_verified: bool,
    pub notes: Vec<String>,
}

const SKIP_EXTENSIONS: &[&str] = &[
    ".sha256",
    ".sha256sum",
    ".sha512",
    ".asc",
    ".sig",
    ".crt",
    ".sbom",
    ".pem",
    ".jsonl",
];
const SKIP_NAMES: &[&str] = &[
    "checksums.txt",
    "sha256sums.txt",
    "sha256sums",
    "dist-manifest.json",
    "changelog.md",
    "license",
];

pub fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    matches!((parts.next(), parts.next(), parts.next()), (Some(a), Some(b), None) if ok(a) && ok(b))
}

pub fn platform() -> (String, String) {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "arm" => "arm",
        other => other,
    };
    (os.to_string(), arch.to_string())
}

pub fn resolve_pattern(pattern: &str, version: &str, os: &str, arch: &str) -> String {
    pattern
        .replace("{version}", version)
        .replace("{os}", os)
        .replace("{arch}", arch)
}

fn is_metadata_asset(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SKIP_NAMES.contains(&lower.as_str()) || SKIP_EXTENSIONS.iter().any(|e| lower.ends_with(e))
}

fn find_checksum<'a>(assets: &'a [(String, String)], target: &str) -> Option<&'a (String, String)> {
    let lower_target = target.to_ascii_lowercase();
    let candidates = [
        format!("{lower_target}.sha256"),
        format!("{lower_target}.sha256sum"),
        "checksums.txt".to_string(),
        "sha256sums.txt".to_string(),
        "sha256sums".to_string(),
        "checksums.sha256".to_string(),
    ];
    candidates.iter().find_map(|c| {
        assets
            .iter()
            .find(|(name, _)| name.to_ascii_lowercase() == *c)
    })
}

pub fn check(
    http: &dyn HttpClient,
    api_base: &str,
    token: Option<&str>,
    repo: &str,
    current: Option<&str>,
    asset_pattern: Option<&str>,
    platform: &(String, String),
) -> Result<Checked, String> {
    if !valid_repo(repo) {
        return Err(format!("invalid repo: {repo}"));
    }
    let pattern = asset_pattern
        .ok_or("no asset pattern configured; set asset_pattern in the source metadata")?;
    let url = format!(
        "{}/repos/{repo}/releases/latest",
        api_base.trim_end_matches('/')
    );
    let mut headers = vec![(
        "Accept".to_string(),
        "application/vnd.github+json".to_string(),
    )];
    if let Some(t) = token {
        headers.push(("Authorization".into(), format!("Bearer {t}")));
    }
    let body = http.get_text(&url, &headers).map_err(|e| match e.status {
        Some(404) => format!("repo not found: {repo}"),
        Some(403) | Some(429) => "rate limited: set GITHUB_TOKEN for higher limits".to_string(),
        _ => format!("could not reach the release API: {e}"),
    })?;
    let release: Value =
        serde_json::from_str(&body).map_err(|_| "release API returned invalid JSON".to_string())?;
    let tag = release
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("release has no tag_name")?
        .to_string();
    let version = tag.trim_start_matches('v').to_string();
    match compare_versions(current.unwrap_or("0.0.0"), &version) {
        None => return Err(format!("could not parse version '{tag}'")),
        Some(Ordering::Less) => {}
        Some(_) => return Ok(Checked::UpToDate { latest: version }),
    }
    let assets: Vec<(String, String)> = release
        .get("assets")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    Some((
                        a.get("name")?.as_str()?.to_string(),
                        a.get("browser_download_url")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    if assets.is_empty() {
        return Err(format!("release {tag} has no downloadable assets"));
    }
    let resolved = resolve_pattern(pattern, &version, &platform.0, &platform.1);
    let matched = assets
        .iter()
        .filter(|(n, _)| n.starts_with(&resolved) && !is_metadata_asset(n))
        .min_by_key(|(n, _)| n.len())
        .ok_or_else(|| {
            let available: Vec<&str> = assets.iter().map(|(n, _)| n.as_str()).collect();
            format!(
                "no asset matching '{resolved}'; available: {}",
                available.join(", ")
            )
        })?;
    let checksum = find_checksum(&assets, &matched.0);
    Ok(Checked::Available(ReleaseCheck {
        tag,
        version,
        asset_name: matched.0.clone(),
        asset_url: matched.1.clone(),
        checksum_name: checksum.map(|c| c.0.clone()),
        checksum_url: checksum.map(|c| c.1.clone()),
    }))
}

pub fn expected_hash(checksum_text: &str, asset_name: &str) -> Option<String> {
    let is_hash = |s: &str| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit());
    let lines: Vec<&str> = checksum_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    for line in &lines {
        let mut parts = line.split_whitespace();
        let (Some(hash), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let name = name.trim_start_matches('*').trim_start_matches("./");
        if is_hash(hash) && name == asset_name {
            return Some(hash.to_ascii_lowercase());
        }
    }
    if lines.len() == 1 {
        let first = lines[0].split_whitespace().next()?;
        let rest = lines[0].split_whitespace().nth(1);
        if is_hash(first) && rest.is_none() {
            return Some(first.to_ascii_lowercase());
        }
    }
    None
}

struct TempDir(PathBuf);

impl TempDir {
    fn create_in(parent: &Path) -> Result<Self, String> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let path = parent.join(format!(".toolport-update-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|e| format!("cannot create work dir: {e}"))?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn archive_kind(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        Some("zip")
    } else if [".tar.gz", ".tgz", ".tar.xz", ".tar.bz2", ".tar"]
        .iter()
        .any(|e| lower.ends_with(e))
    {
        Some("tar")
    } else {
        None
    }
}

fn unsafe_entry(entry: &str) -> bool {
    let e = entry.trim();
    e.starts_with('/')
        || e.starts_with('\\')
        || e.split(['/', '\\']).any(|p| p == "..")
        || e.chars().nth(1) == Some(':')
}

fn extract(archive: &Path, kind: &str, dest: &Path) -> Result<(), String> {
    use std::process::Command;
    let timeout = Duration::from_secs(120);
    let (list, unpack): (Vec<String>, Vec<String>) = if kind == "zip" {
        (
            vec!["-Z1".into(), archive.to_string_lossy().into()],
            vec![
                "-qq".into(),
                "-o".into(),
                archive.to_string_lossy().into(),
                "-d".into(),
                dest.to_string_lossy().into(),
            ],
        )
    } else {
        (
            vec!["-tf".into(), archive.to_string_lossy().into()],
            vec![
                "-xf".into(),
                archive.to_string_lossy().into(),
                "-C".into(),
                dest.to_string_lossy().into(),
            ],
        )
    };
    let program = if kind == "zip" { "unzip" } else { "tar" };
    let mut cmd = Command::new(program);
    cmd.args(&list);
    let listing = super::exec::run_command(cmd, timeout)
        .map_err(|e| format!("could not list archive with {program}: {e}"))?;
    if !listing.ok() {
        return Err(format!(
            "could not list archive: {}",
            listing.first_error_line()
        ));
    }
    if let Some(bad) = listing.stdout.lines().find(|l| unsafe_entry(l)) {
        return Err(format!("archive entry escapes the work directory: {bad}"));
    }
    let mut cmd = Command::new(program);
    cmd.args(&unpack);
    let out = super::exec::run_command(cmd, timeout)
        .map_err(|e| format!("could not extract with {program}: {e}"))?;
    if !out.ok() {
        return Err(format!("could not extract: {}", out.first_error_line()));
    }
    Ok(())
}

fn pick_binary(dir: &Path, target_name: &str, repo_name: &str) -> Result<PathBuf, String> {
    let mut files = Vec::new();
    collect_files(dir, &|_| true, &mut files);
    let mut execs: Vec<PathBuf> = files.into_iter().filter(|p| is_executable(p)).collect();
    execs.sort();
    match execs.len() {
        0 => Err("archive contains no executable file".into()),
        1 => Ok(execs.remove(0)),
        _ => {
            let by_name = |wanted: &str| {
                execs
                    .iter()
                    .find(|p| {
                        p.file_stem().and_then(|s| s.to_str()) == Some(wanted)
                            || p.file_name().and_then(|s| s.to_str()) == Some(wanted)
                    })
                    .cloned()
            };
            by_name(target_name)
                .or_else(|| by_name(repo_name))
                .ok_or_else(|| {
                    let names: Vec<String> = execs
                        .iter()
                        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .collect();
                    format!("multiple executables found: {}", names.join(", "))
                })
        }
    }
}

fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .map_err(|e| format!("cannot stat binary: {e}"))?
            .permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(path, perms).map_err(|e| format!("cannot chmod binary: {e}"))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub fn apply(
    http: &dyn HttpClient,
    shell: &dyn ShellRunner,
    target: &Path,
    repo: &str,
    check: &ReleaseCheck,
    opts: &ApplyOptions,
) -> Result<Applied, String> {
    if !target.exists() {
        return Err(format!("path not found: {}", target.display()));
    }
    let target = std::fs::canonicalize(target).map_err(|e| format!("cannot resolve path: {e}"))?;
    let parent = target
        .parent()
        .ok_or("target has no parent directory")?
        .to_path_buf();
    let target_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("target has no file name")?
        .to_string();
    let work = TempDir::create_in(&parent)
        .map_err(|e| format!("{e}; the target directory must be writable"))?;

    let archive_path = work.0.join("asset");
    let actual = http
        .download(&check.asset_url, &archive_path)
        .map_err(|e| format!("download failed: {e}"))?;

    let mut notes = Vec::new();
    let checksum_verified = match &check.checksum_url {
        Some(url) => {
            let text = http
                .get_text(url, &[])
                .map_err(|e| format!("checksum download failed: {e}"))?;
            let expected = expected_hash(&text, &check.asset_name).ok_or_else(|| {
                format!(
                    "checksum file has no entry for {}; refusing to install",
                    check.asset_name
                )
            })?;
            if expected != actual {
                return Err(format!(
                    "checksum mismatch for {}: expected {expected}, got {actual}",
                    check.asset_name
                ));
            }
            true
        }
        None if opts.allow_unverified => {
            notes.push("no checksum published; installed unverified (explicitly allowed)".into());
            false
        }
        None => {
            return Err(
                "release publishes no checksum for this asset; pass --allow-unverified to install anyway"
                    .into(),
            )
        }
    };

    let repo_name = repo.rsplit('/').next().unwrap_or(repo);
    let binary = match archive_kind(&check.asset_name) {
        Some(kind) => {
            let out = work.0.join("extracted");
            std::fs::create_dir_all(&out).map_err(|e| format!("cannot create dir: {e}"))?;
            extract(&archive_path, kind, &out)?;
            pick_binary(&out, &target_name, repo_name)?
        }
        None => archive_path.clone(),
    };
    let size = std::fs::metadata(&binary).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        return Err("downloaded binary is empty".into());
    }

    let new_path = parent.join(format!("{target_name}.new"));
    let backup = parent.join(format!("{target_name}.bak"));
    let cleanup = |p: &Path| {
        let _ = std::fs::remove_file(p);
    };
    std::fs::copy(&binary, &new_path).map_err(|e| format!("cannot stage binary: {e}"))?;
    if let Err(e) = set_executable(&new_path) {
        cleanup(&new_path);
        return Err(e);
    }
    if let Err(e) = std::fs::copy(&target, &backup) {
        cleanup(&new_path);
        return Err(format!("cannot back up the current binary: {e}"));
    }
    if let Err(e) = std::fs::rename(&new_path, &target) {
        cleanup(&new_path);
        cleanup(&backup);
        return Err(format!("cannot replace the binary: {e}"));
    }
    let restore = |reason: String| -> String {
        match std::fs::rename(&backup, &target) {
            Ok(()) => format!("{reason}; restored the previous version"),
            Err(e) => format!(
                "{reason}; restore failed ({e}), backup kept at {}",
                backup.display()
            ),
        }
    };
    if !is_executable(&target) {
        return Err(restore("installed file is not executable".into()));
    }
    match (&opts.verify_command, opts.allow_commands) {
        (Some(cmd), true) => match shell.run(cmd, &parent, Duration::from_secs(5)) {
            Ok(out) if out.ok() => {}
            Ok(out) => {
                return Err(restore(format!(
                    "verify_command failed (exit {})",
                    out.code
                )))
            }
            Err(e) => return Err(restore(format!("verify_command failed: {e}"))),
        },
        (Some(_), false) => {
            notes.push("verify_command not run: pass --allow-commands to enable".into())
        }
        _ => {}
    }
    cleanup(&backup);
    Ok(Applied {
        version: check.version.clone(),
        sha256: actual,
        checksum_verified,
        notes,
    })
}
