use crate::plus::hashing::hex;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

pub const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

pub trait HttpClient {
    fn get_text(&self, url: &str, headers: &[(String, String)]) -> Result<String, HttpError>;
    /// Streams the body into `dest` and returns its lowercase hex sha256.
    fn download(&self, url: &str, dest: &Path) -> Result<String, HttpError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub status: Option<u16>,
    pub message: String,
}

impl HttpError {
    pub fn new(status: Option<u16>, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(s) => write!(f, "HTTP {s}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '\\', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        stripped.split(']').next()?.to_string()
    } else {
        authority.split(':').next()?.to_string()
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

pub fn url_allowed(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return host_of(url)
            .map(|_| ())
            .ok_or_else(|| "URL has no host".to_string());
    }
    if lower.starts_with("http://") {
        return match host_of(url) {
            Some(h) if is_loopback(&h) => Ok(()),
            _ => Err("plain http is only allowed for loopback hosts".to_string()),
        };
    }
    Err("only https URLs are allowed".to_string())
}

pub struct UreqHttp;

impl UreqHttp {
    fn open(&self, url: &str, headers: &[(String, String)]) -> Result<ureq::Response, HttpError> {
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build();
        let origin_host = host_of(url);
        let mut current = url.to_string();
        for _ in 0..=MAX_REDIRECTS {
            url_allowed(&current).map_err(|m| HttpError::new(None, m))?;
            let mut request = agent.get(&current).set("User-Agent", "toolportctl");
            if host_of(&current) == origin_host {
                for (k, v) in headers {
                    request = request.set(k, v);
                }
            }
            let response = match request.call() {
                Ok(r) => r,
                Err(ureq::Error::Status(code, r)) if (300..400).contains(&code) => r,
                Err(ureq::Error::Status(code, _)) => {
                    return Err(HttpError::new(Some(code), "request failed"))
                }
                Err(ureq::Error::Transport(t)) => {
                    return Err(HttpError::new(
                        None,
                        format!("transport error: {}", t.kind()),
                    ))
                }
            };
            if (300..400).contains(&response.status()) {
                let location = response
                    .header("Location")
                    .ok_or_else(|| {
                        HttpError::new(Some(response.status()), "redirect without Location")
                    })?
                    .to_string();
                current = location;
                continue;
            }
            return Ok(response);
        }
        Err(HttpError::new(None, "too many redirects"))
    }
}

impl HttpClient for UreqHttp {
    fn get_text(&self, url: &str, headers: &[(String, String)]) -> Result<String, HttpError> {
        let response = self.open(url, headers)?;
        let mut body = String::new();
        response
            .into_reader()
            .take(MAX_TEXT_BYTES)
            .read_to_string(&mut body)
            .map_err(|e| HttpError::new(None, format!("read failed: {e}")))?;
        Ok(body)
    }

    fn download(&self, url: &str, dest: &Path) -> Result<String, HttpError> {
        let response = self.open(url, &[])?;
        let mut reader = response.into_reader().take(MAX_DOWNLOAD_BYTES + 1);
        let mut file = std::fs::File::create(dest)
            .map_err(|e| HttpError::new(None, format!("cannot create file: {e}")))?;
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| HttpError::new(None, format!("download failed: {e}")))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > MAX_DOWNLOAD_BYTES {
                return Err(HttpError::new(None, "download exceeds the size limit"));
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .map_err(|e| HttpError::new(None, format!("write failed: {e}")))?;
        }
        Ok(hex(&hasher.finalize()))
    }
}
