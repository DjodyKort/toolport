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
