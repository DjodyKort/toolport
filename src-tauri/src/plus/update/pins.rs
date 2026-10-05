use super::net::HttpClient;
use serde_json::Value;
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinSpec {
    pub arg_index: usize,
    pub name: String,
    pub version: Option<String>,
    pub separator: &'static str,
}

fn split_spec(raw: &str, uvx: bool) -> (String, Option<String>, &'static str) {
    if uvx {
        if let Some((n, v)) = raw.split_once("==") {
            return (n.to_string(), Some(v.to_string()), "==");
        }
        if is_git_url(raw) {
            // `git+ssh://git@host/...@ref`: a bare first-`@` scan would split
            // inside the `user@host` authority, so look for the ref only
            // after the scheme, and take the last `@` there (the ref, if
            // any, is always the final segment).
            let after_scheme = raw.find("://").map(|i| i + 3).unwrap_or(0);
            return match raw[after_scheme..].rfind('@') {
                Some(i) => {
                    let at = after_scheme + i;
                    (raw[..at].to_string(), Some(raw[at + 1..].to_string()), "@")
                }
                None => (raw.to_string(), None, "@"),
            };
        }
    }
    let search_from = usize::from(raw.starts_with('@'));
    match raw[search_from..].find('@') {
        Some(i) => {
            let at = search_from + i;
            (raw[..at].to_string(), Some(raw[at + 1..].to_string()), "@")
        }
        None => (raw.to_string(), None, "@"),
    }
}

fn spec_at(raw: &str, arg_index: usize, uvx: bool) -> Option<PinSpec> {
    let (name, version, separator) = split_spec(raw, uvx);
    if name.is_empty() || name == "@" || name.starts_with('-') {
        return None;
    }
    Some(PinSpec {
        arg_index,
        name,
        version,
        separator,
    })
}

fn is_switch(arg: &str) -> bool {
    if let Some((flag, _)) = arg.split_once('=') {
        return !matches!(flag, "--from" | "--package" | "-p");
    }
    matches!(
        arg,
        "-y" | "--yes"
            | "-q"
            | "--quiet"
            | "--no-install"
            | "--isolated"
            | "--no-cache"
            | "--offline"
            | "--refresh"
    )
}

/// Finds the package argument. A flag that is neither a known switch nor a package flag may
/// take a value of its own, so nothing past it is guessed and the spec stays unresolved.
pub fn parse_spec(args: &[String], uvx: bool) -> Option<PinSpec> {
    for (i, arg) in args.iter().enumerate() {
        let arg = arg.as_str();
        if matches!(arg, "--from" | "--package") || (arg == "-p" && !uvx) {
            return spec_at(args.get(i + 1)?, i + 1, uvx);
        }
        if !arg.starts_with('-') {
            return spec_at(arg, i, uvx);
        }
        if !is_switch(arg) {
            return None;
        }
    }
    None
}

pub fn is_floating(version: &str) -> bool {
    matches!(version, "latest" | "next" | "*" | "")
}

/// How a parsed version spec constrains the installed package (MIG-UPD-8).
/// Exact pins keep today's `compare_versions` path unchanged; only `^`/`~`
/// get a real semver range, so anything else stays byte-for-byte what it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSpec {
    /// `latest`/`next`/`*`/unset: resolves at runtime, nothing is pinned.
    Floating,
    /// A `^`/`~` npm-style range; the raw text is kept for display and rewrite.
    Range(String),
    /// Anything else: a single version pin, compared as an exact value.
    Exact(String),
}

pub fn classify(version: &str) -> VersionSpec {
    if is_floating(version) {
        return VersionSpec::Floating;
    }
    if (version.starts_with('^') || version.starts_with('~'))
        && semver::VersionReq::parse(version).is_ok()
    {
        return VersionSpec::Range(version.to_string());
    }
    VersionSpec::Exact(version.to_string())
}

/// `semver::Version::parse` needs exactly `major.minor.patch`; registry version
/// strings occasionally omit trailing zeros (`"1"`, `"1.2"`), so pad those out
/// the same way `parse_core` already tolerates them for exact-pin comparison.
fn pad_semver(v: &str) -> String {
    let v = v.trim();
    let split_at = v.find(['-', '+']).unwrap_or(v.len());
    let (core, rest) = v.split_at(split_at);
    let core = core.trim_start_matches('v');
    let mut segs: Vec<&str> = core.split('.').collect();
    while segs.len() < 3 {
        segs.push("0");
    }
    format!("{}{rest}", segs[..3].join("."))
}

pub fn parse_semver(v: &str) -> Option<semver::Version> {
    semver::Version::parse(v.trim().trim_start_matches('v'))
        .or_else(|_| semver::Version::parse(&pad_semver(v)))
        .ok()
}

