//! Taps (named git sources of skills). A tap is `{name: {repo, url}}` in `taps.json` beside the
//! registry with its clone at `taps/<name>`; the path is always derived from the name, never read
//! back from the file. The operations live in `tap_ops.rs`.

use super::json::{self, J};
use crate::registry::atomic_write;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const TAPS_FILE: &str = "taps.json";
pub const TAPS_DIR: &str = "taps";
const GITHUB: &str = "https://github.com/";
const MAX_SEGMENT: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tap {
    pub name: String,
    pub repo: String,
    pub url: String,
}

pub fn valid_tap_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['.', '-'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid tap name {name:?}: use letters, digits, '-', '_' and '.' (at most 64, not \
             starting with '.' or '-')"
        ))
    }
}

/// An owner or repository segment of `user/repo`.
pub fn plain_segment(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_SEGMENT
        && text.starts_with(|c: char| c.is_ascii_alphanumeric())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// `scheme://user:secret@` or `scheme://user@` shown as `scheme://***@`, so a clone error or a
/// listing never carries a credential.
pub fn redact(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"([A-Za-z][A-Za-z0-9+.-]*://)[^/@\s'"]*@"#).expect("redact pattern")
    });
    re.replace_all(text, "${1}***@").into_owned()
}

fn has_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

fn helper_syntax(url: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z][A-Za-z0-9+.-]*::").expect("helper pattern"))
        .is_match(url)
}

fn scp_like(url: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^[A-Za-z0-9._-]+@[A-Za-z0-9.-]+:[^\s:][^\s]*$").expect("scp pattern")
    })
    .is_match(url)
}

fn absolute_path(url: &str) -> bool {
    let bytes = url.as_bytes();
    url.starts_with('/')
        || url.starts_with("\\\\")
        || (bytes.len() > 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

/// What git may be asked to clone: https, ssh, git and file URLs, `user@host:path` and absolute
/// paths. Credentials in the URL, transport helpers (`ext::`), option look-alikes and control
/// characters are refused.
pub fn valid_source_url(url: &str) -> Result<(), String> {
    let shown = redact(url);
    if url.trim().is_empty() || has_control(url) || url.starts_with('-') || url.trim() != url {
        return Err(format!("invalid tap url {shown:?}"));
    }
    if helper_syntax(url) {
        return Err(format!("tap url {shown:?} uses a git transport helper"));
    }
    if let Some((scheme, rest)) = url.split_once("://") {
        if !matches!(scheme, "https" | "ssh" | "git" | "file") {
            return Err(format!(
                "tap url scheme {scheme:?} is not allowed; use https, ssh, git or file"
            ));
        }
        let authority = rest.split('/').next().unwrap_or("");
        if let Some((userinfo, _)) = authority.rsplit_once('@') {
            if userinfo.contains(':') {
                return Err(
                    "tap urls must not embed credentials; use ssh or a git credential helper"
                        .to_string(),
                );
            }
        }
        if rest.is_empty() {
            return Err(format!("invalid tap url {shown:?}"));
        }
        return Ok(());
    }
    if scp_like(url) || absolute_path(url) {
        return Ok(());
    }
    Err(format!(
        "invalid tap source {shown:?}: expected user/repo, an https, ssh, git or file URL, \
         user@host:path or an absolute path"
    ))
}

/// Where a tap is cloned from and the name it gets without `--name`.
#[derive(Debug, PartialEq, Eq)]
pub struct Source {
    pub url: String,
    pub name: Result<String, String>,
}

/// `user/repo` is GitHub as in mcpm (name `user-repo`); anything else is a URL or path whose
/// last segment names the tap.
pub fn parse_source(spec: &str) -> Result<Source, String> {
    if let Some((owner, repo)) = spec.split_once('/') {
        if plain_segment(owner) && plain_segment(repo) {
            return Ok(Source {
                url: format!("{GITHUB}{owner}/{repo}.git"),
                name: Ok(format!("{owner}-{repo}")),
            });
        }
    }
    valid_source_url(spec)?;
    let last = spec
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or("");
    let name = last.strip_suffix(".git").unwrap_or(last).to_string();
    Ok(Source {
        url: spec.to_string(),
        name: valid_tap_name(&name).map(|_| name).map_err(|_| {
            format!(
                "cannot derive a tap name from {:?}; pass --name <alias>",
                redact(spec)
            )
        }),
    })
}

pub fn taps_path(config_dir: &Path) -> PathBuf {
    config_dir.join(TAPS_FILE)
}

pub fn taps_root(config_dir: &Path) -> PathBuf {
    config_dir.join(TAPS_DIR)
}

/// The registered taps in file order. Entries with a name or url that could not have been added
/// (a hand-edited file) are ignored.
pub fn load_taps(config_dir: &Path) -> Vec<Tap> {
    let Ok(text) = fs::read_to_string(taps_path(config_dir)) else {
        return Vec::new();
    };
    let Ok(J::Obj(items)) = json::parse(&text) else {
        return Vec::new();
    };
    let mut taps: Vec<Tap> = Vec::new();
    for (name, v) in &items {
        let Some(url) = v.get("url").and_then(J::as_str) else {
            continue;
        };
        if valid_tap_name(name).is_err()
            || valid_source_url(url).is_err()
            || taps.iter().any(|t| t.name == *name)
        {
            continue;
        }
        taps.push(Tap {
            name: name.clone(),
            repo: v.get("repo").and_then(J::as_str).unwrap_or(url).to_string(),
            url: url.to_string(),
        });
    }
    taps
}

pub fn save_taps(config_dir: &Path, taps: &[Tap]) -> Result<(), String> {
    let doc = J::Obj(
        taps.iter()
            .map(|t| {
                (
                    t.name.clone(),
                    J::Obj(vec![
                        ("repo".into(), J::str(&t.repo)),
                        ("url".into(), J::str(&t.url)),
                    ]),
                )
            })
            .collect(),
    );
    atomic_write(&taps_path(config_dir), &doc.dumps())
}

pub fn tap_dir(taps_root: &Path, tap: &Tap) -> PathBuf {
    taps_root.join(&tap.name)
}
