//! What a server's build command and its output may carry. The command is remembered in clear
//! in `server-repos.json` and shown in plans, results and task output, so a secret never goes in
//! it: a command that carries one is refused, and any text that is printed passes through
//! [`shown`] first.

use std::sync::OnceLock;

use regex::{Captures, Regex};
use toolport_plus_api::Host;

use crate::redact::scrub_text;
use crate::servers::env::{credential_reason, entropy, is_opaque_blob};
use crate::skills::taps::redact;

const MASK: &str = "[redacted]";
const BLOB_MIN_LEN: usize = 20;
const BLOB_MIN_ENTROPY: f64 = 4.0;
const AUTH_SCHEMES: [&str; 4] = ["bearer", "basic", "token", "digest"];

const FLAG: &str = r#"(?i)(?:^|[\s"'])-{1,2}[a-z0-9_-]*(?:passw|secret|token|api-?key|auth|credential|private-?key|access-?key|bearer)[a-z0-9_-]*(?:=|\s+)("[^"]*"|'[^']*'|[^\s;&|]+)"#;
const ASSIGN: &str = r#"(?i)(?:^|[\s;&|"'])[a-z0-9_]*(?:passw|secret|token|credential|auth|api-?key|key)[a-z0-9_]*=("[^"]*"|'[^']*'|[^\s;&|]+)"#;
const HEADER: &str = r#"(?i)(?:(?:proxy-)?authorization|x-api-key|x-auth-token|api-key|set-cookie|cookie)["']?\s*[:=]\s*["']?(?:(?:bearer|basic|token|digest)\s+)?([^\s"';&|]+)"#;
const WORD: &str = r"[A-Za-z0-9+_=-]{20,}";

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("screen pattern"))
}

fn flag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, FLAG)
}

fn assign() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, ASSIGN)
}

fn header() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, HEADER)
}

fn word() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    regex(&RE, WORD)
}

/// A value that names a variable instead of carrying one.
fn is_reference(value: &str) -> bool {
    let bare = value.trim_matches(['"', '\'']);
    bare.is_empty()
        || bare.starts_with('$')
        || bare.starts_with('%')
        || bare == MASK
        || AUTH_SCHEMES.contains(&bare.to_ascii_lowercase().as_str())
}

fn carries(re: &Regex, text: &str, skip_options: bool) -> bool {
    re.captures_iter(text).any(|caps| {
        caps.get(1).is_some_and(|value| {
            let value = value.as_str();
            !is_reference(value) && !(skip_options && value.starts_with('-'))
        })
    })
}

fn looks_secret_word(word: &str, hex_too: bool) -> bool {
    let word = word.trim_matches('=');
    let hex = word.chars().all(|c| c.is_ascii_hexdigit());
    if hex && !hex_too {
        return false;
    }
    let has = |test: fn(&char) -> bool| word.chars().any(|c| test(&c));
    is_opaque_blob(word)
        || (word.len() >= BLOB_MIN_LEN
            && has(char::is_ascii_digit)
            && has(char::is_ascii_uppercase)
            && has(char::is_ascii_lowercase)
            && entropy(word) >= BLOB_MIN_ENTROPY)
}

pub(super) fn pattern_reason(text: &str) -> Option<&'static str> {
    if carries(flag(), text, true) {
        return Some("it passes a secret in an option such as --token or --password");
    }
    if carries(assign(), text, false) {
        return Some("it sets a variable whose name says it holds a secret");
    }
    if carries(header(), text, false) {
        return Some("it carries an Authorization, cookie or API-key header");
    }
    if word()
        .find_iter(text)
        .any(|found| looks_secret_word(found.as_str(), true))
    {
        return Some("it contains a long high-entropy string");
    }
    None
}

/// Why `command` may not be remembered or run, without repeating any of it.
pub fn refusal(host: &dyn Host, command: &str) -> Option<String> {
    let why = if host.redact_text(command) != command {
        Some("it contains a value stored in the vault")
    } else {
        credential_reason(host, command).or_else(|| pattern_reason(command))
    }?;
    Some(format!(
        "the build command carries a secret ({why}); put the secret in the server's env (vault) \
         instead, because the command is stored in clear and shown in plans and results"
    ))
}

fn mask_group(re: &Regex, text: &str, skip_options: bool) -> String {
    re.replace_all(text, |caps: &Captures| {
        let whole = caps.get(0).expect("whole match");
        let value = caps.get(1).expect("value group");
        let text = whole.as_str();
        if is_reference(value.as_str()) || (skip_options && value.as_str().starts_with('-')) {
            return text.to_string();
        }
        let (from, to) = (value.start() - whole.start(), value.end() - whole.start());
        format!("{}{MASK}{}", &text[..from], &text[to..])
    })
    .into_owned()
}

/// `text` as it may be printed or stored: what the host knows as a secret, inline secrets, URL
/// credentials and this process's own keys masked.
pub fn shown(host: &dyn Host, text: &str) -> String {
    let text = host.redact_text(text);
    let text = mask_group(flag(), &text, true);
    let text = mask_group(assign(), &text, false);
    let text = mask_group(header(), &text, false);
    let text = word()
        .replace_all(&text, |caps: &Captures| {
            let found = &caps[0];
            if looks_secret_word(found, false) {
                MASK.to_string()
            } else {
                found.to_string()
            }
        })
        .into_owned();
    scrub_text(redact(&text))
}