/// The highest published version that satisfies `req`, if any does.
pub fn highest_matching(req: &semver::VersionReq, versions: &[String]) -> Option<String> {
    versions
        .iter()
        .filter_map(|v| parse_semver(v).map(|sv| (v, sv)))
        .filter(|(_, sv)| req.matches(sv))
        .max_by(|a, b| a.1.cmp(&b.1))
        .map(|(v, _)| v.clone())
}

/// A VCS package source (`uv`/`uvx --from git+<url>[@ref]`, MIG-UPD-8): no
/// registry version exists, so it is routed away from the npm/PyPI pin path
/// entirely and checked with `git ls-remote` instead.
pub fn is_git_url(name: &str) -> bool {
    name.starts_with("git+")
}

/// The URL a real `git` invocation understands, e.g. `git+https://…` -> `https://…`.
pub fn git_clone_url(spec: &str) -> &str {
    spec.strip_prefix("git+").unwrap_or(spec)
}

fn parse_core(v: &str) -> Option<(Vec<u64>, bool)> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.find(['-', '+']) {
        Some(i) => (&v[..i], v.as_bytes()[i] == b'-'),
        None => (v, false),
    };
    let parts: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    let parts = parts?;
    if parts.is_empty() {
        None
    } else {
        Some((parts, pre))
    }
}

pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    let (mut x, xpre) = parse_core(a)?;
    let (mut y, ypre) = parse_core(b)?;
    let len = x.len().max(y.len());
    x.resize(len, 0);
    y.resize(len, 0);
    match x.cmp(&y) {
        Ordering::Equal => Some(match (xpre, ypre) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => Ordering::Equal,
        }),
        other => Some(other),
    }
}

pub fn latest_npm(http: &dyn HttpClient, registry: &str, package: &str) -> Result<String, String> {
    let url = format!(
        "{}/{}/latest",
        registry.trim_end_matches('/'),
        package.replace('/', "%2f")
    );
    let body = http
        .get_text(&url, &[("Accept".into(), "application/json".into())])
        .map_err(|e| format!("npm registry lookup failed: {e}"))?;
    version_field(&body, &["version"], "npm registry")
}

pub fn latest_pypi(http: &dyn HttpClient, index: &str, package: &str) -> Result<String, String> {
    let url = format!("{}/pypi/{}/json", index.trim_end_matches('/'), package);
    let body = http
        .get_text(&url, &[("Accept".into(), "application/json".into())])
        .map_err(|e| format!("PyPI lookup failed: {e}"))?;
    version_field(&body, &["info", "version"], "PyPI")
}

/// The full version list for a package, for range resolution (MIG-UPD-8):
/// `/latest` alone cannot tell whether an older, in-range release is the
/// highest one a caret/tilde range still allows.
pub fn npm_versions(
    http: &dyn HttpClient,
    registry: &str,
    package: &str,
) -> Result<Vec<String>, String> {
    let url = format!(
        "{}/{}",
        registry.trim_end_matches('/'),
        package.replace('/', "%2f")
    );
    let body = http
        .get_text(&url, &[("Accept".into(), "application/json".into())])
        .map_err(|e| format!("npm registry lookup failed: {e}"))?;
    let v: Value =
        serde_json::from_str(&body).map_err(|_| "npm registry returned invalid JSON".to_string())?;
    Ok(v.get("versions")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default())
}

pub fn pypi_versions(
    http: &dyn HttpClient,
    index: &str,
    package: &str,
) -> Result<Vec<String>, String> {
    let url = format!("{}/pypi/{}/json", index.trim_end_matches('/'), package);
    let body = http
        .get_text(&url, &[("Accept".into(), "application/json".into())])
        .map_err(|e| format!("PyPI lookup failed: {e}"))?;
    let v: Value =
        serde_json::from_str(&body).map_err(|_| "PyPI returned invalid JSON".to_string())?;
    Ok(v.get("releases")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default())
}

fn version_field(body: &str, path: &[&str], what: &str) -> Result<String, String> {
    let mut v: Value =
        serde_json::from_str(body).map_err(|_| format!("{what} returned invalid JSON"))?;
    for key in path {
        v = v.get(*key).cloned().unwrap_or(Value::Null);
    }
    v.as_str()
        .map(String::from)
        .ok_or_else(|| format!("{what} response has no version"))
}

pub fn rewrite(args: &mut [String], spec: &PinSpec, new_version: &str) -> bool {
    let Some(slot) = args.get_mut(spec.arg_index) else {
        return false;
    };
    let updated = format!("{}{}{}", spec.name, spec.separator, new_version);
    if *slot == updated {
        return false;
    }
    *slot = updated;
    true
}
