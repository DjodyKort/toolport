use serde_json::Value;

const SENSITIVE_KEYS: &[&str] = &[
    "secret",
    "token",
    "password",
    "passwd",
    "apikey",
    "api_key",
    "authorization",
    "credential",
    "bearer",
    "private_key",
    "passphrase",
];
const EXEMPT_KEYS: &[&str] = &["secretsbackend", "tokens", "writessecrets"];
const MASK: &str = "[redacted]";

pub(crate) fn sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    !EXEMPT_KEYS.contains(&lower.as_str()) && SENSITIVE_KEYS.iter().any(|s| lower.contains(s))
}

const LIVE_SECRET_ENV: [&str; 3] = [
    "TOOLPORT_SECRET_KEY",
    "CONDUIT_SECRET_KEY",
    "TOOLPORT_HTTP_TOKEN",
];

fn live_secrets() -> Vec<String> {
    LIVE_SECRET_ENV
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .filter(|v| v.len() >= 8)
        .collect()
}

pub fn scrub_text(text: String) -> String {
    live_secrets()
        .iter()
        .fold(text, |acc, secret| acc.replace(secret.as_str(), MASK))
}

pub fn scrub(value: Value) -> Value {
    scrub_with(value, &live_secrets())
}

const MIN_SUPPLIED_LEN: usize = 4;

/// Masks values the caller handed in as well as the live ones, so a tool that took a secret as an
/// argument cannot hand it back in a result or an error by mistake.
pub fn supplied(values: &[&str]) -> Vec<String> {
    let mut all = live_secrets();
    all.extend(
        values
            .iter()
            .filter(|v| v.len() >= MIN_SUPPLIED_LEN)
            .map(|v| v.to_string()),
    );
    all
}

pub fn scrub_supplied(value: Value, secrets: &[String]) -> Value {
    scrub_with(value, secrets)
}

pub fn scrub_text_supplied(text: String, secrets: &[String]) -> String {
    secrets
        .iter()
        .fold(text, |acc, secret| acc.replace(secret.as_str(), MASK))
}

fn scrub_with(value: Value, secrets: &[String]) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let masked = sensitive_key(&k)
                        && matches!(v, Value::String(_) | Value::Array(_) | Value::Object(_));
                    let v = if masked {
                        Value::String(MASK.into())
                    } else {
                        scrub_with(v, secrets)
                    };
                    (k, v)
                })
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|v| scrub_with(v, secrets)).collect())
        }
        Value::String(s) => Value::String(
            secrets
                .iter()
                .fold(s, |acc, secret| acc.replace(secret.as_str(), MASK)),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_count_object_is_not_a_secret_but_a_token_inside_it_still_is() {
        let scrubbed = scrub(serde_json::json!({
            "tokens": {"value": 12, "basis": "estimate", "accessToken": "abc"},
            "token": "abc",
        }));
        assert_eq!(scrubbed["tokens"]["value"], 12);
        assert_eq!(scrubbed["tokens"]["accessToken"], MASK);
        assert_eq!(scrubbed["token"], MASK);
    }

    #[test]
    fn the_secrets_backend_name_is_not_a_secret() {
        let scrubbed =
            scrub(serde_json::json!({"secretsBackend": "encrypted-file", "secrets": [1]}));
        assert_eq!(scrubbed["secretsBackend"], "encrypted-file");
        assert_eq!(scrubbed["secrets"], MASK);
    }
}
