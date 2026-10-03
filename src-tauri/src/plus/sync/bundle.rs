use super::fernet::{self, FernetError, FernetKey};
use super::kdf;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) const MANIFEST_FILE: &str = "sync_manifest.json";
pub(crate) const SALT_FILE: &str = "salt.txt";
pub const SERVER_ORIGINS_KEY: &str = "global/server_origins.json";

#[derive(Debug)]
pub enum SyncError {
    Io(String),
    Format(String),
    Crypto(FernetError),
    UnsafePath(String),
    HashMismatch(String),
    Config(String),
    Git(String),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::Io(m) => write!(f, "io error: {m}"),
            SyncError::Format(m) => write!(f, "bundle format error: {m}"),
            SyncError::Crypto(e) => write!(f, "{e}"),
            SyncError::UnsafePath(p) => write!(f, "refusing unsafe bundle path: {p}"),
            SyncError::HashMismatch(k) => write!(f, "content hash mismatch for {k}"),
            SyncError::Config(m) => write!(f, "{m}"),
            SyncError::Git(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for SyncError {}

impl From<FernetError> for SyncError {
    fn from(e: FernetError) -> Self {
        SyncError::Crypto(e)
    }
}

pub(crate) fn io(path: &Path, e: std::io::Error) -> SyncError {
    SyncError::Io(format!("{}: {e}", path.display()))
}

#[derive(Clone, Copy)]
pub enum Credential<'a> {
    Passphrase(&'a str),
    Key(&'a str),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestEntry {
    pub hash: String,
    pub encrypted_file: String,
    pub category: String,
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default = "default_encoding")]
    pub encoding: String,
}

fn default_encoding() -> String {
    "utf-8".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub machine_id: String,
    #[serde(default)]
    pub pushed_at: String,
    #[serde(default)]
    pub entries: BTreeMap<String, ManifestEntry>,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Clone)]
pub struct PortableRoots {
    pub home: String,
    pub mcpm_home: String,
}

impl PortableRoots {
    pub fn resolve(&self, text: &str) -> String {
        text.replace("${MCPM_HOME}", &self.mcpm_home.replace('\\', "/"))
            .replace("${HOME}", &self.home.replace('\\', "/"))
    }

    pub fn make_portable(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (original, token) in [(&self.mcpm_home, "${MCPM_HOME}"), (&self.home, "${HOME}")] {
            if original.is_empty() {
                continue;
            }
            let forward = original.replace('\\', "/");
            let back = original.replace('/', "\\");
            let json_escaped = back.replace('\\', "\\\\");
            out = out.replace(&json_escaped, token);
            out = out.replace(&forward, token);
            out = out.replace(&back, token);
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct BundleFile {
    pub key: String,
    pub category: String,
    pub project_name: Option<String>,
    pub bytes: Vec<u8>,
}

pub struct SourceFile {
    pub key: String,
    pub category: String,
    pub project_name: Option<String>,
    pub bytes: Vec<u8>,
}

pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{}", &hex[..16])
}

pub(crate) fn safe_relative(raw: &str) -> Result<PathBuf, SyncError> {
    let path = Path::new(raw);
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => return Err(SyncError::UnsafePath(raw.to_string())),
        }
    }
    if out.as_os_str().is_empty() || raw.contains('\\') {
        return Err(SyncError::UnsafePath(raw.to_string()));
    }
    Ok(out)
}

fn reject_secret_keys(key: &str) -> Result<(), SyncError> {
    let mut parts = key.split('/');
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    let leaf = key.rsplit('/').next().unwrap_or("");
    if first == "keys" || (first == "global" && second == "keys") || leaf == "sync_keyfile" {
        return Err(SyncError::UnsafePath(key.to_string()));
    }
    Ok(())
}

pub(crate) fn resolve_key(cred: Credential<'_>, dir: &Path) -> Result<FernetKey, SyncError> {
    match cred {
        Credential::Key(encoded) => Ok(FernetKey::parse(encoded)?),
        Credential::Passphrase(passphrase) => {
            let salt = read_salt(dir)?;
            Ok(FernetKey::parse(&kdf::derive_key(passphrase, &salt))?)
        }
    }
}

pub(crate) fn read_salt(dir: &Path) -> Result<Vec<u8>, SyncError> {
    let path = dir.join(SALT_FILE);
    let text = fs::read_to_string(&path).map_err(|e| io(&path, e))?;
    STANDARD
        .decode(text.trim())
        .map_err(|_| SyncError::Format("salt.txt is not base64".into()))
}

fn is_portable_json(entry: &ManifestEntry, key: &str) -> bool {
    entry.category == "global" && key.ends_with(".json")
}

