//! Parity harness: compares a produced file tree against an mcpm golden case.
//!
//! Golden layout (written by `gen-golden`): `<root>/<area>/<case>/{tree/,manifest.json}` plus
//! `<root>/PARITY.lock`. Class A files are compared byte-for-byte after normalizers, class B
//! files as canonical JSON (when the extension is .json/.lock), class C files are skipped here
//! because they are contract tests owned by the area's own test.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Normalizers {
    pub root: Option<String>,
    pub home: Option<String>,
    pub ignore_json_keys: Vec<String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// LF line endings, then `<ROOT>` / `<HOME>` placeholders. HOME is replaced first only when it
/// is not a prefix of ROOT, so the longer path always wins.
pub fn normalize_text(bytes: &[u8], n: &Normalizers) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    let mut out = text.replace("\r\n", "\n");
    let mut subs: Vec<(&str, &str)> = Vec::new();
    if let Some(r) = n.root.as_deref().filter(|s| !s.is_empty()) {
        subs.push((r, "<ROOT>"));
    }
    if let Some(h) = n.home.as_deref().filter(|s| !s.is_empty()) {
        subs.push((h, "<HOME>"));
    }
    subs.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
    for (from, to) in subs {
        out = out.replace(from, to);
    }
    out.into_bytes()
}

fn sorted(v: &Value, ignore: &[String]) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().filter(|k| !ignore.contains(k)).collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for k in keys {
                out.insert(k.clone(), sorted(&m[k], ignore));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| sorted(x, ignore)).collect()),
        other => other.clone(),
    }
}

pub fn canonical_json(bytes: &[u8], ignore_keys: &[String]) -> Result<String, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
    serde_json::to_string_pretty(&sorted(&v, ignore_keys)).map_err(|e| e.to_string())
}

/// Minimal LCS line diff in unified style (single hunk, no context trimming).
pub fn unified_diff(name: &str, expected: &str, actual: &str) -> String {
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut out = format!("--- golden/{name}\n+++ actual/{name}\n");
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            out.push_str(&format!(" {}\n", a[i]));
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            out.push_str(&format!("+{}\n", b[j]));
            j += 1;
        } else {
            out.push_str(&format!("-{}\n", a[i]));
            i += 1;
        }
    }
    out
}

fn byte_mismatch(name: &str, expected: &[u8], actual: &[u8]) -> String {
    match (std::str::from_utf8(expected), std::str::from_utf8(actual)) {
        (Ok(e), Ok(a)) => unified_diff(name, e, a),
        _ => {
            let at = expected
                .iter()
                .zip(actual)
                .position(|(x, y)| x != y)
                .unwrap_or_else(|| expected.len().min(actual.len()));
            format!(
                "binary mismatch in {name}: first difference at byte {at} (golden {} bytes, actual {} bytes)\n",
                expected.len(),
                actual.len()
            )
        }
    }
}

pub fn list_files(root: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, PathBuf>) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                walk(base, &path, out)?;
            } else {
                let rel = path
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(rel, path);
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

pub struct GoldenCase {
    pub dir: PathBuf,
    pub comparison: String,
    pub classes: BTreeMap<String, String>,
}

impl GoldenCase {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let raw = fs::read(dir.join("manifest.json"))
            .map_err(|e| format!("{}: manifest.json: {e}", dir.display()))?;
        let m: Value = serde_json::from_slice(&raw).map_err(|e| format!("manifest.json: {e}"))?;
        let comparison = m["comparison"].as_str().unwrap_or("A").to_string();
        let mut classes = BTreeMap::new();
        if let Some(files) = m["files"].as_object() {
            for (k, v) in files {
                classes.insert(k.clone(), v["class"].as_str().unwrap_or("A").to_string());
            }
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            comparison,
            classes,
        })
    }

    pub fn tree(&self) -> PathBuf {
        self.dir.join("tree")
    }

    pub fn is_class_c(&self) -> bool {
        self.comparison == "C"
    }
}

