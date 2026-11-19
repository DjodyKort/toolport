//! Servers installed from a GitHub release: a binary or an archive the person downloaded once,
//! whose source records the repository, the version and the file name to look for. This module
//! reads that source, asks GitHub for the newest release and the files it carries, and replaces
//! the installed binary. Every request goes through the host's `HttpAccess`, so its egress guard
//! applies and a test answers from a script.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};
use toolport_plus_api::{HttpAccess, HttpRequest, ServerRecord};

use super::{version, Kind, Pinned, Target};
use crate::egress;
use crate::fsx::{atomic_write_bytes, Mode};
use crate::http::fetch_json;
use crate::skills::gitexec::run_command;

pub const SOURCE_KIND: &str = "github-release";
const API: &str = "https://api.github.com";
const API_HOST: &str = "api.github.com";
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(120);
const RELEASE_BODY_CAP: usize = 8 * 1024 * 1024;
const CHECKSUM_BODY_CAP: usize = 1024 * 1024;
const LIST_PAGE: usize = 30;
const SEARCH_DEPTH: usize = 4;
const NAMES_SHOWN: usize = 8;
/// The largest release file the update downloads; a plan for a bigger one is refused.
pub const MAX_ASSET_BYTES: u64 = 256 * 1024 * 1024;
/// Files of a release that sit beside the binary and are never the binary.
const SIDE_FILES: [&str; 12] = [
    ".sha256",
    ".sha256sum",
    ".sha512",
    ".md5",
    ".sig",
    ".asc",
    ".pem",
    ".minisig",
    ".sbom",
    ".sigstore",
    ".txt",
    ".json",
];
const ARCHIVES: [&str; 7] = [
    ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.bz2", ".tbz2", ".tar",
];
/// Target flavor of a source that also follows pre-releases.
pub const PRERELEASE_FLAVOR: &str = "pre";

/// What the server's `mcpmSource` says about a release it was installed from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub repo: String,
    pub version: String,
    /// The release file to take: placeholders `{version}`, `{tag}`, `{os}` and `{arch}`, or a
    /// glob when it holds `*` or `?`.
    pub pattern: Option<String>,
    /// Where the installed binary is.
    pub path: Option<String>,
    /// The file to take out of an archive; the installed file's name when not given.
    pub binary: Option<String>,
    /// A command that has to succeed on the new binary, or the previous one is put back.
    pub verify: Option<String>,
    pub prerelease: bool,
}

fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    [owner, name].iter().all(|part| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    })
}