pub fn read_manifest(dir: &Path) -> Result<Manifest, SyncError> {
    let path = dir.join(MANIFEST_FILE);
    let text = fs::read_to_string(&path).map_err(|e| io(&path, e))?;
    serde_json::from_str(&text).map_err(|e| SyncError::Format(format!("{MANIFEST_FILE}: {e}")))
}

pub fn read_bundle(
    dir: &Path,
    cred: Credential<'_>,
    roots: &PortableRoots,
) -> Result<Vec<BundleFile>, SyncError> {
    let manifest = read_manifest(dir)?;
    let key = resolve_key(cred, dir)?;
    let mut files = Vec::new();
    for (entry_key, entry) in &manifest.entries {
        reject_secret_keys(entry_key)?;
        safe_relative(entry_key)?;
        let blob_path = dir.join(safe_relative(&entry.encrypted_file)?);
        let stored = fs::read(&blob_path).map_err(|e| io(&blob_path, e))?;
        if entry_key == SERVER_ORIGINS_KEY {
            files.push(BundleFile {
                key: entry_key.clone(),
                category: entry.category.clone(),
                project_name: entry.project_name.clone(),
                bytes: stored,
            });
            continue;
        }
        let plaintext = fernet::decrypt(&key, &stored)?;
        if content_hash_for_entry(entry, &plaintext)? != entry.hash {
            return Err(SyncError::HashMismatch(entry_key.clone()));
        }
        let bytes = if entry.encoding == "base64" {
            STANDARD
                .decode(&plaintext)
                .map_err(|_| SyncError::Format(format!("{entry_key}: payload is not base64")))?
        } else if is_portable_json(entry, entry_key) {
            let text = String::from_utf8(plaintext)
                .map_err(|_| SyncError::Format(format!("{entry_key}: payload is not utf-8")))?;
            roots.resolve(&text).into_bytes()
        } else {
            plaintext
        };
        files.push(BundleFile {
            key: entry_key.clone(),
            category: entry.category.clone(),
            project_name: entry.project_name.clone(),
            bytes,
        });
    }
    Ok(files)
}

fn content_hash_for_entry(entry: &ManifestEntry, plaintext: &[u8]) -> Result<String, SyncError> {
    if entry.encoding == "base64" {
        let raw = STANDARD
            .decode(plaintext)
            .map_err(|_| SyncError::Format("base64 payload".into()))?;
        Ok(content_hash(&raw))
    } else {
        Ok(content_hash(plaintext))
    }
}

pub fn write_bundle(
    out_dir: &Path,
    cred: Credential<'_>,
    salt: Option<&[u8]>,
    machine_id: &str,
    now: &str,
    files: &[SourceFile],
    roots: &PortableRoots,
) -> Result<Manifest, SyncError> {
    write_bundle_with_origins(out_dir, cred, salt, machine_id, now, files, roots, None)
}

#[allow(clippy::too_many_arguments)]
pub fn write_bundle_with_origins(
    out_dir: &Path,
    cred: Credential<'_>,
    salt: Option<&[u8]>,
    machine_id: &str,
    now: &str,
    files: &[SourceFile],
    roots: &PortableRoots,
    origins_json: Option<&str>,
) -> Result<Manifest, SyncError> {
    let key = match (cred, salt) {
        (Credential::Key(encoded), _) => FernetKey::parse(encoded)?,
        (Credential::Passphrase(p), Some(s)) => FernetKey::parse(&kdf::derive_key(p, s))?,
        (Credential::Passphrase(_), None) => {
            return Err(SyncError::Format("passphrase needs a salt".into()))
        }
    };
    let blobs = out_dir.join("blobs");
    fs::create_dir_all(&blobs).map_err(|e| io(&blobs, e))?;
    if let Some(salt) = salt {
        let path = out_dir.join(SALT_FILE);
        fs::write(&path, STANDARD.encode(salt)).map_err(|e| io(&path, e))?;
    }
    let mut manifest = Manifest {
        version: 1,
        machine_id: machine_id.to_string(),
        pushed_at: now.to_string(),
        entries: BTreeMap::new(),
    };
    for file in files {
        reject_secret_keys(&file.key)?;
        safe_relative(&file.key)?;
        let probe = ManifestEntry {
            hash: String::new(),
            encrypted_file: String::new(),
            category: file.category.clone(),
            project_name: file.project_name.clone(),
            updated_at: now.to_string(),
            encoding: default_encoding(),
        };
        let (plaintext, hash, encoding) = match String::from_utf8(file.bytes.clone()) {
            Ok(text) => {
                let text = if is_portable_json(&probe, &file.key) {
                    roots.make_portable(&text)
                } else {
                    text
                };
                let bytes = text.into_bytes();
                let hash = content_hash(&bytes);
                (bytes, hash, "utf-8")
            }
            Err(_) => (
                STANDARD.encode(&file.bytes).into_bytes(),
                content_hash(&file.bytes),
                "base64",
            ),
        };
        let blob_name = format!("{}.enc", file.key.replace(['/', '\\'], "__"));
        let token = fernet::encrypt(&key, &plaintext)?;
        let blob_path = blobs.join(&blob_name);
        fs::write(&blob_path, token).map_err(|e| io(&blob_path, e))?;
        manifest.entries.insert(
            file.key.clone(),
            ManifestEntry {
                hash,
                encrypted_file: format!("blobs/{blob_name}"),
                encoding: encoding.to_string(),
                ..probe
            },
        );
    }
    if let Some(origins) = origins_json {
        let blob_name = "global__server_origins.json";
        let blob_path = blobs.join(blob_name);
        fs::write(&blob_path, origins).map_err(|e| io(&blob_path, e))?;
        manifest.entries.insert(
            SERVER_ORIGINS_KEY.to_string(),
            ManifestEntry {
                hash: content_hash(origins.as_bytes()),
                encrypted_file: format!("blobs/{blob_name}"),
                category: "global".into(),
                project_name: None,
                updated_at: now.to_string(),
                encoding: default_encoding(),
            },
        );
    }
    let path = out_dir.join(MANIFEST_FILE);
    let json =
        serde_json::to_string_pretty(&manifest).map_err(|e| SyncError::Format(e.to_string()))?;
    fs::write(&path, json).map_err(|e| io(&path, e))?;
    Ok(manifest)
}

