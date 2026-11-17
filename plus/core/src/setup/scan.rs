//! What looks like a secret: in a block of text (a memory file, a setting) and in a file name
//! (an ignored `.env`). Both lean toward flagging: a flagged file stays out of the readable part
//! of the setup file, and a wrong flag costs one file, a missed one costs a leaked value.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value};

fn token_shapes() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            AKIA[0-9A-Z]{16}
            | gh[pousr]_[A-Za-z0-9]{30,}
            | github_pat_[A-Za-z0-9_]{20,}
            | sk-[A-Za-z0-9_-]{20,}
            | xox[abprs]-[A-Za-z0-9-]{10,}
            | AIza[0-9A-Za-z_-]{35}
            | eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.
            | -----BEGIN\ [A-Z\ ]*PRIVATE\ KEY-----
            | (?i:(?:api[_-]?key|secret|token|passw(?:or)?d|passwd|credential)s?)\s*[:=]\s*['\x22]?[A-Za-z0-9_\-./+=]{12,}",
        )
        .expect("secret shapes")
    })
}

fn sensitive_key() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)token|secret|passw|credential|api[_-]?key|apikey|private[_-]?key|auth(?:orization)?$|bearer")
            .expect("sensitive key")
    })
}

/// True when the text holds something shaped like a token, a key or an assigned secret.
pub fn looks_secret_text(text: &str) -> bool {
    token_shapes().is_match(text)
}

/// True for the name of a setting that is meant to hold a secret.
pub fn secret_key_name(name: &str) -> bool {
    sensitive_key().is_match(name)
}

/// File names that usually hold a secret and that git is told to ignore.
pub fn secret_file_name(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name).to_lowercase();
    base == ".env"
        || base.starts_with(".env.")
            && !matches!(
                base.as_str(),
                ".env.example" | ".env.sample" | ".env.template"
            )
        || base.ends_with(".pem")
        || base.ends_with(".key")
        || base.ends_with(".p12")
        || base.ends_with(".pfx")
        || base.starts_with("id_rsa")
        || base.starts_with("id_ed25519")
        || base == ".netrc"
        || base == ".npmrc"
        || base.starts_with("credentials")
        || base.contains("secret")
}

/// A settings document without the values that look like secrets, and the dotted names of what
/// was taken out. Everything under `env` goes: it is where tokens end up. A string the filter
/// reads as a secret by its key or its shape is removed, not guessed at.
pub fn filter_settings(doc: &Value) -> (Value, Vec<String>) {
    fn walk(path: &str, value: &Value, removed: &mut Vec<String>) -> Option<Value> {
        match value {
            Value::Object(map) => {
                let mut kept = Map::new();
                for (key, inner) in map {
                    let here = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    if path.is_empty() && key == "env" {
                        for name in inner
                            .as_object()
                            .map(|m| m.keys().cloned())
                            .into_iter()
                            .flatten()
                        {
                            removed.push(format!("env.{name}"));
                        }
                        kept.insert(key.clone(), Value::Object(Map::new()));
                        continue;
                    }
                    if inner.is_string() && secret_key_name(key) {
                        removed.push(here);
                        continue;
                    }
                    if let Some(next) = walk(&here, inner, removed) {
                        kept.insert(key.clone(), next);
                    }
                }
                Some(Value::Object(kept))
            }
            Value::Array(items) => Some(Value::Array(
                items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, item)| walk(&format!("{path}[{i}]"), item, removed))
                    .collect(),
            )),
            Value::String(text) if looks_secret_text(text) => {
                removed.push(path.to_string());
                None
            }
            other => Some(other.clone()),
        }
    }
    let mut removed = Vec::new();
    let kept = walk("", doc, &mut removed).unwrap_or(Value::Null);
    (kept, removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn token() -> String {
        ["gh", "p_", &"A1b2C3d4".repeat(5)].concat()
    }

    #[test]
    fn token_shapes_and_assigned_secrets_are_flagged_and_prose_is_not() {
        assert!(looks_secret_text(&format!("use {} for the api", token())));
        assert!(looks_secret_text("password = hunter2hunter2hunter2"));
        assert!(looks_secret_text("-----BEGIN RSA PRIVATE KEY-----\nabc"));
        assert!(!looks_secret_text(
            "remember that the token is rotated monthly"
        ));
        assert!(!looks_secret_text("the user prefers short answers"));
    }

    #[test]
    fn names_that_hold_secrets_are_flagged_and_templates_are_not() {
        for name in [
            ".env",
            "app/.env.local",
            "deploy/key.pem",
            "id_rsa",
            "credentials.json",
        ] {
            assert!(secret_file_name(name), "{name}");
        }
        for name in [".env.example", "README.md", "src/main.rs"] {
            assert!(!secret_file_name(name), "{name}");
        }
    }

    #[test]
    fn a_settings_filter_removes_env_values_secret_keys_and_token_shapes_and_names_them() {
        let doc = json!({
            "model": "opus",
            "env": {"ANTHROPIC_BASE_URL": "http://x", "OTHER": "y"},
            "apiKeyHelper": "echo hi",
            "hooks": {"Stop": [{"command": format!("curl -H 'x: {}'", token())}]},
            "permissions": {"allow": ["Bash(ls)"], "authToken": "abc"},
            "attempts": 3,
        });
        let (kept, removed) = filter_settings(&doc);
        assert_eq!(kept["model"], "opus");
        assert_eq!(kept["env"], json!({}));
        assert_eq!(kept["permissions"]["allow"], json!(["Bash(ls)"]));
        assert_eq!(kept["attempts"], 3);
        assert!(kept.get("apiKeyHelper").is_none());
        assert!(kept["permissions"].get("authToken").is_none());
        assert_eq!(kept["hooks"]["Stop"], json!([{}]));
        for name in [
            "env.ANTHROPIC_BASE_URL",
            "env.OTHER",
            "apiKeyHelper",
            "permissions.authToken",
            "hooks.Stop[0].command",
        ] {
            assert!(removed.contains(&name.to_string()), "{name} in {removed:?}");
        }
        let text = kept.to_string();
        assert!(!text.contains(&token()));
    }
}