/// The release source of a server, when its source says it came from one.
pub fn source_of(server: &ServerRecord) -> Option<Source> {
    let source = server.fork.mcpm_source.as_ref()?;
    if source.kind.as_deref() != Some(SOURCE_KIND) {
        return None;
    }
    let repo = source.repo.clone().filter(|repo| valid_repo(repo))?;
    let text = |key: &str| {
        source
            .extra
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(String::from)
    };
    Some(Source {
        repo,
        version: text("current_version")?,
        pattern: text("asset_pattern"),
        path: source.path.clone().filter(|path| !path.trim().is_empty()),
        binary: text("binary_name"),
        verify: text("verify_command"),
        prerelease: source
            .extra
            .get("include_prerelease")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), crate::fsx::home()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// How an installed release is replaced, when the source says enough to do it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Method {
    pub path: PathBuf,
    pub pattern: String,
    pub binary: String,
    pub verify: Option<String>,
}

/// Whether the install is known well enough to replace the binary, and if it is not, what is
/// missing; the row says to update by hand then.
pub fn method(server: &ServerRecord) -> Result<Method, String> {
    let source = source_of(server).ok_or("the server's source does not record a GitHub release")?;
    let pattern = source.pattern.ok_or(
        "its source records no file name pattern (asset_pattern) to pick the release file with",
    )?;
    let recorded = source
        .path
        .ok_or("its source records no path to the installed file")?;
    let path = expand(&recorded);
    if !path.is_file() {
        return Err(format!("the installed file {recorded} is not there"));
    }
    let command = server
        .text("command")
        .ok_or("the server has no command to compare with the installed file")?;
    if !same_file(&expand(command), &path) {
        return Err(format!(
            "the server runs {command}, but its source says the release is installed at {recorded}"
        ));
    }
    let binary = source.binary.unwrap_or_else(|| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    Ok(Method {
        path,
        pattern,
        binary,
        verify: source.verify,
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The pin of a release source; nothing when the source holds no readable version.
pub fn pinned(server: &ServerRecord) -> Option<Pinned> {
    if server.text("transport").unwrap_or("stdio") != "stdio" {
        return None;
    }
    let source = source_of(server)?;
    version::numbers(&source.version)?;
    Some(Pinned {
        server_id: server.id.clone(),
        server_name: server.name.clone(),
        target: Target {
            kind: Kind::Github,
            name: source.repo,
            flavor: if source.prerelease {
                PRERELEASE_FLAVOR.into()
            } else {
                String::new()
            },
        },
        version: source.version,
        manual: method(server).err(),
    })
}

fn headers(request: HttpRequest) -> HttpRequest {
    request
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "toolport-plus")
}

/// The tag of the newest release, or of the newest one of all when the source follows
/// pre-releases; `Ok(None)` when the repository has no release.
pub fn latest(http: &dyn HttpAccess, target: &Target) -> Result<Option<String>, String> {
    egress::check(API_HOST)?;
    let repo = &target.name;
    if target.flavor == PRERELEASE_FLAVOR {
        let url = format!("{API}/repos/{repo}/releases?per_page={LIST_PAGE}");
        let Some(body) = get_json(http, url, RELEASE_BODY_CAP)? else {
            return Ok(None);
        };
        let tags = body
            .as_array()
            .into_iter()
            .flatten()
            .filter(|release| release["draft"] != true)
            .filter_map(|release| release["tag_name"].as_str());
        return Ok(tags.fold(None::<String>, |newest, tag| match newest {
            Some(best) if !version::newer(tag, &best) => Some(best),
            _ => Some(tag.to_string()),
        }));
    }
    let url = format!("{API}/repos/{repo}/releases/latest");
    Ok(get_json(http, url, RELEASE_BODY_CAP)?
        .and_then(|body| body["tag_name"].as_str().map(String::from)))
}

fn get_json(http: &dyn HttpAccess, url: String, cap: usize) -> Result<Option<Value>, String> {
    let request = headers(
        HttpRequest::get(url)
            .timeout(LOOKUP_TIMEOUT)
            .max_body_bytes(cap),
    );
    let reply = fetch_json(http, request).map_err(|error| format!("{API_HOST}: {error}"))?;
    if reply.status == 404 {
        return Ok(None);
    }
    if !reply.is_success() {
        return Err(format!("{API_HOST} answered {}", reply.status));
    }
    reply
        .json
        .map(Some)
        .ok_or_else(|| format!("{API_HOST} answered something that is not JSON"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
}

/// The release a version names: the tag as given, and with or without the `v` people leave off.
pub fn release_for(
    http: &dyn HttpAccess,
    repo: &str,
    version: &str,
) -> Result<Option<Release>, String> {
    egress::check(API_HOST)?;
    let bare = version.trim_start_matches(['v', 'V']);
    let mut tags: Vec<String> = vec![version.to_string(), format!("v{bare}"), bare.to_string()];
    tags.dedup();
    let mut tried = Vec::new();
    for tag in tags {
        if !valid_tag(&tag) || tried.contains(&tag) {
            continue;
        }
        let url = format!("{API}/repos/{repo}/releases/tags/{tag}");
        if let Some(body) = get_json(http, url, RELEASE_BODY_CAP)? {
            return Ok(Some(read_release(&body)));
        }
        tried.push(tag);
    }
    Ok(None)
}

fn read_release(body: &Value) -> Release {
    Release {
        tag: body["tag_name"].as_str().unwrap_or_default().to_string(),
        assets: body["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|asset| {
                Some(Asset {
                    name: asset["name"].as_str()?.to_string(),
                    url: asset["browser_download_url"].as_str()?.to_string(),
                    size: asset["size"].as_u64().unwrap_or(0),
                })
            })
            .collect(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    pub os: &'static str,
    pub arch: &'static str,
}

impl Platform {
    pub fn current() -> Self {
        Self {
            os: if cfg!(target_os = "macos") {
                "darwin"
            } else if cfg!(windows) {
                "windows"
            } else {
                "linux"
            },
            arch: match std::env::consts::ARCH {
                "x86_64" => "amd64",
                "aarch64" => "arm64",
                "arm" => "arm",
                other => other,
            },
        }
    }
}

fn resolved(pattern: &str, tag: &str, platform: Platform) -> String {
    pattern
        .replace("{version}", tag.trim_start_matches(['v', 'V']))
        .replace("{tag}", tag)
        .replace("{os}", platform.os)
        .replace("{arch}", platform.arch)
}

fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|skip| glob(rest, &text[skip..])),
        Some((b'?', rest)) => !text.is_empty() && glob(rest, &text[1..]),
        Some((byte, rest)) => text.first() == Some(byte) && glob(rest, &text[1..]),
    }
}

fn matches_pattern(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.to_ascii_lowercase(), name.to_ascii_lowercase());
    if pattern.contains(['*', '?']) {
        glob(pattern.as_bytes(), name.as_bytes())
    } else {
        name.starts_with(&pattern)
    }
}

fn side_file(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    SIDE_FILES.iter().any(|suffix| name.ends_with(suffix))
}

fn names(release: &Release) -> String {
    let mut shown: Vec<&str> = release
        .assets
        .iter()
        .take(NAMES_SHOWN)
        .map(|asset| asset.name.as_str())
        .collect();
    if release.assets.len() > NAMES_SHOWN {
        shown.push("...");
    }
    shown.join(", ")
}

/// The one file of the release the pattern names.
pub fn pick_asset<'a>(
    release: &'a Release,
    pattern: &str,
    platform: Platform,
) -> Result<&'a Asset, String> {
    let pattern = resolved(pattern, &release.tag, platform);
    let found: Vec<&Asset> = release
        .assets
        .iter()
        .filter(|asset| !side_file(&asset.name) && matches_pattern(&pattern, &asset.name))
        .collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!(
            "no file of release {} matches {pattern}; it carries: {}",
            release.tag,
            names(release)
        )),
        many => Err(format!(
            "{} files of release {} match {pattern} ({}); narrow the pattern",
            many.len(),
            release.tag,
            many.iter()
                .take(NAMES_SHOWN)
                .map(|asset| asset.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The digest a release publishes for one of its files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checksum {
    pub sha256: String,
    /// The release file the digest came from.
    pub source: String,
}

fn hex64(text: &str) -> Option<String> {
    (text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| text.to_ascii_lowercase())
}

/// The digest `listing` states for `file`: a lone digest, `<digest>  <name>` lines or BSD
/// `SHA256 (<name>) = <digest>` lines.
fn digest_in(listing: &str, file: &str, lone_ok: bool) -> Option<String> {
    let bare = |name: &str| {
        name.trim_start_matches('*')
            .trim_start_matches("./")
            .to_string()
    };
    let mut lone = None;
    for line in listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(rest) = line.strip_prefix("SHA256 (") {
            if let Some((name, digest)) = rest.split_once(") = ") {
                if bare(name) == file {
                    return hex64(digest.trim());
                }
            }
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(first), second) = (words.next(), words.next()) else {
            continue;
        };
        match second {
            Some(name) if bare(name) == file => return hex64(first),
            None if lone_ok => lone = hex64(first),
            _ => {}
        }
    }
    lone
}

fn download_request(url: &str, timeout: Duration, cap: usize) -> HttpRequest {
    HttpRequest::get(url)
        .header("User-Agent", "toolport-plus")
        .timeout(timeout)
        .max_body_bytes(cap)
}

fn fetch_bytes(http: &dyn HttpAccess, asset: &Asset, cap: usize) -> Result<Vec<u8>, String> {
    let host = crate::attention::updates::host_of(&asset.url).unwrap_or_default();
    egress::check(&host)?;
    let timeout = if cap > CHECKSUM_BODY_CAP {
        DOWNLOAD_TIMEOUT
    } else {
        LOOKUP_TIMEOUT
    };
    let reply = http
        .send(&download_request(&asset.url, timeout, cap))
        .map_err(|error| format!("{host}: {error}"))?;
    if !reply.is_success() {
        return Err(format!(
            "{host} answered {} for {}",
            reply.status, asset.name
        ));
    }
    if reply.truncated {
        return Err(format!(
            "{} is larger than the {cap} bytes Toolport takes",
            asset.name
        ));
    }
    Ok(reply.body)
}

/// The digest the release publishes for `asset`, from a checksum file next to it; `Ok(None)`
/// when the release publishes none.
pub fn checksum_for(
    http: &dyn HttpAccess,
    release: &Release,
    asset: &Asset,
) -> Result<Option<Checksum>, String> {
    let own = |suffix: &str| format!("{}{suffix}", asset.name).to_ascii_lowercase();
    let per_file = [own(".sha256"), own(".sha256sum"), own(".sha256.txt")];
    let candidates = release
        .assets
        .iter()
        .filter(|other| other.name != asset.name);
    let chosen = candidates
        .clone()
        .find(|other| per_file.contains(&other.name.to_ascii_lowercase()))
        .map(|other| (other, true))
        .or_else(|| {
            candidates
                .clone()
                .find(|other| {
                    let name = other.name.to_ascii_lowercase();
                    name.contains("checksums")
                        || name.contains("sha256sums")
                        || name == "sha256sum.txt"
                })
                .map(|other| (other, false))
        });
    let Some((file, lone_ok)) = chosen else {
        return Ok(None);
    };
    let listing =
        String::from_utf8_lossy(&fetch_bytes(http, file, CHECKSUM_BODY_CAP)?).into_owned();
    Ok(
        digest_in(&listing, &asset.name, lone_ok).map(|sha256| Checksum {
            sha256,
            source: file.name.clone(),
        }),
    )
}

/// A release file's bytes; refused above [`MAX_ASSET_BYTES`] and when cut short.
pub fn download(http: &dyn HttpAccess, asset: &Asset) -> Result<Vec<u8>, String> {
    fetch_bytes(http, asset, MAX_ASSET_BYTES as usize)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn is_archive(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".zip") || ARCHIVES.iter().any(|suffix| name.ends_with(suffix))
}

fn find_file(dir: &Path, name: &str, depth: usize, found: &mut Vec<(usize, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_file() && entry.file_name().to_string_lossy() == name {
            found.push((depth, path));
        } else if kind.is_dir() && depth < SEARCH_DEPTH {
            find_file(&path, name, depth + 1, found);
        }
    }
}

/// Runs the first program of `attempts` that is there and succeeds; the error is the last one's.
fn run_first(attempts: &[(&str, Vec<String>)]) -> Result<String, String> {
    let mut last = String::new();
    for (program, args) in attempts {
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        match run_command(cmd, EXTRACT_TIMEOUT) {
            Ok(done) if done.ok() => return Ok(done.stdout),
            Ok(done) => last = format!("{program}: {}", done.stderr.trim()),
            Err(error) => last = format!("{program}: {error}"),
        }
    }
    Err(last)
}

/// Whether an archive member would land outside the folder it is unpacked into.
fn escapes(member: &str) -> bool {
    let member = member.replace('\\', "/");
    member.starts_with('/')
        || member.split('/').any(|part| part == "..")
        || member.as_bytes().get(1) == Some(&b':')
}

/// The installed file's new bytes: the release file itself, or the file called `binary` inside
/// the archive, extracted under `work`. A link in the archive is never followed.
pub fn binary_from(
    file_name: &str,
    bytes: &[u8],
    binary: &str,
    work: &Path,
) -> Result<Vec<u8>, String> {
    if !is_archive(file_name) {
        return Ok(bytes.to_vec());
    }
    let out = work.join("extracted");
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let archive = work.join("release-file");
    std::fs::write(&archive, bytes).map_err(|e| format!("{}: {e}", archive.display()))?;
    let zip = file_name.to_ascii_lowercase().ends_with(".zip");
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let mut lists: Vec<(&str, Vec<String>)> = vec![("tar", vec!["-tf".into(), path(&archive)])];
    let mut unpacks: Vec<(&str, Vec<String>)> = vec![(
        "tar",
        vec!["-xf".into(), path(&archive), "-C".into(), path(&out)],
    )];
    if zip {
        lists.insert(0, ("unzip", vec!["-Z1".into(), path(&archive)]));
        unpacks.insert(
            0,
            (
                "unzip",
                vec![
                    "-q".into(),
                    "-o".into(),
                    path(&archive),
                    "-d".into(),
                    path(&out),
                ],
            ),
        );
    }
    let members = run_first(&lists).map_err(|e| format!("could not read {file_name}: {e}"))?;
    if let Some(bad) = members.lines().find(|member| escapes(member)) {
        return Err(format!(
            "{file_name} holds a file outside its own folder ({bad}); nothing was unpacked"
        ));
    }
    run_first(&unpacks).map_err(|e| format!("could not unpack {file_name}: {e}"))?;
    let mut found = Vec::new();
    find_file(&out, binary, 0, &mut found);
    found.sort();
    let (_, path) = found
        .into_iter()
        .next()
        .ok_or_else(|| format!("{file_name} holds no file called {binary}"))?;
    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Replaces `path` with `new`, after copying what is there to `backup`. The mode of the file
/// that was there is kept, so the new one stays executable.
pub fn replace_file(path: &Path, new: &[u8], backup: &Path) -> Result<(), String> {
    if let Some(parent) = backup.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::copy(path, backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    atomic_write_bytes(path, new, Mode::KeepOrDefault)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Puts back what [`replace_file`] set aside.
pub fn restore_file(path: &Path, backup: &Path) -> Result<(), String> {
    let old = std::fs::read(backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    atomic_write_bytes(path, &old, Mode::KeepOrDefault)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether the file at `path` is a non-empty file the system can run.
pub fn runnable(path: &Path) -> Result<(), String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() || meta.len() == 0 {
        return Err("the new file is empty".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return Err("the new file is not executable".into());
        }
    }
    Ok(())
}
