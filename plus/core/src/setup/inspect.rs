//! `setup inspect <file>`: reads the manifest and checks every part against its hash, one part at
//! a time, so one damaged part does not lose the others. Nothing is written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::zipio::{safe_name, ZipFile};
use super::{crypto, hex, PartId, FORMAT, MANIFEST, VERSION};
use crate::op::OpError;

/// How one part stands against the manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    Ok,
    Damaged,
    Missing,
    /// A part id this build does not know; it is skipped with a note.
    Unknown,
}

impl Standing {
    pub const fn as_str(self) -> &'static str {
        match self {
            Standing::Ok => "ok",
            Standing::Damaged => "damaged",
            Standing::Missing => "missing",
            Standing::Unknown => "unknown",
        }
    }
}

pub struct Part {
    pub id: String,
    pub known: Option<PartId>,
    pub row: Value,
    pub standing: Standing,
}

pub struct Opened {
    pub path: PathBuf,
    pub zip: ZipFile,
    pub manifest: Value,
    pub parts: Vec<Part>,
}

struct HashingSink(Sha256);

impl std::io::Write for HashingSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn unreadable(why: impl Into<String>) -> OpError {
    OpError::failed("unreadable", why)
}

/// Hash of the entries that belong to a manifest row, as the export computed it.
fn standing_of(zip: &mut ZipFile, row: &Value) -> Result<Standing, String> {
    let names: Vec<String> = match (row["file"].as_str(), row["dir"].as_str()) {
        (Some(file), _) => vec![file.to_string()],
        (_, Some(dir)) => zip
            .names()
            .filter(|n| n.starts_with(dir))
            .map(String::from)
            .collect(),
        _ => return Ok(Standing::Missing),
    };
    if row["file"].is_string() && zip.entry(&names[0]).is_none() {
        return Ok(Standing::Missing);
    }
    let mut lines: BTreeMap<String, String> = BTreeMap::new();
    for name in &names {
        if !safe_name(name) {
            return Ok(Standing::Damaged);
        }
        let mut sink = HashingSink(Sha256::new());
        if zip.copy(name, &mut sink).is_err() {
            return Ok(Standing::Damaged);
        }
        lines.insert(name.clone(), hex(&sink.0.finalize()));
    }
    let hash = if lines.len() == 1 {
        lines.values().next().cloned().unwrap_or_default()
    } else {
        let mut hasher = Sha256::new();
        for (name, hash) in &lines {
            hasher.update(name.as_bytes());
            hasher.update([0]);
            hasher.update(hash.as_bytes());
            hasher.update(b"\n");
        }
        hex(&hasher.finalize())
    };
    Ok(if Some(hash.as_str()) == row["sha256"].as_str() {
        Standing::Ok
    } else {
        Standing::Damaged
    })
}

pub fn open(path: &Path) -> Result<Opened, OpError> {
    if !path.is_file() {
        return Err(OpError::not_found(format!(
            "{} is not a file",
            path.display()
        )));
    }
    if crypto::is_sealed_file(path) {
        return Err(unreadable(
            "that file is sealed as a whole, as a sync file is: Toolport opens it with `sync pull`",
        ));
    }
    let mut zip =
        ZipFile::open(path).map_err(|_| unreadable("that is not a Toolport setup file"))?;
    let manifest: Value = serde_json::from_slice(
        &zip.read(MANIFEST)
            .map_err(|_| unreadable("that is not a Toolport setup file: it has no manifest"))?,
    )
    .map_err(|e| unreadable(format!("the manifest is not readable: {e}")))?;
    if manifest["format"] != FORMAT {
        return Err(unreadable("that is not a Toolport setup file"));
    }
    let version = manifest["version"].as_u64().unwrap_or(0);
    if version > u64::from(VERSION) {
        return Err(unreadable(format!(
            "this file has format version {version}, newer than this Toolport reads ({VERSION}): update Toolport"
        )));
    }
    let mut parts = Vec::new();
    for row in manifest["parts"].as_array().into_iter().flatten() {
        let id = row["id"].as_str().unwrap_or_default().to_string();
        let known = PartId::parse(&id);
        let standing = if known.is_none() {
            Standing::Unknown
        } else {
            standing_of(&mut zip, row).map_err(unreadable)?
        };
        parts.push(Part {
            id,
            known,
            row: row.clone(),
            standing,
        });
    }
    Ok(Opened {
        path: path.to_path_buf(),
        zip,
        manifest,
        parts,
    })
}

impl Opened {
    pub fn part(&self, id: PartId) -> Option<&Part> {
        self.parts.iter().find(|p| p.known == Some(id))
    }

    /// The part is in the file and matches its hash.
    pub fn usable(&self, id: PartId) -> bool {
        self.part(id).is_some_and(|p| p.standing == Standing::Ok)
    }

    pub fn read_json(&mut self, name: &str) -> Result<Value, String> {
        let bytes = self.zip.read(name)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{name}: {e}"))
    }

    pub fn names_under(&self, dir: &str) -> Vec<String> {
        self.zip
            .names()
            .filter(|n| n.starts_with(dir))
            .map(String::from)
            .collect()
    }

    /// The decrypted JSON of a small sealed part.
    pub fn unseal_json(
        &mut self,
        name: &str,
        passphrase: &str,
    ) -> Result<Value, crypto::OpenError> {
        let sealed = self.zip.read(name).map_err(crypto::OpenError::Unreadable)?;
        let plain = crypto::open_bytes(passphrase, &sealed)?;
        serde_json::from_slice(&plain)
            .map_err(|e| crypto::OpenError::Unreadable(format!("{name}: {e}")))
    }
}

pub fn summary(opened: &Opened) -> Value {
    let parts: Vec<Value> = opened
        .parts
        .iter()
        .map(|part| {
            json!({
                "id": part.id,
                "label": part.known.map_or(part.id.as_str(), |k| k.label()),
                "count": part.row["count"],
                "fileSize": part.row["bytes"],
                "sealed": part.known.is_some_and(PartId::sealed),
                "standing": part.standing.as_str(),
            })
        })
        .collect();
    let problems: Vec<String> = opened
        .parts
        .iter()
        .filter_map(|p| match p.standing {
            Standing::Damaged => Some(format!("{} does not match its hash", p.id)),
            Standing::Missing => Some(format!("{} is listed but not in the file", p.id)),
            Standing::Unknown => Some(format!("{} is from a newer Toolport and is skipped", p.id)),
            Standing::Ok => None,
        })
        .collect();
    json!({
        "file": opened.path.to_string_lossy(),
        "format": opened.manifest["format"],
        "version": opened.manifest["version"],
        "madeAt": opened.manifest["madeAt"],
        "machine": opened.manifest["machine"],
        "toolport": opened.manifest["toolport"],
        "parts": parts,
        "problems": problems,
        "intact": opened.parts.iter().all(|p| p.standing == Standing::Ok),
    })
}

pub fn run(path: &Path) -> Result<Value, OpError> {
    Ok(summary(&open(path)?))
}