pub(crate) fn write_synced_file(path: &Path, key: &str, bytes: &[u8]) -> Result<(), SyncError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    }
    fs::write(path, bytes).map_err(|e| io(path, e))?;
    #[cfg(unix)]
    if key.starts_with("bin/") {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| io(path, e))?;
    }
    #[cfg(not(unix))]
    let _ = key;
    Ok(())
}

pub struct ImportTargets {
    pub config_dir: PathBuf,
    pub skills_repo_dir: PathBuf,
    pub projects: BTreeMap<String, PathBuf>,
}

#[derive(Debug, Default)]
pub struct ImportReport {
    pub written: Vec<String>,
    pub skipped: Vec<String>,
    pub server_origins: Option<serde_json::Value>,
}

fn destination(file: &BundleFile, targets: &ImportTargets) -> Result<Option<PathBuf>, SyncError> {
    destination_for(&file.key, &file.category, targets)
}

pub(crate) fn destination_for(
    key: &str,
    category: &str,
    targets: &ImportTargets,
) -> Result<Option<PathBuf>, SyncError> {
    let rest_after_first = |k: &str| k.split_once('/').map(|(_, rest)| rest.to_string());
    let dest = if let Some(rest) = key.strip_prefix("skills_repo/") {
        targets.skills_repo_dir.join(safe_relative(rest)?)
    } else if let Some(rest) = key.strip_prefix("bin/") {
        targets.config_dir.join("bin").join(safe_relative(rest)?)
    } else if category == "global" {
        match rest_after_first(key) {
            Some(rest) => targets.config_dir.join(safe_relative(&rest)?),
            None => return Ok(None),
        }
    } else if category == "project" {
        let mut parts = key.splitn(3, '/');
        let (_, name, rest) = (parts.next(), parts.next(), parts.next());
        match (name, rest) {
            (Some(name), Some(rest)) => match targets.projects.get(name) {
                Some(root) => root.join(safe_relative(rest)?),
                None => return Ok(None),
            },
            _ => return Ok(None),
        }
    } else {
        return Ok(None);
    };
    Ok(Some(dest))
}

pub fn import_bundle(
    dir: &Path,
    cred: Credential<'_>,
    roots: &PortableRoots,
    targets: &ImportTargets,
    include_projects: bool,
) -> Result<ImportReport, SyncError> {
    let files = read_bundle(dir, cred, roots)?;
    let mut planned = Vec::new();
    let mut report = ImportReport::default();
    for file in &files {
        if file.key == SERVER_ORIGINS_KEY {
            report.server_origins = serde_json::from_slice(&file.bytes).ok();
            continue;
        }
        if file.category == "project" && !include_projects {
            report.skipped.push(file.key.clone());
            continue;
        }
        match destination(file, targets)? {
            Some(path) => planned.push((file, path)),
            None => report.skipped.push(file.key.clone()),
        }
    }
    for (file, path) in planned {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        }
        write_synced_file(&path, &file.key, &file.bytes)?;
        report.written.push(file.key.clone());
    }
    Ok(report)
}
