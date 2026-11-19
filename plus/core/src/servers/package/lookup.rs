//! The one network step of the package check: ask the registry a package lives on for its newest
//! release. Every request goes through the host's `HttpAccess`, so its egress guard applies and a
//! test answers from a script.

use std::time::Duration;

use toolport_plus_api::{HttpAccess, HttpRequest};

use super::{release, version, Kind, Target};
use crate::egress;
use crate::http::fetch_json;

const TIMEOUT: Duration = Duration::from_secs(15);
const PYPI_BODY_CAP: usize = 8 * 1024 * 1024;
const TAG_PAGE: usize = 100;

fn url(target: &Target) -> (String, &'static str) {
    match target.kind {
        Kind::Github => (
            format!("https://api.github.com/repos/{}/releases/latest", target.name),
            "api.github.com",
        ),
        Kind::Npm => (
            format!(
                "https://registry.npmjs.org/{}/latest",
                target.name.replace('/', "%2F")
            ),
            "registry.npmjs.org",
        ),
        Kind::Pypi => (
            format!("https://pypi.org/pypi/{}/json", target.name),
            "pypi.org",
        ),
        Kind::Docker => (
            format!(
                "https://hub.docker.com/v2/repositories/{}/tags?page_size={TAG_PAGE}&ordering=last_updated",
                target.name
            ),
            "hub.docker.com",
        ),
    }
}

/// The newest version of `target`; `Ok(None)` when the registry has no such package (a private
/// one, or one that was removed), which is not a failure of the check.
pub fn latest(http: &dyn HttpAccess, target: &Target) -> Result<Option<String>, String> {
    if target.kind == Kind::Github {
        return release::latest(http, target);
    }
    let (url, host) = url(target);
    egress::check(host)?;
    let mut request = HttpRequest::get(url).timeout(TIMEOUT);
    if target.kind == Kind::Pypi {
        request = request.max_body_bytes(PYPI_BODY_CAP);
    }
    let reply = fetch_json(http, request).map_err(|error| format!("{host}: {error}"))?;
    if reply.status == 404 {
        return Ok(None);
    }
    if !reply.is_success() {
        return Err(format!("{host} answered {}", reply.status));
    }
    let body = reply
        .json
        .ok_or_else(|| format!("{host} answered something that is not JSON"))?;
    let found = match target.kind {
        Kind::Npm => body["version"].as_str().map(String::from),
        Kind::Pypi => body["info"]["version"].as_str().map(String::from),
        Kind::Github => None,
        Kind::Docker => {
            let tags: Vec<String> = body["results"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|tag| tag["name"].as_str().map(String::from))
                .collect();
            version::newest_tag(&tags, &target.flavor)
        }
    };
    Ok(found)
}

fn version_url(target: &Target, version: &str) -> (String, &'static str) {
    match target.kind {
        Kind::Npm => (
            format!(
                "https://registry.npmjs.org/{}/{version}",
                target.name.replace('/', "%2F")
            ),
            "registry.npmjs.org",
        ),
        Kind::Pypi => (
            format!("https://pypi.org/pypi/{}/{version}/json", target.name),
            "pypi.org",
        ),
        Kind::Docker => (
            format!(
                "https://hub.docker.com/v2/repositories/{}/tags/{version}",
                target.name
            ),
            "hub.docker.com",
        ),
        Kind::Github => (String::new(), "api.github.com"),
    }
}

/// Whether the registry has `version` of the package: `Ok(false)` for a version it does not know.
pub fn exists(http: &dyn HttpAccess, target: &Target, version: &str) -> Result<bool, String> {
    if target.kind == Kind::Github {
        return release::release_for(http, &target.name, version).map(|found| found.is_some());
    }
    let (url, host) = version_url(target, version);
    egress::check(host)?;
    let request = HttpRequest::get(url)
        .timeout(TIMEOUT)
        .max_body_bytes(PYPI_BODY_CAP);
    let reply = fetch_json(http, request).map_err(|error| format!("{host}: {error}"))?;
    match reply.status {
        404 => Ok(false),
        _ if reply.is_success() => Ok(true),
        status => Err(format!("{host} answered {status}")),
    }
}
