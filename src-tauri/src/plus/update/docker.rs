//! Docker image tag/digest check (MIG-UPD-8). A server launched through
//! `docker run <image>[:tag|@digest] ...` has no local daemon to ask on this
//! headless host (no docker group, no socket), and a full multi-registry auth
//! client is out of scope for one update-check bullet; Docker Hub's anonymous
//! pull token is the one case cheap enough to do for real. A manifest's
//! digest is defined as the sha256 of its exact served bytes, so hashing the
//! body `get_text` already read reproduces the registry's own
//! `Docker-Content-Digest` without needing response headers from `HttpClient`.
//! Anything else (another registry host, a private image, a rate limit)
//! reports a precise "not supported here" result instead of a bare "skipped".

use super::net::HttpClient;
use crate::plus::hashing::hex;
use serde_json::Value;
use sha2::{Digest, Sha256};

const HUB_AUTH: &str = "https://auth.docker.io";
const HUB_REGISTRY: &str = "https://registry-1.docker.io";
const MANIFEST_ACCEPT: &str = "application/vnd.docker.distribution.manifest.v2+json, \
     application/vnd.docker.distribution.manifest.list.v2+json, \
     application/vnd.oci.image.manifest.v1+json, application/vnd.oci.image.index.v1+json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    /// `None` for Docker Hub; `Some(host)` for any other registry.
    pub registry: Option<String>,
    /// Docker Hub repo path, official images normalised to `library/<name>`.
    pub repo: String,
}

/// Splits a `docker run` image argument into registry host (if any) and repo
/// path. A leading path segment counts as a registry host only when it looks
/// like one (has a dot, a port, or is `localhost`), matching the Docker CLI's
/// own disambiguation so `someorg/image` is not mistaken for a host.
pub fn parse_image(image: &str) -> ImageRef {
    let (registry, rest) = match image.split_once('/') {
        Some((host, rest))
            if host.contains('.') || host.contains(':') || host == "localhost" =>
        {
            (Some(host.to_string()), rest.to_string())
        }
        _ => (None, image.to_string()),
    };
    let repo = if registry.is_none() && !rest.contains('/') {
        format!("library/{rest}")
    } else {
        rest
    };
    ImageRef { registry, repo }
}

pub enum DigestCheck {
    /// `sha256:...` of the manifest currently served for the tag.
    Digest(String),
    /// Precisely why this environment cannot resolve it.
    Unsupported(String),
}

fn hub_token(http: &dyn HttpClient, repo: &str) -> Result<String, String> {
    let url =
        format!("{HUB_AUTH}/token?service=registry.docker.io&scope=repository:{repo}:pull");
    let body = http
        .get_text(&url, &[("Accept".into(), "application/json".into())])
        .map_err(|e| format!("Docker Hub auth: {e}"))?;
    let v: Value = serde_json::from_str(&body)
        .map_err(|_| "Docker Hub auth returned invalid JSON".to_string())?;
    v.get("token")
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| "Docker Hub auth response has no token".to_string())
}

/// The current digest for `image_ref:tag` on Docker Hub, or a precise reason
/// it could not be resolved here. Never a hard `Err` for an unsupported
/// registry or a credentials wall: both are expected, ordinary outcomes.
pub fn current_digest(http: &dyn HttpClient, image_ref: &ImageRef, tag: &str) -> DigestCheck {
    if let Some(host) = &image_ref.registry {
        return DigestCheck::Unsupported(format!(
            "registry {host} is not Docker Hub; tag/digest check not supported in this environment"
        ));
    }
    let token = match hub_token(http, &image_ref.repo) {
        Ok(t) => t,
        Err(e) => {
            return DigestCheck::Unsupported(format!(
                "{e}; tag/digest check not supported in this environment"
            ))
        }
    };
    let url = format!("{HUB_REGISTRY}/v2/{}/manifests/{tag}", image_ref.repo);
    let headers = vec![
        ("Accept".to_string(), MANIFEST_ACCEPT.to_string()),
        ("Authorization".to_string(), format!("Bearer {token}")),
    ];
    match http.get_text(&url, &headers) {
        Ok(body) => DigestCheck::Digest(format!("sha256:{}", hex(&Sha256::digest(body.as_bytes())))),
        Err(e) if matches!(e.status, Some(401) | Some(403) | Some(429)) => DigestCheck::Unsupported(
            format!("{e}; tag/digest check not supported in this environment (private image or rate limited)"),
        ),
        Err(e) => DigestCheck::Unsupported(format!(
            "{e}; tag/digest check not supported in this environment"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_host_is_only_recognised_when_it_looks_like_one() {
        assert_eq!(parse_image("nginx").registry, None);
        assert_eq!(parse_image("nginx").repo, "library/nginx");
        assert_eq!(parse_image("someorg/image").registry, None);
        assert_eq!(parse_image("someorg/image").repo, "someorg/image");
        assert_eq!(
            parse_image("ghcr.io/someorg/image").registry,
            Some("ghcr.io".into())
        );
        assert_eq!(parse_image("ghcr.io/someorg/image").repo, "someorg/image");
        assert_eq!(
            parse_image("localhost:5000/image").registry,
            Some("localhost:5000".into())
        );
    }
}