/// Compare `actual` against the golden `tree/`. Returns every problem found, joined, so one
/// failure shows all differing files.
pub fn compare_case(case: &GoldenCase, actual: &Path, n: &Normalizers) -> Result<(), String> {
    let golden = list_files(&case.tree())?;
    let produced = list_files(actual)?;
    let mut problems = String::new();
    for name in golden.keys() {
        if !produced.contains_key(name) {
            problems.push_str(&format!("missing file: {name}\n"));
        }
    }
    for name in produced.keys() {
        if !golden.contains_key(name) {
            problems.push_str(&format!("unexpected file: {name}\n"));
        }
    }
    for (name, gpath) in &golden {
        let Some(apath) = produced.get(name) else {
            continue;
        };
        let class = case.classes.get(name).map(String::as_str).unwrap_or("A");
        let g = fs::read(gpath).map_err(|e| e.to_string())?;
        let a = fs::read(apath).map_err(|e| e.to_string())?;
        let a = normalize_text(&a, n);
        match class {
            "C" => continue,
            "B" if name.ends_with(".json") || name.ends_with(".lock") => {
                let ge = canonical_json(&g, &n.ignore_json_keys);
                let ac = canonical_json(&a, &n.ignore_json_keys);
                match (ge, ac) {
                    (Ok(ge), Ok(ac)) if ge == ac => {}
                    (Ok(ge), Ok(ac)) => problems.push_str(&unified_diff(name, &ge, &ac)),
                    (Err(e), _) | (_, Err(e)) => problems.push_str(&format!("{name}: {e}\n")),
                }
            }
            _ => {
                if g != a {
                    problems.push_str(&byte_mismatch(name, &g, &a));
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Verify every `sha256  path` line of `<root>/PARITY.lock` against the files on disk and
/// reject files under `root` that the lock does not list (run-meta.json is not locked).
pub fn verify_lock(root: &Path) -> Result<usize, String> {
    let lock = fs::read_to_string(root.join("PARITY.lock"))
        .map_err(|e| format!("PARITY.lock: {e}"))?;
    let mut problems = String::new();
    let mut listed = std::collections::BTreeSet::new();
    let mut checked = 0;
    for line in lock.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((sha, rel)) = line.split_once("  ") else {
            problems.push_str(&format!("malformed PARITY.lock line: {line}\n"));
            continue;
        };
        listed.insert(rel.to_string());
        match fs::read(root.join(rel)) {
            Err(_) => problems.push_str(&format!("locked file missing: {rel}\n")),
            Ok(bytes) => {
                let actual = sha256_hex(&bytes);
                if actual != sha {
                    problems.push_str(&format!(
                        "sha256 mismatch: {rel}\n  lock   {sha}\n  actual {actual}\n"
                    ));
                }
                checked += 1;
            }
        }
    }
    for name in list_files(root)?.keys() {
        if name == "PARITY.lock" || name == "README.md" || name == ".gitignore" || name.ends_with("/run-meta.json") {
            continue;
        }
        if !listed.contains(name) {
            problems.push_str(&format!("file not in PARITY.lock: {name}\n"));
        }
    }
    if problems.is_empty() {
        Ok(checked)
    } else {
        Err(problems)
    }
}

pub fn case_dirs(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for area in fs::read_dir(root).map_err(|e| e.to_string())? {
        let area = area.map_err(|e| e.to_string())?.path();
        if !area.is_dir() {
            continue;
        }
        for case in fs::read_dir(&area).map_err(|e| e.to_string())? {
            let case = case.map_err(|e| e.to_string())?.path();
            if case.join("manifest.json").is_file() {
                out.push(case);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Class-2 (sanitized real shapes) fixtures live outside the public repo.
pub fn private_dir() -> Option<PathBuf> {
    std::env::var_os("PARITY_PRIVATE_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub fn skip_class2() -> bool {
    private_dir().is_none()
}

/// `PARITY_BLESS` rewrites goldens, which may only happen through a golden-bump item locally.
pub fn bless_guard(bless: Option<&str>, ci: Option<&str>) -> Result<bool, String> {
    let bless = bless.is_some_and(|v| !v.is_empty() && v != "0");
    let ci = ci.is_some_and(|v| !v.is_empty() && v != "0" && v != "false");
    if bless && ci {
        return Err("PARITY_BLESS is rejected when CI is set".into());
    }
    Ok(bless)
}

pub fn bless_requested() -> Result<bool, String> {
    bless_guard(
        std::env::var("PARITY_BLESS").ok().as_deref(),
        std::env::var("CI").ok().as_deref(),
    )
}

pub fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parity")
}
