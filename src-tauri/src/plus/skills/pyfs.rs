//! Python text-I/O behaviours that parity depends on.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// `Path.read_text(encoding="utf-8")`: universal newlines turn `\r\n` and `\r` into `\n`.
pub fn read_text(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(universal_newlines(&text))
}

pub fn universal_newlines(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// `"sha256:" + sha256(read_text()).hexdigest()[:16]`, the hash agents and styles lock.
pub fn text_hash(path: &Path) -> Result<String, String> {
    let text = read_text(path)?;
    let hex: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(format!("sha256:{}", &hex[..16]))
}

/// Python `str.title()` for the lowercase-ASCII names mcpm feeds it: the first letter of every
/// alphabetic run is upper-cased, so a letter after a digit is capitalised too.
pub fn title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_cased = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            if prev_cased {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            prev_cased = true;
        } else {
            out.push(c);
            prev_cased = false;
        }
    }
    out
}

pub fn write_text(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, content).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_matches_python() {
        assert_eq!(title("code reviewer"), "Code Reviewer");
        assert_eq!(title("v2x tool"), "V2X Tool");
        assert_eq!(title("a1b"), "A1B");
        assert_eq!(title(""), "");
    }

    #[test]
    fn universal_newlines_folds_cr_and_crlf() {
        assert_eq!(universal_newlines("a\r\nb\rc\n"), "a\nb\nc\n");
    }
}
